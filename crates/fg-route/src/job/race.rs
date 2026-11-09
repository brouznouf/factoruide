//! Choosing the best route, as a tournament counted in tries (the same guide on any machine, only
//! slower with fewer processors): candidate routes (each its own seed and construction settings)
//! are built and improved briefly, and the best `threads` go on, one search each. After each round
//! the better half goes on with twice the searches each, those of a route sharing their best
//! (`share_every`), down to the `final_routes` of the final. The best route wins. The app follows
//! it live (`@progress` and `@route` lines).

use super::Interrupt;
use super::{JobEvent, Watch};
use crate::diagnostics::Diagnostics;
use crate::model::{Model, Profile};
use crate::params::{Construction, Params};
use crate::plan::{Effort, Exchange, Member, Planner, Stop, ZoneChange, fmt_time};
use crate::sync::{Joined, LockExt};
use crate::world::World;
use rand::{RngExt, SeedableRng};
use std::cell::Cell;
use std::sync::atomic::{AtomicBool, AtomicU32, AtomicU64, AtomicUsize, Ordering};
use std::sync::{Mutex, OnceLock};
use std::time::{Duration, Instant};

type Progress<'p> = Watch<'p>;

/// Build routes and improve them in a tournament of `params.threads` searches (see the module).
/// The best wins.
pub fn optimize(planner: &Planner<'_>, progress: Progress<'_>) -> Vec<Stop> {
    optimize_interruptible(planner, progress, &Interrupt::default())
}

/// `optimize`, ended early by `interrupt`: no new candidate starts (but the first ones, so that
/// there is a route), the searches end at their next move and no new round begins. Stopped, the
/// best route so far is returned; cancelled, the caller throws it away.
pub fn optimize_interruptible(planner: &Planner<'_>, progress: Progress<'_>, interrupt: &Interrupt) -> Vec<Stop> {
    let plan = Tournament::new(planner.params);
    progress(&JobEvent::Status(&plan.announce()));
    let live = Live::new(&plan, planner.params, progress);
    let starts: Vec<i64> = if plan.race && planner.params.construction_random && planner.params.forced_starts > 0 {
        let mut zones: Vec<i64> = planner.start_zones().into_iter().map(|(z, _)| z).collect();
        zones.truncate(planner.params.forced_starts);
        zones
    } else {
        Vec::new()
    };
    let searcher = Searcher {
        model: planner.model,
        world: planner.world,
        params: planner.params,
        profile: planner.profile,
        diagnostics: planner.diagnostics,
        live: &live,
        progress,
        interrupt,
        mutants: &[],
        starts: &starts,
    };
    if planner.diagnostics.race {
        progress(&JobEvent::Status(&format!("forced starts: {:?}", searcher.starts)));
    }
    let ranked = std::thread::scope(|scope| {
        scope.spawn(|| {
            let _release = crate::threads::Release;
            live.watch(&searcher);
        });
        // The watcher ends with the searches, also when one panics: the scope waits for it
        // before passing the panic on, the job would hang.
        let _done = Done(&live.done);
        let (mut ranked, mutants) = if plan.race {
            qualify(&searcher, &plan)
        } else {
            (Vec::new(), Vec::new())
        };
        // The rounds go on with the mutants of the race among their routes.
        let searcher = Searcher {
            mutants: &mutants,
            ..searcher
        };
        for (stage, &width) in plan.stages.iter().enumerate() {
            let routes: Vec<(usize, Option<Vec<Stop>>)> = if stage == 0 && !plan.race {
                (0..width).map(|k| (k, None)).collect()
            } else {
                if interrupt.stopping() || ranked.is_empty() {
                    break;
                }
                ranked
                    .iter()
                    .take(width)
                    .map(|(_, k, r)| (*k, Some(r.clone())))
                    .collect()
            };
            ranked = round(&searcher, &plan, stage, &routes);
        }
        ranked
    });
    live.report();
    if !interrupt.cancelled() {
        progress(&JobEvent::Status(&summary(planner, &ranked)));
    }
    ranked.into_iter().next().map(|(_, _, r)| r).unwrap_or_default()
}

/// Sets its flag when dropped, unwinding included.
struct Done<'a>(&'a AtomicBool);

impl Drop for Done<'_> {
    fn drop(&mut self) {
        self.0.store(true, Ordering::Relaxed);
    }
}

/// How the tries are shared: the race of candidates, then the routes of each round.
struct Tournament {
    threads: usize,
    /// Candidates drawing their weights, then the mutants of the best of them.
    candidates: usize,
    mutants: usize,
    /// Candidates improved in the race (`Params::screen`).
    qualifying: usize,
    race: bool,
    /// Routes of each round: `threads`, then half as many each time, the last one the final.
    stages: Vec<usize>,
}

