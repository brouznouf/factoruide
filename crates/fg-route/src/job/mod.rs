//! Running a planning job, from the request to the route: shared by the `fg` CLI and the desktop
//! app.

mod character;
pub(crate) use character::character;
mod draft;
mod edit;
mod error;
mod event;
#[cfg(test)]
mod finalize_tests;
mod flights;
mod group;
mod notes;
mod objectives;
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
pub(crate) use start::objective_progress;
#[cfg(test)]
mod reputation_cancellation_tests;
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
pub use objectives::{ExcludedQuest, ObjectiveCounts, ObjectivePreview, objective_counts, objective_preview};
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
pub use start::{
    Bind, DailyState, InventoryItem, LogObjective, LogQuest, MapPos, QuestId, Skill, Standing, StartState,
};
#[cfg(any(test, feature = "testing"))]
pub(crate) use starts::preview_starts_of;
pub use starts::{StartCandidate, StartOption, StartPreview, preview_starts};
pub use timeline::{InnInfo, Leg, LegMode, NodeInfo, ObjectiveInfo, QuestInfo, StartView, Timeline, TimelineStep};

use crate::diagnostics::Diagnostics;
use crate::export;
use crate::model::Model;
use crate::model::{self, Profile};
use crate::params::Params;
use crate::plan::{Planner, Stop};
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
    let best = finalize_route(&planner, best)?;
    let members = req.members(prepared.params.edition.rules());
    let start = members.first().copied().or(req.start.as_ref());
    let cancelled = || interrupt.cancelled();
    if !planner.params.reputation_goals.is_empty() {
        progress(&JobEvent::Status("Planning reputation activities"));
    }
    let completed = Planner::new(&prepared.model, &prepared.world, &prepared.params, &prepared.profile)
        .diagnosed(&prepared.diagnostics)
        .abortable(&cancelled)
        .complete_reputation(&best, start);
    if interrupt.cancelled() {
        return Err(JobError::Cancelled);
    }
    let completed = completed?;
    let route = export::build_traced(
        &planner,
        &prepared.profile,
        &best,
        &completed.trace,
        completed.total,
        completed.breakdown,
    );
    let reached: Vec<f64> = prepared
        .params
        .reputation_goals
        .iter()
        .map(|g| completed.end.reputation.get(&g.faction).copied().unwrap_or(0.0))
        .collect();
    planner.refresh_reputation_notes(&mut prepared.notes, &reached);
    prepared.notes.extend(completed.program.result.warnings.iter().cloned());
    for missing in planner.missing_mandatory_quests(&best) {
        prepared.notes.push(missing);
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
    let end = Some({
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
        start.after(
            &prepared.model,
            &prepared.world,
            prepared.params.edition.rules(),
            &completed.end,
        )
    });
    let mut plan = edit::route_steps(&planner, &best);
    if !completed.program.phases.is_empty() || !prepared.params.reputation_goals.is_empty() {
        plan.push(RouteStep::ReputationProgram {
            program: Box::new(completed.program),
        });
    }
    let mut outcome = PlanOutcome {
        plan,
        explicit: true,
        end,
        route,
        notes: prepared.notes.clone(),
        walks: walks.map(|_| prepared.world.terrain.memory()),
    };
    let editor = Editor {
        prepared,
        start: start.cloned(),
    };
    editor.set_walks(&mut outcome);
    if interrupt.cancelled() {
        return Err(JobError::Cancelled);
    }
    Ok((outcome, editor.prepared))
}

