//! Explaining a route: class quests left out, why planning stalls, objectives done along the way.

use super::Planner;
use super::state::{Kind, State, Stop};
use std::collections::HashSet;

impl Planner<'_> {
    /// Class quests the route does not turn in, with why they could not be placed.
    pub fn missing_class_quests(&self, route: &[Stop]) -> Vec<String> {
        self.model
            .quests
            .iter()
            .enumerate()
            .filter(|(i, q)| {
                let done = |id: &i64| {
                    self.model
                        .index
                        .get(id)
                        .is_some_and(|&j| route.iter().any(|s| s.kind == Kind::TurnIn && s.index as usize == j))
                };
                q.mandatory
                    && !route.iter().any(|s| s.kind == Kind::TurnIn && s.index as usize == *i)
                    && !q.exclusive.iter().any(done)
            })
            .map(|(i, q)| {
                let mut candidate = route.to_vec();
                self.insert_quest(&mut candidate, i as u32);
                let why = match self.diagnose(&candidate) {
                    Some((_, why)) => why,
                    None => "only after the target level".into(),
                };
                format!("{} ({}, level {}): {why}", q.name, q.id, q.min_level)
            })
            .collect()
    }

    /// Why the greedy construction found nothing to do (debugging aid).
    pub(crate) fn explain_stall(&self, s: &State) {
        let n = self.model.quests.len() as u32;
        let mut reasons: std::collections::BTreeMap<&str, usize> = std::collections::BTreeMap::default();
        for i in 0..n {
            let iu = i as usize;
            let q = self.quest(i);
            let reason = if s.turned[iu] {
                "done"
            } else if s.accepted[iu] {
                "in log"
            } else if q.level + 10 < s.level {
                "too low"
            } else if s.log >= self.params.quest_log_size {
                "log full"
            } else if !self.availability().prereqs_met(s, q) {
                "prerequisites"
            } else if self.availability().blocked(s, q) {
                "exclusive"
            } else if !self
                .trips()
                .fastest(s, &self.places().pos(s, Stop::quest(i, Kind::Accept)))
                .0
                .is_finite()
            {
                "unreachable"
            } else if !self.worth().worth_it(s, i, s.level.max(q.min_level), 0.0) {
                "not worth it"
            } else {
                "available?"
            };
            *reasons.entry(reason).or_default() += 1;
        }
        eprintln!("stalled at level {} with {} quests in log: {reasons:?}", s.level, s.log);
        for i in 0..n {
            let iu = i as usize;
            if s.accepted[iu] && !s.turned[iu] {
                let q = self.quest(i);
                eprintln!(
                    "  in log: {} (level {}), objectives done {:b}/{}",
                    q.name,
                    q.level,
                    s.objectives[iu],
                    q.objectives.len()
                );
            }
        }
    }

    /// Objectives done "along the way" (background quests): long kill/loot objectives of quests
    /// without follow-up, turned in at a quest hub, whose mobs are where other objectives
    /// nearby in the route take place. The addon tracks them without blocking the guide.
    pub fn background_objectives(&self, route: &[Stop]) -> HashSet<(u32, u8)> {
        const WINDOW: usize = 30;
        const NEAR: f64 = 400.0;
        let mut out = HashSet::new();
        for (i, s) in route.iter().enumerate() {
            let Kind::Objective(k) = s.kind else { continue };
            let q = self.quest(s.index);
            let o = &q.objectives[k as usize];
            if o.kills <= 0.0 || o.dungeon.is_some() || self.model.unlocks[s.index as usize] > 0.0 {
                continue;
            }
            if o.kills * self.combat().kill_time(q.level, 0.0, o) < 240.0 {
                continue;
            }
            // Turned in at a hub: another quest starts or ends next to its turn-in NPC.
            let end = q.ends[0].pos;
            let hub = route.iter().any(|t| {
                t.is_quest() && t.index != s.index && {
                    let other = self.quest(t.index);
                    other
                        .starts
                        .iter()
                        .chain(&other.ends)
                        .any(|l| l.pos.dist(&end) <= 300.0)
                }
            });
            // Its mobs are where other objectives around it in the route are done.
            let shared = route[i.saturating_sub(WINDOW)..(i + WINDOW).min(route.len())]
                .iter()
                .any(|t| {
                    matches!(t.kind, Kind::Objective(_))
                        && t.index != s.index
                        && self.places().usual_pos(t).dist(&o.loc.pos) <= NEAR
                });
            if hub && shared {
                out.insert((s.index, k));
            }
        }
        out
    }
}
