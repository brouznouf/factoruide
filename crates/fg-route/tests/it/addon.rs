//! What the addon gets besides the guides' steps, on the Classic database: the walks between
//! steps (for the compass) and the places that serve the player.

use crate::common::repo;
use fg_route::Route;
use fg_route::faction::Faction;
use fg_route::fixture::Fixture;
use fg_route::job::{self, Overrides, Service};
use fg_route::model::Names;
use fg_route::plan::Breakdown;
use fg_route::world::World;
use fg_route::xp::Edition;
use serde_json::json;

fn overrides() -> Overrides {
    Overrides::load_edition(&repo().join("overrides"), Edition::Classic).unwrap()
}

/// The Classic world of a faction on the ground of Tirisfal Glades.
fn world(faction: Faction) -> World {
    crate::common::terrain_world(Edition::Classic, faction, &["classic_tirisfal_terrain"])
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
    let world = &world(Faction::Horde);
    let mut r = route(&[("accept", 30.23, 71.61), ("accept", 30.87, 66.2)]);
    job::add_walk_paths(world, &mut r);
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
    let world = &world(Faction::Horde);
    let mut r = route(&[
        ("accept", 30.23, 71.61),
        ("link", 30.23, 71.61),
        ("accept", 30.87, 66.2),
    ]);
    job::add_walk_paths(world, &mut r);
    assert!(r.steps[2].path.is_empty(), "after the boat");
    let mut r = route(&[
        ("accept", 30.23, 71.61),
        ("objective", 30.87, 66.2),
        ("accept", 30.87, 66.2),
    ]);
    r.steps[1].bg = true;
    job::add_walk_paths(world, &mut r);
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
    let conn = Fixture::new(Edition::Classic).file("classic_places").open();
    let world = &crate::common::bare_world(Edition::Classic, Faction::Alliance);
    let places = job::places(&conn, world, Edition::Classic, &Names::load(&conn, "frFR").unwrap()).unwrap();
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
    let conn = Fixture::new(Edition::Classic).open();
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

/// A long walk is drawn as it goes, not as lines from one tile gate to the next: the points of
/// the walk give the floor by floor path's length (Ziz Fizziks in Stonetalon to Ratchet).
#[test]
fn a_long_walk_follows_the_ground_between_gates() {
    let world = &crate::common::terrain_world(
        Edition::Classic,
        Faction::Alliance,
        &["classic_stonetalon_barrens_terrain"],
    );
    let a = world
        .to_world(world.zone_of_ui_map(1442).unwrap(), 58.99, 62.6)
        .unwrap();
    let b = world
        .to_world(world.zone_of_ui_map(1413).unwrap(), 62.98, 37.22)
        .unwrap();
    let len = |pts: &[fg_route::world::Pos]| pts.windows(2).map(|w| w[0].dist(&w[1])).sum::<f64>();
    let walk = world.walk_path(&a, &b, 28);
    let cells: Vec<_> = world.terrain.cell_path(&a, &b).into_iter().map(|(p, _)| p).collect();
    assert!(cells.len() > 1);
    assert!(
        len(&walk) >= len(&cells) * 0.97,
        "the walk cuts corners: {:.0} yd against {:.0}",
        len(&walk),
        len(&cells)
    );
}

/// The flight path of a town counts as learned once quests were done there; no quest done, none.
#[test]
fn quests_done_in_a_town_give_its_flight_path() {
    let conn = Fixture::new(Edition::Classic).file("classic_stormwind_quests").open();
    let in_stormwind: Vec<job::QuestId> = conn
        .prepare(
            "SELECT DISTINCT r.quest_id FROM m_quest_relation r JOIN m_spawn s
             ON s.entity_type = r.entity_type AND s.entity_id = r.entity_id
             WHERE s.zone_id = 1519 AND r.role = 'start' LIMIT 6",
        )
        .unwrap()
        .query_map([], |r| r.get::<_, i64>(0).map(job::QuestId::from))
        .unwrap()
        .collect::<Result<_, _>>()
        .unwrap();
    assert!(in_stormwind.len() >= 2, "{in_stormwind:?}");
    let stormwind: i64 = conn
        .query_row(
            "SELECT ID FROM client_taxinodes WHERE Name_lang LIKE 'Stormwind%'",
            [],
            |r| r.get(0),
        )
        .unwrap();
    let found = job::inferred_flights(&conn, Faction::Alliance, &in_stormwind).unwrap();
    assert!(found.contains(&stormwind), "{found:?}");
    assert_eq!(
        job::inferred_flights(&conn, Faction::Alliance, &[]).unwrap(),
        Vec::<i64>::new()
    );
}

/// The capital cities are not starts to choose: every route passes through them.
#[test]
fn capitals_are_known_from_the_area_flags() {
    let world = &crate::common::bare_world(Edition::Classic, Faction::Alliance);
    for capital in [1519, 1537, 1657, 1637, 1497, 1638] {
        assert!(world.is_capital(capital), "{}", world.zone_name(capital));
    }
    for zone in [12, 10, 331, 406] {
        assert!(!world.is_capital(zone), "{}", world.zone_name(zone));
    }
}
