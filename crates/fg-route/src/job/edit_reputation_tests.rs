use super::*;
use crate::job::{InventoryItem, StartState};
use crate::model::{BoundReputationSource, BoundSourceKind};
use crate::params::ReputationGoal;
use crate::reputation::*;

fn fixture() -> (Editor, Vec<RouteStep>) {
    let mut p = crate::plan::tests::quests_at_start(0);
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