impl Tournament {
    fn new(params: &Params) -> Self {
        let threads = params.threads.max(1);
        // A low effort races fewer candidates: a preview does not build 64 routes.
        let candidates = ((params.candidates as f64 * params.effort.min(1.0)).round() as usize).max(threads);
        let race = candidates > threads;
        let mutants = if race {
            (params.race_mutants as f64 * params.effort.min(1.0)).round() as usize
        } else {
            0
        };
        let qualifying = match params.screen {
            0 => candidates + mutants,
            screen => screen.max(threads).min(candidates + mutants),
        };
        let last = params.final_routes.clamp(1, threads);
        let mut stages = vec![threads];
        while let Some(&n) = stages.last()
            && n > last
        {
            stages.push((n / 2).max(last));
        }
        // Rounds without tries would only rank the routes: straight to the final (without a race
        // the first round still builds them).
        if params.round_tries <= 0.0 {
            stages = if race || threads == last {
                vec![last]
            } else {
                vec![threads, last]
            };
        }
        Self {
            threads,
            candidates,
            mutants,
            qualifying,
            race,
            stages,
        }
    }

    /// Searches improving each route of round `stage` (as even as can be).
    fn searches(&self, stage: usize, route: usize) -> usize {
        let width = self.stages[stage];
        self.threads / width + usize::from(route < self.threads % width)
    }

    fn is_final(&self, stage: usize) -> bool {
        stage + 1 == self.stages.len()
    }

    /// How many tries the tournament makes (to show its progress).
    fn tries(&self, params: &Params) -> Tries {
        let rounds = (self.stages.len() - 1) as f64;
        Tries {
            candidates: if self.race { self.qualifying as f64 } else { 0.0 },
            qualify: params.qualify_tries,
            qualify_min: params.qualify_min_tries,
            after: self.threads as f64 * (rounds * params.round_tries + params.final_tries),
            effort: params.effort,
        }
    }

    fn announce(&self) -> String {
        let rounds: Vec<String> = self.stages.iter().map(ToString::to_string).collect();
        let rounds = format!("{} routes, {} searches", rounds.join(" -> "), self.threads);
        if self.race {
            format!(
                "Optimizing (a race of {} candidates, {} improved, then {rounds})",
                self.candidates + self.mutants,
                self.qualifying
            )
        } else {
            format!("Optimizing ({rounds})")
        }
    }
}

/// Tries of a tournament (see `Tournament::tries`).
#[derive(Clone, Copy)]
struct Tries {
    /// Candidates of the race (0: no race), their tries per stop and at least.
    candidates: f64,
    qualify: f64,
    qualify_min: f64,
    /// Tries per stop of all the searches after the race.
    after: f64,
    effort: f64,
}

impl Tries {
    /// Tries of a search of `per_stop` tries per stop of its `stops`, at least `min`.
    fn of(per_stop: f64, min: f64, stops: f64, effort: f64) -> u64 {
        ((per_stop * stops).max(min) * effort).round() as u64
    }

    /// Tries of the race and of the whole tournament, for routes of `stops` stops.
    fn at(&self, stops: f64) -> (f64, f64) {
        let race = self.candidates * Self::of(self.qualify, self.qualify_min, stops, self.effort) as f64;
        (race, race + self.after * stops * self.effort)
    }
}

/// Construction settings of race candidate `k`: its seed, and (with `construction_random`)
/// varied greedy weights. A set of weights gives one starting route (the seed plays no part in
/// it), and a slight change can switch a single choice (when to leave a zone or a continent)
/// that changes the whole route by hours: no weights are best everywhere, the race draws many.
/// The ranges are where the routes are better on average, on the six classic starts: zones
/// finished (zone_stay high), no penalty for low quests, no chase of quest XP per second.
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
        p.zone_stay = rng.random_range(3.5..=5.0);
        p.zone_stay_quests = rng.random_range(3.0..=12.0);
        p.continent_stay = rng.random_range(1.0..=4.0);
        p.dungeon_quest_weight = rng.random_range(0.1..=0.6);
        p.fit_penalty = rng.random_range(0.0..=0.15);
        p.fit_above_penalty = rng.random_range(0.2..=0.8);
        p.efficiency_power = rng.random_range(0.0..=0.2);
    }
    p
}

/// What the app sees while the tournament runs. Routes are ranked by score, which weighs
/// grinding, flights and zone changes on top of the play time.
struct Live<'p> {
    progress: Progress<'p>,
    race: bool,
    candidates: usize,
    threads: usize,
    /// Routes built before the race (`screen`), the first ones drawing their weights.
    to_build: usize,
    draws: usize,
    stages: Vec<usize>,
    /// How many tries the tournament makes, for a route size.
    tries: Tries,
    /// How many candidate routes were built, and their stops.
    built: AtomicU32,
    built_stops: AtomicUsize,
    /// Tries of all the searches so far.
    tried: AtomicU64,
    /// Best (score, play time) of each candidate's route so far.
    routes: Mutex<Vec<Option<(f64, f64)>>>,
    /// The round under way (none during the race) and its routes.
    round: Mutex<Option<(usize, Vec<usize>)>>,
    started: OnceLock<Instant>,
    best: Mutex<Best>,
    done: AtomicBool,
    qualified: AtomicUsize,
}

