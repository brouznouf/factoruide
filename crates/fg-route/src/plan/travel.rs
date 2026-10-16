//! The fastest way between two places: walking, boats and zeppelins, flights between known
//! flight paths, class teleports.

use super::state::{Leg, State, Via};
use crate::model::Model;
use crate::params::Params;
use crate::world::{Pos, World};
use rustc_hash::FxHashMap;
use std::cell::RefCell;
use std::rc::Rc;

/// Flight times (seconds) and prices (copper) between flight paths.
pub(crate) type FlightTable = (Vec<Vec<f64>>, Vec<Vec<f64>>);

/// Flight tables by set of known flight paths, computed once each.
pub(crate) type FlightTables = RefCell<FxHashMap<u128, Rc<FlightTable>>>;

/// A point as a key: links opened (6 bits), level (8 bits), continent (16 bits), zone (32 bits)
/// and a flag in one word, then the bits of its coordinates.
type PosKey = [u64; 3];

/// Trips already computed (from with mounted or not and the links opened, to, known flight
/// paths): a replay asks the same trips again and again. 80 bytes a trip; emptied when full.
pub(crate) type Legs = RefCell<FxHashMap<(PosKey, PosKey, u128), (f64, Hop)>>;
/// What a table of 2^18 buckets holds (7/8 of them): it never grows past them.
const LEGS_MAX: usize = (1 << 18) / 8 * 7;

/// A `Leg` as remembered (flight path and link indexes on 16 bits).
#[derive(Clone, Copy)]
pub(crate) enum Hop {
    Walk,
    Fly(u16, u16),
    Link(u16, u16),
}

impl Hop {
    fn of(leg: Leg) -> Option<Self> {
        let pair = |a: usize, b: usize| Some((u16::try_from(a).ok()?, u16::try_from(b).ok()?));
        Some(match leg {
            Leg::Walk => Self::Walk,
            Leg::Fly(a, b) => pair(a, b).map(|(a, b)| Self::Fly(a, b))?,
            Leg::Link(a, b) => pair(a, b).map(|(a, b)| Self::Link(a, b))?,
        })
    }

    fn leg(self) -> Leg {
        match self {
            Self::Walk => Leg::Walk,
            Self::Fly(a, b) => Leg::Fly(a.into(), b.into()),
            Self::Link(a, b) => Leg::Link(a.into(), b.into()),
        }
    }
}

/// The key of point `p` (None when its continent or zone does not fit: not remembered), with a
/// flag and a level (the danger's, 0 when it does not count).
fn pos_key(p: &Pos, flag: bool, level: u8) -> Option<PosKey> {
    let continent = u16::try_from(p.continent).ok()?;
    let zone = u32::try_from(p.zone).ok()?;
    let head = (u64::from(level) << 50) | (u64::from(continent) << 33) | (u64::from(zone) << 1) | u64::from(flag);
    Some([head, p.x.to_bits(), p.y.to_bits()])
}

#[derive(Clone, Copy)]
pub(crate) struct Trips<'t> {
    pub(crate) world: &'t World,
    pub(crate) params: &'t Params,
    pub(crate) model: &'t Model,
    pub(crate) flights: &'t FlightTables,
    pub(crate) legs: &'t Legs,
    /// On foot only: flights, boats and teleports are steps of their own (`Planner::explicit`).
    pub(crate) explicit: bool,
}

