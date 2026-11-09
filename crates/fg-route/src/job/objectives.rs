//! What an objective guide (`PlanRequest::objectives`) holds for a character, before it is
//! computed: the quests left of each zone and dungeon to choose from, then the quests a set of
//! objectives plans and those it cannot.

use super::request::PlanRequest;
use super::{Overrides, StartState};
use crate::model::{self, GuideObjective};
use anyhow::Result;
use rusqlite::Connection;
use std::collections::{HashMap, HashSet};

/// The quests a character has left in each zone (an area and every area above it) and dungeon
/// (instance area), as an objective of that zone or dungeon would take them: of its race and
/// class, not done, no profession nor class quest in a zone.
#[derive(Debug, Clone, Default, serde::Serialize)]
pub struct ObjectiveCounts {
    pub zones: HashMap<i64, usize>,
    pub dungeons: HashMap<i64, usize>,
}

/// The quests a set of objectives plans for the character, and those of them it cannot.
#[derive(Debug, Clone, serde::Serialize)]
pub struct ObjectivePreview {
    /// The quests the guide does, what leads to them included.
    pub quests: usize,
    /// Quests of the objectives the character could take but the guide cannot plan, and why.
    pub excluded: Vec<ExcludedQuest>,
    /// The level the quests ask, when above the character's.
    pub recommended: Option<i64>,
}

#[derive(Debug, Clone, serde::Serialize)]
pub struct ExcludedQuest {
    pub id: i64,
    pub name: String,
    pub reason: String,
}

/// A quest of the database a character could take (`candidates`).
struct Candidate {
    id: i64,
    name: String,
    sort: Option<i64>,
    /// Neither a profession nor a class quest.
    general: bool,
}

/// The quests of the database the character could take and has not done: of its race and class.
fn candidates(conn: &Connection, start: &StartState) -> Result<Vec<Candidate>> {
    let race_bit = model::race_bit(start.race);
    let classes: Vec<(i64, String)> = conn
        .prepare("SELECT ID, coalesce(Filename, '') FROM client_chrclasses")?
        .query_map([], |r| Ok((r.get(0)?, r.get(1)?)))?
        .collect::<rusqlite::Result<_>>()?;
    let all_classes = classes.iter().fold(0i64, |m, (id, _)| m | 1 << (id - 1));
    let class_bit = (classes.iter())
        .find(|(_, f)| f.eq_ignore_ascii_case(&start.class))
        .map_or(0, |(id, _)| 1i64 << (id - 1));
    let done: HashSet<i64> = start.completed.iter().map(|q| q.0).collect();
    let mut stmt = conn.prepare(
        "SELECT id, name, zone_or_sort, coalesce(races_mask, 0), coalesce(classes_mask, 0),
                coalesce(required_skill_id, 0)
         FROM m_quest WHERE status = 'available' AND name IS NOT NULL",
    )?;
    let rows = stmt.query_map([], |r| {
        Ok((
            r.get::<_, i64>(0)?,
            r.get::<_, String>(1)?,
            r.get::<_, Option<i64>>(2)?,
            r.get::<_, i64>(3)?,
            r.get::<_, i64>(4)?,
            r.get::<_, i64>(5)?,
        ))
    })?;
    let mut out = Vec::new();
    for row in rows {
        let (id, name, sort, races, classes, skill) = row?;
        let for_race = races == 0 || race_bit == 0 || races & race_bit != 0;
        let for_class = classes == 0 || class_bit == 0 || classes & class_bit != 0;
        if !for_race || !for_class || done.contains(&id) {
            continue;
        }
        let class_quest = classes != 0 && classes & all_classes != all_classes || sort.is_some_and(|s| s < 0);
        out.push(Candidate {
            id,
            name,
            sort,
            general: skill == 0 && !class_quest,
        });
    }
    Ok(out)
}

