//! Choosing the best route: candidate routes (each its own seed and construction settings) are
//! built and improved briefly in a race, then the best are improved in parallel until the end of
//! the budget. The app follows it live (`@progress` and `@route` lines).

use crate::model::{Model, Profile};
use crate::params::{Construction, Params};
use crate::plan::{Planner, Stop, fmt_time};
use crate::world::World;
use rand::{Rng, SeedableRng};
use std::cell::Cell;
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::sync::{Mutex, OnceLock};
use std::time::{Duration, Instant};

type Progress<'p> = &'p (dyn Fn(&str) + Sync);

/// Build routes and improve them with parallel searches (`params.time_limit_ms`,
/// `params.threads`) within `params.time_limit_ms`, after a race of candidates taking
/// `qualify_share` of it. The best wins.
pub fn optimize(planner: &Planner<'_>, progress: Progress<'_>) -> Vec<Stop> {
    let plan = RacePlan::new(planner.params);
    progress(&plan.announce());
    let live = Live::new(&plan, progress);
    let searcher = Searcher {
        model: planner.model,
        world: planner.world,
        params: planner.params,
        profile: planner.profile,
        live: &live,
        quiet: plan.race,
        progress,
    };
    // The budget counts from here: the first routes' construction is part of it.
    live.started.get_or_init(Instant::now);
    let results = std::thread::scope(|scope| {
        scope.spawn(|| live.watch(&searcher));
        let finalists = if plan.race {
            qualify(&searcher, &plan)
        } else {
            (0..plan.threads).map(|k| (k, None)).collect()
        };
        let results = final_round(&searcher, &plan, finalists);
        live.done.store(true, Ordering::Relaxed);
        results
    });
    live.report();
    progress(&summary(planner, &results));
    results
        .into_iter()
        .min_by(|a, b| a.0.total_cmp(&b.0))
        .map(|(_, r)| r)
        .unwrap_or_default()
}

/// How the time is shared: candidates, threads, qualification and whole budget.
struct RacePlan {
    threads: usize,
    candidates: usize,
    race: bool,
    qualify: Duration,
    budget: Duration,
    /// Routes kept for the final.
    kept: usize,
}

impl RacePlan {
    fn new(params: &Params) -> Self {
        let threads = params.threads.max(1);
        let total = Duration::from_millis(params.time_limit_ms);
        // At least ~2 s of qualification per candidate (building a route takes about one).
        let qualify = total.mul_f64(params.qualify_share.clamp(0.0, 0.9));
        let max_rounds = ((qualify.as_secs_f64() / 2.0) as usize).max(1);
        let candidates = params.candidates.clamp(threads, threads * max_rounds);
        let race = !qualify.is_zero() && candidates > threads;
        Self {
            threads,
            candidates,
            race,
            qualify: if race { qualify } else { Duration::ZERO },
            budget: total,
            kept: params.finalists.clamp(1, candidates).max(threads.min(candidates)),
        }
    }

    fn announce(&self) -> String {
        let searches = format!("{} searches", self.threads);
        if self.race {
            format!(
                "Optimizing ({} s: a {} s race of {} candidates, then {searches})",
                self.budget.as_secs(),
                self.qualify.as_secs(),
                self.candidates
            )
        } else {
            format!("Optimizing ({} s, {searches})", self.budget.as_secs())
        }
    }
}

/// Construction settings of race candidate `k`: its seed, and (with `construction_random`)
/// varied greedy weights.
fn candidate_params(params: &Params, k: usize) -> Params {
    let mut p = params.clone();
    p.seed = params.seed.wrapping_add(k as u64);
    if params.construction == Construction::Mixed {
        p.construction = if k % 2 == 1 {
            Construction::Regions
        } else {
            Construction::Greedy
        };
    }
    if params.construction_random && k > 0 && p.construction == Construction::Greedy {
        let mut rng = rand::rngs::SmallRng::seed_from_u64(p.seed ^ 0x9e37_79b9_7f4a_7c15);
        p.zone_stay = rng.random_range(1.5..=3.5);
        p.continent_stay = rng.random_range(1.0..=4.0);
        p.dungeon_quest_weight = rng.random_range(0.25..=1.0);
        p.fit_penalty = rng.random_range(0.0..=0.4);
        p.fit_above_penalty = rng.random_range(0.0..=0.6);
        p.efficiency_power = rng.random_range(0.0..=0.6);
    }
    p
}