impl<'p> Live<'p> {
    fn new(plan: &Tournament, params: &Params, progress: Progress<'p>) -> Self {
        Self {
            progress,
            race: plan.race,
            candidates: plan.qualifying,
            threads: plan.threads,
            to_build: if plan.qualifying < plan.candidates + plan.mutants {
                plan.candidates + plan.mutants
            } else {
                0
            },
            draws: plan.candidates,
            stages: plan.stages.clone(),
            tries: plan.tries(params),
            built: AtomicU32::new(0),
            built_stops: AtomicUsize::new(0),
            tried: AtomicU64::new(0),
            routes: Mutex::new(vec![None; (plan.candidates + plan.mutants).max(plan.threads)]),
            round: Mutex::new(None),
            started: OnceLock::new(),
            best: Mutex::new(Best::default()),
            done: AtomicBool::new(false),
            qualified: AtomicUsize::new(0),
        }
    }

    /// Report every 300 ms until the tournament is over.
    fn watch(&self, searcher: &Searcher<'_>) {
        let viewer = Planner::new(searcher.model, searcher.world, searcher.params, searcher.profile)
            .diagnosed(searcher.diagnostics);
        let mut last_route = Instant::now();
        let mut last = Instant::now();
        // Short sleeps, so that a short tournament does not wait for the next report to end.
        while !self.done.load(Ordering::Relaxed) {
            std::thread::sleep(Duration::from_millis(5));
            if last.elapsed() >= Duration::from_millis(300) {
                last = Instant::now();
                self.report();
                self.report_route(&mut last_route, &viewer);
            }
        }
    }

    /// Round `stage` begins with these routes.
    fn enter(&self, stage: usize, ids: Vec<usize>) {
        *self.round.locked() = Some((stage, ids));
    }

    /// The routes followed: during the race the best `threads` candidates so far, then the
    /// routes of the round. Each as {id, time (play), score}; `selected` is the best score.
    fn followed(&self) -> (Vec<serde_json::Value>, Option<usize>) {
        let routes = self.routes.locked();
        let ids: Vec<usize> = if let Some((_, ids)) = &*self.round.locked() {
            ids.clone()
        } else {
            let mut ranked: Vec<usize> = (0..routes.len()).filter(|&k| routes[k].is_some()).collect();
            ranked.sort_by(|&a, &b| {
                routes[a]
                    .expect("ranked routes exist")
                    .0
                    .total_cmp(&routes[b].expect("ranked routes exist").0)
            });
            ranked.truncate(self.threads);
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

    /// Share of the tries done (0 to 1), and where the race ends, once a route gives the size of
    /// the routes (the tries are per stop).
    fn progress(&self) -> Option<(f64, f64)> {
        let (n, stops) = (
            self.built.load(Ordering::Relaxed),
            self.built_stops.load(Ordering::Relaxed),
        );
        if n == 0 {
            return None;
        }
        let (race, total) = self.tries.at(stops as f64 / f64::from(n));
        if total <= 0.0 {
            return None;
        }
        let done = self.tried.load(Ordering::Relaxed) as f64;
        Some(((done / total.max(1.0)).min(1.0), race / total))
    }

    /// `@progress {json}`: phase, elapsed seconds, share of the tries done, play time of the best
    /// start and best route, the stage (race, round, final) and the routes followed.
    fn report(&self) {
        let (start, best, built_best) = {
            let b = self.best.locked();
            (b.start.1, b.route.1, b.built.1)
        };
        let Some(t) = self.started.get() else {
            let built = self.built.load(Ordering::Relaxed);
            let best = built_best;
            let msg = serde_json::json!({
                "phase": "construct",
                "built": built,
                "to_build": self.to_build,
                "draws": self.draws,
                "candidates": self.candidates,
                "best": if best.is_finite() { Some(best) } else { None },
            });
            (self.progress)(&JobEvent::Progress(&msg));
            return;
        };
        let (routes, selected) = self.followed();
        let mut msg = serde_json::json!({
            "phase": "optimize",
            "elapsed": t.elapsed().as_secs_f64(),
            "start": if start.is_finite() { Some(start) } else { None },
            "best": if best.is_finite() { Some(best) } else { None },
            "routes": routes,
            "selected": selected,
            "candidates": if self.race { self.candidates } else { 0 },
            "qualified": self.qualified.load(Ordering::Relaxed),
            "searches": self.threads,
            "rounds": self.stages.len(),
        });
        if let Some((done, race_end)) = self.progress() {
            msg["progress"] = done.into();
            msg["race_end"] = race_end.into();
        }
        match &*self.round.locked() {
            None => msg["stage"] = "qualify".into(),
            Some((stage, ids)) => {
                let last = stage + 1 == self.stages.len();
                msg["stage"] = if last { "final" } else { "round" }.into();
                msg["round"] = (stage + 1).into();
                msg["width"] = ids.len().into();
                msg["searches"] = (self.threads / ids.len().max(1)).into();
            }
        }
        (self.progress)(&JobEvent::Progress(&msg));
    }

    /// `@route {points}` for live drawing, when the best route changed (at most every 1.5 s).
    fn report_route(&self, last: &mut Instant, planner: &Planner<'_>) {
        if last.elapsed() < Duration::from_millis(1500) {
            return;
        }
        let route = {
            let mut b = self.best.locked();
            if !b.changed {
                return;
            }
            b.changed = false;
            b.stops.clone()
        };
        *last = Instant::now();
        let points = planner.places().polyline(&route, 800);
        (self.progress)(&JobEvent::Route(&points));
    }

    /// A route built before the race: the best so far is drawn.
    fn on_built(&self, score: f64, play: f64, route: &[Stop]) {
        let mut best = self.best.locked();
        if score < best.built.0 {
            best.built = (score, play);
            best.show(route);
        }
    }

    fn on_best(&self, k: usize, first: &Cell<bool>, score: f64, play: f64, route: &[Stop]) {
        // Several searches may improve the same route: its best of them.
        if let Some(r) = self.routes.locked().get_mut(k)
            && r.is_none_or(|(s, _)| score < s)
        {
            *r = Some((score, play));
        }
        let mut best = self.best.locked();
        if first.replace(false) && score < best.start.0 {
            best.start = (score, play);
        }
        if score < best.route.0 {
            best.route = (score, play);
            best.show(route);
        }
    }
}

/// The best of the tournament so far, each as (score, play time).
struct Best {
    /// Of the routes built before the race.
    built: (f64, f64),
    /// Of the starts, and of the routes improved.
    start: (f64, f64),
    route: (f64, f64),
    /// The route drawn, and whether it changed since last drawn.
    stops: Vec<Stop>,
    changed: bool,
}

impl Default for Best {
    fn default() -> Self {
        let none = (f64::INFINITY, f64::INFINITY);
        Self {
            built: none,
            start: none,
            route: none,
            stops: Vec::new(),
            changed: false,
        }
    }
}

impl Best {
    /// `route` is the one to draw.
    fn show(&mut self, route: &[Stop]) {
        self.stops.clear();
        self.stops.extend_from_slice(route);
        self.changed = true;
    }
}

/// Where a search stands in the tournament.
#[derive(Clone, Copy)]
struct Slot {
    /// Round and search of the route (None in the race: the candidate's own seed).
    round: Option<(usize, usize)>,
    /// Tries per stop of the route, and at least (times `effort`).
    tries_per_stop: f64,
    min_tries: f64,
    /// Its progress is logged (the first search of the best route of a round).
    logs: bool,
}

/// Builds and improves the route of one candidate.
#[derive(Clone, Copy)]
struct Searcher<'a> {
    model: &'a Model,
    world: &'a World,
    params: &'a Params,
    profile: &'a Profile,
    diagnostics: &'a Diagnostics,
    live: &'a Live<'a>,
    progress: Progress<'a>,
    /// The user stopping or cancelling the generation.
    interrupt: &'a Interrupt,
    /// The candidates built as another with one of its zone changes done otherwise (`mutate`),
    /// by candidate.
    mutants: &'a [Option<Mutant>],
    /// Zones rich in quests the first candidates after the first begin in (`start_zones`).
    starts: &'a [i64],
}

/// A race candidate built as a better one with one zone change done otherwise.
#[derive(Clone)]
struct Mutant {
    params: Params,
    parent: usize,
    /// The parent's zone changes, and which one is done otherwise.
    parent_changes: Vec<ZoneChange>,
    what: String,
}

impl Searcher<'_> {
    /// The settings of race candidate `k` (`candidate_params`), the first ones after the first
    /// beginning in a zone rich in quests rather than where the character stands.
    fn candidate(&self, k: usize) -> Params {
        let mut p = candidate_params(self.params, k);
        if k >= 1
            && let Some(&zone) = self.starts.get(k - 1)
        {
            p.start_zone = zone;
        }
        p
    }

