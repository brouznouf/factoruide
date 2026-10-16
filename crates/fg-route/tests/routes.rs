//! Planner behavior on the Classic database: reproductions of the problems met on real guides.

use fg_route::job::{self, Bind, LogObjective, LogQuest, MapPos, Overrides, PlanRequest, Prepared, StartState};
use fg_route::model::{CommonLoot, EntityKind, Guard, Initial, Objective, Spots};
use fg_route::params::ReputationGoal;
use fg_route::plan::{Edit, Event, Kind, Outcome, Planner, Stop};
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

fn prepare(race: &str, class: &str, to_level: i64, params: &[&str]) -> Prepared {
    prepare_from(race, class, to_level, params, None)
}

/// The same for a character met in game (`start`).
fn prepare_from(race: &str, class: &str, to_level: i64, params: &[&str], start: Option<StartState>) -> Prepared {
    prepare_group(race, class, to_level, params, start, &[])
}

/// The same leveling with other players (`group`: their classes).
fn prepare_group(
    race: &str,
    class: &str,
    to_level: i64,
    params: &[&str],
    start: Option<StartState>,
    group: &[&str],
) -> Prepared {
    prepare_request(&PlanRequest {
        group: group.iter().map(|c| (*c).to_owned()).collect(),
        start,
        ..request(race, class, to_level, params)
    })
}

fn overrides() -> Overrides {
    Overrides::load_edition(&repo().join("overrides"), Edition::Classic).unwrap()
}

fn request(race: &str, class: &str, to_level: i64, params: &[&str]) -> PlanRequest {
    let params: Vec<String> = params.iter().map(|p| (*p).to_owned()).collect();
    PlanRequest {
        race: race.into(),
        class: class.into(),
        from_level: 1,
        to_level,
        name: None,
        params: Some(overrides().params.with_overrides(&params).unwrap()),
        required_class_quests: None,
        professions: vec![],
        locale: None,
        group: vec![],
        start: None,
    }
}

fn prepare_request(request: &PlanRequest) -> Prepared {
    job::prepare(&database(), &overrides(), request, &|_| {}).unwrap()
}

/// Orc warlock to 30, cautious: shared by the tests below.
fn orc_warlock() -> &'static Prepared {
    static PREPARED: OnceLock<Prepared> = OnceLock::new();
    PREPARED.get_or_init(|| prepare("orc", "warlock", 30, &["progression=cautious"]))
}

fn human_warrior() -> &'static Prepared {
    static PREPARED: OnceLock<Prepared> = OnceLock::new();
    PREPARED.get_or_init(|| prepare("human", "warrior", 30, &["progression=cautious"]))
}

fn planner(p: &Prepared) -> Planner<'_> {
    Planner::new(&p.model, &p.world, &p.params, &p.profile)
}

fn quest(p: &Prepared, id: i64) -> Option<u32> {
    p.model.index.get(&id).map(|&i| i as u32)
}

/// Level 1 human warrior with only `n` quests of its own: taken and turned in at the start NPC,
/// nothing to do, no XP (each test sets what it checks). Nothing done on the way.
fn quests_at_start(n: usize) -> Prepared {
    quests_at_start_as("human", "warrior", n)
}

/// `quests_at_start` for another race and class.
fn quests_at_start_as(race: &str, class: &str, n: usize) -> Prepared {
    let mut p = prepare(race, class, 2, &[]);
    let here = p.model.start.clone();
    let template = p.model.quests[0].clone();
    p.model.quests = (0..n)
        .map(|i| {
            let mut q = template.clone();
            (q.id, q.name, q.level, q.min_level, q.xp, q.xp_observed) =
                (900_000 + i as i64, format!("quest {i}"), 1, 1, 0, true);
            (q.starts, q.ends) = (vec![here.clone()], vec![here.clone()]);
            q.objectives.clear();
            (q.pre_all, q.pre_any, q.exclusive, q.breadcrumb_for) = (vec![], vec![], vec![], None);
            (q.start_kills, q.start_uses, q.mandatory, q.skill, q.power) = (0.0, 0.0, false, None, None);
            (q.start_guard, q.end_guard) = (Guard::default(), Guard::default());
            (q.reputation, q.zone_forced) = (vec![], false);
            q
        })
        .collect();
    p.model.index = p.model.quests.iter().enumerate().map(|(i, q)| (q.id, i)).collect();
    (p.model.chain_top, p.model.unlocks, p.model.rewards) = (vec![1; n], vec![0.0; n], vec![(vec![], vec![]); n]);
    p.model.initial = Initial::default();
    p.model.power = None;
    p.model.trainers.clear();
    p.model.explore.clear();
    p.model.explore_by_zone.clear();
    (p.params.camp_radius, p.params.farm_on_way, p.params.flight_learn_radius) = (0.0, false, 0.0);
    p
}

/// An objective's mobs are killed once, at its stop.
#[test]
fn objective_finished_on_the_way_is_not_done_twice() {
    let p = orc_warlock();
    let planner = planner(p);
    let boars = quest(p, 788).unwrap();
    let accept = Stop::quest(boars, Kind::Accept);
    let objective = Stop::quest(boars, Kind::Objective(0));
    let (_, once) = planner.simulate_full(&[accept, objective], None).unwrap();
    let kills = p.model.quests[boars as usize].objectives[0].kills;
    let one_kill = p
        .params
        .kill_time(1, 0.0, p.model.quests[boars as usize].objectives[0].mob_level);
    assert!(
        once.fighting < kills * one_kill * 1.6,
        "{} s of fighting for {kills} kills of {one_kill:.0} s",
        once.fighting
    );
}

/// Elite camps need a few more levels than a lone elite, not a multiple of the level.
#[test]
fn elite_camp_needs_a_few_levels_more() {
    let p = human_warrior();
    let planner = planner(p);
    let tharilzun = quest(p, 19).unwrap();
    let q = &p.model.quests[tharilzun as usize];
    let (k, o) = q.objectives.iter().enumerate().find(|(_, o)| o.elite).unwrap();
    let need = planner.combat().need(q, o);
    assert!(
        need <= f64::from(o.mob_level as i32) + 2.0 + 2.5,
        "Tharil'zun needs {need}"
    );
    assert!(planner.combat().level(q, k as u8, 0.0) <= 28);
}

/// An item only another quest's turn-in hands (the bound chapters of The Green Hills of
/// Stranglethorn) is got there once that quest is turned in: the book is planned, and its
/// chapter cannot be counted before.
#[test]
fn item_handed_by_another_quest_waits_for_its_turn_in() {
    let p = prepare("orc", "warrior", 45, &[]);
    let planner = planner(&p);
    let book = quest(&p, 338).expect("The Green Hills of Stranglethorn is planned");
    let chapter = quest(&p, 342).unwrap();
    let k = p.model.quests[book as usize]
        .objectives
        .iter()
        .position(|o| o.handed_by == [342])
        .expect("an objective handed by Chapter IV") as u8;
    // Welcome to the Jungle comes first.
    let jungle = quest(&p, 583).unwrap();
    let take = [
        Stop::quest(jungle, Kind::Accept),
        Stop::quest(jungle, Kind::TurnIn),
        Stop::quest(book, Kind::Accept),
        Stop::quest(chapter, Kind::Accept),
    ];
    let pages: Vec<Stop> = (0..p.model.quests[chapter as usize].objectives.len() as u8)
        .map(|j| Stop::quest(chapter, Kind::Objective(j)))
        .collect();
    let bound = Stop::quest(book, Kind::Objective(k));
    let early: Vec<Stop> = take.iter().copied().chain([bound]).collect();
    assert!(
        planner.simulate_full(&early, None).is_none(),
        "chapter counted before it is bound"
    );
    let route: Vec<Stop> = take
        .iter()
        .copied()
        .chain(pages)
        .chain([Stop::quest(chapter, Kind::TurnIn), bound])
        .collect();
    assert!(
        planner.simulate_full(&route, None).is_some(),
        "{:?}",
        planner.diagnose(&route)
    );
}

/// The item starting a quest drops from a mob the character must be able to take on.
#[test]
fn item_start_waits_for_its_mob() {
    let p = orc_warlock();
    let owatanka = quest(p, 884).unwrap();
    assert!(p.model.quests[owatanka as usize].min_level >= 22);
}

/// The accept step of a quest started by an item a mob drops names that mob, and gives it to the
/// addon's target macro: the place alone shows nothing to loot.
#[test]
fn item_start_names_its_mob() {
    let p = orc_warlock();
    let owatanka = &p.model.quests[quest(p, 884).unwrap() as usize];
    let Some((mob, name)) = owatanka.start_mob.clone() else {
        panic!("Owatanka's Tailspike drops from a mob")
    };
    let planner = planner(p);
    let route = planner.construct(|_| {});
    let guide = fg_route::export::build(&planner, &p.profile, &route);
    let looted: Vec<_> = guide
        .steps
        .iter()
        .filter(|s| s.kind == fg_route::export::StepKind::Accept)
        .filter_map(|s| Some((s, p.model.quests[p.model.index[&s.quest?]].start_mob.clone()?)))
        .collect();
    assert!(!looted.is_empty(), "a quest started by a dropped item in the route");
    for (step, (id, mob)) in looted {
        assert!(step.text.starts_with(&format!("Kill {mob} and loot ")), "{}", step.text);
        assert_eq!((&step.mobs, &step.mob_ids), (&vec![mob.clone()], &vec![id]));
    }
    // Mobs living in several places: the others, for when nothing is at the step's.
    assert!(
        guide
            .steps
            .iter()
            .any(|s| s.kind == fg_route::export::StepKind::Objective && !s.spots.is_empty()),
        "objectives with other places"
    );
    assert!(mob > 0 && !name.is_empty());
}

/// A quest started by an item dropping less than 10% of the time is luck, not a plan.
#[test]
fn rare_drop_start_is_left_out() {
    assert!(
        quest(human_warrior(), 136).is_none(),
        "Captain Sanders' Hidden Treasure"
    );
}

