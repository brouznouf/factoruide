//! Quests from the game client cache (`Cache/WDB/<locale>/questcache.wdb`): the server's answers
//! to quest queries for every quest a character has seen. It proves the quest exists in the
//! game version and gives exact objective targets and amounts.
//!
//! Record layout (Classic 1.15 / Forever 1.60 clients), after a fixed part not decoded here:
//!   objectives, each: u32 id | u8 type | 4 bytes | i32 object id (+9) | i32 amount (+13) | ...
//!                     | i32 visual effect count n (+33) | 4 bytes | n x i32 | u8 description
//!                     length | 1 byte | description
//!   12 bytes of bit-packed string lengths (big endian): title 9, log description 12, quest
//!   description 12, area description 9, giver text 10, giver name 8, turn-in text 10,
//!   turn-in name 8, completion log 11
//!   the strings, ending the record.
//! The string header is found by scanning; the position where a chain of objective records
//! ends exactly at it is preferred (method of Stein-N/WoW-Quest-Database's questcache.py).

use anyhow::{Result, bail};
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;
use std::path::Path;

const WIDTHS: [u32; 9] = [9, 12, 12, 9, 10, 8, 10, 8, 11];
const HEADER_LEN: usize = 12;
const OBJ_FIXED: usize = 43;
const MAX_OBJECTIVES: usize = 12;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Objective {
    pub kind: u8,
    pub target: i32,
    pub amount: i32,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CachedQuest {
    pub title: String,
    pub log: String,
    /// Empty when no objective chain could be confirmed.
    pub objectives: Vec<Objective>,
    pub confirmed: bool,
}

fn u32_at(b: &[u8], p: usize) -> u32 {
    u32::from_le_bytes(b[p..p + 4].try_into().unwrap())
}
fn i32_at(b: &[u8], p: usize) -> i32 {
    i32::from_le_bytes(b[p..p + 4].try_into().unwrap())
}

/// (magic, build, {id: bytes}).
pub type WdbRecords<'a> = (String, u32, BTreeMap<u32, &'a [u8]>);

/// Records of a WDB file.
pub fn read_wdb(data: &[u8]) -> Result<WdbRecords<'_>> {
    if data.len() < 24 {
        bail!("not a WDB file");
    }
    let magic = String::from_utf8_lossy(&data[..4]).into_owned();
    let build = u32_at(data, 4);
    let mut records = BTreeMap::new();
    let mut pos = 24;
    while pos + 8 <= data.len() {
        let (id, len) = (u32_at(data, pos), u32_at(data, pos + 4) as usize);
        if len == 0 || pos + 8 + len > data.len() {
            break;
        }
        records.insert(id, &data[pos + 8..pos + 8 + len]);
        pos += 8 + len;
    }
    Ok((magic, build, records))
}

fn lengths(rec: &[u8], pos: usize) -> [usize; 9] {
    let mut value: u128 = 0;
    for b in &rec[pos..pos + HEADER_LEN] {
        value = (value << 8) | u128::from(*b);
    }
    let mut shift = (HEADER_LEN * 8) as u32;
    let mut out = [0; 9];
    for (i, w) in WIDTHS.iter().enumerate() {
        shift -= w;
        out[i] = ((value >> shift) & ((1u128 << w) - 1)) as usize;
    }
    out
}

