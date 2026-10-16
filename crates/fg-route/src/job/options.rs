//! The choices the app offers: races, classes, zones, dungeons, professions.

use super::overrides::Overrides;
use crate::params::Params;
use anyhow::Result;
use rusqlite::Connection;
use serde::Serialize;
use std::collections::{BTreeMap, HashMap};

/// Choices offered by the app.
#[derive(Debug, Clone, Serialize)]
pub struct Options {
    pub races: Vec<RaceOption>,
    pub classes: Vec<NamedId>,
    pub zones: Vec<ZoneOption>,
    pub dungeons: Vec<DungeonOption>,
    /// Factions quests give reputation with, for the guides' reputation goals.
    pub factions: Vec<FactionOption>,
    pub class_quests: HashMap<String, Vec<String>>,
    pub params: Params,
    /// Level cap of the game version.
    pub max_level: i64,
}

#[derive(Debug, Clone, Serialize)]
pub struct RaceOption {
    pub key: String,
    pub id: i64,
    pub name: String,
    pub faction: crate::faction::Faction,
    /// Classes this race can play (class keys), from the game's CharBaseInfo.
    pub classes: Vec<String>,
}

#[derive(Debug, Clone, Serialize)]
pub struct NamedId {
    pub id: i64,
    pub key: String,
    pub name: String,
    /// Translated names by game locale (frFR...), when known.
    #[serde(skip_serializing_if = "BTreeMap::is_empty")]
    pub names: BTreeMap<String, String>,
}

/// A dungeon, with the levels of its quests (from the quest log's dungeon category).
#[derive(Debug, Clone, Serialize)]
pub struct DungeonOption {
    pub id: i64,
    pub key: String,
    pub name: String,
    /// Translated names by game locale (frFR...), when known.
    #[serde(skip_serializing_if = "BTreeMap::is_empty")]
    pub names: BTreeMap<String, String>,
    /// Levels of most of its quests (the lowest and highest few left out), when it has some.
    pub min_level: Option<i64>,
    pub max_level: Option<i64>,
}

/// A faction quests give reputation with.
#[derive(Debug, Clone, Serialize)]
pub struct FactionOption {
    pub id: i64,
    pub name: String,
    /// The side it belongs to (none: neutral, like the Argent Dawn).
    pub side: Option<crate::faction::Faction>,
    /// Quests giving reputation with it.
    pub quests: usize,
    /// Points each race starts with (ChrRaces ID -> points from the bottom of Neutral).
    pub starts: BTreeMap<i64, i64>,
}

#[derive(Debug, Clone, Serialize)]
pub struct ZoneOption {
    pub id: i64,
    pub name: String,
    /// Translated names by game locale (frFR...), when known.
    #[serde(skip_serializing_if = "BTreeMap::is_empty")]
    pub names: BTreeMap<String, String>,
    pub continent: i64,
}