/// A reputation goal brings in the quests giving that reputation, even worth nothing else (no
/// XP); a quest of another faction stays out, as every one does without goals. The character
/// starts with its race's points (Stormwind: a human's 4000).
#[test]
fn reputation_goal_takes_its_factions_quests() {
    let goal = r#"reputation_goals=[{"faction":72,"target":9000}]"#;
    assert_eq!(
        prepare("human", "warrior", 2, &[goal]).model.initial.reputation,
        vec![4000.0]
    );
    let taken = |goals: Vec<ReputationGoal>| {
        let mut p = quests_at_start(2);
        p.model.quests[0].reputation = vec![(72, 250.0)];
        p.model.quests[1].reputation = vec![(69, 250.0)];
        p.model.initial.reputation = vec![4000.0; goals.len()];
        p.params.reputation_goals = goals;
        let planner = planner(&p);
        let route = planner.construct(|_| {});
        let turned = |i| route.iter().any(|s| s.index == i && s.kind == Kind::TurnIn);
        ((turned(0), turned(1)), planner.reputation_reached(&route))
    };
    assert_eq!(
        taken(vec![ReputationGoal {
            faction: 72,
            target: 9000
        }]),
        ((true, false), vec![4250.0])
    );
    assert_eq!(taken(vec![]), ((false, false), vec![]));
}

/// A forced zone's quests are all required: those filed under it or a subzone (Shadowglen's in
/// Teldrassil), class quests given there (the night elf warrior's Simple Sigil), not those
/// taken after the target level nor another zone's.
#[test]
fn forced_zone_quests_are_required() {
    let p = prepare("nightelf", "warrior", 8, &["forced_zones=[141]"]);
    let forced = |id| {
        quest(&p, id).is_some_and(|i| {
            let q = &p.model.quests[i as usize];
            q.zone_forced && q.mandatory
        })
    };
    assert!(forced(456) && forced(916), "Shadowglen's");
    assert!(forced(2459), "Teldrassil's");
    assert!(forced(3116), "a class quest given in Shadowglen");
    assert!(!forced(941), "taken from level 9");
    assert!(!forced(983), "Darkshore's");
    let none = prepare("nightelf", "warrior", 8, &[]);
    assert!(none.model.quests.iter().all(|q| !q.zone_forced));
}

/// The guide goes on past the target level until the quests of the forced zones are done: a
/// quest after the level is reached counts, where a class quest there does not.
#[test]
fn forced_zone_goes_past_the_target_level() {
    let missing = |zone: bool| {
        let mut p = quests_at_start(2);
        p.model.quests[0].xp = 10_000;
        p.model.quests[1].mandatory = true;
        p.model.quests[1].zone_forced = zone;
        let planner = planner(&p);
        let route: Vec<Stop> = [
            (0, Kind::Accept),
            (0, Kind::TurnIn),
            (1, Kind::Accept),
            (1, Kind::TurnIn),
        ]
        .into_iter()
        .map(|(i, k)| Stop::quest(i, k))
        .collect();
        let (_, b) = planner.simulate_full(&route, None).unwrap();
        (b.missing_class_quests, b.missing_zone_quests)
    };
    assert_eq!(missing(true), (0, 0));
    assert_eq!(missing(false), (1, 0));
}

/// A reputation goal not met at the target level keeps the guide going: the reputation of a
/// quest done after the level is reached counts, as long as the goal is not met.
#[test]
fn reputation_goal_goes_past_the_target_level() {
    let reached = |target: i64| {
        let mut p = quests_at_start(3);
        p.model.quests[0].xp = 10_000;
        p.model.quests[1].reputation = vec![(72, 250.0)];
        p.model.quests[2].reputation = vec![(72, 250.0)];
        p.model.initial.reputation = vec![4000.0];
        p.params.reputation_goals = vec![ReputationGoal { faction: 72, target }];
        let planner = planner(&p);
        let route: Vec<Stop> = (0..3)
            .flat_map(|i| [Stop::quest(i, Kind::Accept), Stop::quest(i, Kind::TurnIn)])
            .collect();
        planner.reputation_reached(&route)[0]
    };
    assert_eq!(reached(4100), 4250.0, "the guide ends once the goal is met");
    assert_eq!(reached(9000), 4500.0, "every quest after the level counts towards it");
}

/// Creatures acted on without being killed (`talk` objectives: healed, talked to) are visited,
/// not fought: no kills, one use each among their places, with the step's own text (Forever's
/// Rediscovering the Light, GitHub #17). Here Kobold Camp Cleanup's vermin, made such a target
/// in a copy of the objectives that shadows the database's.
#[test]
fn talked_to_creatures_are_not_killed() {
    let conn = database();
    conn.execute_batch(
        "CREATE TEMP TABLE m_quest_objective AS SELECT * FROM main.m_quest_objective;
         UPDATE temp.m_quest_objective SET type = 'talk', text = 'Heal Kobold Vermin' WHERE quest_id = 7;",
    )
    .unwrap();
    let p = job::prepare(&conn, &overrides(), &request("human", "warrior", 6, &[]), &|_| {}).unwrap();
    let o = &p.model.quests[quest(&p, 7).unwrap() as usize].objectives;
    assert_eq!(o.len(), 1);
    assert_eq!((o[0].kills, o[0].uses, o[0].count), (0.0, 10.0, 10.0));
    assert_eq!((o[0].loc.kind, o[0].loc.id), (EntityKind::Npc, 6));
    assert_eq!(o[0].text, "Heal Kobold Vermin");
}

/// The warlock's imp is taken first: it makes every fight that follows easier.
#[test]
fn pet_quest_comes_early() {
    let p = orc_warlock();
    let planner = planner(p);
    let route = planner.construct(|_| {});
    let imp = quest(p, 1485).unwrap();
    let at = route
        .iter()
        .position(|s| s.index == imp && s.kind == Kind::TurnIn)
        .unwrap();
    assert!(at < 25, "imp turned in at stop {at}");
}

/// Constructed routes stay valid when a level reached makes quests of a full log hopeless
/// (gone gray, or their dungeon outleveled): the next accept is judged on the log once they are
/// given up, and the construction's replay (`settle`) goes where the route goes next, as the
/// simulation does, so that it reaches the same levels. With these construction weights the
/// route ended with "quest log full" at an accept (w010: given up one stop too late; w142: the
/// replay a level ahead, it saw room and repaired nothing).
#[test]
fn constructed_route_keeps_its_log_in_bounds() {
    for weights in [
        [4.502, 2.569, 1.073, 0.475, 0.58, 0.547],
        [2.899, 5.096, 1.277, 0.732, 0.04, 0.352],
    ] {
        let names = [
            "zone_stay",
            "continent_stay",
            "dungeon_quest_weight",
            "fit_penalty",
            "fit_above_penalty",
            "efficiency_power",
        ];
        let mut params: Vec<String> = names.iter().zip(weights).map(|(n, w)| format!("{n}={w}")).collect();
        params.extend(["progression=cautious".into(), "construction_random=false".into()]);
        let params: Vec<&str> = params.iter().map(String::as_str).collect();
        let p = prepare("orc", "warlock", 60, &params);
        let planner = planner(&p);
        let route = planner.construct(|_| {});
        assert!(
            planner.simulate(&route, None).is_some(),
            "{weights:?}: {:?}",
            planner.diagnose(&route)
        );
    }
}

/// A stopped search (the user stopped the generation) ends at once, however long its budget,
/// and hands back a valid route.
#[test]
fn stopped_search_ends_at_once() {
    let p = orc_warlock();
    let planner = planner(p);
    let route = planner.construct(|_| {});
    let begun = std::time::Instant::now();
    let improved = planner.improve_while(
        route,
        fg_route::plan::Effort::Time(std::time::Duration::from_secs(600)),
        || false,
        |_| {},
        |_, _, _| {},
    );
    assert!(
        begun.elapsed() < std::time::Duration::from_secs(30),
        "took {:?}",
        begun.elapsed()
    );
    assert!(
        planner.simulate(&improved, None).is_some(),
        "{:?}",
        planner.diagnose(&improved)
    );
}

/// An aborted construction (the generation stopped or cancelled) gives up at once with an
/// unfinished route; without abort it reaches the target level.
#[test]
fn aborted_construction_gives_up() {
    let p = orc_warlock();
    let abort = || true;
    let given_up = planner(p).abortable(&abort).construct(|_| {});
    let built = planner(p).construct(|_| {});
    assert!(
        given_up.len() < built.len() / 10,
        "{} stops of {}",
        given_up.len(),
        built.len()
    );
}

/// A job stopped before its optimization (a huge effort) still ends with a guide: the first
/// routes are built and the best one is kept.
#[test]
fn stopped_job_keeps_a_guide() {
    let interrupt = job::Interrupt::default();
    interrupt.stop();
    let request = request("orc", "warlock", 12, &["effort=100"]);
    let begun = std::time::Instant::now();
    let outcome = job::plan_interruptible(&database(), &overrides(), &request, &|_| {}, &interrupt, None).unwrap();
    assert!(
        begun.elapsed() < std::time::Duration::from_secs(300),
        "took {:?}",
        begun.elapsed()
    );
    assert!(outcome.route.total_time.is_finite());
    assert_eq!(outcome.route.to_level, 12);
}

/// A job given the walking memory saves the paths it searched with its outcome (kept through
/// JSON, as in the app's files and the shared guides), and computing it again searches none.
#[test]
fn job_saves_its_walks() {
    let walks = std::sync::Arc::new(fg_route::terrain::Walks::default());
    let interrupt = job::Interrupt::default();
    interrupt.stop();
    let request = request("orc", "warlock", 12, &["effort=100"]);
    let first =
        job::plan_interruptible(&database(), &overrides(), &request, &|_| {}, &interrupt, Some(&walks)).unwrap();
    let memory = first.walks.clone().expect("a memory with the outcome");
    assert!(!memory.is_empty());
    let saved = job::PlanOutcome::from_json(&serde_json::to_string(&first).unwrap()).unwrap();
    assert_eq!(saved.walks.as_ref(), Some(&memory));
    let known = walks.len();
    let again =
        job::plan_interruptible(&database(), &overrides(), &request, &|_| {}, &interrupt, Some(&walks)).unwrap();
    assert_eq!(walks.len(), known, "paths searched again");
    assert_eq!(again.walks.map(|m| m.len()), Some(memory.len()));
}

/// A search that panics ends the job with its panic instead of hanging it: the thread reporting
/// the progress stops with the searches (the app then shows the error).
#[test]
fn panicking_search_ends_the_job() {
    let (sent, received) = std::sync::mpsc::channel();
    std::thread::spawn(move || {
        let request = request("orc", "warlock", 12, &["threads=1", "effort=0.05"]);
        // The search's first message, from its own thread.
        let progress = |m: &str| assert!(!m.starts_with("start:"), "search failed");
        let interrupt = job::Interrupt::default();
        let outcome = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
            job::plan_interruptible(&database(), &overrides(), &request, &progress, &interrupt, None)
        }));
        let _ = sent.send(outcome.is_err());
    });
    let panicked = received
        .recv_timeout(std::time::Duration::from_secs(300))
        .expect("the job hung after its search panicked");
    assert!(panicked);
}

