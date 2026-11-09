use super::{planner, quests_at_start};
use crate::job::{InventoryItem, StartState};
use crate::model::{BoundAcquisition, BoundReputationSource, BoundSourceKind};
use crate::params::ReputationGoal;
use crate::reputation::{
    AcquisitionKind, AwardCap, FactionStandingRequirement, InputItem, RepAction, RepPhase, RepPhaseKind, RepProgram,
    ReputationEffects, ResetKind, RewardEncoding, RoundingPolicy, SignedAward, SourceAcquisition, SourceConfidence,
    SourceCosts, SourceDescriptor, SourceLocation, SourceReset, StandingRange,
};

fn scenario() -> crate::job::Prepared {
    let mut p = quests_at_start(0);
    p.profile.to_level = 1;
    p.params.reputation_goals = vec![ReputationGoal {
        faction: 68,
        target: 100,
    }];
    p.model.initial.reputation = vec![0.0];
    p.model.reputation_sources.sources.clear();
    p.model.reputation_sources.warnings.clear();
    p
}

fn source(p: &crate::job::Prepared, id: &str, faction: i64, amount: f64) -> BoundReputationSource {
    let pos = p.model.start.pos;
    let loc = SourceLocation {
        world_id: Some(pos.continent as u32),
        zone_id: Some(p.model.start.zone),
        x: Some(pos.x),
        y: Some(pos.y),
        ..SourceLocation::default()
    };
    BoundReputationSource {
        descriptor: SourceDescriptor {
            id: id.into(),
            name: id.into(),
            edition: "classic".into(),
            repeatable: true,
            effects: ReputationEffects {
                awards: vec![SignedAward {
                    faction_id: faction,
                    amount,
                    ..SignedAward::default()
                }],
                ..ReputationEffects::default()
            },
            input_items: vec![InputItem {
                item_id: 14047,
                count: 20,
                starter: false,
            }],
            costs: SourceCosts {
                interaction_seconds: 5.0,
                ..SourceCosts::default()
            },
            location: Some(loc.clone()),
            confidence: SourceConfidence::Confirmed,
            ..SourceDescriptor::default()
        },
        kind: BoundSourceKind::Turnin,
        locations: vec![loc],
        giver_locations: vec![],
        creature: None,
        acquisition: vec![],
        prerequisite_any: vec![],
        prerequisite_count: 1,
        supported: true,
        automatic_farming: false,
        warnings: vec![],
        quest_flags: 0,
        quest_flags_ex: 0,
        quest_flags_ex2: 0,
        selected_patch: None,
    }
}

fn stock(items: &[(i64, u32)]) -> StartState {
    StartState {
        inventory_valid: true,
        inventory: items
            .iter()
            .map(|&(item, count)| InventoryItem { item, count })
            .collect(),
        ..StartState::default()
    }
}

#[test]
fn reputation_program_repeats_consume_stock_and_leave_named_partial_goal() {
    let mut p = scenario();
    p.params.reputation_goals.push(ReputationGoal {
        faction: 67,
        target: 100,
    });
    p.model.initial.reputation.push(0.0);
    p.model.reputation_sources.sources.push(source(&p, "cloth", 68, 50.0));
    let done = planner(&p)
        .complete_reputation(&[], Some(&stock(&[(14047, 40)])))
        .unwrap();
    assert_eq!(done.end.reputation[&68], 100.0);
    assert_eq!(done.end.inventory.get(&14047).copied().unwrap_or(0), 0);
    assert_eq!(done.program.phases[0].cycles, 2);
    assert_eq!(done.program.result.deficits.get(&67), Some(&100.0));
    assert!(done.program.result.warnings.iter().any(|w| w.contains("67")));
    assert_eq!(done.breakdown.total, 10.0);
}

#[test]
fn reputation_program_missing_or_invalid_stock_never_funds_deliveries() {
    let mut p = scenario();
    p.model.reputation_sources.sources.push(source(&p, "cloth", 68, 50.0));
    let mut start = stock(&[(14047, 40)]);
    start.inventory_valid = false;
    for imported in [None, Some(&start)] {
        let done = planner(&p).complete_reputation(&[], imported).unwrap();
        assert_eq!(done.program.phases.len(), 0);
        assert_eq!(done.program.result.deficits[&68], 100.0);
    }
}

