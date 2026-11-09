use super::*;
use crate::{
    job::{LogObjective, LogQuest, Prepared},
    testing::{Q, Scenario},
};

fn scenario(n: usize) -> Prepared {
    (0..n)
        .fold(Scenario::new().levels(1, 60), |s, i| {
            s.quest(Q::new(i as i64 + 1, &format!("Quest {}", i + 1)))
        })
        .build()
}
fn selector<'a>(p: &'a Prepared, starts: &'a [&'a StartState]) -> Selector<'a> {
    Selector::new(
        &p.model.quests,
        &p.model.dungeons,
        &p.model.skipped,
        &p.profile,
        &p.params,
        &p.model.known_class_prerequisites,
        starts,
    )
}

#[test]
fn original_cardinality_and_global_backtracking() {
    let mut p = scenario(4);
    p.model.quests[3].pre_any = vec![1, 2, 3];
    p.model.quests[3].pre_need = 2;
    p.model.quests[0].exclusive = vec![2, 3];
    let plan = selector(&p, &[]).select(3).unwrap().0;
    assert!(!plan.quests.contains(&0));
    assert!(plan.quests.contains(&1) && plan.quests.contains(&2));
    p.model.quests[3].pre_any = vec![1, 1, 999];
    assert!(
        selector(&p, &[]).select(3).is_err(),
        "neither duplicates nor absent alternatives reduce the original need"
    );
}

#[test]
fn choices_backtrack_when_a_later_all_dependency_conflicts() {
    let mut p = scenario(4);
    p.model.quests[3].pre_any = vec![1, 2];
    p.model.quests[3].pre_all = vec![3];
    p.model.quests[0].exclusive = vec![3];
    let plan = selector(&p, &[]).select(3).unwrap().0;
    assert!(!plan.quests.contains(&0));
    assert!(plan.quests.contains(&1));
}

#[test]
fn growing_library_tiers_share_twenty_five_of_thirty_eight_books() {
    let mut p = scenario(41);
    for (i, need) in [(38, 10), (39, 20), (40, 25)] {
        p.model.quests[i].pre_any = (1..=38).rev().collect();
        p.model.quests[i].pre_need = need;
    }
    let mut s = selector(&p, &[]);
    for (root, need) in [(38, 10), (39, 20), (40, 25)] {
        let (plan, closure) = s.select(root).unwrap();
        assert_eq!(
            plan.actions
                .iter()
                .filter(|a| matches!(a,BisQuestAction::TurnIn(i) if *i<38))
                .count(),
            need
        );
        s.committed = closure;
    }
    assert_eq!(
        s.committed
            .nodes
            .keys()
            .filter(|a| matches!(a,BisQuestAction::TurnIn(i) if *i<38))
            .count(),
        25
    );
}

#[test]
fn held_parent_is_only_accepted_and_valid_held_item_cycle_is_ordered() {
    use BisQuestAction::*;
    let mut p = scenario(2);
    p.model.quests[1].pre_held = vec![1];
    let plan = selector(&p, &[]).select(1).unwrap().0;
    assert_eq!(plan.actions, vec![Accept(0), Accept(1), TurnIn(1)]);
    let objective = Scenario::new()
        .quest(Q::new(10, "Objective").kill("thing", 1, 1, 1, 50.0, 50.0))
        .build()
        .model
        .quests[0]
        .objectives[0]
        .clone();
    p.model.quests[0].objectives = vec![objective];
    p.model.quests[0].objectives[0].handed_by = vec![2];
    let plan = selector(&p, &[]).select(0).unwrap().0;
    assert_eq!(
        plan.actions,
        vec![
            Accept(0),
            Accept(1),
            TurnIn(1),
            Objective { quest: 0, objective: 0 },
            TurnIn(0)
        ]
    );
    p.model.quests[1].pre_held.clear();
    p.model.quests[1].pre_all = vec![1];
    assert!(selector(&p, &[]).select(0).unwrap_err().contains("cyclic"));
}

