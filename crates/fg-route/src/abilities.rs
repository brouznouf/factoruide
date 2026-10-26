//! Spells learned from quest rewards, class trainers and recorded characters.

use crate::{job::StartState, model::Profile};
use anyhow::Result;
use rusqlite::Connection;
use std::collections::HashMap;

pub struct Ability {
    pub name: String,
    /// Spells actually granted, rather than higher ranks that merely prove the ability is known.
    pub spells: Vec<i64>,
    known_spells: Vec<i64>,
    pub level: Option<i64>,
    pub rank: Option<i64>,
    pub requires: u64,
    quests: Vec<(i64, i64)>,
}

#[derive(Default)]
pub struct Abilities {
    pub entries: Vec<Ability>,
    by_quest: HashMap<i64, u64>,
}

impl Abilities {
    pub fn load(conn: &Connection, profile: &Profile) -> Result<Self> {
        let mut trainer_stmt = conn.prepare(
            "SELECT spell_id,name,rank,level FROM m_class_spell WHERE class_id=?1 ORDER BY name,rank,spell_id",
        )?;
        let trainer = trainer_stmt
            .query_map([profile.class_id], |r| {
                Ok((
                    r.get::<_, i64>(0)?,
                    r.get::<_, String>(1)?,
                    r.get::<_, i64>(2)?,
                    r.get::<_, i64>(3)?,
                ))
            })?
            .collect::<rusqlite::Result<Vec<_>>>()?;
        let mut abilities = Self::default();
        let mut stmt = conn.prepare(
            "SELECT id, reward_spell, coalesce(min_level,1) FROM m_quest WHERE status = 'available' AND reward_spell > 0
             AND (coalesce(classes_mask,0) & ?1 != 0 OR zone_or_sort = ?2)
             AND (coalesce(classes_mask,0) = 0 OR classes_mask & ?1 != 0)
             AND (coalesce(races_mask,0) = 0 OR races_mask & ?3 != 0) ORDER BY reward_spell, id",
        )?;
        let rows = stmt.query_map(
            rusqlite::params![
                profile.class_bit(),
                -crate::model::class_sort(profile.class_id),
                profile.race_bit()
            ],
            |r| Ok((r.get::<_, i64>(0)?, r.get::<_, i64>(1)?, r.get::<_, i64>(2)?)),
        )?;
        for row in rows {
            let (quest, spell, level) = row?;
            let spells = profile
                .quest_overrides
                .iter()
                .filter(|q| q.id == quest)
                .filter_map(|q| q.spells.clone())
                .next_back()
                .unwrap_or_else(|| vec![spell]);
            for spell in spells {
                let i = abilities.entry(conn, &trainer, spell)?;
                abilities.entries[i].quests.push((quest, level));
            }
        }
        // Quest spells and form-dependent ranks have their own learning state.
        for (name, weight) in &profile.spell_weights {
            let Some(required) = weight.requires() else { continue };
            let index = abilities.entry(conn, &trainer, required)?;
            let mask = 1 << index;
            for (spell, _, _, _) in trainer.iter().filter(|(_, n, _, _)| n == name) {
                let i = abilities.entry(conn, &trainer, *spell)?;
                abilities.entries[i].requires = mask;
            }
        }
        // Track later ranks when a quest grants the family's first trainer rank.
        // The last training level alone cannot prove those ranks were learned.
        let quest_families: Vec<_> = abilities
            .entries
            .iter()
            .enumerate()
            .filter(|(_, a)| !a.quests.is_empty() && profile.spell_weights.iter().any(|(name, _)| name == &a.name))
            .filter_map(|(i, a)| {
                let first = trainer
                    .iter()
                    .filter(|(_, name, _, _)| name == &a.name)
                    .map(|(_, _, rank, _)| *rank)
                    .min()?;
                trainer
                    .iter()
                    .any(|(spell, _, rank, _)| *rank == first && a.spells.contains(spell))
                    .then(|| (a.name.clone(), i))
            })
            .collect();
        for (spell, name, _, _) in &trainer {
            if let Some((_, first)) = quest_families.iter().find(|(family, _)| family == name) {
                let i = abilities.entry(conn, &trainer, *spell)?;
                if i != *first {
                    abilities.entries[i].requires |= 1 << *first;
                }
            }
        }
        for (i, a) in abilities.entries.iter_mut().enumerate() {
            a.rank = trainer
                .iter()
                .find(|(s, _, _, _)| a.spells.contains(s))
                .map(|(_, _, rank, _)| *rank);
            for (spell, _, rank, level) in trainer.iter().filter(|(_, n, _, _)| *n == a.name) {
                if a.rank.is_some_and(|known| *rank >= known) {
                    a.known_spells.push(*spell);
                }
                if Some(*rank) == a.rank && a.quests.is_empty() {
                    a.spells.push(*spell);
                    a.level = Some(a.level.map_or(*level, |old| old.min(*level)));
                }
            }
            a.spells.sort_unstable();
            a.spells.dedup();
            for &(quest, _) in &a.quests {
                *abilities.by_quest.entry(quest).or_default() |= 1 << i;
            }
        }
        Ok(abilities)
    }

