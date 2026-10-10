//! Quest reward items and dungeon boss loot the character can wear.

use super::types::{EntityKind, Quest};
use crate::params::Params;
use anyhow::Result;
use rusqlite::Connection;
use std::collections::HashMap;

/// Quest reward items and dungeon boss loot the character can wear.
#[expect(
    clippy::type_complexity,
    reason = "private loader whose parts are destructured by its only caller"
)]
pub(super) fn load_rewards(
    conn: &Connection,
    power: &crate::power::PowerModel,
    quests: &[Quest],
    dungeons: usize,
    inside: &HashMap<(EntityKind, i64), usize>,
    params: &Params,
) -> Result<(Vec<(Vec<u16>, Vec<u16>)>, Vec<Vec<(u16, f64)>>)> {
    let mut by_quest: HashMap<i64, (Vec<u16>, Vec<u16>)> = HashMap::new();
    let mut stmt = conn.prepare("SELECT quest_id, item_id, choice FROM m_quest_reward")?;
    let mut rows = stmt.query([])?;
    while let Some(r) = rows.next()? {
        let (quest, item, choice): (i64, i64, i64) = (r.get(0)?, r.get(1)?, r.get(2)?);
        let Some(i) = power.item(item) else { continue };
        let entry = by_quest.entry(quest).or_default();
        if choice != 0 {
            entry.1.push(i);
        } else {
            entry.0.push(i);
        }
    }
    let rewards = quests
        .iter()
        .map(|q| by_quest.remove(&q.id).unwrap_or_default())
        .collect();
    let mut loot = vec![Vec::new(); dungeons];
    let mut stmt = conn.prepare("SELECT npc_id, item_id, chance FROM m_boss_loot")?;
    let mut rows = stmt.query([])?;
    while let Some(r) = rows.next()? {
        let (npc, item, chance): (i64, i64, f64) = (r.get(0)?, r.get(1)?, r.get(2)?);
        let (Some(&d), Some(i)) = (inside.get(&(EntityKind::Npc, npc)), power.item(item)) else {
            continue;
        };
        loot[d].push((i, (chance * params.dungeon_loot_share).min(1.0)));
    }
    Ok((rewards, loot))
}