#[test]
fn recorded_accept_and_objective_cut_historical_dependencies() {
    use BisQuestAction::*;
    let mut p = Scenario::new()
        .levels(1, 60)
        .quest(Q::new(1, "logged").members(1).kill("thing", 1, 1, 1, 50.0, 50.0))
        .build();
    p.model.quests[0].pre_all = vec![999];
    p.model.quests[0].objectives[0].handed_by = vec![998];
    let start = StartState {
        level: 1,
        log: vec![LogQuest {
            id: 1.into(),
            objectives: vec![LogObjective {
                text: "thing".into(),
                done: 1,
                need: 1,
                finished: true,
            }],
            ..Default::default()
        }],
        ..Default::default()
    };
    assert!(selector(&p, &[]).select(0).is_err());
    assert_eq!(selector(&p, &[&start]).select(0).unwrap().0.actions, vec![TurnIn(0)]);
    let satisfied = recorded_objectives(&p.model.quests, &[&start]);
    super::super::chains::drop_unhanded(&mut p.model.quests, &mut p.model.skipped, &satisfied);
    assert_eq!(
        p.model.quests.len(),
        1,
        "logged finished item survives the pre-selector filter"
    );
}

#[test]
fn member_completion_is_not_a_group_union() {
    let mut p = scenario(1);
    p.model.quests[0].members = 3;
    p.model.quests[0].pre_all = vec![999];
    let done = StartState {
        completed: vec![999.into()],
        ..Default::default()
    };
    let missing = StartState::default();
    p.profile.completed_quests.insert(999);
    assert!(selector(&p, &[&done, &missing]).select(0).is_err());
    assert!(selector(&p, &[&done, &done]).select(0).is_ok());
}

#[test]
fn completion_cycles_target_floors_and_exact_conflicts_reject() {
    let mut p = scenario(2);
    p.model.quests[0].pre_all = vec![2];
    p.model.quests[1].pre_all = vec![1];
    assert!(selector(&p, &[]).select(0).unwrap_err().contains("cyclic"));
    p.model.quests[0].pre_all.clear();
    p.model.quests[1].pre_all.clear();
    p.model.quests[0].min_level = 60;
    assert!(selector(&p, &[]).select(0).unwrap_err().contains("level 60"));
    p.model.quests[0].min_level = 59;
    assert!(selector(&p, &[]).select(0).is_ok());
    p.model.quests[1].required = true;
    p.model.quests[1].exclusive = vec![1];
    assert!(selector(&p, &[]).select(0).unwrap_err().contains("conflicts"));
}

#[test]
fn branching_limit_is_reported_as_uncertainty() {
    let mut p = scenario(132);
    p.model.quests[131].pre_any = (1..=131).collect();
    p.model.quests[131].pre_need = 130;
    assert_eq!(selector(&p, &[]).select(131).unwrap_err(), LIMIT);
}

#[test]
fn members_can_have_different_completed_alternatives() {
    let mut p = scenario(1);
    p.model.quests[0].members = 3;
    p.model.quests[0].pre_any = vec![998, 999];
    let a = StartState {
        completed: vec![998.into()],
        ..Default::default()
    };
    let b = StartState {
        completed: vec![999.into()],
        ..Default::default()
    };
    assert!(selector(&p, &[&a, &b]).select(0).is_ok());
}

#[test]
fn selected_class_constraints_and_breadcrumb_order_are_respected() {
    let mut p = scenario(3);
    p.model.quests[0].mandatory = true;
    p.profile.class_requirements = vec![crate::model::ClassRequirement {
        class_id: 1,
        name: "Class power".into(),
        spell: None,
        max_level: None,
        quests: vec![1],
        quest_names: vec![],
        selected: true,
    }];
    p.model.quests[2].exclusive = vec![1];
    assert!(selector(&p, &[]).select(2).unwrap_err().contains("selected class"));
    p.model.quests[2].exclusive = vec![2];
    p.model.quests[0].pre_all = vec![2];
    assert!(selector(&p, &[]).select(2).unwrap_err().contains("selected class"));
    p.model.quests[0].pre_all.clear();
    p.model.quests[2].exclusive.clear();
    p.model.quests[0].breadcrumb_for = Some(2);
    p.model.quests[2].pre_all = vec![1, 2];
    let plan = selector(&p, &[]).select(2).unwrap().0;
    let at = |a| plan.actions.iter().position(|b| *b == a).unwrap();
    assert!(at(BisQuestAction::TurnIn(0)) < at(BisQuestAction::Accept(1)));
    let start = StartState {
        log: vec![LogQuest {
            id: 2.into(),
            ..Default::default()
        }],
        ..Default::default()
    };
    assert!(
        selector(&p, &[&start])
            .select(0)
            .unwrap_err()
            .contains("recorded quest")
    );
}

