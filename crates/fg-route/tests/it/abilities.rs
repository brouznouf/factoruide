//! Class presets use the selected edition's quest and spell data: a short guide (effort 0) is
//! generated on the real data of each edition, for characters that differ in what the data holds
//! (the classes' spells, the editions' races).

use crate::common::{database_of, repo};
use fg_route::job::{self, Overrides, PlanRequest};
use fg_route::plan::Planner;
use fg_route::xp::Edition;

fn generates_a_guide(edition: Edition, race: &str, class: &str) {
    let conn = database_of(edition.key());
    let overrides = Overrides::load_edition(&repo().join("overrides"), edition).unwrap();
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

macro_rules! preset {
    ($name:ident, $edition:expr, $race:expr, $class:expr) => {
        #[test]
        fn $name() {
            generates_a_guide($edition, $race, $class);
        }
    };
}

preset!(classic_human_warrior, Edition::Classic, "human", "warrior");
preset!(classic_orc_shaman, Edition::Classic, "orc", "shaman");
preset!(tbc_bloodelf_paladin, Edition::Tbc, "bloodelf", "paladin");
preset!(tbc_draenei_shaman, Edition::Tbc, "draenei", "shaman");
preset!(forever_skyborne_druid, Edition::Forever, "skyborne", "druid");
preset!(
    forever_skyborne_horde_druid,
    Edition::Forever,
    "skyborne_horde",
    "druid"
);
