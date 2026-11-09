//! Checkpoints: the levels where many quests open, and what a player skipping to one keeps.

use super::types::{Checkpoint, Step, StepKind};
use crate::model::Profile;
use crate::plan::Planner;
use std::collections::HashSet;

/// Lowest and highest checkpoint levels: below, quests are close together and quick.
const CHECKPOINT_LEVELS: (i64, i64) = (20, 59);
/// A checkpoint level opens this many times more quests than the levels around it.
const CHECKPOINT_PEAK: f64 = 1.5;
const CHECKPOINT_MIN_QUESTS: usize = 10;
const CHECKPOINT_GAP: i64 = 4;

/// Levels where many quests open (what the character can take on at that level: the quest's
/// minimum level and the power its objectives require), placed where the route reaches them;
/// the steps before each one to keep when skipping ahead are marked.
pub(super) fn checkpoints(planner: &Planner<'_>, profile: &Profile, steps: &mut [Step]) -> Vec<Checkpoint> {
    let model = &planner.model;
    let mut opening = [0usize; 64];
    for (i, q) in model.quests.iter().enumerate() {
        let open = (0..q.objectives.len())
            .map(|k| {
                planner
                    .combat()
                    .level(planner.quest(i as u32), k as u8, crate::params::Might::PLAIN)
            })
            .fold(q.min_level, i64::max);
        if let Some(n) = opening.get_mut(open.max(0) as usize) {
            *n += 1;
        }
    }
    let lo = CHECKPOINT_LEVELS.0.max(profile.from_level + 1);
    let hi = CHECKPOINT_LEVELS.1.min(profile.to_level - 1);
    let mut peaks: Vec<(usize, i64)> = (lo..=hi)
        .filter_map(|l| {
            let n = opening[l as usize];
            let around: Vec<usize> = (l - 3..=l + 3)
                .filter(|&m| m != l && (1..64).contains(&m))
                .map(|m| opening[m as usize])
                .collect();
            let mean = around.iter().sum::<usize>() as f64 / around.len().max(1) as f64;
            (n >= CHECKPOINT_MIN_QUESTS && n as f64 >= CHECKPOINT_PEAK * mean).then_some((n, l))
        })
        .collect();
    peaks.sort_by(|a, b| b.cmp(a));
    let mut levels: Vec<i64> = Vec::new();
    for (_, l) in peaks {
        if levels.iter().all(|c| (c - l).abs() >= CHECKPOINT_GAP) {
            levels.push(l);
        }
    }
    levels.sort_unstable();

    let mut out = Vec::new();
    let mut from = 0;
    for level in levels {
        let Some(at) = steps.iter().position(|s| s.level >= level) else {
            break;
        };
        if at <= from {
            continue;
        }
        mark_kept(planner, &mut steps[..], from, at);
        out.push(Checkpoint { level, step: at });
        from = at;
    }
    mark_reputation_kept(planner, steps);
    out
}

/// Required reputation work remains useful when the player outlevels the guide, even if
/// this guide has no checkpoint. Follow only support actually present in the guide: an
/// unchosen alternative must not introduce a new quest or an invented completion goal.
fn mark_reputation_kept(planner: &Planner<'_>, steps: &mut [Step]) {
    if planner.params.reputation_goals.is_empty() {
        return;
    }
    let planned: HashSet<_> = steps.iter().filter_map(|s| s.quest).collect();
    let mut pending: Vec<_> = planned
        .iter()
        .copied()
        .filter(|id| {
            planner.model.index.get(id).is_some_and(|&i| {
                planner.model.quests[i].reputation.iter().any(|&(faction, points)| {
                    points > 0.0 && planner.params.reputation_goals.iter().any(|g| g.faction == faction)
                })
            })
        })
        .collect();
    let mut kept = HashSet::new();
    while let Some(id) = pending.pop() {
        if !kept.insert(id) {
            continue;
        }
        if let Some(&i) = planner.model.index.get(&id) {
            let q = &planner.model.quests[i];
            pending.extend(
                q.pre_all
                    .iter()
                    .chain(&q.pre_any)
                    .chain(&q.pre_held)
                    .chain(q.objectives.iter().flat_map(|o| &o.handed_by))
                    .copied()
                    .filter(|id| planned.contains(id)),
            );
        }
    }
    for step in steps {
        step.keep |= step.quest.is_some_and(|id| kept.contains(&id));
    }
}

