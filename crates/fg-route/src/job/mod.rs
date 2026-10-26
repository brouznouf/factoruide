//! Running a planning job, from the request to the route: shared by the `fg` CLI and the desktop
//! app.

mod character;
mod draft;
mod edit;
mod error;
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
pub use flights::inferred_flights;
pub use options::{
    FactionOption, FlightOption, NamedId, Options, RaceOption, ZoneOption, class_quests, options, race_classes,
};
pub use overrides::{Overrides, RaceDef};
pub use paths::{add_walk_paths, route_walks, walk_world};
pub use race::{optimize, optimize_interruptible};
pub use reputation::race_base;
pub use request::{ClassKey, GROUP_MAX, GROUP_MIN_LEVEL, PlanOutcome, PlanRequest, Prepared, RaceKey, RequiredQuest};
pub use routes::{
    DATA_ADDON, GUIDE_FORMAT, POLESTAR_SOURCE_FOLDER, add_polestar_source, addon_walks, data_addon_files, guides_lua,
    load_routes, lua_long_string, polestar_source_dir, render_routes, save_route, slug, write_addon_routes,
    write_data_addon, write_polestar_pack, write_polestar_source, write_polestar_strings,
};
pub use services::{Place, Service, places, services_lua};
pub use start::{Bind, LogObjective, LogQuest, MapPos, QuestId, Skill, Standing, StartState};
pub use starts::{StartOption, StartPreview, preview_starts};
pub use timeline::{InnInfo, Leg, LegMode, NodeInfo, ObjectiveInfo, QuestInfo, StartView, Timeline, TimelineStep};

use crate::export;
use crate::model::{self, Profile};
use crate::params::Params;
use crate::plan::Planner;
use crate::terrain::Walks;
use crate::world::World;
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
    progress: &(dyn Fn(&str) + Sync),
) -> Result<PlanOutcome, JobError> {
    plan_interruptible(conn, overrides, req, progress, &Interrupt::default(), None)
}

