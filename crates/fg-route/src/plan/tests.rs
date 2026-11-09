//! Small routes built on the Classic database: the incremental evaluation of a changed route
//! gives the cost of its full replay, objectives started and done together.

use super::{Kind, Planner, Stop};
use crate::job::{self, Overrides, PlanRequest, Prepared};
use crate::model::{Guard, Initial, Objective, Pool};
use crate::xp::Edition;
use rusqlite::Connection;
use std::path::PathBuf;

mod dungeons;
mod movement;
mod moves;
mod profession_trainers;
mod shared_kills;
mod xp_buffs;
#[test]
fn completion_window_is_checked_after_exploration_on_the_trip() {
    let mut p = quests_at_start(1);
    p.world.terrain = crate::terrain::Terrain::default();
    p.params.explore_radius = 50.0;
    p.model.initial.xp = 399;
    p.model.initial.accepted = vec![0];
    let q = &mut p.model.quests[0];
    q.required = true;
    q.completion_window.max_level = Some(1);
    q.ends[0].pos.x += 1000.0;
    let area = crate::model::ExploreArea {
        zone: q.ends[0].zone,
        level: 1,
        pos: q.ends[0].pos,
    };
    p.model.explore_by_zone.insert(q.ends[0].zone, vec![0]);
    p.model.explore.push(area);
    let edit = super::Edit::Stop {
        stop: Stop::quest(0, Kind::TurnIn),
        farm: false,
        spot: None,
    };
    let report = planner(&p).edit(&[edit]);
    assert_eq!(
        report.steps[0].outcome,
        super::Outcome::Invalid("quest completion outside requested level range")
    );
    p.model.explore.clear();
    p.model.explore_by_zone.clear();
    assert_eq!(planner(&p).edit(&[edit]).errors(), 0);
}

fn repo() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../..")
}

/// The fixtures' base of an edition (`scripts/test-base.sh`): the game's classes, races, zones and
/// class spells, no quest.
pub(crate) fn base_of(edition: &str) -> Connection {
    fixture_of(edition, &[])
}

/// The fixtures' base of an edition with the fixture `files` (`tests/fixtures/<file>.toml`).
pub(crate) fn fixture_of(edition: &str, files: &[&str]) -> Connection {
    let base = crate::fixture::Fixture::new(Edition::from_key(edition).expect("an edition"));
    files.iter().fold(base, |f, name| f.file(name)).open()
}

/// Level 1 human warrior with only `n` quests of its own: taken and turned in at the start NPC,
/// nothing to do, no XP (each test sets what it checks). Nothing done on the way.
pub(crate) fn quests_at_start(n: usize) -> Prepared {
    use crate::testing::{Q, Scenario};
    at_start(
        Scenario::new()
            .levels(1, 2)
            .quest(Q::new(900, "Quest").level(1))
            .build(),
        n,
    )
}

#[test]
fn quest_gold_projection_keeps_starting_copper_and_exports_the_reward() {
    let mut p = quests_at_start(1);
    p.model.initial.money = 250;
    p.model.quests[0].reward_money = 1200;
    let planner = Planner::new(&p.model, &p.world, &p.params, &p.profile);
    let route = [Stop::quest(0, Kind::Accept), Stop::quest(0, Kind::TurnIn)];
    let (_, spent) = planner.simulate_full(&route, None).unwrap();
    assert_eq!(spent.gold_start, 250);
    assert_eq!(spent.gold_earned, 1200);
    let guide = crate::export::build(&planner, &p.profile, &route);
    assert!(
        guide
            .steps
            .iter()
            .any(|s| s.quest == Some(900_000) && s.money == Some(1200))
    );
}

