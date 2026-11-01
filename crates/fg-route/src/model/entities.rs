//! NPCs, objects and items of the database with their spawn points, and the camps of elites.

use super::types::EntityKind;
use crate::world::{Pos, World};
use anyhow::Result;
use rusqlite::Connection;
use std::collections::{HashMap, HashSet};
use std::sync::Arc;

/// Each entity's spawns: zone, map percent coordinates and height when known (`Pos::z`).
/// The points of a path a patrolling NPC walks: zone and percent coordinates.
type PathPoints = Vec<(i64, f64, f64)>;

pub(super) type Spawns = HashMap<(EntityKind, i64), Vec<(i64, f64, f64, Option<i16>)>>;

pub(super) struct Entities {
    pub(super) npc: Arc<HashMap<i64, (String, i64, i64, i64)>>, // name, min level, max level, rank
    pub(super) object: Arc<HashMap<i64, String>>,
    pub(super) item: Arc<HashMap<i64, String>>,
    pub(super) spawns: Spawns,
    /// NPCs that only talk to the other faction.
    pub(super) hostile: Arc<HashSet<i64>>,
    /// NPCs friendly to the character: objectives on them are talks, not kills.
    pub(super) friendly: Arc<HashSet<i64>>,
    /// NPCs of the character's own side nobody talks to (no gossip, vendor...): Sentinel Shaya,
    /// the Lazy Peons. Quests heal them, wake them, use an item on them; seldom fight them.
    pub(super) own_side: Arc<HashSet<i64>>,
    /// Seconds NPCs and objects take to come back once killed or used, where known.
    pub(super) respawn: Arc<HashMap<(EntityKind, i64), f64>>,
    /// The paths patrolling NPCs walk: zone and percent coordinates of each point, in order.
    pub(super) paths: Arc<HashMap<i64, PathPoints>>,
    /// Items vendors sell in limited supply, by (vendor, item): how many at most, and the
    /// seconds to get one more back.
    pub(super) vendor_stock: Arc<HashMap<(i64, i64), (u32, f64)>>,
    /// Auctioneers the character can buy from (its side's, the neutral ones).
    pub(super) auctioneers: Arc<Vec<i64>>,
}

impl Entities {
    /// What the database says of the entities, read once for a database and a faction.
    pub(super) fn load(conn: &Connection, faction: crate::faction::Faction) -> Result<Self> {
        let read = crate::memo::rows(conn, "entities", crate::memo::setting(faction.letter()), || {
            Self::read(conn, faction)
        })?;
        Ok(Self {
            npc: Arc::clone(&read.npc),
            object: Arc::clone(&read.object),
            item: Arc::clone(&read.item),
            spawns: read.spawns.clone(),
            hostile: Arc::clone(&read.hostile),
            friendly: Arc::clone(&read.friendly),
            own_side: Arc::clone(&read.own_side),
            respawn: Arc::clone(&read.respawn),
            paths: Arc::clone(&read.paths),
            vendor_stock: Arc::clone(&read.vendor_stock),
            auctioneers: Arc::clone(&read.auctioneers),
        })
    }

