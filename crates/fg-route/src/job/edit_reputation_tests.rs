use super::*;
use crate::job::{InventoryItem, StartState};
use crate::model::{BoundAcquisition, BoundReputationSource, BoundSourceKind};
use crate::params::ReputationGoal;
use crate::reputation::*;

fn fixture() -> (Editor, Vec<RouteStep>) {
    fixture_with_quests(0)
}

fn fixture_with_quests(quests: usize) -> (Editor, Vec<RouteStep>) {
    let mut p = crate::plan::tests::quests_at_start(quests);
    p.profile.to_level = 1;
    p.params.reputation_goals = vec![ReputationGoal {
        faction: 68,
        target: 100,
    }];
    p.model.initial.reputation = vec![0.0];
    p.model.reputation_sources.sources.clear();
    let pos = p.model.start.pos;
    let loc = SourceLocation {
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
            edition: "classic".into(),
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
            location: Some(loc.clone()),
            confidence: SourceConfidence::Confirmed,
            ..SourceDescriptor::default()
        },
        kind: BoundSourceKind::Turnin,
        locations: vec![loc],
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
    let start = StartState {
        inventory_valid: true,
        inventory: vec![InventoryItem { item: 14047, count: 40 }],
        ..StartState::default()
    };
    let editor = Editor {
        prepared: p,
        start: Some(start),
    };
    let completed = editor
        .planner()
        .complete_reputation(&[], editor.start.as_ref())
        .unwrap();
    assert_eq!(completed.program.result.reached[&68], 100.0);
    (
        editor,
        vec![RouteStep::ReputationProgram {
            program: Box::new(completed.program),
        }],
    )
}

#[test]
fn reputation_program_edit_save_and_timeline_keep_occurrences_and_resources() {
    let (editor, steps) = fixture();
    let names = Names::default();
    let report = editor.simulate(&steps, &names);
    assert_eq!(report.errors, 0);
    assert_eq!(report.total_time, 10.0);
    let saved = editor.save(&steps, vec![]).unwrap();
    assert!(
        export::polestar::validate_route(&saved.route)
            .unwrap_err()
            .to_string()
            .contains("Factoruide Guide")
    );
    let lua = export::to_lua(std::slice::from_ref(&saved.route));
    assert!(lua.contains("rp = {"));
    assert!(lua.contains("kp = true"));
    assert!(lua.contains("[\"68\"] = 100.0"));
    assert_eq!(saved.route.reputation[0].reached, 100.0);
    assert_eq!(saved.route.steps.last().unwrap().kind, StepKind::Reputation);
    assert_eq!(
        saved
            .end
            .as_ref()
            .unwrap()
            .inventory
            .iter()
            .map(|i| i.count)
            .sum::<u32>(),
        0
    );
    let restored: PlanOutcome = serde_json::from_str(&serde_json::to_string(&saved).unwrap()).unwrap();
    let (restored_steps, _) = editor.steps(&restored);
    assert_eq!(restored_steps, saved.plan);
    let program = restored
        .route
        .steps
        .last()
        .unwrap()
        .reputation_program
        .as_ref()
        .unwrap();
    assert_eq!(program.phases[0].cycles, 2);
    let timeline = editor.timeline(&restored, &names);
    assert_eq!(timeline.steps.len(), restored.route.steps.len());
    assert_eq!(timeline.matched, restored.route.steps.len());
    let requests = super::super::draft::Requests::default();
    let view = editor.view(&restored_steps, &names, &requests.next()).unwrap();
    assert_eq!(view.report.errors, 0);
    assert_eq!(view.lines.last().unwrap().kind, StepKind::Reputation);
    assert_eq!(view.report.total_time, 10.0);
}

#[test]
fn reputation_program_editor_rejects_moved_or_duplicate_programs() {
    let (editor, mut steps) = fixture();
    steps.push(RouteStep::Grind { mobs: 1 });
    assert!(editor.save(&steps, vec![]).is_err());
    steps.pop();
    steps.push(steps[0].clone());
    assert!(editor.save(&steps, vec![]).is_err());
}