/// Mark the steps of `from..to` still needed by a player skipping to step `to`: training,
/// class quests, quests whose reward is planned, and the quests the route goes on
/// with after `to` (their chain, directly or through others of the skipped steps).
fn mark_kept(planner: &Planner<'_>, steps: &mut [Step], from: usize, to: usize) {
    let quest = |id: i64| planner.model.index.get(&id).map(|&i| &planner.model.quests[i]);
    let mut needed: HashSet<i64> = steps[to..].iter().filter_map(|s| s.quest).collect();
    let mut kept: HashSet<i64> = steps[from..to]
        .iter()
        .filter(|s| s.reward.is_some())
        .filter_map(|s| s.quest)
        .collect();
    let segment: Vec<i64> = steps[from..to].iter().filter_map(|s| s.quest).collect();
    for &id in &segment {
        if quest(id).is_some_and(|q| q.mandatory || q.power.is_some()) {
            kept.insert(id);
        }
    }
    // Prerequisites of what is needed, until nothing changes.
    loop {
        let before = needed.len() + kept.len();
        let wanted: Vec<i64> = needed.iter().chain(kept.iter()).copied().collect();
        for id in wanted {
            if let Some(q) = quest(id) {
                for &p in q.pre_all.iter().chain(&q.pre_any) {
                    if segment.contains(&p) {
                        kept.insert(p);
                    }
                }
            }
        }
        for &id in &segment {
            if needed.contains(&id) {
                kept.insert(id);
            }
        }
        needed.extend(kept.iter().copied());
        if needed.len() + kept.len() == before {
            break;
        }
    }
    for s in &mut steps[from..to] {
        s.keep = match s.quest {
            Some(id) => kept.contains(&id),
            None => matches!(s.kind, StepKind::Train | StepKind::FlightMaster),
        };
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::params::ReputationGoal;

    fn step(id: i64, level: i64) -> Step {
        let mut step = Step::bare(StepKind::Accept, level, 0.0);
        step.quest = Some(id);
        step
    }

    #[test]
    fn reputation_support_is_kept_even_without_a_checkpoint() {
        let mut p = crate::plan::tests::quests_at_start(7);
        p.params.reputation_goals = vec![ReputationGoal {
            faction: 68,
            target: 3000,
        }];
        let ids: Vec<_> = p.model.quests.iter().map(|q| q.id).collect();
        p.model.quests[1].pre_all = vec![ids[0]];
        p.model.quests[2].pre_any = vec![ids[1], ids[5]];
        p.model.quests[3].pre_held = vec![ids[2]];
        p.model.quests[4].pre_all = vec![ids[3]];
        p.model.quests[4].reputation = vec![(68, 100.0)];
        p.model.quests[6].reputation = vec![(529, 150.0)];
        let mut steps: Vec<_> = [0, 1, 2, 3, 4, 6].map(|i| step(ids[i], 1)).into();
        // Neither the unused alternative nor the unselected-faction quest is required work.
        let planner = Planner::new(&p.model, &p.world, &p.params, &p.profile);
        assert!(checkpoints(&planner, &p.profile, &mut steps).is_empty());
        assert!(steps[..5].iter().all(|s| s.keep));
        assert!(!steps[5].keep);
    }

    #[test]
    fn checkpoint_marking_cannot_remove_reputation_protection() {
        let mut p = crate::plan::tests::quests_at_start(12);
        p.params.reputation_goals = vec![ReputationGoal {
            faction: 68,
            target: 3000,
        }];
        p.profile.to_level = 30;
        for q in &mut p.model.quests[2..] {
            q.min_level = 20;
            q.level = 20;
        }
        let ids: Vec<_> = p.model.quests.iter().map(|q| q.id).collect();
        p.model.quests[0].reputation = vec![(68, 100.0)];
        p.model.quests[1].reputation = vec![(529, 150.0)];
        let mut steps = vec![step(ids[0], 1), step(ids[1], 1), Step::bare(StepKind::Train, 20, 0.0)];
        let planner = Planner::new(&p.model, &p.world, &p.params, &p.profile);
        let points = checkpoints(&planner, &p.profile, &mut steps);
        assert_eq!(points.len(), 1);
        assert!(steps[0].keep);
        assert!(!steps[1].keep);
    }

    #[test]
    fn planned_item_handing_support_is_kept_with_its_reputation_quest() {
        use crate::testing::{At, Obj, Q, Scenario};
        let mut p = Scenario::new()
            .quest(Q::new(900, "Hand over the item"))
            .quest(
                Q::new(901, "Reputation reward").objective(
                    Obj::visit(
                        "Use handed item",
                        At {
                            zone: 1,
                            x: 50.0,
                            y: 50.0,
                        },
                    )
                    .handed_by(&[900]),
                ),
            )
            .build();
        p.params.reputation_goals = vec![ReputationGoal {
            faction: 68,
            target: 3000,
        }];
        let reward = p.model.index[&901];
        p.model.quests[reward].reputation = vec![(68, 100.0)];
        let mut steps = vec![step(900, 1), step(901, 1)];
        let planner = Planner::new(&p.model, &p.world, &p.params, &p.profile);
        checkpoints(&planner, &p.profile, &mut steps);
        assert!(steps.iter().all(|s| s.keep));
    }

    #[test]
    fn no_reputation_goal_preserves_existing_keep_flags() {
        let mut p = crate::plan::tests::quests_at_start(2);
        p.model.quests[0].reputation = vec![(68, 100.0)];
        let mut steps: Vec<_> = p.model.quests.iter().map(|q| step(q.id, 1)).collect();
        steps[1].keep = true;
        let planner = Planner::new(&p.model, &p.world, &p.params, &p.profile);
        checkpoints(&planner, &p.profile, &mut steps);
        assert!(!steps[0].keep);
        assert!(steps[1].keep);
    }
}
