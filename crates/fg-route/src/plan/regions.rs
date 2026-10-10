//! Region planning: quests are split into groups (a zone's quest hub and level band), each
//! group is done as a block, and the next group is chosen by expected XP per hour including
//! the trip there. Quests leading to another zone ("travel quests") are carried along and
//! favour groups where they can be finished.

use super::{Kind, Planner, State, Stop, fmt_time};
use crate::world::Pos;
use std::collections::HashMap;

/// Quest givers closer than this belong to the same hub.
const HUB_RADIUS: f64 = 500.0;
/// A level gap this large inside a hub starts a new group.
const LEVEL_GAP: i64 = 4;
/// A level band never spans more than this (quests a band apart are done at different times).
const MAX_BAND: i64 = 99;
/// Objectives and turn-ins of carried quests within this distance of a group are done there.
const GROUP_REACH: f64 = 1200.0;
/// Groups smaller than this are merged into a neighbour.
const MIN_GROUP: usize = 4;
/// Preference for staying in the current zone.
const SAME_ZONE_BONUS: f64 = 1.5;
/// Travel quests are taken when their destination has content up to this many levels ahead.
const TRAVEL_AHEAD: i64 = 8;

#[derive(Debug, Clone)]
pub struct Group {
    pub zone: i64,
    pub name: String,
    pub center: Pos,
    pub quests: Vec<u32>,
    pub min_level: i64,
    pub max_level: i64,
}

pub struct Groups {
    pub list: Vec<Group>,
    /// Group of each quest (by its quest giver).
    pub of_quest: Vec<usize>,
    /// Whether a quest leaves its group's zone (objectives or turn-in elsewhere).
    pub travel: Vec<bool>,
}