/// The same request gives the same guide again, whichever search ends or reaches a meeting
/// first: the race, the rounds and the final count tries, not time.
#[test]
fn the_tournament_repeats_its_guide() {
    let request = request(
        "orc",
        "warlock",
        12,
        &[
            "threads=4",
            "candidates=8",
            "final_routes=2",
            "qualify_tries=0.5",
            "round_tries=1",
            "final_tries=3",
            "share_every=50",
        ],
    );
    let interrupt = job::Interrupt::default();
    let plan = || job::plan_interruptible(&database(), &overrides(), &request, &|_| {}, &interrupt, None).unwrap();
    let (first, again) = (plan(), plan());
    assert_eq!(
        serde_json::to_value(&first.plan).unwrap(),
        serde_json::to_value(&again.plan).unwrap()
    );
}

/// A search that panics leaves the meetings: the others do not wait for it, the job ends with
/// the panic.
#[test]
fn panicking_search_does_not_hold_the_meetings() {
    let (sent, received) = std::sync::mpsc::channel();
    std::thread::spawn(move || {
        // A round of two routes, then a final of one route improved by both searches.
        let request = request(
            "orc",
            "warlock",
            12,
            &[
                "threads=2",
                "candidates=2",
                "final_routes=1",
                "round_tries=1",
                "final_tries=20",
                "share_every=1",
            ],
        );
        // The first search of the final, at its first message (the other one waits at a meeting).
        let starts = std::sync::atomic::AtomicUsize::new(0);
        let progress = |m: &str| {
            let final_start = m.starts_with("start:") && starts.fetch_add(1, std::sync::atomic::Ordering::Relaxed) == 1;
            assert!(!final_start, "search failed");
        };
        let interrupt = job::Interrupt::default();
        let outcome = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
            job::plan_interruptible(&database(), &overrides(), &request, &progress, &interrupt, None)
        }));
        let _ = sent.send(outcome.is_err());
    });
    let panicked = received
        .recv_timeout(std::time::Duration::from_secs(300))
        .expect("the job hung after a sharing search panicked");
    assert!(panicked);
}

/// A cancelled job fails with `Cancelled` instead of returning a route (the app saves nothing).
#[test]
fn cancelled_job_returns_no_route() {
    let interrupt = job::Interrupt::default();
    interrupt.cancel();
    let request = request("orc", "warlock", 12, &["effort=100"]);
    let outcome = job::plan_interruptible(&database(), &overrides(), &request, &|_| {}, &interrupt, None);
    let error = outcome.expect_err("a cancelled job has no route");
    assert!(error.downcast_ref::<job::Cancelled>().is_some(), "{error:#}");
}

/// The local search hands back a valid route (cutting the stops after the target level once
/// made the start of a route invalid: a quest it plans later decides what is taken early), and
/// its incremental evaluations match a full replay (quests taken at hand look ahead in the
/// route; a dungeon quest added moves its run).
#[test]
fn improved_route_stays_valid() {
    let p = orc_warlock();
    let planner = planner(p).verified();
    let route = planner.construct(|_| {});
    let improved = planner.improve(route, std::time::Duration::from_secs(2), |_| {}, |_, _, _| {});
    assert!(
        planner.simulate(&improved, None).is_some(),
        "{:?}",
        planner.diagnose(&improved)
    );
}

/// Farming on the way replaces grinding: it gives XP, but never more than its share of each level.
#[test]
fn farm_on_the_way_stays_under_its_cap() {
    let p = human_warrior();
    let planner = planner(p);
    let route = planner.construct(|_| {});
    let (_, spent) = planner.simulate_full(&route, None).unwrap();
    let rules = p.params.edition.rules();
    let levels: i64 = (1..30).map(|l| rules.to_next_level(l)).sum();
    assert!(spent.farm_xp > 0, "nothing farmed on the way");
    assert!(
        spent.farm_xp as f64 <= p.params.farm_cap * levels as f64 * 1.01,
        "{} XP farmed for {levels} XP of levels",
        spent.farm_xp
    );
}

/// Quest objects lying among the mobs of another quest are gathered between the kills: on real
/// quests, doing both together never takes longer than one after the other. Every objective of
/// both quests is done, so that both ways do the same work (an objective done along with
/// another would be extra work).
#[test]
fn gathering_with_kills_never_takes_longer() {
    let p = orc_warlock();
    let alone = p.params.with_overrides(&["gather_overlap=0".to_owned()]).unwrap();
    let (together, apart) = (planner(p), Planner::new(&p.model, &p.world, &alone, &p.profile));
    let quests = &p.model.quests;
    let free = |q: &fg_route::model::Quest| {
        q.pre_all.is_empty()
            && q.pre_any.is_empty()
            && q.level <= 15
            && q.objectives.iter().all(|o| o.dungeon.is_none())
    };
    let mut compared = 0;
    for (a, qa) in quests.iter().enumerate().filter(|(_, q)| free(q)) {
        let Some(ka) = qa.objectives.iter().position(|o| o.kills > 0.0 && o.dungeon.is_none()) else {
            continue;
        };
        for (b, qb) in quests.iter().enumerate().filter(|&(b, q)| b != a && free(q)) {
            let Some(kb) = qb.objectives.iter().position(|o| {
                o.kills <= 0.0
                    && o.uses > 0.0
                    && o.spots.is_some()
                    && o.loc.pos.continent == qa.objectives[ka].loc.pos.continent
                    && o.loc.pos.dist(&qa.objectives[ka].loc.pos) <= p.params.gather_radius
            }) else {
                continue;
            };
            let (a, b) = (a as u32, b as u32);
            let objectives = |q: u32, first: usize| {
                let n = quests[q as usize].objectives.len();
                (0..n).map(move |k| Stop::quest(q, Kind::Objective(((first + k) % n) as u8)))
            };
            let route: Vec<Stop> = [Stop::quest(a, Kind::Accept), Stop::quest(b, Kind::Accept)]
                .into_iter()
                .chain(objectives(a, ka))
                .chain(objectives(b, kb))
                .collect();
            let (Some((_, with)), Some((_, without))) =
                (together.simulate_full(&route, None), apart.simulate_full(&route, None))
            else {
                continue;
            };
            assert!(
                with.fighting <= without.fighting,
                "{} and {}: {:.0} s together, {:.0} s apart",
                qa.name,
                qb.name,
                with.fighting,
                without.fighting
            );
            compared += 1;
        }
    }
    assert!(compared > 0, "no kill and gathering objectives close together");
}

/// Objects lying among the mobs to kill are gathered between the kills, while resting and
/// looking for the next mob: both take less time than one after the other.
#[test]
fn gathering_next_to_kills_overlaps_them() {
    let mut p = quests_at_start(2);
    for (i, kills, uses, dx) in [(0, 6.0, 0.0, 80.0), (1, 0.0, 3.0, 90.0)] {
        let mut loc = p.model.start.clone();
        loc.pos.x += dx;
        let point = (loc.pos.x, loc.pos.y);
        p.model.quests[i].objectives = vec![Objective {
            loc: loc.clone(),
            text: format!("objective {i}"),
            kills,
            mob_level: 1,
            uses,
            extra: 0.0,
            dungeon: None,
            elite: false,
            count: kills + uses,
            mobs: vec![],
            pull: 1.0,
            spawns: 0,
            pack: 1.0,
            spots: Some(Spots {
                continent: loc.pos.continent,
                min: point,
                max: point,
                points: vec![point],
            }),
            guard: Guard::default(),
            alts: vec![],
            after: 0,
            handed_by: vec![],
            stock: None,
        }];
    }
    p.model.initial.accepted = vec![0, 1];
    let route = [Stop::quest(0, Kind::Objective(0)), Stop::quest(1, Kind::Objective(0))];
    let fighting = |p: &Prepared| planner(p).simulate_full(&route, None).unwrap().1.fighting;
    let together = fighting(&p);
    p.params.gather_overlap = 0.0;
    let apart = fighting(&p);
    assert!(together < apart, "{together:.0} s together, {apart:.0} s apart");
}

/// Common loot a quest asks for by the stack (cloth for a donation) comes from the route's kills
/// where it drops: after killing there, only what the bags lack is gathered or bought.
#[test]
fn common_loot_comes_from_the_kills_before() {
    let mut p = quests_at_start(2);
    // Quest 0 kills 100 mobs at the start, quest 1 asks for 20 cloth (a minute each to gather
    // or buy), and a mob there drops one cloth in ten.
    p.model.quests[0].objectives = vec![kills_east_of_start(&p, 0.0, 100.0, 1)];
    let mut cloth = kills_east_of_start(&p, 0.0, 0.0, 1);
    (cloth.count, cloth.extra, cloth.stock) = (20.0, 20.0 * 60.0, Some(0));
    p.model.quests[1].objectives = vec![cloth];
    p.model.loot = CommonLoot {
        items: vec![2589],
        ..CommonLoot::default()
    };
    p.model.loot.by_zone.insert(p.model.start.zone, vec![(0, 0.1)]);
    p.model.initial.accepted = vec![0, 1];
    // The kills' XP must not end the route before the cloth.
    p.profile.to_level = 10;
    let fighting = |route: &[Stop]| planner(&p).simulate_full(route, None).unwrap().1.fighting;
    let (kill, gather) = (Stop::quest(0, Kind::Objective(0)), Stop::quest(1, Kind::Objective(0)));
    let first = fighting(&[gather, kill]);
    let after = fighting(&[kill, gather]);
    assert!(
        (first - after - 10.0 * 60.0).abs() < 1.0,
        "{first:.0} s gathering first, {after:.0} s after the kills"
    );
}

/// Each checkpoint has a step where a character behind the route grinds: at the last place
/// before it where mobs of its level live (to the XP the route has there), else at the
/// checkpoint itself.
#[test]
fn checkpoints_wait_for_their_level() {
    use fg_route::export::StepKind;
    let p = orc_warlock();
    let planner = planner(p);
    let route = planner.construct(|_| {});
    let guide = fg_route::export::build(&planner, &p.profile, &route);
    assert!(!guide.checkpoints.is_empty(), "no checkpoint up to level 30");
    for c in &guide.checkpoints {
        let sync = guide.steps[..=c.step]
            .iter()
            .rev()
            .find(|s| s.kind == StepKind::Grind && s.keep)
            .unwrap();
        let before = sync.level == c.level - 1 && sync.xp.is_some_and(|xp| xp > 0);
        assert!(before || sync.level == c.level, "{sync:?}");
    }
}

