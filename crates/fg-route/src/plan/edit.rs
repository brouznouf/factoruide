//! Replaying a route edited by hand: each step checked in turn (the impossible ones are reported
//! and skipped), what each one did, and the time of the whole, as the optimizer counts it.

use super::Planner;
use super::state::{Breakdown, Event, Kind, Landed, State, Stop, Timed};
use super::step::recorder;
use std::ops::Range;

/// A step of a route edited by hand: one thing done (`Planner::explicit` does nothing else).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Edit {
    /// A stop of the route; `farm`: kill every normal mob met on the way to it, beyond
    /// `farm_cap`; `spot`: an objective's place (0 its own, then its `alts`; None: the best
    /// from where the character is).
    Stop { stop: Stop, farm: bool, spot: Option<u8> },
    /// Fly from a flight path to another (taxi node indexes), both known.
    Fly { from: usize, to: usize },
    /// Boats, zeppelins or trams: board link `first`, leave link `last` (`World::links`).
    Link { first: usize, last: usize },
    /// Cast a class teleport (`Model::teleports`).
    Teleport { to: usize },
    /// Die on purpose: back at the nearest spirit healer (`Model::graveyards`).
    DeathSkip,
    /// Learn the class spells at a trainer (`Model::trainers`).
    Train { trainer: usize },
    /// Sell the bags' loot at a vendor (`Model::vendors`).
    Sell { vendor: usize },
    /// Give up a quest of the log.
    Abandon { quest: u32 },
    /// Kill this many mobs of the character's level where it stands.
    Grind { mobs: u32 },
    /// Kill mobs of the character's level up to this level, at the camps of its levels (going
    /// there first).
    GrindTo { level: i64 },
}

