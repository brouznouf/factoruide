//! Quest chains: what finishing a quest unlocks, and the top of each chain.

use super::types::{ClassRequirement, CompletionWindow, MandatoryOrigins, Profile, Quest};
use crate::params::Params;
use rustc_hash::FxHashMap;
use std::collections::{HashMap, HashSet};

/// Positive selected-faction rewards and their real prerequisite alternatives. This only
/// relaxes the lower level cutoff; exclusions, supported sources and upper gates stay intact.
pub(super) fn reputation_candidates(quests: &[Quest], params: &Params) -> HashSet<i64> {
    if params.reputation_goals.is_empty() {
        return HashSet::new();
    }
    let index: HashMap<_, _> = quests.iter().map(|q| (q.id, q)).collect();
    let mut pending: Vec<_> = quests
        .iter()
        .filter(|q| {
            q.reputation
                .iter()
                .any(|(f, v)| *v > 0.0 && params.reputation_goals.iter().any(|g| g.faction == *f))
        })
        .map(|q| q.id)
        .collect();
    let mut relevant = HashSet::new();
    while let Some(id) = pending.pop() {
        if !relevant.insert(id) {
            continue;
        }
        if let Some(q) = index.get(&id) {
            pending.extend(q.pre_all.iter().chain(&q.pre_any).chain(&q.pre_held));
            pending.extend(q.objectives.iter().flat_map(|o| &o.handed_by));
        }
    }
    relevant
}

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
        // A quest asking for one of several others (the warlock's mount: one of the two letters)
        // has them as versions of its own.
        for chain in &chains {
            let alternatives: Vec<ClassChain> = (alternatives_of(&quests[chain.last]).iter())
                .filter_map(|id| index.get(id))
                .map(|&last| ClassChain { last })
                .collect();
            for (k, a) in alternatives.iter().enumerate() {
                for b in &alternatives[k + 1..] {
                    if a.members(quests) == b.members(quests) {
                        versions.push((a.clone(), b.clone()));
                    }
                }
            }
        }
    }
    versions
}

/// The quests a quest asks one of (`pre_any` of several, one needed).
fn alternatives_of(q: &Quest) -> &[i64] {
    if q.pre_any.len() > 1 && q.pre_need <= 1 {
        &q.pre_any
    } else {
        &[]
    }
}

