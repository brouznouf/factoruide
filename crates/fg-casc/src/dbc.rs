//! The client's databases, `DBFilesClient`: the modern `WDC3`/`WDC4`/`WDC5`
//! (.db2, bitpacked fields, pallet and common data, sections; wowdev.wiki/
//! DB2). What is read of them: the rows' IDs and their 32-bit integers, by
//! column.

use anyhow::{Context, Result, bail};
use std::collections::HashMap;

/// One database: its records, and how to read the columns of each.
#[derive(Debug)]
pub struct Table<'a> {
    data: &'a [u8],
    /// Every record, as (offset, size) into `data` (of its own length with the
    /// offset map, else the header's).
    rows: Vec<(usize, usize)>,
    /// The ID of every record (empty when it is a column of the records:
    /// `id` reads it at `id_index`).
    ids: Vec<u32>,
    /// The column the IDs sit in, when they are a column of the records.
    id_index: Option<usize>,
    /// The fields of the records.
    fields: Vec<Field>,
}

/// One field of a WDC record: where it starts, how long it is, and where its
/// data is when not in the record itself.
#[derive(Debug, Clone, Copy)]
struct Field {
    offset_bits: u32,
    size_bits: u32,
    storage: FieldStorage,
}

/// A field's data outside the record (WDC): a pallet (its entries, indexed by
/// what the record holds) or common data (its default, and its exceptions by
/// record ID).
#[derive(Debug, Clone, Copy)]
enum FieldStorage {
    InRecord,
    Pallet {
        /// Where the entries sit, into `data`.
        at: usize,
        /// How many 32-bit values each entry holds (an indexed array field:
        /// one column of it, the whole array).
        stride: usize,
    },
    Common {
        /// The value every record without an exception has.
        default: u32,
        /// Where the exceptions sit, into `data`: pairs of (record ID, value).
        at: usize,
        /// How many pairs there are.
        count: usize,
    },
}

/// The WDC header's own size, without the version and the schema string
/// (`WDC5`): the counts and the flags, then the section headers.
const WDC_HEADER: usize = 0x48;
/// One WDC section header: its encryption, where it sits, its counts.
const WDC_SECTION: usize = 40;
/// One WDC field's storage info: its place and length, its type, and the
/// type's own data.
const WDC_STORAGE: usize = 24;

impl<'a> Table<'a> {
    /// Read a database out of its whole bytes.
    pub fn read(data: &'a [u8]) -> Result<Table<'a>> {
        let magic: [u8; 4] = data
            .get(..4)
            .and_then(|b| b.try_into().ok())
            .context("a database cut short")?;
        match &magic {
            // `WDC5` adds a version and a schema string to the header, shifting
            // the rest of it; `WDC4` changed nothing of what is read here.
            b"WDC3" | b"WDC4" => Self::read_wdc(data, 0),
            b"WDC5" => Self::read_wdc(data, 4 + 128),
            other => bail!("unknown database magic {other:?}"),
        }
    }

    /// How many records the database holds.
    pub fn len(&self) -> usize {
        self.rows.len()
    }

    /// Whether the database holds no records.
    pub fn is_empty(&self) -> bool {
        self.rows.is_empty()
    }

    /// The raw bytes of record `row`, as they sit in the file.
    pub fn record(&self, row: usize) -> Result<&'a [u8]> {
        let (at, size) = self.rows[row];
        self.data.get(at..at + size).context("a WDC record out of bounds")
    }

    /// The ID of record `row` (from the ID list, or the column the layout
    /// names when there is none).
    pub fn id(&self, row: usize) -> Result<u32> {
        if let Some(id) = self.ids.get(row) {
            return Ok(*id);
        }
        let col = self.id_index.context("the database has no ID list")?;
        self.u32(row, col)
    }

    /// The record `row`'s column `col` as a 32-bit integer (an array column:
    /// its first value, `u32s` the whole of it).
    pub fn u32(&self, row: usize, col: usize) -> Result<u32> {
        Ok(self.u32s(row, col)?.first().map_or(0, |value| *value))
    }

