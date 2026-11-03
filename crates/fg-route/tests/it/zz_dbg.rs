use crate::common::repo;
use fg_route::fixture::Fixture;
use fg_route::job::{self, Overrides, PlanRequest};
use fg_route::xp::Edition;
#[test]
fn zz_dbg() {
    let (ed, files, race, class, to) = (std::env::var("ED").unwrap(), std::env::var("FX").unwrap(), std::env::var("RACE").unwrap(), std::env::var("CLASS").unwrap(), std::env::var("TO").unwrap());
    let edition = match ed.as_str() { "tbc" => Edition::Tbc, "forever" => Edition::Forever, _ => Edition::Classic };
    let conn = files.split(',').fold(Fixture::new(edition), |f, n| f.file(n)).open();
    let o = Overrides::load_edition(&repo().join("overrides"), edition).unwrap();
    let req: PlanRequest = serde_json::from_value(serde_json::json!({"race":race,"class":class,"from_level":1,"to_level":to.parse::<i64>().unwrap()})).unwrap();
    let p = job::prepare(&conn, &o, &req, &|_| {}).unwrap();
    for n in &p.notes { eprintln!("N {n}"); }
    eprintln!("quests {}", p.model.quests.len());
}
