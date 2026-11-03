//! Quest chains: what finishing a quest unlocks, and the top of each chain.

use super::types::{ClassRequirement, Quest};
use crate::params::Params;
use rustc_hash::FxHashMap;
use std::collections::{HashMap, HashSet};

/// QuestSort ID of each class's quests (zoneOrSort = -sort).
pub(crate) fn class_sort(class_id: i64) -> i64 {
    match class_id {
        1 => 81,   // Warrior
        2 => 141,  // Paladin
        3 => 261,  // Hunter
        4 => 162,  // Rogue
        5 => 262,  // Priest
        7 => 82,   // Shaman
        8 => 161,  // Mage
        9 => 61,   // Warlock
        11 => 263, // Druid
        _ => 0,
    }
}

/// For each quest, the XP of the quests it unlocks: a chain's first quest is worth its
/// followers too (each step discounted, since later steps may not fit the route), less so
/// when a follower is long to do (`work`, seconds): sixty cloth to gather make the donation
/// that follows worth little to the first one.
pub(super) fn chain_values(quests: &[Quest], index: &FxHashMap<i64, usize>, work: &[f64]) -> Vec<f64> {
    const DISCOUNT: f64 = 0.7;
    const LOCAL_CHAIN: f64 = 1500.0;
    /// Work (seconds) that halves the share of a follower's XP counted for the chain.
    const CHAIN_WORK: f64 = 1800.0;
    fn value(
        i: usize,
        deps: &[Vec<usize>],
        quests: &[Quest],
        work: &[f64],
        memo: &mut [Option<f64>],
        depth: u32,
    ) -> f64 {
        if let Some(v) = memo[i] {
            return v;
        }
        if depth > 12 {
            return 0.0;
        }
        let v = deps[i]
            .iter()
            .map(|&d| {
                DISCOUNT * CHAIN_WORK / (CHAIN_WORK + work[d])
                    * (quests[d].xp as f64 + value(d, deps, quests, work, memo, depth + 1))
            })
            .sum();
        memo[i] = Some(v);
        v
    }
    let mut dependents: Vec<Vec<usize>> = vec![Vec::new(); quests.len()];
    for (i, q) in quests.iter().enumerate() {
        for p in q.pre_all.iter().chain(&q.pre_any).chain(&q.pre_held) {
            // Only local follow-ups: given near where the previous step is turned in.
            if let Some(&j) = index.get(p) {
                let near = quests[j]
                    .ends
                    .iter()
                    .any(|e| e.pos.dist(&q.starts[0].pos) <= LOCAL_CHAIN);
                if near {
                    dependents[j].push(i);
                }
            }
        }
    }
    let mut memo = vec![None; quests.len()];
    (0..quests.len())
        .map(|i| value(i, &dependents, quests, work, &mut memo, 0))
        .collect()
}

/// For each quest, the highest level among the quests it leads to: those requiring it, and the
/// quest it is a breadcrumb for, along the whole chain.
pub(super) fn chain_tops(quests: &[Quest], index: &FxHashMap<i64, usize>) -> Vec<i64> {
    fn top(i: usize, next: &[Vec<usize>], quests: &[Quest], memo: &mut [Option<i64>], depth: u32) -> i64 {
        if let Some(v) = memo[i] {
            return v;
        }
        if depth > 12 {
            return 0;
        }
        let v = next[i]
            .iter()
            .map(|&d| quests[d].level.max(top(d, next, quests, memo, depth + 1)))
            .max()
            .unwrap_or(0);
        memo[i] = Some(v);
        v
    }
    let mut next: Vec<Vec<usize>> = vec![Vec::new(); quests.len()];
    for (i, q) in quests.iter().enumerate() {
        for p in q.pre_all.iter().chain(&q.pre_any).chain(&q.pre_held) {
            if let Some(&j) = index.get(p) {
                next[j].push(i);
            }
        }
        if let Some(&j) = q.breadcrumb_for.and_then(|b| index.get(&b)) {
            next[i].push(j);
        }
    }
    let mut memo = vec![None; quests.len()];
    (0..quests.len()).map(|i| top(i, &next, quests, &mut memo, 0)).collect()
}

