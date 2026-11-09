use super::*;

fn award(faction_id: i64, amount: f64) -> SignedAward {
    SignedAward {
        faction_id,
        amount,
        ..SignedAward::default()
    }
}

fn primary(awards: Vec<SignedAward>) -> ReputationEvent {
    ReputationEvent {
        source_id: "synthetic:kill".into(),
        edition: "classic".into(),
        confidence: SourceConfidence::ClassicEstimate,
        effects: ReputationEffects {
            encoding: RewardEncoding::PrimaryAwards,
            awards,
            ..ReputationEffects::default()
        },
    }
}

fn spill(primary_faction_id: i64, recipient_faction_id: i64, rate: f64) -> SpilloverRule {
    SpilloverRule {
        primary_faction_id,
        recipient_faction_id,
        rate,
        rounding: RoundingPolicy::Truncate,
        ..SpilloverRule::default()
    }
}

#[test]
fn resolved_fractional_vectors_apply_once_without_inferred_spillover_or_gray_penalty() {
    let mut event = primary(vec![award(67, 12.5), award(68, 50.0), award(76, 12.5)]);
    event.effects.encoding = RewardEncoding::ResolvedVector;
    event.effects.modifiers.gray_multiplier = 0.2;
    event.effects.spillover = SpilloverPolicy::Explicit(vec![spill(68, 67, 0.25), spill(67, 81, 0.25)]);
    let mut state = Standings::new();

    let result = apply_event(&mut state, &event).expect("valid resolved vector");

    assert_eq!(state, Standings::from([(67, 12.5), (68, 50.0), (76, 12.5)]));
    assert_eq!(result.deltas, state);
    assert_eq!(result.confidence, SourceConfidence::ClassicEstimate);
}

#[test]
fn signed_losses_do_not_receive_human_gain_or_gray_modifiers() {
    let mut event = primary(vec![award(21, 10.0), award(87, -25.0)]);
    event.effects.modifiers = RewardModifiers {
        human_bonus: 0.1,
        gain_multiplier: 2.0,
        gray_multiplier: 0.2,
    };
    let mut state = Standings::new();

    apply_event(&mut state, &event).expect("valid signed awards");

    assert!((state[&21] - 4.4).abs() < 1e-12);
    assert_eq!(state[&87], -25.0);
}

#[test]
fn resolved_human_bonus_is_positive_only_and_preserves_fractions() {
    let mut event = primary(vec![award(67, 12.5), award(87, -12.5)]);
    event.effects.encoding = RewardEncoding::ResolvedVector;
    event.effects.modifiers.human_bonus = 0.1;
    let mut state = Standings::new();

    apply_event(&mut state, &event).expect("valid resolved vector");

    assert!((state[&67] - 13.75).abs() < 1e-12);
    assert_eq!(state[&87], -12.5);
}

#[test]
fn explicit_spillover_is_nonrecursive_and_recipient_capped() {
    let mut event = primary(vec![award(1, 5.0)]);
    let mut capped = spill(1, 4, 0.5);
    capped.cap = Some(AwardCap::PreEventRankInclusive { maximum_rank: 4 });
    event.effects.spillover = SpilloverPolicy::Explicit(vec![spill(1, 2, 0.5), spill(2, 3, 0.5), capped]);
    let mut state = Standings::from([(4, 9000.0)]);

    apply_event(&mut state, &event).expect("valid explicit template");

    assert_eq!(state, Standings::from([(1, 5.0), (2, 2.0), (4, 9000.0)]));
}

#[test]
fn primary_slot_suppression_prevents_its_template_without_disabling_other_awards() {
    let mut suppressed = award(1, 10.0);
    suppressed.suppress_spillover = true;
    let mut event = primary(vec![suppressed, award(3, 10.0)]);
    event.effects.spillover = SpilloverPolicy::Explicit(vec![spill(1, 2, 0.5), spill(3, 4, 0.5)]);
    let mut state = Standings::new();

    apply_event(&mut state, &event).expect("per-slot suppression");

    assert_eq!(state, Standings::from([(1, 10.0), (3, 10.0), (4, 5.0)]));
}

