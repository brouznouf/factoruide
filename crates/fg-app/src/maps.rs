//! The index of the maps the guide pages show: the world's (`fg maps`) and the dungeons' floors
//! (`fgd dungeon-maps`), from an edition's database.

use crate::CmdError;
use rusqlite::Connection;
use serde_json::{Map, Value, json};
use std::collections::HashMap;

fn err(e: impl std::fmt::Display) -> CmdError {
    CmdError::App(format!("{e:#}"))
}

/// Every map of the edition: the world, continent and zone maps with the zones' names in the
/// game's languages (`names`: locale -> name), then the dungeons' floors (`kind` 4, `floor` their
/// number, `bounds` the rectangle of the instance they show).
///
/// # Errors
///
/// [`CmdError`] when the database has no map index.
pub fn map_index(db: &Connection) -> Result<Value, CmdError> {
    let text: String = db
        .query_row("SELECT value FROM app_meta WHERE key = 'map_index'", [], |r| r.get(0))
        .map_err(err)?;
    let mut index: Value = serde_json::from_str(&text).map_err(err)?;
    let mut names: HashMap<i64, Map<String, Value>> = HashMap::new();
    if let Ok(mut stmt) =
        db.prepare("SELECT id, locale, name FROM l10n WHERE entity_type = 'zone' AND name IS NOT NULL")
    {
        let rows = stmt
            .query_map([], |r| {
                Ok((r.get::<_, i64>(0)?, r.get::<_, String>(1)?, r.get::<_, String>(2)?))
            })
            .map_err(err)?;
        for (id, locale, name) in rows.flatten() {
            names.entry(id).or_default().insert(locale, name.into());
        }
    }
    let maps = index.as_array_mut().ok_or_else(|| err("the map index is not a list"))?;
    for map in maps.iter_mut() {
        if let Some(n) = map["area"].as_i64().and_then(|a| names.get(&a)) {
            map["names"] = Value::Object(n.clone());
        }
    }
    // A text or bytes (the databases shipped keep it as a blob).
    let floors: Option<Vec<u8>> = db
        .query_row("SELECT value FROM app_meta WHERE key = 'dungeon_maps'", [], |r| {
            Ok(r.get_ref(0)?.as_bytes()?.to_vec())
        })
        .ok();
    let floors: Vec<Value> = floors
        .map_or_else(|| Ok(Vec::new()), |t| serde_json::from_slice(&t))
        .map_err(err)?;
    for f in floors {
        maps.push(json!({
            "id": f["ui_map"],
            "name": f["name"],
            "kind": 4,
            "parent": 0,
            "width": f["width"],
            "height": f["height"],
            "file": f["file"],
            "floor": f["art_floor"],
            "bounds": { "map": f["map"], "min_x": f["min_x"], "max_x": f["max_x"], "min_y": f["min_y"], "max_y": f["max_y"] },
            "area": null,
        }));
    }
    Ok(index)
}

#[cfg(test)]
mod tests {
    use super::map_index;
    use rusqlite::Connection;

    #[test]
    fn the_dungeons_floors_follow_the_world_maps() {
        let db = Connection::open_in_memory().unwrap();
        db.execute_batch(
            r#"CREATE TABLE app_meta (key TEXT PRIMARY KEY, value TEXT NOT NULL);
               INSERT INTO app_meta VALUES ('map_index', '[{"id": 1413, "kind": 3, "area": 17}]');
               INSERT INTO app_meta VALUES ('dungeon_maps', '[{"ui_map": 279, "name": "Wailing Caverns", "map": 43,
                 "min_x": -410, "max_x": 214, "min_y": -376, "max_y": 561, "art_floor": 1, "file": "279.webp",
                 "width": 1002, "height": 668}]');"#,
        )
        .unwrap();
        let index = map_index(&db).unwrap();
        let maps = index.as_array().unwrap();
        assert_eq!(maps.len(), 2);
        assert_eq!(maps[1]["id"], 279);
        assert_eq!(maps[1]["kind"], 4);
        assert_eq!(maps[1]["bounds"]["map"], 43);
    }

    /// The databases shipped keep the floors as bytes (a blob), not a text.
    #[test]
    fn the_dungeons_floors_are_read_from_bytes_too() {
        let db = Connection::open_in_memory().unwrap();
        db.execute_batch(
            r#"CREATE TABLE app_meta (key TEXT PRIMARY KEY, value TEXT NOT NULL);
               INSERT INTO app_meta VALUES ('map_index', '[{"id": 1413, "kind": 3, "area": 17}]');
               INSERT INTO app_meta VALUES ('dungeon_maps', CAST('[{"ui_map": 279, "name": "Wailing Caverns", "map": 43,
                 "min_x": -410, "max_x": 214, "min_y": -376, "max_y": 561, "art_floor": 1, "file": "279.webp",
                 "width": 1002, "height": 668}]' AS BLOB));"#,
        )
        .unwrap();
        let index = map_index(&db).unwrap();
        assert_eq!(index.as_array().unwrap().len(), 2);
    }
}
