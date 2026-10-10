//! What the planner works on: the character's profile, quests and their objectives, places, dungeons.

use super::names::Names;
use crate::world::Pos;
use std::collections::{HashMap, HashSet};

#[derive(Debug, Clone)]
pub struct Profile {
    pub name: String,
    pub race_id: i64,
    pub class_id: i64,
    pub class_name: String,
    pub faction: crate::faction::Faction,
    pub start_npc: i64,
    pub from_level: i64,
    pub to_level: i64,
    /// Names of class quests that must be done (see overrides/class_quests.toml).
    pub required_class_quests: Vec<String>,
    /// Class quests that make the character stronger: name and power gained.
    pub class_powers: Vec<(String, f64)>,
    /// Class trainer spells that count for power (see overrides/spells.toml).
    pub spell_weights: Vec<(String, crate::power::SpellWeight)>,
    /// Dungeon settings (see overrides/dungeons.toml).
    pub dungeons: Vec<DungeonDef>,
    /// Professions to level along the route.
    pub professions: Vec<crate::profession::ProfessionGoal>,
    /// Language of the guide texts (enUS, frFR, deDE...).
    pub locale: Option<String>,
    /// Classes of the other players leveling with the character (ChrClasses IDs).
    pub group_class_ids: Vec<i64>,
    /// The race deduced for each other player's class the character's race cannot play (a night
    /// elf for a human's druid): (class ID, race bit). Its class quests are of that race.
    pub group_races: Vec<(i64, i64)>,
}

/// Quest race bit of a race in QuestieDB Forever's encoding (Skyborne uses bits 32/33).
pub fn race_bit(race_id: i64) -> i64 {
    match race_id {
        95 => 1 << 32,
        96 => 1 << 33,
        id => 1 << (id - 1),
    }
}

impl Profile {
    /// Quest race bit of the character.
    pub fn race_bit(&self) -> i64 {
        race_bit(self.race_id)
    }

    pub fn class_bit(&self) -> i64 {
        1 << (self.class_id - 1)
    }

    /// Race bits a quest of classes `classes` is taken for: the character's, or for another
    /// player's class its race cannot play, the race deduced for that class only.
    pub fn quest_races(&self, classes: Option<i64>) -> i64 {
        let mask = classes.unwrap_or(0);
        if mask == 0 || mask & self.class_bit() != 0 {
            return self.race_bit();
        }
        let deduced = self
            .group_races
            .iter()
            .filter(|(class, _)| mask & (1 << (class - 1)) != 0)
            .fold(0, |m, (_, race)| m | race);
        if deduced == 0 { self.race_bit() } else { deduced }
    }

    /// Distinct classes of the group, the character's first.
    pub fn class_ids(&self) -> Vec<i64> {
        let mut ids = vec![self.class_id];
        for id in &self.group_class_ids {
            if !ids.contains(id) {
                ids.push(*id);
            }
        }
        ids
    }

    /// Class bits of the whole group.
    pub fn group_class_mask(&self) -> i64 {
        self.class_ids().iter().fold(0, |m, id| m | 1 << (id - 1))
    }
}

/// A place to go: where an entity stands (or the center of a group of spawns).
#[derive(Debug, Clone)]
pub struct Loc {
    pub pos: Pos,
    pub zone: i64,
    pub kind: EntityKind,
    pub id: i64,
    pub name: String,
}

#[derive(Debug, Clone)]
pub struct Objective {
    pub loc: Loc,
    pub text: String,
    /// Expected number of kills, their level, and object interactions.
    pub kills: f64,
    pub mob_level: i64,
    pub uses: f64,
    /// Fixed extra time (buying, exploring).
    pub extra: f64,
    /// Done inside this dungeon (index in `Model::dungeons`) during a group run.
    pub dungeon: Option<usize>,
    /// The mobs are elites (longer kills, double XP).
    pub elite: bool,
    /// Amount the quest asks for (kills, items, uses), for display.
    pub count: f64,
    /// NPCs to target for it (mobs to kill, or that drop the item): ID and English name.
    pub mobs: Vec<(i64, String)>,
    /// Where its mobs or objects are, to kill or loot those met on the way to other stops (kill
    /// and loot objectives on normal mobs, objects to use or loot).
    pub spots: Option<Spots>,
    /// Elites expected per pull: 1 for a lone elite (Hogger), more in a camp of elites.
    pub pull: f64,
    /// Hostile mobs around where it is done.
    pub guard: Guard,
}

/// Hostile mobs standing around a place (see `Guards`): the strongest level among them, how
/// many are close to it, and whether one of those is an elite. Default: nobody.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct Guard {
    pub level: i64,
    pub count: u32,
    pub elite: bool,
}

