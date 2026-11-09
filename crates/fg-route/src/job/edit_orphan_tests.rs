use super::*;
use crate::params::ReputationGoal;
use crate::testing::{At, Obj, Q, Scenario};

fn legacy_outcome(mandatory: bool, dependency: bool) -> (Editor, PlanOutcome) {
    let mut p = Scenario::new()
        .levels(60, 60)
        .dungeon("Already missed dungeon", 1, 70.0, 70.0)
        .quest(
            Q::new(1, "Unused dungeon pickup").level(60).xp(0).objective(
                Obj::visit(
                    "Inside",
                    At {
                        zone: 1,
                        x: 70.0,
                        y: 70.0,
                    },
                )
                .inside(0),
            ),
        )
        .quest(Q::new(2, "Useful reputation").level(60).xp(0).reputation(68, 25.0))
        .quest(Q::new(3, "Ordinary unfinished pickup").level(60).xp(0))
        .params(|p| {
            p.reputation_goals = vec![ReputationGoal {
                faction: 68,
                target: 100,
            }];
        })
        .build();
    p.model.quests[0].mandatory = mandatory;
    if dependency {
        p.model.quests[1].pre_held.push(1);
    }
    let editor = Editor::from_prepared(p);
    let mut outcome = editor.blank_guide();
    outcome.explicit = true;
    outcome.plan = vec![
        RouteStep::Accept { quest: 1 },
        RouteStep::Accept { quest: 2 },
        RouteStep::TurnIn { quest: 2 },
        RouteStep::Accept { quest: 3 },
        RouteStep::ReputationProgram {
            program: Box::new(crate::reputation::RepProgram {
                version: 1,
                ..Default::default()
            }),
        },
    ];
    let persisted = serde_json::from_slice(&serde_json::to_vec(&outcome).unwrap()).unwrap();
    (editor, persisted)
}

#[test]
fn opening_a_legacy_generated_guide_repairs_only_unused_dungeon_pickups() {
    let (editor, outcome) = legacy_outcome(false, false);
    let names = Names::default();
    assert_eq!(
        editor.simulate(&outcome.plan, &names).errors,
        1,
        "the recorded generation defect"
    );
    let (steps, _) = editor.steps(&outcome);
    assert!(
        !steps.contains(&RouteStep::Accept { quest: 1 }),
        "unused dungeon pickup: {steps:?}"
    );
    assert!(
        steps.contains(&RouteStep::Accept { quest: 3 }),
        "ordinary unfinished work remains"
    );
    let requests = super::super::Requests::default();
    let view = editor.view(&steps, &names, &requests.next()).unwrap();
    assert_eq!(view.report.errors, 0);
    let saved = editor.save(&steps, outcome.notes).unwrap();
    assert_eq!(saved.route.reputation[0].reached, 25.0);
    let (again, _) = editor.steps(&saved);
    assert_eq!(again, saved.plan, "opening the repaired version is stable");
    let resaved = editor.save(&again, saved.notes.clone()).unwrap();
    assert_eq!(resaved.route.total_time, saved.route.total_time);
    assert_eq!(resaved.route.reputation[0].reached, 25.0);
}

#[test]
fn load_repair_keeps_mandatory_dungeon_pickups() {
    let (editor, outcome) = legacy_outcome(true, false);
    let (steps, _) = editor.steps(&outcome);
    assert!(steps.contains(&RouteStep::Accept { quest: 1 }));
    assert!(
        editor.save(&steps, outcome.notes).is_err(),
        "required work still needs its dungeon"
    );
}

#[test]
fn load_repair_cannot_remove_a_holder_needed_for_reputation_work() {
    let (editor, outcome) = legacy_outcome(false, true);
    let (steps, _) = editor.steps(&outcome);
    assert!(steps.contains(&RouteStep::Accept { quest: 1 }));
    assert!(
        editor
            .simulate(&steps, &Names::default())
            .unfinished
            .iter()
            .any(|q| q.id == 1)
    );
}

#[test]
fn load_repair_does_not_hide_an_unrelated_invalid_turn_in() {
    let (editor, mut outcome) = legacy_outcome(false, false);
    outcome.plan.swap(1, 2);
    let (steps, _) = editor.steps(&outcome);
    assert_eq!(steps, outcome.plan);
    assert!(editor.save(&steps, outcome.notes).is_err());
}