    fn read(conn: &Connection, faction: crate::faction::Faction) -> Result<Self> {
        let letter = faction.letter();
        let mut hostile = HashSet::new();
        let mut stmt = conn.prepare(
            "SELECT id FROM m_npc WHERE friendly_to IS NOT NULL AND friendly_to != '' AND instr(friendly_to, ?1) = 0",
        )?;
        for row in stmt.query_map([letter], |r| r.get::<_, i64>(0))? {
            hostile.insert(row?);
        }
        let mut friendly = HashSet::new();
        // friendly_to is also set on many monsters: only NPCs you can interact with (gossip, quest
        // giver, vendor...) are talked to.
        let mut stmt = conn.prepare(
            "SELECT id FROM m_npc WHERE instr(coalesce(friendly_to, ''), ?1) > 0 AND coalesce(npc_flags, 0) != 0",
        )?;
        for row in stmt.query_map([letter], |r| r.get::<_, i64>(0))? {
            friendly.insert(row?);
        }
        // Auctioneers (flag 0x1000) the character can deal with: its side's and the neutral ones.
        let mut auctioneers = Vec::new();
        let mut stmt = conn.prepare(
            "SELECT id FROM m_npc WHERE coalesce(npc_flags, 0) & 4096 != 0 AND instr(coalesce(friendly_to, ''), ?1) > 0 ORDER BY id",
        )?;
        for row in stmt.query_map([letter], |r| r.get::<_, i64>(0))? {
            auctioneers.push(row?);
        }
        let mut own_side = HashSet::new();
        let mut stmt = conn.prepare("SELECT id FROM m_npc WHERE friendly_to = ?1 AND coalesce(npc_flags, 0) = 0")?;
        for row in stmt.query_map([letter], |r| r.get::<_, i64>(0))? {
            own_side.insert(row?);
        }
        let mut npc = HashMap::new();
        let mut stmt = conn.prepare(
            "SELECT id, coalesce(name, ''), coalesce(min_level, 1), coalesce(max_level, min_level, 1), coalesce(rank, 0) FROM m_npc",
        )?;
        for row in stmt.query_map([], |r| Ok((r.get(0)?, (r.get(1)?, r.get(2)?, r.get(3)?, r.get(4)?))))? {
            let (id, v) = row?;
            npc.insert(id, v);
        }
        let mut object = HashMap::new();
        let mut stmt = conn.prepare("SELECT id, coalesce(name, '') FROM m_object")?;
        for row in stmt.query_map([], |r| Ok((r.get(0)?, r.get(1)?)))? {
            let (id, v) = row?;
            object.insert(id, v);
        }
        let mut item = HashMap::new();
        let mut stmt = conn.prepare("SELECT id, coalesce(name, '') FROM m_item")?;
        for row in stmt.query_map([], |r| Ok((r.get(0)?, r.get(1)?)))? {
            let (id, v) = row?;
            item.insert(id, v);
        }
        let has = |table: &str| -> Result<bool> {
            Ok(
                conn.query_row("SELECT count(*) > 0 FROM sqlite_schema WHERE name = ?1", [table], |r| {
                    r.get(0)
                })?,
            )
        };
        // Databases built before the heights have none.
        let heights: bool = conn.query_row(
            "SELECT count(*) > 0 FROM pragma_table_info('m_spawn') WHERE name = 'z'",
            [],
            |r| r.get(0),
        )?;
        let mut spawns: Spawns = HashMap::new();
        let mut stmt = conn.prepare(&format!(
            "SELECT entity_type, entity_id, zone_id, x, y, {} FROM m_spawn WHERE x >= 0",
            if heights { "z" } else { "NULL" }
        ))?;
        for row in stmt.query_map([], |r| {
            Ok((
                r.get::<_, String>(0)?,
                r.get::<_, i64>(1)?,
                r.get(2)?,
                r.get(3)?,
                r.get(4)?,
                r.get::<_, Option<f64>>(5)?,
            ))
        })? {
            let (t, id, zone, x, y, z) = row?;
            if let Some(kind) = EntityKind::parse(&t) {
                spawns
                    .entry((kind, id))
                    .or_default()
                    .push((zone, x, y, crate::world::height(z)));
            }
        }
        let mut respawn = HashMap::new();
        if has("m_respawn")? {
            let mut stmt = conn.prepare("SELECT entity_type, entity_id, seconds FROM m_respawn")?;
            for row in stmt.query_map([], |r| {
                Ok((r.get::<_, String>(0)?, r.get::<_, i64>(1)?, r.get::<_, f64>(2)?))
            })? {
                let (t, id, seconds) = row?;
                if let Some(kind) = EntityKind::parse(&t) {
                    respawn.insert((kind, id), seconds);
                }
            }
        }
        let mut paths: HashMap<i64, Vec<(i64, f64, f64)>> = HashMap::new();
        if has("m_npc_path")? {
            let mut stmt = conn.prepare("SELECT npc_id, zone_id, x, y FROM m_npc_path ORDER BY npc_id, point")?;
            for row in stmt.query_map([], |r| Ok((r.get::<_, i64>(0)?, (r.get(1)?, r.get(2)?, r.get(3)?))))? {
                let (id, point) = row?;
                paths.entry(id).or_default().push(point);
            }
        }
        let mut vendor_stock = HashMap::new();
        if has("m_vendor_stock")? {
            let mut stmt = conn.prepare("SELECT npc_id, item_id, max_count, restock FROM m_vendor_stock")?;
            for row in stmt.query_map([], |r| {
                Ok((
                    (r.get::<_, i64>(0)?, r.get::<_, i64>(1)?),
                    (r.get::<_, u32>(2)?, r.get::<_, f64>(3)?),
                ))
            })? {
                let (key, stock) = row?;
                vendor_stock.insert(key, stock);
            }
        }
        Ok(Self {
            npc: Arc::new(npc),
            object: Arc::new(object),
            item: Arc::new(item),
            spawns,
            hostile: Arc::new(hostile),
            friendly: Arc::new(friendly),
            own_side: Arc::new(own_side),
            respawn: Arc::new(respawn),
            paths: Arc::new(paths),
            vendor_stock: Arc::new(vendor_stock),
            auctioneers: Arc::new(auctioneers),
        })
    }

