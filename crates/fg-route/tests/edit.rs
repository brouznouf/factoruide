//! The guide editor on the Classic database: a guide edited by hand is replayed like the
//! optimizer's own routes, its impossible steps reported.

use fg_route::job::{self, Editor, Overrides, PlanOutcome, PlanRequest, RouteStep};
use fg_route::model::Names;
use fg_route::xp::Edition;
use rusqlite::Connection;
use std::path::PathBuf;
use std::sync::{Mutex, OnceLock};

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

fn request() -> PlanRequest {
    request_to(10)
}

fn request_to(to_level: i64) -> PlanRequest {
    PlanRequest {
        race: "orc".into(),
        class: "warlock".into(),
        from_level: 1,
        to_level,
        name: None,
        params: Some(
            overrides()
                .params
                .with_overrides(&["progression=cautious".to_owned()])
                .unwrap(),
        ),
        required_class_quests: None,
        professions: vec![],
        locale: None,
        group: vec![],
        start: None,
    }
}

/// An orc warlock guide to 10 (its first routes, no optimization).
fn guide() -> &'static PlanOutcome {
    static OUTCOME: OnceLock<PlanOutcome> = OnceLock::new();
    OUTCOME.get_or_init(|| {
        let interrupt = job::Interrupt::default();
        interrupt.stop();
        job::plan_interruptible(&database(), &overrides(), &request(), &|_| {}, &interrupt, None).unwrap()
    })
}

fn editor() -> (Editor, Names) {
    let conn = database();
    let editor = Editor::open(&conn, &overrides(), &request(), None).unwrap();
    (editor, Names::load(&conn, "enUS").unwrap())
}

fn first(steps: &[RouteStep], pick: impl Fn(&RouteStep) -> bool) -> usize {
    steps.iter().position(pick).expect("such a step in the guide")
}

/// The guide's route as it was computed replays to the guide's time, every step possible.
#[test]
fn unedited_guide_keeps_its_time() {
    let (editor, names) = editor();
    let (steps, rebuilt) = editor.steps(guide());
    assert!(!rebuilt, "the route is saved with the guide");
    let report = editor.simulate(&steps, &names);
    assert_eq!(report.errors, 0);
    assert_eq!(report.total_time, guide().route.total_time);
}

/// Each step of a guide does one thing, its own: training, a trip's flights and boats, flight
/// paths learned in passing are steps of their own, so that each one can be moved or removed.
/// Replayed unchanged, the guide keeps its time.
#[test]
fn every_thing_done_is_a_step_of_its_own() {
    use fg_route::export::StepKind;
    let conn = database();
    let interrupt = job::Interrupt::default();
    interrupt.stop();
    let request = request_to(25);
    let guide = job::plan_interruptible(&conn, &overrides(), &request, &|_| {}, &interrupt, None).unwrap();
    let editor = Editor::open(&conn, &overrides(), &request, None).unwrap();
    let names = Names::load(&conn, "enUS").unwrap();
    let (steps, _) = editor.steps(&guide);
    let count = |pick: fn(&RouteStep) -> bool| steps.iter().filter(|s| pick(s)).count();
    assert!(count(|s| matches!(s, RouteStep::Fly { .. })) > 0, "a guide to 25 flies");
    assert!(
        count(|s| matches!(s, RouteStep::Train { .. })) > 0,
        "a guide to 25 trains"
    );
    let report = editor.simulate(&steps, &names);
    assert_eq!(report.errors, 0);
    assert!(
        (report.total_time - guide.route.total_time).abs() < 1.0,
        "{} replayed, {} computed",
        report.total_time,
        guide.route.total_time
    );
    for (k, (step, r)) in steps.iter().zip(&report.steps).enumerate() {
        let Some(line) = &r.line else {
            panic!("step {k} ({step:?}) did nothing: {r:?}");
        };
        let kind = match step {
            RouteStep::Service {
                service: fg_route::plan::ServiceKind::Vendor,
                ..
            } => StepKind::Vendor,
            RouteStep::Service {
                service: fg_route::plan::ServiceKind::Mailbox,
                ..
            } => StepKind::Mailbox,
            RouteStep::DungeonExit { .. } => StepKind::DungeonExit,
            RouteStep::Accept { .. } => StepKind::Accept,
            RouteStep::Objective { .. } => StepKind::Objective,
            RouteStep::TurnIn { .. } => StepKind::TurnIn,
            RouteStep::FlightMaster { .. } => StepKind::FlightMaster,
            RouteStep::Bind { .. } => StepKind::Bind,
            RouteStep::Hearth { .. } => StepKind::Hearth,
            RouteStep::Fly { .. } => StepKind::Fly,
            RouteStep::Link { .. } => StepKind::Link,
            RouteStep::Teleport { .. } => StepKind::Teleport,
            RouteStep::Train { .. } => StepKind::Train,
            RouteStep::Dungeon { .. } => StepKind::Dungeon,
            RouteStep::Abandon { .. } => StepKind::Abandon,
            RouteStep::Grind { .. } | RouteStep::GrindTo { .. } | RouteStep::CampGrind { .. } => StepKind::Grind,
        };
        assert_eq!(line.kind, kind, "step {k}: {step:?}");
    }
}