/// Only flight masters are flight paths: not the nodes of scripted flights (quests, towers), nor
/// those shown to the other faction only.
#[test]
fn flight_paths_are_flight_masters() {
    let world = &orc_warlock().world;
    let has = |name: &str| world.taxi_nodes.iter().any(|n| n.name.starts_with(name));
    assert!(has("Ratchet") && has("Crossroads") && has("Hammerfall"));
    assert!(!has("Quest Path"), "a shaman quest's flight");
    assert!(!has("Plaguewood Tower"), "an Eastern Plaguelands tower");
    assert!(!has("Northshire Abbey"));
}

/// Zone maps overlap (Durotar's covers Ratchet and the Crossroads): flight paths are placed in
/// their own zone (a city's own map in a city), and towns are named.
#[test]
fn flight_paths_are_in_their_zone() {
    let world = &orc_warlock().world;
    let zone_of = |town: &str| {
        let n = world.taxi_nodes.iter().find(|n| n.name.starts_with(town)).unwrap();
        world.zone_name(n.pos.zone).to_owned()
    };
    assert_eq!(zone_of("Ratchet"), "The Barrens");
    assert_eq!(zone_of("Crossroads"), "The Barrens");
    assert_eq!(zone_of("Orgrimmar"), "Orgrimmar");
    assert_eq!(zone_of("Thunder Bluff"), "Thunder Bluff");
    let ratchet = world.taxi_nodes.iter().find(|n| n.name.starts_with("Ratchet")).unwrap();
    assert_eq!(world.place_name(&ratchet.pos), Some("Ratchet"));
}

/// Marla's Last Wish: Samuel's Remains are looted from Samuel Fipps, then buried at Marla's
/// grave. The guide gets them first; burying them before is not a valid route.
#[test]
fn item_used_by_an_objective_is_got_first() {
    let mut p = prepare("undead", "warrior", 10, &[]);
    let marla = quest(&p, 6395).unwrap();
    let objectives = &p.model.quests[marla as usize].objectives;
    let get = objectives
        .iter()
        .position(|o| o.mobs.iter().any(|(id, _)| *id == 1919))
        .expect("Samuel Fipps killed for his remains");
    let bury = objectives
        .iter()
        .position(|o| o.after & (1 << get) != 0)
        .expect("burying them comes after");
    p.model.initial.accepted = vec![marla as usize];
    let planner = planner(&p);
    let objective = |k: usize| Stop::quest(marla, Kind::Objective(k as u8));
    let turn_in = Stop::quest(marla, Kind::TurnIn);
    assert!(
        planner
            .simulate(&[objective(get), objective(bury), turn_in], None)
            .is_some()
    );
    assert!(
        planner
            .simulate(&[objective(bury), objective(get), turn_in], None)
            .is_none()
    );
}

/// Rut'theran Village lies at the foot of Teldrassil: no walk goes up to Dolanaar nor across the
/// sea to Auberdine, the portal and the boat do. Darnassus is walked to from Dolanaar.
#[test]
fn rut_theran_is_left_by_portal_or_boat() {
    let p = prepare("nightelf", "warrior", 2, &[]);
    let w = &p.world;
    let at = |zone, x, y| w.to_world(zone, x, y).unwrap();
    let (village, dolanaar) = (at(141, 55.5, 90.0), at(141, 55.6, 59.8));
    assert!(w.walk_distance(&village, &dolanaar).is_infinite());
    assert!(w.walk_distance(&village, &at(148, 37.0, 44.0)).is_infinite());
    assert!(w.walk_distance(&dolanaar, &at(1657, 40.0, 50.0)).is_finite());
    let portal = w
        .links
        .iter()
        .find(|l| l.name == "Portal Rut'theran Village - Darnassus")
        .unwrap();
    assert!(w.walk_distance(&village, &portal.from).is_finite());
    assert!(portal.seconds < 30.0, "a portal is taken at once");
}

/// The passages of the Eastern Kingdoms' south (Stormwind, Elwynn) and north (Dun Morogh,
/// Silverpine) do not meet: going from one to the other takes the tram, a flight or a boat, never
/// a walk in a straight line over the mountains.
#[test]
fn no_walk_between_the_south_and_north_of_the_eastern_kingdoms() {
    let p = prepare("gnome", "rogue", 22, &[]);
    let w = &p.world;
    let at = |zone, x, y| w.to_world(zone, x, y).unwrap();
    let stormwind = at(1519, 77.0, 30.3);
    assert!(w.walk_distance(&stormwind, &at(130, 58.2, 69.9)).is_infinite());
    assert!(w.walk_distance(&at(12, 42.0, 65.0), &at(38, 35.0, 46.0)).is_infinite());
    assert!(w.walk_distance(&stormwind, &at(40, 56.0, 47.0)).is_finite());
    assert!(w.walk_distance(&at(1, 47.0, 52.0), &at(38, 35.0, 46.0)).is_finite());
}

/// The mountain skip from Dun Morogh into the Wetlands is a drop: taken that way only, and a step
/// of the guide (it is not on any road).
#[test]
fn mountain_skip_is_one_way_and_a_step_of_the_guide() {
    let mut p = quests_at_start_as("dwarf", "warrior", 1);
    let w = &p.world;
    let at = |zone, x, y| w.to_world(zone, x, y).unwrap();
    let (kharanos, menethil) = (at(1, 47.0, 52.0), at(11, 10.5, 56.0));
    let skip = w.passes.iter().position(|q| q.name.contains("mountain skip")).unwrap();
    assert_eq!(w.walk_shortcuts(&kharanos, &menethil), [skip]);
    assert_eq!(w.walk_shortcuts(&menethil, &kharanos), Vec::<usize>::new());
    assert!(w.walk_distance(&menethil, &kharanos) > w.walk_distance(&kharanos, &menethil));
    let mut o = one_kill_east_of_start(&p, 0.0);
    (o.loc.pos, o.loc.zone) = (menethil, 11);
    p.model.quests[0].objectives = vec![o];
    p.model.initial.accepted = vec![0];
    let mut trace = Vec::new();
    planner(&p)
        .simulate_full(&[Stop::quest(0, Kind::Objective(0))], Some(&mut trace))
        .unwrap();
    assert!(
        trace
            .iter()
            .any(|t| matches!(t.event, Event::Shortcut { pass } if pass == skip))
    );
    // In the guide editor, the shortcut is part of the walk to the step, not a step of its own.
    let edited = planner(&p).explicit().edit(&[Edit::Stop {
        stop: Stop::quest(0, Kind::Objective(0)),
        farm: false,
        spot: None,
    }]);
    assert_eq!(edited.steps[0].outcome, Outcome::Done);
}

/// In the guide editor, grinding up several levels is one step even when it moves from camp to
/// camp (each level at the camp of its mobs).
#[test]
fn editor_grind_over_several_camps_is_one_step() {
    let p = human_warrior();
    let edited = planner(p).explicit().edit(&[Edit::GrindTo { level: 12 }]);
    let grinds = edited
        .trace
        .iter()
        .filter(|t| matches!(t.event, Event::Grind { .. }))
        .count();
    assert!(grinds > 1, "one camp only: nothing checked");
    assert_eq!(edited.steps[0].outcome, Outcome::Done);
}

/// A link kept to some races (Forever's Skyborne portals) is only in their guides.
#[test]
fn link_kept_to_a_race() {
    let mut o = overrides();
    let mut tram = (o.links.iter())
        .find(|l| l.name.starts_with("Deeprun Tram"))
        .unwrap()
        .clone();
    (tram.name, tram.races) = ("Gnomes only".into(), vec!["gnome".into()]);
    o.links.push(tram);
    let has = |race: &str| {
        let p = job::prepare(&database(), &o, &request(race, "warrior", 2, &[]), &|_| {}).unwrap();
        p.world.links.iter().any(|l| l.name == "Gnomes only")
    };
    assert!(has("gnome"));
    assert!(!has("human"));
}

/// A link a quest opens (Forever's Skyborne leave their island by the teleport the giver of its
/// last quest offers) is not taken before that quest is.
#[test]
fn link_opened_by_a_quest() {
    let mut p = quests_at_start(2);
    let point = |zone, x, y| fg_route::world::ZonePoint { zone, x, y };
    let link = fg_route::world::LinkDef {
        name: "Opened by quest 1".into(),
        faction: None,
        from: point(12, 48.17, 42.94),
        to: point(1537, 50.0, 50.0),
        seconds: 5.0,
        one_way: true,
        instant: true,
        races: vec![],
        after_quest: Some(p.model.quests[1].id),
    };
    let (faction, speed, wait) = (
        p.profile.faction,
        p.params.run_speed / p.params.detour,
        p.params.link_wait,
    );
    p.world.add_links(&[link], faction, speed, wait);
    let mut o = one_kill_east_of_start(&p, 0.0);
    (o.loc.pos, o.loc.zone) = (p.world.to_world(1537, 50.0, 50.0).unwrap(), 1537);
    p.model.quests[0].objectives = vec![o];
    let takes_it = |p: &mut Prepared, accepted: Vec<usize>| {
        p.model.initial.accepted = accepted;
        let mut trace = Vec::new();
        planner(p)
            .simulate_full(&[Stop::quest(0, Kind::Objective(0))], Some(&mut trace))
            .unwrap();
        trace
            .iter()
            .any(|t| matches!(t.event, Event::Link { first, .. } if p.world.links[first].name == "Opened by quest 1"))
    };
    assert!(!takes_it(&mut p, vec![0]), "quest 1 not taken: closed");
    assert!(takes_it(&mut p, vec![0, 1]), "quest 1 taken: open");
}

/// A quest given up is not turned in: it opens no follow-up. Fire Hardened Mail, left in the log
/// when Razorfen Kraul is not run, does not give Furen's Armor (it used to).
#[test]
fn quest_given_up_opens_no_follow_up() {
    let mut p = prepare("human", "warrior", 30, &["progression=cautious"]);
    p.params.dungeons = false;
    let (mail, armor) = (quest(&p, 1701).unwrap(), quest(&p, 1782).unwrap());
    let first = Stop::quest(quest(&p, 783).unwrap(), Kind::Accept);
    let route = [first, Stop::quest(armor, Kind::Accept)];
    p.model.initial.turned = vec![mail as usize];
    assert!(
        planner(&p).simulate(&route, None).is_some(),
        "turned in, it opens the follow-up"
    );
    p.model.initial.turned.clear();
    p.model.initial.accepted = vec![mail as usize];
    let mut trace = Vec::new();
    assert!(planner(&p).simulate(&route[..1], Some(&mut trace)).is_some());
    assert!(
        trace
            .iter()
            .any(|t| matches!(t.event, Event::Abandon { quest } if quest == mail as usize)),
        "Fire Hardened Mail is given up"
    );
    assert!(planner(&p).simulate(&route, None).is_none(), "given up, it does not");
}