fn acquired_fixture(turnin_x: f64, acquisition_x: f64) -> (Editor, Vec<RouteStep>) {
    acquired_fixture_with_quests(turnin_x, acquisition_x, 0)
}

fn acquired_fixture_with_quests(turnin_x: f64, acquisition_x: f64, quests: usize) -> (Editor, Vec<RouteStep>) {
    let (mut editor, _) = fixture_with_quests(quests);
    editor.prepared.model.start.pos.x = 1800.0;
    editor.prepared.model.initial.money = 100;
    editor.start.as_mut().unwrap().inventory.clear();
    let source = &mut editor.prepared.model.reputation_sources.sources[0];
    source.locations[0].x = Some(turnin_x);
    source.descriptor.location = Some(source.locations[0].clone());
    let mut location = source.locations[0].clone();
    location.x = Some(acquisition_x);
    let acquisition = SourceAcquisition {
        kind: AcquisitionKind::PricedVendor,
        item_id: 14047,
        units_per_acquisition: 1,
        npc_id: Some(777),
        unit_price_copper: Some(1),
        location: Some(location.clone()),
        ..SourceAcquisition::default()
    };
    source.descriptor.acquisition.push(acquisition.clone());
    source.acquisition.push(BoundAcquisition {
        descriptor: acquisition,
        locations: vec![location],
        creature: None,
        stock: None,
    });
    let completed = editor
        .planner()
        .complete_reputation(&[], editor.start.as_ref())
        .unwrap();
    assert_eq!(completed.program.result.reached[&68], 100.0);
    assert_eq!(completed.program.phases[0].cycles, 2);
    (
        editor,
        vec![RouteStep::ReputationProgram {
            program: Box::new(completed.program),
        }],
    )
}

fn assert_acquired_program_json_roundtrip(turnin_x: f64, acquisition_x: f64) {
    let (editor, steps) = acquired_fixture(turnin_x, acquisition_x);
    let saved = editor.save(&steps, vec![]).unwrap();
    let json = serde_json::to_string(&saved).unwrap();
    let restored: PlanOutcome = serde_json::from_str(&json).unwrap();
    let (restored_steps, rebuilt) = editor.steps(&restored);
    assert!(!rebuilt);
    let requests = super::super::draft::Requests::default();
    let view = editor
        .view(&restored_steps, &Names::default(), &requests.next())
        .unwrap();
    assert_eq!(view.report.errors, 0);
    assert_eq!(view.report.total_time, saved.route.total_time);
    let replayed = view.lines.last().unwrap().reputation_program.as_ref().unwrap();
    assert_eq!(replayed.result.reached[&68], 100.0, "{:?}", replayed.result.warnings);
    assert_eq!(view.report.breakdown.vendor_cost, 40);
    assert!(replayed.result.deficits.is_empty());
    let resaved = editor.save(&restored_steps, vec![]).unwrap();
    assert_eq!(resaved.route.total_time, saved.route.total_time);
    assert_eq!(resaved.route.reputation[0].reached, 100.0);
}

#[test]
fn reputation_program_json_roundtrip_keeps_bound_turnin_location() {
    // A real saved guide coordinate whose default JSON parse changes one ULP.
    assert_acquired_program_json_roundtrip(1775.4436311950685, 1800.0);
}

#[test]
fn reputation_program_json_roundtrip_keeps_bound_acquisition_location() {
    assert_acquired_program_json_roundtrip(1800.0, 1775.4436311950685);
}

