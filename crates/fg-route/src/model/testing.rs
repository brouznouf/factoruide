//! Small scenarios built by hand, for tests: a few flat zones, a few quests, a character. No
//! database: the planner, the editor and the export run on what a test describes, in a few
//! milliseconds. (What the database's queries build from the game data is tested apart, on a small
//! database: `tests/it/loaders.rs`.)
//!
//! ```ignore
//! let p = Scenario::new()
//!     .quest(Q::new(783, "A Threat Within").level(5).giver(14, 40.0, 40.0).kill("Defias", 5, 8, 14, 45.0, 45.0))
//!     .required(783)
//!     .build();
//! ```

#![allow(clippy::return_self_not_must_use, reason = "builders chained by the tests, which use the result")]
#![allow(clippy::must_use_candidate, reason = "builders chained by the tests, which use the result")]

use super::{
    Dungeon, EntityKind, Guard, Initial, Loc, Model, Names, Objective, Profile, Quest, chains, required, sources::QuestRow,
};
use crate::faction::Faction;
use crate::job::{PlanRequest, Prepared, StartState};
use crate::params::Params;
use crate::world::{Pos, World};
use rustc_hash::FxHashMap;
use std::collections::{HashMap, HashSet};

/// The continent every zone of a scenario is on.
const CONTINENT: i64 = 1;
/// Zones are 1000 yards on a side, side by side along x.
const ZONE_SIDE: f64 = 1000.0;

/// A place of a scenario: a zone (AreaTable ID) and percent coordinates on its map.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct At {
    pub zone: i64,
    pub x: f64,
    pub y: f64,
}

/// An objective of a quest of a scenario.
#[derive(Debug, Clone)]
pub struct Obj {
    pub text: String,
    pub at: At,
    pub kills: f64,
    pub mob_level: i64,
    pub uses: f64,
    pub elite: bool,
    pub dungeon: Option<usize>,
    pub handed_by: Vec<i64>,
}

impl Obj {
    /// Kill `count` creatures of `level`.
    pub fn kill(name: &str, level: i64, count: u32, at: At) -> Self {
        Self {
            text: format!("{name} x{count}"),
            at,
            kills: f64::from(count),
            mob_level: level,
            uses: 0.0,
            elite: false,
            dungeon: None,
            handed_by: Vec::new(),
        }
    }

    /// Use an object, or talk to someone: no kill.
    pub fn visit(text: &str, at: At) -> Self {
        Self {
            text: text.to_owned(),
            at,
            kills: 0.0,
            mob_level: 0,
            uses: 1.0,
            elite: false,
            dungeon: None,
            handed_by: Vec::new(),
        }
    }

    /// Elite creatures.
    pub fn elite(mut self) -> Self {
        self.elite = true;
        self
    }

    /// Done inside dungeon `d` (index in the scenario's dungeons).
    pub fn inside(mut self, d: usize) -> Self {
        self.dungeon = Some(d);
        self
    }

    /// Got with the turn-in of these quests.
    pub fn handed_by(mut self, quests: &[i64]) -> Self {
        self.handed_by = quests.to_vec();
        self
    }
}

/// A quest of a scenario (`Q::new(id, name)`, then what it needs).
#[derive(Debug, Clone)]
pub struct Q {
    quest: Quest,
    giver: At,
    turn_in: At,
    objectives: Vec<Obj>,
    excluded: bool,
}

impl Q {
    pub fn new(id: i64, name: &str) -> Self {
        let here = At {
            zone: 1,
            x: 50.0,
            y: 50.0,
        };
        let quest = Quest {
            id,
            name: name.to_owned(),
            level: 1,
            min_level: 1,
            xp: 500,
            xp_observed: false,
            time_limit: None,
            starts: Vec::new(),
            ends: Vec::new(),
            objectives: Vec::new(),
            pre_all: Vec::new(),
            pre_any: Vec::new(),
            pre_need: 1,
            exclusive: Vec::new(),
            pre_held: Vec::new(),
            breadcrumb_for: None,
            start_kills: 0.0,
            start_mob_level: 0,
            start_uses: 0.0,
            start_mob: None,
            start_spots: None,
            start_dungeon: None,
            held: None,
            carried: false,
            mandatory: false,
            zone_forced: false,
            required: false,
            skill: None,
            power: None,
            reward_spell: None,
            start_guard: Guard::default(),
            end_guard: Guard::default(),
            reputation: Vec::new(),
            members: 1,
        };
        Self {
            quest,
            giver: here,
            turn_in: here,
            objectives: Vec::new(),
            excluded: false,
        }
    }