impl Trips<'_> {
    pub(crate) fn walk(&self, a: &Pos, b: &Pos, level: i64) -> f64 {
        self.world.walk_distance_at(a, b, self.danger_level(level)) * self.params.detour / self.params.speed(level)
    }

    /// The level the danger of walks is counted at (0: it does not count).
    pub(crate) fn danger_level(&self, level: i64) -> u8 {
        if self.params.danger_weight > 0.0 {
            level.clamp(1, 255) as u8
        } else {
            0
        }
    }

    /// The links `s` may take (see `World::link_chain`): those a quest opens once it is taken,
    /// or when the character cannot take it.
    pub(crate) fn open_links(&self, s: &State) -> u8 {
        (self.world.link_quests.iter().enumerate())
            .filter(|&(_, id)| self.model.index.get(id).is_none_or(|&q| s.accepted[q] || s.turned[q]))
            .fold(0, |open, (k, _)| open | 1 << k)
    }

    /// Fastest trip from `from` to `b` without the hearthstone, with the links of `open`.
    pub(crate) fn leg(&self, from: &Pos, b: &Pos, level: i64, known: u128, open: u8) -> (f64, Leg) {
        // The level only matters through the speed: mounted or not.
        let mounted = level >= self.params.mount_level;
        let (Some(mut a), Some(z)) = (pos_key(from, mounted, self.danger_level(level)), pos_key(b, false, 0)) else {
            return self.compute_leg(from, b, level, known, open);
        };
        a[0] |= u64::from(open) << 58;
        let key = (a, z, known);
        if let Some(&(t, hop)) = self.legs.borrow().get(&key) {
            return (t, hop.leg());
        }
        let leg = self.compute_leg(from, b, level, known, open);
        if let Some(hop) = Hop::of(leg.1) {
            let mut legs = self.legs.borrow_mut();
            if legs.len() >= LEGS_MAX {
                legs.clear();
            }
            legs.insert(key, (leg.0, hop));
        }
        leg
    }

    fn compute_leg(&self, from: &Pos, b: &Pos, level: i64, known: u128, open: u8) -> (f64, Leg) {
        let walk = self.walk(from, b, level);
        let mut best = (walk, Leg::Walk);
        let links = &self.world.links;
        let chain = self.world.link_chain(open);
        if !self.explicit && !links.is_empty() && (from.continent != b.continent || walk > 240.0) {
            // The walks from each link to `b`, once (when a link is worth boarding).
            let mut to_b: Vec<f64> = Vec::new();
            for (i, li) in links.iter().enumerate() {
                let wa = self.walk(from, &li.from, level);
                if !wa.is_finite() || wa >= best.0 {
                    continue;
                }
                if to_b.is_empty() {
                    to_b = links.iter().map(|lj| self.walk(&lj.to, b, level)).collect();
                }
                for (j, wb) in to_b.iter().enumerate() {
                    let t = wa + chain[i][j] + wb;
                    if t < best.0 {
                        best = (t, Leg::Link(i, j));
                    }
                }
            }
        }
        if known.count_ones() < 2 || self.explicit {
            return best;
        }
        let flights = self.flight_table(known);
        let (flights, prices) = (&flights.0, &flights.1);
        // Flights weigh more than their time (flight_weight) and cost gold (gold_weight): the
        // trip is chosen on that weighted time, its real time is returned.
        let mut weighted = best.0;
        // The 3 known flight paths nearest on foot (ties: the first). A walk is never shorter
        // than the straight line: the paths are walked to nearest first as the crow flies, until
        // the next one is farther in a straight line than the third nearest walk. Walked to
        // them from `from`, from them to `b` (`landing`): not the same both ways (drops).
        let nearest = |p: &Pos, landing: bool| {
            let line = |q: &Pos| p.dist(q) * self.params.detour / self.params.speed(level);
            let mut by_line: Vec<(usize, f64)> = (0..self.world.taxi_nodes.len())
                .filter(|i| known & (1u128 << i) != 0)
                .map(|i| (i, line(&self.world.taxi_nodes[i].pos)))
                .filter(|(_, d)| d.is_finite())
                .collect();
            by_line.sort_by(|x, y| x.1.total_cmp(&y.1));
            let mut v: Vec<(usize, f64)> = Vec::with_capacity(4);
            for (i, line) in by_line {
                if v.len() == 3 && line * (1.0 - 1e-9) > v[2].1 {
                    break;
                }
                let node = &self.world.taxi_nodes[i].pos;
                let walk = if landing {
                    self.walk(node, p, level)
                } else {
                    self.walk(p, node, level)
                };
                if walk.is_finite() {
                    let at = v.partition_point(|x| x.1.total_cmp(&walk).then(x.0.cmp(&i)).is_lt());
                    v.insert(at, (i, walk));
                    v.truncate(3);
                }
            }
            v
        };
        let near_b = nearest(b, true);
        for (i, wa) in nearest(from, false) {
            for &(j, wb) in &near_b {
                if i == j {
                    continue;
                }
                let t = wa + flights[i][j] + wb + self.params.flight_overhead;
                let w = t
                    + (self.params.flight_weight - 1.0) * (flights[i][j] + self.params.flight_overhead)
                    + self.params.gold_weight * prices[i][j] / 10_000.0;
                if w < weighted {
                    weighted = w;
                    best = (t, Leg::Fly(i, j));
                }
            }
        }
        best
    }

    /// Fastest trip from the current position, a class teleport first when learned (trained at
    /// its level) and faster. The hearthstone is a stop of its own (`Kind::Hearth`).
    pub(crate) fn fastest(&self, s: &State, b: &Pos) -> (f64, Via) {
        let open = self.open_links(s);
        let (t, leg) = self.leg(&s.pos, b, s.level, s.known, open);
        let mut best = (t, Via { teleport: None, leg });
        for (i, tp) in self.model.teleports.iter().enumerate() {
            if self.explicit {
                break;
            }
            // Not before it is learned, nor when it cannot be faster: landing on another
            // continent, or farther than the trip as the crow flies at flight speed.
            if !s.knows(tp)
                || tp.to.pos.continent != b.continent
                || self.params.teleport_cast + tp.to.pos.dist(b) / self.params.flight_speed >= best.0
            {
                continue;
            }
            let (t, leg) = self.leg(&tp.to.pos, b, s.level, s.known, open);
            let t = t + self.params.teleport_cast;
            if t < best.0 {
                best = (t, Via { teleport: Some(i), leg });
            }
        }
        best
    }

    pub(crate) fn flight_table(&self, known: u128) -> Rc<FlightTable> {
        self.flights
            .borrow_mut()
            .entry(known)
            .or_insert_with(|| Rc::new(self.world.flights(known)))
            .clone()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Points that differ only by their zone, or by being reached mounted, have their own key.
    #[test]
    fn keys_tell_points_apart() {
        let p = Pos {
            continent: 1,
            x: 10.0,
            y: 20.0,
            zone: 14,
        };
        let key = |p: &Pos, flag| pos_key(p, flag, 0).unwrap();
        assert_ne!(key(&p, false), key(&Pos { zone: 15, ..p }, false));
        assert_ne!(key(&p, false), key(&Pos { continent: 0, ..p }, false));
        assert_ne!(key(&p, false), key(&p, true));
        assert!(pos_key(&Pos { zone: -1, ..p }, false, 0).is_none());
    }
}
