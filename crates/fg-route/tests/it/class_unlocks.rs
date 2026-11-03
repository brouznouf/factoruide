//! Class chains through normal planning and recorded character starts.

use crate::common::{database_of, repo};
use fg_route::fixture::Fixture;
use fg_route::job::{self, Overrides, PlanRequest, StartState};
use fg_route::plan::{Edit, Kind, Outcome, Planner, Stop};
use fg_route::testing::{Q, Scenario};
use fg_route::xp::Edition;

fn request(race: &str, class: &str) -> PlanRequest {
    serde_json::from_value(serde_json::json!({
        "race":race, "class":class, "from_level":1, "to_level":5,
        "required_class_quests":[]
    }))
    .unwrap()
}

#[test]
fn known_reward_does_not_put_its_parent_in_the_log() {
    for edition in Edition::ALL {
        let conn = Fixture::new(edition).file("classic_shaman").open();
        let mut overrides = Overrides::load_edition(&repo().join("overrides"), edition).unwrap();
        overrides.params.all_class_quests = true;
        for (held, completed) in [(false, false), (true, false), (false, true)] {
            let mut req = request("tauren", "shaman");
            req.from_level = 4;
            req.to_level = 5;
            req.start = Some(StartState {
                level: 4,
                spells: if held { vec![] } else { vec![8071] },
                completed: if completed { vec![job::QuestId(1521)] } else { vec![] },
                log: if held {
                    vec![job::LogQuest {
                        id: job::QuestId(1521),
                        complete: false,
                        objectives: vec![],
                    }]
                } else {
                    vec![]
                },
                ..StartState::default()
            });
            let p = job::prepare(&conn, &overrides, &req, &|_| {}).unwrap();
            assert_eq!(p.profile.completed_quests.contains(&1521), completed);
            if !held && !completed {
                assert!(!p.model.index.contains_key(&1521));
            }
            let child = &p.model.quests[p.model.index[&1462]];
            assert!(child.pre_held.contains(&1521));
            assert_eq!(
                child.mandatory, held,
                "{edition:?}: Earth Sapta needs Call of Earth in the log"
            );
        }
    }
}

#[test]
// The source data makes 4962 and 4963 exclusive children of 1799. This history chose
// 4962, which Cleansing of the Orb of Orahil (4961) requires.
fn a_completed_child_keeps_later_class_quests_feasible() {
    for edition in Edition::ALL {
        let conn = Fixture::new(edition).file("classic_warlock_orb").open();
        let mut overrides = Overrides::load_edition(&repo().join("overrides"), edition).unwrap();
        overrides.params.all_class_quests = true;
        let mut req = request("orc", "warlock");
        req.from_level = 35;
        req.to_level = 41;
        req.start = Some(StartState {
            level: 35,
            completed: vec![job::QuestId(1799), job::QuestId(4962)],
            ..StartState::default()
        });
        let p = job::prepare(&conn, &overrides, &req, &|_| {}).unwrap();
        let next = &p.model.quests[p.model.index[&4961]];
        assert!(next.pre_all.contains(&4962));
        assert!(
            next.mandatory,
            "{edition:?}: Cleansing of the Orb of Orahil has its prerequisite done"
        );
    }
}

#[test]
fn normal_skyborne_druid_planning_completes_the_cat_chain() {
    let conn = Fixture::new(Edition::Forever).file("forever_druid").open();
    let overrides = Overrides::load_edition(&repo().join("overrides"), Edition::Forever).unwrap();
    let mut req = request("skyborne_horde", "druid");
    req.to_level = 30;
    req.required_class_quests = None;
    // The fixture's world is thin: its character is level 27 when it gets to the form quest, a
    // level past the window a class requirement has by default (18 to 26).
    req.required_quests = vec![serde_json::from_value(serde_json::json!({"quest": 98362, "max_level": 30})).unwrap()];
    let p = job::prepare(&conn, &overrides, &req, &|_| {}).unwrap();
    let mut params = p.params;
    params.effort = 0.0;
    params.threads = 1;
    req.params = Some(params);
    let outcome = job::plan(&conn, &overrides, &req, &|_| {}).unwrap();
    let end = outcome.end.unwrap();
    assert!(
        end.spells.contains(&768) && end.spells.contains(&18960),
        "earned form and teleport survive continuation"
    );
    assert!(
        end.completed.contains(&job::QuestId(98362)),
        "the real form quest completion is retained"
    );
    let guide = outcome.route;
    for id in [98340, 98341, 98404, 98738, 98362] {
        assert!(
            guide
                .steps
                .iter()
                .any(|s| s.quest == Some(id) && s.kind == fg_route::export::StepKind::TurnIn),
            "the normal class selection completes quest {id}"
        );
    }
    let reward = guide
        .steps
        .iter()
        .find(|s| s.quest == Some(98362) && s.kind == fg_route::export::StepKind::TurnIn)
        .unwrap();
    assert!(
        (20..30).contains(&reward.level),
        "the form is acquired in the level 20s: {}",
        reward.level
    );
    assert_eq!(guide.breakdown.missing_class_quests, 0);
}

