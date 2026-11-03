//! Explicit quest requirements through request resolution, model construction and guide saving.

use crate::common::repo;
use fg_route::faction::Faction;
use fg_route::fixture::Fixture;
use fg_route::job::{self, Editor, LogQuest, Overrides, PlanRequest, QuestId, RequiredQuest, RouteStep, StartState};
use fg_route::plan::{Kind, Planner, Stop};
use fg_route::testing::{At, Obj, Q, Scenario};
use fg_route::xp::Edition;

fn request(race: &str, class: &str, required: &[RequiredQuest]) -> PlanRequest {
    serde_json::from_value(serde_json::json!({
        "race": race, "class": class, "from_level": 1, "to_level": 5,
        "required_class_quests": [], "required_quests": required,
    }))
    .unwrap()
}

/// A must-have quest brings in the quests it needs, and the guide turns them all in, even with no
/// penalty for leaving a mandatory quest out.
#[test]
fn required_quest_chain_is_included_and_survives_zero_penalty() {
    let p = Scenario::new()
        .params(|p| p.mandatory_penalty = 0.0)
        .quest(
            Q::new(20, "Echo Ridge Beginnings")
                .giver(1, 40.0, 40.0)
                .kill("Kobold", 1, 4, 1, 45.0, 45.0),
        )
        .quest(
            Q::new(21, "Skirmish at Echo Ridge")
                .level(2)
                .after_any(&[20])
                .giver(1, 41.0, 41.0)
                .kill("Kobold", 2, 4, 1, 46.0, 46.0),
        )
        .quest(
            Q::new(30, "Wolves")
                .giver(1, 60.0, 60.0)
                .kill("Wolf", 1, 4, 1, 62.0, 62.0),
        )
        .required(21)
        .build();
    let target = &p.model.quests[p.model.index[&21]];
    assert!(target.required);
    assert_ne!(target.pre_any, [] as [i64; 0]);
    for pre in &target.pre_any {
        assert!(p.model.quests[p.model.index[pre]].required);
    }
    assert!(!p.model.quests[p.model.index[&30]].required);
    let planner = Planner::new(&p.model, &p.world, &p.params, &p.profile);
    let route = planner.greedy();
    planner.validate_required(&route).unwrap();
    assert!(route.contains(&Stop::quest(p.model.index[&21] as u32, Kind::TurnIn)));
    assert!(planner.validate_required(&[]).is_err());
}

/// The editor refuses to save a guide that does not turn in a must-have quest, and says which.
#[test]
fn required_quest_editor_refuses_removal() {
    let scenario = |required: bool| {
        let s = Scenario::new()
            .quest(
                Q::new(783, "A Threat Within")
                    .level(5)
                    .giver(1, 40.0, 40.0)
                    .kill("Defias", 5, 4, 1, 45.0, 45.0),
            )
            .quest(
                Q::new(33, "Wolves")
                    .level(2)
                    .giver(1, 60.0, 60.0)
                    .kill("Wolf", 2, 4, 1, 62.0, 62.0),
            );
        Editor::from_prepared(if required { s.required(783) } else { s }.build())
    };
    let editor = scenario(true);
    assert!(editor.save(&[], vec![]).is_err());
    assert!(editor.save(&[RouteStep::Accept { quest: 783 }], vec![]).is_err());
    let unrelated = [
        RouteStep::Accept { quest: 33 },
        RouteStep::Objective {
            quest: 33,
            objective: 0,
            farm: false,
            spot: None,
        },
        RouteStep::TurnIn { quest: 33 },
    ];
    scenario(false).save(&unrelated, vec![]).unwrap();
    let error = editor.save(&unrelated, vec![]).err().unwrap();
    assert!(format!("{error:#}").contains("A Threat Within (783)"));
    editor
        .save(
            &[
                RouteStep::Accept { quest: 783 },
                RouteStep::Objective {
                    quest: 783,
                    objective: 0,
                    farm: false,
                    spot: None,
                },
                RouteStep::TurnIn { quest: 783 },
            ],
            vec![],
        )
        .unwrap();
}

