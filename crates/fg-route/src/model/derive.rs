//! What the planner works out from the quests and dungeons built, without the game data: the same
//! for a model read from a database (`load`) and one built by hand (`testing::Scenario`).

use super::sources::QuestRow;
use super::{Dungeon, Profile, Quest, chains, dungeons, required};
use crate::params::Params;
use anyhow::Result;
use rustc_hash::FxHashMap;
use std::collections::{HashMap, HashSet};

/// What the characters know and have done that changes which quests the guide takes.
#[derive(Debug, Default)]
pub(crate) struct Knowledge {
    /// Quests whose reward (a spell) every character knows already: left out unless asked for.
    pub(crate) rewards: HashSet<i64>,
    /// Quests counted as done when another needs them first.
    pub(crate) prerequisites: HashSet<i64>,
    /// Quests whose reward is a spell of a character's class.
    pub(crate) reward_quests: HashSet<i64>,
    /// Quests in a character's log or done.
    pub(crate) logged: HashSet<i64>,
    /// The class quests of the data (`Sources::class_quest_ids`).
    pub(crate) class_quests: HashSet<i64>,
    /// No character was recorded: what is known is assumed from the levels, and a must-have
    /// quest is not taken as known.
    pub(crate) assumed: bool,
}

/// The quests built settled: the must-haves, the class quests' versions and what they need, the
/// quests already known or that cannot be handed in left out; then the dungeons' levels
/// (`levels`: the dungeons' own, by area) and the dungeons the must-haves need. `skipped` gets the
/// quests left out. Returns the names of the quests whose reward is known (all their versions).
#[expect(
    clippy::too_many_arguments,
    reason = "the parts of a model, each settled with the others"
)]
pub(crate) fn settle(
    quests: &mut Vec<Quest>,
    skipped: &mut Vec<(i64, String, String)>,
    dungeons: &mut [Dungeon],
    rows: &[QuestRow],
    knowledge: &mut Knowledge,
    levels: &HashMap<i64, i64>,
    profile: &Profile,
    params: &Params,
) -> Result<HashSet<String>> {
    required::apply(quests, rows, &knowledge.class_quests, skipped, profile, params)?;
    if knowledge.assumed {
        for q in quests.iter().filter(|q| q.required) {
            knowledge.rewards.remove(&q.id);
            knowledge.prerequisites.remove(&q.id);
        }
    }
    let mut names_known = HashMap::<String, bool>::new();
    for r in rows.iter().filter(|r| {
        profile.class_ids().iter().any(|c| r.of_class(*c))
            && profile.members_of(r.races, r.classes) != 0
            && knowledge.reward_quests.contains(&r.id)
    }) {
        *names_known.entry(r.name.clone()).or_insert(true) &= knowledge.rewards.contains(&r.id);
    }
    let known_names: HashSet<_> = names_known.into_iter().filter_map(|(n, k)| k.then_some(n)).collect();
    chains::drop_unhanded(quests, skipped);
    for q in quests.iter_mut() {
        if known_names.contains(&q.name) && !q.required && !q.zone_forced {
            q.mandatory = false;
        }
    }
    quests.retain(|q| {
        !knowledge.rewards.contains(&q.id)
            || knowledge.logged.contains(&q.id)
            || profile.completed_quests.contains(&q.id)
            || knowledge.class_quests.contains(&q.id)
            || q.required
            || q.zone_forced
    });
    // Class unlocks kept below the guide's levels give little XP: left to the search they come late.
    for q in quests.iter_mut() {
        q.late_unlock = knowledge.class_quests.contains(&q.id)
            && !profile.within_levels(q.level, q.min_level, params.max_quest_above);
    }
    super::settle_quests(quests, dungeons, params, &knowledge.prerequisites, profile);
    dungeons::set_dungeon_levels(
        dungeons,
        quests,
        levels,
        (params.dungeon_from, params.dungeon_to),
        params.edition.rules().max_level,
    );
    super::settle_dungeons(dungeons, quests, profile);
    Ok(known_names)
}

/// What the planner looks up in the quests settled.
pub(crate) struct Indexes {
    /// Position of each quest by its ID.
    pub(crate) index: FxHashMap<i64, usize>,
    /// The value of what each quest unlocks (`chains::chain_values`).
    pub(crate) unlocks: Vec<f64>,
    /// The first quest of each quest's chain (`chains::chain_tops`).
    pub(crate) chain_top: Vec<i64>,
    /// The quests needing any `pre_need` of several, by quest they may need.
    pub(crate) bundles: Vec<Vec<u32>>,
}

pub(crate) fn indexes(quests: &[Quest], params: &Params) -> Indexes {
    let index: FxHashMap<i64, usize> = quests.iter().enumerate().map(|(i, q)| (q.id, i)).collect();
    let unlocks = chains::chain_values(quests, &index, &chains::quest_work(quests, params));
    let chain_top = chains::chain_tops(quests, &index);
    let mut bundles = vec![Vec::new(); quests.len()];
    for (f, q) in quests.iter().enumerate().filter(|(_, q)| q.pre_need > 1) {
        for &i in q.pre_any.iter().filter_map(|id| index.get(id)) {
            bundles[i].push(f as u32);
        }
    }
    Indexes {
        index,
        unlocks,
        chain_top,
        bundles,
    }
}
