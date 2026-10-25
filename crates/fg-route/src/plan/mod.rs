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
//! - `places`: where each stop takes place, `spot`: the place of an objective done in several,
//!   `camp`: where the character grinds;
//! - `availability`: what the character can do now;
//! - `growth`: XP, rewards, power, professions;
//! - `worth`: whether a quest is worth its trip.
//!
//! Orchestrators (they call the services in order):
//! - `step`: doing one stop, `on_the_way`: what happens while moving;
//! - `evaluate`: replaying a whole route (its time and score);
//! - `construct`, `regions`: building a first route, `hearth`: where the hearthstone is bound;
//! - `search` with `moves`, `acceptance`, `alns`: improving it;
//! - `follow`: replaying an imposed route; `report`: explaining a route.

mod acceptance;
mod alns;
mod availability;
mod camp;
mod combat;
mod construct;
mod edit;
mod evaluate;
mod follow;
mod growth;
mod hearth;
mod moves;
mod on_the_way;
mod places;
mod regions;
mod report;
mod search;
mod share;
mod spot;
mod state;
mod step;
mod together;
mod travel;
mod worth;

#[cfg(test)]
pub(crate) mod tests;

pub use alns::ALNS_OPS;
pub use combat::Combat;
pub use edit::{Edit, Edited, EditedStep, Farmed, Outcome};
pub use evaluate::End;
pub use follow::{Followed, Want};
pub(crate) use on_the_way::segment_dist;
pub use places::Places;
pub use regions::Group;
pub use search::Effort;
pub use share::{Exchange, Member};
pub use state::{Breakdown, Event, Kind, Stop, Timed, fmt_time};

use crate::model::{Model, Profile, Quest};
use crate::params::Params;
use crate::world::{Pos, World};
use crate::xp;
use availability::{Availability, blocks, followers};
use camp::NearCamps;
use growth::Growth;
use on_the_way::FarmWalks;
pub(crate) use state::State;
use state::{Flags, Landed, Planned};
use std::cell::{Cell, RefCell};
use std::rc::Rc;
use step::{CampIndex, VendorIndex, camp_index, vendor_index};
use travel::{FlightTables, Legs, Trips};
use worth::Worth;

/// What a planner simulates: its own routes, or an imposed route replayed as it is (no grinding
/// before the steps it does under their level, gray quests taken as it says).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Mode {
    Plan,
    Replay,
}

/// A zone change of the greedy construction: (level, time, from, to).
pub(crate) type ZoneChange = (i64, f64, i64, i64);