pub fn options(conn: &Connection, overrides: &Overrides) -> Result<Options> {
    let race_names: HashMap<i64, String> = {
        let mut stmt = conn.prepare("SELECT ID, Name_lang FROM client_chrraces")?;
        stmt.query_map([], |r| Ok((r.get(0)?, r.get(1)?)))?
            .collect::<rusqlite::Result<_>>()?
    };
    let races: Vec<RaceOption> = overrides
        .races
        .iter()
        .map(|(key, r)| {
            Ok(RaceOption {
                key: key.clone(),
                id: r.id,
                name: race_names.get(&r.id).cloned().unwrap_or_else(|| key.clone()),
                faction: r.faction,
                classes: race_classes(conn, r.id)?,
            })
        })
        .collect::<Result<_>>()?;
    // Classes some race can play.
    let classes = {
        let mut stmt = conn.prepare("SELECT ID, Filename, Name_male_lang FROM client_chrclasses ORDER BY ID")?;
        stmt.query_map([], |r| {
            Ok(NamedId {
                id: r.get(0)?,
                key: r.get::<_, String>(1)?.to_lowercase(),
                name: r.get(2)?,
                names: BTreeMap::new(),
            })
        })?
        .collect::<rusqlite::Result<Vec<_>>>()?
        .into_iter()
        .filter(|c| races.iter().all(|r| r.classes.is_empty()) || races.iter().any(|r| r.classes.contains(&c.key)))
        .collect()
    };
    // Zone and dungeon names in the game's languages.
    let mut area_names: HashMap<i64, BTreeMap<String, String>> = HashMap::new();
    if let Ok(mut stmt) =
        conn.prepare("SELECT id, locale, name FROM l10n WHERE entity_type = 'zone' AND name IS NOT NULL")
    {
        let rows = stmt.query_map([], |r| {
            Ok((r.get::<_, i64>(0)?, r.get::<_, String>(1)?, r.get::<_, String>(2)?))
        })?;
        for row in rows {
            let (id, locale, name) = row?;
            area_names.entry(id).or_default().insert(locale, name);
        }
    }
    let names_of = |id: i64| area_names.get(&id).cloned().unwrap_or_default();
    let zones = {
        let mut stmt = conn.prepare(
            "SELECT DISTINCT a.AreaID, t.AreaName_lang, a.MapID FROM client_uimapassignment a
             JOIN client_areatable t ON t.ID = a.AreaID WHERE a.AreaID != 0 ORDER BY t.AreaName_lang",
        )?;
        stmt.query_map([], |r| {
            Ok(ZoneOption {
                id: r.get(0)?,
                name: r.get(1)?,
                names: names_of(r.get(0)?),
                continent: r.get(2)?,
            })
        })?
        .collect::<rusqlite::Result<_>>()?
    };
    // Dungeons with quests of this game version: something a quest needs lives inside (a mob or
    // object, or one dropping a quest item), or quests are filed under it (done by running it,
    // even when the sources place nothing inside: Forever's Ruins of Lordaeron, GitHub #18),
    // plus those declared in dungeons.toml.
    let dungeons = {
        let mut stmt = conn.prepare(
            "WITH needed(entity_type, entity_id) AS (
               SELECT CASE o.type WHEN 'object' THEN 'object' ELSE 'npc' END, o.target_id
               FROM m_quest_objective o JOIN m_quest q ON q.id = o.quest_id
               WHERE q.status = 'available' AND q.level <= ?1 AND o.type != 'item'
               UNION SELECT i.from_type, i.from_id
               FROM m_quest_objective o JOIN m_quest q ON q.id = o.quest_id
               JOIN m_item_source i ON i.item_id = o.target_id AND i.from_type IN ('npc', 'object')
               WHERE q.status = 'available' AND q.level <= ?1 AND o.type = 'item'),
             -- The areas inside each dungeon, then the spawns read once (not once per dungeon).
             inside(dungeon, zone) AS (
               SELECT area, area FROM dungeon
               UNION SELECT d.area, j.value FROM dungeon d, json_each(coalesce(d.alt_areas, '[]')) j),
             found(dungeon) AS (
               SELECT DISTINCT i.dungeon FROM m_spawn s JOIN inside i ON i.zone = s.zone_id
               WHERE (s.entity_type, s.entity_id) IN needed)
             SELECT d.area, d.name, coalesce(d.alt_areas, '[]') FROM dungeon d JOIN client_areatable a ON a.ID = d.area
             JOIN client_map m ON m.InstanceType = 1 AND (m.ID = a.ContinentID OR m.AreaTableID = d.area)
             WHERE d.area IN found OR d.area IN (SELECT i.dungeon FROM inside i JOIN m_quest q ON q.zone_or_sort = i.zone
                                                 WHERE q.status = 'available' AND q.level <= ?1)
             GROUP BY d.area",
        )?;
        let max_level = overrides.params.edition.rules().max_level + 2;
        // Quest levels by quest log category (a dungeon's quests are filed under it).
        let mut quest_levels: HashMap<i64, Vec<i64>> = HashMap::new();
        {
            let mut stmt = conn.prepare(
                "SELECT zone_or_sort, level FROM m_quest WHERE status = 'available' AND zone_or_sort > 0 AND level > 0",
            )?;
            for row in stmt.query_map([], |r| Ok((r.get::<_, i64>(0)?, r.get::<_, i64>(1)?)))? {
                let (area, level) = row?;
                quest_levels.entry(area).or_default().push(level);
            }
        }
        // Its quests may be filed under one of its other areas (Gnomeregan's under the zone's).
        let dungeon = |area: i64, name: String, alt_areas: &[i64]| {
            let mut levels: Vec<i64> = std::iter::once(&area)
                .chain(alt_areas)
                .flat_map(|a| quest_levels.get(a).into_iter().flatten().copied())
                .collect();
            levels.sort_unstable();
            let n = levels.len();
            let def = overrides.dungeons.iter().find(|d| d.area == area);
            DungeonOption {
                id: area,
                key: area.to_string(),
                name,
                names: names_of(area),
                min_level: def
                    .and_then(|d| d.min_level)
                    .or_else(|| (n > 0).then(|| levels[n * 15 / 100])),
                max_level: def
                    .and_then(|d| d.max_level)
                    .or_else(|| (n > 0).then(|| levels[(n * 85).div_ceil(100).max(1) - 1])),
            }
        };
        let mut list: Vec<DungeonOption> = stmt
            .query_map([max_level], |r| {
                Ok((r.get::<_, i64>(0)?, r.get::<_, String>(1)?, r.get::<_, String>(2)?))
            })?
            .map(|r| {
                r.map(|(area, name, alt)| {
                    dungeon(area, name, &serde_json::from_str::<Vec<i64>>(&alt).unwrap_or_default())
                })
            })
            .collect::<rusqlite::Result<_>>()?;
        for def in &overrides.dungeons {
            if let Some(name) = &def.name
                && !list.iter().any(|d| d.id == def.area)
            {
                list.push(dungeon(def.area, name.clone(), &[]));
            }
        }
        // By level (those without, last), then name.
        list.sort_by(|a, b| {
            (a.min_level.unwrap_or(i64::MAX), a.max_level, &a.name).cmp(&(
                b.min_level.unwrap_or(i64::MAX),
                b.max_level,
                &b.name,
            ))
        });
        list
    };
    let factions = factions(conn, &races)?;
    Ok(Options {
        races,
        classes,
        zones,
        dungeons,
        factions,
        class_quests: class_quests(conn, overrides)?,
        params: overrides.params.clone(),
        max_level: overrides.params.edition.rules().max_level,
    })
}

