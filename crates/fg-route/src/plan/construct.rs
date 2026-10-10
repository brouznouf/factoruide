//! Building a first route: greedy or region by region, then repairs (class quests, pet quests first, settling).

use super::Planner;
use super::state::{Kind, State, Stop, fmt_time};
use crate::params::Construction;
use crate::world::Pos;

impl Planner<'_> {
    /// Greedy construction: nearest worthwhile stop first.
    pub fn greedy(&self) -> Vec<Stop> {
        let mut s = self.initial_state();
        let mut route = Vec::new();
        let n = self.model.quests.len() as u32;
        while s.level < self.profile.to_level {
            let mut best: Option<(f64, Stop)> = None;
            let found = std::cell::Cell::new(false);
            // Quest value only decides what is worth accepting. Weights favour finishing what
            // is in the log, and class quests.
            let mut consider = |stop: Stop, weight: f64, s: &State| {
                let (t, _) = self.trips().fastest(s, &self.places().pos(s, stop));
                // Finish the zone before leaving it (clear a zone, then move on).
                let stay = if self.places().zone(stop) == s.zone {
                    1.0
                } else {
                    self.params.zone_stay
                };
                // Same for another continent (a detour to Kalimdor while Eastern Kingdoms has more).
                let stay = if self.places().pos(s, stop).continent == s.pos.continent {
                    stay
                } else {
                    stay * self.params.continent_stay
                };
                let score = (t + self.params.stop_overhead) * weight * stay;
                if score.is_finite() && best.as_ref().is_none_or(|b| score < b.0) {
                    best = Some((score, stop));
                    found.set(true);
                }
            };
            for i in 0..n {
                let iu = i as usize;
                if s.turned[iu] {
                    continue;
                }
                let class = if self.quest(i).mandatory { 0.6 } else { 1.0 };
                if s.accepted[iu] {
                    if self.availability().objectives_done(&s, i) {
                        consider(Stop::quest(i, Kind::TurnIn), 0.7 * class, &s);
                    } else {
                        let q = self.quest(i);
                        for k in 0..q.objectives.len() {
                            let o = &q.objectives[k];
                            let level_ok = self.combat().need(q, o) <= self.params.reach(s.level, s.bonus);
                            if s.objectives[iu] & (1 << k) == 0 && level_ok && o.dungeon.is_none() {
                                consider(Stop::quest(i, Kind::Objective(k as u8)), class, &s);
                            }
                        }
                    }
                } else if self.quest(i).min_level <= s.level && self.availability().can_accept(&s, i) && {
                    let trip = self
                        .trips()
                        .fastest(&s, &self.places().pos(&s, Stop::quest(i, Kind::Accept)))
                        .0;
                    self.worth().worth_it(&s, i, s.level, trip)
                } {
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
                    consider(
                        Stop::quest(i, Kind::Accept),
                        1.3 * class
                            * dungeon
                            * preferred
                            * self.worth().level_fit(i, s.level)
                            * self.worth().efficiency_weight(&s, i, s.level),
                        &s,
                    );
                }
            }

            for d in 0..self.model.dungeons.len() {
                if let Some(stop) = self.dungeon_ready(&s, d) {
                    consider(
                        stop,
                        if self.availability().dungeon_forced(d) {
                            0.5
                        } else {
                            0.8
                        },
                        &s,
                    );
                }
            }

            // Zone exhausted: move on to the closest quest worth doing on its own, wherever it is.
            if !found.get() {
                for i in 0..n {
                    if self.quest(i).min_level <= s.level
                        && self.availability().can_accept(&s, i)
                        && self.worth().worth_it(&s, i, s.level, 0.0)
                    {
                        consider(Stop::quest(i, Kind::Accept), 1.0, &s);
                    }
                }
            }
            let next = if let Some((_, stop)) = best {
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
                            if s.objectives[iu] & (1 << k) == 0 && o.kills > 0.0 && o.dungeon.is_none() {
                                options.push((
                                    self.params.level_for(self.combat().need(q, o), s.bonus),
                                    Stop::quest(i, Kind::Objective(k as u8)),
                                ));
                            }
                        }
                    } else if self.availability().can_accept(&s, i)
                        && q.min_level > s.level
                        && (q.mandatory || self.worth().worth_it(&s, i, q.min_level, 0.0))
                    {
                        options.push((q.min_level, Stop::quest(i, Kind::Accept)));
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
            self.apply(&mut s, next, None);
            route.push(next);
        }

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
        let mut s = self.initial_state();
        s.planned = self.planned(route);
        let mut dropped = vec![false; self.model.quests.len()];
        let mut out = Vec::with_capacity(route.len());
        for (pos, &stop) in route.iter().enumerate() {
            let i = stop.index as usize;
            if stop.is_quest() && dropped[i] {
                continue;
            }
            if s.level >= self.profile.to_level {
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
        let route = if self.params.construction == Construction::Greedy {
            route
        } else {
            self.insert_missing_mandatory(self.settle(route))
        };
        let route = self.power_first(route);
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

    /// Class quests not reached yet (gated by level or prerequisites): insert them where they fit.
    pub(crate) fn insert_missing_mandatory(&self, mut route: Vec<Stop>) -> Vec<Stop> {
        let n = self.model.quests.len() as u32;
        let mut missing: Vec<u32> = (0..n)
            .filter(|&i| self.quest(i).mandatory && !route.iter().any(|s| s.is_quest() && s.index == i))
            .collect();
        missing.sort_by_key(|&i| self.quest(i).min_level);
        for q in missing {
            let Some(run) = self.run(&route) else { break };
            // Not before the level it needs, nor before its prerequisites.
            let quest = self.quest(q);
            let after = run
                .states
                .iter()
                .position(|st| st.level >= quest.min_level && self.availability().prereqs_met(st, quest))
                .unwrap_or(route.len())
                .min(route.len());
            let mut candidate = route.clone();
            self.insert_quest_after(&mut candidate, q, after);
            if self.simulate(&candidate, None).is_some_and(|c| c < run.total) {
                route = candidate;
            }
        }
        route
    }

    /// Index right after the stop closest to `pos`, not before `after`.
    pub(crate) fn insert_near(&self, route: &[Stop], pos: &Pos, after: usize) -> usize {
        let s = self.initial_state();
        (after..route.len())
            .min_by(|&a, &b| {
                let da = self.places().pos(&s, route[a]).dist(pos);
                let db = self.places().pos(&s, route[b]).dist(pos);
                da.total_cmp(&db)
            })
            .map_or(route.len(), |i| i + 1)
    }

    /// Insert all the stops of quest `q`, each after the stop closest to its place. Returns the
    /// first index of the route it changed.
    pub(crate) fn insert_quest(&self, route: &mut Vec<Stop>, q: u32) -> usize {
        self.insert_quest_after(route, q, 0)
    }

    /// Insert quest `q` as `insert_quest`, its accept not before `after`. Returns the first index
    /// changed: the accept, or the dungeon run moved after it from earlier in the route.
    pub(crate) fn insert_quest_after(&self, route: &mut Vec<Stop>, q: u32, after: usize) -> usize {
        let quest = self.quest(q);
        let mut at = self.insert_near(route, &quest.starts[0].pos, after);
        route.insert(at, Stop::quest(q, Kind::Accept));
        let mut first = at;
        for k in 0..quest.objectives.len() {
            if let Some(d) = quest.objectives[k].dungeon {
                // Done by the dungeon run, which must come after accepting.
                let run = Stop {
                    index: d as u32,
                    kind: Kind::Dungeon,
                };
                match route.iter().position(|s| *s == run) {
                    Some(p) if p > at => at = at.max(p),
                    Some(p) => {
                        route.remove(p);
                        first = first.min(p);
                        at -= 1;
                        let r = self
                            .insert_near(route, &quest.objectives[k].loc.pos, at + 1)
                            .max(at + 1);
                        route.insert(r, run);
                        at = r;
                    }
                    None => {
                        let r = self
                            .insert_near(route, &quest.objectives[k].loc.pos, at + 1)
                            .max(at + 1);
                        route.insert(r, run);
                        at = r;
                    }
                }
                continue;
            }
            at = self
                .insert_near(route, &quest.objectives[k].loc.pos, at + 1)
                .max(at + 1);
            route.insert(at, Stop::quest(q, Kind::Objective(k as u8)));
        }
        at = self.insert_near(route, &quest.ends[0].pos, at + 1).max(at + 1);
        route.insert(at, Stop::quest(q, Kind::TurnIn));
        first
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
        let at = (0..=route.len())
            .filter(|&p| (lo..=hi).contains(&level(levels, p)))
            .min_by(|&a, &b| {
                let d = |p: usize| {
                    if p == 0 {
                        f64::INFINITY
                    } else {
                        self.places().pos(&s0, route[p - 1]).dist(&giver)
                    }
                };
                d(a).total_cmp(&d(b))
            });
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
                .min_by(|&x, &y| {
                    self.places()
                        .pos(&s0, route[x])
                        .dist(pos)
                        .total_cmp(&self.places().pos(&s0, route[y]).dist(pos))
                })
                .map_or(from, |i| i + 1)
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