#[test]
fn slots_and_spillover_recipient_gates_run_in_recorded_order() {
    let mut capped = award(1, 10.0);
    capped.cap = Some(AwardCap::PreEventInclusive { maximum: 9.0 });
    let mut event = primary(vec![award(1, 10.0), capped]);
    let mut recipient = spill(1, 2, 0.5);
    recipient.cap = Some(AwardCap::PreEventInclusive { maximum: 4.0 });
    event.effects.spillover = SpilloverPolicy::Explicit(vec![recipient]);
    let mut state = Standings::new();

    apply_event(&mut state, &event).expect("ordered source slots");

    assert_eq!(state, Standings::from([(1, 10.0), (2, 5.0)]));
}

#[test]
fn source_bounds_clamp_both_gains_and_losses_independently_of_rank_caps() {
    let mut event = primary(vec![award(1, 5.0), award(2, -25.0)]);
    event.effects.standing_bounds = StandingRange {
        minimum: Some(-42000.0),
        maximum: Some(42999.0),
    };
    let mut state = Standings::from([(1, 42998.0), (2, -41990.0)]);

    apply_event(&mut state, &event).expect("explicit Classic global bounds");

    assert_eq!(state, Standings::from([(1, 42999.0), (2, -42000.0)]));
}

#[test]
fn primary_pre_event_inclusive_cap_allows_full_overshoot_then_blocks() {
    let mut gain = award(470, 5.0);
    gain.cap = Some(AwardCap::PreEventInclusive { maximum: 8999.0 });
    let event = primary(vec![gain]);
    let mut state = Standings::from([(470, 8999.0)]);

    apply_event(&mut state, &event).expect("last eligible event");
    assert_eq!(state[&470], 9004.0);
    let blocked = apply_event(&mut state, &event).expect("capped event");

    assert_eq!(state[&470], 9004.0);
    assert!(blocked.deltas.is_empty());
}

#[test]
fn classic_rank_gate_includes_entire_rank_and_tests_pre_event_standing() {
    let mut gain = award(470, 5.0);
    gain.cap = Some(AwardCap::PreEventRankInclusive { maximum_rank: 5 });
    let event = primary(vec![gain]);
    let mut state = Standings::from([(470, 20999.5)]);

    apply_event(&mut state, &event).expect("last honored event");
    apply_event(&mut state, &event).expect("revered event blocked");

    assert_eq!(state[&470], 21004.5);
}

#[test]
fn per_kill_spillover_truncates_before_batching_and_after_gray_reduction() {
    let mut event = primary(vec![award(470, 5.0)]);
    event.effects.spillover = SpilloverPolicy::Explicit(vec![spill(470, 21, 0.5)]);
    let mut state = Standings::new();

    apply_batch(&mut state, &event, 3).expect("three normal kills");
    assert_eq!(state, Standings::from([(21, 6.0), (470, 15.0)]));

    event.effects.modifiers.gray_multiplier = 0.2;
    apply_batch(&mut state, &event, 3).expect("three gray kills");
    assert_eq!(state, Standings::from([(21, 6.0), (470, 18.0)]));
}

#[test]
fn ordinary_spillover_shares_primary_gate_but_team_award_has_its_own_gate() {
    let mut gain = award(470, 5.0);
    gain.cap = Some(AwardCap::PreEventInclusive { maximum: 8999.0 });
    let mut event = primary(vec![gain]);
    let mut team = spill(470, 169, 0.5);
    team.gate = SpilloverGate::Independent;
    event.effects.spillover = SpilloverPolicy::Explicit(vec![spill(470, 21, 0.5), team]);
    let mut state = Standings::from([(470, 8999.0)]);

    apply_batch(&mut state, &event, 2).expect("primary then independently gated team awards");

    assert_eq!(state, Standings::from([(21, 2.0), (169, 4.0), (470, 9004.0)]));
}

