//! What a planning job is asked, what it prepares and what it returns.

use crate::export::Route;
use crate::model::{self, Profile};
use crate::params::Params;
use crate::world::World;
use serde::{Deserialize, Serialize};

/// The key of a race in `races.toml`. A name of the editions' data rather than a closed list, so a
/// type of its own: it cannot be mixed up with a class or a free text.
#[derive(Debug, Clone, PartialEq, Eq, Hash, Default, Serialize, Deserialize)]
#[serde(transparent)]
pub struct RaceKey(String);

/// A class, by its file name or its name.
#[derive(Debug, Clone, PartialEq, Eq, Hash, Default, Serialize, Deserialize)]
#[serde(transparent)]
pub struct ClassKey(String);

macro_rules! text_key {
    ($($name:ident),*) => {$(
        impl $name {
            pub fn as_str(&self) -> &str {
                &self.0
            }
        }
        impl std::ops::Deref for $name {
            type Target = str;
            fn deref(&self) -> &str {
                &self.0
            }
        }
        impl From<&str> for $name {
            fn from(key: &str) -> Self {
                Self(key.to_owned())
            }
        }
        impl From<String> for $name {
            fn from(key: String) -> Self {
                Self(key)
            }
        }
        impl std::fmt::Display for $name {
            fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
                f.write_str(&self.0)
            }
        }
    )*};
}
text_key!(RaceKey, ClassKey);

/// A planning request: who, which levels, and every setting.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PlanRequest {
    /// Race key from `races.toml` (gnome, human...): the editions' data, not a closed list, so it
    /// is checked when the request is prepared rather than by its type.
    pub race: RaceKey,
    /// Class file name or name (mage, WARRIOR...).
    pub class: ClassKey,
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
    /// Quests selected by ID or exact name; prerequisites are included and completion is required.
    #[serde(default)]
    pub required_quests: Vec<RequiredQuest>,
    /// Professions to level along the route.
    #[serde(default)]
    pub professions: Vec<crate::profession::ProfessionGoal>,
    /// Language of the guide texts (enUS by default; English where a name is not translated).
    #[serde(default)]
    pub locale: Option<String>,
    /// The character as the addon recorded it in game: the route goes on from there (its level
    /// replaces `from_level`).
    #[serde(default)]
    pub start: Option<super::StartState>,
    /// The other players of a group guide, as their addon recorded them (empty: solo). The guide
    /// is the same for everyone (see `members`).
    #[serde(default)]
    pub mates: Vec<super::StartState>,
}

/// A quest ID or an exact name, resolved before planning.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(untagged)]
pub enum RequiredQuest {
    Id(i64),
    Name(String),
}

/// Players in a group guide, the character included.
pub const GROUP_MAX: usize = 5;
/// Level every member of a group guide must have reached.
pub const GROUP_MIN_LEVEL: i64 = 10;

impl PlanRequest {
    /// The members of a group guide (empty alone), the one with the least XP first: the route
    /// is planned for it, the others' XP followed beside (`Profile::mates`).
    pub fn members(&self, rules: &crate::xp::Rules) -> Vec<&super::StartState> {
        if self.mates.is_empty() {
            return Vec::new();
        }
        let mut members: Vec<_> = self.start.iter().chain(&self.mates).collect();
        let total = |m: &super::StartState| (1..m.level).map(|l| rules.to_next_level(l)).sum::<i64>() + m.xp;
        // Stable: the guide's own character first among equals.
        members.sort_by_key(|m| total(m));
        members
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PlanOutcome {
    pub route: Route,
    /// Human readable notes: skipped quests, class quests not planned...
    pub notes: Vec<String>,
    /// The paths around the terrain the planning searched (see `terrain::WalkMemory`): saved and
    /// shared with the guide, so that computing it again, or other guides, do not search them.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub walks: Option<crate::terrain::WalkMemory>,
    /// The route the guide was written from, as steps the guide editor changes (empty in guides
    /// computed before the editor).
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub plan: Vec<super::RouteStep>,
    /// `plan` has a step for each thing done (training, a trip's flights and boats, quests given
    /// up...); guides saved before only have the route's stops, which the editor completes.
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub explicit: bool,
    /// The character at the end of the guide, as its addon would record it (not saved).
    #[serde(skip)]
    pub end: Option<super::StartState>,
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