#[test]
fn a_late_start_keeps_missing_class_unlock_prerequisites() {
    let conn = Fixture::new(Edition::Forever).file("forever_druid").open();
    let overrides = Overrides::load_edition(&repo().join("overrides"), Edition::Forever).unwrap();
    let mut req = request("skyborne_horde", "druid");
    req.from_level = 40;
    req.to_level = 41;
    req.start = Some(StartState {
        level: 40,
        ..StartState::default()
    });
    req.required_class_quests = None;
    let p = job::prepare(&conn, &overrides, &req, &|_| {}).unwrap();
    for id in [94913, 98340, 98341, 98404, 98738, 98362] {
        assert!(
            p.model.index.contains_key(&id),
            "missing class unlock quest {id} survives the lower level filter"
        );
        let quest = &p.model.quests[p.model.index[&id]];
        assert!(quest.late_unlock, "{} is weighed for waiting", quest.name);
    }
}

#[test]
fn a_synthetic_midlevel_preset_does_not_return_for_earlier_class_unlocks() {
    let conn = Fixture::new(Edition::Classic).file("classic_druid").open();
    let overrides = Overrides::load_edition(&repo().join("overrides"), Edition::Classic).unwrap();
    let mut req = request("nightelf", "druid");
    req.from_level = 50;
    req.to_level = 51;
    req.required_class_quests = None;
    let p = job::prepare(&conn, &overrides, &req, &|_| {}).unwrap();
    let planner = Planner::new(&p.model, &p.world, &p.params, &p.profile);
    let route = planner.construct(|_| {});
    let guide = fg_route::export::build(&planner, &p.profile, &route);
    assert_eq!(guide.breakdown.missing_class_quests, 0);
    assert!(
        !guide.steps.iter().any(|s| s.quest == Some(456)),
        "the preset need not redo The Balance of Nature at level 50"
    );
    let mut params = p.params.clone();
    params.effort = 0.0;
    params.threads = 1;
    req.params = Some(params);
    let start = job::plan(&conn, &overrides, &req, &|_| {}).unwrap().end.unwrap();
    let assumed = start.spells.clone();
    let mut next = req;
    next.to_level = start.level + 1;
    next.start = Some(start);
    let continued = job::prepare(&conn, &overrides, &next, &|_| {}).unwrap();
    let planner = Planner::new(
        &continued.model,
        &continued.world,
        &continued.params,
        &continued.profile,
    );
    let route = planner.construct(|_| {});
    let guide = fg_route::export::build(&planner, &continued.profile, &route);
    assert_eq!(guide.breakdown.missing_class_quests, 0);
    assert!(
        !guide.steps.iter().any(|s| s.kind == fg_route::export::StepKind::TurnIn
            && matches!(
                s.quest_name.as_deref(),
                Some("Body and Heart" | "Moonglade" | "Aquatic Form")
            )),
        "continuing does not repeat earlier class unlocks"
    );
    // Spell knowledge leaves the recorded quest history unchanged.
    let request = next.start.as_ref().unwrap();
    assert!(assumed.contains(&5487));
    assert!(
        assumed.contains(&18960),
        "the continuing character retains its Moonglade teleport"
    );
    let known: Vec<i64> = conn
        .prepare("SELECT id FROM m_quest WHERE reward_spell=5487 OR name='Moonglade'")
        .unwrap()
        .query_map([], |r| r.get(0))
        .unwrap()
        .collect::<rusqlite::Result<_>>()
        .unwrap();
    assert!(known.iter().all(|id| !request.completed.contains(&job::QuestId(*id))));
    let saved = job::plan(&conn, &overrides, &next, &|_| {}).unwrap().end.unwrap();
    assert!(known.iter().all(|id| !saved.completed.contains(&job::QuestId(*id))));
}

