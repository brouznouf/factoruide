//! Hostile mobs standing around the places a route goes to: a turn-in in the middle of a camp
//! (Sven's Revenge: a mound among level 25-27 Defias), an object among its guards. The
//! character must be able to fight through them before going there.

use super::types::Guard;
use crate::world::{Pos, World};
use anyhow::Result;
use rusqlite::Connection;
use std::collections::{HashMap, HashSet};

/// Spawns of a cell (continent, x, y): position, level, elite.
type Cells = HashMap<(i64, i64, i64), Vec<(f32, f32, u8, bool)>>;

/// Spawns of mobs hostile to both factions, by cell of `Guards::CELL` yards.
#[derive(Debug, Default)]
pub struct Guards {
    cells: Cells,
}

impl Guards {
    const CELL: f64 = 100.0;
    /// Spawns closer than this are one mob (the sources list some twice).
    const SAME: f64 = 10.0;
    /// Mobs this close to a place are met there (yards).
    pub const RADIUS: f64 = 30.0;
    /// Mobs this many levels below the strongest one around do not make the place harder.
    const LEVELS: u8 = 2;

    pub(super) fn load(conn: &Connection, world: &World) -> Result<Self> {
        let mut stmt = conn.prepare(
            "SELECT s.zone_id, s.x, s.y, coalesce(n.min_level, 1), coalesce(n.max_level, n.min_level, 1),
                    coalesce(n.rank, 0)
             FROM m_spawn s JOIN m_npc n ON n.id = s.entity_id
             WHERE s.entity_type = 'npc' AND s.x >= 0 AND coalesce(n.friendly_to, '') = ''",
        )?;
        let mut cells = Cells::new();
        let mut seen = HashSet::new();
        let rows = stmt.query_map([], |r| {
            Ok((
                r.get::<_, i64>(0)?,
                r.get::<_, f64>(1)?,
                r.get::<_, f64>(2)?,
                r.get::<_, i64>(3)?,
                r.get::<_, i64>(4)?,
                r.get::<_, i64>(5)?,
            ))
        })?;
        for row in rows {
            let (zone, x, y, min, max, rank) = row?;
            let Some(p) = world.to_world(zone, x, y) else { continue };
            let level = min.max(max).clamp(1, 255) as u8;
            let same = (
                p.continent,
                (p.x / Self::SAME).floor() as i64,
                (p.y / Self::SAME).floor() as i64,
                level,
            );
            if seen.insert(same) {
                cells.entry(Self::cell(p.continent, p.x, p.y)).or_default().push((
                    p.x as f32,
                    p.y as f32,
                    level,
                    rank == 1 || rank == 2,
                ));
            }
        }
        Ok(Self { cells })
    }

    fn cell(continent: i64, x: f64, y: f64) -> (i64, i64, i64) {
        (
            continent,
            (x / Self::CELL).floor() as i64,
            (y / Self::CELL).floor() as i64,
        )
    }

    /// The mobs within `RADIUS` of `p`: the strongest level, and how many are close to it.
    /// Nobody for a lone mob: it can be walked around, or only shows up during an event (Eliza
    /// near Abercrombie's hut).
    pub fn around(&self, p: &Pos) -> Guard {
        let r = Self::RADIUS;
        let (lo, hi) = (
            Self::cell(p.continent, p.x - r, p.y - r),
            Self::cell(p.continent, p.x + r, p.y + r),
        );
        let mut near = Vec::new();
        for cx in lo.1..=hi.1 {
            for cy in lo.2..=hi.2 {
                for &(x, y, level, elite) in self.cells.get(&(p.continent, cx, cy)).into_iter().flatten() {
                    let (dx, dy) = (f64::from(x) - p.x, f64::from(y) - p.y);
                    if dx * dx + dy * dy <= r * r {
                        near.push((level, elite));
                    }
                }
            }
        }
        let Some(top) = near.iter().map(|m| m.0).max() else {
            return Guard::default();
        };
        let strong = near.iter().filter(|m| m.0 + Self::LEVELS >= top);
        if strong.clone().count() < 2 {
            return Guard::default();
        }
        Guard {
            level: i64::from(top),
            count: strong.clone().count() as u32,
            elite: strong.clone().any(|m| m.1),
        }
    }
}
