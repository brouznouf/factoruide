//! Opt-in cleanup observation must expose every boundary without changing the search.

use super::{Kind, Stop, planner, quests_at_start};
use crate::plan::{CleanupStage, Effort};
use std::cell::{Cell, RefCell};

const STAGES: [CleanupStage; 7] = [
    CleanupStage::SelectedInput,
    CleanupStage::PowerFirst,
    CleanupStage::Rebind,
    CleanupStage::PlaceHearths,
    CleanupStage::DropUnfinished,
    CleanupStage::TrimTarget,
    CleanupStage::DropDungeonless,
];

// Missing or conditional callbacks would hide unchanged passes from the diagnostic report.
#[test]
fn cleanup_observation_includes_unchanged_boundaries_in_order() {
    let p = quests_at_start(1);
    let initial = vec![Stop::quest(0, Kind::Accept), Stop::quest(0, Kind::TurnIn)];
    let snapshots = RefCell::new(Vec::new());
    let result = planner(&p).improve_while_observed(
        initial.clone(),
        Effort::Tries(0),
        || true,
        |_| {},
        |_, _, _| {},
        |stage, route| snapshots.borrow_mut().push((stage, route.to_vec())),
    );
    let snapshots = snapshots.into_inner();
    assert_eq!(snapshots.iter().map(|(stage, _)| *stage).collect::<Vec<_>>(), STAGES);
    assert!(snapshots.iter().all(|(_, route)| *route == initial));
    assert_eq!(result, initial);
}

// Extra evaluations or random draws in observation can change both accepted moves and output.
#[test]
fn cleanup_observation_preserves_fixed_attempt_search_and_scores() {
    let mut p = super::elwynn_trips();
    p.params.hearth_min_saving = f64::INFINITY;
    let initial = super::elwynn_three_trips();
    let plain_best = RefCell::new(Vec::new());
    let plain = planner(&p).improve_while(
        initial.clone(),
        Effort::Tries(100),
        || true,
        |_| {},
        |score, play, route| plain_best.borrow_mut().push((score, play, route.to_vec())),
    );
    let observed_best = RefCell::new(Vec::new());
    let snapshots = RefCell::new(Vec::new());
    let observed = planner(&p).improve_while_observed(
        initial,
        Effort::Tries(100),
        || true,
        |_| {},
        |score, play, route| observed_best.borrow_mut().push((score, play, route.to_vec())),
        |stage, route| snapshots.borrow_mut().push((stage, route.to_vec())),
    );
    assert_eq!(observed_best.borrow().as_slice(), plain_best.borrow().as_slice());
    assert_eq!(observed, plain);
    let snapshots = snapshots.into_inner();
    assert_eq!(snapshots.len(), STAGES.len());
    assert_eq!(snapshots[0].1, observed_best.borrow().last().unwrap().2);
    assert_eq!(snapshots.last().unwrap().1, observed);
    let plain_score = planner(&p).simulate_full(&plain, None).unwrap().0;
    let observed_score = planner(&p).simulate_full(&observed, None).unwrap().0;
    assert_eq!(observed_score, plain_score);
}

// Stopping attempts is distinct from aborting: a normal stop still observes cleanup.
#[test]
fn cleanup_observation_starts_from_the_selected_route_when_attempts_stop() {
    let p = quests_at_start(2);
    let initial = vec![
        Stop::quest(0, Kind::Accept),
        Stop::quest(0, Kind::TurnIn),
        Stop::quest(1, Kind::Accept),
        Stop::quest(1, Kind::TurnIn),
    ];
    let keep_going_calls = Cell::new(0);
    let snapshots = RefCell::new(Vec::new());
    let result = planner(&p).improve_while_observed(
        initial.clone(),
        Effort::Tries(10),
        || {
            keep_going_calls.set(keep_going_calls.get() + 1);
            false
        },
        |_| {},
        |_, _, _| {},
        |stage, route| snapshots.borrow_mut().push((stage, route.to_vec())),
    );
    assert_eq!(keep_going_calls.get(), 1);
    let snapshots = snapshots.into_inner();
    assert_eq!(snapshots.first(), Some(&(CleanupStage::SelectedInput, initial)));
    assert_eq!(snapshots.last(), Some(&(CleanupStage::DropDungeonless, result)));
}

// Calling observers after abort would expose cleanup that the planner intentionally skips.
#[test]
fn cleanup_observation_is_skipped_after_abort() {
    let p = quests_at_start(1);
    let initial = vec![Stop::quest(0, Kind::Accept), Stop::quest(0, Kind::TurnIn)];
    let abort = Cell::new(false);
    let should_abort = || abort.get();
    let snapshots = Cell::new(0);
    let result = planner(&p).abortable(&should_abort).improve_while_observed(
        initial.clone(),
        Effort::Tries(10),
        || false,
        |_| {},
        |_, _, _| abort.set(true),
        |_, _| snapshots.set(snapshots.get() + 1),
    );
    assert_eq!(result, initial);
    assert_eq!(snapshots.get(), 0);
}