/// What a step of an edited route did.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Outcome {
    Done,
    /// Nothing to do there (why): taken or turned in at hand earlier, objective done on the way,
    /// the hearthstone on cooldown, the target level reached before.
    Skipped(&'static str),
    /// Not possible there (why): the step is left out.
    Invalid(&'static str),
}

/// Mobs killed on the way to a step.
#[derive(Debug, Clone, Copy, Default, PartialEq, serde::Serialize)]
pub struct Farmed {
    pub kills: u32,
    pub xp: i64,
    pub seconds: f64,
}

/// A step of an edited route as replayed.
#[derive(Debug, Clone)]
pub struct EditedStep {
    pub outcome: Outcome,
    /// The one thing it did, in the trace (none when skipped or not possible).
    pub event: Option<usize>,
    /// Its events in the trace: its action, and what is not one (mobs farmed on the way, the
    /// reward picked).
    pub(crate) events: Range<usize>,
    /// Time before and after it, level after it, and the XP it gave.
    pub start: f64,
    pub end: f64,
    pub level: i64,
    pub xp: i64,
    /// Mobs farmed on the way to it.
    pub farm: Option<Farmed>,
    /// An objective done without `farm`: what killing every mob on the way to it would give.
    pub farm_all: Option<Farmed>,
    /// Done, but through a zone too strong for the character (the walk there is allowed to an
    /// edited step, not to the planner).
    pub warning: Option<&'static str>,
}

/// An edited route as replayed.
pub struct Edited {
    pub steps: Vec<EditedStep>,
    /// The stops done, in order (for the guide).
    pub route: Vec<Stop>,
    pub trace: Vec<Timed>,
    /// Real play time in `breakdown.total`; `score` adds the optimizer's penalties.
    pub breakdown: Breakdown,
    pub score: f64,
    /// Grinding added at the end to reach the target level (seconds).
    pub final_grind: f64,
    /// Quests the route takes but never turns in.
    pub unfinished: Vec<u32>,
}

impl Edited {
    /// Steps that are not possible.
    pub fn errors(&self) -> usize {
        self.steps
            .iter()
            .filter(|s| matches!(s.outcome, Outcome::Invalid(_)))
            .count()
    }
}

impl Planner<'_> {
    /// Replay an edited route as the optimizer simulates its own (`simulate_full`: the same
    /// route gives the same time), each step checked: an impossible one is reported and left out.
    pub fn edit(&self, edits: &[Edit]) -> Edited {
        self.edit_observed(edits, &mut |_| true)
            .expect("a replay nothing stops goes to the end")
    }

    /// `edit`, `observe` shown the character before the first step, after each step, and once
    /// the guide is over (the final grind done). None when `observe` says to stop (false), or
    /// the planner gives up (`abortable`: a newer replay is asked for).
    pub(crate) fn edit_observed(&self, edits: &[Edit], observe: &mut dyn FnMut(&State) -> bool) -> Option<Edited> {
        let stops: Vec<Stop> = edits
            .iter()
            .filter_map(|e| match e {
                Edit::Stop { stop, .. } => Some(*stop),
                _ => None,
            })
            .collect();
        let mut s = self.start_state(self.planned(&stops));
        let mut trace = Vec::new();
        let mut route = Vec::new();
        let mut steps = Vec::with_capacity(edits.len());
        let mut at = 0;
        if !observe(&s) {
            return None;
        }
        // The next stop of each step (an objective is done at its place on the way to it).
        let nexts: Vec<Option<Stop>> = (0..edits.len())
            .map(|e| {
                edits[e + 1..].iter().find_map(|x| match x {
                    Edit::Stop { stop, .. } => Some(*stop),
                    _ => None,
                })
            })
            .collect();
        for (e, edit) in edits.iter().enumerate() {
            if self.aborted() {
                return None;
            }
            let (start, xp, first) = (s.time, self.total_xp(&s), trace.len());
            let farm_xp = s.spent.farm_xp;
            let mut farm_all = None;
            let mut warning = None;
            let outcome = loop {
                let outcome = if self.ended(&s) {
                    Outcome::Skipped("target level reached")
                } else {
                    match *edit {
                        Edit::Stop { stop, farm, spot } => {
                            s.at = at;
                            s.next = nexts[e].map(|next| self.places().pos(&s, next));
                            s.spot = match stop.kind {
                                Kind::Objective(k) => spot.map(|i| (stop.index, k, i)),
                                _ => None,
                            };
                            if !farm && let Kind::Objective(_) = stop.kind {
                                farm_all = self.farm_all_estimate(&s, stop);
                            }
                            let outcome = self.edit_stop(&mut s, stop, farm, &mut trace);
                            s.spot = None;
                            if !matches!(outcome, Outcome::Invalid(_)) {
                                route.push(stop);
                            }
                            outcome
                        }
                        Edit::Fly { from, to } => self.edit_fly(&mut s, from, to, &mut trace),
                        Edit::Link { first, last } => self.edit_link(&mut s, first, last, &mut trace),
                        Edit::Teleport { to } => self.edit_teleport(&mut s, to, &mut trace),
                        Edit::DeathSkip => self.edit_death_skip(&mut s, &mut trace),
                        Edit::Train { trainer } => self.edit_train(&mut s, trainer, &mut trace),
                        Edit::Sell { vendor } => self.edit_sell(&mut s, vendor, &mut trace),
                        Edit::Abandon { quest } => {
                            let i = quest as usize;
                            if s.accepted[i] && !s.turned[i] {
                                self.abandon(&mut s, quest, &mut recorder(&mut Some(&mut trace)));
                                Outcome::Done
                            } else {
                                Outcome::Invalid("quest not in the log")
                            }
                        }
                        Edit::Grind { mobs } => {
                            if mobs == 0 {
                                Outcome::Invalid("no mob to kill")
                            } else {
                                let mut trace = Some(&mut trace);
                                let mut record = recorder(&mut trace);
                                let (near, zone) = (s.pos, s.zone);
                                let seconds = self.growth().grind_mobs(&mut s, mobs);
                                record(
                                    &s,
                                    Event::GrindMobs {
                                        mobs,
                                        seconds,
                                        near,
                                        zone,
                                    },
                                );
                                self.train(&mut s, &mut record);
                                Outcome::Done
                            }
                        }
                        Edit::GrindTo { level } => {
                            if level <= s.level {
                                Outcome::Skipped("level reached")
                            } else {
                                let mut trace = Some(&mut trace);
                                let mut record = recorder(&mut trace);
                                self.grind_at_camps(&mut s, level, &mut record);
                                self.train(&mut s, &mut record);
                                Outcome::Done
                            }
                        }
                    }
                };
                // A step that cannot be made only because a zone is closed to the character: the
                // editor goes there (the planner never does), and says so.
                if self.explicit && !self.open_walks.get() && outcome == Outcome::Invalid("unreachable") {
                    self.open_walks.set(true);
                    warning = Some("walks through a zone too strong for the character");
                    continue;
                }
                break outcome;
            };
            if self.open_walks.replace(false) && matches!(outcome, Outcome::Invalid(_)) {
                warning = None;
            }
            let actions: Vec<usize> = (first..trace.len()).filter(|&n| is_action(&trace[n].event)).collect();
            // Each step of the guide editor does one thing: anything else is a step of its own.
            // Grinding up several levels is one step, recorded at each camp it moves to.
            let one = actions.len() <= 1 || matches!(edit, Edit::GrindTo { .. });
            assert!(
                !self.explicit || one && actions.iter().all(|&n| is_own(*edit, &trace[n].event)),
                "step {e} ({edit:?}) does more than its own action: {:?}",
                actions.iter().map(|&n| &trace[n].event).collect::<Vec<_>>()
            );
            if let Edit::Stop { .. } = edit {
                at += 1;
            }
            steps.push(EditedStep {
                outcome,
                event: actions.into_iter().find(|&n| is_own(*edit, &trace[n].event)),
                events: first..trace.len(),
                start,
                end: s.time,
                level: s.level,
                xp: (self.total_xp(&s) - xp) as i64,
                farm: farmed(&trace[first..], s.spent.farm_xp - farm_xp),
                farm_all,
                warning,
            });
            if !observe(&s) {
                return None;
            }
        }
        // A dungeon quest taken whose dungeon is never run (and never turned in) cannot be done:
        // its accept is not possible.
        for (step, edit) in steps.iter_mut().zip(edits) {
            if let Edit::Stop { stop, .. } = edit
                && stop.kind == Kind::Accept
                && step.outcome == Outcome::Done
            {
                let i = stop.index as usize;
                let undone =
                    |(k, o): (usize, &crate::model::Objective)| o.dungeon.is_some() && s.objectives[i] & (1 << k) == 0;
                if !s.turned[i] && self.model.quests[i].objectives.iter().enumerate().any(undone) {
                    step.outcome = Outcome::Invalid("dungeon quest taken but its dungeon is not run");
                }
            }
        }
        let grinding = s.spent.grinding;
        let score = self.finish(&mut s, Some(&mut trace));
        if !observe(&s) {
            return None;
        }
        // Taken and never turned in by the route, left in the log or given up (those it turns in
        // after the target level are not: the guide ends there).
        let has = |i: usize, kind: Kind| stops.iter().any(|t| t.index as usize == i && t.kind == kind);
        let unfinished = (0..self.model.quests.len())
            .filter(|&i| {
                !s.rewarded(i) && (s.accepted[i] || s.turned[i]) && has(i, Kind::Accept) && !has(i, Kind::TurnIn)
            })
            .map(|i| i as u32)
            .collect();
        Some(Edited {
            steps,
            route,
            trace,
            final_grind: s.spent.grinding - grinding,
            breakdown: s.spent,
            score,
            unfinished,
        })
    }

    /// A stop of an edited route, checked first.
    fn edit_stop(&self, s: &mut State, stop: Stop, farm: bool, trace: &mut Vec<Timed>) -> Outcome {
        // Nothing to do rather than impossible: a flight path known, the hearthstone bound there.
        let i = stop.index as usize;
        match stop.kind {
            Kind::LearnFlight if s.known & (1u128 << i) != 0 => return Outcome::Skipped("flight path already known"),
            Kind::Bind if s.bind == i => return Outcome::Skipped("hearthstone already bound there"),
            _ => {}
        }
        if !self.availability().valid(s, stop) {
            return Outcome::Invalid(self.why_invalid(s, stop));
        }
        // A flight path at hand (on foot too): a detour to it, as one learned on the way.
        let detour = (self.explicit
            && stop.kind == Kind::LearnFlight
            && s.landed == Landed::No
            && s.pos.dist(&self.world.taxi_nodes[i].pos) <= self.params.flight_learn_radius)
            .then(|| 2.0 * self.trips().walk(&s.pos, &self.world.taxi_nodes[i].pos, s.level) + 5.0)
            .filter(|d| d.is_finite());
        if let Some(detour) = detour {
            s.time += detour;
            s.spent.travel += detour;
            s.known |= 1u128 << i;
            recorder(&mut Some(trace))(s, Event::LearnFlight { node: i });
            return Outcome::Done;
        }
        let before = Self::already_done(s, stop);
        let (saved, mark) = (s.clone(), trace.len());
        self.farm_all.set(farm);
        self.apply(s, stop, Some(trace));
        self.farm_all.set(false);
        if !s.time.is_finite() {
            *s = saved;
            trace.truncate(mark);
            return Outcome::Invalid("unreachable");
        }
        // A timed quest turned in too late is given up: a step of its own.
        if self.explicit && trace[mark..].iter().any(|t| matches!(t.event, Event::Abandon { .. })) {
            *s = saved;
            trace.truncate(mark);
            return Outcome::Invalid("quest time limit passed");
        }
        let hearth = || trace[mark..].iter().any(|t| matches!(t.event, Event::Hearth { .. }));
        if before {
            Outcome::Skipped("done before")
        } else if stop.kind == Kind::Hearth && !hearth() {
            Outcome::Skipped("not ready")
        } else {
            Outcome::Done
        }
    }

    /// What killing every normal mob on the way to `stop` would give.
    fn farm_all_estimate(&self, s: &State, stop: Stop) -> Option<Farmed> {
        if !self.availability().valid(s, stop) || Self::already_done(s, stop) {
            return None;
        }
        let mut s = s.clone();
        let mut trace = Vec::new();
        let farm_xp = s.spent.farm_xp;
        self.farm_all.set(true);
        self.apply(&mut s, stop, Some(&mut trace));
        self.farm_all.set(false);
        farmed(&trace, s.spent.farm_xp - farm_xp)
    }

    /// A flight of an edited route: to the flight master first.
    fn edit_fly(&self, s: &mut State, from: usize, to: usize, trace: &mut Vec<Timed>) -> Outcome {
        let known = |n: usize| s.known & (1u128 << n) != 0;
        if from == to {
            return Outcome::Invalid("same flight path");
        }
        if !known(from) {
            return Outcome::Invalid("flight path not known");
        }
        if !known(to) {
            return Outcome::Invalid("destination not known");
        }
        let table = self.trips().flight_table(s.known);
        let (time, price) = (table.0[from][to], table.1[from][to]);
        if !time.is_finite() {
            return Outcome::Invalid("no flight between them");
        }
        let nodes = &self.world.taxi_nodes;
        let mut trace = Some(trace);
        let mut record = recorder(&mut trace);
        // As a trip taking the flight by itself: the walk to the flight master and the flight
        // count as flying, the walk on to the next stop ends it (`State::landed`).
        let (start, end) = (nodes[from].pos, nodes[to].pos);
        let walk = self.trips().walk(&s.pos, &start, s.level);
        s.time += walk;
        s.spent.flights += walk;
        record(s, Event::Fly { from, to });
        let seconds = time + self.params.flight_overhead;
        s.time += seconds;
        s.spent.flights += seconds;
        if price.is_finite() {
            s.spent.flight_cost += price as i64;
            s.spent.flight_gold += self.params.gold_weight_at(s.level) * price / 10_000.0;
        }
        s.pos = end;
        s.landed = Landed::Flight;
        Outcome::Done
    }

    /// Boats, zeppelins or trams of an edited route: walking to the first one.
    fn edit_link(&self, s: &mut State, first: usize, last: usize, trace: &mut Vec<Timed>) -> Outcome {
        let links = &self.world.links;
        let ride = self.world.link_chain(self.trips().open_links(s))[first][last];
        if !ride.is_finite() {
            return Outcome::Invalid("no such crossing");
        }
        let walk = self.trips().walk(&s.pos, &links[first].from, s.level);
        if !walk.is_finite() {
            return Outcome::Invalid("unreachable");
        }
        s.time += walk;
        s.spent.travel += walk;
        recorder(&mut Some(trace))(s, Event::Link { first, last });
        s.time += ride;
        s.spent.travel += ride;
        s.pos = links[last].to;
        s.landed = Landed::Transport;
        Outcome::Done
    }

    /// A class teleport of an edited route.
    fn edit_teleport(&self, s: &mut State, to: usize, trace: &mut Vec<Timed>) -> Outcome {
        let teleport = &self.model.teleports[to];
        if !s.knows(teleport) {
            return Outcome::Invalid("teleport not learned");
        }
        recorder(&mut Some(trace))(s, Event::Teleport { to });
        s.time += self.params.teleport_cast;
        s.spent.travel += self.params.teleport_cast;
        s.pos = teleport.to.pos;
        s.landed = Landed::Transport;
        Outcome::Done
    }

    /// A death skip of an edited route: at the spirit healer nearest to where the character
    /// stands, while no player is past `DEATH_SKIP_UNTIL`.
    fn edit_death_skip(&self, s: &mut State, trace: &mut Vec<Timed>) -> Outcome {
        if !self.trips().may_die(s) {
            return Outcome::Invalid("too high for a death skip");
        }
        let Some(g) = self.trips().graveyard(s) else {
            return Outcome::Invalid("no spirit healer");
        };
        let gy = &self.model.graveyards[g];
        recorder(&mut Some(trace))(
            s,
            Event::DeathSkip {
                pos: gy.pos,
                zone: gy.zone,
            },
        );
        s.time += self.params.death_skip_time;
        s.spent.travel += self.params.death_skip_time;
        s.pos = gy.pos;
        s.zone = gy.zone;
        s.landed = Landed::Transport;
        Outcome::Done
    }

    /// Training of an edited route, at the trainer it says.
    fn edit_train(&self, s: &mut State, trainer: usize, trace: &mut Vec<Timed>) -> Outcome {
        let abilities = self.model.abilities.trained(s.abilities, s.level) & !s.abilities;
        if s.trained >= s.level && abilities == 0 {
            return Outcome::Skipped("nothing to learn");
        }
        let (saved, mark) = (s.clone(), trace.len());
        self.train_at(s, trainer, &mut recorder(&mut Some(&mut *trace)));
        if !s.time.is_finite() {
            *s = saved;
            trace.truncate(mark);
            return Outcome::Invalid("unreachable");
        }
        Outcome::Done
    }

    fn edit_sell(&self, s: &mut State, vendor: usize, trace: &mut Vec<Timed>) -> Outcome {
        if vendor >= self.model.vendors.len() {
            return Outcome::Invalid("unknown vendor");
        }
        let (saved, mark) = (s.clone(), trace.len());
        self.sell_at(s, vendor, &mut recorder(&mut Some(&mut *trace)));
        if !s.time.is_finite() {
            *s = saved;
            trace.truncate(mark);
            return Outcome::Invalid("unreachable");
        }
        Outcome::Done
    }

    /// The same route with each thing the simulation did by itself (a trip's flights, boats and
    /// teleports, training, flight paths learned in passing, quests given up, grinding) as a step
    /// of its own: what the explicit planner replays the same.
    pub fn expand(&self, edits: &[Edit]) -> Vec<Edit> {
        let edited = self.edit(edits);
        let mut out = Vec::new();
        for (step, edit) in edited.steps.iter().zip(edits) {
            // A flight path learned on arriving comes after the stop: the explicit planner
            // learns it as a detour from there.
            let mut learned = Vec::new();
            for t in &edited.trace[step.events.clone()] {
                let next = match t.event {
                    Event::Stop { stop, ref loc } => {
                        let farm = matches!(edit, Edit::Stop { stop: own, farm: true, .. } if *own == stop);
                        // Where it was done: replayed there.
                        let spot = match stop.kind {
                            Kind::Objective(k) => self.spot_index(stop.index, k, &loc.pos),
                            _ => None,
                        };
                        out.push(Edit::Stop { stop, farm, spot });
                        out.append(&mut learned);
                        continue;
                    }
                    Event::LearnFlight { node } => {
                        learned.push(Edit::Stop {
                            stop: Stop {
                                index: node as u32,
                                kind: Kind::LearnFlight,
                            },
                            farm: false,
                            spot: None,
                        });
                        continue;
                    }
                    Event::Hearth { recall, .. } => Edit::Stop {
                        stop: Stop {
                            index: u32::from(recall),
                            kind: Kind::Hearth,
                        },
                        farm: false,
                        spot: None,
                    },
                    Event::Fly { from, to } => Edit::Fly { from, to },
                    Event::Link { first, last } => Edit::Link { first, last },
                    Event::Teleport { to } => Edit::Teleport { to },
                    Event::DeathSkip { .. } => Edit::DeathSkip,
                    Event::Train { trainer, .. } => Edit::Train { trainer },
                    Event::Sell { vendor } => Edit::Sell { vendor },
                    Event::Abandon { quest } => Edit::Abandon { quest: quest as u32 },
                    Event::Grind { to_level, .. } => Edit::GrindTo { level: to_level },
                    Event::GrindMobs { mobs, .. } => Edit::Grind { mobs },
                    Event::Reward { .. } | Event::Farm { .. } | Event::Shortcut { .. } | Event::CatchUp { .. } => {
                        continue;
                    }
                };
                out.push(next);
            }
            out.append(&mut learned);
        }
        // The trip to the final grind (no step of its own: done after the last one) is one:
        // the explicit planner goes on foot otherwise.
        let tail = edited.steps.last().map_or(0, |step| step.events.end);
        for t in edited.trace.get(tail..).unwrap_or_default() {
            match t.event {
                Event::Fly { from, to } => out.push(Edit::Fly { from, to }),
                Event::Link { first, last } => out.push(Edit::Link { first, last }),
                Event::Teleport { to } => out.push(Edit::Teleport { to }),
                Event::DeathSkip { .. } => out.push(Edit::DeathSkip),
                _ => {}
            }
        }
        out
    }
}