/// A must-have quest the character already did needs no repeat, and leaves the other class choices
/// as they were.
#[test]
fn required_quest_completed_profile_needs_no_repeat() {
    let scenario = |required: bool| {
        let s = Scenario::new()
            .levels(30, 31)
            .start(StartState {
                level: 30,
                completed: vec![QuestId(783)],
                ..StartState::default()
            })
            .quest(
                Q::new(783, "A Threat Within")
                    .level(5)
                    .giver(1, 40.0, 40.0)
                    .kill("Defias", 5, 4, 1, 45.0, 45.0),
            )
            .quest(
                Q::new(3100, "Tome")
                    .level(30)
                    .mandatory()
                    .giver(1, 60.0, 60.0)
                    .kill("Imp", 30, 4, 1, 62.0, 62.0),
            );
        if required { s.required(783) } else { s }.build()
    };
    let (p, ordinary) = (scenario(true), scenario(false));
    assert!(p.model.quests.iter().all(|q| !q.required));
    let choices = |p: &job::Prepared| p.model.quests.iter().map(|q| (q.id, q.mandatory)).collect::<Vec<_>>();
    assert_eq!(
        choices(&p),
        choices(&ordinary),
        "completed requirements leave unrelated class choices intact"
    );
}

/// A must-have quest is still required once another quest reached the target level.
#[test]
fn required_quests_continue_after_target_level() {
    let p = Scenario::new()
        .levels(1, 5)
        .quest(
            Q::new(783, "A Threat Within")
                .level(5)
                .xp(1_500)
                .giver(1, 40.0, 40.0)
                .kill("Defias", 5, 4, 1, 45.0, 45.0),
        )
        .quest(Q::new(7, "Kobolds").xp(1_500).giver(1, 60.0, 60.0))
        .required(783)
        .build();
    let planner = Planner::new(&p.model, &p.world, &p.params, &p.profile);
    let (other, i) = (p.model.index[&7] as u32, p.model.index[&783] as u32);
    let route = [
        Stop::quest(other, Kind::Accept),
        Stop::quest(other, Kind::TurnIn),
        Stop::quest(i, Kind::Accept),
        Stop::quest(i, Kind::Objective(0)),
        Stop::quest(i, Kind::TurnIn),
    ];
    planner.validate_required(&route).unwrap();
}

#[test]
fn required_sleeping_bag_route_exports_the_factions_chain_by_id_or_name() {
    let conn = Fixture::new(Edition::Forever).file("forever_sleeping_bag").open();
    let mut overrides = Overrides::load_edition(&repo().join("overrides"), Edition::Forever).unwrap();
    overrides.params.dungeons = false;
    for (race, class, choice, starter, unavailable) in [
        ("skyborne_horde", "druid", RequiredQuest::Id(79976), 79007, 79008),
        (
            "human",
            "warrior",
            RequiredQuest::Name("This Must Be The Place".into()),
            79008,
            79007,
        ),
    ] {
        let mut req = request(race, class, &[choice]);
        req.from_level = 35;
        req.to_level = 36;
        let p = job::prepare(&conn, &overrides, &req, &|_| {}).unwrap();
        let planner = Planner::new(&p.model, &p.world, &p.params, &p.profile);
        let route = planner.greedy();
        planner.validate_required(&route).unwrap();
        let guide = fg_route::export::build(&planner, &p.profile, &route);
        let turned: Vec<_> = guide
            .steps
            .iter()
            .filter(|s| s.kind == fg_route::export::StepKind::TurnIn)
            .filter_map(|s| s.quest)
            .collect();
        for id in [starter, 79192, 79975, 79976] {
            assert!(turned.contains(&id), "{race}: required quest {id} is exported");
        }
        assert!(
            !turned.contains(&unavailable),
            "{race}: the other faction's starter is absent"
        );
    }
}

