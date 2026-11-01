//! Mobs anyone can farm on the way: the spawns of normal hostile or neutral mobs, by cell, to
//! count those met along a walk, and the camps where a character grinds.

use super::types::Quest;
use crate::world::{Pos, World};
use anyhow::Result;
use rusqlite::Connection;
use rustc_hash::FxHashMap;

/// A spawn: position, level, and the quests whose objectives are done on its mob (see
/// `FarmMobs::quests`).
#[derive(Debug, Clone, Copy)]
struct Spawn {
    x: f32,
    y: f32,
    level: u8,
    tag: u32,
}

/// A cell: continent, x, y.
type CellKey = (i64, i64, i64);

/// Spawns of a cell.
type Cells = FxHashMap<CellKey, Vec<Spawn>>;
/// The zone and height of each spawn of a cell, in the order of its `Cells` (for the camps).
type CellPlaces = FxHashMap<CellKey, Vec<(i64, Option<i16>)>>;

/// A spawn of a normal mob as the database gives it.
#[derive(Clone)]
pub(super) struct MobRow {
    pub(super) zone: i64,
    pub(super) x: f64,
    pub(super) y: f64,
    pub(super) min_level: i64,
    pub(super) max_level: i64,
    pub(super) npc: i64,
    pub(super) z: Option<f64>,
}

/// Mobs met along a walk: their level, how many, and the quests whose objectives are done on
/// them (`FarmMobs::quests`; 0: none).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Met {
    pub level: u8,
    pub count: u16,
    pub tag: u32,
}

/// Spawns of normal mobs by cell of `FarmMobs::CELL` yards.
#[derive(Debug, Default)]
pub struct FarmMobs {
    cells: Cells,
    /// Camps to grind at, by continent and character level (`camps`).
    camps: FxHashMap<(i64, u8), Vec<Pos>>,
    /// The quests (indices in `Model::quests`) whose kill or loot objectives are done on the
    /// mobs of each tag, tag 1 first (0: none).
    quests: Vec<Box<[u32]>>,
}

impl FarmMobs {
    const CELL: f64 = 100.0;
    /// Spawns closer than this are one mob (the sources list some twice).
    const SAME: f64 = 10.0;
    /// A cell is a camp for a character of a level when it holds this many mobs of that level,
    /// give or take one.
    const CAMP_MOBS: usize = 8;

    /// Normal mobs (not elite or rare, no gossip, vendor or quest flag) that are not of a faction:
    /// hostile to both, or neutral beasts. None in the capitals (the game's flag 0x100): what the
    /// sources know nothing of there is a passer-by (Theramore Emissary in Stormwind), a pet
    /// (the warlock trainers' demons) or a dungeon's mob put at its door (the Stockade's).
    /// Each is tagged with the `quests` whose objectives are done on it.
    pub(super) fn load(conn: &Connection, world: &World, quests: &[Quest]) -> Result<Self> {
        let (tags, by_tag) = Self::quest_tags(quests);
        // Databases built before the heights have none.
        let heights: bool = conn.query_row(
            "SELECT count(*) > 0 FROM pragma_table_info('m_spawn') WHERE name = 'z'",
            [],
            |r| r.get(0),
        )?;
        let rows = crate::memo::rows(conn, "farm mobs", 0, || {
            let mut stmt = conn.prepare(&format!(
                "SELECT s.zone_id, s.x, s.y, coalesce(n.min_level, 1), coalesce(n.max_level, n.min_level, 1), n.id, {}
                 FROM m_spawn s JOIN m_npc n ON n.id = s.entity_id
                 WHERE s.entity_type = 'npc' AND s.x >= 0 AND coalesce(n.rank, 0) = 0
                   AND coalesce(n.npc_flags, 0) = 0 AND coalesce(n.friendly_to, '') IN ('', 'AH')
                   AND s.zone_id NOT IN (SELECT ID FROM client_areatable WHERE (Flags_0 & 256) != 0)",
                if heights { "s.z" } else { "NULL" }
            ))?;
            let rows = stmt
                .query_map([], |r| {
                    Ok(MobRow {
                        zone: r.get::<_, i64>(0)?,
                        x: r.get::<_, f64>(1)?,
                        y: r.get::<_, f64>(2)?,
                        min_level: r.get::<_, i64>(3)?,
                        max_level: r.get::<_, i64>(4)?,
                        npc: r.get::<_, i64>(5)?,
                        z: r.get::<_, Option<f64>>(6)?,
                    })
                })?
                .collect::<rusqlite::Result<Vec<_>>>()?;
            Ok(rows)
        })?;
        Ok(Self::from_rows(world, rows.to_vec(), &tags, by_tag))
    }