/// What the app sees while the race runs. Routes are ranked by score, which weighs grinding,
/// flights and zone changes on top of the play time.
struct Live<'p> {
    progress: Progress<'p>,
    race: bool,
    candidates: usize,
    kept: usize,
    budget: Duration,
    qualify: Duration,
    /// When the final began (seconds into the optimization).
    final_at: OnceLock<f64>,
    /// Time spent building candidate routes, and how many were built.
    built: Mutex<(Duration, u32)>,
    /// Best (score, play time) of each candidate's route so far.
    routes: Mutex<Vec<Option<(f64, f64)>>>,
    /// Candidates in the final.
    finalists: Mutex<Vec<usize>>,
    started: OnceLock<Instant>,
    /// (best start score, its play time, best score, its play time)
    scores: Mutex<(f64, f64, f64, f64)>,
    /// Best route so far, and whether it changed since last drawn.
    best_route: Mutex<(Vec<Stop>, bool)>,
    done: AtomicBool,
    qualified: AtomicUsize,
    final_stage: AtomicBool,
}

impl<'p> Live<'p> {
    fn new(plan: &RacePlan, progress: Progress<'p>) -> Self {
        Self {
            progress,
            race: plan.race,
            candidates: plan.candidates,
            kept: plan.kept,
            budget: plan.budget,
            qualify: plan.qualify,
            final_at: OnceLock::new(),
            built: Mutex::new((Duration::ZERO, 0)),
            routes: Mutex::new(vec![None; plan.candidates.max(plan.threads)]),
            finalists: Mutex::new(if plan.race {
                Vec::new()
            } else {
                (0..plan.threads).collect()
            }),
            started: OnceLock::new(),
            scores: Mutex::new((f64::INFINITY, f64::INFINITY, f64::INFINITY, f64::INFINITY)),
            best_route: Mutex::new((Vec::new(), false)),
            done: AtomicBool::new(false),
            qualified: AtomicUsize::new(0),
            final_stage: AtomicBool::new(!plan.race),
        }
    }

    /// Report every 300 ms until the race is over.
    fn watch(&self, searcher: &Searcher<'_>) {
        let viewer = Planner::new(searcher.model, searcher.world, searcher.params, searcher.profile);
        let mut last_route = Instant::now();
        while !self.done.load(Ordering::Relaxed) {
            std::thread::sleep(Duration::from_millis(300));
            self.report();
            self.report_route(&mut last_route, &viewer);
        }
    }

    /// The routes followed: while qualifying the best `kept` candidates so far, in the final the
    /// finalists. Each as {id, time (play), score}; `selected` is the best score (the route kept).
    fn followed(&self) -> (Vec<serde_json::Value>, Option<usize>) {
        let routes = self.routes.lock().unwrap();
        let ids: Vec<usize> = if self.final_stage.load(Ordering::Relaxed) {
            self.finalists.lock().unwrap().clone()
        } else {
            let mut ranked: Vec<usize> = (0..routes.len()).filter(|&k| routes[k].is_some()).collect();
            ranked.sort_by(|&a, &b| routes[a].unwrap().0.total_cmp(&routes[b].unwrap().0));
            ranked.truncate(self.kept);
            ranked
        };
        let shown: Vec<(usize, f64, f64)> = ids
            .into_iter()
            .filter_map(|k| routes.get(k).copied().flatten().map(|(score, play)| (k, score, play)))
            .filter(|(_, score, _)| score.is_finite())
            .collect();
        let selected = shown.iter().min_by(|a, b| a.1.total_cmp(&b.1)).map(|r| r.0);
        let list = shown
            .iter()
            .map(|&(id, score, play)| serde_json::json!({ "id": id, "time": play, "score": score }))
            .collect();
        (list, selected)
    }