impl Planner<'_> {
    pub fn build_groups(&self) -> Groups {
        let quests = &self.model.quests;
        let travel: Vec<bool> = quests
            .iter()
            .map(|q| {
                let zone = q.starts[0].zone;
                q.objectives.iter().any(|o| o.loc.zone != zone) || q.ends.iter().all(|e| e.zone != zone)
            })
            .collect();

        // Hubs: leader clustering of quest givers per zone, densest givers first.
        let mut by_zone: HashMap<i64, Vec<u32>> = HashMap::new();
        for (i, q) in quests.iter().enumerate() {
            by_zone.entry(q.starts[0].zone).or_default().push(i as u32);
        }
        let mut groups = Vec::new();
        let mut of_quest = vec![0; quests.len()];
        let mut zones: Vec<_> = by_zone.into_iter().collect();
        zones.sort_by_key(|(z, _)| *z);
        for (zone, ids) in zones {
            let pos = |i: u32| quests[i as usize].starts[0].pos;
            let mut order = ids.clone();
            order.sort_by_key(|&i| {
                std::cmp::Reverse(ids.iter().filter(|&&j| pos(i).dist(&pos(j)) <= HUB_RADIUS).count())
            });
            let mut hubs: Vec<(Pos, Vec<u32>)> = Vec::new();
            for i in order {
                match hubs.iter_mut().find(|(c, _)| c.dist(&pos(i)) <= HUB_RADIUS) {
                    Some((_, members)) => members.push(i),
                    None => hubs.push((pos(i), vec![i])),
                }
            }
            // Level bands inside each hub.
            for (center, mut members) in hubs {
                members.sort_by_key(|&i| quests[i as usize].min_level);
                let mut band: Vec<u32> = Vec::new();
                let mut flush = |band: &mut Vec<u32>, groups: &mut Vec<Group>| {
                    if band.is_empty() {
                        return;
                    }
                    let levels = band.iter().map(|&i| quests[i as usize].min_level);
                    let (lo, hi) = (levels.clone().min().unwrap(), levels.max().unwrap());
                    for &i in band.iter() {
                        of_quest[i as usize] = groups.len();
                    }
                    groups.push(Group {
                        zone,
                        name: format!("{} {lo}-{hi}", self.world.zone_name(zone)),
                        center,
                        quests: std::mem::take(band),
                        min_level: lo,
                        max_level: hi,
                    });
                };
                for i in members {
                    let lvl = quests[i as usize].min_level;
                    let gap = band
                        .last()
                        .is_some_and(|&l| lvl - quests[l as usize].min_level >= LEVEL_GAP);
                    let wide = band
                        .first()
                        .is_some_and(|&f| lvl - quests[f as usize].min_level >= MAX_BAND);
                    if gap || wide {
                        flush(&mut band, &mut groups);
                    }
                    band.push(i);
                }
                flush(&mut band, &mut groups);
            }
        }
        // Tiny groups make the route hop around: fold them into a nearby group of the same zone
        // with overlapping levels.
        loop {
            let small = (0..groups.len())
                .find(|&g| groups[g].quests.len() < MIN_GROUP && Self::merge_target(&groups, g).is_some());
            let Some(g) = small else { break };
            let target = Self::merge_target(&groups, g).unwrap();
            let moved = groups.remove(g);
            let target = if target > g { target - 1 } else { target };
            let t = &mut groups[target];
            t.min_level = t.min_level.min(moved.min_level);
            t.max_level = t.max_level.max(moved.max_level);
            t.quests.extend(moved.quests);
            t.name = format!("{} {}-{}", self.world.zone_name(t.zone), t.min_level, t.max_level);
        }
        for (g, group) in groups.iter().enumerate() {
            for &i in &group.quests {
                of_quest[i as usize] = g;
            }
        }
        Groups {
            list: groups,
            of_quest,
            travel,
        }
    }

    fn merge_target(groups: &[Group], g: usize) -> Option<usize> {
        let me = &groups[g];
        (0..groups.len())
            .filter(|&o| o != g && groups[o].zone == me.zone)
            .filter(|&o| groups[o].min_level.min(me.min_level) + MAX_BAND >= groups[o].max_level.max(me.max_level))
            .filter(|&o| groups[o].center.dist(&me.center) <= GROUP_REACH * 1.5)
            .min_by(|&a, &b| {
                groups[a]
                    .center
                    .dist(&me.center)
                    .total_cmp(&groups[b].center.dist(&me.center))
            })
    }

    /// Whether a stop of a quest in the log can be done while working on group `g`.
    fn near_group(&self, s: &State, stop: Stop, g: &Group) -> bool {
        self.places().pos(s, stop).dist(&g.center) <= GROUP_REACH
    }

    /// A stop of a quest in the log that can be done now around group `g`.
    fn next_stop_near(&self, s: &State, i: u32, group: &Group) -> Option<Stop> {
        let iu = i as usize;
        if !s.accepted[iu] || s.turned[iu] {
            return None;
        }
        if self.availability().objectives_done(s, i) {
            let stop = Stop::quest(i, Kind::TurnIn);
            return self.near_group(s, stop, group).then_some(stop);
        }
        let q = self.quest(i);
        q.objectives.iter().enumerate().find_map(|(k, o)| {
            let stop = Stop::quest(i, Kind::Objective(k as u8));
            let level_ok = self.combat().need(q, o) <= self.params.reach(s.level, s.bonus);
            (s.objectives[iu] & (1 << k) == 0 && level_ok && o.dungeon.is_none() && self.near_group(s, stop, group))
                .then_some(stop)
        })
    }

    /// Quests of a group worth accepting now.
    fn doable(&self, s: &State, groups: &Groups, g: usize) -> Vec<u32> {
        groups.list[g]
            .quests
            .iter()
            .copied()
            .filter(|&i| {
                let q = self.quest(i);
                q.min_level <= s.level
                    && self.availability().can_accept(s, i)
                    && self.worth().worth_it(s, i, s.level, 0.0)
                    && (!groups.travel[i as usize] || self.travel_destination_ok(s, groups, i))
            })
            .collect()
    }

    /// Travel quests are taken when their destination has a group we will reach in the next
    /// levels (they often open a whole chain there): carried until we work around it.
    fn travel_destination_ok(&self, s: &State, groups: &Groups, i: u32) -> bool {
        let q = self.quest(i);
        let dest = q.ends[0].pos;
        q.mandatory
            || groups.list.iter().any(|g| {
                g.center.dist(&dest) <= GROUP_REACH * 2.0
                    && g.min_level <= s.level + TRAVEL_AHEAD
                    && g.max_level + 4 >= s.level
            })
    }

    /// Expected XP per second of going to group `g` now, and its doable quests.
    fn group_score(&self, s: &State, groups: &Groups, g: usize) -> Option<f64> {
        let group = &groups.list[g];
        let doable = self.doable(s, groups, g);
        let mut xp: f64 = doable.iter().map(|&i| self.worth().quest_worth(i, s.level)).sum();
        // Shared paths in a hub: standalone costs overestimate the time.
        let mut time: f64 = doable
            .iter()
            .map(|&i| self.worth().quest_cost(i, s.level, s.bonus))
            .sum::<f64>()
            * 0.6;
        // Carried quests that can progress here are almost free.
        let mut carried = 0;
        for i in 0..self.model.quests.len() as u32 {
            if let Some(stop) = self.next_stop_near(s, i, group) {
                carried += 1;
                let share = if stop.kind == Kind::TurnIn { 0.6 } else { 0.3 };
                xp += self.worth().quest_value(i, s.level) * share;
            }
        }
        if doable.is_empty() && carried == 0 {
            return None;
        }
        let trip = self.trips().fastest(s, &group.center).0;
        if !trip.is_finite() {
            return None;
        }
        time += trip + 60.0;
        let bonus = if group.zone == s.zone { SAME_ZONE_BONUS } else { 1.0 };
        Some(xp / time * bonus)
    }

    /// Work inside a group: nearest worthwhile stop among the group's quests and the carried
    /// quests that can be finished around it, until nothing is left at this level.
    fn work_group(&self, s: &mut State, route: &mut Vec<Stop>, groups: &Groups, g: usize) {
        let group = &groups.list[g];
        loop {
            if s.level >= self.profile.to_level {
                return;
            }
            let mut best: Option<(f64, Stop)> = None;
            let mut consider = |stop: Stop, weight: f64, s: &State| {
                let (t, _) = self.trips().fastest(s, &self.places().pos(s, stop));
                let score = (t + self.params.stop_overhead) * weight;
                if score.is_finite() && best.as_ref().is_none_or(|b| score < b.0) {
                    best = Some((score, stop));
                }
            };
            for (i, q) in self.model.quests.iter().enumerate() {
                let iu = i;
                let i = i as u32;
                if s.turned[iu] || !s.accepted[iu] {
                    continue;
                }
                if self.availability().objectives_done(s, i) {
                    let stop = Stop::quest(i, Kind::TurnIn);
                    if groups.of_quest[iu] == g || self.near_group(s, stop, group) {
                        consider(stop, 0.7, s);
                    }
                } else {
                    for (k, o) in q.objectives.iter().enumerate() {
                        let stop = Stop::quest(i, Kind::Objective(k as u8));
                        let level_ok = self.combat().need(q, o) <= self.params.reach(s.level, s.bonus);
                        if s.objectives[iu] & (1 << k) == 0
                            && level_ok
                            && o.dungeon.is_none()
                            && (groups.of_quest[iu] == g || self.near_group(s, stop, group))
                        {
                            consider(stop, 1.0, s);
                        }
                    }
                }
            }
            for i in self.doable(s, groups, g) {
                consider(Stop::quest(i, Kind::Accept), 1.3, s);
            }
            let Some((_, stop)) = best else { return };
            self.apply(s, stop, None);
            route.push(stop);
        }
    }

    /// Bind the hearthstone at the inn of a group we will stay in for a while.
    fn maybe_bind(&self, s: &mut State, route: &mut Vec<Stop>, groups: &Groups, g: usize) {
        let center = groups.list[g].center;
        let Some((inn, d)) = self
            .model
            .inns
            .iter()
            .enumerate()
            .filter(|(i, _)| *i != self.model.start_inn)
            .map(|(i, l)| (i, l.pos.dist(&center)))
            .min_by(|a, b| a.1.total_cmp(&b.1))
        else {
            return;
        };
        let doable = self.doable(s, groups, g).len();
        if d <= 600.0 && inn != s.bind && doable >= 5 {
            let stop = Stop {
                index: inn as u32,
                kind: Kind::Bind,
            };
            self.apply(s, stop, None);
            route.push(stop);
        }
    }

    /// Route built group by group.
    pub fn plan_regions(&self, log: impl Fn(&str)) -> Vec<Stop> {
        let groups = self.build_groups();
        log(&format!(
            "{} quest groups ({} travel quests)",
            groups.list.len(),
            groups.travel.iter().filter(|t| **t).count()
        ));
        let mut s = self.initial_state();
        let mut route = Vec::new();
        let verbose = std::env::var_os("FG_DEBUG").is_some();
        // Groups where nothing progressed, ignored until the next level.
        let mut stuck: Vec<bool> = vec![false; groups.list.len()];
        let mut stuck_level = s.level;
        while s.level < self.profile.to_level {
            if s.level != stuck_level {
                stuck.fill(false);
                stuck_level = s.level;
            }
            let best = self.choose_group(&s, &groups, &stuck);
            // Not better than grinding: grind to the next unlock instead.
            let best = best.filter(|(_, sc)| *sc >= 0.6 * self.combat().grind_rate(s.level, s.bonus, &s.pos));
            let Some((g, score)) = best else {
                // Nothing to do at this level anywhere: grind to the next unlock.
                let next = (0..self.model.quests.len() as u32)
                    .filter(|&i| self.availability().can_accept(&s, i) && self.quest(i).min_level > s.level)
                    .filter(|&i| {
                        self.trips()
                            .fastest(&s, &self.places().pos(&s, Stop::quest(i, Kind::Accept)))
                            .0
                            .is_finite()
                    })
                    .min_by_key(|&i| self.quest(i).min_level);
                match next {
                    Some(i) => {
                        let stop = Stop::quest(i, Kind::Accept);
                        self.apply(&mut s, stop, None);
                        route.push(stop);
                        continue;
                    }
                    None => break,
                }
            };
            if verbose {
                eprintln!(
                    "{} L{} -> {} ({:.1} xp/s)",
                    fmt_time(s.time),
                    s.level,
                    groups.list[g].name,
                    score
                );
            }
            let before = route.len();
            self.maybe_bind(&mut s, &mut route, &groups, g);
            self.work_group(&mut s, &mut route, &groups, g);
            if route.len() == before {
                stuck[g] = true;
            }
        }
        route
    }
}

