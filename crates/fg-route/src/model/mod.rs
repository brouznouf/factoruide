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
#[cfg(any(test, feature = "testing"))]
pub mod testing;
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
    settle_quests(&mut quests, &sources.dungeons, params);
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
    settle_dungeons(&mut sources.dungeons, &quests, profile);
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
    let graveyards = graveyards(conn, world, &sources)?;
    let mut bundles = vec![Vec::new(); quests.len()];
    for (f, q) in quests.iter().enumerate().filter(|(_, q)| q.pre_need > 1) {
        for &i in q.pre_any.iter().filter_map(|id| index.get(id)) {
            bundles[i].push(f as u32);
        }
    }
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
        graveyards,
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
        bundles,
        dungeon_loot,
        patrols,
    })
}

/// What follows from the quests built, without the data: the class quests that teach a spell, the
/// quests of the dungeons run with all their quests, what a quest needs first, the class quests
/// that cannot be done, and the must-haves as mandatory.
pub(crate) fn settle_quests(quests: &mut [Quest], dungeons: &[Dungeon], params: &Params) {
    chains::narrow_class_quests_to_spells(quests, params.all_class_quests);
    for q in quests.iter_mut() {
        let forced = |d: usize| params.forced_dungeon_quests.contains(&dungeons[d].area);
        q.mandatory |= q.objectives.iter().any(|o| o.dungeon.is_some_and(forced));
    }
    chains::require_prerequisites(quests);
    chains::release_impossible_class_quests(quests);
    for q in quests.iter_mut() {
        q.mandatory |= q.required;
    }
}

/// A dungeon holding an objective of a must-have quest is run like a forced one, and past its
/// levels too: the quest asks for it. A dungeon the player forced stays within its levels, the
/// route being refused when it cannot be run there.
pub(crate) fn settle_dungeons(dungeons: &mut [Dungeon], quests: &[Quest], profile: &Profile) {
    for q in quests.iter().filter(|q| q.required) {
        for d in q.objectives.iter().filter_map(|o| o.dungeon) {
            dungeons[d].needed = true;
        }
    }
    for d in dungeons.iter_mut().filter(|d| d.needed) {
        d.max_level = d.max_level.max(profile.to_level);
    }
}

/// The spirit healers the character can reach: where one dying respawns.
fn graveyards(conn: &Connection, world: &World, sources: &Sources) -> Result<Vec<Loc>> {
    let mut stmt = conn.prepare("SELECT id FROM m_npc WHERE name = 'Spirit Healer'")?;
    let ids = stmt
        .query_map([], |r| r.get::<_, i64>(0))?
        .collect::<rusqlite::Result<Vec<_>>>()?;
    Ok((ids.iter())
        .flat_map(|&id| {
            (sources.entities.points(world, EntityKind::Npc, id).into_iter()).map(move |(pos, zone)| Loc {
                pos,
                zone,
                kind: EntityKind::Npc,
                id,
                name: "Spirit Healer".to_owned(),
            })
        })
        .collect())
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

/// Class quests the planner cannot do are worth knowing about.
fn report_skipped_class_quests(sources: &Sources, skipped: &[(i64, String, String)]) {
    for (id, name, reason) in skipped {
        if sources.class_quest_ids.contains(id) {
            eprintln!("  class quest skipped: {name} ({id}): {reason}");
        }
    }
}
