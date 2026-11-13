//! Simulating a route one stop at a time: the character's state at the start, each stop done
//! (`step`), what happens on the way there (`on_the_way`, `camp`, `spot`), the end of the guide.

use super::availability::Availability;
use super::combat::Combat;
use super::growth::Growth;
use super::indexes::Indexes;
use super::places::Places;
use super::state::{self, Breakdown, Flags, Kind, Landed, Planned, State, Stop};
use super::travel::Trips;
use super::worth::Worth;
use super::{Memo, Mode};
use crate::model::{Model, Profile, Quest};
use crate::params::Params;
use crate::world::{Pos, World};
use crate::xp;
use std::cell::{Cell, RefCell};
use std::rc::Rc;

/// What a stop does to the character, for a planner's routes (`Planner::sim`). Cheap to copy:
/// a variant of it does a step otherwise (`opening_walks`, `farming_all`, `recording_heres`).
#[derive(Clone, Copy)]
pub(crate) struct Simulator<'s> {
    pub(crate) model: &'s Model,
    pub(crate) world: &'s World,
    pub(crate) params: &'s Params,
    pub(crate) profile: &'s Profile,
    pub(super) ix: &'s Indexes,
    /// Where each stop takes place seen from the start (see `pos_from_start`).
    pub(super) from_start: &'s [Vec<Pos>],
    pub(super) memo: &'s Memo,
    pub(super) mode: Mode,
    /// Nothing is done that the route does not say (see `Planner::explicit`).
    pub(super) explicit: bool,
    /// Walks go into the zones too strong for the character (a step of an edited route that
    /// cannot be made otherwise: the editor allows it, with a warning).
    pub(super) open_walks: bool,
    /// The next walk kills every normal mob met, beyond `farm_cap`, while it holds true (a step
    /// of an edited route): the first walk sets it back to false.
    pub(super) farm_all: Option<&'s Cell<bool>>,
    /// Where `while_here` looked for quests at hand (see `Run::heres`).
    pub(super) heres: Option<&'s RefCell<Vec<Pos>>>,
}

impl<'s> Simulator<'s> {
    /// The same simulator walking through the zones too strong for the character when `open`.
    #[must_use]
    pub(crate) fn opening_walks(self, open: bool) -> Self {
        Self {
            open_walks: open,
            ..self
        }
    }

    /// The same simulator killing every normal mob on its next walk while `once` holds true.
    #[must_use]
    pub(crate) fn farming_all(self, once: &'s Cell<bool>) -> Self {
        Self {
            farm_all: Some(once),
            ..self
        }
    }

    /// The same simulator recording into `heres` where it looks for quests at hand.
    #[must_use]
    pub(crate) fn recording_heres(self, heres: &'s RefCell<Vec<Pos>>) -> Self {
        Self {
            heres: Some(heres),
            ..self
        }
    }

    pub(crate) fn quest(&self, i: u32) -> &'s Quest {
        &self.model.quests[i as usize]
    }

    /// Where the stops of a route take place.
    pub(crate) fn places(&self) -> Places<'s> {
        Places {
            model: self.model,
            world: self.world,
        }
    }

    /// Fights: kill times, the power objectives require, mob XP, grinding.
    pub(crate) fn combat(&self) -> Combat<'s> {
        Combat {
            params: self.params,
            dungeons: &self.model.dungeons,
        }
    }

    /// Experience rules of the guide's game version.
    pub(crate) fn rules(&self) -> &'static xp::Rules {
        self.params.edition.rules()
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
            // A character met in game knows the teleports its training gives (taught anywhere).
            taught: if init.trained.is_some() {
                (self.model.teleports.iter().enumerate())
                    .filter(|(_, tp)| tp.level <= level)
                    .fold(0, |mask, (i, _)| mask | 1 << i)
            } else {
                0
            },
            abilities: init.abilities,
            ranks: self.ranks_known_at_start(level),
            dungeons: 0,
            inside: None,
            accepted: Flags::new(n),
            turned: Flags::new(n),
            dropped: Vec::new(),
            objectives: vec![0; n],
            completion_error: None,
            timers: Vec::new(),
            started: Vec::new(),
            together: state::Together::default(),
            next: None,
            spot: None,
            landed: Landed::No,
            planned: Rc::new(Planned::default()),
            at: 0,
            class_bonus: self
                .profile
                .class_powers
                .iter()
                .enumerate()
                .filter(|(k, _)| self.model.known_class_powers & (1 << k) != 0)
                .map(|(_, (_, power))| Growth::class_power_levels(*power))
                .sum(),
            powers: self.model.known_class_powers,
            gear: crate::power::Gear::default(),
            bonus: 0.0,
            gear_bonus: 0.0,
            gear_quality: 0.0,
            explored: Flags::new(self.model.explore.len()),
            abandon_check: (0, 0, None),
            log: 0,
            grind_used: 0,
            farm_used: 0,
            reputation: (0..self.params.reputation_goals.len() + self.model.reputation_factions.len())
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

    /// The level a walk at `level` is made at: the zones closed to it (`World::walk_level`), none
    /// when the walks are open.
    pub(crate) fn walk_level(&self, level: i64) -> u8 {
        if self.open_walks {
            0
        } else {
            self.world.walk_level(level)
        }
    }

    /// What the character can do now: prerequisites, exclusive and gray quests, dungeons.
    pub(crate) fn availability(&self) -> Availability<'s> {
        Availability {
            model: self.model,
            params: self.params,
            profile: self.profile,
            replaying: self.mode == Mode::Replay,
            explicit: self.explicit,
            followers: &self.ix.followers,
            breadcrumbs: &self.ix.breadcrumbs,
        }
    }

    /// How the character grows: XP, rewards, power, professions.
    pub(crate) fn growth(&self) -> Growth<'s> {
        Growth {
            model: self.model,
            params: self.params,
            profile: self.profile,
        }
    }

    /// Whether quests are worth their trip.
    pub(crate) fn worth(&self) -> Worth<'s> {
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
    pub(crate) fn trips(&self) -> Trips<'s> {
        Trips {
            world: self.world,
            params: self.params,
            model: self.model,
            flights: &self.memo.flights,
            legs: &self.memo.legs,
            grounds: &self.memo.grounds,
            abilities: self.model.initial.abilities,
            explicit: self.explicit,
            open_walks: self.open_walks,
        }
    }

    /// Score added to the play time: grinding (beyond its cap even more) and farming on the
    /// way, flights weighed, gold spent on flights, zone changes, quests given up.
    pub(crate) fn weights(&self, spent: &Breakdown) -> f64 {
        (self.params.grind_weight() - 1.0) * spent.grinding
            + (self.params.grind_over_weight() - self.params.grind_weight()) * spent.grind_over
            + (self.params.grind_weight - 1.0).max(0.0) * spent.farming
            + (self.params.flight_weight - 1.0) * spent.flights
            + spent.flight_gold
            + spent.bag_overflow
            + self.params.zone_change_weight * f64::from(spent.zone_changes)
            + self.params.preferred_zone_weight * spent.outside_preferred
            + self.params.abandon_weight * f64::from(spent.abandoned)
            + self.params.unlock_delay_weight * spent.unlock_delay
            + self.params.group_sync_weight * spent.desync
    }

    /// Where stop `k` of `route` is followed: the next stop's place (see `State::next`).
    pub(crate) fn next_pos(&self, s: &State, route: &[Stop], k: usize) -> Option<Pos> {
        route.get(k + 1).map(|&next| self.places().pos(s, next))
    }
}
