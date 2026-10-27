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

/// Death skips are planned below this level and a half (`Params::death_skips`): the game gives no
/// resurrection sickness before level 11, and a player a little ahead of the guide stays below it.
pub(crate) const DEATH_SKIP_UNTIL: f64 = 10.5;
/// Flight tables by set of known flight paths, computed once each.
pub(crate) type FlightTables = RefCell<FxHashMap<u128, Rc<FlightTable>>>;

/// A point as a key: links opened (6 bits), level (8 bits), continent (16 bits), zone (32 bits)
/// and a flag in one word, then the bits of its coordinates.
type PosKey = [u64; 4];

/// Trips already computed (from with mounted or not and the links opened, to, known flight
/// paths): a replay asks the same trips again and again. 88 bytes a trip; emptied when full.
pub(crate) type Legs = RefCell<FxHashMap<(PosKey, PosKey, u128), (f64, Hop)>>;
/// What a table of 2^18 buckets holds (7/8 of them): it never grows past them.
const LEGS_MAX: usize = (1 << 18) / 8 * 7;

/// A `Leg` as remembered (flight path and link indexes on 16 bits).
#[derive(Clone, Copy)]
pub(crate) enum Hop {
    Walk,
    Fly(u16, u16),
    Link(u16, u16),
    FlyLink([u16; 4]),
    LinkFly([u16; 4]),
}

impl Hop {
    fn of(leg: Leg) -> Option<Self> {
        let pair = |a: usize, b: usize| Some((u16::try_from(a).ok()?, u16::try_from(b).ok()?));
        let four = |a: usize, b: usize, c: usize, d: usize| {
            let ((a, b), (c, d)) = (pair(a, b)?, pair(c, d)?);
            Some([a, b, c, d])
        };
        Some(match leg {
            Leg::Walk => Self::Walk,
            Leg::Fly(a, b) => pair(a, b).map(|(a, b)| Self::Fly(a, b))?,
            Leg::Link(a, b) => pair(a, b).map(|(a, b)| Self::Link(a, b))?,
            Leg::FlyLink(a, b, c, d) => Self::FlyLink(four(a, b, c, d)?),
            Leg::LinkFly(a, b, c, d) => Self::LinkFly(four(a, b, c, d)?),
        })
    }

    fn leg(self) -> Leg {
        match self {
            Self::Walk => Leg::Walk,
            Self::Fly(a, b) => Leg::Fly(a.into(), b.into()),
            Self::Link(a, b) => Leg::Link(a.into(), b.into()),
            Self::FlyLink([a, b, c, d]) => Leg::FlyLink(a.into(), b.into(), c.into(), d.into()),
            Self::LinkFly([a, b, c, d]) => Leg::LinkFly(a.into(), b.into(), c.into(), d.into()),
        }
    }
}

/// The key of point `p` (None when its continent or zone does not fit: not remembered), with a
/// flag and a level (the walk level, or the band for the weight of gold; 0 when neither counts).
fn pos_key(p: &Pos, flag: bool, level: u8) -> Option<PosKey> {
    let continent = u16::try_from(p.continent).ok()?;
    let zone = u32::try_from(p.zone).ok()?;
    let head = (u64::from(level) << 50) | (u64::from(continent) << 33) | (u64::from(zone) << 1) | u64::from(flag);
    Some([head, p.x.to_bits(), p.y.to_bits(), p.height_key()])
}

/// A lower bound of `Trips::fastest` from a state: every walk is at least the straight line
/// (the terrain's paths and the passages never are shorter), so a trip takes at least the
/// straight line from where it starts (here, or a teleport's landing after its cast) or from
/// where a boat, a portal or a flight lands, after the least time to get there.
pub(crate) struct TripBound {
    per_yard: f64,
    /// Points the last walk starts from, and the least time to stand there.
    starts: Vec<(Pos, f64)>,
}

impl TripBound {
    /// At least how long the trip to `b` takes (infinite: it cannot be made).
    pub(crate) fn at(&self, b: &Pos) -> f64 {
        let t = (self.starts.iter())
            .map(|(p, t)| t + p.dist(b) * self.per_yard)
            .fold(f64::INFINITY, f64::min);
        // Below the sums of walks rounded otherwise.
        t * (1.0 - 1e-9)
    }
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
    /// Walks go into the zones closed to the character (`Planner::open_walks`).
    pub(crate) open_walks: bool,
}

