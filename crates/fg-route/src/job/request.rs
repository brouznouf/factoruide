//! What a planning job is asked, what it prepares and what it returns.

use crate::export::Route;
use crate::model::{self, Profile};
use crate::params::Params;
use crate::world::World;
use serde::{Deserialize, Serialize};

/// A planning request: who, which levels, and every setting.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PlanRequest {
    /// Race key from `races.toml` (gnome, human...).
    pub race: String,
    /// Class file name or name (mage, WARRIOR...).
    pub class: String,
    pub from_level: i64,
    pub to_level: i64,
    /// Route name (default `<race>-<class>`).
    #[serde(default)]
    pub name: Option<String>,
    /// All tunables; defaults to `overrides/routing.toml`.
    #[serde(default)]
    pub params: Option<Params>,
    /// Class quests that must be done; defaults to `overrides/class_quests.toml`.
    #[serde(default)]
    pub required_class_quests: Option<Vec<String>>,
    /// Professions to level along the route.
    #[serde(default)]
    pub professions: Vec<crate::profession::ProfessionGoal>,
    /// Language of the guide texts (enUS by default; English where a name is not translated).
    #[serde(default)]
    pub locale: Option<String>,
    /// Classes of the other players leveling with the character (empty: solo).
    #[serde(default)]
    pub group: Vec<String>,
    /// The character as the addon recorded it in game: the route goes on from there (its level
    /// replaces `from_level`).
    #[serde(default)]
    pub start: Option<super::StartState>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PlanOutcome {
    pub route: Route,
    /// Human readable notes: skipped quests, class quests not planned...
    pub notes: Vec<String>,
}

impl PlanOutcome {
    /// An outcome as saved (JSON), its guide migrated to the current format.
    pub fn from_value(mut value: serde_json::Value) -> anyhow::Result<Self> {
        if let Some(route) = value.get_mut("route") {
            crate::export::format::migrate(route)?;
        }
        Ok(serde_json::from_value(value)?)
    }

    pub fn from_json(json: &str) -> anyhow::Result<Self> {
        Self::from_value(serde_json::from_str(json)?)
    }
}

/// Everything a planner needs, loaded for a request.
pub struct Prepared {
    pub profile: Profile,
    pub params: Params,
    pub world: World,
    pub model: model::Model,
    pub notes: Vec<String>,
}
