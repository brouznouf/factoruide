//! Completion windows through planning, persisted requests and edited guides.
use crate::common::{database_of, repo};
use fg_route::faction::Faction;
use fg_route::job::{self, Editor, LogQuest, MapPos, Overrides, PlanRequest, QuestId, RouteStep, StartState};
use fg_route::model::Names;
use fg_route::plan::{Event, Kind, Planner, Timed};
use fg_route::xp::Edition;
use serde_json::json;

fn request(quests: &serde_json::Value) -> PlanRequest {
    serde_json::from_value(json!({"race":"human","class":"warrior","from_level":1,"to_level":5,"required_class_quests":[],"required_quests":quests})).unwrap()
}

fn overrides(edition: Edition) -> Overrides {
    let mut o = Overrides::load_edition(&repo().join("overrides"), edition).unwrap();
    o.params.farm_on_way = false;
    o.params.explore_radius = 0.0;
    o.params.dungeons = false;
    o
}

fn before_turn(trace: &[Timed], index: usize, from: i64) -> i64 {
    let at = trace
        .iter()
        .position(
            |t| matches!(t.event, Event::Stop { stop,.. } if stop.index as usize == index && stop.kind == Kind::TurnIn),
        )
        .unwrap();
    trace[..at].last().map_or(from, |t| t.level)
}

#[test]
fn quest_windows_plan_the_chain_with_prerequisites_before_the_lower_bound() {
    for edition in Edition::ALL {
        let conn = database_of(edition.key());
        let req = request(&json!([{"quest":21,"min_level":3,"max_level":4}]));
        let mut o = overrides(edition);
        o.params.effort = 0.01;
        o.params.candidates = 1;
        o.params.threads = 1;
        let guide = job::plan(&conn, &o, &req, &|_| {}).unwrap();
        let editor = Editor::open(&conn, &o, &req, None).unwrap();
        let report = editor.simulate(&guide.plan, &Names::load(&conn, "enUS").unwrap());
        assert_eq!(report.errors, 0);
        let at = |quest| {
            guide
                .plan
                .iter()
                .position(|s| matches!(s, RouteStep::TurnIn {quest:id} if *id==quest))
                .unwrap()
        };
        assert!((3..=4).contains(&report.steps[at(21) - 1].level));
        let early_prerequisite = editor.simulate(
            &[RouteStep::Accept { quest: 783 }, RouteStep::TurnIn { quest: 783 }],
            &Names::load(&conn, "enUS").unwrap(),
        );
        assert_eq!(early_prerequisite.errors, 0);
        assert_eq!(early_prerequisite.steps[1].level, 1);
        editor.save(&guide.plan, vec![]).unwrap();
    }
}

// A nearby optional reward would level the player before the required two-quest chain.
#[test]
fn generation_prioritizes_the_deadline_chain_and_reports_a_missed_window() {
    let conn = database_of("classic");
    conn.execute_batch(
        "CREATE TEMP TABLE m_quest AS SELECT * FROM main.m_quest WHERE id IN (783,7,33);
         UPDATE m_quest SET xp=0, xp_observed=1;
         UPDATE m_quest SET xp=400 WHERE id=33;
         CREATE TEMP TABLE m_quest_objective AS SELECT * FROM main.m_quest_objective WHERE 0;",
    )
    .unwrap();
    let mut req = request(&json!([{"quest":7,"max_level":1}]));
    req.to_level = 2;
    req.start = Some(StartState {
        level: 1,
        position: Some(MapPos {
            map: 1429,
            x: 48.94,
            y: 40.16,
        }),
        ..StartState::default()
    });
    let mut o = overrides(Edition::Classic);
    o.params.effort = 0.0;
    o.params.candidates = 1;
    o.params.threads = 1;
    let guide = job::plan(&conn, &o, &req, &|_| {}).unwrap();
    let at = |quest| {
        guide
            .plan
            .iter()
            .position(|s| matches!(s,RouteStep::TurnIn{quest:id} if *id==quest))
            .unwrap()
    };
    assert!(at(783) < at(7) && at(7) < at(33));
    let report = Editor::open(&conn, &o, &req, None)
        .unwrap()
        .simulate(&guide.plan, &Names::load(&conn, "enUS").unwrap());
    assert_eq!(report.errors, 0);
    assert_eq!(report.steps[at(7)].level, 1);

    // The prerequisite's reward makes the same deadline impossible.
    conn.execute("UPDATE m_quest SET xp=400 WHERE id=783", []).unwrap();
    let error = job::plan(&conn, &o, &req, &|_| {}).err().unwrap();
    let error = format!("{error:#}");
    assert!(
        error.contains("Kobold Camp Cleanup (7)") && error.contains("level 1"),
        "{error}"
    );
}

