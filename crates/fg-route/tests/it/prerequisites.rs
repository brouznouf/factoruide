//! Existing quest chains remain usable across duplicated sources and faction variants.
use crate::common::{database_of, repo};
use fg_route::job::{self, Overrides, PlanRequest};
use fg_route::plan::{Edit, Kind, Outcome, Planner, Stop};
use fg_route::xp::Edition;

#[test]
fn a_group_keeps_each_classes_distinct_prerequisite() {
    let conn = database_of("classic");
    let overrides = Overrides::load_edition(&repo().join("overrides"), Edition::Classic).unwrap();
    let req: PlanRequest = serde_json::from_value(serde_json::json!({
        "race":"orc","class":"warrior","from_level":3,"to_level":4,
        "required_class_quests":[],
        "start":{"name":"Warrior-Realm","race":2,"class":"WARRIOR","faction":"Horde","level":3,
            "completed":[792],"position":{"map":1411,"x":43.3,"y":68.7}},
        "mates":[{"name":"Warlock-Realm","race":2,"class":"WARLOCK","faction":"Horde","level":3,
            "completed":[1499],"position":{"map":1411,"x":43.3,"y":68.7}}]
    }))
    .unwrap();
    let p = job::prepare(&conn, &overrides, &req, &|_| {}).unwrap();
    let prerequisites = &p.model.quests[p.model.index[&794]].pre_any;
    assert_eq!(prerequisites.len(), 2);
    assert!(prerequisites.contains(&792), "the warrior's Vile Familiars branch");
    assert!(prerequisites.contains(&1499), "the warlock's Vile Familiars branch");
}

#[test]
fn duplicate_links_preserve_each_factions_cat_prerequisite() {
    let conn = database_of("forever");
    let overrides = Overrides::load_edition(&repo().join("overrides"), Edition::Forever).unwrap();
    for (race, prerequisite) in [("skyborne_horde", 98340), ("skyborne", 98393)] {
        let req: PlanRequest = serde_json::from_value(serde_json::json!({
            "race":race,"class":"druid","from_level":1,"to_level":30
        }))
        .unwrap();
        let p = job::prepare(&conn, &overrides, &req, &|_| {}).unwrap();
        assert!(
            p.model.quests[p.model.index[&prerequisite]].mandatory,
            "{race}: the eligible Windborne Cat prerequisite must be kept"
        );
    }
}

/// The character and the quest of a case of `alternatives`.
struct Who {
    quest: i64,
    race: &'static str,
    class: &'static str,
    level: i64,
    completed: Option<&'static [i64]>,
}

/// A quest whose predecessors are alternatives needs the one of the character's own faction or
/// class: taking `quest` at `level` as `race` `class`, with `completed` done (None: nothing known of
/// the character), works or not (`expected`).
fn alternatives(edition: Edition, refreshed: bool, who: &Who, expected: bool) {
    let Who {
        quest,
        race,
        class,
        level,
        completed,
    } = *who;
    let conn = database_of(edition.key());
    let overrides = Overrides::load_edition(&repo().join("overrides"), edition).unwrap();
    if refreshed {
        conn.execute_batch(
            "CREATE TEMP TABLE m_quest_link AS SELECT * FROM main.m_quest_link;
             UPDATE m_quest_link SET kind='pre_any' WHERE quest_id=79192 AND kind='pre_all';",
        )
        .unwrap();
    }
    let req: PlanRequest = serde_json::from_value(serde_json::json!({
        "race":race,"class":class,"from_level":level,"to_level":level+1,
        "required_class_quests":[],"start":completed.map(|completed|serde_json::json!({"level":level,"completed":completed}))
    }))
    .unwrap();
    let mut p = job::prepare(&conn, &overrides, &req, &|_| {}).unwrap();
    let i = p.model.index[&quest];
    let here = &p.model.quests[i].starts[0];
    p.model.initial.pos = Some((here.pos, here.zone));
    let planner = Planner::new(&p.model, &p.world, &p.params, &p.profile)
        .replaying()
        .explicit();
    let edited = planner.edit(&[Edit::Stop {
        stop: Stop::quest(i as u32, Kind::Accept),
        farm: false,
        spot: None,
    }]);
    assert_eq!(
        edited.steps[0].outcome == Outcome::Done,
        expected,
        "{race} {class}: quest {quest} needs its eligible predecessor"
    );
}