    /// The farm mobs of these spawns (zone, percent coordinates, levels, NPC), as the database's
    /// query gives them: each tagged with `tags` (NPC ID to tag), the same one listed twice kept once.
    pub(super) fn from_rows(
        world: &World,
        rows: Vec<MobRow>,
        tags: &FxHashMap<i64, u32>,
        by_tag: Vec<Box<[u32]>>,
    ) -> Self {
        let mut cells = Cells::default();
        // The zone and height of each spawn, in the order of its cell's (for the camps).
        let mut zones = CellPlaces::default();
        // Each spawn kept: its cell and its place there.
        let mut seen: FxHashMap<(i64, i64, i64, u8), (CellKey, usize)> = FxHashMap::default();
        for MobRow {
            zone,
            x,
            y,
            min_level: min,
            max_level: max,
            npc,
            z,
        } in rows
        {
            let Some(p) = world.to_world(zone, x, y) else { continue };
            let level = i64::midpoint(min, max).clamp(1, 255) as u8;
            let tag = tags.get(&npc).copied().unwrap_or(0);
            let same = (
                p.continent,
                (p.x / Self::SAME).floor() as i64,
                (p.y / Self::SAME).floor() as i64,
                level,
            );
            let cell = Self::cell(&p);
            match seen.entry(same) {
                std::collections::hash_map::Entry::Vacant(e) => {
                    let spawns = cells.entry(cell).or_default();
                    e.insert((cell, spawns.len()));
                    spawns.push(Spawn {
                        x: p.x as f32,
                        y: p.y as f32,
                        level,
                        tag,
                    });
                    zones.entry(cell).or_default().push((p.zone, crate::world::height(z)));
                }
                // The same mob listed twice (two sources' IDs): a quest's mob if either is.
                std::collections::hash_map::Entry::Occupied(e) => {
                    let (cell, i) = *e.get();
                    let kept = &mut cells.get_mut(&cell).expect("kept spawn")[i];
                    if kept.tag == 0 {
                        kept.tag = tag;
                    }
                }
            }
        }
        let camps = Self::find_camps(&cells, &zones);
        Self {
            cells,
            camps,
            quests: by_tag,
        }
    }

    /// A tag for each mob (NPC ID) some quest's kill or loot objectives are done on, at any of
    /// their places, and the quests of each tag (tag 1 first).
    pub(super) fn quest_tags(quests: &[Quest]) -> (FxHashMap<i64, u32>, Vec<Box<[u32]>>) {
        let mut by_npc: FxHashMap<i64, Vec<u32>> = FxHashMap::default();
        for (i, q) in quests.iter().enumerate() {
            for o in q.objectives.iter().flat_map(super::types::Objective::places) {
                for &(npc, _) in &o.mobs {
                    let list = by_npc.entry(npc).or_default();
                    if list.last() != Some(&(i as u32)) {
                        list.push(i as u32);
                    }
                }
            }
        }
        let mut npcs: Vec<i64> = by_npc.keys().copied().collect();
        npcs.sort_unstable();
        let tags = npcs.iter().enumerate().map(|(t, &npc)| (npc, t as u32 + 1)).collect();
        let by_tag = npcs.iter().map(|npc| by_npc[npc].clone().into_boxed_slice()).collect();
        (tags, by_tag)
    }

    /// Mobs at the given places (place, level, NPC ID), tagged with the `quests` whose
    /// objectives are done on them, for tests.
    #[cfg(test)]
    pub(crate) fn from_spawns(spawns: &[(Pos, u8, i64)], quests: &[Quest]) -> Self {
        let (tags, by_tag) = Self::quest_tags(quests);
        let mut farm = Self {
            quests: by_tag,
            ..Self::default()
        };
        for &(p, level, npc) in spawns {
            farm.cells.entry(Self::cell(&p)).or_default().push(Spawn {
                x: p.x as f32,
                y: p.y as f32,
                level,
                tag: tags.get(&npc).copied().unwrap_or(0),
            });
        }
        farm
    }

