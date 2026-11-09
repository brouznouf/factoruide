use super::{MandatoryOrigins as Origins, chains, derive, sources::QuestRow};
use crate::{
    plan::{MandatoryGoalCounts, Planner},
    testing::{Q, Scenario},
};
use std::collections::{HashMap, HashSet};

#[test]
fn loader_seeds_only_applicable_selected_or_all_class_requirements() {
    use crate::{fixture::Fixture, job, xp::Edition};
    let conn = Fixture::new(Edition::Classic)
        .toml(
            r#"
[[npc]]
id = 990001
friendly_to = "AH"
at = [{ zone = 12, x = 48.0, y = 42.0 }]
[[quest]]
id = 990101
name = "Selected class"
zone = -81
giver = 990001
[[quest]]
id = 990102
name = "Unselected class"
classes_mask = 1
giver = 990001
[[quest]]
id = 990103
name = "Ordinary selected member"
giver = 990001
"#,
        )
        .open();
    conn.execute_batch(
        "INSERT INTO m_class_requirement (id,class_id,name,by_default,source) VALUES (990100,1,'Selected',0,'test');
        INSERT INTO m_class_requirement_quest (requirement_id,quest_id) VALUES (990100,990101),(990100,990103);",
    )
    .unwrap();
    let overrides = job::Overrides::load_edition(
        &std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../overrides"),
        Edition::Classic,
    )
    .unwrap();
    for all in [false, true] {
        let request = serde_json::from_value(serde_json::json!({
            "race":"human", "class":"warrior", "from_level":1, "to_level":5,
            "required_class_quests":["Selected"],
            "params":{"edition":"classic", "bis":"off", "all_class_quests":all}
        }))
        .unwrap();
        let p = job::prepare(&conn, &overrides, &request, &|_| {}).unwrap();
        for (id, expected) in [(990101, true), (990102, all), (990103, false)] {
            let q = &p.model.quests[p.model.index[&id]];
            assert_eq!(q.mandatory, expected);
            assert_eq!(
                q.mandatory_origins.contains(Origins::CLASS),
                expected,
                "{id}, all={all}"
            );
        }
    }
}

#[test]
fn released_class_version_reused_for_gear_has_no_retired_class_origin() {
    let mut p = Scenario::new()
        .levels(1, 5)
        .quest(Q::new(1, "Chosen").mandatory_from(Origins::CLASS).zone_forced())
        .quest(Q::new(2, "Released").mandatory_from(Origins::CLASS.union(Origins::EXACT)))
        .requirement("Unlock", &[1, 2])
        .build();
    assert!(!p.model.quests[1].mandatory);
    assert!(!p.model.quests[1].mandatory_origins.contains(Origins::CLASS));
    assert!(p.model.quests[1].mandatory_origins.contains(Origins::EXACT));
    super::select_test_sources(&mut p, &[1], &[]);
    assert!(p.model.quests[1].mandatory_origins.contains(Origins::GEAR));
    assert!(!p.model.quests[1].mandatory_origins.contains(Origins::CLASS));
    assert_eq!(
        Planner::new(&p.model, &p.world, &p.params, &p.profile)
            .simulate_full(&[], None)
            .unwrap()
            .1
            .mandatory_goal_counts,
        Some(MandatoryGoalCounts {
            class_quests: 1,
            bis_quests: 1,
            other_quests: 0
        })
    );
}

#[test]
fn impossible_class_release_retires_only_class_origin() {
    let mut p = Scenario::new().quest(Q::new(1, "Blocked")).build();
    let q = &mut p.model.quests[0];
    q.pre_all = vec![9999];
    q.mandatory = true;
    q.mandatory_origins = Origins::CLASS.union(Origins::EXACT);
    chains::release_impossible_class_quests(&mut p.model.quests, &HashSet::new(), &HashSet::new());
    let q = &p.model.quests[0];
    assert!(!q.mandatory);
    assert!(!q.mandatory_origins.contains(Origins::CLASS));
    assert!(q.mandatory_origins.contains(Origins::EXACT));
}

#[test]
fn released_class_root_does_not_leave_class_origins_on_its_alternative_versions() {
    for blocked in [false, true] {
        let root = Q::new(3, "Class root")
            .mandatory_from(Origins::CLASS)
            .after_any(&[1, 2]);
        let root = if blocked { root.after(&[9999]) } else { root };
        let p = Scenario::new()
            .levels(1, 5)
            .quest(Q::new(1, "Alternative A"))
            .quest(Q::new(2, "Alternative B"))
            .quest(root)
            .requirement("Unlock", &[3])
            .build();
        assert_eq!(p.model.quests[2].mandatory, !blocked);
        for q in &p.model.quests[..2] {
            assert!(q.mandatory, "legacy flags stay unchanged");
            assert_eq!(q.mandatory_origins.contains(Origins::CLASS), !blocked);
        }
    }
}

#[test]
fn known_class_reward_reused_by_an_independent_goal_loses_class_origin() {
    for (exact, zone) in [(false, false), (true, false), (false, true)] {
        let mut p = Scenario::new()
            .quest(Q::new(1, "Known unlock").mandatory_from(Origins::CLASS))
            .build();
        p.model.quests[0].required = exact;
        p.model.quests[0].zone_forced = zone;
        if zone {
            p.model.quests[0].mandatory_origins.insert(Origins::FORCED_ZONE);
        }
        let rows = [QuestRow {
            id: 1,
            name: "Known unlock".into(),
            level: Some(1),
            min_level: Some(1),
            classes: Some(1),
            sort: Some(-81),
            ..QuestRow::default()
        }];
        let mut knowledge = derive::Knowledge {
            rewards: HashSet::from([1]),
            reward_quests: HashSet::from([1]),
            class_quests: HashSet::from([1]),
            ..derive::Knowledge::default()
        };
        derive::settle(
            &mut p.model.quests,
            &mut Vec::new(),
            &mut [],
            &rows,
            &mut knowledge,
            &HashMap::new(),
            &p.profile,
            &p.params,
        )
        .unwrap();
        assert_eq!(p.model.quests[0].mandatory, exact || zone);
        assert!(!p.model.quests[0].mandatory_origins.contains(Origins::CLASS));
        super::select_test_sources(&mut p, &[0], &[]);
        assert!(!p.model.quests[0].mandatory_origins.contains(Origins::CLASS));
    }
}

#[test]
fn dungeon_guide_origins_reach_the_outdoor_prerequisite() {
    use crate::testing::{At, Obj};
    let p = Scenario::new()
        .levels(18, 18)
        .zone(1, "Westfall")
        .dungeon("The Deadmines", 1, 70.0, 70.0)
        .objectives(&[super::GuideObjective::Dungeon(900)])
        .quest(Q::new(1, "Lead").level(18))
        .quest(
            Q::new(2, "Inside").level(18).after(&[1]).objective(
                Obj::kill(
                    "Defias",
                    18,
                    6,
                    At {
                        zone: 1,
                        x: 70.0,
                        y: 70.0,
                    },
                )
                .inside(0),
            ),
        )
        .build();
    for id in [1, 2] {
        let q = &p.model.quests[p.model.index[&id]];
        assert!(q.mandatory && q.zone_forced);
        assert!(
            q.mandatory_origins.contains(Origins::FORCED_DUNGEON),
            "{id} keeps the reason it is required"
        );
    }
}