    /// Improve `route` (built when None) with candidate `k`'s settings for the tries of `slot`,
    /// meeting the other searches of the route at `member`'s exchange. Returns its score under
    /// the request's own settings; None when the generation is cancelled, or stopped while
    /// building a route that is not `needed` (the first ones are: without them there would be no
    /// guide to keep).
    fn search(
        &self,
        k: usize,
        route: Option<Vec<Stop>>,
        slot: Slot,
        needed: bool,
        member: Option<&Member<'_>>,
    ) -> Option<Found> {
        if self.interrupt.cancelled() {
            return None;
        }
        let mutant = self.mutants.get(k).and_then(Option::as_ref);
        let mut p = mutant.as_ref().map_or_else(|| self.candidate(k), |m| m.params.clone());
        if let Some((stage, search)) = slot.round {
            p.seed = round_seed(p.seed, stage, search);
        }
        let give_up = || self.interrupt.cancelled() || (!needed && self.interrupt.stopping());
        let planner = Planner::new(self.model, self.world, &p, self.profile)
            .diagnosed(self.diagnostics)
            .abortable(&give_up)
            .sharing(member);
        let log = |m: &str| {
            if slot.logs {
                (self.progress)(&JobEvent::Status(m));
            }
        };
        // A start: built here, or a candidate of the race built beforehand (`screen`).
        let first = Cell::new(route.is_none() || slot.round.is_none());
        let (start, changes) = if let Some(route) = route {
            (route, None)
        } else {
            let (route, changes) = self.build(k, &planner, mutant, log)?;
            (route, Some(changes))
        };
        self.live.started.get_or_init(Instant::now);
        let tries = Tries::of(
            slot.tries_per_stop,
            slot.min_tries,
            start.len() as f64,
            self.params.effort,
        );
        // With FG_RACE_LOG, each candidate's best at doublings of its tries: how soon the race
        // ranking settles.
        let checkpoints = slot.round.is_none() && self.diagnostics.race;
        let (tried, best) = (Cell::new(0u64), Cell::new(f64::INFINITY));
        let route = planner.improve_while(
            start,
            Effort::Tries(tries),
            || {
                self.live.tried.fetch_add(1, Ordering::Relaxed);
                let done = tried.replace(tried.get() + 1);
                if checkpoints && (done == 0 || (done % 2500 == 0 && (done / 2500).is_power_of_two())) {
                    (self.progress)(&JobEvent::Status(&format!(
                        "candidate {k} at {done} tries: {:.0}",
                        best.get()
                    )));
                }
                !self.interrupt.stopping()
            },
            log,
            |score, play, route| {
                best.set(best.get().min(score));
                self.live.on_best(k, &first, score, play, route);
            },
        );
        if self.interrupt.cancelled() {
            return None;
        }
        Some(Found {
            score: self.score(&route),
            k,
            route,
            changes,
        })
    }

