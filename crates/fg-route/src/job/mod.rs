//! Running a planning job, from the request to the route: shared by the `fg` CLI and the desktop
//! app.

mod character;
pub(crate) use character::character;
mod draft;
mod edit;
mod error;
mod event;
mod flights;
mod group;
mod notes;
mod options;
mod overrides;
mod paths;
mod race;
mod reputation;
mod request;
mod required;
mod routes;
mod services;
mod start;
mod starts;
mod timeline;

pub use draft::{EditView, ObjectiveAt, PlaceAt, QuestAt, Request, Requests, Suggestions};
pub use edit::{
    CatalogDungeon, CatalogLink, CatalogPlace, CatalogQuest, EditCatalog, EditReport, Editor, Line, QuestRef,
    RouteStep, StepReport,
};
pub use error::{JobError, RequestError};
pub use event::{JobEvent, Watch};
pub use flights::inferred_flights;
pub use options::{
    FactionOption, FlightOption, NamedId, Options, RaceOption, ZoneOption, class_quests, options, race_classes,
};
pub use overrides::{Overrides, RaceDef, curve_by_level};
pub use paths::{add_walk_paths, route_walks, walk_world};
pub use race::{optimize, optimize_interruptible};
pub use reputation::race_base;
pub use request::{
    ClassKey, GROUP_MAX, GROUP_MIN_LEVEL, PlanOutcome, PlanRequest, Prepared, QuestKey, RaceKey, RequiredQuest,
};
pub use required::{QuestChoice, quest_choices};
pub use routes::{
    DATA_ADDON, GUIDE_FORMAT, POLESTAR_SOURCE_FOLDER, add_polestar_source, addon_walks, data_addon_files, guides_lua,
    load_routes, lua_long_string, polestar_source_dir, render_routes, save_route, slug, write_addon_routes,
    write_data_addon, write_polestar_pack, write_polestar_source, write_polestar_strings,
};
pub(crate) use services::profession_of;
pub use services::{Place, Service, places, services_lua};
pub use start::{Bind, LogObjective, LogQuest, MapPos, QuestId, Skill, Standing, StartState};
#[cfg(any(test, feature = "testing"))]
pub(crate) use starts::preview_starts_of;
pub use starts::{StartCandidate, StartOption, StartPreview, preview_starts};
pub use timeline::{InnInfo, Leg, LegMode, NodeInfo, ObjectiveInfo, QuestInfo, StartView, Timeline, TimelineStep};

use crate::diagnostics::Diagnostics;
use crate::export;
use crate::model::Model;
use crate::model::{self, Profile};
use crate::params::Params;
use crate::plan::Planner;
use crate::terrain::Walks;
use crate::world::{World, WorldBuilder};
use anyhow::Result;
use rusqlite::Connection;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};

/// What the user asks a running job, from another thread (the app's buttons).
#[derive(Debug, Default)]
pub struct Interrupt {
    stop: AtomicBool,
    cancel: AtomicBool,
}

impl Interrupt {
    /// End the optimization now: the job goes on with the best route found so far.
    pub fn stop(&self) {
        self.stop.store(true, Ordering::Relaxed);
    }

    /// Give the job up: it fails with `Cancelled`.
    pub fn cancel(&self) {
        self.cancel.store(true, Ordering::Relaxed);
    }

    /// Ready for a new job.
    pub fn reset(&self) {
        self.stop.store(false, Ordering::Relaxed);
        self.cancel.store(false, Ordering::Relaxed);
    }

    /// The optimization must end (stopped or cancelled).
    pub(crate) fn stopping(&self) -> bool {
        self.stop.load(Ordering::Relaxed) || self.cancelled()
    }

    pub(crate) fn cancelled(&self) -> bool {
        self.cancel.load(Ordering::Relaxed)
    }
}

/// The error of a planning job the user cancelled (`Interrupt::cancel`).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Cancelled;

impl std::fmt::Display for Cancelled {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("cancelled")
    }
}

impl std::error::Error for Cancelled {}

/// Plan a route. `progress` receives status lines (it may be called from several threads).
pub fn plan(
    conn: &Connection,
    overrides: &Overrides,
    req: &PlanRequest,
    progress: Watch<'_>,
) -> Result<PlanOutcome, JobError> {
    plan_interruptible(
        conn,
        overrides,
        req,
        progress,
        &Interrupt::default(),
        None,
        &Diagnostics::default(),
    )
}

