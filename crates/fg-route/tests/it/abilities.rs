//! Class presets use the selected edition's quest and spell data: a guide is generated for each
//! class in each edition (a test each, so that they run side by side).

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
preset!(classic_human_paladin, Edition::Classic, "human", "paladin");
preset!(classic_orc_hunter, Edition::Classic, "orc", "hunter");
preset!(classic_human_rogue, Edition::Classic, "human", "rogue");
preset!(classic_human_priest, Edition::Classic, "human", "priest");
preset!(classic_orc_shaman, Edition::Classic, "orc", "shaman");
preset!(classic_human_mage, Edition::Classic, "human", "mage");
preset!(classic_human_warlock, Edition::Classic, "human", "warlock");
preset!(classic_nightelf_druid, Edition::Classic, "nightelf", "druid");
preset!(tbc_human_warrior, Edition::Tbc, "human", "warrior");
preset!(tbc_human_paladin, Edition::Tbc, "human", "paladin");
preset!(tbc_orc_hunter, Edition::Tbc, "orc", "hunter");
preset!(tbc_human_rogue, Edition::Tbc, "human", "rogue");
preset!(tbc_human_priest, Edition::Tbc, "human", "priest");
preset!(tbc_orc_shaman, Edition::Tbc, "orc", "shaman");
preset!(tbc_human_mage, Edition::Tbc, "human", "mage");
preset!(tbc_human_warlock, Edition::Tbc, "human", "warlock");
preset!(tbc_nightelf_druid, Edition::Tbc, "nightelf", "druid");
preset!(tbc_bloodelf_paladin, Edition::Tbc, "bloodelf", "paladin");
preset!(tbc_draenei_shaman, Edition::Tbc, "draenei", "shaman");
preset!(forever_human_warrior, Edition::Forever, "human", "warrior");
preset!(forever_human_paladin, Edition::Forever, "human", "paladin");
preset!(forever_orc_hunter, Edition::Forever, "orc", "hunter");
preset!(forever_human_rogue, Edition::Forever, "human", "rogue");
preset!(forever_human_priest, Edition::Forever, "human", "priest");
preset!(forever_orc_shaman, Edition::Forever, "orc", "shaman");
preset!(forever_human_mage, Edition::Forever, "human", "mage");
preset!(forever_human_warlock, Edition::Forever, "human", "warlock");
preset!(forever_nightelf_druid, Edition::Forever, "nightelf", "druid");
preset!(forever_skyborne_druid, Edition::Forever, "skyborne", "druid");
preset!(
    forever_skyborne_horde_druid,
    Edition::Forever,
    "skyborne_horde",
    "druid"
);