#[test]
fn moonglade_teaches_teleport_before_completing_its_quest() {
    for edition in Edition::ALL {
        let conn = Fixture::new(edition).file("classic_druid").open();
        let overrides = Overrides::load_edition(&repo().join("overrides"), edition).unwrap();
        let mut req = request("nightelf", "druid");
        req.to_level = 11;
        req.start = Some(StartState {
            level: 10,
            ..StartState::default()
        });
        req.required_class_quests = None;
        let p = job::prepare(&conn, &overrides, &req, &|_| {}).unwrap();
        let planner = Planner::new(&p.model, &p.world, &p.params, &p.profile);
        let accept = Stop::quest(p.model.index[&5921] as u32, Kind::Accept);
        let teleport = p
            .model
            .teleports
            .iter()
            .position(|t| t.name == "Teleport: Moonglade")
            .unwrap();
        let edited = planner.edit(&[
            Edit::Stop {
                stop: accept,
                farm: false,
                spot: None,
            },
            Edit::Teleport { to: teleport },
        ]);
        assert!(matches!(edited.steps[0].outcome, Outcome::Done));
        assert!(
            matches!(edited.steps[1].outcome, Outcome::Done),
            "accepting Moonglade teaches its teleport"
        );
        // The addon can record the character between acceptance and turn-in.
        req.start = Some(
            serde_json::from_value(serde_json::json!({
                "level":10, "spells":[18960], "log":[{"id":5921,"complete":true}]
            }))
            .unwrap(),
        );
        let p = job::prepare(&conn, &overrides, &req, &|_| {}).unwrap();
        let planner = Planner::new(&p.model, &p.world, &p.params, &p.profile);
        let teleport = p
            .model
            .teleports
            .iter()
            .position(|t| t.name == "Teleport: Moonglade")
            .unwrap();
        let stop = |id, kind| Edit::Stop {
            stop: Stop::quest(p.model.index[&id] as u32, kind),
            farm: false,
            spot: None,
        };
        let edited = planner.edit(&[
            Edit::Teleport { to: teleport },
            stop(5929, Kind::Accept),
            stop(5921, Kind::TurnIn),
            stop(5929, Kind::Accept),
        ]);
        assert!(
            matches!(edited.steps[0].outcome, Outcome::Done),
            "known teleport remains usable in {edition:?}: {:?}",
            edited.steps[0].outcome
        );
        assert!(
            matches!(edited.steps[1].outcome, Outcome::Invalid(_)),
            "Great Bear Spirit still needs Moonglade completed"
        );
        assert!(
            matches!(edited.steps[2].outcome, Outcome::Done),
            "the pending Moonglade quest survives continuation"
        );
        assert!(
            matches!(edited.steps[3].outcome, Outcome::Done),
            "the successor opens after the real turn-in"
        );
    }
}

#[test]
fn an_unambiguous_reward_allows_its_ordinary_successor() {
    let conn = Fixture::new(Edition::Classic).file("classic_hunter").open();
    let overrides = Overrides::load_edition(&repo().join("overrides"), Edition::Classic).unwrap();
    let mut req = request("dwarf", "hunter");
    req.to_level = 11;
    req.start = Some(StartState {
        level: 10,
        spells: vec![1515],
        ..StartState::default()
    });
    let p = job::prepare(&conn, &overrides, &req, &|_| {}).unwrap();
    let planner = Planner::new(&p.model, &p.world, &p.params, &p.profile);
    let edited = planner.edit(&[Edit::Stop {
        stop: Stop::quest(p.model.index[&6086] as u32, Kind::Accept),
        farm: false,
        spot: None,
    }]);
    assert!(
        matches!(edited.steps[0].outcome, Outcome::Done),
        "Tame Beast proves the unique completed Taming the Beast quest: {:?}",
        edited.steps[0].outcome
    );
}

