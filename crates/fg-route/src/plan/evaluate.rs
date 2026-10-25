//! Replaying a whole route: its time and score, incremental evaluation of a changed route, why a route is invalid.

use super::Planner;
use super::on_the_way::CAMP_AHEAD;
use super::state::{Breakdown, Kind, Planned, State, Stop, Timed};
use crate::world::Pos;
use std::rc::Rc;

/// Where a route leaves the character.
#[derive(Debug, Clone)]
pub struct End {
    pub level: i64,
    pub xp: i64,
    pub rested: i64,
    pub pos: Pos,
    pub zone: i64,
    /// Inn the hearthstone is bound to, flight paths known (a bit per taxi node).
    pub bind: usize,
    pub known: u128,
    /// Quests turned in (indices), and those in the log with whether their objectives are done.
    pub turned: Vec<usize>,
    pub log: Vec<(usize, bool)>,
}

/// States before each stop of a route, and its total time.
pub(crate) struct Run {
    pub(crate) states: Vec<State>,
    /// Optimizer score, and the real play time.
    pub(crate) total: f64,
    pub(crate) play: f64,
    /// Whether the target level is reached within the route (stops after it have no effect).
    pub(crate) reached: bool,
    /// Where quests were looked for at hand during stop `k`: `heres[here_at[k]..here_at[k + 1]]`.
    /// The places really visited: an objective done at another of its places, the inn after the
    /// hearthstone, the trainer...
    pub(crate) heres: Vec<Pos>,
    pub(crate) here_at: Vec<u32>,
}

