//! Hostile mobs standing around the places a route goes to: a turn-in in the middle of a camp
//! (Sven's Revenge: a mound among level 25-27 Defias), an object among its guards. The
//! character must be able to fight through them before going there.

use super::types::Guard;
use crate::faction::Faction;
use crate::world::{Pos, World};
use anyhow::Result;
use rusqlite::Connection;
use rustc_hash::FxHashMap;
use std::collections::{HashMap, HashSet};

/// Spawns of a cell (continent, x, y): position, level, elite.
type Cells = FxHashMap<(i64, i64, i64), Vec<(f32, f32, u8, bool)>>;

/// Spawns of mobs hostile to both factions, by cell of `Guards::CELL` yards.
#[derive(Debug, Default)]
pub struct Guards {
    cells: Cells,
    /// Spawns of the mobs only the other side likes (a Kul Tiras camp, for the Horde): hostile
    /// to the character too, but not to the other side, so left out of `cells`.
    rivals: Cells,
    /// Spawns of the creatures of neither a faction nor a side known (WoW Forever's own NPCs the
    /// sources know little of): townsfolk around a giver, but maybe monsters out in the field,
    /// so left out of `cells` and counted only out of the character's towns, near its level.
    unknown: Cells,
    /// Spawns of the creatures of the character's side: where they stand is one of its towns or
    /// camps, the creatures of no known side there are its townsfolk.
    friends: Cells,
    /// Every one of them (only those at the same place merged: one spawn point the sources give
    /// for several mobs that take turns), to count the mobs of a pull.
    mobs: HashMap<(i64, i64, i64), Vec<(f32, f32)>>,
}

impl Guards {
    const CELL: f64 = 100.0;
    /// Spawns closer than this are one mob (the sources list some twice).
    const SAME: f64 = 10.0;
    /// Mobs this close to a place are met there (yards).
    pub(crate) const RADIUS: f64 = 30.0;
    /// Mobs this many levels below the strongest one around do not make the place harder.
    const LEVELS: u8 = 2;
    /// Creatures of no known side this many levels or more above a quest guard nothing of it:
    /// townsfolk the sources placed with no side (Southshore Defenders around Tarren Mill, level 55
    /// by a level 20 quest), not the monsters of its place.
    const UNKNOWN_ABOVE: i64 = 10;
    /// Spawn points closer than this are one mob (mobs taking turns at one point).
    pub(crate) const ONE_POINT: f64 = 1.5;
    /// Mobs this close to the one pulled come with it (yards).
    const LINK: f64 = 8.0;

    pub(super) fn load(conn: &Connection, world: &World, faction: Faction) -> Result<Self> {
        let (mine, other) = match faction {
            Faction::Alliance => ("A", "H"),
            Faction::Horde => ("H", "A"),
        };
        // The hostile spawns the character can meet, read once for a database and a side.
        let rows = crate::memo::rows(conn, "guards", crate::memo::setting(mine), || {
            let mut stmt = conn.prepare(
                "SELECT s.zone_id, s.x, s.y, coalesce(n.min_level, 1), coalesce(n.max_level, n.min_level, 1),
                        coalesce(n.rank, 0), coalesce(n.friendly_to, ''),
                        coalesce(n.faction_id, 0) = 0 AND coalesce(n.friendly_to, '') = ''
                 FROM m_spawn s JOIN m_npc n ON n.id = s.entity_id
                 WHERE s.entity_type = 'npc' AND s.x >= 0",
            )?;
            let rows = stmt.query_map([], |r| {
                Ok((
                    r.get::<_, i64>(0)?,
                    r.get::<_, f64>(1)?,
                    r.get::<_, f64>(2)?,
                    r.get::<_, i64>(3)?,
                    r.get::<_, i64>(4)?,
                    r.get::<_, i64>(5)?,
                    r.get::<_, String>(6)?,
                    r.get::<_, bool>(7)?,
                ))
            })?;
            Ok(rows.collect::<rusqlite::Result<Vec<_>>>()?)
        })?;
        let mut cells = Cells::default();
        let mut rivals = Cells::default();
        let mut unknown_cells = Cells::default();
        let mut friends = Cells::default();
        let mut mobs: HashMap<(i64, i64, i64), Vec<(f32, f32)>> = HashMap::new();
        let mut seen = HashSet::new();
        for (zone, x, y, min, max, rank, friendly, unknown) in rows.iter() {
            let (zone, x, y, min, max, rank) = (*zone, *x, *y, *min, *max, *rank);
            // Neither a faction nor a side known, in a capital: a townsman the sources know little
            // of (WoW Forever's own NPCs), not a monster guarding the givers around it.
            if *unknown && world.is_capital(zone) {
                continue;
            }
            let Some(p) = world.to_world(zone, x, y) else { continue };
            let level = min.max(max).clamp(1, 255) as u8;
            if friendly.contains(mine) {
                friends
                    .entry(Self::cell(p.continent, p.x, p.y))
                    .or_default()
                    .push((p.x as f32, p.y as f32, level, false));
                continue;
            }
            if friendly.contains(other) {
                rivals.entry(Self::cell(p.continent, p.x, p.y)).or_default().push((
                    p.x as f32,
                    p.y as f32,
                    level,
                    rank == 1 || rank == 2,
                ));
                continue;
            }
            let cell = mobs.entry(Self::cell(p.continent, p.x, p.y)).or_default();
            if !cell
                .iter()
                .any(|&(x, y)| (f64::from(x) - p.x).hypot(f64::from(y) - p.y) < Self::ONE_POINT)
            {
                cell.push((p.x as f32, p.y as f32));
            }
            let same = (
                p.continent,
                (p.x / Self::SAME).floor() as i64,
                (p.y / Self::SAME).floor() as i64,
                level,
                *unknown,
            );
            if seen.insert(same) {
                let to = if *unknown { &mut unknown_cells } else { &mut cells };
                to.entry(Self::cell(p.continent, p.x, p.y)).or_default().push((
                    p.x as f32,
                    p.y as f32,
                    level,
                    rank == 1 || rank == 2,
                ));
            }
        }
        Ok(Self {
            cells,
            rivals,
            unknown: unknown_cells,
            friends,
            mobs,
        })
    }

