//! The NPCs a route visits besides quests: class trainers, innkeepers; and the class spells
//! that travel: teleports to the capitals, a second hearthstone.

use super::entities::Entities;
use super::types::{EntityKind, Loc, ProfessionTrainer, Profile, Teleport};
use crate::world::World;
use anyhow::Result;
use rusqlite::Connection;

pub(super) struct Places {
    pub(super) trainers: Vec<Loc>,
    pub(super) profession_trainers: Vec<ProfessionTrainer>,
    pub(super) inns: Vec<Loc>,
    pub(super) vendors: Vec<Loc>,
    /// Where the hearthstone of a new character takes it: its starting place (not an inn, it
    /// cannot be bound there).
    pub(super) start_inn: usize,
    pub(super) teleports: Vec<Teleport>,
    pub(super) recall: Option<i64>,
}

impl Places {
    pub(super) fn load(
        conn: &Connection,
        world: &World,
        entities: &Entities,
        profile: &Profile,
        start: &Loc,
        edition: crate::xp::Edition,
        profession_trainers: bool,
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
        let trainers = finder.with_subname(&format!("{} Trainer", profile.class_name))?;
        let vendors = finder.vendors(edition, profile)?;
        let profession_trainers = if profession_trainers {
            finder.profession_trainers(edition)?
        } else {
            Vec::new()
        };
        let spells = class_spells(conn, profile.class_id)?;
        // Where a teleport lands: the class trainer of that city, else its inn (the character's
        // capitals only: the others have none friendly to it).
        let in_city = |locs: &[Loc], city: &str| {
            locs.iter()
                .find(|l| {
                    let zone = world.zone_name(l.zone);
                    zone == city || zone.strip_prefix(city).is_some_and(|rest| rest.starts_with(' '))
                })
                .cloned()
        };
        let teleports = spells
            .iter()
            .filter_map(|(name, level)| {
                let city = name.strip_prefix("Teleport: ")?;
                let to = in_city(&trainers, city).or_else(|| in_city(&inns[..start_inn], city))?;
                Some(Teleport {
                    name: name.clone(),
                    level: *level,
                    ability: 0,
                    to,
                })
            })
            .collect();
        let recall = spells
            .iter()
            .find(|(name, _)| name == "Astral Recall")
            .map(|(_, level)| *level);
        Ok(Self {
            trainers,
            profession_trainers,
            inns,
            vendors,
            start_inn,
            teleports,
            recall,
        })
    }
}

/// The teleports the character's quests teach: none when it cannot take any of those quests.
pub(super) fn quest_teleports(
    world: &World,
    profile: &Profile,
    quests: &[super::Quest],
    known: &std::collections::HashSet<String>,
    abilities: &crate::abilities::Abilities,
) -> Vec<Teleport> {
    profile
        .quest_teleports
        .iter()
        .filter(|d| d.class.eq_ignore_ascii_case(&profile.class_name))
        .filter_map(|def| {
            let taught: Vec<usize> = (quests.iter().enumerate())
                .filter(|(_, q)| q.name == def.quest)
                .map(|(i, _)| i)
                .collect();
            let pos = world.to_world(def.to.zone, def.to.x, def.to.y)?;
            (!taught.is_empty() || known.contains(&def.quest)).then(|| Teleport {
                name: def.name.clone(),
                level: 0,
                ability: abilities
                    .bit(def.spell)
                    .expect("quest teleports are registered as abilities"),
                to: Loc {
                    pos,
                    zone: def.to.zone,
                    kind: EntityKind::Area,
                    id: 0,
                    name: world.zone_name(def.to.zone).to_owned(),
                    inside: None,
                    floor: None,
                },
            })
        })
        .collect()
}

