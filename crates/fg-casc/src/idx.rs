//! The local indices (`Data/data/*.idx`): where every file the game installed
//! sits in the archives (`data.000`, `data.001`...), by the first 9 bytes of its
//! encoding key. One index per bucket (the low nibble of the key's own hash),
//! all of them loaded at once.

use anyhow::{Context, Result, bail};
use std::collections::HashMap;
use std::fs;
use std::path::Path;

/// Where a file sits: which archive, at which offset, and how long.
#[derive(Debug, Clone, Copy)]
pub struct Location {
    pub archive: u32,
    pub offset: u32,
    pub size: u32,
}

/// Every file the game installed, by the first 9 bytes of its encoding key.
#[derive(Debug, Default)]
pub struct Indices(HashMap<[u8; 9], Location>);

impl Indices {
    /// Load every `Data/data/*.idx` of the install (16 of them usually, one per
    /// bucket; keys of older builds linger in old files, the newest wins).
    pub fn load(data: &Path) -> Result<Self> {
        let mut out = HashMap::new();
        for entry in fs::read_dir(data)?.flatten() {
            let path = entry.path();
            if path.extension().is_none_or(|e| e != "idx") {
                continue;
            }
            let file = fs::read(&path).with_context(|| format!("{}", path.display()))?;
            let index = Self::read(&file).with_context(|| format!("{}", path.display()))?;
            out.extend(index);
        }
        Ok(Self(out))
    }

    /// One index file: a 0x28-byte header (wowdev.wiki/CASC), then 18-byte
    /// entries (9 of the key, 5 of the archive and offset as one big-endian
    /// number, 4 little-endian of the size).
    fn read(file: &[u8]) -> Result<HashMap<[u8; 9], Location>> {
        if file.len() < 0x28 {
            bail!("index file cut short");
        }
        // Header: how many bytes each part of an entry takes.
        let size_len = file[0x0C] as usize;
        let offset_len = file[0x0D] as usize;
        let key_len = file[0x0E] as usize;
        let offset_bits = file[0x0F] as usize;
        let entries_len = u32::from_le_bytes(file[0x20..0x24].try_into()?) as usize;
        let archive_bits = offset_len * 8 - offset_bits;
        let stride = size_len + offset_len + key_len;
        if stride == 0
            || key_len > 9
            || offset_bits >= offset_len * 8
            || archive_bits > 32
            || 0x28 + entries_len > file.len()
        {
            bail!("index entries out of bounds");
        }
        Ok(file[0x28..0x28 + entries_len]
            .chunks_exact(stride)
            .filter_map(|entry| {
                let key: [u8; 9] = entry[..key_len].try_into().ok()?;
                // The offset: a big-endian number whose top bits name the
                // archive and the low ones the place in it. The size: little.
                let raw = (entry[key_len..key_len + offset_len])
                    .iter()
                    .fold(0u64, |v, b| v << 8 | u64::from(*b));
                let size = (entry[key_len + offset_len..key_len + offset_len + size_len])
                    .iter()
                    .rev()
                    .fold(0u64, |v, b| v << 8 | u64::from(*b));
                Some((
                    key,
                    Location {
                        archive: ((raw >> offset_bits) & ((1 << archive_bits) - 1)) as u32,
                        offset: (raw & ((1 << offset_bits) - 1)) as u32,
                        size: size.min(u64::from(u32::MAX)) as u32,
                    },
                ))
            })
            .collect())
    }

    /// Where the file with this encoding key sits (its first bytes).
    pub fn get(&self, key: &[u8]) -> Option<Location> {
        self.0.get(&key[..key.len().min(9)]).copied()
    }
}
