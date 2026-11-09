//! The client's hotfix cache (`Cache\ADB\<locale>\DBCache.bin`): the records the
//! server pushed into the client as it played, among them the TACT keys its
//! encrypted files decrypt with — embedded in BroadcastText records, as their
//! extra data (wowdev.wiki/ADB). The shipped `TactKey` tables carry the old
//! keys; the server hands the current ones out this way.

use crate::blte::Keys;
use anyhow::{Context, Result, bail};

/// The magic of a hotfix cache and of each of its entries.
const MAGIC: u32 = 0x5846_5448;
/// The one version read: its entries hold their own size.
const VERSION: u32 = 9;
/// The table the BroadcastText records belong to.
const BROADCAST_TEXT: u32 = 0x0218_26BB;
/// The marker of a TACT key embedded as a BroadcastText record's extra data.
const TACT_KEY: u32 = 0xDF2F_53CF;

/// The TACT keys the server pushed into the client, by the name the encrypted
/// blocks ask for. (Every entry of the cache, not only the latest of each: an
/// old key beats none.)
pub fn tact_keys(data: &[u8]) -> Result<Keys> {
    if data.len() < 44 || u32::from_be_bytes(data.get(..4).context("a cache cut short")?.try_into()?) != MAGIC {
        bail!("not a hotfix cache");
    }
    if u32::from_le_bytes(data.get(4..8).context("a cache cut short")?.try_into()?) != VERSION {
        bail!("an unknown hotfix cache version");
    }
    let mut keys = Keys::new();
    // The header of its own 44 bytes, then its entries: a 32-byte head each
    // (its region, its push, the table and the ID of its record, the record's
    // size), then the record's bytes.
    let mut at = 44;
    while at + 32 <= data.len() {
        if u32::from_be_bytes(data.get(at..at + 4).context("a cache cut short")?.try_into()?) != MAGIC {
            bail!("a hotfix entry out of step");
        }
        let le = |o: usize| -> Result<u32> {
            let bytes = data
                .get(at + o..at + o + 4)
                .with_context(|| format!("a hotfix entry cut short at {o}"))?;
            Ok(u32::from_le_bytes(bytes.try_into()?))
        };
        let size = le(24)? as usize;
        let record = data
            .get(at + 32..at + 32 + size)
            .context("a hotfix entry out of bounds")?;
        if le(16)? == BROADCAST_TEXT
            && let Some((name, key)) = embedded_key(record)
        {
            keys.insert(name, key);
        }
        at += 32 + size;
    }
    Ok(keys)
}

/// The TACT key embedded as a BroadcastText record's extra data, if it carries
/// one: after its two texts, among its fields, a marker, then the key's name
/// and the key itself.
fn embedded_key(record: &[u8]) -> Option<([u8; 8], [u8; 16])> {
    // The two texts, null-terminated.
    let mut at = record.iter().position(|&b| b == 0)? + 1;
    at += record[at..].iter().position(|&b| b == 0)? + 1;
    // 43 bytes of the record's other fields, then the marker.
    at += 1 + 43;
    let rest = record.get(at..)?;
    if rest.len() < 4 + 8 + 16 || u32::from_le_bytes(rest[..4].try_into().ok()?) != TACT_KEY {
        return None;
    }
    Some((rest[4..12].try_into().ok()?, rest[12..28].try_into().ok()?))
}
