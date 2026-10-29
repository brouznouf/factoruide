//! Select a feasible prerequisite branch for every exact quest requirement.

use super::{Profile, Quest};
use crate::params::Params;
use anyhow::{Context, Result, bail};
use std::collections::{HashMap, HashSet};

struct Requirements<'a> {
    quests: &'a [Quest],
    index: HashMap<i64, usize>,
    completed: &'a HashSet<i64>,
    skipped: &'a [(i64, String, String)],
    roots: &'a [i64],
}

impl Requirements<'_> {
    fn add(&self, id: i64, selected: &mut HashSet<i64>, visiting: &mut HashSet<i64>) -> Result<()> {
        if self.completed.contains(&id) || selected.contains(&id) {
            return Ok(());
        }
        visiting.insert(id);
        let Some(&i) = self.index.get(&id) else {
            let why = self
                .skipped
                .iter()
                .find(|q| q.0 == id)
                .map_or("missing, unavailable, or restricted to another race/class", |q| {
                    q.2.as_str()
                });
            bail!("quest {id}: {why}");
        };
        let q = &self.quests[i];
        let conflicts = |other: i64| {
            q.exclusive.contains(&other)
                || self
                    .index
                    .get(&other)
                    .is_some_and(|&j| self.quests[j].exclusive.contains(&id))
        };
        if let Some(other) = selected
            .iter()
            .chain(self.completed)
            .chain(self.roots)
            .chain(visiting.iter())
            .copied()
            .find(|&other| other != id && conflicts(other))
        {
            bail!("quest {} ({id}) conflicts with quest {other}", q.name);
        }
        for &pre in q.pre_all.iter().chain(&q.pre_held) {
            self.add(pre, selected, visiting)
                .with_context(|| format!("{} ({id}) needs {pre}", q.name))?;
        }
        self.some_of(&q.pre_any, q.pre_need, selected, visiting)?;
        for objective in &q.objectives {
            self.some_of(&objective.handed_by, 1, selected, visiting)?;
            if let Some(child) = objective.child {
                self.add(child, selected, visiting)?;
            }
        }
        visiting.remove(&id);
        selected.insert(id);
        Ok(())
    }

    /// Adds `count` of `alternatives` to the selection (the cheapest first), the ones already
    /// done or selected counting.
    fn some_of(
        &self,
        alternatives: &[i64],
        count: usize,
        selected: &mut HashSet<i64>,
        visiting: &HashSet<i64>,
    ) -> Result<()> {
        let have = alternatives
            .iter()
            .filter(|id| self.completed.contains(id) || selected.contains(id))
            .count();
        if alternatives.is_empty() || have >= count.min(alternatives.len()) {
            return Ok(());
        }
        let mut missing = count.min(alternatives.len()) - have;
        let mut candidates: Vec<_> = (alternatives.iter())
            .filter(|id| !self.completed.contains(id) && !selected.contains(id))
            .copied()
            .collect();
        candidates.sort_by_key(|id| {
            self.index
                .get(id)
                .map_or((i64::MAX, *id), |&i| (self.quests[i].min_level, *id))
        });
        let mut reasons = Vec::new();
        for id in candidates {
            let mut branch = selected.clone();
            match self.add(id, &mut branch, &mut visiting.clone()) {
                Ok(()) => {
                    *selected = branch;
                    missing -= 1;
                    if missing == 0 {
                        return Ok(());
                    }
                }
                Err(error) => reasons.push(format!("{error:#}")),
            }
        }
        bail!("no supported prerequisite alternative: {}", reasons.join("; "))
    }
}

pub(super) fn apply(
    quests: &mut Vec<Quest>,
    rows: &[super::sources::QuestRow],
    skipped: &[(i64, String, String)],
    profile: &Profile,
    params: &Params,
) -> Result<()> {
    if profile.required_quests.is_empty() {
        return Ok(());
    }
    let required = Requirements {
        quests,
        index: quests.iter().enumerate().map(|(i, q)| (q.id, i)).collect(),
        completed: &profile.completed_quests,
        skipped,
        roots: &profile.required_quests,
    };
    let mut selected = HashSet::new();
    for &id in &profile.required_quests {
        required
            .add(id, &mut selected, &mut HashSet::new())
            .with_context(|| format!("cannot plan required quest {id}"))?;
    }
    // Optional quests must not close an explicitly selected branch before it can be done.
    let incompatible: HashSet<_> = quests
        .iter()
        .filter(|q| {
            !selected.contains(&q.id)
                && (q.exclusive.iter().any(|id| selected.contains(id))
                    || quests
                        .iter()
                        .any(|other| selected.contains(&other.id) && other.exclusive.contains(&q.id)))
        })
        .map(|q| q.id)
        .collect();
    for q in quests.iter_mut() {
        q.required = selected.contains(&q.id);
    }
    // The ordinary window uses the source quest levels, before mob/item modeling raises a
    // quest's effective minimum level. Selecting a completed quest must not alter that window.
    let ordinary: HashSet<_> = rows
        .iter()
        .filter(|r| {
            profile.within_levels(
                r.level.unwrap_or(1).max(1),
                r.min_level.unwrap_or(1).max(1),
                params.max_quest_above,
            )
        })
        .map(|r| r.id)
        .collect();
    quests.retain(|q| !incompatible.contains(&q.id) && (q.required || ordinary.contains(&q.id)));
    Ok(())
}