#[test]
fn expired_dungeon_and_explicit_window_are_not_supported() {
    let mut p = Scenario::new()
        .levels(20, 60)
        .dungeon("Test dungeon", 1, 50.0, 50.0)
        .quest(Q::new(1, "Dungeon quest").level(20))
        .build();
    p.model.quests[0].start_dungeon = Some(0);
    p.model.dungeons[0].min_level = 5;
    p.model.dungeons[0].max_level = 19;
    assert!(selector(&p, &[]).select(0).unwrap_err().contains("window"));
    p.model.dungeons[0].max_level = 30;
    assert!(selector(&p, &[]).select(0).is_ok());
    p.model.quests[0].completion_window.min_level = Some(31);
    assert!(selector(&p, &[]).select(0).unwrap_err().contains("window"));
}

#[test]
fn storage_order_does_not_change_alternative_choice() {
    let mut p = scenario(4);
    p.model.quests[3].pre_any = vec![3, 2, 1];
    let selected = selector(&p, &[]).select(3).unwrap().0;
    let ids: Vec<_> = selected.quests.iter().map(|&i| p.model.quests[i as usize].id).collect();
    assert_eq!(ids, vec![1, 4]);
    p.model.quests.reverse();
    let selected = selector(&p, &[]).select(0).unwrap().0;
    let mut ids: Vec<_> = selected.quests.iter().map(|&i| p.model.quests[i as usize].id).collect();
    ids.sort_unstable();
    assert_eq!(ids, vec![1, 4]);
}

#[test]
fn final_global_graph_keeps_cross_root_held_order_and_recorded_participants() {
    use BisQuestAction::*;
    let mut p = scenario(2);
    p.model.quests[1].pre_held = vec![1];
    let mut s = selector(&p, &[]);
    s.committed = s.select(0).unwrap().1;
    let (plan, closure) = s.select(1).unwrap();
    assert_eq!(
        plan.quests,
        vec![0, 1],
        "local participants retain a recorded held parent"
    );
    s.committed = closure;
    let graph = s.finalize().unwrap();
    assert!(graph.edges.contains(&(Accept(1), TurnIn(0))));
    assert!(
        graph.actions.iter().position(|a| *a == Accept(1)).unwrap()
            < graph.actions.iter().position(|a| *a == TurnIn(0)).unwrap()
    );
    let start = StartState {
        log: vec![LogQuest {
            id: 1.into(),
            ..Default::default()
        }],
        ..Default::default()
    };
    let starts = [&start];
    let mut s = selector(&p, &starts);
    let (plan, closure) = s.select(1).unwrap();
    assert_eq!(
        plan.quests,
        vec![0, 1],
        "local participants retain a recorded held parent"
    );
    s.committed = closure;
    let graph = s.finalize().unwrap();
    assert_eq!(graph.quests, vec![0, 1]);
    assert!(!graph.actions.contains(&Accept(0)) && !graph.actions.contains(&TurnIn(0)));
}

fn class_case() -> Prepared {
    let mut p = scenario(5);
    p.model.quests[0].mandatory = true;
    p.model.quests[0].pre_any = vec![2, 3];
    p.profile.class_requirements = vec![crate::model::ClassRequirement {
        class_id: 1,
        name: "Protected class power".into(),
        spell: None,
        max_level: None,
        quests: vec![1],
        quest_names: vec![],
        selected: true,
    }];
    p
}

#[test]
fn review_class_one_of_all_branches_blocked_rejects_gear() {
    let mut p = class_case();
    p.model.quests[4].exclusive = vec![2, 3];
    assert!(
        selector(&p, &[]).select(4).is_err(),
        "gear must preserve a class branch"
    );
}