#[test]
fn imported_horde_class_quest_can_finish_during_level_twenty() {
    let conn = database_of("forever");
    let mut req = request(&json!([{"quest":98362,"min_level":20,"max_level":20}]));
    req.race = "skyborne_horde".into();
    req.class = "druid".into();
    req.to_level = 21;
    req.start = Some(StartState {
        level: 20,
        completed: vec![QuestId(98738)],
        log: vec![LogQuest {
            id: QuestId(98362),
            complete: true,
            objectives: vec![],
        }],
        position: Some(MapPos {
            map: 1456,
            x: 77.0,
            y: 29.0,
        }),
        ..StartState::default()
    });
    let p = job::prepare(&conn, &overrides(Edition::Forever), &req, &|_| {}).unwrap();
    let planner = Planner::new(&p.model, &p.world, &p.params, &p.profile);
    let route = planner.greedy();
    planner.validate_required(&route).unwrap();
    let mut trace = Vec::new();
    planner.simulate(&route, Some(&mut trace)).unwrap();
    assert_eq!(before_turn(&trace, p.model.index[&98362], 20), 20);
}

#[test]
fn saved_windows_are_checked_before_reward_xp_and_in_the_editor() {
    for edition in Edition::ALL {
        let conn = database_of(edition.key());
        let o = overrides(edition);
        let names = Names::load(&conn, "enUS").unwrap();
        let mut req = request(&json!([{"quest":783,"max_level":1}]));
        req.start = Some(StartState {
            level: 1,
            xp: 399,
            ..StartState::default()
        });
        let req: PlanRequest = serde_json::from_slice(&serde_json::to_vec(&req).unwrap()).unwrap();
        let editor = Editor::open(&conn, &o, &req, None).unwrap();
        let steps = [RouteStep::Accept { quest: 783 }, RouteStep::TurnIn { quest: 783 }];
        let report = editor.simulate(&steps, &names);
        assert_eq!(report.errors, 0);
        assert_eq!(
            report.steps[1].level, 2,
            "the turn-in's reward can cross the upper bound"
        );
        editor.save(&steps, vec![]).unwrap();
        let late = [steps[0].clone(), RouteStep::GrindTo { level: 2 }, steps[1].clone()];
        assert_eq!(editor.simulate(&late, &names).errors, 1);
        assert!(editor.save(&late, vec![]).is_err());

        let req = request(&json!([{"quest":783,"min_level":2}]));
        let editor = Editor::open(&conn, &o, &req, None).unwrap();
        assert_eq!(editor.simulate(&steps, &names).errors, 1);
        assert!(editor.save(&steps, vec![]).is_err());
        assert_eq!(editor.simulate(&late, &names).errors, 0);
        editor.save(&late, vec![]).unwrap();
    }
}

