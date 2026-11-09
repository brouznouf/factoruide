//! One physical kill can advance more than one accepted creature-kill objective.

use super::{Kind, Prepared, Stop, planner};
use crate::testing::{Q, Scenario};

fn two_quests(a: u32, b: u32) -> Prepared {
    let mut p = Scenario::new()
        .levels(10, 11)
        .quest(Q::new(1, "A").level(10).xp(0).kill("Wolf", 10, a, 1, 50.0, 50.0))
        .quest(Q::new(2, "B").level(10).xp(0).kill("Wolf", 10, b, 1, 50.0, 50.0))
        .params(|params| {
            params.camp_radius = 0.0;
            params.farm_on_way = false;
            params.stop_overhead = 0.0;
            params.keep_gray = true;
        })
        .build();
    p.model.initial.accepted = vec![0, 1];
    for q in &mut p.model.quests {
        let o = &mut q.objectives[0];
        o.loc.id = 777;
        o.direct_kill = Some(777);
        o.mobs = vec![(777, "Wolf".into())];
        o.spawns = 100;
        o.extra = 0.0;
    }
    p
}

fn objective(quest: u32) -> Stop {
    Stop::quest(quest, Kind::Objective(0))
}

#[test]
fn shared_kills_charge_the_larger_count_once() {
    for (a, b, kills) in [(2, 2, 2), (2, 5, 5), (5, 2, 5)] {
        let p = two_quests(a, b);
        let planner = planner(&p);
        let mut state = planner.sim().initial_state();
        planner.sim().apply(&mut state, objective(0), None);
        planner.sim().apply(&mut state, objective(1), None);
        assert_eq!(state.objectives, [1, 1]);
        assert_eq!(state.spent.fighting, f64::from(kills) * 24.0);
        assert_eq!(state.spent.mob_xp, i64::from(kills) * 95);
    }
}

#[test]
fn shared_completion_allows_turn_in_without_another_objective_stop() {
    let p = two_quests(5, 2);
    let planner = planner(&p);
    let mut state = planner.sim().initial_state();
    planner.sim().apply(&mut state, objective(0), None);
    let turn_in = Stop::quest(1, Kind::TurnIn);
    assert!(planner.availability().valid(&state, turn_in));
    planner.sim().apply(&mut state, turn_in, None);
    assert!(state.rewarded(1));
    assert_eq!(state.spent.fighting, 120.0);
}

#[test]
fn different_mobs_at_the_same_place_do_not_share_credit() {
    let mut p = two_quests(2, 5);
    p.model.quests[1].objectives[0].loc.id = 778;
    p.model.quests[1].objectives[0].direct_kill = Some(778);
    p.model.quests[1].objectives[0].mobs = vec![(778, "Other wolf".into())];
    let planner = planner(&p);
    let mut state = planner.sim().initial_state();
    planner.sim().apply(&mut state, objective(0), None);
    planner.sim().apply(&mut state, objective(1), None);
    assert_eq!(state.spent.fighting, 168.0);
    assert_eq!(state.spent.mob_xp, 665);
}

#[test]
fn kills_before_a_quest_is_accepted_give_it_no_credit() {
    let mut p = two_quests(2, 5);
    p.model.initial.accepted = vec![0];
    let planner = planner(&p);
    let mut state = planner.sim().initial_state();
    for stop in [objective(0), Stop::quest(1, Kind::Accept), objective(1)] {
        planner.sim().apply(&mut state, stop, None);
    }
    assert_eq!(state.spent.fighting, 168.0);
    assert_eq!(state.spent.mob_xp, 665);
}

#[test]
fn shared_credit_adds_to_imported_partial_progress() {
    let mut p = two_quests(2, 5);
    p.model.initial.progress = vec![(1, 0, 0.4)];
    let planner = planner(&p);
    let mut state = planner.sim().initial_state();
    planner.sim().apply(&mut state, objective(0), None);
    planner.sim().apply(&mut state, objective(1), None);
    assert_eq!(state.objectives, [1, 1]);
    assert!((state.spent.fighting - 72.0).abs() < 1e-6);
    assert_eq!(state.spent.mob_xp, 285);
}

