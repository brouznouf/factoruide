//! The character a request plans for: race, class, the players of its group, and what the
//! configuration says about them (class quests, powers, spells).

use super::StartState;
use super::options::race_classes;
use super::overrides::{Overrides, RaceDef};
use super::request::{GROUP_MAX, GROUP_MIN_LEVEL, PlanRequest};
use crate::model::{Mate, Profile};
use crate::params::Params;
use anyhow::{Context, Result, bail};
use rusqlite::{Connection, OptionalExtension};
use std::collections::HashMap;

pub(super) fn character(conn: &Connection, overrides: &Overrides, req: &PlanRequest) -> Result<(Profile, Params)> {
    let mut params = req.params.clone().unwrap_or_else(|| overrides.params.clone());
    let members = req.members(params.edition.rules());
    check_group(overrides, req, &members)?;
    // A group is planned for the member with the least XP.
    let (race_key, class_key) = match members.first() {
        Some(m) => (race_key_of(overrides, m.race)?, m.class.clone()),
        None => (req.race.to_lowercase().replace([' ', '-'], ""), req.class.clone()),
    };
    let Some(race) = overrides.races.get(&race_key) else {
        bail!("unknown race {}", req.race);
    };
    let (class_id, class_name) = class_of(conn, &class_key)?;
    check_playable(conn, race, &race_key, class_id, &class_name)?;
    let mates: Vec<Mate> = (members.iter().skip(1))
        .map(|m| {
            let (class_id, class_name) = class_of(conn, &m.class)?;
            Ok(Mate {
                name: m.name.clone(),
                race_id: m.race,
                class_id,
                spell_weights: (overrides.spells.get(&class_name.to_lowercase()))
                    .map(|w| w.iter().map(|(k, v)| (k.clone(), v.clone())).collect())
                    .unwrap_or_default(),
            })
        })
        .collect::<Result<_>>()?;
    let group: Vec<String> = members.iter().skip(1).map(|m| m.class.to_lowercase()).collect();
    params.group_size = 1 + mates.len() as i64;
    let class = class_name.to_lowercase();
    // The class's curve and gear dependence, or the player's own (advanced settings).
    let power = overrides.class_power.get(&class).cloned().unwrap_or_default();
    let points = if params.power_curve.is_empty() {
        power.points()?
    } else {
        params.power_curve.clone()
    };
    params.class_curve = super::overrides::curve_by_level(&points, req.to_level.max(100));
    params.gear_dependence = params.power_gear.or(power.gear).unwrap_or(1.0);
    // A group takes on what its weakest class does at each level: the higher mobs, the more
    // that one misses.
    if params.power_curve.is_empty() {
        for other in &group {
            let points = overrides.class_power.get(other).cloned().unwrap_or_default().points()?;
            let curve = super::overrides::curve_by_level(&points, req.to_level.max(100));
            for (mine, theirs) in params.class_curve.iter_mut().zip(curve) {
                *mine = mine.min(theirs);
            }
        }
    }
    let start = members.first().copied().or(req.start.as_ref());
    let from_level = start.map_or(req.from_level, |s| s.level);
    if from_level >= req.to_level {
        bail!(
            "the character is level {from_level}: nothing to plan up to level {}",
            req.to_level
        );
    }
    let mut professions = req.professions.clone();
    if let Some(start) = start {
        start.anchor_professions(&mut professions);
    }
    let mut completed_quests: std::collections::HashSet<_> =
        start.map(|s| s.completed.iter().copied().collect()).unwrap_or_default();
    if members.len() > 1 {
        completed_quests.extend(members.iter().flat_map(|m| m.completed.iter()).copied());
        let mut scopes =
            conn.prepare("SELECT coalesce(races_mask, 0), coalesce(classes_mask, 0) FROM m_quest WHERE id = ?1")?;
        for id in completed_quests.clone() {
            let scope = scopes
                .query_row([id], |r| Ok((r.get::<_, i64>(0)?, r.get::<_, i64>(1)?)))
                .optional()?;
            if members
                .iter()
                .zip(std::iter::once(class_id).chain(mates.iter().map(|m| m.class_id)))
                .any(|(mate, class)| {
                    let eligible = scope.is_none_or(|(races, classes)| {
                        (races == 0 || races & crate::model::race_bit(mate.race) != 0)
                            && (classes == 0 || classes & (1 << (class - 1)) != 0)
                    });
                    eligible && !mate.completed.contains(&id)
                })
            {
                completed_quests.remove(&id);
            }
        }
    }
    let profile = Profile {
        name: req.name.clone().unwrap_or_else(|| format!("{race_key}-{class}")),
        race_id: race.id,
        class_id,
        class_name: class_name.clone(),
        faction: race.faction,
        start_npc: race.start_npc,
        from_level,
        to_level: req.to_level,
        required_class_quests: required_class_quests(
            &super::options::class_quests(conn, overrides)?,
            req,
            &class,
            &group,
        ),
        required_quests: vec![],
        completed_quests,
        class_powers: overrides.class_powers.get(&class).cloned().unwrap_or_default(),
        spell_weights: overrides
            .spells
            .get(&class)
            .map(|m| m.iter().map(|(k, v)| (k.clone(), v.clone())).collect())
            .unwrap_or_default(),
        dungeons: overrides.dungeons.clone(),
        quest_overrides: overrides.quest_overrides.clone(),
        objective_orders: overrides.objective_orders.clone(),
        quest_teleports: overrides
            .teleports
            .iter()
            .filter(|t| t.class == class)
            .cloned()
            .collect(),
        professions,
        locale: req.locale.clone(),
        player: if mates.is_empty() {
            String::new()
        } else {
            members[0].name.clone()
        },
        mates,
    };
    Ok((profile, params))
}

