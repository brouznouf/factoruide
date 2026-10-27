//! Improving a route: a time-boxed local search trying small moves and large neighbourhood
//! moves, keeping the best route, then a clean-up.

use super::Planner;
use super::acceptance::Acceptance;
use super::alns::Operators;
use super::evaluate::Run;
use super::moves::Change;
use super::state::{Event, Kind, Stop, fmt_time};
use rand::rngs::SmallRng;
use rand::{RngExt, SeedableRng};
use std::collections::HashSet;
use std::time::Duration;
use std::time::Instant;

/// How long a search tries.
#[derive(Debug, Clone, Copy)]
pub enum Effort {
    /// Until this time is over.
    Time(Duration),
    /// This many tries: the same route on any machine, only slower or faster.
    Tries(u64),
}

/// Where the search stands: the current route, the best one, how it decides.
struct Search {
    route: Vec<Stop>,
    run: Run,
    best: (f64, Vec<Stop>),
    rng: SmallRng,
    acceptance: Acceptance,
    operators: Operators,
    started: Instant,
    effort: Effort,
    tries: u64,
    wins: u64,
}

impl Search {
    fn progress(&self) -> f64 {
        match self.effort {
            Effort::Tries(n) => self.tries as f64 / n.max(1) as f64,
            Effort::Time(budget) => self.started.elapsed().as_secs_f64() / budget.as_secs_f64().max(1e-9),
        }
    }

    fn more(&self) -> bool {
        match self.effort {
            Effort::Tries(n) => self.tries < n,
            Effort::Time(budget) => self.started.elapsed() < budget,
        }
    }
}

