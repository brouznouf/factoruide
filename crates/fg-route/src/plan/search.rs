//! Improving a route: a time-boxed local search trying small moves and large neighbourhood
//! moves, keeping the best route, then a clean-up.

use super::Planner;
use super::acceptance::Acceptance;
use super::alns::Operators;
use super::evaluate::Run;
use super::moves::Change;
use super::state::{Event, Kind, Stop, fmt_time};
use rand::rngs::SmallRng;
use rand::{Rng, SeedableRng};
use std::collections::HashSet;
use std::time::Duration;
use std::time::Instant;

/// Where the search stands: the current route, the best one, how it decides.
struct Search {
    route: Vec<Stop>,
    run: Run,
    best: (f64, Vec<Stop>),
    rng: SmallRng,
    acceptance: Acceptance,
    operators: Operators,
    started: Instant,
    budget: Duration,
    tries: u64,
    wins: u64,
}

impl Search {
    fn progress(&self) -> f64 {
        self.started.elapsed().as_secs_f64() / self.budget.as_secs_f64().max(1e-9)
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
            budget,
            tries: 0,
            wins: 0,
        };
        while search.started.elapsed() < budget && search.route.len() > 2 {
            search.tries += 1;
            self.try_a_move(&mut search, &on_best);
        }
        let (tries, wins) = (search.tries, search.wins);
        let (route, run) = self.clean_up(search);
        log(&format!(
            "improved: {} stops, {} ({tries} tries, {wins} improvements)",
            route.len(),
            fmt_time(run.total)
        ));
        route
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
            self.random_move(&mut candidate, &mut search.rng)
        };
        let Some(change) = change else { return };
        let Some(cost) = self.evaluate_change(&search.run, &candidate, change) else {
            return;
        };
        let current = search.run.total;
        let improves = cost < current - 0.5;
        if search
            .acceptance
            .accepts(cost, current, search.progress(), search.tries, &mut search.rng)
            && let Some(run) = self.run(&candidate)
        {
            search.run = run;
            search.route = candidate;
            search.wins += 1;
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

    /// Cost of a changed route, incremental (checked against a full replay when verifying).
    fn evaluate_change(&self, run: &Run, candidate: &[Stop], change: Change) -> Option<f64> {
        let evaluated = self.evaluate(run, candidate, change.first, change.last, change.reordered);
        if self.verify {
            let full = self.simulate(candidate, None);
            let ok = match (evaluated, full) {
                (Some(x), Some(y)) => (x - y).abs() < 1e-6,
                (None, None) => true,
                _ => false,
            };
            assert!(
                ok,
                "incremental {evaluated:?} != full {full:?} ({}..{}, reordered {}): {:?}",
                change.first,
                change.last,
                change.reordered,
                self.diagnose(candidate)
            );
        }
        evaluated
    }

    /// The best route found, with the pet quests early again (`class_power_slack`), the quests
    /// never turned in dropped and the stops after the target level cut, each change kept only
    /// when it does not make the route worse.
    fn clean_up(&self, search: Search) -> (Vec<Stop>, Run) {
        let (mut route, mut run) = (search.route, search.run);
        if search.best.0 < run.total - 0.5 {
            route = search.best.1;
            run = self.run(&route).expect("best route was valid");
        }
        let early = self.power_first(route.clone());
        if early != route {
            route = early;
            run = self.run(&route).expect("power_first keeps the route valid");
        }
        if let Some(pruned) = self.drop_unfinished(&route, run.total) {
            route = pruned;
            run = self.run(&route).expect("pruned route was valid");
        }
        // The stops after the target level are cut, unless the route needs them: the quests it
        // plans later decide what is taken early and turned in at hand before (not a quest
        // blocking one it takes later; an XP reward lost to grinding instead).
        let cut = route[..run.states.len() - 1].to_vec();
        if let Some(r) = self.run(&cut)
            && r.total <= run.total + 0.5
        {
            (route, run) = (cut, r);
        }
        (route, run)
    }

    /// The route without the quests it takes but never turns in (given up, or still in the log at
    /// the end) when that costs nothing: taking them costs only seconds, so the search leaves
    /// them, but a guide should not ask for them. Those another quest needs stay. None when
    /// there is nothing to remove.
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
        let mut unfinished: Vec<u32> = route
            .iter()
            .filter(|s| s.kind == Kind::Accept && !turned.contains(&s.index) && !self.quest(s.index).mandatory)
            .map(|s| s.index)
            .collect();
        unfinished.dedup();
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
