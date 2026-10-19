//! The quests a character can do, built from the merged database (`m_*` tables).

mod builder;
mod chains;
mod crowd;
mod dungeons;
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
mod rewards;
mod selection;
mod services;
mod sources;
mod types;

pub(crate) use chains::{class_sort, required_by_name};
pub use farm::{FarmMobs, Met};
pub use guard::Guards;
pub use loot::CommonLoot;
pub use names::Names;
pub use types::{
    Dungeon, DungeonDef, EntityKind, ExploreArea, Guard, Initial, Loc, Model, Objective, ProfessionPlan, Profile,
    Quest, Spots, Teleport, TeleportDef, Vendor, race_bit,
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
    chains::drop_unhanded(&mut quests, &mut skipped);
    report_skipped_class_quests(&sources, &skipped);
    let loot = CommonLoot::load(conn, &mut quests)?;
    let places = Places::load(conn, world, &sources.entities, profile, &sources.start)?;
    chains::narrow_class_quests_to_spells(&mut quests);
    chains::require_prerequisites(&mut quests);
    chains::release_impossible_class_quests(&mut quests);
    dungeons::set_dungeon_levels(&mut sources.dungeons, &quests);
    let index: FxHashMap<i64, usize> = quests.iter().enumerate().map(|(i, q)| (q.id, i)).collect();
    let mut teleports = places.teleports;
    teleports.extend(places::quest_teleports(world, profile, &quests));
    let unlocks = chains::chain_values(&quests, &index, &chains::quest_work(&quests, params));
    let chain_top = chains::chain_tops(&quests, &index);
    let (explore, explore_by_zone) = explore::load_explore(conn, world)?;
    let farm = FarmMobs::load(conn, world, &quests)?;
    let power = crate::power::PowerModel::load(
        conn,
        profile.class_id,
        profile.race_id,
        profile.to_level.max(60),
        &profile.spell_weights,
        params,
    )?;
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
    let services = services::load(conn, world, profile, params)?;
    Ok(Model {
        services,
        quests,
        dungeons: sources.dungeons,
        professions: profile.professions.iter().filter_map(ProfessionPlan::new).collect(),
        index,
        start: sources.start,
        unlocks,
        chain_top,
        trainers: places.trainers,
        inns: places.inns,
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
    let mut quests = Vec::new();
    let mut skipped = Vec::new();
    for row in &sources.rows {
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