#[test]
fn reputation_program_starter_is_consumed_for_each_repeat_and_shared_stock_once() {
    let mut p = scenario();
    let mut a = source(&p, "a", 68, 25.0);
    a.descriptor.input_items.push(InputItem {
        item_id: 90001,
        count: 1,
        starter: true,
    });
    p.model.reputation_sources.sources.push(a);
    p.model.reputation_sources.sources.push(source(&p, "b", 68, 25.0));
    let done = planner(&p)
        .complete_reputation(&[], Some(&stock(&[(14047, 60), (90001, 1)])))
        .unwrap();
    assert_eq!(done.end.reputation[&68], 75.0);
    assert_eq!(done.end.inventory.get(&14047).copied().unwrap_or(0), 0);
    assert_eq!(done.end.inventory.get(&90001).copied().unwrap_or(0), 0);
}

#[test]
fn reputation_program_priced_vendor_requires_actual_remaining_money() {
    let mut p = scenario();
    p.model.initial.money = 39;
    let mut a = source(&p, "vendor", 68, 50.0);
    let descriptor = SourceAcquisition {
        kind: AcquisitionKind::PricedVendor,
        item_id: 14047,
        units_per_acquisition: 1,
        npc_id: Some(777),
        unit_price_copper: Some(1),
        location: a.descriptor.location.clone(),
        ..SourceAcquisition::default()
    };
    a.descriptor.acquisition.push(descriptor.clone());
    a.acquisition.push(BoundAcquisition {
        descriptor,
        locations: a.locations.clone(),
        creature: None,
        stock: None,
    });
    p.model.reputation_sources.sources.push(a);
    let done = planner(&p).complete_reputation(&[], None).unwrap();
    assert_eq!(done.end.reputation[&68], 50.0);
    assert_eq!(done.end.money_copper, 19);
    assert_eq!(done.breakdown.vendor_cost, 20);
    let replay = planner(&p)
        .replay_reputation(&[], Some(&stock(&[(14047, 20)])), &done.program)
        .unwrap();
    assert_eq!(replay.end.reputation[&68], 50.0);
    assert_eq!(
        replay.end.money_copper, 39,
        "already-held materials satisfy the saved acquisition target without another purchase"
    );
    assert_eq!(replay.end.inventory.get(&14047).copied().unwrap_or(0), 0);
    assert!(
        done.program
            .result
            .warnings
            .iter()
            .any(|w| w.to_lowercase().contains("money"))
    );
}

#[test]
fn reputation_program_daily_sources_run_together_before_next_reset() {
    let mut p = scenario();
    for id in ["a", "b"] {
        let mut a = source(&p, id, 68, 25.0);
        a.descriptor.reset = SourceReset {
            kind: ResetKind::Daily,
            quota_key: Some(id.into()),
            units_per_reset: 1,
        };
        p.model.reputation_sources.sources.push(a);
    }
    let done = planner(&p)
        .complete_reputation(&[], Some(&stock(&[(14047, 80)])))
        .unwrap();
    assert_eq!(done.end.reputation[&68], 100.0);
    assert_eq!(done.program.phases.len(), 1);
    assert_eq!(done.program.phases[0].kind, RepPhaseKind::DailyCircuit);
    assert_eq!(done.program.phases[0].cycles, 2);
    assert_eq!(done.program.phases[0].actions.len(), 2);
    assert_eq!(done.program.result.daily_opportunities, 2);
    assert_eq!(done.program.result.reset_transitions, 1);
    assert_eq!(done.program.result.calendar_wait_seconds, None);
    let replay = planner(&p)
        .replay_reputation(&[], Some(&stock(&[(14047, 80)])), &done.program)
        .unwrap();
    assert_eq!(replay.end.reputation, done.end.reputation);
    assert_eq!(replay.end.money_copper, done.end.money_copper);
    assert_eq!(replay.breakdown.total, done.breakdown.total);
}