    pub fn level(mut self, level: i64) -> Self {
        self.quest.level = level;
        self.quest.min_level = self.quest.min_level.min(level);
        self
    }

    pub fn min_level(mut self, level: i64) -> Self {
        self.quest.min_level = level;
        self
    }

    pub fn xp(mut self, xp: i64) -> Self {
        self.quest.xp = xp;
        self
    }

    /// Where it is taken (and turned in, unless `turn_in_at` says otherwise).
    pub fn giver(mut self, zone: i64, x: f64, y: f64) -> Self {
        let at = At { zone, x, y };
        if self.turn_in == self.giver {
            self.turn_in = at;
        }
        self.giver = at;
        self
    }

    pub fn turn_in_at(mut self, zone: i64, x: f64, y: f64) -> Self {
        self.turn_in = At { zone, x, y };
        self
    }

    pub fn objective(mut self, objective: Obj) -> Self {
        self.objectives.push(objective);
        self
    }

    /// Kill `count` creatures of `level` at (`zone`, `x`, `y`).
    pub fn kill(self, name: &str, level: i64, count: u32, zone: i64, x: f64, y: f64) -> Self {
        self.objective(Obj::kill(name, level, count, At { zone, x, y }))
    }

    /// Needs all of these quests turned in first.
    pub fn after(mut self, quests: &[i64]) -> Self {
        self.quest.pre_all = quests.to_vec();
        self
    }

    /// Needs one of these quests turned in first.
    pub fn after_any(mut self, quests: &[i64]) -> Self {
        self.quest.pre_any = quests.to_vec();
        self
    }

    /// Taken while these quests are in the log (its parent).
    pub fn while_in_log(mut self, quests: &[i64]) -> Self {
        self.quest.pre_held = quests.to_vec();
        self
    }

    pub fn exclusive_with(mut self, quests: &[i64]) -> Self {
        self.quest.exclusive = quests.to_vec();
        self
    }

    /// Timed: seconds from taking it to turning it in.
    pub fn timed(mut self, seconds: f64) -> Self {
        self.quest.time_limit = Some(seconds);
        self
    }

    /// A class quest, always planned.
    /// Left out of the guide (excluded by the player) though the game has it: its players still
    /// count for what was done.
    pub fn excluded(mut self) -> Self {
        self.excluded = true;
        self
    }

    /// Which players of a group it is for (a bit each, the character's first).
    pub fn members(mut self, mask: u8) -> Self {
        self.quest.members = mask;
        self
    }

    pub fn mandatory(mut self) -> Self {
        self.quest.mandatory = true;
        self
    }

    fn at(at: At, id: i64, name: &str, world: &World) -> Loc {
        let map = world.to_world(at.zone, at.x, at.y);
        Loc {
            pos: map.unwrap_or(Pos {
                continent: CONTINENT,
                x: 0.0,
                y: 0.0,
                zone: at.zone,
                z: None,
            }),
            zone: at.zone,
            kind: EntityKind::Npc,
            id,
            name: name.to_owned(),
        }
    }

    fn finish(mut self, world: &World) -> Quest {
        let id = self.quest.id;
        self.quest.starts = vec![Self::at(self.giver, 100_000 + id, "Giver", world)];
        self.quest.ends = vec![Self::at(self.turn_in, 100_000 + id, "Giver", world)];
        self.quest.objectives = self
            .objectives
            .iter()
            .enumerate()
            .map(|(k, o)| {
                let loc = Loc {
                    kind: if o.kills > 0.0 { EntityKind::Npc } else { EntityKind::Object },
                    id: 200_000 + id * 10 + k as i64,
                    ..Self::at(o.at, 0, &o.text, world)
                };
                Objective {
                    kills: o.kills,
                    mob_level: o.mob_level,
                    uses: o.uses,
                    elite: o.elite,
                    dungeon: o.dungeon,
                    handed_by: o.handed_by.clone(),
                    count: o.kills.max(o.uses),
                    slot: u8::try_from(k + 1).ok(),
                    ..Objective::plain(loc, o.text.clone(), 0.0)
                }
            })
            .collect();
        self.quest
    }
}

