//! Quest rewards, real training and continuing character spell knowledge.

use super::*;

#[test]
fn late_form_unlock_does_not_credit_unlearned_attack_ranks() {
    let conn = database_of("forever");
    for (family, rank, quest_id) in [
        ("Claw", 3029, 98362),
        ("Rip", 9492, 98362),
        ("Rake", 1822, 98362),
        ("Shred", 5221, 98362),
        ("Tiger's Fury", 5217, 98362),
        ("Ferocious Bite", 22568, 98362),
        ("Maul", 6808, 94638),
        ("Swipe", 779, 94638),
    ] {
        let mut overrides = Overrides::load_edition(&repo().join("overrides"), Edition::Forever).unwrap();
        overrides
            .spells
            .get_mut("druid")
            .unwrap()
            .retain(|name, _| name == family);
        let req: PlanRequest = serde_json::from_value(serde_json::json!({
            "race":"skyborne_horde", "class":"druid", "from_level":40, "to_level":41,
            "required_class_quests":[], "start":{"level":40}
        }))
        .unwrap();
        let mut original = job::prepare(&conn, &overrides, &req, &|_| {}).unwrap();
        let power = original.model.power.take();
        let mut p = at_start(original, 2);
        p.model.power = power;
        p.model.trainers.push(p.model.start.clone());
        p.model.quests[0].id = quest_id;
        p.model.quests[0].level = if quest_id == 98362 { 20 } else { 10 };
        p.model.index = [(quest_id, 0)].into_iter().collect();
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
        let train = crate::plan::Edit::Train { trainer: 0 };
        let plan = planner(&p).replaying().explicit();
        let before = plan.edit(&[train, quest[0], quest[1]]);
        let after = plan.edit(&[quest[0], quest[1], train]);
        let export = |edited: &crate::plan::Edited| {
            crate::export::build_traced(
                &plan,
                &p.profile,
                &edited.route,
                &edited.trace,
                edited.breakdown.total,
                edited.breakdown,
            )
        };
        let before_guide = export(&before);
        let after_guide = export(&after);
        let power =
            |guide: &crate::export::Route, kind| guide.steps.iter().find(|s| s.kind == kind).unwrap().power.unwrap();
        let trained_spells = |guide: &crate::export::Route| {
            guide
                .steps
                .iter()
                .filter(|s| s.kind == crate::export::StepKind::Train)
                .flat_map(|s| s.spells.iter().copied())
                .collect::<Vec<_>>()
        };
        let fully_learned = power(&after_guide, crate::export::StepKind::Train);
        if family == "Claw" || family == "Rip" {
            assert!(
                power(&before_guide, crate::export::StepKind::TurnIn)
                    > power(&before_guide, crate::export::StepKind::Accept),
                "{family}: the free starter attack contributes after the quest"
            );
        }
        assert!(
            power(&before_guide, crate::export::StepKind::Accept) < fully_learned,
            "{family}: no combat share before the form unlock"
        );
        assert!(
            power(&before_guide, crate::export::StepKind::TurnIn) < fully_learned,
            "{family}: the earlier trainer visit cannot supply later combat ranks after the unlock"
        );
        assert!(
            power(&after_guide, crate::export::StepKind::TurnIn) < fully_learned,
            "{family}: the quest does not train every rank available at level 40"
        );
        assert!(
            !trained_spells(&before_guide).contains(&rank),
            "{family}: cannot train before the form"
        );
        assert!(
            trained_spells(&after_guide).contains(&rank),
            "{family}: can train after the quest at the same level"
        );
        assert!(
            !trained_spells(&after_guide).contains(&1082),
            "Claw 1 came free from the quest"
        );
        let reward = plan.end_of(&before.route).unwrap();
        let reward = job::StartState::default().after(&p.model, &p.world, Edition::Forever.rules(), &reward);
        if quest_id == 98362 {
            for starter in [768, 1082, 1079, 5215] {
                assert!(
                    reward.spells.contains(&starter),
                    "Cat quest grants starter spell {starter}"
                );
            }
            assert!(
                !after_guide.spells.contains(&768),
                "quest-only forms are not trainer purchases"
            );
        }
        assert!(
            !reward.spells.contains(&rank),
            "{family}: the earlier trainer visit did not learn a locked rank"
        );
        p.params.train_min_gain = 0.000_001;
        p.params.train_trip_gain = 0.000_001;
        let automatic = planner(&p).replaying();
        let route = [
            Stop::quest(0, Kind::Accept),
            Stop::quest(0, Kind::TurnIn),
            Stop::quest(1, Kind::Accept),
            Stop::quest(1, Kind::TurnIn),
        ];
        let end = automatic.end_of(&route).unwrap();
        let learned = job::StartState::default().after(&p.model, &p.world, Edition::Forever.rules(), &end);
        assert!(
            learned.spells.contains(&rank),
            "{family}: continuing profiles retain the trained rank"
        );
        let mut next = req.clone();
        next.to_level = learned.level + 1;
        next.start = Some(learned);
        let mut continued = at_start(job::prepare(&conn, &overrides, &next, &|_| {}).unwrap(), 1);
        continued.model.trainers.push(continued.model.start.clone());
        let next_plan = planner(&continued).replaying().explicit();
        let edited = next_plan.edit(&[train]);
        let guide = crate::export::build_traced(
            &next_plan,
            &continued.profile,
            &edited.route,
            &edited.trace,
            edited.breakdown.total,
            edited.breakdown,
        );
        assert!(
            !trained_spells(&guide).contains(&rank),
            "{family}: continuing does not rebuy the learned rank"
        );
        if family == "Claw" {
            // The spellbook records only the highest rank; continuing guides must not rebuy it.
            let mut imported = req;
            imported.start.as_mut().unwrap().spells = vec![768, 3029, 1079];
            let imported = job::prepare(&conn, &overrides, &imported, &|_| {}).unwrap();
            p.model.initial.abilities = imported.model.initial.abilities;
            let plan = planner(&p).replaying().explicit();
            let replay = plan.edit(&[train]);
            let guide = crate::export::build_traced(
                &plan,
                &p.profile,
                &replay.route,
                &replay.trace,
                replay.breakdown.total,
                replay.breakdown,
            );
            assert!(!trained_spells(&guide).contains(&3029));
            assert!(!trained_spells(&guide).contains(&1082));
        }
    }
}

