//! Unrewarded outdoor work remains grinding after a real quest timeout.

use super::{Kind, Prepared, Stop, planner};
use crate::plan::state::{Event, State, Timed};
use crate::testing::{At, Obj, Q, Scenario};
use crate::xp::Edition;

/// Four level-one wolves give 200 XP and take 96 seconds. The 450-yard walk
/// then crosses the timed quest's deadline, before its remaining visit objective.
/// An unrelated 400-XP reward ends the guide without any fallback grinding.
fn scenario(timed: bool, direct: bool, shared_reward: bool) -> (Prepared, Vec<Stop>) {
    let far = At {
        zone: 1,
        x: 95.0,
        y: 50.0,
    };
    let mut p = Scenario::new()
        .levels(1, 2)
        .quest(
            Q::new(1, "Unrewarded wolves")
                .xp(0)
                .kill("Wolf", 1, 4, 1, 50.0, 50.0)
                .objective(Obj::visit("Report after the walk", far).after(&[0])),
        )
        .quest(Q::new(2, "Independent reward").xp(400).giver(1, 95.0, 50.0))
        .quest(
            Q::new(3, "Shared wolves")
                .xp(0)
                .kill("Wolf", 1, 4, 1, 50.0, 50.0)
                .turn_in_at(1, 95.0, 50.0),
        )
        .params(|params| {
            params.edition = Edition::Classic;
            params.camp_radius = 0.0;
            params.explore_radius = 0.0;
            params.farm_on_way = false;
            params.farm_cap = 0.0;
            params.grind_cap = 0.0;
            params.allow_grind = true;
            params.grind_weight = 1.5;
            params.grind_over_weight = 4.0;
            params.kill_time = 24.0;
            params.object_time = 0.0;
            params.stop_overhead = 0.0;
            params.bag_weight = 0.0;
            params.abandon_weight = 0.0;
            params.zone_change_weight = 0.0;
            params.keep_gray = true;
        })
        .build();
    p.model.quests[0].time_limit = timed.then_some(200.0);
    for i in [0, 2] {
        let o = &mut p.model.quests[i].objectives[0];
        o.loc.id = 777;
        o.direct_kill = direct.then_some(777);
        o.mobs = vec![(777, "Wolf".into())];
        o.spawns = 100;
        o.extra = 0.0;
    }
    let mut route = vec![Stop::quest(0, Kind::Accept)];
    if shared_reward {
        route.push(Stop::quest(2, Kind::Accept));
    }
    route.extend([
        Stop::quest(0, Kind::Objective(0)),
        Stop::quest(1, Kind::Accept),
        Stop::quest(0, Kind::Objective(1)),
    ]);
    if shared_reward {
        route.push(Stop::quest(2, Kind::TurnIn));
    }
    route.push(Stop::quest(1, Kind::TurnIn));
    (p, route)
}

fn replay(timed: bool, direct: bool, shared_reward: bool) -> (f64, State, Vec<Timed>) {
    let (p, route) = scenario(timed, direct, shared_reward);
    let mut trace = Vec::new();
    let (score, end) = planner(&p)
        .simulate_state(&route, Some(&mut trace))
        .expect("the complete route is valid, including the timed failure");
    assert_eq!(end.level, 2);
    assert_eq!(end.spent.mob_xp, 200, "exactly four actual outdoor kills");
    assert_eq!(end.spent.quest_xp, 400, "only the unrelated quest supplies XP");
    assert_eq!(end.spent.grind_xp, 0, "no fallback grind obscures the classification");
    assert_eq!(end.spent.farming, 0.0);
    assert_eq!(end.spent.explore_xp, 0);
    assert!(!end.rewarded(0), "the wolves quest never earns a reward");
    assert_eq!(end.rewarded(2), shared_reward);
    assert_eq!(end.dropped.contains(&0), timed);
    let kill = trace
        .iter()
        .find(|t| matches!(&t.event, Event::Stop { stop, .. } if *stop == Stop::quest(0, Kind::Objective(0))))
        .expect("the four kills really happened before any timeout");
    assert_eq!(kill.xp, 200);
    if timed {
        let abandoned = trace
            .iter()
            .find(|t| matches!(&t.event, Event::Abandon { quest: 0 }))
            .expect("normal timed expiry abandons the quest");
        assert!(abandoned.time > kill.time);
    } else {
        assert!(end.accepted[0] && !end.turned[0]);
    }
    (score, end, trace)
}

