//! Walks the planner asks, worked out from cold memories: those between terrain areas joined only
//! through passages (`World::bridge`), and others between zones.

#![expect(clippy::unwrap_used, reason = "a bench that cannot load its data fails")]
use criterion::{BatchSize, Criterion, criterion_group, criterion_main};
use fg_route::faction::Faction;
use fg_route::job::Overrides;
use fg_route::world::{Pos, World, WorldBuilder};
use fg_route::xp::Edition;
use rusqlite::Connection;
use std::hint::black_box;
use std::path::PathBuf;
use std::time::Duration;

fn repo() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../..")
}

/// The Forever database, unpacked next to the packed one when missing or older.
fn database() -> Connection {
    let (path, packed) = (
        repo().join("data/forever.sqlite"),
        repo().join("data/forever.sqlite.zst"),
    );
    let modified = |p: &PathBuf| std::fs::metadata(p).and_then(|m| m.modified()).ok();
    if !path.exists() || modified(&packed) > modified(&path) {
        let data = zstd::decode_all(std::fs::File::open(&packed).unwrap()).unwrap();
        std::fs::write(&path, data).unwrap();
    }
    Connection::open(path).unwrap()
}

/// The Horde's world as a route's (`job::load_world`), without the boats: walks only.
fn world() -> World {
    let conn = database();
    let overrides = Overrides::load_edition(&repo().join("overrides"), Edition::Forever).unwrap();
    let params = &overrides.params;
    let passes: Vec<_> = (overrides.passes.iter())
        .filter(|p| params.shortcuts || !p.shortcut)
        .cloned()
        .collect();
    let (world, _) = WorldBuilder::load(
        &conn,
        Faction::parse("Horde").unwrap(),
        params.flight_speed,
        params.drop_tier(),
    )
    .unwrap()
    .zone_levels(params.zone_gap)
    .unwrap()
    .closed(&overrides.closed)
    .enclosed(&overrides.enclosed)
    .caves(&overrides.caves)
    .passes(&passes, fg_route::world::WorldTables::shared())
    .unwrap()
    .build();
    world
}

fn walks(name: &str) -> Vec<(Pos, Pos, u8)> {
    let text =
        std::fs::read_to_string(PathBuf::from(env!("CARGO_MANIFEST_DIR")).join(format!("benches/walks/{name}.txt")))
            .unwrap();
    (text.lines())
        .filter(|l| !l.starts_with('#'))
        .map(|l| {
            let v: Vec<f64> = l.split_whitespace().map(|x| x.parse().unwrap()).collect();
            let pos = |zone: f64, x: f64, y: f64| Pos {
                continent: v[0] as i64,
                x,
                y,
                zone: zone as i64,
                z: None,
            };
            (pos(v[1], v[2], v[3]), pos(v[4], v[5], v[6]), v[7] as u8)
        })
        .collect()
}

fn bench(c: &mut Criterion) {
    let world = world();
    let mut group = c.benchmark_group("walks");
    group
        .sample_size(10)
        .warm_up_time(Duration::from_millis(500))
        .measurement_time(Duration::from_secs(3));
    for name in ["bridged", "crossing"] {
        for (k, (a, p, level)) in walks(name).into_iter().enumerate() {
            group.bench_function(format!("{name}/{k}"), |b| {
                b.iter_batched(
                    || world.forget_walks(),
                    |()| world.walk_distance_at(black_box(&a), black_box(&p), world.walk_level(i64::from(level))),
                    BatchSize::PerIteration,
                );
            });
        }
    }
    group.finish();
}

criterion_group!(benches, bench);
criterion_main!(benches);