/// Each area's parent (`client_areatable.ParentAreaID`).
fn area_parents(conn: &Connection) -> Result<HashMap<i64, i64>> {
    Ok(conn
        .prepare("SELECT ID, ParentAreaID FROM client_areatable WHERE ParentAreaID > 0")?
        .query_map([], |r| Ok((r.get(0)?, r.get(1)?)))?
        .collect::<rusqlite::Result<_>>()?)
}

/// The area and every area above it.
fn areas_up(parents: &HashMap<i64, i64>, area: i64) -> impl Iterator<Item = i64> + '_ {
    std::iter::successors(Some(area), |a| parents.get(a).copied().filter(|p| *p > 0)).take(8)
}

/// The quests a character met in game has left in each zone and dungeon (`ObjectiveCounts`).
///
/// # Errors
///
/// The game data could not be read.
pub fn objective_counts(conn: &Connection, start: &StartState) -> Result<ObjectiveCounts> {
    let parents = area_parents(conn)?;
    let dungeons: HashSet<i64> = conn
        .prepare("SELECT area FROM dungeon")?
        .query_map([], |r| r.get(0))?
        .collect::<rusqlite::Result<_>>()?;
    let mut counts = ObjectiveCounts::default();
    for c in candidates(conn, start)? {
        let Some(sort) = c.sort.filter(|s| *s > 0) else {
            continue;
        };
        if dungeons.contains(&sort) {
            *counts.dungeons.entry(sort).or_default() += 1;
        } else if c.general {
            for area in areas_up(&parents, sort) {
                *counts.zones.entry(area).or_default() += 1;
            }
        }
    }
    Ok(counts)
}

/// What the request's objectives plan for its character (`ObjectivePreview`), as the guide
/// would: the quests prepared, and those of the objectives left out with the reason.
///
/// # Errors
///
/// The request is refused (no objective, no character, nothing left), or the game data could
/// not be read.
pub fn objective_preview(conn: &Connection, overrides: &Overrides, req: &PlanRequest) -> Result<ObjectivePreview> {
    let objectives = req.objectives();
    anyhow::ensure!(!objectives.is_empty(), "no objective");
    let start = req.start.as_ref().ok_or_else(|| anyhow::anyhow!("no character"))?;
    let prepared = super::prepare(conn, overrides, req, &|_| {})?;
    let model = &prepared.model;
    let planned: HashSet<i64> = (model.quests.iter())
        .enumerate()
        .filter(|(i, q)| q.zone_forced && !model.initial.turned.contains(i))
        .map(|(_, q)| q.id)
        .collect();
    let parents = area_parents(conn)?;
    let asked = |c: &Candidate| {
        objectives.iter().any(|o| match *o {
            GuideObjective::Quest(id) => id == c.id,
            GuideObjective::Dungeon(area) => c.sort == Some(area),
            GuideObjective::Zone(area) => {
                c.general
                    && c.sort
                        .is_some_and(|s| s > 0 && areas_up(&parents, s).any(|a| a == area))
            }
        })
    };
    let skipped: HashMap<i64, &str> = (model.skipped.iter())
        .map(|(id, _, reason)| (*id, reason.as_str()))
        .collect();
    let excluded = (candidates(conn, start)?.into_iter())
        .filter(|c| asked(c) && !planned.contains(&c.id))
        .map(|c| {
            let reason = match (skipped.get(&c.id), model.index.contains_key(&c.id)) {
                (Some(reason), _) => (*reason).to_owned(),
                (None, true) => "a quest it needs first cannot be planned".to_owned(),
                (None, false) => "not available to this character".to_owned(),
            };
            ExcludedQuest {
                id: c.id,
                name: model.names.get("quest", c.id, &c.name).to_owned(),
                reason,
            }
        })
        .collect();
    Ok(ObjectivePreview {
        quests: planned.len(),
        excluded,
        recommended: model.above_level.first().map(|&(_, level)| level),
    })
}