#[test]
fn a_shared_reward_does_not_open_another_quest_branch() {
    for edition in Edition::ALL {
        let conn = Fixture::new(edition).file("classic_warrior").open();
        let overrides = Overrides::load_edition(&repo().join("overrides"), edition).unwrap();
        for (completed, available) in [(1678, false), (1665, true)] {
            let mut req = request("dwarf", "warrior");
            req.to_level = 12;
            req.start = Some(StartState {
                level: 11,
                spells: vec![71], // Defensive Stance: both Vejrek and Bartleby's Mug grant it.
                completed: vec![job::QuestId(completed)],
                ..StartState::default()
            });
            let p = job::prepare(&conn, &overrides, &req, &|_| {}).unwrap();
            let planner = Planner::new(&p.model, &p.world, &p.params, &p.profile);
            let edited = planner.edit(&[Edit::Stop {
                stop: Stop::quest(p.model.index[&1666] as u32, Kind::Accept),
                farm: false,
                spot: None,
            }]);
            assert_eq!(
                matches!(edited.steps[0].outcome, Outcome::Done),
                available,
                "Marshal Haggard needs Bartleby's Mug, not just Defensive Stance: {:?}",
                edited.steps[0].outcome
            );
        }
    }
}

#[test]
fn known_class_rewards_require_every_eligible_group_member() {
    let conn = Fixture::new(Edition::Classic).file("classic_druid").open();
    let overrides = Overrides::load_edition(&repo().join("overrides"), Edition::Classic).unwrap();
    let mut req: PlanRequest = serde_json::from_value(serde_json::json!({
        "race":"human", "class":"mage", "from_level":30, "to_level":31,
        "start":{"name":"Mage","race":1,"class":"MAGE","faction":"Alliance","level":30},
        "mates":[{"name":"Druid","race":4,"class":"DRUID","faction":"Alliance","level":30,"xp":1,"spells":[5487,18960]}]
    }))
    .unwrap();
    let missing_for = |req: &PlanRequest| {
        let p = job::prepare(&conn, &overrides, req, &|_| {}).unwrap();
        Planner::new(&p.model, &p.world, &p.params, &p.profile).missing_class_quests(&[])
    };
    assert!(
        !missing_for(&req)
            .iter()
            .any(|n| n.contains("Body and Heart") || n.contains("Moonglade")),
        "the mage's missing spells do not reopen the druid's known class rewards"
    );
    let start = req.start.as_mut().unwrap();
    start.class = "DRUID".into();
    start.race = 4;
    start.spells = vec![];
    let missing = missing_for(&req);
    assert!(
        missing.iter().any(|n| n.contains("Body and Heart")),
        "the lower-XP druid still needs the bear unlock"
    );
    assert!(
        missing.iter().any(|n| n.contains("Moonglade")),
        "each eligible druid must know the teleport"
    );
    req.start.as_mut().unwrap().spells = vec![5487, 18960];
    req.mates[0].spells.clear();
    let missing = missing_for(&req);
    assert!(
        missing.iter().any(|n| n.contains("Body and Heart")) && missing.iter().any(|n| n.contains("Moonglade")),
        "the higher-XP druid also needs its missing unlocks"
    );
}

#[test]
fn recorded_spells_do_not_complete_an_explicit_quest_requirement() {
    let conn = Fixture::new(Edition::Forever).file("forever_druid").open();
    let overrides = Overrides::load_edition(&repo().join("overrides"), Edition::Forever).unwrap();
    let req:PlanRequest=serde_json::from_value(serde_json::json!({"race":"skyborne_horde","class":"druid","from_level":20,"to_level":21,"required_class_quests":[],"required_quests":[98362],"start":{"level":20,"spells":[768]}})).unwrap();
    let p = job::prepare(&conn, &overrides, &req, &|_| {}).unwrap();
    assert!(
        Planner::new(&p.model, &p.world, &p.params, &p.profile)
            .validate_required(&[])
            .is_err()
    );
}

#[test]
fn known_rewards_keep_imported_log_quests() {
    for edition in Edition::ALL {
        let conn = Fixture::new(edition).file("classic_warrior").open();
        let overrides = Overrides::load_edition(&repo().join("overrides"), edition).unwrap();
        let mut req = request("dwarf", "warrior");
        req.from_level = 11;
        req.to_level = 12;
        req.start = Some(
            serde_json::from_value(
                serde_json::json!({"level":11,"spells":[71],"completed":[1678],"log":[{"id":1665,"complete":true}]}),
            )
            .unwrap(),
        );
        let p = job::prepare(&conn, &overrides, &req, &|_| {}).unwrap();
        let plan = Planner::new(&p.model, &p.world, &p.params, &p.profile);
        let edited = plan.edit(&[Edit::Stop {
            stop: Stop::quest(p.model.index[&1665] as u32, Kind::TurnIn),
            farm: false,
            spot: None,
        }]);
        assert!(
            matches!(edited.steps[0].outcome, Outcome::Done),
            "the held quest can still be turned in: {:?}",
            edited.steps[0].outcome
        );
    }
}