#[test]
fn shared_kills_respect_objective_order() {
    for unlocked_before in [false, true] {
        let mut p = two_quests(5, 5);
        let mut kills = p.model.quests[1].objectives[0].clone();
        let mut unlock = kills.clone();
        unlock.kills = 0.0;
        unlock.direct_kill = None;
        unlock.uses = 1.0;
        kills.after = 1;
        p.model.quests[1].objectives = vec![unlock, kills];
        let planner = planner(&p);
        let mut state = planner.sim().initial_state();
        if unlocked_before {
            planner.sim().apply(&mut state, objective(1), None);
        }
        planner.sim().apply(&mut state, objective(0), None);
        if unlocked_before {
            assert_eq!(state.objectives[1], 3);
            assert_eq!(state.spent.mob_xp, 475);
        } else {
            assert_eq!(
                state.objectives[1], 0,
                "earlier kills cannot complete a locked objective"
            );
            planner.sim().apply(&mut state, objective(1), None);
            planner
                .sim()
                .apply(&mut state, Stop::quest(1, Kind::Objective(1)), None);
            assert_eq!(state.spent.mob_xp, 950);
        }
    }
}

#[test]
fn shared_kills_respect_handed_items_and_expired_deadlines() {
    for timed in [false, true] {
        let mut p = two_quests(2, 2);
        if timed {
            p.model.quests[1].time_limit = Some(1.0);
        } else {
            p.model.quests[1].objectives[0].handed_by = vec![1];
        }
        let planner = planner(&p);
        let mut state = planner.sim().initial_state();
        if timed {
            state.timers.push((1, -1.0));
        }
        planner.sim().apply(&mut state, objective(0), None);
        assert_eq!(state.objectives[1], 0);
    }
}

#[test]
fn shared_completion_can_use_a_perishable_item_before_the_batch_ends() {
    for (needed, lasts) in [(2, 80.0), (3, 80.0), (2, 64.0)] {
        let mut p = two_quests(5, needed);
        let mut kills = p.model.quests[1].objectives[0].clone();
        let mut item = kills.clone();
        item.kills = 0.0;
        item.direct_kill = None;
        item.item = true;
        item.lasts = Some(lasts);
        kills.after = 1;
        p.model.quests[1].objectives = vec![item, kills];
        p.model.initial.progress = vec![(1, 0, 1.0)];
        let planner = planner(&p);
        let mut state = planner.sim().initial_state();
        assert_eq!(state.timers, [(1, lasts * 0.75)]);
        planner.sim().apply(&mut state, objective(0), None);
        assert_eq!(state.time, 120.0);
        if needed == 2 {
            assert_eq!(state.objectives[1], 3, "two kills finish at 48 seconds, before expiry");
            assert_eq!(state.timers.len(), 0, "the item was used in time");
            let turn_in = Stop::quest(1, Kind::TurnIn);
            assert!(planner.availability().valid(&state, turn_in));
            planner.sim().apply(&mut state, turn_in, None);
            assert!(state.rewarded(1));
        } else {
            assert_eq!(state.objectives[1], 1, "three kills would finish after expiry");
            assert_eq!(state.timers.len(), 1);
        }
    }
}

#[test]
fn shared_completion_does_not_extend_a_quest_wide_turn_in_deadline() {
    let mut p = two_quests(5, 2);
    p.model.quests[1].time_limit = Some(80.0);
    let planner = planner(&p);
    let mut state = planner.sim().initial_state();
    state.timers.push((1, 60.0));
    planner.sim().apply(&mut state, objective(0), None);
    planner.sim().apply(&mut state, Stop::quest(1, Kind::TurnIn), None);
    assert!(
        !state.rewarded(1),
        "finishing kills in time does not extend the turn-in deadline"
    );
}

