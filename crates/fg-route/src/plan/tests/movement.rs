//! Learned movement abilities, activation time and terrain through route estimates.

use super::*;

#[test]
fn known_and_trained_movement_spells_change_the_eligible_walking_legs() {
    use crate::terrain::TestGround::{Covered, Dry, Mixed, Shallow, Unknown};
    for (edition, race, class, spell, seconds) in [
        (Edition::Classic, "nightelf", "druid", 783, 61.22449),
        (Edition::Classic, "orc", "shaman", 2645, 63.22449),
        (Edition::Classic, "orc", "hunter", 5118, 65.93407),
        (Edition::Forever, "nightelf", "druid", 783, 61.22449),
        (Edition::Forever, "orc", "shaman", 2645, 61.22449),
        (Edition::Tbc, "nightelf", "druid", 783, 61.22449),
        (Edition::Tbc, "orc", "shaman", 2645, 61.22449),
        (Edition::Tbc, "orc", "hunter", 5118, 65.93407),
        (Edition::Forever, "orc", "hunter", 5118, 65.93407),
    ] {
        let conn = database_of(edition.key());
        let overrides = Overrides::load_edition(&repo().join("overrides"), edition).unwrap();
        let mut req:PlanRequest=serde_json::from_value(serde_json::json!({"race":race,"class":class,"from_level":30,"to_level":31,"required_class_quests":[],"start":{"level":30,"spells":[]}})).unwrap();
        let mut p = at_start(job::prepare(&conn, &overrides, &req, &|_| {}).unwrap(), 1);
        let a = crate::world::Pos {
            continent: 1,
            x: -100.0,
            y: -100.0,
            zone: 0,
            z: None,
        };
        let mut trainer = p.model.start.clone();
        trainer.pos = a;
        p.model.trainers = vec![trainer];
        (p.params.run_speed, p.params.detour) = (7.0, 1.0);
        let quest = [
            crate::plan::Edit::Stop {
                stop: Stop::quest(0, Kind::Accept),
                farm: false,
                spot: None,
            },
            crate::plan::Edit::Stop {
                stop: Stop::quest(0, Kind::TurnIn),
                farm: false,
                spot: None,
            },
        ];
        let trained = [crate::plan::Edit::Train { trainer: 0 }, quest[0], quest[1]];
        let short = if class == "shaman" && edition == Edition::Classic {
            2.0
        } else if class == "hunter" {
            1.53846
        } else {
            1.42857
        };
        let acquisition = if spell == 783 { 30 } else { 20 };
        let earlier = if class == "druid" && edition != Edition::Forever {
            65.93407 // The trainer already offers Cat Form at level 29.
        } else {
            85.71429
        };
        for (level, distance, ground, mount_level, mount_bonus, expected) in [
            (acquisition - 1, 300.0, Dry, 40, 0.6, Some(earlier)),
            (acquisition, 300.0, Dry, 40, 0.6, Some(seconds)),
            (30, 7.0, Dry, 40, 0.6, Some(short)),
            (30, 300.0, Shallow, 40, 0.6, Some(seconds)),
            (edition.mount_level(), 300.0, Dry, edition.mount_level(), 0.6, None),
            (
                edition.mount_level(),
                300.0,
                Dry,
                edition.mount_level(),
                0.2,
                Some(seconds),
            ),
            (30, 300.0, Mixed, 40, 0.6, None),
            (30, 300.0, Covered, 40, 0.6, None),
            (30, 300.0, Unknown, 40, 0.6, None),
        ] {
            let a = crate::world::Pos {
                z: if matches!(ground, Covered) { Some(0) } else { None },
                ..a
            };
            (p.params.mount_level, p.params.mount_bonus) = (mount_level, mount_bonus);
            p.profile.from_level = level;
            p.profile.to_level = level + 1;
            p.model.initial.pos = Some((a, 0));
            p.model.trainers[0].pos = a;
            p.world.terrain = crate::terrain::movement_test_ground(ground);
            let q = &mut p.model.quests[0];
            q.level = level;
            q.xp = 100_000;
            q.starts[0].pos = crate::world::Pos { x: a.x - distance, ..a };
            q.ends[0].pos = a;
            let plan = planner(&p).explicit();
            let before = plan.edit(&quest);
            let after = plan.edit(&trained);
            assert_eq!(after.errors(), 0);
            if let Some(seconds) = expected {
                assert!(
                    (after.breakdown.travel - seconds).abs() < 0.05,
                    "{edition:?} {class} level {level} mount {mount_bonus}: got {}",
                    after.breakdown.travel
                );
                let baseline = if level >= mount_level {
                    71.42857
                } else if distance > 7.0 {
                    85.71429
                } else {
                    2.0
                };
                assert!(
                    (before.breakdown.travel - baseline).abs() < 0.05,
                    "{edition:?} {class}: recorded empty spellbook must wait for training"
                );
                assert!(after.breakdown.travel <= before.breakdown.travel);
                assert_eq!(
                    plan.edit(&quest).breakdown.travel,
                    before.breakdown.travel,
                    "untrained replay keeps its speed"
                );
                if distance > 7.0 && level >= acquisition {
                    let automatic = Planner::new(&p.model, &p.world, &p.params, &p.profile);
                    let guide = crate::export::build(&automatic, &p.profile, &automatic.greedy());
                    assert!(
                        guide
                            .steps
                            .iter()
                            .any(|s| s.kind == crate::export::StepKind::Train && s.spells.contains(&spell)),
                        "{class}: schedule movement training"
                    );
                    assert!(
                        guide.spells.is_empty(),
                        "without a combat catalog the addon may train every spell"
                    );
                }
            } else {
                assert_eq!(
                    after.breakdown.travel, before.breakdown.travel,
                    "{class}: excluded terrain and faster mounts keep the baseline"
                );
            }
        }
        p.world.terrain = crate::terrain::movement_test_ground(Dry);
        req.start.as_mut().unwrap().spells = vec![spell];
        let recorded = job::prepare(&conn, &overrides, &req, &|_| {}).unwrap();
        let native_planner = planner(&recorded);
        let native_guide = crate::export::build(&native_planner, &recorded.profile, &[]);
        if edition == Edition::Tbc {
            assert!(
                native_guide.spells.is_empty(),
                "TBC keeps unrestricted trainer purchases"
            );
        } else {
            assert!(native_guide.spells.contains(&spell));
        }
        let recorded_abilities = recorded.model.initial.abilities;
        p.model.initial.abilities = recorded_abilities;
        assert!(
            (planner(&p).explicit().edit(&quest).breakdown.travel - seconds).abs() < 0.05,
            "{class}: imported knowledge applies from the first leg"
        );
        req.start = None;
        let synthetic = job::prepare(&conn, &overrides, &req, &|_| {}).unwrap();
        p.model.initial.abilities = synthetic.model.initial.abilities;
        assert!(
            (planner(&p).explicit().edit(&quest).breakdown.travel - seconds).abs() < 0.05,
            "{class}: presets follow available acquisition data"
        );
        p.model.initial.abilities = recorded_abilities;
        p.params.assume_movement_talents = false;
        let expected = match (class, edition) {
            ("shaman", Edition::Tbc) => 65.22449,
            ("shaman", _) => 67.22449,
            _ => seconds,
        };
        assert!(
            (planner(&p).explicit().edit(&quest).breakdown.travel - expected).abs() < 0.05,
            "the talent option changes Ghost Wolf's cast, not Cheetah or Travel Form"
        );
    }
}

