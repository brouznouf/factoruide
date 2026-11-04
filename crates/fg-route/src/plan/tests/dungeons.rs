//! Dungeons as zones: stops inside a dungeon enter it, once.

use super::*;
use crate::plan::Event;
use crate::plan::moves::Change;
use crate::testing::{At, Obj, Q, Scenario};

/// Two quests of a dungeon at the level of a character of 18 (its entrance at 70, 70), and a
/// quest outside.
fn two_dungeon_quests() -> Prepared {
    let inside = At {
        zone: 1,
        x: 70.0,
        y: 70.0,
    };
    let quest = |id, name: &str| {
        Q::new(id, name)
            .level(18)
            .giver(1, 40.0, 40.0)
            .objective(Obj::kill("Defias", 18, 6, inside).inside(0))
    };
    Scenario::new()
        .levels(18, 20)
        .zone(1, "Westfall")
        .dungeon("The Deadmines", 1, 70.0, 70.0)
        .quest(quest(300, "Red Silk Bandanas"))
        .quest(quest(301, "Collecting Memories"))
        .quest(
            Q::new(302, "Outside")
                .level(18)
                .giver(1, 41.0, 41.0)
                .objective(Obj::kill(
                    "Murloc",
                    18,
                    4,
                    At {
                        zone: 1,
                        x: 20.0,
                        y: 20.0,
                    },
                )),
        )
        .build()
}

fn stop(p: &Prepared, id: i64, kind: Kind) -> Stop {
    Stop::quest(p.model.index[&id] as u32, kind)
}

/// A stop inside a dungeon enters it: the run's time once, however many stops are done inside.
#[test]
fn stops_inside_a_dungeon_enter_it_once() {
    let p = two_dungeon_quests();
    let planner = planner(&p);
    let route = [
        stop(&p, 300, Kind::Accept),
        stop(&p, 301, Kind::Accept),
        stop(&p, 300, Kind::Objective(0)),
        stop(&p, 301, Kind::Objective(0)),
    ];
    let mut trace = Vec::new();
    let (_, spent) = planner.simulate_full(&route, Some(&mut trace)).expect("a valid route");
    let entries = trace
        .iter()
        .filter(|t| matches!(t.event, Event::Enter { dungeon: 0 }))
        .count();
    assert_eq!(entries, 1);
    assert_eq!(spent.dungeon_runs, 1);
    assert!((spent.dungeons - p.model.dungeons[0].minutes * 60.0).abs() < 1e-6);
}

/// A dungeon left (a stop outside between two inside) is not entered again: the route is not
/// valid.
#[test]
fn a_dungeon_left_is_not_entered_again() {
    let p = two_dungeon_quests();
    let planner = planner(&p);
    let route = [
        stop(&p, 300, Kind::Accept),
        stop(&p, 301, Kind::Accept),
        stop(&p, 302, Kind::Accept),
        stop(&p, 300, Kind::Objective(0)),
        stop(&p, 302, Kind::Objective(0)),
        stop(&p, 301, Kind::Objective(0)),
    ];
    assert!(planner.simulate(&route, None).is_none());
}

/// A search move taking a dungeon's stop elsewhere takes the dungeon's other stops with it.
#[test]
fn a_moved_dungeon_stop_takes_the_dungeon_along() {
    let p = two_dungeon_quests();
    let planner = planner(&p);
    let (a300, a301, a302) = (
        stop(&p, 300, Kind::Accept),
        stop(&p, 301, Kind::Accept),
        stop(&p, 302, Kind::Accept),
    );
    let (o300, o301, o302) = (
        stop(&p, 300, Kind::Objective(0)),
        stop(&p, 301, Kind::Objective(0)),
        stop(&p, 302, Kind::Objective(0)),
    );
    // The objective of 301 moved after the one outside.
    let mut route = vec![a300, a301, a302, o300, o302, o301];
    let mut change = Change {
        first: 5,
        last: 5,
        reordered: true,
    };
    planner.gather_dungeons(&mut route, &mut change);
    assert_eq!(route, [a300, a301, a302, o302, o300, o301]);
    assert!(change.first <= 3 && change.last >= 5);
    assert!(planner.simulate(&route, None).is_some());
}