    /// Build candidate `k`'s route (None when given up), with its zone changes.
    fn build(
        &self,
        k: usize,
        planner: &Planner<'_>,
        mutant: Option<&Mutant>,
        log: impl Fn(&str),
    ) -> Option<(Vec<Stop>, Vec<ZoneChange>)> {
        let (route, changes) = planner.construct_zones(log);
        if planner.aborted() {
            return None;
        }
        self.live.built.fetch_add(1, Ordering::Relaxed);
        self.live.built_stops.fetch_add(route.len(), Ordering::Relaxed);
        if let Some(m) = mutant
            && self.diagnostics.race
        {
            let parent = &m.parent_changes;
            let same = changes.iter().zip(parent).take_while(|(a, b)| a == b).count();
            (self.progress)(&JobEvent::Status(&format!(
                "candidate {k} (mutant of {}, {}): zone changes as its parent's up to {same} of {}",
                m.parent,
                m.what,
                parent.len()
            )));
        }
        Some((route, changes))
    }

    /// Build candidate `k`'s route only, scored (None when cancelled, or stopped while building a
    /// route that is not `needed`).
    fn build_only(&self, k: usize, needed: bool) -> Option<Found> {
        let mutant = self.mutants.get(k).and_then(Option::as_ref);
        let p = mutant.as_ref().map_or_else(|| self.candidate(k), |m| m.params.clone());
        let give_up = || self.interrupt.cancelled() || (!needed && self.interrupt.stopping());
        let planner = Planner::new(self.model, self.world, &p, self.profile)
            .diagnosed(self.diagnostics)
            .abortable(&give_up);
        let (route, changes) = self.build(k, &planner, mutant, |_| {})?;
        let (score, play) = self.scored(&route);
        self.live.on_built(score, play, &route);
        Some(Found {
            score,
            k,
            route,
            changes: Some(changes),
        })
    }

    /// Score of a route under the request's own settings.
    fn score(&self, route: &[Stop]) -> f64 {
        self.scored(route).0
    }

    /// Score and play time of a route under the request's own settings.
    fn scored(&self, route: &[Stop]) -> (f64, f64) {
        Planner::new(self.model, self.world, self.params, self.profile)
            .diagnosed(self.diagnostics)
            .simulate_full(route, None)
            .map_or((f64::INFINITY, f64::INFINITY), |(score, spent)| (score, spent.total))
    }
}

/// Seed of search `search` of a route in round `stage`, from the candidate's seed.
fn round_seed(seed: u64, stage: usize, search: usize) -> u64 {
    let mix = ((stage as u64 + 1) << 32) | search as u64;
    rand::rngs::SmallRng::seed_from_u64(seed ^ mix.wrapping_mul(0x9e37_79b9_7f4a_7c15)).random()
}

/// Routes ranked by score, then by candidate (the same tries give the same order).
fn rank(mut routes: Vec<(f64, usize, Vec<Stop>)>) -> Vec<(f64, usize, Vec<Stop>)> {
    routes.sort_by(|a, b| a.0.total_cmp(&b.0).then(a.1.cmp(&b.1)));
    routes
}