impl Planner<'_> {
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
            + self.params.group_sync_weight * spent.desync
    }

    /// Time to reach the target level following `route` (plus penalties), or None if invalid.
    pub fn simulate(&self, route: &[Stop], trace: Option<&mut Vec<Timed>>) -> Option<f64> {
        self.simulate_full(route, trace).map(|(t, _)| t)
    }

    /// Quests `route` accepts (bit 1), turns in (bit 2) or does an objective of (bit 4).
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
                Kind::Objective(_) => planned.flags[i] |= 4,
                _ => {}
            }
        }
        Rc::new(planned)
    }

    pub fn simulate_full(&self, route: &[Stop], trace: Option<&mut Vec<Timed>>) -> Option<(f64, Breakdown)> {
        self.simulate_state(route, trace).map(|(total, s)| (total, s.spent))
    }

    /// Where `route` leaves the character (None if invalid).
    pub fn end_of(&self, route: &[Stop]) -> Option<End> {
        let (_, s) = self.simulate_state(route, None)?;
        let n = self.model.quests.len();
        Some(End {
            level: s.level,
            xp: s.xp,
            rested: s.rested,
            pos: s.pos,
            zone: s.zone,
            bind: s.bind,
            known: s.known,
            turned: (0..n).filter(|&i| s.rewarded(i)).collect(),
            log: (0..n)
                .filter(|&i| s.accepted[i] && !s.turned[i])
                .map(|i| (i, self.availability().objectives_done(&s, i as u32)))
                .collect(),
        })
    }

    /// Points with the faction of each reputation goal at the end of `route`.
    pub fn reputation_reached(&self, route: &[Stop]) -> Vec<f64> {
        self.simulate_state(route, None)
            .map(|(_, s)| s.reputation.iter().map(|&v| f64::from(v)).collect())
            .unwrap_or_default()
    }

    /// The score of `route` and the character's state at its end.
    pub(super) fn simulate_state(&self, route: &[Stop], mut trace: Option<&mut Vec<Timed>>) -> Option<(f64, State)> {
        let mut s = self.start_state(self.planned(route));
        for (k, &stop) in route.iter().enumerate() {
            if self.ended(&s) {
                break;
            }
            if !self.availability().valid(&s, stop) {
                return None;
            }
            s.at = k as u32;
            s.next = self.next_pos(&s, route, k);
            self.apply(&mut s, stop, trace.as_deref_mut());
            if !s.time.is_finite() {
                return None;
            }
        }
        let total = self.finish(&mut s, trace);
        Some((total, s))
    }

    /// Replay a route keeping the state before each stop, for incremental evaluation.
    pub(crate) fn run(&self, route: &[Stop]) -> Option<Run> {
        *self.heres.borrow_mut() = Some(Vec::new());
        let run = self.replay_states(route);
        let heres = self.heres.borrow_mut().take().unwrap_or_default();
        run.map(|run| Run { heres, ..run })
    }

    /// `run`, the places looked at hand recorded by `while_here` meanwhile.
    fn replay_states(&self, route: &[Stop]) -> Option<Run> {
        let recorded = || self.heres.borrow().as_ref().map_or(0, Vec::len) as u32;
        let mut s = self.start_state(self.planned(route));
        let mut states = Vec::with_capacity(route.len() + 1);
        let mut here_at = Vec::with_capacity(route.len() + 1);
        states.push(s.clone());
        here_at.push(0);
        for (k, &stop) in route.iter().enumerate() {
            if self.ended(&s) {
                break;
            }
            if !self.availability().valid(&s, stop) {
                return None;
            }
            s.at = k as u32;
            s.next = self.next_pos(&s, route, k);
            self.apply(&mut s, stop, None);
            if !s.time.is_finite() {
                return None;
            }
            states.push(s.clone());
            here_at.push(recorded());
        }
        let reached = self.ended(&s);
        let total = self.finish(&mut s, None);
        Some(Run {
            states,
            total,
            play: s.spent.total,
            reached,
            heres: Vec::new(),
            here_at,
        })
    }

    /// First stop before `a` of the evaluated route whose quests taken or turned in at hand
    /// (`while_here`) may differ with the plan `new` of the candidate: a stop with an NPC at hand
    /// of a quest whose plan changed, where the stop really looked (`Run::heres`). A quest is taken at hand up to `CAMP_AHEAD` stops before
    /// the route accepts it, turned in at hand once in the log, and not taken when it would
    /// block one the route takes later.
    fn replanned_from(&self, run: &Run, new: &Planned, a: usize) -> usize {
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
                run.heres[run.here_at[*k] as usize..run.here_at[*k + 1] as usize]
                    .iter()
                    .any(|here| q.starts.iter().chain(&q.ends).any(|l| l.pos.dist(here) <= radius))
            };
            if let Some(k) = (from..to.min(*start).min(done)).find(near) {
                *start = k;
            }
        };
        for i in 0..new.flags.len() {
            let (flags, at) = (
                (old.flags[i] & 3, new.flags[i] & 3),
                (old.accept_at[i], new.accept_at[i]),
            );
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
    #[cfg(test)]
    pub(crate) fn evaluate(&self, run: &Run, candidate: &[Stop], a: usize, b: usize, same_set: bool) -> Option<f64> {
        self.evaluate_within(run, candidate, a, b, same_set, None)
            .map(|(cost, _)| cost)
    }

    /// `evaluate`, its replay ended before stop `window` (when given) with the rest estimated
    /// (`estimate`); the cost, and whether it is exact. Exact anyway when the move changes a
    /// quest whose absence the end of the route counts (mandatory ones).
    pub(crate) fn evaluate_within(
        &self,
        run: &Run,
        candidate: &[Stop],
        a: usize,
        b: usize,
        same_set: bool,
        window: Option<usize>,
    ) -> Option<(f64, bool)> {
        // The stop before `a` too: where it does an objective depends on the next stop.
        let a = a.saturating_sub(1);
        // Positions of the accepts change even when the stops are only reordered.
        let planned = self.planned(candidate);
        // The quests of the character's log given up at the start changed: from the start.
        let old = &run.states[0].planned;
        let window = window.filter(|_| {
            (self.model.quests.iter().enumerate())
                .all(|(i, q)| !q.mandatory || (old.flags[i] ^ planned.flags[i]) & 2 == 0)
        });
        let restart =
            (self.model.initial.accepted.iter()).any(|&i| Planned::left(old, i) != Planned::left(&planned, i));
        let start = if restart {
            0
        } else {
            self.replanned_from(run, &planned, a)
        };
        if run.reached && start >= run.states.len() - 1 {
            return Some((run.total, true)); // after the target level: no effect
        }
        let (mut s, mut k) = if restart {
            (self.start_state(planned), 0)
        } else {
            let mut s = run.states[start].clone();
            s.planned = planned;
            (s, start)
        };
        while k < candidate.len() {
            if self.ended(&s) {
                break;
            }
            if window == Some(k)
                && let Some(cost) = self.estimate(run, &s)
            {
                return Some((cost, false));
            }
            if !self.availability().valid(&s, candidate[k]) {
                return None;
            }
            s.at = k as u32;
            s.next = self.next_pos(&s, candidate, k);
            self.apply(&mut s, candidate[k], None);
            if !s.time.is_finite() {
                return None;
            }
            k += 1;
            if same_set && k == b + 1 && k < run.states.len() {
                let old = &run.states[k];
                if s.level == old.level
                    && s.xp == old.xp
                    && s.rested == old.rested
                    && s.mates == old.mates
                    && s.order == old.order
                    && s.synced == old.synced
                    && s.stock == old.stock
                    && s.bags.to_bits() == old.bags.to_bits()
                    && s.held == old.held
                    && s.known == old.known
                    && s.log == old.log
                    && s.pos == old.pos
                    && s.zone == old.zone
                    && s.bind == old.bind
                    && s.hearth_ready <= s.time
                    && old.hearth_ready <= old.time
                    && s.recall_ready <= s.time
                    && old.recall_ready <= old.time
                    && s.trained == old.trained
                    && s.dungeons == old.dungeons
                    && s.abandon_check == old.abandon_check
                    && s.accepted == old.accepted
                    && s.turned == old.turned
                    && s.dropped == old.dropped
                    && s.objectives == old.objectives
                    && s.started == old.started
                    && s.powers == old.powers
                    && s.gear == old.gear
                    && s.explored == old.explored
                    && s.grind_used == old.grind_used
                    && s.farm_used == old.farm_used
                    && s.reputation == old.reputation
                    && s.forced_left == old.forced_left
                    && Self::same_timers(&s, old)
                    && s.together == old.together
                {
                    let extra = self.weights(&s.spent) - self.weights(&old.spent);
                    return Some((run.total + s.time - old.time + extra, true));
                }
            }
        }
        Some((self.finish(&mut s, None), true))
    }

    /// The cost of a route whose replay reached state `s`, estimated from the evaluated route
    /// `run`: its cost, plus how much later (or sooner) `s` comes than `run` had the same XP
    /// (score at that XP, between the two states around it). None when `s` has more XP than
    /// `run` ever had.
    fn estimate(&self, run: &Run, s: &State) -> Option<f64> {
        let rules = self.rules();
        let total_xp = |st: &State| (1..st.level).map(|l| rules.to_next_level(l)).sum::<i64>() + st.xp;
        let score = |st: &State| st.time + self.weights(&st.spent);
        let xp = total_xp(s);
        let j = run.states.partition_point(|st| total_xp(st) < xp);
        let after = run.states.get(j)?;
        let at = if j == 0 {
            score(after)
        } else {
            let before = &run.states[j - 1];
            let (x0, x1) = (total_xp(before), total_xp(after));
            let f = if x1 > x0 {
                (xp - x0) as f64 / (x1 - x0) as f64
            } else {
                1.0
            };
            score(before) + f * (score(after) - score(before))
        };
        Some(run.total + score(s) - at)
    }

    /// Where stop `k` of `route` is followed: the next stop's place (see `State::next`).
    pub(crate) fn next_pos(&self, s: &State, route: &[Stop], k: usize) -> Option<Pos> {
        route.get(k + 1).map(|&next| self.places().pos(s, next))
    }

    /// Whether the timed quests of both states have the same time left (the rest of the route is
    /// shifted in time: their deadlines are compared to each state's time). Exact, so that a
    /// rounding difference only makes the evaluation replay the rest.
    fn same_timers(s: &State, old: &State) -> bool {
        s.timers.len() == old.timers.len()
            && s.timers.iter().zip(&old.timers).all(|(&(q, due), &(oq, old_due))| {
                q == oq && (due - s.time).to_bits() == (old_due - old.time).to_bits()
            })
    }

    /// First stop that makes a route invalid, with the reason. Replayed as `simulate` does: with
    /// the route's plan (the quests taken and turned in at hand), up to the target level.
    pub fn diagnose(&self, route: &[Stop]) -> Option<(usize, String)> {
        let mut s = self.start_state(self.planned(route));
        for (n, &stop) in route.iter().enumerate() {
            if self.ended(&s) {
                break;
            }
            let label = if stop.is_quest() {
                format!("{} {:?}", self.quest(stop.index).name, stop.kind)
            } else {
                format!("{:?} {}", stop.kind, stop.index)
            };
            if !self.availability().valid(&s, stop) {
                let reason = match stop.kind {
                    Kind::Accept if s.log >= self.params.quest_log_size => "quest log full".to_owned(),
                    Kind::Accept if !self.availability().prereqs_met(&s, stop.index) => {
                        let q = self.quest(stop.index);
                        format!(
                            "prerequisites {:?}/{:?} not done (or {:?} not in the log)",
                            q.pre_all, q.pre_any, q.pre_held
                        )
                    }
                    Kind::Accept if self.availability().blocked(&s, self.quest(stop.index)) => {
                        "exclusive or breadcrumb conflict".to_owned()
                    }
                    _ => "not in the right state".to_owned(),
                };
                return Some((n, format!("{label}: {reason}")));
            }
            s.at = n as u32;
            s.next = self.next_pos(&s, route, n);
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
            Kind::Accept if !self.availability().prereqs_met(s, stop.index) => "prerequisite not done",
            Kind::Accept if self.availability().blocked(s, self.quest(stop.index)) => "exclusive quest taken",
            Kind::Accept if self.availability().gray_dead_end(s, self.quest(stop.index), stop.index) => {
                "gray quest leading nowhere"
            }
            Kind::Accept => "dungeon quest (dungeons off)",
            Kind::Objective(_) | Kind::TurnIn if !s.accepted[i] => "quest not accepted",
            Kind::Objective(_) | Kind::TurnIn if s.turned[i] => "already turned in",
            Kind::Objective(_) => "objective done in a dungeon run",
            Kind::TurnIn => "objectives not done",
            Kind::LearnFlight => "flight path already known",
            Kind::Bind => "hearthstone already bound there",
            Kind::Hearth => "no such spell",
            Kind::Dungeon => "dungeon already run or left out",
        }
    }
}