/// `plan`, which `interrupt` can end early from another thread. Stopped, the optimization ends
/// at once and the route is the best found so far (the first routes are built if there are
/// none yet). Cancelled, it fails with `Cancelled` once the step under way ends (loading the
/// data is not cut). With `walks`, the walking memory kept between jobs, the paths it already
/// knows are not searched again, and the outcome holds those the route needed (to save with it).
/// `diagnostics`: what a developer asks it to tell or check.
pub fn plan_interruptible(
    conn: &Connection,
    overrides: &Overrides,
    req: &PlanRequest,
    progress: Watch<'_>,
    interrupt: &Interrupt,
    walks: Option<&Arc<Walks>>,
    diagnostics: &Diagnostics,
) -> Result<PlanOutcome, JobError> {
    let mut prepared = prepare(conn, overrides, req, progress)?;
    prepared.diagnostics = diagnostics.clone();
    plan_prepared(prepared, req, progress, interrupt, walks).map(|(outcome, _)| outcome)
}

/// The guide of what `prepare` loaded (see `plan_interruptible`), with it back for the editor.
pub(crate) fn plan_prepared(
    mut prepared: Prepared,
    req: &PlanRequest,
    progress: Watch<'_>,
    interrupt: &Interrupt,
    walks: Option<&Arc<Walks>>,
) -> Result<(PlanOutcome, Prepared), JobError> {
    if let Some(walks) = walks {
        prepared.world.terrain.share(Arc::clone(walks));
    }
    if interrupt.cancelled() {
        return Err(JobError::Cancelled);
    }
    notes::list_skipped(&prepared.model, &prepared.diagnostics, progress);
    let planner = Planner::new(&prepared.model, &prepared.world, &prepared.params, &prepared.profile)
        .diagnosed(&prepared.diagnostics);
    let best = optimize_interruptible(&planner, progress, interrupt);
    if interrupt.cancelled() {
        return Err(JobError::Cancelled);
    }
    // Objectives at the same place one after the other, done at once.
    let best = planner.route_together(best);
    planner.validate_required(&best)?;
    let route = export::build(&planner, &prepared.profile, &best);
    for missing in planner.missing_class_quests(&best) {
        prepared.notes.push(format!("required quest not planned: {missing}"));
    }
    for missing in planner.missing_zone_quests(&best) {
        prepared.notes.push(format!("zone quest not planned: {missing}"));
    }
    for late in planner.late_timed_quests(&best) {
        prepared.notes.push(format!("timed quest given up: {late}"));
    }
    if !route.total_time.is_finite() {
        let why = planner
            .diagnose(&best)
            .map(|(n, w)| format!("stop {n}: {w}"))
            .unwrap_or_default();
        return Err(JobError::NoRoute(why));
    }
    progress(&JobEvent::Status(&format!(
        "{}: level {} -> {} in {} with {} quests",
        route.name, route.from_level, route.to_level, route.total_time_text, route.quests
    )));
    let end = planner.end_of(&best).map(|end| {
        let members = req.members(prepared.params.edition.rules());
        let start = members
            .first()
            .copied()
            .or(req.start.as_ref())
            .cloned()
            .unwrap_or_else(|| StartState {
                name: prepared.profile.name.clone(),
                race: prepared.profile.race_id,
                class: req.class.to_uppercase(),
                faction: Some(prepared.profile.faction),
                ..StartState::default()
            });
        start.after(&prepared.model, &prepared.world, prepared.params.edition.rules(), &end)
    });
    let mut outcome = PlanOutcome {
        plan: edit::route_steps(&planner, &best),
        explicit: true,
        end,
        route,
        notes: prepared.notes.clone(),
        walks: walks.map(|_| prepared.world.terrain.memory()),
    };
    let editor = Editor { prepared };
    editor.set_walks(&mut outcome);
    Ok((outcome, editor.prepared))
}

/// A guide of no steps, to write by hand in the guide editor: the character starts as `plan`
/// starts it, with the same settings; until steps are added, its time is the grinding to the
/// target level. `walks` as for `plan_interruptible`.
pub fn manual(
    conn: &Connection,
    overrides: &Overrides,
    req: &PlanRequest,
    walks: Option<&Arc<Walks>>,
) -> Result<PlanOutcome, JobError> {
    let editor = Editor::open(conn, overrides, req, walks)?;
    let notes = editor.prepared.notes.clone();
    Ok(editor.blank(notes))
}

