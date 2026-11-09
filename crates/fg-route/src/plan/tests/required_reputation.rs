use crate::{
    job,
    params::ReputationGoal,
    plan::{Kind, Planner, Stop},
    testing::{At, Obj, Q, Scenario},
};

fn goals(p: &mut crate::params::Params) {
    p.reputation_goals = vec![ReputationGoal {
        faction: 68,
        target: 100,
    }];
    p.reputation_weight = 0.0;
    p.mandatory_penalty = 0.0;
    p.stop_overhead = 1000.0;
    p.camp_radius = 0.0;
}

fn route(q: u32) -> Vec<Stop> {
    vec![Stop::quest(q, Kind::Accept), Stop::quest(q, Kind::TurnIn)]
}

#[test]
fn costly_old_zero_reward_ancestor_is_constructed_after_target_level() {
    let p = Scenario::new()
        .levels(60, 60)
        .quest(Q::new(1, "Old ancestor").level(1).xp(0))
        .quest(
            Q::new(2, "Useful contribution")
                .level(1)
                .xp(0)
                .after(&[1])
                .reputation(68, 100.0),
        )
        .params(goals)
        .build();
    let planner = Planner::new(&p.model, &p.world, &p.params, &p.profile);
    let built = planner.greedy();
    assert_eq!(planner.reputation_reached(&built), vec![100.0]);
    let finalized = job::finalize_route(&planner, Vec::new()).unwrap();
    assert_eq!(planner.reputation_reached(&finalized), vec![100.0]);
}

#[test]
fn reputation_only_hard_rank_prefers_costly_actual_progress_with_gear_off() {
    let p = Scenario::new()
        .levels(20, 21)
        .quest(Q::new(1, "Partial progress").level(20).xp(0).reputation(68, 40.0))
        .params(goals)
        .build();
    let planner = Planner::new(&p.model, &p.world, &p.params, &p.profile);
    let (_, _, empty) = planner.scored_goals(&[]).unwrap();
    let (_, _, partial) = planner.scored_goals(&route(0)).unwrap();
    assert!(partial < empty, "actual fractional deficit must rank ahead of score");
}

#[test]
fn cleanup_preserves_each_satisfied_faction_even_when_another_improves() {
    let p = Scenario::new()
        .levels(20, 21)
        .quest(Q::new(1, "First faction").level(20).xp(0).reputation(68, 100.0))
        .quest(Q::new(2, "Second faction").level(20).xp(0).reputation(67, 1000.0))
        .params(|p| {
            goals(p);
            p.reputation_goals.push(ReputationGoal {
                faction: 67,
                target: 1000,
            });
        })
        .build();
    let planner = Planner::new(&p.model, &p.world, &p.params, &p.profile);
    assert_eq!(planner.keep_hard_goals(route(0), route(1)), route(0));
}

#[test]
fn unsupported_target_returns_leveling_guide_with_named_actual_shortfall() {
    let mut p = Scenario::new().levels(20, 21).params(goals).build();
    p.model.faction_names.insert(68, "Undercity".into());
    let request = serde_json::from_value(serde_json::json!({
        "race":"human", "class":"warrior", "from_level":20, "to_level":21,
    }))
    .unwrap();
    let (outcome, _) = job::plan_prepared(p, &request, &|_| {}, &job::Interrupt::default(), None).unwrap();
    assert_eq!(outcome.route.to_level, 21);
    assert!(outcome.route.total_time.is_finite());
    let warning = outcome.notes.join("; ");
    for expected in [
        "Undercity (68)",
        "start 0",
        "estimated race/class baseline",
        "target 100",
        "actual 0",
        "deficit 100",
        "no supported",
        "best-found",
    ] {
        assert!(warning.contains(expected), "missing {expected:?}: {warning}");
    }
}

#[test]
fn unsupported_target_does_not_discard_another_factions_reachable_reward() {
    let p = Scenario::new()
        .levels(60, 60)
        .quest(Q::new(1, "Reachable faction").level(1).xp(0).reputation(67, 100.0))
        .params(|p| {
            goals(p);
            p.reputation_goals.push(ReputationGoal {
                faction: 67,
                target: 100,
            });
        })
        .build();
    let planner = Planner::new(&p.model, &p.world, &p.params, &p.profile);
    let best = job::finalize_route(&planner, Vec::new()).unwrap();
    assert_eq!(planner.reputation_reached(&best), vec![0.0, 100.0]);
}