#[test]
fn cat_speed_requires_its_quest_and_the_talent_assumption() {
    let conn = database_of("forever");
    let overrides = Overrides::load_edition(&repo().join("overrides"), Edition::Forever).unwrap();
    let req:PlanRequest=serde_json::from_value(serde_json::json!({"race":"skyborne_horde","class":"druid","from_level":20,"to_level":21,"required_class_quests":[],"start":{"level":20}})).unwrap();
    let mut p = at_start(job::prepare(&conn, &overrides, &req, &|_| {}).unwrap(), 2);
    let a = crate::world::Pos {
        continent: 1,
        x: -100.0,
        y: -100.0,
        zone: 0,
        z: None,
    };
    p.model.initial.pos = Some((a, 0));
    p.world.terrain = crate::terrain::movement_test_ground(crate::terrain::TestGround::Dry);
    (p.params.run_speed, p.params.detour) = (7.0, 1.0);
    p.model.quests[0].id = 98362; // The final Horde Cat Form hand-in, after its trial.
    p.model.index = p.model.quests.iter().enumerate().map(|(i, q)| (q.id, i)).collect();
    for q in &mut p.model.quests {
        q.level = 20;
        q.starts[0].pos = a;
        q.ends[0].pos = a;
    }
    p.model.quests[1].xp = 100_000;
    p.model.quests[1].starts[0].pos = crate::world::Pos { x: -400.0, ..a };
    p.model.quests[1].ends[0].pos = crate::world::Pos { x: -400.0, ..a };
    let mut trainer = p.model.start.clone();
    trainer.pos = a;
    p.model.trainers = vec![trainer];
    let edits = |i| {
        [
            crate::plan::Edit::Stop {
                stop: Stop::quest(i, Kind::Accept),
                farm: false,
                spot: None,
            },
            crate::plan::Edit::Stop {
                stop: Stop::quest(i, Kind::TurnIn),
                farm: false,
                spot: None,
            },
        ]
    };
    let far = edits(1);
    let mut trained = vec![crate::plan::Edit::Train { trainer: 0 }];
    trained.extend(far);
    let plan = planner(&p).explicit();
    let locked = plan.edit(&trained);
    let mut unlock = edits(0).to_vec();
    unlock.extend(far);
    let learned = plan.edit(&unlock);
    assert_eq!(learned.errors(), 0);
    assert!(
        (locked.breakdown.travel - 42.85714).abs() < 0.01,
        "training cannot buy Cat Form: {}",
        locked.breakdown.travel
    );
    assert!(
        (learned.breakdown.travel - 32.96703).abs() < 0.01,
        "the completed quest enables Feral Swiftness"
    );
    p.profile.from_level = 20;
    p.profile.to_level = 21;
    p.model.initial.abilities = 0;
    p.params.assume_movement_talents = false;
    let without = planner(&p).explicit().edit(&unlock);
    assert!(
        (without.breakdown.travel - locked.breakdown.travel).abs() < 0.01,
        "without the talent Cat Form has no speed bonus"
    );
}