    /// The record `row`'s column `col` as 32-bit integers: every value of an
    /// array column, one value of every other kind.
    pub fn u32s(&self, row: usize, col: usize) -> Result<Vec<u32>> {
        let field = self.fields.get(col).context("a column out of bounds")?;
        let raw = self.raw(row, field)?;
        Ok(match field.storage {
            FieldStorage::InRecord => vec![raw],
            FieldStorage::Pallet { at, stride } => {
                let from = at + raw as usize * stride * 4;
                let bytes = self
                    .data
                    .get(from..from + stride * 4)
                    .context("a pallet entry out of bounds")?;
                (0..stride)
                    .map(|n| u32::from_le_bytes(bytes[n * 4..n * 4 + 4].try_into().expect("four bytes")))
                    .collect()
            }
            // Common data: the default, unless this record has an exception.
            FieldStorage::Common { default, at, count } => {
                let id = self.id(row)?;
                vec![
                    (0..count)
                        .find(|k| self.u32_at(at + k * 8) == id)
                        .map_or(default, |k| self.u32_at(at + k * 8 + 4)),
                ]
            }
        })
    }

    /// What the record itself holds for `field` (its value, or its index into
    /// the field's pallet), read out of its bits.
    fn raw(&self, row: usize, field: &Field) -> Result<u32> {
        let (at, size) = self.rows[row];
        let from = at + field.offset_bits as usize / 8;
        let span = (field.size_bits as usize + field.offset_bits as usize % 8).div_ceil(8);
        let bytes = self
            .data
            .get(from..from + span)
            .filter(|_| from + span <= at + size)
            .context("a WDC field out of its record")?;
        let raw: u64 = bytes.iter().rev().fold(0u64, |v, b| v << 8 | u64::from(*b));
        let bits = field.offset_bits % 8;
        Ok((raw >> bits) as u32 & ((1u64 << field.size_bits.min(32)) - 1) as u32)
    }

    /// A 32-bit little-endian integer at `at`, into `data`.
    fn u32_at(&self, at: usize) -> u32 {
        self.data
            .get(at..at + 4)
            .map_or(0, |b| u32::from_le_bytes(b.try_into().unwrap_or([0; 4])))
    }