fn assert_unrewarded(score: f64, end: &State) {
    assert_eq!(end.spent.grinding, 96.0, "four unrewarded kills remain grinding");
    assert_eq!(end.spent.grind_over, 96.0, "zero allowance puts all work over the cap");
    assert_eq!(end.spent.fighting, 0.0);
    // The time is already in player time; the weight adds another three times it.
    assert!((score - end.spent.total - 288.0).abs() < 1e-6);
}

#[test]
fn active_unfinished_outdoor_kills_count_as_grinding() {
    for direct in [true, false] {
        let (score, end, _) = replay(false, direct, false);
        assert_unrewarded(score, &end);
    }
}

#[test]
fn last_direct_kill_beneficiary_expiring_does_not_erase_grinding() {
    let (score, end, _) = replay(true, true, false);
    assert_unrewarded(score, &end);
}

#[test]
fn legacy_outdoor_objective_expiring_does_not_erase_grinding() {
    let (score, end, _) = replay(true, false, false);
    assert_unrewarded(score, &end);
}

#[test]
fn expired_quest_with_a_genuine_shared_reward_keeps_productive_work() {
    let (score, end, _) = replay(true, true, true);
    assert_eq!(end.spent.fighting, 96.0);
    assert_eq!(end.spent.grinding, 0.0);
    assert_eq!(end.spent.grind_over, 0.0);
    assert!((score - end.spent.total).abs() < 1e-6);
}

#[test]
fn imported_complete_legacy_work_is_never_charged_to_this_route() {
    for retained in [false, true] {
        let (mut p, _) = scenario(false, false, false);
        p.model.initial.accepted = vec![0];
        p.model.initial.progress = vec![(0, 0, 1.0)];
        // Imported perishable progress starts a real deadline through `hold`.
        let prior = &mut p.model.quests[0].objectives[0];
        prior.item = true;
        prior.lasts = Some(200.0);
        p.model.quests[2].xp = 400;
        let mut route = vec![
            Stop::quest(2, Kind::Accept),
            Stop::quest(2, Kind::Objective(0)),
            Stop::quest(1, Kind::Accept),
        ];
        if retained {
            route.push(Stop::quest(0, Kind::Objective(1)));
        }
        route.push(Stop::quest(2, Kind::TurnIn));
        let mut trace = Vec::new();
        let (_, end) = planner(&p).simulate_state(&route, Some(&mut trace)).unwrap();
        assert_eq!(end.spent.mob_xp, 200, "only the productive quest performs kills");
        assert_eq!(end.spent.grind_xp, 0);
        assert!(end.dropped.contains(&0));
        assert_eq!(
            trace.iter().any(|t| matches!(t.event, Event::Abandon { quest: 0 })),
            retained,
            "omission abandons at start; retention expires during the route"
        );
        assert_eq!(end.spent.fighting, 96.0, "productive legacy work keeps a nonzero cap");
        assert_eq!(end.spent.grinding, 0.0, "prior work must not consume that cap");
    }
}

#[test]
fn imported_partial_legacy_work_charges_only_the_two_new_kills() {
    for expired in [false, true] {
        let (mut p, mut route) = scenario(false, false, false);
        p.model.initial.accepted = vec![0];
        p.model.initial.progress = vec![(0, 0, 0.5)];
        route.remove(0);
        if expired {
            let o = &mut p.model.quests[0].objectives[0];
            o.item = true;
            o.lasts = Some(60.0);
        }
        let mut trace = Vec::new();
        let (score, end) = planner(&p).simulate_state(&route, Some(&mut trace)).unwrap();
        assert_eq!(end.spent.mob_xp, 100);
        assert_eq!(end.spent.grind_xp, 0);
        assert_eq!(end.dropped.contains(&0), expired);
        assert_eq!(end.spent.grinding, 48.0);
        assert_eq!(end.spent.fighting, 0.0);
        assert!((score - end.spent.total - 144.0).abs() < 1e-6);
    }
}

#[test]
fn genuine_legacy_turn_in_settles_only_performed_partial_work() {
    let (mut p, _) = scenario(false, false, false);
    p.model.initial.accepted = vec![0];
    p.model.initial.progress = vec![(0, 0, 0.5)];
    p.model.quests[0].objectives.truncate(1);
    p.model.quests[0].xp = 400;
    let route = [Stop::quest(0, Kind::Objective(0)), Stop::quest(0, Kind::TurnIn)];
    let (_, end) = planner(&p).simulate_state(&route, None).unwrap();
    assert!(end.rewarded(0));
    assert_eq!(end.spent.mob_xp, 100);
    assert_eq!(end.spent.fighting, 48.0);
    assert_eq!(end.spent.grinding, 0.0);
}

