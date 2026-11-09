//! The walks of a guide for the addon: where each walk from one step to the next turns, so that
//! the compass shows which way to go around the steep ground.

use super::Overrides;
use crate::export::{Route, Step, StepKind};
use crate::faction::Faction;
use crate::sync::Joined;
use crate::world::{Pos, World, WorldBuilder};
use anyhow::Result;
use rusqlite::Connection;
use std::sync::atomic::{AtomicUsize, Ordering};

/// Walks shorter than this (yards) are not worked out: the step is in sight.
const SHORT: f64 = 40.0;

/// The world as the planner walks it for a faction: the terrain, the areas left only by boat and
/// the passages between zones.
pub fn walk_world(conn: &Connection, overrides: &Overrides, faction: Faction) -> Result<World> {
    let params = &overrides.params;
    let passes: Vec<_> = (overrides.passes.iter())
        .filter(|p| params.shortcuts || !p.shortcut)
        .cloned()
        .collect();
    let (world, _) = WorldBuilder::load(conn, faction, params.flight_speed, params.drop_tier())?
        .closed(&overrides.closed)
        .enclosed(&overrides.enclosed)
        .caves(&overrides.caves)
        .passes(&passes, crate::world::WorldTables::shared())?
        .build();
    Ok(world)
}

/// Fill each step's `path`: the start and turns of the walk to it from where the step before
/// leaves the character (a flight's landing, the inn of a hearthstone), when it does not go
/// straight. None after a boat, a teleport or a step without a place, for the steps done along
/// the way and inside dungeons.
pub fn add_walk_paths(world: &World, route: &mut Route) {
    for step in &mut route.steps {
        step.path.clear();
    }
    // The walks, in order (where one starts depends on the steps before), then searched side by
    // side: each on its own.
    let walks = route_walks(world, route);
    let next = AtomicUsize::new(0);
    let threads = std::thread::available_parallelism()
        .map_or(1, usize::from)
        .min(walks.len());
    let found: Vec<(usize, Vec<[f64; 2]>)> = std::thread::scope(|scope| {
        let workers: Vec<_> = (0..threads)
            .map(|_| {
                scope.spawn(|| {
                    let _release = crate::threads::Release;
                    let mut found = Vec::new();
                    while let Some(&(k, a, b, level)) = walks.get(next.fetch_add(1, Ordering::Relaxed)) {
                        let walk = world.walk_path(&a, &b, level);
                        if walk.len() > 2 {
                            found.push((
                                k,
                                walk[..walk.len() - 1]
                                    .iter()
                                    .map(|p| [p.x.round(), p.y.round()])
                                    .collect(),
                            ));
                        }
                    }
                    found
                })
            })
            .collect();
        workers.into_iter().flat_map(|w| w.join().joined()).collect()
    });
    for (k, path) in found {
        route.steps[k].path = path;
    }
}

/// The walks of a route from one step to the next (`add_walk_paths`): the step walked to, from
/// where, to where, and the walk level of the character then (`World::walk_level`).
pub fn route_walks(world: &World, route: &Route) -> Vec<(usize, Pos, Pos, u8)> {
    let mut walks: Vec<(usize, Pos, Pos, u8)> = Vec::new();
    let mut from: Option<Pos> = None;
    for (k, step) in route.steps.iter().enumerate() {
        if step.bg || step.inside {
            continue;
        }
        let here = position(world, step);
        if let (Some(a), Some(b)) = (&from, &here)
            && a.continent == b.continent
            && a.dist(b) >= SHORT
        {
            walks.push((k, *a, *b, world.walk_level(step.level)));
        }
        from = match step.kind {
            // Where the flight lands.
            StepKind::Fly => step.target.as_ref().and_then(|t| {
                let node = world.taxi_nodes.iter().find(|n| n.id == t.id)?;
                Some(Pos {
                    zone: world.zone_at(&node.pos).unwrap_or(0),
                    ..node.pos
                })
            }),
            StepKind::Link | StepKind::Teleport => None,
            _ => here.or(from),
        };
    }
    walks
}

/// Where a step is: its world position, else (guides made before they had it) its map's.
fn position(world: &World, step: &Step) -> Option<Pos> {
    let p = if let Some([continent, x, y]) = step.world {
        Pos {
            continent: continent as i64,
            x,
            y,
            zone: step.zone_id.unwrap_or(0),
            z: step.height,
        }
    } else {
        let map = step.map.as_ref()?;
        let zone = world.zone_of_ui_map(map.ui_map)?;
        world.to_world(zone, map.x, map.y)?
    };
    Some(Pos {
        zone: if p.zone == 0 {
            world.zone_at(&p).unwrap_or(0)
        } else {
            p.zone
        },
        ..p
    })
}