/// Objectives whose records chain up to end exactly at `header`, the longest chain found.
fn objective_chain(rec: &[u8], header: usize) -> Option<Vec<Objective>> {
    let mut best: Option<Vec<Objective>> = None;
    let lowest = header.saturating_sub(MAX_OBJECTIVES * (OBJ_FIXED + 255));
    let mut start = header.checked_sub(OBJ_FIXED)?;
    loop {
        let (mut pos, mut ids, mut objs) = (start, Vec::new(), Vec::new());
        while pos + OBJ_FIXED <= header {
            if u32_at(rec, pos) < 1000 || rec[pos + 4] > 30 {
                break;
            }
            let effects = i32_at(rec, pos + 33);
            if !(0..=8).contains(&effects) || pos + OBJ_FIXED + 4 * effects as usize > header {
                break;
            }
            let desc_len = rec[pos + 41 + 4 * effects as usize] as usize;
            let desc_start = pos + OBJ_FIXED + 4 * effects as usize;
            let end = desc_start + desc_len;
            if end > header || std::str::from_utf8(&rec[desc_start..end]).is_err() {
                break;
            }
            ids.push(u32_at(rec, pos));
            objs.push(Objective {
                kind: rec[pos + 4],
                target: i32_at(rec, pos + 9),
                amount: i32_at(rec, pos + 13),
            });
            pos = end;
        }
        let ids_ok = ids.windows(2).all(|w| w[1] > w[0] && w[1] - w[0] < 64);
        let amounts_ok = objs.iter().all(|o| o.amount > 0 && o.amount < 10000);
        if pos == header
            && !objs.is_empty()
            && ids_ok
            && amounts_ok
            && best.as_ref().is_none_or(|b| objs.len() > b.len())
        {
            best = Some(objs);
        }
        if start == 0 || start <= lowest {
            break;
        }
        start -= 1;
    }
    best
}

fn looks_like_title(t: &str) -> bool {
    !t.is_empty()
        && t.chars().count() <= 120
        && !t.contains('\n')
        && !t.starts_with(' ')
        && !t.chars().next().is_some_and(char::is_lowercase)
        && t.chars().all(|c| c as u32 >= 32)
}

pub fn parse_quest(rec: &[u8]) -> Option<CachedQuest> {
    let mut found: Vec<(bool, usize, CachedQuest)> = Vec::new();
    for pos in 0..rec.len().saturating_sub(HEADER_LEN) {
        let lens = lengths(rec, pos);
        let total: usize = lens.iter().sum();
        if lens[0] == 0 || total > rec.len() - pos - HEADER_LEN {
            continue;
        }
        let mut offset = rec.len() - total;
        let mut strings = Vec::with_capacity(9);
        let mut ok = true;
        for n in lens {
            if let Ok(s) = std::str::from_utf8(&rec[offset..offset + n]) {
                strings.push(s.to_owned());
            } else {
                ok = false;
                break;
            }
            offset += n;
        }
        if !ok || !looks_like_title(&strings[0]) || strings[1].is_empty() {
            continue;
        }
        let chain = objective_chain(rec, pos);
        let confirmed = chain.is_some();
        found.push((
            confirmed,
            pos,
            CachedQuest {
                title: strings[0].clone(),
                log: strings[1].clone(),
                objectives: chain.unwrap_or_default(),
                confirmed,
            },
        ));
    }
    found.sort_by_key(|f| (f.0, f.1));
    found.pop().map(|f| f.2)
}

/// Objective type sent by the server -> our objective kind.
pub fn kind(t: u8) -> Option<&'static str> {
    match t {
        0 => Some("creature"),
        1 => Some("item"),
        2 => Some("object"),
        _ => None,
    }
}

/// The quests of one locale's cache file.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CacheFile {
    /// Client locale (enUS, frFR...).
    pub locale: String,
    pub build: u32,
    pub quests: BTreeMap<u32, CachedQuest>,
}

/// Every quest cache of a game client folder (`_classic_era_`...), one per locale.
pub fn read_cache(client_dir: &Path) -> Vec<CacheFile> {
    let mut out = Vec::new();
    let Ok(dir) = std::fs::read_dir(client_dir.join("Cache/WDB")) else {
        return out;
    };
    for entry in dir.flatten() {
        let Ok(data) = std::fs::read(entry.path().join("questcache.wdb")) else {
            continue;
        };
        let Ok((magic, build, records)) = read_wdb(&data) else {
            continue;
        };
        if magic != "TSQW" {
            continue;
        }
        let quests = records
            .into_iter()
            .filter_map(|(id, rec)| parse_quest(rec).map(|q| (id, q)))
            .collect();
        out.push(CacheFile {
            locale: entry.file_name().to_string_lossy().into_owned(),
            build,
            quests,
        });
    }
    out.sort_by(|a, b| a.locale.cmp(&b.locale));
    out
}