impl Guard {
    /// The harder of two places.
    #[must_use]
    pub fn max(self, other: Self) -> Self {
        if (other.level, other.count, other.elite) > (self.level, self.count, self.elite) {
            other
        } else {
            self
        }
    }
}

/// Spawn points of an objective's mobs on one continent, one per cell of a small grid.
#[derive(Debug, Clone)]
pub struct Spots {
    pub continent: i64,
    pub min: (f64, f64),
    pub max: (f64, f64),
    pub points: Vec<(f64, f64)>,
}

impl Spots {
    /// The spawns of `points` on the continent of `center`; None when there are none.
    pub(super) fn new(points: &[(Pos, i64)], center: &Pos) -> Option<Self> {
        const CELL: f64 = 20.0;
        let mut cells = HashSet::new();
        let points: Vec<(f64, f64)> = points
            .iter()
            .map(|(p, _)| p)
            .filter(|p| p.continent == center.continent)
            .filter(|p| cells.insert(((p.x / CELL).floor() as i64, (p.y / CELL).floor() as i64)))
            .map(|p| (p.x, p.y))
            .collect();
        if points.is_empty() {
            return None;
        }
        let (mut min, mut max) = ((f64::INFINITY, f64::INFINITY), (f64::NEG_INFINITY, f64::NEG_INFINITY));
        for &(x, y) in &points {
            min = (min.0.min(x), min.1.min(y));
            max = (max.0.max(x), max.1.max(y));
        }
        Some(Self {
            continent: center.continent,
            min,
            max,
            points,
        })
    }
}

#[derive(Debug, Clone)]
pub struct Quest {
    pub id: i64,
    pub name: String,
    pub level: i64,
    pub min_level: i64,
    pub xp: i64,
    /// `xp` was received in game (without level penalty), not taken from Classic data.
    pub xp_observed: bool,
    pub starts: Vec<Loc>,
    pub ends: Vec<Loc>,
    pub objectives: Vec<Objective>,
    pub pre_all: Vec<i64>,
    pub pre_any: Vec<i64>,
    pub exclusive: Vec<i64>,
    pub breadcrumb_for: Option<i64>,
    /// Work to get the item that starts the quest (kills at mob level, object uses).
    pub start_kills: f64,
    pub start_mob_level: i64,
    pub start_uses: f64,
    /// Class quests: always planned, the optimizer cannot drop them.
    pub mandatory: bool,
    /// Profession quest: index in `Model::professions` and skill needed to accept it.
    pub skill: Option<(usize, i64)>,
    /// Class quest making the character stronger: index in `Profile::class_powers` (quests of
    /// the same name share it) and power gained when turned in.
    pub power: Option<(u8, f64)>,
    /// Hostile mobs around its givers and its turn-ins (the hardest of them).
    pub start_guard: Guard,
    pub end_guard: Guard,
}

pub struct Model {
    pub quests: Vec<Quest>,
    pub dungeons: Vec<Dungeon>,
    pub professions: Vec<ProfessionPlan>,
    pub index: HashMap<i64, usize>,
    pub start: Loc,
    /// XP of the quests each quest unlocks (whole chain, discounted per step).
    pub unlocks: Vec<f64>,
    /// Highest level among the quests each quest leads to (follow-ups and breadcrumb target,
    /// whole chain; 0 for none).
    pub chain_top: Vec<i64>,
    /// Class trainers and innkeepers the character can use.
    pub trainers: Vec<Loc>,
    pub inns: Vec<Loc>,
    /// Where the hearthstone takes a new character: its starting place (not an inn to bind).
    pub start_inn: usize,
    /// Quests left out, with the reason (for reports and debugging).
    pub skipped: Vec<(i64, String, String)>,
    pub initial: Initial,
    /// Names in the guide's language.
    pub names: Names,
    /// Areas that give XP when discovered.
    pub explore: Vec<ExploreArea>,
    /// Indexes into `explore` by zone.
    pub explore_by_zone: HashMap<i64, Vec<usize>>,
    /// Normal mobs anyone can farm: on the way (see `Params::farm_on_way`), and where the
    /// guide sends the character to grind.
    pub farm: super::FarmMobs,
    /// Character power data (gear, class spells); `None` without the data (TBC).
    pub power: Option<crate::power::PowerModel>,
    /// Items each quest gives (indexes in `power.items`): always, and one of the choices.
    pub rewards: Vec<(Vec<u16>, Vec<u16>)>,
    /// Items each dungeon's bosses drop, with the chance the character gets each.
    pub dungeon_loot: Vec<Vec<(u16, f64)>>,
}

/// An area that gives XP the first time the character enters it.
#[derive(Debug, Clone)]
pub struct ExploreArea {
    /// Center of the area (its map exploration overlay).
    pub pos: Pos,
    pub zone: i64,
    /// Exploration level (sets the XP).
    pub level: i64,
}

