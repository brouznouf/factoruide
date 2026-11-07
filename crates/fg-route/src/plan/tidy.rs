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

/// Stops this close (yards) to the first of a run of stops are one visit of a place.
const VISIT_RADIUS: f64 = 400.0;
/// How many visits back a visit is looked to be merged into.
const VISITS_BACK: usize = 12;

impl Planner<'_> {
    /// `route` with each visit of a place (a run of stops within `VISIT_RADIUS` of its first one)
    /// moved whole to the end of one of the `VISITS_BACK` visits of the same place before it, when
    /// that makes the route better: a return to a place for what could have been done there
    /// before (Gadgetzan's quests and inn after the water survey at its gates). The search moves
    /// a stop or a few: a return held by several quests stays. The route and its score, or None
    /// when nothing moved.
    pub(crate) fn merge_visits(&self, route: &[Stop]) -> Option<(Vec<Stop>, f64)> {
        let mut run = self.run(route)?;
        let mut route = route.to_vec();
        let mut merged = false;
        let mut v = 1;
        while !self.aborted() {
            // Where the character stands after each stop done, and the visits: (first, last).
            let pos = |k: usize| run.states[k + 1].pos;
            let mut visits: Vec<(usize, usize)> = Vec::new();
            for k in 0..run.states.len() - 1 {
                match visits.last_mut() {
                    Some(last) if pos(last.0).dist(&pos(k)) <= VISIT_RADIUS => last.1 = k,
                    _ => visits.push((k, k)),
                }
            }
            let Some(&(a, b)) = visits.get(v) else { break };
            // Not into the visit right before: that is moving nothing.
            let found = (v.saturating_sub(VISITS_BACK)..v.saturating_sub(1)).find_map(|w| {
                let (c, d) = visits[w];
                if pos(c).dist(&pos(a)) > VISIT_RADIUS {
                    return None;
                }
                let mut candidate = route.clone();
                let visit: Vec<Stop> = candidate.drain(a..=b).collect();
                candidate.splice((d + 1)..=d, visit);
                let (cost, _) = self.evaluate_within(&run, &candidate, c, b, true, None)?;
                (cost < run.total - 0.5).then_some(candidate)
            });
            match found.and_then(|candidate| Some((self.replace_run(&mut run, &route, &candidate)?, candidate))) {
                // The visits are counted again: the same index is the next visit now.
                Some((new, candidate)) => (route, run, merged) = (candidate, new, true),
                None => v += 1,
            }
        }
        merged.then_some((route, run.total))
    }
}