#[test]
fn reputation_program_cap_and_signed_loss_use_ordered_kernel() {
    let mut p = scenario();
    let mut a = source(&p, "capped", 68, 50.0);
    a.descriptor.effects.awards[0].cap = Some(AwardCap::PreEventInclusive { maximum: 0.0 });
    a.descriptor.effects.awards.push(SignedAward {
        faction_id: 76,
        amount: -7.5,
        ..SignedAward::default()
    });
    p.model.reputation_sources.sources.push(a);
    let done = planner(&p)
        .complete_reputation(&[], Some(&stock(&[(14047, 200)])))
        .unwrap();
    assert_eq!(done.end.reputation[&68], 50.0);
    assert_eq!(done.end.reputation[&76], -7.5);
    assert_eq!(done.end.inventory[&14047], 180);
}

#[test]
fn reputation_program_prefix_only_keeps_time_and_terminal_accounting_once() {
    let mut p = quests_at_start(1);
    p.model.quests[0].reputation = vec![(68, 12.5), (67, -2.5)];
    p.params.reputation_goals = vec![ReputationGoal {
        faction: 68,
        target: 20,
    }];
    p.profile.to_level = 1;
    let route = [
        super::Stop::quest(0, super::Kind::Accept),
        super::Stop::quest(0, super::Kind::TurnIn),
    ];
    let plain = planner(&p).simulate_full(&route, None).unwrap().1;
    let done = planner(&p).complete_reputation(&route, None).unwrap();
    assert_eq!(done.breakdown.total, plain.total);
    assert_eq!(done.breakdown.grinding, plain.grinding);
    assert_eq!(done.end.reputation[&68], 12.5);
    assert_eq!(done.end.reputation[&67], -2.5);
}

#[test]
fn reputation_program_continuation_keeps_fractional_forecast_resources() {
    let mut p = scenario();
    p.model.initial.money = 42;
    p.model
        .reputation_sources
        .sources
        .push(source(&p, "fractional", 68, 12.5));
    let start = stock(&[(14047, 20)]);
    let done = planner(&p).complete_reputation(&[], Some(&start)).unwrap();
    let after = start.after(&p.model, &p.world, p.params.edition.rules(), &done.end);
    assert_eq!(after.reputation.iter().find(|r| r.id == 68).unwrap().value, 12.5);
    assert_eq!(after.money, 42);
    assert!(after.inventory.iter().all(|i| i.item != 14047 || i.count == 0));
    assert!(after.reputation_inferred);
    p.model.initial.reputation_inferred = after.reputation_inferred;
    p.model.initial.reputation_recorded.insert(68);
    assert!(
        planner(&p)
            .reputation_warnings(&[12.5])
            .iter()
            .any(|w| w.contains("inferred continuation"))
    );
}

#[test]
fn reputation_program_prefix_consumption_cannot_spend_imported_stock_twice() {
    let mut p = quests_at_start(1);
    p.profile.to_level = 1;
    p.params.reputation_goals = vec![ReputationGoal {
        faction: 68,
        target: 100,
    }];
    p.model.reputation_sources.sources.clear();
    p.model.reputation_sources.sources.push(source(&p, "repeat", 68, 50.0));
    p.model.reputation_sources.quest_inputs.insert(
        p.model.quests[0].id,
        vec![InputItem {
            item_id: 14047,
            count: 20,
            starter: false,
        }],
    );
    let route = [
        super::Stop::quest(0, super::Kind::Accept),
        super::Stop::quest(0, super::Kind::TurnIn),
    ];
    let done = planner(&p)
        .complete_reputation(&route, Some(&stock(&[(14047, 20)])))
        .unwrap();
    assert_eq!(done.end.reputation[&68], 0.0);
    assert_eq!(done.program.phases.len(), 0);
}

