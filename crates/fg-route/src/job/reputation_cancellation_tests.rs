use super::{Interrupt, JobError, JobEvent, PlanRequest, Prepared, plan_prepared};
use crate::job::{InventoryItem, StartState};
use crate::model::{BoundReputationSource, BoundSourceKind};
use crate::params::ReputationGoal;
use crate::reputation::{InputItem, ReputationEffects, SignedAward, SourceCosts, SourceDescriptor, SourceLocation};
use std::sync::atomic::{AtomicBool, Ordering};

fn fixture() -> (Prepared, PlanRequest) {
    let mut p = crate::plan::tests::quests_at_start(0);
    p.profile.to_level = 1;
    p.params.threads = 1;
    p.params.candidates = 1;
    p.params.qualify_tries = 0.0;
    p.params.round_tries = 0.0;
    p.params.final_tries = 0.0;
    p.params.reputation_goals = vec![ReputationGoal {
        faction: 68,
        target: 100,
    }];
    p.model.initial.reputation = vec![0.0];
    p.model.reputation_sources.sources.clear();
    let pos = p.model.start.pos;
    let location = SourceLocation {
        world_id: Some(pos.continent as u32),
        zone_id: Some(p.model.start.zone),
        x: Some(pos.x),
        y: Some(pos.y),
        ..SourceLocation::default()
    };
    p.model.reputation_sources.sources.push(BoundReputationSource {
        descriptor: SourceDescriptor {
            id: "cloth".into(),
            name: "Cloth delivery".into(),
            repeatable: true,
            effects: ReputationEffects {
                awards: vec![SignedAward {
                    faction_id: 68,
                    amount: 50.0,
                    ..SignedAward::default()
                }],
                ..ReputationEffects::default()
            },
            input_items: vec![InputItem {
                item_id: 14047,
                count: 20,
                starter: false,
            }],
            costs: SourceCosts {
                interaction_seconds: 5.0,
                ..SourceCosts::default()
            },
            location: Some(location.clone()),
            ..SourceDescriptor::default()
        },
        kind: BoundSourceKind::Turnin,
        locations: vec![location],
        giver_locations: vec![],
        creature: None,
        acquisition: vec![],
        prerequisite_any: vec![],
        prerequisite_count: 1,
        supported: true,
        automatic_farming: false,
        warnings: vec![],
        quest_flags: 0,
        quest_flags_ex: 0,
        quest_flags_ex2: 0,
        selected_patch: None,
    });
    let mut req: PlanRequest =
        serde_json::from_str(r#"{"race":"orc","class":"warrior","from_level":1,"to_level":1}"#).unwrap();
    req.start = Some(StartState {
        inventory_valid: true,
        inventory: vec![InventoryItem { item: 14047, count: 40 }],
        ..StartState::default()
    });
    (p, req)
}

#[test]
fn reputation_completion_cancellation_does_not_export_or_persist_a_guide() {
    let (prepared, req) = fixture();
    let interrupt = Interrupt::default();
    let entered = AtomicBool::new(false);
    let progress = |event: &JobEvent<'_>| {
        if matches!(event, JobEvent::Status("Planning reputation activities")) {
            entered.store(true, Ordering::Relaxed);
            interrupt.cancel();
        }
    };
    let outcome = plan_prepared(prepared, &req, &progress, &interrupt, None);
    assert!(entered.load(Ordering::Relaxed));
    assert!(
        matches!(outcome, Err(JobError::Cancelled)),
        "cancellation during completion must not return a publishable guide"
    );
}

#[test]
fn reputation_completion_stop_keeps_best_route_and_finishes_supported_sources() {
    let (prepared, req) = fixture();
    let interrupt = Interrupt::default();
    let entered = AtomicBool::new(false);
    let progress = |event: &JobEvent<'_>| {
        if matches!(event, JobEvent::Status("Planning reputation activities")) {
            entered.store(true, Ordering::Relaxed);
            interrupt.stop();
        }
    };
    let (outcome, _) = plan_prepared(prepared, &req, &progress, &interrupt, None).unwrap();
    assert!(entered.load(Ordering::Relaxed));
    assert!(!interrupt.cancelled());
    assert_eq!(outcome.route.reputation[0].reached, 100.0);
    assert_eq!(outcome.route.total_time, 10.0);
    assert_eq!(
        outcome
            .end
            .unwrap()
            .inventory
            .iter()
            .map(|item| item.count)
            .sum::<u32>(),
        0
    );
}

#[test]
fn reputation_completion_late_cancellation_cannot_return_a_finished_guide() {
    let (prepared, req) = fixture();
    let interrupt = Interrupt::default();
    let completing = AtomicBool::new(false);
    let cancelled = AtomicBool::new(false);
    let progress = |event: &JobEvent<'_>| {
        if matches!(event, JobEvent::Status("Planning reputation activities")) {
            completing.store(true, Ordering::Relaxed);
        } else if matches!(event, JobEvent::Status(_)) && completing.load(Ordering::Relaxed) {
            cancelled.store(true, Ordering::Relaxed);
            interrupt.cancel();
        }
    };
    let outcome = plan_prepared(prepared, &req, &progress, &interrupt, None);
    assert!(cancelled.load(Ordering::Relaxed));
    assert!(
        matches!(outcome, Err(JobError::Cancelled)),
        "late cancellation must be checked before returning a finished guide"
    );
}
