//! A guide replayed to be watched, on the Classic database: its times are the guide's, the
//! character moves to where each step is done, and what it holds follows the steps.

use crate::common::{database, repo};
use fg_route::export::StepKind;
use fg_route::job::{self, Editor, LegMode, Overrides, PlanOutcome, PlanRequest, Timeline};
use fg_route::model::Names;
use fg_route::xp::Edition;
use std::collections::HashSet;
use std::sync::OnceLock;

fn overrides() -> Overrides {
    Overrides::load_edition(&repo().join("overrides"), Edition::Classic).unwrap()
}

/// An orc warlock to 25 (its first routes, no optimization): it flies and takes boats.
fn request() -> PlanRequest {
    PlanRequest {
        race: "orc".into(),
        class: "warlock".into(),
        from_level: 1,
        to_level: 25,
        name: None,
        params: Some(
            overrides()
                .params
                .with_overrides(&["progression=cautious".to_owned()])
                .unwrap(),
        ),
        required_class_quests: None,
        required_quests: vec![],
        professions: vec![],
        locale: None,
        mates: vec![],
        start: None,
    }
}

/// The guide, the editor that replays it and its timeline.
fn setup() -> &'static (PlanOutcome, Editor, Timeline) {
    static SETUP: OnceLock<(PlanOutcome, Editor, Timeline)> = OnceLock::new();
    SETUP.get_or_init(|| {
        let conn = database();
        let interrupt = job::Interrupt::default();
        interrupt.stop();
        let guide = job::plan_interruptible(&conn, &overrides(), &request(), &|_| {}, &interrupt, None).unwrap();
        let editor = Editor::open(&conn, &overrides(), &request(), None).unwrap();
        let timeline = editor.timeline(&guide, &Names::load(&conn, "enUS").unwrap());
        (guide, editor, timeline)
    })
}

/// The quests in the log after each step, rebuilt from the timeline.
fn logs(timeline: &Timeline) -> Vec<HashSet<i64>> {
    let mut log: HashSet<i64> = timeline.start.log.iter().copied().collect();
    timeline
        .steps
        .iter()
        .map(|s| {
            log.extend(&s.accepted);
            for q in &s.removed {
                log.remove(q);
            }
            log.clone()
        })
        .collect()
}

/// A step of the timeline per step of the guide, each one when the guide says: the last ends
/// with the guide.
#[test]
fn times_are_the_guides() {
    let (guide, _, timeline) = setup();
    let steps = &guide.route.steps;
    assert_eq!(timeline.steps.len(), steps.len());
    assert_eq!(timeline.matched, steps.len(), "the replay writes the guide again");
    for (k, (s, t)) in steps.iter().zip(&timeline.steps).enumerate() {
        assert!((t.end - s.time).abs() < 0.1, "step {k}: {} for {}", t.end, s.time);
        if k > 0 {
            assert!((t.start - steps[k - 1].time).abs() < 0.1, "step {k}");
        }
        let split = t.travel + t.combat + t.interact + t.wait;
        assert!(
            (split - (t.end - t.start)).abs() < 1.0,
            "step {k}: {split} in {}",
            t.end - t.start
        );
    }
    let last = timeline.steps.last().unwrap();
    assert!(
        (last.end - guide.route.total_time).abs() < 1.0,
        "{} vs {}",
        last.end,
        guide.route.total_time
    );
}

/// The quest is in the log once its accept step is done, and out of it once turned in.
#[test]
fn accepted_quests_are_in_the_log_until_turned_in() {
    let (guide, _, timeline) = setup();
    let logs = logs(timeline);
    let mut checked = 0;
    for (k, s) in guide.route.steps.iter().enumerate() {
        let Some(quest) = s.quest else { continue };
        match s.kind {
            StepKind::Accept => assert!(logs[k].contains(&quest), "step {k}: {quest} not in the log"),
            StepKind::TurnIn => assert!(!logs[k].contains(&quest), "step {k}: {quest} still in the log"),
            _ => continue,
        }
        checked += 1;
    }
    assert!(checked > 20);
    // The log never holds more than its size.
    assert!(logs.iter().all(|l| l.len() <= timeline.log_size));
    // What the log holds is named.
    let named: HashSet<i64> = timeline.quests.iter().map(|q| q.id).collect();
    assert!(logs.iter().flatten().all(|q| named.contains(q)));
}

/// Flights go from a flight path to another the character knows, and the character walks to
/// where each step is done: the moves before a step end there.
#[test]
fn the_character_moves_to_each_step() {
    let (guide, _, timeline) = setup();
    let nodes: Vec<[f64; 3]> = timeline.nodes.iter().map(|n| n.at).collect();
    let near = |a: [f64; 3], b: [f64; 3]| a[0] as i64 == b[0] as i64 && (a[1] - b[1]).hypot(a[2] - b[2]) < 30.0;
    let flights: Vec<_> = timeline.legs.iter().filter(|l| l.mode == LegMode::Flight).collect();
    assert!(!flights.is_empty(), "a guide to 25 flies");
    for f in &flights {
        assert!(
            nodes.iter().any(|&n| near(n, f.from)) && nodes.iter().any(|&n| near(n, f.to)),
            "{f:?}"
        );
        assert!(f.end > f.start, "{f:?}");
    }
    // One move after the other.
    for w in timeline.legs.windows(2) {
        assert!(w[1].start >= w[0].end - 0.05, "{:?} then {:?}", w[0], w[1]);
    }
    // Where the character is when a quest step is done: the last move before it ends there.
    let mut checked = 0;
    for (k, (s, t)) in guide.route.steps.iter().zip(&timeline.steps).enumerate() {
        let quest = matches!(s.kind, StepKind::Accept | StepKind::TurnIn);
        let (Some(at), true) = (s.world, quest) else { continue };
        let Some(last) = timeline.legs.iter().rev().find(|l| l.end <= t.end + 0.05) else {
            continue;
        };
        assert!(
            near(last.to, at),
            "step {k} ({}) at {at:?}, the character at {:?}",
            s.text,
            last.to
        );
        checked += 1;
    }
    assert!(checked > 20);
}