/// The factions available quests give reputation with, most quests first, with each race's
/// starting points (the race's entry of the Faction row, any class).
fn factions(conn: &Connection, races: &[RaceOption]) -> Result<Vec<FactionOption>> {
    let mut counts: HashMap<i64, usize> = HashMap::new();
    let mut stmt = conn.prepare(
        "SELECT reputation_reward FROM m_quest WHERE status = 'available' AND reputation_reward IS NOT NULL",
    )?;
    for row in stmt.query_map([], |r| r.get::<_, String>(0))? {
        let rewards: Vec<(i64, i64)> = serde_json::from_str(&row?).unwrap_or_default();
        for (faction, points) in rewards {
            if points > 0 {
                *counts.entry(faction).or_default() += 1;
            }
        }
    }
    let Ok(mut stmt) = conn.prepare("SELECT ID, Name_lang, ParentFactionID FROM client_faction") else {
        return Ok(Vec::new());
    };
    let rows: HashMap<i64, (String, i64)> = stmt
        .query_map([], |r| {
            Ok((
                r.get::<_, i64>(0)?,
                (
                    r.get::<_, Option<String>>(1)?.unwrap_or_default(),
                    r.get::<_, Option<i64>>(2)?.unwrap_or(0),
                ),
            ))
        })?
        .collect::<rusqlite::Result<_>>()?;
    let side = |mut id: i64| {
        for _ in 0..8 {
            match id {
                469 => return Some(crate::faction::Faction::Alliance),
                67 => return Some(crate::faction::Faction::Horde),
                _ => id = rows.get(&id).map_or(0, |r| r.1),
            }
        }
        None
    };
    let mut out = Vec::new();
    for (&id, &quests) in &counts {
        let Some((name, _)) = rows.get(&id).filter(|(name, _)| !name.is_empty()) else {
            continue;
        };
        let mut starts = BTreeMap::new();
        for race in races {
            if let Some(points) = super::race_base(conn, id, crate::model::race_bit(race.id), -1)? {
                starts.insert(race.id, points);
            }
        }
        out.push(FactionOption {
            id,
            name: name.clone(),
            side: side(id),
            quests,
            starts,
        });
    }
    out.sort_by(|a, b| b.quests.cmp(&a.quests).then_with(|| a.name.cmp(&b.name)));
    Ok(out)
}

/// Class quests teaching a spell from this level on are endgame (see `class_quests`).
const SPELL_QUESTS_BELOW: i64 = 50;

