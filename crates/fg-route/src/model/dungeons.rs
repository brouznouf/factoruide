//! Dungeons: entrances, the areas inside, and what lives there.

use super::types::{Dungeon, DungeonDef, EntityKind, Loc, Quest};
use crate::world::World;
use crate::xp;
use anyhow::Result;
use rusqlite::Connection;
use std::collections::{HashMap, HashSet};

/// A dungeon of the database: area, name, other areas inside, entrance (zone, x, y), expansion.
type DungeonRow = (
    i64,
    String,
    Option<String>,
    Option<i64>,
    Option<f64>,
    Option<f64>,
    Option<i64>,
);

/// Reachable 5-man dungeons, and which entities live inside each.
#[expect(
    clippy::type_complexity,
    reason = "private loader whose parts are destructured by its only caller"
)]
pub(super) fn load_dungeons(
    conn: &Connection,
    world: &World,
    defs: &[DungeonDef],
    reachable: &HashSet<i64>,
) -> Result<(Vec<Dungeon>, HashMap<i64, usize>, HashMap<(EntityKind, i64), usize>)> {
    let mut rows: Vec<DungeonRow> = {
        let mut stmt = conn.prepare(
            "SELECT d.area, d.name, d.alt_areas, d.entrance_zone, d.entrance_x, d.entrance_y, m.ExpansionID
             FROM dungeon d
             JOIN client_areatable a ON a.ID = d.area
             JOIN client_map m ON m.ID = a.ContinentID
             WHERE m.InstanceType = 1",
        )?;
        stmt.query_map([], |r| {
            Ok((
                r.get(0)?,
                r.get(1)?,
                r.get(2)?,
                r.get(3)?,
                r.get(4)?,
                r.get(5)?,
                r.get(6)?,
            ))
        })?
        .collect::<rusqlite::Result<_>>()?
    };
    for def in defs {
        if !rows.iter().any(|r| r.0 == def.area) {
            rows.push((
                def.area,
                def.name.clone().unwrap_or_default(),
                None,
                None,
                None,
                None,
                None,
            ));
        }
    }
    let mut dungeons = Vec::new();
    let mut by_area = HashMap::new();
    for (area, name, alt, zone, x, y, expansion) in rows {
        let def = defs.iter().find(|d| d.area == area).cloned().unwrap_or_default();
        let entrance = def.entrance.map(|e| (e.zone, e.x, e.y)).or(match (zone, x, y) {
            (Some(z), Some(x), Some(y)) => Some((z, x, y)),
            _ => None,
        });
        let Some((ez, ex, ey)) = entrance else { continue };
        let Some(pos) = world.to_world(ez, ex, ey) else {
            continue;
        };
        if !reachable.contains(&pos.continent) {
            continue;
        }
        let index = dungeons.len();
        by_area.insert(area, index);
        let alt: Vec<i64> = alt.and_then(|a| serde_json::from_str(&a).ok()).unwrap_or_default();
        for a in alt {
            by_area.insert(a, index);
        }
        let name = def.name.clone().unwrap_or(name);
        dungeons.push(Dungeon {
            area,
            entrance: Loc {
                pos,
                zone: ez,
                kind: EntityKind::Dungeon,
                id: area,
                name: name.clone(),
            },
            name,
            min_level: def.min_level.unwrap_or(0),
            max_level: def.max_level.unwrap_or(0),
            minutes: def.minutes.unwrap_or(0.0),
            kills: def.kills.unwrap_or(0.0),
            mob_level: 0,
            content: expansion.map_or_else(|| xp::content_at(pos.continent, ez), |e| e.max(0) as usize),
        });
    }
    let mut inside = HashMap::new();
    let mut stmt = conn.prepare("SELECT DISTINCT entity_type, entity_id, zone_id FROM m_spawn")?;
    for row in stmt.query_map([], |r| {
        Ok((r.get::<_, String>(0)?, r.get::<_, i64>(1)?, r.get::<_, i64>(2)?))
    })? {
        let (t, id, zone) = row?;
        if let (Some(&d), Some(kind)) = (by_area.get(&zone), EntityKind::parse(&t)) {
            inside.insert((kind, id), d);
        }
    }
    Ok((dungeons, by_area, inside))
}

/// Dungeon levels from their quests, and default run time and kills. A dungeon without quests
/// is not planned.
pub(super) fn set_dungeon_levels(dungeons: &mut [Dungeon], quests: &[Quest]) {
    for (d, dungeon) in dungeons.iter_mut().enumerate() {
        let qs: Vec<&Quest> = quests
            .iter()
            .filter(|q| q.objectives.iter().any(|o| o.dungeon == Some(d)))
            .collect();
        if qs.is_empty() {
            dungeon.min_level = i64::MAX;
            continue;
        }
        let mut levels: Vec<i64> = qs.iter().map(|q| q.level).collect();
        levels.sort_unstable();
        if dungeon.min_level == 0 {
            dungeon.min_level = qs.iter().map(|q| q.min_level).min().unwrap().max(levels[0] - 2);
        }
        if dungeon.max_level == 0 {
            dungeon.max_level = levels[levels.len() - 1] + 3;
        }
        dungeon.mob_level = levels[levels.len() / 2];
        if dungeon.minutes == 0.0 {
            dungeon.minutes = (40.0 + dungeon.mob_level as f64).clamp(50.0, 120.0);
        }
        if dungeon.kills == 0.0 {
            dungeon.kills = 60.0 + 2.0 * dungeon.mob_level as f64;
        }
    }
}
