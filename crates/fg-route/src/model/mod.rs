//! The quests a character can do, built from the merged database (`m_*` tables).

mod builder;
mod chains;
mod crowd;
mod dungeons;
pub use dungeons::dungeon_levels;
mod entities;
mod explore;
mod farm;
mod givers;
mod guard;
mod item;
mod loot;
mod names;
mod objectives;
mod places;
mod pvp;
mod quest;
mod required;
mod rewards;
mod selection;
mod sources;
mod types;

pub(crate) use chains::{class_sort, required_by_name};
pub use farm::{FarmMobs, Met};
pub use givers::{Dropper, UNKNOWN_START_CHANCE, assumed_start_chance};
pub use guard::Guards;
pub use loot::CommonLoot;
pub use names::Names;
pub use types::{
    Dungeon, DungeonDef, EntityKind, ExploreArea, Guard, Initial, Loc, Mate, MateStart, Model, Objective,
    ObjectiveOrder, ProfessionPlan, Profile, Quest, QuestOverride, Spots, Teleport, TeleportDef, Vendor, race_bit,
};

use crate::params::Params;
use crate::world::World;
use anyhow::Result;
use builder::QuestBuilder;
use places::Places;
use rusqlite::Connection;
use rustc_hash::FxHashMap;
use sources::Sources;
use std::collections::HashMap;

pub fn load(conn: &Connection, world: &World, profile: &Profile, params: &Params) -> Result<Model> {
    let mut sources = Sources::read(conn, world, profile, params)?;
    let (mut quests, mut skipped) = build_quests(&sources, world, profile, params);
    required::apply(&mut quests, &sources.rows, &skipped, profile, params)?;
    chains::drop_unhanded(&mut quests, &mut skipped);
    report_skipped_class_quests(&sources, &skipped);
    let places = Places::load(conn, world, &sources.entities, profile, &sources.start, params.edition)?;
    chains::narrow_class_quests_to_spells(&mut quests, params.all_class_quests);
    // The quests of the dungeons run with all their quests.
    for q in &mut quests {
        let forced = |d: usize| params.forced_dungeon_quests.contains(&sources.dungeons[d].area);
        q.mandatory |= q.objectives.iter().any(|o| o.dungeon.is_some_and(forced));
    }
    chains::require_prerequisites(&mut quests);
    chains::release_impossible_class_quests(&mut quests);
    for q in &mut quests {
        q.mandatory |= q.required;
    }
    let loot = CommonLoot::load(conn, &mut quests)?;
    let levels = dungeon_levels(conn, &profile.dungeons)?;
    let window = (params.dungeon_from, params.dungeon_to);
    dungeons::set_dungeon_levels(
        &mut sources.dungeons,
        &quests,
        &levels,
        window,
        params.edition.rules().max_level,
    );
    let index: FxHashMap<i64, usize> = quests.iter().enumerate().map(|(i, q)| (q.id, i)).collect();
    let mut teleports = places.teleports;
    teleports.extend(places::quest_teleports(world, profile, &quests));
    let unlocks = chains::chain_values(&quests, &index, &chains::quest_work(&quests, params));
    let chain_top = chains::chain_tops(&quests, &index);
    let (explore, explore_by_zone) = explore::load_explore(conn, world)?;
    let farm = FarmMobs::load(conn, world, &quests)?;
    let abilities = crate::abilities::Abilities::load(conn, profile)?;
    let power = crate::power::PowerModel::load(
        conn,
        profile.class_id,
        profile.race_id,
        profile.to_level.max(60),
        &profile.spell_weights,
        params,
        &abilities,
    )?;
    // A group's other classes: the spells each one learns (the addon of each player trains its own).
    let mut mate_spells = Vec::new();
    for m in &profile.mates {
        let abilities = crate::abilities::Abilities::load(
            conn,
            &Profile {
                race_id: m.race_id,
                class_id: m.class_id,
                spell_weights: m.spell_weights.clone(),
                ..profile.clone()
            },
        )?;
        let mate = crate::power::PowerModel::load(
            conn,
            m.class_id,
            m.race_id,
            profile.to_level.max(60),
            &m.spell_weights,
            params,
            &abilities,
        )?;
        mate_spells.push(
            mate.map(|p| p.ranks().map(|r| r.id).chain(abilities.spells()).collect())
                .unwrap_or_default(),
        );
    }
    let patrols = (sources.entities.paths.keys())
        .map(|&id| (id, sources.entities.path(world, id)))
        .filter(|(_, path)| path.len() >= 2)
        .collect();
    let (rewards, dungeon_loot) = match &power {
        Some(p) => rewards::load_rewards(conn, p, &quests, sources.dungeons.len(), &sources.inside, params)?,
        None => (
            vec![(Vec::new(), Vec::new()); quests.len()],
            vec![Vec::new(); sources.dungeons.len()],
        ),
    };
    Ok(Model {
        abilities,
        quests,
        dungeons: sources.dungeons,
        professions: (profile.profession_goals().into_iter())
            .filter_map(|(goal, members)| {
                Some(ProfessionPlan {
                    members,
                    ..ProfessionPlan::new(goal)?
                })
            })
            .collect(),
        index,
        start: sources.start,
        unlocks,
        chain_top,
        trainers: places.trainers,
        inns: places.inns,
        vendors: places.vendors,
        start_inn: places.start_inn,
        teleports,
        recall: places.recall,
        skipped,
        initial: Initial::default(),
        names: Names::load(conn, profile.locale.as_deref().unwrap_or("enUS"))?,
        faction_names: faction_names(conn, params),
        explore,
        explore_by_zone,
        farm,
        loot,
        power,
        mate_spells,
        rewards,
        dungeon_loot,
        patrols,
    })
}