/// A class requirement's quest asking for one of several others needs one of them: they are
/// required, and the route does one (`class_quest_versions`).
pub(super) fn require_alternatives(quests: &mut [Quest], requirements: &[ClassRequirement]) {
    let index: HashMap<i64, usize> = quests.iter().enumerate().map(|(i, q)| (q.id, i)).collect();
    let wanted: HashSet<i64> = (requirements.iter().filter(|c| c.selected))
        .flat_map(|c| c.quests.iter().copied())
        .filter_map(|id| index.get(&id))
        .filter(|&&i| quests[i].mandatory)
        .flat_map(|&i| alternatives_of(&quests[i]).to_vec())
        .collect();
    for id in wanted {
        if let Some(&i) = index.get(&id) {
            quests[i].mandatory = true;
        }
    }
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
            quests[i].mandatory_origins.remove(MandatoryOrigins::CLASS);
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

/// The levels a must-have quest is done at when its window says nothing: while the game shows it
/// yellow (orange when the progression is risky) or green, not before (too hard) nor once it is
/// gray (no XP). A must-have is a quest the player asked for or a class requirement; a quest
/// already gray when the guide starts has no limit, nothing could be done about it.
pub(super) fn default_completion_windows(quests: &mut [Quest], profile: &Profile, params: &Params) {
    let above = match params.progression {
        crate::params::Progression::Risky => 4,
        _ => 2,
    };
    let required_by_class: HashSet<i64> = (profile.class_requirements.iter())
        .filter(|c| c.selected)
        .flat_map(|c| c.quests.iter().copied())
        .collect();
    for q in quests.iter_mut() {
        let asked = profile.required_quests.iter().any(|r| r.id == q.id);
        let must_have = q.mandatory && (asked || required_by_class.contains(&q.id));
        if !must_have || q.completion_window.min_level.is_some() || q.completion_window.max_level.is_some() {
            continue;
        }
        let gray_from = (q.level..=q.level + 40).find(|&l| crate::xp::Rules::gray_level(l) >= q.level);
        // A quest that cannot be taken before it is gray (its level under its minimum) has no limit.
        let max = gray_from
            .map(|l| l - 1)
            .filter(|&m| m >= profile.from_level && m >= q.min_level);
        let min = (q.level - above).max(1);
        q.completion_window = CompletionWindow {
            min_level: Some(min).filter(|&m| m > profile.from_level),
            max_level: max,
        };
        q.implicit_window = true;
    }
    // The route goes for them before the level they turn gray: the quest and what leads to it get
    // the deadline, the earliest of those sharing a prerequisite winning.
    let index: HashMap<i64, usize> = quests.iter().enumerate().map(|(i, q)| (q.id, i)).collect();
    let mut due: Vec<(i64, usize)> = (quests.iter().enumerate())
        .filter(|(_, q)| q.implicit_window)
        .filter_map(|(i, q)| Some((q.completion_window.max_level?, i)))
        .collect();
    due.sort_unstable();
    for (max, i) in due {
        let mut stack = vec![i];
        let mut seen = HashSet::new();
        while let Some(k) = stack.pop() {
            if !seen.insert(k) {
                continue;
            }
            if quests[k].priority_deadline.is_none_or(|d| d > max) {
                quests[k].priority_deadline = Some(max);
            }
            let q = &quests[k];
            let only_one = q.pre_any.iter().filter(|_| q.pre_any.len() == 1);
            let needed: Vec<i64> = q.pre_all.iter().chain(only_one).chain(&q.pre_held).copied().collect();
            stack.extend(
                needed
                    .iter()
                    .filter_map(|id| index.get(id))
                    .filter(|&&j| quests[j].mandatory),
            );
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

/// Metadata only: the ordinary pass above owns mandatory flags and the first zone owner.
/// Gear completion support is selected separately by the source graph; never infer it here
/// from held parents or unchosen alternative branches.
pub(super) fn propagate_mandatory_origins(quests: &mut [Quest], requirements: &[ClassRequirement]) {
    let pos: HashMap<i64, usize> = quests.iter().enumerate().map(|(i, q)| (q.id, i)).collect();
    let class_roots: HashSet<i64> = requirements
        .iter()
        .filter(|c| c.selected)
        .flat_map(|c| c.quests.iter().copied())
        .collect();
    let mut stack: Vec<usize> = (0..quests.len()).filter(|&i| quests[i].mandatory).collect();
    while let Some(i) = stack.pop() {
        let q = &quests[i];
        let mut origins = q.mandatory_origins;
        origins.remove(MandatoryOrigins::GEAR);
        let any: Vec<usize> = q.pre_any.iter().filter_map(|p| pos.get(p).copied()).collect();
        let only = (any.len() == 1 && q.pre_need <= 1).then(|| quests[any[0]].id);
        let needed: Vec<i64> = q.pre_all.iter().chain(&q.pre_held).copied().chain(only).collect();
        // These are the alternative class versions the unchanged require_alternatives pass
        // made mandatory. Seed only after release, from a still-active class requirement.
        let class_alternatives = if class_roots.contains(&q.id) && origins.contains(MandatoryOrigins::CLASS) {
            alternatives_of(q).to_vec()
        } else {
            Vec::new()
        };
        for p in needed {
            if let Some(&j) = pos.get(&p)
                && quests[j].mandatory
                && quests[j].mandatory_origins.insert(origins)
            {
                stack.push(j);
            }
        }
        for p in class_alternatives {
            if let Some(&j) = pos.get(&p)
                && quests[j].mandatory
                && quests[j].mandatory_origins.insert(MandatoryOrigins::CLASS)
            {
                stack.push(j);
            }
        }
    }
}

/// Quests asking for an item that only quests left out hand (`Objective::handed_by`) cannot be
/// finished: left out too, until none is left (a chain of them).
pub(super) fn drop_unhanded(
    quests: &mut Vec<Quest>,
    skipped: &mut Vec<(i64, String, String)>,
    satisfied: &HashSet<(i64, u8)>,
) {
    loop {
        let ids: HashSet<i64> = quests.iter().map(|q| q.id).collect();
        let before = quests.len();
        quests.retain_mut(|q| {
            let mut ok = true;
            for (k, o) in q
                .objectives
                .iter_mut()
                .enumerate()
                .filter(|(_, o)| !o.handed_by.is_empty())
            {
                if satisfied.contains(&(q.id, k as u8)) {
                    continue;
                }
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

/// A class quest whose prerequisite cannot be planned cannot be required either. Returns the
/// forced quests released (a zone's, an objective guide's).
pub(super) fn release_impossible_class_quests(
    quests: &mut [Quest],
    known: &HashSet<i64>,
    completed: &HashSet<i64>,
) -> Vec<(i64, String)> {
    let mut released = Vec::new();
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
                    if q.zone_forced {
                        released.push((q.id, q.name.clone()));
                    }
                    q.mandatory = false;
                    q.zone_forced = false;
                    q.mandatory_origins.remove(MandatoryOrigins::CLASS);
                }
            }
        }
        if !changed {
            break;
        }
    }
    released
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