/// Character state at the start of the route when it does not start from scratch (for
/// instance to start a route at level 21).
#[derive(Debug, Clone, Default)]
pub struct Initial {
    /// Quests (indices) already turned in or in the log.
    pub turned: Vec<usize>,
    pub accepted: Vec<usize>,
    /// Start position instead of the race start NPC.
    pub pos: Option<(Pos, i64)>,
    /// Inn (index) the hearthstone is bound to.
    pub bind: Option<usize>,
    /// Known flight paths (bit per taxi node).
    pub known: u128,
    /// XP into the starting level, and rested XP (kills give double up to it).
    pub xp: i64,
    pub rested: i64,
    /// Objectives of the quests in the log already started: (quest, objective, share done).
    pub progress: Vec<(usize, u8, f64)>,
    /// Items worn (IDs).
    pub gear: Vec<i64>,
    /// Level of the last class training (default: the starting level).
    pub trained: Option<i64>,
    /// Every quest turned in, in the model or not, for a character met in game: prerequisites
    /// the model lacks are then known instead of assumed done past level 1.
    pub completed: Option<HashSet<i64>>,
    /// The log of a character met in game (ID, name): its quests are left to the guide as if
    /// never taken (it takes again those it wants), the others are abandoned at its start.
    pub log: Vec<(i64, String)>,
}

impl Initial {
    /// Whether quest `id`, which the model does not have, was turned in before the route.
    pub fn done_outside(&self, id: i64, from_level: i64) -> bool {
        self.completed.as_ref().map_or(from_level > 1, |c| c.contains(&id))
    }
}

/// Per-dungeon settings from `overrides/dungeons.toml`.
#[derive(Debug, Clone, Default, serde::Deserialize)]
pub struct DungeonDef {
    pub area: i64,
    pub name: Option<String>,
    pub entrance: Option<crate::world::ZonePoint>,
    pub min_level: Option<i64>,
    pub max_level: Option<i64>,
    pub minutes: Option<f64>,
    pub kills: Option<f64>,
}

/// A 5-man dungeon run: its objectives are completed in one go with a group.
#[derive(Debug, Clone)]
pub struct Dungeon {
    pub area: i64,
    pub name: String,
    pub entrance: Loc,
    pub min_level: i64,
    pub max_level: i64,
    pub minutes: f64,
    pub kills: f64,
    pub mob_level: i64,
    /// Expansion of its content, for kill XP (that of its instance map: the Caverns of Time
    /// dungeons are Outland content behind a door in Tanaris).
    pub content: usize,
}

/// A profession the character levels: the skill it should have at each level.
#[derive(Debug, Clone)]
pub struct ProfessionPlan {
    pub key: String,
    pub name: String,
    pub target: i64,
    /// Level at which the profession is taken (the curve starts from 0 there).
    pub start: i64,
    /// Skill to have by character level (index 0..=60).
    pub curve: Vec<i64>,
}

impl ProfessionPlan {
    /// The plan of a profession goal (None for an unknown profession).
    pub fn new(goal: &crate::profession::ProfessionGoal) -> Option<Self> {
        let def = crate::profession::find(&goal.key)?;
        let target = goal.target.clamp(1, 300);
        let start = goal.start_level.unwrap_or(crate::profession::START_LEVEL).clamp(1, 60);
        let milestones = if goal.milestones.is_empty() {
            crate::profession::default_milestones(def.key)
        } else {
            goal.milestones.clone()
        };
        Some(Self {
            key: def.key.to_owned(),
            name: def.name.to_owned(),
            target,
            start,
            curve: (0..=60)
                .map(|l| crate::profession::target_at(target, &milestones, start, l))
                .collect(),
        })
    }

    /// Skill the character has at `level`, following the curve.
    pub fn skill_at(&self, level: i64) -> i64 {
        self.curve[level.clamp(0, 60) as usize]
    }
}

/// What a place is: an NPC, a world object, where an item is got, an area to reach, a flight
/// master. Written as its lowercase name in the database and the exported routes.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum EntityKind {
    Npc,
    Object,
    Item,
    Area,
    Taxi,
    /// A dungeon's entrance.
    Dungeon,
}

impl EntityKind {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Npc => "npc",
            Self::Object => "object",
            Self::Item => "item",
            Self::Area => "area",
            Self::Taxi => "taxi",
            Self::Dungeon => "dungeon",
        }
    }

    pub fn parse(name: &str) -> Option<Self> {
        match name {
            "npc" => Some(Self::Npc),
            "object" => Some(Self::Object),
            "item" => Some(Self::Item),
            "area" => Some(Self::Area),
            "taxi" => Some(Self::Taxi),
            "dungeon" => Some(Self::Dungeon),
            _ => None,
        }
    }
}