#[test]
fn windows_check_each_member_who_still_needs_the_quest() {
    let conn = database_of("classic");
    let o = overrides(Edition::Classic);
    let names = Names::load(&conn, "enUS").unwrap();
    for (completed, rested, mobs, max, second_xp, valid) in [
        (false, 0, 1, 1, 399, false),
        (true, 0, 1, 1, 399, true),
        (true, 600, 27, 2, 300, false),
    ] {
        let mut req = request(&json!([{"quest":783,"min_level":1,"max_level":max}]));
        let start = StartState {
            name: "First".into(),
            class: "WARRIOR".into(),
            race: 1,
            faction: Some(Faction::Alliance),
            level: 1,
            rested,
            position: Some(MapPos {
                map: 1429,
                x: 48.0,
                y: 43.0,
            }),
            ..StartState::default()
        };
        req.start = Some(start.clone());
        req.mates = vec![StartState {
            name: "Second".into(),
            xp: second_xp,
            rested: 0,
            completed: if completed { vec![QuestId(783)] } else { vec![] },
            ..start
        }];
        let editor = Editor::open(&conn, &o, &req, None).unwrap();
        let steps = [
            RouteStep::Grind { mobs },
            RouteStep::Accept { quest: 783 },
            RouteStep::TurnIn { quest: 783 },
        ];
        let report = editor.simulate(&steps, &names);
        assert_eq!(report.errors, usize::from(!valid));
        assert!(report.steps[1].level <= max, "the current lead is within the window");
        if rested > 0 {
            assert!(
                report.steps[0].level > max,
                "the player needing the quest was ahead before the lead changed"
            );
        }
        assert_eq!(editor.save(&steps, vec![]).is_ok(), valid);
    }
}

#[test]
fn imported_completion_counts_after_the_window_and_unfinished_late_starts_fail() {
    let conn = database_of("classic");
    let o = overrides(Edition::Classic);
    let mut req = request(&json!([{"quest":783,"max_level":2}]));
    req.to_level = 31;
    req.start = Some(StartState {
        level: 30,
        completed: vec![QuestId(783)],
        ..StartState::default()
    });
    Editor::open(&conn, &o, &req, None).unwrap().save(&[], vec![]).unwrap();
    req.start.as_mut().unwrap().completed.clear();
    let error = job::prepare(&conn, &o, &req, &|_| {}).err().unwrap();
    assert!(format!("{error:#}").contains("already above completion level 2"));
}

#[test]
fn group_lower_bound_waits_for_the_member_needing_the_quest() {
    let conn = database_of("classic");
    let o = overrides(Edition::Classic);
    let mut req = request(&json!([{"quest":783,"min_level":3}]));
    let first = StartState {
        name: "First".into(),
        class: "WARRIOR".into(),
        race: 1,
        faction: Some(Faction::Alliance),
        level: 1,
        xp: 40,
        completed: vec![QuestId(783)],
        position: Some(MapPos {
            map: 1429,
            x: 48.0,
            y: 43.0,
        }),
        ..StartState::default()
    };
    req.start = Some(first.clone());
    req.mates = vec![StartState {
        name: "Second".into(),
        level: 2,
        xp: 0,
        completed: vec![],
        ..first
    }];
    let p = job::prepare(&conn, &o, &req, &|_| {}).unwrap();
    let planner = Planner::new(&p.model, &p.world, &p.params, &p.profile);
    let route = planner.greedy();
    planner.validate_required(&route).unwrap();
    let mut trace = Vec::new();
    planner.simulate(&route, Some(&mut trace)).unwrap();
    let turn=trace.iter().find(|t|matches!(t.event, Event::Stop {stop,..} if stop.index as usize==p.model.index[&783] && stop.kind==Kind::TurnIn)).unwrap();
    assert!(turn.party[1].0 >= 3);
}