/// Everything a route is planned from: the character, the settings, the world and the quests.
pub fn prepare(conn: &Connection, overrides: &Overrides, req: &PlanRequest, progress: Watch<'_>) -> Result<Prepared> {
    let (mut profile, mut params) = character(conn, overrides, req)?;
    profile.required_quests = required::resolve(conn, req, &profile, &params)?;
    if params.edition == crate::xp::Edition::Forever
        && params.sleeping_bag
        && !profile
            .required_quests
            .iter()
            .any(|r| r.id == crate::xp::SLEEPING_BAG_QUEST)
    {
        profile.required_quests.push(model::QuestRequirement {
            id: crate::xp::SLEEPING_BAG_QUEST,
            window: model::CompletionWindow::default(),
        });
    }
    progress(&JobEvent::Status("Loading the world"));
    progress(&JobEvent::Progress(
        &serde_json::json!({ "phase": "prepare", "step": "world" }),
    ));
    let world = load_world(conn, overrides, &profile, &params, progress)?;
    progress(&JobEvent::Status("Loading quests"));
    progress(&JobEvent::Progress(
        &serde_json::json!({ "phase": "prepare", "step": "quests" }),
    ));
    let members = req.members(params.edition.rules());
    let starts: Vec<_> = if members.is_empty() {
        req.start.iter().collect()
    } else {
        members.clone()
    };
    let mut model = model::load(conn, &world, &profile, &params, &starts)?;
    check_dungeon_levels(&model, &params, &profile)?;
    let mut start_notes = Vec::new();
    check_exclusive_choices(&model, &members)?;
    // A group is planned for the member with the least XP.
    let start = members.first().copied().or(req.start.as_ref());
    if let Some(start) = start {
        // Its race's flight paths and those of the towns it did quests in count as learned,
        // unless the list was settled by hand.
        let with_deduced = |s: &StartState| {
            let found = if s.flights_final {
                None
            } else {
                s.faction
                    .and_then(|side| inferred_flights(conn, side, &s.completed).ok())
            };
            s.with_deduced_flights(overrides.start_flights(s.race), found)
        };
        let deduced = with_deduced(start);
        let start = &deduced;
        let mates: Vec<StartState> = members.iter().skip(1).map(|m| with_deduced(m)).collect();
        start_from(&mut model, &world, &mut params, start, &mates, &mut start_notes);
        // Quests of the log the model does not have: named from the database, for their abandon.
        for (id, name) in model.initial.log.iter_mut().filter(|(_, name)| name.is_empty()) {
            *name = conn
                .query_row("SELECT name FROM m_quest WHERE id = ?1", [*id], |r| {
                    r.get::<_, Option<String>>(0)
                })
                .ok()
                .flatten()
                .unwrap_or_else(|| format!("Quest {id}"));
        }
        progress(&JobEvent::Status(&format!(
            "Starting from {} (level {}): {} quests done, {} in the log",
            start.name,
            start.level,
            model.initial.turned.len(),
            model.initial.log.len()
        )));
    } else {
        model.initial.known = start::flight_bits(&world, overrides.start_flights(profile.race_id));
    }
    required::check_start(&model, &profile, &members)?;
    model.initial.abilities = model.abilities.initial(start, profile.from_level);
    model.initial.reputation = reputation::starting(conn, &params.reputation_goals, &profile, start, &mut start_notes)?;
    let mut notes = notes::model_notes(&model, &world);
    notes.extend(start_notes);
    Ok(Prepared {
        profile,
        params,
        world,
        model,
        notes,
        diagnostics: Diagnostics::default(),
    })
}

/// Every dungeon the character could run has levels to run it at, and a dungeon the player
/// forced has some left at the level the guide starts from.
pub(crate) fn check_dungeon_levels(model: &Model, params: &Params, profile: &Profile) -> Result<(), RequestError> {
    if let Some(d) = (model.dungeons.iter()).find(|d| d.min_level != i64::MAX && d.min_level > d.max_level) {
        return Err(RequestError::NoDungeonLevels {
            dungeon: d.name.clone(),
            from: params.dungeon_from,
            to: params.dungeon_to,
        });
    }
    let forced = |area: &i64| params.forced_dungeons.contains(area) || params.forced_dungeon_quests.contains(area);
    match (model.dungeons.iter()).find(|d| {
        (d.min_level != i64::MAX || d.mob_level > 0)
            && forced(&d.area)
            && !params.excluded_dungeons.contains(&d.area)
            && !d.needed
            && d.max_level < profile.from_level
    }) {
        Some(d) => Err(RequestError::ForcedDungeonTooLow {
            dungeon: d.name.clone(),
            ends: d.max_level,
            from: profile.from_level,
        }),
        None => Ok(()),
    }
}

