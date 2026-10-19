//! Small routes built on the Classic database: the incremental evaluation of a changed route
//! gives the cost of its full replay, objectives started and done together.

use super::{Kind, Planner, Stop};
use crate::job::{self, Overrides, PlanRequest, Prepared};
use crate::model::{Guard, Initial, Objective};
use crate::xp::Edition;
use rusqlite::Connection;
use std::path::PathBuf;
use std::sync::Mutex;

fn repo() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../..")
}

/// The Classic database, unpacked next to the packed one when missing or older.
pub(crate) fn database() -> Connection {
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

/// Level 1 human warrior with only `n` quests of its own: taken and turned in at the start NPC,
/// nothing to do, no XP (each test sets what it checks). Nothing done on the way.
pub(crate) fn quests_at_start(n: usize) -> Prepared {
    let overrides = Overrides::load_edition(&repo().join("overrides"), Edition::Classic).unwrap();
    let request = PlanRequest {
        race: "human".into(),
        class: "warrior".into(),
        from_level: 1,
        to_level: 2,
        name: None,
        params: Some(overrides.params.clone()),
        required_class_quests: None,
        professions: vec![],
        locale: None,
        group: vec![],
        start: None,
    };
    let mut p = job::prepare(&database(), &overrides, &request, &|_| {}).unwrap();
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

fn planner(p: &Prepared) -> Planner<'_> {
    Planner::new(&p.model, &p.world, &p.params, &p.profile)
}

/// Swapping two accepts leaves the same state after them but for a timed quest's deadline: the
/// rest of the route is replayed (the quest is turned in in time now, it expired before).
#[test]
fn timed_quest_deadline_is_part_of_the_state() {
    let mut p = quests_at_start(2);
    p.model.quests[0].time_limit = Some(400.0);
    p.model.quests[0].xp = 400;
    // Taking quest 1 takes time.
    p.model.quests[1].start_uses = 100.0;
    let planner = planner(&p);
    let route = [
        Stop::quest(0, Kind::Accept),
        Stop::quest(1, Kind::Accept),
        Stop::quest(1, Kind::TurnIn),
        Stop::quest(0, Kind::TurnIn),
    ];
    let swapped = [route[1], route[0], route[2], route[3]];
    let run = planner.run(&route).unwrap();
    let incremental = planner.evaluate(&run, &swapped, 0, 1, true).unwrap();
    let full = planner.simulate(&swapped, None).unwrap();
    assert!(
        (incremental - full).abs() < 1e-6,
        "incremental {incremental} != full {full}"
    );
}

/// An objective done at another of its places, where a quest of the route is taken and turned
/// in at hand: removing that quest replays the objective's stop, whose place is the one really
/// visited (not the objective's nearest place).
#[test]
fn quests_at_hand_are_looked_for_where_the_stop_went() {
    let mut p = quests_at_start(2);
    p.params.camp_radius = 10.0;
    p.world.terrain = crate::terrain::Terrain::default();
    let (mut near, mut far) = (p.model.start.clone(), p.model.start.clone());
    near.pos.x += 100.0;
    far.pos.x += 300.0;
    // The nearest place takes 20 minutes more than the other one.
    let mut objective = Objective {
        loc: near,
        text: "Gather object".into(),
        kills: 0.0,
        mob_level: 0,
        uses: 1.0,
        extra: 1200.0,
        dungeon: None,
        elite: false,
        count: 1.0,
        mobs: vec![],
        pull: 1.0,
        spawns: 1,
        pack: 1.0,
        spots: None,
        guard: Guard::default(),
        alts: vec![],
        after: 0,
        handed_by: vec![],
        child: None,
        child_name: None,
        vendors: vec![],
        auction: vec![],
        stock: None,
    };
    let mut other = objective.clone();
    (other.loc, other.extra) = (far.clone(), 0.0);
    objective.alts = vec![other];
    p.model.quests[0].objectives = vec![objective];
    (p.model.quests[1].starts, p.model.quests[1].ends) = (vec![far.clone()], vec![far]);
    p.model.quests[1].xp = 100;
    let planner = planner(&p);
    let route = [
        Stop::quest(0, Kind::Accept),
        Stop::quest(0, Kind::Objective(0)),
        Stop::quest(1, Kind::Accept),
        Stop::quest(1, Kind::TurnIn),
        Stop::quest(0, Kind::TurnIn),
    ];
    let without = [route[0], route[1], route[4]];
    let run = planner.run(&route).unwrap();
    let incremental = planner.evaluate(&run, &without, 2, route.len(), false).unwrap();
    let full = planner.simulate(&without, None).unwrap();
    assert!(
        (incremental - full).abs() < 1e-6,
        "incremental {incremental} != full {full}"
    );
}

/// An objective started before the route (a character met in game) whose places need different
/// kills (drop rates): its share done counts at the other place. 10 items, 10 % done where it
/// takes 20 kills, leave 9 kills where it takes 10.
#[test]
fn objective_started_counts_its_share_at_another_place() {
    let mut p = quests_at_start(1);
    p.profile.to_level = 10;
    p.world.terrain = crate::terrain::Terrain::default();
    let mut loc = p.model.start.clone();
    loc.pos.x += 100.0;
    let mut objective = Objective {
        loc: loc.clone(),
        text: "Gather 10 items".into(),
        kills: 20.0,
        mob_level: 1,
        uses: 0.0,
        extra: 0.0,
        dungeon: None,
        elite: false,
        count: 10.0,
        mobs: vec![],
        pull: 1.0,
        spawns: 20,
        pack: 1.0,
        spots: None,
        guard: Guard::default(),
        alts: vec![],
        after: 0,
        handed_by: vec![],
        child: None,
        child_name: None,
        vendors: vec![],
        auction: vec![],
        stock: None,
    };
    let mut other = objective.clone();
    (other.loc.pos.x, other.kills) = (loc.pos.x + 200.0, 10.0);
    objective.alts = vec![other];
    p.model.quests[0].objectives = vec![objective];
    p.model.initial.accepted = vec![0];
    p.model.initial.progress = vec![(0, 0, 0.1)];
    let planner = planner(&p);
    let mut s = planner.initial_state();
    s.pos.x += 300.0;
    let xp = s.spent.mob_xp;
    planner.apply(&mut s, Stop::quest(0, Kind::Objective(0)), None);
    let each = planner.combat().mob_xp(1, &p.model.quests[0].objectives[0].alts[0]);
    let kills = (s.spent.mob_xp - xp) as f64 / each;
    assert!((kills - 9.0).abs() < 1e-6, "{kills} kills at the other place");
    assert_eq!(s.objectives[0], 1);
}

/// Objects gathered right after the kills of another quest at the same place take part of the
/// kills' time (`gather_overlap`); with another stop between them, nothing is saved: each stop
/// stands on its own, what is done together comes from the route's order.
#[test]
fn gathering_next_to_kills_saves_time_when_consecutive() {
    let p = camp_of_two_objectives();
    let planner = planner(&p);
    let fighting = |route: &[Stop]| planner.simulate_full(route, None).unwrap().1.fighting;
    let (kill, gather) = (Stop::quest(0, Kind::Objective(0)), Stop::quest(1, Kind::Objective(0)));
    let (k, g) = (fighting(&[kill]), fighting(&[gather]));
    let saved = g.min(k * p.params.gather_overlap);
    assert!(saved > 1.0);
    assert!((fighting(&[kill, gather]) - (k + g - saved)).abs() < 1e-6);
    assert!((fighting(&[gather, kill]) - (k + g - saved)).abs() < 1e-6);
    // Another stop between them (a quest taken at the start).
    let apart = fighting(&[kill, Stop::quest(2, Kind::Accept), gather]);
    assert!((apart - (k + g)).abs() < 1e-6, "{apart} with a stop between");
}

/// Kill and gathering objectives at a camp 100 yards east of the start, for quests 0 and 1 (in
/// the log), and a third quest given at the start.
fn camp_of_two_objectives() -> Prepared {
    let mut p = quests_at_start(3);
    p.profile.to_level = 10;
    p.world.terrain = crate::terrain::Terrain::default();
    let mut camp = p.model.start.clone();
    camp.pos.x += 100.0;
    let kills = Objective {
        loc: camp,
        text: "Kill 10 mobs".into(),
        kills: 10.0,
        mob_level: 1,
        uses: 0.0,
        extra: 0.0,
        dungeon: None,
        elite: false,
        count: 10.0,
        mobs: vec![],
        pull: 1.0,
        spawns: 10,
        pack: 1.0,
        spots: None,
        guard: Guard::default(),
        alts: vec![],
        after: 0,
        handed_by: vec![],
        child: None,
        child_name: None,
        vendors: vec![],
        auction: vec![],
        stock: None,
    };
    let mut gather = Objective {
        text: "Gather 8 objects".into(),
        kills: 0.0,
        uses: 8.0,
        count: 8.0,
        ..kills.clone()
    };
    gather.loc.pos.x += 50.0;
    p.model.quests[0].objectives = vec![kills];
    p.model.quests[1].objectives = vec![gather];
    p.model.initial.accepted = vec![0, 1];
    p
}

/// The last pass brings objectives done at the same place next to each other (the guide then
/// shows them together): the gathering among the kills saves its share.
#[test]
fn objectives_at_one_place_are_brought_together() {
    let p = camp_of_two_objectives();
    let planner = planner(&p);
    let (kill, gather) = (Stop::quest(0, Kind::Objective(0)), Stop::quest(1, Kind::Objective(0)));
    let accept = Stop::quest(2, Kind::Accept);
    let apart = vec![kill, accept, gather];
    let together = planner.route_together(apart.clone());
    assert_eq!(together, vec![kill, gather, accept]);
    let fighting = |route: &[Stop]| planner.simulate_full(route, None).unwrap().1.fighting;
    assert!(fighting(&together) < fighting(&apart) - 1.0);
}

/// A move that would make the route worse is not kept.
#[test]
fn bringing_together_never_makes_it_worse() {
    let p = camp_of_two_objectives();
    let planner = planner(&p);
    let (kill, gather) = (Stop::quest(0, Kind::Objective(0)), Stop::quest(1, Kind::Objective(0)));
    let apart = vec![kill, Stop::quest(2, Kind::Accept), gather];
    let places = vec![
        Some(p.model.quests[0].objectives[0].loc.pos),
        None,
        Some(p.model.quests[1].objectives[0].loc.pos),
    ];
    // Any order but this one scores worse.
    let kept = planner.bring_together(
        apart.clone(),
        |s| Some(*s),
        places,
        |r| Some(if r == apart.as_slice() { 0.0 } else { 1.0 }),
    );
    assert_eq!(kept, apart);
}

/// A quest the route takes and gives up when it goes gray costs `abandon_weight`: the same route
/// without it is better by that much (its taking costs only seconds).
#[test]
fn a_quest_taken_and_given_up_costs_the_route() {
    let mut p = quests_at_start(2);
    p.profile.to_level = 20;
    // Quest 1 takes the character to level 7: quest 0 (level 1, leading nowhere) is gray then.
    p.model.quests[1].xp = 11_200;
    let route = [
        Stop::quest(0, Kind::Accept),
        Stop::quest(1, Kind::Accept),
        Stop::quest(1, Kind::TurnIn),
    ];
    let without = [route[1], route[2]];
    let score = |p: &Prepared, route: &[Stop]| planner(p).simulate_full(route, None).unwrap();
    p.params.abandon_weight = 600.0;
    let ((given_up, spent), (left_out, _)) = (score(&p, &route), score(&p, &without));
    assert_eq!(spent.abandoned, 1);
    assert!(given_up - left_out > 590.0, "{given_up} vs {left_out}");
    p.params.abandon_weight = 0.0;
    let (given_up, _) = score(&p, &route);
    assert!(given_up - left_out < 30.0, "{given_up} vs {left_out}");
}

/// A road east of the start lined with mobs of level 1: three of quest 0's (its objective is
/// done at the end of the road) and three others. Farming on the way is on, without a cap, and
/// every mob met is killed (`farm_share` 1).
fn road_of_mobs() -> (Prepared, crate::world::Pos, crate::world::Pos) {
    let mut p = quests_at_start(1);
    p.profile.to_level = 10;
    p.world.terrain = crate::terrain::Terrain::default();
    (
        p.params.farm_on_way,
        p.params.farm_cap,
        p.params.farm_share,
        p.params.crowded,
    ) = (true, 1.0, 1.0, 0.0);
    let start = p.model.start.pos;
    let mut end = p.model.start.clone();
    end.pos.x += 400.0;
    let mut spawns = Vec::new();
    for i in 0..3 {
        let x = start.x + 100.0 * f64::from(i + 1);
        spawns.push((crate::world::Pos { x, ..start }, 1, 777));
        spawns.push((
            crate::world::Pos {
                x,
                y: start.y + 5.0,
                ..start
            },
            1,
            888,
        ));
    }
    p.model.quests[0].objectives = vec![Objective {
        loc: end.clone(),
        text: "Kill 3 wolves".into(),
        kills: 3.0,
        mob_level: 1,
        uses: 0.0,
        extra: 0.0,
        dungeon: None,
        elite: false,
        count: 3.0,
        mobs: vec![(777, "Wolf".into())],
        pull: 1.0,
        spawns: 3,
        pack: 1.0,
        spots: None,
        guard: Guard::default(),
        alts: vec![],
        after: 0,
        handed_by: vec![],
        child: None,
        child_name: None,
        vendors: vec![],
        auction: vec![],
        stock: None,
    }];
    p.model.farm = crate::model::FarmMobs::from_spawns(&spawns, &p.model.quests);
    (p, start, end.pos)
}

/// Mobs killed and XP farmed on the walk from `a` to `b`, from state `s`.
fn farmed_on_walk(
    planner: &Planner<'_>,
    mut s: super::State,
    a: &crate::world::Pos,
    b: &crate::world::Pos,
) -> (u32, i64) {
    let mut kills = 0;
    planner.farm_along(&mut s, a, b, &mut |_, e| {
        if let super::Event::Farm { kills: k, .. } = e {
            kills += k;
        }
    });
    (kills, s.spent.farm_xp)
}

/// The mobs of a quest of the log are not farmed on the way, to its objective or back: the
/// objective counts them where it is done (GitHub #20). Before it is taken, and once it is
/// turned in, they are mobs like the others.
#[test]
fn the_logs_quest_mobs_are_not_farmed_on_the_way() {
    let (p, start, end) = road_of_mobs();
    let planner = planner(&p);
    let s0 = planner.initial_state();
    assert_eq!(farmed_on_walk(&planner, s0.clone(), &start, &end).0, 6, "not taken yet");
    let mut taken = s0.clone();
    taken.accepted.set(0, true);
    assert_eq!(
        farmed_on_walk(&planner, taken.clone(), &start, &end).0,
        3,
        "on the way there"
    );
    let mut done = taken.clone();
    done.objectives[0] = 1;
    assert_eq!(
        farmed_on_walk(&planner, done.clone(), &end, &start).0,
        3,
        "on the way back"
    );
    let mut turned = done;
    turned.turned.set(0, true);
    assert_eq!(farmed_on_walk(&planner, turned, &start, &end).0, 6, "turned in");
}

/// Farming on the way kills `farm_share` of the mobs met, and fewer on a crowded server: the
/// share left by the other players around, as at an objective's spot (one in 1 + crowd ×
/// `crowd_players`).
#[test]
fn farming_on_the_way_kills_a_share_of_the_mobs_met() {
    let (mut p, start, end) = road_of_mobs();
    let xp = |p: &Prepared| {
        let planner = planner(p);
        farmed_on_walk(&planner, planner.initial_state(), &start, &end).1 as f64
    };
    let all = xp(&p);
    assert!(all > 0.0);
    p.params.farm_share = 0.8;
    let most = xp(&p);
    assert!((most - 0.8 * all).abs() <= 1.0, "{most} of {all}");
    (p.params.crowded, p.params.crowd_players) = (1.0, 3.0);
    let crowd = p.params.crowd_at(1);
    assert!(crowd > 0.0);
    let crowded = xp(&p);
    let expected = 0.8 * all / (1.0 + crowd * 3.0);
    assert!((crowded - expected).abs() <= 1.0, "{crowded} vs {expected}");
}

/// A quest given at the start whose mobs are a few levels above the character: nothing else to
/// do, the greedy construction grinds to the level they need and does it, rather than stopping.
#[test]
fn greedy_grinds_to_the_mobs_of_a_quest_in_reach() {
    let mut p = quests_at_start(1);
    p.profile.to_level = 10;
    p.world.terrain = crate::terrain::Terrain::default();
    let mut camp = p.model.start.clone();
    camp.pos.x += 100.0;
    p.model.quests[0].xp = 2000;
    p.model.quests[0].objectives = vec![Objective {
        loc: camp,
        text: "Kill the chief".into(),
        kills: 1.0,
        mob_level: 7,
        uses: 0.0,
        extra: 0.0,
        dungeon: None,
        elite: false,
        count: 1.0,
        mobs: vec![],
        pull: 1.0,
        spawns: 1,
        pack: 1.0,
        spots: None,
        guard: Guard::default(),
        alts: vec![],
        after: 0,
        handed_by: vec![],
        child: None,
        child_name: None,
        vendors: vec![],
        auction: vec![],
        stock: None,
    }];
    let route = planner(&p).greedy();
    assert!(route.contains(&Stop::quest(0, Kind::TurnIn)), "{route:?}");
}