/// A version of a class requirement: its last quest, with what leads to it when it is chosen.
#[derive(Clone)]
pub(super) struct ClassChain {
    /// Index of its last quest, the one the requirement names.
    last: usize,
}

impl ClassChain {
    /// It with the must-have quests leading only to it: taking another version closes those too,
    /// or they would stay to do for nothing. A quest something else needs is not one of them.
    fn with_own_prerequisites(&self, quests: &[Quest]) -> Vec<usize> {
        let index: HashMap<i64, usize> = quests.iter().enumerate().map(|(i, q)| (q.id, i)).collect();
        let needed = |q: &Quest| -> Vec<i64> {
            let only_one = q.pre_any.iter().filter(|_| q.pre_any.len() == 1);
            q.pre_all.iter().chain(only_one).chain(&q.pre_held).copied().collect()
        };
        let mut path = vec![self.last];
        let mut k = 0;
        while k < path.len() {
            for id in needed(&quests[path[k]]) {
                let Some(&j) = index.get(&id) else { continue };
                let elsewhere = (quests.iter().enumerate()).any(|(f, q)| {
                    let needs = |p: &i64| *p == id;
                    q.mandatory
                        && !path.contains(&f)
                        && q.pre_all.iter().chain(&q.pre_any).chain(&q.pre_held).any(needs)
                });
                if quests[j].mandatory && !path.contains(&j) && !elsewhere {
                    path.push(j);
                }
            }
            k += 1;
        }
        path
    }

    /// The players of a group who can take it.
    fn members(&self, quests: &[Quest]) -> u8 {
        quests[self.last].members
    }
}

/// Versions of a class quest: the quests of one requirement (`Profile::class_requirements`: the
/// quests teaching the same spell, or those the overrides list) that the character can take.
/// The Skyborne shaman can take two Call of Fire chains for one Searing Totem.
pub(super) fn class_quest_versions(
    quests: &[Quest],
    requirements: &[ClassRequirement],
    all_required: bool,
) -> Vec<(ClassChain, ClassChain)> {
    let mut versions = Vec::new();
    if all_required {
        return versions;
    }
    let index: HashMap<i64, usize> = quests.iter().enumerate().map(|(i, q)| (q.id, i)).collect();
    for requirement in requirements.iter().filter(|c| c.selected) {
        let chains: Vec<ClassChain> = requirement
            .quests
            .iter()
            .filter_map(|id| index.get(id))
            .map(|&last| ClassChain { last })
            .collect();
        for (k, a) in chains.iter().enumerate() {
            for b in &chains[k + 1..] {
                if a.members(quests) == b.members(quests) {
                    versions.push((a.clone(), b.clone()));
                }
            }
        }
    }
    versions
}

/// One requirement for all the versions of a class quest: the route does one, the others closed
/// once it is taken (not doing them counts as done, see `Availability::alternative_done`), route
/// building and the search (`swap_version`) choosing. One the player asked for, or in a zone they
/// forced, is the one done. A version with a step that cannot be planned is no alternative:
/// nothing changes.
pub(super) fn settle_class_quest_versions(quests: &mut [Quest], versions: &[(ClassChain, ClassChain)]) {
    let release = |quests: &mut [Quest], chain: &ClassChain| {
        for i in chain.with_own_prerequisites(quests) {
            quests[i].mandatory = false;
        }
    };
    let doable = |c: &ClassChain, quests: &[Quest]| quests[c.last].mandatory;
    let open: Vec<_> = (versions.iter())
        .filter(|(a, b)| doable(a, quests) && doable(b, quests))
        .collect();
    for (a, b) in open {
        let asked = |c: &ClassChain, quests: &[Quest]| {
            c.with_own_prerequisites(quests)
                .iter()
                .any(|&i| quests[i].required || quests[i].zone_forced)
        };
        match (asked(a, quests), asked(b, quests)) {
            (true, true) => {}
            (true, false) => release(quests, b),
            (false, true) => release(quests, a),
            (false, false) => {
                let (path_a, path_b) = (a.with_own_prerequisites(quests), b.with_own_prerequisites(quests));
                let close = |quests: &mut [Quest], path: &[usize], others: &[usize]| {
                    let ids: Vec<i64> = others.iter().map(|&i| quests[i].id).collect();
                    for &i in path {
                        for id in &ids {
                            if !quests[i].exclusive.contains(id) {
                                quests[i].exclusive.push(*id);
                            }
                        }
                    }
                };
                close(quests, &path_a, &path_b);
                close(quests, &path_b, &path_a);
            }
        }
    }
}

