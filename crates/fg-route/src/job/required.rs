//! Resolve must-have quest IDs and names for the character or group.

use super::{PlanRequest, RequiredQuest};
use crate::model::Profile;
use anyhow::{Result, bail};
use rusqlite::Connection;

pub(super) fn resolve(conn: &Connection, request: &PlanRequest, profile: &Profile) -> Result<Vec<i64>> {
    let mut out = Vec::new();
    for choice in &request.required_quests {
        let mut stmt = conn.prepare(
            "SELECT id, coalesce(name, ''), coalesce(races_mask, 0), coalesce(classes_mask, 0)
             FROM m_quest WHERE (?1 > 0 AND id = ?1) OR (?2 != '' AND lower(name) = lower(?2) AND status = 'available') ORDER BY id",
        )?;
        let (id, name) = match choice {
            RequiredQuest::Id(id) => (*id, ""),
            RequiredQuest::Name(name) => (0, name.trim()),
        };
        let matches = stmt
            .query_map(rusqlite::params![id, name], |r| {
                Ok((
                    r.get::<_, i64>(0)?,
                    r.get::<_, String>(1)?,
                    r.get::<_, i64>(2)?,
                    r.get::<_, i64>(3)?,
                ))
            })?
            .collect::<rusqlite::Result<Vec<_>>>()?;
        let eligible: Vec<_> = matches
            .iter()
            .filter(|(_, _, races, classes)| profile.members_of(Some(*races), Some(*classes)) != 0)
            .collect();
        let label = match choice {
            RequiredQuest::Id(id) => id.to_string(),
            RequiredQuest::Name(name) => name.clone(),
        };
        let selected = match eligible.as_slice() {
            [quest] => quest.0,
            [] if matches.is_empty() => bail!("required quest {label:?}: not found in this edition's data"),
            [] => bail!("required quest {label:?}: unavailable to this character's race or class"),
            _ => bail!(
                "required quest {label:?}: ambiguous name; choose an ID ({})",
                eligible.iter().map(|q| q.0.to_string()).collect::<Vec<_>>().join(", ")
            ),
        };
        if !out.contains(&selected) {
            out.push(selected);
        }
    }
    Ok(out)
}

/// A quest offered when choosing must-have quests: its name in the guide's language when known.
#[derive(Debug, Clone, serde::Serialize)]
pub struct QuestChoice {
    pub id: i64,
    pub name: String,
    pub level: i64,
    /// The zone it is filed under, when a zone.
    pub zone: Option<String>,
}

/// Quests a character of race `race` (races.toml key) and class `class` can do that match every
/// word of `query`: a number is an ID or a level ("45"), other words are part of the quest's or
/// its zone's name, in English or in `locale` ("barrens 20"). The quest of that ID first, then
/// the names starting with the query, by level.
pub fn quest_choices(
    conn: &Connection,
    overrides: &super::Overrides,
    race: &str,
    class: &str,
    query: &str,
    locale: Option<&str>,
) -> Result<Vec<QuestChoice>> {
    use rusqlite::types::Value;
    const SHOWN: i64 = 100;
    let words: Vec<&str> = query.split_whitespace().collect();
    if words.is_empty() {
        return Ok(Vec::new());
    }
    let race_bit = overrides.races.get(race).map_or(0, |r| crate::model::race_bit(r.id));
    let class_bit: i64 = conn
        .query_row(
            "SELECT 1 << (ID - 1) FROM client_chrclasses WHERE lower(Filename) = lower(?1)",
            [class],
            |r| r.get(0),
        )
        .unwrap_or(0);
    let query = query.trim();
    let mut params: Vec<Value> = vec![
        Value::Text(locale.unwrap_or("").to_owned()),
        Value::Integer(race_bit),
        Value::Integer(class_bit),
        Value::Integer(query.parse().unwrap_or(0)),
        Value::Text(query.to_owned()),
        Value::Integer(SHOWN),
    ];
    let mut matches = Vec::new();
    for word in words {
        params.push(match word.parse::<i64>() {
            Ok(n) => Value::Integer(n),
            Err(_) => Value::Text(word.to_owned()),
        });
        let n = params.len();
        matches.push(if word.parse::<i64>().is_ok() {
            format!("(q.id = ?{n} OR q.level = ?{n})")
        } else {
            format!(
                "(q.name LIKE '%' || ?{n} || '%' OR l.name LIKE '%' || ?{n} || '%'
                  OR a.AreaName_lang LIKE '%' || ?{n} || '%' OR z.name LIKE '%' || ?{n} || '%')"
            )
        });
    }
    let sql = format!(
        "SELECT q.id, coalesce(l.name, q.name), coalesce(q.level, 0), coalesce(z.name, a.AreaName_lang)
         FROM m_quest q
         LEFT JOIN l10n l ON l.entity_type = 'quest' AND l.id = q.id AND l.locale = ?1
         LEFT JOIN client_areatable a ON a.ID = q.zone_or_sort
         LEFT JOIN l10n z ON z.entity_type = 'zone' AND z.id = q.zone_or_sort AND z.locale = ?1
         WHERE q.status = 'available' AND q.name IS NOT NULL
           AND (coalesce(q.races_mask, 0) = 0 OR ?2 = 0 OR q.races_mask & ?2 != 0)
           AND (coalesce(q.classes_mask, 0) = 0 OR ?3 = 0 OR q.classes_mask & ?3 != 0)
           AND {}
         ORDER BY q.id != ?4, lower(coalesce(l.name, q.name)) NOT LIKE lower(?5) || '%', q.level, q.id
         LIMIT ?6",
        matches.join(" AND ")
    );
    let mut stmt = conn.prepare(&sql)?;
    let rows = stmt.query_map(rusqlite::params_from_iter(params), |r| {
        Ok(QuestChoice {
            id: r.get(0)?,
            name: r.get(1)?,
            level: r.get(2)?,
            zone: r.get(3)?,
        })
    })?;
    Ok(rows.collect::<rusqlite::Result<_>>()?)
}
