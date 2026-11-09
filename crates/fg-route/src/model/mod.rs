//! The quests a character can do, built from the merged database (`m_*` tables).

mod builder;
mod chains;
mod crowd;
mod derive;
mod dungeons;
pub use dungeons::dungeon_levels;
mod entities;
mod explore;
mod farm;
#[cfg(any(test, feature = "testing"))]
pub mod fixture;
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
pub mod requirements;
mod rewards;
mod selection;
mod sources;
#[cfg(any(test, feature = "testing"))]
pub mod testing;
mod types;

pub(crate) use chains::class_sort;
pub use farm::{FarmMobs, Met};
pub use givers::{Dropper, UNKNOWN_START_CHANCE, assumed_start_chance};
pub use guard::Guards;
pub use loot::CommonLoot;
pub use names::Names;
pub use types::{
    ClassRequirement, CompletionWindow, Dungeon, DungeonDef, EntityKind, ExploreArea, FloorPoint, Guard,
    GuideObjective, Initial, Loc, Mate, MateStart, Model, Objective, ObjectiveOrder, Pool, ProfessionPlan,
    ProfessionTrainer, Profile, Quest, QuestOverride, QuestRequirement, Spots, Teleport, TeleportDef, Vendor, race_bit,
};

use crate::params::Params;
use crate::world::World;
use anyhow::Result;
use builder::QuestBuilder;
use places::Places;
use rusqlite::Connection;
use sources::Sources;
use std::collections::{HashMap, HashSet};