    /// `@progress {json}`: phase, elapsed, budget, play time of the best start and best route,
    /// and the routes followed (see `followed`).
    fn report(&self) {
        let (_, start, _, best) = *self.scores.lock().unwrap();
        let mut msg = match self.started.get() {
            None => serde_json::json!({ "phase": "construct" }),
            Some(t) => serde_json::json!({
                "phase": "optimize",
                "elapsed": t.elapsed().as_secs_f64(),
                "budget": self.budget.as_secs_f64(),
                "start": if start.is_finite() { Some(start) } else { None },
                "best": if best.is_finite() { Some(best) } else { None },
            }),
        };
        if self.started.get().is_some() {
            let (routes, selected) = self.followed();
            msg["routes"] = routes.into();
            msg["selected"] = selected.into();
        }
        if self.race {
            msg["stage"] = if self.final_stage.load(Ordering::Relaxed) {
                "final"
            } else {
                "qualify"
            }
            .into();
            msg["candidates"] = self.candidates.into();
            msg["qualified"] = self.qualified.load(Ordering::Relaxed).into();
            msg["finalists"] = self.kept.into();
            msg["qualify"] = self
                .final_at
                .get()
                .copied()
                .unwrap_or(self.qualify.as_secs_f64())
                .into();
        }
        (self.progress)(&format!("@progress {msg}"));
    }

    /// `@route {points}` for live drawing, when the best route changed (at most every 1.5 s).
    fn report_route(&self, last: &mut Instant, planner: &Planner<'_>) {
        if last.elapsed() < Duration::from_millis(1500) {
            return;
        }
        let route = {
            let mut b = self.best_route.lock().unwrap();
            if !b.1 {
                return;
            }
            b.1 = false;
            b.0.clone()
        };
        *last = Instant::now();
        let points = planner.places().polyline(&route, 800);
        (self.progress)(&format!("@route {}", serde_json::json!({ "points": points })));
    }

    fn on_best(&self, k: usize, first: &Cell<bool>, score: f64, play: f64, route: &[Stop]) {
        if let Some(r) = self.routes.lock().unwrap().get_mut(k) {
            *r = Some((score, play));
        }
        let mut s = self.scores.lock().unwrap();
        if first.replace(false) && score < s.0 {
            (s.0, s.1) = (score, play);
        }
        if score < s.2 {
            (s.2, s.3) = (score, play);
            *self.best_route.lock().unwrap() = (route.to_vec(), true);
        }
    }
}

/// Builds and improves the route of one candidate.
struct Searcher<'a> {
    model: &'a Model,
    world: &'a World,
    params: &'a Params,
    profile: &'a Profile,
    live: &'a Live<'a>,
    /// During a race the candidates do not log their progress.
    quiet: bool,
    progress: Progress<'a>,
}

impl Searcher<'_> {
    /// Improve `route` (built when None) with candidate `k`'s settings until `until` after the
    /// start of the optimization. Returns its score under the request's own settings.
    fn search(&self, k: usize, route: Option<Vec<Stop>>, until: Duration) -> (f64, Vec<Stop>) {
        let p = candidate_params(self.params, k);
        let planner = Planner::new(self.model, self.world, &p, self.profile);
        let log = |m: &str| {
            if k == 0 && !self.quiet {
                (self.progress)(m);
            }
        };
        let first = Cell::new(route.is_none());
        let start = route.unwrap_or_else(|| {
            let begun = Instant::now();
            let route = planner.construct(log);
            let mut built = self.live.built.lock().unwrap();
            built.0 += begun.elapsed();
            built.1 += 1;
            route
        });
        let begun = *self.live.started.get_or_init(Instant::now);
        let route = planner.improve(
            start,
            until.saturating_sub(begun.elapsed()),
            log,
            |score, play, route| {
                self.live.on_best(k, &first, score, play, route);
            },
        );
        let base = Planner::new(self.model, self.world, self.params, self.profile);
        (base.simulate(&route, None).unwrap_or(f64::INFINITY), route)
    }
}