#[test]
fn batch_matches_ordered_events_across_rank_caps_signed_losses_and_recipient_gates() {
    let mut gain = award(470, 5.0);
    gain.cap = Some(AwardCap::PreEventRankInclusive { maximum_rank: 4 });
    let mut loss = award(87, -3.0);
    loss.standing.minimum = Some(-7.0);
    let mut event = primary(vec![gain, loss]);
    let mut recipient = spill(470, 21, 0.5);
    recipient.cap = Some(AwardCap::PreEventInclusive { maximum: 3.0 });
    event.effects.spillover = SpilloverPolicy::Explicit(vec![recipient]);
    let mut batched = Standings::from([(470, 8989.0)]);
    let mut individual = batched.clone();

    let result = apply_batch(&mut batched, &event, 20).expect("ordered batch");
    for _ in 0..20 {
        apply_event(&mut individual, &event).expect("individual event");
    }

    assert_eq!(batched, individual);
    assert_eq!(batched, Standings::from([(21, 4.0), (87, -9.0), (470, 9004.0)]));
    assert_eq!(result.changed_units, 3);
}

#[test]
fn batch_rechecks_gates_when_losses_reopen_a_cap() {
    let mut gain = award(1, 5.0);
    gain.cap = Some(AwardCap::PreEventInclusive { maximum: 9.0 });
    let mut event = primary(vec![gain, award(1, -1.0)]);
    event.effects.spillover = SpilloverPolicy::Explicit(vec![spill(1, 2, 0.5)]);
    let mut state = Standings::from([(1, 9.0)]);
    let mut individual = state.clone();

    apply_batch(&mut state, &event, 101).expect("losses reopen a cap");
    for _ in 0..101 {
        apply_event(&mut individual, &event).expect("individual event");
    }

    assert_eq!(state, individual);
    assert_eq!(state[&1], 13.0);
    assert_eq!(state[&2], 42.0);
}

#[test]
fn batch_accounts_for_earlier_slot_offsets_when_approaching_a_later_gate() {
    let mut loss = award(1, -99.0);
    loss.cap = Some(AwardCap::PreEventInclusive { maximum: 150.0 });
    let event = primary(vec![award(1, 100.0), loss]);
    let mut state = Standings::new();

    apply_batch(&mut state, &event, 100).expect("later slot gate");

    assert_eq!(state[&1], 4951.0);
}

#[test]
fn hard_ceiling_clips_gains_but_does_not_erase_losses() {
    let mut gain = award(1, 5.0);
    gain.cap = Some(AwardCap::HardCeiling { maximum: 10.0 });
    let mut state = Standings::from([(1, 9.0)]);

    apply_batch(&mut state, &primary(vec![gain]), 2).expect("hard ceiling");
    assert_eq!(state[&1], 10.0);
    let mut loss = award(1, -3.0);
    loss.cap = Some(AwardCap::HardCeiling { maximum: 10.0 });
    apply_event(&mut state, &primary(vec![loss])).expect("loss below hard ceiling");
    assert_eq!(state[&1], 7.0);
}

#[test]
fn batch_recomputes_partially_clipped_awards_before_applying_a_later_loss() {
    let mut gain = award(1, 5.0);
    gain.cap = Some(AwardCap::HardCeiling { maximum: 10.0 });
    let event = primary(vec![gain, award(1, -1.0)]);
    let mut state = Standings::from([(1, 8.0)]);

    apply_batch(&mut state, &event, 2).expect("partially clipped event");

    assert_eq!(state[&1], 9.0);
}

#[test]
fn fractional_batch_does_not_change_the_number_of_awards_at_an_inclusive_gate() {
    let mut gain = award(1, 0.1);
    gain.cap = Some(AwardCap::PreEventInclusive { maximum: 0.1 });
    let event = primary(vec![gain]);
    let mut state = Standings::from([(1, -1.3)]);
    let mut ordered = state.clone();

    let batch = apply_batch(&mut state, &event, 200).expect("fractional inclusive gate");
    for _ in 0..200 {
        apply_event(&mut ordered, &event).expect("ordered fractional event");
    }

    assert_eq!(state, ordered);
    assert_eq!(batch.changed_units, 15);
}