#[test]
fn undead_paladin_learns_holy_light_from_its_quest() {
    let conn = database_of("forever");
    let overrides = Overrides::load_edition(&repo().join("overrides"), Edition::Forever).unwrap();
    let req: PlanRequest = serde_json::from_value(serde_json::json!({
        "race":"undead","class":"paladin","from_level":6,"to_level":7,
        "required_class_quests":[],"start":{"level":6}
    }))
    .unwrap();
    let mut p = at_start(job::prepare(&conn, &overrides, &req, &|_| {}).unwrap(), 1);
    p.model.quests[0].id = 90902; // Rediscovering the Light rewards Holy Light 1.
    p.model.index = [(90902, 0)].into_iter().collect();
    let trainer = p.model.start.clone();
    p.model.trainers.push(trainer);
    let plan = planner(&p).replaying().explicit();
    let before = plan.edit(&[crate::plan::Edit::Train { trainer: 0 }]);
    let after = plan.edit(&[
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
    ]);
    let spells = |route: &[Stop]| {
        let end = plan.end_of(route).unwrap();
        job::StartState::default()
            .after(&p.model, &p.world, Edition::Forever.rules(), &end)
            .spells
    };
    assert!(
        !spells(&before.route).contains(&635),
        "the trainer cannot bypass the class quest"
    );
    let before_guide = crate::export::build_traced(
        &plan,
        &p.profile,
        &before.route,
        &before.trace,
        before.breakdown.total,
        before.breakdown,
    );
    assert!(
        !before_guide
            .steps
            .iter()
            .any(|s| s.kind == crate::export::StepKind::Train && s.spells.contains(&639)),
        "a later rank cannot bypass the starter quest"
    );
    assert!(
        spells(&after.route).contains(&635),
        "another class uses the same quest reward path"
    );
    let learned = plan.edit(&[
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
        crate::plan::Edit::Train { trainer: 0 },
    ]);
    let guide = crate::export::build_traced(
        &plan,
        &p.profile,
        &learned.route,
        &learned.trace,
        learned.breakdown.total,
        learned.breakdown,
    );
    assert!(
        guide
            .steps
            .iter()
            .any(|s| s.kind == crate::export::StepKind::Train && s.spells.contains(&639)),
        "training after the quest teaches Holy Light 2"
    );
}
#[test]
fn starting_knowledge_uses_recorded_spells_or_earlier_synthetic_unlocks() {
    let conn = database_of("forever");
    let overrides = Overrides::load_edition(&repo().join("overrides"), Edition::Forever).unwrap();
    for (level, recorded, spells, completed, expected) in [
        (20, true, vec![], vec![], false),
        (20, true, vec![768], vec![], true),
        (20, true, vec![], vec![98362], true),
        (21, true, vec![], vec![], false),
        (21, true, vec![768], vec![], true),
        (21, true, vec![], vec![98362], true),
        (20, false, vec![], vec![], false),
        (21, false, vec![], vec![], true),
    ] {
        for selected in [false, true] {
            let req: PlanRequest = serde_json::from_value(serde_json::json!({
                "race":"skyborne_horde", "class":"druid", "from_level":level, "to_level":level+1,
                "required_class_quests":if selected { serde_json::Value::Null } else { serde_json::json!([]) },
                "start":if recorded { serde_json::json!({"level":level,"spells":spells,"completed":completed}) } else { serde_json::Value::Null }
            })).unwrap();
            let mut p = at_start(job::prepare(&conn, &overrides, &req, &|_| {}).unwrap(), 1);
            p.model.quests[0].level = level;
            p.model.quests[0].xp = 100_000;
            let plan = planner(&p).explicit();
            let end = plan
                .end_of(&[Stop::quest(0, Kind::Accept), Stop::quest(0, Kind::TurnIn)])
                .unwrap();
            let start = job::StartState::default().after(&p.model, &p.world, Edition::Forever.rules(), &end);
            assert_eq!(
                start.spells.contains(&768),
                expected,
                "level {level}, recorded {recorded}, class quests selected {selected}"
            );
        }
    }
}

