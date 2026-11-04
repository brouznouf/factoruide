//! The small changes the local search tries on a route, one function each. Each returns the
//! range of the route it changed, or None when it found nothing to change.

use super::Planner;
use super::evaluate::Run;
use super::hearth::INNS_TRIED;
use super::state::{Kind, Stop, same_item};
use crate::world::Pos;
use rand::RngExt;
use rand::rngs::SmallRng;

/// Changed stops of a candidate route: first and last index, and whether the stops were only
/// reordered (the incremental evaluation can then stop early).
#[derive(Clone, Copy)]
pub(crate) struct Change {
    pub(crate) first: usize,
    pub(crate) last: usize,
    pub(crate) reordered: bool,
}

impl Change {
    fn moved(first: usize, last: usize) -> Self {
        Self {
            first,
            last,
            reordered: true,
        }
    }

    fn edited(first: usize, len: usize) -> Self {
        Self {
            first,
            last: len,
            reordered: false,
        }
    }
}

impl Planner<'_> {
    /// Each dungeon's stops back together: a dungeon is entered once, so a move taking some of
    /// its stops elsewhere takes the others with them (the whole dungeon moves). `change` grows to
    /// what moved.
    pub(crate) fn gather_dungeons(&self, route: &mut Vec<Stop>, change: &mut Change) {
        let places = self.places();
        // Most moves leave each dungeon's stops together: one look at the route says so.
        let mut spans = vec![(usize::MAX, 0, 0); self.model.dungeons.len()];
        for (k, &stop) in route.iter().enumerate() {
            if let Some(d) = places.usual_inside(stop) {
                let (first, last, n) = &mut spans[d];
                *first = (*first).min(k);
                *last = k;
                *n += 1;
            }
        }
        if spans.iter().all(|&(first, last, n)| n == 0 || last - first + 1 == n) {
            return;
        }
        for d in places.entered(route) {
            let d = d as usize;
            let at: Vec<usize> = (0..route.len())
                .filter(|&k| places.usual_inside(route[k]) == Some(d))
                .collect();
            if at.windows(2).all(|w| w[1] == w[0] + 1) {
                continue;
            }
            // The part a move took apart (the smallest one: the stops it moved), the others joining it.
            let mut parts: Vec<&[usize]> = Vec::new();
            let mut from = 0;
            for n in 1..=at.len() {
                if n == at.len() || at[n] != at[n - 1] + 1 {
                    parts.push(&at[from..n]);
                    from = n;
                }
            }
            // Of parts as small, the one the move touched.
            let touched = |p: &[usize]| p.iter().any(|&k| (change.first..=change.last).contains(&k));
            let anchor = (parts.iter())
                .min_by_key(|p| (p.len(), !touched(p)))
                .map_or(at[0], |p| p[0]);
            let block: Vec<Stop> = at.iter().map(|&k| route[k]).collect();
            // Where the anchor lands once the others are taken out.
            let start = anchor - at.iter().filter(|&&k| k < anchor).count();
            for &k in at.iter().rev() {
                route.remove(k);
            }
            let first = at[0].min(start);
            for (n, stop) in block.into_iter().enumerate() {
                route.insert(start + n, stop);
            }
            change.first = change.first.min(first);
            change.last = change
                .last
                .max(at[at.len() - 1])
                .max(start + at.len() - 1)
                .min(route.len() - 1);
            change.reordered = true;
        }
    }
}

