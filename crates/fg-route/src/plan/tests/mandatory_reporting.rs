use crate::{
    model::{MandatoryOrigins as Origins, select_test_sources},
    plan::{Kind, MandatoryGoalCounts, Planner, Stop},
    testing::{At, Obj, Q, Scenario},
};

#[test]
fn missing_selected_gear_root_and_prerequisite_have_authoritative_gear_counts() {
    let mut p = Scenario::new()
        .levels(1, 5)
        .quest(Q::new(1, "Support").xp(0))
        .quest(Q::new(2, "Gear source").xp(0).after(&[1]))
        .build();
    select_test_sources(&mut p, &[1], &[]);
    let planner = Planner::new(&p.model, &p.world, &p.params, &p.profile);
    let (_, spent) = planner.simulate_full(&[], None).unwrap();
    assert_eq!(spent.missing_class_quests, 2, "legacy non-zone aggregate");
    assert_eq!(
        spent.mandatory_goal_counts,
        Some(MandatoryGoalCounts {
            class_quests: 0,
            bis_quests: 2,
            other_quests: 0
        })
    );
    let notes = planner.missing_mandatory_quests(&[]);
    assert_eq!(notes.len(), 2);
    assert!(
        notes
            .iter()
            .all(|n| n.contains("gear quest or prerequisite not completed"))
    );
    assert!(notes.iter().all(|n| n.contains("local insertion check")));
}

#[test]
fn deep_overlapping_origins_preserve_legacy_zone_order_and_score() {
    for zone_last in [false, true] {
        let class = Q::new(3, "Class").xp(0).after(&[2]).mandatory_from(Origins::CLASS);
        let zone = Q::new(4, "Zone")
            .xp(0)
            .after(&[2])
            .mandatory_from(Origins::FORCED_ZONE)
            .zone_forced();
        let (first, last) = if zone_last { (class, zone) } else { (zone, class) };
        let mut p = Scenario::new()
            .levels(1, 5)
            .quest(Q::new(1, "Grandparent").xp(0))
            .quest(Q::new(2, "Shared").xp(0).after(&[1]))
            .quest(first)
            .quest(last)
            .build();
        let class = p.model.index[&3] as u32;
        select_test_sources(&mut p, &[class], &[]);
        for id in [1, 2, 3] {
            let q = &p.model.quests[p.model.index[&id]];
            assert!(q.mandatory_origins.contains(Origins::CLASS), "class did not reach {id}");
            assert!(q.mandatory_origins.contains(Origins::GEAR), "gear did not reach {id}");
        }
        for id in [1, 2] {
            let q = &p.model.quests[p.model.index[&id]];
            assert!(q.mandatory_origins.contains(Origins::FORCED_ZONE));
            assert!(q.mandatory);
            assert_eq!(q.zone_forced, zone_last, "legacy first propagating root wins");
        }
        let planner = Planner::new(&p.model, &p.world, &p.params, &p.profile);
        let (score, spent) = planner.simulate_full(&[], None).unwrap();
        assert_eq!(spent.missing_class_quests, if zone_last { 1 } else { 3 });
        assert_eq!(spent.missing_zone_quests, if zone_last { 3 } else { 1 });
        assert_eq!(
            spent.mandatory_goal_counts,
            Some(MandatoryGoalCounts {
                class_quests: 3,
                bis_quests: 3,
                other_quests: 0,
            })
        );
        for q in &mut p.model.quests {
            q.mandatory_origins = Origins::default();
        }
        let (unclassified_score, old) = Planner::new(&p.model, &p.world, &p.params, &p.profile)
            .simulate_full(&[], None)
            .unwrap();
        assert_eq!(score, unclassified_score);
        assert_eq!(spent.missing_class_quests, old.missing_class_quests);
        assert_eq!(spent.missing_zone_quests, old.missing_zone_quests);
    }
}

#[test]
fn actual_completion_and_exclusive_representatives_drive_notes_and_counts() {
    let mut p = Scenario::new()
        .levels(1, 2)
        .params(|p| p.camp_radius = 0.0)
        .quest(
            Q::new(1, "Class A")
                .xp(0)
                .mandatory_from(Origins::CLASS)
                .exclusive_with(&[2]),
        )
        .quest(
            Q::new(2, "Class B")
                .xp(0)
                .mandatory_from(Origins::CLASS)
                .exclusive_with(&[1]),
        )
        .quest(Q::new(3, "Leveling").xp(100000))
        .build();
    let route = [
        Stop::quest(2, Kind::Accept),
        Stop::quest(2, Kind::TurnIn),
        Stop::quest(0, Kind::Accept),
        Stop::quest(0, Kind::TurnIn),
    ];
    let planner = Planner::new(&p.model, &p.world, &p.params, &p.profile);
    let (_, spent) = planner.simulate_full(&route, None).unwrap();
    assert_eq!(spent.missing_class_quests, 1);
    let notes = planner.missing_class_quests(&route);
    assert_eq!(
        notes.len(),
        1,
        "intended post-target turn-in must remain missing: {notes:?}"
    );
    assert!(notes[0].contains("Class A"));
    let (_, completed) = planner
        .simulate_full(&[Stop::quest(1, Kind::Accept), Stop::quest(1, Kind::TurnIn)], None)
        .unwrap();
    assert_eq!(completed.mandatory_goal_counts, Some(MandatoryGoalCounts::default()));
    assert_eq!(
        planner.missing_class_quests(&[Stop::quest(1, Kind::Accept), Stop::quest(1, Kind::TurnIn)]),
        Vec::<String>::new()
    );
    p.model.initial.turned = vec![1];
    let planner = Planner::new(&p.model, &p.world, &p.params, &p.profile);
    assert_eq!(planner.missing_class_quests(&[]), Vec::<String>::new());
    assert_eq!(
        planner.simulate_full(&[], None).unwrap().1.mandatory_goal_counts,
        Some(MandatoryGoalCounts::default())
    );
}

