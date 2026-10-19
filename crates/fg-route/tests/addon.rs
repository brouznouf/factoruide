//! What the addon gets besides the guides' steps, on the Classic database: the walks between
//! steps (for the compass) and the places that serve the player.

use fg_route::Route;
use fg_route::faction::Faction;
use fg_route::job::{self, Overrides, Service};
use fg_route::model::Names;
use fg_route::plan::Breakdown;
use fg_route::xp::Edition;
use rusqlite::Connection;
use serde_json::json;
use std::path::PathBuf;
use std::sync::Mutex;

fn repo() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../..")
}

/// The Classic database, unpacked next to the packed one when missing or older.
fn database() -> Connection {
    static UNPACK: Mutex<()> = Mutex::new(());
    let _guard = UNPACK.lock().unwrap();
    let (path, packed) = (
        repo().join("data/classic.sqlite"),
        repo().join("data/classic.sqlite.zst"),
    );
    let modified = |p: &PathBuf| std::fs::metadata(p).and_then(|m| m.modified()).ok();
    if !path.exists() || modified(&packed) > modified(&path) {
        let data = zstd::decode_all(std::fs::File::open(&packed).unwrap()).unwrap();
        std::fs::write(&path, data).unwrap();
    }
    Connection::open(path).unwrap()
}

fn overrides() -> Overrides {
    Overrides::load_edition(&repo().join("overrides"), Edition::Classic).unwrap()
}

/// A guide of these steps (`kind`, Tirisfal Glades map percent), as guides made before the
/// steps had their world position.
fn route(steps: &[(&str, f64, f64)]) -> Route {
    let steps: Vec<_> = (steps.iter())
        .map(|(kind, x, y)| json!({ "kind": kind, "map": { "ui_map": 1420, "x": x, "y": y }, "level": 2, "time": 0.0 }))
        .collect();
    serde_json::from_value(json!({
        "name": "walks", "race_id": 5, "class_id": 1, "class": "Warrior", "faction": "Horde",
        "from_level": 1, "to_level": 10, "total_time": 0.0, "total_time_text": "", "quests": 0,
        "breakdown": serde_json::to_value(Breakdown::default()).unwrap(), "steps": steps,
    }))
    .unwrap()
}

fn length(points: &[[f64; 2]]) -> f64 {
    points
        .windows(2)
        .map(|w| (w[1][0] - w[0][0]).hypot(w[1][1] - w[0][1]))
        .sum()
}

/// Deathknell to the slope north of it: the walk goes around the steep ground. It starts where
/// the step before is, and is longer than the straight line.
#[test]
fn a_walk_around_the_steep_ground_starts_at_the_step_before() {
    let conn = database();
    let world = job::walk_world(&conn, &overrides(), Faction::Horde).unwrap();
    let mut r = route(&[("accept", 30.23, 71.61), ("accept", 30.87, 66.2)]);
    job::add_walk_paths(&world, &mut r);
    assert!(r.steps[0].path.is_empty(), "nothing before the first step");
    let path = &r.steps[1].path;
    assert!(path.len() >= 2, "the walk turns: {path:?}");
    let start = world.to_world(85, 30.23, 71.61).unwrap();
    let end = world.to_world(85, 30.87, 66.2).unwrap();
    assert!((path[0][0] - start.x).abs() < 1.0 && (path[0][1] - start.y).abs() < 1.0);
    let mut walked = path.clone();
    walked.push([end.x, end.y]);
    assert!(length(&walked) > start.dist(&end) + 5.0, "around, not straight");
}

/// No walk after a boat (the character lands elsewhere), nor to a step done along the way.
#[test]
fn no_walk_after_a_boat_nor_to_a_step_along_the_way() {
    let conn = database();
    let world = job::walk_world(&conn, &overrides(), Faction::Horde).unwrap();
    let mut r = route(&[
        ("accept", 30.23, 71.61),
        ("link", 30.23, 71.61),
        ("accept", 30.87, 66.2),
    ]);
    job::add_walk_paths(&world, &mut r);
    assert!(r.steps[2].path.is_empty(), "after the boat");
    let mut r = route(&[
        ("accept", 30.23, 71.61),
        ("objective", 30.87, 66.2),
        ("accept", 30.87, 66.2),
    ]);
    r.steps[1].bg = true;
    job::add_walk_paths(&world, &mut r);
    assert!(r.steps[1].path.is_empty(), "to the step along the way");
    assert!(
        !r.steps[2].path.is_empty(),
        "the next walk starts at the step before it, not along the way"
    );
}

/// The places of Coldridge Valley: its mage trainer, innkeeper and the Ironforge flight master,
/// named in the guide's language; the NPCs left unused by the game's makers are not.
#[test]
fn places_of_services() {
    let conn = database();
    let world = job::walk_world(&conn, &overrides(), Faction::Alliance).unwrap();
    let places = job::places(&conn, &world, Edition::Classic, &Names::load(&conn, "frFR").unwrap()).unwrap();
    let find = |name: &str| places.iter().filter(|p| p.name == name).collect::<Vec<_>>();
    let mage = find("Marryk Nurribit");
    assert!(
        mage.iter()
            .any(|p| p.service == Service::Class && p.of == 8 && p.factions == "A" && p.npc == 944)
    );
    assert!(find("Aubergiste Farley").iter().any(|p| p.service == Service::Inn));
    assert!(find("Thor").iter().any(|p| p.service == Service::Flight));
    assert!(places.iter().any(|p| p.service == Service::Mail && p.factions == "AH"));
    assert!(places.iter().all(|p| !p.name.contains("UNUSED")));
    // One place per NPC standing somewhere.
    assert_eq!(mage.iter().filter(|p| p.service == Service::Class).count(), 1);
    let lua = job::services_lua(&places);
    assert!(lua.contains(r#"{ "class", "A", 8, 0, "#) && lua.contains(r#""Marryk Nurribit", 944 }"#));
}

/// Each guide carries the format of the guides written today, which the addon checks against
/// the newest it follows (Import.lua): a guide made for a newer addon asks to update it.
#[test]
fn each_guide_carries_its_format() {
    let import = std::fs::read_to_string(repo().join("addon/Factoruide_Guide/Import.lua")).unwrap();
    let addon_format: u32 = import
        .lines()
        .find_map(|line| line.strip_prefix("Import.FORMAT = "))
        .unwrap()
        .parse()
        .unwrap();
    assert_eq!(
        job::GUIDE_FORMAT,
        addon_format,
        "app exports must match the addon's supported format"
    );
    let conn = database();
    let mut other = route(&[("accept", 30.87, 66.2)]);
    other.name = "other".into();
    let lua = job::guides_lua(
        &conn,
        &overrides(),
        Edition::Classic,
        vec![(route(&[("accept", 30.23, 71.61)]), 1000, 3), (other, 1100, 1)],
    )
    .unwrap();
    for name in ["walks", "other"] {
        let line = format!("FG.Routes[\"{name}\"].format = {}\n", job::GUIDE_FORMAT);
        assert_eq!(lua.matches(&line).count(), 1, "{name}: its format once");
    }
}
