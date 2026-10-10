//! When the local search keeps a candidate route: when it is better, when it beats the route
//! of `lahc` steps ago (late acceptance), or by chance early on (simulated annealing: moving a
//! whole zone earlier only pays once the rest follows).

use rand::Rng;
use rand::rngs::SmallRng;

pub(crate) struct Acceptance {
    temperature0: f64,
    history: Vec<f64>,
}

impl Acceptance {
    pub(crate) fn new(start: f64, anneal: f64, lahc: usize) -> Self {
        Self {
            temperature0: start * anneal,
            history: vec![start; lahc],
        }
    }

    /// Whether to move from the current route (`current`) to a candidate costing `cost`, at
    /// `progress` (0 to 1) of the search; `tries` picks the late acceptance slot.
    pub(crate) fn accepts(&self, cost: f64, current: f64, progress: f64, tries: u64, rng: &mut SmallRng) -> bool {
        let temperature = self.temperature0 * (1.0 - progress).max(0.0);
        cost < current - 0.5
            || (!self.history.is_empty() && cost.is_finite() && cost <= self.history[self.slot(tries)])
            || (temperature > 0.0
                && cost.is_finite()
                && rng.random_bool((-(cost - current) / temperature).exp().min(1.0)))
    }

    /// Remember the current route's cost in its late acceptance slot.
    pub(crate) fn remember(&mut self, current: f64, tries: u64) {
        if !self.history.is_empty() {
            let slot = self.slot(tries);
            self.history[slot] = current;
        }
    }

    fn slot(&self, tries: u64) -> usize {
        if self.history.is_empty() {
            0
        } else {
            tries as usize % self.history.len()
        }
    }
}
