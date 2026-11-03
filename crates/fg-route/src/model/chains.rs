//! Quest chains: what finishing a quest unlocks, and the top of each chain.

use super::types::{EntityKind, Quest};
use crate::params::Params;
use rustc_hash::FxHashMap;
use std::collections::{BTreeMap, BTreeSet, HashMap, HashSet};

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

/// Names of quests some version of which teaches a spell (a pet, a totem...).
fn spell_quest_names(quests: &[Quest]) -> HashSet<&str> {
    quests
        .iter()
        .filter(|q| q.reward_spell.is_some())
        .map(|q| q.name.as_str())
        .collect()
}

/// The quests a required name designates: where quests of that name teach a spell (the spell is
/// what is required), those and the steps of the name leading to them (Summon Felsteed's
/// letters before the quest teaching it); else every quest of the name.
pub(crate) fn required_by_name(quests: &[Quest]) -> HashSet<i64> {
    let names = spell_quest_names(quests);
    let by_id: HashMap<i64, &Quest> = quests.iter().map(|q| (q.id, q)).collect();
    let mut keep: HashSet<i64> = quests
        .iter()
        .filter(|q| q.reward_spell.is_some() || !names.contains(q.name.as_str()))
        .map(|q| q.id)
        .collect();
    let mut stack: Vec<&Quest> = quests.iter().filter(|q| q.reward_spell.is_some()).collect();
    while let Some(q) = stack.pop() {
        for p in q.pre_all.iter().chain(&q.pre_any).filter_map(|id| by_id.get(id)) {
            if p.name == q.name && keep.insert(p.id) {
                stack.push(p);
            }
        }
    }
    keep
}

/// Class quests are required by name (class_quests.toml, the app's list) and give their power
/// by name. Where quests of that name teach a spell, the spell is what counts: only those and
/// the steps leading to them stay required (`required_by_name`), only those give the power, and
/// a quest teaching a spell a power quest teaches gives its power. The second Vile Familiars
/// (talk to Zureetha Fargaze in Durotar, after the orc's Imp) teaches nothing: the undead
/// warlock, whose Piercing the Veil teaches the Imp, has no reason to go there.
pub(super) fn narrow_class_quests_to_spells(quests: &mut [Quest], all_required: bool) {
    let keep = required_by_name(quests);
    let names: HashSet<String> = spell_quest_names(quests).into_iter().map(str::to_owned).collect();
    let mut powers: HashMap<i64, (u8, f64)> = HashMap::new();
    for q in quests.iter() {
        if let (Some(spell), Some(power)) = (q.reward_spell, q.power) {
            powers.entry(spell).or_insert(power);
        }
    }
    for q in quests.iter_mut() {
        if let Some(spell) = q.reward_spell {
            q.power = q.power.or_else(|| powers.get(&spell).copied());
        } else if names.contains(&q.name) {
            q.power = None;
            if !all_required && !keep.contains(&q.id) {
                q.mandatory &= q.zone_forced;
            }
        }
    }
}

/// A chain of a class quest required by name: the quests of the name linked to each other, by
/// a prerequisite, a breadcrumb or as exclusive ones.
#[derive(Clone)]
pub(super) struct ClassChain {
    quests: Vec<usize>,
    /// Those the name requires (not a breadcrumb to the chain, see `required_by_name`).
    by_name: Vec<usize>,
}

impl ClassChain {
    /// Its last quests: those no other quest of the chain leads to.
    fn last<'q>(&self, quests: &'q [Quest]) -> Vec<&'q Quest> {
        let leads = |q: &Quest, to: &Quest| {
            let needs = |id: &i64| *id == q.id;
            to.pre_all.iter().chain(&to.pre_any).chain(&to.pre_held).any(needs) || q.breadcrumb_for == Some(to.id)
        };
        let of: Vec<&Quest> = self.quests.iter().map(|&i| &quests[i]).collect();
        of.iter()
            .filter(|q| !of.iter().any(|to| leads(q, to)))
            .copied()
            .collect()
    }

    /// The same thing as `other`: the same spell, else an item in common (the choice of two of
    /// Call of Fire 97257 has the Fire Totem of 1527). The data lists no reward for some quests
    /// (Call of Air 96136): without one, the same NPC ending them.
    fn gives_the_same(&self, other: &Self, quests: &[Quest]) -> bool {
        let spells = |c: &Self| -> BTreeSet<i64> { c.quests.iter().filter_map(|&i| quests[i].reward_spell).collect() };
        let items = |c: &Self| -> BTreeSet<i64> {
            c.last(quests)
                .into_iter()
                .flat_map(|q| q.reward_items.iter().copied())
                .collect()
        };
        let ends = |c: &Self| -> HashSet<(EntityKind, i64)> {
            c.last(quests)
                .into_iter()
                .flat_map(|q| q.ends.iter().map(|e| (e.kind, e.id)))
                .collect()
        };
        let (a, b) = (spells(self), spells(other));
        if !a.is_empty() || !b.is_empty() {
            return a == b;
        }
        let (a, b) = (items(self), items(other));
        if !a.is_empty() && !b.is_empty() {
            return !a.is_disjoint(&b);
        }
        !ends(self).is_disjoint(&ends(other))
    }

    /// The players of a group who can take all of it.
    fn members(&self, quests: &[Quest]) -> u8 {
        self.quests.iter().fold(u8::MAX, |m, &i| m & quests[i].members)
    }
}

