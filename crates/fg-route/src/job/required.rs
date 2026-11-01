//! Resolve must-have quest IDs and names for the character or group.

use super::{PlanRequest, QuestKey, RequiredQuest};
use crate::model::{CompletionWindow, Profile, QuestRequirement};
use crate::params::Params;
use anyhow::{Result, bail};
use rusqlite::Connection;

pub(super) fn resolve(
    conn: &Connection,
    request: &PlanRequest,
    profile: &Profile,
    params: &Params,
) -> Result<Vec<QuestRequirement>> {
    let mut out: Vec<QuestRequirement> = Vec::new();
    for choice in &request.required_quests {
        let mut stmt = conn.prepare(
            "SELECT id, coalesce(name, ''), coalesce(races_mask, 0), coalesce(classes_mask, 0)
             FROM m_quest WHERE (?1 > 0 AND id = ?1) OR (?2 != '' AND lower(name) = lower(?2) AND status = 'available') ORDER BY id",
        )?;
        let (id, name, window) = match choice {
            RequiredQuest::Id(id) => (*id, "", CompletionWindow::default()),
            RequiredQuest::Name(name) => (0, name.trim(), CompletionWindow::default()),
            RequiredQuest::Window { quest, window } => match quest {
                QuestKey::Id(id) => (*id, "", *window),
                QuestKey::Name(name) => (0, name.trim(), *window),
            },
        };
        let label = if id != 0 { id.to_string() } else { name.to_owned() };
        anyhow::ensure!(
            window
                .min_level
                .into_iter()
                .chain(window.max_level)
                .all(|l| (1..=params.edition.rules().max_level).contains(&l)),
            "required quest {label:?}: completion levels must be between 1 and {}",
            params.edition.rules().max_level
        );
        anyhow::ensure!(
            window
                .min_level
                .zip(window.max_level)
                .is_none_or(|(min, max)| min <= max),
            "required quest {label:?}: minimum completion level exceeds maximum"
        );
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
        let selected = match eligible.as_slice() {
            [quest] => quest.0,
            [] if matches.is_empty() => bail!("required quest {label:?}: not found in this edition's data"),
            [] => bail!("required quest {label:?}: unavailable to this character's race or class"),
            _ => bail!(
                "required quest {label:?}: ambiguous name; choose an ID ({})",
                eligible.iter().map(|q| q.0.to_string()).collect::<Vec<_>>().join(", ")
            ),
        };
        if let Some(previous) = out.iter_mut().find(|q| q.id == selected) {
            previous.window = CompletionWindow {
                min_level: previous.window.min_level.into_iter().chain(window.min_level).max(),
                max_level: previous.window.max_level.into_iter().chain(window.max_level).min(),
            };
            anyhow::ensure!(
                previous
                    .window
                    .min_level
                    .zip(previous.window.max_level)
                    .is_none_or(|(min, max)| min <= max),
                "required quest {label:?}: completion windows do not overlap"
            );
        } else {
            out.push(QuestRequirement { id: selected, window });
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
    let race_bit = overrides.races.get(race).map_or(0, |r| crate::model::race_bit(r.id));
    let class_bit: i64 = conn
        .query_row(
            "SELECT 1 << (ID - 1) FROM client_chrclasses WHERE lower(Filename) = lower(?1)",
            [class],
            |r| r.get(0),
        )
        .unwrap_or(0);
    choices(conn, (race_bit, class_bit), query, locale)
}

/// The quests of a race and class (as bits, 0: any) matching the query (see `quest_choices`).
fn choices(
    conn: &Connection,
    (race_bit, class_bit): (i64, i64),
    query: &str,
    locale: Option<&str>,
) -> Result<Vec<QuestChoice>> {
    use rusqlite::types::Value;
    const SHOWN: i64 = 100;
    let words: Vec<&str> = query.split_whitespace().collect();
    if words.is_empty() {
        return Ok(Vec::new());
    }
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

/// Starting deadlines use the same participation and completion masks as the simulation.
pub(super) fn check_start(
    model: &crate::model::Model,
    profile: &Profile,
    members: &[&super::StartState],
) -> Result<()> {
    for (i, q) in model.quests.iter().enumerate() {
        let Some(max) = q.completion_window.max_level else {
            continue;
        };
        if model.initial.turned.contains(&i) {
            continue;
        }
        anyhow::ensure!(
            profile.completed_quests.contains(&q.id) || model.initial.accepted.contains(&i) || q.min_level <= max,
            "required quest {} ({}): needs level {}, above the requested completion window",
            q.name,
            q.id,
            q.min_level
        );
        if members.is_empty() {
            anyhow::ensure!(
                profile.completed_quests.contains(&q.id) || profile.from_level <= max,
                "required quest {} ({}): character is already above completion level {max}",
                q.name,
                q.id
            );
        } else {
            let need = model.initial.need(i);
            for (_, member) in members.iter().enumerate().filter(|(k, _)| need & (1 << k) != 0) {
                anyhow::ensure!(
                    member.level <= max,
                    "required quest {} ({}): {} is already above completion level {max}",
                    q.name,
                    q.id,
                    member.name
                );
            }
        }
    }
    Ok(())
}

#[cfg(test)]
mod window_tests {
    #[test]
    fn a_held_quest_can_finish_after_a_refresh_raises_its_effective_minimum() {
        let mut p = crate::plan::tests::quests_at_start(1);
        p.model.quests[0].min_level = 5;
        p.model.quests[0].completion_window.max_level = Some(1);
        p.model.initial.accepted = vec![0];
        super::check_start(&p.model, &p.profile, &[]).unwrap();
        p.model.initial.accepted.clear();
        assert!(super::check_start(&p.model, &p.profile, &[]).is_err());
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::faction::Faction;
    use crate::job::RequiredQuest;
    use crate::model::testing::profile;

    /// A database of the quests given (id, name, races mask, classes mask, status, level, zone).
    fn db(quests: &[(i64, &str, i64, i64, &str, i64, i64)]) -> Connection {
        let conn = Connection::open_in_memory().unwrap();
        conn.execute_batch(
            "CREATE TABLE m_quest(id INTEGER, name TEXT, races_mask INTEGER, classes_mask INTEGER, status TEXT,
                                  level INTEGER, zone_or_sort INTEGER);
             CREATE TABLE l10n(entity_type TEXT, id INTEGER, locale TEXT, name TEXT);
             CREATE TABLE client_areatable(ID INTEGER, AreaName_lang TEXT);",
        )
        .unwrap();
        for q in quests {
            conn.execute(
                "INSERT INTO m_quest VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7)",
                rusqlite::params![q.0, q.1, q.2, q.3, q.4, q.5, q.6],
            )
            .unwrap();
        }
        conn
    }

    fn request(choices: Vec<RequiredQuest>) -> PlanRequest {
        let mut r: PlanRequest = serde_json::from_value(
            serde_json::json!({"race": "human", "class": "warrior", "from_level": 1, "to_level": 5}),
        )
        .unwrap();
        r.required_quests = choices;
        r
    }

    fn human_warrior() -> Profile {
        Profile {
            race_id: 1,
            class_id: 1,
            ..profile(Faction::Alliance, 1, 5)
        }
    }

    const HUMAN: i64 = 1;
    const WARRIOR: i64 = 1;

    #[test]
    fn a_must_have_quest_is_found_by_id_or_exact_name_and_once() {
        let conn = db(&[(21, "Skirmish at Echo Ridge", 0, 0, "available", 2, 12)]);
        let both = request(vec![
            RequiredQuest::Id(21),
            RequiredQuest::Name(" skirmish at echo ridge ".into()),
        ]);
        assert_eq!(
            resolve(&conn, &both, &human_warrior(), &Params::default())
                .unwrap()
                .iter()
                .map(|q| q.id)
                .collect::<Vec<_>>(),
            [21]
        );
    }

    #[test]
    fn an_unknown_quest_is_not_found() {
        let conn = db(&[]);
        let error = resolve(
            &conn,
            &request(vec![RequiredQuest::Id(999_999_999)]),
            &human_warrior(),
            &Params::default(),
        )
        .unwrap_err();
        assert!(format!("{error:#}").contains("not found"), "{error:#}");
    }

    #[test]
    fn a_quest_of_another_race_or_class_is_unavailable_to_the_character() {
        let conn = db(&[
            (3087, "Orc only", 1 << 1, 0, "available", 10, 0),
            (3088, "Mage only", 0, 1 << 7, "available", 10, 0),
        ]);
        for id in [3087, 3088] {
            let error = resolve(
                &conn,
                &request(vec![RequiredQuest::Id(id)]),
                &human_warrior(),
                &Params::default(),
            )
            .unwrap_err();
            assert!(format!("{error:#}").contains("race or class"), "{error:#}");
        }
    }

    #[test]
    fn a_name_shared_by_quests_the_character_can_do_lists_their_ids() {
        let name = "A Call to Arms: The Plaguelands!";
        let conn = db(&[
            (5066, name, 0, 0, "available", 50, 0),
            (5090, name, 0, 0, "available", 50, 0),
            (5091, name, 0, 0, "available", 50, 0),
            (5092, name, 0, 0, "removed", 50, 0),
        ]);
        let error = resolve(
            &conn,
            &request(vec![RequiredQuest::Name(name.into())]),
            &human_warrior(),
            &Params::default(),
        )
        .unwrap_err();
        assert!(
            format!("{error:#}").contains("ambiguous name; choose an ID (5066, 5090, 5091)"),
            "{error:#}"
        );
    }

    /// Must-have quests are found by ID, level, part of their name or of their zone's, in English or
    /// the guide's language, among those of the character's race and class.
    #[test]
    fn quests_are_searched_by_id_level_name_or_zone() {
        let conn = db(&[
            (91777, "Rare Books", 1 << (HUMAN - 1), 0, "available", 45, 1519),
            (91778, "Rare Metals", 0, 0, "available", 30, 17),
            (91779, "Orc Rare Books", 1 << 1, 0, "available", 20, 17),
            (91780, "Barrens Run", 0, 0, "available", 20, 17),
            (91781, "Gone", 0, 0, "removed", 20, 17),
        ]);
        conn.execute_batch(
            "INSERT INTO client_areatable VALUES (1519, 'Stormwind City');
             INSERT INTO client_areatable VALUES (17, 'The Barrens');
             INSERT INTO l10n VALUES ('quest', 91777, 'frFR', 'Livres rares');",
        )
        .unwrap();
        let human = (1 << (HUMAN - 1), 1 << (WARRIOR - 1));
        let find = |who, query: &str, locale| {
            choices(&conn, who, query, locale)
                .unwrap()
                .into_iter()
                .map(|q| (q.id, q.name))
                .collect::<Vec<_>>()
        };
        assert_eq!(find(human, "91777", None)[0], (91777, "Rare Books".to_owned()));
        assert_eq!(find(human, "rare book", None)[0].0, 91777);
        assert_eq!(find(human, "livres", Some("frFR"))[0].0, 91777);
        let orc = (1 << 1, 1 << (WARRIOR - 1));
        assert!(
            find(orc, "Rare Books", None).iter().all(|(id, _)| *id != 91777),
            "Alliance only"
        );
        assert_eq!(find(human, "", None), []);
        let level = choices(&conn, human, "45", None).unwrap();
        assert!(!level.is_empty() && level.iter().all(|q| q.level == 45 || q.id == 45));
        let barrens = choices(&conn, orc, "barrens 20", None).unwrap();
        assert!(!barrens.is_empty());
        assert!(
            barrens
                .iter()
                .all(|q| q.level == 20 && q.zone.as_deref() == Some("The Barrens")),
            "{barrens:?}"
        );
    }
}
