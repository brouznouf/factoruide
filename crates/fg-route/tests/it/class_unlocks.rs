//! Class chains through normal planning and recorded character starts.

use crate::common::repo;
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

/// A forced zone wants its quests: the version there is the one done.
#[test]
fn a_version_of_a_class_quest_in_a_forced_zone_replaces_the_others() {
    let p = two_versions(Q::zone_forced, |q| q, one_requirement);
    let mandatory = |id: i64| p.model.quests[p.model.index[&id]].mandatory;
    assert!(mandatory(1) && mandatory(2), "the version in the zone");
    assert!(!mandatory(3) && !mandatory(4) && !mandatory(5), "the other");
    assert_eq!(exclusive_of(&p, 2), Vec::<i64>::new());
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