    pub(super) fn name(&self, kind: EntityKind, id: i64) -> String {
        match kind {
            EntityKind::Npc => self.npc.get(&id).map(|n| n.0.clone()),
            EntityKind::Object => self.object.get(&id).cloned(),
            EntityKind::Item => self.item.get(&id).cloned(),
            EntityKind::Area | EntityKind::Taxi | EntityKind::Dungeon => None,
        }
        .unwrap_or_else(|| format!("{} {id}", kind.as_str()))
    }

    pub(super) fn npc_level(&self, id: i64) -> i64 {
        self.npc.get(&id).map_or(1, |n| i64::midpoint(n.1, n.2))
    }

    pub(super) fn is_elite(&self, id: i64) -> bool {
        self.npc.get(&id).is_some_and(|n| matches!(n.3, 1..=3))
    }

    /// The path a patrolling NPC walks, in world positions (none: it stands at its spawns).
    pub(super) fn path(&self, world: &World, id: i64) -> Vec<Pos> {
        (self.paths.get(&id).into_iter().flatten())
            .filter_map(|&(z, x, y)| world.to_world(z, x, y))
            .collect()
    }

    /// World positions of an entity's spawns.
    pub(super) fn points(&self, world: &World, kind: EntityKind, id: i64) -> Vec<(Pos, i64)> {
        self.spawns
            .get(&(kind, id))
            .into_iter()
            .flatten()
            .filter_map(|&(zone, x, y, z)| world.to_world(zone, x, y).map(|p| (Pos { z, ..p }, zone)))
            .collect()
    }

    /// The spawns of unknown height take the height of their entity's nearest spawn of known
    /// height within `BORROWED_HEIGHT` yards (`borrow_heights`).
    pub(super) fn borrow_heights(&mut self, world: &World) {
        for points in self.spawns.values_mut() {
            if points.iter().all(|p| p.3.is_some()) || points.iter().all(|p| p.3.is_none()) {
                continue;
            }
            let at: Vec<Option<Pos>> = (points.iter())
                .map(|&(zone, x, y, _)| world.to_world(zone, x, y))
                .collect();
            let mut heights: Vec<Option<i16>> = points.iter().map(|p| p.3).collect();
            borrow_heights(&at, &mut heights);
            for (p, z) in points.iter_mut().zip(heights) {
                p.3 = z;
            }
        }
    }
}

