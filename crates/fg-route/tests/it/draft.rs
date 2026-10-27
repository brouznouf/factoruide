//! A guide edited on its own page, on the Classic database: the draft replayed in the
//! background (a newer replay makes the older give up), and what the page offers to add at a
//! moment of it.

use crate::common::{database, repo};
use fg_route::job::{self, Editor, Overrides, PlanOutcome, PlanRequest, Requests, RouteStep};
use fg_route::model::Names;
use fg_route::xp::Edition;
use std::sync::OnceLock;

fn overrides() -> Overrides {
    Overrides::load_edition(&repo().join("overrides"), Edition::Classic).unwrap()
}

fn request() -> PlanRequest {
    PlanRequest {
        race: "orc".into(),
        class: "warlock".into(),
        from_level: 1,
        to_level: 10,
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
        mate_professions: vec![],
        start: None,
    }
}

/// An orc warlock guide to 10 (its first routes, no optimization), its editor and names.
fn setup() -> &'static (PlanOutcome, Editor, Names) {
    static SETUP: OnceLock<(PlanOutcome, Editor, Names)> = OnceLock::new();
    SETUP.get_or_init(|| {
        let conn = database();
        let interrupt = job::Interrupt::default();
        interrupt.stop();
        let guide = job::plan_interruptible(&conn, &overrides(), &request(), &|_| {}, &interrupt, None).unwrap();
        let editor = Editor::open(&conn, &overrides(), &request(), None).unwrap();
        (guide, editor, Names::load(&conn, "enUS").unwrap())
    })
}

/// A quest the guide takes, does (an objective step, done) and turns in, not first: its id, and
/// the indexes of its accept, that objective step (and the objective) and its turn-in.
fn a_quest(steps: &[RouteStep], editor: &Editor, names: &Names) -> (i64, usize, (usize, u8), usize) {
    let report = editor.simulate(steps, names);
    steps
        .iter()
        .enumerate()
        .find_map(|(a, s)| {
            let RouteStep::Accept { quest } = *s else { return None };
            let objective = (a + 1..steps.len()).find_map(|k| match steps[k] {
                RouteStep::Objective {
                    quest: q, objective, ..
                } if q == quest && report.steps[k].status == "done" => Some((k, objective)),
                _ => None,
            })?;
            let turn_in = steps.iter().position(|s| *s == RouteStep::TurnIn { quest })?;
            (a > 0 && objective.0 < turn_in && report.steps[a].status == "done")
                .then_some((quest, a, objective, turn_in))
        })
        .expect("a quest taken, done and turned in")
}

/// One line and one step of timeline per step of the draft, the report's.
#[test]
fn the_draft_is_written_a_line_per_step() {
    let (guide, editor, names) = setup();
    let requests = Requests::default();
    let view = editor.view(&guide.plan, names, &requests.next()).unwrap();
    assert_eq!(view.lines.len(), guide.plan.len());
    assert_eq!(view.timeline.steps.len(), guide.plan.len());
    assert_eq!(view.report.steps.len(), guide.plan.len());
    for (k, (line, r)) in view.lines.iter().zip(&view.report.steps).enumerate() {
        // Done within its step (a flight's line when it is taken, the hearthstone's when cast).
        if r.status == "done" {
            assert!(
                line.time >= r.start - 1.0 && line.time <= r.end + 1.0,
                "step {k}: {} in {}..{}",
                line.time,
                r.start,
                r.end
            );
        }
    }
}

/// A replay asked for before a newer one gives up; the newer one is done.
#[test]
fn a_newer_request_cancels_an_older_one() {
    let (guide, editor, names) = setup();
    let requests = Requests::default();
    let older = requests.next();
    let newer = requests.next();
    assert!(older.superseded() && !newer.superseded());
    assert!(editor.view(&guide.plan, names, &older).is_none());
    assert!(editor.suggest(&guide.plan, Some(3), names, &older).is_none());
    assert!(editor.view(&guide.plan, names, &newer).is_some());
}

/// A quest the draft no longer takes is offered where the guide took it, with its giver; the
/// quests the draft takes are not.
#[test]
fn a_quest_left_out_is_offered_where_it_could_be_taken() {
    let (guide, editor, names) = setup();
    let (quest, accept, ..) = a_quest(&guide.plan, editor, names);
    let draft: Vec<RouteStep> = guide
        .plan
        .iter()
        .filter(|s| !matches!(s, RouteStep::Accept { quest: q } | RouteStep::TurnIn { quest: q } | RouteStep::Objective { quest: q, .. } if *q == quest))
        .cloned()
        .collect();
    let requests = Requests::default();
    let offered = editor
        .suggest(&draft, Some(accept - 1), names, &requests.next())
        .unwrap();
    let it = offered
        .untaken
        .iter()
        .find(|q| q.quest == quest)
        .expect("the quest left out is offered");
    assert!(!it.places.is_empty());
    let taken: Vec<i64> = draft
        .iter()
        .filter_map(|s| match s {
            RouteStep::Accept { quest } => Some(*quest),
            _ => None,
        })
        .collect();
    assert!(offered.untaken.iter().all(|q| !taken.contains(&q.quest)));
}

