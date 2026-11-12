//! A guide as the source Polestar's own tools read (a `.guide` file of a pack: one line per goal,
//! its guide format v1), for players who work on it with them (its route planner, its
//! playground, its compiler). The same steps and goals as the guide pack (`super::plan`), in
//! English: Polestar's tools fill in the rest (prerequisites, places of the mobs, heights, groups)
//! from their own data, and show the names in each client's language.

use super::super::types::Route;
use super::{Goal, PACK_ID, plan};
use std::fmt::Write as _;

/// The `pack.json` of a pack holding these guides (`fg polestar --source`): their guide ids
/// start with its id.
pub fn pack_json(version: &str) -> String {
    serde_json::to_string_pretty(&serde_json::json!({
        "id": PACK_ID,
        "title": "Factoruide",
        "author": "Factoruide",
        "version": version,
        "license": "MIT",
        "website": "https://github.com/brouznouf/factoruide",
        "source": "Guides made in the Factoruide app",
        "folder": super::ADDON,
    }))
    .unwrap_or_default()
        + "\n"
}

/// A guide id part as Polestar takes it: lower-case letters, digits, `_` and `-`.
pub fn guide_name(id: &str) -> String {
    let out: String = id
        .chars()
        .map(|c| c.to_ascii_lowercase())
        .map(|c| {
            if c.is_ascii_alphanumeric() || c == '_' || c == '-' {
                c
            } else {
                '-'
            }
        })
        .collect();
    let out = out.trim_matches('-');
    if out.is_empty() { "guide".into() } else { out.to_owned() }
}

/// The guide's source (`id`: the part after the pack's id), for a guide written in English.
pub fn source(id: &str, route: &Route) -> String {
    let name = guide_name(id);
    let steps = plan(route);
    let mut out = String::new();
    let _ = writeln!(out, "# {}: a guide made in the Factoruide app.", one_line(&route.name));
    let _ = writeln!(
        out,
        "# For Polestar's guide tools: save it as {name}.guide in a Polestar pack whose pack.json has the id \"{PACK_ID}\"."
    );
    let _ = writeln!(out, "guide {PACK_ID}/{name}");
    let _ = writeln!(out, "title {}", quote(&route.name));
    let _ = writeln!(out, "faction {}", route.faction.name());
    if let Some(race) = super::race_file(route.race_id) {
        let _ = writeln!(out, "race {race}");
    }
    if let Some(class) = super::class_file(route.class_id) {
        let mut chars = class.chars();
        let first = chars.next().map(|c| c.to_string()).unwrap_or_default();
        let _ = writeln!(out, "class {first}{}", chars.as_str().to_lowercase());
    }
    let from = route.from_level.max(1);
    let _ = writeln!(out, "levels {from}-{}", route.to_level.max(from));
    // Coordinates in the maps the game shows (not those of the first version of the game).
    let mut zone = steps
        .iter()
        .flat_map(|(_, goals)| goals)
        .find_map(|g| g.get("map"))
        .unwrap_or("1")
        .to_owned();
    let _ = writeln!(out, "zone map {zone}\ncoords live");
    for (title, goals) in &steps {
        out.push_str("\nstep");
        if let Some(title) = title.as_deref().filter(|t| !t.trim().is_empty()) {
            let _ = write!(out, " {}", quote(title));
        }
        out.push('\n');
        // The step's zone: its first goal's map.
        if let Some(map) = goals.iter().find_map(|g| g.get("map"))
            && map != zone
        {
            map.clone_into(&mut zone);
            let _ = writeln!(out, "  zone map {zone}");
        }
        for goal in goals {
            let _ = writeln!(out, "  {}", line(goal, &zone));
        }
    }
    out
}

/// A text on one line.
pub(super) fn one_line(text: &str) -> String {
    text.split_whitespace().collect::<Vec<_>>().join(" ")
}

/// A quoted string (`\"` and `\\` escaped), on one line.
fn quote(text: &str) -> String {
    let text = one_line(text);
    let text = if text.is_empty() { "?".to_owned() } else { text };
    format!("\"{}\"", text.replace('\\', "\\\\").replace('"', "\\\""))
}