/// A quest one player of a group did stays required for the other.
#[test]
fn required_quest_completed_by_one_group_member_is_still_required_by_the_other() {
    let leader = StartState {
        name: "Leader".into(),
        race: 1,
        class: "WARRIOR".into(),
        faction: Some(Faction::Alliance),
        level: 10,
        xp: 1,
        completed: vec![QuestId(783)],
        ..StartState::default()
    };
    let partner = StartState {
        name: "Partner".into(),
        xp: 0,
        completed: vec![],
        ..leader.clone()
    };
    let p = Scenario::new()
        .faction(Faction::Alliance)
        .levels(10, 11)
        .start(leader)
        .mates(vec![partner])
        .quest(
            Q::new(783, "A Threat Within")
                .level(5)
                .members(0b11)
                .giver(1, 40.0, 40.0)
                .kill("Defias", 5, 4, 1, 45.0, 45.0),
        )
        .required(783)
        .build();
    let planner = Planner::new(&p.model, &p.world, &p.params, &p.profile);
    assert!(planner.validate_required(&[]).is_err());
    let i = p.model.index[&783] as u32;
    planner
        .validate_required(&[
            Stop::quest(i, Kind::Accept),
            Stop::quest(i, Kind::Objective(0)),
            Stop::quest(i, Kind::TurnIn),
        ])
        .unwrap();
}

/// A guide that cannot finish a must-have quest is refused, naming it (a timed quest with a second
/// to turn it in).
#[test]
fn required_generation_refuses_an_incomplete_chain() {
    let p = Scenario::new()
        .levels(1, 5)
        .quest(
            Q::new(79975, "Eagle's Fist")
                .level(1)
                .giver(1, 40.0, 40.0)
                .kill("Cat", 1, 4, 1, 45.0, 45.0),
        )
        .quest(
            Q::new(79976, "This Must Be The Place")
                .level(2)
                .after(&[79975])
                .timed(1.0)
                .giver(1, 41.0, 41.0)
                .kill("Cat", 2, 4, 1, 46.0, 46.0),
        )
        .required(79976)
        .build();
    let planner = Planner::new(&p.model, &p.world, &p.params, &p.profile);
    let route = planner.construct(|_| {});
    let error = planner.validate_required(&route).unwrap_err();
    assert!(format!("{error:#}").contains("no route completing every required quest was found:"));
    assert!(format!("{error:#}").contains("This Must Be The Place (79976)"));
}

/// A prerequisite of a must-have quest the character did is not done again.
#[test]
fn completed_required_prerequisite_is_not_repeated() {
    let p = Scenario::new()
        .levels(30, 31)
        .start(StartState {
            level: 30,
            completed: vec![QuestId(79007)],
            ..StartState::default()
        })
        .quest(
            Q::new(79007, "Starter")
                .giver(1, 40.0, 40.0)
                .kill("Cat", 1, 4, 1, 45.0, 45.0),
        )
        .quest(
            Q::new(79976, "This Must Be The Place")
                .level(30)
                .after(&[79007])
                .giver(1, 41.0, 41.0)
                .kill("Cat", 30, 4, 1, 46.0, 46.0),
        )
        .required(79976)
        .build();
    assert!(!p.model.quests.iter().any(|q| q.required && q.id == 79007));
    assert!(p.model.quests[p.model.index[&79976]].required);
    let planner = Planner::new(&p.model, &p.world, &p.params, &p.profile);
    let route = planner.greedy();
    planner.validate_required(&route).unwrap();
    assert!(
        !route
            .iter()
            .any(|stop| stop.is_quest() && p.model.quests[stop.index as usize].id == 79007)
    );
}