impl Trips<'_> {
    pub(crate) fn walk(&self, a: &Pos, b: &Pos, level: i64) -> f64 {
        let closed = if self.open_walks {
            0
        } else {
            self.world.walk_level(level)
        };
        self.world.walk_distance_at(a, b, closed) * self.params.detour / self.params.speed(level)
    }

    /// The level a trip is remembered at (`Legs`): what its time and choice depend on apart
    /// from the speed, the zones closed to walks (`World::walk_level`) and the weight of gold
    /// (`Params::gold_weight_at`, by bands of 5 levels, the same up to level 30).
    fn leg_level(&self, level: i64) -> u8 {
        let band = ((level.clamp(1, 250) + 4) / 5 * 5) as u8;
        if self.params.gold_weight > 0.0 && f64::from(band) > crate::params::GOLD_WEIGHT_LEVEL {
            band
        } else {
            self.world.walk_level(level)
        }
    }

    /// The links `s` may take (see `World::link_chain`): those a quest opens once it is taken,
    /// or when the character cannot take it; those open only while a quest is in the log, while
    /// it is.
    pub(crate) fn open_links(&self, s: &State) -> u8 {
        let open = |&(id, held): &(i64, bool)| match self.model.index.get(&id) {
            Some(&q) if held => s.accepted[q] && !s.turned[q],
            Some(&q) => s.accepted[q] || s.turned[q],
            None => !held,
        };
        (self.world.link_quests.iter().enumerate())
            .filter(|(_, l)| open(l))
            .fold(0, |open, (k, _)| open | 1 << k)
    }

    /// Fastest trip from `from` to `b` without the hearthstone, with the links of `open`.
    pub(crate) fn leg(&self, from: &Pos, b: &Pos, level: i64, known: u128, open: u8) -> (f64, Leg) {
        // Walks open to the closed zones are not remembered with the others.
        if self.open_walks {
            return self.compute_leg(from, b, level, known, open);
        }
        // The level only matters through the speed: mounted or not.
        let mounted = level >= self.params.mount_level;
        let (Some(mut a), Some(z)) = (pos_key(from, mounted, self.leg_level(level)), pos_key(b, false, 0)) else {
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
            // A walk is never shorter than the straight line: the walks to a link and from
            // one to `b` (each once) only when they could make the trip shorter.
            let line = |p: &Pos, q: &Pos| p.dist(q) * self.params.detour / self.params.speed(level);
            let mut to_b: Vec<Option<f64>> = vec![None; links.len()];
            for (i, li) in links.iter().enumerate() {
                if line(from, &li.from) >= best.0 {
                    continue;
                }
                let wa = self.walk(from, &li.from, level);
                if !wa.is_finite() || wa >= best.0 {
                    continue;
                }
                for (j, lj) in links.iter().enumerate() {
                    if wa + chain[i][j] + line(&lj.to, b) >= best.0 {
                        continue;
                    }
                    let wb = *to_b[j].get_or_insert_with(|| self.walk(&lj.to, b, level));
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
        let overhead = self.params.flight_overhead;
        // Flights weigh more than their time (flight_weight) and cost gold (gold_weight, less
        // the higher the level): the trip is chosen on that weighted time, its real time is
        // returned.
        let gold = self.params.gold_weight_at(level) / 10_000.0;
        let extra =
            |i: usize, j: usize| (self.params.flight_weight - 1.0) * (flights[i][j] + overhead) + gold * prices[i][j];
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
        // The best flight from the paths `takeoffs` (walked to) to `landings` (walked from):
        // its time and weighted time.
        let fly = |takeoffs: &[(usize, f64)], landings: &[(usize, f64)]| {
            let mut best: Option<(f64, f64, usize, usize)> = None;
            for &(i, wa) in takeoffs {
                for &(j, wb) in landings {
                    if i == j || !flights[i][j].is_finite() {
                        continue;
                    }
                    let t = wa + flights[i][j] + overhead + wb;
                    let w = t + extra(i, j);
                    if best.is_none_or(|x| w < x.1) {
                        best = Some((t, w, i, j));
                    }
                }
            }
            best
        };
        let near_from = nearest(from, false);
        let near_b = nearest(b, true);
        if let Some((t, w, i, j)) = fly(&near_from, &near_b)
            && w < weighted
        {
            weighted = w;
            best = (t, Leg::Fly(i, j));
        }
        if links.is_empty() || (from.continent == b.continent && walk <= 240.0) {
            return best;
        }
        // A flight and a boat (or a zeppelin, a portal): flown to the port, or flown on from
        // where it lands. Each tried only when the straight lines could make the trip shorter.
        let line = |p: &Pos, q: &Pos| p.dist(q) * self.params.detour / self.params.speed(level);
        let least = |v: &[(usize, f64)]| v.iter().map(|x| x.1).fold(f64::INFINITY, f64::min);
        let (from_least, b_least) = (least(&near_from), least(&near_b));
        let mut to_b: Vec<Option<f64>> = vec![None; links.len()];
        let mut from_a: Vec<Option<f64>> = vec![None; links.len()];
        let mut ports: Vec<Option<Vec<(usize, f64)>>> = vec![None; links.len()];
        let mut docks: Vec<Option<Vec<(usize, f64)>>> = vec![None; links.len()];
        for (k, lk) in links.iter().enumerate() {
            for (l, ll) in links.iter().enumerate() {
                let ride = chain[k][l];
                if !ride.is_finite() {
                    continue;
                }
                // Flown to the port of `k`, then by boat to `l`'s landing, walked to `b`.
                if from_least + overhead + ride + line(&ll.to, b) < weighted {
                    let wb = *to_b[l].get_or_insert_with(|| self.walk(&ll.to, b, level));
                    if from_least + overhead + ride + wb < weighted {
                        let port = ports[k].get_or_insert_with(|| nearest(&lk.from, true));
                        if let Some((t, w, i, j)) = fly(&near_from, port)
                            && w + ride + wb < weighted
                        {
                            weighted = w + ride + wb;
                            best = (t + ride + wb, Leg::FlyLink(i, j, k, l));
                        }
                    }
                }
                // Walked to the port of `k`, by boat to `l`'s landing, flown on to `b`.
                if line(from, &lk.from) + ride + overhead + b_least < weighted {
                    let wa = *from_a[k].get_or_insert_with(|| self.walk(from, &lk.from, level));
                    if wa + ride + overhead + b_least < weighted {
                        let dock = docks[l].get_or_insert_with(|| nearest(&ll.to, false));
                        if let Some((t, w, i, j)) = fly(dock, &near_b)
                            && wa + ride + w < weighted
                        {
                            weighted = wa + ride + w;
                            best = (wa + ride + t, Leg::LinkFly(k, l, i, j));
                        }
                    }
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
        let mut best = (
            t,
            Via {
                teleport: None,
                death: None,
                leg,
            },
        );
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
                best = (
                    t,
                    Via {
                        teleport: Some(i),
                        death: None,
                        leg,
                    },
                );
            }
        }
        // Dying here, respawning at the nearest spirit healer.
        if let Some(g) = self.death_skip(s)
            && let gy = &self.model.graveyards[g]
            && gy.pos.continent == b.continent
            && self.params.death_skip_time + gy.pos.dist(b) * self.params.detour / self.params.speed(s.level) < best.0
        {
            let (t, leg) = self.leg(&gy.pos, b, s.level, s.known, open);
            let t = t + self.params.death_skip_time;
            if t < best.0 {
                best = (
                    t,
                    Via {
                        teleport: None,
                        death: Some(g),
                        leg,
                    },
                );
            }
        }
        best
    }

    /// The spirit healer a death skip from where the character stands respawns it at (index in
    /// `Model::graveyards`): the nearest, when death skips are on and every player is below
    /// `DEATH_SKIP_UNTIL` (none in the guide editor, whose steps say it).
    pub(crate) fn death_skip(&self, s: &State) -> Option<usize> {
        if !self.params.death_skips || self.explicit || !self.may_die(s) {
            return None;
        }
        self.graveyard(s)
    }

    /// Whether every player is below `DEATH_SKIP_UNTIL` (its level with its XP into the level).
    pub(crate) fn may_die(&self, s: &State) -> bool {
        let rules = self.params.edition.rules();
        let progress = |level: i64, xp: i64| level as f64 + xp as f64 / rules.to_next_level(level).max(1) as f64;
        (s.mates[..self.model.initial.mates.len()].iter())
            .map(|m| progress(m.level, m.xp))
            .fold(progress(s.level, s.xp), f64::max)
            < DEATH_SKIP_UNTIL
    }

    /// The spirit healer nearest to where the character stands (index in `Model::graveyards`).
    pub(crate) fn graveyard(&self, s: &State) -> Option<usize> {
        (self.model.graveyards.iter().enumerate())
            .filter(|(_, g)| g.pos.continent == s.pos.continent)
            .min_by(|(_, a), (_, b)| a.pos.dist(&s.pos).total_cmp(&b.pos.dist(&s.pos)))
            .map(|(g, _)| g)
    }

    /// What `fastest` from `s` takes at least to any place (`TripBound::at`), without walking
    /// the terrain.
    pub(crate) fn bound(&self, s: &State) -> TripBound {
        let per_yard = self.params.detour / self.params.speed(s.level);
        let line = |a: &Pos, b: &Pos| a.dist(b) * per_yard;
        // Where the trip starts: here, or where a class teleport lands.
        let mut starts = vec![(s.pos, 0.0)];
        if !self.explicit {
            starts.extend(
                (self.model.teleports.iter())
                    .filter(|tp| s.knows(tp))
                    .map(|tp| (tp.to.pos, self.params.teleport_cast)),
            );
        }
        if let Some(g) = self.death_skip(s) {
            starts.push((self.model.graveyards[g].pos, self.params.death_skip_time));
        }
        // Where a boat, a portal or a flight lands, and the least time to get there: flights
        // first, boats from where the trip starts or a flight lands, then flights from where
        // a boat lands (a trip takes a flight and boats, in either order, `compute_leg`).
        // The least time to stand at each of `boardings` from `origins` (points and the time
        // to be there).
        let arrive = |origins: &[(Pos, f64)], boardings: &[Pos]| -> Vec<f64> {
            (boardings.iter())
                .map(|p| {
                    (origins.iter())
                        .map(|(q, at)| at + line(q, p))
                        .fold(f64::INFINITY, f64::min)
                })
                .collect()
        };
        let flights = (!self.explicit && s.known.count_ones() >= 2).then(|| self.flight_table(s.known));
        let nodes: Vec<usize> = (0..self.world.taxi_nodes.len())
            .filter(|i| s.known & (1u128 << i) != 0)
            .collect();
        let takeoffs: Vec<Pos> = nodes.iter().map(|&i| self.world.taxi_nodes[i].pos).collect();
        // Where flights from `origins` land.
        let fly_from = |origins: &[(Pos, f64)], hubs: &mut Vec<(Pos, f64)>| {
            let Some(flights) = &flights else {
                return;
            };
            let at = arrive(origins, &takeoffs);
            for &j in &nodes {
                let t = (nodes.iter().zip(&at))
                    .filter(|&(&i, _)| i != j)
                    .map(|(&i, at)| at + flights.0[i][j] + self.params.flight_overhead)
                    .fold(f64::INFINITY, f64::min);
                if t.is_finite() {
                    hubs.push((self.world.taxi_nodes[j].pos, t));
                }
            }
        };
        let mut hubs = Vec::new();
        fly_from(&starts, &mut hubs);
        if !self.explicit && !self.world.links.is_empty() {
            let chain = self.world.link_chain(self.open_links(s));
            let boardings: Vec<Pos> = self.world.links.iter().map(|l| l.from).collect();
            let origins: Vec<(Pos, f64)> = starts.iter().chain(&hubs).copied().collect();
            let at = arrive(&origins, &boardings);
            let mut landed = Vec::new();
            for (j, lj) in self.world.links.iter().enumerate() {
                let t = (at.iter().enumerate())
                    .map(|(i, at)| at + chain[i][j])
                    .fold(f64::INFINITY, f64::min);
                if t.is_finite() {
                    landed.push((lj.to, t));
                }
            }
            fly_from(&landed, &mut hubs);
            hubs.extend(landed);
        }
        starts.extend(hubs);
        TripBound { per_yard, starts }
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

    /// Points that differ only by their zone, their height, or by being reached mounted, have
    /// their own key.
    #[test]
    fn keys_tell_points_apart() {
        let p = Pos {
            continent: 1,
            x: 10.0,
            y: 20.0,
            zone: 14,
            z: None,
        };
        let key = |p: &Pos, flag| pos_key(p, flag, 0).unwrap();
        assert_ne!(key(&p, false), key(&Pos { zone: 15, ..p }, false));
        assert_ne!(
            key(&p, false),
            key(&Pos { z: Some(0), ..p }, false),
            "a height on another floor"
        );
        assert_ne!(
            key(&Pos { z: Some(-3), ..p }, false),
            key(&Pos { z: Some(3), ..p }, false)
        );
        assert_ne!(key(&p, false), key(&Pos { continent: 0, ..p }, false));
        assert_ne!(key(&p, false), key(&p, true));
        assert!(pos_key(&Pos { zone: -1, ..p }, false, 0).is_none());
    }
}