#[test]
fn review_class_two_of_three_preserves_original_cardinality() {
    let mut p = class_case();
    p.model.quests[0].pre_any = vec![2, 3, 4];
    p.model.quests[0].pre_need = 2;
    p.model.quests[4].exclusive = vec![2, 3];
    assert!(
        selector(&p, &[]).select(4).is_err(),
        "one remaining branch cannot satisfy two"
    );
}

#[test]
fn review_class_sole_handed_item_provider_is_protected() {
    let mut p = class_case();
    p.model.quests[0].pre_any.clear();
    let objective = Scenario::new()
        .quest(Q::new(9, "Item").kill("thing", 1, 1, 1, 50.0, 60.0))
        .build()
        .model
        .quests[0]
        .objectives[0]
        .clone();
    p.model.quests[0].objectives = vec![objective];
    p.model.quests[0].objectives[0].handed_by = vec![2];
    p.model.quests[4].exclusive = vec![2];
    assert!(
        selector(&p, &[]).select(4).is_err(),
        "the objective still needs its provider"
    );
}

#[test]
fn review_class_one_compatible_branch_does_not_reserve_all_alternatives() {
    let mut p = class_case();
    p.model.quests[4].exclusive = vec![2];
    p.model.quests[1].exclusive = vec![3];
    let mut s = selector(&p, &[]);
    let (plan, closure) = s.select(4).unwrap();
    assert_eq!(
        plan.actions,
        vec![BisQuestAction::Accept(4), BisQuestAction::TurnIn(4)],
        "only the gear root can acquire a gear completion requirement"
    );
    s.committed = closure;
    assert_eq!(s.finalize().unwrap().actions, plan.actions);
    assert_eq!(
        s.finalize().unwrap().edges,
        vec![(BisQuestAction::Accept(4), BisQuestAction::TurnIn(4))]
    );
    assert_eq!(s.finalize().unwrap().quests, vec![4], "class proof is not gear support");
}

#[test]
fn review_combat_floor_notes_are_qualified_and_actual_minimum_is_distinct() {
    let mut p = Scenario::new()
        .levels(1, 60)
        .quest(Q::new(1, "Dangerous prerequisite").kill("High mob", 100, 10, 1, 50.0, 70.0))
        .quest(Q::new(2, "Gear").after(&[1]))
        .build();
    let note = selector(&p, &[]).select(1).unwrap_err();
    assert!(
        note.contains("conservative combat estimate")
            && note.contains("plain gear")
            && note.contains("route ends at level 60"),
        "{note}"
    );
    p.model.quests[0].objectives.clear();
    p.model.quests[0].min_level = 60;
    let note = selector(&p, &[]).select(1).unwrap_err();
    assert!(
        note.contains("minimum") && note.contains("level 60") && !note.contains("combat estimate"),
        "{note}"
    );
}

#[test]
fn review_already_unsupported_class_does_not_disable_independent_gear() {
    let mut p = class_case();
    p.model.quests[0].pre_all = vec![999];
    assert!(selector(&p, &[]).select(4).is_ok());
}

#[test]
fn round2_missing_class_support_does_not_permit_a_direct_conflict() {
    let mut p = class_case();
    p.model.quests[0].pre_all = vec![999];
    p.model.quests[4].exclusive = vec![1];
    assert!(selector(&p, &[]).select(4).is_err());
}

#[test]
fn round2_combat_estimate_failure_keeps_class_and_unavoidable_support_protected() {
    let mut p = class_case();
    p.model.quests[0].pre_any.clear();
    p.model.quests[0].pre_all = vec![2];
    let objective = Scenario::new()
        .quest(Q::new(9, "Hard class").kill("Boss", 100, 10, 1, 50.0, 70.0))
        .build()
        .model
        .quests[0]
        .objectives[0]
        .clone();
    p.model.quests[0].objectives = vec![objective];
    assert!(
        selector(&p, &[]).select(4).is_ok(),
        "genuinely unrelated gear remains eligible"
    );
    for conflict in [1, 2] {
        p.model.quests[4].exclusive = vec![conflict];
        assert!(
            selector(&p, &[]).select(4).is_err(),
            "unproven class support remains protected: {conflict}"
        );
    }
}

