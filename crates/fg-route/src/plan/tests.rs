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

fn database_of(edition: &str) -> Connection {
    static UNPACK: Mutex<()> = Mutex::new(());
    let _guard = UNPACK.lock().unwrap();
    let (path, packed) = (
        repo().join(format!("data/{edition}.sqlite")),
        repo().join(format!("data/{edition}.sqlite.zst")),
    );
    let modified = |p: &PathBuf| std::fs::metadata(p).and_then(|m| m.modified()).ok();
    if !path.exists() || modified(&packed) > modified(&path) {
        let data = zstd::decode_all(std::fs::File::open(&packed).unwrap()).unwrap();
        std::fs::write(&path, data).unwrap();
    }
    Connection::open(path).unwrap()
}

/// The Classic database, unpacked next to the packed one when missing or older.
fn database() -> Connection {
    database_of("classic")
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
        required_quests: vec![],
        professions: vec![],
        locale: None,
        mates: vec![],
        start: None,
    };
    at_start(job::prepare(&database(), &overrides, &request, &|_| {}).unwrap(), n)
}

fn at_start(mut p: Prepared, n: usize) -> Prepared {
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
    p.model.initial = Initial {
        abilities: p.model.initial.abilities,
        trained: p.model.initial.trained,
        ..Initial::default()
    };
    p.model.power = None;
    p.model.trainers.clear();
    p.model.teleports.clear();
    p.model.vendors.clear();
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
        beside_giver: false,
        item: false,
        alts: vec![],
        after: 0,
        handed_by: vec![],
        child: None,
        child_name: None,
        vendors: vec![],
        auction: vec![],
        slot: None,
        lasts: None,
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
        beside_giver: false,
        item: false,
        alts: vec![],
        after: 0,
        handed_by: vec![],
        child: None,
        child_name: None,
        vendors: vec![],
        auction: vec![],
        slot: None,
        lasts: None,
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
        beside_giver: false,
        item: false,
        alts: vec![],
        after: 0,
        handed_by: vec![],
        child: None,
        child_name: None,
        vendors: vec![],
        auction: vec![],
        slot: None,
        lasts: None,
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

/// A profession quest (Camping 101: its reward is the trainer's recipe, not XP) taken early and
/// handed in once gray is not given up, and leaving it out costs `profession_quest_weight`, once
/// for a group of exclusive alternatives (the same quest at another race's trainer).
#[test]
fn profession_quests_are_kept_once_gray() {
    let mut p = quests_at_start(3);
    p.profile.to_level = 20;
    // Quest 1 takes the character to level 7: quest 0 (level 1, leading nowhere) is gray then.
    p.model.quests[1].xp = 11_200;
    let mut skinning = crate::model::ProfessionPlan::new(&crate::profession::ProfessionGoal {
        key: "skinning".into(),
        target: 300,
        start_level: None,
        milestones: vec![],
    })
    .unwrap();
    skinning.curve = vec![1; 61];
    p.model.professions = vec![skinning];
    let (camping, other) = (p.model.quests[0].id, p.model.quests[2].id);
    p.model.quests[0].skill = Some((0, 1));
    p.model.quests[2].skill = Some((0, 1));
    p.model.quests[0].exclusive = vec![other];
    p.model.quests[2].exclusive = vec![camping];
    p.params.profession_quest_weight = 300.0;
    let route = [
        Stop::quest(0, Kind::Accept),
        Stop::quest(1, Kind::Accept),
        Stop::quest(1, Kind::TurnIn),
        Stop::quest(0, Kind::TurnIn),
    ];
    let without = [route[1], route[2]];
    let score = |p: &Prepared, route: &[Stop]| planner(p).simulate_full(route, None).unwrap();
    let ((kept, spent), (left_out, spent_without)) = (score(&p, &route), score(&p, &without));
    assert_eq!(spent.abandoned, 0, "not given up once gray");
    assert_eq!(spent.missing_profession_quests, 0);
    assert_eq!(spent_without.missing_profession_quests, 1, "one for both alternatives");
    assert!(left_out - kept > 270.0, "{kept} vs {left_out}");
    // Without the profession leveled, the quest is nothing the guide misses.
    p.model.quests[0].skill = None;
    p.model.quests[2].skill = None;
    assert_eq!(score(&p, &without).1.missing_profession_quests, 0);
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
        beside_giver: false,
        item: false,
        alts: vec![],
        after: 0,
        handed_by: vec![],
        child: None,
        child_name: None,
        vendors: vec![],
        auction: vec![],
        slot: None,
        lasts: None,
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
    let mut taken = s0;
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
        beside_giver: false,
        item: false,
        alts: vec![],
        after: 0,
        handed_by: vec![],
        child: None,
        child_name: None,
        vendors: vec![],
        auction: vec![],
        slot: None,
        lasts: None,
        stock: None,
    }];
    let route = planner(&p).greedy();
    assert!(route.contains(&Stop::quest(0, Kind::TurnIn)), "{route:?}");
}

