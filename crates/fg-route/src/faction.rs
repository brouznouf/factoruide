//! The character's faction: which NPCs talk to it, which flight paths and transports it uses.

use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum Faction {
    #[serde(alias = "alliance")]
    Alliance,
    #[serde(alias = "horde")]
    Horde,
}

impl Faction {
    pub fn name(self) -> &'static str {
        match self {
            Self::Alliance => "Alliance",
            Self::Horde => "Horde",
        }
    }

    /// Its letter in the database's `friendly_to` columns.
    pub fn letter(self) -> &'static str {
        match self {
            Self::Alliance => "A",
            Self::Horde => "H",
        }
    }

    /// "Horde" or "Alliance", whatever the case.
    pub fn parse(name: &str) -> Option<Self> {
        match name.to_lowercase().as_str() {
            "alliance" => Some(Self::Alliance),
            "horde" => Some(Self::Horde),
            _ => None,
        }
    }
}

impl std::fmt::Display for Faction {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(self.name())
    }
}