/// Prerequisites of required quests are required too: all of `pre_all`, and the one of
/// `pre_any` the character can take when it is the only one (Archaic Rune before the dwarf's
/// Call of Earth).
pub(super) fn require_prerequisites(quests: &mut [Quest]) {
    let pos: HashMap<i64, usize> = quests.iter().enumerate().map(|(i, q)| (q.id, i)).collect();
    let mut stack: Vec<usize> = (0..quests.len()).filter(|&i| quests[i].mandatory).collect();
    while let Some(i) = stack.pop() {
        let any: Vec<usize> = quests[i].pre_any.iter().filter_map(|p| pos.get(p).copied()).collect();
        let only = (any.len() == 1 && quests[i].pre_need <= 1).then(|| quests[any[0]].id);
        let held = quests[i].pre_held.clone();
        for p in quests[i].pre_all.clone().into_iter().chain(held).chain(only) {
            if let Some(&j) = pos.get(&p)
                && !quests[j].mandatory
            {
                quests[j].mandatory = true;
                quests[j].zone_forced = quests[i].zone_forced;
                stack.push(j);
            }
        }
    }
}

/// Quests asking for an item that only quests left out hand (`Objective::handed_by`) cannot be
/// finished: left out too, until none is left (a chain of them).
pub(super) fn drop_unhanded(quests: &mut Vec<Quest>, skipped: &mut Vec<(i64, String, String)>) {
    loop {
        let ids: HashSet<i64> = quests.iter().map(|q| q.id).collect();
        let before = quests.len();
        quests.retain_mut(|q| {
            let mut ok = true;
            for o in q.objectives.iter_mut().filter(|o| !o.handed_by.is_empty()) {
                o.handed_by.retain(|g| ids.contains(g));
                ok &= !o.handed_by.is_empty();
            }
            if !ok {
                skipped.push((
                    q.id,
                    q.name.clone(),
                    "objective item handed by quests left out".to_owned(),
                ));
            }
            ok
        });
        if quests.len() == before {
            break;
        }
    }
}

/// A class quest whose prerequisite cannot be planned cannot be required either.
pub(super) fn release_impossible_class_quests(quests: &mut [Quest], known: &HashSet<i64>, completed: &HashSet<i64>) {
    let mut doable: HashSet<i64> = quests.iter().map(|q| q.id).collect();
    loop {
        let mut changed = false;
        for q in quests.iter_mut() {
            if !doable.contains(&q.id) || completed.contains(&q.id) {
                continue;
            }
            let satisfied = |p: &i64| doable.contains(p) || known.contains(p);
            let blocked = q.pre_all.iter().any(|p| !satisfied(p))
                || q.pre_held.iter().any(|p| !doable.contains(p) || completed.contains(p))
                || (!q.pre_any.is_empty()
                    && q.pre_any.iter().filter(|p| satisfied(p)).count() < q.pre_need.min(q.pre_any.len()));
            if blocked {
                doable.remove(&q.id);
                changed = true;
                if q.mandatory {
                    eprintln!(
                        "  {} quest impossible: {} ({}): prerequisite not plannable",
                        if q.zone_forced { "zone" } else { "class" },
                        q.name,
                        q.id
                    );
                    q.mandatory = false;
                    q.zone_forced = false;
                }
            }
        }
        if !changed {
            break;
        }
    }
}

/// Work of each quest's objectives (kills at the quest's level, objects, fixed time).
pub(super) fn quest_work(quests: &[Quest], params: &Params) -> Vec<f64> {
    quests
        .iter()
        .map(|q| {
            q.objectives
                .iter()
                .map(|o| o.kills * params.kill_time(q.level, 0.0, o.mob_level) + o.uses * params.object_time + o.extra)
                .sum()
        })
        .collect()
}