impl Planner<'_> {
    /// Time-boxed local search on top of the greedy route.
    /// `on_best` receives the score, the real play time and the route each time the best route
    /// improves (and at the start).
    pub fn improve(
        &self,
        route: Vec<Stop>,
        budget: Duration,
        log: impl Fn(&str),
        on_best: impl Fn(f64, f64, &[Stop]),
    ) -> Vec<Stop> {
        self.improve_while(route, Effort::Time(budget), || true, log, on_best)
    }

    /// `improve` for `effort`, ending as soon as `keep_going` says no (it is asked before each
    /// try; the user stopped the generation): the best route so far is cleaned up and returned. An aborted planner (`abortable`) skips the
    /// clean-up: its route is thrown away, or cleaned up when improved again.
    pub fn improve_while(
        &self,
        route: Vec<Stop>,
        effort: Effort,
        keep_going: impl Fn() -> bool,
        log: impl Fn(&str),
        on_best: impl Fn(f64, f64, &[Stop]),
    ) -> Vec<Stop> {
        let Some(run) = self.run(&route) else {
            log("greedy route is invalid");
            return route;
        };
        log(&format!("start: {} stops, {}", route.len(), fmt_time(run.total)));
        on_best(run.total, run.play, &route);
        let mut search = Search {
            acceptance: Acceptance::new(run.total, self.params.anneal, self.params.lahc),
            best: (run.total, route.clone()),
            route,
            run,
            rng: SmallRng::seed_from_u64(self.params.seed),
            operators: Operators::new(),
            started: Instant::now(),
            effort,
            tries: 0,
            wins: 0,
        };
        while search.more() && search.route.len() > 2 && keep_going() {
            search.tries += 1;
            self.try_a_move(&mut search, &on_best);
            if self.params.share_every > 0 && search.tries.is_multiple_of(self.params.share_every) {
                self.meet(&mut search, &on_best);
            }
        }
        if let Some(member) = self.share {
            member.leave();
        }
        if self.aborted() {
            return search.best.1;
        }
        let (tries, wins) = (search.tries, search.wins);
        let (route, total) = self.clean_up(search);
        log(&format!(
            "improved: {} stops, {} ({tries} tries, {wins} improvements)",
            route.len(),
            fmt_time(total)
        ));
        route
    }

    /// At a meeting of the final (`sharing`): go on from the best route of all when it beats
    /// the current one.
    fn meet(&self, search: &mut Search, on_best: &impl Fn(f64, f64, &[Stop])) {
        let Some(member) = self.share else { return };
        let Some((score, route)) = member.meet(self.params.seed, &search.best) else {
            return;
        };
        if score >= search.run.total - 0.5 {
            return;
        }
        if let Some(run) = self.replace_run(&mut search.run, &search.route, &route) {
            search.run = run;
            search.route = route;
            if search.run.total < search.best.0 - 0.5 {
                search.best = (search.run.total, search.route.clone());
                on_best(search.run.total, search.run.play, &search.route);
            }
        }
    }

    /// Draw a move (large neighbourhood or small), evaluate it and keep it or not.
    fn try_a_move(&self, search: &mut Search, on_best: &impl Fn(f64, f64, &[Stop])) {
        let mut candidate = search.route.clone();
        let mut operator = None;
        let change = if self.params.alns > 0.0 && search.rng.random_bool(self.params.alns) {
            let op = search.operators.pick(&mut search.rng);
            operator = Some(op);
            self.ruin_recreate(&mut candidate, &search.run, op, &mut search.rng)
                .map(|(first, last, reordered)| Change { first, last, reordered })
        } else {
            self.random_move(&mut candidate, &search.run, &mut search.rng)
        };
        let Some(change) = change else { return };
        // The start chosen (`Params::start_zone`) is kept: the route begins in that zone.
        if self.begins_in_start_zone(&search.route) && !self.begins_in_start_zone(&candidate) {
            return;
        }
        // Replayed up to a window past the change, the rest estimated; to the end only when the
        // estimate would keep it (and always when it changes the dungeons run).
        let dungeons = |r: &[Stop]| r.iter().filter(|s| s.kind == Kind::Dungeon).count();
        // The last stop changed: past it, both routes end alike (the moves that edit a route
        // only say it changes up to its end).
        let same_end = (candidate.iter().rev())
            .zip(search.route.iter().rev())
            .take_while(|(x, y)| x == y)
            .count();
        let last = change.last.min(candidate.len() - same_end);
        let window = (self.params.eval_window > 0 && self.params.reputation_goals.is_empty())
            .then(|| last + 1 + self.params.eval_window)
            .filter(|_| dungeons(&candidate) == dungeons(&search.route));
        let Some((estimate, exact)) = self.evaluate_change(&search.run, &candidate, change, window) else {
            return;
        };
        let current = search.run.total;
        let (progress, tries) = (search.progress(), search.tries);
        let mut draw = None;
        let mut accepts = |cost: f64, rng: &mut SmallRng| {
            search
                .acceptance
                .accepts(cost, current, progress, tries, &mut draw, rng)
        };
        let cost = if !accepts(estimate, &mut search.rng) {
            None
        } else if exact {
            Some(estimate)
        } else {
            self.evaluate_change(&search.run, &candidate, change, None)
                .map(|(cost, _)| cost)
                .filter(|&cost| accepts(cost, &mut search.rng))
        };
        if let Some(cost) = cost
            && let Some(run) = self.replace_run(&mut search.run, &search.route, &candidate)
        {
            let improves = cost < current - 0.5;
            search.run = run;
            search.route = candidate;
            search.wins += u64::from(improves);
            if search.run.total < search.best.0 - 0.5 {
                search.best = (search.run.total, search.route.clone());
                on_best(search.run.total, search.run.play, &search.route);
                if let Some(op) = operator {
                    search.operators.reward(op, 3.0);
                }
            } else if let (Some(op), true) = (operator, improves) {
                search.operators.reward(op, 1.0);
            }
        }
        search.acceptance.remember(search.run.total, search.tries);
        if operator.is_some() {
            search.operators.adapt();
        }
    }

    /// Whether the route begins in `Params::start_zone` (always when none is chosen).
    fn begins_in_start_zone(&self, route: &[Stop]) -> bool {
        let zone = self.params.start_zone;
        zone == 0 || route.first().is_none_or(|&stop| self.places().zone(stop) == zone)
    }

    /// Cost of a changed route, incremental (checked against a full replay when verifying).
    fn evaluate_change(
        &self,
        run: &Run,
        candidate: &[Stop],
        change: Change,
        window: Option<usize>,
    ) -> Option<(f64, bool)> {
        let evaluated = self.evaluate_within(run, candidate, change.first, change.last, change.reordered, window);
        if self.verify && evaluated.is_none_or(|(_, exact)| exact) {
            let cost = evaluated.map(|(cost, _)| cost);
            let full = self.simulate(candidate, None);
            let ok = match (cost, full) {
                (Some(x), Some(y)) => (x - y).abs() < 1e-6,
                (None, None) => true,
                _ => false,
            };
            assert!(
                ok,
                "incremental {cost:?} != full {full:?} ({}..{}, reordered {}): {:?}",
                change.first,
                change.last,
                change.reordered,
                self.diagnose(candidate)
            );
        }
        evaluated
    }

    /// The best route found, with the pet quests early again, the quests never turned in dropped,
    /// the stops after the target level cut and the dungeon quests without their run dropped.
    /// Dropping unfinished quests and cutting are kept only when they do not make the route worse
    /// (the dungeon quests go anyway); moving a pet quest early may, by up to `class_power_slack`
    /// each (on purpose: a pet or a form early is worth more to the player than the model says).
    /// Its score is returned with it.
    fn clean_up(&self, search: Search) -> (Vec<Stop>, f64) {
        // Only the score and the stops done of each route are kept: the states of a run weigh
        // megabytes, and the clean-up replays routes of its own.
        let done = |run: Run| (run.total, run.states.len() - 1);
        let Search { route, run, best, .. } = search;
        let (mut route, mut run) = (route, done(run));
        if best.0 < run.0 - 0.5 {
            route = best.1;
            run = done(self.run(&route).expect("best route was valid"));
        }
        let early = self.power_first(route.clone());
        if early != route {
            route = early;
            run = done(self.run(&route).expect("power_first keeps the route valid"));
        }
        let rebound = self.place_hearths(self.rebind(route.clone()));
        if rebound != route {
            route = rebound;
            run = done(self.run(&route).expect("rebind keeps the route valid"));
        }
        if let Some(pruned) = self.drop_unfinished(&route, run.0) {
            route = pruned;
            run = done(self.run(&route).expect("pruned route was valid"));
        }
        // The stops after the target level are cut, unless the route needs them: the quests it
        // plans later decide what is taken early and turned in at hand before (not a quest
        // blocking one it takes later; an XP reward lost to grinding instead).
        let cut = route[..run.1].to_vec();
        if let Some(r) = self.run(&cut)
            && r.total <= run.0 + 0.5
        {
            (route, run) = (cut, done(r));
        }
        if let Some(kept) = self.drop_dungeonless(&route) {
            route = kept;
            run = done(self.run(&route).expect("dropped route was valid"));
        }
        (route, run.0)
    }

    /// The route without the dungeon quests it takes but whose dungeon it never runs (its run
    /// cut past the target level, or never planned): they cannot be finished, so they go
    /// whatever they save. Those another quest needs stay. None when there is nothing to remove.
    pub(crate) fn drop_dungeonless(&self, route: &[Stop]) -> Option<Vec<Stop>> {
        let runs: HashSet<u32> = (route.iter())
            .filter(|s| s.kind == Kind::Dungeon)
            .map(|s| s.index)
            .collect();
        let mut left: Vec<u32> = (route.iter())
            .filter(|s| s.kind == Kind::Accept && !self.quest(s.index).mandatory)
            .filter(|s| {
                (self.quest(s.index).objectives.iter()).any(|o| o.dungeon.is_some_and(|d| !runs.contains(&(d as u32))))
            })
            .map(|s| s.index)
            .collect();
        left.dedup();
        let (mut best, mut changed) = (route.to_vec(), false);
        for q in left {
            let candidate: Vec<Stop> = (best.iter().copied())
                .filter(|s| !(s.is_quest() && s.index == q))
                .collect();
            if self.simulate(&candidate, None).is_some() {
                (best, changed) = (candidate, true);
            }
        }
        changed.then_some(best)
    }

    /// The run of `candidate`, built once the states of `run` (of `route`) are released: two
    /// runs of a long route weigh tens of megabytes. Called for a candidate whose evaluation
    /// just succeeded, so it seldom fails; when it does, `run` is built again.
    pub(crate) fn replace_run(&self, run: &mut Run, route: &[Stop], candidate: &[Stop]) -> Option<Run> {
        run.states = Vec::new();
        let new = self.run(candidate);
        if new.is_none() {
            *run = self.run(route).expect("the current route was valid");
        }
        new
    }

    /// The route without the quests it takes (or finds in the character's log) but never turns
    /// in (given up, or still in the log at the end) when that costs nothing: taking them costs
    /// only seconds, so the search leaves them, but a guide should not ask for them. Those
    /// another quest needs stay. None when there is nothing to remove.
    pub(crate) fn drop_unfinished(&self, route: &[Stop], score: f64) -> Option<Vec<Stop>> {
        let mut trace = Vec::new();
        self.simulate_full(route, Some(&mut trace))?;
        let turned: HashSet<u32> = trace
            .iter()
            .filter_map(|t| match &t.event {
                Event::Stop { stop, .. } if stop.kind == Kind::TurnIn => Some(stop.index),
                _ => None,
            })
            .collect();
        let taken = |s: &Stop| s.kind == Kind::Accept || (s.is_quest() && self.in_log_at_start(s.index));
        let mut seen = HashSet::new();
        let unfinished: Vec<u32> = route
            .iter()
            .filter(|s| taken(s) && !turned.contains(&s.index) && !self.quest(s.index).mandatory)
            .map(|s| s.index)
            .filter(|&q| seen.insert(q))
            .collect();
        let (mut best, mut score, mut changed) = (route.to_vec(), score, false);
        for q in unfinished {
            let candidate: Vec<Stop> = best
                .iter()
                .copied()
                .filter(|s| !(s.is_quest() && s.index == q))
                .collect();
            if let Some(cost) = self.simulate(&candidate, None)
                && cost <= score + 1e-6
            {
                (best, score, changed) = (candidate, cost, true);
            }
        }
        changed.then_some(best)
    }

    /// The stays of a route: runs of consecutive stops in one zone (first, last index, zone).
    pub(crate) fn stays(&self, route: &[Stop]) -> Vec<(usize, usize, i64)> {
        let mut out: Vec<(usize, usize, i64)> = Vec::new();
        for (i, st) in route.iter().enumerate() {
            let z = self.places().zone(*st);
            match out.last_mut() {
                Some(last) if last.2 == z => last.1 = i,
                _ => out.push((i, i, z)),
            }
        }
        out
    }
}
