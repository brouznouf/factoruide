//! What the integration tests share: the repository's place and the editions' databases.

use fg_route::testing::{Q, Scenario};
use std::path::PathBuf;

pub fn repo() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../..")
}

/// Quests in three zones side by side (a valley, a plain, a far barrens); a trainer in the valley;
/// Orgrimmar's flight path (known) in the plain and the Crossroads' (not) in the barrens.
pub fn leveling(to_level: i64) -> Scenario {
    leveling_of(to_level, 30)
}

/// `leveling` with only its first `quests` quests: a search over fewer stops, for the tests of
/// what a search does rather than of the world.
pub fn leveling_of(to_level: i64, quests: i64) -> Scenario {
    let mut s = Scenario::new()
        .levels(1, to_level)
        .zone(2, "Durotar")
        .zone(3, "The Barrens")
        .trainer(1, 50.0, 50.0)
        .vendor(1, 52.0, 52.0)
        .inn("Valley Inn", 1, 54.0, 54.0)
        .flight_master(23, "Orgrimmar", 2, 50.0, 50.0)
        .flight_master(25, "Crossroads", 3, 50.0, 50.0)
        .knows_flight(23);
    for i in 0..quests {
        let (zone, x) = match i {
            0..=5 => (1, 20.0 + 10.0 * i as f64),
            6..=11 => (2, 20.0 + 10.0 * (i - 6) as f64),
            _ => (3, 20.0 + 5.0 * (i - 12) as f64),
        };
        let level = 1 + i;
        // Mobs of the quest's level on the way between its giver and its objective, a camp of
        // each level to grind at.
        s = s.mobs(zone, x, 50.0, level, 3).mobs(1, 90.0, 90.0, level, 10);
        s = s.quest(
            Q::new(100 + i, &format!("Quest {i}"))
                .level(level)
                .xp(150 * level)
                .giver(zone, x, 30.0)
                .kill("Beast", level, 6, zone, x, 70.0),
        );
    }
    s
}

/// The walking world of an edition for a faction on a database without terrain: its zones, passes
/// and capitals (the game's constants of the base), for the tests that do not walk.
pub fn bare_world(edition: fg_route::xp::Edition, faction: fg_route::faction::Faction) -> fg_route::world::World {
    let overrides = fg_route::job::Overrides::load_edition(&repo().join("overrides"), edition).unwrap();
    fg_route::job::game_world(&fg_route::fixture::Fixture::new(edition).open(), &overrides, faction, 0).unwrap()
}

/// The walking world of an edition for a faction on the terrain of fixtures' files (their
/// `[[terrain]]` regions), built for each test.
pub fn terrain_world(
    edition: fg_route::xp::Edition,
    faction: fg_route::faction::Faction,
    files: &[&str],
) -> fg_route::world::World {
    let overrides = fg_route::job::Overrides::load_edition(&repo().join("overrides"), edition).unwrap();
    let conn = files
        .iter()
        .fold(fg_route::fixture::Fixture::new(edition), |f, n| f.file(n))
        .open();
    fg_route::job::game_world(&conn, &overrides, faction, 0).unwrap()
}