/// The class quests required by default, by class key: the configured ones
/// (overrides/class_quests.toml), then the class quests the data says teach a spell (a pet, a
/// stance, a totem, Redemption...), by level: a guide does not leave a class spell out. The app
/// lists them, the player takes out those not wanted. Those from level 50 (an epic mount,
/// Inferno, Ritual of Doom: raids and elite dungeons) are endgame, not leveling. Then the start
/// zone's letter to the class trainer (Encrypted Sigil): little XP, but it leads to the trainer.
pub fn class_quests(conn: &Connection, overrides: &Overrides) -> Result<HashMap<String, Vec<String>>> {
    let mut out = overrides.class_quests.clone();
    // Databases built before the column existed: the configured ones only.
    let known: bool = conn.query_row(
        "SELECT count(*) > 0 FROM pragma_table_info('m_quest') WHERE name = 'reward_spell'",
        [],
        |r| r.get(0),
    )?;
    let Ok(mut classes) = conn.prepare("SELECT ID, lower(Filename) FROM client_chrclasses ORDER BY ID") else {
        return Ok(out);
    };
    if !known {
        return Ok(out);
    }
    let classes: Vec<(i64, String)> = classes
        .query_map([], |r| Ok((r.get(0)?, r.get(1)?)))?
        .collect::<rusqlite::Result<_>>()?;
    let mut stmt = conn.prepare(
        "SELECT name FROM m_quest
         WHERE reward_spell IS NOT NULL AND status = 'available' AND (classes_mask = ?1 OR zone_or_sort = ?2)
         GROUP BY name HAVING min(coalesce(min_level, level, 1)) < ?3
         ORDER BY min(coalesce(min_level, level, 1)), name",
    )?;
    let has_subname: bool = conn.query_row(
        "SELECT count(*) > 0 FROM pragma_table_info('m_npc') WHERE name = 'subname'",
        [],
        |r| r.get(0),
    )?;
    let mut letters = conn.prepare(if has_subname {
        "SELECT DISTINCT q.name FROM m_quest q
         JOIN m_quest_relation r ON r.quest_id = q.id AND r.role = 'end' AND r.entity_type = 'npc'
         JOIN m_npc n ON n.id = r.entity_id
         WHERE q.status = 'available' AND q.classes_mask = ?1 AND coalesce(q.level, 1) BETWEEN 0 AND 2
           AND n.subname LIKE '% Trainer'
         ORDER BY q.name"
    } else {
        "SELECT name FROM m_quest WHERE 0 AND ?1"
    })?;
    for (id, key) in classes.into_iter().filter(|(id, _)| (1..=31).contains(id)) {
        let names: Vec<String> = stmt
            .query_map(
                rusqlite::params![1i64 << (id - 1), -crate::model::class_sort(id), SPELL_QUESTS_BELOW],
                |r| r.get(0),
            )?
            .collect::<rusqlite::Result<_>>()?;
        let letters: Vec<String> = letters
            .query_map([1i64 << (id - 1)], |r| r.get(0))?
            .collect::<rusqlite::Result<_>>()?;
        let list = out.entry(key).or_default();
        for name in names.into_iter().chain(letters) {
            if !list.contains(&name) {
                list.push(name);
            }
        }
    }
    Ok(out)
}

