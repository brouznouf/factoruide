//! What the contribution server records of each contribution it accepts, one JSON line per
//! contribution in `index.jsonl` next to the files (`<edition>/<sha256>.json`).

use serde::{Deserialize, Serialize};

pub const INDEX_FILE: &str = "index.jsonl";

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Receipt {
    /// SHA-256 of the document (hex): its file name, and what duplicates are recognized by.
    pub sha256: String,
    /// Unix seconds.
    pub received: u64,
    pub edition: String,
    pub contributor: String,
    /// Salted hash of the sender's address (its /64 for IPv6): tells contributions sent from
    /// the same place apart from independent ones, without keeping the address.
    pub client: String,
    pub app_version: String,
    pub bytes: u64,
    pub quests: usize,
    pub events: usize,
}
