//! The library over two kinds of files: the disk, and a map in memory (as the browser's).
#![expect(clippy::unwrap_used, reason = "the helpers of a test fail it")]

use fg_library::{DiskFiles, Files, GuideId, Library, LibraryError, SharedGuide};
use fg_route::xp::Edition;
use std::collections::BTreeMap;
use std::io;
use std::sync::Mutex;

#[derive(Default)]
struct Memory(Mutex<BTreeMap<String, Vec<u8>>>);

impl Files for Memory {
    fn read(&self, path: &str) -> io::Result<Vec<u8>> {
        self.0
            .lock()
            .unwrap()
            .get(path)
            .cloned()
            .ok_or_else(|| io::Error::new(io::ErrorKind::NotFound, "not found"))
    }

    fn write(&self, path: &str, data: &[u8]) -> io::Result<()> {
        self.0.lock().unwrap().insert(path.to_owned(), data.to_vec());
        Ok(())
    }

    fn list(&self, dir: &str) -> Vec<String> {
        let prefix = format!("{dir}/");
        let mut names: Vec<String> = self
            .0
            .lock()
            .unwrap()
            .keys()
            .filter_map(|k| k.strip_prefix(&prefix)?.split('/').next().map(str::to_owned))
            .collect();
        names.dedup();
        names
    }

    fn remove_dir(&self, dir: &str) -> io::Result<()> {
        let prefix = format!("{dir}/");
        let mut files = self.0.lock().unwrap();
        let before = files.len();
        files.retain(|k, _| !k.starts_with(&prefix));
        if files.len() == before {
            return Err(io::Error::new(io::ErrorKind::NotFound, "not found"));
        }
        Ok(())
    }
}

fn id(name: &str) -> GuideId {
    GuideId::try_from(name.to_owned()).unwrap()
}

