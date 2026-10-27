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

/// Quests a character of race `race` (races.toml key) and class `class` can do whose ID is
/// `query` or whose name holds it (in English or in `locale`), the closest names first.
pub fn quest_choices(
    conn: &Connection,
    overrides: &super::Overrides,
    race: &str,
    class: &str,
    query: &str,
    locale: Option<&str>,
) -> Result<Vec<QuestChoice>> {
    const SHOWN: i64 = 20;
    let query = query.trim();
    if query.is_empty() {
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
    let mut stmt = conn.prepare(
        "SELECT q.id, coalesce(l.name, q.name), coalesce(q.level, 0), a.AreaName_lang
         FROM m_quest q
         LEFT JOIN l10n l ON l.entity_type = 'quest' AND l.id = q.id AND l.locale = ?3
         LEFT JOIN client_areatable a ON a.ID = q.zone_or_sort
         WHERE q.status = 'available' AND q.name IS NOT NULL
           AND (coalesce(q.races_mask, 0) = 0 OR ?4 = 0 OR q.races_mask & ?4 != 0)
           AND (coalesce(q.classes_mask, 0) = 0 OR ?5 = 0 OR q.classes_mask & ?5 != 0)
           AND (q.id = ?1 OR q.name LIKE '%' || ?2 || '%' OR l.name LIKE '%' || ?2 || '%')
         ORDER BY q.id != ?1, lower(coalesce(l.name, q.name)) != lower(?2),
                  lower(coalesce(l.name, q.name)) NOT LIKE lower(?2) || '%', q.level, q.id
         LIMIT ?6",
    )?;
    let id = query.parse::<i64>().unwrap_or(0);
    let rows = stmt.query_map(
        rusqlite::params![id, query, locale.unwrap_or(""), race_bit, class_bit, SHOWN],
        |r| {
            Ok(QuestChoice {
                id: r.get(0)?,
                name: r.get(1)?,
                level: r.get(2)?,
                zone: r.get(3)?,
            })
        },
    )?;
    Ok(rows.collect::<rusqlite::Result<_>>()?)
}
