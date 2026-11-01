//! Common loot a route gathers by itself: cloth, book pages... items hundreds of mob types drop,
//! that quests ask for by the stack (sixty Wool Cloth for a donation). The kills of the route
//! (objectives, grinding, mobs met on the way) collect them where they happen, and an objective
//! asking for them only gathers or buys what the bags do not hold yet.

use super::types::Quest;
use anyhow::Result;
use rusqlite::Connection;
use rustc_hash::FxHashMap;

/// The common loot quests ask for, and what a kill drops of it in each zone.
#[derive(Debug, Default)]
pub struct CommonLoot {
    /// Item of each index (`Objective::stock`, `State::stock`).
    pub items: Vec<i64>,
    /// By zone: the items expected per kill of a normal mob there (index, count). A zone's mobs
    /// are counted from their spawns, the droppers' share weighted by their drop chance.
    pub by_zone: FxHashMap<i64, Vec<(u16, f32)>>,
}

impl CommonLoot {
    /// The common loot of the objectives built (`Objective::stock` set), each objective given
    /// its item's index.
    pub(super) fn load(conn: &Connection, quests: &mut [Quest]) -> Result<Self> {
        let mut items: Vec<i64> = Vec::new();
        for o in quests.iter_mut().flat_map(|q| q.objectives.iter_mut()) {
            if o.stock.is_some() {
                let i = items.iter().position(|&it| it == o.loc.id).unwrap_or_else(|| {
                    items.push(o.loc.id);
                    items.len() - 1
                });
                o.stock = Some(i as u16);
            }
        }
        if items.is_empty() {
            return Ok(Self::default());
        }
        // Normal hostile (or neutral) mobs, as `FarmMobs` counts them: the kills of a zone.
        let mobs = crate::memo::rows(conn, "mobs of a zone", 0, || {
            let mut stmt = conn.prepare(
                "SELECT s.zone_id, count(*) FROM m_spawn s JOIN m_npc n ON n.id = s.entity_id
                 WHERE s.entity_type = 'npc' AND s.x >= 0 AND coalesce(n.rank, 0) = 0
                   AND coalesce(n.npc_flags, 0) = 0 AND coalesce(n.friendly_to, '') IN ('', 'AH')
                 GROUP BY s.zone_id",
            )?;
            Ok(stmt
                .query_map([], |r| Ok((r.get::<_, i64>(0)?, r.get::<_, f64>(1)?)))?
                .collect::<Result<FxHashMap<i64, f64>, _>>()?)
        })?;
        let list = items.iter().map(i64::to_string).collect::<Vec<_>>().join(",");
        let drops = crate::memo::rows(conn, "drops of items", crate::memo::setting(&list), || {
            let mut stmt = conn.prepare(&format!(
                "SELECT i.item_id, s.zone_id, sum(i.chance) FROM m_item_source i
                 JOIN m_spawn s ON s.entity_type = 'npc' AND s.entity_id = i.from_id AND s.x >= 0
                 WHERE i.from_type = 'npc' AND i.chance > 0 AND i.item_id IN ({list})
                 GROUP BY i.item_id, s.zone_id"
            ))?;
            let rows = stmt.query_map([], |r| {
                Ok((r.get::<_, i64>(0)?, r.get::<_, i64>(1)?, r.get::<_, f64>(2)?))
            })?;
            Ok(rows.collect::<rusqlite::Result<Vec<_>>>()?)
        })?;
        let mut by_zone: FxHashMap<i64, Vec<(u16, f32)>> = FxHashMap::default();
        for &(item, zone, dropped) in drops.iter() {
            let (Some(i), Some(&total)) = (items.iter().position(|&it| it == item), mobs.get(&zone)) else {
                continue;
            };
            // Droppers' spawns weighted by their chance, over every mob of the zone.
            let each = (dropped / total).min(1.0) as f32;
            if each > 0.0 {
                by_zone.entry(zone).or_default().push((i as u16, each));
            }
        }
        Ok(Self { items, by_zone })
    }
}