#[test]
fn legacy_work_uses_executed_alternative_and_consumed_stock_cost() {
    let (mut p, _) = scenario(false, false, false);
    p.model.loot.items.push(900);
    let o = &mut p.model.quests[0].objectives[0];
    o.extra = 200.0;
    let mut alternative = o.clone();
    alternative.kills = 2.0;
    alternative.extra = 80.0;
    alternative.count = 4.0;
    alternative.stock = Some(0);
    o.alts.push(alternative);
    let planner = planner(&p);
    let mut state = planner.sim().initial_state();
    // Real inventory consumption leaves half the alternative's fixed work.
    state.stock[0] = 2.0;
    planner.sim().apply(&mut state, Stop::quest(0, Kind::Accept), None);
    state.spot = Some((0, 0, 1));
    planner
        .sim()
        .apply(&mut state, Stop::quest(0, Kind::Objective(0)), None);
    state.spot = None;
    assert_eq!(state.stock[0], 0.0);
    assert_eq!(state.spent.mob_xp, 100);
    assert_eq!(state.spent.fighting, 88.0);
    // Productive legacy work keeps the final cap above the unpaid 88 seconds. A nominal
    // estimate of the canonical objective would now wrongly consume these 96 seconds too.
    for stop in [
        Stop::quest(2, Kind::Accept),
        Stop::quest(2, Kind::Objective(0)),
        Stop::quest(2, Kind::TurnIn),
    ] {
        planner.sim().apply(&mut state, stop, None);
    }
    assert!(state.rewarded(2));
    assert_eq!(state.spent.mob_xp, 300);
    assert_eq!(state.spent.fighting, 184.0);
    planner.sim().apply(&mut state, Stop::quest(1, Kind::Accept), None);
    planner.sim().apply(&mut state, Stop::quest(1, Kind::TurnIn), None);
    planner.sim().finish(&mut state, None);
    assert_eq!(state.spent.grind_xp, 0);
    assert_eq!(
        state.spent.grinding, 88.0,
        "48 seconds of kills plus 40 of actual extra work"
    );
    assert_eq!(state.spent.fighting, 96.0);
}

#[test]
fn rewarded_gathering_cannot_protect_unrewarded_direct_kills() {
    for gather_first in [false, true] {
        for expired in [false, true] {
            let (mut p, _) = scenario(expired, true, false);
            p.params.object_time = 12.0;
            p.params.gather_overlap = 0.5;
            p.model.quests[0].objectives[1].uses = 0.0;
            p.model.quests[2].xp = 400;
            let gather = &mut p.model.quests[2].objectives[0];
            gather.kills = 0.0;
            gather.direct_kill = None;
            gather.uses = 4.0;
            let mut work = [Stop::quest(0, Kind::Objective(0)), Stop::quest(2, Kind::Objective(0))];
            if gather_first {
                work.swap(0, 1);
            }
            let route = [
                Stop::quest(0, Kind::Accept),
                Stop::quest(2, Kind::Accept),
                work[0],
                work[1],
                Stop::quest(1, Kind::Accept),
                Stop::quest(0, Kind::Objective(1)),
                Stop::quest(2, Kind::TurnIn),
            ];
            let (_, end) = planner(&p).simulate_state(&route, None).unwrap();
            assert_eq!(end.dropped.contains(&0), expired);
            assert!(end.rewarded(2));
            assert_eq!(end.spent.mob_xp, 200);
            assert_eq!(end.spent.grind_xp, 0);
            assert_eq!(end.spent.fighting, 0.0);
            assert_eq!(
                end.spent.grinding, 96.0,
                "rewarded overlapping gathering is not rewarded kill time"
            );
        }
    }
}

