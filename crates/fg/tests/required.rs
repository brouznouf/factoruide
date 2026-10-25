//! The CLI forwards repeated IDs and names to the planner's requirement checks.
use std::{path::PathBuf, process::Command};

#[test]
fn repeated_quest_choices_reach_the_planner_as_ids_and_names() {
    let repo = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../..");
    let temp = std::env::temp_dir().join(format!("factoruide-required-cli-{}", std::process::id()));
    std::fs::create_dir_all(&temp).unwrap();
    let data = zstd::decode_all(std::fs::File::open(repo.join("data/classic.sqlite.zst")).unwrap()).unwrap();
    let database = temp.join("classic.sqlite");
    std::fs::write(&database, data).unwrap();
    // Each selected quest conflicts with a user exclusion in turn: dropping either CLI
    // option would lose that quest's error. The second option is a real exact quest name.
    for id in [783, 33] {
        let out = Command::new(env!("CARGO_BIN_EXE_fg"))
            .current_dir(&repo)
            .args(["--edition", "classic", "--db"])
            .arg(&database)
            .arg("--routes")
            .arg(temp.join("routes"))
            .arg("--addon-dir")
            .arg(temp.join("addon"))
            .args([
                "route",
                "--race",
                "human",
                "--class",
                "warrior",
                "--to-level",
                "2",
                "--effort",
                "0",
                "--required-quest",
                "783",
                "--required-quest",
                "Wolves Across the Border",
                "--param",
            ])
            .arg(format!("excluded_quests=[{id}]"))
            .output()
            .unwrap();
        let error = String::from_utf8(out.stderr).unwrap();
        assert!(!out.status.success(), "{error}");
        assert!(error.contains(&format!("cannot plan required quest {id}")), "{error}");
        assert!(error.contains("excluded by configuration"), "{error}");
    }
    std::fs::remove_dir_all(temp).unwrap();
}