fn dungeon_chain() -> Prepared {
    let mut p = Scenario::new()
        .levels(1, 60)
        .dungeon("Scholomance", 1, 50.0, 50.0)
        .quest(Q::new(5382, "Doctor Theolen Krastinov, the Butcher").kill("Boss", 1, 1, 1, 50.0, 60.0))
        .quest(
            Q::new(5515, "Krastinov's Bag of Horrors")
                .after(&[5382])
                .kill("Bag", 1, 1, 1, 50.0, 60.0),
        )
        .quest(
            Q::new(5384, "Kirtonos the Herald")
                .after(&[5515])
                .kill("Herald", 1, 1, 1, 50.0, 60.0),
        )
        .quest(Q::new(7, "Alternate source"))
        .build();
    p.model.dungeons[0].min_level = 1;
    p.model.dungeons[0].max_level = 60;
    for q in &mut p.model.quests[..3] {
        q.members = 1;
        for loc in q.starts.iter_mut().chain(&mut q.ends) {
            loc.id = 11216;
            loc.name = "Eva Sarkhoff".into();
        }
        q.objectives[0].loc.inside = Some(0);
    }
    p
}

#[test]
fn round2_dungeon_chain_requiring_reentry_is_not_supported() {
    let p = dungeon_chain();
    let error = selector(&p, &[]).select(2).unwrap_err();
    assert!(error.contains("one continuous visit"), "{error}");
}

#[test]
fn round2_sequential_inside_npcs_and_recorded_history_remain_supported() {
    let mut p = dungeon_chain();
    for q in &mut p.model.quests[..3] {
        for loc in q.starts.iter_mut().chain(&mut q.ends) {
            loc.inside = Some(0);
        }
    }
    assert!(selector(&p, &[]).select(2).is_ok());
    let p = dungeon_chain();
    let start = StartState {
        completed: vec![5382.into(), 5515.into()],
        ..Default::default()
    };
    assert!(selector(&p, &[&start]).select(2).is_ok());
}

#[test]
fn round2_dungeon_topology_backtracks_and_checks_committed_roots() {
    let mut p = dungeon_chain();
    let mut outside = p.model.quests[0].objectives[0].clone();
    outside.loc.inside = None;
    p.model.quests[3].objectives = vec![outside.clone(), outside];
    p.model.quests[2].pre_all.clear();
    p.model.quests[2].pre_any = vec![5515, 7];
    let plan = selector(&p, &[]).select(2).unwrap().0;
    assert!(plan.quests.contains(&3) && !plan.quests.contains(&1));
    let mut s = selector(&p, &[]);
    s.committed = s.select(0).unwrap().1;
    s.committed = s.select(2).unwrap().1;
    let graph = s.finalize().unwrap();
    let inside: Vec<_> = graph
        .actions
        .iter()
        .enumerate()
        .filter_map(|(i, a)| {
            matches!(
                a,
                BisQuestAction::Objective {
                    quest: 0 | 2,
                    objective: 0
                }
            )
            .then_some(i)
        })
        .collect();
    assert_eq!(inside.len(), 2);
    assert_eq!(
        inside[1],
        inside[0] + 1,
        "the finalized source ordering keeps the shared visit continuous"
    );
    p.model.quests[3].pre_all = vec![5382];
    let mut s = selector(&p, &[]);
    s.committed = s.select(0).unwrap().1;
    assert!(
        s.select(2).is_err(),
        "cross-root outside dependencies still require re-entry"
    );
}

#[test]
fn round2_completed_logged_objective_cuts_prior_visit() {
    let mut p = dungeon_chain();
    p.model.quests[2].pre_all = vec![5382];
    let start = StartState {
        log: vec![LogQuest {
            id: 5382.into(),
            complete: true,
            ..Default::default()
        }],
        ..Default::default()
    };
    assert!(selector(&p, &[&start]).select(2).is_ok());
}

#[test]
fn round2_alternate_inside_givers_can_preserve_one_visit() {
    let mut p = dungeon_chain();
    for q in &mut p.model.quests[..3] {
        let mut inside = q.starts[0].clone();
        inside.inside = Some(0);
        q.starts.push(inside);
        let mut inside = q.ends[0].clone();
        inside.inside = Some(0);
        q.ends.push(inside);
    }
    assert!(selector(&p, &[]).select(2).is_ok());
}

