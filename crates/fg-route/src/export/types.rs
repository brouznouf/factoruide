//! What a guide is made of: its steps, their kinds and targets, and the route around them.

use super::phrase::Phrase;
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
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
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
    #[serde(rename = "teleport")]
    Teleport,
    #[serde(rename = "train")]
    Train,
    #[serde(rename = "sell")]
    Sell,
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
            Self::Teleport => "teleport",
            Self::Train => "train",
            Self::Sell => "sell",
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
    /// Accept steps: what the quest needs before it can be taken, so that the addon lists the
    /// quests that can be taken now: quests all done (`pre`), one of them done (`pre_any`), the
    /// character's level.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub pre: Vec<i64>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub pre_any: Vec<i64>,
    /// How many of `pre_any` must be done, when more than one (the library's books).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub pre_need: Option<usize>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub min_level: Option<i64>,
    /// Objective done along the way while doing other quests (does not block the guide).
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub bg: bool,
    /// An item sold in limited supply, offered at another vendor passed by on the way (along the
    /// way, `bg`): gone once the item is got, or once left behind.
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub try_buy: bool,
    /// An item to buy (from a vendor, at an auction house): the addon buys it at any merchant
    /// selling it while the guide still needs it.
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub buy: bool,
    /// A quest done inside the dungeon of the run step before it: the run is done on entering
    /// the dungeon, its quests as they progress.
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub inside: bool,
    /// Accept step of a quest its giver offers for an item in the bags (a book for the
    /// library): done once the item is looted; the quest is taken with its turn-in.
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub held: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub objective: Option<u8>,
    /// The quest's own objective it is, from 1 in the game's order; 0 for what the quest only
    /// needs (an item it uses). None in guides made before it was written.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub slot: Option<u8>,
    /// Objective steps handing in a child quest on the way to its parent's turn-in (Distracting
    /// Jarven for Bitter Rivals): done once that quest is turned in.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub child: Option<i64>,
    /// That child quest's name.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub child_name: Option<String>,
    /// The step's sentence in a language: written from `say` (`render`), else (a guide of
    /// format 1) kept as it was written.
    #[serde(default)]
    pub text: String,
    /// The step's sentences, language-neutral.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub say: Vec<Phrase>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub target: Option<Target>,
    /// Zone of the step (its English name, written in a language by `render`) and its ID.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub zone: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub zone_id: Option<i64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub map: Option<MapPointOut>,
    /// Other places its mobs are, closest first: where to look when there is nothing at `map`
    /// (mobs to kill or loot, the mob dropping the item that starts a quest).
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub spots: Vec<MapPointOut>,
    /// The step's NPC patrols: `spots` are places along its path (the addon points at the
    /// nearest first).
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub patrol: bool,
    /// World position: [continent (map ID), x, y], to draw the route on any map.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub world: Option<[f64; 3]>,
    /// Height of the place (yards) when known: the floor it stands on where several lie over
    /// each other (a cave under a hill, a bridge over a river).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub height: Option<i16>,
    /// The walk to the step from the step before when it does not go straight (world x, y on
    /// the step's continent): where it starts, then where it turns, around the steep ground and
    /// through the passages between zones. The walk the guide's replay makes (`Editor::set_walks`),
    /// the one the app draws; a guide saved without it gets one when the addon is installed
    /// (`job::add_walk_paths`).
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub path: Vec<[f64; 2]>,
    /// Profession skill a profession quest needs (its accept step is optional: skipped without
    /// it); in routes made before professions left the guide, the skill to reach (practice) or
    /// the rank cap learned (profession).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub skill: Option<i64>,
    /// Profession skill line of that skill.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub line: Option<i64>,
    /// Mobs to target (kill, or that drop the item), for the target macro: their names (English,
    /// written in a language by `render`) and IDs.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub mobs: Vec<String>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub mob_ids: Vec<i64>,
    /// Objective on elite mobs, and the level of its mobs (hardcore warnings).
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub elite: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub mob_level: Option<i64>,
    /// Reward to pick among the quest's choices (item ID) on a turn-in.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub reward: Option<i64>,
    /// XP a turn-in gives at the level it is planned at: a quest complete in the log counts for
    /// the grind steps aiming at a level it is turned in before.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub reward_xp: Option<i64>,
    /// XP the route has in `level` once the step is done: where the player should be, for the
    /// addon's XP bars.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub route_xp: Option<i64>,
    /// Class spells to learn (spell IDs) on a training step.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub spells: Vec<i64>,
    /// Character level and time (seconds) when the step is done, and its power (level plus
    /// what gear and spells add, see `power`) when known.
    /// Grind steps: XP to have in `level` before going on (none: reach `level`).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub xp: Option<i64>,
    /// Mobs to kill on the way to the step (farming on the way), and the XP they give (rested
    /// bonus included): the addon counts the XP of the kills (guides made before: the kills).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub farm: Option<u32>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub farm_xp: Option<i64>,
    pub level: i64,
    pub time: f64,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub power: Option<f64>,
    /// The part of `power` the gear brings (levels).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub gear: Option<f64>,
    /// The players of a group guide when the step is done, in the guide's `players` order: level
    /// and XP into it (`level` is the lowest one's).
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub party: Vec<(i64, i64)>,
    /// In a group guide, the players the step is for (a bit each in the guide's `players`
    /// order) when not everyone: a quest some did already, players catching up.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub members: Option<u8>,
    /// Kept when skipping ahead to the next checkpoint: a chain going on after it, a class
    /// quest, a planned reward, training.
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub keep: bool,
    /// The event of the replay it is written from (index in its trace; not saved): ties a guide
    /// built from a replay back to it (`job::timeline`). None for the steps added after it
    /// (checkpoints, the quests of the character's log given up first).
    #[serde(skip)]
    pub event: Option<usize>,
}

