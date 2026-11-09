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

#![allow(
    clippy::return_self_not_must_use,
    reason = "builders chained by the tests, which use the result"
)]
#![allow(
    clippy::must_use_candidate,
    reason = "builders chained by the tests, which use the result"
)]

use super::farm::MobRow;
use super::{
    Dungeon, EntityKind, Guard, Initial, Loc, Mate, Model, Names, Objective, Profile, Quest, Vendor, sources::QuestRow,
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
    /// Objectives (numbered from 0) to do first.
    pub after: Vec<usize>,
    /// Seconds what the objective gets lasts (a carcass that rots).
    pub lasts: Option<f64>,
    /// An item bought: the auction house it can be bought at too (the vendor is `at`; its
    /// supply is limited).
    pub auction: Option<At>,
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
            after: Vec::new(),
            lasts: None,
            auction: None,
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
            after: Vec::new(),
            lasts: None,
            auction: None,
        }
    }

    /// Buy `count` of an item of limited supply at a vendor at `at`, or at an auction house at
    /// `auction`.
    pub fn bought(name: &str, count: u32, at: At, auction: At) -> Self {
        Self {
            auction: Some(auction),
            ..Self::visit(&format!("Buy {name}"), at)
        }
        .counting(count)
    }

    fn counting(mut self, count: u32) -> Self {
        self.uses = f64::from(count);
        self
    }

    /// Elite creatures.
    pub fn elite(mut self) -> Self {
        self.elite = true;
        self
    }

    /// Done inside dungeon `d` (index in the scenario's dungeons): a stop there, its mobs those of
    /// the run.
    pub fn inside(mut self, d: usize) -> Self {
        self.dungeon = Some(d);
        self
    }

    /// What it gets is gone after `seconds`.
    pub fn lasts(mut self, seconds: f64) -> Self {
        self.lasts = Some(seconds);
        self
    }

    /// Done once objectives `k` (numbered from 0) are.
    pub fn after(mut self, k: &[usize]) -> Self {
        self.after = k.to_vec();
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
    ender: Option<i64>,
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
            late_unlock: false,
            mandatory: false,
            zone_forced: false,
            required: false,
            completion_window: super::CompletionWindow::default(),
            implicit_window: false,
            priority_deadline: None,
            skill: None,
            power: None,
            reward_spell: None,
            reward_items: Vec::new(),
            start_guard: Guard::default(),
            end_guard: Guard::default(),
            reputation: Vec::new(),
            members: 0xFF,
        };
        Self {
            quest,
            giver: here,
            turn_in: here,
            ender: None,
            objectives: Vec::new(),
            excluded: false,
        }
    }

    /// Its XP was received in game: no level penalty applies to it.
    pub fn observed(mut self) -> Self {
        self.quest.xp_observed = true;
        self
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

    /// Its giver stands among `count` hostile mobs of level `level`.
    pub fn guarded(mut self, level: i64, count: u32) -> Self {
        self.quest.start_guard = Guard {
            level,
            count,
            elite: false,
        };
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

    /// Turned in to this NPC (else one of its own).
    pub fn ended_by(mut self, npc: i64) -> Self {
        self.ender = Some(npc);
        self
    }

    /// Its turn-in teaches this spell.
    pub fn teaches(mut self, spell: i64) -> Self {
        self.quest.reward_spell = Some(spell);
        self
    }

    /// Its turn-in gives these items.
    pub fn rewards(mut self, items: &[i64]) -> Self {
        self.quest.reward_items = items.to_vec();
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

    /// A breadcrumb to quest `id`: closed once that one is taken.
    pub fn breadcrumb_for(mut self, id: i64) -> Self {
        self.quest.breadcrumb_for = Some(id);
        self
    }

    /// Timed: seconds from taking it to turning it in.
    /// Reputation with a faction (Faction ID) the turn-in gives.
    pub fn reputation(mut self, faction: i64, amount: f64) -> Self {
        self.quest.reputation.push((faction, amount));
        self
    }

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

    pub fn zone_forced(mut self) -> Self {
        self.quest.zone_forced = true;
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
            inside: None,
            floor: None,
        }
    }

    fn finish(mut self, world: &World) -> Quest {
        let id = self.quest.id;
        self.quest.starts = vec![Self::at(self.giver, 100_000 + id, "Giver", world)];
        let ender = self.ender.unwrap_or(100_000 + id);
        self.quest.ends = vec![Self::at(self.turn_in, ender, "Giver", world)];
        self.quest.objectives = self
            .objectives
            .iter()
            .enumerate()
            .map(|(k, o)| {
                let loc_id = 200_000 + id * 10 + k as i64;
                let loc = Loc {
                    kind: if o.kills > 0.0 {
                        EntityKind::Npc
                    } else if o.auction.is_some() {
                        EntityKind::Item
                    } else {
                        EntityKind::Object
                    },
                    id: loc_id,
                    ..Self::at(o.at, 0, &o.text, world)
                };
                let (vendors, alts, auction, item) = match o.auction {
                    Some(auction) => {
                        let vendor = Vendor {
                            id: loc_id,
                            name: "Vendor".to_owned(),
                            zone: loc.zone,
                            zone_name: String::new(),
                            pos: loc.pos,
                            stock: Some((5, 3600.0)),
                        };
                        let house = Self::at(auction, loc_id + 1, "Auction House", world);
                        let alt = Objective {
                            vendors: vec![vendor.clone()],
                            auction: vec![(house.id, house.name.clone())],
                            ..Objective::plain(house, o.text.clone(), 0.0)
                        };
                        (vec![vendor], vec![alt], Vec::new(), true)
                    }
                    None => (Vec::new(), Vec::new(), Vec::new(), false),
                };
                Objective {
                    vendors,
                    alts,
                    auction,
                    item,
                    kills: if o.dungeon.is_some() { 0.0 } else { o.kills },
                    mob_level: o.mob_level,
                    uses: if o.dungeon.is_some() { 0.0 } else { o.uses },
                    elite: o.elite,
                    handed_by: o.handed_by.clone(),
                    lasts: o.lasts,
                    after: o.after.iter().fold(0u8, |mask, k| mask | 1u8 << k),
                    count: o.kills.max(o.uses),
                    slot: u8::try_from(k + 1).ok(),
                    // The creatures to target: one of the objective's name.
                    mobs: if o.kills > 0.0 {
                        vec![(loc_id, o.text.clone())]
                    } else {
                        Vec::new()
                    },
                    ..Objective::plain(
                        Loc {
                            inside: o.dungeon.map(|d| u8::try_from(d).expect("a scenario's dungeon")),
                            ..loc
                        },
                        o.text.clone(),
                        0.0,
                    )
                }
            })
            .collect();
        super::objectives::mark_beside_givers(&self.quest.starts, &self.quest.ends, &mut self.quest.objectives);
        self.quest
    }
}

/// A character, a few zones and quests, and the settings of the plan.
#[derive(Clone)]
pub struct Scenario {
    zones: Vec<(i64, &'static str)>,
    quests: Vec<Q>,
    required: Vec<i64>,
    params: Params,
    profile: Profile,
    dungeons: Vec<(&'static str, At)>,
    start: Option<StartState>,
    mates: Vec<StartState>,
    /// Flight masters (id, name, place), trainers, vendors and inns of the character's side.
    flight_masters: Vec<(i64, &'static str, At)>,
    known_flights: Vec<i64>,
    trainers: Vec<At>,
    vendors: Vec<At>,
    inns: Vec<(&'static str, At)>,
    /// Normal mobs anyone can farm: where, their level and how many.
    mobs: Vec<(At, i64, u32)>,
    teleports: Vec<(&'static str, i64, At)>,
    graveyards: Vec<At>,
    /// Boats between two places (name, ends, seconds), both ways.
    boats: Vec<(&'static str, At, At, f64)>,
    /// The class requirements of the guide (`Scenario::requirement`).
    requirements: Vec<super::ClassRequirement>,
    /// The class's spells and power as the database has them (`Scenario::of_class`).
    class: Option<(crate::abilities::Abilities, Option<crate::power::PowerModel>)>,
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
            flight_masters: Vec::new(),
            known_flights: Vec::new(),
            trainers: Vec::new(),
            vendors: Vec::new(),
            inns: Vec::new(),
            mobs: Vec::new(),
            teleports: Vec::new(),
            graveyards: Vec::new(),
            boats: Vec::new(),
            class: None,
            requirements: Vec::new(),
        }
    }

    /// A character of a request (its race, class, levels and settings) in a scenario with the
    /// class's spells and power read from the database, and nothing else of it: for the tests of
    /// what a character knows and learns.
    pub fn of_class(
        conn: &rusqlite::Connection,
        overrides: &crate::job::Overrides,
        req: &PlanRequest,
    ) -> anyhow::Result<Self> {
        let (profile, params) = crate::job::character(conn, overrides, req)?;
        let abilities = crate::abilities::Abilities::load(conn, &profile)?;
        let power = crate::power::PowerModel::load(
            conn,
            profile.class_id,
            profile.race_id,
            profile.to_level.max(60),
            &profile.spell_weights,
            &params,
            &abilities,
        )?;
        let mut scenario = Self::new();
        (scenario.profile, scenario.params) = (profile, params);
        (scenario.start, scenario.mates) = (req.start.clone(), req.mates.clone());
        scenario.class = Some((abilities, power));
        Ok(scenario)
    }

    /// `count` normal mobs of `level` at a place: the character kills them on the way, or grinds
    /// there (a camp holds eight of a level).
    pub fn mobs(mut self, zone: i64, x: f64, y: f64, level: i64, count: u32) -> Self {
        self.mobs.push((At { zone, x, y }, level, count));
        self
    }

    /// A class spell taking the character to a place, learned at the trainer from `level`.
    pub fn teleport(mut self, name: &'static str, level: i64, zone: i64, x: f64, y: f64) -> Self {
        self.teleports.push((name, level, At { zone, x, y }));
        self
    }

    /// A boat between two places, both ways.
    pub fn boat(mut self, name: &'static str, from: At, to: At, seconds: f64) -> Self {
        self.boats.push((name, from, to, seconds));
        self
    }

    /// A flight master (the character knows the flight paths given to `knows_flight`).
    pub fn flight_master(mut self, id: i64, name: &'static str, zone: i64, x: f64, y: f64) -> Self {
        self.flight_masters.push((id, name, At { zone, x, y }));
        self
    }

    /// The character knows this flight path from the start.
    pub fn knows_flight(mut self, id: i64) -> Self {
        self.known_flights.push(id);
        self
    }

    /// A spirit healer, where a character that dies respawns.
    pub fn graveyard(mut self, zone: i64, x: f64, y: f64) -> Self {
        self.graveyards.push(At { zone, x, y });
        self
    }

    /// A class trainer: the character trains there as it levels.
    pub fn trainer(mut self, zone: i64, x: f64, y: f64) -> Self {
        self.trainers.push(At { zone, x, y });
        self
    }

    /// A vendor, where the bags' loot is sold.
    pub fn vendor(mut self, zone: i64, x: f64, y: f64) -> Self {
        self.vendors.push(At { zone, x, y });
        self
    }

    /// An inn (the hearthstone binds there). The first one is where the character starts bound.
    pub fn inn(mut self, name: &'static str, zone: i64, x: f64, y: f64) -> Self {
        self.inns.push((name, At { zone, x, y }));
        self
    }

    /// The same character before it was met in game: nothing known but its class's spells.
    pub fn without_start(mut self) -> Self {
        self.start = None;
        self
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

    /// An objective guide of these objectives (`PlanRequest::objectives`), at the character's
    /// level (`levels`).
    pub fn objectives(mut self, objectives: &[crate::model::GuideObjective]) -> Self {
        self.profile.objectives = objectives.to_vec();
        self.profile.to_level = self.profile.from_level;
        self.params.for_objective_guide(objectives);
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

    /// A class requirement of the guide: the route does one of its quests (the last of each version).
    pub fn requirement(mut self, name: &str, quests: &[i64]) -> Self {
        self.requirements.push(super::ClassRequirement {
            class_id: 1,
            name: name.to_owned(),
            spell: None,
            max_level: None,
            quests: quests.to_vec(),
            quest_names: Vec::new(),
            selected: true,
        });
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
        let flights: Vec<(i64, &str, i64, f64, f64)> = self
            .flight_masters
            .iter()
            .map(|&(id, name, at)| (id, name, at.zone, at.x, at.y))
            .collect();
        let mut world = World::flat(&zones, &flights, self.params.flight_speed);
        let boats: Vec<crate::world::LinkDef> = self
            .boats
            .iter()
            .map(|&(name, from, to, seconds)| {
                serde_json::from_value(serde_json::json!({
                    "name": name, "seconds": seconds,
                    "from": {"zone": from.zone, "x": from.x, "y": from.y},
                    "to": {"zone": to.zone, "x": to.x, "y": to.y},
                }))
                .unwrap_or_else(|e| panic!("{e}"))
            })
            .collect();
        world.add_links(
            &boats,
            self.profile.faction,
            self.params.run_speed,
            self.params.link_wait,
            crate::world::WorldTables::shared(),
        );
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
        self.profile.class_requirements = std::mem::take(&mut self.requirements);
        self.profile.required_quests = self
            .required
            .iter()
            .map(|&id| super::QuestRequirement {
                id,
                window: super::CompletionWindow::default(),
            })
            .collect();
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
        // The players: the character is the first, the others its mates.
        if let Some(first) = first {
            self.profile.player.clone_from(&first.name);
            self.profile.from_level = first.level;
            if first.race != 0 {
                self.profile.race_id = first.race;
            }
            self.profile.class_id = class_id(&first.class);
            if !first.class.is_empty() {
                let lower = first.class.to_lowercase();
                self.profile.class_name = lower[..1].to_uppercase() + &lower[1..];
            }
        }
        self.profile.mates = members
            .iter()
            .skip(1)
            .map(|m| Mate {
                class_name: m.class.clone(),
                name: m.name.clone(),
                race_id: m.race,
                class_id: class_id(&m.class),
                spell_weights: Vec::new(),
                professions: Vec::new(),
            })
            .collect();
        self.params.group_size = members.len().max(1) as i64;
        // What the character did; in a group, what every player who could do it did.
        self.profile.completed_quests = first
            .map(|s| s.completed.iter().map(|q| q.0).collect())
            .unwrap_or_default();
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
        // Quests outside the levels of the character are not built, but for the must-haves (and in
        // an objective guide, `QuestBuilder::any_level`).
        quests.retain(|q| {
            self.required.contains(&q.id)
                || self.profile.objective_guide()
                || self
                    .profile
                    .within_levels(q.level, q.min_level, self.params.max_quest_above)
        });
        // The same settling as a model read from a database, nothing known of the characters.
        let mut dungeons = dungeons;
        let mut skipped = Vec::new();
        let mut known = super::derive::Knowledge {
            assumed: true,
            ..super::derive::Knowledge::default()
        };
        super::derive::settle(
            &mut quests,
            &mut skipped,
            &mut dungeons,
            &rows,
            &mut known,
            &HashMap::new(),
            &self.profile,
            &self.params,
        )?;
        let mut model = bare(quests, dungeons, start, &self.params);
        model.skipped = skipped;
        if let Some((abilities, power)) = self.class.take() {
            (model.abilities, model.power) = (abilities, power);
        }
        let (tags, by_tag) = super::FarmMobs::quest_tags(&model.quests);
        let mut rows = Vec::new();
        for &(at, level, count) in &self.mobs {
            for k in 0..count {
                // Twelve yards apart (a percent is ten): one mob each, a few to a camp's cell.
                rows.push(MobRow {
                    zone: at.zone,
                    x: at.x + f64::from(k % 4) * 1.2,
                    y: at.y + f64::from(k / 4) * 1.2,
                    min_level: level,
                    max_level: level,
                    npc: 700_000 + i64::from(k),
                    z: None,
                });
            }
        }
        model.farm = super::FarmMobs::from_rows(&world, rows, &tags, by_tag);
        model.graveyards = self
            .graveyards
            .iter()
            .enumerate()
            .map(|(i, &at)| Q::at(at, 900_000 + i as i64, "Spirit Healer", &world))
            .collect();
        model.teleports = self
            .teleports
            .iter()
            .enumerate()
            .map(|(i, &(name, level, at))| super::Teleport {
                name: name.to_owned(),
                level,
                ability: 0,
                to: Q::at(at, 800_000 + i as i64, name, &world),
            })
            .collect();
        let place = |at: At, id: i64, name: &str| Q::at(at, id, name, &world);
        model.trainers = self
            .trainers
            .iter()
            .enumerate()
            .map(|(i, &at)| place(at, 400_000 + i as i64, "Trainer"))
            .collect();
        model.vendors = self
            .vendors
            .iter()
            .enumerate()
            .map(|(i, &at)| place(at, 500_000 + i as i64, "Vendor"))
            .collect();
        if !self.inns.is_empty() {
            model.inns = self
                .inns
                .iter()
                .enumerate()
                .map(|(i, &(name, at))| place(at, 600_000 + i as i64, name))
                .collect();
            model.start_inn = 0;
        }
        model.initial.known = self
            .known_flights
            .iter()
            .filter_map(|id| world.taxi_nodes.iter().position(|n| n.id == *id))
            .fold(0u128, |bits, i| bits | 1u128 << i);
        crate::job::check_dungeon_levels(&model, &self.params, &self.profile)?;
        let mut notes = Vec::new();
        crate::job::check_exclusive_choices(&model, &members)?;
        let mates: Vec<StartState> = members.iter().skip(1).copied().cloned().collect();
        if let Some(start) = first {
            crate::job::start_from(&mut model, &world, &mut self.params, start, &mates, &mut notes);
        }
        crate::job::settle_objective_guide(&mut model, &world, &self.params, &self.profile)?;
        model.initial.abilities = model.abilities.initial(first, self.profile.from_level);
        Ok(Prepared {
            profile: self.profile,
            params: self.params,
            world,
            model,
            notes,
            diagnostics: crate::diagnostics::Diagnostics::default(),
        })
    }
}

impl Scenario {
    /// The scenario planned as a job is: with `interrupt`, `walks` and a `progress` to listen to.
    pub fn job(
        self,
        progress: crate::job::Watch<'_>,
        interrupt: &crate::job::Interrupt,
        walks: Option<&std::sync::Arc<crate::terrain::Walks>>,
    ) -> Result<crate::job::PlanOutcome, crate::job::JobError> {
        let (from, to) = (self.profile.from_level, self.profile.to_level);
        let request: PlanRequest = serde_json::from_value(serde_json::json!({
            "race": "orc", "class": "warrior", "from_level": from, "to_level": to,
        }))
        .unwrap_or_else(|e| panic!("{e}"));
        crate::job::plan_prepared(self.build(), &request, progress, interrupt, walks).map(|(outcome, _)| outcome)
    }

    /// The starts a new guide would offer for the scenario (see `job::preview_starts`).
    pub fn starts(self) -> crate::job::StartPreview {
        crate::job::preview_starts_of(self.build(), &|_| {}, &crate::job::Interrupt::default(), None)
            .unwrap_or_else(|e| panic!("{e:#}"))
    }

    /// The guide of the scenario (its first routes, no optimization) and the editor on it.
    pub fn guide(self) -> (crate::job::Editor, crate::job::PlanOutcome) {
        let (from, to) = (self.profile.from_level, self.profile.to_level);
        let request: PlanRequest = serde_json::from_value(serde_json::json!({
            "race": "orc", "class": "warrior", "from_level": from, "to_level": to,
        }))
        .unwrap_or_else(|e| panic!("{e}"));
        let interrupt = crate::job::Interrupt::default();
        interrupt.stop();
        let (outcome, prepared) = crate::job::plan_prepared(self.build(), &request, &|_| {}, &interrupt, None)
            .unwrap_or_else(|e| panic!("{e:#}"));
        (crate::job::Editor::from_prepared(prepared), outcome)
    }
}

/// The game's ID of a class (its file name, any case).
fn class_id(class: &str) -> i64 {
    const CLASSES: [&str; 11] = [
        "WARRIOR",
        "PALADIN",
        "HUNTER",
        "ROGUE",
        "PRIEST",
        "DEATHKNIGHT",
        "SHAMAN",
        "MAGE",
        "WARLOCK",
        "MONK",
        "DRUID",
    ];
    CLASSES
        .iter()
        .position(|c| c.eq_ignore_ascii_case(class))
        .map_or(1, |i| i as i64 + 1)
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
        class_requirements: Vec::new(),
        spell_weights: Vec::new(),
        abilities: Vec::new(),
        dungeons: Vec::new(),
        quest_teleports: Vec::new(),
        objective_orders: Vec::new(),
        professions: Vec::new(),
        locale: None,
        mates: Vec::new(),
        player: String::new(),
        objectives: Vec::new(),
    }
}

/// A model of these quests and dungeons, with what the planner derives from them (indexes, the
/// value of the chains) and nothing else: no vendors, trainers, inns, gear or farm.
pub fn bare(quests: Vec<Quest>, dungeons: Vec<Dungeon>, start: Loc, params: &Params) -> Model {
    let super::derive::Indexes {
        index,
        unlocks,
        chain_top,
        bundles,
    } = super::derive::indexes(&quests, params);
    let (n, d) = (quests.len(), dungeons.len());
    Model {
        known_class_powers: 0,
        known_class_prerequisites: HashSet::new(),
        abilities: crate::abilities::Abilities::default(),
        quests,
        dungeons,
        professions: Vec::new(),
        index,
        start: start.clone(),
        unlocks,
        chain_top,
        trainers: Vec::new(),
        profession_trainers: Vec::new(),
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
        above_level: Vec::new(),
        levels_ignored: false,
    }
}
