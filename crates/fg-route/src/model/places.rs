//! The NPCs a route visits besides quests: class trainers, innkeepers.

use super::entities::Entities;
use super::types::{EntityKind, Loc, Profile};
use crate::world::World;
use anyhow::Result;
use rusqlite::Connection;

pub(super) struct Places {
    pub(super) trainers: Vec<Loc>,
    pub(super) inns: Vec<Loc>,
    /// Where the hearthstone of a new character takes it: its starting place (not an inn, it
    /// cannot be bound there).
    pub(super) start_inn: usize,
}

impl Places {
    pub(super) fn load(
        conn: &Connection,
        world: &World,
        entities: &Entities,
        profile: &Profile,
        start: &Loc,
    ) -> Result<Self> {
        let finder = NpcFinder {
            conn,
            world,
            entities,
            faction: profile.faction.letter(),
        };
        let mut inns = finder.with_subname("Innkeeper")?;
        // A new character's hearthstone takes it back to its starting place (the Valley of
        // Trials, not the closest inn as the crow flies) until it is bound at an inn.
        inns.push(start.clone());
        let start_inn = inns.len() - 1;
        Ok(Self {
            trainers: finder.with_subname(&format!("{} Trainer", profile.class_name))?,
            inns,
            start_inn,
        })
    }
}

struct NpcFinder<'a> {
    conn: &'a Connection,
    world: &'a World,
    entities: &'a Entities,
    faction: &'static str,
}

impl NpcFinder<'_> {
    fn place(&self, id: i64) -> Option<Loc> {
        let (pos, zone) = *self.entities.points(self.world, EntityKind::Npc, id).first()?;
        Some(Loc {
            pos,
            zone,
            kind: EntityKind::Npc,
            id,
            name: self.entities.name(EntityKind::Npc, id),
        })
    }

    /// NPCs of a title ("Innkeeper", "Warrior Trainer") friendly to the character.
    fn with_subname(&self, subname: &str) -> Result<Vec<Loc>> {
        let mut stmt = self.conn.prepare(
            "SELECT id FROM m_npc WHERE subname = ?1 AND (friendly_to IS NULL OR instr(friendly_to, ?2) > 0)",
        )?;
        let ids: Vec<i64> = stmt
            .query_map(rusqlite::params![subname, self.faction], |r| r.get(0))?
            .collect::<rusqlite::Result<_>>()?;
        Ok(ids.into_iter().filter_map(|id| self.place(id)).collect())
    }
}