#[test]
fn known_pet_power_survives_continuation() {
    let conn = Fixture::new(Edition::Classic).file("classic_hunter").open();
    let overrides = Overrides::load_edition(&repo().join("overrides"), Edition::Classic).unwrap();
    let mut req = request("dwarf", "hunter");
    req.from_level = 11;
    req.to_level = 12;
    let time = |req: &PlanRequest| {
        let p = job::prepare(&conn, &overrides, req, &|_| {}).unwrap();
        Planner::new(&p.model, &p.world, &p.params, &p.profile)
            .simulate_full(&[], None)
            .unwrap()
            .1
            .grinding
    };
    req.start = Some(serde_json::from_value(serde_json::json!({"level":11})).unwrap());
    let unknown = time(&req);
    for state in [
        serde_json::json!({"level":11,"completed":[6086]}),
        serde_json::json!({"level":11,"spells":[5149]}),
    ] {
        req.start = Some(serde_json::from_value(state).unwrap());
        assert!(
            (unknown / time(&req) - 1.3).abs() < 0.001,
            "the learned pet keeps its 1.3x power"
        );
    }
}

/// The warlock's class power and its spells come with the base: no quest needed.
fn one_known_demon_keeps_the_shared_binding_power(edition: Edition) {
    {
        let conn = Fixture::new(edition).file("warlock_binding").open();
        let overrides = Overrides::load_edition(&repo().join("overrides"), edition).unwrap();
        let mut without = overrides.clone();
        without
            .class_powers
            .get_mut("warlock")
            .unwrap()
            .retain(|(name, _)| name != "Summon Voidwalker");
        let mut req = request("undead", "warlock");
        req.from_level = 25;
        req.to_level = 26;
        for start in [
            None,
            Some(serde_json::from_value(serde_json::json!({"level":25,"spells":[697]})).unwrap()),
        ] {
            req.start = start;
            let time = |overrides: &Overrides| {
                let p = job::prepare(&conn, overrides, &req, &|_| {}).unwrap();
                Planner::new(&p.model, &p.world, &p.params, &p.profile)
                    .simulate_full(&[], None)
                    .unwrap()
                    .1
                    .grinding
            };
            assert!(
                (time(&without) / time(&overrides) - 1.2).abs() < 0.001,
                "{edition:?}: the first known Binding reward grants its power once"
            );
        }
    }
}

#[test]
fn classic_one_known_demon_keeps_the_shared_binding_power() {
    one_known_demon_keeps_the_shared_binding_power(Edition::Classic);
}

#[test]
fn tbc_one_known_demon_keeps_the_shared_binding_power() {
    one_known_demon_keeps_the_shared_binding_power(Edition::Tbc);
}

#[test]
fn forever_one_known_demon_keeps_the_shared_binding_power() {
    one_known_demon_keeps_the_shared_binding_power(Edition::Forever);
}

/// A character with two chains of one class quest, side by side: 1 then 2, and 3 then 4 then 5,
/// what each gives set by the caller. `scenario` adds to it (the requirements).
fn two_versions(
    first: impl Fn(Q) -> Q,
    second: impl Fn(Q) -> Q,
    scenario: impl Fn(Scenario) -> Scenario,
) -> job::Prepared {
    let step = |id, before: &[i64]| {
        Q::new(id, "Call of Air")
            .mandatory()
            .xp(100)
            .after_any(before)
            .giver(1, 40.0 + id as f64, 40.0)
    };
    scenario(
        Scenario::new()
            .levels(1, 3)
            .quest(step(1, &[]))
            .quest(first(step(2, &[1])))
            .quest(step(3, &[]))
            .quest(step(4, &[3]))
            .quest(second(step(5, &[4]))),
    )
    .build()
}

/// Call of Air as the requirement of both chains: the last quest of each is a version.
fn one_requirement(s: Scenario) -> Scenario {
    s.requirement("Call of Air", &[2, 5])
}

fn exclusive_of(p: &job::Prepared, id: i64) -> Vec<i64> {
    let mut with = p.model.quests[p.model.index[&id]].exclusive.clone();
    with.sort_unstable();
    with
}