/// A goal's line: its verb, what it is about, then its modifiers (`zone`: the map in effect).
fn line(goal: &Goal, zone: &str) -> String {
    let get = |key: &str| goal.get(key).filter(|v| !v.trim().is_empty());
    let mut words: Vec<String> = vec![goal.verb.to_owned()];
    let mut add = |word: String| words.push(word);
    // Its place (`goto`: the coordinates are the goal), on another map than the step's named.
    let place = |words: &mut Vec<String>, at: bool| {
        if let (Some(map), Some(x), Some(y)) = (get("map"), get("x"), get("y")) {
            words.push(if at { format!("@{x},{y}") } else { format!("{x},{y}") });
            if map != zone {
                words.push(format!("map {map}"));
            }
        }
    };
    // The quest objective it counts.
    let quest = |words: &mut Vec<String>| {
        if let Some(q) = get("quest") {
            words.push(match get("oi") {
                Some(oi) => format!("quest {q}.{oi}"),
                None => format!("quest {q}"),
            });
        }
    };
    let count = |words: &mut Vec<String>| {
        if let Some(n) = get("count") {
            words.push(format!("x{n}"));
        }
    };
    match goal.verb {
        "accept" | "turnin" | "complete" => {
            add(get("quest").unwrap_or("0").to_owned());
            if let Some(title) = get("title") {
                add(quote(title));
            }
            for kind in ["npc", "object", "item"] {
                // A quest an item starts: taken from the bags, handed in to someone.
                if let Some(id) = get(kind).filter(|_| goal.verb == "accept" || kind != "item") {
                    add(format!("{kind} {id}"));
                }
            }
            if let Some(level) = get("minLevel").filter(|l| l.parse::<i64>().is_ok_and(|l| l > 0)) {
                add(format!("lvl {level}"));
            }
            place(&mut words, true);
            if get("direct").is_some() {
                words.push("direct".into());
            }
        }
        "kill" => {
            match get("npc") {
                Some(npc) => {
                    add(npc.to_owned());
                    if let Some(target) = get("target") {
                        add(quote(target));
                    }
                }
                None => add(quote(get("target").unwrap_or("?"))),
            }
            count(&mut words);
            quest(&mut words);
            place(&mut words, true);
        }
        "collect" | "buy" => {
            add(get("item").unwrap_or("0").to_owned());
            if let Some(name) = get("itemName") {
                add(quote(name));
            }
            count(&mut words);
            if goal.verb == "collect" {
                quest(&mut words);
            }
            place(&mut words, true);
        }
        "interact" => {
            add(get("object").unwrap_or("0").to_owned());
            if let Some(name) = get("objName") {
                add(quote(name));
            }
            quest(&mut words);
            place(&mut words, true);
        }
        "talk" | "train" => {
            add(get("npc").unwrap_or("0").to_owned());
            if let Some(name) = get("npcName") {
                add(quote(name));
            }
            place(&mut words, true);
        }
        "getfp" => {
            add(quote(get("location").unwrap_or("?")));
            place(&mut words, true);
        }
        "hearth" | "fly" => add(quote(get("location").unwrap_or("?"))),
        "goto" => {
            place(&mut words, false);
            if let Some(text) = get("text") {
                words.push(quote(text));
            }
        }
        "level" => add(get("level").unwrap_or("1").to_owned()),
        _ => add(quote(get("text").unwrap_or("?"))),
    }
    if get("optional").is_some() && goal.verb != "note" {
        words.push("optional".into());
    }
    words.join(" ")
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn route(steps: serde_json::Value) -> Route {
        let mut steps = steps;
        for s in steps.as_array_mut().unwrap() {
            if s.get("level").is_none() {
                s["level"] = json!(10);
            }
            s["time"] = json!(0.0);
        }
        serde_json::from_value(json!({
            "name": "My \"dwarf\" warrior", "race_id": 3, "class_id": 1, "class": "Warrior",
            "faction": "Alliance", "from_level": 1, "to_level": 60, "total_time": 0.0,
            "total_time_text": "", "quests": 0, "breakdown": crate::plan::Breakdown::default(),
            "locale": "enUS", "steps": steps,
        }))
        .unwrap()
    }

    fn npc(id: i64, name: &str) -> serde_json::Value {
        json!({ "kind": "npc", "id": id, "name": name })
    }

    fn at(map: i64, x: f64) -> serde_json::Value {
        json!({ "ui_map": map, "x": x, "y": 50.0 })
    }

    /// The lines of the source that are not comments or blank.
    fn lines(text: &str) -> Vec<&str> {
        text.lines()
            .filter(|l| !l.trim().is_empty() && !l.starts_with('#'))
            .collect()
    }

    #[test]
    fn a_guide_with_its_header_steps_and_goals() {
        let r = route(json!([
            { "kind": "accept", "quest": 179, "quest_name": "Dwarven Outfitters", "min_level": 1,
              "target": npc(658, "Sten Stoutarm"), "map": at(1426, 29.93) },
            { "kind": "objective", "quest": 179, "quest_name": "Dwarven Outfitters", "objective": 0,
              "target": { "kind": "item", "id": 750, "name": "Tough Wolf Meat" }, "map": at(1426, 28.9),
              "say": [{ "k": "objective.get", "a": { "n": { "num": 8 } } }] },
            { "kind": "objective", "quest": 5, "quest_name": "Q", "objective": 1, "bg": true,
              "target": npc(299, "Young Wolf"), "say": [{ "k": "objective.kill", "a": { "n": { "num": 10 } } }],
              "map": at(1427, 40.0) },
            { "kind": "objective", "quest": 310, "quest_name": "Bitter Rivals", "objective": 2, "child": 308,
              "child_name": "Distracting Jarven", "target": npc(1373, "Jarven Thunderbrew"), "map": at(1427, 47.65) },
            { "kind": "hearth", "zone": "Dun Morogh", "zone_id": 1 },
            { "kind": "fly", "target": { "kind": "taxi", "id": 6, "name": "Ironforge, Dun Morogh" } },
            { "kind": "flight_master", "target": { "kind": "taxi", "id": 6, "name": "Ironforge, Dun Morogh" },
              "map": at(1455, 55.89) },
            { "kind": "grind", "level": 12, "map": at(1455, 40.0), "text": "Grind to 12" },
        ]));
        let text = source("mon-guide", &r);
        assert_eq!(
            lines(&text),
            [
                "guide factoruide/mon-guide",
                "title \"My \\\"dwarf\\\" warrior\"",
                "faction Alliance",
                "race Dwarf",
                "class Warrior",
                "levels 1-60",
                "zone map 1426",
                "coords live",
                "step \"Sten Stoutarm\"",
                "  accept 179 \"Dwarven Outfitters\" npc 658 lvl 1 @29.93,50",
                "step",
                "  collect 750 \"Tough Wolf Meat\" x8 quest 179.1 @28.9,50",
                "step \"Young Wolf\"",
                "  zone map 1427",
                "  kill 299 \"Young Wolf\" x10 quest 5.2 @40,50 optional",
                "step \"Jarven Thunderbrew\"",
                "  turnin 308 \"Distracting Jarven\" npc 1373 @47.65,50 direct",
                "step",
                "  zone map 1455",
                "  hearth \"Dun Morogh\"",
                "  fly \"Ironforge\"",
                "  getfp \"Ironforge\" @55.89,50",
                "step",
                "  goto 40,50 \"Grind to 12\"",
                "  level 12",
            ],
            "{text}"
        );
    }

    #[test]
    fn goals_on_another_map_than_the_step_name_it() {
        let r = route(json!([
            { "kind": "train", "target": npc(1, "Trainer"), "map": at(1426, 10.0) },
            { "kind": "train", "target": npc(1, "Trainer"), "map": at(1453, 20.0) },
        ]));
        let text = source("g", &r);
        // The two at the same NPC make one step, in the first one's zone.
        assert!(
            text.contains("  train 1 \"Trainer\" @10,50\n  train 1 \"Trainer\" @20,50 map 1453\n"),
            "{text}"
        );
    }

    #[test]
    fn guide_ids_polestar_takes() {
        assert_eq!(guide_name("Mon Guide é/2"), "mon-guide---2");
        assert_eq!(guide_name("é"), "guide");
        let json: serde_json::Value = serde_json::from_str(&pack_json("1.2.0")).unwrap();
        assert_eq!(json["id"], "factoruide");
        assert_eq!(json["version"], "1.2.0");
    }
}
