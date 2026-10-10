//! Names in the guide's language.

use anyhow::Result;
use rusqlite::Connection;
use std::collections::HashMap;

/// Names in the guide's language, English (the base data) when there is no translation.
#[derive(Debug, Clone, Default)]
pub struct Names {
    pub locale: String,
    quest: HashMap<i64, String>,
    npc: HashMap<i64, String>,
    item: HashMap<i64, String>,
    object: HashMap<i64, String>,
    /// Zones and dungeons (AreaTable IDs).
    zone: HashMap<i64, String>,
}

impl Names {
    /// No translation: the English names everywhere, sentences in `locale`.
    pub fn new(locale: &str) -> Self {
        Self {
            locale: locale.to_owned(),
            ..Default::default()
        }
    }

    pub fn load(conn: &Connection, locale: &str) -> Result<Self> {
        let mut names = Names {
            locale: locale.to_owned(),
            ..Default::default()
        };
        if locale == "enUS" {
            return Ok(names);
        }
        // Names seen in the game client first, then QuestieDB's translations.
        let mut stmt = conn.prepare(
            "SELECT entity_type, id, name FROM l10n WHERE locale = ?1 AND name IS NOT NULL AND name != ''
             ORDER BY CASE source WHEN 'client_cache' THEN 0 ELSE 1 END DESC",
        )?;
        for row in stmt.query_map([locale], |r| {
            Ok((r.get::<_, String>(0)?, r.get::<_, i64>(1)?, r.get::<_, String>(2)?))
        })? {
            let (kind, id, name) = row?;
            let map = match kind.as_str() {
                "quest" => &mut names.quest,
                "npc" => &mut names.npc,
                "item" => &mut names.item,
                "object" => &mut names.object,
                "zone" => &mut names.zone,
                _ => continue,
            };
            map.insert(id, name); // ordered so that the preferred source is inserted last
        }
        Ok(names)
    }

    /// Translated name of an entity ("quest", "npc", "item", "object", "zone"), else `english`.
    pub fn get<'a>(&'a self, kind: &str, id: i64, english: &'a str) -> &'a str {
        let map = match kind {
            "quest" => &self.quest,
            "npc" => &self.npc,
            "item" => &self.item,
            "object" => &self.object,
            "zone" => &self.zone,
            _ => return english,
        };
        map.get(&id).map_or(english, String::as_str)
    }
}