    /// Hostile mobs that come together when the one at `p` is pulled, itself included (1 when
    /// it stands alone, or is not a known hostile spawn).
    pub(crate) fn pull_at(&self, p: &Pos) -> u32 {
        let r = Self::LINK;
        let (lo, hi) = (
            Self::cell(p.continent, p.x - r, p.y - r),
            Self::cell(p.continent, p.x + r, p.y + r),
        );
        let mut n = 0;
        for cx in lo.1..=hi.1 {
            for cy in lo.2..=hi.2 {
                for &(x, y) in self.mobs.get(&(p.continent, cx, cy)).into_iter().flatten() {
                    if (f64::from(x) - p.x).hypot(f64::from(y) - p.y) <= r {
                        n += 1;
                    }
                }
            }
        }
        n.max(1)
    }

    fn cell(continent: i64, x: f64, y: f64) -> (i64, i64, i64) {
        (
            continent,
            (x / Self::CELL).floor() as i64,
            (y / Self::CELL).floor() as i64,
        )
    }

    /// The mobs within `RADIUS` of the place `p` of an objective of a quest of `level`: the
    /// strongest level, and how many are close to it. Nobody for a lone mob: it can be walked
    /// around, or only shows up during an event (Eliza near Abercrombie's hut). The creatures of
    /// no known side count out of the character's towns and up to `UNKNOWN_ABOVE` levels above
    /// the quest.
    pub fn around(&self, p: &Pos, level: i64) -> Guard {
        self.around_in(p, false, Some(level))
    }

    /// The same around what gives or takes back a quest (a creature, an object): the creatures
    /// of no known side standing by it are its townsfolk, not guards.
    pub fn around_giver(&self, p: &Pos) -> Guard {
        self.around_in(p, false, None)
    }

    /// `around` with the mobs of the other side that are hostile to the character: for a place
    /// found where it lies (an item, an object), not a town's givers, which the other side's
    /// guards stand around.
    pub fn around_hostile(&self, p: &Pos, level: i64) -> Guard {
        self.around_in(p, true, Some(level))
    }

    /// Spawns of `cells` within `RADIUS` of `p`.
    fn within<'c>(cells: &'c Cells, p: &Pos) -> impl Iterator<Item = (u8, bool)> + 'c {
        let r = Self::RADIUS;
        let (lo, hi) = (
            Self::cell(p.continent, p.x - r, p.y - r),
            Self::cell(p.continent, p.x + r, p.y + r),
        );
        let (px, py) = (p.x, p.y);
        let continent = p.continent;
        (lo.1..=hi.1)
            .flat_map(move |cx| (lo.2..=hi.2).map(move |cy| (continent, cx, cy)))
            .filter_map(move |cell| cells.get(&cell))
            .flatten()
            .filter(move |&&(x, y, _, _)| {
                let (dx, dy) = (f64::from(x) - px, f64::from(y) - py);
                dx * dx + dy * dy <= r * r
            })
            .map(|&(_, _, level, elite)| (level, elite))
    }

    /// `unknown`: the level of the quest when the creatures of no known side count.
    fn around_in(&self, p: &Pos, rivals: bool, unknown: Option<i64>) -> Guard {
        let mut near: Vec<(u8, bool)> = Self::within(&self.cells, p).collect();
        if rivals {
            near.extend(Self::within(&self.rivals, p));
        }
        if let Some(quest) = unknown
            && Self::within(&self.friends, p).next().is_none()
        {
            near.extend(Self::within(&self.unknown, p).filter(|m| i64::from(m.0) < quest + Self::UNKNOWN_ABOVE));
        }
        let Some(top) = near.iter().map(|m| m.0).max() else {
            return Guard::default();
        };
        // Saturating: levels are stored up to 255 (bosses of unknown level).
        let strong = near.iter().filter(|m| m.0.saturating_add(Self::LEVELS) >= top);
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
