//! NPCs, objects and items of the database with their spawn points, and the camps of elites.

use super::types::EntityKind;
use crate::world::{Pos, World};
use anyhow::Result;
use rusqlite::Connection;
use std::collections::{HashMap, HashSet};

pub(super) type Spawns = HashMap<(EntityKind, i64), Vec<(i64, f64, f64)>>;

pub(super) struct Entities {
    pub(super) npc: HashMap<i64, (String, i64, i64, i64)>, // name, min level, max level, rank
    pub(super) object: HashMap<i64, String>,
    pub(super) item: HashMap<i64, String>,
    pub(super) spawns: Spawns,
    /// NPCs that only talk to the other faction.
    pub(super) hostile: HashSet<i64>,
    /// NPCs friendly to the character: objectives on them are talks, not kills.
    pub(super) friendly: HashSet<i64>,
}

impl Entities {
    pub(super) fn load(conn: &Connection, faction: crate::faction::Faction) -> Result<Self> {
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
        let mut spawns: Spawns = HashMap::new();
        let mut stmt = conn.prepare("SELECT entity_type, entity_id, zone_id, x, y FROM m_spawn WHERE x >= 0")?;
        for row in stmt.query_map([], |r| {
            Ok((
                r.get::<_, String>(0)?,
                r.get::<_, i64>(1)?,
                r.get(2)?,
                r.get(3)?,
                r.get(4)?,
            ))
        })? {
            let (t, id, z, x, y) = row?;
            if let Some(kind) = EntityKind::parse(&t) {
                spawns.entry((kind, id)).or_default().push((z, x, y));
            }
        }
        Ok(Self {
            npc,
            object,
            item,
            spawns,
            hostile,
            friendly,
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

    /// World positions of an entity's spawns.
    pub(super) fn points(&self, world: &World, kind: EntityKind, id: i64) -> Vec<(Pos, i64)> {
        self.spawns
            .get(&(kind, id))
            .into_iter()
            .flatten()
            .filter_map(|&(z, x, y)| world.to_world(z, x, y).map(|p| (p, z)))
            .collect()
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
        };
        let distance = near.map_or(0.0, |n| n.dist(&center));
        let score = (members.len() as f64).sqrt() / (1.0 + distance / 800.0);
        if best.as_ref().is_none_or(|b| score > b.0) {
            best = Some((score, center, *zone, members.len()));
        }
    }
    best.map(|(_, c, z, n)| (c, z, n))
}
