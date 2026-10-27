//! What the planner works on: the character's profile, quests and their objectives, places, dungeons.

use super::names::Names;
use crate::world::Pos;
use rustc_hash::FxHashMap;
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
    pub quest_overrides: Vec<QuestOverride>,
    /// Exact quests selected by the player.
    pub required_quests: Vec<i64>,
    /// Confirmed completions from the starting profile, never inferred from character level.
    pub completed_quests: HashSet<i64>,
    /// Class quests that make the character stronger: name and power gained.
    pub class_powers: Vec<(String, f64)>,
    /// Class trainer spells that count for power (see overrides/spells.toml).
    pub spell_weights: Vec<(String, crate::power::SpellWeight)>,
    /// Dungeon settings (see overrides/dungeons.toml).
    pub dungeons: Vec<DungeonDef>,
    /// The class's teleports that a quest teaches (see overrides/travel.toml).
    pub quest_teleports: Vec<TeleportDef>,
    /// Objectives done only once others of their quest are (see overrides/objectives.toml).
    pub objective_orders: Vec<ObjectiveOrder>,
    /// Professions to level along the route.
    pub professions: Vec<crate::profession::ProfessionGoal>,
    /// Language of the guide texts (enUS, frFR, deDE...).
    pub locale: Option<String>,
    /// The other players of a group guide (none: a solo guide). The character is the one with
    /// the least XP; the route is the same for everyone.
    pub mates: Vec<Mate>,
    /// The character's "Name-Realm" in a group guide (empty alone).
    pub player: String,
}

/// Another player of a group guide.
#[derive(Debug, Clone)]
pub struct Mate {
    /// "Name-Realm".
    pub name: String,
    pub race_id: i64,
    pub class_id: i64,
    /// Its class trainer spells that count for power (see overrides/spells.toml).
    pub spell_weights: Vec<(String, crate::power::SpellWeight)>,
    /// The professions it levels.
    pub professions: Vec<crate::profession::ProfessionGoal>,
}

/// Quest race bit of a race in the quest data's Forever encoding (Skyborne uses bits 32/33).
pub fn race_bit(race_id: i64) -> i64 {
    match race_id {
        95 => 1 << 32,
        96 => 1 << 33,
        id => 1 << (id - 1),
    }
}

impl Profile {
    pub(super) fn within_levels(&self, level: i64, min_level: i64, max_above: i64) -> bool {
        min_level <= self.to_level && level <= self.to_level + max_above && level + 10 >= self.from_level
    }

    /// Reputation gained for each point a quest gives: humans' Diplomacy adds 10%.
    pub(crate) fn reputation_rate(&self) -> f64 {
        if self.race_id == 1 { 1.1 } else { 1.0 }
    }

    /// Quest race bit of the character.
    pub fn race_bit(&self) -> i64 {
        race_bit(self.race_id)
    }

    pub(crate) fn class_bit(&self) -> i64 {
        1 << (self.class_id - 1)
    }

