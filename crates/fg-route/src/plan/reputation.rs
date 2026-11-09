//! Best-found selected faction totals and shortfalls from supported direct quest rewards.

use super::state::{Event, Kind};
use super::{Planner, Stop};
use std::collections::HashSet;

const SHORTFALL_PREFIX: &str = "reputation shortfall (best-found): ";

impl Planner<'_> {
    pub(crate) fn has_hard_goals(&self) -> bool {
        !self.model.bis.required_roots().is_empty() || !self.params.reputation_goals.is_empty()
    }

    pub(super) fn preserves_reputation(&self, before: &[f64], after: &[f64]) -> bool {
        self.params.reputation_goals.iter().enumerate().all(|(i, goal)| {
            before.get(i).copied().unwrap_or(0.0) < goal.target as f64
                || after.get(i).copied().unwrap_or(0.0) >= goal.target as f64
        })
    }

    pub(super) fn preserves_reputation_routes(&self, before: &[Stop], after: &[Stop]) -> bool {
        self.params.reputation_goals.is_empty()
            || self.preserves_reputation(&self.reputation_reached(before), &self.reputation_reached(after))
    }

    /// Continue construction from actual state, accepting costly useful work and its ancestors.
    /// Every insertion is replayed with the original route, preserving previously satisfied goals.
    pub fn repair_required_reputation(&self, route: Vec<Stop>) -> Vec<Stop> {
        if self.params.reputation_goals.is_empty() || self.aborted() {
            return route;
        }
        let Some((_, before)) = self.simulate_state(&route, None) else {
            return route;
        };
        if self.sim().reputation_goals_unmet(&before) == 0 {
            return route;
        }
        let candidate = self.greedy_from(route.clone()).0;
        if self.simulate_state(&candidate, None).is_some_and(|(_, after)| {
            self.preserves_goals(&before, &after) && self.hard_goals(&after) < self.hard_goals(&before)
        }) {
            candidate
        } else {
            route
        }
    }

    /// Selected standings from the actual turn-ins of an explicit or ordinary replay.
    /// Resolved quest rewards already include their bonuses and spillover; apply them once.
    pub(crate) fn reputation_reached_trace(&self, trace: &[super::Timed]) -> Vec<f64> {
        let initial = self.sim().initial_state();
        let mut reputation = initial.reputation.clone();
        let mut turned = HashSet::new();
        for i in trace.iter().filter_map(|t| match t.event {
            Event::Stop {
                stop: Stop {
                    index,
                    kind: Kind::TurnIn,
                },
                ..
            } => Some(index as usize),
            _ => None,
        }) {
            if !turned.insert(i)
                || initial.rewarded(i)
                || self.model.quests[i].members & 1 == 0
                || self.model.initial.need(i) & 1 == 0
            {
                continue;
            }
            // Preserve actual event order and fractional rewards exactly as simulation does.
            for (g, goal) in self.params.reputation_goals.iter().enumerate() {
                for &(faction, points) in &self.model.quests[i].reputation {
                    if faction == goal.faction {
                        reputation[g] += points;
                    }
                }
            }
        }
        reputation
    }

    /// Explain unmet goals without rejecting leveling or other factions' useful work.
    /// `reached` may come from a route state or from its actual edited trace.
    pub fn reputation_warnings(&self, reached: &[f64]) -> Vec<String> {
        self
            .params
            .reputation_goals
            .iter()
            .enumerate()
            .filter_map(|(i, goal)| {
                let have = reached.get(i).copied().unwrap_or(0.0);
                (have < goal.target as f64).then(|| {
                    let name = self
                        .model
                        .faction_names
                        .get(&goal.faction)
                        .cloned()
                        .unwrap_or_else(|| format!("Faction {}", goal.faction));
                    let start = self.model.initial.reputation.get(i).copied().unwrap_or(0.0);
                    let provenance = if self.model.initial.reputation_recorded.contains(&goal.faction) {
                        "recorded standing"
                    } else {
                        "estimated race/class baseline"
                    };
                    let limitation = if self.model.quests.iter().any(|q| {
                        q.members & 1 != 0
                            && q.reputation.iter().any(|&(faction, points)| faction == goal.faction && points > 0.0)
                    }) {
                        "this forecast supports direct single-completion quest rewards only; the selected turn-ins do not reach the target under current settings"
                    } else {
                        "no supported direct single-completion quest reward is modeled for this faction under current settings"
                    };
                    format!(
                        "{SHORTFALL_PREFIX}{name} ({}): start {start} ({provenance}), target {}, actual {have}, deficit {}; {limitation}; the result is not a proven maximum",
                        goal.faction,
                        goal.target,
                        goal.target as f64 - have
                    )
                })
            })
            .collect()
    }

    /// Refresh generated notes after an edit while preserving other preparation/user notes.
    pub(crate) fn refresh_reputation_notes(&self, notes: &mut Vec<String>, reached: &[f64]) {
        notes.retain(|note| !note.starts_with(SHORTFALL_PREFIX));
        notes.extend(self.reputation_warnings(reached));
    }
}