/// Two chains listed in one requirement are one requirement: the route does one of them (Call of
/// Fire's Searing Totem, 1527 or 97257), and that one is not missing.
#[test]
fn the_versions_of_a_class_requirement_are_done_once() {
    let p = two_versions(|q| q, |q| q, one_requirement);
    assert_eq!(exclusive_of(&p, 1), [3, 4, 5]);
    assert_eq!(exclusive_of(&p, 5), [1, 2]);
    let planner = Planner::new(&p.model, &p.world, &p.params, &p.profile);
    let route = planner.construct(|_| {});
    let turned = |id: i64| {
        let i = p.model.index[&id] as u32;
        route.iter().any(|s| s.kind == Kind::TurnIn && s.index == i)
    };
    assert!(turned(2) != turned(5), "one chain only: {route:?}");
    assert_eq!(planner.missing_class_quests(&route), Vec::<String>::new());
}

/// The Binding: one name, a demon each. Each is a requirement of its own.
#[test]
fn class_quests_of_different_requirements_stay_required() {
    let p = two_versions(
        |q| q,
        |q| q,
        |s| {
            s.requirement("Summon Voidwalker", &[2])
                .requirement("Summon Succubus", &[5])
        },
    );
    assert_eq!(exclusive_of(&p, 1), Vec::<i64>::new());
    assert_eq!(exclusive_of(&p, 5), Vec::<i64>::new());
    assert!(p.model.quests.iter().all(|q| q.mandatory));
}

/// A version the player asked for is the one done: the other is not required, and does not close it.
#[test]
fn a_must_have_version_of_a_class_quest_replaces_the_others() {
    let p = two_versions(|q| q, |q| q, |s| one_requirement(s).required(5));
    let mandatory = |id: i64| p.model.quests[p.model.index[&id]].mandatory;
    assert!(!mandatory(1) && !mandatory(2));
    assert!(mandatory(3) && mandatory(5));
    assert_eq!(exclusive_of(&p, 5), Vec::<i64>::new());
}

/// In a group, a version only one player can take is no substitute for the one everyone can.
#[test]
fn versions_of_a_class_quest_for_other_players_stay_required() {
    let p = two_versions(|q| q.members(0b01), |q| q.members(0b11), one_requirement);
    assert_eq!(exclusive_of(&p, 5), Vec::<i64>::new());
}

/// A chain that cannot be finished is no alternative: the other stays required.
#[test]
fn a_class_quest_chain_that_cannot_be_finished_closes_nothing() {
    let p = two_versions(|q| q.after(&[999]), |q| q, one_requirement);
    assert_eq!(exclusive_of(&p, 3), Vec::<i64>::new());
    assert!(p.model.quests[p.model.index[&5]].mandatory);
}

/// The real versions: the Skyborne shaman's two Call of Fire chains and its Call of Air, and the
/// human priest's Divine Grace. The Binding's demons stay apart.
#[test]
fn same_name_class_quests_in_the_real_data() {
    let conn = database_of("forever");
    let overrides = Overrides::load_edition(&repo().join("overrides"), Edition::Forever).unwrap();
    let prepared = |race: &str, class: &str, to_level| {
        let mut req = request(race, class);
        req.required_class_quests = None;
        req.to_level = to_level;
        job::prepare(&conn, &overrides, &req, &|_| {}).unwrap()
    };
    let linked = |p: &job::Prepared, a: i64, b: i64| exclusive_of(p, a).contains(&b) && exclusive_of(p, b).contains(&a);
    let shaman = prepared("skyborne_horde", "shaman", 60);
    assert!(linked(&shaman, 1527, 97257), "Call of Fire: the same Searing Totem");
    assert!(linked(&shaman, 1532, 96136), "Call of Air: 96136 has no reward row");
    assert!(linked(&shaman, 1531, 96136), "Call of Air");
    let priest = prepared("human", "priest", 20);
    assert!(linked(&priest, 94773, 94774), "Divine Grace");
    let warlock = prepared("orc", "warlock", 40);
    for (a, b) in [(1504, 1513), (1504, 65597), (1513, 65597), (1513, 1795)] {
        assert!(!linked(&warlock, a, b), "The Binding: {a} and {b} are other demons");
    }
}

