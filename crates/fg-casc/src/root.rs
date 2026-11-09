//! The root manifest: which file of the game (its file data ID, and the hash
//! of its path) is which content. A header, then blocks of consecutive IDs
//! (wowdev.wiki/TACT#Root): the game's own files are found by path, the
//! additions of later versions (`dbfilesclient/locale/...`) by ID.

use super::jenkins::hashpath;
use anyhow::{Context, Result, bail};
use std::collections::HashMap;

/// The flag of a manifest block whose files are named: it holds their path
/// hashes too (a locale block: none).
const NAMED: u32 = 0x1000_0000;

/// The game's files: by path (its hash) and by file data ID, each to the
/// content hashes of the file (more than one when the manifest lists it once
/// per locale).
#[derive(Debug, Default)]
pub struct Root {
    by_name: HashMap<[u8; 8], Vec<[u8; 16]>>,
    by_fdid: HashMap<u32, Vec<[u8; 16]>>,
}

/// Read the root manifest out of its whole bytes (as BLTE decoded them). The
/// manifest of 8.2 and later: one `MFST` chunk of blocks (the pre-8.2 file, of
/// interleaved blocks, is not read: every client of the game's today writes
/// the chunked one).
pub fn read(data: &[u8]) -> Result<Root> {
    if data.len() < 4 || data[..4] != [0x54, 0x53, 0x46, 0x4d] {
        // (the manifest's own magic is little-endian: `TSFM` on the disk)
        bail!("not a root manifest");
    }
    let le = |from: usize| -> Result<u32> {
        Ok(u32::from_le_bytes(
            data.get(from..from + 4).context("the manifest cut short")?.try_into()?,
        ))
    };
    // 10.1.7+: the header's own size, then a version (1, or 2 from 11.1: the
    // blocks' content flags are split into three fields). Before: the counts
    // right after the magic.
    let (mut at, version, total, named) = if data.len() >= 24 && matches!(le(4)?, 0x14 | 0x18) {
        (le(4)? as usize, le(8)?, le(12)?, le(16)?)
    } else {
        (12, 1, le(4)?, le(8)?)
    };
    if version != 1 && version != 2 {
        bail!("unknown root manifest version {version}");
    }
    // A manifest with unnamed blocks (its counts differ): those hold no path
    // hashes, only the file data IDs.
    let allow_unnamed = total != named;
    let mut root = Root::default();
    // The blocks with named files hold the path hashes too (a locale block:
    // none); a manifest whose counts differ has those unnamed blocks in.
    while at + 4 <= data.len() {
        let count = le(at)? as usize;
        let mut head = at + 4;
        let flags = if version == 2 {
            // The block's content flags, split into three fields.
            let byte = *data.get(head + 12).context("a manifest block cut short")?;
            let flags = le(head + 4)? | le(head + 8)? | (u32::from(byte) << 17);
            head += 13;
            flags
        } else {
            let flags = le(head)?;
            head += 8;
            flags
        };
        let names = !(allow_unnamed && flags & NAMED != 0);
        let span = count * (4 + 16) + usize::from(names) * count * 8;
        if count == 0 || head + span > data.len() {
            break; // trailing padding
        }
        // The file data IDs: the first is itself, each one after adds its delta
        // and one (consecutive IDs have a delta of 0).
        let deltas = &data[head..head + count * 4];
        let ckeys_at = head + count * 4;
        let names_at = ckeys_at + count * 16;
        let mut prev: Option<u32> = None;
        for k in 0..count {
            let delta = i32::from_le_bytes(deltas[k * 4..k * 4 + 4].try_into()?);
            let fdid = match prev {
                None => delta as u32,
                Some(prev) => prev.wrapping_add(delta as u32).wrapping_add(1),
            };
            prev = Some(fdid);
            let ckey: [u8; 16] = data[ckeys_at + k * 16..ckeys_at + k * 16 + 16].try_into()?;
            if fdid != 0 && fdid != u32::MAX {
                let ekeys = root.by_fdid.entry(fdid).or_default();
                if names {
                    let hash: [u8; 8] = data[names_at + k * 8..names_at + k * 8 + 8].try_into()?;
                    root.by_name.entry(hash).or_default().push(ckey);
                }
                if !ekeys.contains(&ckey) {
                    ekeys.push(ckey);
                }
            }
        }
        at = names_at + usize::from(names) * count * 8;
    }
    Ok(root)
}

impl Root {
    /// The content hashes of the file at `path`, if the game has it. Case and
    /// slashes are normalized the way the game hashes them.
    pub fn by_name(&self, path: &str) -> Option<&[[u8; 16]]> {
        self.by_name.get(&hashpath(path)).map(Vec::as_slice)
    }

    /// The content hashes of the file with this data ID.
    pub fn by_fdid(&self, fdid: u32) -> Option<&[[u8; 16]]> {
        self.by_fdid.get(&fdid).map(Vec::as_slice)
    }

    /// How many files the manifest holds (for a report).
    pub fn len(&self) -> usize {
        self.by_fdid.len()
    }

    /// Whether the manifest holds no files.
    pub fn is_empty(&self) -> bool {
        self.by_fdid.is_empty()
    }
}