fn gathering_before_shared_kills(perishable: bool) -> Prepared {
    let mut p = two_quests(5, 2);
    p.params.gather_overlap = 0.5;
    let kills = p.model.quests[0].objectives[0].clone();
    let mut gather = kills.clone();
    gather.kills = 0.0;
    gather.direct_kill = None;
    gather.uses = 5.0; // 60 seconds, overlapped by the following five kills.
    p.model.quests[0].objectives = vec![gather, kills];
    if perishable {
        let mut kills = p.model.quests[1].objectives[0].clone();
        let mut item = kills.clone();
        item.kills = 0.0;
        item.direct_kill = None;
        item.item = true;
        item.lasts = Some(120.0); // 90 seconds after the safety margin.
        kills.after = 1;
        p.model.quests[1].objectives = vec![item, kills];
        p.model.initial.progress = vec![(1, 0, 1.0)];
    }
    p
}

#[test]
fn perishable_shared_credit_uses_the_gathering_overlap_clock() {
    let p = gathering_before_shared_kills(true);
    let planner = planner(&p);
    let mut state = planner.sim().initial_state();
    planner.sim().apply(&mut state, objective(0), None);
    planner
        .sim()
        .apply(&mut state, Stop::quest(0, Kind::Objective(1)), None);
    assert_eq!(state.time, 120.0);
    assert_eq!(
        state.objectives[1], 3,
        "two kills finish at 84 seconds after overlap, before expiry"
    );
    assert_eq!(state.timers.len(), 0);
    planner.sim().apply(&mut state, Stop::quest(1, Kind::TurnIn), None);
    assert!(state.rewarded(1));
}

#[test]
fn unfinished_gathering_cannot_reclassify_rewarded_direct_kills() {
    let mut p = gathering_before_shared_kills(false);
    p.model.quests[1].xp = p.params.edition.rules().to_next_level(10) - 475;
    let planner = planner(&p);
    let mut state = planner.sim().initial_state();
    planner.sim().apply(&mut state, objective(0), None);
    planner
        .sim()
        .apply(&mut state, Stop::quest(0, Kind::Objective(1)), None);
    planner.sim().apply(&mut state, Stop::quest(1, Kind::TurnIn), None);
    planner.sim().finish(&mut state, None);
    assert_eq!(state.spent.fighting, 48.0);
    assert_eq!(state.spent.grinding, 72.0);
}

#[test]
fn loot_and_proxy_credit_estimates_are_not_shared_direct_kills() {
    for loot in [false, true] {
        let mut p = two_quests(2, 5);
        let o = &mut p.model.quests[1].objectives[0];
        if loot {
            o.item = true;
        } else {
            o.direct_kill = None;
        }
        let planner = planner(&p);
        let mut state = planner.sim().initial_state();
        planner.sim().apply(&mut state, objective(0), None);
        assert_eq!(state.objectives[1], 0);
        assert_eq!(state.started.len(), 0);
    }
}

#[test]
fn shared_credit_follows_target_identity_across_alternative_places() {
    let mut p = two_quests(2, 5);
    let mut alt = p.model.quests[0].objectives[0].clone();
    alt.loc.pos.x += 20.0;
    p.model.quests[0].objectives[0].alts = vec![alt];
    let planner = planner(&p);
    let mut state = planner.sim().initial_state();
    state.spot = Some((0, 0, 1));
    planner.sim().apply(&mut state, objective(0), None);
    state.spot = None;
    planner.sim().apply(&mut state, objective(1), None);
    assert_eq!(state.spent.mob_xp, 475);
    assert_eq!(state.spent.fighting, 120.0);
}

