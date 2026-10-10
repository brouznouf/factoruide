//! The small changes the local search tries on a route, one function each. Each returns the
//! range of the route it changed, or None when it found nothing to change.

use super::Planner;
use super::state::{Kind, Stop, same_item};
use rand::Rng;
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
    /// One of the small moves, drawn at random.
    pub(crate) fn random_move(&self, route: &mut Vec<Stop>, rng: &mut SmallRng) -> Option<Change> {
        match rng.random_range(0..23) {
            0..=9 => Some(Self::move_one_stop(route, rng)),
            10..=13 => Some(Self::move_block(route, rng)),
            14 | 15 => self.drop_quest_or_visit(route, rng),
            16 => self.add_visit(route, rng),
            20 => self.move_zone_stay(route, rng),
            21 | 22 => self.gather_zone_visits(route, rng),
            _ => self.add_unused_quest(route, rng),
        }
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
        let pick = route[rng.random_range(0..len)];
        if !pick.is_quest() {
            let first = route.iter().position(|&s| same_item(s, pick)).unwrap();
            route.retain(|&s| !same_item(s, pick));
            return Some(Change::edited(first, len));
        }
        let chain = self.availability().chain_after(route, pick.index)?;
        let gone = |s: &Stop| s.is_quest() && chain.contains(&s.index);
        let first = route.iter().position(gone).unwrap();
        route.retain(|s| !gone(s));
        Some(Change::edited(first, len))
    }

    /// A detour to a flight master, a dungeon run, or the hearthstone bound somewhere.
    fn add_visit(&self, route: &mut Vec<Stop>, rng: &mut SmallRng) -> Option<Change> {
        let (index, kind) = match rng.random_range(0..3) {
            0 if !self.world.taxi_nodes.is_empty() => {
                (rng.random_range(0..self.world.taxi_nodes.len()), Kind::LearnFlight)
            }
            1 if !self.model.dungeons.is_empty() => (rng.random_range(0..self.model.dungeons.len()), Kind::Dungeon),
            _ => (rng.random_range(0..self.model.inns.len()), Kind::Bind),
        };
        let stop = Stop {
            index: index as u32,
            kind,
        };
        if kind == Kind::Dungeon && route.contains(&stop) {
            return None;
        }
        let at = self.insert_near(route, &self.places().pos(&self.initial_state(), stop), 0);
        route.insert(at, stop);
        Some(Change::edited(at, route.len()))
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