/// The chains of the class quests required by name, a player having forced none of their zones
/// (who then wants all the quests there).
fn class_chains(quests: &[Quest]) -> Vec<ClassChain> {
    fn root(parent: &mut [usize], mut i: usize) -> usize {
        while parent[i] != i {
            parent[i] = parent[parent[i]];
            i = parent[i];
        }
        i
    }
    let index: HashMap<i64, usize> = quests.iter().enumerate().map(|(i, q)| (q.id, i)).collect();
    let mut parent: Vec<usize> = (0..quests.len()).collect();
    for (i, q) in quests.iter().enumerate() {
        let linked = (q.pre_all.iter().chain(&q.pre_any).chain(&q.pre_held))
            .chain(&q.exclusive)
            .chain(&q.breadcrumb_for);
        for &j in linked.filter_map(|id| index.get(id)) {
            if quests[j].name == q.name {
                let (a, b) = (root(&mut parent, i), root(&mut parent, j));
                parent[a] = b;
            }
        }
    }
    let mut chains: BTreeMap<usize, Vec<usize>> = BTreeMap::new();
    for i in 0..quests.len() {
        chains.entry(root(&mut parent, i)).or_default().push(i);
    }
    chains
        .into_values()
        .filter(|chain| !chain.iter().any(|&i| quests[i].zone_forced))
        .map(|quests_of_chain| ClassChain {
            by_name: (quests_of_chain.iter().copied())
                .filter(|&i| quests[i].mandatory)
                .collect(),
            quests: quests_of_chain,
        })
        .filter(|chain| !chain.by_name.is_empty())
        .collect()
}

/// Versions of a class quest: chains of one required name giving the same thing, for the same
/// players. The Skyborne shaman can take two Call of Fire chains for one Searing Totem. Each
/// demon of The Binding is another spell: those stay required. Read from the quests required by
/// name, before their prerequisites join them.
pub(super) fn class_quest_versions(quests: &[Quest], all_required: bool) -> Vec<(ClassChain, ClassChain)> {
    let mut versions = Vec::new();
    if all_required {
        return versions;
    }
    let mut chains = class_chains(quests);
    while let Some(chain) = chains.pop() {
        let (same, rest): (Vec<_>, Vec<_>) = chains.into_iter().partition(|other| {
            quests[other.quests[0]].name == quests[chain.quests[0]].name
                && other.members(quests) == chain.members(quests)
                && chain.gives_the_same(other, quests)
        });
        chains = rest;
        let mut group = vec![chain];
        group.extend(same);
        while let Some(a) = group.pop() {
            for b in &group {
                versions.push((a.clone(), b.clone()));
            }
        }
    }
    versions
}

/// One requirement for all the versions of a class quest: the route does one, the others closed
/// to it (not doing them counts as done, see `Availability::alternative_done`), the search
/// choosing. One the player asked for is the one done. A version with a step that cannot be
/// planned is no alternative: nothing changes.
pub(super) fn settle_class_quest_versions(quests: &mut [Quest], versions: &[(ClassChain, ClassChain)]) {
    let doable = |c: &ClassChain, quests: &[Quest]| c.by_name.iter().all(|&i| quests[i].mandatory);
    let open: Vec<_> = (versions.iter())
        .filter(|(a, b)| doable(a, quests) && doable(b, quests))
        .collect();
    for (a, b) in open {
        let asked = |c: &ClassChain, quests: &[Quest]| c.quests.iter().any(|&i| quests[i].required);
        match (asked(a, quests), asked(b, quests)) {
            (true, true) => {}
            (true, false) => b.by_name.iter().for_each(|&i| quests[i].mandatory = false),
            (false, true) => a.by_name.iter().for_each(|&i| quests[i].mandatory = false),
            (false, false) => {
                let ids = |c: &ClassChain| c.quests.iter().map(|&i| quests[i].id).collect::<Vec<_>>();
                let (of_a, of_b) = (ids(a), ids(b));
                for &i in &a.quests {
                    quests[i].exclusive.extend(&of_b);
                }
                for &i in &b.quests {
                    quests[i].exclusive.extend(&of_a);
                }
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
