//! Explaining a route: class quests left out, why planning stalls, objectives done along the way.

use super::Planner;
use super::state::{Event, Kind, State, Stop};
use std::collections::HashSet;

impl Planner<'_> {
    /// Exact requirements checked against what the simulation actually turned in.
    fn required_missing(&self, route: &[Stop]) -> Vec<String> {
        let Some(end) = self.end_of(route) else {
            return vec!["route cannot be simulated".into()];
        };
        self.model
            .quests
            .iter()
            .enumerate()
            .filter(|(i, q)| q.required && !end.turned.contains(i))
            .map(|(_, q)| format!("{} ({})", q.name, q.id))
            .collect()
    }

    pub fn validate_required(&self, route: &[Stop]) -> anyhow::Result<()> {
        if !self.model.quests.iter().any(|q| q.required) {
            return Ok(());
        }
        let missing = self.required_missing(route);
        anyhow::ensure!(
            missing.is_empty(),
            "no route completing every required quest was found: {}",
            missing.join(", ")
        );
        Ok(())
    }

    /// Editor replays include explicit actions, so validate their actual trace, not a fresh
    /// replay that could insert automatic actions or finish at a different time.
    pub(crate) fn validate_required_trace(&self, trace: &[super::Timed]) -> anyhow::Result<()> {
        let initial = self.initial_state();
        let turned: HashSet<_> = trace
            .iter()
            .filter_map(|t| match t.event {
                Event::Stop {
                    stop:
                        Stop {
                            index,
                            kind: Kind::TurnIn,
                        },
                    ..
                } => Some(index as usize),
                _ => None,
            })
            .collect();
        let missing: Vec<_> = self
            .model
            .quests
            .iter()
            .enumerate()
            .filter(|(i, q)| q.required && !initial.rewarded(*i) && !turned.contains(i))
            .map(|(_, q)| format!("{} ({})", q.name, q.id))
            .collect();
        anyhow::ensure!(
            missing.is_empty(),
            "required quests are missing: {}",
            missing.join(", ")
        );
        Ok(())
    }

    /// Class quests the route does not turn in, with why they could not be placed.
    pub fn missing_class_quests(&self, route: &[Stop]) -> Vec<String> {
        self.missing_required(route, false)
    }

    /// Quests of the forced zones the route does not turn in, with why.
    pub fn missing_zone_quests(&self, route: &[Stop]) -> Vec<String> {
        self.missing_required(route, true)
    }

    /// Timed quests the route takes but turns in after their limit (with the planner's margin):
    /// the guide gives them up instead, the game fails them. The planner avoids it when it can;
    /// one left is a quest whose limit the route cannot keep.
    pub(crate) fn late_timed_quests(&self, route: &[Stop]) -> Vec<String> {
        let mut trace = Vec::new();
        if self.simulate(route, Some(&mut trace)).is_none() {
            return Vec::new();
        }
        let mut late: Vec<usize> = trace
            .iter()
            .filter_map(|t| match t.event {
                Event::Abandon { quest } => Some(quest),
                _ => None,
            })
            .filter(|&i| {
                self.model.quests[i].time_limit.is_some()
                    && route.iter().any(|s| s.kind == Kind::TurnIn && s.index as usize == i)
            })
            .collect();
        late.dedup();
        late.into_iter()
            .map(|i| {
                let q = &self.model.quests[i];
                let minutes = q.time_limit.unwrap_or_default() / 60.0;
                format!("{} ({}): not turned in within its {minutes:.0} minutes", q.name, q.id)
            })
            .collect()
    }

    fn missing_required(&self, route: &[Stop], zone: bool) -> Vec<String> {
        // Done before the route (in a group: by everyone it is for), or in it.
        let s0 = self.initial_state();
        let turned = |j: usize| s0.turned[j] || route.iter().any(|s| s.kind == Kind::TurnIn && s.index as usize == j);
        self.model
            .quests
            .iter()
            .enumerate()
            .filter(|(i, q)| {
                let done = |id: &i64| self.model.index.get(id).is_some_and(|&j| turned(j));
                q.mandatory && q.zone_forced == zone && !turned(*i) && !q.exclusive.iter().any(done)
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
            } else if !self.availability().prereqs_met(s, i) {
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
    pub(crate) fn background_objectives(&self, route: &[Stop]) -> HashSet<(u32, u8)> {
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
                        && self.places().usual_pos(*t).dist(&o.loc.pos) <= NEAR
                });
            if hub && shared {
                out.insert((s.index, k));
            }
        }
        out
    }
}