impl Planner<'_> {
    /// Why plannable quests did not make it into a route (debugging aid).
    pub fn explain_unused(&self, route: &[Stop]) -> std::collections::BTreeMap<String, usize> {
        let mut reasons = std::collections::BTreeMap::new();
        let Some(run) = self.run(route) else { return reasons };
        let used: std::collections::HashSet<u32> = route.iter().filter(|s| s.is_quest()).map(|s| s.index).collect();
        let visited: std::collections::HashSet<i64> = run.states.iter().map(|s| s.zone).collect();
        for (i, q) in self.model.quests.iter().enumerate() {
            if used.contains(&(i as u32)) || q.level > self.profile.to_level {
                continue;
            }
            // State of the route when we were the quest's level.
            let Some(s) = run.states.iter().find(|s| s.level >= q.level.max(q.min_level)) else {
                continue;
            };
            let band = format!("{:02}", q.level / 10 * 10);
            let hardest = q
                .objectives
                .iter()
                .map(|o| self.combat().need(q, o))
                .fold(0.0, f64::max);
            let impossible = q.pre_all.iter().any(|p| !self.model.index.contains_key(p))
                || (!q.pre_any.is_empty() && q.pre_any.iter().all(|p| !self.model.index.contains_key(p)));
            let reason = if impossible {
                "prerequisite not plannable"
            } else if !self.availability().prereqs_met(s, q) {
                if std::env::var_os("FG_DEBUG_PREREQ").is_some() && q.level >= 45 {
                    let names: Vec<String> = q
                        .pre_all
                        .iter()
                        .chain(&q.pre_any)
                        .filter_map(|p| self.model.index.get(p).map(|&j| (p, j)))
                        .map(|(p, j)| {
                            let pq = &self.model.quests[j];
                            let used = used.contains(&(j as u32));
                            format!("{} ({p}, L{}, used={used})", pq.name, pq.level)
                        })
                        .collect();
                    eprintln!("    {} (L{}) needs: {}", q.name, q.level, names.join(" | "));
                }
                "prerequisites not done"
            } else if self.availability().blocked(s, q) {
                "exclusive"
            } else if hardest > self.params.reach(s.level + 1, s.bonus) {
                "mobs too high"
            } else if !self.worth().worth_it(s, i as u32, s.level, 0.0) {
                "not worth it"
            } else if !visited.contains(&q.starts[0].zone) {
                "zone never visited"
            } else {
                "skipped by planner"
            };
            if let Ok(ids) = std::env::var("FG_DEBUG_QUESTS")
                && ids.split(',').any(|x| x.trim() == q.id.to_string())
            {
                let starts: Vec<String> = q
                    .starts
                    .iter()
                    .map(|l| format!("{} in {}", l.name, self.world.zone_name(l.zone)))
                    .collect();
                eprintln!(
                    "    {} ({}, L{} min {}): {reason}; value {:.0} (+chain {:.0}), cost {:.0}s, starts {:?}",
                    q.name,
                    q.id,
                    q.level,
                    q.min_level,
                    self.worth().quest_value(i as u32, s.level),
                    self.model.unlocks[i],
                    self.worth().quest_cost(i as u32, s.level, s.bonus),
                    starts
                );
            }
            *reasons.entry(format!("{band} {reason}")).or_insert(0) += 1;
            if reason == "prerequisites not done" && std::env::var_os("FG_DEBUG_ROOT").is_some() {
                // Walk up the chain to the first quest not done.
                let mut j = i;
                for _ in 0..20 {
                    let qj = &self.model.quests[j];
                    let missing = qj
                        .pre_all
                        .iter()
                        .chain(&qj.pre_any)
                        .filter_map(|p| self.model.index.get(p).copied())
                        .find(|k| !used.contains(&(*k as u32)));
                    match missing {
                        Some(k) => j = k,
                        None => break,
                    }
                }
                let root = &self.model.quests[j];
                let zone = root
                    .starts
                    .first()
                    .map(|l| self.world.zone_name(l.zone).to_owned())
                    .unwrap_or_default();
                eprintln!(
                    "    blocked {} (L{}) <- root {} ({}, L{} min {}, {zone}, value {:.0} + chain {:.0}, cost {:.0}s)",
                    q.name,
                    q.level,
                    root.name,
                    root.id,
                    root.level,
                    root.min_level,
                    self.worth().quest_value(j as u32, root.level),
                    self.model.unlocks[j],
                    self.worth().quest_cost(j as u32, root.level, 0.0)
                );
            }
        }
        reasons
    }
}

