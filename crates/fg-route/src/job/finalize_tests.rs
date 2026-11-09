use super::*;
use crate::model::Names;
use crate::params::ReputationGoal;
use crate::plan::Kind;
use crate::testing::{At, Obj, Q, Scenario};

fn repaired_dungeon_tail() -> Prepared {
    Scenario::new()
        .levels(60, 60)
        .dungeon("One continuous visit", 1, 70.0, 70.0)
        .quest(
            Q::new(1, "Already completed dungeon work")
                .level(60)
                .xp(0)
                .reputation(76, 100.0)
                .objective(
                    Obj::visit(
                        "First dungeon objective",
                        At {
                            zone: 1,
                            x: 70.0,
                            y: 70.0,
                        },
                    )
                    .inside(0),
                ),
        )
        .quest(
            Q::new(2, "Unusable late dungeon accept")
                .level(60)
                .xp(0)
                .reputation(68, 100.0)
                .objective(
                    Obj::visit(
                        "Late dungeon objective",
                        At {
                            zone: 1,
                            x: 70.0,
                            y: 70.0,
                        },
                    )
                    .inside(0),
                ),
        )
        .quest(
            Q::new(3, "Reachable partial progress")
                .level(60)
                .xp(0)
                .reputation(68, 25.0),
        )
        .params(|p| {
            p.reputation_goals = vec![
                ReputationGoal {
                    faction: 76,
                    target: 100,
                },
                ReputationGoal {
                    faction: 68,
                    target: 100,
                },
                ReputationGoal {
                    faction: 67,
                    target: 100,
                },
            ];
            p.camp_radius = 0.0;
            p.tidy_slack = -1.0;
        })
        .build()
}

#[test]
fn finalization_removes_an_unusable_accept_added_by_reputation_repair() {
    let p = repaired_dungeon_tail();
    let planner = Planner::new(&p.model, &p.world, &p.params, &p.profile);
    let before = vec![
        Stop::quest(0, Kind::Accept),
        Stop::quest(0, Kind::Objective(0)),
        Stop::quest(0, Kind::TurnIn),
    ];
    let repaired = planner.repair_required_reputation(before.clone());
    assert!(repaired.contains(&Stop::quest(1, Kind::Accept)), "{repaired:?}");
    assert!(!repaired.contains(&Stop::quest(1, Kind::Objective(0))), "{repaired:?}");

    let finalized = finalize_route(&planner, before).unwrap();
    assert!(
        !finalized.iter().any(|s| s.is_quest() && s.index == 1),
        "the new guide must not ask for dungeon work its earlier visit cannot complete: {finalized:?}"
    );
    assert_eq!(planner.reputation_reached(&finalized), vec![100.0, 25.0, 0.0]);
    assert!(finalized.contains(&Stop::quest(0, Kind::Objective(0))));

    let completed = planner.complete_reputation(&finalized, None).unwrap();
    let mut steps = edit::route_steps(&planner, &finalized);
    steps.push(RouteStep::ReputationProgram {
        program: Box::new(completed.program),
    });
    let editor = Editor::from_prepared(p);
    let saved = editor.save(&steps, vec![]).unwrap();
    let restored: PlanOutcome = serde_json::from_slice(&serde_json::to_vec(&saved).unwrap()).unwrap();
    let (steps, rebuilt) = editor.steps(&restored);
    assert!(!rebuilt);
    let requests = Requests::default();
    let view = editor.view(&steps, &Names::default(), &requests.next()).unwrap();
    assert_eq!(view.report.errors, 0, "{:?}", view.report.steps);
    let resaved = editor.save(&steps, vec![]).unwrap();
    assert_eq!(resaved.route.total_time, saved.route.total_time);
    assert_eq!(
        resaved.route.reputation.iter().map(|r| r.reached).collect::<Vec<_>>(),
        vec![100.0, 25.0, 0.0]
    );
}