#[test]
fn last_of_two_direct_beneficiaries_abandoning_preserves_work_once() {
    let (p, _) = scenario(false, true, false);
    let planner = planner(&p);
    let mut state = planner.sim().initial_state();
    for stop in [
        Stop::quest(0, Kind::Accept),
        Stop::quest(2, Kind::Accept),
        Stop::quest(0, Kind::Objective(0)),
    ] {
        planner.sim().apply(&mut state, stop, None);
    }
    assert_eq!(state.kill_work.len(), 1);
    planner.sim().abandon(&mut state, 0, &mut |_, _| {});
    assert_eq!(state.kill_work.len(), 1);
    assert_eq!(state.kill_work[0].quests, [2]);
    assert_eq!(state.orphaned_work, 0.0);
    planner.sim().abandon(&mut state, 2, &mut |_, _| {});
    assert!(state.kill_work.is_empty());
    assert_eq!(state.orphaned_work, 96.0);
    state.settle_kill_work(2, false);
    assert_eq!(
        state.orphaned_work, 96.0,
        "settlement cannot transfer the same work twice"
    );
    for stop in [Stop::quest(1, Kind::Accept), Stop::quest(1, Kind::TurnIn)] {
        planner.sim().apply(&mut state, stop, None);
    }
    planner.sim().finish(&mut state, None);
    assert_eq!(state.spent.mob_xp, 200);
    assert_eq!(state.spent.grind_xp, 0);
    assert_eq!(state.spent.grinding, 96.0);
    assert_eq!(state.spent.fighting, 0.0);
}

#[test]
fn legacy_pending_and_expired_work_match_full_incremental_replay() {
    for timed in [false, true] {
        let (p, route) = scenario(timed, false, false);
        let planner = planner(&p);
        let run = planner.run(&route).unwrap();
        let mut moved = route.clone();
        moved.swap(1, 2);
        let incremental = planner.evaluate(&run, &moved, 1, 2, true).unwrap();
        let full = planner.simulate(&moved, None).unwrap();
        assert!((incremental - full).abs() < 1e-6, "{timed}: {incremental} vs {full}");
    }
}

#[test]
fn expired_gathering_cannot_reclassify_rewarded_direct_kills() {
    for gather_first in [false, true] {
        let (mut p, _) = scenario(false, true, false);
        p.params.object_time = 12.0;
        p.params.gather_overlap = 0.5;
        p.model.quests[0].time_limit = Some(220.0);
        let mut kills = p.model.quests[0].objectives[0].clone();
        kills.kills = 5.0;
        let mut gather = kills.clone();
        gather.kills = 0.0;
        gather.direct_kill = None;
        gather.uses = 5.0;
        let mut visit = p.model.quests[0].objectives[1].clone();
        visit.after = 2;
        visit.uses = 0.0;
        p.model.quests[0].objectives = vec![gather, kills, visit];
        p.model.quests[2].objectives[0].kills = 2.0;
        p.model.quests[2].xp = 150;
        let mut work = [Stop::quest(0, Kind::Objective(0)), Stop::quest(0, Kind::Objective(1))];
        if !gather_first {
            work.swap(0, 1);
        }
        let route = [
            Stop::quest(0, Kind::Accept),
            Stop::quest(2, Kind::Accept),
            work[0],
            work[1],
            Stop::quest(1, Kind::Accept),
            Stop::quest(0, Kind::Objective(2)),
            Stop::quest(2, Kind::TurnIn),
        ];
        let mut trace = Vec::new();
        let (_, end) = planner(&p).simulate_state(&route, Some(&mut trace)).unwrap();
        assert_eq!(end.spent.mob_xp, 250);
        assert_eq!(end.spent.grind_xp, 0);
        assert!(trace.iter().any(|t| matches!(t.event, Event::Abandon { quest: 0 })));
        assert!(end.rewarded(2));
        assert_eq!(end.spent.fighting, 48.0, "two actually shared kills earned a reward");
        assert_eq!(
            end.spent.grinding, 72.0,
            "three kills remain, with gathering overlapped"
        );
    }
}