/// No mob is farmed on a walk through a capital: what the sources know nothing of there is a
/// passer-by, a pet or a dungeon's mob put at its door; outside, the mobs met are.
#[test]
fn no_farming_in_capitals() {
    let p = orc_warlock();
    let at = |zone, x, y| p.world.to_world(zone, x, y).unwrap();
    // Stormwind: Old Town to the Dwarven District, wide.
    let city = p.model.farm.along(&[at(1519, 72.3, 47.7), at(1519, 62.4, 34.2)], 200.0);
    assert!(city.is_empty(), "{city:?}");
    // Elwynn Forest, around Goldshire.
    let forest = p.model.farm.along(&[at(12, 40.0, 60.0), at(12, 50.0, 75.0)], 200.0);
    assert!(!forest.is_empty(), "no mob met around Goldshire");
}

/// Grinding takes place at a camp of mobs of the character's level, gone to first: not in a
/// capital, where no mob lives.
#[test]
fn grinding_goes_to_a_camp() {
    let mut p = prepare("human", "warrior", 4, &["progression=cautious"]);
    let city = p.world.to_world(1519, 62.4, 34.2).unwrap();
    p.model.initial.pos = Some((city, 1519));
    let planner = planner(&p);
    let mut trace = Vec::new();
    let (_, spent) = planner.simulate_full(&[], Some(&mut trace)).unwrap();
    let camps: Vec<_> = (trace.iter())
        .filter_map(|t| match t.event {
            Event::Grind { near, zone, .. } => Some((near, zone)),
            _ => None,
        })
        .collect();
    assert_ne!(camps, []);
    for (near, zone) in &camps {
        assert_ne!(*zone, 1519, "grinding in Stormwind");
        assert!(p.model.farm.around(near, 150.0, 1..=5) >= 8, "no camp at {near:?}");
    }
    assert!(spent.travel > 0.0, "no trip to the camp");
    assert!(spent.grinding > 0.0);
}

/// The addon gets how many items an item objective asks for (its items to keep) and the guide's
/// reputation goals (its reputation tab).
#[test]
fn addon_gets_item_counts_and_reputation_goals() {
    let goal = r#"reputation_goals=[{"faction":72,"target":9000}]"#;
    let p = prepare("human", "warrior", 6, &[goal]);
    let planner = planner(&p);
    let route = planner.construct(|_| {});
    let guide = fg_route::export::build(&planner, &p.profile, &route);
    let lua = fg_route::export::to_lua(std::slice::from_ref(&guide));
    let item = guide
        .steps
        .iter()
        .find(|s| {
            s.target
                .as_ref()
                .is_some_and(|t| t.kind == fg_route::export::TargetKind::Item)
        })
        .expect("an item objective");
    let line = lua
        .lines()
        .find(|l| l.contains(&format!("t = {:?}", item.text)))
        .unwrap();
    assert!(line.contains(", c = "), "{line}");
    assert!(lua.contains("reputations = { { id = 72, name = "), "{}", &lua[..300]);
}

/// The mobs farmed on the way to a step are a count of their own (the addon counts the kills),
/// not words of its text.
#[test]
fn mobs_on_the_way_are_counted_apart() {
    let p = orc_warlock();
    let planner = planner(p);
    let route = planner.construct(|_| {});
    let guide = fg_route::export::build(&planner, &p.profile, &route);
    let farmed: Vec<_> = guide.steps.iter().filter(|s| s.farm.is_some()).collect();
    assert!(!farmed.is_empty(), "no mobs farmed on the way");
    assert!(
        farmed.iter().all(|s| !s.text.contains("on the way)")),
        "{}",
        farmed[0].text
    );
    assert!(fg_route::export::to_lua(std::slice::from_ref(&guide)).contains(", fk = "));
}

/// A dungeon run is a step that takes the player in, then a step per quest of the route done
/// inside (marked `inside`, in the run's time), each turned in later.
#[test]
fn dungeon_quests_are_steps_of_their_own() {
    use fg_route::export::StepKind;
    let p = prepare(
        "human",
        "warrior",
        25,
        &["progression=cautious", "forced_dungeons=[1581]"],
    );
    let planner = planner(&p);
    let route = planner.construct(|_| {});
    let guide = fg_route::export::build(&planner, &p.profile, &route);
    let steps = &guide.steps;
    assert!(steps.iter().any(|s| s.kind == StepKind::Dungeon), "the forced run");
    assert!(steps.iter().any(|s| s.inside), "no quest step in the runs");
    for (i, s) in steps.iter().enumerate().filter(|(_, s)| s.inside) {
        let before = &steps[i - 1];
        assert!(
            before.kind == StepKind::Dungeon || before.inside,
            "{} not after a run ({:?} {})",
            s.text,
            before.kind,
            before.text
        );
        assert_eq!(s.kind, StepKind::Objective);
        let q = s.quest.unwrap();
        let turnin = steps
            .iter()
            .position(|t| t.kind == StepKind::TurnIn && t.quest == Some(q));
        assert!(turnin.is_some_and(|t| t > i), "{} not turned in after the run", s.text);
    }
    assert!(
        steps.windows(2).all(|w| w[0].time <= w[1].time + 1e-6),
        "time goes back"
    );
    // The addon tells them apart (the run step is then done on entering the dungeon).
    assert!(fg_route::export::to_lua(std::slice::from_ref(&guide)).contains("di = true"));
}

/// A timed quest reached after its time has failed: the guide gives it up instead of turning it
/// in (no reward); in time, it is turned in.
#[test]
fn late_timed_quest_is_given_up() {
    let mut p = prepare("human", "warrior", 30, &["progression=cautious"]);
    (p.profile.from_level, p.profile.to_level) = (22, 30);
    let i = p
        .model
        .quests
        .iter()
        .position(|q| q.id == 1699)
        .expect("The Rethban Gauntlet");
    assert_eq!(p.model.quests[i].time_limit, Some(3600.0), "its limit, from the data");
    let objectives = p.model.quests[i].objectives.len() as u8;
    let mut route = vec![Stop::quest(i as u32, Kind::Accept)];
    route.extend((0..objectives).map(|k| Stop::quest(i as u32, Kind::Objective(k))));
    route.push(Stop::quest(i as u32, Kind::TurnIn));
    for (limit, turned_in) in [(Some(1.0), false), (Some(36_000.0), true), (None, true)] {
        p.model.quests[i].time_limit = limit;
        let mut trace = Vec::new();
        let (_, spent) = planner(&p)
            .simulate_full(&route, Some(&mut trace))
            .expect("a valid route");
        let given_up = trace
            .iter()
            .any(|t| matches!(t.event, Event::Abandon { quest } if quest == i));
        let turned = trace
            .iter()
            .any(|t| matches!(t.event, Event::Stop { stop, .. } if stop.kind == Kind::TurnIn));
        assert_eq!((turned, given_up), (turned_in, !turned_in), "limit {limit:?}");
        assert_eq!(spent.quest_xp > 0, turned_in, "reward with limit {limit:?}");
    }
}

/// Dungeon mobs keep their level: a character far above them gets no XP from the run (they used
/// to follow the character's level).
#[test]
fn outleveled_dungeon_gives_no_mob_xp() {
    let mut p = prepare("human", "warrior", 30, &["progression=cautious"]);
    (p.profile.from_level, p.profile.to_level) = (32, 40);
    let deadmines = p.model.dungeons.iter().position(|d| d.name == "The Deadmines").unwrap();
    let run = Stop {
        index: deadmines as u32,
        kind: Kind::Dungeon,
    };
    let (_, spent) = planner(&p).simulate_full(&[run], None).unwrap();
    assert_eq!(spent.dungeon_runs, 1);
    assert_eq!(spent.mob_xp, 0);
}

/// Where an entity of the model stands, as the addon records it.
fn map_pos(p: &Prepared, pos: &fg_route::world::Pos, zone: i64) -> MapPos {
    let m = p.world.to_map(zone, pos).unwrap();
    MapPos {
        map: m.ui_map,
        x: m.x,
        y: m.y,
    }
}

/// An orc of level 4 standing next to Gornek, Cutting Teeth turned in and Sting of the Scorpid
/// in the log (with `objectives` as its log shows them).
fn orc_met_in_game(objectives: Vec<LogObjective>, complete: bool, rested: i64) -> StartState {
    let p = orc_warlock();
    let gornek = &p.model.quests[quest(p, 788).unwrap() as usize].ends[0];
    StartState {
        name: "Brouz-Realm".into(),
        level: 4,
        xp: 300,
        rested,
        completed: vec![4641, 788],
        log: vec![LogQuest {
            id: 789,
            complete,
            objectives,
        }],
        position: Some(map_pos(p, &gornek.pos, gornek.zone)),
        ..StartState::default()
    }
}

/// A character met in game goes on from where it stands: its level, the quests it did open
/// their follow-ups, and a quest of its log is there to take (as if never taken) next to it.
#[test]
fn a_character_met_in_game_goes_on_from_where_it_stands() {
    let new = orc_warlock();
    let scorpids = quest(new, 789).unwrap();
    let accept = Stop::quest(scorpids, Kind::Accept);
    assert!(
        planner(new).simulate(&[accept], None).is_none(),
        "a new character must do Cutting Teeth first"
    );

    let p = prepare_from(
        "orc",
        "warlock",
        30,
        &["progression=cautious"],
        Some(orc_met_in_game(vec![], true, 0)),
    );
    assert_eq!(p.profile.from_level, 4);
    let scorpids = quest(&p, 789).unwrap();
    let accept = Stop::quest(scorpids, Kind::Accept);
    let mut trace = Vec::new();
    assert!(
        planner(&p).simulate(&[accept], Some(&mut trace)).is_some(),
        "Cutting Teeth done: open"
    );
    let at = trace
        .iter()
        .find(|t| matches!(t.event, Event::Stop { stop, .. } if stop == accept))
        .unwrap();
    assert!(at.time < 30.0, "next to Gornek: no walk ({} s)", at.time);
    assert!(at.level >= 4);
}