#[test]
fn round2_blackbox_credit_needs_acceptance_before_entry() {
    let mut p = dungeon_chain();
    p.model.quests[0].starts[0].inside = Some(0);
    p.model.quests[0].objectives[0].dungeon = Some(0);
    assert!(
        selector(&p, &[]).select(0).is_err(),
        "acceptance inside cannot precede entry credit"
    );
    p.model.quests[0].objectives[0].dungeon = None;
    assert!(
        selector(&p, &[]).select(0).is_ok(),
        "a direct objective inside can follow an inside accept"
    );
}

#[test]
fn review_recorded_class_accept_cuts_old_alternative_edges() {
    let mut p = class_case();
    p.model.quests[0].members = 1;
    p.model.quests[4].exclusive = vec![2, 3];
    let start = StartState {
        log: vec![LogQuest {
            id: 1.into(),
            ..Default::default()
        }],
        ..Default::default()
    };
    assert!(selector(&p, &[&start]).select(4).is_ok());
}

#[test]
fn review_class_horizon_does_not_use_strict_gear_endpoint() {
    let mut p = class_case();
    p.model.quests[0].min_level = 60;
    p.model.quests[4].exclusive = vec![2, 3];
    assert!(
        selector(&p, &[]).select(4).is_err(),
        "class baseline remains supported at its normal endpoint"
    );
    p.model.quests[4].exclusive = vec![2];
    assert!(selector(&p, &[]).select(4).is_ok());
}

#[test]
fn review_gear_choice_backtracks_to_preserve_a_coherent_class_closure() {
    let mut p = class_case();
    p.model.quests[4].pre_any = vec![2, 4];
    p.model.quests[1].exclusive = vec![1];
    let plan = selector(&p, &[]).select(4).unwrap().0;
    assert!(!plan.quests.contains(&1));
    assert!(plan.quests.contains(&3));
}

#[test]
fn review_propagated_combat_estimate_keeps_its_qualification() {
    let mut p = Scenario::new()
        .levels(1, 60)
        .quest(Q::new(1, "Combat").kill("Mob", 30, 10, 1, 50.0, 70.0))
        .quest(Q::new(2, "Earlier hand-in").after(&[1]))
        .build();
    p.model.quests[1].completion_window.max_level = Some(10);
    let note = selector(&p, &[]).select(1).unwrap_err();
    assert!(
        note.contains("Earlier hand-in")
            && note.contains("conservative combat estimate")
            && note.contains("ending at 10"),
        "{note}"
    );
}

#[test]
fn review_class_requirements_need_one_jointly_coherent_witness() {
    let mut p = scenario(7);
    p.model.quests[0].mandatory = true;
    p.model.quests[0].pre_any = vec![2, 3];
    p.model.quests[3].mandatory = true;
    p.model.quests[3].pre_any = vec![2, 6];
    p.model.quests[2].exclusive = vec![6];
    let template = class_case().profile.class_requirements[0].clone();
    p.profile.class_requirements = vec![
        template.clone(),
        crate::model::ClassRequirement {
            quests: vec![4],
            name: "Other class power".into(),
            ..template
        },
    ];
    p.model.quests[6].exclusive = vec![2];
    assert!(
        selector(&p, &[]).select(6).is_err(),
        "individually possible class branches must also coexist"
    );
    p.model.quests[6].exclusive.clear();
    assert!(selector(&p, &[]).select(6).is_ok());
}

fn placement_case() -> Prepared {
    let mut p = dungeon_chain();
    p.profile.from_level = 20;
    p.model.dungeons[0].min_level = 5;
    p.model.dungeons[0].max_level = 19;
    p.model.quests[0].min_level = 20;
    p.model.quests[0].objectives.clear();
    p
}

fn selected_visits(p: &Prepared) -> BTreeMap<BisQuestAction, Option<usize>> {
    let mut s = selector(p, &[]);
    s.committed = s.select(0).expect("source has a legal placement").1;
    s.finalize().unwrap().visits
}

#[test]
fn final_fix_expired_optional_inside_turnin_uses_outside_witness() {
    let mut p = placement_case();
    let mut inside = p.model.quests[0].ends[0].clone();
    inside.inside = Some(0);
    p.model.quests[0].ends.push(inside);
    assert_eq!(selected_visits(&p)[&BisQuestAction::TurnIn(0)], None);
}