/// Classes a race can play (class keys: lowercase file names), from the game's CharBaseInfo.
pub fn race_classes(conn: &Connection, race_id: i64) -> Result<Vec<String>> {
    // Databases imported before CharBaseInfo (Classic Era comparisons) have no list: no restriction.
    let Ok(mut stmt) = conn.prepare(
        "SELECT lower(c.Filename) FROM client_charbaseinfo b JOIN client_chrclasses c ON c.ID = b.ClassID
         WHERE b.RaceID = ?1 ORDER BY c.ID",
    ) else {
        return Ok(Vec::new());
    };
    Ok(stmt
        .query_map([race_id], |r| r.get(0))?
        .collect::<rusqlite::Result<_>>()?)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The class quests teaching a spell join the configured ones, by level, once each name:
    /// filed under the class (Forever's) or given to it only; removed quests, other classes' and
    /// endgame ones do not.
    #[test]
    fn class_quests_teaching_a_spell_are_required_by_default() {
        let conn = Connection::open_in_memory().unwrap();
        conn.execute_batch(
            "CREATE TABLE client_chrclasses (ID, Filename);
             INSERT INTO client_chrclasses VALUES (2, 'PALADIN'), (3, 'HUNTER');
             CREATE TABLE m_quest (id, name, level, min_level, classes_mask, zone_or_sort, status, reward_spell);
             INSERT INTO m_quest VALUES
               (1, 'The Tome of Divinity', 12, 12, 2, -141, 'available', 7328),
               (2, 'The Tome of Nobility', 40, 40, 2, -141, 'available', 13819),
               (3, 'A Lesson in Divinity', 12, 12, NULL, -141, 'available', 7328),
               (4, 'A Lesson in Divinity', 12, 12, NULL, -141, 'available', NULL),
               (5, 'The Tome of Valor', 20, 20, 2, -141, 'removed', 5502),
               (6, 'Seeking the Kor Gem', 25, 25, 2, -141, 'available', NULL),
               (8, 'Judgment and Redemption', 60, 60, 2, -141, 'available', 23214),
               (7, 'Taming the Beast', 10, 10, 4, -261, 'available', 1515);",
        )
        .unwrap();
        let mut overrides = Overrides::default();
        overrides
            .class_quests
            .insert("paladin".into(), vec!["The Tome of Divinity".into()]);
        let quests = class_quests(&conn, &overrides).unwrap();
        assert_eq!(
            quests["paladin"],
            ["The Tome of Divinity", "A Lesson in Divinity", "The Tome of Nobility"]
        );
        assert_eq!(quests["hunter"], ["Taming the Beast"]);
    }

    /// The start zone's letter to the class trainer is required; a level 1 class quest turned in
    /// elsewhere, or a later one at the trainer, is not.
    #[test]
    fn letters_to_the_class_trainer_are_required() {
        let conn = Connection::open_in_memory().unwrap();
        conn.execute_batch(
            "CREATE TABLE client_chrclasses (ID, Filename);
             INSERT INTO client_chrclasses VALUES (4, 'ROGUE');
             CREATE TABLE m_quest (id, name, level, min_level, classes_mask, zone_or_sort, status, reward_spell);
             CREATE TABLE m_quest_relation (quest_id, role, entity_type, entity_id);
             CREATE TABLE m_npc (id, subname);
             INSERT INTO m_npc VALUES (1, 'Rogue Trainer'), (2, NULL);
             INSERT INTO m_quest VALUES (1, 'Encrypted Sigil', 1, 1, 8, NULL, 'available', NULL),
                                        (2, 'Plain Errand', 1, 1, 8, NULL, 'available', NULL),
                                        (3, 'The Deadliest Trap', 12, 12, 8, NULL, 'available', NULL);
             INSERT INTO m_quest_relation VALUES (1, 'end', 'npc', 1), (2, 'end', 'npc', 2), (3, 'end', 'npc', 1);",
        )
        .unwrap();
        let quests = class_quests(&conn, &Overrides::default()).unwrap();
        assert_eq!(quests["rogue"], ["Encrypted Sigil"]);
    }

    /// The factions available quests give reputation with, most quests first, with their side
    /// (up the parents to the Alliance or the Horde; none: neutral) and each race's start.
    #[test]
    fn factions_of_the_quests_with_their_side_and_starts() {
        let conn = Connection::open_in_memory().unwrap();
        let mut columns = vec!["ID".to_owned(), "Name_lang".to_owned(), "ParentFactionID".to_owned()];
        for k in 0..4 {
            columns.extend([
                format!("ReputationRaceMasks{k}_0"),
                format!("ReputationRaceMasks{k}_1"),
                format!("ReputationClassMask_{k}"),
                format!("ReputationBase_{k}"),
            ]);
        }
        conn.execute_batch(&format!(
            "CREATE TABLE client_faction ({});
             INSERT INTO client_faction VALUES
               (469, 'Alliance', 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0),
               (1118, 'Classic', 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0),
               (891, 'Alliance Forces', 469, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0),
               (69, 'Darnassus', 469, 8, 0, 0, 4000, 1309324210, -1440044374, 0, -42000, 0, 0, 0, 0, 0, 0, 0, 0),
               (729, 'Frostwolf Clan', 67, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0),
               (529, 'Argent Dawn', 1118, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0);
             CREATE TABLE m_quest (status, reputation_reward);
             INSERT INTO m_quest VALUES ('available', '[[69,100]]'), ('available', '[[69,250],[529,50]]'),
               ('removed', '[[729,500]]'), ('available', '[[891,-10]]'), ('available', NULL);",
            columns.join(", ")
        ))
        .unwrap();
        let race = |id, key: &str, faction| RaceOption {
            key: key.into(),
            id,
            name: key.into(),
            faction,
            classes: vec![],
        };
        let races = [
            race(4, "nightelf", crate::faction::Faction::Alliance),
            race(2, "orc", crate::faction::Faction::Horde),
        ];
        let found: Vec<_> = factions(&conn, &races)
            .unwrap()
            .into_iter()
            .map(|f| (f.id, f.side, f.quests, f.starts.into_iter().collect::<Vec<_>>()))
            .collect();
        assert_eq!(
            found,
            [
                (
                    69,
                    Some(crate::faction::Faction::Alliance),
                    2,
                    vec![(2, -42000), (4, 4000)]
                ),
                (529, None, 1, vec![(2, 0), (4, 0)]),
            ]
        );
    }
}