/// Shared production finalization, also used by the single-start comparison harness.
/// Merge returns, settle visits, group objectives, then validate required content.
pub fn finalize_route(planner: &Planner<'_>, best: Vec<Stop>) -> Result<Vec<Stop>, JobError> {
    let best = planner.repair_required_gear(best);
    let best = planner.repair_required_reputation(best);
    let saved = planner.has_hard_goals().then(|| best.clone());
    let best = planner.merge_visits(&best).map_or(best, |(merged, _)| merged);
    let best = planner.tidy(&best).map_or(best, |(tidied, _)| tidied);
    let best = planner.route_together(best);
    let best = if let Some(old) = saved {
        planner.keep_hard_goals(old, best)
    } else {
        best
    };
    // Reputation repair can add accepts after the search's cleanup, including quests
    // whose single dungeon visit has already passed. Remove that unused work too.
    let best = match planner.drop_dungeonless(&best) {
        Some(kept) => planner.keep_hard_goals(best, kept),
        None => best,
    };
    planner.validate_required(&best)?;
    Ok(best)
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
        && !profile.objective_guide()
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
    // Its race's flight paths and those of the towns it did quests in count as learned, unless
    // the list was settled by hand.
    let with_deduced = |s: &StartState| {
        let found = if s.flights_final {
            None
        } else {
            s.faction
                .and_then(|side| inferred_flights(conn, side, &s.completed).ok())
        };
        s.with_deduced_flights(overrides.start_flights(s.race), found)
    };
    let starting: Option<(StartState, Vec<StartState>)> = start.map(|s| {
        (
            with_deduced(s),
            members.iter().skip(1).map(|m| with_deduced(m)).collect(),
        )
    });
    match &starting {
        Some((start, mates)) => start_from(&mut model, &world, &mut params, start, mates, &mut start_notes),
        None => model.initial.known = start::flight_bits(&world, overrides.start_flights(profile.race_id)),
    }
    settle_objective_guide(&mut model, &world, &params, &profile)?;
    if let Some((start, _)) = &starting {
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
    }
    required::check_start(&model, &profile, &members)?;
    model.initial.abilities = model.abilities.initial(start, profile.from_level);
    model.initial.reputation_inferred = start.is_some_and(|s| s.reputation_inferred);
    model.initial.reputation_recorded = params
        .reputation_goals
        .iter()
        .filter(|g| start.is_some_and(|s| s.reputation.iter().any(|r| r.id == g.faction)))
        .map(|g| g.faction)
        .collect();
    model.initial.reputation = reputation::starting(conn, &params.reputation_goals, &profile, start, &mut start_notes)?;
    model.initial.reputation_all =
        reputation::source_starting(conn, &model, &params, &profile, start, &mut start_notes)?;
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

/// An objective guide is planned at the character's level, its quests in the best order whatever
/// the levels they ask: those gated above it are kept for the warning (`Model::above_level`, the
/// guide recommending the highest), then no stop waits for a level (`Model::levels_ignored`).
/// The guide ends with its quests (no level to reach). Refused when the character has none of
/// them left.
pub(crate) fn settle_objective_guide(
    model: &mut Model,
    world: &World,
    params: &Params,
    profile: &Profile,
) -> Result<()> {
    if !profile.objective_guide() {
        return Ok(());
    }
    let levels = Planner::new(model, world, params, profile).forced_quest_levels();
    if levels.is_empty() {
        anyhow::bail!("no quest of these objectives left for this character");
    }
    let mut above: Vec<(i64, i64)> = (levels.iter())
        .filter(|&&(_, level)| level > profile.from_level)
        .map(|&(i, level)| (model.quests[i as usize].id, level))
        .collect();
    above.sort_by_key(|&(id, level)| (std::cmp::Reverse(level), id));
    model.above_level = above;
    model.levels_ignored = true;
    for q in &mut model.quests {
        q.min_level = q.min_level.min(profile.from_level);
        q.completion_window = model::CompletionWindow::default();
    }
    for d in &mut model.dungeons {
        d.min_level = d.min_level.min(profile.from_level);
        d.max_level = params.edition.rules().max_level;
    }
    Ok(())
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

/// A dungeon only needed by must-have quests (or a dungeon guide's) the character has already
/// turned in, or finished in its log, is not needed any more: its objectives are done.
fn release_done_dungeons(model: &mut Model, start: &StartState) {
    let finished =
        |i: usize, id: i64| model.initial.turned.contains(&i) || start.log.iter().any(|l| l.id.0 == id && l.complete);
    let mut needed = vec![false; model.dungeons.len()];
    for (i, q) in model.quests.iter().enumerate() {
        if !(q.required || q.zone_forced) || finished(i, q.id) {
            continue;
        }
        for (d, need) in needed.iter_mut().enumerate() {
            *need |= q.in_dungeon(d);
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