/// A trip flies to a port before its boat, and flies on from where a boat lands, when that is
/// faster: Ironforge to Theramore by the flight to Menethil Harbor and the boat, and back.
#[test]
fn trips_fly_to_a_boat_and_from_it() {
    use super::state::Leg;
    let mut p = quests_at_start(0);
    p.params.gold_weight = 0.0;
    let planner = planner(&p);
    let trips = planner.trips();
    let node = |name: &str| {
        (p.world.taxi_nodes.iter())
            .position(|n| n.name.starts_with(name))
            .unwrap()
    };
    let (ironforge, menethil, theramore) = (node("Ironforge"), node("Menethil"), node("Theramore"));
    let boat = (p.world.links.iter())
        .position(|l| l.name == "Boat Menethil Harbor - Theramore")
        .unwrap();
    let back = (p.world.links.iter())
        .rposition(|l| l.name == "Boat Menethil Harbor - Theramore")
        .unwrap();
    let known = (1u128 << ironforge) | (1u128 << menethil);
    let (a, b) = (p.world.taxi_nodes[ironforge].pos, p.world.taxi_nodes[theramore].pos);
    let (with_flights, leg) = trips.leg(&a, &b, 30, known, 0);
    assert!(
        matches!(leg, Leg::FlyLink(f, t, k, l) if (f, t, k, l) == (ironforge, menethil, boat, boat)),
        "{leg:?}"
    );
    let (by_boat, _) = trips.leg(&a, &b, 30, 1u128 << ironforge, 0);
    assert!(with_flights < by_boat, "{with_flights} vs {by_boat}");
    let (_, leg) = trips.leg(&b, &a, 30, known, 0);
    assert!(
        matches!(leg, Leg::LinkFly(k, l, f, t) if (k, l, f, t) == (back, back, menethil, ironforge)),
        "{leg:?}"
    );
}

/// A kill's loot fills the bags (`Params::loot_per_kill`) up to the room a level 1 character
/// has; what no longer fits costs `bag_weight` a slot, and quest items take room too.
#[test]
fn loot_fills_the_bags_and_what_no_longer_fits_costs() {
    let p = quests_at_start(1);
    let planner = planner(&p);
    let mut s = planner.initial_state();
    let (room, each) = (planner.growth().bag_room(1), p.params.loot_per_kill);
    planner.growth().fill_bags(&mut s, room / each / 2.0);
    assert!((f64::from(s.bags) - room / 2.0).abs() < 1e-3, "half full: {}", s.bags);
    assert!(s.spent.bag_overflow.abs() < 1e-9, "all of it fits");
    s.held = 2;
    planner.growth().fill_bags(&mut s, room / each);
    assert!(
        (f64::from(s.bags) - (room - 2.0)).abs() < 1e-3,
        "full but the quest items: {}",
        s.bags
    );
    let lost = room / 2.0 + 2.0;
    assert!(
        (s.spent.bag_overflow - lost * p.params.bag_weight).abs() < 1e-3,
        "{}",
        s.spent.bag_overflow
    );
}

/// An item given on accepting takes a slot until the turn-in.
#[test]
fn quest_items_take_a_slot_until_the_turn_in() {
    let mut p = quests_at_start(1);
    p.model.quests[0].carried = true;
    let planner = planner(&p);
    let mut s = planner.initial_state();
    planner.accept(&mut s, 0);
    assert_eq!(s.held, 1);
    planner.turn_in(&mut s, 0);
    assert_eq!(s.held, 0);
}