/// The class's spells that travel ("Teleport: Stormwind", "Astral Recall") and the level they
/// are learned at; none without the class spell data.
fn class_spells(conn: &Connection, class_id: i64) -> Result<Vec<(String, i64)>> {
    let known: i64 = conn.query_row(
        "SELECT count(*) FROM sqlite_master WHERE name = 'm_class_spell'",
        [],
        |r| r.get(0),
    )?;
    if known == 0 {
        return Ok(Vec::new());
    }
    let mut stmt = conn.prepare(
        "SELECT name, min(level) FROM m_class_spell
         WHERE class_id = ?1 AND (name LIKE 'Teleport: %' OR name = 'Astral Recall')
         GROUP BY name",
    )?;
    let rows = stmt.query_map([class_id], |r| Ok((r.get(0)?, r.get(1)?)))?;
    Ok(rows.collect::<rusqlite::Result<_>>()?)
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
            inside: None,
            floor: None,
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

    /// The profession trainers friendly to the character, from their title ("Journeyman Blacksmith",
    /// "Skinning Trainer") and the trainer flag; none without the flags column.
    fn profession_trainers(&self, edition: crate::xp::Edition) -> Result<Vec<ProfessionTrainer>> {
        let known: bool = self.conn.query_row(
            "SELECT count(*) > 0 FROM pragma_table_info('m_npc') WHERE name = 'npc_flags'",
            [],
            |r| r.get(0),
        )?;
        if !known {
            return Ok(Vec::new());
        }
        let mut stmt = self.conn.prepare(
            "SELECT id, subname, npc_flags FROM m_npc
             WHERE subname IS NOT NULL AND subname != '' AND npc_flags IS NOT NULL
               AND (friendly_to IS NULL OR instr(friendly_to, ?1) > 0)
             ORDER BY id",
        )?;
        let rows = stmt
            .query_map([self.faction], |r| {
                Ok((r.get::<_, i64>(0)?, r.get::<_, String>(1)?, r.get::<_, i64>(2)?))
            })?
            .collect::<rusqlite::Result<Vec<_>>>()?;
        Ok(rows
            .into_iter()
            .filter_map(|(id, title, flags)| {
                let line = crate::job::profession_of(&title, flags, edition)?;
                let def = crate::profession::PROFESSIONS.iter().find(|d| d.skill_id == line)?;
                Some(ProfessionTrainer {
                    line,
                    cap: rank_cap_of_title(&title, def.kind),
                    loc: self.place(id)?,
                })
            })
            .collect())
    }

    /// Friendly vendors, excluding known class-gated shops the whole group cannot open.
    fn vendors(&self, edition: crate::xp::Edition, profile: &Profile) -> Result<Vec<Loc>> {
        let known: bool = self.conn.query_row(
            "SELECT count(*) > 0 FROM pragma_table_info('m_npc') WHERE name = 'npc_flags'",
            [],
            |r| r.get(0),
        )?;
        if !known {
            return Ok(Vec::new());
        }
        // The vendor flag of the client the data was read from (1.12 for Classic and Forever).
        let flag = if edition == crate::xp::Edition::Tbc { 128 } else { 4 };
        let mut stmt = self.conn.prepare(
            "SELECT id, coalesce(subname, '') FROM m_npc
             WHERE (npc_flags & ?1) != 0 AND (friendly_to IS NULL OR instr(friendly_to, ?2) > 0)",
        )?;
        let rows: Vec<(i64, String)> = stmt
            .query_map(rusqlite::params![flag, self.faction], |r| Ok((r.get(0)?, r.get(1)?)))?
            .collect::<rusqlite::Result<_>>()?;
        // The demon trainers' shops (and Gorzeeki's, who has no title) open for warlocks only,
        // though the NPCs carry the vendor flag; seen in game on WoW Forever. TBC's data does not
        // show the same restriction.
        let can_use_warlock_shops =
            edition == crate::xp::Edition::Tbc || profile.class_ids().into_iter().all(|class| class == 9);
        Ok(rows
            .into_iter()
            .filter(|(id, title)| {
                can_use_warlock_shops || (*id != 14437 && !matches!(title.as_str(), "Demon Trainer" | "Demon Master"))
            })
            .filter_map(|(id, _)| self.place(id))
            .collect())
    }
}

/// The highest skill cap a profession trainer's title says it teaches ("Artisan" 300, "Expert"
/// 225, "Journeyman" 150, "Apprentice" 75). A plain title ("Skinning Trainer") says nothing of
/// the rank: crafting trainers of those teach the first rank; gathering and secondary ones the
/// first three, as far as the game data shows.
fn rank_cap_of_title(title: &str, kind: crate::profession::Kind) -> i64 {
    let says = |words: &[&str]| words.iter().any(|w| title.contains(w));
    // "Master ..." titles are the specializations (Master Goblin Engineer), not the rank.
    if says(&["Artisan"]) {
        300
    } else if says(&["Expert"]) {
        225
    } else if says(&["Journeyman"]) {
        150
    } else if says(&["Apprentice", "Junior"]) || kind == crate::profession::Kind::Crafting {
        75
    } else {
        225
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::profession::Kind;

    #[test]
    fn a_title_says_how_far_a_trainer_teaches() {
        let cap = |title, kind| rank_cap_of_title(title, kind);
        assert_eq!(cap("Artisan Tailor", Kind::Crafting), 300);
        assert_eq!(cap("Expert Leatherworker", Kind::Crafting), 225);
        assert_eq!(cap("Journeyman Blacksmith", Kind::Crafting), 150);
        assert_eq!(cap("Junior Skinning Trainer", Kind::Gathering), 75);
        // A plain title: the first rank of a crafting profession, three of the others.
        assert_eq!(cap("Alchemy Trainer", Kind::Crafting), 75);
        assert_eq!(cap("Skinning Trainer", Kind::Gathering), 225);
        // Specializations teach no rank.
        assert_eq!(cap("Master Goblin Engineer", Kind::Crafting), 75);
    }
}