    /// Race and class bits of each member of the group, the character first.
    pub fn members(&self) -> impl Iterator<Item = (i64, i64)> + '_ {
        std::iter::once((self.race_bit(), self.class_bit()))
            .chain(self.mates.iter().map(|m| (race_bit(m.race_id), 1 << (m.class_id - 1))))
    }

    /// The members (a bit each, the character's 1) a quest of races `races` and classes
    /// `classes` is for.
    /// The professions of the whole group the planner knows, each once (the first player's
    /// goal), with the members leveling it (a bit each, the character's 1).
    pub(crate) fn profession_goals(&self) -> Vec<(&crate::profession::ProfessionGoal, u8)> {
        let mut goals: Vec<(&crate::profession::ProfessionGoal, u8)> = Vec::new();
        let players = std::iter::once(&self.professions).chain(self.mates.iter().map(|m| &m.professions));
        for (k, own) in players.enumerate() {
            for goal in own.iter().filter(|g| crate::profession::find(&g.key).is_some()) {
                match goals.iter_mut().find(|(g, _)| g.key == goal.key) {
                    Some((_, members)) => *members |= 1 << k,
                    None => goals.push((goal, 1 << k)),
                }
            }
        }
        goals
    }

    pub(crate) fn members_of(&self, races: Option<i64>, classes: Option<i64>) -> u8 {
        let (races, classes) = (races.unwrap_or(0), classes.unwrap_or(0));
        (self.members().enumerate())
            .filter(|(_, (race, class))| (races == 0 || races & race != 0) && (classes == 0 || classes & class != 0))
            .fold(0, |m, (k, _)| m | 1 << k)
    }

    /// Names of the members of the group in `members` (a bit each, the character's 1), without
    /// their realm.
    pub(crate) fn member_names(&self, members: u8) -> Vec<String> {
        let short = |n: &str| n.split('-').next().unwrap_or(n).to_owned();
        (std::iter::once(&self.player)
            .chain(self.mates.iter().map(|m| &m.name))
            .enumerate())
        .filter(|(k, _)| members & (1 << k) != 0)
        .map(|(_, n)| short(n))
        .collect()
    }

    /// Distinct classes of the group, the character's first.
    pub(crate) fn class_ids(&self) -> Vec<i64> {
        let mut ids = vec![self.class_id];
        for m in &self.mates {
            if !ids.contains(&m.class_id) {
                ids.push(m.class_id);
            }
        }
        ids
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
    /// Normal mobs: how many of them live at its spot (0: unknown), and how many hostile mobs
    /// come in each pull there (1: they stand alone).
    pub spawns: u32,
    pub pack: f64,
    /// Hostile mobs around where it is done.
    pub guard: Guard,
    /// An object used right beside the quest's giver or turn-in, no elite around (the campfire
    /// to sit at next to Kaga Wildhoof): it requires no power, as talking to them.
    pub beside_giver: bool,
    /// An item to bring back: it takes a bag slot until the turn-in.
    pub item: bool,
    /// The other places it can be done (other groups of its mobs or objects), each with what
    /// changes there: the mobs, their level and packs, the guards, the wait for respawns. The
    /// planner goes to the best one from where the character is (`Planner::spot`).
    pub alts: Vec<Objective>,
    /// Objectives of the quest done before this one (bits): the item it uses must be got first
    /// (Samuel's Remains looted before they are buried).
    pub after: u8,
    /// Quests whose turn-in hands the item it asks for, when nothing else gives it: it is got
    /// once one of them is turned in, where they are (the chapters of The Green Hills of
    /// Stranglethorn bound by Barnil Stonepot). Empty for the others.
    pub handed_by: Vec<i64>,
    /// Common loot the route's kills gather by themselves (index in `Model::loot`): only what
    /// the bags lack is gathered or bought, `extra` is the time for the whole count.
    pub stock: Option<u16>,
    /// The child quest this objective hands in (see `LinkKind::Child`): done once it is turned in.
    pub child: Option<i64>,
    /// That child quest's name.
    pub child_name: Option<String>,
    /// An item bought: the vendors selling it, the one the place is at first, then the others
    /// from the nearest.
    pub vendors: Vec<Vendor>,
    /// A place to buy it at an auction house: its auctioneers (ID, English name). Empty elsewhere.
    pub auction: Vec<(i64, String)>,
    /// Which of the quest's own objectives it is, from 1 in the game's order; None for what
    /// the quest only needs (an item it uses, a child quest).
    pub slot: Option<u8>,
    /// Seconds the item it gets lasts (a Fresh Zhevra Carcass rots in 30 minutes): the
    /// objectives using it are done before.
    pub lasts: Option<f64>,
}

/// A vendor selling an objective's item: where it stands, and its supply when limited (how many
/// it holds at most, seconds to get one more back).
#[derive(Debug, Clone)]
pub struct Vendor {
    pub id: i64,
    pub name: String,
    pub zone: i64,
    /// The zone's English name.
    pub zone_name: String,
    pub pos: Pos,
    pub stock: Option<(u32, f64)>,
}

impl Objective {
    /// Its places: itself, then its `alts`.
    pub fn places(&self) -> impl Iterator<Item = &Self> {
        std::iter::once(self).chain(&self.alts)
    }

    /// The place closest to `pos` (as the crow flies).
    #[must_use]
    pub fn nearest(&self, pos: &Pos) -> &Self {
        self.places()
            .map(|o| (o, pos.dist(&o.loc.pos)))
            .min_by(|a, b| a.1.total_cmp(&b.1))
            .map_or(self, |(o, _)| o)
    }
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