/// Bags half full by a vendor: the loot is sold there, a step of the guide; nearly empty, no.
#[test]
fn a_vendor_on_the_way_empties_half_full_bags() {
    let mut p = quests_at_start(1);
    let mut vendor = p.model.start.clone();
    vendor.pos.x += 30.0;
    p.model.vendors = vec![vendor];
    let planner = planner(&p);
    let mut s = planner.initial_state();
    let mut events = Vec::new();
    s.bags = 2.0;
    planner.sell(&mut s, &mut |_, e| events.push(e));
    assert!(events.is_empty() && s.bags > 0.0, "not worth a stop yet");
    s.bags = (planner.growth().bag_room(1) * 0.6) as f32;
    let before = s.time;
    planner.sell(&mut s, &mut |_, e| events.push(e));
    assert!(
        matches!(events[..], [super::state::Event::Sell { vendor: 0 }]),
        "{events:?}"
    );
    assert!(s.bags == 0.0 && s.time >= before + p.params.sell_time);
}

/// What the bags keep for the class and the gathering professions: a hunter's quiver takes a bag,
/// a herbalist keeps a share for the herbs from the level it is taken.
#[test]
fn the_class_and_gathering_keep_bag_room() {
    let mut p = quests_at_start(1);
    let warrior = planner(&p).growth().bag_room(30);
    assert!(
        (warrior - (crate::params::bag_slots(30) - 4.0)).abs() < 1e-9,
        "{warrior}"
    );
    p.profile.class_id = 3;
    let hunter = planner(&p).growth().bag_room(30);
    assert!(
        (warrior - hunter - (crate::params::bag_slots(30) - 16.0) / 4.0).abs() < 1e-9,
        "{hunter}"
    );
    p.profile.class_id = 1;
    p.profile.professions = vec![crate::profession::ProfessionGoal {
        key: "herbalism".into(),
        target: 300,
        start_level: Some(10),
        milestones: vec![],
    }];
    assert!(
        (planner(&p).growth().bag_room(5) - (crate::params::bag_slots(5) - 4.0)).abs() < 1e-9,
        "not taken yet"
    );
    let herbalist = planner(&p).growth().bag_room(30);
    assert!(
        (warrior - herbalist - p.params.gather_share * crate::params::bag_slots(30)).abs() < 1e-9,
        "{herbalist}"
    );
}

/// A timed quest whose mob is too strong for the character is taken once it is strong enough
/// for it (the grinding comes before the clock starts), and turned in in time; untimed, the same
/// quest is taken at once and its mob waited for.
#[test]
fn timed_quest_taken_once_strong_enough_for_its_objectives() {
    let mut p = quests_at_start(1);
    p.profile.to_level = 20;
    let mut loc = p.model.start.clone();
    loc.pos.x += 50.0;
    p.model.quests[0].objectives = vec![Objective {
        loc,
        text: "Kill a strong mob".into(),
        kills: 1.0,
        mob_level: 12,
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
        beside_giver: false,
        item: false,
        alts: vec![],
        after: 0,
        handed_by: vec![],
        child: None,
        child_name: None,
        vendors: vec![],
        auction: vec![],
        slot: Some(1),
        lasts: None,
        stock: None,
    }];
    // Of the mob's level (not gray once there), offered from level 1.
    (p.model.quests[0].xp, p.model.quests[0].level) = (100, 12);
    let route = [
        Stop::quest(0, Kind::Accept),
        Stop::quest(0, Kind::Objective(0)),
        Stop::quest(0, Kind::TurnIn),
    ];
    let gates = |p: &Prepared| {
        let planner = planner(p);
        let s = planner.start_state(planner.planned(&route));
        (planner.gate_level(&s, route[0]), planner.gate_level(&s, route[1]))
    };
    let (accept, objective) = gates(&p);
    assert!(
        objective > 1 && accept == 1,
        "untimed: taken at once ({accept}, {objective})"
    );
    p.model.quests[0].time_limit = Some(600.0);
    let (accept, objective) = gates(&p);
    assert_eq!(accept, objective, "timed: taken at the mob's level");
    let mut trace = Vec::new();
    planner(&p).simulate(&route, Some(&mut trace)).expect("a valid route");
    let at = |kind: Kind| {
        trace.iter().find_map(|t| match t.event {
            super::state::Event::Stop { stop, .. } if stop.kind == kind => Some(t.level),
            _ => None,
        })
    };
    assert!(
        at(Kind::Accept).is_some_and(|level| level >= objective),
        "{:?}",
        at(Kind::Accept)
    );
    assert!(at(Kind::TurnIn).is_some(), "turned in in time");
    assert!(
        !trace
            .iter()
            .any(|t| matches!(t.event, super::state::Event::Abandon { .. })),
        "not given up"
    );
}