/// A class quest one player did is not required again for the group, whatever the others' class.
#[test]
fn ineligible_group_member_does_not_repeat_a_completed_class_quest() {
    let leader = StartState {
        name: "Leader".into(),
        race: 1,
        class: "WARRIOR".into(),
        faction: Some(Faction::Alliance),
        level: 10,
        completed: vec![QuestId(3100)],
        ..StartState::default()
    };
    let mage = StartState {
        name: "Mage".into(),
        class: "MAGE".into(),
        completed: vec![],
        ..leader.clone()
    };
    let p = Scenario::new()
        .faction(Faction::Alliance)
        .levels(10, 11)
        .start(leader)
        .mates(vec![mage])
        .quest(
            Q::new(3100, "Berserker")
                .level(10)
                .members(0b01)
                .giver(1, 40.0, 40.0)
                .kill("Imp", 10, 4, 1, 45.0, 45.0),
        )
        .required(3100)
        .build();
    assert!(!p.model.quests.iter().any(|q| q.required));
}

/// Must-have quests that exclude each other, or one excluded by a quest done, are reported.
#[test]
fn mutually_exclusive_required_quests_report_the_conflict() {
    let scenario = || {
        Scenario::new()
            .quest(Q::new(282, "Left").exclusive_with(&[287]).giver(1, 40.0, 40.0))
            .quest(Q::new(287, "Right").exclusive_with(&[282]).giver(1, 41.0, 41.0))
    };
    let error = scenario().required(282).required(287).try_build().err().unwrap();
    assert!(format!("{error:#}").contains("conflicts with quest"), "{error:#}");
    let error = scenario()
        .levels(10, 11)
        .start(StartState {
            level: 10,
            completed: vec![QuestId(287)],
            ..StartState::default()
        })
        .required(282)
        .try_build()
        .err()
        .unwrap();
    assert!(format!("{error:#}").contains("conflicts with quest 287"), "{error:#}");
}

/// A class quest of a group member with more XP than the one the guide is planned for is still
/// planned, and a requirement already met needs no repeat even when the quest is left out.
#[test]
fn required_class_quest_can_belong_to_a_higher_xp_group_member() {
    let warrior = StartState {
        name: "Warrior".into(),
        race: 1,
        class: "WARRIOR".into(),
        faction: Some(Faction::Alliance),
        level: 10,
        xp: 1,
        completed: vec![QuestId(783), QuestId(7)],
        ..StartState::default()
    };
    let mage = StartState {
        name: "Mage".into(),
        class: "MAGE".into(),
        xp: 0,
        ..warrior.clone()
    };
    let scenario = |warrior: StartState, with_quest: bool| {
        let s = Scenario::new()
            .faction(Faction::Alliance)
            .levels(10, 11)
            .start(warrior)
            .mates(vec![mage.clone()])
            .required(3100);
        // The warrior has more XP than the mage the guide is planned for: second of the players.
        let quest = Q::new(3100, "Berserker")
            .level(10)
            .members(0b10)
            .giver(1, 40.0, 40.0)
            .kill("Imp", 10, 4, 1, 45.0, 45.0);
        s.quest(if with_quest { quest } else { quest.excluded() }).build()
    };
    let saved = Editor::from_prepared(scenario(warrior.clone(), true))
        .save(
            &[
                RouteStep::Accept { quest: 3100 },
                RouteStep::Objective {
                    quest: 3100,
                    objective: 0,
                    farm: false,
                    spot: None,
                },
                RouteStep::TurnIn { quest: 3100 },
            ],
            vec![],
        )
        .unwrap();
    assert!(
        saved
            .route
            .steps
            .iter()
            .any(|s| s.quest == Some(3100) && s.kind == fg_route::export::StepKind::TurnIn)
    );
    // The mage cannot do the warrior's quest. Once the warrior completed it, a saved
    // requirement needs no repeat, even if the quest is excluded from the new guide.
    let mut done = warrior;
    done.completed.push(QuestId(3100));
    Editor::from_prepared(scenario(done, false)).save(&[], vec![]).unwrap();
}

