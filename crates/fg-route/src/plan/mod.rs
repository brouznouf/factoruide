//! Route planning.
//!
//! A route is an ordered list of stops: quest stops (accept, complete an objective, turn in) and
//! visits (learn a flight path, bind the hearthstone, run a dungeon, learn a profession rank).
//!
//! Data (what is manipulated): `state` (stops, the character's state, the trace of a replay),
//! `indexes` (what is looked up all the time, computed once from the model).
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
//! `sim` (`Simulator`): what a stop does to the character, built on the services: `step` doing
//! one stop, `on_the_way` what happens while moving. A planner gives its own (`Planner::sim`).
//!
//! Orchestrators (they call the simulator and the services in order):
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
mod indexes;
mod moves;
mod on_the_way;
mod places;
mod profession_trainers;
mod regions;
mod report;
mod search;
mod share;
mod sim;
mod spot;
mod state;
mod step;
mod tidy;
mod together;
mod travel;
mod worth;

#[cfg(test)]
pub(crate) mod tests;

pub use combat::Combat;
pub use edit::{Edit, Edited, EditedStep, Farmed, Outcome};
pub use evaluate::End;
pub use follow::{Followed, Want};
pub(crate) use on_the_way::segment_dist;
pub use places::Places;
pub(crate) use profession_trainers::RANKS;
pub use regions::Group;
pub use search::Effort;
pub use share::{Exchange, Member};
pub use state::{Breakdown, Event, Kind, Stop, Timed, fmt_time};

use crate::diagnostics::{self, Diagnostics};
use crate::model::{Model, Profile, Quest};
use crate::params::Params;
use crate::world::{Pos, World};
use crate::xp;
use availability::Availability;
use camp::NearCamps;
use growth::Growth;
use indexes::Indexes;
use on_the_way::FarmWalks;
pub(crate) use sim::Simulator;
pub(crate) use state::State;
use std::rc::Rc;
use travel::{FlightTables, Grounds, Legs, Trips};

use worth::Worth;

/// What a planner computed already, kept for its next calls. The keys hold only what changes from
/// one call of a planner to the next (places, levels, flight paths known): a memo is for the
/// planner of one model, world, settings and mode (an `explicit` one does not take the same trips).
#[derive(Default)]
struct Memo {
    /// Flight times and prices between known flight paths, by known set.
    flights: FlightTables,
    /// Trips already computed (see `travel::Legs`).
    legs: Legs,
    grounds: Grounds,
    /// Mobs farmed on each walk, by level (see `on_the_way::farm_along`).
    farm_walks: FarmWalks,
    /// The camps nearest to each place grinding starts at (see `camp::camp`).
    near_camps: NearCamps,
}

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
    /// What it computed already: its own, its keys tell only the calls of one planner apart.
    memo: Memo,
    mode: Mode,
    /// What is looked up all the time, computed once from the model (shared with `replaying`).
    ix: Rc<Indexes>,
    /// Where each stop takes place seen from the start (`Places::pos` from the initial state),
    /// by quest: accept, turn-in, then its objectives. The insertions ask it all the time.
    from_start: Rc<[Vec<Pos>]>,
    /// Check every incremental evaluation of the search against a full replay (`verified`, or
    /// the diagnostics' `verify`).
    verify: bool,
    /// What the job is asked to tell (`diagnosed`).
    pub(crate) diagnostics: &'a Diagnostics,
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
    /// The level every stop of the forced quests can be made at from the start, by their gates
    /// (minimum levels, guards, mobs within reach, dungeons' levels); None without any. A dungeon
    /// guide is planned at it. A stop gated above the game's levels is never made: left out (the
    /// guide reports its quest not planned) rather than holding every other one back.
    pub(crate) fn forced_quests_level(&self) -> Option<i64> {
        let s = self.sim().initial_state();
        (0..self.model.quests.len() as u32)
            .filter(|&i| self.model.quests[i as usize].zone_forced && !s.turned[i as usize])
            .flat_map(|i| {
                let objectives = self.model.quests[i as usize].objectives.len();
                [Kind::Accept, Kind::TurnIn]
                    .into_iter()
                    .chain((0..objectives).map(|k| Kind::Objective(k as u8)))
                    .map(move |kind| Stop::quest(i, kind))
            })
            .map(|stop| self.sim().gate_level(&s, stop))
            .filter(|&level| level <= self.rules().max_level)
            .max()
    }

    pub fn new(model: &'a Model, world: &'a World, params: &'a Params, profile: &'a Profile) -> Self {
        let ix = Rc::new(Indexes::new(model, params));
        let mut planner = Self::with_indexes(model, world, params, profile, ix, Rc::from([]));
        let s0 = planner.sim().initial_state();
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

    /// A planner of indexes and places from the start already computed.
    fn with_indexes(
        model: &'a Model,
        world: &'a World,
        params: &'a Params,
        profile: &'a Profile,
        ix: Rc<Indexes>,
        from_start: Rc<[Vec<Pos>]>,
    ) -> Self {
        Self {
            model,
            world,
            params,
            profile,
            memo: Memo::default(),
            mode: Mode::Plan,
            ix,
            from_start,
            verify: false,
            diagnostics: &diagnostics::OFF,
            abort: None,
            share: None,
            explicit: false,
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

    /// The same planner telling and checking what `diagnostics` asks.
    #[must_use]
    pub fn diagnosed(self, diagnostics: &'a Diagnostics) -> Self {
        Self {
            verify: self.verify || diagnostics.verify,
            diagnostics,
            ..self
        }
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
            ..Self::with_indexes(
                self.model,
                self.world,
                self.params,
                self.profile,
                Rc::clone(&self.ix),
                Rc::clone(&self.from_start),
            )
        }
    }

    /// The simulator of this planner's routes.
    pub(crate) fn sim(&self) -> Simulator<'_> {
        Simulator {
            model: self.model,
            world: self.world,
            params: self.params,
            profile: self.profile,
            ix: &self.ix,
            from_start: &self.from_start,
            memo: &self.memo,
            mode: self.mode,
            explicit: self.explicit,
            open_walks: false,
            farm_all: None,
            heres: None,
        }
    }

    /// What the character can do now: prerequisites, exclusive and gray quests, dungeons.
    pub(crate) fn availability(&self) -> Availability<'_> {
        self.sim().availability()
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
        self.sim().worth()
    }

    /// Trips: walking, boats, flights, the hearthstone.
    pub(crate) fn trips(&self) -> Trips<'_> {
        self.sim().trips()
    }

    pub(crate) fn quest(&self, i: u32) -> &Quest {
        &self.model.quests[i as usize]
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