/// The race: every candidate is built and improved for `qualify_tries` (at least
/// `qualify_min_tries`), `threads` at a time, then `race_mutants` more as the best of them with
/// one zone change done otherwise: weights give one zone order each, mutations look around the
/// good ones. With `screen`, all are built first and only the best built routes are improved
/// (the ranking of the built routes already tells the winner).
/// Stopped, no new candidate starts but the first ones (there must be a route).
fn qualify(searcher: &Searcher<'_>, plan: &Tournament) -> (Ranked, Vec<Option<Mutant>>) {
    let (first, end) = (plan.candidates, plan.candidates + plan.mutants);
    // The zone changes of each candidate's construction, for the mutants.
    let mut changes = Vec::new();
    let mut mutants = Vec::new();
    let ranked = if plan.qualifying < end {
        let mut built = split(build_all(searcher, plan, 0..first), &mut changes);
        if plan.mutants > 0 && !searcher.interrupt.stopping() {
            mutants = mutate(searcher, &changes, &rank(built.clone()), first..end);
            let with = Searcher {
                mutants: &mutants,
                ..*searcher
            };
            built.extend(split(build_all(&with, plan, first..end), &mut changes));
        }
        let built = rank(built);
        if searcher.diagnostics.race {
            for (score, k, _) in &built {
                (searcher.progress)(&JobEvent::Status(&format!("built {k}: {}", fmt_time(*score))));
            }
        }
        let kept = built
            .into_iter()
            .take(plan.qualifying)
            .map(|(_, k, r)| (k, Some(r)))
            .collect();
        let with = Searcher {
            mutants: &mutants,
            ..*searcher
        };
        rank(split(race(&with, plan, kept), &mut changes))
    } else {
        let mut qualified = split(
            race(searcher, plan, (0..first).map(|k| (k, None)).collect()),
            &mut changes,
        );
        if plan.mutants > 0 && !searcher.interrupt.stopping() {
            mutants = mutate(searcher, &changes, &rank(qualified.clone()), first..end);
            let with = Searcher {
                mutants: &mutants,
                ..*searcher
            };
            qualified.extend(split(
                race(&with, plan, (first..end).map(|k| (k, None)).collect()),
                &mut changes,
            ));
        }
        rank(qualified)
    };
    if searcher.diagnostics.race {
        for (score, k, _) in &ranked {
            (searcher.progress)(&JobEvent::Status(&format!("candidate {k}: {}", fmt_time(*score))));
        }
    }
    (ranked, mutants)
}

/// Routes as (score, candidate, route), the best first (`rank`).
type Ranked = Vec<(f64, usize, Vec<Stop>)>;

/// A candidate's route, built or improved.
struct Found {
    /// Its score under the request's own settings.
    score: f64,
    /// The candidate.
    k: usize,
    route: Vec<Stop>,
    /// The zone changes of its construction (None when the route was given).
    changes: Option<Vec<ZoneChange>>,
}

/// The routes found, as ranked, their zone changes kept in `changes` by candidate.
fn split(found: Vec<Found>, changes: &mut Vec<Vec<ZoneChange>>) -> Vec<(f64, usize, Vec<Stop>)> {
    (found.into_iter())
        .map(|f| {
            if let Some(c) = f.changes {
                if changes.len() <= f.k {
                    changes.resize(f.k + 1, Vec::new());
                }
                changes[f.k] = c;
            }
            (f.score, f.k, f.route)
        })
        .collect()
}

/// Candidates `ks` built only, `threads` at a time. Stopped, the first ones are still built
/// (there must be a route).
fn build_all(searcher: &Searcher<'_>, plan: &Tournament, ks: std::ops::Range<usize>) -> Vec<Found> {
    let next = AtomicUsize::new(ks.start);
    std::thread::scope(|scope| {
        let workers: Vec<_> = (0..plan.threads)
            .map(|_| {
                scope.spawn(|| {
                    let _release = crate::threads::Release;
                    let mut built = Vec::new();
                    loop {
                        let k = next.fetch_add(1, Ordering::Relaxed);
                        let stopping = searcher.interrupt.stopping() && k >= plan.threads;
                        if k >= ks.end || stopping || searcher.interrupt.cancelled() {
                            break;
                        }
                        built.extend(searcher.build_only(k, k < plan.threads));
                    }
                    built
                })
            })
            .collect();
        workers.into_iter().flat_map(|w| w.join().joined()).collect()
    })
}

/// Parents of the mutated candidates: the best of the first ones.
const PARENTS: usize = 4;

/// Candidates `ks` as the best `ranked` ones in turn, with one of their zone changes done
/// otherwise (`Params::zone_detour`): left a few levels earlier or later, or for a zone it went to
/// later. The greedy construction gives the same route up to there.
fn mutate(
    searcher: &Searcher<'_>,
    changes: &[Vec<ZoneChange>],
    ranked: &[(f64, usize, Vec<Stop>)],
    ks: std::ops::Range<usize>,
) -> Vec<Option<Mutant>> {
    let parents: Vec<usize> = ranked
        .iter()
        .map(|(_, k, _)| *k)
        .filter(|&k| changes.get(k).is_some_and(|c| !c.is_empty()))
        .take(PARENTS)
        .collect();
    if parents.is_empty() {
        return Vec::new();
    }
    let mut mutants = vec![None; ks.end];
    for (i, k) in ks.enumerate() {
        let parent = parents[i % parents.len()];
        let seed = searcher.params.seed.wrapping_add(k as u64);
        let mut rng = rand::rngs::SmallRng::seed_from_u64(seed ^ 0x5851_f42d_4c95_7f2d);
        let (detour, what) = detour(&changes[parent], &mut rng);
        let mut params = searcher.candidate(parent);
        params.seed = seed;
        params.zone_detour = detour;
        mutants[k] = Some(Mutant {
            params,
            parent,
            parent_changes: changes[parent].clone(),
            what,
        });
    }
    mutants
}

