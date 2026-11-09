//! The character a request plans for: race, class, the players of its group, and what the
//! configuration says about them (class quests, powers, spells).

use super::StartState;
use super::options::race_classes;
use super::overrides::{Overrides, RaceDef};
use super::request::PlanRequest;
use super::{GROUP_MAX, GROUP_MIN_LEVEL, RequestError};
use crate::model::{Mate, Profile};
use crate::params::Params;
use anyhow::{Context, Result};
use rusqlite::{Connection, OptionalExtension};
use std::collections::HashMap;

pub(crate) fn character(conn: &Connection, overrides: &Overrides, req: &PlanRequest) -> Result<(Profile, Params)> {
    let mut params = req.params.clone().unwrap_or_else(|| overrides.params.clone());
    let objectives = req.objectives();
    if !objectives.is_empty() {
        params.for_objective_guide(&objectives);
    }
    let members = req.members(params.edition.rules());
    check_group(overrides, req, &members)?;
    // A group is planned for the member with the least XP.
    let (race_key, class_key) = match members.first() {
        Some(m) => (race_key_of(overrides, m.race)?, m.class.clone()),
        None => (req.race.to_lowercase().replace([' ', '-'], ""), req.class.to_string()),
    };
    let Some(race) = overrides.races.get(&race_key) else {
        return Err(RequestError::UnknownRace(req.race.to_string()).into());
    };
    let (class_id, class_name) = class_of(conn, &class_key)?;
    check_playable(conn, race, &race_key, class_id, &class_name)?;
    // Each player's professions, from where its profile has them.
    let professions_of = |m: &StartState| {
        let mut goals = req.professions_of(&m.name).to_vec();
        m.anchor_professions(&mut goals);
        goals
    };
    let mates: Vec<Mate> = (members.iter().skip(1))
        .map(|m| {
            let (class_id, class_name) = class_of(conn, &m.class)?;
            Ok(Mate {
                class_name: class_name.clone(),
                name: m.name.clone(),
                race_id: m.race,
                class_id,
                professions: professions_of(m),
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
    (params.class_curve, params.gear_dependence) = class_power(overrides, &class, &params, req.to_level.max(100))?;
    // A group takes on what its weakest class does at each level: the higher mobs, the more
    // that one misses.
    if params.power_curve.is_empty() {
        overrides.weakest_curve(&mut params.class_curve, &group, req.to_level.max(100))?;
    }
    let start = members.first().copied().or(req.start.as_ref());
    let from_level = start.map_or(req.from_level, |s| s.level);
    let to_level = if !objectives.is_empty() {
        // No level to reach: the guide ends with its quests.
        from_level
    } else if from_level >= req.to_level {
        return Err(RequestError::NothingToPlan {
            from: from_level,
            to: req.to_level,
        }
        .into());
    } else {
        req.to_level
    };
    let professions = if objectives.is_empty() {
        start.map_or_else(|| req.professions.clone(), professions_of)
    } else {
        Vec::new()
    };
    let mut completed_quests: std::collections::HashSet<_> = start
        .map(|s| s.completed.iter().map(|q| q.0).collect())
        .unwrap_or_default();
    if members.len() > 1 {
        completed_quests.extend(members.iter().flat_map(|m| m.completed.iter()).map(|q| q.0));
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
                    eligible && !mate.completed.contains(&super::QuestId(id))
                })
            {
                completed_quests.remove(&id);
            }
        }
    }
    let required_class_quests = if objectives.is_empty() {
        required_class_quests(&super::options::class_quests(conn, overrides)?, req, &class, &group)
    } else {
        Vec::new()
    };
    let class_ids: Vec<i64> = std::iter::once(class_id)
        .chain(mates.iter().map(|m| m.class_id))
        .collect();
    let class_requirements = crate::model::requirements::select(
        crate::model::requirements::load(conn, &class_ids)?,
        &required_class_quests,
    );
    let profile = Profile {
        name: req.name.clone().unwrap_or_else(|| format!("{race_key}-{class}")),
        race_id: race.id,
        class_id,
        class_name,
        faction: race.faction,
        start_npc: race.start_npc,
        from_level,
        to_level,
        required_class_quests,
        class_requirements,
        required_quests: vec![],
        completed_quests,
        class_powers: overrides.class_powers.get(&class).cloned().unwrap_or_default(),
        spell_weights: overrides
            .spells
            .get(&class)
            .map(|m| m.iter().map(|(k, v)| (k.clone(), v.clone())).collect())
            .unwrap_or_default(),
        abilities: overrides
            .abilities
            .iter()
            .filter(|a| a.class == class)
            .cloned()
            .collect(),
        dungeons: overrides.dungeons.clone(),
        quest_overrides: overrides.quest_overrides.clone(),
        objective_orders: overrides.objective_orders.clone(),
        quest_teleports: overrides.teleports.clone(),
        professions,
        locale: req.locale.clone(),
        player: if mates.is_empty() {
            String::new()
        } else {
            members[0].name.clone()
        },
        mates,
        objectives,
    };
    Ok((profile, params))
}

/// The members of a group guide must be of the same faction, `GROUP_MAX` at most and each once.
/// Each is at least level `GROUP_MIN_LEVEL`, unless they all start in the same zone (a group of
/// new characters leveling together from the start, at their races' starting place when the
/// addon never saw them).
fn check_group(overrides: &Overrides, req: &PlanRequest, members: &[&StartState]) -> Result<()> {
    if req.mates.is_empty() {
        return Ok(());
    }
    if req.start.is_none() {
        return Err(RequestError::GroupWithoutProfiles.into());
    }
    if members.len() > GROUP_MAX {
        return Err(RequestError::GroupTooBig.into());
    }
    // Where each starts: the map the addon saw it on, or a new character's race's starting NPC.
    let zone = |m: &StartState| match &m.position {
        Some(p) => Some((true, p.map)),
        None => (overrides.races.values())
            .find(|r| r.id == m.race)
            .map(|r| (false, r.start_npc)),
    };
    let together = zone(members[0]).is_some() && members.iter().all(|m| zone(m) == zone(members[0]));
    let mut faction = None;
    for (k, m) in members.iter().enumerate() {
        if m.level < GROUP_MIN_LEVEL && !together {
            return Err(RequestError::GroupMemberTooLow {
                name: m.name.clone(),
                level: m.level,
            }
            .into());
        }
        if members[..k].iter().any(|o| o.name == m.name) {
            return Err(RequestError::GroupDuplicate(m.name.clone()).into());
        }
        let f = overrides.races[&race_key_of(overrides, m.race)?].faction;
        if faction.is_some_and(|g| g != f) {
            return Err(RequestError::GroupMixedFactions.into());
        }
        faction = Some(f);
    }
    Ok(())
}

/// The power curve (by level up to `top`) and gear dependence of a class (lowercase), or the
/// player's own (advanced settings).
fn class_power(overrides: &Overrides, class: &str, params: &Params, top: i64) -> Result<(Vec<f64>, f64)> {
    let power = overrides.class_power.get(class).cloned().unwrap_or_default();
    let points = if params.power_curve.is_empty() {
        power.points()?
    } else {
        params.power_curve.clone()
    };
    Ok((
        super::overrides::curve_by_level(&points, top),
        params.power_gear.or(power.gear).unwrap_or(1.0),
    ))
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
        return Err(RequestError::ClassNotPlayable {
            race: race_key.to_owned(),
            class: class_name.to_owned(),
            playable: allowed,
        }
        .into());
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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::job::MapPos;
    use crate::xp::Edition;

    fn player(name: &str, race: i64, class: &str, level: i64) -> StartState {
        StartState {
            name: name.into(),
            race,
            class: class.to_uppercase(),
            level,
            ..StartState::default()
        }
    }

    /// Why a group is refused, or an empty string.
    fn refusal(start: StartState, mates: Vec<StartState>) -> String {
        let dir = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../overrides");
        let overrides = Overrides::load_edition(&dir, Edition::Classic).unwrap();
        let req = PlanRequest {
            start: Some(start),
            mates,
            ..serde_json::from_value(serde_json::json!({
                "race": "human", "class": "warrior", "from_level": 1, "to_level": 30,
            }))
            .unwrap()
        };
        let members = req.members(overrides.params.edition.rules());
        check_group(&overrides, &req, &members)
            .err()
            .map(|e| e.to_string())
            .unwrap_or_default()
    }

    /// The class power curve and gear dependence of the advanced settings replace the class's.
    #[test]
    fn own_power_curve_replaces_the_class_one() {
        let dir = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../overrides");
        let overrides = Overrides::load_edition(&dir, Edition::Classic).unwrap();
        let (curve, gear) = class_power(&overrides, "warrior", &overrides.params, 100).unwrap();
        assert!(curve[15] < 0.0 && (gear - 1.5).abs() < 1e-9);
        let own = overrides
            .params
            .with_overrides(&[
                "power_curve=[[10, 1.0], [30, 2.0]]".to_owned(),
                "power_gear=0.5".to_owned(),
            ])
            .unwrap();
        let (curve, gear) = class_power(&overrides, "warrior", &own, 100).unwrap();
        assert!((curve[5] - 1.0).abs() < 1e-9);
        assert!((curve[20] - 1.5).abs() < 1e-9);
        assert!((gear - 0.5).abs() < 1e-9);
    }

    /// A group guide needs every player at level 10, or all in one zone, and of one faction,
    /// five players at most.
    #[test]
    fn group_guides_have_rules() {
        // Below level 10 it needs the same zone for everyone.
        let at = |mut p: StartState, map: i64| {
            p.position = Some(MapPos { map, x: 50.0, y: 50.0 });
            p
        };
        let low = refusal(at(player("A", 1, "warrior", 12), 1429), vec![player("B", 1, "mage", 9)]);
        assert!(low.contains("level 10"), "{low}");
        // New characters the addon never saw start at their race's starting place: an orc and a
        // troll together in the Valley of Trials, not a human and a dwarf.
        let new = refusal(player("A", 2, "warrior", 1), vec![player("B", 8, "mage", 1)]);
        assert!(new.is_empty(), "{new}");
        let far = refusal(player("A", 1, "warrior", 1), vec![player("B", 3, "mage", 1)]);
        assert!(far.contains("same zone"), "{far}");
        let apart = refusal(
            at(player("A", 1, "warrior", 5), 1429),
            vec![at(player("B", 1, "mage", 6), 1426)],
        );
        assert!(apart.contains("same zone"), "{apart}");
        let together = refusal(
            at(player("A", 1, "warrior", 5), 1429),
            vec![at(player("B", 1, "mage", 6), 1429)],
        );
        assert!(together.is_empty(), "new characters together in Elwynn: {together}");
        let horde = refusal(player("A", 1, "warrior", 12), vec![player("B", 2, "warrior", 12)]);
        assert!(horde.contains("faction"), "{horde}");
        let six = refusal(
            player("A", 1, "warrior", 12),
            (0..5).map(|k| player(&format!("B{k}"), 1, "mage", 12)).collect(),
        );
        assert!(six.contains("at most"), "{six}");
        let twice = refusal(player("A", 1, "warrior", 12), vec![player("A", 1, "warrior", 12)]);
        assert!(twice.contains("twice"), "{twice}");
    }
}
