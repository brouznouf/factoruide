//! The search improving a route (`Planner::improve_while`), a fixed number of tries, on a world
//! made by hand (`fg_route::testing`): a long route through three zones and four dungeons, so
//! that what a try costs on the stops of a whole leveling shows.

use criterion::{Criterion, criterion_group, criterion_main};
use fg_route::job::Prepared;
use fg_route::plan::{Effort, Planner};
use fg_route::testing::{At, Obj, Q, Scenario};
use std::hint::black_box;
use std::time::Duration;

/// Quests from level 1 to 40 in three zones side by side, a camp of each level, four dungeons
/// with quests whose objectives are inside.
fn scenario() -> Prepared {
    let mut s = Scenario::new()
        .levels(1, 40)
        .zone(2, "Plain")
        .zone(3, "Barrens")
        .trainer(1, 50.0, 50.0)
        .vendor(1, 52.0, 52.0)
        .inn("Valley Inn", 1, 54.0, 54.0);
    for i in 0..240_i64 {
        let zone = 1 + i / 80;
        let (x, y) = (5.0 + (i % 80 / 9) as f64 * 10.0, 5.0 + (i % 9) as f64 * 10.0);
        let level = 1 + i / 6;
        s = s.mobs(zone, x, y + 4.0, level, 3).mobs(zone, 95.0, 95.0, level, 10);
        s = s.quest(
            Q::new(1000 + i, &format!("Quest {i}"))
                .level(level)
                .xp(120 * level)
                .giver(zone, x, y)
                .kill("Beast", level, 6, zone, x + 3.0, y + 3.0),
        );
    }
    for d in 0..4_i64 {
        let (zone, level) = (1 + d % 3, 12 + 7 * d);
        let entrance = At {
            zone,
            x: 90.0,
            y: 10.0 + 20.0 * d as f64,
        };
        s = s.dungeon(
            ["Mines", "Caverns", "Keep", "Halls"][d as usize],
            zone,
            entrance.x,
            entrance.y,
        );
        for k in 0..3_i64 {
            s = s.quest(
                Q::new(2000 + 10 * d + k, &format!("Dungeon {d} quest {k}"))
                    .level(level)
                    .xp(400 * level)
                    .giver(zone, 80.0, entrance.y)
                    .objective(Obj::kill("Defias", level, 8, entrance).inside(d as usize)),
            );
        }
    }
    s.build()
}

fn bench(c: &mut Criterion) {
    let p = scenario();
    let planner = Planner::new(&p.model, &p.world, &p.params, &p.profile);
    let route = planner.construct(|_| {});
    let mut group = c.benchmark_group("search");
    group
        .sample_size(10)
        .warm_up_time(Duration::from_secs(2))
        .measurement_time(Duration::from_secs(15));
    group.bench_function("2000 tries", |b| {
        b.iter(|| {
            planner.improve_while(
                black_box(route.clone()),
                Effort::Tries(2000),
                || true,
                |_| {},
                |_, _, _| {},
            )
        });
    });
    group.finish();
}

criterion_group!(benches, bench);
criterion_main!(benches);
