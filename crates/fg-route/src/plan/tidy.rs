//! The last tidying of a route: what can be done in a place before leaving it is not left for
//! the way back.

use super::Planner;
use super::state::{Kind, Stop};

/// How many stops back a stop is looked to be done earlier.
const BACK: usize = 40;

impl Planner<'_> {
    /// `route` with each stop done at the first visit of its place among the `BACK` stops before
    /// it, when the route went elsewhere in between (within `tidy_radius` of the stop is its
    /// place) and it costs at most `tidy_slack` seconds: the search sees no gain in it, the
    /// player sees a quest taken or the hearthstone bound on the way back where it could have
    /// been in passing. The route and its score, or None when nothing moved.
    pub(crate) fn tidy(&self, route: &[Stop]) -> Option<(Vec<Stop>, f64)> {
        let (slack, radius) = (self.params.tidy_slack, self.params.tidy_radius);
        if slack < 0.0 {
            return None;
        }
        let mut run = self.run(route)?;
        let mut route = route.to_vec();
        let mut moved = false;
        // The stops done: `states[k]` is where the character stands before stop `k`.
        let mut j = 1;
        while j + 1 < run.states.len() && !self.aborted() {
            if matches!(route[j].kind, Kind::Hearth | Kind::Dungeon) {
                j += 1;
                continue;
            }
            let at = run.states[j + 1].pos;
            let here = |k: usize| run.states[k].pos.dist(&at) <= radius;
            let mut found = None;
            for i in j.saturating_sub(BACK).max(1)..j {
                // Standing there before stop `i`, and going elsewhere before stop `j`.
                if !here(i) || (i + 1..=j).all(here) {
                    continue;
                }
                let mut candidate = route.clone();
                let stop = candidate.remove(j);
                candidate.insert(i, stop);
                if let Some((cost, _)) = self.evaluate_within(&run, &candidate, i, j, true, None)
                    && cost <= run.total + slack
                {
                    found = Some(candidate);
                    break;
                }
            }
            if let Some(candidate) = found
                && let Some(new) = self.replace_run(&mut run, &route, &candidate)
            {
                (route, run, moved) = (candidate, new, true);
            }
            j += 1;
        }
        moved.then_some((route, run.total))
    }
}