#[test]
fn actual_legacy_work_is_bounded_by_active_quests_and_settles_once() {
    let (mut p, _) = scenario(false, false, false);
    let mut first = p.model.quests[0].objectives[0].clone();
    first.kills = 1.0;
    p.model.quests[0].objectives = vec![first.clone(), first.clone()];
    p.model.quests[2].objectives = vec![first];
    let planner = planner(&p);
    let mut state = planner.sim().initial_state();
    for stop in [
        Stop::quest(0, Kind::Accept),
        Stop::quest(2, Kind::Accept),
        Stop::quest(0, Kind::Objective(0)),
        Stop::quest(0, Kind::Objective(1)),
        Stop::quest(2, Kind::Objective(0)),
    ] {
        planner.sim().apply(&mut state, stop, None);
    }
    assert_eq!(&*state.legacy_work, &[(0, 48.0), (2, 24.0)]);
    let snapshot = state.clone();
    planner.sim().abandon(&mut state, 0, &mut |_, _| {});
    assert_eq!(&*state.legacy_work, &[(2, 24.0)]);
    assert_eq!(state.orphaned_work, 48.0);
    assert_eq!(
        &*snapshot.legacy_work,
        &[(0, 48.0), (2, 24.0)],
        "settling preserves earlier snapshots"
    );
    state.settle_legacy_work(0, false);
    assert_eq!(state.orphaned_work, 48.0);
    planner.sim().apply(&mut state, Stop::quest(2, Kind::TurnIn), None);
    assert!(state.rewarded(2));
    assert!(state.legacy_work.is_empty(), "neither settled quest retains a row");
    assert_eq!(
        state.rewarded_kill_work, 0.0,
        "legacy rewards are not protected direct kills"
    );
    for stop in [Stop::quest(1, Kind::Accept), Stop::quest(1, Kind::TurnIn)] {
        planner.sim().apply(&mut state, stop, None);
    }
    planner.sim().finish(&mut state, None);
    assert_eq!(state.spent.mob_xp, 150);
    assert_eq!(state.spent.grind_xp, 0);
    assert_eq!(state.spent.fighting, 24.0);
    assert_eq!(state.spent.grinding, 48.0);
}

#[test]
fn incremental_reuse_distinguishes_pending_and_orphaned_work_ownership() {
    for expired in [false, true] {
        let (mut p, _) = scenario(false, true, false);
        p.model.loot.items.push(900);
        p.model.loot.by_zone.insert(1, vec![(0, 1.0)]);
        // One actual kill supplies one item, enough for only one of these two quests.
        let mut source = p.model.quests[0].objectives[0].clone();
        source.kills = 1.0;
        p.model.quests[1].objectives = vec![source];
        p.model.quests[1].xp = 350;
        p.model.quests[1].ends[0].pos = p.model.quests[0].starts[0].pos;
        for i in [0, 2] {
            let o = &mut p.model.quests[i].objectives[0];
            o.kills = 0.0;
            o.direct_kill = None;
            o.extra = 96.0;
            o.stock = Some(0);
            o.count = 1.0;
        }
        p.model.quests[0].time_limit = expired.then_some(400.0);
        let route = [
            Stop::quest(0, Kind::Accept),
            Stop::quest(2, Kind::Accept),
            Stop::quest(1, Kind::Accept),
            Stop::quest(1, Kind::Objective(0)),
            Stop::quest(0, Kind::Objective(0)),
            Stop::quest(2, Kind::Objective(0)),
            Stop::quest(2, Kind::TurnIn),
            Stop::quest(0, Kind::Objective(1)),
            Stop::quest(1, Kind::TurnIn),
        ];
        let mut moved = route;
        moved.swap(4, 5);
        let planner = planner(&p);
        let run = planner.run(&route).unwrap();
        let moved_run = planner.run(&moved).unwrap();
        // Before settlement only legacy ownership differs; after settlement only orphaned
        // work differs. Either missing equality guard would wrongly reuse the old suffix.
        let boundary = if expired { 7 } else { 5 };
        let old = &run.states[boundary + 1];
        let new = &moved_run.states[boundary + 1];
        assert_eq!(old.time, new.time);
        assert_eq!(old.objectives, new.objectives);
        assert_eq!(old.stock, new.stock);
        assert_eq!(old.together, new.together);
        if expired {
            assert!(old.dropped.contains(&0) && new.dropped.contains(&0));
            assert!(old.legacy_work.is_empty() && new.legacy_work.is_empty());
            assert_eq!(old.orphaned_work, 0.0);
            assert_eq!(new.orphaned_work, 96.0);
        } else {
            assert_eq!(&*old.legacy_work, &[(2, 96.0)]);
            assert_eq!(&*new.legacy_work, &[(0, 96.0)]);
        }
        let incremental = planner.evaluate(&run, &moved, 4, boundary, true).unwrap();
        let (full, end) = planner.simulate_state(&moved, None).unwrap();
        assert_eq!(end.spent.mob_xp, 50);
        assert_eq!(end.spent.grind_xp, 0);
        assert_eq!(end.spent.grinding, 96.0);
        assert!((full - run.total - 288.0).abs() < 1e-6);
        assert!((incremental - full).abs() < 1e-6, "{expired}: {incremental} vs {full}");
    }
}