#[test]
fn classic_and_tbc_cat_speed_follow_the_available_feral_swiftness_rank() {
    for edition in [Edition::Classic, Edition::Tbc] {
        let conn = database_of(edition.key());
        let mut overrides = Overrides::parse_edition(
            |name| {
                if edition == Edition::Classic && name == "classic/movement.toml" {
                    // A refreshed native catalog takes precedence over an older supplemental level.
                    Some("[training]\n\"Cat Form\" = 19\n".into())
                } else {
                    std::fs::read_to_string(repo().join("overrides").join(name)).ok()
                }
            },
            edition,
        )
        .unwrap();
        overrides.params.zone_gap = 0.0; // A supported setting: zone closures do not separate cached legs.
        let req: PlanRequest = serde_json::from_value(serde_json::json!({
            "race":"nightelf","class":"druid","from_level":20,"to_level":22,
            "required_class_quests":[],"start":{"level":20,"spells":[]}
        }))
        .unwrap();
        let mut p = at_start(job::prepare(&conn, &overrides, &req, &|_| {}).unwrap(), 2);
        let a = crate::world::Pos {
            continent: 1,
            x: -100.0,
            y: -100.0,
            zone: 0,
            z: None,
        };
        let mut trainer = p.model.start.clone();
        trainer.pos = a;
        p.model.trainers = vec![trainer];
        p.model.initial.pos = Some((a, 0));
        p.world.terrain = crate::terrain::movement_test_ground(crate::terrain::TestGround::Dry);
        (p.params.run_speed, p.params.detour) = (7.0, 1.0);
        for q in &mut p.model.quests {
            q.starts[0].pos = a;
            q.ends[0].pos = a;
            q.level = 20;
        }
        p.model.quests[0].xp = edition.rules().to_next_level(20);
        p.model.quests[1].xp = 100_000;
        let b = crate::world::Pos { x: -400.0, ..a };
        p.model.quests[1].starts[0].pos = b;
        p.model.quests[1].ends[0].pos = b;
        let edits = |i| {
            [
                crate::plan::Edit::Stop {
                    stop: Stop::quest(i, Kind::Accept),
                    farm: false,
                    spot: None,
                },
                crate::plan::Edit::Stop {
                    stop: Stop::quest(i, Kind::TurnIn),
                    farm: false,
                    spot: None,
                },
            ]
        };
        let far = edits(1);
        let plan = planner(&p).explicit();
        let train = crate::plan::Edit::Train { trainer: 0 };
        let locked = plan.edit(&far);
        assert!(
            (locked.breakdown.travel - 42.85714).abs() < 0.01,
            "Cat Form is unavailable before training"
        );
        p.profile.from_level = 19;
        let too_early = planner(&p).explicit().edit(&[train, far[0], far[1]]);
        assert!(
            (too_early.breakdown.travel - 42.85714).abs() < 0.01,
            "a level-19 trainer visit cannot teach Cat Form"
        );
        p.profile.from_level = 20;
        let plan = planner(&p).explicit();
        let before = plan.edit(&[train, far[0], far[1]]);
        let after = plan.edit(&[train, edits(0)[0], edits(0)[1], far[0], far[1]]);
        assert_eq!(after.errors(), 0);
        assert!(
            (before.breakdown.travel - 37.26708).abs() < 0.01,
            "rank one at level 20"
        );
        assert!(
            (after.breakdown.travel - 32.96703).abs() < 0.01,
            "the same leg at level 21 uses rank two"
        );
        p.params.assume_movement_talents = false;
        let without = planner(&p).explicit().edit(&[train, far[0], far[1]]);
        assert!(
            (without.breakdown.travel - 42.85714).abs() < 0.01,
            "without the talent Cat Form keeps running speed"
        );
    }
}