/// Spawns of an entity this close stand on the same floor (yards): a camp, a pack.
const BORROWED_HEIGHT: f64 = 50.0;

/// The heights of an entity's spawns (`at`, None when not placed) where unknown: that of its
/// nearest spawn of known height within `BORROWED_HEIGHT` yards, a mob of a mine standing in it
/// as its pack does.
fn borrow_heights(at: &[Option<Pos>], heights: &mut [Option<i16>]) {
    let known: Vec<(Pos, i16)> = (at.iter().zip(heights.iter()))
        .filter_map(|(p, z)| Some(((*p)?, (*z)?)))
        .collect();
    for (p, z) in at.iter().zip(heights.iter_mut()) {
        let Some(p) = p.filter(|_| z.is_none()) else { continue };
        *z = (known.iter())
            .map(|(q, h)| (q.dist(&p), *h))
            .filter(|(d, _)| *d <= BORROWED_HEIGHT)
            .min_by(|a, b| a.0.total_cmp(&b.0))
            .map(|(_, h)| h);
    }
}

/// Elite spawns by cell, to tell a lone elite from a camp of elites.
pub(super) struct EliteSpawns {
    pub(super) cells: HashMap<(i64, i64, i64), Vec<(i64, Pos)>>,
    pub(super) points: HashMap<i64, Vec<Pos>>,
}

impl EliteSpawns {
    /// Elites within this distance of a fight join it (aggro and social pulls, in yards).
    const RADIUS: f64 = 20.0;
    /// An NPC with this many spawn points or fewer is one mob with alternative spawns (Hogger).
    const POOLED: usize = 6;
    const MAX_PULL: f64 = 3.0;

    pub(super) fn new(entities: &Entities, world: &World) -> Self {
        let mut cells: HashMap<(i64, i64, i64), Vec<(i64, Pos)>> = HashMap::new();
        let mut points: HashMap<i64, Vec<Pos>> = HashMap::new();
        for (kind, id) in entities.spawns.keys() {
            if *kind != EntityKind::Npc || !entities.is_elite(*id) {
                continue;
            }
            let pts: Vec<Pos> = entities
                .points(world, EntityKind::Npc, *id)
                .into_iter()
                .map(|(p, _)| p)
                .collect();
            for p in &pts {
                cells.entry(Self::cell(p)).or_default().push((*id, *p));
            }
            points.insert(*id, pts);
        }
        Self { cells, points }
    }

    pub(super) fn cell(p: &Pos) -> (i64, i64, i64) {
        (
            p.continent,
            (p.x / Self::RADIUS).floor() as i64,
            (p.y / Self::RADIUS).floor() as i64,
        )
    }

    /// Elites expected per pull around the spawns of `ids`: the mob itself and the other elites
    /// close to it (not the alternative spawns of a single named mob), on average.
    pub(super) fn pull(&self, ids: impl Iterator<Item = i64>) -> f64 {
        let (mut sum, mut n) = (0.0, 0usize);
        for id in ids {
            for p in self.points.get(&id).into_iter().flatten() {
                let (c, x, y) = Self::cell(p);
                let mut around = 1usize;
                for dx in -1..=1 {
                    for dy in -1..=1 {
                        for (other, q) in self.cells.get(&(c, x + dx, y + dy)).into_iter().flatten() {
                            let pooled = self.points.get(other).is_some_and(|v| v.len() <= Self::POOLED);
                            if q != p && !(*other == id && pooled) && q.dist(p) <= Self::RADIUS {
                                around += 1;
                            }
                        }
                    }
                }
                sum += around as f64;
                n += 1;
            }
        }
        // Beyond a few elites at a time, it is group content anyway (and spawns on several floors
        // of a keep look close on the map).
        if n == 0 {
            1.0
        } else {
            (sum / n as f64).min(Self::MAX_PULL)
        }
    }
}