/// Rested XP doubles the XP of kills until it runs out.
#[test]
fn rested_xp_doubles_kill_xp() {
    let objectives = || {
        vec![LogObjective {
            text: "Scorpid Worker Tail: 0/10".into(),
            need: 10,
            ..LogObjective::default()
        }]
    };
    let mob_xp = |rested| {
        let p = prepare_from(
            "orc",
            "warlock",
            30,
            &["progression=cautious", "farm_on_way=false"],
            Some(orc_met_in_game(objectives(), false, rested)),
        );
        let scorpids = quest(&p, 789).unwrap();
        let route = [
            Stop::quest(scorpids, Kind::Accept),
            Stop::quest(scorpids, Kind::Objective(0)),
        ];
        planner(&p).simulate_full(&route, None).unwrap().1.mob_xp
    };
    let (plain, rested) = (mob_xp(0), mob_xp(100_000));
    assert!(plain > 0);
    assert!((rested - 2 * plain).abs() <= 10, "{rested} vs 2 x {plain}");
}

/// The quests of the log count as never taken: the guide takes again those it wants, and
/// abandons the others at its start (the quest the model does not have too).
#[test]
fn quests_of_the_log_the_guide_does_not_take_are_abandoned_first() {
    let mut start = orc_met_in_game(vec![], false, 0);
    start.log.push(LogQuest {
        id: 999_999,
        ..LogQuest::default()
    });
    let p = prepare_from("orc", "warlock", 30, &["progression=cautious"], Some(start));
    let planner = planner(&p);
    let scorpids = quest(&p, 789).unwrap();
    let abandoned = |route: &[Stop]| -> Vec<i64> {
        let guide = fg_route::export::build(&planner, &p.profile, route);
        guide
            .steps
            .iter()
            .take_while(|s| s.kind == fg_route::export::StepKind::Abandon)
            .filter_map(|s| s.quest)
            .collect()
    };
    assert_eq!(abandoned(&[]), [789, 999_999]);
    assert_eq!(abandoned(&[Stop::quest(scorpids, Kind::Accept)]), [999_999]);
}

/// The hearthstone goes where the character bound it, the flight paths it knows are known, and
/// the quests of its log leave the whole log to the guide (it abandons those it does not take).
#[test]
fn hearthstone_flight_paths_and_log_of_a_character_met_in_game() {
    let mut p = prepare("orc", "warlock", 30, &["progression=cautious"]);
    let (inn, node) = (3, 5);
    let start = StartState {
        level: 12,
        log: vec![LogQuest {
            id: 999_999,
            ..LogQuest::default()
        }],
        bind: Some(Bind {
            name: String::new(),
            position: Some(map_pos(&p, &p.model.inns[inn].pos, p.model.inns[inn].zone)),
        }),
        flights: vec![p.world.taxi_nodes[node].id],
        ..StartState::default()
    };
    let log_size = p.params.quest_log_size;
    let mut notes = Vec::new();
    let initial = start.initial(&p.model, &p.world, &mut p.params, &mut notes);
    assert_eq!(initial.bind, Some(inn));
    assert_eq!(initial.known, 1 << node);
    assert_eq!(p.params.quest_log_size, log_size);
    assert_eq!(initial.log, [(999_999, String::new())]);
    assert_eq!(
        initial.pos.map(|(pos, _)| pos),
        Some(p.model.inns[inn].pos),
        "no position: at the hearthstone"
    );
}

/// The hearthstone is bound anew as the route moves on (more than one inn over 30 levels), each
/// bind worth it: the route without them takes longer. Built with fixed construction weights (the
/// defaults change with the measures): with others, as with any, this route can bind nowhere.
#[test]
fn hearthstone_follows_the_route() {
    let p = &prepare(
        "human",
        "warrior",
        30,
        &[
            "progression=cautious",
            "zone_stay=2.5",
            "continent_stay=1",
            "dungeon_quest_weight=0.5",
            "fit_penalty=0",
            "fit_above_penalty=0",
            "efficiency_power=0",
        ],
    );
    let planner = planner(p);
    let route = planner.construct(|_| {});
    let inns: Vec<&str> = route
        .iter()
        .filter(|s| s.kind == Kind::Bind)
        .map(|s| p.world.zone_name(p.model.inns[s.index as usize].zone))
        .collect();
    assert!(inns.len() >= 2, "bound at {inns:?}");
    let without: Vec<Stop> = route.iter().filter(|s| s.kind != Kind::Bind).copied().collect();
    let (with, without) = (planner.simulate(&route, None), planner.simulate(&without, None));
    assert!(
        with < without,
        "{with:?} with the binds, {without:?} without ({inns:?})"
    );
}

/// An objective `dx` yards east of the start, finished by killing one level 1 mob.
fn one_kill_east_of_start(p: &Prepared, dx: f64) -> Objective {
    kills_east_of_start(p, dx, 1.0, 1)
}

/// Back to town twice within the hour: the hearthstone is kept for the longer trip (from the
/// farther objective), not used on the first trip it would shorten.
#[test]
fn hearthstone_is_kept_for_the_longer_trip_back() {
    let mut p = quests_at_start(2);
    p.model.quests[0].objectives = vec![one_kill_east_of_start(&p, 1500.0)];
    p.model.quests[1].objectives = vec![one_kill_east_of_start(&p, 3000.0)];
    p.model.initial.accepted = vec![0, 1];
    let route = vec![
        Stop::quest(0, Kind::Objective(0)),
        Stop::quest(0, Kind::TurnIn),
        Stop::quest(1, Kind::Objective(0)),
        Stop::quest(1, Kind::TurnIn),
    ];
    let planner = planner(&p);
    let placed = planner.place_hearths(route.clone());
    let at = placed.iter().position(|s| s.kind == Kind::Hearth);
    assert_eq!(at, Some(3), "{placed:?}");
    assert!(planner.simulate(&placed, None) < planner.simulate(&route, None));
}

/// The hearthstone does not take the character back to an inn of an excluded zone (a character
/// gone to another continent, bound in its start zone it no longer wants).
#[test]
fn hearthstone_avoids_an_excluded_zone() {
    let mut p = quests_at_start(2);
    p.model.quests[0].objectives = vec![one_kill_east_of_start(&p, 1500.0)];
    p.model.quests[1].objectives = vec![one_kill_east_of_start(&p, 3000.0)];
    p.model.initial.accepted = vec![0, 1];
    let route = vec![
        Stop::quest(0, Kind::Objective(0)),
        Stop::quest(0, Kind::TurnIn),
        Stop::quest(1, Kind::Objective(0)),
        Stop::quest(1, Kind::TurnIn),
    ];
    p.params.excluded_zones = vec![p.model.inns[p.model.start_inn].zone];
    let placed = planner(&p).place_hearths(route.clone());
    assert!(!placed.iter().any(|s| s.kind == Kind::Hearth), "{placed:?}");
}

/// Time spent outside the preferred zones counts in the route's score: the same route scores
/// worse when its zone is not the preferred one.
#[test]
fn time_outside_the_preferred_zones_counts() {
    let mut p = quests_at_start(1);
    p.model.quests[0].objectives = vec![one_kill_east_of_start(&p, 1500.0)];
    p.model.initial.accepted = vec![0];
    let route = [Stop::quest(0, Kind::Objective(0)), Stop::quest(0, Kind::TurnIn)];
    let zone = p.model.inns[p.model.start_inn].zone;
    p.params.preferred_zones = vec![zone];
    let inside = planner(&p).simulate(&route, None).unwrap();
    p.params.preferred_zones = vec![zone + 1];
    let outside = planner(&p).simulate(&route, None).unwrap();
    assert!(outside > inside, "{outside} outside, {inside} inside");
}

/// A mage trained in its teleports goes to a capital with one rather than across the world.
#[test]
fn mage_teleports_to_a_capital() {
    let mut p = quests_at_start_as("human", "mage", 1);
    let names: Vec<(&str, i64)> = p.model.teleports.iter().map(|t| (t.name.as_str(), t.level)).collect();
    assert!(names.contains(&("Teleport: Ironforge", 20)), "{names:?}");
    assert!(!names.iter().any(|(n, _)| n.contains("Orgrimmar")), "{names:?}");
    let ironforge = p
        .model
        .teleports
        .iter()
        .find(|t| t.name == "Teleport: Ironforge")
        .unwrap()
        .to
        .clone();
    let mut o = one_kill_east_of_start(&p, 0.0);
    o.loc = ironforge;
    p.model.quests[0].objectives = vec![o];
    p.model.initial.accepted = vec![0];
    (p.profile.from_level, p.profile.to_level, p.model.initial.trained) = (25, 30, Some(25));
    let route = [Stop::quest(0, Kind::Objective(0))];
    // When the objective is done (the grinding to the target level comes after).
    let done = |p: &Prepared, teleport: bool| {
        let mut trace = Vec::new();
        planner(p).simulate_full(&route, Some(&mut trace)).unwrap();
        assert_eq!(
            trace.iter().any(|t| matches!(t.event, Event::Teleport { .. })),
            teleport
        );
        (trace.iter())
            .find(|t| matches!(t.event, Event::Stop { .. }))
            .unwrap()
            .time
    };
    let with = done(&p, true);
    p.model.teleports.clear();
    let without = done(&p, false);
    assert!(
        with * 3.0 < without,
        "{with:.0} s to the objective with the teleport, {without:.0} s without"
    );
}

/// A shaman has a second hearthstone from level 30.
#[test]
fn shaman_has_astral_recall() {
    let p = quests_at_start_as("orc", "shaman", 0);
    assert_eq!(p.model.recall, Some(30));
    assert!(p.model.teleports.is_empty());
}

/// Training starts from the spells the character learned: the first rank it lacks.
#[test]
fn last_training_from_the_spells_learned() {
    let power = orc_warlock().model.power.as_ref().unwrap();
    let up_to = |level: i64| power.ranks().filter(|r| r.level <= level).map(|r| r.id).collect();
    assert_eq!(power.trained_level(&up_to(12), 12), Some(12));
    let trained = power.trained_level(&up_to(6), 12).unwrap();
    assert!((6..12).contains(&trained), "trained at {trained}");
    assert_eq!(power.trained_level(&[1].into(), 12), None, "other data");
}

/// A turn-in among stronger mobs waits until the character can fight through them: Sven's
/// Revenge ends at a mound in a camp of level 25-27 Defias (a level 22 warrior died there).
#[test]
fn turn_in_among_strong_mobs_waits() {
    let p = human_warrior();
    let sven = quest(p, 95).unwrap();
    assert!(p.model.quests[sven as usize].end_guard.level >= 26);
    let route = [Stop::quest(sven, Kind::Accept), Stop::quest(sven, Kind::TurnIn)];
    let mut trace = Vec::new();
    assert!(planner(p).simulate(&route, Some(&mut trace)).is_some());
    let at = |stop: Stop| {
        trace
            .iter()
            .find(|t| matches!(t.event, Event::Stop { stop: s, .. } if s == stop))
            .unwrap()
    };
    assert!(at(route[0]).level < 24, "taken early (level {})", at(route[0]).level);
    assert!(at(route[1]).power >= 26.0, "turned in at power {}", at(route[1]).power);
}

