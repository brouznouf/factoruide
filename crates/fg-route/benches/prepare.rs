//! What a plan costs before any route is searched (`job::prepare`), a stage at a time: the
//! terrain, the world, its passages. Run on the Forever data, a Skyborne druid. What is read from
//! the data is kept by database (`memo`): the figures are of the next plans, the first one is
//! the warm-up. The passages are timed cold, as a world with no stamp has them.

#![expect(clippy::unwrap_used, reason = "a bench that cannot load its data fails")]
use criterion::{Criterion, criterion_group, criterion_main};
use fg_route::faction::Faction;
use fg_route::job::{self, Overrides, PlanRequest};
use fg_route::terrain::Terrain;
use fg_route::world::World;
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

fn bench(c: &mut Criterion) {
    let conn = database();
    let overrides = Overrides::load_edition(&repo().join("overrides"), Edition::Forever).unwrap();
    let params = &overrides.params;
    let request: PlanRequest = serde_json::from_value(serde_json::json!({
        "race": "skyborne_horde", "class": "druid", "from_level": 40, "to_level": 41,
        "required_class_quests": [], "start": {"level": 40}
    }))
    .unwrap();
    let horde = Faction::parse("Horde").unwrap();
    let mut group = c.benchmark_group("prepare");
    group
        .sample_size(10)
        .warm_up_time(Duration::from_secs(2))
        .measurement_time(Duration::from_secs(10));
    group.bench_function("all", |b| {
        b.iter(|| job::prepare(&conn, &overrides, black_box(&request), &|_| {}).unwrap());
    });
    group.bench_function("world", |b| {
        b.iter(|| job::game_world(&conn, &overrides, horde, 2).unwrap());
    });
    group.bench_function("world/terrain copy", |b| {
        b.iter(|| Terrain::load(&conn, params.drop_tier()).unwrap());
    });
    group.bench_function("world/load", |b| {
        b.iter(|| World::load(&conn, horde, params.flight_speed, params.drop_tier()).unwrap());
    });
    group.bench_function("world/zone_levels", |b| {
        let mut world = World::load(&conn, horde, params.flight_speed, params.drop_tier()).unwrap();
        b.iter(|| world.add_zone_levels(&conn, params.zone_gap).unwrap());
    });
    group.bench_function("world/passes cold", |b| {
        let passes: Vec<_> = (overrides.passes.iter())
            .filter(|p| params.shortcuts || !p.shortcut)
            .cloned()
            .collect();
        b.iter_with_setup(
            || {
                let mut world = World::load(&conn, horde, params.flight_speed, params.drop_tier()).unwrap();
                world.add_zone_levels(&conn, params.zone_gap).unwrap();
                world
            },
            |mut world| world.add_passes(&conn, &passes).unwrap(),
        );
    });
    group.finish();
}

criterion_group!(benches, bench);
criterion_main!(benches);
