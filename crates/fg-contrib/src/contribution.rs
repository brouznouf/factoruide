//! A player's contribution: what their game client and the addon saw, as one JSON document
//! sent by the app and imported into the quest database.

use crate::collector::Character;
use crate::wdb::CacheFile;
use anyhow::Result;
use serde::{Deserialize, Serialize};

pub const FORMAT: &str = "factoruide-contribution";
/// 2: `contributor` added.
pub const VERSION: u32 = 2;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Contribution {
    pub format: String,
    pub version: u32,
    /// Game version (forever, classic, tbc).
    pub edition: String,
    pub app_version: String,
    /// Random id of the app install (32 hex digits), the same for all its contributions: tells
    /// how many different players back a fact. Empty in version 1.
    #[serde(default)]
    pub contributor: String,
    /// Unix seconds.
    pub created: u64,
    /// The client's quest cache, per locale.
    pub quest_cache: Vec<CacheFile>,
    /// The addon's recordings, characters anonymized.
    pub collector: Vec<Character>,
}

impl Contribution {
    pub fn quests(&self) -> usize {
        let mut ids: Vec<u32> = self.quest_cache.iter().flat_map(|f| f.quests.keys().copied()).collect();
        ids.sort_unstable();
        ids.dedup();
        ids.len()
    }

    pub fn events(&self) -> usize {
        self.collector.iter().map(|c| c.events.len()).sum()
    }
}

/// Opaque, stable id of a character ("Name-Realm"): the same character always gives the same
/// id (duplicates are recognized) without its name leaving the computer.
pub fn character_id(name: &str) -> String {
    // FNV-1a 64 bits.
    let mut h: u64 = 0xcbf29ce484222325;
    for b in name.as_bytes() {
        h ^= u64::from(*b);
        h = h.wrapping_mul(0x100000001b3);
    }
    format!("{h:016x}")
}

/// Everything a game client folder holds for a contribution.
#[cfg(feature = "local")]
pub fn collect(
    client_dir: &std::path::Path,
    edition: &str,
    app_version: &str,
    contributor: &str,
) -> Result<Contribution> {
    use crate::{collector, wdb};
    let mut characters = collector::read(&collector::find_saved_variables(client_dir))?;
    for c in &mut characters {
        c.character = character_id(&c.character);
    }
    Ok(Contribution {
        format: FORMAT.into(),
        version: VERSION,
        edition: edition.into(),
        app_version: app_version.into(),
        contributor: contributor.into(),
        created: std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)?
            .as_secs(),
        quest_cache: wdb::read_cache(client_dir),
        collector: characters,
    })
}

/// Read a contribution document and check every field of it (`validate`): what comes from
/// outside is never trusted, whoever reads it (server, import).
pub fn parse(json: &[u8]) -> Result<Contribution> {
    anyhow::ensure!(json.len() <= crate::validate::MAX_BYTES, "contribution too large");
    let c: Contribution = serde_json::from_slice(json)
        // serde's messages may quote the document: only say where it went wrong.
        .map_err(|e| anyhow::anyhow!("invalid contribution document (line {}, column {})", e.line(), e.column()))?;
    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)?
        .as_secs();
    crate::validate::validate(&c, now)?;
    Ok(c)
}