#[test]
fn reputation_program_exact_acquisition_charges_travel_combat_respawns_and_xp() {
    let mut p = scenario();
    let mut a = source(&p, "farm", 68, 50.0);
    a.descriptor.input_items[0].count = 1;
    let mut location = a.descriptor.location.clone().unwrap();
    location.x = location.x.map(|x| x + 70.0);
    let path = SourceAcquisition {
        kind: AcquisitionKind::ExactCreatureDrop,
        item_id: 14047,
        units_per_acquisition: 1,
        npc_id: Some(777),
        location: Some(location.clone()),
        drop_chance: Some(0.5),
        ..SourceAcquisition::default()
    };
    a.descriptor.acquisition.push(path.clone());
    a.acquisition.push(BoundAcquisition {
        descriptor: path,
        locations: vec![location],
        creature: Some(crate::model::BoundCreature {
            npc_id: 777,
            min_level: 1,
            max_level: 1,
            rank: 0,
            respawn_seconds: Some(100.0),
            spawn_count: 1,
        }),
        stock: None,
    });
    p.model.reputation_sources.sources.push(a);
    let done = planner(&p).complete_reputation(&[], None).unwrap();
    assert_eq!(done.end.reputation[&68], 100.0);
    assert!(done.breakdown.travel > 0.0);
    assert!(done.breakdown.fighting > 0.0);
    assert!(done.breakdown.waiting > 0.0);
    assert!(done.breakdown.mob_xp > 0);
    assert_eq!(done.program.phases[0].cycles, 2);
    assert_eq!(
        done.program.phases[0].actions[0].units_per_cycle, 1,
        "carry only one turn-in cycle"
    );
    let replay = planner(&p).replay_reputation(&[], None, &done.program).unwrap();
    assert_eq!(done.breakdown.total, replay.breakdown.total);
    assert_eq!(done.end.xp, replay.end.xp);
    p.params.group_size = 2;
    let group = planner(&p).complete_reputation(&[], None).unwrap();
    assert!(
        group.program.phases.is_empty(),
        "unverified individual drop ownership cannot fund a group turn-in"
    );
    assert!(
        group
            .program
            .result
            .warnings
            .iter()
            .any(|w| w.contains("group") && w.contains("credit"))
    );
}

#[test]
fn reputation_program_partial_daily_tail_and_shared_quota_survive_replay() {
    let mut p = scenario();
    p.params.reputation_goals[0].target = 75;
    for id in ["a", "b"] {
        let mut a = source(&p, id, 68, 25.0);
        a.descriptor.reset = SourceReset {
            kind: ResetKind::Daily,
            quota_key: Some(id.into()),
            units_per_reset: 1,
        };
        p.model.reputation_sources.sources.push(a);
    }
    let start = stock(&[(14047, 60)]);
    let done = planner(&p).complete_reputation(&[], Some(&start)).unwrap();
    assert_eq!(done.program.phases.len(), 2);
    assert_eq!(done.program.phases[0].actions.len(), 2);
    assert_eq!(done.program.phases[1].actions.len(), 1);
    assert_eq!(done.program.result.daily_opportunities, 2);
    assert_eq!(done.program.result.reset_transitions, 1);
    let replay = planner(&p).replay_reputation(&[], Some(&start), &done.program).unwrap();
    assert_eq!(replay.end.reputation, done.end.reputation);
    assert_eq!(replay.program.result.reset_transitions, 1);
    // Distinct source IDs sharing one daily quota cannot both complete in one window.
    p.model.reputation_sources.sources[1].descriptor.reset.quota_key = Some("a".into());
    let shared = planner(&p).complete_reputation(&[], Some(&start)).unwrap();
    assert_eq!(shared.program.result.daily_opportunities, 3);
    assert_eq!(shared.program.result.reset_transitions, 2);
}

#[test]
fn reputation_program_unfinished_work_is_finalized_only_after_source_combat() {
    let mut p = crate::testing::Scenario::new()
        .levels(1, 1)
        .quest(
            crate::testing::Q::new(1, "Unfinished wolves")
                .xp(0)
                .kill("Wolf", 1, 4, 1, 50.0, 50.0),
        )
        .params(|params| {
            params.camp_radius = 0.0;
            params.farm_on_way = false;
            params.kill_time = 24.0;
            params.stop_overhead = 0.0;
            params.bag_weight = 0.0;
            params.reputation_goals = vec![ReputationGoal {
                faction: 68,
                target: 100,
            }];
        })
        .build();
    let mut a = source(&p, "kill", 68, 50.0);
    a.kind = BoundSourceKind::Kill;
    a.automatic_farming = true;
    a.descriptor.input_items.clear();
    a.descriptor.costs.interaction_seconds = 0.0;
    a.creature = Some(crate::model::BoundCreature {
        npc_id: 888,
        min_level: 1,
        max_level: 1,
        rank: 0,
        respawn_seconds: Some(100.0),
        spawn_count: 100,
    });
    a.descriptor.identity.npc_id = Some(888);
    p.model.reputation_sources.sources.clear();
    p.model.reputation_sources.sources.push(a);
    let route = [
        super::Stop::quest(0, super::Kind::Accept),
        super::Stop::quest(0, super::Kind::Objective(0)),
    ];
    let plain = planner(&p).simulate_full(&route, None).unwrap().1;
    let done = planner(&p).complete_reputation(&route, None).unwrap();
    assert_eq!(done.breakdown.grinding, plain.grinding);
    assert!(
        done.breakdown.fighting >= 48.0,
        "source combat must not be reclassified a second time"
    );
}