    fn entry(&mut self, conn: &Connection, trainer: &[(i64, String, i64, i64)], spell: i64) -> Result<usize> {
        if let Some(i) = self.entries.iter().position(|a| a.spells.contains(&spell)) {
            return Ok(i);
        }
        let name = match trainer.iter().find(|(id, _, _, _)| *id == spell) {
            Some((_, name, _, _)) => name.clone(),
            None => conn.query_row("SELECT Name_lang FROM client_spellname WHERE ID=?1", [spell], |r| {
                r.get(0)
            })?,
        };
        self.entries.push(Ability {
            name,
            spells: vec![spell],
            known_spells: vec![spell],
            level: None,
            rank: None,
            requires: 0,
            quests: vec![],
        });
        Ok(self.entries.len() - 1)
    }

    /// Recorded characters use their spellbook and completed quests. Without a profile,
    /// assume class unlocks from below the starting level were already acquired.
    pub fn initial(&self, start: Option<&StartState>, level: i64) -> u64 {
        if let Some(start) = start {
            return self
                .entries
                .iter()
                .enumerate()
                .filter(|(_, a)| {
                    a.known_spells.iter().any(|s| start.spells.contains(s))
                        || a.quests.iter().any(|(q, _)| start.completed.contains(q))
                })
                .fold(0, |mask, (i, _)| mask | 1 << i);
        }
        let earlier = self
            .entries
            .iter()
            .enumerate()
            .filter(|(_, a)| a.quests.iter().any(|(_, at)| *at < level))
            .fold(0, |mask, (i, _)| mask | 1 << i);
        self.trained(earlier, level)
    }

    pub fn trained(&self, known: u64, level: i64) -> u64 {
        let available = |a: &Ability| a.level.is_some_and(|at| at <= level);
        let forms = self
            .entries
            .iter()
            .enumerate()
            .filter(|(_, a)| available(a) && a.requires == 0)
            .fold(known, |mask, (i, _)| mask | 1 << i);
        self.entries
            .iter()
            .enumerate()
            .filter(|(_, a)| available(a) && forms & a.requires == a.requires)
            .fold(forms, |mask, (i, _)| mask | 1 << i)
    }

    pub fn bit(&self, spell: i64) -> Option<u64> {
        self.entries
            .iter()
            .position(|a| a.spells.contains(&spell))
            .map(|i| 1 << i)
    }

    pub fn quest(&self, quest: i64) -> u64 {
        self.by_quest.get(&quest).copied().unwrap_or(0)
    }

    /// Only trainer-learned spells belong in the addon's purchase list.
    pub fn spells(&self) -> impl Iterator<Item = i64> + '_ {
        self.entries
            .iter()
            .filter(|a| a.level.is_some())
            .flat_map(|a| a.spells.iter().copied())
    }
}