/// The warlock's Components for the Enchanted Gold Bloodrobe can be required (GitHub #72): the
/// Soul Shard is made by the warlock, the Robes of Arcana bought, the Fine Gold Thread asked for
/// at Xizk, whose repeatable quest has nothing to do.
#[test]
fn the_warlocks_gold_bloodrobe_chain_can_be_required() {
    let conn = Fixture::new(Edition::Forever).file("forever_bloodrobe").open();
    let overrides = Overrides::load_edition(&repo().join("overrides"), Edition::Forever).unwrap();
    let mut req = request("human", "warlock", &[RequiredQuest::Id(4784)]);
    req.to_level = 60;
    let p = job::prepare(&conn, &overrides, &req, &|_| {}).unwrap();
    let quest = p.model.quests.iter().find(|q| q.id == 4784).expect("planned");
    assert!(quest.mandatory);
}

/// A guide from level 40 to 60 with Ragefire Chasm forced (levels 13-19) is refused at once, saying
/// which dungeon (GitHub #84): it could not be run.
#[test]
fn a_forced_dungeon_below_the_guides_levels_is_refused() {
    let conn = Fixture::new(Edition::Forever).file("forever_ragefire").open();
    let overrides = Overrides::load_edition(&repo().join("overrides"), Edition::Forever).unwrap();
    let mut req = request("orc", "warrior", &[]);
    (req.from_level, req.to_level) = (40, 60);
    let ragefire_chasm = 2437;
    let mut params = overrides.params.clone();
    let plan = |params: &fg_route::Params| {
        let req = PlanRequest {
            params: Some(params.clone()),
            ..req.clone()
        };
        job::prepare(&conn, &overrides, &req, &|_| {})
    };
    assert!(plan(&params).is_ok());
    params.forced_dungeons = vec![ragefire_chasm];
    let error = plan(&params).err().expect("refused").to_string();
    assert!(
        error.contains("Ragefire Chasm") && error.contains("level 40"),
        "{error}"
    );
}

/// A must-have quest whose items drop inside a dungeon (Trolls of a Feather, GitHub #73) runs the
/// dungeon, which was not planned unless the player forced it too.
#[test]
fn a_required_quest_inside_a_dungeon_runs_the_dungeon() {
    let p = Scenario::new()
        .levels(40, 58)
        .dungeon("The Temple", 1, 70.0, 50.0)
        .quest(
            Q::new(8422, "Trolls of a Feather")
                .level(52)
                .giver(1, 40.0, 40.0)
                .objective(
                    Obj::kill(
                        "Troll",
                        50,
                        6,
                        At {
                            zone: 1,
                            x: 70.0,
                            y: 50.0,
                        },
                    )
                    .inside(0),
                ),
        )
        .required(8422)
        .build();
    assert!(p.model.dungeons[0].needed);
    let planner = Planner::new(&p.model, &p.world, &p.params, &p.profile);
    let route = planner.construct(|_| {});
    planner.validate_required(&route).unwrap();
    assert!(route.iter().any(|s| s.kind == Kind::Dungeon));
}

/// A must-have quest whose dungeon objectives are already complete in the log (GitHub #87) does
/// not ask for the dungeon: it is only turned in.
#[test]
fn a_finished_required_dungeon_quest_does_not_need_the_dungeon() {
    let p = Scenario::new()
        .levels(55, 56)
        .dungeon("The Temple", 1, 70.0, 50.0)
        .start(StartState {
            level: 55,
            log: vec![LogQuest {
                id: 8422.into(),
                complete: true,
                ..LogQuest::default()
            }],
            ..StartState::default()
        })
        .quest(
            Q::new(8422, "Trolls of a Feather")
                .level(52)
                .giver(1, 40.0, 40.0)
                .objective(
                    Obj::kill(
                        "Troll",
                        50,
                        6,
                        At {
                            zone: 1,
                            x: 70.0,
                            y: 50.0,
                        },
                    )
                    .inside(0),
                ),
        )
        .required(8422)
        .build();
    assert!(!p.model.dungeons[0].needed);
}

