//! Large neighbourhood moves: a set of quests removed then reinserted where they fit, by
//! operators whose weights follow their recent success.

use super::Planner;
use super::evaluate::Run;
use super::state::{Kind, Stop};
use rand::Rng;
use rand::rngs::SmallRng;

/// Ruin-and-recreate operators of the large neighbourhood search (see `Params::alns`).
pub const ALNS_OPS: [&str; 6] = [
    "zone stay",
    "route window",
    "random quests",
    "late quests",
    "zone visits",
    "drop late quests",
];

/// The operators' weights, adapted every 100 uses to how often each improved the route.
pub(crate) struct Operators {
    weights: [f64; ALNS_OPS.len()],
    score: [f64; ALNS_OPS.len()],
    uses: [u32; ALNS_OPS.len()],
}

impl Operators {
    pub(crate) fn new() -> Self {
        Self {
            weights: [1.0; ALNS_OPS.len()],
            score: [0.0; ALNS_OPS.len()],
            uses: [0; ALNS_OPS.len()],
        }
    }

    pub(crate) fn pick(&mut self, rng: &mut SmallRng) -> usize {
        let total: f64 = self.weights.iter().sum();
        let mut pick = rng.random_range(0.0..total);
        let op = self
            .weights
            .iter()
            .position(|w| {
                pick -= w;
                pick < 0.0
            })
            .unwrap_or(0);
        self.uses[op] += 1;
        op
    }

    /// Credit an operator: 3 for a new best route, 1 for an improvement.
    pub(crate) fn reward(&mut self, op: usize, points: f64) {
        self.score[op] += points;
    }

    pub(crate) fn adapt(&mut self) {
        if self.uses.iter().sum::<u32>() % 100 != 0 {
            return;
        }
        for o in 0..ALNS_OPS.len() {
            if self.uses[o] > 0 {
                self.weights[o] =
                    (0.7 * self.weights[o] + 0.3 * (0.05 + self.score[o] / f64::from(self.uses[o]))).max(0.05);
            }
        }
        self.score = [0.0; ALNS_OPS.len()];
        self.uses = [0; ALNS_OPS.len()];
    }
}

impl Planner<'_> {
    /// Remove a set of quests (all their stops) chosen by operator `op`, then reinsert each
    /// where the character has its level, near its places. Returns the changed range.
    pub(crate) fn ruin_recreate(
        &self,
        route: &mut Vec<Stop>,
        run: &Run,
        op: usize,
        rng: &mut SmallRng,
    ) -> Option<(usize, usize, bool)> {
        let len = route.len();
        let level_at = |i: usize| run.states[i.min(run.states.len() - 1)].level;
        let removable = |s: &Stop| s.is_quest() && self.quest(s.index).objectives.iter().all(|o| o.dungeon.is_none());
        // Gather the visits of a zone made around the same level into one stay, at the place of
        // the longest one (each keeps its order): a zone done in one go instead of coming back.
        if op == 4 {
            let stays = self.stays(route);
            let (lo, _, z) = stays[rng.random_range(0..stays.len())];
            let level = level_at(lo);
            let members: Vec<usize> = (0..stays.len())
                .filter(|&m| stays[m].2 == z && (level_at(stays[m].0) - level).abs() <= 3)
                .collect();
            if members.len() < 2 {
                return None;
            }
            let anchor = *members.iter().max_by_key(|&&m| stays[m].1 - stays[m].0)?;
            let mut block = Vec::new();
            for &m in &members {
                block.extend_from_slice(&route[stays[m].0..=stays[m].1]);
            }
            let first = stays[members[0]].0;
            let last = stays[*members.last()?].1;
            let mut out = Vec::with_capacity(route.len());
            for (k, st) in stays.iter().enumerate() {
                if k == anchor {
                    out.extend_from_slice(&block);
                } else if !members.contains(&k) {
                    out.extend_from_slice(&route[st.0..=st.1]);
                }
            }
            *route = out;
            return Some((first, last, true));
        }
        // Give up quests turned in far below the character's level, with the rest of their
        // chain: reinserted at their level they would cost a long detour, kept they hardly pay.
        if op == 5 {
            let late: Vec<u32> = route
                .iter()
                .enumerate()
                .filter(|(i, s)| {
                    s.kind == Kind::TurnIn && removable(s) && level_at(*i) - self.quest(s.index).level >= 6
                })
                .map(|(_, s)| s.index)
                .collect();
            if late.is_empty() {
                return None;
            }
            let mut gone: Vec<u32> = Vec::new();
            for _ in 0..rng.random_range(1..=4) {
                let q = late[rng.random_range(0..late.len())];
                if let Some(chain) = self.availability().chain_after(route, q) {
                    for c in chain {
                        if !gone.contains(&c) {
                            gone.push(c);
                        }
                    }
                }
            }
            let first = route.iter().position(|s| s.is_quest() && gone.contains(&s.index))?;
            route.retain(|s| !(s.is_quest() && gone.contains(&s.index)));
            return Some((first, route.len(), false));
        }
        let mut remove: Vec<u32> = Vec::new();
        let add = |q: u32, remove: &mut Vec<u32>| {
            if !remove.contains(&q) {
                remove.push(q);
            }
        };
        match op {
            0 => {
                let i = rng.random_range(0..len);
                let z = self.places().zone(route[i]);
                let (mut lo, mut hi) = (i, i);
                while lo > 0 && self.places().zone(route[lo - 1]) == z && hi - lo < 150 {
                    lo -= 1;
                }
                while hi + 1 < len && self.places().zone(route[hi + 1]) == z && hi - lo < 150 {
                    hi += 1;
                }
                for s in &route[lo..=hi] {
                    if removable(s) {
                        add(s.index, &mut remove);
                    }
                }
            }
            1 => {
                let size = rng.random_range(20..=80).min(len);
                let i = rng.random_range(0..=len - size);
                for s in &route[i..i + size] {
                    if removable(s) {
                        add(s.index, &mut remove);
                    }
                }
            }
            2 => {
                for _ in 0..rng.random_range(4..=12) {
                    let s = route[rng.random_range(0..len)];
                    if removable(&s) {
                        add(s.index, &mut remove);
                    }
                }
            }
            _ => {
                let late: Vec<u32> = route
                    .iter()
                    .enumerate()
                    .filter(|(i, s)| {
                        s.kind == Kind::TurnIn && removable(s) && level_at(*i) - self.quest(s.index).level >= 5
                    })
                    .map(|(_, s)| s.index)
                    .collect();
                if late.is_empty() {
                    return None;
                }
                for _ in 0..rng.random_range(3..=12) {
                    add(late[rng.random_range(0..late.len())], &mut remove);
                }
            }
        }
        if remove.is_empty() {
            return None;
        }
        let first = route.iter().position(|s| s.is_quest() && remove.contains(&s.index))?;
        // Levels before each remaining stop, kept aligned while reinserting.
        let mut levels: Vec<i64> = Vec::with_capacity(len);
        let mut kept: Vec<Stop> = Vec::with_capacity(len);
        for (i, s) in route.iter().enumerate() {
            if !(s.is_quest() && remove.contains(&s.index)) {
                kept.push(*s);
                levels.push(level_at(i));
            }
        }
        *route = kept;
        remove.sort_by_key(|&q| self.quest(q).level);
        let mut changed = first;
        for q in remove {
            let at = self.insert_quest_leveled(route, &mut levels, q);
            changed = changed.min(at);
        }
        Some((changed, route.len(), false))
    }
}
