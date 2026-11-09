//! Dungeon cleanup keeps a removed quest's useful class-training service independently.

use super::*;
use crate::plan::Event;
use crate::testing::{At, Obj, Q, Scenario};

fn training_fixture() -> Prepared {
    class_training_fixture("human", "mage")
}

fn class_training_fixture(race: &str, class: &str) -> Prepared {
    let conn = base_of("classic");
    let overrides = Overrides::load_edition(&repo().join("overrides"), Edition::Classic).unwrap();
    let req: PlanRequest = serde_json::from_value(serde_json::json!({
        "race":race, "class":class, "from_level":20, "to_level":21,
        "required_class_quests":[], "start":{"level":20, "spells":[]}
    }))
    .unwrap();
    let mut original = class_prepared(&conn, &overrides, &req);
    let power = original.model.power.take();
    let mut p = at_start(original, 2);
    p.model.power = power;
    p.profile.from_level = 20;
    p.profile.to_level = 21;
    p.model.initial.trained = Some(1);
    p.params.train_radius = 100.0;
    p.params.train_min_gain = 0.000001;
    p.params.train_trip_gain = f64::INFINITY;
    let mut trainer = p.model.start.clone();
    trainer.pos.x += 1000.0;
    trainer.id = 12345;
    p.model.quests[0].starts = vec![trainer.clone()];
    p.model.trainers = vec![trainer];
    let dungeon = Scenario::new()
        .levels(20, 21)
        .dungeon("Missing dungeon", 0, 0.0, 0.0)
        .quest(
            Q::new(901, "Missing quest").level(20).objective(
                Obj::kill(
                    "Dungeon mob",
                    20,
                    1,
                    At {
                        zone: 0,
                        x: 0.0,
                        y: 0.0,
                    },
                )
                .inside(0),
            ),
        )
        .build();
    p.model.dungeons = dungeon.model.dungeons;
    p.model.dungeon_loot = dungeon.model.dungeon_loot;
    p.model.quests[0].objectives = dungeon.model.quests[0].objectives.clone();
    p.model.quests[0].level = 20;
    p.model.quests[1].level = 20;
    p
}

#[test]
fn cleanup_training_preserves_useful_accept_training_after_removing_dungeon_quest() {
    let p = training_fixture();
    let plan = planner(&p);
    let original = [Stop::quest(0, Kind::Accept), Stop::quest(1, Kind::Accept)];
    let bare = [original[1]];
    let mut trace = Vec::new();
    let (_, trained) = plan.simulate_full(&original, Some(&mut trace)).unwrap();
    assert_eq!(
        trace.iter().filter(|t| matches!(t.event, Event::Train { .. })).count(),
        1
    );
    let mut bare_trace = Vec::new();
    let (bare_score, untrained) = plan.simulate_full(&bare, Some(&mut bare_trace)).unwrap();
    assert!(!bare_trace.iter().any(|t| matches!(t.event, Event::Train { .. })));
    assert!(
        trained.grinding < untrained.grinding,
        "real spell training improves downstream combat"
    );
    let cleaned = plan.drop_dungeonless(&original).unwrap();
    assert!(!cleaned.iter().any(|s| s.is_quest() && s.index == 0));
    trace.clear();
    let (score, _) = plan.simulate_full(&cleaned, Some(&mut trace)).unwrap();
    assert_eq!(
        trace.iter().filter(|t| matches!(t.event, Event::Train { .. })).count(),
        1,
        "cleanup must retain the useful training as its own action"
    );
    assert!(score < bare_score);
}

fn train(trainer: u32) -> Stop {
    Stop {
        index: trainer,
        kind: Kind::Train,
    }
}

fn edit(stop: Stop) -> super::super::Edit {
    super::super::Edit::Stop {
        stop,
        farm: false,
        spot: None,
    }
}

#[test]
fn cleanup_training_bare_deletion_wins_without_benefit_or_trainer() {
    for no_trainer in [false, true] {
        let mut p = training_fixture();
        if no_trainer {
            p.model.trainers.clear();
        } else {
            // The lesson's cost exceeds all the combat time it could save.
            p.params.train_time = 1_000_000.0;
        }
        let route = [Stop::quest(0, Kind::Accept), Stop::quest(1, Kind::Accept)];
        assert_eq!(planner(&p).drop_dungeonless(&route).unwrap(), [route[1]]);
    }
}