/// A must-have quest with a child quest (Need for a Cure and Finding the Antidote, GitHub #74):
/// the child is a quest of its own, taken while the parent is in the log and turned in before the
/// parent, whether the pair is put in the route together or found by the search (`child_glue`).
#[test]
fn a_child_quest_is_done_between_its_parent_accept_and_turn_in() {
    for glue in [0.0, 0.5, 1.0] {
        let p = Scenario::new()
            .levels(8, 10)
            .params(|p| p.child_glue = glue)
            .zone(2, "Orgrimmar")
            .quest(
                Q::new(812, "Need for a Cure")
                    .level(8)
                    .timed(2700.0)
                    .giver(1, 50.0, 50.0)
                    .objective(
                        Obj::visit(
                            "Venomtail Antidote",
                            At {
                                zone: 2,
                                x: 50.0,
                                y: 50.0,
                            },
                        )
                        .handed_by(&[813]),
                    ),
            )
            .quest(
                Q::new(813, "Finding the Antidote")
                    .level(8)
                    .while_in_log(&[812])
                    .giver(2, 50.0, 50.0)
                    .kill("Scorpid", 8, 4, 1, 70.0, 50.0),
            )
            .required(812)
            .build();
        let planner = Planner::new(&p.model, &p.world, &p.params, &p.profile);
        let route = planner.construct(|_| {});
        planner.validate_required(&route).unwrap();
        let at = |quest: i64, kind| {
            let i = p.model.index[&quest] as u32;
            route.iter().position(|s| *s == Stop::quest(i, kind))
        };
        let (accept, turn_in) = (at(812, Kind::Accept).unwrap(), at(812, Kind::TurnIn).unwrap());
        let (c_accept, c_turn_in) = (at(813, Kind::Accept).unwrap(), at(813, Kind::TurnIn).unwrap());
        assert!(
            accept < c_accept && c_accept < c_turn_in && c_turn_in < turn_in,
            "child_glue={glue}"
        );
    }
}

#[test]
fn sleeping_bag_option_requires_the_chain_and_recognizes_imported_completion() {
    let conn = Fixture::new(Edition::Forever).file("forever_sleeping_bag").open();
    let mut overrides = Overrides::load_edition(&repo().join("overrides"), Edition::Forever).unwrap();
    overrides.params.sleeping_bag = true;
    for (race, class, starter, other) in [
        ("human", "warrior", 79008, 79007),
        ("skyborne_horde", "druid", 79007, 79008),
    ] {
        let mut req = request(race, class, &[]);
        req.from_level = 35;
        req.to_level = 36;
        let p = job::prepare(&conn, &overrides, &req, &|_| {}).unwrap();
        for id in [starter, 79192, 79980, 79974, 79975, 79976] {
            assert!(p.model.quests[p.model.index[&id]].required, "required bag quest {id}");
        }
        assert!(!p.model.index.contains_key(&other));
        req.start = Some(StartState {
            level: 35,
            completed: vec![QuestId(79976)],
            ..StartState::default()
        });
        let imported = job::prepare(&conn, &overrides, &req, &|_| {}).unwrap();
        assert!(imported.model.quests.iter().all(|q| !q.required));
    }
    for edition in [Edition::Classic, Edition::Tbc] {
        let conn = Fixture::new(edition).open();
        let mut other = Overrides::load_edition(&repo().join("overrides"), edition).unwrap();
        other.params.sleeping_bag = true;
        let p = job::prepare(&conn, &other, &request("human", "warrior", &[]), &|_| {}).unwrap();
        let required: Vec<i64> = p.profile.required_quests.iter().map(|r| r.id).collect();
        assert_eq!(required, Vec::<i64>::new());
    }
}

#[test]
fn sleeping_bag_option_preserves_an_explicit_completion_window() {
    let conn = Fixture::new(Edition::Forever).file("forever_sleeping_bag").open();
    let o = Overrides::load_edition(&repo().join("overrides"), Edition::Forever).unwrap();
    let req = serde_json::from_value(serde_json::json!({
        "race":"human","class":"warrior","from_level":1,"to_level":60,
        "required_class_quests":[],"required_quests":[{"quest":79976,"min_level":20,"max_level":60}],
        "params":{"edition":"forever","sleeping_bag":true,"food_xp":true}
    }))
    .unwrap();
    let p = job::prepare(&conn, &o, &req, &|_| {}).unwrap();
    let choices: Vec<_> = p.profile.required_quests.iter().filter(|q| q.id == 79976).collect();
    assert_eq!(choices.len(), 1);
    assert_eq!(choices[0].window.min_level, Some(20));
    assert_eq!(
        p.model.quests[p.model.index[&79976]].completion_window.max_level,
        Some(60)
    );
}