    /// The modern database. What is read is its first section: the later ones
    /// hold the client's own newer rows, encrypted one by one (their keys are
    /// not public).
    fn read_wdc(data: &'a [u8], shift: usize) -> Result<Table<'a>> {
        let u32le = |at: usize| -> Result<u32> {
            Ok(u32::from_le_bytes(
                data.get(at..at + 4).context("a WDC header cut short")?.try_into()?,
            ))
        };
        let u16le = |at: usize| -> Result<u16> {
            Ok(u16::from_le_bytes(
                data.get(at..at + 2).context("a WDC header cut short")?.try_into()?,
            ))
        };
        let at = shift;
        let field_count = u32le(at + 0x08)? as usize;
        let record_size = u32le(at + 0x0C)? as usize;
        let flags = u16le(at + 0x28)?;
        let id_index = u16le(at + 0x2A)? as usize;
        let storage_info_size = u32le(at + 0x38)? as usize;
        let pallet_size = u32le(at + 0x40)? as usize;
        let section_count = u32le(at + 0x44)? as usize;
        // The first section: whether it is encrypted, where its records sit,
        // and its own counts. The field info follows every section's header.
        let section = at + WDC_HEADER;
        let fields_at = section + section_count * WDC_SECTION;
        let encrypted = u32le(section)? != 0 || u32le(section + 4)? != 0;
        if encrypted {
            bail!("an encrypted WDC section, its key is not known");
        }
        let records_at = u32le(section + 0x08)? as usize;
        let section_records = u32le(section + 0x0C)? as usize;
        let strings_len = u32le(section + 0x10)? as usize;
        let records_end = u32le(section + 0x14)? as usize;
        let id_list_size = u32le(section + 0x18)? as usize;
        let copy_count = u32le(section + 0x24)? as usize;
        let map_count = u32le(section + 0x20)? as usize;
        // The fields: where each starts, how long it is, and where its data is
        // when not in the record — the pallet and common data blocks are
        // shared by the fields, each its own run of them, in their order.
        let storage_at = fields_at + field_count * 4;
        let pallet_at = storage_at + storage_info_size;
        let common_at = pallet_at + pallet_size;
        let mut fields = Vec::with_capacity(field_count);
        let mut pallet_run = pallet_at;
        let mut common_run = common_at;
        for k in 0..field_count {
            let info = storage_at + k * WDC_STORAGE;
            let offset_bits = u32::from(u16le(info)?);
            let size_bits = u32::from(u16le(info + 2)?);
            let data_size = u32le(info + 4)? as usize;
            let storage = match u32le(info + 8)? {
                // In the record, plainly or bitpacked (the signed kind too).
                0 | 1 | 5 => FieldStorage::InRecord,
                // Common data: the default in the entry, the exceptions in the
                // block, as pairs of (record ID, value).
                2 => {
                    let storage = FieldStorage::Common {
                        default: u32le(info + 12)?,
                        at: common_run,
                        count: data_size / 8,
                    };
                    common_run += data_size;
                    storage
                }
                // A pallet: its entries in the block, indexed by the record —
                // one value each, or an array of them (an indexed array
                // field's count is in its entry).
                3 | 4 => {
                    let stride = if u32le(info + 8)? == 4 {
                        u32le(info + 20)? as usize
                    } else {
                        1
                    };
                    if stride == 0 {
                        bail!("a WDC pallet field of no values");
                    }
                    let storage = FieldStorage::Pallet { at: pallet_run, stride };
                    pallet_run += data_size;
                    storage
                }
                other => bail!("a WDC field of unknown storage {other}"),
            };
            fields.push(Field {
                offset_bits,
                size_bits,
                storage,
            });
        }
        // The records: of the header's size, or (with the offset map) pointed
        // at by 6-byte entries after the ID list and the copy table — each its
        // own length.
        let offset_map = flags & 0x01 != 0;
        let mut rows = if offset_map {
            let map_at = records_end + id_list_size + copy_count * 8;
            let mut rows = Vec::with_capacity(map_count);
            for k in 0..map_count {
                let entry = map_at + k * 6;
                let offset = u32le(entry)? as usize;
                let size = u16le(entry + 4)? as usize;
                if offset + size > data.len() {
                    bail!("a WDC record out of bounds");
                }
                rows.push((offset, size));
            }
            rows
        } else {
            let strings_at = records_at + section_records * record_size;
            if strings_at + strings_len > data.len() {
                bail!("WDC records out of bounds");
            }
            (0..section_records)
                .map(|k| (records_at + k * record_size, record_size))
                .collect()
        };
        // The IDs: listed apart (after the records' strings, or the offset
        // map's own list after its entries), or a column of the records.
        let mut ids = if flags & 0x04 != 0 {
            let list_at = if offset_map {
                records_end + id_list_size + copy_count * 8 + map_count * 6
            } else {
                records_at + section_records * record_size + strings_len
            };
            if list_at + id_list_size > data.len() {
                bail!("a WDC ID list out of bounds");
            }
            (0..id_list_size / 4)
                .map(|k| {
                    Ok(u32::from_le_bytes(
                        data.get(list_at + k * 4..list_at + k * 4 + 4)
                            .context("a WDC ID cut short")?
                            .try_into()?,
                    ))
                })
                .collect::<Result<_>>()?
        } else {
            Vec::new()
        };
        // The copy table: rows that repeat another's data, as pairs of (the
        // new row's ID, the copied row's ID) after the ID list. (A section's
        // records are read apart from its copies; the copies follow them.)
        let mut by_id: HashMap<u32, usize> = ids.iter().enumerate().map(|(row, &id)| (id, row)).collect();
        let copies_at = if offset_map {
            records_end + id_list_size
        } else {
            records_at + section_records * record_size + strings_len + id_list_size
        };
        for k in 0..copy_count {
            let entry = copies_at + k * 8;
            let (new, source) = (
                u32::from_le_bytes(data.get(entry..entry + 4).context("a WDC copy cut short")?.try_into()?),
                u32::from_le_bytes(
                    data.get(entry + 4..entry + 8)
                        .context("a WDC copy cut short")?
                        .try_into()?,
                ),
            );
            let row = *by_id
                .get(&source)
                .context("a WDC copy of a row the section does not hold")?;
            let record = rows[row];
            by_id.insert(new, rows.len());
            rows.push(record);
            ids.push(new);
        }
        Ok(Table {
            data,
            rows,
            ids,
            id_index: (flags & 0x04 == 0).then_some(id_index),
            fields,
        })
    }
}