/// Whether an event is something done (not the mobs farmed on the way, the reward picked, nor a
/// shortcut taken on the way: part of the walk to the step, shown but not an edit of its own).
fn is_action(event: &Event) -> bool {
    !matches!(
        event,
        Event::Farm { .. } | Event::Reward { .. } | Event::Shortcut { .. }
    )
}

/// Whether an event is the step's own action.
fn is_own(edit: Edit, event: &Event) -> bool {
    match (edit, event) {
        (Edit::Stop { stop, .. }, Event::Stop { stop: done, .. }) => *done == stop,
        (Edit::Stop { stop, .. }, Event::Hearth { recall, .. }) => {
            stop.kind == Kind::Hearth && stop.index == u32::from(*recall)
        }
        (Edit::Stop { stop, .. }, Event::LearnFlight { node }) => {
            stop.kind == Kind::LearnFlight && stop.index as usize == *node
        }
        (Edit::Fly { from, to }, Event::Fly { from: a, to: b }) => (from, to) == (*a, *b),
        (Edit::Link { first, last }, Event::Link { first: a, last: b }) => (first, last) == (*a, *b),
        (Edit::Teleport { to }, Event::Teleport { to: done }) => to == *done,
        (Edit::Train { trainer }, Event::Train { trainer: done, .. }) => trainer == *done,
        (Edit::Sell { vendor }, Event::Sell { vendor: done }) => vendor == *done,
        (Edit::Abandon { quest }, Event::Abandon { quest: done }) => quest as usize == *done,
        (Edit::Grind { .. }, Event::GrindMobs { .. })
        | (Edit::GrindTo { .. }, Event::Grind { .. })
        | (Edit::DeathSkip, Event::DeathSkip { .. }) => true,
        _ => false,
    }
}

/// The mobs farmed among `events`, which gave `xp`.
fn farmed(events: &[Timed], xp: i64) -> Option<Farmed> {
    let (kills, seconds) = events
        .iter()
        .filter_map(|t| match t.event {
            Event::Farm { kills, seconds, .. } => Some((kills, seconds)),
            _ => None,
        })
        .fold((0, 0.0), |(k, s), (kills, seconds)| (k + kills, s + seconds));
    (kills > 0).then_some(Farmed { kills, xp, seconds })
}
