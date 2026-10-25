//! Building a first route: greedy or region by region, then repairs (class quests, pet quests first, settling).

use super::Planner;
use super::state::{Kind, State, Stop, fmt_time};
use crate::params::Construction;
use crate::world::Pos;

/// A stop `nearest_stop` weighs: where it is, its weights, the quest that must be worth taking
/// on the trip, and the least score the trip's bound gives.
struct Candidate {
    order: usize,
    stop: Stop,
    pos: Pos,
    weight: f64,
    stay: f64,
    /// In the character's zone (the stay weight does not apply).
    here: bool,
    worth: Option<u32>,
    least: f64,
}

impl Planner<'_> {
    /// Greedy construction: nearest worthwhile stop first.
    pub fn greedy(&self) -> Vec<Stop> {
        let mut s = self.initial_state();
        // The quests of the character's log not worth finishing from here are given up (the
        // replay does it too, `start_state`): `keep_log_quests` puts back those on the way.
        for &i in &self.model.initial.accepted {
            if s.accepted[i] && !self.worth().finish_worth_it(&s, i as u32) {
                self.abandon(&mut s, i as u32, &mut |_, _| {});
            }
        }
        let mut route = Vec::new();
        let mut changes = Vec::new();
        // The detour (`Params::zone_detour`): the zone held until its level, once begun.
        let mut held: Option<i64> = None;
        let mut detoured = false;
        let n = self.model.quests.len() as u32;
        // `Params::start_zone`: the zone gone to first, whatever is nearer.
        let mut arrived = self.params.start_zone == 0;
        while !self.ended(&s) && !self.aborted() {
            if !arrived && s.zone == self.params.start_zone {
                arrived = true;
            }
            let pull = match self.params.zone_detour[..] {
                _ if !arrived => Some(self.params.start_zone),
                [after, level, zone] if !detoured && changes.len() as i64 >= after => {
                    if s.level < level {
                        Some(*held.get_or_insert(s.zone))
                    } else if s.zone == zone {
                        detoured = true;
                        None
                    } else {
                        Some(zone)
                    }
                }
                _ => None,
            };
            let next = if let Some(stop) = self.nearest_stop(&s, pull) {
                stop
            } else {
                // Nothing doable at this level: the smallest grind that unlocks something,
                // a level-gated quest or an objective whose mobs are still too high.
                let reachable =
                    |s: &State, stop: Stop| self.trips().fastest(s, &self.places().pos(s, stop)).0.is_finite();
                let mut options: Vec<(i64, Stop)> = Vec::new();
                for i in 0..n {
                    let iu = i as usize;
                    let q = self.quest(i);
                    if s.accepted[iu] && !s.turned[iu] {
                        for (k, o) in q.objectives.iter().enumerate() {
                            if self.availability().objective_open(&s, i, k) && o.kills > 0.0 && o.dungeon.is_none() {
                                options.push((
                                    self.params.level_for(self.combat().need(q, o), s.might()),
                                    Stop::quest(i, Kind::Objective(k as u8)),
                                ));
                            }
                        }
                    } else if self.availability().can_accept(&s, i) {
                        let accept = Stop::quest(i, Kind::Accept);
                        let gate = self.gate_level(&s, accept);
                        // Or the level its hardest objective needs (`worth_it` looks one level
                        // ahead): its giver in reach, its mobs not yet.
                        let hardest = (q.objectives.iter())
                            .filter(|o| o.dungeon.is_none())
                            .map(|o| self.combat().need(q, o))
                            .fold(0.0, f64::max);
                        let gate = gate.max(self.params.level_for(hardest, s.might()) - 1);
                        if gate > s.level && (q.mandatory || self.worth().worth_it(&s, i, gate, 0.0)) {
                            options.push((gate, accept));
                        }
                    }
                }
                options.sort_by_key(|(lvl, _)| *lvl);
                if std::env::var_os("FG_DEBUG_STALL").is_some() && options.first().is_some_and(|(l, _)| *l > s.level) {
                    eprintln!(
                        "gated fallback at {} (to level {:?})",
                        fmt_time(s.time),
                        options.first().map(|o| o.0)
                    );
                    self.explain_stall(&s);
                }
                if let Some((_, stop)) = options.into_iter().find(|(_, stop)| reachable(&s, *stop)) {
                    stop
                } else {
                    if std::env::var_os("FG_DEBUG").is_some() {
                        self.explain_stall(&s);
                    }
                    break;
                }
            };
            let zone = self.places().zone(next);
            if zone != s.zone {
                changes.push((s.level, s.time, s.zone, zone));
            }
            self.apply(&mut s, next, None);
            route.push(next);
        }
        *self.zone_changes.borrow_mut() = changes;

        if std::env::var_os("FG_DEBUG").is_some() {
            eprintln!(
                "greedy ended at level {} after {} stops ({}), replay: {:?}",
                s.level,
                route.len(),
                fmt_time(s.time),
                self.simulate(&route, None).map(fmt_time)
            );
        }
        let route = self.settle(route);
        self.insert_missing_mandatory(route)
    }

    /// Zones other than the character's with the most quests worth doing from the start, the
    /// richest first with their number of quests (at least `MIN_START_QUESTS`, in reach, the
    /// capital cities left out: every route passes through them): where a candidate of the race
    /// may begin instead of in the zone it stands in, `Params::start_zone`.
    pub(crate) fn start_zones(&self) -> Vec<(i64, usize)> {
        const MIN_START_QUESTS: usize = 5;
        let s = self.initial_state();
        let mut counts: std::collections::HashMap<i64, usize> = std::collections::HashMap::new();
        for i in 0..self.model.quests.len() as u32 {
            let accept = Stop::quest(i, Kind::Accept);
            if self.gate_level(&s, accept) <= s.level
                && self.availability().can_accept(&s, i)
                && self.worth().worth_it(&s, i, s.level, 0.0)
            {
                let zone = self.places().zone(accept);
                if zone != s.zone
                    && !self.world.is_capital(zone)
                    && self.trips().fastest(&s, &self.places().pos(&s, accept)).0.is_finite()
                {
                    *counts.entry(zone).or_default() += 1;
                }
            }
        }
        let mut zones: Vec<(i64, usize)> = counts.into_iter().filter(|&(_, n)| n >= MIN_START_QUESTS).collect();
        zones.sort_by(|a, b| b.1.cmp(&a.1).then(a.0.cmp(&b.0)));
        zones
    }

    /// The stop the greedy construction takes next from `s` (None: nothing worth doing at this
    /// level), pulled to a zone by a detour.
    ///
    /// The stop of least score, the first of equal ones. Scores grow with the trip, which takes
    /// at least `TripBound::at`: the stops are weighed from the least bound up, until the bound
    /// alone scores more than the best (the walks to far stops are never searched).
    fn nearest_stop(&self, s: &State, pull: Option<i64>) -> Option<Stop> {
        let n = self.model.quests.len() as u32;
        let bound = self.trips().bound(s);
        // Quest value only decides what is worth accepting. Weights favour finishing what
        // is in the log, and class quests.
        let mut candidates: Vec<Candidate> = Vec::new();
        // Going to the chosen start zone first is not a matter of distance: the other stops weigh
        // far more until the character is there.
        let pull_weight = if self.params.start_zone != 0 && s.zone != self.params.start_zone {
            1e4
        } else {
            self.params.zone_detour_pull
        };
        let mut add = |stop: Stop, weight: f64, worth: Option<u32>, lower: f64| {
            let pos = self.places().pos(s, stop);
            let zone = self.places().zone(stop);
            // Finish the zone before leaving it (clear a zone, then move on).
            let stay = if zone == s.zone { 1.0 } else { self.params.zone_stay };
            let stay = match pull {
                // Turn-ins where the character stands are not pulled away from, except on the way to
                // the chosen start zone (`Params::start_zone`): it begins there.
                Some(z)
                    if zone != z
                        && !(zone == s.zone
                            && stop.kind == Kind::TurnIn
                            && (self.params.start_zone == 0 || s.zone == self.params.start_zone)) =>
                {
                    stay * pull_weight
                }
                _ => stay,
            };
            // Same for another continent (a detour to Kalimdor while Eastern Kingdoms has more).
            let stay = if pos.continent == s.pos.continent {
                stay
            } else {
                stay * self.params.continent_stay
            };
            // Never reached: never the best.
            if !lower.is_finite() {
                return;
            }
            let least = if weight * stay > 0.0 {
                (lower + self.params.stop_overhead) * weight * stay
            } else {
                f64::NEG_INFINITY
            };
            candidates.push(Candidate {
                order: candidates.len(),
                stop,
                pos,
                weight,
                stay,
                here: zone == s.zone,
                worth,
                least,
            });
        };
        for i in 0..n {
            let iu = i as usize;
            if s.turned[iu] {
                continue;
            }
            let class = if self.quest(i).mandatory { 0.6 } else { 1.0 };
            // Givers and turn-ins among mobs too strong for the character wait.
            let reach = self.params.reach(s.level, s.might());
            let guarded = |g| self.combat().guard_need(g) > reach;
            if s.accepted[iu] {
                if self.availability().objectives_done(s, i) {
                    if !guarded(&self.quest(i).end_guard) {
                        let stop = Stop::quest(i, Kind::TurnIn);
                        add(stop, 0.7 * class, None, bound.at(&self.places().pos(s, stop)));
                    }
                } else {
                    let q = self.quest(i);
                    for k in 0..q.objectives.len() {
                        let o = &q.objectives[k];
                        let level_ok = self.combat().need(q, o) <= self.params.reach(s.level, s.might());
                        if self.availability().objective_open(s, i, k) && level_ok && o.dungeon.is_none() {
                            let stop = Stop::quest(i, Kind::Objective(k as u8));
                            add(stop, class, None, bound.at(&self.places().pos(s, stop)));
                        }
                    }
                }
            } else if self.quest(i).min_level <= s.level
                && !guarded(&self.quest(i).start_guard)
                && self.availability().can_accept(s, i)
            {
                // Worth it on its trip, which is no shorter than the bound: not worth it on the
                // bound, not worth it at all.
                let stop = Stop::quest(i, Kind::Accept);
                let lower = bound.at(&self.places().pos(s, stop));
                if !self.worth().worth_it(s, i, s.level, lower) {
                    continue;
                }
                // Dungeon quests: worth a detour only when the dungeon is close in level.
                // Quests of a dungeon close in level: forced ones first, the others weighed by
                // `dungeon_quest_weight` (the optimizer then keeps the runs that pay off).
                let dungeon = match self.quest(i).objectives.iter().find_map(|o| o.dungeon) {
                    Some(d) if s.level + 2 >= self.model.dungeons[d].min_level => {
                        if self.availability().dungeon_forced(d) {
                            0.25
                        } else {
                            self.params.dungeon_quest_weight
                        }
                    }
                    _ => 1.0,
                };
                let preferred = if self.params.preferred_zones.contains(&self.quest(i).starts[0].zone) {
                    0.5
                } else {
                    1.0
                };
                add(
                    stop,
                    1.3 * class
                        * dungeon
                        * preferred
                        * self.worth().level_fit(i, s.level)
                        * self.worth().efficiency_weight(s, i, s.level),
                    Some(i),
                    lower,
                );
            }
        }

        for d in 0..self.model.dungeons.len() {
            if let Some(stop) = self.dungeon_ready(s, d) {
                let weight = if self.availability().dungeon_forced(d) {
                    0.5
                } else {
                    0.8
                };
                add(stop, weight, None, bound.at(&self.places().pos(s, stop)));
            }
        }
        // Staying in a zone only matters while it has something left to do: with few stops left
        // there, the weight of leaving it fades to nothing (`Params::zone_stay_quests`).
        let want = self.params.zone_stay_quests;
        if want > 0.0 && self.params.zone_stay > 1.0 {
            let left = candidates.iter().filter(|c| c.here).count() as f64;
            let eff = 1.0 + (self.params.zone_stay - 1.0) * (left / want).min(1.0);
            let scale = eff / self.params.zone_stay;
            for c in candidates.iter_mut().filter(|c| !c.here) {
                c.stay *= scale;
                c.least *= scale;
            }
        }
        if let Some(stop) = self.least_score(s, candidates) {
            return Some(stop);
        }

        // Zone exhausted: move on to the closest quest worth doing on its own, wherever it is.
        let mut candidates: Vec<Candidate> = Vec::new();
        for i in 0..n {
            // Its level, and the camp around its giver (`gate_level`).
            if self.gate_level(s, Stop::quest(i, Kind::Accept)) <= s.level
                && self.availability().can_accept(s, i)
                && self.worth().worth_it(s, i, s.level, 0.0)
            {
                let stop = Stop::quest(i, Kind::Accept);
                let pos = self.places().pos(s, stop);
                let zone = self.places().zone(stop);
                let stay = if zone == s.zone { 1.0 } else { self.params.zone_stay };
                let stay = match pull {
                    Some(z) if zone != z => stay * self.params.zone_detour_pull,
                    _ => stay,
                };
                let stay = if pos.continent == s.pos.continent {
                    stay
                } else {
                    stay * self.params.continent_stay
                };
                let lower = bound.at(&pos);
                if lower.is_finite() {
                    candidates.push(Candidate {
                        order: candidates.len(),
                        stop,
                        pos,
                        weight: 1.0,
                        stay,
                        here: zone == s.zone,
                        worth: None,
                        least: if stay > 0.0 {
                            (lower + self.params.stop_overhead) * stay
                        } else {
                            f64::NEG_INFINITY
                        },
                    });
                }
            }
        }
        self.least_score(s, candidates)
    }

    /// The candidate of least score (the first of equal ones), weighed from the least bound up.
    fn least_score(&self, s: &State, mut candidates: Vec<Candidate>) -> Option<Stop> {
        candidates.sort_by(|a, b| a.least.total_cmp(&b.least).then(a.order.cmp(&b.order)));
        let mut best: Option<(f64, usize, Stop)> = None;
        for c in candidates {
            if best.is_some_and(|(score, _, _)| c.least > score) {
                break;
            }
            let (t, _) = self.trips().fastest(s, &c.pos);
            if c.worth.is_some_and(|i| !self.worth().worth_it(s, i, s.level, t)) {
                continue;
            }
            let score = (t + self.params.stop_overhead) * c.weight * c.stay;
            let better = |(b, order, _): (f64, usize, Stop)| match score.partial_cmp(&b) {
                Some(std::cmp::Ordering::Less) => true,
                Some(std::cmp::Ordering::Equal) => c.order < order,
                _ => false,
            };
            if score.is_finite() && best.is_none_or(better) {
                best = Some((score, c.order, c.stop));
            }
        }
        best.map(|(_, _, stop)| stop)
    }

    /// Make a constructed route replay as it was built. The construction does not know the
    /// route yet, so it never takes the quests at hand; the replay does (`while_here`), which
    /// changes the walks (mobs no longer killed on the way) and the log. Replayed leniently: an
    /// objective finished on the way becomes a stop of its own (skipped when done on the way),
    /// a quest that can no longer be taken or finished leaves the route. Repeated until the
    /// route is valid, the quests it takes at hand changing with it.
    pub(crate) fn settle(&self, mut route: Vec<Stop>) -> Vec<Stop> {
        for round in 0..5 {
            if self.simulate(&route, None).is_some() {
                break;
            }
            let settled = self.settle_once(&route);
            if std::env::var_os("FG_DEBUG").is_some() {
                let quests = |r: &[Stop]| r.iter().filter(|s| s.kind == Kind::TurnIn).count();
                eprintln!(
                    "settle {round}: {} -> {} stops, {} -> {} turn-ins",
                    route.len(),
                    settled.len(),
                    quests(&route),
                    quests(&settled)
                );
            }
            route = settled;
        }
        route
    }

    pub(crate) fn settle_once(&self, route: &[Stop]) -> Vec<Stop> {
        let mut s = self.start_state(self.planned(route));
        let mut dropped = vec![false; self.model.quests.len()];
        let mut out = Vec::with_capacity(route.len());
        for (pos, &stop) in route.iter().enumerate() {
            let i = stop.index as usize;
            if stop.is_quest() && dropped[i] {
                continue;
            }
            if self.ended(&s) {
                out.extend(
                    route[pos..]
                        .iter()
                        .filter(|x| !x.is_quest() || !dropped[x.index as usize]),
                );
                break;
            }
            s.at = pos as u32;
            if stop.kind == Kind::TurnIn
                && s.accepted[i]
                && !s.turned[i]
                && !self.availability().objectives_done(&s, stop.index)
            {
                for k in 0..self.quest(stop.index).objectives.len() {
                    let o = Stop::quest(stop.index, Kind::Objective(k as u8));
                    if s.objectives[i] & (1 << k) == 0 && self.availability().valid(&s, o) {
                        s.next = Some(self.places().pos(&s, stop));
                        self.apply(&mut s, o, None);
                        out.push(o);
                    }
                }
            }
            if !self.availability().valid(&s, stop) {
                if stop.is_quest() {
                    dropped[i] = true;
                }
                continue;
            }
            // Where it goes next, as `simulate` replays it: the objectives are done on that side
            // (`spot`), which changes the mobs killed on the way, the level and the log.
            s.next = self.next_pos(&s, route, pos);
            self.apply(&mut s, stop, None);
            out.push(stop);
        }
        out
    }

    /// Build the initial route: by quest groups (default) or plain nearest-neighbour greedy.
    pub fn construct(&self, log: impl Fn(&str)) -> Vec<Stop> {
        let route = if self.params.construction == Construction::Greedy {
            self.greedy()
        } else {
            self.plan_regions(&log)
        };
        // Given up (see `abortable`): the unfinished route is thrown away.
        if self.aborted() {
            return route;
        }
        let route = if self.params.construction == Construction::Greedy {
            route
        } else {
            self.insert_missing_mandatory(self.settle(route))
        };
        let route = self.keep_log_quests(route);
        let route = self.place_hearths(self.rebind(self.place_hearths(self.power_first(route))));
        if std::env::var_os("FG_DEBUG").is_some() {
            for (reason, n) in self.explain_unused(&route) {
                log(&format!("  unused {reason}: {n}"));
            }
        }
        if std::env::var_os("FG_DEBUG").is_some() {
            log(&format!(
                "constructed {} stops: {:?}",
                route.len(),
                self.simulate(&route, None).map(fmt_time)
            ));
        }
        route
    }

    /// The quests of the character's log the route does not turn in (given up, or left after
    /// an objective), each put back where the route passes closest to what is left of it (often
    /// only the turn-in, in a town it goes through later) when the route gets better for it.
    pub(crate) fn keep_log_quests(&self, mut route: Vec<Stop>) -> Vec<Stop> {
        let turned_in = |route: &[Stop], q: u32| route.contains(&Stop::quest(q, Kind::TurnIn));
        if (self.model.initial.accepted.iter()).all(|&i| turned_in(&route, i as u32)) {
            return route;
        }
        let Some(mut best) = self.simulate(&route, None) else {
            return route;
        };
        for &i in &self.model.initial.accepted {
            let q = i as u32;
            if self.aborted() || turned_in(&route, q) {
                continue;
            }
            let mut candidate: Vec<Stop> = (route.iter().copied())
                .filter(|s| !(s.is_quest() && s.index == q))
                .collect();
            self.insert_quest(&mut candidate, q);
            if let Some(t) = self.simulate(&candidate, None)
                && t < best
            {
                (route, best) = (candidate, t);
            }
        }
        route
    }

    /// Class quests that give power (a pet, a form) done as soon as taken: their objectives and
    /// turn-in moved to the earliest place after their accept costing at most
    /// `class_power_slack` more than the route. The local search moves one stop or block at a
    /// time and cannot find it alone.
    pub(crate) fn power_first(&self, mut route: Vec<Stop>) -> Vec<Stop> {
        let Some(mut total) = self.simulate(&route, None) else {
            return route;
        };
        let quests: Vec<u32> = route
            .iter()
            .filter(|s| s.kind == Kind::Accept && self.quest(s.index).power.is_some())
            .map(|s| s.index)
            .collect();
        for q in quests {
            let work = |s: &Stop| s.index == q && matches!(s.kind, Kind::Objective(_) | Kind::TurnIn);
            let moved: Vec<Stop> = route.iter().copied().filter(|s| work(s)).collect();
            let rest: Vec<Stop> = route.iter().copied().filter(|s| !work(s)).collect();
            let Some(at) = rest.iter().position(|s| s.kind == Kind::Accept && s.index == q) else {
                continue;
            };
            // Where it is now (the same index in `rest`): never later than that.
            let Some(now) = route.iter().position(work) else {
                continue;
            };
            // Right after the accept, or a few stops later (once the mobs are within reach).
            for p in at + 1..now.min(at + 40).min(rest.len()) {
                let mut candidate = rest.clone();
                candidate.splice(p..p, moved.iter().copied());
                if let Some(t) = self.simulate(&candidate, None)
                    && t <= total + self.params.class_power_slack
                {
                    total = t;
                    route = candidate;
                    break;
                }
            }
        }
        route
    }

    /// Required quests not turned in yet (gated by level or prerequisites, or taken and left in
    /// the log when the construction reached the target level): inserted whole where they fit.
    pub(crate) fn insert_missing_mandatory(&self, mut route: Vec<Stop>) -> Vec<Stop> {
        let n = self.model.quests.len() as u32;
        let turned = |route: &[Stop], i: u32| route.iter().any(|s| s.kind == Kind::TurnIn && s.index == i);
        let mut missing: Vec<u32> = (0..n)
            .filter(|&i| self.quest(i).mandatory && !turned(&route, i))
            .collect();
        // Prerequisites first (a chain of quests of one level, a forced zone's), then by level.
        let depth = self.chain_depths();
        missing.sort_by_key(|&i| (depth[i as usize], self.quest(i).min_level));
        for q in missing {
            let Some((total, end)) = self.simulate_state(&route, None) else {
                break;
            };
            let mut candidate: Vec<Stop> = route
                .iter()
                .copied()
                .filter(|s| !s.is_quest() || s.index != q)
                .collect();
            let Some(run) = self.run(&candidate) else { continue };
            // Not before the level it needs, nor before its prerequisites.
            let quest = self.quest(q);
            let after = run
                .states
                .iter()
                .position(|st| st.level >= quest.min_level && self.availability().prereqs_met(st, q))
                .unwrap_or(candidate.len())
                .min(candidate.len());
            self.insert_quest_after(&mut candidate, q, after);
            if self
                .simulate_state(&candidate, None)
                .is_some_and(|(cost, state)| cost < total || quest.required && state.required_left < end.required_left)
            {
                route = candidate;
            }
        }
        route
    }

    /// How many quests come before each one in its chain (prerequisites of prerequisites...).
    fn chain_depths(&self) -> Vec<u32> {
        fn of(
            i: usize,
            quests: &[crate::model::Quest],
            index: &rustc_hash::FxHashMap<i64, usize>,
            depth: &mut [Option<u32>],
            seen: u32,
        ) -> u32 {
            if let Some(d) = depth[i] {
                return d;
            }
            // A cycle in the data stops here.
            if seen > 64 {
                return 0;
            }
            let q = &quests[i];
            let d = (q.pre_all.iter().chain(&q.pre_any))
                .filter_map(|p| index.get(p))
                .map(|&j| of(j, quests, index, depth, seen + 1) + 1)
                .max()
                .unwrap_or(0);
            depth[i] = Some(d);
            d
        }
        let quests = &self.model.quests;
        let mut depth: Vec<Option<u32>> = vec![None; quests.len()];
        (0..quests.len())
            .map(|i| of(i, quests, &self.model.index, &mut depth, 0))
            .collect()
    }

    /// Index right after the stop closest to `pos`, not before `after`.
    pub(crate) fn insert_near(&self, route: &[Stop], pos: &Pos, after: usize) -> usize {
        let s = self.initial_state();
        // Each distance once (the first of the nearest, as `min_by` keeps).
        (after..route.len())
            .map(|i| (i, self.pos_from_start(&s, route[i]).dist(pos)))
            .min_by(|a, b| a.1.total_cmp(&b.1))
            .map_or(route.len(), |(i, _)| i + 1)
    }

    /// Insert all the stops of quest `q`, each after the stop closest to its place. Returns the
    /// first index of the route it changed.
    pub(crate) fn insert_quest(&self, route: &mut Vec<Stop>, q: u32) -> usize {
        self.insert_quest_after(route, q, 0)
    }

    /// Insert quest `q` as `insert_quest`, its accept not before `after`. Returns the first index
    /// changed: the accept, or the dungeon run moved after it from earlier in the route. A quest
    /// of the character's log has no accept, nor the objectives it has done.
    pub(crate) fn insert_quest_after(&self, route: &mut Vec<Stop>, q: u32, after: usize) -> usize {
        let quest = self.quest(q);
        let in_log = self.in_log_at_start(q);
        // First index the next stop may take.
        let mut next = after;
        let mut first = usize::MAX;
        if !in_log {
            let at = self.insert_near(route, &quest.starts[0].pos, after);
            route.insert(at, Stop::quest(q, Kind::Accept));
            (first, next) = (at, at + 1);
        }
        for k in 0..quest.objectives.len() {
            if in_log && self.done_at_start(q, k) {
                continue;
            }
            if let Some(d) = quest.objectives[k].dungeon {
                // Done by the dungeon run, which must come after accepting.
                let run = Stop {
                    index: d as u32,
                    kind: Kind::Dungeon,
                };
                match route.iter().position(|s| *s == run) {
                    Some(p) if p >= next => next = p + 1,
                    found => {
                        if let Some(p) = found {
                            route.remove(p);
                            first = first.min(p);
                            next -= 1;
                        }
                        let r = self.insert_near(route, &quest.objectives[k].loc.pos, next).max(next);
                        route.insert(r, run);
                        first = first.min(r);
                        next = r + 1;
                    }
                }
                continue;
            }
            let at = self.insert_near(route, &quest.objectives[k].loc.pos, next).max(next);
            route.insert(at, Stop::quest(q, Kind::Objective(k as u8)));
            first = first.min(at);
            next = at + 1;
        }
        let at = self.insert_near(route, &quest.ends[0].pos, next).max(next);
        route.insert(at, Stop::quest(q, Kind::TurnIn));
        first.min(at)
    }

    /// Quest `q` is in the character's log at the start.
    pub(crate) fn in_log_at_start(&self, q: u32) -> bool {
        self.model.initial.accepted.contains(&(q as usize))
    }

    /// Objective `k` of quest `q` is done in the character's log at the start.
    fn done_at_start(&self, q: u32, k: usize) -> bool {
        (self.model.initial.progress.iter())
            .any(|&(i, o, share)| i == q as usize && usize::from(o) == k && share >= 1.0)
    }

    /// Insert a quest: accepted where the character is around its level (closest to the giver),
    /// objectives and turn-in at the closest places within the following stretch.
    pub(crate) fn insert_quest_leveled(&self, route: &mut Vec<Stop>, levels: &mut Vec<i64>, q: u32) -> usize {
        let quest = self.quest(q);
        let (lo, hi) = (quest.min_level.max(quest.level - 3), quest.level + 2);
        let s0 = self.initial_state();
        let level = |levels: &Vec<i64>, p: usize| {
            if levels.is_empty() {
                1
            } else {
                levels[p.min(levels.len() - 1)]
            }
        };
        let giver = quest.starts[0].pos;
        // Each distance once (the first of the nearest, as `min_by` keeps).
        // A quest of the log is not accepted: its stops go where they fit best.
        let at = (0..=route.len())
            .filter(|_| !self.in_log_at_start(q))
            .filter(|&p| (lo..=hi).contains(&level(levels, p)))
            .map(|p| {
                let d = if p == 0 {
                    f64::INFINITY
                } else {
                    self.pos_from_start(&s0, route[p - 1]).dist(&giver)
                };
                (p, d)
            })
            .min_by(|a, b| a.1.total_cmp(&b.1))
            .map(|(p, _)| p);
        let Some(mut at) = at else {
            let first = self.insert_quest(route, q);
            // Keep levels aligned (approximate: copy the neighbour's level).
            for p in 0..route.len() {
                if levels.len() < route.len() && route[p].is_quest() && route[p].index == q {
                    let l = level(levels, p);
                    levels.insert(p, l);
                }
            }
            return first;
        };
        let first = at;
        let put = |route: &mut Vec<Stop>, levels: &mut Vec<i64>, p: usize, stop: Stop| {
            let l = level(levels, p);
            route.insert(p, stop);
            levels.insert(p, l);
        };
        put(route, levels, at, Stop::quest(q, Kind::Accept));
        let window = |from: usize, route: &Vec<Stop>| (from, (from + 300).min(route.len()));
        let nearest = |route: &Vec<Stop>, pos: &Pos, from: usize| {
            let (a, b) = window(from, route);
            (a..b)
                .map(|x| (x, self.pos_from_start(&s0, route[x]).dist(pos)))
                .min_by(|x, y| x.1.total_cmp(&y.1))
                .map_or(from, |(i, _)| i + 1)
        };
        for k in 0..quest.objectives.len() {
            at = nearest(route, &quest.objectives[k].loc.pos, at + 1).max(at + 1);
            put(route, levels, at, Stop::quest(q, Kind::Objective(k as u8)));
        }
        at = nearest(route, &quest.ends[0].pos, at + 1).max(at + 1);
        put(route, levels, at, Stop::quest(q, Kind::TurnIn));
        first
    }

    /// A dungeon run worth doing now: some quests of the log need it, and every quest of the
    /// dungeon that can be picked up at this level has been picked up first.
    pub(crate) fn dungeon_ready(&self, s: &State, d: usize) -> Option<Stop> {
        let dungeon = &self.model.dungeons[d];
        if !self.availability().dungeon_allowed(d) {
            return None;
        }
        if s.dungeons & (1u128 << d) != 0 || s.level < dungeon.min_level || s.level > dungeon.max_level {
            return None;
        }
        let mut needed = false;
        for i in self.availability().dungeon_quests(d) {
            let iu = i as usize;
            if s.accepted[iu] && !s.turned[iu] {
                needed = true;
            } else if !s.turned[iu]
                && s.log < self.params.quest_log_size
                && self.quest(i).min_level <= s.level
                && self.availability().can_accept(s, i)
                && self.worth().worth_it(s, i, s.level, 0.0)
            {
                return None; // pick it up first
            }
        }
        (needed || self.availability().dungeon_forced(d)).then_some(Stop {
            index: d as u32,
            kind: Kind::Dungeon,
        })
    }
}
