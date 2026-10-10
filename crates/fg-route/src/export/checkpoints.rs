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
            .map(|k| planner.combat().level(planner.quest(i as u32), k as u8, 0.0))
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
    out
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