/// Names of the factions of the reputation goals.
fn faction_names(conn: &Connection, params: &Params) -> HashMap<i64, String> {
    let mut out = HashMap::new();
    for goal in &params.reputation_goals {
        let name: Option<String> = conn
            .query_row(
                "SELECT Name_lang FROM client_faction WHERE ID = ?1",
                [goal.faction],
                |r| r.get(0),
            )
            .ok();
        out.insert(
            goal.faction,
            name.unwrap_or_else(|| format!("Faction {}", goal.faction)),
        );
    }
    out
}

/// Every quest the character can do, and the others with why they are left out.
fn build_quests(
    sources: &Sources,
    world: &World,
    profile: &Profile,
    params: &Params,
) -> (Vec<Quest>, Vec<(i64, String, String)>) {
    let builder = QuestBuilder::new(sources, world, profile, params);
    // The quests the player asked for are done even when their mobs are elites.
    let with_elites = Params {
        allow_elite: true,
        ..params.clone()
    };
    let asked = QuestBuilder::new(sources, world, profile, &with_elites);
    let mut quests = Vec::new();
    let mut skipped = Vec::new();
    for row in &sources.rows {
        let builder = if profile.required_quests.contains(&row.id) {
            &asked
        } else {
            &builder
        };
        match builder.build(row) {
            // The objectives done are a mask of 8 bits (`State::objectives`).
            Ok(Some(quest)) if quest.objectives.len() > 8 => {
                skipped.push((row.id, row.name.clone(), "more than 8 objectives".to_owned()));
            }
            Ok(Some(quest)) => quests.push(quest),
            Ok(None) => {}
            Err(reason) => skipped.push((row.id, row.name.clone(), reason.to_owned())),
        }
    }
    (quests, skipped)
}

/// Class quests the planner cannot do are worth knowing about (not the repeatable ones that only
/// hand an item again, like the paladin's tome).
fn report_skipped_class_quests(sources: &Sources, skipped: &[(i64, String, String)]) {
    for (id, name, reason) in skipped {
        if sources.class_quest_ids.contains(id) && !sources.repeatable.contains(id) {
            eprintln!("  class quest skipped: {name} ({id}): {reason}");
        }
    }
}