    /// Other places to look for them than around `first` (world x, y): the centres of up to
    /// `max` groups of spawns away from it and from each other, but not in another part of the
    /// world, densest first, then ordered from the closest to `first`. For the guide, when
    /// nothing is found where it points.
    #[must_use]
    pub fn groups(&self, first: (f64, f64), max: usize) -> Vec<(f64, f64)> {
        /// Spawns this close to a group's centre belong to it (yards).
        const RADIUS: f64 = 100.0;
        /// Places this close to another are the same place (yards): beyond, the mob may be
        /// there and not here.
        const APART: f64 = 150.0;
        /// Places farther are not where to look instead (yards): another zone's copy of the
        /// object (each starting area's Basic Campfire), the mob's spawns elsewhere.
        const FAR: f64 = 2000.0;
        let dist = |a: (f64, f64), b: (f64, f64)| (a.0 - b.0).hypot(a.1 - b.1);
        let mut left: Vec<(f64, f64)> = self
            .points
            .iter()
            .copied()
            .filter(|p| dist(*p, first) > APART && dist(*p, first) <= FAR)
            .collect();
        let mut out = Vec::new();
        while out.len() < max && !left.is_empty() {
            // Large sets: sample candidates to bound the quadratic cost.
            let step = (left.len() / 300).max(1);
            let Some(center) = left
                .iter()
                .step_by(step)
                .map(|p| {
                    let members: Vec<_> = left.iter().filter(|q| dist(*p, **q) <= RADIUS).collect();
                    let n = members.len() as f64;
                    (
                        members.len(),
                        (
                            members.iter().map(|q| q.0).sum::<f64>() / n,
                            members.iter().map(|q| q.1).sum::<f64>() / n,
                        ),
                    )
                })
                .max_by_key(|(n, _)| *n)
                .map(|(_, c)| c)
            else {
                break;
            };
            left.retain(|p| dist(*p, center) > APART);
            if out.iter().all(|o| dist(*o, center) > APART) {
                out.push(center);
            }
        }
        out.sort_by(|a, b| dist(*a, first).total_cmp(&dist(*b, first)));
        out
    }
}

#[cfg(test)]
mod spots_tests {
    use super::Spots;