#[test]
fn generated_quest_prefix_and_acquired_reputation_program_survive_json_editor_roundtrip() {
    let (mut editor, _) = acquired_fixture_with_quests(1775.4436311950685, 1775.4436311950685, 1);
    let here = editor.prepared.model.start.pos;
    let quest = &mut editor.prepared.model.quests[0];
    quest.reputation = vec![(68, 12.5)];
    quest.required = true;
    quest.starts[0].pos = here;
    quest.ends[0].pos = here;
    let request: PlanRequest = serde_json::from_value(serde_json::json!({
        "race": "human", "class": "warrior", "from_level": 1, "to_level": 1,
        "start": editor.start,
    }))
    .unwrap();
    let interrupt = super::super::Interrupt::default();
    interrupt.stop();
    let (generated, prepared) =
        super::super::plan_prepared(editor.prepared, &request, &|_| {}, &interrupt, None).unwrap();
    let editor = Editor {
        prepared,
        start: request.start,
    };
    assert_eq!(generated.plan.len(), 3);
    assert_eq!(generated.plan[0], RouteStep::Accept { quest: 900_000 });
    assert_eq!(generated.plan[1], RouteStep::TurnIn { quest: 900_000 });
    assert_eq!(generated.route.quests, 1);
    assert_eq!(generated.route.reputation[0].reached, 112.5);
    let program = generated
        .route
        .steps
        .last()
        .unwrap()
        .reputation_program
        .as_ref()
        .unwrap();
    assert_eq!(program.result.start[&68], 12.5);
    assert_eq!(program.phases.len(), 1);
    assert_eq!(program.phases[0].cycles, 2);
    assert_eq!(program.phases[0].actions.len(), 2);
    assert_eq!(program.phases[0].actions[0].kind, RepActionKind::Acquire);
    assert_eq!(program.phases[0].actions[1].kind, RepActionKind::Turnin);

    let persisted = serde_json::to_vec(&generated).unwrap();
    let restored: PlanOutcome = serde_json::from_slice(&persisted).unwrap();
    let (steps, rebuilt) = editor.steps(&restored);
    assert!(!rebuilt);
    assert_eq!(steps, generated.plan);
    let requests = super::super::draft::Requests::default();
    let view = editor.view(&steps, &Names::default(), &requests.next()).unwrap();
    assert_eq!(view.report.errors, 0);
    assert_eq!(view.report.steps.len(), 3);
    assert!(view.report.steps.iter().all(|s| s.status == "done"));
    assert_eq!(view.report.total_time, generated.route.total_time);
    let replayed = view.lines.last().unwrap().reputation_program.as_ref().unwrap();
    assert_eq!(replayed.result.start[&68], 12.5);
    assert_eq!(replayed.result.reached[&68], 112.5);
    assert_eq!(replayed.phases, program.phases);
    assert_eq!(view.report.breakdown.vendor_cost, 40);

    let resaved = editor.save(&steps, vec![]).unwrap();
    assert_eq!(resaved.plan, generated.plan);
    assert_eq!(resaved.route.total_time, generated.route.total_time);
    assert_eq!(resaved.route.quests, 1);
    assert_eq!(resaved.route.reputation[0].reached, 112.5);
    let end = resaved.end.as_ref().unwrap();
    assert_eq!(end.money, 60);
    assert_eq!(end.inventory.iter().map(|i| i.count).sum::<u32>(), 0);
}

#[test]
fn reputation_program_replay_rejects_unbound_action_locations() {
    for kind in [RepActionKind::Acquire, RepActionKind::Turnin] {
        let (editor, mut steps) = acquired_fixture(1800.0, 1800.0);
        let RouteStep::ReputationProgram { program } = &mut steps[0] else {
            unreachable!()
        };
        let action = program.phases[0].actions.iter_mut().find(|a| a.kind == kind).unwrap();
        action.location.as_mut().unwrap().x = Some(1900.0);
        let saved = editor.save(&steps, vec![]).unwrap();
        assert_eq!(saved.route.reputation[0].reached, 0.0);
        let program = saved.route.steps.last().unwrap().reputation_program.as_ref().unwrap();
        assert!(
            program
                .result
                .warnings
                .iter()
                .any(|w| w.contains("no longer supported")),
            "{:?}",
            program.result.warnings
        );
    }
}
