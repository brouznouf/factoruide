//! Planner behavior on the Classic database: reproductions of the problems met on real guides.

use fg_route::job::{self, Bind, LogObjective, LogQuest, MapPos, Overrides, PlanRequest, Prepared, StartState};
use fg_route::model::{Guard, Initial, Objective, Spots};
use fg_route::plan::{Event, Kind, Planner, Stop};
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
    let mut p = prepare("human", "warrior", 2, &[]);
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
    (
        p.params.camp_radius,
        p.params.along_corridor,
        p.params.farm_on_way,
        p.params.flight_learn_radius,
    ) = (0.0, 0.0, false, 0.0);
    p
}

/// Mobs killed on the way to an objective finish it: arriving there does not kill them again.
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

/// The item starting a quest drops from a mob the character must be able to take on.
#[test]
fn item_start_waits_for_its_mob() {
    let p = orc_warlock();
    let owatanka = quest(p, 884).unwrap();
    assert!(p.model.quests[owatanka as usize].min_level >= 22);
}

/// A quest started by an item dropping less than 10% of the time is luck, not a plan.
#[test]
fn rare_drop_start_is_left_out() {
    assert!(
        quest(human_warrior(), 136).is_none(),
        "Captain Sanders' Hidden Treasure"
    );
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
fn farm_on_the_way_stays_under_the_grind_cap() {
    let p = human_warrior();
    let planner = planner(p);
    let route = planner.construct(|_| {});
    let (_, spent) = planner.simulate_full(&route, None).unwrap();
    let rules = p.params.edition.rules();
    let levels: i64 = (1..30).map(|l| rules.to_next_level(l)).sum();
    assert!(spent.farm_xp > 0, "nothing farmed on the way");
    assert!(
        spent.farm_xp as f64 <= p.params.grind_cap * levels as f64 * 1.01,
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
            spots: Some(Spots {
                continent: loc.pos.continent,
                min: point,
                max: point,
                points: vec![point],
            }),
            guard: Guard::default(),
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
/// over: Miran's escort (level 15) and Bellygrub (one spawn) take waiting, the wolves of
/// Northshire (many spawns) hardly any.
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
    assert!(wait(309) >= 900.0, "escort: {} s", wait(309));
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

/// The fixed time of an objective (the wait of a crowded server) is paid even when its mobs
/// are met on the way: done there or at its stop, it takes as long (the wait was lost).
#[test]
fn wait_of_a_crowded_target_is_paid_on_the_way() {
    let mut p = quests_at_start(1);
    let here = p.model.start.pos;
    let mut loc = p.model.start.clone();
    loc.pos.x += 50.0;
    let point = (here.x + 25.0, here.y);
    p.model.quests[0].objectives = vec![Objective {
        loc,
        text: "crowded target".into(),
        kills: 1.0,
        mob_level: 1,
        uses: 0.0,
        extra: 900.0,
        dungeon: None,
        elite: false,
        count: 1.0,
        mobs: vec![],
        pull: 1.0,
        spots: Some(Spots {
            continent: here.continent,
            min: point,
            max: point,
            points: vec![point],
        }),
        guard: Guard::default(),
    }];
    p.model.initial.accepted = vec![0];
    let fighting = |p: &Prepared| {
        let route = [Stop::quest(0, Kind::Objective(0))];
        planner(p).simulate_full(&route, None).unwrap().1.fighting
    };
    let at_its_stop = fighting(&p);
    assert!(at_its_stop > 900.0);
    p.params.along_corridor = 35.0;
    let on_the_way = fighting(&p);
    assert!(
        (on_the_way - at_its_stop).abs() < 1.0,
        "{at_its_stop} s at its stop, {on_the_way} s on the way"
    );
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

/// An object gathered between the kills of another objective needs what its own stop needs:
/// guarded by three level 25 mobs, it is not picked up at level 1 next to an easy fight (it was).
#[test]
fn object_gathered_between_kills_waits_for_its_guards() {
    let mut p = quests_at_start(2);
    for (i, kills, uses, guard) in [
        (0, 1.0, 0.0, Guard::default()),
        (
            1,
            0.0,
            1.0,
            Guard {
                level: 25,
                count: 3,
                elite: false,
            },
        ),
    ] {
        let mut loc = p.model.start.clone();
        loc.pos.x += 80.0 * i as f64;
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
            count: 1.0,
            mobs: vec![],
            pull: 1.0,
            spots: Some(Spots {
                continent: loc.pos.continent,
                min: point,
                max: point,
                points: vec![point],
            }),
            guard,
        }];
    }
    p.model.initial.accepted = vec![0, 1];
    assert!(p.params.gather_overlap > 0.0);
    let mut trace = Vec::new();
    planner(&p)
        .simulate_full(&[Stop::quest(0, Kind::Objective(0))], Some(&mut trace))
        .unwrap();
    assert!(
        !trace.iter().any(|t| matches!(
            t.event,
            Event::Along {
                quest: 1,
                finished: true,
                ..
            }
        )),
        "the guarded object was gathered at level 1"
    );
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
    let overrides = Overrides::load_edition(&repo().join("overrides"), Edition::Classic).unwrap();
    let druid = &overrides.class_quests["druid"];
    for (id, name) in &both {
        assert!(
            hunter.contains_key(id) || druid.contains(name),
            "{name} ({id}) required with the druid, not without"
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