/// Two players: quest 0 is still for both, quest 1 only for the character (the other did it).
fn duo(xp: i64) -> Prepared {
    let mut p = quests_at_start(2);
    for q in &mut p.model.quests {
        q.xp = xp;
    }
    p.model.initial.mates = vec![crate::model::MateStart {
        level: 1,
        ..crate::model::MateStart::default()
    }];
    p.model.initial.need = vec![0b11, 0b01];
    p
}

const BOTH: [Stop; 4] = [
    Stop {
        index: 0,
        kind: Kind::Accept,
    },
    Stop {
        index: 0,
        kind: Kind::TurnIn,
    },
    Stop {
        index: 1,
        kind: Kind::Accept,
    },
    Stop {
        index: 1,
        kind: Kind::TurnIn,
    },
];

/// Each player gets the XP of the quests still for it, the kills' XP at its own level. The
/// lowest one is the character planned for (`State::order`).
#[test]
fn each_player_gets_the_xp_of_its_quests() {
    let mut p = duo(100);
    p.params.group_sync_max = 10.0;
    let planner = planner(&p);
    let mut s = planner.start_state(planner.planned(&BOTH));
    for stop in BOTH {
        planner.apply(&mut s, stop, None);
    }
    assert_eq!(s.party(), [(1, 200), (1, 100)]);
    assert_eq!((s.level, s.xp), (1, 100), "the one behind is planned for");
    planner.growth().gain_kills(&mut s, 50);
    assert_eq!(s.party(), [(1, 250), (1, 150)], "same level: the same kill XP");
}

/// The guide is over once everyone reached the target level, not the character alone.
#[test]
fn the_guide_waits_for_everyone() {
    let p = duo(0);
    let planner = planner(&p);
    let mut s = planner.initial_state();
    s.level = 2;
    assert!(!planner.ended(&s), "the other player is still level 1");
    s.mates[0].level = 2;
    assert!(planner.ended(&s));
}

/// The time the players are levels apart beyond the window costs the score; within it nothing.
#[test]
fn levels_apart_cost_the_score() {
    let mut p = duo(200);
    (p.params.group_sync_window, p.params.group_sync_max) = (0.05, 10.0);
    p.model.quests[0].start_uses = 60.0;
    let route = [BOTH[2], BOTH[3], BOTH[0], BOTH[1]];
    let planner = planner(&p);
    let (_, apart) = planner.simulate_full(&route, None).unwrap();
    // Quest 1 puts the character 200 XP ahead (a level is 400 at level 1): half a level apart
    // while taking quest 0, beyond the window.
    let bar = planner.rules().to_next_level(1) as f64;
    let over = 200.0 / bar - p.params.group_sync_window;
    assert!(apart.desync > 0.0, "{apart:?}");
    assert!(
        apart.desync <= over * (apart.total + 1.0),
        "{} for {over} levels",
        apart.desync
    );
    p.model.initial.need = vec![0b11, 0b11];
    let planner = Planner::new(&p.model, &p.world, &p.params, &p.profile);
    let (_, together) = planner.simulate_full(&route, None).unwrap();
    assert_eq!(together.desync, 0.0);
}

/// Together once, then further apart than `group_sync_max`: the player behind kills mobs on
/// its own until it is within half the window of the one ahead. Below the most, nothing.
#[test]
fn the_player_behind_catches_up() {
    let mut p = duo(100);
    (p.params.group_sync_window, p.params.group_sync_max) = (0.05, 0.1);
    let planner = planner(&p);
    let mut s = planner.start_state(planner.planned(&BOTH));
    let mut events = Vec::new();
    for stop in BOTH {
        planner.apply(&mut s, stop, Some(&mut events));
    }
    let caught = events.iter().find_map(|t| match t.event {
        super::Event::CatchUp { members, seconds, .. } => Some((members, seconds)),
        _ => None,
    });
    let (members, seconds) = caught.expect("a catch-up");
    assert_eq!(members, 0b10, "the other player, behind");
    assert!(seconds > 0.0);
    assert!(planner.growth().spread(&s) <= 0.05, "{}", planner.growth().spread(&s));
    assert_eq!(s.party()[0], (1, 200), "the character waits");
    p.params.group_sync_max = 0.3;
    let planner = Planner::new(&p.model, &p.world, &p.params, &p.profile);
    let mut s = planner.start_state(planner.planned(&BOTH));
    for stop in BOTH {
        planner.apply(&mut s, stop, None);
    }
    assert_eq!(
        s.party(),
        [(1, 200), (1, 100)],
        "a quarter of a level apart: within the most"
    );
}

mod abilities;
