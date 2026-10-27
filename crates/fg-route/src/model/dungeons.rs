//! Dungeons: entrances, the areas inside, and what lives there.

use super::types::{Dungeon, DungeonDef, EntityKind, Loc, Quest};
use crate::faction::Faction;
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
    faction: Faction,
    defs: &[DungeonDef],
    reachable: &HashSet<i64>,
) -> Result<(Vec<Dungeon>, HashMap<i64, usize>, HashMap<(EntityKind, i64), usize>)> {
    let mut rows: Vec<DungeonRow> = {
        let mut stmt = conn.prepare(
            "SELECT d.area, d.name, d.alt_areas, d.entrance_zone, d.entrance_x, d.entrance_y, m.ExpansionID
             FROM dungeon d
             JOIN client_areatable a ON a.ID = d.area
             JOIN client_map m ON m.InstanceType = 1 AND (m.ID = a.ContinentID OR m.AreaTableID = d.area)
             GROUP BY d.area",
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
    let own = faction_entrances(conn, faction)?;
    let mut dungeons = Vec::new();
    let mut by_area = HashMap::new();
    for (area, name, alt, zone, x, y, expansion) in rows {
        let def = defs.iter().find(|d| d.area == area).cloned().unwrap_or_default();
        // The settings' entrance first, then the one of the character's faction (a dungeon
        // with a door for each side), then the one every faction uses.
        let entrance = def
            .entrance
            .map(|e| (e.zone, e.x, e.y))
            .or_else(|| own.get(&area).copied())
            .or(match (zone, x, y) {
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
            min_level: 0,
            max_level: 0,
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
    // Bosses no source places but filed under the dungeon (Forever's: The Baron, Ruins of Lordaeron).
    let mut stmt = conn.prepare(
        "SELECT id, zone_id FROM m_npc n WHERE zone_id IS NOT NULL
           AND NOT EXISTS (SELECT 1 FROM m_spawn s WHERE s.entity_type = 'npc' AND s.entity_id = n.id)",
    )?;
    for row in stmt.query_map([], |r| Ok((r.get::<_, i64>(0)?, r.get::<_, i64>(1)?)))? {
        let (id, zone) = row?;
        if let Some(&d) = by_area.get(&zone) {
            inside.entry((EntityKind::Npc, id)).or_insert(d);
        }
    }
    Ok((dungeons, by_area, inside))
}

/// The entrances of a faction's own (zone, x, y) by dungeon area: dungeons with a door for each
/// side (`dungeon_entrance`, from the data overrides). An old database has no such table.
pub(super) fn faction_entrances(conn: &Connection, faction: Faction) -> Result<HashMap<i64, (i64, f64, f64)>> {
    let has_table: bool = conn.query_row(
        "SELECT count(*) > 0 FROM sqlite_master WHERE type = 'table' AND name = 'dungeon_entrance'",
        [],
        |r| r.get(0),
    )?;
    if !has_table {
        return Ok(HashMap::new());
    }
    let mut stmt =
        conn.prepare("SELECT area, zone, x, y FROM dungeon_entrance WHERE faction = ?1 ORDER BY source DESC")?;
    let rows = stmt.query_map([faction.letter()], |r| {
        Ok((r.get(0)?, (r.get(1)?, r.get(2)?, r.get(3)?)))
    })?;
    let mut entrances = HashMap::new();
    for row in rows {
        let (area, at) = row?;
        entrances.insert(area, at);
    }
    Ok(entrances)
}

/// The level of each dungeon (AreaTable ID): the middle of the game's range when set
/// (`DungeonDef::levels`), else the median level of its quests: those filed under it, else those
/// with an objective that lives only inside it. The app shows it and the planner runs the dungeon from it
/// (`Params::dungeon_from`, `Params::dungeon_to`).
pub fn dungeon_levels(conn: &Connection, defs: &[DungeonDef]) -> Result<HashMap<i64, i64>> {
    let mut stmt = conn.prepare(
        "WITH inside(dungeon, zone) AS (
           SELECT area, area FROM dungeon
           UNION SELECT d.area, j.value FROM dungeon d, json_each(coalesce(d.alt_areas, '[]')) j),
         -- What lives only in one dungeon (nowhere outside it).
         within(dungeon, entity_type, entity_id) AS (
           SELECT min(i.dungeon), s.entity_type, s.entity_id FROM m_spawn s LEFT JOIN inside i ON i.zone = s.zone_id
           GROUP BY s.entity_type, s.entity_id HAVING count(i.dungeon) = count(*) AND min(i.dungeon) = max(i.dungeon)),
         filed(dungeon, id, level) AS (
           SELECT i.dungeon, q.id, q.level FROM inside i JOIN m_quest q ON q.zone_or_sort = i.zone
           WHERE q.status = 'available' AND q.level > 0),
         done_inside(dungeon, id, level) AS (
           SELECT w.dungeon, q.id, q.level FROM m_quest_objective o JOIN m_quest q ON q.id = o.quest_id
           JOIN within w ON w.entity_type = CASE o.type WHEN 'object' THEN 'object' ELSE 'npc' END AND w.entity_id = o.target_id
           WHERE q.status = 'available' AND q.level > 0 AND o.type != 'item'
           UNION SELECT w.dungeon, q.id, q.level FROM m_quest_objective o JOIN m_quest q ON q.id = o.quest_id
           JOIN m_item_source i ON i.item_id = o.target_id AND i.from_type IN ('npc', 'object')
           JOIN within w ON w.entity_type = i.from_type AND w.entity_id = i.from_id
           WHERE q.status = 'available' AND q.level > 0 AND o.type = 'item')
         SELECT dungeon, level FROM filed
         UNION ALL SELECT dungeon, level FROM done_inside WHERE dungeon NOT IN (SELECT dungeon FROM filed)",
    )?;
    let mut levels: HashMap<i64, Vec<i64>> = HashMap::new();
    for row in stmt.query_map([], |r| Ok((r.get::<_, i64>(0)?, r.get::<_, i64>(1)?)))? {
        let (dungeon, level) = row?;
        levels.entry(dungeon).or_default().push(level);
    }
    let mut out: HashMap<i64, i64> = (levels.into_iter())
        .map(|(dungeon, mut l)| {
            l.sort_unstable();
            (dungeon, l[l.len() / 2])
        })
        .collect();
    for def in defs {
        if let Some([low, high]) = def.levels {
            out.insert(def.area, (low + high + 1) / 2);
        }
    }
    Ok(out)
}

/// Each dungeon's level (`dungeon_levels`, else its planned quests' median) and the window it is
/// run in (`window` around it, within the game's levels up to `max_level`; empty when the window
/// leaves none), with the default run time and kills. A dungeon without quests is not planned.
pub(super) fn set_dungeon_levels(
    dungeons: &mut [Dungeon],
    quests: &[Quest],
    levels: &HashMap<i64, i64>,
    window: (i64, i64),
    max_level: i64,
) {
    for (d, dungeon) in dungeons.iter_mut().enumerate() {
        let mut own: Vec<i64> = (quests.iter())
            .filter(|q| q.objectives.iter().any(|o| o.dungeon == Some(d)))
            .map(|q| q.level)
            .collect();
        if own.is_empty() {
            dungeon.min_level = i64::MAX;
            continue;
        }
        own.sort_unstable();
        dungeon.mob_level = levels.get(&dungeon.area).copied().unwrap_or(own[own.len() / 2]);
        dungeon.min_level = (dungeon.mob_level + window.0).max(1);
        dungeon.max_level = (dungeon.mob_level + window.1).min(max_level);
        if dungeon.minutes == 0.0 {
            dungeon.minutes = (40.0 + dungeon.mob_level as f64).clamp(50.0, 120.0);
        }
        if dungeon.kills == 0.0 {
            dungeon.kills = 60.0 + 2.0 * dungeon.mob_level as f64;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Each faction gets its own door of a dungeon; a dungeon without one for a faction, and a
    /// database from before the table, give nothing (the shared entrance is used).
    #[test]
    fn a_faction_takes_its_own_door_of_a_dungeon() {
        let conn = Connection::open_in_memory().unwrap();
        assert!(
            faction_entrances(&conn, Faction::Alliance).unwrap().is_empty(),
            "no table"
        );
        conn.execute_batch(
            "CREATE TABLE dungeon_entrance (area, faction, zone, x, y, source);
             INSERT INTO dungeon_entrance VALUES (7, 'A', 36, 14.7, 63.0, 'manual'),
                                                 (7, 'H', 36, 8.46, 59.21, 'manual'),
                                                 (8, 'H', 17, 50.0, 50.0, 'manual');",
        )
        .unwrap();
        let alliance = faction_entrances(&conn, Faction::Alliance).unwrap();
        let horde = faction_entrances(&conn, Faction::Horde).unwrap();
        assert_eq!(alliance.get(&7), Some(&(36, 14.7, 63.0)));
        assert_eq!(horde.get(&7), Some(&(36, 8.46, 59.21)));
        assert_eq!(alliance.get(&8), None, "no door for the Alliance: the shared one");
        assert_eq!(horde.get(&8), Some(&(17, 50.0, 50.0)));
    }
}