/// A stay in a zone shorter than this (seconds of play) is passing through: its zone changes
/// are not detoured.
const MAJOR_STAY: f64 = 1800.0;

/// A detour (see `Params::zone_detour`) of one of the zone changes `changes` (level, time, from,
/// to) into a long stay, and what it does: left earlier (from the stay before) or later, or for
/// the zone of a later stay.
fn detour(changes: &[ZoneChange], rng: &mut impl RngExt) -> (Vec<i64>, String) {
    let major: Vec<usize> = (0..changes.len())
        .filter(|&i| {
            changes
                .get(i + 1)
                .is_none_or(|next| next.1 - changes[i].1 >= MAJOR_STAY)
        })
        .collect();
    let m = rng.random_range(0..major.len());
    let n = major[m];
    let (level, _, _, to) = changes[n];
    let later: Vec<i64> = major[m + 1..]
        .iter()
        .map(|&i| changes[i].3)
        .filter(|&z| z != to)
        .take(4)
        .collect();
    let shift = rng.random_range(1..=3);
    let since = if m == 0 { 0 } else { major[m - 1] + 1 };
    match rng.random_range(0..3) {
        0 => (
            vec![since as i64, level - shift, to],
            format!("change {n} {shift} levels earlier"),
        ),
        2 if !later.is_empty() => {
            let zone = later[rng.random_range(0..later.len())];
            (vec![n as i64, level, zone], format!("change {n} to zone {zone}"))
        }
        _ => (
            vec![n as i64, level + shift, to],
            format!("change {n} {shift} levels later"),
        ),
    }
}

/// Candidates of the race (their route, built when None), `threads` at a time.
fn race(searcher: &Searcher<'_>, plan: &Tournament, candidates: Vec<(usize, Option<Vec<Stop>>)>) -> Vec<Found> {
    let next = AtomicUsize::new(0);
    // Each taken by one search only (`next`): the lock is never waited for.
    let candidates: Vec<_> = candidates.into_iter().map(|c| Mutex::new(Some(c))).collect();
    let slot = Slot {
        round: None,
        tries_per_stop: searcher.params.qualify_tries,
        min_tries: searcher.params.qualify_min_tries,
        logs: false,
    };
    std::thread::scope(|scope| {
        let workers: Vec<_> = (0..plan.threads)
            .map(|_| {
                scope.spawn(|| {
                    let _release = crate::threads::Release;
                    let mut qualified = Vec::new();
                    loop {
                        let i = next.fetch_add(1, Ordering::Relaxed);
                        let stopping = searcher.interrupt.stopping() && i >= plan.threads;
                        if i >= candidates.len() || stopping || searcher.interrupt.cancelled() {
                            break;
                        }
                        let Some((k, route)) = candidates[i].locked().take() else {
                            continue;
                        };
                        if let Some(found) = searcher.search(k, route, slot, i < plan.threads, None) {
                            qualified.push(found);
                            searcher.live.qualified.fetch_add(1, Ordering::Relaxed);
                        }
                    }
                    qualified
                })
            })
            .collect();
        workers.into_iter().flat_map(|w| w.join().joined()).collect()
    })
}

/// Round `stage`: each route improved by its searches (`Tournament::searches`), which share their
/// best (`share_every`). Each route's best, ranked.
fn round(
    searcher: &Searcher<'_>,
    plan: &Tournament,
    stage: usize,
    routes: &[(usize, Option<Vec<Stop>>)],
) -> Vec<(f64, usize, Vec<Stop>)> {
    searcher.live.enter(stage, routes.iter().map(|(k, _)| *k).collect());
    let tries_per_stop = if plan.is_final(stage) {
        searcher.params.final_tries
    } else {
        searcher.params.round_tries
    };
    let exchanges: Vec<Exchange> = (0..routes.len())
        .map(|i| Exchange::new(plan.searches(stage, i)))
        .collect();
    let found: Vec<(f64, usize, Vec<Stop>)> = std::thread::scope(|scope| {
        let mut handles = Vec::new();
        for (i, (k, route)) in routes.iter().enumerate() {
            let searches = plan.searches(stage, i);
            let exchange = &exchanges[i];
            for search in 0..searches {
                let slot = Slot {
                    round: Some((stage, search)),
                    tries_per_stop,
                    min_tries: 0.0,
                    logs: i == 0 && search == 0,
                };
                handles.push(scope.spawn(move || {
                    let _release = crate::threads::Release;
                    // Dropped at the end, panic included: the others stop waiting for it.
                    let member = exchange.member();
                    let seat = (searcher.params.share_every > 0 && searches > 1).then_some(&member);
                    let result = searcher.search(*k, route.clone(), slot, true, seat);
                    result.map(|f| (f.score, f.k, f.route))
                }));
            }
        }
        handles.into_iter().filter_map(|h| h.join().joined()).collect()
    });
    // Each route's best search (the first one on a tie).
    let mut best: Vec<(f64, usize, Vec<Stop>)> = Vec::new();
    for (score, k, route) in found {
        match best.iter_mut().find(|b| b.1 == k) {
            Some(b) if score < b.0 => *b = (score, k, route),
            Some(_) => {}
            None => best.push((score, k, route)),
        }
    }
    let ranked = rank(best);
    if searcher.diagnostics.race {
        for (score, k, _) in &ranked {
            (searcher.progress)(&JobEvent::Status(&format!(
                "round {} route {k}: {}",
                stage + 1,
                fmt_time(*score)
            )));
        }
    }
    ranked
}

