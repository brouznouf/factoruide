//! Characters' profiles the addon saves in its SavedVariables (`FactoruideDB.profiles`, see
//! the addon's Profile.lua): level, quests done and in the log, position, flight paths... for
//! the app to plan a guide from where a character stands. Never part of a contribution.

use crate::lua::{self, Val};
use anyhow::Result;
use std::collections::BTreeSet;
use std::path::PathBuf;

/// C_TaxiMap state of a flight path the character does not know.
const UNKNOWN_FLIGHT: i64 = 2;

/// The profiles of SavedVariables files (see `collector::find_saved_variables`), as JSON objects
/// (the planner's `StartState`), most recent first. Flight paths also come from the flight maps
/// the collector recorded before the addon kept them in the profile.
pub fn read(files: &[PathBuf]) -> Result<Vec<serde_json::Value>> {
    let mut out = Vec::new();
    for file in files {
        if !file.ends_with("Factoruide.lua") {
            continue;
        }
        let lua = lua::new_lua();
        let Some(db) = lua::load_globals(&lua, file, &["FactoruideDB"])?.pop() else {
            continue;
        };
        for (key, p) in db.field("profiles").str_entries() {
            if p.field("level").int().is_none() {
                continue;
            }
            let mut json = p.to_json();
            let mut flights: BTreeSet<i64> = p
                .field("flights")
                .entries()
                .into_iter()
                .filter(|(_, v)| !matches!(v, Val::Nil | Val::Bool(false)))
                .map(|(id, _)| id)
                .collect();
            flights.extend(recorded_flights(db.field("characters").field(key)));
            json["flights"] = flights.into_iter().collect();
            if json["name"].as_str().is_none_or(str::is_empty) {
                json["name"] = key.into();
            }
            out.push(json);
        }
    }
    out.sort_by_key(|p| std::cmp::Reverse(p["time"].as_i64().unwrap_or(0)));
    Ok(out)
}

/// Flight paths shown known on the flight maps the collector recorded (`taxi` events).
fn recorded_flights(character: &Val) -> Vec<i64> {
    character
        .field("events")
        .list()
        .into_iter()
        .filter(|e| e.get(3).str() == Some("taxi"))
        .flat_map(|e| e.get(11).field("nodes").list())
        .filter(|n| n.field("state").int().unwrap_or(UNKNOWN_FLIGHT) != UNKNOWN_FLIGHT)
        .filter_map(|n| n.field("id").int())
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn profiles_with_flights_from_the_profile_and_the_flight_maps() {
        let dir = std::env::temp_dir().join(format!("fg-profile-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let file = dir.join("Factoruide.lua");
        std::fs::write(
            &file,
            r#"FactoruideDB = {
                ["characters"] = { ["Brouz-Realm"] = { ["seq"] = 2, ["events"] = {
                    { 1, 100, "taxi", false, false, false, 1411, 50, 50, 12,
                      { ["map"] = 1411, ["nodes"] = { { ["id"] = 25, ["state"] = 0 }, { ["id"] = 22, ["state"] = 2 } } } },
                    { 2, 101, "accept", 788 },
                } } },
                ["profiles"] = {
                    ["Brouz-Realm"] = { ["level"] = 12, ["xp"] = 3400, ["time"] = 200,
                        ["completed"] = { 4641, 788 }, ["log"] = {}, ["flights"] = { [23] = true } },
                    ["Old-Realm"] = { ["level"] = 5, ["time"] = 300 },
                    ["Empty-Realm"] = {},
                },
            }"#,
        )
        .unwrap();
        let profiles = read(&[file]).unwrap();
        std::fs::remove_dir_all(&dir).unwrap();
        let names: Vec<&str> = profiles.iter().map(|p| p["name"].as_str().unwrap()).collect();
        assert_eq!(names, ["Old-Realm", "Brouz-Realm"], "recorded ones, most recent first");
        let brouz = &profiles[1];
        assert_eq!(brouz["flights"], serde_json::json!([23, 25]));
        assert_eq!(brouz["completed"], serde_json::json!([4641, 788]));
        assert_eq!(brouz["log"], serde_json::json!([]));
    }
}