#[test]
fn fractional_slot_arithmetic_is_preserved_even_when_the_net_delta_is_an_integer() {
    let mut gated = award(1, 0.1);
    gated.cap = Some(AwardCap::PreEventInclusive { maximum: 18.0 });
    let event = primary(vec![gated, award(1, 0.1), award(1, 0.8)]);
    let mut state = Standings::from([(1, 0.0)]);
    let mut ordered = state.clone();

    apply_batch(&mut state, &event, 30).expect("fractional intermediate slots");
    for _ in 0..30 {
        apply_event(&mut ordered, &event).expect("ordered intermediate slots");
    }

    assert_eq!(state, ordered);
}

#[test]
fn mean_and_conservative_rounding_are_explicit_forecasts() {
    let mut gain = award(1, 1.5);
    gain.rounding = RoundingPolicy::MeanForecast;
    let mut event = primary(vec![gain]);
    event.effects.spillover = SpilloverPolicy::Explicit(vec![spill(1, 2, 0.5)]);
    let mut state = Standings::new();

    apply_event(&mut state, &event).expect("mean stochastic forecast");
    assert_eq!(state, Standings::from([(1, 1.5), (2, 0.5)]));

    event.effects.awards[0].rounding = RoundingPolicy::ConservativeForecast;
    let mut conservative = Standings::new();
    apply_event(&mut conservative, &event).expect("conservative forecast");
    assert_eq!(conservative, Standings::from([(1, 1.0)]));
}

#[test]
fn zero_change_and_large_batches_terminate_without_expanding_every_event() {
    let event = primary(vec![award(1, 5.0)]);
    let mut state = Standings::new();
    let large = apply_batch(&mut state, &event, 1_000_000_000).expect("constant segment");
    assert_eq!(state[&1], 5_000_000_000.0);
    assert_eq!(large.changed_units, 1_000_000_000);

    let zero = apply_batch(&mut state, &primary(vec![award(1, 0.0)]), u32::MAX).expect("zero segment");
    assert_eq!(zero.changed_units, 0);
    assert!(zero.deltas.is_empty());
    assert_eq!(state[&1], 5_000_000_000.0);
}

#[test]
fn invalid_nonfinite_rules_are_rejected_before_mutating_standings() {
    let mut state = Standings::from([(1, 10.0)]);
    let before = state.clone();
    let event = primary(vec![award(1, 5.0), award(2, f64::NAN)]);

    assert!(matches!(
        apply_event(&mut state, &event),
        Err(TransitionError::InvalidRule(_))
    ));
    assert_eq!(state, before);
}

#[test]
fn program_roundtrip_retains_daily_circuit_order_and_unknown_wait() {
    let program = RepProgram {
        version: 1,
        phases: vec![RepPhase {
            id: "daily:0".into(),
            kind: RepPhaseKind::DailyCircuit,
            cycles: 3,
            actions: vec![
                RepAction {
                    id: "run:a".into(),
                    source_id: "quest:a".into(),
                    units_per_cycle: 1,
                    total_unit_limit: 3,
                    ..RepAction::default()
                },
                RepAction {
                    id: "run:b".into(),
                    source_id: "quest:b".into(),
                    units_per_cycle: 1,
                    total_unit_limit: 3,
                    ..RepAction::default()
                },
            ],
        }],
        result: RepProgramResult {
            daily_opportunities: 3,
            reset_transitions: 2,
            ..RepProgramResult::default()
        },
        ..RepProgram::default()
    };
    let json = serde_json::to_string(&program).expect("serializable program");
    let restored: RepProgram = serde_json::from_str(&json).expect("readable program");
    assert_eq!(restored, program);
    assert!(restored.result.calendar_wait_seconds.is_none());
}