/// A member who already chose an exclusive alternative cannot meet an exact requirement.
pub(crate) fn check_exclusive_choices(model: &Model, members: &[&StartState]) -> Result<()> {
    for q in model.quests.iter().filter(|q| q.required) {
        for (k, member) in members.iter().enumerate() {
            if q.members & (1 << k) != 0
                && !member.completed.contains(&QuestId(q.id))
                && q.exclusive.iter().any(|&id| member.completed.contains(&QuestId(id)))
            {
                anyhow::bail!(
                    "required quest {} ({}) conflicts with an exclusive quest already completed by {}",
                    q.name,
                    q.id,
                    member.name
                );
            }
        }
    }
    Ok(())
}

/// The model from where `start` stands (what it did, its log, its place), and the other players
/// of its group joined to it.
pub(crate) fn start_from(
    model: &mut Model,
    world: &World,
    params: &mut Params,
    start: &StartState,
    mates: &[StartState],
    notes: &mut Vec<String>,
) {
    model.initial = start.initial(model, world, params, notes);
    release_done_dungeons(model, start);
    if !mates.is_empty() {
        group::join(model, world, start, &mates.iter().collect::<Vec<_>>(), notes);
    }
}

/// A dungeon only needed by must-have quests the character has already turned in, or finished
/// in its log, is not needed any more: its objectives are done.
fn release_done_dungeons(model: &mut Model, start: &StartState) {
    let finished =
        |i: usize, id: i64| model.initial.turned.contains(&i) || start.log.iter().any(|l| l.id.0 == id && l.complete);
    let mut needed = vec![false; model.dungeons.len()];
    for (i, q) in model.quests.iter().enumerate() {
        if !q.required || finished(i, q.id) {
            continue;
        }
        for d in q.objectives.iter().filter_map(model::Objective::dungeon_of) {
            needed[d] = true;
        }
    }
    for (d, dungeon) in model.dungeons.iter_mut().enumerate() {
        dungeon.needed &= needed[d];
    }
}

/// Zones and flights of the character's faction, with the walking passages between zones and
/// the boats, zeppelins and trams.
fn load_world(
    conn: &Connection,
    overrides: &Overrides,
    profile: &Profile,
    params: &Params,
    progress: Watch<'_>,
) -> Result<World> {
    world_for(conn, overrides, profile.faction, profile.race_id, params, progress)
}

/// The world of a race, as a plan walks it (the terrain, the zones' levels, the passages, the
/// boats and portals of its race), with the default settings: for the tools and the tests.
///
/// # Errors
///
/// When the game data cannot be read.
pub fn game_world(
    conn: &Connection,
    overrides: &Overrides,
    faction: crate::faction::Faction,
    race_id: i64,
) -> Result<World> {
    world_for(conn, overrides, faction, race_id, &overrides.params, &|_| {})
}

fn world_for(
    conn: &Connection,
    overrides: &Overrides,
    faction: crate::faction::Faction,
    race_id: i64,
    params: &Params,
    progress: Watch<'_>,
) -> Result<World> {
    let passes: Vec<_> = overrides
        .passes
        .iter()
        .filter(|p| params.shortcuts || !p.shortcut)
        .cloned()
        .collect();
    let tables = crate::world::WorldTables::shared();
    let (world, warnings) = WorldBuilder::load(conn, faction, params.flight_speed, params.drop_tier())?
        .zone_levels(params.zone_gap)?
        .closed(&overrides.closed)
        .enclosed(&overrides.enclosed)
        .caves(&overrides.caves)
        // What the walks between the passages depend on, besides the database.
        .stamp(&format!(
            "{:?}{:?}{:?}{:?}{:?}{:?}{:?}",
            overrides.closed,
            overrides.enclosed,
            overrides.caves,
            passes,
            params.zone_gap,
            params.detour,
            params.run_speed
        ))
        .passes(&passes, tables)?
        .links(
            &overrides.links_of_race(race_id),
            faction,
            params.run_speed / params.detour,
            params.link_wait,
            tables,
        )
        .build();
    for warning in &warnings {
        progress(&JobEvent::Status(warning));
    }
    Ok(world)
}