#[test]
fn cleanup_training_does_not_attribute_a_retained_slots_training_to_a_removed_slot() {
    let mut p = training_fixture();
    p.model.quests[1].starts = p.model.quests[0].starts.clone();
    let route = [Stop::quest(1, Kind::Accept), Stop::quest(0, Kind::Accept)];
    let plan = planner(&p);
    let mut trace = Vec::new();
    plan.simulate_full(&route, Some(&mut trace)).unwrap();
    assert_eq!(
        trace.iter().filter(|t| matches!(t.event, Event::Train { .. })).count(),
        1
    );
    assert_eq!(plan.drop_dungeonless(&route).unwrap(), [route[0]]);
}

#[test]
fn cleanup_training_tie_favors_bare_deletion_when_next_stop_supplies_the_lesson() {
    let mut p = training_fixture();
    p.model.quests[1].starts = p.model.quests[0].starts.clone();
    let route = [Stop::quest(0, Kind::Accept), Stop::quest(1, Kind::Accept)];
    let plan = planner(&p);
    assert_eq!(
        plan.simulate(&[train(0), route[1]], None),
        plan.simulate(&[route[1]], None)
    );
    assert_eq!(plan.drop_dungeonless(&route).unwrap(), [route[1]]);
}

#[test]
fn cleanup_training_rebuilds_provenance_for_multiple_deletions_and_removes_every_quest_stop() {
    let mut p = training_fixture();
    let mut other = p.model.quests[0].clone();
    other.id += 100;
    p.model.quests.push(other);
    p.model.chain_top.push(1);
    p.model.unlocks.push(0.0);
    p.model.rewards.push((vec![], vec![]));
    p.model.bundles.push(vec![]);
    let route = [
        Stop::quest(0, Kind::Accept),
        Stop::quest(2, Kind::Accept),
        Stop::quest(0, Kind::Accept),
        Stop::quest(2, Kind::Accept),
        Stop::quest(1, Kind::Accept),
    ];
    let plan = planner(&p);
    // Deleting quest 0 first is a tie: quest 2 now supplies its lesson. On the
    // original route quest 2 supplied none, so stale provenance would lose it.
    let cleaned = plan.drop_dungeonless(&route).unwrap();
    assert_eq!(cleaned, [train(0), route[4]]);
    let mut trace = Vec::new();
    plan.simulate_full(&cleaned, Some(&mut trace)).unwrap();
    assert_eq!(
        trace.iter().filter(|t| matches!(t.event, Event::Train { .. })).count(),
        1
    );
}

#[test]
fn cleanup_training_respects_dungeon_runs_mandatory_quests_and_prerequisites() {
    let mut p = training_fixture();
    let route = [Stop::quest(0, Kind::Accept), Stop::quest(1, Kind::Accept)];
    p.model.quests[0].mandatory = true;
    assert!(planner(&p).drop_dungeonless(&route).is_none());
    p.model.quests[0].mandatory = false;
    p.model.quests[1].pre_held = vec![p.model.quests[0].id];
    assert!(planner(&p).simulate(&route, None).is_some());
    assert!(planner(&p).drop_dungeonless(&route).is_none());
    p.model.quests[1].pre_held.clear();
    let with_dungeon = [
        route[0],
        Stop {
            index: 0,
            kind: Kind::Dungeon,
        },
        Stop::quest(0, Kind::Objective(0)),
        route[1],
    ];
    assert!(planner(&p).drop_dungeonless(&with_dungeon).is_none());
}

#[test]
fn cleanup_training_stop_is_a_valid_noop_when_learned_but_rejects_unknown_trainer() {
    let p = training_fixture();
    let plan = planner(&p);
    let once = [train(0)];
    let twice = [train(0), train(0)];
    let mut trace = Vec::new();
    let (score, spent) = plan.simulate_full(&twice, Some(&mut trace)).unwrap();
    assert_eq!(score, plan.simulate(&once, None).unwrap());
    assert_eq!(spent.training, p.params.train_time);
    assert_eq!(
        trace.iter().filter(|t| matches!(t.event, Event::Train { .. })).count(),
        1
    );
    assert!(!trace.iter().any(|t| matches!(t.event, Event::Stop { .. })));
    assert!(plan.simulate(&[train(1)], None).is_none());
    assert!(plan.simulate(&[train(0), train(1)], None).is_none());
    let bad = plan.explicit().edit(&[edit(train(1))]);
    assert_eq!(bad.steps[0].outcome, super::super::Outcome::Invalid("unknown trainer"));
}

#[test]
fn cleanup_training_incremental_evaluation_matches_full_replay() {
    let p = training_fixture();
    let plan = planner(&p);
    let original = [Stop::quest(0, Kind::Accept), Stop::quest(1, Kind::Accept)];
    let candidate = [train(0), original[1]];
    let run = plan.run(&original).unwrap();
    let incremental = plan.evaluate(&run, &candidate, 0, 0, false).unwrap();
    assert!((incremental - plan.simulate(&candidate, None).unwrap()).abs() < 1e-6);
}