#[test]
fn alternative_place_costs_use_the_remaining_shared_work() {
    let mut p = two_quests(4, 5);
    let o = &mut p.model.quests[1].objectives[0];
    o.extra = 50.0;
    let mut alt = o.clone();
    alt.extra = 0.0;
    alt.loc.pos.x += 70.0;
    let alt_pos = alt.loc.pos;
    o.alts = vec![alt];
    let planner = planner(&p);
    let mut state = planner.sim().initial_state();
    assert_eq!(
        planner.sim().spot(&state, 1, 0).loc.pos,
        alt_pos,
        "the full wait makes the detour worthwhile"
    );
    planner.sim().apply(&mut state, objective(0), None);
    assert_eq!(
        planner.sim().spot(&state, 1, 0).loc.pos,
        p.model.quests[1].objectives[0].loc.pos,
        "only one kill's share of the wait remains"
    );
}

#[test]
fn abandoning_one_shared_quest_preserves_the_others_progress() {
    let mut p = two_quests(2, 5);
    p.model.quests[1].xp = p.params.edition.rules().to_next_level(10) - 475;
    let planner = planner(&p);
    let mut state = planner.sim().initial_state();
    planner.sim().apply(&mut state, objective(0), None);
    planner.sim().abandon(&mut state, 0, &mut |_, _| {});
    planner.sim().apply(&mut state, objective(1), None);
    assert_eq!(state.spent.mob_xp, 475);
    assert_eq!(state.spent.fighting, 120.0);
    planner.sim().apply(&mut state, Stop::quest(1, Kind::TurnIn), None);
    planner.sim().finish(&mut state, None);
    assert_eq!(state.spent.fighting, 120.0);
    assert_eq!(state.spent.grinding, 0.0);
}

#[test]
fn loaded_creature_objectives_retain_exact_targets_but_loot_and_talk_do_not() {
    let conn = crate::fixture::Fixture::new(super::Edition::Classic)
        .toml(
            r#"
[[npc]]
id = 823
name = "Giver"
npc_flags = 3
friendly_to = "A"
at = [{zone=12, x=48.0, y=42.0}]
[[npc]]
id = 777
name = "Wolf"
level = 10
at = [{zone=12, x=48.1, y=42.1}]
[[item]]
id = 990880
name = "Wolf tooth"
drops = [{npc=777, chance=0.5}]
[[quest]]
id = 990900
name = "Kill two"
level = 10
min_level = 1
giver = 823
xp = 0
objectives = [{creature=777, amount=2}]
[[quest]]
id = 990901
name = "Kill five"
level = 10
min_level = 1
giver = 823
xp = 0
objectives = [{creature=777, amount=5}]
[[quest]]
id = 990902
name = "Find teeth"
level = 10
min_level = 1
giver = 823
xp = 0
objectives = [{item=990880, amount=2}]
[[quest]]
id = 990903
name = "Talk"
level = 10
min_level = 1
giver = 823
xp = 0
objectives = [{talk=823, amount=1}]
"#,
        )
        .open();
    let overrides = super::Overrides::load_edition(&super::repo().join("overrides"), super::Edition::Classic).unwrap();
    let request = serde_json::from_value(serde_json::json!({
        "race":"human", "class":"warrior", "from_level":10, "to_level":11,
        "required_class_quests":[], "params":{"edition":"classic"}
    }))
    .unwrap();
    let p = crate::job::prepare(&conn, &overrides, &request, &|_| {}).unwrap();
    for id in [990900, 990901] {
        let o = &p.model.quests[p.model.index[&id]].objectives[0];
        assert_eq!(o.shared_kill_target(), Some(777));
    }
    for id in [990902, 990903] {
        let o = &p.model.quests[p.model.index[&id]].objectives[0];
        assert_eq!(o.shared_kill_target(), None);
    }
}