#[test]
fn held_only_and_unchosen_gear_support_do_not_acquire_completion_origins() {
    let mut p = Scenario::new()
        .levels(1, 5)
        .quest(Q::new(1, "Held parent").xp(0).mandatory_from(Origins::CLASS))
        .quest(Q::new(2, "First choice").xp(0))
        .quest(Q::new(3, "Unchosen").xp(0))
        .quest(Q::new(4, "Source").xp(0).while_in_log(&[1]).after_any(&[2, 3]))
        .build();
    select_test_sources(&mut p, &[3], &[]);
    assert!(!p.model.bis.requires(0));
    assert!(!p.model.quests[0].mandatory_origins.contains(Origins::GEAR));
    assert!(p.model.quests[0].mandatory_origins.contains(Origins::CLASS));
    assert!(!p.model.quests[2].mandatory);
    assert_eq!(p.model.quests[2].mandatory_origins, Origins::default());
    let spent = Planner::new(&p.model, &p.world, &p.params, &p.profile)
        .simulate_full(&[], None)
        .unwrap()
        .1;
    assert_eq!(
        spent.mandatory_goal_counts,
        Some(MandatoryGoalCounts {
            class_quests: 1,
            bis_quests: 2,
            other_quests: 0,
        })
    );
}

#[test]
fn generic_and_exact_goals_are_other_and_fresh_empty_counts_are_authoritative() {
    let p = Scenario::new()
        .levels(1, 5)
        .quest(Q::new(1, "Generic").mandatory())
        .quest(Q::new(2, "Exact support"))
        .quest(Q::new(3, "Exact").after(&[2]))
        .required(3)
        .build();
    let spent = Planner::new(&p.model, &p.world, &p.params, &p.profile)
        .simulate_full(&[], None)
        .unwrap()
        .1;
    assert_eq!(
        spent.mandatory_goal_counts,
        Some(MandatoryGoalCounts {
            class_quests: 0,
            bis_quests: 0,
            other_quests: 3,
        })
    );
    for id in [2, 3] {
        assert!(
            p.model.quests[p.model.index[&id]]
                .mandatory_origins
                .contains(Origins::EXACT)
        );
    }
    let p = Scenario::new().levels(1, 5).build();
    let planner = Planner::new(&p.model, &p.world, &p.params, &p.profile);
    for _ in 0..2 {
        assert_eq!(
            planner.simulate_full(&[], None).unwrap().1.mandatory_goal_counts,
            Some(MandatoryGoalCounts::default())
        );
    }
}

#[test]
fn exclusive_mixed_origins_share_one_penalty_and_keep_both_categories() {
    for reverse in [false, true] {
        let class = Q::new(1, "Class version")
            .mandatory_from(Origins::CLASS)
            .exclusive_with(&[2]);
        let gear = Q::new(2, "Gear version").exclusive_with(&[1]);
        let (first, second) = if reverse { (gear, class) } else { (class, gear) };
        let mut p = Scenario::new().levels(1, 5).quest(first).quest(second).build();
        let root = p.model.index[&2] as u32;
        // The selector correctly protects a selected class goal; this test isolates only the
        // existing reporting representation with independently modeled completion origins.
        p.model.quests[root as usize].mandatory = true;
        p.model.quests[root as usize].mandatory_origins = Origins::GEAR;
        let planner = Planner::new(&p.model, &p.world, &p.params, &p.profile);
        let spent = planner.simulate_full(&[], None).unwrap().1;
        assert_eq!(spent.missing_class_quests, 1);
        assert_eq!(
            spent.mandatory_goal_counts,
            Some(MandatoryGoalCounts {
                class_quests: 1,
                bis_quests: 1,
                other_quests: 0,
            })
        );
        let notes = planner.missing_mandatory_quests(&[]);
        assert_eq!(notes.len(), 1);
        assert!(notes[0].contains("class and gear quest not completed"));
    }
}

