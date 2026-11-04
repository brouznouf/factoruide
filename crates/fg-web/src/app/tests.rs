//! The web backend's calls, as the page makes them, on the game data of the repository.

use super::*;
use crate::host::native::{FILES, REPLIES};

/// A call as the page makes it: the answer's value, or its error.
fn call(cmd: &str, args: &serde_json::Value) -> Result<serde_json::Value, String> {
    let answer: serde_json::Value = serde_json::from_str(&dispatch(
        serde_json::json!({ "cmd": cmd, "args": args }).to_string().as_bytes(),
    ))
    .unwrap();
    match answer.get("err") {
        Some(e) => Err(e.as_str().unwrap_or_default().to_owned()),
        None => Ok(answer["ok"].clone()),
    }
}

/// The page's files are shared: one test using them at a time.
static STORE: Mutex<()> = Mutex::new(());

/// The file the site serves as `path`, replaced.
fn set_file(path: String, bytes: Vec<u8>) {
    let mut files = FILES.lock().unwrap();
    files.retain(|(p, _)| *p != path);
    files.push((path, bytes));
    forget_databases();
}

/// A small world of an edition (the fixtures' files of fg-route) served as its database.
fn serve_fixture(edition: Edition, files: &[&str]) {
    let mut conn = files
        .iter()
        .fold(fg_route::fixture::Fixture::new(edition), |f, name| f.file(name))
        .open();
    let bytes = conn.serialize(rusqlite::MAIN_DB).unwrap().to_vec();
    set_file(
        format!("data/{}.sqlite.zst", edition.key()),
        zstd::encode_all(&bytes[..], 1).unwrap(),
    );
}

fn serve_classic() {
    serve_fixture(
        Edition::Classic,
        &["classic_services", "classic_coldridge", "classic_map_index"],
    );
}

/// What `/fg export` gives for a dwarf warrior.
fn profile_export(name: &str, level: i64) -> String {
    let json = serde_json::json!({
        "format": "factoruide-profile", "version": 1, "edition": "classic", "addon": "0.13.0",
        "character": name,
        "profile": {
            "name": name, "race": 3, "class": "WARRIOR", "faction": "Alliance", "level": level,
            "xp": 1200, "xp_max": 2800, "rested": 0, "resting": false, "completed": [179, 233, 234],
            "log": [], "position": { "map": 1426, "x": 0.468, "y": 0.52 }, "instance": false,
            "bind": { "name": "Coldridge Valley" }, "flights": [], "professions": [], "riding": 0,
            "gear": [], "spells": [], "reputation": [], "money": 1234, "time": 1_760_800_000
        },
    });
    fgstring::encode(fgstring::PROFILE_PREFIX, json.to_string().as_bytes())
}