impl Step {
    /// A step of `kind` with nothing else said, done at `time` at `level`.
    pub fn bare(kind: StepKind, level: i64, time: f64) -> Self {
        Self {
            kind,
            quest: None,
            quest_name: None,
            alt: Vec::new(),
            pre: Vec::new(),
            pre_any: Vec::new(),
            pre_need: None,
            min_level: None,
            bg: false,
            try_buy: false,
            buy: false,
            inside: false,
            held: false,
            objective: None,
            slot: None,
            child: None,
            child_name: None,
            text: String::new(),
            say: Vec::new(),
            target: None,
            zone: None,
            zone_id: None,
            map: None,
            spots: Vec::new(),
            patrol: false,
            world: None,
            height: None,
            path: Vec::new(),
            skill: None,
            line: None,
            mobs: Vec::new(),
            mob_ids: Vec::new(),
            elite: false,
            mob_level: None,
            reward: None,
            reward_xp: None,
            route_xp: None,
            spells: Vec::new(),
            xp: None,
            farm: None,
            farm_xp: None,
            level,
            time,
            power: None,
            gear: None,
            party: Vec::new(),
            members: None,
            keep: false,
            event: None,
        }
    }
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
    /// Format of the guide file (`format::FORMAT`); files of an older format are migrated
    /// when read (`format::migrate`).
    #[serde(default = "super::format::first")]
    pub format: u32,
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
    /// Language the guide was made in (its texts are written in it, and in others by `render`).
    #[serde(default)]
    pub locale: Option<String>,
    /// Class spells worth learning (spell IDs, every rank): the addon buys only these.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub spells: Vec<i64>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub checkpoints: Vec<Checkpoint>,
    /// The reputation goals: where the character starts and ends with each faction.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub reputation: Vec<ReputationReached>,
    /// The players of a group guide (none alone), in the order of the steps' `members` bits:
    /// the one planned for first (its race and class are the guide's).
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub players: Vec<Player>,
    /// Levels a group's players are apart at most once together (`Params::group_sync_max`):
    /// further, the addon tells them to make the guide again.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub sync_max: Option<f64>,
    /// The spells worth learning of the group's other classes (as `spells`): class ID, spells.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub class_spells: Vec<(i64, Vec<i64>)>,
    pub steps: Vec<Step>,
}

/// A player of a group guide.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Player {
    /// "Name-Realm".
    pub name: String,
    pub race_id: i64,
    pub class_id: i64,
    /// The professions it levels, as the guide's `professions` for the character.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub professions: Vec<RouteProfession>,
}

/// A reputation goal and how far the guide gets: points counted from the bottom of Neutral.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ReputationReached {
    pub faction: i64,
    pub name: String,
    pub start: i64,
    pub reached: i64,
    pub target: i64,
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