#[test]
fn categorized_breakdown_round_trips_without_inventing_legacy_categories() {
    let p = Scenario::new().levels(1, 5).quest(Q::new(1, "Optional")).build();
    let fresh = Planner::new(&p.model, &p.world, &p.params, &p.profile)
        .simulate_full(&[], None)
        .unwrap()
        .1;
    let json = serde_json::to_value(fresh).unwrap();
    assert_eq!(
        json["mandatory_goal_counts"],
        serde_json::json!({
            "class_quests": 0, "bis_quests": 0, "other_quests": 0
        })
    );
    let roundtrip: crate::plan::Breakdown = serde_json::from_value(json.clone()).unwrap();
    assert_eq!(roundtrip.mandatory_goal_counts, fresh.mandatory_goal_counts);
    let mut legacy = json;
    legacy.as_object_mut().unwrap().remove("mandatory_goal_counts");
    legacy["missing_class_quests"] = 25.into();
    let old: crate::plan::Breakdown = serde_json::from_value(legacy).unwrap();
    assert_eq!(old.mandatory_goal_counts, None);
    let saved = serde_json::to_value(old).unwrap();
    assert_eq!(saved["missing_class_quests"], 25);
    assert!(saved.get("mandatory_goal_counts").is_none());
    let overlapping = crate::plan::Breakdown {
        mandatory_goal_counts: Some(MandatoryGoalCounts {
            class_quests: 2,
            bis_quests: 3,
            other_quests: 1,
        }),
        ..crate::plan::Breakdown::default()
    };
    assert_eq!(
        serde_json::from_value::<crate::plan::Breakdown>(serde_json::to_value(overlapping).unwrap())
            .unwrap()
            .mandatory_goal_counts,
        overlapping.mandatory_goal_counts
    );
}

#[test]
fn only_selected_n_of_m_completion_support_gets_gear_origins() {
    let mut p = Scenario::new()
        .levels(1, 5)
        .quest(Q::new(1, "First"))
        .quest(Q::new(2, "Second"))
        .quest(Q::new(3, "Unchosen"))
        .quest(Q::new(4, "Two books").after_any(&[1, 2, 3]))
        .build();
    p.model.quests[3].pre_need = 2;
    select_test_sources(&mut p, &[3], &[]);
    for (i, q) in p.model.quests.iter().enumerate() {
        assert_eq!(q.mandatory_origins.contains(Origins::GEAR), p.model.bis.requires(i));
    }
    assert!(!p.model.quests[2].mandatory);
    assert_eq!(
        Planner::new(&p.model, &p.world, &p.params, &p.profile)
            .simulate_full(&[], None)
            .unwrap()
            .1
            .mandatory_goal_counts,
        Some(MandatoryGoalCounts {
            class_quests: 0,
            bis_quests: 3,
            other_quests: 0
        })
    );
}

#[test]
fn forced_dungeon_goals_and_support_are_other_without_changing_zone_or_run_counts() {
    let here = At {
        zone: 1,
        x: 50.0,
        y: 50.0,
    };
    let p = Scenario::new()
        .levels(1, 10)
        .dungeon("Forced", 1, 50.0, 50.0)
        .quest(Q::new(1, "Dungeon support"))
        .quest(
            Q::new(2, "Dungeon goal")
                .after(&[1])
                .objective(Obj::visit("Dungeon credit", here).inside(0)),
        )
        .params(|p| p.forced_dungeon_quests = vec![900])
        .build();
    for q in &p.model.quests {
        assert!(q.mandatory_origins.contains(Origins::FORCED_DUNGEON));
        assert!(!q.zone_forced);
    }
    let spent = Planner::new(&p.model, &p.world, &p.params, &p.profile)
        .simulate_full(&[], None)
        .unwrap()
        .1;
    assert_eq!(spent.missing_class_quests, 2);
    assert_eq!(spent.missing_zone_quests, 0);
    assert_eq!(spent.dungeon_runs, 0);
    assert_eq!(
        spent.mandatory_goal_counts,
        Some(MandatoryGoalCounts {
            class_quests: 0,
            bis_quests: 0,
            other_quests: 2,
        })
    );
}

#[test]
fn accepted_and_completed_objectives_are_missing_until_actual_reward() {
    let here = At {
        zone: 1,
        x: 50.0,
        y: 50.0,
    };
    let mut p = Scenario::new()
        .levels(1, 5)
        .params(|p| p.camp_radius = 0.0)
        .quest(
            Q::new(1, "Class goal")
                .mandatory_from(Origins::CLASS)
                .objective(Obj::visit("Do work", here)),
        )
        .build();
    p.model.initial.accepted = vec![0];
    let planner = Planner::new(&p.model, &p.world, &p.params, &p.profile);
    let objective = [Stop::quest(0, Kind::Objective(0))];
    let (_, spent) = planner.simulate_full(&objective, None).unwrap();
    assert_eq!(
        spent.mandatory_goal_counts,
        Some(MandatoryGoalCounts {
            class_quests: 1,
            bis_quests: 0,
            other_quests: 0,
        })
    );
    assert_eq!(planner.missing_mandatory_quests(&objective).len(), 1);
    let turnin = [objective[0], Stop::quest(0, Kind::TurnIn)];
    assert_eq!(
        planner.simulate_full(&turnin, None).unwrap().1.mandatory_goal_counts,
        Some(MandatoryGoalCounts::default())
    );
    assert_eq!(planner.missing_mandatory_quests(&turnin), Vec::<String>::new());
}