/// From a character pasted to the text the addon imports: the guide made from it, followed by
/// it, sent to the game; its recordings kept until sent; everything saved for the next visit.
#[test]
fn a_character_pasted_gets_its_guide_into_the_game() {
    let _store = STORE.lock().unwrap_or_else(std::sync::PoisonError::into_inner);
    serve_classic();
    store::clear();
    call("set_edition", &serde_json::json!({ "edition": "classic" })).unwrap();

    // A text that is not an export, or damaged, says so.
    assert_eq!(
        call("import_profile", &serde_json::json!({ "content": "hello" })).unwrap_err(),
        "not_a_profile"
    );
    let export = profile_export("Brouz-Realm", 5);
    let damaged = format!("{}x!", &export[..export.len() / 2]);
    assert_eq!(
        call("import_profile", &serde_json::json!({ "content": damaged })).unwrap_err(),
        "corrupt_profile"
    );

    let imported = call("import_profile", &serde_json::json!({ "content": export })).unwrap();
    assert_eq!(imported["character"], "Brouz-Realm");
    assert_eq!(imported["level"], 5);
    // Pasted again later: the profile replaced.
    let again = profile_export("Brouz-Realm", 6);
    assert_eq!(
        call("import_profile", &serde_json::json!({ "content": again })).unwrap()["level"],
        6
    );
    let characters = call("list_characters", &serde_json::json!({})).unwrap();
    assert_eq!(characters.as_array().unwrap().len(), 1);
    assert_eq!(characters[0]["race"], "dwarf");
    assert_eq!(characters[0]["class"], "warrior");

    // A guide from the character, a short one.
    let mut request: serde_json::Value =
        serde_json::from_str(r#"{"race":"dwarf","class":"warrior","from_level":6,"to_level":8,"name":"Brouz web"}"#)
            .unwrap();
    request["start"] = characters[0]["profile"].clone();
    let mut params = serde_json::to_value(overrides(Edition::Classic).unwrap().params.clone()).unwrap();
    params["effort"] = 0.0.into();
    request["params"] = params;
    let meta = call("plan_route", &serde_json::json!({ "request": request })).unwrap();
    assert_eq!(meta["character"], "Brouz-Realm");
    let guide = meta["guide"].as_str().unwrap().to_owned();
    assert_eq!(
        call("list_guides", &serde_json::json!({}))
            .unwrap()
            .as_array()
            .unwrap()
            .len(),
        1
    );

    // Its text for the game: the guide, when it was computed and its version, the places.
    let export = call("guide_text", &serde_json::json!({ "guide": guide, "version": 1 })).unwrap();
    let lua =
        String::from_utf8(fgstring::decode(fgstring::GUIDE_PREFIX, export["text"].as_str().unwrap()).unwrap()).unwrap();
    assert!(lua.contains("FG.Routes[\"Brouz web\"] = {"), "the guide");
    assert!(
        lua.contains(&format!("FG.Routes[\"Brouz web\"].created = {}", meta["created"])),
        "when it was computed"
    );
    assert!(lua.contains("FG.Routes[\"Brouz web\"].version = 1"), "its version");
    assert!(lua.contains("FG.Services"), "the places");
    assert!(
        !lua.contains("FG.Assignments") && !lua.contains("FG.Config") && !lua.contains("FactoruideSent"),
        "no assignment, no settings, no sends"
    );
    // Every guide, as the data addon, zipped.
    let zip = call("data_zip", &serde_json::json!({})).unwrap();
    assert_eq!(zip["file"], "Factoruide_Data.zip");
    assert!(zip["base64"].as_str().unwrap().len() > 1000);

    // Watched, edited by hand, compared: the calls of the guide's page.
    let timeline = call(
        "guide_timeline",
        &serde_json::json!({ "guide": guide, "version": 1, "locale": "frFR" }),
    )
    .unwrap();
    assert_ne!(timeline["steps"].as_array().unwrap().len(), 0);
    let opened = call(
        "edit_open",
        &serde_json::json!({ "guide": guide, "version": 1, "locale": "enUS" }),
    )
    .unwrap();
    let mut steps = opened["steps"].as_array().unwrap().clone();
    assert!(steps.len() > 2);
    steps.pop();
    let view = call(
        "edit_simulate",
        &serde_json::json!({ "steps": steps, "locale": "enUS" }),
    )
    .unwrap();
    assert_eq!(view["report"]["steps"].as_array().unwrap().len(), steps.len());
    call(
        "edit_suggest",
        &serde_json::json!({ "steps": steps, "after": 0, "locale": "enUS" }),
    )
    .unwrap();
    let edited = call("edit_save", &serde_json::json!({ "steps": steps })).unwrap();
    assert_eq!(
        (edited["version"].as_u64(), edited["edited"].as_bool()),
        (Some(2), Some(true))
    );
    let compared = call(
        "compare_versions",
        &serde_json::json!({ "guide": guide, "a": 1, "b": 2, "locale": "enUS" }),
    )
    .unwrap();
    assert_ne!(compared["text"].as_str().unwrap().len(), 0);
    call("delete_version", &serde_json::json!({ "guide": guide, "version": 2 })).unwrap();
    // The other pages' data.
    assert_ne!(
        call("map_index", &serde_json::json!({}))
            .unwrap()
            .as_array()
            .unwrap()
            .len(),
        0
    );
    assert_eq!(call("get_options", &serde_json::json!({})).unwrap()["max_level"], 60);

    // A file to share comes back as a new version.
    let shared = call("export_version", &serde_json::json!({ "guide": guide, "version": 1 })).unwrap();
    assert_eq!(shared["file"], format!("{guide}-v1.fgguide"));
    let v2 = call("import_guide", &serde_json::json!({ "content": shared["text"] })).unwrap();
    assert_eq!(
        (v2["version"].as_u64(), v2["imported"].as_bool()),
        (Some(2), Some(true))
    );

    // Every file the page saved, given back at the next visit, gives the same guides.
    let saved: Vec<(String, Vec<u8>)> = ["settings.json", &format!("guides/classic/{guide}/guide.json")]
        .iter()
        .map(|p| ((*p).to_owned(), store::read(p).unwrap()))
        .collect();
    for (p, d) in saved {
        store::load(&p, d);
    }
    assert_eq!(
        call("get_guide", &serde_json::json!({ "id": guide })).unwrap()["versions"]
            .as_array()
            .unwrap()
            .len(),
        2
    );

    // Deleted: nothing left to send.
    call("delete_guide", &serde_json::json!({ "guide": guide })).unwrap();
    assert_eq!(call("data_zip", &serde_json::json!({})).unwrap_err(), "no guide yet");
    let _ = REPLIES.lock().unwrap().drain(..);
}

/// A guide created by hand from a character pasted: saved with no step, edited by hand, from the
/// character as generation starts it; watched, opened in the editor, given its steps, exported.
#[test]
fn a_guide_created_by_hand_starts_empty_from_the_character() {
    let _store = STORE.lock().unwrap_or_else(std::sync::PoisonError::into_inner);
    serve_classic();
    store::clear();
    call("set_edition", &serde_json::json!({ "edition": "classic" })).unwrap();
    call(
        "import_profile",
        &serde_json::json!({ "content": profile_export("Brouz-Realm", 6) }),
    )
    .unwrap();
    let characters = call("list_characters", &serde_json::json!({})).unwrap();
    let mut request: serde_json::Value = serde_json::from_str(
        r#"{"race":"dwarf","class":"warrior","from_level":6,"to_level":8,"name":"Brouz by hand"}"#,
    )
    .unwrap();
    request["start"] = characters[0]["profile"].clone();
    let meta = call("manual_route", &serde_json::json!({ "request": request })).unwrap();
    assert_eq!(
        (
            meta["version"].as_u64(),
            meta["edited"].as_bool(),
            meta["character"].as_str()
        ),
        (Some(1), Some(true), Some("Brouz-Realm"))
    );
    let guide = meta["guide"].as_str().unwrap().to_owned();
    let version = call("get_version", &serde_json::json!({ "guide": guide, "version": 1 })).unwrap();
    assert_eq!(version["outcome"]["route"]["quests"], 0);
    assert_eq!(version["outcome"]["route"]["from_level"], 6);
    assert!(meta["total_time"].as_f64().unwrap() > 0.0, "the grind to level 8");

    call(
        "guide_timeline",
        &serde_json::json!({ "guide": guide, "version": 1, "locale": "enUS" }),
    )
    .unwrap();
    let opened = call(
        "edit_open",
        &serde_json::json!({ "guide": guide, "version": 1, "locale": "enUS" }),
    )
    .unwrap();
    assert_eq!(opened["steps"], serde_json::json!([]));
    assert!(opened["view"]["report"]["final_grind"].as_f64().unwrap() > 0.0);
    let steps = serde_json::json!([{ "kind": "grind", "mobs": 5 }]);
    let view = call(
        "edit_simulate",
        &serde_json::json!({ "steps": steps, "locale": "enUS" }),
    )
    .unwrap();
    assert_eq!(view["report"]["errors"], 0);
    let edited = call("edit_save", &serde_json::json!({ "steps": steps })).unwrap();
    assert_eq!(
        (edited["version"].as_u64(), edited["edited"].as_bool()),
        (Some(2), Some(true))
    );
    call(
        "compare_versions",
        &serde_json::json!({ "guide": guide, "a": 1, "b": 2, "locale": "enUS" }),
    )
    .unwrap();
    for v in [1, 2] {
        call("guide_text", &serde_json::json!({ "guide": guide, "version": v })).unwrap();
        call("export_version", &serde_json::json!({ "guide": guide, "version": v })).unwrap();
    }
    call("delete_guide", &serde_json::json!({ "guide": guide })).unwrap();
    let _ = REPLIES.lock().unwrap().drain(..);
}

/// The exports of the released addons (tests/profiles, one per release whose export changed, see
/// its README) are still read: the character, its game version, level, race and class.
#[test]
fn the_exports_of_released_addons_are_read() {
    let _store = STORE.lock().unwrap_or_else(std::sync::PoisonError::into_inner);
    serve_fixture(Edition::Forever, &[]);
    let dir = concat!(env!("CARGO_MANIFEST_DIR"), "/tests/profiles");
    let mut files: Vec<_> = std::fs::read_dir(dir)
        .unwrap()
        .map(|e| e.unwrap().path())
        .filter(|p| p.extension().is_some_and(|e| e == "txt"))
        .collect();
    files.sort();
    assert!(!files.is_empty(), "no export in {dir}");
    for file in files {
        store::clear();
        let what = file.display().to_string();
        let content = std::fs::read_to_string(&file).unwrap();
        let imported = call("import_profile", &serde_json::json!({ "content": content })).unwrap();
        assert_eq!(imported["character"], "Brouz-Realm", "{what}");
        assert_eq!(imported["edition"], "forever", "{what}");
        assert_eq!(imported["level"], 4, "{what}");
        call("set_edition", &serde_json::json!({ "edition": "forever" })).unwrap();
        let characters = call("list_characters", &serde_json::json!({})).unwrap();
        assert_eq!(characters.as_array().unwrap().len(), 1, "{what}");
        assert_eq!(characters[0]["race"], "gnome", "{what}");
        assert_eq!(characters[0]["class"], "mage", "{what}");
        let profile = &characters[0]["profile"];
        assert_eq!(profile["flights"], serde_json::json!([2, 12]), "{what}");
        assert_eq!(profile["completed"], serde_json::json!([179, 233]), "{what}");
    }
}

/// The page's calls run on threads of their own: several reading the game data at the same time
/// (the replay in a few languages, the editor, the export to the game) all answer, as each one
/// alone does.
#[test]
fn calls_reading_the_game_data_at_the_same_time_all_answer() {
    let _store = STORE.lock().unwrap_or_else(std::sync::PoisonError::into_inner);
    serve_fixture(Edition::Forever, &["forever_services", "forever_zephras"]);
    store::clear();
    call("set_edition", &serde_json::json!({ "edition": "forever" })).unwrap();
    let mut request: serde_json::Value =
        serde_json::from_str(r#"{"race":"skyborne","class":"mage","from_level":1,"to_level":5,"name":"Concurrent"}"#)
            .unwrap();
    let mut params = serde_json::to_value(overrides(Edition::Forever).unwrap().params.clone()).unwrap();
    params["effort"] = 0.0.into();
    request["params"] = params;
    let meta = call("plan_route", &serde_json::json!({ "request": request })).unwrap();
    let guide = meta["guide"].as_str().unwrap().to_owned();
    let _ = REPLIES.lock().unwrap().drain(..);

    let calls = [
        (
            "guide_timeline",
            serde_json::json!({ "guide": guide, "version": 1, "locale": "enUS" }),
        ),
        (
            "guide_timeline",
            serde_json::json!({ "guide": guide, "version": 1, "locale": "frFR" }),
        ),
        (
            "guide_timeline",
            serde_json::json!({ "guide": guide, "version": 1, "locale": "deDE" }),
        ),
        (
            "edit_open",
            serde_json::json!({ "guide": guide, "version": 1, "locale": "esES" }),
        ),
        ("guide_text", serde_json::json!({ "guide": guide, "version": 1 })),
        (
            "guide_timeline",
            serde_json::json!({ "guide": guide, "version": 1, "locale": "ruRU" }),
        ),
    ];
    let first_id = 9000;
    for (i, (cmd, args)) in calls.iter().enumerate() {
        let request = serde_json::json!({ "cmd": cmd, "args": args }).to_string();
        crate::call(first_id + i as u32, request.into_bytes());
    }
    let ids = first_id..first_id + calls.len() as u32;
    let started = std::time::Instant::now();
    let answers = loop {
        let answers: Vec<(u32, String)> = (REPLIES.lock().unwrap().iter())
            .filter(|(id, _)| ids.contains(id))
            .cloned()
            .collect();
        if answers.len() == calls.len() {
            break answers;
        }
        assert!(
            started.elapsed().as_secs() < 600,
            "{} of {} calls answered",
            answers.len(),
            calls.len()
        );
        std::thread::sleep(std::time::Duration::from_millis(2));
    };
    for (id, answer) in answers {
        let (cmd, _) = &calls[(id - first_id) as usize];
        let answer: serde_json::Value = serde_json::from_str(&answer).unwrap();
        assert!(answer.get("err").is_none(), "{cmd}: {}", answer["err"]);
        if *cmd == "guide_text" {
            assert_ne!(answer["ok"]["text"].as_str().unwrap(), "");
        } else {
            assert_ne!(answer["ok"]["steps"].as_array().unwrap().len(), 0, "{cmd}");
        }
    }

    call("edit_close", &serde_json::json!({})).unwrap();
    call("delete_guide", &serde_json::json!({ "guide": guide })).unwrap();
    let _ = REPLIES.lock().unwrap().drain(..);
}
