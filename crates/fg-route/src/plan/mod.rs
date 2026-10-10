//! Route planning.
//!
//! A route is an ordered list of stops: quest stops (accept, complete an objective, turn in) and
//! visits (learn a flight path, bind the hearthstone, run a dungeon, learn a profession rank).
//!
//! Data (what is manipulated): `state` (stops, the character's state, the trace of a replay).
//!
//! Services (calculations, each one need):
//! - `combat`: kill times, the power an objective requires, mob XP, grinding;
//! - `travel` (`Trips`): the fastest way between two places;
//! - `places`: where each stop takes place;
//! - `availability`: what the character can do now;
//! - `growth`: XP, rewards, power, professions;
//! - `worth`: whether a quest is worth its trip.
//!
//! Orchestrators (they call the services in order):
//! - `step`: doing one stop, `on_the_way`: what happens while moving;
//! - `evaluate`: replaying a whole route (its time and score);
//! - `construct`, `regions`: building a first route;
//! - `search` with `moves`, `acceptance`, `alns`: improving it;
//! - `follow`: replaying an imposed route; `report`: explaining a route.

mod acceptance;
mod alns;
mod availability;
mod combat;
mod construct;
mod evaluate;
mod follow;
mod growth;
mod moves;
mod on_the_way;
mod places;
mod regions;
mod report;
mod search;
mod state;
mod step;
mod travel;
mod worth;

pub use alns::ALNS_OPS;
pub use combat::Combat;
pub use follow::{Followed, Want};
pub(crate) use on_the_way::segment_dist;
pub use places::Places;
pub use regions::Group;
pub use state::{Breakdown, Event, Kind, Stop, Timed, fmt_time};

use crate::model::{Model, Profile, Quest};
use crate::params::Params;
use crate::world::World;
use crate::xp;
use availability::{Availability, blocks, followers};
use growth::Growth;
use on_the_way::FarmWalks;
use state::{Planned, State};
use std::cell::RefCell;
use std::collections::HashMap;
use std::rc::Rc;
use step::{CampIndex, camp_index};
use travel::{FlightTable, Trips};
use worth::Worth;

/// What a planner simulates: its own routes, or an imposed route replayed as it is (no grinding
/// before the steps it does under their level, gray quests taken as it says).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Mode {
    Plan,
    Replay,
}

pub struct Planner<'a> {
    pub model: &'a Model,
    pub world: &'a World,
    pub params: &'a Params,
    pub profile: &'a Profile,
    /// Flight times and prices between known flight paths, by known set.
    flights: RefCell<HashMap<u128, Rc<FlightTable>>>,
    /// Mobs farmed on each walk, by level (see `on_the_way::farm_along`).
    farm_walks: FarmWalks,
    mode: Mode,
    /// Quest givers and turn-in NPCs by cell of `camp_radius` (continent, x, y): (quest,
    /// index in its starts or ends, giver?).
    camp: CampIndex,
    /// Quests each quest blocks once taken: those exclusive with it, the breadcrumbs to it.
    blocks: Vec<Vec<u32>>,
    /// The reverse: quests whose taking blocks each quest.
    blocked_by: Vec<Vec<u32>>,
    /// Quests that need each quest done first (the rest of its chain, one step at a time).
    followers: Vec<Vec<u32>>,
    /// Check every incremental evaluation of the search against a full replay (`verified`, or
    /// the FG_VERIFY environment variable).
    verify: bool,
}

impl<'a> Planner<'a> {
    pub fn new(model: &'a Model, world: &'a World, params: &'a Params, profile: &'a Profile) -> Self {
        let blocks = blocks(model);
        let mut blocked_by = vec![Vec::new(); blocks.len()];
        for (x, ys) in blocks.iter().enumerate() {
            for &y in ys {
                blocked_by[y as usize].push(x as u32);
            }
        }
        Self {
            model,
            world,
            params,
            profile,
            flights: RefCell::new(HashMap::new()),
            farm_walks: RefCell::new(HashMap::new()),
            mode: Mode::Plan,
            camp: camp_index(model, params.camp_radius),
            blocks,
            blocked_by,
            followers: followers(model),
            verify: std::env::var_os("FG_VERIFY").is_some(),
        }
    }

