//! Class presets use the selected edition's quest and spell data.

use crate::common::{database_of, repo};
use fg_route::job::{self, Overrides, PlanRequest};
use fg_route::plan::Planner;
use fg_route::xp::Edition;

#[test]
fn native_class_presets_generate_guides_in_each_edition() {
    for edition in Edition::ALL {
        let conn = database_of(edition.key());
        let overrides = Overrides::load_edition(&repo().join("overrides"), edition).unwrap();
        let mut characters = vec![
            ("human", "warrior"),
            ("human", "paladin"),
            ("orc", "hunter"),
            ("human", "rogue"),
            ("human", "priest"),
            ("orc", "shaman"),
            ("human", "mage"),
            ("human", "warlock"),
            ("nightelf", "druid"),
        ];
        match edition {
            Edition::Tbc => characters.extend([("bloodelf", "paladin"), ("draenei", "shaman")]),
            Edition::Forever => characters.extend([("skyborne", "druid"), ("skyborne_horde", "druid")]),
            Edition::Classic => {}
        }
        for (race, class) in characters {
            let request: PlanRequest = serde_json::from_value(serde_json::json!({
                "race":race,"class":class,"from_level":1,"to_level":5
            }))
            .unwrap();
            let p = job::prepare(&conn, &overrides, &request, &|_| {}).unwrap();
            let planner = Planner::new(&p.model, &p.world, &p.params, &p.profile);
            let route = planner.construct(|_| {});
            assert!(planner.simulate(&route, None).is_some(), "the generated route replays");
            let guide = fg_route::export::build(&planner, &p.profile, &route);
            let level = guide.steps.iter().map(|s| s.level).max().unwrap_or(0);
            assert!(level >= 5, "{edition:?} {race} {class}: reaches the target");
            assert!(guide.quests > 0, "{edition:?} {race} {class}: follows quests");
        }
    }
}