#[test]
fn reputation_program_limited_vendor_restocks_each_missing_unit() {
    let mut p = scenario();
    p.model.initial.money = 6;
    let mut a = source(&p, "limited", 68, 50.0);
    a.descriptor.input_items[0].count = 3;
    let descriptor = SourceAcquisition {
        kind: AcquisitionKind::PricedVendor,
        item_id: 14047,
        npc_id: Some(777),
        unit_price_copper: Some(1),
        location: a.descriptor.location.clone(),
        ..SourceAcquisition::default()
    };
    a.acquisition.push(BoundAcquisition {
        descriptor,
        locations: a.locations.clone(),
        creature: None,
        stock: Some((3, 600.0)),
    });
    p.model.reputation_sources.sources.push(a);
    let done = planner(&p).complete_reputation(&[], None).unwrap();
    assert_eq!(done.end.reputation[&68], 100.0);
    assert_eq!(done.breakdown.waiting, 1800.0);
}

#[test]
fn reputation_program_source_standing_maximum_is_exclusive() {
    let mut p = scenario();
    let mut a = source(&p, "repair", 68, 499.0);
    a.descriptor.eligibility.standing.push(FactionStandingRequirement {
        faction_id: 68,
        standing: StandingRange {
            minimum: None,
            maximum: Some(0.0),
        },
    });
    p.model.reputation_sources.sources.push(a);
    p.model.initial.reputation[0] = -499.0;
    let start = stock(&[(14047, 100)]);
    let done = planner(&p).complete_reputation(&[], Some(&start)).unwrap();
    assert_eq!(done.end.reputation[&68], 0.0);
    assert_eq!(done.end.inventory[&14047], 80);
    p.model.initial.reputation[0] = 0.0;
    let neutral = planner(&p).complete_reputation(&[], Some(&start)).unwrap();
    assert_eq!(neutral.program.phases.len(), 0);
}

#[test]
fn reputation_program_kill_gray_scaling_changes_at_actual_level_up() {
    let mut p = scenario();
    p.profile.from_level = 5;
    p.profile.to_level = 5;
    p.model.initial.xp = p.params.edition.rules().to_next_level(5) - 1;
    p.params.reputation_goals[0].target = 6;
    let mut a = source(&p, "gray", 68, 5.0);
    a.kind = BoundSourceKind::Kill;
    a.automatic_farming = true;
    a.descriptor.input_items.clear();
    a.descriptor.costs.interaction_seconds = 0.0;
    a.descriptor.effects.encoding = RewardEncoding::PrimaryAwards;
    a.descriptor.effects.awards[0].rounding = RoundingPolicy::Truncate;
    a.creature = Some(crate::model::BoundCreature {
        npc_id: 888,
        min_level: 1,
        max_level: 1,
        rank: 0,
        respawn_seconds: Some(100.0),
        spawn_count: 100,
    });
    p.model.reputation_sources.sources.push(a);
    let done = planner(&p).complete_reputation(&[], None).unwrap();
    assert_eq!(done.end.level, 6);
    assert_eq!(done.end.reputation[&68], 6.0);
    assert_eq!(done.program.phases[0].cycles, 2);
}