pub fn load(
    conn: &Connection,
    world: &World,
    profile: &Profile,
    params: &Params,
    starts: &[&crate::job::StartState],
) -> Result<Model> {
    let abilities = crate::abilities::Abilities::load(conn, profile)?;
    let mut mate_abilities = Vec::new();
    for m in &profile.mates {
        mate_abilities.push(crate::abilities::Abilities::load(
            conn,
            &Profile {
                race_id: m.race_id,
                class_id: m.class_id,
                class_name: m.class_name.clone(),
                spell_weights: m.spell_weights.clone(),
                abilities: Vec::new(),
                ..profile.clone()
            },
        )?);
    }
    let mut knowledge = HashMap::<i64, (bool, bool)>::new();
    for (i, book) in std::iter::once(&abilities).chain(&mate_abilities).enumerate() {
        let start = starts.get(i).copied();
        let known = book.initial(start, start.map_or(profile.from_level, |s| s.level));
        for (quest, learned, unique) in book.reward_knowledge(known) {
            let entry = knowledge.entry(quest).or_insert((true, true));
            entry.0 &= learned;
            entry.1 &= unique || start.is_some_and(|s| s.completed.contains(&crate::job::QuestId(quest)));
        }
    }
    let reward_quests: HashSet<_> = knowledge.keys().copied().collect();
    let known_prerequisites: HashSet<_> = knowledge
        .iter()
        .filter_map(|(&q, &(_, known))| known.then_some(q))
        .collect();
    let known_rewards: HashSet<_> = knowledge
        .into_iter()
        .filter_map(|(q, (known, _))| known.then_some(q))
        .collect();
    let logged: HashSet<_> = starts.iter().flat_map(|s| s.log.iter().map(|q| q.id.0)).collect();
    let finished: HashSet<_> = starts.iter().flat_map(|s| s.completed.iter().map(|q| q.0)).collect();
    let skip_known: HashSet<_> = known_rewards
        .iter()
        .copied()
        .filter(|q| !logged.contains(q) && !finished.contains(q))
        .collect();
    let mut sources = Sources::read(
        conn,
        world,
        profile,
        params,
        &skip_known,
        &known_prerequisites,
        &reward_quests,
    )?;
    let (mut quests, mut skipped) = build_quests(&sources, world, profile, params);
    let mut known = derive::Knowledge {
        rewards: known_rewards,
        prerequisites: known_prerequisites,
        reward_quests,
        logged,
        class_quests: std::mem::take(&mut sources.class_quest_ids),
        assumed: starts.is_empty(),
    };
    let levels = dungeon_levels(conn, &profile.dungeons)?;
    let known_names = derive::settle(
        &mut quests,
        &mut skipped,
        &mut sources.dungeons,
        &sources.rows,
        &mut known,
        &levels,
        profile,
        params,
    )?;
    sources.class_quest_ids = known.class_quests;
    let known_prerequisites = known.prerequisites;
    report_skipped_class_quests(&sources, &skipped);
    let places = Places::load(
        conn,
        world,
        &sources.entities,
        profile,
        &sources.start,
        params.edition,
        params.profession_trainers && !profile.profession_goals().is_empty(),
    )?;
    let loot = CommonLoot::load(conn, &mut quests)?;
    let derive::Indexes {
        index,
        unlocks,
        chain_top,
        bundles,
    } = derive::indexes(&quests, params);
    let mut teleports = places.teleports;
    teleports.extend(places::quest_teleports(
        world,
        profile,
        &quests,
        &known_names,
        &abilities,
    ));
    let (explore, explore_by_zone) = explore::load_explore(conn, world)?;
    let farm = FarmMobs::load(conn, world, &quests)?;
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
    for (m, abilities) in profile.mates.iter().zip(&mate_abilities) {
        let mate = crate::power::PowerModel::load(
            conn,
            m.class_id,
            m.race_id,
            profile.to_level.max(60),
            &m.spell_weights,
            params,
            abilities,
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
    let own_knowledge = abilities.initial(starts.first().copied(), profile.from_level);
    let known_class_powers = profile
        .class_powers
        .iter()
        .enumerate()
        .filter(|(_, (name, _))| {
            // The quests of the requirement of that name (a pet, a form) give the power.
            (profile.class_requirements.iter())
                .filter(|c| &c.name == name)
                .flat_map(|c| &c.quests)
                .any(|id| abilities.quest(*id) & own_knowledge != 0)
        })
        .fold(0, |bits, (k, _)| bits | (1 << k));
    Ok(Model {
        known_class_powers,
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
        profession_trainers: places.profession_trainers,
        inns: places.inns,
        vendors: places.vendors,
        start_inn: places.start_inn,
        teleports,
        graveyards,
        recall: places.recall,
        skipped,
        initial: Initial::default(),
        known_class_prerequisites: known_prerequisites,
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
        above_level: Vec::new(),
        levels_ignored: false,
    })
}

/// What follows from the quests built, without the data: the class quests that teach a spell, the
/// quests of the dungeons run with all their quests, what a quest needs first, the class quests
/// that cannot be done, and the must-haves as mandatory. Returns the forced quests released
/// (`chains::release_impossible_class_quests`).
pub(crate) fn settle_quests(
    quests: &mut [Quest],
    dungeons: &[Dungeon],
    params: &Params,
    known: &HashSet<i64>,
    profile: &Profile,
) -> Vec<(i64, String)> {
    let (completed, requirements) = (&profile.completed_quests, &profile.class_requirements);
    let versions = chains::class_quest_versions(quests, requirements, params.all_class_quests);
    for q in quests.iter_mut() {
        let forced = |d: usize| params.forced_dungeon_quests.contains(&dungeons[d].area);
        q.mandatory |= (0..dungeons.len()).any(|d| forced(d) && q.in_dungeon(d));
    }
    for objective in &profile.objectives {
        match *objective {
            GuideObjective::Dungeon(area) => force_dungeon_quests(quests, dungeons, area),
            GuideObjective::Quest(id) => {
                for q in quests.iter_mut().filter(|q| q.id == id) {
                    q.mandatory = true;
                    q.zone_forced = true;
                }
            }
            // Marked when built (`QuestBuilder::zone_forced`): by what the quest is filed under.
            GuideObjective::Zone(_) => {}
        }
    }
    chains::require_alternatives(quests, requirements);
    chains::require_prerequisites(quests);
    let released = chains::release_impossible_class_quests(quests, known, completed);
    chains::settle_class_quest_versions(quests, &versions);
    for q in quests.iter_mut() {
        q.mandatory |= q.required;
    }
    chains::default_completion_windows(quests, profile, params);
    released
}

/// The quests of an objective guide's dungeon (area `area`): those of it (`Quest::in_dungeon`)
/// or started there, whatever their level (the guide tells the level they need,
/// `Planner::objective_guide_level`). Every one of them is done, as the quests of a forced zone
/// (`zone_forced`, prerequisites included by `chains::require_prerequisites`).
fn force_dungeon_quests(quests: &mut [Quest], dungeons: &[Dungeon], area: i64) {
    let Some(d) = dungeons.iter().position(|d| d.area == area) else {
        return;
    };
    for q in quests.iter_mut() {
        if q.in_dungeon(d) || q.start_dungeon == Some(d) {
            q.mandatory = true;
            q.zone_forced = true;
        }
    }
}

/// Whether a quest is one of an objective guide's zones or quests, by its row (a dungeon's are
/// found once its objectives are placed, `force_dungeon_quests`): a zone's are filed under it or
/// a subzone, class and profession quests left out.
fn objective_row(sources: &Sources, profile: &Profile, r: &sources::QuestRow) -> bool {
    profile.objectives.iter().any(|o| match *o {
        GuideObjective::Quest(id) => id == r.id,
        GuideObjective::Zone(area) => {
            r.sort.is_some_and(|s| s > 0 && sources.in_zones(s, &[area]))
                && r.skill.unwrap_or(0) == 0
                && !sources.class_quest_ids.contains(&r.id)
        }
        GuideObjective::Dungeon(area) => r.sort == Some(area),
    })
}

/// A dungeon holding an objective of a must-have quest is run like a forced one, and past its
/// levels too: the quest asks for it. A dungeon the player forced stays within its levels, the
/// route being refused when it cannot be run there. An objective guide's dungeon is run at any level
/// (up to `max_level`, the game's): the player chose it.
pub(crate) fn settle_dungeons(dungeons: &mut [Dungeon], quests: &[Quest], profile: &Profile, max_level: i64) {
    for q in quests.iter().filter(|q| q.required) {
        for (d, dungeon) in dungeons.iter_mut().enumerate() {
            dungeon.needed |= q.in_dungeon(d);
        }
    }
    for d in dungeons.iter_mut().filter(|d| d.needed) {
        d.max_level = d.max_level.max(profile.to_level);
    }
    for d in (dungeons.iter_mut()).filter(|d| profile.objectives.contains(&GuideObjective::Dungeon(d.area))) {
        d.needed = true;
        d.max_level = max_level;
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
                inside: None,
                floor: None,
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
    let mut builder = QuestBuilder::new(sources, world, profile, params);
    // The quests the player asked for (see `asked_quests`) are done even when their mobs are
    // elites, or an item they ask for is only sold at the auction house.
    let for_asked = Params {
        allow_elite: true,
        auction_house: true,
        ..params.clone()
    };
    let mut asked = QuestBuilder::new(sources, world, profile, &for_asked);
    // An objective guide's quests and their chains are its whatever their level (the last of a
    // chain is often above the dungeon's); the others are left out once its quests are known
    // (`derive::settle`).
    builder.any_level = profile.objective_guide();
    asked.any_level = builder.any_level;
    let asked_ids = asked_quests(sources, profile, params);
    let mut quests = Vec::new();
    let mut skipped = Vec::new();
    for row in &sources.rows {
        let builder = if asked_ids.contains(&row.id) { &asked } else { &builder };
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
    let index: HashMap<i64, usize> = quests.iter().enumerate().map(|(i, q)| (q.id, i)).collect();
    for (&item, givers) in &sources.reward_of {
        for i in givers.iter().filter_map(|q| index.get(q)) {
            quests[*i].reward_items.push(item);
        }
    }
    (quests, skipped)
}

/// The quests the player asked for: the must-haves, those of a dungeon forced with its quests or
/// of an objective guide's objectives, and every quest they need first.
fn asked_quests(sources: &Sources, profile: &Profile, params: &Params) -> HashSet<i64> {
    let mut todo: Vec<i64> = profile.required_quests.iter().map(|q| q.id).collect();
    todo.extend(
        (sources.rows.iter())
            .filter(|r| {
                r.sort.is_some_and(|zone| params.forced_dungeon_quests.contains(&zone))
                    || objective_row(sources, profile, r)
            })
            .map(|r| r.id),
    );
    let mut asked = HashSet::new();
    while let Some(id) = todo.pop() {
        if asked.insert(id) {
            let needs = sources.links.get(&id).into_iter().flatten();
            todo.extend(
                needs
                    .filter(|(kind, other)| {
                        matches!(kind, sources::LinkKind::PreAll | sources::LinkKind::PreAny) && *other > 0
                    })
                    .map(|(_, other)| *other),
            );
        }
    }
    asked
}

/// Class quests the planner cannot do are worth knowing about.
fn report_skipped_class_quests(sources: &Sources, skipped: &[(i64, String, String)]) {
    for (id, name, reason) in skipped {
        if sources.class_quest_ids.contains(id) {
            eprintln!("  class quest skipped: {name} ({id}): {reason}");
        }
    }
}