#[test]
fn invalid_and_conflicting_windows_report_the_requested_quest() {
    let conn = database_of("classic");
    let o = overrides(Edition::Classic);
    for quests in [
        json!([{"quest":783,"min_level":0}]),
        json!([{"quest":783,"max_level":61}]),
        json!([{"quest":783,"min_level":3,"max_level":2}]),
        json!([{"quest":783,"min_level":3},{"quest":783,"max_level":2}]),
    ] {
        let error = job::prepare(&conn, &o, &request(&quests), &|_| {}).err().unwrap();
        assert!(format!("{error:#}").contains("required quest \"783\""));
    }
    job::prepare(
        &database_of("tbc"),
        &overrides(Edition::Tbc),
        &request(&json!([{"quest":783,"max_level":65}])),
        &|_| {},
    )
    .unwrap();
    let req = request(&json!([{"quest":783,"min_level":2},{"quest":"A Threat Within","max_level":2}]));
    let editor = Editor::open(&conn, &o, &req, None).unwrap();
    let names = Names::load(&conn, "enUS").unwrap();
    for (level, valid) in [(1, false), (2, true), (3, false)] {
        let mut steps = vec![RouteStep::Accept { quest: 783 }];
        if level > 1 {
            steps.push(RouteStep::GrindTo { level });
        }
        steps.push(RouteStep::TurnIn { quest: 783 });
        assert_eq!(editor.simulate(&steps, &names).errors == 0, valid);
    }
}

#[test]
fn malformed_windows_report_the_quest_instead_of_ignoring_the_bound() {
    for window in [json!({"quest":783,"min_level":3.5}), json!({"quest":783,"max":2})] {
        let error = serde_json::from_value::<PlanRequest>(
            json!({"race":"human","class":"warrior","from_level":1,"to_level":5,"required_quests":[window]}),
        )
        .unwrap_err();
        assert!(error.to_string().contains("required quest 783"), "{error}");
    }
}

#[test]
fn starting_deadlines_only_check_members_pursuing_the_quests_profession() {
    let conn = database_of("classic");
    let mut req:PlanRequest = serde_json::from_value(json!({
        "race":"orc","class":"warrior","from_level":45,"to_level":60,"required_class_quests":[],
        "required_quests":[{"quest":6610,"max_level":50}],
        "professions":[{"key":"cooking","target":300}],
        "start":{"name":"Cook","race":2,"class":"WARRIOR","faction":"Horde","level":45,"professions":[{"line":185,"rank":225}]},
        "mates":[{"name":"Noncook","race":2,"class":"WARRIOR","faction":"Horde","level":55}]
    })).unwrap();
    let p = job::prepare(&conn, &overrides(Edition::Classic), &req, &|_| {}).unwrap();
    let q = p.model.index[&6610];
    assert_eq!(p.model.initial.need(q), 1, "the older non-cook is not a participant");
    req.start.as_mut().unwrap().completed = vec![QuestId(6610)];
    req.required_quests = serde_json::from_value(json!([{"quest":6610,"max_level":30}])).unwrap();
    let p = job::prepare(&conn, &overrides(Edition::Classic), &req, &|_| {}).unwrap();
    assert_eq!(
        p.model.initial.need(p.model.index[&6610]),
        0,
        "all actual participants already completed it"
    );
    req.start.as_mut().unwrap().completed.clear();
    req.required_quests = serde_json::from_value(json!([{"quest":6610,"max_level":50}])).unwrap();
    req.mate_professions = vec![req.professions.clone()];
    req.mates[0].professions = req.start.as_ref().unwrap().professions.clone();
    let error = job::prepare(&conn, &overrides(Edition::Classic), &req, &|_| {})
        .err()
        .unwrap();
    assert!(format!("{error:#}").contains("above completion level"), "{error:#}");
}

#[test]
fn a_deadline_at_the_level_cap_does_not_change_the_greedy_route() {
    for edition in Edition::ALL {
        let conn = database_of(edition.key());
        let o = overrides(edition);
        let plain = request(&json!([21]));
        let capped = request(&json!([{"quest":21,"max_level":edition.rules().max_level}]));
        let a = job::prepare(&conn, &o, &plain, &|_| {}).unwrap();
        let b = job::prepare(&conn, &o, &capped, &|_| {}).unwrap();
        let pa = Planner::new(&a.model, &a.world, &a.params, &a.profile);
        let pb = Planner::new(&b.model, &b.world, &b.params, &b.profile);
        assert_eq!(
            pa.greedy(),
            pb.greedy(),
            "{edition:?}: a non-binding maximum should not force detours"
        );
    }
}