    /// The quests whose kill or loot objectives are done on the mobs of `tag` (`Met::tag`).
    pub fn quests(&self, tag: u32) -> &[u32] {
        tag.checked_sub(1)
            .and_then(|t| self.quests.get(t as usize))
            .map_or(&[], |q| q)
    }

    /// The camps of each level: the cells holding `CAMP_MOBS` mobs of the level give or take
    /// one, each at the spawn of those mobs nearest to their middle (a place a mob stands on),
    /// in the order of the cells.
    fn find_camps(cells: &Cells, zones: &CellPlaces) -> FxHashMap<(i64, u8), Vec<Pos>> {
        let mut keys: Vec<_> = cells.keys().copied().collect();
        keys.sort_unstable();
        let mut camps: FxHashMap<(i64, u8), Vec<Pos>> = FxHashMap::default();
        for key in keys {
            let spawns = &cells[&key];
            let (lo, hi) = spawns
                .iter()
                .fold((u8::MAX, 0), |(lo, hi), m| (lo.min(m.level), hi.max(m.level)));
            for level in lo.saturating_sub(1).max(1)..=hi.saturating_add(1) {
                let near = |l: u8| l.abs_diff(level) <= 1;
                let mobs: Vec<usize> = (0..spawns.len()).filter(|&i| near(spawns[i].level)).collect();
                if mobs.len() < Self::CAMP_MOBS {
                    continue;
                }
                let n = mobs.len() as f64;
                let (mx, my) = mobs.iter().fold((0.0, 0.0), |(x, y), &i| {
                    (x + f64::from(spawns[i].x) / n, y + f64::from(spawns[i].y) / n)
                });
                let middle = |i: usize| (f64::from(spawns[i].x) - mx).hypot(f64::from(spawns[i].y) - my);
                let at = mobs.iter().copied().min_by(|&a, &b| middle(a).total_cmp(&middle(b)));
                if let Some(i) = at {
                    camps.entry((key.0, level)).or_default().push(Pos {
                        continent: key.0,
                        x: f64::from(spawns[i].x),
                        y: f64::from(spawns[i].y),
                        zone: zones[&key][i].0,
                        z: zones[&key][i].1,
                    });
                }
            }
        }
        camps
    }