#[test]
fn reputation_program_editor_replays_actual_prefix_and_keeps_one_marker() {
    let mut p = scenario();
    p.model.reputation_sources.sources.push(source(&p, "cloth", 68, 50.0));
    let start = stock(&[(14047, 40)]);
    let done = planner(&p).complete_reputation(&[], Some(&start)).unwrap();
    let edits = [
        crate::plan::Edit::Grind { mobs: 1 },
        crate::plan::Edit::ReputationProgram,
    ];
    let replay = planner(&p)
        .edit_with_reputation(&edits, Some(&start), &done.program)
        .unwrap();
    assert_eq!(replay.errors(), 0);
    assert_eq!(replay.steps.len(), 2);
    assert!(replay.steps[1].end > replay.steps[1].start);
    assert_eq!(replay.end.as_ref().unwrap().reputation[&68], 100.0);
    assert!(
        replay.end.as_ref().unwrap().xp > done.end.xp,
        "explicit prefix grind is retained"
    );
    assert!(
        planner(&p)
            .edit_with_reputation(&[], Some(&start), &done.program)
            .is_err()
    );
    let invalid = planner(&p)
        .edit_with_reputation(
            &[
                crate::plan::Edit::ReputationProgram,
                crate::plan::Edit::Grind { mobs: 1 },
            ],
            Some(&start),
            &done.program,
        )
        .unwrap();
    assert_eq!(invalid.errors(), 1);
    assert!(invalid.reputation_program.is_none());
}

#[test]
fn reputation_program_spawn_pool_at_other_locations_is_not_free_local_supply() {
    let mut p = scenario();
    let mut a = source(&p, "one-spawn", 68, 50.0);
    a.kind = BoundSourceKind::Kill;
    a.automatic_farming = true;
    a.descriptor.input_items.clear();
    a.creature = Some(crate::model::BoundCreature {
        npc_id: 888,
        min_level: 1,
        max_level: 1,
        rank: 0,
        respawn_seconds: Some(100.0),
        spawn_count: 2,
    });
    let mut far = a.locations[0].clone();
    far.x = far.x.map(|x| x + 7000.0);
    a.locations.push(far);
    p.model.reputation_sources.sources.push(a);
    let done = planner(&p).complete_reputation(&[], None).unwrap();
    assert!(
        done.breakdown.waiting > 0.0,
        "one selected location cannot borrow a distant spawn"
    );
}

#[test]
fn reputation_program_incidental_identified_rewards_do_not_truncate_valid_prefix() {
    let mut p = crate::testing::Scenario::new()
        .levels(1, 1)
        .quest(
            crate::testing::Q::new(1, "Known wolves")
                .xp(0)
                .kill("Wolf", 1, 2, 1, 50.0, 50.0),
        )
        .quest(crate::testing::Q::new(2, "Preserved prefix delivery").xp(0))
        .params(|params| {
            params.camp_radius = 0.0;
            params.farm_on_way = false;
            params.reputation_goals = vec![ReputationGoal {
                faction: 68,
                target: 100,
            }];
        })
        .build();
    p.model.quests[0].objectives[0].mobs = vec![(888, "Wolf".into())];
    let mut a = source(&p, "identified", 68, 50.0);
    a.kind = BoundSourceKind::Kill;
    a.automatic_farming = false;
    a.descriptor.identity.npc_id = Some(888);
    a.descriptor.input_items.clear();
    a.creature = Some(crate::model::BoundCreature {
        npc_id: 888,
        min_level: 1,
        max_level: 1,
        rank: 0,
        respawn_seconds: Some(100.0),
        spawn_count: 2,
    });
    p.model.reputation_sources.sources.clear();
    p.model.reputation_sources.sources.push(a);
    let route = [
        super::Stop::quest(0, super::Kind::Accept),
        super::Stop::quest(0, super::Kind::Objective(0)),
        super::Stop::quest(0, super::Kind::TurnIn),
        super::Stop::quest(1, super::Kind::Accept),
        super::Stop::quest(1, super::Kind::TurnIn),
    ];
    let done = planner(&p).complete_reputation(&route, None).unwrap();
    assert_eq!(done.end.reputation[&68], 100.0);
    assert!(done.end.completed_quests.contains(&2));
}