#[test]
fn edited_trace_reports_actual_turnin_and_retains_fractional_deficit() {
    let mut p = Scenario::new()
        .levels(20, 21)
        .quest(Q::new(1, "Almost enough").level(20).xp(0).reputation(68, 99.5))
        .params(goals)
        .build();
    p.model.initial.reputation_recorded.insert(68);
    p.model.initial.reputation = vec![0.25];
    let editor = job::Editor::from_prepared(p);
    let accepted = editor.save(&[job::RouteStep::Accept { quest: 1 }], vec![]).unwrap();
    assert!(
        accepted
            .notes
            .iter()
            .any(|note| note.contains("actual 0.25, deficit 99.75"))
    );
    let summary = serde_json::to_value(&accepted.route.reputation).unwrap();
    assert_eq!(summary[0]["start"].as_f64(), Some(0.25));
    assert_eq!(summary[0]["reached"].as_f64(), Some(0.25));
    let completed = editor
        .save(
            &[job::RouteStep::Accept { quest: 1 }, job::RouteStep::TurnIn { quest: 1 }],
            vec![],
        )
        .unwrap();
    let warning = completed.notes.join("; ");
    assert!(warning.contains("actual 99.75, deficit 0.25"), "{warning}");
    assert!(warning.contains("recorded standing"), "{warning}");
    let summary = serde_json::to_value(&completed.route.reputation).unwrap();
    assert_eq!(summary[0]["reached"].as_f64(), Some(99.75));
}

#[test]
fn unmet_reputation_does_not_hide_or_relax_missing_required_gear() {
    let mut p = Scenario::new()
        .levels(20, 21)
        .quest(Q::new(1, "Mandatory equipment").level(20).xp(0))
        .params(goals)
        .build();
    crate::model::select_test_sources(&mut p, &[0], &[]);
    let planner = Planner::new(&p.model, &p.world, &p.params, &p.profile);
    let error = planner.validate_required(&[]).unwrap_err().to_string();
    assert!(
        error.contains("required gear source") && error.contains("Mandatory equipment"),
        "{error}"
    );
    let editor = job::Editor::from_prepared(p);
    let error = editor.save(&[], vec![]).unwrap_err().to_string();
    assert!(
        error.contains("required gear sources") && error.contains("Mandatory equipment"),
        "{error}"
    );
}

#[test]
fn no_targets_keep_optional_old_work_optional() {
    let p = Scenario::new()
        .levels(60, 60)
        .quest(Q::new(1, "Optional").level(1).xp(0).reputation(68, 100.0))
        .build();
    let planner = Planner::new(&p.model, &p.world, &p.params, &p.profile);
    assert_eq!(job::finalize_route(&planner, Vec::new()).unwrap(), Vec::<Stop>::new());
}

#[test]
fn database_keeps_old_useful_roots_alternative_ancestors_and_fractional_rewards() {
    let conn = crate::fixture::Fixture::new(crate::xp::Edition::Classic)
        .toml(
            r#"
[[npc]]
id = 990001
name = "Reputation giver"
friendly_to = "AH"
at = [{ zone = 12, x = 48.0, y = 42.0 }]
[[quest]]
id = 990002
name = "Old ancestor"
level = 1
min_level = 1
giver = 990001
[[quest]]
id = 990003
name = "Fractional reward"
pre_any = [990002, 990004]
level = 1
min_level = 1
giver = 990001
[[quest]]
id = 990004
name = "Other ancestor"
level = 1
min_level = 1
giver = 990001
[[quest]]
id = 990005
name = "Unrelated old work"
level = 1
min_level = 1
giver = 990001
"#,
        )
        .open();
    conn.execute(
        "UPDATE m_quest SET reputation_reward = '[[67,12.5],[68,50],[76,12.5]]' WHERE id=990003",
        [],
    )
    .unwrap();
    let overrides = job::Overrides::load_edition(
        &std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../overrides"),
        crate::xp::Edition::Classic,
    )
    .unwrap();
    let req = serde_json::from_value(serde_json::json!({
        "race":"human", "class":"warrior", "from_level":20, "to_level":21,
        "required_class_quests":[], "params":{"edition":"classic","bis":"off", "reputation_goals":[{"faction":68,"target":100}]}
    })).unwrap();
    let p = job::prepare(&conn, &overrides, &req, &|_| {}).unwrap();
    assert!(p.model.index.contains_key(&990002), "lost gray zero-reward ancestor");
    assert!(p.model.index.contains_key(&990004), "lost prerequisite alternative");
    assert!(!p.model.index.contains_key(&990005), "unrelated old quest retained");
    let q = &p.model.quests[p.model.index[&990003]];
    assert_eq!(
        q.reputation,
        vec![
            (67, 13.750000000000002),
            (68, 55.00000000000001),
            (76, 13.750000000000002)
        ]
    );
}