/// On a crowded server, escorts wait for their turn and targets with few spawns are fought
/// over: Miran's escort (level 15: 90 % of the crowd) and Bellygrub (one spawn) take waiting,
/// the wolves of Northshire (many spawns) hardly any.
#[test]
fn crowded_server_waits_for_escorts_and_rare_spawns() {
    let quiet = human_warrior();
    let crowded = prepare("human", "warrior", 30, &["progression=cautious", "crowded=true"]);
    let wait = |id: i64| {
        let extra = |p: &Prepared| {
            p.model.quests[quest(p, id).unwrap() as usize]
                .objectives
                .iter()
                .map(|o| o.extra)
                .sum::<f64>()
        };
        extra(&crowded) - extra(quiet)
    };
    assert!(wait(309) >= 0.9 * 900.0 - 1e-6, "escort: {} s", wait(309));
    assert!(wait(34) >= 90.0, "Bellygrub: {} s", wait(34));
    assert!(wait(33) / 8.0 < wait(34) / 2.0, "wolves: {} s for 8 meats", wait(33));
}

/// The clean-up at the end of the search never makes the route worse: cutting the stops after
/// the target level would drop a turn-in planned there that was made at hand before, losing its
/// XP to grinding (it did).
#[test]
fn clean_up_keeps_a_quest_turned_in_at_hand() {
    let mut p = quests_at_start(1);
    (p.model.quests[0].xp, p.params.camp_radius) = (400, 80.0);
    let planner = planner(&p);
    let route = vec![Stop::quest(0, Kind::Accept), Stop::quest(0, Kind::TurnIn)];
    let (before, spent) = planner.simulate_full(&route, None).unwrap();
    assert_eq!(spent.quest_xp, 400, "turned in at hand at the first stop");
    let cleaned = planner.improve(route, std::time::Duration::ZERO, |_| {}, |_, _, _| {});
    let (after, spent) = planner.simulate_full(&cleaned, None).unwrap();
    assert_eq!(spent.quest_xp, 400);
    assert!(after <= before + 0.5, "{before} -> {after}");
}

/// A quest taken at hand needs what its own stop needs: the camp around its giver (three level
/// 25 mobs) is fought through at the same level, not at level 1 because another giver stands
/// nearby (it was).
#[test]
fn quest_at_hand_waits_for_its_camp() {
    let mut p = quests_at_start(2);
    p.params.camp_radius = 80.0;
    p.model.quests[1].start_guard = Guard {
        level: 25,
        count: 3,
        elite: false,
    };
    let planner = planner(&p);
    let taken_at = |route: &[Stop]| {
        let mut trace = Vec::new();
        planner.simulate_full(route, Some(&mut trace)).unwrap();
        trace.iter().find_map(|t| {
            matches!(t.event, Event::Stop { stop, .. } if stop == Stop::quest(1, Kind::Accept)).then_some(t.level)
        })
    };
    let own_stop = taken_at(&[Stop::quest(1, Kind::Accept)]).unwrap();
    assert!(own_stop > 20, "taken at level {own_stop}");
    let at_hand = taken_at(&[Stop::quest(0, Kind::Accept), Stop::quest(1, Kind::Accept)]).unwrap();
    assert_eq!(at_hand, own_stop);
}

/// Kill `kills` mobs of `mob_level` `dx` yards east of the start.
fn kills_east_of_start(p: &Prepared, dx: f64, kills: f64, mob_level: i64) -> Objective {
    let mut loc = p.model.start.clone();
    loc.pos.x += dx;
    Objective {
        loc,
        text: format!("kill x{kills} at {dx} yd"),
        kills,
        mob_level,
        uses: 0.0,
        extra: 0.0,
        dungeon: None,
        elite: false,
        count: kills,
        mobs: vec![],
        pull: 1.0,
        spawns: 0,
        pack: 1.0,
        spots: None,
        guard: Guard::default(),
        alts: vec![],
        after: 0,
        handed_by: vec![],
        stock: None,
    }
}

/// Where the objective is done: east of the start by how many yards.
fn done_at(p: &Prepared) -> f64 {
    let mut trace = Vec::new();
    planner(p)
        .simulate_full(&[Stop::quest(0, Kind::Objective(0))], Some(&mut trace))
        .unwrap();
    let loc = trace
        .iter()
        .find_map(|t| match &t.event {
            Event::Stop { loc, .. } => Some(loc.pos.x),
            _ => None,
        })
        .unwrap();
    (loc - p.model.start.pos.x).round()
}

/// Mobs living in several places: the objective is done at the closest one, unless its mobs are
/// too strong there (the levels to grind first cost more than the walk to the other).
#[test]
fn objective_is_done_at_its_best_place() {
    let mut p = quests_at_start(1);
    p.model.initial.accepted = vec![0];
    let mut far = kills_east_of_start(&p, 900.0, 8.0, 1);
    far.alts = vec![kills_east_of_start(&p, 100.0, 8.0, 1)];
    p.model.quests[0].objectives = vec![far.clone()];
    assert_eq!(done_at(&p), 100.0, "the closest place");
    far.alts = vec![kills_east_of_start(&p, 100.0, 8.0, 12)];
    p.model.quests[0].objectives = vec![far];
    assert_eq!(done_at(&p), 900.0, "the weaker mobs, further away");
}

/// In a group, the class quests required of each class are required, not only the
/// character's: a warrior leveling with a warlock goes for the felsteed.
#[test]
fn group_class_quests_are_required() {
    let p = prepare_group("human", "warrior", 40, &[], None, &["warlock"]);
    let felsteed: Vec<bool> = p
        .model
        .quests
        .iter()
        .filter(|q| q.name == "Summon Felsteed")
        .map(|q| q.mandatory)
        .collect();
    assert!(!felsteed.is_empty() && felsteed.iter().all(|&m| m), "{felsteed:?}");
}

/// Moving a pet quest early (`power_first`, at the end of the search) may cost time, on purpose,
/// but never more than `class_power_slack`; without slack the clean-up never makes the route
/// worse.
#[test]
fn pet_quest_moves_early_within_its_slack() {
    let mut p = quests_at_start(2);
    p.world.terrain = fg_route::terrain::Terrain::default();
    p.model.quests[0].power = Some((0, 1.4));
    p.model.quests[0].ends[0].pos.x += 700.0;
    let route = vec![
        Stop::quest(0, Kind::Accept),
        Stop::quest(1, Kind::Accept),
        Stop::quest(1, Kind::TurnIn),
        Stop::quest(0, Kind::TurnIn),
    ];
    let cost = |p: &Prepared| {
        let planner = planner(p);
        let before = planner.simulate(&route, None).unwrap();
        let cleaned = planner.improve(route.clone(), std::time::Duration::ZERO, |_| {}, |_, _, _| {});
        planner.simulate(&cleaned, None).unwrap() - before
    };
    let slack = p.params.class_power_slack;
    let with_slack = cost(&p);
    assert!(with_slack <= slack + 0.5, "{with_slack} s for a slack of {slack} s");
    p.params.class_power_slack = 0.0;
    let strict = cost(&p);
    assert!(strict <= 0.5, "{strict} s without slack");
}

/// A companion of a class the character's race cannot play is of a race that can: a human
/// leveling with a druid plans the druid's Aquatic Form, a night elf quest, and its chain.
#[test]
fn companion_class_quests_of_its_race() {
    let p = prepare_group("human", "warrior", 30, &[], None, &["druid"]);
    let aquatic: Vec<bool> = p
        .model
        .quests
        .iter()
        .filter(|q| q.name == "Aquatic Form")
        .map(|q| q.mandatory)
        .collect();
    assert_eq!(aquatic, [true]);
}

/// Each companion keeps its own race: adding a druid (a night elf for a human) does not bring
/// the night elf variants of the hunter's quests (the hunter is a dwarf, as without the druid).
#[test]
fn each_companion_keeps_its_race() {
    let mandatory = |group: &[&str]| -> std::collections::HashMap<i64, String> {
        let p = prepare_group("human", "warrior", 30, &[], None, group);
        p.model
            .quests
            .iter()
            .filter(|q| q.mandatory)
            .map(|q| (q.id, q.name.clone()))
            .collect()
    };
    let hunter = mandatory(&["hunter"]);
    let both = mandatory(&["hunter", "druid"]);
    assert!(!hunter.is_empty());
    // The druid brings its own quests and their chains, no other variant of the hunter's.
    let hunter_names: std::collections::HashSet<&String> = hunter.values().collect();
    for (id, name) in &both {
        assert!(
            hunter.contains_key(id) || !hunter_names.contains(name),
            "{name} ({id}): a hunter quest of another race required with the druid"
        );
    }
    assert!(hunter.keys().all(|id| both.contains_key(id)));
}

/// A guide is saved without a language: it is written in any of them when shown or installed.
/// The guide saved is the same whatever language it is made in, but for its texts.
#[test]
fn a_guide_is_written_in_any_language() {
    use fg_route::export::{StepKind, read_route, render};
    use fg_route::model::Names;
    let saved = |locale: &str| {
        let p = prepare_request(&PlanRequest {
            locale: Some(locale.into()),
            ..request("orc", "warlock", 4, &[])
        });
        let planner = planner(&p);
        let route = planner.construct(|_| {});
        let mut saved = serde_json::to_value(fg_route::export::build(&planner, &p.profile, &route)).unwrap();
        for step in saved["steps"].as_array_mut().unwrap() {
            step.as_object_mut().unwrap().remove("text");
        }
        saved.as_object_mut().unwrap().remove("locale");
        saved
    };
    let in_english = saved("enUS");
    assert_eq!(saved("frFR"), in_english);
    let guide = read_route(in_english).unwrap();
    let conn = database();
    let names = |locale: &str| Names::load(&conn, locale).unwrap();
    let accept = |g: &fg_route::Route| {
        g.steps
            .iter()
            .find(|s| s.kind == StepKind::Accept && s.quest == Some(788))
            .unwrap()
            .clone()
    };
    let en = render(&guide, &names("enUS"));
    let fr = render(&guide, &names("frFR"));
    let de = render(&guide, &names("deDE"));
    assert_eq!(accept(&en).quest_name.as_deref(), Some("Cutting Teeth"));
    assert!(
        accept(&en).text.starts_with("Accept Cutting Teeth from "),
        "{}",
        accept(&en).text
    );
    assert_eq!(accept(&fr).quest_name.as_deref(), Some("La dent tranchante"));
    assert!(
        accept(&fr).text.starts_with("Prendre La dent tranchante auprès de "),
        "{}",
        accept(&fr).text
    );
    assert!(
        accept(&de).text.starts_with("Scharfe Zähne bei "),
        "{}",
        accept(&de).text
    );
    for locale in [
        "enUS", "frFR", "deDE", "esES", "esMX", "ptBR", "ruRU", "koKR", "zhCN", "zhTW",
    ] {
        let g = render(&guide, &names(locale));
        assert_eq!(g.locale.as_deref(), Some(locale));
        for s in &g.steps {
            assert!(!s.text.is_empty() && !s.text.contains('{'), "{locale}: {s:?}");
        }
    }
}

