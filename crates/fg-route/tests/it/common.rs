//! What the integration tests share: the repository's place and the editions' databases.

use fg_route::testing::{Q, Scenario};
use rusqlite::Connection;
use std::path::PathBuf;
use std::sync::Mutex;

pub fn repo() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../..")
}

/// The Classic database, unpacked next to the packed one when missing or older.
pub fn database() -> Connection {
    database_of("classic")
}

/// An edition's database (classic, tbc, forever), unpacked the same way.
pub fn database_of(edition: &str) -> Connection {
    static UNPACK: Mutex<()> = Mutex::new(());
    let _guard = UNPACK.lock().unwrap();
    let (path, packed) = (
        repo().join(format!("data/{edition}.sqlite")),
        repo().join(format!("data/{edition}.sqlite.zst")),
    );
    let modified = |p: &PathBuf| std::fs::metadata(p).and_then(|m| m.modified()).ok();
    if !path.exists() || modified(&packed) > modified(&path) {
        let data = zstd::decode_all(std::fs::File::open(&packed).unwrap()).unwrap();
        std::fs::write(&path, data).unwrap();
    }
    Connection::open(path).unwrap()
}

/// Quests in three zones side by side (a valley, a plain, a far barrens); a trainer in the valley;
/// Orgrimmar's flight path (known) in the plain and the Crossroads' (not) in the barrens.
pub fn leveling(to_level: i64) -> Scenario {
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
    for i in 0..24_i64 {
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