/// A guide saved before each thing done was a step (its plan only has the route's stops) opens
/// with its training as steps of their own, every step possible.
#[test]
fn older_guide_opens_with_its_training_as_steps() {
    let (editor, names) = editor();
    let mut older = guide().clone();
    older.plan.retain(|s| {
        matches!(
            s,
            RouteStep::Accept { .. }
                | RouteStep::Objective { .. }
                | RouteStep::TurnIn { .. }
                | RouteStep::Bind { .. }
                | RouteStep::Hearth { .. }
                | RouteStep::Dungeon { .. }
        )
    });
    older.explicit = false;
    let (steps, rebuilt) = editor.steps(&older);
    assert!(!rebuilt);
    assert!(steps.iter().any(|s| matches!(s, RouteStep::Train { .. })), "{steps:?}");
    let report = editor.simulate(&steps, &names);
    assert_eq!(report.errors, 0);
}

/// A turn-in moved before its quest is taken is not possible: reported there, and the guide
/// cannot be saved.
#[test]
fn turn_in_before_accept_is_an_error() {
    let (editor, names) = editor();
    let mut steps = guide().plan.clone();
    let turn_in = first(&steps, |s| matches!(s, RouteStep::TurnIn { .. }));
    let RouteStep::TurnIn { quest } = steps[turn_in] else {
        unreachable!()
    };
    let accept = first(&steps, |s| *s == RouteStep::Accept { quest });
    let moved = steps.remove(turn_in);
    steps.insert(accept, moved);
    let report = editor.simulate(&steps, &names);
    assert_eq!(report.steps[accept].status, "error");
    assert_eq!(report.steps[accept].reason, Some("quest not accepted"));
    assert!(editor.save(&steps, vec![]).is_err());
}

/// A turn-in removed (of a quest no other one needs) leaves the quest unfinished: a warning, the
/// guide can still be saved.
#[test]
fn removed_turn_in_leaves_the_quest_unfinished() {
    let (editor, names) = editor();
    let before = editor.simulate(&guide().plan, &names);
    let (steps, quest, report) = (0..guide().plan.len())
        .find_map(|k| {
            let RouteStep::TurnIn { quest } = guide().plan[k] else {
                return None;
            };
            if before.steps[k].status != "done" {
                return None;
            }
            let mut steps = guide().plan.clone();
            steps.remove(k);
            let report = editor.simulate(&steps, &names);
            (report.errors == 0).then_some((steps, quest, report))
        })
        .expect("a turn-in no other quest needs");
    assert!(
        report.unfinished.iter().any(|q| q.id == quest),
        "{:?}",
        report.unfinished
    );
    // The same steps, objectives at one place possibly brought together.
    let sorted = |steps: &[RouteStep]| {
        let mut v: Vec<String> = steps.iter().map(|s| format!("{s:?}")).collect();
        v.sort();
        v
    };
    let saved = editor.save(&steps, vec![]).unwrap();
    assert_eq!(sorted(&saved.plan), sorted(&steps));
}

/// Mobs killed where the character stands give their XP and take their time, and the guide
/// gets a step for them.
#[test]
fn grind_step_kills_its_mobs() {
    let (editor, names) = editor();
    let mut steps = guide().plan.clone();
    steps.insert(1, RouteStep::Grind { mobs: 10 });
    let report = editor.simulate(&steps, &names);
    let grind = &report.steps[1];
    assert_eq!(grind.status, "done");
    assert!(grind.xp > 0 && grind.end > grind.start, "{grind:?}");
    assert!(grind.line.as_ref().unwrap().text.starts_with("Kill 10 mobs"));
    let saved = editor.save(&steps, vec![]).unwrap();
    assert!(saved.route.steps.iter().any(|s| s.text.starts_with("Kill 10 mobs")));
}

/// Killing every mob on the way to an objective gives what the editor announced for it.
#[test]
fn farm_on_the_way_gives_its_estimate() {
    let (editor, names) = editor();
    let mut steps = guide().plan.clone();
    let report = editor.simulate(&steps, &names);
    let (k, estimate) = report
        .steps
        .iter()
        .enumerate()
        .find_map(|(k, s)| s.farm_all.filter(|f| f.xp > 0).map(|f| (k, f)))
        .expect("an objective with mobs on the way");
    let RouteStep::Objective { farm, .. } = &mut steps[k] else {
        panic!("{:?}", steps[k])
    };
    *farm = true;
    let farmed = editor.simulate(&steps, &names).steps[k].farm;
    assert_eq!(farmed, Some(estimate));
}

/// A flight from a flight path the character does not know is not possible.
#[test]
fn flight_needs_known_flight_paths() {
    let (editor, names) = editor();
    let mut steps = guide().plan.clone();
    // Orgrimmar to the Crossroads.
    steps.insert(0, RouteStep::Fly { from: 23, to: 25 });
    let report = editor.simulate(&steps, &names);
    assert_eq!(report.steps[0].status, "error");
    assert_eq!(report.steps[0].reason, Some("flight path not known"));
}