/// Accept steps tell what their quest needs (quests done, level), so that the addon lists only
/// the quests that can be taken now.
#[test]
fn accept_steps_carry_what_the_quest_needs() {
    let p = human_warrior();
    let planner = planner(p);
    let route = planner.construct(|_| {});
    let guide = fg_route::export::build(&planner, &p.profile, &route);
    let accepts: Vec<_> = guide
        .steps
        .iter()
        .filter(|s| s.kind == fg_route::export::StepKind::Accept)
        .collect();
    let follow_up = accepts
        .iter()
        .find(|s| !s.pre_any.is_empty())
        .expect("a follow-up quest in the route");
    let q = &p.model.quests[p.model.index[&follow_up.quest.unwrap()]];
    assert_eq!((&follow_up.pre, &follow_up.pre_any), (&q.pre_all, &q.pre_any));
    assert!(accepts.iter().all(|s| s.min_level.is_some()));
    let lua = fg_route::export::to_lua(std::slice::from_ref(&guide));
    let ids: Vec<String> = q.pre_any.iter().map(ToString::to_string).collect();
    assert!(
        lua.contains(&format!("pa = {{ {} }}", ids.join(", "))),
        "prerequisites in the addon's routes"
    );
}

/// Each step carries the time it is planned to take (the addon records it next to the time the
/// player took): the steps' times add up to the route's.
#[test]
fn steps_carry_their_planned_time() {
    let p = human_warrior();
    let planner = planner(p);
    let route = planner.construct(|_| {});
    let guide = fg_route::export::build(&planner, &p.profile, &route);
    let lua = fg_route::export::to_lua(std::slice::from_ref(&guide));
    let durations: Vec<f64> = lua
        .lines()
        .filter_map(|line| line.split(", d = ").nth(1))
        .map(|rest| rest.split([',', ' ']).next().unwrap().parse().unwrap())
        .collect();
    assert!(durations.len() > guide.steps.len() / 2, "most steps take some time");
    let end = guide.steps.last().unwrap().time;
    let sum: f64 = durations.iter().sum();
    assert!(
        (sum - end).abs() <= guide.steps.len() as f64,
        "planned times {sum} s, route {end} s"
    );
}

/// Blackrock Menace (axes from Blackrock orcs, Redridge): its spot near the giver holds only a
/// few orcs standing in pairs, one point for a Grunt or an Outrunner taking turns. The pairs
/// make it harder than lone orcs, the ~22 kills wait for respawns, more on a crowded server.
#[test]
fn small_spot_in_pairs_is_harder_and_slower() {
    let p = human_warrior();
    let objective = |p: &Prepared| p.model.quests[quest(p, 20).unwrap() as usize].objectives[0].clone();
    let o = objective(p);
    assert!((5..=10).contains(&o.spawns), "{} spawns", o.spawns);
    assert!(o.pack >= 1.5, "{} mobs per pull", o.pack);
    let q = &p.model.quests[quest(p, 20).unwrap() as usize];
    let need = planner(p).combat().need(q, &o);
    assert!(need >= o.mob_level as f64 + p.params.pack_extra() + 0.5, "need {need}");
    assert!(o.extra > 0.0, "respawns waited for");
    let crowded = prepare("human", "warrior", 30, &["progression=cautious", "crowded=1"]);
    assert!(
        objective(&crowded).extra > 3.0 * o.extra,
        "{} vs {}",
        objective(&crowded).extra,
        o.extra
    );
}

/// Settings saved before the crowd scale (`crowded: true`) read as a full crowd.
#[test]
fn crowded_on_reads_as_a_full_crowd() {
    let on: fg_route::params::Params = serde_json::from_str(r#"{"crowded": true}"#).unwrap();
    let half: fg_route::params::Params = serde_json::from_str(r#"{"crowded": 0.5}"#).unwrap();
    assert!((on.crowded - 1.0).abs() < 1e-9 && (half.crowded - 0.5).abs() < 1e-9);
}

/// The crowd thins out with the levels: all of it in the starting zones, half from level 40.
#[test]
fn crowd_thins_out_with_the_levels() {
    let p: fg_route::params::Params = serde_json::from_str(r#"{"crowded": 1}"#).unwrap();
    let at: Vec<f64> = [1, 10, 11, 25, 35, 45, 60, 70].iter().map(|&l| p.crowd_at(l)).collect();
    assert_eq!(at, [1.0, 1.0, 0.9, 0.8, 0.6, 0.5, 0.5, 0.5]);
}

/// Every level the route reaches has a step to catch up (grind) for a character behind it, at
/// a place of the level where mobs live, asking at most 90 % of the level: a character a few
/// percent behind goes on.
#[test]
fn every_level_has_a_step_to_catch_up() {
    let p = human_warrior();
    let planner = planner(p);
    let route = planner.construct(|_| {});
    let guide = fg_route::export::build(&planner, &p.profile, &route);
    let rules = p.params.edition.rules();
    let gates: Vec<_> = guide
        .steps
        .iter()
        .filter(|s| s.say.first().is_some_and(|p| p.key == "catch_up"))
        .collect();
    let levels: std::collections::BTreeSet<i64> = gates.iter().map(|s| s.level).collect();
    assert!(levels.len() >= 20, "levels with a step: {levels:?}");
    for s in &gates {
        assert!(
            s.xp.unwrap() as f64 <= 0.9 * rules.to_next_level(s.level) as f64 + 1.0,
            "level {} asks {} xp",
            s.level,
            s.xp.unwrap()
        );
    }
}

/// Turn-in steps tell the XP they give, for the addon's grind steps to count the quests
/// complete in the log.
#[test]
fn turn_ins_carry_their_xp() {
    let p = human_warrior();
    let planner = planner(p);
    let guide = fg_route::export::build(&planner, &p.profile, &planner.construct(|_| {}));
    let turn_ins: Vec<_> = guide
        .steps
        .iter()
        .filter(|s| s.kind == fg_route::export::StepKind::TurnIn)
        .collect();
    assert!(turn_ins.iter().all(|s| s.reward_xp.is_some()));
    assert!(turn_ins.iter().filter(|s| s.reward_xp.unwrap() > 0).count() * 10 >= turn_ins.len() * 9);
    assert!(fg_route::export::to_lua(std::slice::from_ref(&guide)).contains(", rx = "));
}

/// Every step tells the XP the route has once it is done, never going down from one step to
/// the next (planned grinds moved earlier included): the addon shows it on the XP bar.
#[test]
fn steps_carry_the_planned_xp() {
    let p = human_warrior();
    let planner = planner(p);
    let guide = fg_route::export::build(&planner, &p.profile, &planner.construct(|_| {}));
    let rules = p.params.edition.rules();
    let total = |s: &fg_route::export::Step| {
        (1..s.level).map(|l| rules.to_next_level(l)).sum::<i64>() + s.route_xp.expect("planned xp")
    };
    for pair in guide.steps.windows(2) {
        assert!(
            total(&pair[1]) >= total(&pair[0]),
            "{} -> {}",
            pair[0].text,
            pair[1].text
        );
    }
}

/// The tauren druid's bear form: Body and Heart is required with its chain through Moonglade
/// (one possible prerequisite each), Moonglade is reached by the teleport the quest
/// "Moonglade" teaches, and Lunaclaw Spirit is met at the Horde's Moonkin Stone, in the Barrens.
#[test]
fn druid_bear_form_through_moonglade() {
    let p = prepare("tauren", "druid", 16, &[]);
    let at = |id: i64| &p.model.quests[p.model.index[&id]];
    for id in [5922, 5930, 5932, 6002] {
        assert!(at(id).mandatory, "{} ({id}) not required", at(id).name);
    }
    assert_eq!(at(6002).objectives[0].loc.zone, 17);
    let teleport = p
        .model
        .teleports
        .iter()
        .find(|t| t.name == "Teleport: Moonglade")
        .unwrap();
    let taught: Vec<&str> = teleport
        .quests
        .iter()
        .map(|&i| p.model.quests[i].name.as_str())
        .collect();
    assert!(!taught.is_empty() && taught.iter().all(|n| *n == "Moonglade"));
    assert!(
        p.model
            .teleports
            .iter()
            .all(|t| t.name != "Teleport: Moonglade" || t.level == 0)
    );
}

/// A dungeon whose quests are filed under it is listed even when the sources place nothing its
/// quests need inside (WoW Forever's Ruins of Lordaeron, GitHub #18): here The Deadmines with every
/// spawn inside removed.
#[test]
fn dungeons_with_quests_filed_under_them() {
    let path = std::env::temp_dir().join(format!("fg-dungeons-{}.sqlite", std::process::id()));
    let _ = std::fs::remove_file(&path);
    database().execute("VACUUM INTO ?1", [path.to_str().unwrap()]).unwrap();
    let conn = Connection::open(&path).unwrap();
    conn.execute(
        "DELETE FROM m_spawn WHERE zone_id IN (SELECT area FROM dungeon WHERE name = 'The Deadmines'
           UNION SELECT j.value FROM dungeon, json_each(coalesce(alt_areas, '[]')) j WHERE name = 'The Deadmines')",
        [],
    )
    .unwrap();
    let options = job::options(&conn, &overrides()).unwrap();
    drop(conn);
    std::fs::remove_file(&path).ok();
    assert!(options.dungeons.iter().any(|d| d.name == "The Deadmines"));
}

/// Dungeons whose zone the game files under the continent (Blackrock Depths, Blackrock Spire,
/// the Sunken Temple) are found through their instance map.
#[test]
fn dungeons_filed_under_the_continent() {
    let options = job::options(&database(), &overrides()).unwrap();
    for name in ["Blackrock Depths", "Blackrock Spire", "The Temple of Atal'Hakkar"] {
        assert!(options.dungeons.iter().any(|d| d.name == name), "{name} missing");
    }
}