pub struct Planner<'a> {
    pub model: &'a Model,
    pub world: &'a World,
    pub params: &'a Params,
    pub profile: &'a Profile,
    /// Flight times and prices between known flight paths, by known set.
    flights: FlightTables,
    /// Trips already computed (see `travel::Legs`).
    legs: Legs,
    /// Mobs farmed on each walk, by level (see `on_the_way::farm_along`).
    farm_walks: FarmWalks,
    /// The camps nearest to each place grinding starts at (see `camp::camp`).
    near_camps: NearCamps,
    /// The next walk kills every normal mob met, beyond `farm_cap` (a step of an edited route).
    farm_all: Cell<bool>,
    /// Walks go into the zones too strong for the character (a step of an edited route that cannot
    /// be made otherwise: the editor allows it, with a warning).
    open_walks: Cell<bool>,
    /// Where `while_here` looked for quests at hand, recorded while `run` replays a route (see
    /// `Run::heres`).
    heres: RefCell<Option<Vec<Pos>>>,
    /// The zone changes of the last greedy construction: (level, time, from, to).
    zone_changes: RefCell<Vec<ZoneChange>>,
    mode: Mode,
    /// Quest givers and turn-in NPCs by cell of `camp_radius` (continent, x, y): (quest,
    /// index in its starts or ends, giver?).
    camp: CampIndex,
    /// Vendors by cell of `sell_radius` (continent, x, y).
    vendors: VendorIndex,
    /// Quests each quest blocks once taken: those exclusive with it, the breadcrumbs to it.
    blocks: Vec<Vec<u32>>,
    /// The reverse: quests whose taking blocks each quest.
    blocked_by: Vec<Vec<u32>>,
    /// Quests that need each quest done first (the rest of its chain, one step at a time).
    followers: Vec<Vec<u32>>,
    /// The objectives inside each dungeon: (quest, objective).
    in_dungeon: Vec<Vec<(u32, u8)>>,
    /// Where each stop takes place seen from the start (`Places::pos` from the initial state),
    /// by quest: accept, turn-in, then its objectives. The insertions ask it all the time.
    from_start: Vec<Vec<Pos>>,
    /// Check every incremental evaluation of the search against a full replay (`verified`, or
    /// the FG_VERIFY environment variable).
    verify: bool,
    /// Tells the construction to give up (`abortable`).
    abort: Option<&'a dyn Fn() -> bool>,
    /// The search's seat at the meetings of the final (`sharing`).
    share: Option<&'a Member<'a>>,
    /// Nothing is done that the route does not say (the guide editor): no flight, boat or
    /// teleport taken by a trip, no training, flight path learned in passing, quest given up or
    /// grinding by itself. Each of them is a step of the route (`Edit`), so that each step does
    /// one thing and can be moved or removed apart.
    explicit: bool,
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
        let mut in_dungeon = vec![Vec::new(); model.dungeons.len()];
        for (i, q) in model.quests.iter().enumerate() {
            for (k, o) in q.objectives.iter().enumerate() {
                if let Some(d) = o.dungeon {
                    in_dungeon[d].push((i as u32, k as u8));
                }
            }
        }
        let mut planner = Self {
            model,
            world,
            params,
            profile,
            flights: FlightTables::default(),
            legs: Legs::default(),
            farm_walks: FarmWalks::default(),
            near_camps: NearCamps::default(),
            farm_all: Cell::new(false),
            open_walks: Cell::new(false),
            heres: RefCell::new(None),
            zone_changes: RefCell::new(Vec::new()),
            mode: Mode::Plan,
            camp: camp_index(model, params.camp_radius),
            vendors: vendor_index(model, params.sell_radius),
            blocks,
            blocked_by,
            followers: followers(model),
            in_dungeon,
            from_start: Vec::new(),
            verify: std::env::var_os("FG_VERIFY").is_some(),
            abort: None,
            share: None,
            explicit: false,
        };
        let s0 = planner.initial_state();
        let places = planner.places();
        planner.from_start = (0..model.quests.len() as u32)
            .map(|i| {
                let kinds = [Kind::Accept, Kind::TurnIn]
                    .into_iter()
                    .chain((0..model.quests[i as usize].objectives.len()).map(|k| Kind::Objective(k as u8)));
                kinds.map(|kind| places.pos(&s0, Stop::quest(i, kind))).collect()
            })
            .collect();
        planner
    }

    /// `Places::pos` from the initial state, without computing it again for a quest stop.
    pub(crate) fn pos_from_start(&self, s0: &State, stop: Stop) -> Pos {
        let at = |k: usize| self.from_start[stop.index as usize][k];
        match stop.kind {
            Kind::Accept => at(0),
            Kind::TurnIn => at(1),
            Kind::Objective(k) => at(2 + k as usize),
            _ => self.places().pos(s0, stop),
        }
    }

    /// The same planner whose construction gives up as soon as `abort` says so (the user stopped
    /// or cancelled the generation): it then returns the route built so far, unfinished. A replay
    /// of the guide editor gives up too (`edit_observed`: a newer one is asked for).
    #[must_use]
    pub fn abortable(self, abort: &'a dyn Fn() -> bool) -> Self {
        Self {
            abort: Some(abort),
            ..self
        }
    }

    /// The same planner whose search meets the other searches of the final at `member`'s
    /// exchange every `share_every` tries (see `share`).
    #[must_use]
    pub fn sharing(self, member: Option<&'a Member<'a>>) -> Self {
        Self { share: member, ..self }
    }

    /// Whether the construction must give up (see `abortable`).
    pub fn aborted(&self) -> bool {
        self.abort.is_some_and(|abort| abort())
    }

    /// The zone changes of the last greedy construction: (level, time, from, to).
    pub(crate) fn zone_changes(&self) -> Vec<ZoneChange> {
        self.zone_changes.borrow().clone()
    }

    /// The same planner doing only what the route says (`explicit`).
    #[must_use]
    pub fn explicit(self) -> Self {
        Self { explicit: true, ..self }
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
            xp: init.xp.min(self.rules().to_next_level(level) - 1).max(0),
            rested: init.rested,
            mates: std::array::from_fn(|k| {
                init.mates.get(k).map_or_else(state::MateXp::default, |m| {
                    let level = m.level.max(1);
                    state::MateXp {
                        level,
                        xp: m.xp.min(self.rules().to_next_level(level) - 1).max(0),
                        rested: m.rested.max(0),
                    }
                })
            }),
            order: std::array::from_fn(|k| k as u8),
            synced: true,
            stock: vec![0.0; self.model.loot.items.len()],
            bags: 0.0,
            held: 0,
            known: init.known,
            bind: init.bind.unwrap_or(self.model.start_inn),
            hearth_ready: if init.unbound { f64::INFINITY } else { 0.0 },
            recall_ready: if init.unbound { f64::INFINITY } else { 0.0 },
            trained: init.trained.unwrap_or(level).min(level),
            dungeons: 0,
            accepted: Flags::new(n),
            turned: Flags::new(n),
            dropped: Vec::new(),
            objectives: vec![0; n],
            timers: Vec::new(),
            started: Vec::new(),
            together: state::Together::default(),
            next: None,
            spot: None,
            landed: Landed::No,
            planned: Rc::new(Planned::default()),
            at: 0,
            class_bonus: 0.0,
            powers: 0,
            gear: crate::power::Gear::default(),
            bonus: 0.0,
            gear_bonus: 0.0,
            gear_quality: 0.0,
            explored: Flags::new(self.model.explore.len()),
            abandon_check: (0, 0),
            log: 0,
            grind_used: 0,
            farm_used: 0,
            reputation: (0..self.params.reputation_goals.len())
                .map(|g| init.reputation.get(g).copied().unwrap_or(0.0) as f32)
                .collect(),
            forced_left: 0,
            required_left: 0,
            spent: Breakdown::default(),
        };
        for &i in &init.turned {
            s.turned.set(i, true);
            self.growth().gain_power(&mut s, i as u32);
        }
        for &i in &init.accepted {
            if !s.turned[i] {
                s.accepted.set(i, true);
                s.log += 1;
                s.held += u16::from(self.model.quests[i].carried);
                for &(_, k, _) in init.progress.iter().filter(|p| p.0 == i && p.2 >= 1.0) {
                    self.hold(&mut s, i, k);
                }
            }
        }
        // Objectives started: their share so far is done when the route gets to them.
        for &(q, k, share) in &init.progress {
            if s.accepted[q] && share > 0.0 && share < 1.0 {
                s.started.push((q as u32, k, share));
            }
        }
        if let Some(power) = &self.model.power {
            for &id in &init.gear {
                if let Some(item) = power.item(id) {
                    power.equip(&mut s.gear, item, 1.0, level);
                }
            }
        }
        self.growth().refresh_power(&mut s);
        s.synced = self.growth().spread(&s) <= self.params.group_sync_window;
        s.required_left = self
            .model
            .quests
            .iter()
            .enumerate()
            .filter(|(i, q)| q.required && !s.rewarded(*i))
            .count() as u32;
        s.forced_left = (self.model.quests.iter().enumerate())
            .filter(|(i, q)| q.zone_forced && !s.turned[*i])
            .count() as u32;
        s
    }

    /// The initial state of a route of plan `planned`: the quests of the character's log it has
    /// no stop of are abandoned at once (`left_at_start`), freeing their place in the log. Not
    /// the route's choice: no penalty (`Params::abandon_weight`), no event (the guide's first
    /// steps, `export`).
    pub(crate) fn start_state(&self, planned: Rc<Planned>) -> State {
        let mut s = self.initial_state();
        for i in self.left_at_start(&planned) {
            self.abandon(&mut s, i as u32, &mut |_, _| {});
        }
        s.planned = planned;
        s
    }

    /// The quests of the character's log a route of plan `planned` has nothing to do with.
    pub(crate) fn left_at_start<'p>(&'p self, planned: &'p Planned) -> impl Iterator<Item = usize> + 'p {
        (self.model.initial.accepted.iter().copied()).filter(|&i| planned.left(i))
    }

    /// Whether the guide is over: the target level reached by everyone, the quests of the forced zones
    /// done and the reputation goals met.
    pub(crate) fn ended(&self, s: &State) -> bool {
        self.growth().lowest_level(s) >= self.profile.to_level
            && s.forced_left == 0
            && s.required_left == 0
            && self.reputation_missing(s) <= 0.0
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
            to_level: self.profile.to_level,
        }
    }

    /// Trips: walking, boats, flights, the hearthstone.
    pub(crate) fn trips(&self) -> Trips<'_> {
        Trips {
            world: self.world,
            params: self.params,
            model: self.model,
            flights: &self.flights,
            legs: &self.legs,
            explicit: self.explicit,
            open_walks: self.open_walks.get(),
        }
    }

    /// The level a walk at `level` is made at: the zones closed to it (`World::walk_level`), none
    /// when the walks are open.
    pub(crate) fn walk_level(&self, level: i64) -> u8 {
        if self.open_walks.get() {
            0
        } else {
            self.world.walk_level(level)
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
