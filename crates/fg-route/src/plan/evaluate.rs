//! Replaying a whole route: its time and score, incremental evaluation of a changed route, why a route is invalid.

use super::Planner;
use super::on_the_way::CAMP_AHEAD;
use super::state::{Breakdown, Kind, Planned, State, Stop, Timed};
use std::rc::Rc;

/// States before each stop of a route, and its total time.
pub(crate) struct Run {
    pub(crate) states: Vec<State>,
    /// Optimizer score, and the real play time.
    pub(crate) total: f64,
    pub(crate) play: f64,
    /// Whether the target level is reached within the route (stops after it have no effect).
    pub(crate) reached: bool,
}

impl Planner<'_> {
    /// Score added to the play time: grinding (beyond its cap even more) and farming on the
    /// way, flights weighed, gold spent on flights.
    pub(crate) fn weights(&self, spent: &Breakdown) -> f64 {
        (self.params.grind_weight() - 1.0) * spent.grinding
            + (self.params.grind_over_weight() - self.params.grind_weight()) * spent.grind_over
            + (self.params.grind_weight - 1.0).max(0.0) * spent.farming
            + (self.params.flight_weight - 1.0) * spent.flights
            + self.params.gold_weight * spent.flight_cost as f64 / 10_000.0
            + self.params.zone_change_weight * f64::from(spent.zone_changes)
    }

    /// Time to reach the target level following `route` (plus penalties), or None if invalid.
    pub fn simulate(&self, route: &[Stop], trace: Option<&mut Vec<Timed>>) -> Option<f64> {
        self.simulate_full(route, trace).map(|(t, _)| t)
    }

    /// Quests `route` accepts (bit 1) and turns in (bit 2).
    pub(crate) fn planned(&self, route: &[Stop]) -> Rc<Planned> {
        let n = self.model.quests.len();
        let mut planned = Planned {
            flags: vec![0; n],
            accept_at: vec![u32::MAX; n],
        };
        for (k, stop) in route.iter().enumerate() {
            let i = stop.index as usize;
            match stop.kind {
                Kind::Accept => {
                    planned.flags[i] |= 1;
                    planned.accept_at[i] = planned.accept_at[i].min(k as u32);
                }
                Kind::TurnIn => planned.flags[i] |= 2,
                _ => {}
            }
        }
        Rc::new(planned)
    }

    pub fn simulate_full(&self, route: &[Stop], mut trace: Option<&mut Vec<Timed>>) -> Option<(f64, Breakdown)> {
        let mut s = self.initial_state();
        s.planned = self.planned(route);
        for (k, &stop) in route.iter().enumerate() {
            if s.level >= self.profile.to_level {
                break;
            }
            if !self.availability().valid(&s, stop) {
                return None;
            }
            s.at = k as u32;
            self.apply(&mut s, stop, trace.as_deref_mut());
            if !s.time.is_finite() {
                return None;
            }
        }
        let total = self.finish(&mut s, trace);
        Some((total, s.spent))
    }

    /// Replay a route keeping the state before each stop, for incremental evaluation.
    pub(crate) fn run(&self, route: &[Stop]) -> Option<Run> {
        let mut s = self.initial_state();
        s.planned = self.planned(route);
        let mut states = Vec::with_capacity(route.len() + 1);
        states.push(s.clone());
        for (k, &stop) in route.iter().enumerate() {
            if s.level >= self.profile.to_level {
                break;
            }
            if !self.availability().valid(&s, stop) {
                return None;
            }
            s.at = k as u32;
            self.apply(&mut s, stop, None);
            if !s.time.is_finite() {
                return None;
            }
            states.push(s.clone());
        }
        let reached = s.level >= self.profile.to_level;
        let total = self.finish(&mut s, None);
        Some(Run {
            states,
            total,
            play: s.spent.total,
            reached,
        })
    }

    /// First stop before `a` of the evaluated route whose quests taken or turned in at hand
    /// (`while_here`) may differ with the plan `new` of `candidate`: a stop with an NPC at hand
    /// of a quest whose plan changed. A quest is taken at hand up to `CAMP_AHEAD` stops before
    /// the route accepts it, turned in at hand once in the log, and not taken when it would
    /// block one the route takes later.
    fn replanned_from(&self, run: &Run, candidate: &[Stop], new: &Planned, a: usize) -> usize {
        let radius = self.params.camp_radius;
        let old = &run.states[0].planned;
        // Stops after the target level were not done.
        let done = a.min(run.states.len() - 1);
        if radius <= 0.0 || done == 0 {
            return a;
        }
        let ahead = |at: u32| at.saturating_sub(CAMP_AHEAD) as usize;
        let mut start = a;
        // Stops `from..to` (before `start`) with an NPC of quest `i` at hand.
        let at_hand = |i: usize, from: usize, to: usize, start: &mut usize| {
            let q = &self.model.quests[i];
            let near = |k: &usize| {
                let here = self.places().pos(&run.states[*k], candidate[*k]);
                q.starts.iter().chain(&q.ends).any(|l| l.pos.dist(&here) <= radius)
            };
            if let Some(k) = (from..to.min(*start).min(done)).find(near) {
                *start = k;
            }
        };
        for i in 0..new.flags.len() {
            let (flags, at) = ((old.flags[i], new.flags[i]), (old.accept_at[i], new.accept_at[i]));
            if flags.0 == flags.1 && at.0 == at.1 {
                continue;
            }
            let from = if self.model.initial.accepted.contains(&i) {
                0
            } else {
                ahead(at.0.min(at.1))
            };
            // Only the accept moved: taken at hand where one plan takes it soon and not the other.
            let to = if flags.0 == flags.1 {
                ahead(at.0.max(at.1))
            } else {
                usize::MAX
            };
            at_hand(i, from, to, &mut start);
            if (flags.0 ^ flags.1) & 1 != 0 {
                for &x in &self.blocked_by[i] {
                    let x = x as usize;
                    at_hand(x, ahead(old.accept_at[x].min(new.accept_at[x])), usize::MAX, &mut start);
                }
            }
        }
        start
    }

    /// Cost of `candidate`, which differs from the evaluated route only from index `a` (its
    /// replay starts earlier when quests taken at hand before `a` may change with the plan).
    /// When `same_set` (stops `a..=b` were only reordered) and the state after `b` matches the
    /// old one, the rest of the route is unchanged: only the time difference is added.
    pub(crate) fn evaluate(&self, run: &Run, candidate: &[Stop], a: usize, b: usize, same_set: bool) -> Option<f64> {
        // Positions of the accepts change even when the stops are only reordered.
        let planned = self.planned(candidate);
        let start = self.replanned_from(run, candidate, &planned, a);
        if run.reached && start >= run.states.len() - 1 {
            return Some(run.total); // after the target level: no effect
        }
        let mut s = run.states[start].clone();
        s.planned = planned;
        let mut k = start;
        while k < candidate.len() {
            if s.level >= self.profile.to_level {
                break;
            }
            if !self.availability().valid(&s, candidate[k]) {
                return None;
            }
            s.at = k as u32;
            self.apply(&mut s, candidate[k], None);
            if !s.time.is_finite() {
                return None;
            }
            k += 1;
            if same_set && k == b + 1 && k < run.states.len() {
                let old = &run.states[k];
                if s.level == old.level
                    && s.xp == old.xp
                    && s.known == old.known
                    && s.log == old.log
                    && s.pos == old.pos
                    && s.zone == old.zone
                    && s.bind == old.bind
                    && s.hearth_ready <= s.time
                    && old.hearth_ready <= old.time
                    && s.trained == old.trained
                    && s.dungeons == old.dungeons
                    && s.abandon_check == old.abandon_check
                    && s.accepted == old.accepted
                    && s.turned == old.turned
                    && s.dropped == old.dropped
                    && s.objectives == old.objectives
                    && s.along == old.along
                    && s.powers == old.powers
                    && s.gear == old.gear
                    && s.explored == old.explored
                    && s.grind_used == old.grind_used
                {
                    let extra = self.weights(&s.spent) - self.weights(&old.spent);
                    return Some(run.total + s.time - old.time + extra);
                }
            }
        }
        Some(self.finish(&mut s, None))
    }

    /// First stop that makes a route invalid, with the reason.
    pub fn diagnose(&self, route: &[Stop]) -> Option<(usize, String)> {
        let mut s = self.initial_state();
        for (n, &stop) in route.iter().enumerate() {
            let label = if stop.is_quest() {
                format!("{} {:?}", self.quest(stop.index).name, stop.kind)
            } else {
                format!("{:?} {}", stop.kind, stop.index)
            };
            if !self.availability().valid(&s, stop) {
                let reason = match stop.kind {
                    Kind::Accept if s.log >= self.params.quest_log_size => "quest log full".to_owned(),
                    Kind::Accept if !self.availability().prereqs_met(&s, self.quest(stop.index)) => {
                        let q = self.quest(stop.index);
                        format!("prerequisites {:?}/{:?} not done", q.pre_all, q.pre_any)
                    }
                    Kind::Accept if self.availability().blocked(&s, self.quest(stop.index)) => {
                        "exclusive or breadcrumb conflict".to_owned()
                    }
                    _ => "not in the right state".to_owned(),
                };
                return Some((n, format!("{label}: {reason}")));
            }
            self.apply(&mut s, stop, None);
            if !s.time.is_finite() {
                let loc = self.places().loc(&s, stop);
                return Some((
                    n,
                    format!(
                        "{label}: unreachable ({} in zone {})",
                        loc.name,
                        self.world.zone_name(loc.zone)
                    ),
                ));
            }
        }
        None
    }

    pub(crate) fn why_invalid(&self, s: &State, stop: Stop) -> &'static str {
        let i = stop.index as usize;
        match stop.kind {
            Kind::Accept if s.turned[i] => "already turned in",
            Kind::Accept if s.accepted[i] => "already in the log",
            Kind::Accept if s.log >= self.params.quest_log_size => "quest log full",
            Kind::Accept if !self.availability().prereqs_met(s, self.quest(stop.index)) => "prerequisite not done",
            Kind::Accept if self.availability().blocked(s, self.quest(stop.index)) => "exclusive quest taken",
            Kind::Accept if self.availability().gray_dead_end(s, self.quest(stop.index), stop.index) => {
                "gray quest leading nowhere"
            }
            Kind::Accept => "dungeon quest (dungeons off)",
            Kind::Objective(_) | Kind::TurnIn if !s.accepted[i] => "quest not accepted",
            Kind::Objective(_) | Kind::TurnIn if s.turned[i] => "already turned in",
            Kind::Objective(_) => "objective already done or in a dungeon",
            Kind::TurnIn => "objectives not done (dungeon)",
            _ => "not possible",
        }
    }
}