#[test]
fn quest_gold_projection_does_not_pay_an_already_completed_quest_again() {
    let mut p = quests_at_start(1);
    p.model.initial.money = 250;
    p.model.initial.turned = vec![0];
    p.model.quests[0].reward_money = 1200;
    let planner = Planner::new(&p.model, &p.world, &p.params, &p.profile);
    let route = [Stop::quest(0, Kind::Accept), Stop::quest(0, Kind::TurnIn)];
    let (_, spent) = planner.simulate_full(&route, None).unwrap();
    assert_eq!(spent.gold_start, 250);
    assert_eq!(spent.gold_earned, 0);
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
    p.model.bundles = vec![Vec::new(); n];
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

/// The character of a request with its class's spells and power from the database, and one quest.
pub(crate) fn class_scenario(conn: &Connection, overrides: &Overrides, req: &PlanRequest) -> crate::testing::Scenario {
    use crate::testing::{Q, Scenario};
    let level = req.from_level.max(1);
    let mut scenario = Scenario::of_class(conn, overrides, req)
        .unwrap()
        .quest(Q::new(900, "Quest").level(level).min_level(level));
    // The quests the character did.
    for done in req.start.iter().flat_map(|s| &s.completed) {
        scenario = scenario.quest(Q::new(done.0, "Done").level(level).min_level(level));
    }
    scenario
}

pub(crate) fn class_prepared(conn: &Connection, overrides: &Overrides, req: &PlanRequest) -> Prepared {
    class_scenario(conn, overrides, req).build()
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
        direct_kill: None,
        pull: 1.0,
        spawns: 1,
        pack: 1.0,
        pool: Pool::default(),
        spots: None,
        guard: Guard::default(),
        beside_giver: false,
        item: false,
        alts: vec![],
        after: 0,
        handed_by: vec![],
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
        direct_kill: None,
        pull: 1.0,
        spawns: 20,
        pack: 1.0,
        pool: Pool::default(),
        spots: None,
        guard: Guard::default(),
        beside_giver: false,
        item: false,
        alts: vec![],
        after: 0,
        handed_by: vec![],
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
    let mut s = planner.sim().initial_state();
    s.pos.x += 300.0;
    let xp = s.spent.mob_xp;
    planner.sim().apply(&mut s, Stop::quest(0, Kind::Objective(0)), None);
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
    let fighting = |route: &[Stop]| fighting_with_turn_ins(&planner, route);
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

/// A small spot of `spawns` mobs of kind `mob`, back in `respawn` seconds, killed in 20 s each,
/// for `kills` kills: the objective's pool, its wait alone in its fixed time (as the model sets it).
fn small_spot(o: &mut Objective, mob: i64, spawns: f64, respawn: f64, kills: f64) {
    let rate = spawns / respawn;
    let wait = Pool::wait_for(kills, spawns, rate, 20.0);
    (o.kills, o.count, o.mobs) = (kills, kills, vec![(mob, format!("mob {mob}"))]);
    o.pool = Pool {
        mine: spawns,
        rate,
        each: 20.0,
        wait,
    };
    o.extra = wait;
}

/// Two kill objectives at one camp (the Defias and the Riverpaw of Gold Coast Quarry), each asking
/// more kills than its mobs' spawns: alone, each waits for its respawns; one after the other,
/// the mobs of one are killed while the other's come back. A rare mob's long wait is not spared
/// by the kills of others beyond their time. With another stop between them, each waits on its
/// own again.
#[test]
fn kills_at_one_camp_wait_for_respawns_together() {
    let mut p = camp_of_two_objectives();
    let mut second = p.model.quests[0].objectives[0].clone();
    small_spot(&mut p.model.quests[0].objectives[0], 1, 9.0, 300.0, 15.0);
    small_spot(&mut second, 2, 9.0, 300.0, 15.0);
    p.model.quests[1].objectives = vec![second];
    let alone = p.model.quests[0].objectives[0].pool.wait;
    assert!(alone > 60.0, "{alone}");
    let planner = planner(&p);
    let waiting = |route: &[Stop]| planner.simulate_full(route, None).unwrap().1.waiting;
    let (a, b) = (Stop::quest(0, Kind::Objective(0)), Stop::quest(1, Kind::Objective(0)));
    assert!((waiting(&[a]) - alone).abs() < 1e-6);
    // 30 kills among 18 spawns coming back every 17 s, quicker than a kill: no wait left.
    let together = waiting(&[a, b]);
    assert!(together.abs() < 1e-6, "{together} waited together");
    let apart = waiting(&[a, Stop::quest(2, Kind::Accept), b]);
    assert!((apart - 2.0 * alone).abs() < 1e-6, "{apart} with a stop between");
    // A rare mob instead (3 spawns back in 10 minutes): its wait less the other's 300 s of kills.
    small_spot(&mut p.model.quests[1].objectives[0], 2, 3.0, 600.0, 15.0);
    let rare = p.model.quests[1].objectives[0].pool.wait;
    let planner = super::tests::planner(&p);
    let waiting = |route: &[Stop]| planner.simulate_full(route, None).unwrap().1.waiting;
    let together = waiting(&[a, b]);
    assert!((together - (rare - 300.0)).abs() < 1e-6, "{together} {rare}");
}

/// An objective whose mobs live in a large group far off and a small one at the camp where the
/// character just killed another quest's mobs: alone, the small group's respawns cost more than
/// the walk to the large one; after the kills at the camp, they came back meanwhile and the
/// objective is done there.
#[test]
fn an_objective_is_done_where_the_kills_just_done_share_its_wait() {
    let mut p = camp_of_two_objectives();
    let camp = p.model.quests[0].objectives[0].loc.clone();
    small_spot(&mut p.model.quests[0].objectives[0], 1, 40.0, 300.0, 30.0);
    let mut far = p.model.quests[0].objectives[0].clone();
    small_spot(&mut far, 2, 30.0, 300.0, 15.0);
    far.loc.pos.x += 600.0;
    let mut here = far.clone();
    here.loc = camp.clone();
    // Alone: 10 respawns to wait for, 40 s each beyond the kill; after 30 kills (10 minutes), none.
    small_spot(&mut here, 2, 5.0, 300.0, 15.0);
    far.alts = vec![here];
    p.model.quests[1].objectives = vec![far];
    let planner = planner(&p);
    let sim = planner.sim();
    let mut s = sim.initial_state();
    assert_ne!(sim.spot(&s, 1, 0).loc.pos, camp.pos, "alone, the large group far off");
    sim.apply(&mut s, Stop::quest(0, Kind::Objective(0)), None);
    assert_eq!(sim.spot(&s, 1, 0).loc.pos, camp.pos, "after the kills at the camp");
}

/// The fighting of `route` with the quests whose objective it does turned in at its end: a quest
/// left in the log is grinding (`Planner::count_unfinished_as_grind`), not fighting.
fn fighting_with_turn_ins(planner: &Planner<'_>, route: &[Stop]) -> f64 {
    let mut full = route.to_vec();
    full.extend(
        route
            .iter()
            .filter(|s| s.kind == Kind::Objective(0))
            .map(|s| Stop::quest(s.index, Kind::TurnIn)),
    );
    planner.simulate_full(&full, None).unwrap().1.fighting
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
        direct_kill: None,
        pull: 1.0,
        spawns: 10,
        pack: 1.0,
        pool: Pool::default(),
        spots: None,
        guard: Guard::default(),
        beside_giver: false,
        item: false,
        alts: vec![],
        after: 0,
        handed_by: vec![],
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
    let fighting = |route: &[Stop]| fighting_with_turn_ins(&planner, route);
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

/// A class unlock kept below the guide's levels costs `unlock_delay_weight` for each level gained
/// before it is turned in: the same route with it done first is better by that much.
#[test]
fn a_late_class_unlock_costs_each_level_it_waits() {
    let mut p = quests_at_start(2);
    p.profile.to_level = 20;
    // Quest 1 takes the character to level 7.
    p.model.quests[1].xp = 11_200;
    p.model.quests[0].late_unlock = true;
    p.model.quests[0].level = 10;
    let (accept, turn_in) = (|i| Stop::quest(i, Kind::Accept), |i| Stop::quest(i, Kind::TurnIn));
    let first = [accept(0), turn_in(0), accept(1), turn_in(1)];
    let last = [accept(1), turn_in(1), accept(0), turn_in(0)];
    let score = |p: &Prepared, route: &[Stop]| {
        planner(p)
            .simulate_full(route, None)
            .unwrap_or_else(|| panic!("{route:?}"))
            .0
    };
    p.params.unlock_delay_weight = 0.0;
    let free = score(&p, &last) - score(&p, &first);
    p.params.unlock_delay_weight = 300.0;
    let weighed = score(&p, &last) - score(&p, &first);
    assert!(weighed - free > 1700.0, "{weighed} vs {free}");
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
        direct_kill: None,
        pull: 1.0,
        spawns: 3,
        pack: 1.0,
        pool: Pool::default(),
        spots: None,
        guard: Guard::default(),
        beside_giver: false,
        item: false,
        alts: vec![],
        after: 0,
        handed_by: vec![],
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
    planner.sim().farm_along(&mut s, a, b, &mut |_, e| {
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
    let s0 = planner.sim().initial_state();
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
        farmed_on_walk(&planner, planner.sim().initial_state(), &start, &end).1 as f64
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
        direct_kill: None,
        pull: 1.0,
        spawns: 1,
        pack: 1.0,
        pool: Pool::default(),
        spots: None,
        guard: Guard::default(),
        beside_giver: false,
        item: false,
        alts: vec![],
        after: 0,
        handed_by: vec![],
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
    use crate::testing::{At, Scenario};
    let mut p = Scenario::new()
        .levels(1, 2)
        .zone(2, "Wetlands")
        .zone(3, "Dustwallow")
        .flight_master(1, "Ironforge", 1, 50.0, 50.0)
        .flight_master(2, "Menethil Harbor", 2, 50.0, 50.0)
        .flight_master(3, "Theramore", 3, 50.0, 50.0)
        .boat(
            "Boat Menethil Harbor - Theramore",
            At {
                zone: 2,
                x: 52.0,
                y: 50.0,
            },
            At {
                zone: 3,
                x: 48.0,
                y: 50.0,
            },
            30.0,
        )
        .params(|p| p.link_wait = 0.0)
        .build();
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
    let mut s = planner.sim().initial_state();
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
    let mut s = planner.sim().initial_state();
    planner.sim().accept(&mut s, 0);
    assert_eq!(s.held, 1);
    let rewards = planner.growth().quest_rewards(&s, 0);
    planner.sim().turn_in(&mut s, 0, &rewards);
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
    let mut s = planner.sim().initial_state();
    let mut events = Vec::new();
    s.bags = 2.0;
    planner.sim().sell(&mut s, &mut |_, e| events.push(e));
    assert!(events.is_empty() && s.bags > 0.0, "not worth a stop yet");
    s.bags = (planner.growth().bag_room(1) * 0.6) as f32;
    let before = s.time;
    planner.sim().sell(&mut s, &mut |_, e| events.push(e));
    assert!(
        matches!(events[..], [super::state::Event::Sell { vendor: 0 }]),
        "{events:?}"
    );
    assert!(s.bags == 0.0 && s.time >= before + p.params.sell_time);
}

/// Inside a dungeon, no stop at a vendor nor a trainer by its entrance (the steps of a dungeon
/// whose places are not known stand at its entrance, in town): once out of it.
#[test]
fn no_vendor_nor_trainer_while_in_a_dungeon() {
    let mut p = quests_at_start(1);
    let mut vendor = p.model.start.clone();
    vendor.pos.x += 30.0;
    p.model.vendors = vec![vendor];
    let planner = planner(&p);
    let mut s = planner.sim().initial_state();
    let mut events = Vec::new();
    s.bags = (planner.growth().bag_room(1) * 0.6) as f32;
    s.inside = Some(0);
    planner.sim().sell(&mut s, &mut |_, e| events.push(e));
    planner.sim().train(&mut s, &mut |_, e| events.push(e));
    assert!(events.is_empty(), "{events:?}");
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
    // A group goes to the vendor when the fullest bags are: a hunter's, with a quiver.
    p.profile.professions.clear();
    p.profile.mates = vec![crate::model::Mate {
        class_name: "Hunter".into(),
        name: "Hunter".into(),
        race_id: 2,
        class_id: 3,
        spell_weights: vec![],
        professions: vec![],
    }];
    assert!((planner(&p).growth().bag_room(30) - hunter).abs() < 1e-9);
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
        direct_kill: None,
        pull: 1.0,
        spawns: 1,
        pack: 1.0,
        pool: Pool::default(),
        spots: None,
        guard: Guard::default(),
        beside_giver: false,
        item: false,
        alts: vec![],
        after: 0,
        handed_by: vec![],
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
        let s = planner.sim().start_state(planner.planned(&route));
        (
            planner.sim().gate_level(&s, route[0]),
            planner.sim().gate_level(&s, route[1]),
        )
    };
    let (accept, objective) = gates(&p);
    assert!(
        objective > 1 && accept == 1,
        "untimed: taken at once ({accept}, {objective})"
    );
    p.model.quests[0].time_limit = Some(600.0);
    let (accept, objective) = gates(&p);
    assert_eq!(accept, objective, "timed: taken at the mob's level");
    p.model.quests[0].required = true;
    p.model.quests[0].mandatory = true;
    p.model.quests[0].completion_window.min_level = Some(18);
    assert_eq!(gates(&p).0, 18, "wait for the turn-in window before starting the clock");
    let mut trace = Vec::new();
    let plan = planner(&p);
    let result = plan.simulate(&route, Some(&mut trace));
    assert!(result.is_some(), "a valid route: {:?}", plan.diagnose(&route));
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
    let mut s = planner.sim().start_state(planner.planned(&BOTH));
    for stop in BOTH {
        planner.sim().apply(&mut s, stop, None);
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
    let mut s = planner.sim().initial_state();
    s.level = 2;
    assert!(!planner.sim().ended(&s), "the other player is still level 1");
    s.mates[0].level = 2;
    assert!(planner.sim().ended(&s));
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
    let mut s = planner.sim().start_state(planner.planned(&BOTH));
    let mut events = Vec::new();
    for stop in BOTH {
        planner.sim().apply(&mut s, stop, Some(&mut events));
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
    let mut s = planner.sim().start_state(planner.planned(&BOTH));
    for stop in BOTH {
        planner.sim().apply(&mut s, stop, None);
    }
    assert_eq!(
        s.party(),
        [(1, 200), (1, 100)],
        "a quarter of a level apart: within the most"
    );
}

mod abilities;

#[test]
fn an_unbounded_required_quest_stays_in_the_log_when_gray() {
    let conn = crate::fixture::Fixture::new(Edition::Classic)
        .file("classic_northshire")
        .open();
    let o = Overrides::load_edition(&repo().join("overrides"), Edition::Classic).unwrap();
    let req=serde_json::from_value(serde_json::json!({"race":"human","class":"warrior","from_level":20,"to_level":21,"required_class_quests":[],"required_quests":[783],"start":{"level":20,"log":[{"id":783,"complete":true}]}})).unwrap();
    let p = job::prepare(&conn, &o, &req, &|_| {}).unwrap();
    let i = p.model.index[&783];
    assert!(
        p.model.quests[i].required && p.model.quests[i].mandatory,
        "model construction makes exact requirements mandatory"
    );
    let plan = planner(&p);
    let mut state = plan.sim().initial_state();
    plan.sim().abandon_hopeless(&mut state, &mut |_, _| {});
    assert!(state.accepted[i]);
}

/// Two versions of a must-have class quest, of two steps each: taking one closes the other. The
/// first is far away, the second at the start.
fn far_and_near_versions() -> Prepared {
    class_versions(quests_at_start(4))
}

fn class_versions(mut p: Prepared) -> Prepared {
    p.world.terrain = crate::terrain::Terrain::default();
    let ids: Vec<i64> = p.model.quests.iter().map(|q| q.id).collect();
    for q in &mut p.model.quests {
        q.mandatory = true;
    }
    p.model.quests[1].pre_all = vec![ids[0]];
    p.model.quests[3].pre_all = vec![ids[2]];
    for i in 0..2 {
        p.model.quests[i].exclusive = vec![ids[2], ids[3]];
        p.model.quests[i + 2].exclusive = vec![ids[0], ids[1]];
        p.model.quests[i].starts[0].pos.x += 5000.0;
        p.model.quests[i].ends[0].pos.x += 5000.0;
    }
    p
}

fn far_version_route() -> Vec<Stop> {
    vec![
        Stop::quest(0, Kind::Accept),
        Stop::quest(0, Kind::TurnIn),
        Stop::quest(1, Kind::Accept),
        Stop::quest(1, Kind::TurnIn),
    ]
}

/// No other move can change the version: a must-have quest stays, and the other is closed.
#[test]
fn the_other_version_of_a_class_quest_replaces_the_one_in_the_route() {
    use rand::SeedableRng;
    let p = far_and_near_versions();
    let planner = planner(&p);
    let mut route = far_version_route();
    let run = planner.run(&route).unwrap();
    let mut changed = None;
    let mut rng = rand::rngs::SmallRng::seed_from_u64(1);
    for _ in 0..64 {
        changed = changed.or_else(|| planner.swap_version(&mut route, &run, &mut rng));
    }
    let change = changed.expect("the other version is open");
    assert_eq!(change.first, 0);
    let quests: Vec<u32> = route.iter().map(|s| s.index).collect();
    assert!(quests.iter().all(|&q| q >= 2), "the old version is gone: {quests:?}");
    assert_eq!(route.len(), 4, "the whole new version: {quests:?}");
    let (with, spent) = planner.simulate_full(&route, None).unwrap();
    assert_eq!(spent.missing_class_quests, 0);
    assert!(with < run.total, "{with} vs {}", run.total);
}

#[test]
fn the_search_changes_to_the_better_version_of_a_class_quest() {
    let p = far_and_near_versions();
    let route = planner(&p).improve_while(
        far_version_route(),
        super::Effort::Tries(4000),
        || true,
        |_| {},
        |_, _, _| {},
    );
    let quests: Vec<u32> = route.iter().filter(|s| s.is_quest()).map(|s| s.index).collect();
    assert!(quests.contains(&2) && quests.contains(&3), "{quests:?}");
    assert!(!quests.contains(&0) && !quests.contains(&1), "{quests:?}");
}

/// A quest with no other version: the move does nothing, and draws nothing (routes of every
/// other character stay the same).
#[test]
fn a_class_quest_with_one_version_has_nothing_to_swap() {
    use rand::{RngExt, SeedableRng};
    let mut p = quests_at_start(2);
    p.model.quests[0].mandatory = true;
    let planner = planner(&p);
    let mut route = vec![Stop::quest(0, Kind::Accept), Stop::quest(0, Kind::TurnIn)];
    let run = planner.run(&route).unwrap();
    let mut rng = rand::rngs::SmallRng::seed_from_u64(1);
    let mut same = rand::rngs::SmallRng::seed_from_u64(1);
    assert!(planner.swap_version(&mut route, &run, &mut rng).is_none());
    assert_eq!(route.len(), 2);
    assert_eq!(rng.random::<u64>(), same.random::<u64>());
}

/// The move shares its slot with adding a new quest, the only one: a character with versions
/// still gets most of those tries, for the whole search.
#[test]
fn a_route_with_another_version_still_tries_new_quests() {
    use rand::SeedableRng;
    let p = far_and_near_versions();
    let planner = planner(&p);
    let route = far_version_route();
    let run = planner.run(&route).unwrap();
    let mut rng = rand::rngs::SmallRng::seed_from_u64(1);
    let swaps = (0..1000)
        .filter(|_| planner.swap_version(&mut route.clone(), &run, &mut rng).is_some())
        .count();
    assert!((50..=250).contains(&swaps), "{swaps} swaps of 1000 tries");
}

/// A quest the route took and never turned in is grinding in all but name: the time of its
/// objectives is counted as grinding, not as the fighting of a quest; turned in, it stays fighting.
#[test]
fn a_quest_never_turned_in_counts_as_grinding() {
    use crate::testing::{Q, Scenario};
    let p = Scenario::new()
        .levels(1, 3)
        .quest(
            Q::new(1, "Kills")
                .level(1)
                .xp(50)
                .giver(1, 40.0, 40.0)
                .kill("Boar", 1, 10, 1, 45.0, 45.0),
        )
        .build();
    let planner = planner(&p);
    let (accept, objective, turn_in) = (
        Stop::quest(0, Kind::Accept),
        Stop::quest(0, Kind::Objective(0)),
        Stop::quest(0, Kind::TurnIn),
    );
    let (_, done) = planner.simulate_full(&[accept, objective, turn_in], None).unwrap();
    let (_, left) = planner.simulate_full(&[accept, objective], None).unwrap();
    assert!(done.fighting > 1.0, "turned in: the quest's fighting {done:?}");
    assert!(left.fighting < 1.0, "never turned in: grinding {left:?}");
    assert!(left.grinding > done.grinding, "{} vs {}", left.grinding, done.grinding);
}

/// A town (50, 50) and boars 300 yards south: the ribs taken and turned in in town, the tools
/// taken 20 yards away on the way back from the boars. `tools` moves the tools' giver, `ribs`
/// where the ribs are turned in.
fn town_and_boars(tools: (f64, f64), ribs: (f64, f64)) -> Prepared {
    use crate::testing::{Q, Scenario};
    Scenario::new()
        .levels(1, 3)
        .quest(
            Q::new(1, "Ribs")
                .level(1)
                .xp(50)
                .giver(1, 50.0, 50.0)
                .turn_in_at(1, ribs.0, ribs.1)
                .kill("Boar", 1, 6, 1, 50.0, 80.0),
        )
        .quest(
            Q::new(2, "Tools")
                .level(1)
                .xp(50)
                .giver(1, tools.0, tools.1)
                .turn_in_at(1, 52.0, 50.0),
        )
        .build()
}

/// The ribs' boars killed, then the tools taken on the way back.
fn ribs_then_tools() -> [Stop; 5] {
    [
        Stop::quest(0, Kind::Accept),
        Stop::quest(0, Kind::Objective(0)),
        Stop::quest(1, Kind::Accept),
        Stop::quest(0, Kind::TurnIn),
        Stop::quest(1, Kind::TurnIn),
    ]
}

/// Whether tidying `ribs_then_tools` takes the tools before the boars.
fn tools_taken_first(p: &Prepared) -> bool {
    let route = ribs_then_tools();
    let tidied = planner(p).tidy(&route).map_or(route.to_vec(), |(r, _)| r);
    let at = |stop| tidied.iter().position(|&s| s == stop);
    at(route[2]) < at(route[1])
}

/// A quest taken in town on the way back from a trip is taken before leaving: it costs nothing.
#[test]
fn a_quest_in_town_is_taken_before_leaving() {
    assert!(tools_taken_first(&town_and_boars((52.0, 50.0), (50.0, 50.0))));
}

/// Not when it costs more than `tidy_slack` (no slack and a detour), nor when the giver is
/// out of town (beyond `tidy_radius`).
#[test]
fn tidying_stays_within_its_slack_and_radius() {
    // 40 yards north of town, the ribs turned in 100 yards north: the tools are on the way
    // back, taking them first is a detour.
    let mut p = town_and_boars((50.0, 46.0), (50.0, 40.0));
    p.params.tidy_slack = 0.0;
    assert!(!tools_taken_first(&p));
    p.params.tidy_slack = 1000.0;
    assert!(tools_taken_first(&p));
    p.params.tidy_radius = 30.0;
    assert!(!tools_taken_first(&p));
}

/// Elwynn's report: Thomas (70, 50) sends a report to Dughan (30, 50); the murlocs' bounty,
/// given by Thomas, needs the report delivered. Quest indices: 0 the report, 1 the bounty, 2 a
/// quest from Dughan back to Thomas.
fn report_then_bounty() -> Prepared {
    use crate::testing::{Q, Scenario};
    Scenario::new()
        .levels(1, 5)
        .quest(
            Q::new(1, "Report")
                .level(1)
                .giver(1, 70.0, 50.0)
                .turn_in_at(1, 30.0, 50.0),
        )
        .quest(
            Q::new(2, "Bounty")
                .level(1)
                .giver(1, 70.0, 50.0)
                .turn_in_at(1, 70.0, 50.0)
                .after_any(&[1])
                .kill("Murloc", 1, 8, 1, 75.0, 60.0),
        )
        .quest(
            Q::new(3, "Back east")
                .level(1)
                .giver(1, 30.0, 50.0)
                .turn_in_at(1, 70.0, 50.0),
        )
        .build()
}

/// A quest reinserted by the large moves is taken once its prerequisites are turned in, not at
/// the first visit of its giver: the bounty goes with the second visit to Thomas.
#[test]
fn a_reinserted_quest_is_taken_after_its_prerequisites() {
    let p = report_then_bounty();
    let planner = planner(&p);
    let mut route = vec![
        Stop::quest(0, Kind::Accept),
        Stop::quest(0, Kind::TurnIn),
        Stop::quest(2, Kind::Accept),
        Stop::quest(2, Kind::TurnIn),
    ];
    let s0 = planner.sim().initial_state();
    let mut levels = vec![1; route.len()];
    let mut places: Vec<_> = route.iter().map(|&s| planner.sim().pos_from_start(&s0, s)).collect();
    planner.insert_quest_leveled(&mut route, &mut levels, &mut places, &s0, 1);
    let at = |stop| route.iter().position(|&s| s == stop).unwrap();
    assert!(
        at(Stop::quest(1, Kind::Accept)) > at(Stop::quest(0, Kind::TurnIn)),
        "{route:?}"
    );
    assert!(planner.simulate(&route, None).is_some(), "{route:?}");
}

/// The quests a large move takes out are put back prerequisites first, whatever their order in
/// the route.
#[test]
fn removed_quests_go_back_prerequisites_first() {
    let p = report_then_bounty();
    let mut remove = vec![1, 2, 0];
    planner(&p).prerequisites_first(&mut remove);
    let at = |q| remove.iter().position(|&r| r == q).unwrap();
    assert!(at(0) < at(1), "{remove:?}");
}

/// Elwynn as the group guide did it. Goldshire (Dughan and the trainer, 30, 50) to the west,
/// Thomas (70, 50) to the east, 400 yards apart. Quest indices:
/// 0 Report to Thomas (Dughan to Thomas), 1 Deliver Thomas' Report (Thomas to Dughan, after 0),
/// 2 Protect the Frontier (Thomas, wolves at 75, 40), 3 Bounty on Murlocs (Thomas, after 1,
/// murlocs at 75, 65), 4 Westbrook (Dughan to Westbrook, 10, 50, once 3 is done), 5 Defias (Dughan once 0 is
/// done, at 65, 45).
fn elwynn_trips() -> Prepared {
    use crate::model::testing::At;
    use crate::testing::{Obj, Q, Scenario};
    let (dughan, thomas) = ((30.0, 50.0), (70.0, 50.0));
    let q = |id, name, from: (f64, f64), to: (f64, f64)| {
        Q::new(id, name)
            .level(1)
            .xp(10)
            .giver(1, from.0, from.1)
            .turn_in_at(1, to.0, to.1)
    };
    Scenario::new()
        .levels(1, 10)
        .trainer(1, dughan.0, dughan.1)
        .quest(q(1, "Report to Thomas", dughan, thomas).xp(3000))
        .quest(q(2, "Deliver Thomas' Report", thomas, dughan).after(&[1]))
        .quest(q(3, "Protect the Frontier", thomas, thomas).kill("Wolf", 1, 8, 1, 75.0, 40.0))
        .quest(
            q(4, "Bounty on Murlocs", thomas, thomas)
                .after(&[2])
                .kill("Murloc", 1, 8, 1, 75.0, 65.0),
        )
        .quest(q(5, "Westbrook", dughan, (10.0, 50.0)).after(&[4]))
        .quest(q(6, "Defias", dughan, dughan).after(&[1]).objective(Obj::kill(
            "Defias",
            1,
            8,
            At {
                zone: 1,
                x: 65.0,
                y: 45.0,
            },
        )))
        .build()
}

fn stops(list: &[(u32, Kind)]) -> Vec<Stop> {
    list.iter().map(|&(q, k)| Stop::quest(q, k)).collect()
}

/// The guide's route: east, west, east (the report taken late), west to deliver it, east again
/// for the murlocs (the frontier turned in only then), west.
fn elwynn_three_trips() -> Vec<Stop> {
    use Kind::{Accept as A, Objective as O, TurnIn as T};
    stops(&[
        (0, A),
        (0, T),
        (2, A),
        (2, O(0)), // east
        (5, A),    // west
        (5, O(0)),
        (1, A), // east
        (5, T),
        (1, T), // west
        (3, A),
        (2, T),
        (3, O(0)),
        (3, T), // east
        (4, A),
        (4, T), // west
    ])
}

/// The same quests in two trips: the report taken at once and delivered on the first way back.
fn elwynn_two_trips() -> Vec<Stop> {
    use Kind::{Accept as A, Objective as O, TurnIn as T};
    stops(&[
        (0, A),
        (0, T),
        (1, A),
        (2, A),
        (2, O(0)), // east
        (1, T),
        (5, A), // west
        (3, A),
        (5, O(0)),
        (2, T),
        (3, O(0)),
        (3, T), // east
        (5, T),
        (4, A),
        (4, T), // west
    ])
}

/// The route best moving one stop, or a block of up to four, anywhere; with its cost.
fn best_block_move(planner: &Planner<'_>, route: &[Stop]) -> (f64, Vec<Stop>) {
    let mut best = (planner.simulate(route, None).unwrap(), route.to_vec());
    for len in 1..=4.min(route.len()) {
        for from in 0..=route.len() - len {
            for to in 0..=route.len() - len {
                let mut r = route.to_vec();
                let block: Vec<Stop> = r.drain(from..from + len).collect();
                r.splice(to..to, block);
                if let Some(c) = planner.simulate(&r, None)
                    && c < best.0 - 0.5
                {
                    best = (c, r);
                }
            }
        }
    }
    best
}

/// The group guide's Elwynn: the report to Thomas levels the character up with its training
/// late. When the trip to the trainer left at once, taking Thomas' report right then cost a trip
/// there and back: every route that took it later was better, and the murlocs it opens took a
/// third trip east no move of one stop or a few could save. The trip now leaves with the route:
/// moving stops from the guide's three trips finds the report taken at once, and two trips.
#[test]
fn moves_from_the_guides_three_trips_find_two() {
    let mut p = elwynn_trips();
    p.params.hearth_min_saving = f64::INFINITY;
    let planner = planner(&p);
    let mut route = elwynn_three_trips();
    loop {
        let (_, better) = best_block_move(&planner, &route);
        if better == route {
            break;
        }
        route = better;
    }
    let at = |stop| route.iter().position(|&s| s == stop).unwrap();
    assert_eq!(
        at(Stop::quest(1, Kind::Accept)),
        at(Stop::quest(0, Kind::TurnIn)) + 1,
        "{route:?}"
    );
    let best = planner.simulate(&route, None).unwrap();
    assert!(
        best < planner.simulate(&elwynn_two_trips(), None).unwrap() + 0.5,
        "{route:?}"
    );
}

/// Thomas (70, 50) and the trainer in Goldshire (30, 50), 400 yards apart: the report to Thomas
/// takes the character three levels up (its training late), the next quest is taken from
/// Thomas too and handed in at Goldshire. Quest indices: 0 the report, 1 the next quest.
fn late_training_at_thomas() -> Prepared {
    use crate::testing::{Q, Scenario};
    Scenario::new()
        .levels(1, 10)
        .trainer(1, 30.0, 50.0)
        .quest(
            Q::new(1, "Report to Thomas")
                .level(1)
                .xp(3000)
                .giver(1, 30.0, 50.0)
                .turn_in_at(1, 70.0, 50.0),
        )
        .quest(
            Q::new(2, "Deliver Thomas' Report")
                .level(1)
                .xp(10)
                .giver(1, 70.0, 50.0)
                .turn_in_at(1, 30.0, 50.0),
        )
        .build()
}

/// A training trip due where the route has another stop at hand waits for it: the trip to the
/// trainer leaves with the route, it does not go there and back first.
#[test]
fn a_late_training_waits_for_the_stops_at_hand() {
    let p = late_training_at_thomas();
    let planner = planner(&p);
    let route = [
        Stop::quest(0, Kind::Accept),
        Stop::quest(0, Kind::TurnIn),
        Stop::quest(1, Kind::Accept),
        Stop::quest(1, Kind::TurnIn),
    ];
    let mut trace = Vec::new();
    planner.simulate(&route, Some(&mut trace)).unwrap();
    let at = |f: &dyn Fn(&super::Event) -> bool| trace.iter().position(|t| f(&t.event));
    let train = at(&|e| matches!(e, super::Event::Train { .. })).expect("trained");
    let taken = at(&|e| matches!(e, super::Event::Stop { stop, .. } if *stop == route[2])).unwrap();
    assert!(
        train > taken,
        "{:?}",
        trace.iter().map(|t| &t.event).collect::<Vec<_>>()
    );
}

/// A town (50, 50) and a camp 480 yards south (50, 98): the camp's quest taken in town, two
/// town quests taken and turned in on a return from the camp. Quest indices: 0 the camp's,
/// 1 and 2 the town's.
fn town_and_camp() -> Prepared {
    use crate::testing::{Q, Scenario};
    let town = |id, name| {
        Q::new(id, name)
            .level(1)
            .xp(10)
            .giver(1, 51.0, 50.0)
            .turn_in_at(1, 52.0, 50.0)
    };
    Scenario::new()
        .levels(1, 10)
        .quest(
            Q::new(1, "Camp")
                .level(1)
                .xp(10)
                .giver(1, 50.0, 50.0)
                .turn_in_at(1, 50.0, 99.0)
                .kill("Boar", 1, 6, 1, 50.0, 98.0),
        )
        .quest(town(2, "Tools"))
        .quest(town(3, "Supplies"))
        .build()
}

/// A return to a place for stops it could have done before is merged into the earlier visit:
/// the town's quests are done before leaving for the camp, not on a trip back.
#[test]
fn a_return_to_a_place_is_merged_into_the_visit_before() {
    let p = town_and_camp();
    let planner = planner(&p);
    let (a, o, t) = (Kind::Accept, Kind::Objective(0), Kind::TurnIn);
    let route = stops(&[(0, a), (0, o), (0, t), (1, a), (2, a), (1, t), (2, t)]);
    let (merged, cost) = planner.merge_visits(&route).expect("merged");
    assert_eq!(merged, stops(&[(0, a), (1, a), (2, a), (1, t), (2, t), (0, o), (0, t)]));
    assert!(cost < planner.simulate(&route, None).unwrap());
}