#[test]
fn repair_keeps_exact_and_gear_work_and_uses_available_alternative() {
    let mut p = Scenario::new()
        .levels(20, 21)
        .quest(Q::new(1, "Required existing work").level(20).xp(0))
        .quest(Q::new(2, "Unavailable alternative").level(1).xp(0).excluded())
        .quest(Q::new(3, "Available old ancestor").level(1).xp(0))
        .quest(
            Q::new(4, "Reward")
                .level(1)
                .xp(0)
                .after_any(&[2, 3])
                .reputation(68, 100.0),
        )
        .required(1)
        .params(goals)
        .build();
    crate::model::select_test_sources(&mut p, &[0], &[]);
    let planner = Planner::new(&p.model, &p.world, &p.params, &p.profile);
    let repaired = job::finalize_route(&planner, route(0)).unwrap();
    let (_, state) = planner.simulate_state(&repaired, None).unwrap();
    assert!(state.rewarded(p.model.index[&1]));
    assert!(state.rewarded(p.model.index[&3]));
    assert!(state.rewarded(p.model.index[&4]));
    assert_eq!(state.reputation, vec![100.0]);
}

#[test]
fn recorded_completions_are_not_counted_twice_and_exclusions_stay_excluded() {
    let mut p = Scenario::new()
        .levels(20, 21)
        .quest(Q::new(1, "Already done").level(1).xp(0).reputation(68, 100.0))
        .quest(Q::new(2, "Excluded").level(1).xp(0).reputation(68, 100.0).excluded())
        .params(goals)
        .build();
    p.model.initial.turned.push(0);
    p.model.initial.reputation = vec![50.0];
    let planner = Planner::new(&p.model, &p.world, &p.params, &p.profile);
    assert_eq!(planner.reputation_reached(&route(0)), vec![50.0]);
    let best = job::finalize_route(&planner, Vec::new()).unwrap();
    assert_eq!(planner.reputation_reached(&best), vec![50.0]);
}

#[test]
fn satisfied_explicit_trace_and_partial_acceptance_both_validate_structurally() {
    let p = Scenario::new()
        .levels(20, 21)
        .quest(Q::new(1, "Reward").level(20).xp(0).reputation(68, 100.0))
        .params(goals)
        .build();
    let planner = Planner::new(&p.model, &p.world, &p.params, &p.profile);
    let edit = |kind| super::super::Edit::Stop {
        stop: Stop::quest(0, kind),
        farm: false,
        spot: None,
    };
    let accepted = planner.edit(&[edit(Kind::Accept)]);
    planner.validate_required_trace(&accepted.trace).unwrap();
    let completed = planner.edit(&[edit(Kind::Accept), edit(Kind::TurnIn)]);
    planner.validate_required_trace(&completed.trace).unwrap();
}

#[test]
fn mate_only_turnin_does_not_satisfy_the_characters_reputation() {
    let mut p = Scenario::new()
        .levels(20, 21)
        .quest(Q::new(1, "Mate only").level(20).xp(0).reputation(68, 100.0))
        .params(goals)
        .build();
    p.model.quests[0].members = 2;
    p.model.initial.need = vec![2];
    let planner = Planner::new(&p.model, &p.world, &p.params, &p.profile);
    assert_eq!(planner.reputation_reached(&route(0)), vec![0.0]);
    let mut trace = Vec::new();
    planner.simulate(&route(0), Some(&mut trace)).unwrap();
    planner.validate_required_trace(&trace).unwrap();
}