    #[test]
    fn other_groups_away_from_the_first_not_too_far_closest_first() {
        let points = vec![
            (0.0, 0.0),
            (10.0, 0.0),
            (1000.0, 0.0),
            (1010.0, 5.0),
            (400.0, 0.0),
            (5000.0, 0.0),
        ];
        let spots = Spots {
            continent: 0,
            min: (0.0, 0.0),
            max: (5000.0, 5.0),
            points,
        };
        let groups = spots.groups((0.0, 0.0), 5);
        assert_eq!(groups.len(), 2, "not the spawn far away: {groups:?}");
        assert_eq!(groups[0], (400.0, 0.0));
        assert!(
            (groups[1].0 - 1005.0).abs() < 1e-9,
            "the two spawns together: {groups:?}"
        );
        assert_eq!(spots.groups((0.0, 0.0), 1).len(), 1, "at most max");
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
    /// Timed quest: seconds from taking it to turn it in, else it fails.
    pub time_limit: Option<f64>,
    pub starts: Vec<Loc>,
    pub ends: Vec<Loc>,
    pub objectives: Vec<Objective>,
    pub pre_all: Vec<i64>,
    pub pre_any: Vec<i64>,
    pub exclusive: Vec<i64>,
    /// Quests to hold in the log when taking it: those whose starting item is a bag holding an
    /// item it asks for (The Warsong Reports' Bundle of Reports, opened for the Warsong Report
    /// each scout's update asks for).
    pub pre_held: Vec<i64>,
    pub breadcrumb_for: Option<i64>,
    /// Work to get the item that starts the quest (kills at mob level, object uses).
    pub start_kills: f64,
    pub start_mob_level: i64,
    pub start_uses: f64,
    /// The mob dropping the item that starts the quest (ID, name), for the guide to name it,
    /// and its spawns (the guide's other places to look for it).
    pub start_mob: Option<(i64, String)>,
    pub start_spots: Option<Spots>,
    /// The dungeon whose boss drops the item that starts the quest: accepted during the run.
    pub start_dungeon: Option<usize>,
    /// The item its giver offers it for (a book for the library): its accept is looting the
    /// item, taken and turned in together at the giver.
    pub held: Option<i64>,
    /// An item given on accepting, carried in the bags until the turn-in.
    pub carried: bool,
    /// Class quests: always planned, the optimizer cannot drop them.
    pub mandatory: bool,
    /// Required because its zone is forced (`Params::forced_zones`), or as a prerequisite of
    /// such a quest: `mandatory` too, reported apart from the class quests.
    pub zone_forced: bool,
    /// Explicit player requirement or a prerequisite selected to reach one.
    pub required: bool,
    /// Profession quest: index in `Model::professions` and skill needed to accept it.
    pub skill: Option<(usize, i64)>,
    /// Class quest making the character stronger: index in `Profile::class_powers` (quests of
    /// the same name, or teaching the same spell, share it) and power gained when turned in.
    pub power: Option<(u8, f64)>,
    /// The spell its turn-in teaches (a pet, a totem...).
    pub reward_spell: Option<i64>,
    /// Hostile mobs around its givers and its turn-ins (the hardest of them).
    pub start_guard: Guard,
    pub end_guard: Guard,
    /// Reputation points its turn-in gives the character, by Faction ID (the human's Diplomacy
    /// counted).
    pub reputation: Vec<(i64, f64)>,
    /// The members of the group it is for (race and class), a bit each: the character's 1.
    pub members: u8,
}

pub struct Model {
    pub abilities: crate::abilities::Abilities,
    pub quests: Vec<Quest>,
    pub dungeons: Vec<Dungeon>,
    pub professions: Vec<ProfessionPlan>,
    pub index: FxHashMap<i64, usize>,
    pub start: Loc,
    /// XP of the quests each quest unlocks (whole chain, discounted per step).
    pub unlocks: Vec<f64>,
    /// Highest level among the quests each quest leads to (follow-ups and breadcrumb target,
    /// whole chain; 0 for none).
    pub chain_top: Vec<i64>,
    /// Class trainers and innkeepers the character can use.
    pub trainers: Vec<Loc>,
    pub inns: Vec<Loc>,
    /// Vendors friendly to the character: the bags' loot is sold there.
    pub vendors: Vec<Loc>,
    /// Where the hearthstone takes a new character: its starting place (not an inn to bind).
    pub start_inn: usize,
    /// Class spells taking the character to a capital (mage teleports).
    pub teleports: Vec<Teleport>,
    /// The spirit healers, where a character dying respawns (the nearest).
    pub graveyards: Vec<Loc>,
    /// Level at which the character learns a second hearthstone with a cooldown of its own (the
    /// shaman's Astral Recall, `Params::recall_cooldown`).
    pub recall: Option<i64>,
    /// Quests left out, with the reason (for reports and debugging).
    pub skipped: Vec<(i64, String, String)>,
    pub initial: Initial,
    /// Names in the guide's language.
    pub names: Names,
    /// Names of the factions of the reputation goals (Faction ID -> name in the database).
    pub faction_names: HashMap<i64, String>,
    /// Areas that give XP when discovered.
    pub explore: Vec<ExploreArea>,
    /// Indexes into `explore` by zone.
    pub explore_by_zone: FxHashMap<i64, Vec<usize>>,
    /// Normal mobs anyone can farm: on the way (see `Params::farm_on_way`), and where the
    /// guide sends the character to grind.
    pub farm: super::FarmMobs,
    /// Common loot quests ask for, and what the kills of each zone drop of it.
    pub loot: super::CommonLoot,
    /// Character power data (gear, class spells); `None` without the data (TBC).
    pub power: Option<crate::power::PowerModel>,
    /// The spells worth learning of each other player of a group (`Profile::mates` order):
    /// every rank of those its class counts for power.
    pub mate_spells: Vec<Vec<i64>>,
    /// Items each quest gives (indexes in `power.items`): always, and one of the choices.
    pub rewards: Vec<(Vec<u16>, Vec<u16>)>,
    /// Items each dungeon's bosses drop, with the chance the character gets each.
    pub dungeon_loot: Vec<Vec<(u16, f64)>>,
    /// The paths NPCs who patrol walk (NPC ID -> its points in order): the guide shows places
    /// along them, the planner goes to their usual place.
    pub patrols: HashMap<i64, Vec<Pos>>,
}

/// A class spell taking the character to a city (a mage's "Teleport: Stormwind"): learned at the
/// trainer from `level`, or on taking one of `quests` (indexes in `Model::quests`) when there
/// are some; it lands at `to` (the class trainer there, else the inn).
#[derive(Debug, Clone)]
pub struct Teleport {
    pub name: String,
    pub level: i64,
    pub quests: Vec<usize>,
    pub to: Loc,
}

/// A class teleport a quest teaches, not the trainer (`[[teleport]]` of overrides/travel.toml):
/// taking "Moonglade" teaches the druid Teleport: Moonglade.
#[derive(Debug, Clone, serde::Deserialize)]
pub struct TeleportDef {
    pub name: String,
    /// Class file name, lower case (druid).
    pub class: String,
    /// Name of the quests teaching it.
    pub quest: String,
    pub to: crate::world::ZonePoint,
}

/// Edition-specific quest corrections, applied over the bundled database.
#[derive(Debug, Clone, serde::Deserialize)]
pub struct QuestOverride {
    pub id: i64,
    pub pre_all: Option<Vec<i64>>,
    pub pre_any: Option<Vec<i64>>,
    pub spells: Option<Vec<i64>>,
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
    pub abilities: u64,
    /// Every quest turned in, in the model or not, for a character met in game: prerequisites
    /// the model lacks are then known instead of assumed done past level 1.
    pub completed: Option<HashSet<i64>>,
    /// The log of a character met in game (ID, name). Those the model has are `accepted` too:
    /// the route finishes those it turns in, the others are abandoned at its start
    /// (`Planner::left_at_start`), as are those the model lacks.
    pub log: Vec<(i64, String)>,
    /// Points with the faction of each reputation goal (`Params::reputation_goals`) at the start.
    pub reputation: Vec<f64>,
    /// The other players of a group guide, as `Profile::mates`.
    pub mates: Vec<MateStart>,
    /// In a group: the members each quest is still for (a bit each, the character's 1), not
    /// done yet by them nor closed (another race or class, an exclusive one done). Empty alone.
    pub need: Vec<u8>,
    /// In a group whose hearthstones are bound to different inns: none can be used before
    /// everyone binds it at the same inn.
    pub unbound: bool,
}

/// Where another player of a group guide starts.
#[derive(Debug, Clone, Default)]
pub struct MateStart {
    pub level: i64,
    pub xp: i64,
    pub rested: i64,
    /// Every quest it turned in.
    pub completed: HashSet<i64>,
}

impl Initial {
    /// Whether quest `id`, which the model does not have, was turned in before the route.
    pub(crate) fn done_outside(&self, id: i64, from_level: i64) -> bool {
        self.completed.as_ref().map_or(from_level > 1, |c| c.contains(&id))
    }