#[test]
fn reputation_program_known_prefix_kills_keep_batch_xp_and_following_level_gate() {
    let mut p = crate::testing::Scenario::new()
        .levels(1, 3)
        .quest(
            crate::testing::Q::new(1, "Known wolves")
                .xp(0)
                .kill("Wolf", 1, 26, 1, 50.0, 50.0),
        )
        .quest(
            crate::testing::Q::new(2, "Level three delivery")
                .level(3)
                .min_level(3)
                .xp(0),
        )
        .params(|params| {
            params.camp_radius = 0.0;
            params.farm_on_way = false;
            params.reputation_goals = vec![ReputationGoal {
                faction: 68,
                target: 100,
            }];
        })
        .build();
    p.model.quests[0].objectives[0].mobs = vec![(888, "Wolf".into())];
    let mut a = source(&p, "identified", 68, 1.0);
    a.kind = BoundSourceKind::Kill;
    a.automatic_farming = false;
    a.descriptor.identity.npc_id = Some(888);
    a.descriptor.input_items.clear();
    a.creature = Some(crate::model::BoundCreature {
        npc_id: 888,
        min_level: 1,
        max_level: 1,
        rank: 0,
        respawn_seconds: Some(100.0),
        spawn_count: 2,
    });
    p.model.reputation_sources.sources.clear();
    p.model.reputation_sources.sources.push(a);
    let route = [
        super::Stop::quest(0, super::Kind::Accept),
        super::Stop::quest(0, super::Kind::Objective(0)),
        super::Stop::quest(0, super::Kind::TurnIn),
        super::Stop::quest(1, super::Kind::Accept),
        super::Stop::quest(1, super::Kind::TurnIn),
    ];
    let ordinary = planner(&p).simulate_full(&route, None).unwrap().1;
    let done = planner(&p)
        .complete_reputation(&route, None)
        .expect("ordinary valid prefix remains valid");
    assert!(done.end.completed_quests.contains(&2));
    assert_eq!(done.breakdown.mob_xp, ordinary.mob_xp);
    assert_eq!(done.breakdown.total, ordinary.total);
    assert_eq!(done.breakdown.grind_xp, ordinary.grind_xp);
}

#[test]
fn reputation_program_classic_resolved_prefix_clamps_verified_global_bounds() {
    let mut p = quests_at_start(1);
    p.profile.to_level = 1;
    p.params.edition = crate::xp::Edition::Classic;
    p.params.reputation_goals = vec![ReputationGoal {
        faction: 68,
        target: 42999,
    }];
    p.model.initial.reputation = vec![42990.0];
    p.model.initial.reputation_all.insert(68, 42990.0);
    p.model.initial.reputation_all.insert(76, -41990.0);
    p.model.quests[0].reputation = vec![(68, 100.0), (76, -100.0)];
    let route = [
        super::Stop::quest(0, super::Kind::Accept),
        super::Stop::quest(0, super::Kind::TurnIn),
    ];
    let done = planner(&p).complete_reputation(&route, None).unwrap();
    assert_eq!(done.end.reputation[&68], 42999.0);
    assert_eq!(done.end.reputation[&76], -42000.0);
    p.params.edition = crate::xp::Edition::Forever;
    let unknown = planner(&p).complete_reputation(&route, None).unwrap();
    assert_eq!(
        unknown.end.reputation[&68], 43090.0,
        "Forever limits are not confirmed by Classic data"
    );
    assert_eq!(unknown.end.reputation[&76], -42090.0);
}

#[test]
fn reputation_program_empty_and_unknown_replay_cycles_stop_with_named_limitations() {
    let p = scenario();
    for actions in [
        vec![],
        vec![RepAction {
            source_id: "missing-source".into(),
            units_per_cycle: u32::MAX,
            total_unit_limit: u32::MAX,
            ..RepAction::default()
        }],
    ] {
        let program = RepProgram {
            version: 1,
            phases: vec![RepPhase {
                id: "unavailable".into(),
                kind: RepPhaseKind::DailyCircuit,
                cycles: u32::MAX,
                actions,
            }],
            ..RepProgram::default()
        };
        let done = planner(&p).replay_reputation(&[], None, &program).unwrap();
        assert!(
            done.program
                .result
                .warnings
                .iter()
                .any(|w| w.contains("remaining cycles") || w.contains("visit limit")),
            "a saved no-op circuit must disclose its replay bound"
        );
        assert_eq!(done.program.result.daily_opportunities, 0);
        assert_eq!(done.program.result.reset_transitions, 0);
        assert_eq!(done.end.reputation[&68], 0.0);
    }
}