/// An objective the draft no longer does is shown missing once its quest is taken, with its
/// places; a turn-in the draft no longer does is shown missing once the objectives are done.
#[test]
fn missing_objectives_and_turn_ins_are_shown() {
    let (guide, editor, names) = setup();
    let (quest, accept, (objective_step, objective), turn_in) = a_quest(&guide.plan, editor, names);
    let requests = Requests::default();

    let mut without_objective = guide.plan.clone();
    without_objective.remove(objective_step);
    let offered = editor
        .suggest(&without_objective, Some(accept), names, &requests.next())
        .unwrap();
    let missing = offered
        .objectives
        .iter()
        .find(|o| o.quest == quest && o.objective == objective)
        .expect("the objective left out is missing");
    assert!(!missing.places.is_empty());

    let mut without_turn_in = guide.plan.clone();
    without_turn_in.remove(turn_in);
    let offered = editor
        .suggest(&without_turn_in, Some(turn_in - 1), names, &requests.next())
        .unwrap();
    let missing = offered
        .turn_ins
        .iter()
        .find(|q| q.quest == quest)
        .expect("the turn-in left out is missing");
    assert!(!missing.places.is_empty());
    // With its turn-in, it is not.
    let offered = editor
        .suggest(&guide.plan, Some(turn_in - 1), names, &requests.next())
        .unwrap();
    assert!(offered.turn_ins.iter().all(|q| q.quest != quest));
}

/// A quest offered after step k, added there, is taken at step k + 1 of the replayed draft.
#[test]
fn an_offered_quest_added_after_a_step_is_taken_there() {
    let (guide, editor, names) = setup();
    let requests = Requests::default();
    let k = (1..guide.plan.len())
        .find(|&k| {
            editor
                .suggest(&guide.plan, Some(k), names, &requests.next())
                .is_some_and(|s| !s.untaken.is_empty())
        })
        .expect("a step after which a quest is offered");
    let offered = editor.suggest(&guide.plan, Some(k), names, &requests.next()).unwrap();
    let quest = offered.untaken[0].quest;
    let mut draft = guide.plan.clone();
    draft.insert(k + 1, RouteStep::Accept { quest });
    let view = editor.view(&draft, names, &requests.next()).unwrap();
    assert_eq!(
        view.report.steps[k + 1].status,
        "done",
        "{:?}",
        view.report.steps[k + 1]
    );
    assert_eq!(view.lines[k + 1].quest, Some(quest));
    assert_eq!(view.lines[k + 1].kind.as_str(), "accept");
}

/// A dungeon quest taken whose dungeon is no longer run (and that is not turned in) is an
/// error of its accept, which the draft cannot be saved with; the dungeon's objectives are then
/// offered at its entrance.
#[test]
fn a_dungeon_quest_without_its_run_is_an_error() {
    let conn = database();
    let mut request = request();
    (request.race, request.class, request.to_level) = ("human".into(), "warrior".into(), 25);
    request.params = Some(
        overrides()
            .params
            .with_overrides(&["progression=cautious".to_owned(), "forced_dungeons=[1581]".to_owned()])
            .unwrap(),
    );
    let interrupt = job::Interrupt::default();
    interrupt.stop();
    let guide = job::plan_interruptible(&conn, &overrides(), &request, &|_| {}, &interrupt, None).unwrap();
    let editor = Editor::open(&conn, &overrides(), &request, None).unwrap();
    let names = Names::load(&conn, "enUS").unwrap();
    assert_eq!(editor.simulate(&guide.plan, &names).errors, 0, "the guide as planned");

    let run = (guide.plan.iter())
        .position(|s| matches!(s, RouteStep::Dungeon { dungeon: 1581 }))
        .expect("the forced run");
    let mut draft = guide.plan;
    draft.remove(run);
    // Its turn-ins, which cannot be done without the run, go too.
    let report = editor.simulate(&draft, &names);
    let draft: Vec<RouteStep> = (draft.iter().zip(&report.steps))
        .filter(|(s, r)| !(matches!(s, RouteStep::TurnIn { .. }) && r.status == "error"))
        .map(|(s, _)| s.clone())
        .collect();
    let report = editor.simulate(&draft, &names);
    let wrong: Vec<usize> = (report.steps.iter().enumerate())
        .filter(|(_, r)| r.reason == Some("dungeon quest taken but its dungeon is not run"))
        .map(|(k, _)| k)
        .collect();
    assert!(
        !wrong.is_empty(),
        "{:?}",
        report.steps.iter().filter_map(|r| r.reason).collect::<Vec<_>>()
    );
    assert!(wrong.iter().all(|&k| matches!(draft[k], RouteStep::Accept { .. })));
    assert!(report.errors >= wrong.len());
    assert!(
        editor.save(&draft, vec![]).is_err(),
        "saved with a dungeon quest left undone"
    );

    // After that accept, its dungeon objectives are offered at the dungeon's entrance.
    let RouteStep::Accept { quest } = draft[wrong[0]] else {
        unreachable!()
    };
    let offered = editor
        .suggest(&draft, Some(wrong[0]), &names, &Requests::default().next())
        .unwrap();
    let inside = (offered.objectives.iter())
        .find(|o| o.quest == quest)
        .expect("its dungeon objective is offered");
    assert_eq!(inside.dungeon.as_ref().map(|d| d.0), Some(1581));
    assert_eq!(inside.places.len(), 1, "the entrance");
}