#[test]
fn group_travel_keeps_the_shared_speed_when_the_druid_knows_travel_form() {
    for edition in Edition::ALL {
        let conn = database_of(edition.key());
        let overrides = Overrides::load_edition(&repo().join("overrides"), edition).unwrap();
        let req: PlanRequest = serde_json::from_value(serde_json::json!({
            "race":"nightelf","class":"druid","from_level":30,"to_level":31,
            "required_class_quests":[],
            "start":{"name":"Bark-Realm","race":4,"class":"DRUID","faction":"Alliance","level":30,"spells":[783]},
            "mates":[{"name":"Steel-Realm","race":1,"class":"WARRIOR","faction":"Alliance","level":30}]
        }))
        .unwrap();
        let prepared = job::prepare(&conn, &overrides, &req, &|_| {}).unwrap();
        let mates = prepared.model.initial.mates.clone();
        let mut p = at_start(prepared, 1);
        p.model.initial.mates = mates;
        p.model.initial.need = vec![0b11];
        let a = crate::world::Pos {
            continent: 1,
            x: -100.0,
            y: -100.0,
            zone: 0,
            z: None,
        };
        p.model.initial.pos = Some((a, 0));
        p.world.terrain = crate::terrain::movement_test_ground(crate::terrain::TestGround::Dry);
        (p.params.run_speed, p.params.detour, p.params.mount_level) = (7.0, 1.0, 40);
        let q = &mut p.model.quests[0];
        q.level = 30;
        q.xp = 100_000;
        q.starts[0].pos = crate::world::Pos { x: -400.0, ..a };
        q.ends[0].pos = a;
        let planner = planner(&p).explicit();
        let route = [Stop::quest(0, Kind::Accept), Stop::quest(0, Kind::TurnIn)];
        assert!(planner.end_of(&route).is_some(), "the group route replays");
        let guide = crate::export::build(&planner, &p.profile, &route);
        assert!(
            (guide.breakdown.travel - 85.71429).abs() < 0.01,
            "{edition:?}: the warrior cannot share the druid's movement speed"
        );
    }
}