/// Class quests WoW Forever's pages open to the races and classes that are new there can be
/// required: the Orc mage takes the Undead's and the Troll's quests, the Troll warlock the Orc's
/// Voidwalker quests, the High Order Skyborne mage the Human's and the Gnome's, the druid the
/// Night Elf's.
fn class_quests_are_open_to_the_races_new_to_a_class_of(race: &str, class: &str, quests: &[i64]) {
    let conn = Fixture::new(Edition::Forever).file("forever_new_race_classes").open();
    let overrides = Overrides::load_edition(&repo().join("overrides"), Edition::Forever).unwrap();
    for &id in quests {
        let mut req = request(race, class, &[RequiredQuest::Id(id)]);
        req.to_level = 60;
        // Some of these cannot be planned for any race (1960's item has no known source):
        // what must not happen is being refused for the character's race or class.
        match job::prepare(&conn, &overrides, &req, &|_| {}) {
            Ok(p) => {
                let quest = p.model.quests.iter().find(|q| q.id == id).expect("planned");
                assert!(quest.mandatory, "{race} {class}: {} ({id}) is required", quest.name);
            }
            Err(e) => {
                let e = format!("{e:#}");
                assert!(
                    !e.contains("unavailable to this character"),
                    "{race} {class}, quest {id}: {e}"
                );
            }
        }
    }
}

/// A must-have quest with no window of its own is done while the game shows it yellow (orange for
/// a risky character) or green: not before, not once it is gray; a quest already gray at the start
/// has no limit. A window the player set stays as it is.
#[test]
fn a_must_have_quest_is_done_while_it_is_not_too_hard_nor_gray() {
    let window = |progression: Option<&str>, level: i64, from: i64| {
        let s = Scenario::new()
            .levels(from, 40)
            .params(|p| {
                if progression == Some("risky") {
                    p.progression = fg_route::params::Progression::Risky;
                }
            })
            .quest(
                Q::new(1, "Quest")
                    .level(level)
                    .giver(1, 40.0, 40.0)
                    .kill("Mob", level, 2, 1, 45.0, 45.0),
            );
        let p = s.required(1).build();
        let w = p.model.quests[0].completion_window;
        (w.min_level, w.max_level)
    };
    // Yellow from two levels below, gray from level 18 (a quest of level 12).
    assert_eq!(window(None, 12, 1), (Some(10), Some(17)));
    // Orange from four levels below for a risky character.
    assert_eq!(window(Some("risky"), 12, 1), (Some(8), Some(17)));
    // Already in the window at the start: no minimum.
    assert_eq!(window(None, 12, 11), (None, Some(17)));
    // Already gray at the start: nothing to ask.
    assert_eq!(window(None, 12, 20), (None, None));
}

#[test]
fn class_quests_are_open_to_the_orc_mage() {
    class_quests_are_open_to_the_races_new_to_a_class_of("orc", "mage", &[1943, 1944, 1945, 1946, 1959, 1960, 1962]);
}

#[test]
fn class_quests_are_open_to_the_troll_warlock() {
    class_quests_are_open_to_the_races_new_to_a_class_of("troll", "warlock", &[1501, 1504]);
}

#[test]
fn class_quests_are_open_to_the_skyborne_mage() {
    class_quests_are_open_to_the_races_new_to_a_class_of(
        "skyborne",
        "mage",
        &[1860, 1919, 1921, 1938, 1940, 1941, 1942],
    );
}

#[test]
fn class_quests_are_open_to_the_skyborne_druid() {
    class_quests_are_open_to_the_races_new_to_a_class_of("skyborne", "druid", &[26, 29, 5061, 6123, 6124, 6125]);
}