    /// The members quest `i` is still for (see `need`): the character only, alone.
    pub fn need(&self, i: usize) -> u8 {
        self.need.get(i).copied().unwrap_or(1)
    }

    /// Whether member `m` (0: the character) turned in quest `id` before the route.
    pub(crate) fn done_by(&self, m: usize, id: i64) -> bool {
        match m {
            0 => self.completed.as_ref().is_some_and(|c| c.contains(&id)),
            m => self.mates.get(m - 1).is_some_and(|mate| mate.completed.contains(&id)),
        }
    }
}

/// An objective the game lets be done only once other objectives of its quest are
/// (`[[order]]` of `overrides/objectives.toml`): the creature an event brings out once a trigger
/// starts it (Big Will comes out to fight once the grate of The Affray is stepped on).
#[derive(Debug, Clone, Default, serde::Deserialize)]
pub struct ObjectiveOrder {
    pub quest: i64,
    /// The objective, by its number in the quest (from 1, in the game's order).
    pub objective: u8,
    /// The objectives done before it, by number.
    pub after: Vec<u8>,
}

/// Per-dungeon settings from `overrides/dungeons.toml`.
#[derive(Debug, Clone, Default, serde::Deserialize)]
pub struct DungeonDef {
    pub area: i64,
    pub name: Option<String>,
    pub entrance: Option<crate::world::ZonePoint>,
    /// The game's level range for the dungeon, when its quests' levels say otherwise: its
    /// monsters are at the middle of it.
    pub levels: Option<[i64; 2]>,
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
    /// The members of a group leveling it (a bit each, the character's 1).
    pub members: u8,
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
            members: 1,
        })
    }

    /// Skill the character has at `level`, following the curve.
    pub(crate) fn skill_at(&self, level: i64) -> i64 {
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