/// A case of `alternatives` in each edition (Forever's Windborne Cat quest in Forever only), with
/// the quest links as built and with the faction branch written as `pre_any`.
macro_rules! branch {
    ($name:ident, $edition:expr, $refreshed:expr, $quest:expr, $race:expr, $class:expr, $level:expr, $completed:expr, $expected:expr) => {
        #[test]
        fn $name() {
            let who = Who {
                quest: $quest,
                race: $race,
                class: $class,
                level: $level,
                completed: $completed,
            };
            alternatives($edition, $refreshed, &who, $expected);
        }
    };
}

branch!(
    classic_built_warlock_branch_missing,
    Edition::Classic,
    false,
    794,
    "orc",
    "warrior",
    3,
    None,
    false
);
branch!(
    classic_built_warlock_branch_done,
    Edition::Classic,
    false,
    794,
    "orc",
    "warrior",
    3,
    Some(&[792]),
    true
);
branch!(
    classic_refreshed_warlock_branch_missing,
    Edition::Classic,
    true,
    794,
    "orc",
    "warrior",
    3,
    None,
    false
);
branch!(
    classic_refreshed_warlock_branch_done,
    Edition::Classic,
    true,
    794,
    "orc",
    "warrior",
    3,
    Some(&[792]),
    true
);
branch!(
    tbc_built_warlock_branch_missing,
    Edition::Tbc,
    false,
    794,
    "orc",
    "warrior",
    3,
    None,
    false
);
branch!(
    tbc_built_warlock_branch_done,
    Edition::Tbc,
    false,
    794,
    "orc",
    "warrior",
    3,
    Some(&[792]),
    true
);
branch!(
    tbc_refreshed_warlock_branch_missing,
    Edition::Tbc,
    true,
    794,
    "orc",
    "warrior",
    3,
    None,
    false
);
branch!(
    tbc_refreshed_warlock_branch_done,
    Edition::Tbc,
    true,
    794,
    "orc",
    "warrior",
    3,
    Some(&[792]),
    true
);
branch!(
    forever_built_warlock_branch_missing,
    Edition::Forever,
    false,
    794,
    "orc",
    "warrior",
    3,
    None,
    false
);
branch!(
    forever_built_warlock_branch_done,
    Edition::Forever,
    false,
    794,
    "orc",
    "warrior",
    3,
    Some(&[792]),
    true
);
branch!(
    forever_built_orc_cat_done,
    Edition::Forever,
    false,
    79192,
    "orc",
    "warrior",
    20,
    Some(&[79007]),
    true
);
branch!(
    forever_built_human_cat_done,
    Edition::Forever,
    false,
    79192,
    "human",
    "warrior",
    20,
    Some(&[79008]),
    true
);
branch!(
    forever_built_orc_cat_none_done,
    Edition::Forever,
    false,
    79192,
    "orc",
    "warrior",
    20,
    Some(&[]),
    false
);
branch!(
    forever_built_orc_cat_unknown,
    Edition::Forever,
    false,
    79192,
    "orc",
    "warrior",
    20,
    None,
    false
);
branch!(
    forever_built_human_cat_unknown,
    Edition::Forever,
    false,
    79192,
    "human",
    "warrior",
    20,
    None,
    false
);
branch!(
    forever_refreshed_warlock_branch_missing,
    Edition::Forever,
    true,
    794,
    "orc",
    "warrior",
    3,
    None,
    false
);
branch!(
    forever_refreshed_warlock_branch_done,
    Edition::Forever,
    true,
    794,
    "orc",
    "warrior",
    3,
    Some(&[792]),
    true
);
branch!(
    forever_refreshed_orc_cat_done,
    Edition::Forever,
    true,
    79192,
    "orc",
    "warrior",
    20,
    Some(&[79007]),
    true
);
branch!(
    forever_refreshed_human_cat_done,
    Edition::Forever,
    true,
    79192,
    "human",
    "warrior",
    20,
    Some(&[79008]),
    true
);
branch!(
    forever_refreshed_orc_cat_none_done,
    Edition::Forever,
    true,
    79192,
    "orc",
    "warrior",
    20,
    Some(&[]),
    false
);
branch!(
    forever_refreshed_orc_cat_unknown,
    Edition::Forever,
    true,
    79192,
    "orc",
    "warrior",
    20,
    None,
    false
);
branch!(
    forever_refreshed_human_cat_unknown,
    Edition::Forever,
    true,
    79192,
    "human",
    "warrior",
    20,
    None,
    false
);