/// The voidwalker in the real data: an orc or undead warlock chooses between Orgrimmar's and
/// Undercity's, closed once one is taken; a troll has Orgrimmar's only (Forever's sources agree).
#[test]
fn every_race_the_game_allows_can_take_each_voidwalker_version() {
    let conn = database_of("forever");
    let overrides = Overrides::load_edition(&repo().join("overrides"), Edition::Forever).unwrap();
    let prepared = |race: &str| {
        let mut req = request(race, "warlock");
        req.required_class_quests = None;
        req.to_level = 20;
        job::prepare(&conn, &overrides, &req, &|_| {}).unwrap()
    };
    for race in ["orc", "undead"] {
        let p = prepared(race);
        for (orgrimmar, undercity) in [(1501, 1473), (1504, 1471)] {
            assert!(p.model.index.contains_key(&orgrimmar), "{race}: {orgrimmar}");
            assert!(p.model.index.contains_key(&undercity), "{race}: {undercity}");
            assert!(exclusive_of(&p, orgrimmar).contains(&undercity), "{race}: {orgrimmar}");
            assert!(exclusive_of(&p, undercity).contains(&orgrimmar), "{race}: {undercity}");
        }
    }
    let troll = prepared("troll");
    assert!(
        troll.model.quests[troll.model.index[&1504]].mandatory,
        "troll: Orgrimmar's"
    );
    assert!(!troll.model.index.contains_key(&1471), "troll: not Undercity's");
}

/// Call of Earth in the real data: each Horde shaman chooses between Durotar's and Mulgore's
/// (the Troll has Durotar's second step only), for one Earth Totem; the Skyborne have their own.
/// Each version closes the others once taken.
#[test]
fn every_horde_shaman_can_take_each_version_of_call_of_earth() {
    let conn = database_of("forever");
    let overrides = Overrides::load_edition(&repo().join("overrides"), Edition::Forever).unwrap();
    for race in ["orc", "troll", "tauren", "skyborne_horde"] {
        let mut req = request(race, "shaman");
        req.required_class_quests = None;
        req.to_level = 20;
        let p = job::prepare(&conn, &overrides, &req, &|_| {}).unwrap();
        let closes = |a: i64, b: i64| {
            assert!(exclusive_of(&p, a).contains(&b), "{race}: {a} closes {b}");
            assert!(exclusive_of(&p, b).contains(&a), "{race}: {b} closes {a}");
        };
        if race == "skyborne_horde" {
            assert!(!p.model.index.contains_key(&1518), "{race}: not the old chain");
            let earth = p.model.quests[p.model.index[&92468]].mandatory;
            assert!(earth, "{race}: its own Call of Earth");
            continue;
        }
        closes(1518, 1521);
        if race != "troll" {
            closes(1516, 1519);
        }
    }
}

/// A forced zone wants its quests: the version there is the one done.
#[test]
fn a_version_of_a_class_quest_in_a_forced_zone_replaces_the_others() {
    let p = two_versions(Q::zone_forced, |q| q, one_requirement);
    let mandatory = |id: i64| p.model.quests[p.model.index[&id]].mandatory;
    assert!(mandatory(1) && mandatory(2), "the version in the zone");
    assert!(!mandatory(3) && !mandatory(4) && !mandatory(5), "the other");
    assert_eq!(exclusive_of(&p, 2), Vec::<i64>::new());
}

/// Every race and class in the real data: no spell is required twice. Quests teaching it are
/// versions, each closing the others once taken.
#[test]
fn no_race_and_class_is_required_to_learn_a_spell_twice() {
    let overrides = Overrides::load_edition(&repo().join("overrides"), Edition::Forever).unwrap();
    let races = [
        "human",
        "orc",
        "dwarf",
        "nightelf",
        "undead",
        "tauren",
        "gnome",
        "troll",
        "skyborne",
        "skyborne_horde",
    ];
    let classes = [
        "warrior", "paladin", "hunter", "rogue", "priest", "shaman", "mage", "warlock", "druid",
    ];
    let doubles = std::thread::scope(|scope| {
        let of_class = |class: &'static str| {
            let overrides = &overrides;
            scope.spawn(move || {
                let conn = database_of("forever");
                let mut doubles = Vec::new();
                for race in races {
                    let mut req = request(race, class);
                    req.required_class_quests = None;
                    req.to_level = 60;
                    let Ok(p) = job::prepare(&conn, overrides, &req, &|_| {}) else {
                        continue; // Not a class of this race.
                    };
                    let teaching: Vec<_> = (p.model.quests.iter())
                        .filter(|q| q.mandatory && q.reward_spell.is_some())
                        .collect();
                    for a in &teaching {
                        for b in teaching
                            .iter()
                            .filter(|b| a.id < b.id && a.reward_spell == b.reward_spell)
                        {
                            // Two steps of one path (Returning Home before Stars of Elune).
                            let needs = |q: &fg_route::model::Quest, id: i64| {
                                q.pre_all.iter().chain(&q.pre_any).chain(&q.pre_held).any(|&p| p == id)
                            };
                            if needs(a, b.id) || needs(b, a.id) {
                                continue;
                            }
                            if !a.exclusive.contains(&b.id) || !b.exclusive.contains(&a.id) {
                                doubles.push(format!("{race} {class}: {} {} and {}", a.name, a.id, b.id));
                            }
                        }
                    }
                }
                doubles
            })
        };
        let running: Vec<_> = classes.map(of_class).into_iter().collect();
        running.into_iter().flat_map(|t| t.join().unwrap()).collect::<Vec<_>>()
    });
    assert_eq!(doubles, Vec::<String>::new());
}