/// The walks go around the terrain: from where they start to where they end, on their
/// continent, never shorter than the straight line.
#[test]
fn walks_go_around_the_terrain() {
    let (_, _, timeline) = setup();
    let mut bent = 0;
    for leg in timeline
        .legs
        .iter()
        .filter(|l| matches!(l.mode, LegMode::Walk | LegMode::Mount))
    {
        if leg.via.is_empty() {
            continue;
        }
        bent += 1;
        let mut points = vec![[leg.from[1], leg.from[2]]];
        points.extend(leg.via.iter().map(|p| [f64::from(p[0]), f64::from(p[1])]));
        points.push([leg.to[1], leg.to[2]]);
        let length: f64 = points
            .windows(2)
            .map(|w| (w[0][0] - w[1][0]).hypot(w[0][1] - w[1][1]))
            .sum();
        let straight = (leg.from[1] - leg.to[1]).hypot(leg.from[2] - leg.to[2]);
        assert!(length >= straight - 0.5, "{length} < {straight}");
    }
    assert!(bent > 0, "some walks go around the terrain");
}

/// The walk written in a step for the addon is the one the app draws: every step with a way
/// around the terrain has it from the replay's leg, and no walk the app draws is lost.
#[test]
fn the_addon_follows_the_walks_the_app_draws() {
    let (guide, _, timeline) = setup();
    let steps = &guide.route.steps;
    let mut walks = 0;
    for (k, (step, shown)) in steps.iter().zip(&timeline.steps).enumerate() {
        let Some(at) = step.world else {
            assert!(step.path.is_empty(), "step {k}: a walk to a step without a place");
            continue;
        };
        let leg = timeline.legs.iter().rev().find(|l| {
            matches!(l.mode, LegMode::Walk | LegMode::Mount)
                && !l.via.is_empty()
                && l.end > shown.start - 0.05
                && l.end <= shown.end + 0.05
                && (l.to[1] - at[1]).abs() < 1.0
                && (l.to[2] - at[2]).abs() < 1.0
        });
        match leg {
            Some(leg) => {
                walks += 1;
                assert_eq!(step.path.len(), leg.via.len() + 1, "step {k}");
                assert!((step.path[0][0] - leg.from[1]).abs() <= 0.5 && (step.path[0][1] - leg.from[2]).abs() <= 0.5);
                for (p, v) in step.path[1..].iter().zip(&leg.via) {
                    assert!(
                        (p[0] - f64::from(v[0])).abs() <= 0.5 && (p[1] - f64::from(v[1])).abs() <= 0.5,
                        "step {k}"
                    );
                }
            }
            None => assert!(step.path.is_empty(), "step {k}: a walk the app does not draw"),
        }
    }
    assert!(walks > 10, "the guide has walks around the terrain: {walks}");
}

/// A character starting at level 1 has one place to begin: nothing to choose from.
#[test]
fn a_new_character_has_no_start_to_choose() {
    let conn = database();
    let preview = job::preview_starts(
        &conn,
        &overrides(),
        &request(),
        &|_| {},
        &job::Interrupt::default(),
        None,
    )
    .unwrap();
    assert!(preview.options.len() < 2, "{} starts", preview.options.len());
}

/// Below level 10 the first quests are in one place: no start is tried, whatever the zones.
#[test]
fn no_start_is_compared_before_level_10() {
    let conn = database();
    let mut req = request();
    req.from_level = 9;
    let preview = job::preview_starts(&conn, &overrides(), &req, &|_| {}, &job::Interrupt::default(), None).unwrap();
    assert_eq!((preview.tried, preview.options.len()), (0, 0));
}

/// A quest of the log with nothing done is left aside (planned as any other); one done or begun
/// is held, to be turned in or finished.
#[test]
fn log_quests_with_nothing_done_are_not_held() {
    use fg_route::job::{LogObjective, LogQuest, StartState};
    let conn = database();
    let base = job::prepare(&conn, &overrides(), &request(), &|_| {}).unwrap();
    let ids: Vec<i64> = base.model.quests.iter().take(3).map(|q| q.id).collect();
    let mut req = request();
    req.start = Some(StartState {
        name: "Test-Realm".into(),
        race: 2,
        class: "WARLOCK".into(),
        faction: "Horde".into(),
        level: 5,
        xp_max: 1000,
        log: vec![
            LogQuest {
                id: ids[0],
                complete: false,
                objectives: vec![],
            },
            LogQuest {
                id: ids[1],
                complete: false,
                objectives: vec![LogObjective {
                    text: "x".into(),
                    done: 2,
                    need: 8,
                    finished: false,
                }],
            },
            LogQuest {
                id: ids[2],
                complete: true,
                objectives: vec![],
            },
        ],
        ..StartState::default()
    });
    let prepared = job::prepare(&conn, &overrides(), &req, &|_| {}).unwrap();
    let held: Vec<i64> = prepared
        .model
        .initial
        .accepted
        .iter()
        .map(|&i| prepared.model.quests[i].id)
        .collect();
    assert!(!held.contains(&ids[0]), "nothing done: {held:?}");
    assert!(held.contains(&ids[1]) && held.contains(&ids[2]), "{held:?}");
    // All three stay in the log, to be abandoned or not by the guide.
    assert_eq!(prepared.model.initial.log.len(), 3);
}
