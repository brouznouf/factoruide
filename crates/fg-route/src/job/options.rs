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
    pub dungeons: Vec<NamedId>,
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
    // object, or one dropping a quest item), plus those declared in dungeons.toml.
    let dungeons = {
        let mut stmt = conn.prepare(
            "WITH needed(entity_type, entity_id) AS (
               SELECT CASE o.type WHEN 'object' THEN 'object' ELSE 'npc' END, o.target_id
               FROM m_quest_objective o JOIN m_quest q ON q.id = o.quest_id
               WHERE q.status = 'available' AND q.level <= ?1 AND o.type != 'item'
               UNION SELECT i.from_type, i.from_id
               FROM m_quest_objective o JOIN m_quest q ON q.id = o.quest_id
               JOIN m_item_source i ON i.item_id = o.target_id AND i.from_type IN ('npc', 'object')
               WHERE q.status = 'available' AND q.level <= ?1 AND o.type = 'item')
             SELECT d.area, d.name FROM dungeon d JOIN client_areatable a ON a.ID = d.area
             JOIN client_map m ON m.ID = a.ContinentID
             WHERE m.InstanceType = 1 AND EXISTS (
               SELECT 1 FROM needed n JOIN m_spawn s ON s.entity_type = n.entity_type AND s.entity_id = n.entity_id
               WHERE s.zone_id = d.area OR s.zone_id IN (SELECT value FROM json_each(coalesce(d.alt_areas, '[]'))))",
        )?;
        let max_level = overrides.params.edition.rules().max_level + 2;
        let mut list: Vec<NamedId> = stmt
            .query_map([max_level], |r| {
                Ok(NamedId {
                    id: r.get(0)?,
                    key: r.get::<_, i64>(0)?.to_string(),
                    name: r.get(1)?,
                    names: names_of(r.get(0)?),
                })
            })?
            .collect::<rusqlite::Result<_>>()?;
        for def in &overrides.dungeons {
            if let Some(name) = &def.name
                && !list.iter().any(|d| d.id == def.area)
            {
                list.push(NamedId {
                    id: def.area,
                    key: def.area.to_string(),
                    name: name.clone(),
                    names: names_of(def.area),
                });
            }
        }
        list.sort_by(|a, b| a.name.cmp(&b.name));
        list
    };
    Ok(Options {
        races,
        classes,
        zones,
        dungeons,
        class_quests: overrides.class_quests.clone(),
        params: overrides.params.clone(),
        max_level: overrides.params.edition.rules().max_level,
    })
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
