//! Route and share forward quest choices and completion windows to the planner.
use std::{path::PathBuf, process::Command};

#[test]
fn quest_choices_and_completion_windows_reach_the_planner() {
    let repo = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../..");
    let temp = std::env::temp_dir().join(format!("factoruide-required-cli-{}", std::process::id()));
    std::fs::create_dir_all(&temp).unwrap();
    // The Human's first quests, in a database of their own.
    let mut world = fg_route::fixture::Fixture::new(fg_route::xp::Edition::Classic)
        .file("classic_northshire")
        .open();
    let database = temp.join("classic.sqlite");
    std::fs::write(&database, world.serialize(rusqlite::MAIN_DB).unwrap().to_vec()).unwrap();
    // Each selected quest conflicts with a user exclusion in turn: dropping either CLI
    // option would lose that quest's error. The second option is a real exact quest name.
    for subcommand in ["route", "share"] {
        for (id, choice, reason) in [
            (Some(783), "783", "excluded by configuration"),
            (Some(33), "783", "excluded by configuration"),
            (
                None,
                r#"{"quest":783,"min_level":3,"max_level":2}"#,
                "minimum completion level exceeds maximum",
            ),
        ] {
            let mut command = Command::new(env!("CARGO_BIN_EXE_fg"));
            command
                .current_dir(&repo)
                .args(["--edition", "classic", "--db"])
                .arg(&database)
                .arg("--routes")
                .arg(temp.join("routes"))
                .arg("--addon-dir")
                .arg(temp.join("addon"))
                .args([
                    subcommand,
                    "--race",
                    "human",
                    "--class",
                    "warrior",
                    "--to-level",
                    "2",
                    "--effort",
                    "0",
                    "--required-quest",
                    choice,
                    "--required-quest",
                    "Wolves Across the Border",
                    "--param",
                ])
                .arg(format!(
                    "excluded_quests={}",
                    id.map_or_else(|| "[]".to_owned(), |id| format!("[{id}]"))
                ));
            if subcommand == "share" {
                command
                    .args(["--name", "required-quests", "--out"])
                    .arg(temp.join("required.fgguide"));
            }
            let out = command.output().unwrap();
            let error = String::from_utf8(out.stderr).unwrap();
            assert!(!out.status.success(), "{error}");
            if let Some(id) = id {
                assert!(error.contains(&format!("cannot plan required quest {id}")), "{error}");
            }
            assert!(error.contains(reason), "{error}");
        }
    }
    std::fs::remove_dir_all(temp).unwrap();
}