    /// The camps where a character of `level` grinds on `continent`: places with `CAMP_MOBS`
    /// mobs of its level give or take one within a cell, none in the capitals.
    pub fn camps(&self, continent: i64, level: i64) -> &[Pos] {
        u8::try_from(level)
            .ok()
            .and_then(|l| self.camps.get(&(continent, l)))
            .map_or(&[], Vec::as_slice)
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
                    .filter(|m| {
                        levels.contains(&i64::from(m.level))
                            && (f64::from(m.x) - p.x).hypot(f64::from(m.y) - p.y) <= radius
                    })
                    .count();
            }
        }
        n
    }

    /// Mobs within `corridor` of the walk through `path` (its points in order, on the same
    /// continent), each once, by level then tag.
    pub fn along(&self, path: &[Pos], corridor: f64) -> Vec<Met> {
        let mut counts: FxHashMap<(u8, u32), u16> = FxHashMap::default();
        let Some(first) = path.first() else {
            return Vec::new();
        };
        let legs: Vec<(Pos, Pos)> = match path {
            [p] => vec![(*p, *p)],
            _ => path.windows(2).map(|w| (w[0], w[1])).collect(),
        };
        let near = |p: &Pos, reach: f64| legs.iter().any(|(a, b)| crate::plan::segment_dist(p, a, b) <= reach);
        // The cells of the box of each leg grown by the corridor, once each, in order.
        let mut cells: Vec<(i64, i64)> = Vec::new();
        for (a, b) in &legs {
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
            cells.extend((lo.1..=hi.1).flat_map(|cx| (lo.2..=hi.2).map(move |cy| (cx, cy))));
        }
        cells.sort_unstable();
        cells.dedup();
        for (cx, cy) in cells {
            // Cells farther than the corridor from the walk hold nothing to count.
            let center = Pos {
                x: (cx as f64 + 0.5) * Self::CELL,
                y: (cy as f64 + 0.5) * Self::CELL,
                ..*first
            };
            if !near(&center, corridor + Self::CELL * 0.75) {
                continue;
            }
            for m in self.cells.get(&(first.continent, cx, cy)).into_iter().flatten() {
                let p = Pos {
                    x: f64::from(m.x),
                    y: f64::from(m.y),
                    ..*first
                };
                if near(&p, corridor) {
                    let n = counts.entry((m.level, m.tag)).or_default();
                    *n = n.saturating_add(1);
                }
            }
        }
        let mut met: Vec<Met> = counts
            .into_iter()
            .map(|((level, tag), count)| Met { level, count, tag })
            .collect();
        met.sort_unstable_by_key(|m| (m.level, m.tag));
        met
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// What the farm query takes of the spawns: normal mobs of no side, not those of a capital,
    /// nor those with a role (a vendor, a quest giver), elites or friendly ones.
    #[test]
    fn farm_mobs_are_the_normal_hostile_mobs_outside_capitals() {
        let conn = Connection::open_in_memory().unwrap();
        conn.execute_batch(
            "CREATE TABLE m_spawn (zone_id INT, x REAL, y REAL, entity_type TEXT, entity_id INT);
             CREATE TABLE m_npc (id INT, min_level INT, max_level INT, rank INT, npc_flags INT, friendly_to TEXT);
             CREATE TABLE client_areatable (ID INT, Flags_0 INT);
             INSERT INTO client_areatable VALUES (1, 0), (2, 256);
             INSERT INTO m_npc VALUES
                 (10, 5, 5, 0, 0, NULL), (11, 5, 5, 0, 0, NULL), (12, 5, 5, 1, 0, NULL),
                 (13, 5, 5, 0, 128, NULL), (14, 5, 5, 0, 0, 'A'), (15, 5, 5, 0, 0, 'AH');
             INSERT INTO m_spawn VALUES
                 (1, 10, 10, 'npc', 10), (2, 10, 10, 'npc', 11), (1, 20, 10, 'npc', 12),
                 (1, 30, 10, 'npc', 13), (1, 40, 10, 'npc', 14), (1, 50, 10, 'npc', 15);",
        )
        .unwrap();
        let world = World::flat(&[(1, "Field", 1, 0.0, 0.0), (2, "Capital", 1, 1000.0, 0.0)], &[], 0.0);
        let farm = FarmMobs::load(&conn, &world, &[]).unwrap();
        let at = |zone, x, y| world.to_world(zone, x, y).unwrap();
        let near = |zone, x, y| farm.around(&at(zone, x, y), 20.0, 1..=10);
        assert_eq!(near(1, 10.0, 10.0), 1, "a normal mob");
        assert_eq!(near(2, 10.0, 10.0), 0, "in a capital");
        assert_eq!(near(1, 20.0, 10.0), 0, "an elite");
        assert_eq!(near(1, 30.0, 10.0), 0, "a vendor");
        assert_eq!(near(1, 40.0, 10.0), 0, "friendly to the Alliance");
        assert_eq!(near(1, 50.0, 10.0), 1, "flagged AH (hostile to both): kept");
    }

    fn at(x: f64, y: f64) -> Pos {
        Pos {
            continent: 1,
            x,
            y,
            zone: 0,
            z: None,
        }
    }

    /// A walk around a corner meets the mobs along its two legs, each once (the one at the
    /// corner too), not those on the straight line it does not take.
    #[test]
    fn mobs_met_along_the_walk_taken() {
        let farm = FarmMobs::from_spawns(
            &[
                (at(0.0, 500.0), 10, 1),
                (at(250.0, 250.0), 11, 1),
                (at(500.0, 500.0), 12, 1),
                (at(500.0, 250.0), 13, 1),
            ],
            &[],
        );
        let met = |level, count| Met { level, count, tag: 0 };
        let (a, corner, b) = (at(0.0, 0.0), at(0.0, 500.0), at(500.0, 500.0));
        assert_eq!(farm.along(&[a, corner, b], 25.0), vec![met(10, 1), met(12, 1)]);
        assert_eq!(farm.along(&[a, b], 25.0), vec![met(11, 1), met(12, 1)]);
    }
}
