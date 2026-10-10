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
mod names;
mod objectives;
mod places;
mod pvp;
mod quest;
mod rewards;
mod selection;
mod sources;
mod types;

pub use farm::FarmMobs;
pub use guard::Guards;
pub use names::Names;
pub use types::{
    Dungeon, DungeonDef, EntityKind, ExploreArea, Guard, Initial, Loc, Model, Objective, ProfessionPlan, Profile,
    Quest, Spots, race_bit,
};

use crate::params::Params;
use crate::world::World;
use anyhow::Result;
use builder::QuestBuilder;
use places::Places;
use rusqlite::Connection;
use sources::Sources;
use std::collections::HashMap;

pub fn load(conn: &Connection, world: &World, profile: &Profile, params: &Params) -> Result<Model> {
    let mut sources = Sources::read(conn, world, profile, params)?;
    let (mut quests, skipped) = build_quests(&sources, world, profile, params);
    report_skipped_class_quests(&sources, &skipped);
    let places = Places::load(conn, world, &sources.entities, profile, &sources.start)?;
    chains::require_prerequisites(&mut quests);
    chains::release_impossible_class_quests(&mut quests);
    dungeons::set_dungeon_levels(&mut sources.dungeons, &quests);
    let index: HashMap<i64, usize> = quests.iter().enumerate().map(|(i, q)| (q.id, i)).collect();
    let unlocks = chains::chain_values(&quests, &index, &chains::quest_work(&quests, params));
    let chain_top = chains::chain_tops(&quests, &index);
    let (explore, explore_by_zone) = explore::load_explore(conn, world)?;
    let farm = FarmMobs::load(conn, world)?;
    let power = crate::power::PowerModel::load(
        conn,
        profile.class_id,
        profile.race_id,
        profile.to_level.max(60),
        &profile.spell_weights,
        params,
    )?;
    let (rewards, dungeon_loot) = match &power {
        Some(p) => rewards::load_rewards(conn, p, &quests, sources.dungeons.len(), &sources.inside, params)?,
        None => (
            vec![(Vec::new(), Vec::new()); quests.len()],
            vec![Vec::new(); sources.dungeons.len()],
        ),
    };
    Ok(Model {
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
        skipped,
        initial: Initial::default(),
        names: Names::load(conn, profile.locale.as_deref().unwrap_or("enUS"))?,
        explore,
        explore_by_zone,
        farm,
        power,
        rewards,
        dungeon_loot,
    })
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