#[test]
fn final_fix_inside_accept_checks_its_selected_window() {
    let mut p = placement_case();
    p.model.quests[0].starts[0].inside = Some(0);
    assert!(selector(&p, &[]).select(0).is_err(), "inside start has expired");
    let mut outside = p.model.quests[0].starts[0].clone();
    outside.inside = None;
    p.model.quests[0].starts.push(outside);
    assert_eq!(selected_visits(&p)[&BisQuestAction::Accept(0)], None);
    p.model.quests[0].start_dungeon = Some(0);
    assert!(
        selector(&p, &[]).select(0).is_err(),
        "fixed dungeon-item start stays fixed"
    );
}

#[test]
fn final_fix_optional_inside_objective_uses_outside_combat_and_window() {
    let mut p = placement_case();
    let mut objective = dungeon_chain().model.quests[0].objectives[0].clone();
    let mut outside = objective.clone();
    outside.loc.inside = None;
    objective.alts.push(outside);
    p.model.quests[0].objectives = vec![objective];
    assert_eq!(
        selected_visits(&p)[&BisQuestAction::Objective { quest: 0, objective: 0 }],
        None
    );
    p.model.quests[0].objectives[0].alts[0].mob_level = 100;
    let error = selector(&p, &[]).select(0).unwrap_err();
    assert!(!error.is_empty(), "neither placement meets its own floor/window");
}

#[test]
fn final_fix_backtracks_from_outside_combat_to_legal_inside_objective() {
    let mut p = placement_case();
    p.model.dungeons[0].max_level = 59;
    let mut outside = dungeon_chain().model.quests[0].objectives[0].clone();
    outside.loc.inside = None;
    let inside = dungeon_chain().model.quests[0].objectives[0].clone();
    outside.mob_level = 100;
    outside.alts.push(inside);
    p.model.quests[0].objectives = vec![outside];
    let a = BisQuestAction::Objective { quest: 0, objective: 0 };
    assert_eq!(selected_visits(&p)[&a], Some(0), "outside cannot borrow inside combat");
    p.model.quests[0].objectives[0].alts[0].mob_level = 100;
    let error = selector(&p, &[]).select(0).unwrap_err();
    assert!(error.contains("conservative combat estimate"), "{error}");
}

#[test]
fn final_fix_inside_start_floor_propagates_and_backtracks_to_outside() {
    let mut p = placement_case();
    p.model.dungeons[0].min_level = 40;
    p.model.dungeons[0].max_level = 59;
    p.model.quests[0].completion_window.max_level = Some(30);
    p.model.quests[0].starts[0].inside = Some(0);
    assert!(selector(&p, &[]).select(0).is_err(), "start floor must reach hand-in");
    let mut outside = p.model.quests[0].starts[0].clone();
    outside.inside = None;
    p.model.quests[0].starts.push(outside);
    assert_eq!(selected_visits(&p)[&BisQuestAction::Accept(0)], None);
}

#[test]
fn final_fix_outside_fast_path_retains_windows_and_gear_endpoint() {
    let mut p = placement_case();
    assert!(selector(&p, &[]).select(0).is_ok());
    p.model.quests[0].min_level = 60;
    assert!(selector(&p, &[]).select(0).is_err());
    p.model.quests[0].min_level = 20;
    p.model.quests[0].completion_window.max_level = Some(19);
    assert!(selector(&p, &[]).select(0).is_err());
}

#[test]
fn final_fix_blackbox_credit_does_not_use_outside_alternative_or_combat() {
    let mut p = placement_case();
    let mut credit = dungeon_chain().model.quests[0].objectives[0].clone();
    let mut outside = credit.clone();
    outside.loc.inside = None;
    credit.dungeon = Some(0);
    credit.mob_level = 100;
    credit.alts.push(outside);
    p.model.quests[0].objectives = vec![credit];
    assert!(selector(&p, &[]).select(0).is_err(), "entry-credit window stays fixed");
    p.model.dungeons[0].max_level = 59;
    assert_eq!(
        selected_visits(&p)[&BisQuestAction::Objective { quest: 0, objective: 0 }],
        Some(0)
    );
}
