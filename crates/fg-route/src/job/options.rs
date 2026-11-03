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
    /// The flight paths of the game version, to tick those a character knows.
    pub flights: Vec<FlightOption>,
    pub dungeons: Vec<DungeonOption>,
    /// Factions quests give reputation with, for the guides' reputation goals.
    pub factions: Vec<FactionOption>,
    pub class_quests: HashMap<String, Vec<String>>,
    /// Each class's power curve and gear dependence (class_power.toml), by class name in lower
    /// case like `class_quests`: what the advanced settings start from.
    pub class_power: HashMap<String, ClassPowerOption>,
    pub params: Params,
    /// Level cap of the game version.
    pub max_level: i64,
}

/// A flight path (TaxiNodes ID), the continent it is on (map ID) and the sides that can use it.
#[derive(Debug, Clone, Serialize)]
pub struct FlightOption {
    pub id: i64,
    pub name: String,
    pub continent: i64,
    pub alliance: bool,
    pub horde: bool,
}

#[derive(Debug, Clone, Serialize)]
pub struct ClassPowerOption {
    pub curve: Vec<(i64, f64)>,
    pub gear: f64,
}

#[derive(Debug, Clone, Serialize)]
pub struct RaceOption {
    pub key: String,
    pub id: i64,
    pub name: String,
    pub faction: crate::faction::Faction,
    /// Classes this race can play (class keys), from the game's CharBaseInfo.
    pub classes: Vec<String>,
    /// The NPC its new characters start next to (races sharing a starting place share it).
    pub start_npc: i64,
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
    /// Its level (`model::dungeon_levels`): it is run from `Params::dungeon_from` to
    /// `Params::dungeon_to` around it.
    pub level: Option<i64>,
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
                start_npc: r.start_npc,
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
             SELECT d.area, d.name FROM dungeon d JOIN client_areatable a ON a.ID = d.area
             JOIN client_map m ON m.InstanceType = 1 AND (m.ID = a.ContinentID OR m.AreaTableID = d.area)
             WHERE d.area IN found OR d.area IN (SELECT i.dungeon FROM inside i JOIN m_quest q ON q.zone_or_sort = i.zone
                                                 WHERE q.status = 'available' AND q.level <= ?1)
             GROUP BY d.area",
        )?;
        let max_level = overrides.params.edition.rules().max_level + 2;
        let levels = crate::model::dungeon_levels(conn, &overrides.dungeons)?;
        let dungeon = |area: i64, name: String| DungeonOption {
            id: area,
            key: area.to_string(),
            name,
            names: names_of(area),
            level: levels.get(&area).copied(),
        };
        let mut list: Vec<DungeonOption> = stmt
            .query_map([max_level], |r| Ok(dungeon(r.get(0)?, r.get(1)?)))?
            .collect::<rusqlite::Result<_>>()?;
        for def in &overrides.dungeons {
            if let Some(name) = &def.name
                && !list.iter().any(|d| d.id == def.area)
            {
                list.push(dungeon(def.area, name.clone()));
            }
        }
        // By level (those without, last), then name.
        list.sort_by(|a, b| (a.level.unwrap_or(i64::MAX), &a.name).cmp(&(b.level.unwrap_or(i64::MAX), &b.name)));
        list
    };
    let factions = factions(conn, &races)?;
    // As the planner takes them (`World::load`): a mount of the side, shown on its flight map.
    let flights: Vec<FlightOption> = conn
        .prepare(
            "SELECT ID, Name_lang, ContinentID, MountCreatureID_0 != 0 AND (Flags & 2) != 0,
                    MountCreatureID_1 != 0 AND (Flags & 1) != 0
             FROM client_taxinodes ORDER BY ContinentID, Name_lang",
        )
        .and_then(|mut stmt| {
            stmt.query_map([], |r| {
                Ok(FlightOption {
                    id: r.get(0)?,
                    name: r.get(1)?,
                    continent: r.get(2)?,
                    horde: r.get(3)?,
                    alliance: r.get(4)?,
                })
            })?
            .collect::<rusqlite::Result<Vec<_>>>()
        })
        .unwrap_or_default()
        .into_iter()
        .filter(|f| f.alliance || f.horde)
        .collect();
    Ok(Options {
        races,
        classes,
        zones,
        flights,
        dungeons,
        factions,
        class_quests: class_quests(conn, overrides)?,
        class_power: (overrides.class_power.iter())
            .map(|(class, p)| {
                let curve = p.points()?;
                Ok((
                    class.clone(),
                    ClassPowerOption {
                        curve,
                        gear: p.gear.unwrap_or(1.0),
                    },
                ))
            })
            .collect::<Result<_>>()?,
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

/// Class requirements from this level on are endgame (see `class_quests`).
const SPELL_QUESTS_BELOW: i64 = 50;

/// The class requirements by default, by class key: those the database builds (`m_class_requirement`:
/// a class spell a pet, a stance, a totem, Redemption... needs one of its quests, a start zone's
/// letter to the class trainer, the overrides' lists), then the configured ones
/// (overrides/class_quests.toml). A guide does not leave a class spell out. The app lists them,
/// the player takes out those not wanted. Those whose quests start from level 50 (an epic mount,
/// Inferno, Ritual of Doom: raids and elite dungeons) are endgame, not leveling.
pub fn class_quests(conn: &Connection, overrides: &Overrides) -> Result<HashMap<String, Vec<String>>> {
    let mut out = overrides.class_quests.clone();
    let Ok(mut classes) = conn.prepare("SELECT ID, lower(Filename) FROM client_chrclasses ORDER BY ID") else {
        return Ok(out);
    };
    let classes: Vec<(i64, String)> = classes
        .query_map([], |r| Ok((r.get(0)?, r.get(1)?)))?
        .collect::<rusqlite::Result<_>>()?;
    for (id, key) in classes.into_iter().filter(|(id, _)| (1..=31).contains(id)) {
        let names = crate::model::requirements::defaults(conn, id, SPELL_QUESTS_BELOW)?;
        let list = out.entry(key).or_default();
        for name in names {
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

    /// The class requirements the database has are required by default, the endgame ones (their
    /// quests from level 50 on) and those the overrides leave to the player not; the configured
    /// ones come first.
    #[test]
    fn class_requirements_are_required_by_default_unless_endgame() {
        let conn = Connection::open_in_memory().unwrap();
        conn.execute_batch(
            "CREATE TABLE client_chrclasses (ID, Filename);
             INSERT INTO client_chrclasses VALUES (2, 'PALADIN'), (3, 'HUNTER');
             CREATE TABLE m_class_requirement (id, class_id, name, spell_id, max_level, min_level, by_default, source);
             INSERT INTO m_class_requirement VALUES
               (1, 2, 'Redemption', 7328, NULL, 12, 1, 'data'),
               (2, 2, 'Summon Warhorse', 13819, NULL, 40, 1, 'data'),
               (3, 2, 'Summon Charger', 23214, NULL, 60, 1, 'data'),
               (4, 2, 'Encrypted Sigil', NULL, NULL, 1, 1, 'data'),
               (5, 3, 'Tame Beast', 1515, NULL, 10, 1, 'data'),
               (6, 3, 'Beast Lore', NULL, NULL, 10, 0, 'manual');",
        )
        .unwrap();
        let mut overrides = Overrides::default();
        overrides
            .class_quests
            .insert("paladin".into(), vec!["The Tome of Divinity".into()]);
        let quests = class_quests(&conn, &overrides).unwrap();
        assert_eq!(
            quests["paladin"],
            [
                "The Tome of Divinity",
                "Encrypted Sigil",
                "Redemption",
                "Summon Warhorse"
            ]
        );
        assert_eq!(quests["hunter"], ["Tame Beast"]);
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
            start_npc: 0,
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
