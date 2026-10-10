//! What a guide is made of: its steps, their kinds and targets, and the route around them.

use crate::faction::Faction;
use crate::model::EntityKind;
use crate::plan::Breakdown;
use crate::world::MapPoint;
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Target {
    pub kind: TargetKind,
    pub id: i64,
    pub name: String,
}

/// What a step of the guide asks.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum StepKind {
    #[serde(rename = "accept")]
    Accept,
    #[serde(rename = "objective")]
    Objective,
    #[serde(rename = "turnin")]
    TurnIn,
    #[serde(rename = "fly")]
    Fly,
    #[serde(rename = "link")]
    Link,
    #[serde(rename = "flight_master")]
    FlightMaster,
    #[serde(rename = "grind")]
    Grind,
    #[serde(rename = "practice")]
    Practice,
    #[serde(rename = "deathskip")]
    DeathSkip,
    #[serde(rename = "hearth")]
    Hearth,
    #[serde(rename = "train")]
    Train,
    #[serde(rename = "profession")]
    Profession,
    #[serde(rename = "dungeon")]
    Dungeon,
    #[serde(rename = "bind")]
    Bind,
    #[serde(rename = "abandon")]
    Abandon,
}

impl StepKind {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Accept => "accept",
            Self::Objective => "objective",
            Self::TurnIn => "turnin",
            Self::Fly => "fly",
            Self::Link => "link",
            Self::FlightMaster => "flight_master",
            Self::Grind => "grind",
            Self::Practice => "practice",
            Self::DeathSkip => "deathskip",
            Self::Hearth => "hearth",
            Self::Train => "train",
            Self::Profession => "profession",
            Self::Dungeon => "dungeon",
            Self::Bind => "bind",
            Self::Abandon => "abandon",
        }
    }
}

/// What a step's target is: a place of the world, a transport link, a profession skill.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum TargetKind {
    Npc,
    Object,
    Item,
    Area,
    Taxi,
    Dungeon,
    Link,
    Skill,
}

impl TargetKind {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Link => "link",
            Self::Skill => "skill",
            Self::Npc => EntityKind::Npc.as_str(),
            Self::Object => EntityKind::Object.as_str(),
            Self::Item => EntityKind::Item.as_str(),
            Self::Area => EntityKind::Area.as_str(),
            Self::Taxi => EntityKind::Taxi.as_str(),
            Self::Dungeon => EntityKind::Dungeon.as_str(),
        }
    }
}

impl From<EntityKind> for TargetKind {
    fn from(kind: EntityKind) -> Self {
        match kind {
            EntityKind::Npc => Self::Npc,
            EntityKind::Object => Self::Object,
            EntityKind::Item => Self::Item,
            EntityKind::Area => Self::Area,
            EntityKind::Taxi => Self::Taxi,
            EntityKind::Dungeon => Self::Dungeon,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Step {
    pub kind: StepKind,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub quest: Option<i64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub quest_name: Option<String>,
    /// Quests whose completion makes this one unavailable (exclusive alternatives, the quest a
    /// breadcrumb leads to): the addon skips the step when one of them is done.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub alt: Vec<i64>,
    /// Objective done along the way while doing other quests (does not block the guide).
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub bg: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub objective: Option<u8>,
    pub text: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub target: Option<Target>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub zone: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub map: Option<MapPointOut>,
    /// World position: [continent (map ID), x, y], to draw the route on any map.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub world: Option<[f64; 3]>,
    /// Profession skill a profession quest needs (its accept step is optional: skipped without
    /// it); in routes made before professions left the guide, the skill to reach (practice) or
    /// the rank cap learned (profession).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub skill: Option<i64>,
    /// Profession skill line of that skill.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub line: Option<i64>,
    /// Mobs to target (kill, or that drop the item), in the guide's language: for the target macro.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub mobs: Vec<String>,
    /// Objective on elite mobs, and the level of its mobs (hardcore warnings).
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub elite: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub mob_level: Option<i64>,
    /// Reward to pick among the quest's choices (item ID) on a turn-in.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub reward: Option<i64>,
    /// Class spells to learn (spell IDs) on a training step.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub spells: Vec<i64>,
    /// Character level and time (seconds) when the step is done, and its power (level plus
    /// what gear and spells add, see `power`) when known.
    /// Grind steps: XP to have in `level` before going on (none: reach `level`).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub xp: Option<i64>,
    pub level: i64,
    pub time: f64,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub power: Option<f64>,
    /// The part of `power` the gear brings (levels).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub gear: Option<f64>,
    /// Kept when skipping ahead to the next checkpoint: a chain going on after it, a class
    /// quest, a planned reward, training.
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub keep: bool,
}

/// A level where many quests open: a player reaching it before `step` (index in the steps) may
/// skip the steps up to it but those marked `keep`.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Checkpoint {
    pub level: i64,
    pub step: usize,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MapPointOut {
    pub ui_map: i64,
    pub x: f64,
    pub y: f64,
}

impl From<MapPoint> for MapPointOut {
    fn from(m: MapPoint) -> Self {
        Self {
            ui_map: m.ui_map,
            x: m.x,
            y: m.y,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Route {
    pub name: String,
    pub race_id: i64,
    pub class_id: i64,
    pub class: String,
    pub faction: Faction,
    pub from_level: i64,
    pub to_level: i64,
    pub total_time: f64,
    pub total_time_text: String,
    pub quests: usize,
    pub breakdown: Breakdown,
    #[serde(default)]
    pub professions: Vec<RouteProfession>,
    /// Language of the texts.
    #[serde(default)]
    pub locale: Option<String>,
    /// Class spells worth learning (spell IDs, every rank): the addon buys only these.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub spells: Vec<i64>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub checkpoints: Vec<Checkpoint>,
    pub steps: Vec<Step>,
}

/// A profession followed along the route, with its skill target per character level.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RouteProfession {
    pub key: String,
    pub name: String,
    pub skill_line: i64,
    pub target: i64,
    /// Level at which the profession is taken: the curve starts from 0 there.
    #[serde(default)]
    pub start: i64,
    /// Skill to have at each character level (index 0..=60).
    pub curve: Vec<i64>,
}
