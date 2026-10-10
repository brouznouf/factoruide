//! The character a request plans for: race, class, the classes leveling with it, and what the
//! configuration says about them (class quests, powers, spells).

use super::options::race_classes;
use super::overrides::{Overrides, RaceDef};
use super::request::PlanRequest;
use crate::model::Profile;
use crate::params::Params;
use anyhow::{Context, Result, bail};
use rusqlite::Connection;

pub(super) fn character(conn: &Connection, overrides: &Overrides, req: &PlanRequest) -> Result<(Profile, Params)> {
    let race_key = req.race.to_lowercase().replace([' ', '-'], "");
    let Some(race) = overrides.races.get(&race_key) else {
        bail!("unknown race {}", req.race);
    };
    let (class_id, class_name) = class_of(conn, &req.class)?;
    check_playable(conn, race, req, class_id, &class_name)?;
    let group: Vec<(i64, String)> = req
        .group
        .iter()
        .take(4)
        .map(|c| class_of(conn, c))
        .collect::<Result<_>>()?;
    let mut params = req.params.clone().unwrap_or_else(|| overrides.params.clone());
    params.group_size = 1 + group.len() as i64;
    let class = class_name.to_lowercase();
    let profile = Profile {
        name: req.name.clone().unwrap_or_else(|| format!("{race_key}-{class}")),
        race_id: race.id,
        class_id,
        class_name: class_name.clone(),
        faction: race.faction,
        start_npc: race.start_npc,
        from_level: req.from_level,
        to_level: req.to_level,
        required_class_quests: required_class_quests(overrides, req, &class, &group),
        class_powers: overrides.class_powers.get(&class).cloned().unwrap_or_default(),
        spell_weights: overrides
            .spells
            .get(&class)
            .map(|m| m.iter().map(|(k, v)| (k.clone(), v.clone())).collect())
            .unwrap_or_default(),
        dungeons: overrides.dungeons.clone(),
        professions: req.professions.clone(),
        locale: req.locale.clone(),
        group_class_ids: group.iter().map(|(id, _)| *id).collect(),
    };
    Ok((profile, params))
}

/// Class ID and name from a class file name or name (mage, WARRIOR...).
fn class_of(conn: &Connection, class: &str) -> Result<(i64, String)> {
    conn.query_row(
        "SELECT ID, Name_male_lang FROM client_chrclasses WHERE lower(Filename) = lower(?1) OR lower(Name_male_lang) = lower(?1)",
        [class],
        |r| Ok((r.get(0)?, r.get(1)?)),
    )
    .with_context(|| format!("unknown class {class}"))
}

/// Only the combinations the game offers (when the client data lists them).
fn check_playable(conn: &Connection, race: &RaceDef, req: &PlanRequest, class_id: i64, class_name: &str) -> Result<()> {
    let allowed = race_classes(conn, race.id)?;
    let class_key: String = conn.query_row(
        "SELECT lower(Filename) FROM client_chrclasses WHERE ID = ?1",
        [class_id],
        |r| r.get(0),
    )?;
    if !allowed.is_empty() && !allowed.contains(&class_key) {
        bail!(
            "{} cannot be {} (playable: {})",
            req.race,
            class_name,
            allowed.join(", ")
        );
    }
    Ok(())
}

/// The character's class quests (as requested, or the configured ones), then each companion's.
fn required_class_quests(
    overrides: &Overrides,
    req: &PlanRequest,
    class: &str,
    group: &[(i64, String)],
) -> Vec<String> {
    let mut quests = req
        .required_class_quests
        .clone()
        .unwrap_or_else(|| overrides.class_quests.get(class).cloned().unwrap_or_default());
    for (_, name) in group {
        for q in overrides.class_quests.get(&name.to_lowercase()).into_iter().flatten() {
            if !quests.contains(q) {
                quests.push(q.clone());
            }
        }
    }
    quests
}