#[test]
fn cleanup_training_added_removed_and_reordered_stops_replay_training_state_and_cost() {
    for (race, class) in [("human", "mage"), ("dwarf", "hunter")] {
        let mut p = class_training_fixture(race, class);
        if class == "mage" {
            p.model.teleports.push(crate::model::Teleport {
                name: "Teleport: Stormwind".into(),
                level: 20,
                ability: 0,
                to: p.model.trainers[0].clone(),
            });
        }
        let plan = planner(&p);
        let bare = [Stop::quest(1, Kind::Accept)];
        let added = [train(0), bare[0]];
        let reordered = [bare[0], train(0)];
        let bare_run = plan.run(&bare).unwrap();
        let trained_run = plan.run(&added).unwrap();
        for (run, candidate, same_set, last) in [
            (&bare_run, added.as_slice(), false, 0),
            (&trained_run, bare.as_slice(), false, 0),
            (&trained_run, reordered.as_slice(), true, 1),
        ] {
            let incremental = plan.evaluate(run, candidate, 0, last, same_set).unwrap();
            assert!((incremental - plan.simulate(candidate, None).unwrap()).abs() < 1e-6);
        }
        let learned = &trained_run.states[1];
        assert_eq!(learned.trained, 20);
        assert!(learned.spent.training_cost > 0, "real ranks cost copper");
        assert_eq!(learned.spent.training, p.params.train_time);
        if class == "hunter" {
            assert_ne!(learned.abilities & p.model.abilities.bit(5118).unwrap(), 0);
        } else {
            assert_eq!(learned.taught, 1);
        }
        // Replay the suffix from the saved post-training state, then compare the
        // training-sensitive final state and cost with a fresh complete run.
        let mut incremental = learned.clone();
        incremental.at = 1;
        incremental.next = plan.sim().next_pos(&incremental, &added, 1);
        plan.sim().apply(&mut incremental, bare[0], None);
        plan.sim().finish(&mut incremental, None);
        let (_, full) = plan.simulate_state(&added, None).unwrap();
        assert_eq!(incremental.trained, full.trained);
        assert_eq!(incremental.abilities, full.abilities);
        assert_eq!(incremental.taught, full.taught);
        assert_eq!(incremental.spent.training_cost, full.spent.training_cost);
        assert_eq!(incremental.spent.training, full.spent.training);
        assert_eq!(incremental.bonus, full.bonus);
        assert_eq!(bare_run.states[1].trained, 1);
        assert_eq!(bare_run.states[1].spent.training_cost, 0);
    }
}

#[test]
fn cleanup_training_stop_exports_one_training_action_and_replays_one_lesson() {
    let p = training_fixture();
    let plan = planner(&p);
    let guide = crate::export::build(&plan, &p.profile, &[train(0)]);
    assert_eq!(
        guide
            .steps
            .iter()
            .filter(|s| s.kind == crate::export::StepKind::Train)
            .count(),
        1
    );
    assert!(
        !guide
            .steps
            .iter()
            .any(|s| s.kind == crate::export::StepKind::FlightMaster)
    );
    let expanded = plan.expand(&[edit(train(0))]);
    assert_eq!(
        expanded
            .iter()
            .filter(|e| matches!(e, super::super::Edit::Train { trainer: 0 }))
            .count(),
        1
    );
    let replayed = plan.explicit().edit(&expanded);
    assert_eq!(replayed.errors(), 0);
    assert_eq!(replayed.breakdown.training, p.params.train_time);
    assert_eq!(replayed.breakdown.overhead, 0.0);
}

#[test]
fn cleanup_training_stop_leaves_dungeon_and_preserves_landing_walk_accounting() {
    use super::super::state::Landed;
    let p = training_fixture();
    let plan = planner(&p).explicit();
    let sim = plan.sim();
    let mut s = sim.start_state(plan.planned(&[train(0)]));
    s.inside = Some(0);
    s.landed = Landed::Flight;
    // Within the ordinary detour radius: a landing must still walk to the trainer,
    // instead of going there and back and leaving the character at the entrance.
    s.pos = p.model.trainers[0].pos;
    s.pos.x -= 50.0;
    let mut trace = Vec::new();
    sim.apply(&mut s, train(0), Some(&mut trace));
    assert_eq!(s.inside, None);
    assert_eq!(s.pos, p.model.trainers[0].pos);
    assert!(s.spent.flights > 0.0);
    assert_eq!(s.spent.travel, 0.0);
    assert_eq!(s.spent.training, p.params.train_time);
    assert_eq!(s.spent.overhead, 0.0);
    assert_eq!(trace.len(), 1);
    assert!(matches!(trace[0].event, Event::Train { .. }));
}