/// Qualification: every candidate gets the same short search, `threads` at a time, within
/// `plan.qualify`: a new candidate starts only when its route can be built in the time left
/// (building takes as long as it took the others; `plan.candidates` is a maximum). The best
/// `kept` go to the final.
fn qualify(searcher: &Searcher<'_>, plan: &RacePlan) -> Vec<(usize, Option<Vec<Stop>>)> {
    let rounds = plan.candidates.div_ceil(plan.threads) as u32;
    let slot = plan.qualify / rounds;
    let next = AtomicUsize::new(0);
    let qualified: Mutex<Vec<(f64, usize, Vec<Stop>)>> = Mutex::new(Vec::new());
    let elapsed = || searcher.live.started.get().map_or(Duration::ZERO, Instant::elapsed);
    std::thread::scope(|scope| {
        for _ in 0..plan.threads {
            scope.spawn(|| {
                loop {
                    let k = next.fetch_add(1, Ordering::Relaxed);
                    if k >= plan.candidates {
                        break;
                    }
                    let (time, n) = *searcher.live.built.lock().unwrap();
                    let building = time / n.max(1);
                    if k >= plan.threads && elapsed() + building > plan.qualify {
                        break;
                    }
                    // The thread's last candidate (no time to build another after it) improves
                    // until the end of the qualification.
                    let last = n > 0 && elapsed() + building * 2 > plan.qualify;
                    let round = (k / plan.threads) as u32 + 1;
                    let until = if last {
                        plan.qualify
                    } else {
                        (slot * round).min(plan.qualify)
                    };
                    let (score, route) = searcher.search(k, None, until);
                    qualified.lock().unwrap().push((score, k, route));
                    searcher.live.qualified.fetch_add(1, Ordering::Relaxed);
                }
            });
        }
    });
    let mut q = qualified.into_inner().unwrap();
    q.sort_by(|a, b| a.0.total_cmp(&b.0));
    if std::env::var_os("FG_RACE_LOG").is_some() {
        for (score, k, _) in &q {
            (searcher.progress)(&format!("candidate {k}: {}", fmt_time(*score)));
        }
    }
    let kept: Vec<(usize, Option<Vec<Stop>>)> = q.into_iter().take(plan.kept).map(|(_, k, r)| (k, Some(r))).collect();
    *searcher.live.finalists.lock().unwrap() = kept.iter().map(|(k, _)| *k).collect();
    searcher.live.final_at.get_or_init(|| elapsed().as_secs_f64());
    searcher.live.final_stage.store(true, Ordering::Relaxed);
    kept
}

/// Final: the kept routes improved until the end of the budget.
fn final_round(
    searcher: &Searcher<'_>,
    plan: &RacePlan,
    finalists: Vec<(usize, Option<Vec<Stop>>)>,
) -> Vec<(f64, Vec<Stop>)> {
    let results: Vec<(usize, (f64, Vec<Stop>))> = std::thread::scope(|scope| {
        let handles: Vec<_> = finalists
            .into_iter()
            .map(|(k, route)| scope.spawn(move || (k, searcher.search(k, route, plan.budget))))
            .collect();
        handles.into_iter().map(|h| h.join().unwrap()).collect()
    });
    if std::env::var_os("FG_RACE_LOG").is_some() {
        for (k, (t, _)) in &results {
            (searcher.progress)(&format!("final {k}: {}", fmt_time(*t)));
        }
    }
    results.into_iter().map(|(_, r)| r).collect()
}

/// Play time of each search's route (they are ranked by score).
fn summary(planner: &Planner<'_>, results: &[(f64, Vec<Stop>)]) -> String {
    let times: Vec<String> = results
        .iter()
        .map(|(_, r)| {
            if let Some((_, b)) = planner.simulate_full(r, None) {
                return fmt_time(b.total);
            }
            if std::env::var_os("FG_RACE_LOG").is_some()
                && let Some((n, why)) = planner.diagnose(r)
            {
                eprintln!("invalid route ({} stops) at stop {n}: {why}", r.len());
            }
            "-".into()
        })
        .collect();
    format!("{} searches: {}", results.len(), times.join(" "))
}