impl Planner<'_> {
    /// One of the small moves, drawn at random.
    pub(crate) fn random_move(&self, route: &mut Vec<Stop>, run: &Run, rng: &mut SmallRng) -> Option<Change> {
        match rng.random_range(0..25) {
            0..=9 => Some(Self::move_one_stop(route, rng)),
            10..=13 => Some(Self::move_block(route, rng)),
            14 | 15 => self.drop_quest_or_visit(route, rng),
            16 => self.add_visit(route, rng),
            17 => self.rebind_nearby(route, rng),
            18 => Some(self.move_hearth(route, rng)),
            20 => self.move_zone_stay(route, rng),
            21 | 22 => self.gather_zone_visits(route, rng),
            23 | 24 => self.relocate_objective(route, run, rng),
            _ => (self.swap_version(route, run, rng)).or_else(|| self.add_unused_quest(route, rng)),
        }
    }

    /// The version of a must-have quest in the route swapped for another (Undercity's voidwalker
    /// for Orgrimmar's): no other move can, a must-have quest staying and the other closed. Each
    /// quest of the new one goes where the route passes nearest, from the level it needs. Draws
    /// nothing without another version, one time in eight else: it shares its slot with adding a
    /// new quest, which a route needs far more often.
    pub(crate) fn swap_version(&self, route: &mut Vec<Stop>, run: &Run, rng: &mut SmallRng) -> Option<Change> {
        let in_route = |route: &[Stop], i: u32| route.iter().any(|s| s.is_quest() && s.index == i);
        let others = |i: u32| {
            (self.quest(i).exclusive.iter())
                .filter_map(|id| self.model.index.get(id))
                .map(|&j| j as u32)
                .filter(|&j| self.quest(j).mandatory)
        };
        let picks: Vec<(u32, u32)> = (route.iter())
            .filter(|s| s.kind == Kind::Accept && self.quest(s.index).mandatory)
            .flat_map(|s| others(s.index).map(|other| (s.index, other)))
            .filter(|&(_, other)| !in_route(route, other))
            .collect();
        if picks.is_empty() || !rng.random_bool(0.125) {
            return None;
        }
        let (old, new) = picks[rng.random_range(0..picks.len())];
        // The quests of `from`'s version that `takes` allows, walking its prerequisites and followers.
        let version = |from: u32, takes: &dyn Fn(u32, &[u32]) -> bool| {
            let mut quests = vec![from];
            let mut k = 0;
            while k < quests.len() {
                let i = quests[k];
                let q = self.quest(i);
                let before = (q.pre_all.iter().chain(&q.pre_any).chain(&q.pre_held))
                    .filter_map(|id| self.model.index.get(id))
                    .map(|&j| j as u32);
                let next: Vec<u32> = before
                    .chain(self.availability().followers[i as usize].iter().copied())
                    .filter(|&j| others(j).next().is_some())
                    .collect();
                for j in next {
                    if !quests.contains(&j) && takes(j, &quests) {
                        quests.push(j);
                    }
                }
                k += 1;
            }
            quests
        };
        let mut gone = version(old, &|j, _| in_route(route, j));
        // A breadcrumb to the old version leads nowhere now.
        let breadcrumbs: Vec<u32> = (route.iter())
            .filter(|s| s.kind == Kind::Accept && !self.quest(s.index).mandatory)
            .map(|s| s.index)
            .filter(|&i| {
                let target = self
                    .quest(i)
                    .breadcrumb_for
                    .as_ref()
                    .and_then(|id| self.model.index.get(id));
                target.is_some_and(|&t| gone.contains(&(t as u32)))
            })
            .collect();
        gone.extend(breadcrumbs);
        let is_gone = |s: &Stop| s.is_quest() && gone.contains(&s.index);
        let first_gone = route.iter().position(is_gone)?;
        let depth = self.chain_depths();
        // Of the choices a quest leaves (Call of Earth 1517 or 1520 after 1516), the first.
        let closed = |a: u32, b: u32| self.quest(a).exclusive.contains(&self.quest(b).id);
        let mut added = version(new, &|j, taken| !taken.iter().any(|&k| closed(j, k) || closed(k, j)));
        added.sort_by_key(|&i| depth[i as usize]);
        // Where each new quest's level is reached, in the route without the old version.
        let at_level: Vec<usize> = (added.iter())
            .map(|&i| {
                let at = (run.states.iter())
                    .position(|st| st.level >= self.quest(i).min_level)
                    .unwrap_or(route.len())
                    .min(route.len());
                route[..at].iter().filter(|s| !is_gone(s)).count()
            })
            .collect();
        route.retain(|s| !is_gone(s));
        let mut first = first_gone;
        for (&q, &level) in added.iter().zip(&at_level) {
            let quest = self.quest(q);
            let after_prerequisites = (quest.pre_all.iter().chain(&quest.pre_any).chain(&quest.pre_held))
                .filter_map(|id| self.model.index.get(id))
                .filter_map(|&p| Self::position_of(route, p as u32, Kind::TurnIn))
                .map(|at| at + 1)
                .max()
                .unwrap_or(0);
            first = first.min(self.insert_quest_after(route, q, level.max(after_prerequisites)));
        }
        Some(Change::edited(first, route.len()))
    }

    /// A stop moved, mostly nearby.
    fn move_one_stop(route: &mut Vec<Stop>, rng: &mut SmallRng) -> Change {
        let len = route.len();
        let i = rng.random_range(0..len);
        let stop = route.remove(i);
        let j = if rng.random_bool(0.8) {
            rng.random_range(i.saturating_sub(25)..=(i + 25).min(len - 1))
        } else {
            rng.random_range(0..len)
        };
        route.insert(j, stop);
        Change::moved(i.min(j), i.max(j))
    }

    /// An objective stop moved where it makes the shortest detour, between its quest's accept and
    /// turn-in: between two consecutive stops whose way passes by one of its places, or next to
    /// a stop done there (its mobs killed on the way, the objects gathered among another quest's
    /// mobs, as stops of their own). The places come from the current run.
    fn relocate_objective(&self, route: &mut Vec<Stop>, run: &Run, rng: &mut SmallRng) -> Option<Change> {
        let i = rng.random_range(0..route.len());
        let stop = route[i];
        let Kind::Objective(k) = stop.kind else {
            return None;
        };
        let o = &self.quest(stop.index).objectives[k as usize];
        let same = |s: &Stop, kind: Kind| s.index == stop.index && s.kind == kind;
        let from = route[..i]
            .iter()
            .rposition(|s| same(s, Kind::Accept))
            .map_or(0, |a| a + 1);
        let to = route[i + 1..]
            .iter()
            .position(|s| same(s, Kind::TurnIn))
            .map_or(route.len(), |t| i + 1 + t);
        // Where the character is after stop `j`, the stop moved aside.
        let after = |j: usize| run.states.get(j + 1).map(|s| s.pos);
        let detour = |a: &Pos, b: &Pos| {
            o.places()
                .map(|p| a.dist(&p.loc.pos) + p.loc.pos.dist(b))
                .fold(f64::INFINITY, f64::min)
                - a.dist(b)
        };
        // The stop goes between `j` and the next stop but itself (`j` = from - 1: first).
        let mut best: Option<(f64, usize)> = None;
        for j in from.saturating_sub(1)..to {
            if j == i {
                continue;
            }
            let next = if j + 1 == i { i + 1 } else { j + 1 };
            let (Some(a), Some(b)) = (after(j), after(next)) else {
                continue;
            };
            let d = detour(&a, &b);
            if d.is_finite() && best.is_none_or(|(x, _)| d < x) {
                best = Some((d, j));
            }
        }
        let (_, j) = best?;
        // Inserted after stop `j` (one index less once the stop is out, when after it).
        let at = if j < i { j + 1 } else { j };
        if at == i {
            return None;
        }
        route.remove(i);
        route.insert(at, stop);
        Some(Change::moved(i.min(at), i.max(at)))
    }

    /// A block of consecutive stops moved together (keeps local batches together).
    fn move_block(route: &mut Vec<Stop>, rng: &mut SmallRng) -> Change {
        let len = route.len();
        let size = rng.random_range(2..=6).min(len - 1);
        let i = rng.random_range(0..=len - size);
        let block: Vec<Stop> = route.drain(i..i + size).collect();
        let j = if rng.random_bool(0.7) {
            rng.random_range(i.saturating_sub(40)..=(i + 40).min(route.len()))
        } else {
            rng.random_range(0..=route.len())
        };
        for (k, s) in block.into_iter().enumerate() {
            route.insert(j + k, s);
        }
        Change::moved(i.min(j), (i + size).max(j + size) - 1)
    }

    /// A quest dropped with the rest of its chain in the route (dropping it alone would leave
    /// its follow-ups impossible), or a visit. Class quests stay.
    fn drop_quest_or_visit(&self, route: &mut Vec<Stop>, rng: &mut SmallRng) -> Option<Change> {
        let len = route.len();
        let at = rng.random_range(0..len);
        let pick = route[at];
        // A hearthstone stop alone: the others are other trips.
        if pick.kind == Kind::Hearth {
            route.remove(at);
            return Some(Change::edited(at, len));
        }
        if !pick.is_quest() {
            let first = route
                .iter()
                .position(|&s| same_item(s, pick))
                .expect("the pick is in the route");
            route.retain(|&s| !same_item(s, pick));
            return Some(Change::edited(first, len));
        }
        let chain = self.availability().chain_after(route, pick.index)?;
        let gone = |s: &Stop| s.is_quest() && chain.contains(&s.index);
        let first = route
            .iter()
            .position(gone)
            .expect("the chain contains a stop of the route");
        route.retain(|s| !gone(s));
        Some(Change::edited(first, len))
    }

    /// A detour to a flight master (`add_flight_visit`), a dungeon run, or the hearthstone
    /// bound (`rebind_nearby`).
    fn add_visit(&self, route: &mut Vec<Stop>, rng: &mut SmallRng) -> Option<Change> {
        let (index, kind) = match rng.random_range(0..3) {
            0 if !self.world.taxi_nodes.is_empty() => return self.add_flight_visit(route, rng),
            1 if !self.model.dungeons.is_empty() => (rng.random_range(0..self.model.dungeons.len()), Kind::Dungeon),
            _ => return self.rebind_nearby(route, rng),
        };
        let stop = Stop {
            index: index as u32,
            kind,
        };
        if kind == Kind::Dungeon && route.contains(&stop) {
            return None;
        }
        let at = self.insert_near(route, &self.places().pos(&self.sim().initial_state(), stop), 0);
        route.insert(at, stop);
        Some(Change::edited(at, route.len()))
    }

    /// A flight path drawn at random learned earlier than the route learns it: a detour to it
    /// after the stop closest to it before the route visits it, or passes close enough to pick
    /// it up (`flight_learn_radius`). The first time the route comes near (into its zone, or
    /// past it) is often much earlier than the stops at the flight master's town, and every
    /// trip in between may fly. A visit of the route to it later is moved there.
    fn add_flight_visit(&self, route: &mut Vec<Stop>, rng: &mut SmallRng) -> Option<Change> {
        let node = rng.random_range(0..self.world.taxi_nodes.len());
        let s0 = self.sim().initial_state();
        if s0.known & (1u128 << node) != 0 {
            return None;
        }
        let pos = self.world.taxi_nodes[node].pos;
        let visit = Stop {
            index: node as u32,
            kind: Kind::LearnFlight,
        };
        let learned = route
            .iter()
            .position(|&st| {
                st == visit
                    || (st.kind != Kind::LearnFlight
                        && self.sim().pos_from_start(&s0, st).dist(&pos) <= self.params.flight_learn_radius)
            })
            .unwrap_or(route.len());
        if learned == 0 {
            return None;
        }
        let at = (0..learned)
            .map(|i| (i, self.sim().pos_from_start(&s0, route[i]).dist(&pos)))
            .filter(|(_, d)| d.is_finite())
            .min_by(|a, b| a.1.total_cmp(&b.1))
            .map(|(i, _)| i + 1)?;
        let len = route.len();
        if route.get(learned) == Some(&visit) {
            route.remove(learned);
        }
        route.insert(at, visit);
        Some(Change::edited(at, len.max(route.len())))
    }

    /// The hearthstone bound at one of the inns closest to a stop drawn at random, after the stop
    /// around it closest to the inn; or a bind of the route moved to such an inn.
    fn rebind_nearby(&self, route: &mut Vec<Stop>, rng: &mut SmallRng) -> Option<Change> {
        const AROUND: usize = 15;
        let s0 = self.sim().initial_state();
        let k = rng.random_range(0..route.len());
        let inns = self.inns_near(&self.places().pos(&s0, route[k]), INNS_TRIED);
        let inn = *inns.get(rng.random_range(0..inns.len().max(1)))?;
        let bind = Stop {
            index: inn as u32,
            kind: Kind::Bind,
        };
        let (lo, hi) = (k.saturating_sub(AROUND), (k + AROUND).min(route.len()));
        if let Some(b) = (lo..hi).find(|&b| route[b].kind == Kind::Bind)
            && rng.random_bool(0.5)
        {
            route[b] = bind;
            return Some(Change::edited(b, route.len()));
        }
        let pos = self.model.inns[inn].pos;
        let at = (lo..hi)
            .min_by(|&x, &y| {
                let dx = self.places().pos(&s0, route[x]).dist(&pos);
                let dy = self.places().pos(&s0, route[y]).dist(&pos);
                dx.total_cmp(&dy)
            })
            .map_or(route.len(), |b| b + 1);
        route.insert(at, bind);
        Some(Change::edited(at, route.len()))
    }

    /// The hearthstone (or Astral Recall) used elsewhere: a use moved a few stops, or to the
    /// end of a stay in a zone (back to town once its quests are done), or a new one there.
    fn move_hearth(&self, route: &mut Vec<Stop>, rng: &mut SmallRng) -> Change {
        let len = route.len();
        let used: Vec<usize> = (0..len).filter(|&i| route[i].kind == Kind::Hearth).collect();
        let stays = self.stays(route);
        let stay_end = |rng: &mut SmallRng| stays[rng.random_range(0..stays.len())].1 + 1;
        if used.is_empty() || rng.random_bool(0.3) {
            let recall = self.model.recall.is_some() && rng.random_bool(0.5);
            let at = if rng.random_bool(0.5) {
                stay_end(rng)
            } else {
                rng.random_range(0..=len)
            };
            route.insert(
                at.min(len),
                Stop {
                    index: u32::from(recall),
                    kind: Kind::Hearth,
                },
            );
            return Change::edited(at.min(len), len + 1);
        }
        let from = used[rng.random_range(0..used.len())];
        let stop = route.remove(from);
        let to = if rng.random_bool(0.5) {
            stay_end(rng).min(route.len())
        } else {
            rng.random_range(from.saturating_sub(10)..=(from + 10).min(route.len()))
        };
        route.insert(to, stop);
        Change::moved(from.min(to), from.max(to))
    }

    /// An unused quest inserted next to the stops closest to its places.
    fn add_unused_quest(&self, route: &mut Vec<Stop>, rng: &mut SmallRng) -> Option<Change> {
        let q = rng.random_range(0..self.model.quests.len() as u32);
        if route.iter().any(|s| s.is_quest() && s.index == q) {
            return None;
        }
        let first = self.insert_quest(route, q);
        Some(Change::edited(first, route.len()))
    }

    /// A whole stay in a zone (its consecutive stops) moved to the boundary of another stay.
    fn move_zone_stay(&self, route: &mut Vec<Stop>, rng: &mut SmallRng) -> Option<Change> {
        let len = route.len();
        let zone = |s: &Stop| self.places().zone(*s);
        let i = rng.random_range(0..len);
        let z = zone(&route[i]);
        let (mut lo, mut hi) = (i, i);
        while lo > 0 && zone(&route[lo - 1]) == z && hi - lo < 150 {
            lo -= 1;
        }
        while hi + 1 < len && zone(&route[hi + 1]) == z && hi - lo < 150 {
            hi += 1;
        }
        let block: Vec<Stop> = route.drain(lo..=hi).collect();
        let size = block.len();
        let bounds: Vec<usize> = (0..=route.len())
            .filter(|&k| k == 0 || k == route.len() || zone(&route[k - 1]) != zone(&route[k]))
            .filter(|&k| k != lo)
            .collect();
        if bounds.is_empty() {
            return None;
        }
        let near: Vec<usize> = bounds.iter().copied().filter(|k| k.abs_diff(lo) <= 400).collect();
        let pool = if !near.is_empty() && rng.random_bool(0.8) {
            &near
        } else {
            &bounds
        };
        let j = pool[rng.random_range(0..pool.len())];
        for (k, s) in block.into_iter().enumerate() {
            route.insert(j + k, s);
        }
        Some(Change::moved(lo.min(j), (lo + size).max(j + size) - 1))
    }

    /// A short stay moved, as a block in its order, right next to the nearest other stay in
    /// the same zone (before or after it).
    fn gather_zone_visits(&self, route: &mut Vec<Stop>, rng: &mut SmallRng) -> Option<Change> {
        let stays = self.stays(route);
        let small: Vec<usize> = (0..stays.len()).filter(|&k| stays[k].1 - stays[k].0 < 12).collect();
        if small.is_empty() {
            return None;
        }
        let k = small[rng.random_range(0..small.len())];
        let (lo, hi, z) = stays[k];
        let before = stays[..k].iter().rposition(|st| st.2 == z);
        let after = stays[k + 1..].iter().position(|st| st.2 == z).map(|p| p + k + 1);
        let target = match (before, after) {
            (Some(b), Some(a)) => {
                if rng.random_bool(0.5) {
                    b
                } else {
                    a
                }
            }
            (Some(b), None) => b,
            (None, Some(a)) => a,
            (None, None) => return None,
        };
        let size = hi - lo + 1;
        let block: Vec<Stop> = route.drain(lo..=hi).collect();
        let at = if target < k {
            stays[target].1 + 1
        } else {
            stays[target].0 - size
        };
        route.splice(at..at, block);
        Some(Change::moved(lo.min(at), hi.max(at + size - 1)))
    }
}