/// Play time of the routes of the last round (ranked by score).
fn summary(planner: &Planner<'_>, ranked: &[(f64, usize, Vec<Stop>)]) -> String {
    let times: Vec<String> = ranked
        .iter()
        .map(|(_, _, r)| {
            if let Some((_, b)) = planner.simulate_full(r, None) {
                return fmt_time(b.total);
            }
            if planner.diagnostics.race
                && let Some((n, why)) = planner.diagnose(r)
            {
                eprintln!("invalid route ({} stops) at stop {n}: {why}", r.len());
            }
            "-".into()
        })
        .collect();
    format!("{} routes: {}", ranked.len(), times.join(" "))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn tournament(threads: usize, candidates: usize, final_routes: usize, effort: f64) -> Tournament {
        Tournament::new(&Params {
            candidates,
            round_tries: 20.0,
            final_routes,
            effort,
            threads,
            ..Params::default()
        })
    }

    /// Without tries in the rounds, the race goes straight to the final; without a race the
    /// routes are built by a first round of their own.
    #[test]
    fn no_rounds_without_tries() {
        let params = |effort| Params {
            candidates: 32,
            round_tries: 0.0,
            final_routes: 1,
            effort,
            threads: 8,
            ..Params::default()
        };
        assert_eq!(Tournament::new(&params(1.0)).stages, vec![1]);
        let preview = Tournament::new(&params(0.1));
        assert!(!preview.race);
        assert_eq!(preview.stages, vec![8, 1]);
        assert_eq!(preview.searches(1, 0), 8);
    }

    /// Each round keeps half the routes, with twice the searches each, down to the final.
    #[test]
    fn rounds_halve_the_routes() {
        let plan = tournament(8, 64, 2, 1.0);
        assert!(plan.race);
        assert_eq!(plan.stages, vec![8, 4, 2]);
        let searches = |stage| {
            (0..plan.stages[stage])
                .map(|i| plan.searches(stage, i))
                .collect::<Vec<_>>()
        };
        assert_eq!(searches(0), vec![1; 8]);
        assert_eq!(searches(1), vec![2; 4]);
        assert_eq!(searches(2), vec![4; 2]);
        assert_eq!(tournament(6, 64, 2, 1.0).stages, vec![6, 3, 2]);
        assert_eq!(tournament(1, 64, 2, 1.0).stages, vec![1]);
    }

    /// A detour changes one zone change of the route: when, or where to.
    #[test]
    fn detours_change_one_zone_change() {
        let changes = [
            (3, 0.0, 1, 2),
            (8, 3600.0, 2, 3),
            (8, 3700.0, 3, 2),
            (12, 9000.0, 2, 4),
            (15, 12000.0, 4, 2),
        ];
        let mut rng = rand::rngs::SmallRng::seed_from_u64(7);
        for _ in 0..200 {
            let (d, what) = detour(&changes, &mut rng);
            // Not into the stay of 100 s.
            assert!(!what.starts_with("change 1 "), "{what}");
            assert!(d[1] != 8 || d[2] != 3, "{d:?}");
        }
    }

    /// A low effort races fewer candidates, down to none (one route per search).
    #[test]
    fn a_preview_races_fewer_candidates() {
        assert_eq!(tournament(8, 64, 2, 0.5).candidates, 32);
        let preview = tournament(8, 64, 2, 0.1);
        assert_eq!(preview.candidates, 8);
        assert!(!preview.race);
    }

    /// The tries are counted per stop: the race's share of them, then every search's. A race
    /// candidate of a short guide gets its minimum instead. With `screen`, only the candidates
    /// kept are improved.
    #[test]
    fn progress_counts_the_tries_per_stop() {
        let params = Params {
            candidates: 64,
            race_mutants: 0,
            screen: 0,
            qualify_tries: 2.0,
            qualify_min_tries: 1000.0,
            round_tries: 10.0,
            final_routes: 2,
            final_tries: 50.0,
            effort: 1.0,
            threads: 8,
            ..Params::default()
        };
        let tries = Tournament::new(&params).tries(&params);
        let (race, total) = tries.at(1000.0);
        assert!((race - 64.0 * 2000.0).abs() < 1e-9);
        assert!((total - (race + 8.0 * (2.0 * 10.0 + 50.0) * 1000.0)).abs() < 1e-9);
        let (short, _) = tries.at(100.0);
        assert!((short - 64.0 * 1000.0).abs() < 1e-9);
        let screened = Params {
            race_mutants: 100,
            screen: 8,
            ..params
        };
        let plan = Tournament::new(&screened);
        assert_eq!((plan.candidates, plan.mutants, plan.qualifying), (64, 100, 8));
        let (race, _) = plan.tries(&screened).at(1000.0);
        assert!((race - 8.0 * 2000.0).abs() < 1e-9);
    }
}