fn temp_root(name: &str) -> std::path::PathBuf {
    let dir = std::env::temp_dir().join(format!("fg-library-{}-{name}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    dir
}

#[test]
fn a_library_without_files_has_no_guides() {
    let files = Memory::default();
    let library = Library::new(&files, Edition::Classic);
    assert_eq!(library.ids().len(), 0);
    assert_eq!(library.list().len(), 0);
    assert_eq!(library.followed().len(), 0);
    assert_eq!(library.walk_memories().len(), 0);
}

#[test]
fn a_missing_guide_is_named_in_the_error() {
    let files = Memory::default();
    let library = Library::new(&files, Edition::Classic);
    let e = library.delete_guide(&id("nobody")).unwrap_err();
    assert!(matches!(e, LibraryError::NoGuide(_)));
    assert_eq!(e.to_string(), "no guide nobody");
    let e = library.version(&id("nobody"), 1).unwrap_err();
    assert_eq!(e.to_string(), "guides/classic/nobody/v1/outcome.json: not found");
}

#[test]
fn a_summary_needs_the_guide_file() {
    let files = Memory::default();
    files
        .write("guides/classic/a/guide.json", br#"{"name":"A","installed":2}"#)
        .unwrap();
    let library = Library::new(&files, Edition::Classic);
    let summary = library.summary(&id("a")).unwrap();
    assert_eq!((summary.name.as_str(), summary.installed), ("A", Some(2)));
    assert_eq!(summary.versions.len(), 0);
    // A guide without a version is not listed.
    assert_eq!(library.list().len(), 0);
    library.set_installed(&id("a"), None).unwrap();
    assert_eq!(library.info(&id("a")).unwrap().installed, None);
}

/// A guide of no step, as the planner would give it.
fn empty_guide(name: &str) -> (fg_route::job::PlanRequest, fg_route::job::PlanOutcome) {
    let request = serde_json::from_value(serde_json::json!({
        "race": "dwarf", "class": "warrior", "from_level": 1, "to_level": 2, "name": name,
    }))
    .unwrap();
    let outcome = serde_json::from_value(serde_json::json!({
        "route": {
            "name": name, "race_id": 3, "class_id": 1, "class": "WARRIOR", "faction": "Alliance",
            "from_level": 1, "to_level": 2, "total_time": 0.0, "total_time_text": "0h00", "quests": 0,
            "breakdown": {
                "travel": 0.0, "flights": 0.0, "fighting": 0.0, "grinding": 0.0, "overhead": 0.0,
                "quest_xp": 0, "mob_xp": 0, "grind_xp": 0,
            },
            "steps": [],
        },
        "notes": [],
    }))
    .unwrap();
    (request, outcome)
}

/// A version chosen for the game stays it until the player makes a new one: the new version
/// (computed again, set up again, edited or imported) is the one the game gets.
#[test]
fn a_new_version_is_followed_even_when_an_older_one_was_chosen() {
    let files = Memory::default();
    let library = Library::new(&files, Edition::Classic);
    let (request, outcome) = empty_guide("A");
    for _ in 0..2 {
        library
            .add_version(&request, &outcome, None, fg_library::Origin::COMPUTED, 0)
            .unwrap();
    }
    library.set_installed(&id("a"), Some(1)).unwrap();
    assert_eq!(library.followed_version(&id("a")), Some(1));

    library
        .add_version(&request, &outcome, None, fg_library::Origin::COMPUTED, 0)
        .unwrap();
    assert_eq!(library.followed_version(&id("a")), Some(3));
}

#[test]
fn a_corrupt_file_says_which() {
    let files = Memory::default();
    files.write("guides/tbc/a/guide.json", b"{").unwrap();
    let e = Library::new(&files, Edition::Tbc).info(&id("a")).unwrap_err();
    assert!(matches!(e, LibraryError::Corrupt { .. }));
    assert!(e.to_string().starts_with("guides/tbc/a/guide.json: "), "{e}");
}

#[test]
fn editions_do_not_share_guides() {
    let files = Memory::default();
    files.write("guides/classic/a/guide.json", br#"{"name":"A"}"#).unwrap();
    assert_eq!(Library::new(&files, Edition::Classic).ids(), [id("a")]);
    assert_eq!(Library::new(&files, Edition::Tbc).ids().len(), 0);
}

#[test]
fn files_on_the_disk_make_their_folders_and_report_the_missing() {
    let root = temp_root("disk");
    let files = DiskFiles::new(&root);
    files.write("guides/classic/a/guide.json", br#"{"name":"A"}"#).unwrap();
    assert_eq!(files.list("guides/classic"), ["a"]);
    assert_eq!(files.list("guides/none").len(), 0);
    let library = Library::new(&files, Edition::Classic);
    assert_eq!(library.info(&id("a")).unwrap().name, "A");
    library.delete_guide(&id("a")).unwrap();
    assert!(matches!(library.delete_guide(&id("a")), Err(LibraryError::NoGuide(_))));
    let _ = std::fs::remove_dir_all(&root);
}

#[test]
fn a_file_that_is_not_a_guide_is_refused() {
    for content in ["", "[]", r#"{"format":"other"}"#, "not json"] {
        assert!(
            matches!(SharedGuide::parse(content), Err(LibraryError::NotAGuideFile(_))),
            "{content:?}"
        );
    }
}

proptest::proptest! {
    /// A file received is a guide or is refused, whatever it holds.
    #[test]
    fn any_text_is_a_guide_or_refused(text in ".{0,512}") {
        let _ = SharedGuide::parse(&text);
    }
}

#[test]
fn an_identifier_cannot_point_out_of_the_guides_folder() {
    for bad in ["", ".", "..", "../x", "a/b", "a\\b", "c:", "a\0b"] {
        assert!(GuideId::try_from(bad.to_owned()).is_err(), "{bad:?}");
    }
    for good in ["warrior-human", "route", "Guide 1", "été"] {
        assert!(GuideId::try_from(good.to_owned()).is_ok(), "{good:?}");
    }
    let e = serde_json::from_str::<GuideId>(r#""../../etc""#).unwrap_err();
    assert!(e.to_string().contains("not a guide identifier"), "{e}");
    assert_eq!(GuideId::from_name("Orc Warlock 1-60").as_str(), "orc-warlock-1-60");
}
