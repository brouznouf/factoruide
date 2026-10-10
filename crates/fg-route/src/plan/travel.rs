//! The fastest way between two places: walking, boats and zeppelins, flights between known
//! flight paths, the hearthstone when it is ready and saves enough.

use super::state::{Leg, State, Via};
use crate::model::Model;
use crate::params::Params;
use crate::world::{Pos, World};
use std::cell::RefCell;
use std::collections::HashMap;
use std::rc::Rc;

/// Flight times (seconds) and prices (copper) between flight paths.
pub(crate) type FlightTable = (Vec<Vec<f64>>, Vec<Vec<f64>>);

/// Flight tables by set of known flight paths, computed once each.
pub(crate) type FlightTables = RefCell<HashMap<u128, Rc<FlightTable>>>;

#[derive(Clone, Copy)]
pub(crate) struct Trips<'t> {
    pub(crate) world: &'t World,
    pub(crate) params: &'t Params,
    pub(crate) model: &'t Model,
    pub(crate) flights: &'t FlightTables,
}

impl Trips<'_> {
    pub(crate) fn walk(&self, a: &Pos, b: &Pos, level: i64) -> f64 {
        self.world.walk_distance(a, b) * self.params.detour / self.params.speed(level)
    }

    /// Fastest trip from `from` to `b` without the hearthstone.
    pub(crate) fn leg(&self, from: &Pos, b: &Pos, level: i64, known: u128) -> (f64, Leg) {
        let walk = self.walk(from, b, level);
        let mut best = (walk, Leg::Walk);
        let links = &self.world.links;
        if !links.is_empty() && (from.continent != b.continent || walk > 240.0) {
            for (i, li) in links.iter().enumerate() {
                let wa = self.walk(from, &li.from, level);
                if !wa.is_finite() || wa >= best.0 {
                    continue;
                }
                for (j, lj) in links.iter().enumerate() {
                    let t = wa + self.world.link_chain[i][j] + self.walk(&lj.to, b, level);
                    if t < best.0 {
                        best = (t, Leg::Link(i, j));
                    }
                }
            }
        }
        if known.count_ones() < 2 {
            return best;
        }
        let flights = self.flight_table(known);
        let (flights, prices) = (&flights.0, &flights.1);
        // Flights weigh more than their time (flight_weight) and cost gold (gold_weight): the
        // trip is chosen on that weighted time, its real time is returned.
        let mut weighted = best.0;
        let nearest = |p: &Pos| {
            let mut v: Vec<(usize, f64)> = (0..self.world.taxi_nodes.len())
                .filter(|i| known & (1u128 << i) != 0)
                .map(|i| (i, self.walk(p, &self.world.taxi_nodes[i].pos, level)))
                .filter(|(_, d)| d.is_finite())
                .collect();
            v.sort_by(|x, y| x.1.total_cmp(&y.1));
            v.truncate(3);
            v
        };
        for (i, wa) in nearest(from) {
            for (j, wb) in nearest(b) {
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

    /// Fastest trip from the current position, using the hearthstone when ready and faster.
    pub(crate) fn fastest(&self, s: &State, b: &Pos) -> (f64, Via) {
        let (t, leg) = self.leg(&s.pos, b, s.level, s.known);
        let mut best = (t, Via { hearth: false, leg });
        if s.time >= s.hearth_ready {
            let inn = &self.model.inns[s.bind].pos;
            let (t, leg) = self.leg(inn, b, s.level, s.known);
            let t = t + self.params.hearth_cast;
            if self.params.hearth_worth(best.0, t) {
                best = (t, Via { hearth: true, leg });
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