    /// The same planner checking each incremental evaluation of its search against a full
    /// replay, panicking when they differ (slow: for tests).
    #[must_use]
    pub fn verified(self) -> Self {
        Self { verify: true, ..self }
    }

    /// The same planner replaying an imposed route.
    #[must_use]
    pub fn replaying(&self) -> Self {
        Self {
            mode: Mode::Replay,
            ..Self::new(self.model, self.world, self.params, self.profile)
        }
    }

    pub(crate) fn quest(&self, i: u32) -> &Quest {
        &self.model.quests[i as usize]
    }

    pub(crate) fn initial_state(&self) -> State {
        let n = self.model.quests.len();
        let level = self.profile.from_level.max(1);
        let init = &self.model.initial;
        let (pos, zone) = init.pos.unwrap_or((self.model.start.pos, self.model.start.zone));
        let mut s = State {
            pos,
            zone,
            time: 0.0,
            level,
            xp: 0,
            known: init.known,
            bind: init.bind.unwrap_or(self.model.start_inn),
            hearth_ready: 0.0,
            trained: level,
            dungeons: 0,
            accepted: vec![false; n],
            turned: vec![false; n],
            dropped: Vec::new(),
            objectives: vec![0; n],
            along: Vec::new(),
            planned: Rc::new(Planned::default()),
            at: 0,
            class_bonus: 0.0,
            powers: 0,
            gear: crate::power::Gear::default(),
            bonus: 0.0,
            gear_bonus: 0.0,
            explored: vec![false; self.model.explore.len()],
            abandon_check: (0, 0),
            log: 0,
            grind_used: 0,
            spent: Breakdown::default(),
        };
        for &i in &init.turned {
            s.turned[i] = true;
            self.growth().gain_power(&mut s, i as u32);
        }
        for &i in &init.accepted {
            if !s.turned[i] {
                s.accepted[i] = true;
                s.log += 1;
                self.open_along(&mut s, i as u32);
            }
        }
        self.growth().refresh_power(&mut s);
        s
    }

    /// Experience rules of the guide's game version.
    /// What the character can do now: prerequisites, exclusive and gray quests, dungeons.
    pub(crate) fn availability(&self) -> Availability<'_> {
        Availability {
            model: self.model,
            params: self.params,
            profile: self.profile,
            replaying: self.mode == Mode::Replay,
            followers: &self.followers,
        }
    }

    /// How the character grows: XP, rewards, power, professions.
    pub(crate) fn growth(&self) -> Growth<'a> {
        Growth {
            model: self.model,
            params: self.params,
            profile: self.profile,
        }
    }

    /// Whether quests are worth their trip.
    pub(crate) fn worth(&self) -> Worth<'_> {
        Worth {
            model: self.model,
            params: self.params,
            combat: self.combat(),
            trips: self.trips(),
            growth: self.growth(),
            availability: self.availability(),
        }
    }

    /// Trips: walking, boats, flights, the hearthstone.
    pub(crate) fn trips(&self) -> Trips<'_> {
        Trips {
            world: self.world,
            params: self.params,
            model: self.model,
            flights: &self.flights,
        }
    }

    /// Where the stops of a route take place.
    pub fn places(&self) -> Places<'a> {
        Places {
            model: self.model,
            world: self.world,
        }
    }

    /// Fights: kill times, the power objectives require, mob XP, grinding.
    pub fn combat(&self) -> Combat<'a> {
        Combat {
            params: self.params,
            dungeons: &self.model.dungeons,
        }
    }

    pub fn rules(&self) -> &'static xp::Rules {
        self.params.edition.rules()
    }
}