#[test]
fn shared_progress_and_work_are_consistent_in_incremental_replay() {
    let mut p = two_quests(2, 5);
    p.model.initial.accepted.clear();
    let planner = planner(&p);
    let route = [
        Stop::quest(0, Kind::Accept),
        Stop::quest(1, Kind::Accept),
        objective(0),
        objective(1),
        Stop::quest(0, Kind::TurnIn),
        Stop::quest(1, Kind::TurnIn),
    ];
    let run = planner.run(&route).unwrap();
    let mut moved = route;
    moved.swap(2, 3);
    let incremental = planner.evaluate(&run, &moved, 2, 3, true).unwrap();
    let full = planner.simulate(&moved, None).unwrap();
    assert!((incremental - full).abs() < 1e-6, "{incremental} vs {full}");
    let late_accept = [route[0], route[2], route[1], route[3], route[4], route[5]];
    let incremental = planner.evaluate(&run, &late_accept, 1, 2, true).unwrap();
    let full = planner.simulate(&late_accept, None).unwrap();
    assert!((incremental - full).abs() < 1e-6, "{incremental} vs {full}");
}

#[test]
fn shared_kills_consume_rested_xp_and_fill_bags_once() {
    let mut p = two_quests(2, 5);
    p.model.initial.rested = 500;
    let planner = planner(&p);
    let mut state = planner.sim().initial_state();
    planner.sim().apply(&mut state, objective(0), None);
    planner.sim().apply(&mut state, objective(1), None);
    assert_eq!(state.spent.mob_xp, 950);
    assert_eq!(state.rested, 25);
    assert!((f64::from(state.bags) - 5.0 * p.params.loot_per_kill).abs() < 1e-6);
}

#[test]
fn an_unturned_shared_quest_does_not_reclassify_rewarded_work_as_grinding() {
    let mut p = two_quests(5, 2);
    p.model.quests[0].xp = p.params.edition.rules().to_next_level(10) - 475;
    let planner = planner(&p);
    let mut state = planner.sim().initial_state();
    planner.sim().apply(&mut state, objective(0), None);
    assert_eq!(state.objectives[1], 1);
    planner.sim().apply(&mut state, Stop::quest(0, Kind::TurnIn), None);
    assert_eq!(state.level, 11);
    planner.sim().finish(&mut state, None);
    assert_eq!(state.spent.fighting, 120.0);
    assert_eq!(state.spent.grinding, 0.0);
}

#[test]
fn only_kills_beyond_the_rewarded_shared_goal_count_as_grinding() {
    let mut p = two_quests(5, 2);
    p.model.quests[1].xp = p.params.edition.rules().to_next_level(10) - 475;
    let planner = planner(&p);
    let mut state = planner.sim().initial_state();
    planner.sim().apply(&mut state, objective(0), None);
    assert!(planner.availability().valid(&state, Stop::quest(1, Kind::TurnIn)));
    planner.sim().apply(&mut state, Stop::quest(1, Kind::TurnIn), None);
    assert_eq!(state.level, 11);
    planner.sim().finish(&mut state, None);
    assert_eq!(state.spent.fighting, 48.0);
    assert_eq!(state.spent.grinding, 72.0);
}

#[test]
fn shared_kill_respawns_count_only_new_kills_and_not_imported_progress() {
    for imported in [0.0, 0.4] {
        let mut p = two_quests(2, 5);
        p.model.initial.progress = vec![(1, 0, imported)];
        for q in &mut p.model.quests {
            let o = &mut q.objectives[0];
            o.pool = crate::model::Pool {
                mine: 1.0,
                rate: 1.0 / 60.0,
                each: 24.0,
                wait: (o.kills - 1.0) * 36.0,
            };
            o.extra = o.pool.wait;
        }
        let planner = planner(&p);
        let mut state = planner.sim().initial_state();
        planner.sim().apply(&mut state, objective(0), None);
        planner.sim().apply(&mut state, objective(1), None);
        let kills = if imported == 0.0 { 5 } else { 3 };
        let wait = f64::from(kills - 1) * 36.0;
        assert_eq!(state.spent.mob_xp, i64::from(kills) * 95);
        assert!((state.spent.waiting - wait).abs() < 1e-6);
        assert!((state.spent.fighting - (f64::from(kills) * 24.0 + wait)).abs() < 1e-6);
    }
}