/// The members of a group guide must be of the same faction, `GROUP_MAX` at most and each once.
/// Each is at least level `GROUP_MIN_LEVEL`, unless they all start in the same zone (a group of
/// new characters leveling together from the start).
fn check_group(overrides: &Overrides, req: &PlanRequest, members: &[&StartState]) -> Result<()> {
    if req.mates.is_empty() {
        return Ok(());
    }
    if req.start.is_none() {
        bail!("a group guide starts from the characters as the addon recorded them");
    }
    if members.len() > GROUP_MAX {
        bail!("a group has {GROUP_MAX} players at most");
    }
    let zone = |m: &StartState| m.position.as_ref().map(|p| p.map);
    let together = zone(members[0]).is_some() && members.iter().all(|m| zone(m) == zone(members[0]));
    let mut faction = None;
    for (k, m) in members.iter().enumerate() {
        if m.level < GROUP_MIN_LEVEL && !together {
            bail!(
                "{} is level {}: below level {GROUP_MIN_LEVEL}, the players of a group guide must all start in the same zone",
                m.name,
                m.level
            );
        }
        if members[..k].iter().any(|o| o.name == m.name) {
            bail!("{} is twice in the group", m.name);
        }
        let f = overrides.races[&race_key_of(overrides, m.race)?].faction;
        if faction.is_some_and(|g| g != f) {
            bail!("the players of a group guide are all of the same faction");
        }
        faction = Some(f);
    }
    Ok(())
}

/// Key in `races.toml` of a race ID.
fn race_key_of(overrides: &Overrides, race_id: i64) -> Result<String> {
    (overrides.races.iter())
        .find(|(_, r)| r.id == race_id)
        .map(|(k, _)| k.clone())
        .with_context(|| format!("unknown race ID {race_id}"))
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
fn check_playable(conn: &Connection, race: &RaceDef, race_key: &str, class_id: i64, class_name: &str) -> Result<()> {
    let allowed = race_classes(conn, race.id)?;
    let class_key: String = conn.query_row(
        "SELECT lower(Filename) FROM client_chrclasses WHERE ID = ?1",
        [class_id],
        |r| r.get(0),
    )?;
    if !allowed.is_empty() && !allowed.contains(&class_key) {
        bail!(
            "{} cannot be {} (playable: {})",
            race_key,
            class_name,
            allowed.join(", ")
        );
    }
    Ok(())
}

/// The character's class quests (as requested, or the configured ones), then each companion's.
fn required_class_quests(
    defaults: &HashMap<String, Vec<String>>,
    req: &PlanRequest,
    class: &str,
    group: &[String],
) -> Vec<String> {
    let mut quests = req
        .required_class_quests
        .clone()
        .unwrap_or_else(|| defaults.get(class).cloned().unwrap_or_default());
    for name in group {
        for q in defaults.get(name).into_iter().flatten() {
            if !quests.contains(q) {
                quests.push(q.clone());
            }
        }
    }
    quests
}
