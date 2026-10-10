//! Running a planning job, from the request to the route: shared by the `fg` CLI and the desktop
//! app.

mod character;
mod notes;
mod options;
mod overrides;
mod race;
mod request;
mod routes;

pub use options::{NamedId, Options, RaceOption, ZoneOption, options, race_classes};
pub use overrides::{Overrides, RaceDef};
pub use race::optimize;
pub use request::{PlanOutcome, PlanRequest, Prepared};
pub use routes::{load_routes, save_route, slug, write_addon_config, write_addon_routes};

use crate::export;
use crate::model::{self, Profile};
use crate::params::Params;
use crate::plan::Planner;
use crate::world::World;
use anyhow::{Result, bail};
use rusqlite::Connection;

/// Plan a route. `progress` receives status lines (it may be called from several threads).
pub fn plan(
    conn: &Connection,
    overrides: &Overrides,
    req: &PlanRequest,
    progress: &(dyn Fn(&str) + Sync),
) -> Result<PlanOutcome> {
    let mut prepared = prepare(conn, overrides, req, progress)?;
    let planner = Planner::new(&prepared.model, &prepared.world, &prepared.params, &prepared.profile);
    let best = optimize(&planner, progress);
    let route = export::build(&planner, &prepared.profile, &best);
    for missing in planner.missing_class_quests(&best) {
        prepared.notes.push(format!("class quest not planned: {missing}"));
    }
    if !route.total_time.is_finite() {
        let why = planner
            .diagnose(&best)
            .map(|(n, w)| format!("stop {n}: {w}"))
            .unwrap_or_default();
        bail!("no valid route found ({why})");
    }
    progress(&format!(
        "{}: level {} -> {} in {} with {} quests",
        route.name, route.from_level, route.to_level, route.total_time_text, route.quests
    ));
    Ok(PlanOutcome {
        route,
        notes: prepared.notes,
    })
}

/// Everything a route is planned from: the character, the settings, the world and the quests.
pub fn prepare(
    conn: &Connection,
    overrides: &Overrides,
    req: &PlanRequest,
    progress: &(dyn Fn(&str) + Sync),
) -> Result<Prepared> {
    let (profile, params) = character::character(conn, overrides, req)?;
    progress("Loading the world");
    progress(r#"@progress {"phase":"prepare","step":"world"}"#);
    let world = load_world(conn, overrides, &profile, &params, progress)?;
    progress("Loading quests");
    progress(r#"@progress {"phase":"prepare","step":"quests"}"#);
    let model = model::load(conn, &world, &profile, &params)?;
    let notes = notes::model_notes(&model, &world, progress);
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
    let mut world = World::load(conn, profile.faction, params.flight_speed)?;
    let passes: Vec<_> = overrides
        .passes
        .iter()
        .filter(|p| params.shortcuts || !p.shortcut)
        .cloned()
        .collect();
    for warning in world.add_passes(conn, &passes)? {
        progress(&warning);
    }
    world.add_links(
        &overrides.links,
        profile.faction,
        params.run_speed / params.detour,
        params.link_wait,
    );
    Ok(world)
}