/// The middle of the heights known among `heights` (None when none is): the floor most of a
/// group's spawns stand on.
pub(super) fn median_height(heights: impl Iterator<Item = Option<i16>>) -> Option<i16> {
    let mut known: Vec<i16> = heights.flatten().collect();
    if known.is_empty() {
        return None;
    }
    let k = known.len() / 2;
    Some(*known.select_nth_unstable(k).1)
}

/// Densest group of points, favoring those close to `near`. Returns its center and zone.
pub(super) fn best_cluster(points: &[(Pos, i64)], near: Option<&Pos>) -> Option<(Pos, i64, usize)> {
    const RADIUS: f64 = 200.0;
    // Large sets: sample candidates to bound the quadratic cost.
    let step = (points.len() / 400).max(1);
    let mut best: Option<(f64, Pos, i64, usize)> = None;
    for (p, zone) in points.iter().step_by(step) {
        let members: Vec<&Pos> = points.iter().map(|(q, _)| q).filter(|q| p.dist(q) <= RADIUS).collect();
        let center = Pos {
            continent: p.continent,
            zone: p.zone,
            x: members.iter().map(|q| q.x).sum::<f64>() / members.len() as f64,
            y: members.iter().map(|q| q.y).sum::<f64>() / members.len() as f64,
            z: median_height(members.iter().map(|q| q.z)),
        };
        let distance = near.map_or(0.0, |n| n.dist(&center));
        let score = (members.len() as f64).sqrt() / (1.0 + distance / 800.0);
        if best.as_ref().is_none_or(|b| score > b.0) {
            best = Some((score, center, *zone, members.len()));
        }
    }
    best.map(|(_, c, z, n)| (c, z, n))
}

/// Other groups of points than the one around `first`, where an objective needing `work` kills
/// or uses can be done too: up to `max` of them, apart from each other, of a few spawns at
/// least. None for one or two targets (a named mob: it stands at one of its spawns).
pub(super) fn alternatives(points: &[(Pos, i64)], first: &Pos, near: &Pos, work: f64, max: usize) -> Vec<(Pos, i64)> {
    /// Places this close to a chosen one are the same place (yards).
    const APART: f64 = 500.0;
    const MIN_SPAWNS: usize = 3;
    if work <= 2.0 || max == 0 {
        return Vec::new();
    }
    let mut left: Vec<(Pos, i64)> = points.iter().filter(|(p, _)| p.dist(first) > APART).copied().collect();
    let mut out = Vec::new();
    // A few stray spawns are passed over, a bounded number of times.
    for _ in 0..8 {
        let Some((pos, zone, n)) = best_cluster(&left, Some(near)) else {
            break;
        };
        left.retain(|(p, _)| p.dist(&pos) > APART);
        if n >= MIN_SPAWNS {
            out.push((pos, zone));
            if out.len() == max {
                break;
            }
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    fn at(x: f64) -> Pos {
        Pos {
            continent: 0,
            x,
            y: 0.0,
            zone: 12,
            z: None,
        }
    }

    /// A spawn of unknown height stands where its nearest neighbour of known height does, when
    /// close: in the mine with its pack, not on the hill 30 yards away; alone far off, unknown.
    #[test]
    fn spawns_borrow_the_height_of_their_pack() {
        let points = [
            Some(at(0.0)),
            Some(at(20.0)),
            Some(at(50.0)),
            Some(at(65.0)),
            Some(at(300.0)),
            None,
        ];
        let mut heights = [Some(10), None, Some(45), None, None, None];
        borrow_heights(&points, &mut heights);
        assert_eq!(heights, [Some(10), Some(10), Some(45), Some(45), None, None]);
    }

    /// The height of a group is that of most of its spawns.
    #[test]
    fn a_group_stands_at_the_height_of_most_of_its_spawns() {
        assert_eq!(
            median_height([Some(10), None, Some(12), Some(45)].into_iter()),
            Some(12)
        );
        assert_eq!(median_height([Some(10), Some(11), Some(45)].into_iter()), Some(11));
        assert_eq!(median_height([None, None].into_iter()), None);
    }
}