/// `plan`, which `interrupt` can end early from another thread. Stopped, the optimization ends
/// at once and the route is the best found so far (the first routes are built if there are
/// none yet). Cancelled, it fails with `Cancelled` once the step under way ends (loading the
/// data is not cut). With `walks`, the walking memory kept between jobs, the paths it already
/// knows are not searched again, and the outcome holds those the route needed (to save with it).
pub fn plan_interruptible(
    conn: &Connection,
    overrides: &Overrides,
    req: &PlanRequest,
    progress: &(dyn Fn(&str) + Sync),
    interrupt: &Interrupt,
    walks: Option<&Arc<Walks>>,
) -> Result<PlanOutcome, JobError> {
    let mut prepared = prepare(conn, overrides, req, progress)?;
    if let Some(walks) = walks {
        prepared.world.terrain.share(Arc::clone(walks));
    }
    if interrupt.cancelled() {
        return Err(JobError::Cancelled);
    }
    let planner = Planner::new(&prepared.model, &prepared.world, &prepared.params, &prepared.profile);
    let best = optimize_interruptible(&planner, progress, interrupt);
    if interrupt.cancelled() {
        return Err(JobError::Cancelled);
    }
    // Objectives at the same place one after the other, done at once.
    let best = planner.route_together(best);
    planner.validate_required(&best)?;
    let route = export::build(&planner, &prepared.profile, &best);
    for missing in planner.missing_class_quests(&best) {
        prepared.notes.push(format!("class quest not planned: {missing}"));
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
    progress(&format!(
        "{}: level {} -> {} in {} with {} quests",
        route.name, route.from_level, route.to_level, route.total_time_text, route.quests
    ));
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
    Ok(outcome)
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
pub fn prepare(
    conn: &Connection,
    overrides: &Overrides,
    req: &PlanRequest,
    progress: &(dyn Fn(&str) + Sync),
) -> Result<Prepared> {
    let (mut profile, mut params) = character::character(conn, overrides, req)?;
    profile.required_quests = required::resolve(conn, req, &profile)?;
    progress("Loading the world");
    progress(r#"@progress {"phase":"prepare","step":"world"}"#);
    let world = load_world(conn, overrides, &profile, &params, progress)?;
    progress("Loading quests");
    progress(r#"@progress {"phase":"prepare","step":"quests"}"#);
    let mut model = model::load(conn, &world, &profile, &params)?;
    let mut start_notes = Vec::new();
    let members = req.members(params.edition.rules());
    // A member who already chose an exclusive alternative cannot meet an exact requirement.
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
    // A group is planned for the member with the least XP.
    let start = members.first().copied().or(req.start.as_ref());
    if let Some(start) = start {
        // Its race's flight paths and those of the towns it did quests in count as learned,
        // unless the list was settled by hand.
        let with_deduced = |s: &StartState| {
            let mut copy = s.clone();
            if !s.flights_final {
                copy.flights.extend(overrides.start_flights(s.race));
                if let Some(Ok(found)) = s.faction.map(|side| inferred_flights(conn, side, &s.completed)) {
                    copy.flights.extend(found);
                }
                copy.flights.sort_unstable();
                copy.flights.dedup();
            }
            copy
        };
        let deduced = with_deduced(start);
        let start = &deduced;
        model.initial = start.initial(&model, &world, &mut params, &mut start_notes);
        if members.len() > 1 {
            let mates: Vec<StartState> = members[1..].iter().map(|m| with_deduced(m)).collect();
            group::join(
                &mut model,
                &world,
                start,
                &mates.iter().collect::<Vec<_>>(),
                &mut start_notes,
            );
        }
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
        progress(&format!(
            "Starting from {} (level {}): {} quests done, {} in the log",
            start.name,
            start.level,
            model.initial.turned.len(),
            model.initial.log.len()
        ));
    } else {
        model.initial.known = start::flight_bits(&world, overrides.start_flights(profile.race_id));
    }
    model.initial.abilities = model.abilities.initial(start, profile.from_level);
    model.initial.reputation = reputation::starting(conn, &params.reputation_goals, &profile, start, &mut start_notes)?;
    let mut notes = notes::model_notes(&model, &world, progress);
    notes.extend(start_notes);
    Ok(Prepared {
        profile,
        params,
        world,
        model,
        notes,
    })
}

/// Zones and flights of the character's faction, with the walking passages between zones and
/// the boats, zeppelins and trams.
fn load_world(
    conn: &Connection,
    overrides: &Overrides,
    profile: &Profile,
    params: &Params,
    progress: &(dyn Fn(&str) + Sync),
) -> Result<World> {
    let mut world = World::load(conn, profile.faction, params.flight_speed, params.drop_tier())?;
    let passes: Vec<_> = overrides
        .passes
        .iter()
        .filter(|p| params.shortcuts || !p.shortcut)
        .cloned()
        .collect();
    world.add_zone_levels(conn, params.zone_gap)?;
    for warning in world.close(&overrides.closed) {
        progress(&warning);
    }
    for warning in world.enclose(&overrides.enclosed) {
        progress(&warning);
    }
    for warning in world.add_caves(&overrides.caves) {
        progress(&warning);
    }
    for warning in world.add_passes(conn, &passes)? {
        progress(&warning);
    }
    // Links kept to some races (the Skyborne's portals): those of the character's race.
    let links: Vec<_> = (overrides.links.iter())
        .filter(|l| {
            l.races.is_empty()
                || l.races
                    .iter()
                    .any(|r| overrides.races.get(r).is_some_and(|d| d.id == profile.race_id))
        })
        .cloned()
        .collect();
    world.add_links(
        &links,
        profile.faction,
        params.run_speed / params.detour,
        params.link_wait,
    );
    Ok(world)
}
