//! Mobs anyone can farm on the way: the spawns of normal hostile or neutral mobs, by cell, to
//! count those met along a walk.

use crate::world::{Pos, World};
use anyhow::Result;
use rusqlite::Connection;
use std::collections::{HashMap, HashSet};

/// Spawns of a cell (continent, x, y): position and level.
type Cells = HashMap<(i64, i64, i64), Vec<(f32, f32, u8)>>;

/// Spawns of normal mobs by cell of `FarmMobs::CELL` yards.
#[derive(Debug, Default)]
pub struct FarmMobs {
    cells: Cells,
}

impl FarmMobs {
    const CELL: f64 = 100.0;
    /// Spawns closer than this are one mob (the sources list some twice).
    const SAME: f64 = 10.0;

    /// Normal mobs (not elite or rare, no gossip, vendor or quest flag) that are not of a faction:
    /// hostile to both, or neutral beasts.
    pub(super) fn load(conn: &Connection, world: &World) -> Result<Self> {
        let mut stmt = conn.prepare(
            "SELECT s.zone_id, s.x, s.y, coalesce(n.min_level, 1), coalesce(n.max_level, n.min_level, 1)
             FROM m_spawn s JOIN m_npc n ON n.id = s.entity_id
             WHERE s.entity_type = 'npc' AND s.x >= 0 AND coalesce(n.rank, 0) = 0
               AND coalesce(n.npc_flags, 0) = 0 AND coalesce(n.friendly_to, '') IN ('', 'AH')",
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
            ))
        })?;
        for row in rows {
            let (zone, x, y, min, max) = row?;
            let Some(p) = world.to_world(zone, x, y) else { continue };
            let level = i64::midpoint(min, max).clamp(1, 255) as u8;
            let same = (
                p.continent,
                (p.x / Self::SAME).floor() as i64,
                (p.y / Self::SAME).floor() as i64,
                level,
            );
            if seen.insert(same) {
                cells
                    .entry(Self::cell(&p))
                    .or_default()
                    .push((p.x as f32, p.y as f32, level));
            }
        }
        Ok(Self { cells })
    }

    fn cell(p: &Pos) -> (i64, i64, i64) {
        (
            p.continent,
            (p.x / Self::CELL).floor() as i64,
            (p.y / Self::CELL).floor() as i64,
        )
    }

    /// Mobs within `radius` of `p` whose level is in `levels`.
    pub fn around(&self, p: &Pos, radius: f64, levels: std::ops::RangeInclusive<i64>) -> usize {
        let (lo, hi) = (
            Self::cell(&Pos {
                x: p.x - radius,
                y: p.y - radius,
                ..*p
            }),
            Self::cell(&Pos {
                x: p.x + radius,
                y: p.y + radius,
                ..*p
            }),
        );
        let mut n = 0;
        for cx in lo.1..=hi.1 {
            for cy in lo.2..=hi.2 {
                n += self
                    .cells
                    .get(&(p.continent, cx, cy))
                    .into_iter()
                    .flatten()
                    .filter(|&&(x, y, level)| {
                        levels.contains(&i64::from(level)) && (f64::from(x) - p.x).hypot(f64::from(y) - p.y) <= radius
                    })
                    .count();
            }
        }
        n
    }

    /// Mobs within `corridor` of the segment `a`-`b` (same continent), by level: (level, count).
    pub fn along(&self, a: &Pos, b: &Pos, corridor: f64) -> Vec<(u8, u16)> {
        let mut counts = [0u16; 256];
        let (lo, hi) = (
            Self::cell(&Pos {
                x: a.x.min(b.x) - corridor,
                y: a.y.min(b.y) - corridor,
                ..*a
            }),
            Self::cell(&Pos {
                x: a.x.max(b.x) + corridor,
                y: a.y.max(b.y) + corridor,
                ..*a
            }),
        );
        for cx in lo.1..=hi.1 {
            for cy in lo.2..=hi.2 {
                // Cells farther than the corridor from the segment hold nothing to count.
                let center = Pos {
                    x: (cx as f64 + 0.5) * Self::CELL,
                    y: (cy as f64 + 0.5) * Self::CELL,
                    ..*a
                };
                if crate::plan::segment_dist(&center, a, b) > corridor + Self::CELL * 0.75 {
                    continue;
                }
                for &(x, y, level) in self.cells.get(&(a.continent, cx, cy)).into_iter().flatten() {
                    let p = Pos {
                        x: f64::from(x),
                        y: f64::from(y),
                        ..*a
                    };
                    if crate::plan::segment_dist(&p, a, b) <= corridor {
                        counts[level as usize] = counts[level as usize].saturating_add(1);
                    }
                }
            }
        }
        counts
            .iter()
            .enumerate()
            .filter(|(_, n)| **n > 0)
            .map(|(level, n)| (level as u8, *n))
            .collect()
    }
}
