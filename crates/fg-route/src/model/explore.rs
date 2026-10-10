//! Areas to explore for their discovery XP.

use super::types::ExploreArea;
use crate::world::World;
use anyhow::Result;
use rusqlite::Connection;
use std::collections::{HashMap, HashSet};

/// Explorable areas: one per map exploration overlay area, at the overlay's center.
#[expect(
    clippy::type_complexity,
    reason = "private loader whose parts are destructured by its only caller"
)]
pub(super) fn load_explore(conn: &Connection, world: &World) -> Result<(Vec<ExploreArea>, HashMap<i64, Vec<usize>>)> {
    let mut stmt = conn.prepare(
        "SELECT t.ID, a.AreaID, t.ExplorationLevel,
                (o.OffsetX + o.TextureWidth / 2.0) * 100.0 / l.LayerWidth,
                (o.OffsetY + o.TextureHeight / 2.0) * 100.0 / l.LayerHeight
         FROM client_worldmapoverlay o
         JOIN client_uimapxmapart x ON x.UiMapArtID = o.UiMapArtID
         JOIN client_uimapart ua ON ua.ID = o.UiMapArtID
         JOIN client_uimapartstylelayer l ON l.UiMapArtStyleID = ua.UiMapArtStyleID AND l.LayerIndex = 0
         JOIN client_uimapassignment a ON a.UiMapID = x.UiMapID AND a.OrderIndex = 0
         JOIN client_areatable t ON t.ID IN (o.AreaID_0, o.AreaID_1, o.AreaID_2, o.AreaID_3)
         WHERE t.ExplorationLevel > 0",
    )?;
    let mut seen = HashSet::new();
    let mut areas = Vec::new();
    for row in stmt.query_map([], |r| {
        Ok((
            r.get::<_, i64>(0)?,
            r.get::<_, i64>(1)?,
            r.get::<_, i64>(2)?,
            r.get::<_, f64>(3)?,
            r.get::<_, f64>(4)?,
        ))
    })? {
        let (id, zone, level, x, y) = row?;
        if !seen.insert(id) {
            continue;
        }
        if let Some(pos) = world.to_world(zone, x, y) {
            areas.push(ExploreArea { pos, zone, level });
        }
    }
    let mut by_zone: HashMap<i64, Vec<usize>> = HashMap::new();
    for (i, a) in areas.iter().enumerate() {
        by_zone.entry(a.zone).or_default().push(i);
    }
    Ok((areas, by_zone))
}