/// Candidates simulated at each choice, and for the second step of the lookahead.
const SHORTLIST: usize = 8;
const SHORTLIST_NEXT: usize = 4;
/// Groups of the same zone chained when valuing a group (amortizes long trips).
const ZONE_LOOKAHEAD: usize = 3;

impl Planner<'_> {
    /// Total XP earned since level 1, to measure gains across level-ups.
    fn total_xp(&self, s: &State) -> f64 {
        (1..s.level).map(|l| self.rules().to_next_level(l) as f64).sum::<f64>() + s.xp as f64
    }

    /// Best groups by the quick estimate.
    fn shortlist(&self, s: &State, groups: &Groups, stuck: &[bool], n: usize) -> Vec<usize> {
        let mut scored: Vec<(usize, f64)> = (0..groups.list.len())
            .filter(|&g| !stuck[g])
            .filter_map(|g| self.group_score(s, groups, g).map(|sc| (g, sc)))
            .collect();
        scored.sort_by(|a, b| b.1.total_cmp(&a.1));
        scored.into_iter().take(n).map(|(g, _)| g).collect()
    }

    /// Simulate going to group `g` and doing what can be done there.
    fn try_group(&self, s: &State, groups: &Groups, g: usize) -> Option<State> {
        let mut t = s.clone();
        let mut scratch = Vec::new();
        self.maybe_bind(&mut t, &mut scratch, groups, g);
        let before = scratch.len();
        self.work_group(&mut t, &mut scratch, groups, g);
        (scratch.len() > before).then_some(t)
    }

    /// Choose the next group by simulated XP per second, looking one group further ahead.
    fn choose_group(&self, s: &State, groups: &Groups, stuck: &[bool]) -> Option<(usize, f64)> {
        let base_xp = self.total_xp(s);
        let mut best: Option<(usize, f64)> = None;
        for g in self.shortlist(s, groups, stuck, SHORTLIST) {
            let Some(s1) = self.try_group(s, groups, g) else {
                continue;
            };
            let rate = |st: &State| (self.total_xp(st) - base_xp) / (st.time - s.time).max(1.0);
            let mut value = rate(&s1);
            // A long trip pays off over the whole zone: keep working its best groups.
            let zone = groups.list[g].zone;
            let mut cur = s1.clone();
            let mut done = vec![g];
            for _ in 0..ZONE_LOOKAHEAD {
                let next = (0..groups.list.len())
                    .filter(|o| groups.list[*o].zone == zone && !done.contains(o) && !stuck[*o])
                    .filter_map(|o| self.try_group(&cur, groups, o).map(|st| (o, st)))
                    .max_by(|a, b| rate(&a.1).total_cmp(&rate(&b.1)));
                let Some((o, st)) = next else { break };
                value = value.max(rate(&st));
                done.push(o);
                cur = st;
            }
            if s1.level < self.profile.to_level {
                let none = vec![false; groups.list.len()];
                for g2 in self.shortlist(&s1, groups, &none, SHORTLIST_NEXT) {
                    if let Some(s2) = self.try_group(&s1, groups, g2) {
                        value = value.max(rate(&s2));
                    }
                }
            }
            if best.is_none_or(|b| value > b.1) {
                best = Some((g, value));
            }
        }
        best
    }
}