#[test]
fn higher_rank_and_cosmetic_mage_quests_do_not_gate_basic_training() {
    for (edition, key) in [(Edition::Classic, "classic"), (Edition::Forever, "forever")] {
        let conn = database_of(key);
        let overrides = Overrides::load_edition(&repo().join("overrides"), edition).unwrap();
        let req: PlanRequest = serde_json::from_value(serde_json::json!({
            "race":"human", "class":"mage", "from_level":8, "to_level":9,
            "required_class_quests":[], "start":{"level":8,"spells":[133]}
        }))
        .unwrap();
        let mut original = job::prepare(&conn, &overrides, &req, &|_| {}).unwrap();
        let power = original.model.power.take();
        let mut p = at_start(original, 1);
        p.model.power = power;
        p.model.trainers.push(p.model.start.clone());
        let plan = planner(&p).replaying().explicit();
        let edited = plan.edit(&[crate::plan::Edit::Train { trainer: 0 }]);
        let guide = crate::export::build_traced(
            &plan,
            &p.profile,
            &edited.route,
            &edited.trace,
            edited.breakdown.total,
            edited.breakdown,
        );
        let taught: Vec<_> = guide
            .steps
            .iter()
            .filter(|s| s.kind == crate::export::StepKind::Train)
            .flat_map(|s| s.spells.iter().copied())
            .collect();
        for spell in [5504, 118] {
            assert!(
                taught.contains(&spell),
                "{key}: basic mage spell {spell} needs no level-60 quest"
            );
        }
        let end = plan.end_of(&edited.route).unwrap();
        let learned = job::StartState::default().after(&p.model, &p.world, edition.rules(), &end);
        let mut next = req;
        next.to_level = learned.level + 1;
        next.start = Some(job::StartState {
            spells: taught,
            ..learned.clone()
        });
        let continued = at_start(job::prepare(&conn, &overrides, &next, &|_| {}).unwrap(), 1);
        let end = planner(&continued).replaying().explicit().end_of(&[]).unwrap();
        let learned = learned.after(&continued.model, &continued.world, edition.rules(), &end);
        for quest_spell in [10140, 28272] {
            assert!(
                !learned.spells.contains(&quest_spell),
                "{key}: a lower rank does not prove quest reward {quest_spell}"
            );
        }
    }
}