fn one_spell_by_two_names(extra: impl Fn(Scenario) -> Scenario) -> job::Prepared {
    let step = |id, name: &str| Q::new(id, name).mandatory().xp(100).giver(1, 40.0 + id as f64, 40.0);
    extra(
        Scenario::new()
            .levels(1, 3)
            .requirement("Defensive Stance", &[2, 3])
            .quest(step(1, "Path of Defense"))
            .quest(step(2, "Bartleby's Mug").after_any(&[1]))
            .quest(step(3, "The Skybreaker Bulwark")),
    )
    .build()
}

/// A requirement lists quests of any names: another version, the quests leading to it closed with it.
#[test]
fn class_quests_of_other_names_listed_in_one_requirement_are_versions() {
    let p = one_spell_by_two_names(|s| s);
    assert_eq!(exclusive_of(&p, 3), vec![1, 2]);
    assert_eq!(exclusive_of(&p, 2), vec![3]);
    assert_eq!(exclusive_of(&p, 1), vec![3]);
}

/// A quest another requirement needs stays open: taking the other version must not close it.
#[test]
fn a_prerequisite_something_else_needs_is_not_closed_with_a_version() {
    let p = one_spell_by_two_names(|s| {
        s.requirement("Other", &[4]).quest(
            Q::new(4, "Other")
                .mandatory()
                .xp(100)
                .giver(1, 44.0, 40.0)
                .after_any(&[1]),
        )
    });
    assert_eq!(exclusive_of(&p, 3), vec![2]);
    assert_eq!(exclusive_of(&p, 1), Vec::<i64>::new());
}

/// Every warlock's demon quests: the demon is fought at the quest's own summoning circle, not in
/// another city where the data has seen the same demon.
#[test]
fn a_summoned_demon_is_fought_at_the_circle_of_its_quest() {
    let conn = database_of("forever");
    let overrides = Overrides::load_edition(&repo().join("overrides"), Edition::Forever).unwrap();
    let mut stmt = conn
        .prepare(
            "SELECT q.id, s.zone_id
             FROM m_quest q, json_each(q.extra, '$.extra_objectives') e, json_each(e.value, '$.\"5\"') t, m_spawn s
             WHERE q.classes_mask = 256 AND json_extract(t.value, '$[0]') = 'object'
               AND s.entity_type = 'object' AND s.entity_id = json_extract(t.value, '$[1]')",
        )
        .unwrap();
    let circles: Vec<(i64, i64)> = stmt
        .query_map([], |r| Ok((r.get(0)?, r.get(1)?)))
        .unwrap()
        .map(Result::unwrap)
        .collect();
    assert!(circles.len() >= 10, "{circles:?}");
    let mut checked = 0;
    for race in ["orc", "undead", "troll", "human", "gnome"] {
        let mut req = request(race, "warlock");
        req.required_class_quests = None;
        req.to_level = 60;
        let p = job::prepare(&conn, &overrides, &req, &|_| {}).unwrap();
        for &(id, zone) in &circles {
            let Some(&i) = p.model.index.get(&id) else { continue };
            let at: Vec<i64> = p.model.quests[i].objectives.iter().map(|o| o.loc.zone).collect();
            assert!(
                !at.is_empty() && at.iter().all(|z| *z == zone),
                "{race} {id}: {at:?}, circle in {zone}"
            );
            checked += 1;
        }
    }
    assert!(checked >= 10, "{checked}");
    println!("{checked} demon quests checked");
}
