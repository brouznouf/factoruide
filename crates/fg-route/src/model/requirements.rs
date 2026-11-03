//! The class quests the route must do: `m_class_requirement`, built from the quests teaching a
//! spell and the overrides' lists (`overrides/database`).

use super::types::ClassRequirement;
use anyhow::Result;
use rusqlite::Connection;

/// Whether the database has the table (databases built before it do not).
fn exists(conn: &Connection) -> Result<bool> {
    Ok(conn.query_row(
        "SELECT count(*) > 0 FROM sqlite_master WHERE name = 'm_class_requirement'",
        [],
        |r| r.get(0),
    )?)
}

/// The requirements of the classes (game IDs), each with its quests.
pub fn load(conn: &Connection, class_ids: &[i64]) -> Result<Vec<ClassRequirement>> {
    if !exists(conn)? {
        return Ok(Vec::new());
    }
    let mut stmt = conn.prepare(
        "SELECT c.id, c.class_id, c.name, c.spell_id, c.max_level, q.id, q.name
         FROM m_class_requirement c
         JOIN m_class_requirement_quest r ON r.requirement_id = c.id
         JOIN m_quest q ON q.id = r.quest_id
         ORDER BY c.id, q.id",
    )?;
    let rows = stmt.query_map([], |r| {
        Ok((
            r.get::<_, i64>(0)?,
            r.get::<_, i64>(1)?,
            r.get::<_, String>(2)?,
            r.get::<_, Option<i64>>(3)?,
            r.get::<_, Option<i64>>(4)?,
            r.get::<_, i64>(5)?,
            r.get::<_, String>(6)?,
        ))
    })?;
    let mut out: Vec<(i64, ClassRequirement)> = Vec::new();
    for row in rows {
        let (id, class_id, name, spell, max_level, quest, quest_name) = row?;
        if !class_ids.contains(&class_id) {
            continue;
        }
        match out.last_mut() {
            Some((last, req)) if *last == id => {
                req.quests.push(quest);
                if !req.quest_names.contains(&quest_name) {
                    req.quest_names.push(quest_name);
                }
            }
            _ => out.push((
                id,
                ClassRequirement {
                    class_id,
                    name,
                    spell,
                    max_level,
                    quests: vec![quest],
                    quest_names: vec![quest_name],
                    selected: false,
                },
            )),
        }
    }
    Ok(out.into_iter().map(|(_, r)| r).collect())
}

/// The requirements `names` ask for are selected: those of that name, and (the guides saved
/// before the requirements named quests) those with a quest of that name.
pub fn select(mut all: Vec<ClassRequirement>, names: &[String]) -> Vec<ClassRequirement> {
    for r in &mut all {
        r.selected = names.iter().any(|n| *n == r.name || r.quest_names.contains(n));
    }
    all
}

/// The names of the requirements a class has by default: not those the overrides leave to the
/// player (`default = false`), nor those of its quests from level `endgame` on, which are endgame
/// (an epic mount, Inferno, Ritual of Doom: raids and elite dungeons).
pub fn defaults(conn: &Connection, class_id: i64, endgame: i64) -> Result<Vec<String>> {
    if !exists(conn)? {
        return Ok(Vec::new());
    }
    let mut stmt = conn.prepare(
        "SELECT name FROM m_class_requirement WHERE class_id = ?1 AND coalesce(min_level, 1) < ?2 AND by_default != 0
         ORDER BY coalesce(min_level, 1), name",
    )?;
    let names = stmt
        .query_map(rusqlite::params![class_id, endgame], |r| r.get(0))?
        .collect::<rusqlite::Result<Vec<String>>>()?;
    Ok(names)
}
