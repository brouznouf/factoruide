//! Mobs met on the way: which deserve a line of their own in the guide.

use crate::plan::{Event, Kind, Timed};
use std::collections::HashSet;

/// Mobs met on the way not worth a line of their own (indexes in `trace`): the objective's own
/// stop comes within the next few stops (it is shown there), or the objective was already
/// shown on the way and is not done yet. Also returns the objective stops announced on the way
/// before.
pub(super) fn hidden_along(trace: &[Timed]) -> (HashSet<usize>, HashSet<usize>) {
    const SOON: usize = 3;
    let mut hidden = HashSet::new();
    let mut announced = HashSet::new();
    let mut shown = HashSet::new();
    for (n, t) in trace.iter().enumerate() {
        match &t.event {
            Event::Along { quest, objective, .. } => {
                let key = (*quest, *objective);
                let own_soon = trace[n + 1..]
                    .iter()
                    .filter_map(|t| match &t.event {
                        Event::Stop { stop, .. } => Some(stop),
                        _ => None,
                    })
                    .take(SOON)
                    .any(|stop| stop.index as usize == *quest && stop.kind == Kind::Objective(*objective));
                if own_soon || !shown.insert(key) {
                    hidden.insert(n);
                }
            }
            Event::Stop { stop, .. } => {
                if let Kind::Objective(k) = stop.kind
                    && shown.remove(&(stop.index as usize, k))
                {
                    announced.insert(n);
                }
            }
            _ => {}
        }
    }
    (hidden, announced)
}
