//! The encoding table: the content hash of every file of the game to the
//! encoding hashes its data is stored under (its `EKey`, what the local
//! indices know). A content hash can have more than one encoding hash (an
//! encrypted and a plain variant, say): the file is read under whichever the
//! install has. A header, a block of encoding specifications, then pages of
//! entries sorted by content hash (wowdev.wiki/TACT).

use anyhow::{Result, bail};
use std::collections::HashMap;

/// Content hash → its encoding hashes.
#[derive(Debug, Default)]
pub struct Encoding(pub(super) HashMap<[u8; 16], Vec<[u8; 16]>>);

/// Read the encoding table out of its whole bytes (as BLTE decoded them).
pub fn read(data: &[u8]) -> Result<Encoding> {
    if data.len() < 0x16 || &data[..2] != b"EN" {
        bail!("not an encoding table");
    }
    let be =
        |from: usize, len: usize| -> u64 { data[from..from + len].iter().fold(0u64, |v, b| v << 8 | u64::from(*b)) };
    let ckey_len = data[3] as usize;
    let ekey_len = data[4] as usize;
    let page_size = be(5, 2) as usize * 1024;
    let page_count = be(9, 4) as usize;
    let espec_len = be(0x12, 4) as usize;
    if ckey_len != 16 || ekey_len != 16 {
        bail!("unexpected hash sizes {ckey_len}/{ekey_len}");
    }
    // The page index: the first content key of every page, then the pages
    // themselves, packed with entries as long as a whole entry fits.
    let pages_at = 0x16 + espec_len + page_count * (ckey_len + 16);
    let mut out: HashMap<[u8; 16], Vec<[u8; 16]>> = HashMap::new();
    for page in 0..page_count {
        let from = pages_at + page * page_size;
        let to = (from + page_size).min(data.len());
        let mut at = from;
        while at + 1 + 5 + ckey_len <= to {
            let key_count = data[at] as usize;
            let size = 1 + 5 + ckey_len + key_count * ekey_len;
            if key_count == 0 || at + size > to {
                break; // the page is padded to its end
            }
            let ckey: [u8; 16] = data[at + 6..at + 6 + ckey_len].try_into()?;
            let ekeys = out.entry(ckey).or_default();
            for k in 0..key_count {
                let start = at + 6 + ckey_len + k * ekey_len;
                let ekey: [u8; 16] = data[start..start + ekey_len].try_into()?;
                ekeys.push(ekey);
            }
            at += size;
        }
    }
    Ok(Encoding(out))
}

impl Encoding {
    /// The encoding hashes of the file with this content hash, in the table's
    /// order.
    pub fn all(&self, ckey: &[u8; 16]) -> Option<&[[u8; 16]]> {
        self.0.get(ckey).map(Vec::as_slice)
    }
}