/// A character, a few zones and quests, and the settings of the plan.
pub struct Scenario {
    zones: Vec<(i64, &'static str)>,
    quests: Vec<Q>,
    required: Vec<i64>,
    params: Params,
    profile: Profile,
    dungeons: Vec<(&'static str, At)>,
    start: Option<StartState>,
    mates: Vec<StartState>,
}

impl Default for Scenario {
    fn default() -> Self {
        Self::new()
    }
}

impl Scenario {
    /// A Horde warrior from level 1 to 30, in zone 1 ("Valley").
    pub fn new() -> Self {
        Self {
            zones: vec![(1, "Valley")],
            quests: Vec::new(),
            required: Vec::new(),
            params: Params::default(),
            profile: profile(Faction::Horde, 1, 30),
            dungeons: Vec::new(),
            start: None,
            mates: Vec::new(),
        }
    }

    /// A character met in game: what it did, its log, its level (`StartState`).
    pub fn start(mut self, start: StartState) -> Self {
        self.profile.from_level = start.level;
        self.start = Some(start);
        self
    }

    /// The other players of a group guide.
    pub fn mates(mut self, mates: Vec<StartState>) -> Self {
        self.mates = mates;
        self
    }

    pub fn levels(mut self, from: i64, to: i64) -> Self {
        self.profile.from_level = from;
        self.profile.to_level = to;
        self
    }

    pub fn faction(mut self, faction: Faction) -> Self {
        self.profile.faction = faction;
        self
    }

    /// Another zone, next to the others (its first quests are walked to).
    pub fn zone(mut self, id: i64, name: &'static str) -> Self {
        self.zones.push((id, name));
        self
    }

    pub fn quest(mut self, quest: Q) -> Self {
        self.quests.push(quest);
        self
    }

    /// A must-have quest (and what it needs first).
    pub fn required(mut self, id: i64) -> Self {
        self.required.push(id);
        self
    }

    /// A dungeon at (`zone`, `x`, `y`): objectives `inside(d)` are done by running it.
    pub fn dungeon(mut self, name: &'static str, zone: i64, x: f64, y: f64) -> Self {
        self.dungeons.push((name, At { zone, x, y }));
        self
    }

    /// The settings, changed by `change`.
    pub fn params(mut self, change: impl FnOnce(&mut Params)) -> Self {
        change(&mut self.params);
        self
    }

    /// The scenario, planned as the loader would: panics when a must-have quest cannot be done.
    pub fn build(self) -> Prepared {
        self.try_build().unwrap_or_else(|e| panic!("{e:#}"))
    }

    /// The scenario, or why a must-have quest cannot be planned (`required::apply`).
    pub fn try_build(mut self) -> anyhow::Result<Prepared> {
        let zones: Vec<(i64, &str, i64, f64, f64)> = self
            .zones
            .iter()
            .enumerate()
            .map(|(i, &(id, name))| (id, name, CONTINENT, i as f64 * ZONE_SIDE, 0.0))
            .collect();
        let world = World::flat(&zones);
        let start = Q::at(
            At {
                zone: 1,
                x: 50.0,
                y: 50.0,
            },
            1,
            "Start",
            &world,
        );
        let dungeons: Vec<Dungeon> = self
            .dungeons
            .iter()
            .enumerate()
            .map(|(i, &(name, at))| {
                let entrance = Q::at(at, 300_000 + i as i64, name, &world);
                Dungeon {
                    area: 900 + i as i64,
                    name: name.to_owned(),
                    entrance: Loc {
                        kind: EntityKind::Dungeon,
                        ..entrance
                    },
                    min_level: 0,
                    max_level: self.profile.to_level,
                    minutes: 60.0,
                    kills: 120.0,
                    mob_level: 0,
                    content: 0,
                    needed: false,
                }
            })
            .collect();
        let mut scopes: HashMap<i64, u8> = HashMap::new();
        let mut quests: Vec<Quest> = Vec::new();
        for q in std::mem::take(&mut self.quests) {
            let (excluded, built) = (q.excluded, q.finish(&world));
            scopes.insert(built.id, built.members);
            if !excluded {
                quests.push(built);
            }
        }
        self.profile.required_quests.clone_from(&self.required);
        // What the loader does with the quests it built.
        let rows: Vec<QuestRow> = quests
            .iter()
            .map(|q| QuestRow {
                id: q.id,
                name: q.name.clone(),
                level: Some(q.level),
                min_level: Some(q.min_level),
                ..QuestRow::default()
            })
            .collect();
        // The players, the one with the least XP first, as a request puts them.
        let mut request: PlanRequest = serde_json::from_value(serde_json::json!({
            "race": "orc", "class": "warrior",
            "from_level": self.profile.from_level, "to_level": self.profile.to_level,
        }))?;
        (request.start, request.mates) = (self.start.take(), std::mem::take(&mut self.mates));
        let members = request.members(self.params.edition.rules());
        let first = members.first().copied().or(request.start.as_ref());
        // What the character did; in a group, what every player who could do it did.
        self.profile.completed_quests = first.map(|s| s.completed.iter().map(|q| q.0).collect()).unwrap_or_default();
        if members.len() > 1 {
            self.profile
                .completed_quests
                .extend(members.iter().flat_map(|m| m.completed.iter()).map(|q| q.0));
            let done = self.profile.completed_quests.clone();
            for id in done {
                let for_players = scopes.get(&id).copied().unwrap_or(!0);
                if members
                    .iter()
                    .enumerate()
                    .any(|(k, m)| for_players & (1 << k) != 0 && !m.completed.iter().any(|q| q.0 == id))
                {
                    self.profile.completed_quests.remove(&id);
                }
            }
        }
        // Quests outside the levels of the character are not built, but for the must-haves.
        quests.retain(|q| {
            self.required.contains(&q.id)
                || self
                    .profile
                    .within_levels(q.level, q.min_level, self.params.max_quest_above)
        });
        required::apply(&mut quests, &rows, &[], &self.profile, &self.params)?;
        super::settle_quests(&mut quests, &dungeons, &self.params);
        let mut dungeons = dungeons;
        super::settle_dungeons(&mut dungeons, &quests, &self.profile, &self.params);
        let mut model = bare(quests, dungeons, start, &self.params);
        let mut notes = Vec::new();
        crate::job::check_exclusive_choices(&model, &members)?;
        if let Some(start) = first {
            let mates: Vec<StartState> = members.iter().skip(1).copied().cloned().collect();
            crate::job::start_from(&mut model, &world, &mut self.params, start, &mates, &mut notes);
        }
        model.initial.abilities = model.abilities.initial(first, self.profile.from_level);
        Ok(Prepared {
            profile: self.profile,
            params: self.params,
            world,
            model,
            notes,
        })
    }
}

/// A character of `faction` going from level `from` to `to`.
pub fn profile(faction: Faction, from: i64, to: i64) -> Profile {
    Profile {
        name: String::new(),
        race_id: 2,
        class_id: 1,
        class_name: "Warrior".to_owned(),
        faction,
        start_npc: 0,
        from_level: from,
        to_level: to,
        required_class_quests: Vec::new(),
        quest_overrides: Vec::new(),
        required_quests: Vec::new(),
        completed_quests: HashSet::new(),
        class_powers: Vec::new(),
        spell_weights: Vec::new(),
        dungeons: Vec::new(),
        quest_teleports: Vec::new(),
        objective_orders: Vec::new(),
        professions: Vec::new(),
        locale: None,
        mates: Vec::new(),
        player: String::new(),
    }
}

/// A model of these quests and dungeons, with what the planner derives from them (indexes, the
/// value of the chains) and nothing else: no vendors, trainers, inns, gear or farm.
pub fn bare(quests: Vec<Quest>, dungeons: Vec<Dungeon>, start: Loc, params: &Params) -> Model {
    let index: FxHashMap<i64, usize> = quests.iter().enumerate().map(|(i, q)| (q.id, i)).collect();
    let unlocks = chains::chain_values(&quests, &index, &chains::quest_work(&quests, params));
    let chain_top = chains::chain_tops(&quests, &index);
    let (n, d) = (quests.len(), dungeons.len());
    let mut bundles = vec![Vec::new(); n];
    for (f, q) in quests.iter().enumerate().filter(|(_, q)| q.pre_need > 1) {
        for &i in q.pre_any.iter().filter_map(|id| index.get(id)) {
            bundles[i].push(f as u32);
        }
    }
    Model {
        abilities: crate::abilities::Abilities::default(),
        quests,
        dungeons,
        professions: Vec::new(),
        index,
        start: start.clone(),
        unlocks,
        chain_top,
        trainers: Vec::new(),
        inns: vec![start],
        vendors: Vec::new(),
        start_inn: 0,
        teleports: Vec::new(),
        graveyards: Vec::new(),
        recall: None,
        skipped: Vec::new(),
        initial: Initial::default(),
        names: Names::default(),
        faction_names: HashMap::new(),
        explore: Vec::new(),
        explore_by_zone: FxHashMap::default(),
        farm: super::FarmMobs::default(),
        loot: super::CommonLoot::default(),
        power: None,
        mate_spells: Vec::new(),
        rewards: vec![(Vec::new(), Vec::new()); n],
        bundles,
        dungeon_loot: vec![Vec::new(); d],
        patrols: HashMap::new(),
    }
}