#[test]
fn old_held_parent_stays_in_log_until_reputation_child_is_accepted() {
    let p = Scenario::new()
        .levels(60, 60)
        .quest(Q::new(1, "Held only").level(1).xp(0))
        .quest(
            Q::new(2, "Reputation child")
                .level(1)
                .xp(0)
                .while_in_log(&[1])
                .reputation(68, 100.0),
        )
        .params(|p| {
            goals(p);
            p.camp_radius = 100.0;
        })
        .build();
    let planner = Planner::new(&p.model, &p.world, &p.params, &p.profile);
    let route = job::finalize_route(&planner, Vec::new()).unwrap();
    assert_eq!(planner.reputation_reached(&route), vec![100.0]);
}

#[test]
fn recorded_held_parent_survives_replay_without_invented_turnin() {
    let mut p = Scenario::new()
        .levels(60, 60)
        .quest(
            Q::new(2, "Reputation child")
                .level(1)
                .xp(0)
                .while_in_log(&[1])
                .reputation(68, 100.0),
        )
        .quest(Q::new(1, "Recorded parent").level(1).xp(0))
        .params(goals)
        .build();
    p.model.initial.accepted = vec![1];
    let planner = Planner::new(&p.model, &p.world, &p.params, &p.profile);
    assert_eq!(planner.reputation_reached(&route(0)), vec![100.0]);
}

#[test]
fn final_cleanup_preserves_progress_toward_still_unmet_target() {
    let p = Scenario::new()
        .levels(20, 21)
        .quest(Q::new(1, "More progress").level(20).xp(0).reputation(68, 40.0))
        .quest(Q::new(2, "Less progress").level(20).xp(0).reputation(68, 30.0))
        .params(goals)
        .build();
    let planner = Planner::new(&p.model, &p.world, &p.params, &p.profile);
    assert_eq!(planner.keep_hard_goals(route(0), route(1)), route(0));
}

#[test]
fn unavailable_or_blocked_held_child_does_not_hold_its_rewarding_parent() {
    for excluded in [false, true] {
        let child = Q::new(2, "Blocked child")
            .level(1)
            .xp(0)
            .while_in_log(&[1])
            .exclusive_with(&[3])
            .reputation(68, 100.0);
        let mut p = Scenario::new()
            .levels(60, 60)
            .quest(Q::new(1, "Useful parent").level(1).xp(0).reputation(68, 100.0))
            .quest(if excluded { child.excluded() } else { child })
            .params(goals)
            .build();
        p.model.initial.completed = Some([3].into_iter().collect());
        let planner = Planner::new(&p.model, &p.world, &p.params, &p.profile);
        let repaired = job::finalize_route(&planner, Vec::new()).unwrap();
        assert_eq!(planner.reputation_reached(&repaired), vec![100.0]);
    }
}

#[test]
fn accepted_reputation_child_releases_parent_needed_for_its_handed_item() {
    for camp_radius in [0.0, 100.0] {
        let p = Scenario::new()
            .levels(60, 60)
            .quest(Q::new(1, "Held parent and item supplier").level(1).xp(0))
            .quest(
                Q::new(2, "Child needing supplied item")
                    .level(1)
                    .xp(0)
                    .while_in_log(&[1])
                    .objective(
                        Obj::visit(
                            "Use supplied item",
                            At {
                                zone: 1,
                                x: 50.0,
                                y: 50.0,
                            },
                        )
                        .handed_by(&[1]),
                    )
                    .reputation(68, 100.0),
            )
            .params(|p| {
                goals(p);
                p.camp_radius = camp_radius;
            })
            .build();
        let planner = Planner::new(&p.model, &p.world, &p.params, &p.profile);
        let witness = vec![
            Stop::quest(0, Kind::Accept),
            Stop::quest(1, Kind::Accept),
            Stop::quest(0, Kind::TurnIn),
            Stop::quest(1, Kind::Objective(0)),
            Stop::quest(1, Kind::TurnIn),
        ];
        assert_eq!(planner.reputation_reached(&witness), vec![100.0]);
        planner.validate_required(&witness).unwrap();
        let constructed = planner.greedy();
        assert_eq!(
            planner.reputation_reached(&constructed),
            vec![100.0],
            "camp radius {camp_radius}, route {constructed:?}"
        );
        let finalized = job::finalize_route(&planner, Vec::new()).unwrap();
        assert_eq!(planner.reputation_reached(&finalized), vec![100.0]);
    }
}
