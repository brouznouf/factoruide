//! Areas to explore for their discovery XP.

use super::types::ExploreArea;
use crate::world::World;
use anyhow::Result;
use rusqlite::Connection;
use rustc_hash::FxHashMap;
use std::collections::HashSet;

/// Explorable areas: one per map exploration overlay area, at the overlay's center.
#[expect(
    clippy::type_complexity,
    reason = "private loader whose parts are destructured by its only caller"
)]
pub(super) fn load_explore(conn: &Connection, world: &World) -> Result<(Vec<ExploreArea>, FxHashMap<i64, Vec<usize>>)> {
    let mut seen = HashSet::new();
    let mut areas = Vec::new();
    for a in crate::world::map_areas(conn)?
        .iter()
        .filter(|a| a.exploration_level > 0)
    {
        if !seen.insert(a.id) {
            continue;
        }
        if let Some(pos) = world.to_world(a.zone, a.x, a.y) {
            areas.push(ExploreArea {
                pos,
                zone: a.zone,
                level: a.exploration_level,
            });
        }
    }
    let mut by_zone: FxHashMap<i64, Vec<usize>> = FxHashMap::default();
    for (i, a) in areas.iter().enumerate() {
        by_zone.entry(a.zone).or_default().push(i);
    }
    Ok((areas, by_zone))
}
