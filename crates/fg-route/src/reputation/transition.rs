use super::{
    AwardCap, BatchTransitionResult, ReputationEvent, RewardEncoding, RoundingPolicy, SpilloverGate, SpilloverPolicy,
    StandingRange, Standings, TransitionResult,
};
use std::collections::{BTreeMap, BTreeSet};

const RANK_STARTS: [f64; 8] = [-42000.0, -6000.0, -3000.0, 0.0, 3000.0, 9000.0, 21000.0, 42000.0];
const MAX_BATCH_SEGMENTS: usize = 100_000;
const MAX_EXACT_INTEGER: f64 = 9_007_199_254_740_992.0;

#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum TransitionError {
    #[error("invalid reputation rule: {0}")]
    InvalidRule(String),
    #[error("reputation batch exceeds the bounded transition budget")]
    BatchTooComplex,
}

/// Apply one event in recorded slot order, with direct spillover before each primary
/// and independently gated team awards afterward. Invalid arithmetic is atomic.
/// Mean/conservative policies are deterministic forecasts, not game RNG outcomes.
pub fn apply_event(standings: &mut Standings, event: &ReputationEvent) -> Result<TransitionResult, TransitionError> {
    validate(standings, event)?;
    let mut next = standings.clone();
    let probe = step(&mut next, event)?;
    *standings = next;
    Ok(TransitionResult {
        source_id: event.source_id.clone(),
        confidence: event.confidence,
        deltas: probe.deltas,
    })
}

/// Replay an ordered batch, skipping constant segments only up to the next gate.
/// Repeated gated states permit exact cycle skipping. Pathological non-repeating
/// programs return a bounded-complexity error without changing the input.
/// Grouping additions can differ from single events by floating-point precision.
pub fn apply_batch(
    standings: &mut Standings,
    event: &ReputationEvent,
    units: u32,
) -> Result<BatchTransitionResult, TransitionError> {
    validate(standings, event)?;
    let mut next = standings.clone();
    let gate_factions = gate_factions(event);
    let mut seen: BTreeMap<Vec<(i64, u64)>, Cycle> = BTreeMap::new();
    let mut completed = 0;
    let mut changed_units = 0;
    let mut segments = 0;
    while completed < units {
        if segments >= MAX_BATCH_SEGMENTS {
            return Err(TransitionError::BatchTooComplex);
        }
        let key: Vec<_> = gate_factions
            .iter()
            .map(|&faction| {
                let value = value(&next, faction);
                (faction, if value == 0.0 { 0 } else { value.to_bits() })
            })
            .collect();
        if let Some(previous) = seen.get(&key) {
            let period = completed - previous.completed;
            let cycles = (units - completed) / period;
            if cycles > 0 {
                let cycle_deltas = differences(&previous.standings, &next)?;
                add_scaled(&mut next, &cycle_deltas, cycles)?;
                changed_units += (changed_units - previous.changed_units) * cycles;
                completed += period * cycles;
                continue;
            }
        } else {
            seen.insert(
                key,
                Cycle {
                    completed,
                    changed_units,
                    standings: next.clone(),
                },
            );
        }
        let before = next.clone();
        let probe = step(&mut next, event)?;
        segments += 1;
        if probe.deltas.is_empty() {
            break;
        }
        let count = segment_units(&before, &probe, units - completed);
        add_scaled(&mut next, &probe.deltas, count - 1)?;
        completed += count;
        changed_units += count;
    }
    let deltas = differences(standings, &next)?;
    *standings = next;
    Ok(BatchTransitionResult {
        source_id: event.source_id.clone(),
        confidence: event.confidence,
        units,
        changed_units,
        deltas,
    })
}

struct Cycle {
    completed: u32,
    changed_units: u32,
    standings: Standings,
}

#[derive(Default)]
struct Probe {
    deltas: Standings,
    varying: bool,
    fractional: bool,
    has_gate: bool,
    /// Gate positions translated back to the beginning of this event. Earlier
    /// slots can move a later slot's gate even when the event's net delta is tiny.
    boundaries: Vec<(i64, f64)>,
}

fn step(standings: &mut Standings, event: &ReputationEvent) -> Result<Probe, TransitionError> {
    let before = standings.clone();
    let mut probe = Probe::default();
    let effects = &event.effects;
    let primary = effects.encoding == RewardEncoding::PrimaryAwards;
    for award in &effects.awards {
        let raw = if award.amount > 0.0 {
            award.amount
                * effects.modifiers.gain_multiplier
                * (1.0 + effects.modifiers.human_bonus)
                * if primary {
                    effects.modifiers.gray_multiplier
                } else {
                    1.0
                }
        } else {
            award.amount
        };
        finite(raw)?;
        let amount = if primary { round(raw, award.rounding) } else { raw };
        record_gates(
            &mut probe,
            &before,
            standings,
            award.faction_id,
            award.standing,
            award.cap,
        );
        let allowed = eligible(value(standings, award.faction_id), award.standing, award.cap, amount);
        let rules = if primary && !award.suppress_spillover {
            match &effects.spillover {
                SpilloverPolicy::Explicit(rules) => rules.as_slice(),
                SpilloverPolicy::None => &[],
            }
        } else {
            &[]
        };
        for gate in [SpilloverGate::PrimaryAward, SpilloverGate::Independent] {
            if gate == SpilloverGate::Independent && allowed {
                record_gates(
                    &mut probe,
                    &before,
                    standings,
                    award.faction_id,
                    effects.standing_bounds,
                    None,
                );
                record_addition(
                    &mut probe,
                    &before,
                    standings,
                    award.faction_id,
                    amount,
                    award.cap,
                    effects.standing_bounds,
                );
                add(standings, award.faction_id, amount, award.cap, effects.standing_bounds)?;
            }
            if gate == SpilloverGate::PrimaryAward && !allowed {
                continue;
            }
            for rule in rules
                .iter()
                .filter(|r| r.primary_faction_id == award.faction_id && r.gate == gate)
            {
                let spill = if award.rounding == RoundingPolicy::MeanForecast {
                    // Truncating the mean would misstate a stochastic half award:
                    // 1/2 * trunc(1*.5) + 1/2 * trunc(2*.5) is .5, not zero.
                    let lower = raw.floor();
                    let fraction = raw - lower;
                    round(lower * rule.rate, rule.rounding) * (1.0 - fraction)
                        + round(raw.ceil() * rule.rate, rule.rounding) * fraction
                } else {
                    round(amount * rule.rate, rule.rounding)
                };
                finite(spill)?;
                record_gates(
                    &mut probe,
                    &before,
                    standings,
                    rule.recipient_faction_id,
                    rule.standing,
                    rule.cap,
                );
                if eligible(
                    value(standings, rule.recipient_faction_id),
                    rule.standing,
                    rule.cap,
                    spill,
                ) {
                    record_gates(
                        &mut probe,
                        &before,
                        standings,
                        rule.recipient_faction_id,
                        effects.standing_bounds,
                        None,
                    );
                    record_addition(
                        &mut probe,
                        &before,
                        standings,
                        rule.recipient_faction_id,
                        spill,
                        rule.cap,
                        effects.standing_bounds,
                    );
                    add(
                        standings,
                        rule.recipient_faction_id,
                        spill,
                        rule.cap,
                        effects.standing_bounds,
                    )?;
                }
            }
        }
    }
    probe.deltas = differences(&before, standings)?;
    Ok(probe)
}

fn value(standings: &Standings, faction: i64) -> f64 {
    standings.get(&faction).copied().unwrap_or(0.0)
}

fn round(value: f64, rounding: RoundingPolicy) -> f64 {
    match rounding {
        RoundingPolicy::ExactFractional | RoundingPolicy::MeanForecast => value,
        RoundingPolicy::Truncate => value.trunc(),
        RoundingPolicy::ConservativeForecast => value.floor(),
    }
}

fn eligible(standing: f64, range: StandingRange, cap: Option<AwardCap>, amount: f64) -> bool {
    range.minimum.is_none_or(|minimum| standing >= minimum)
        && range.maximum.is_none_or(|maximum| standing <= maximum)
        && match cap {
            Some(AwardCap::PreEventInclusive { maximum }) => standing <= maximum,
            Some(AwardCap::PreEventRankInclusive { maximum_rank }) => {
                rank_end(maximum_rank).is_none_or(|end| standing < end)
            }
            Some(AwardCap::HardCeiling { maximum }) => amount <= 0.0 || standing < maximum,
            None => true,
        }
}

fn rank_end(rank: u8) -> Option<f64> {
    RANK_STARTS.get(usize::from(rank) + 1).copied()
}

fn add(
    standings: &mut Standings,
    faction: i64,
    amount: f64,
    cap: Option<AwardCap>,
    bounds: StandingRange,
) -> Result<(), TransitionError> {
    if amount == 0.0 {
        return Ok(());
    }
    let current = value(standings, faction);
    let mut next = current + amount;
    if amount > 0.0 {
        if let Some(AwardCap::HardCeiling { maximum }) = cap {
            next = next.min(maximum).max(current);
        }
        if let Some(maximum) = bounds.maximum {
            next = next.min(maximum).max(current);
        }
    } else if let Some(minimum) = bounds.minimum {
        next = next.max(minimum).min(current);
    }
    finite(next)?;
    if next - current != 0.0 {
        standings.insert(faction, next);
    }
    Ok(())
}

fn differences(before: &Standings, after: &Standings) -> Result<Standings, TransitionError> {
    let mut result = Standings::new();
    for (&faction, &standing) in after {
        let delta = standing - value(before, faction);
        finite(delta)?;
        if delta != 0.0 {
            result.insert(faction, delta);
        }
    }
    Ok(result)
}

fn add_scaled(standings: &mut Standings, deltas: &Standings, units: u32) -> Result<(), TransitionError> {
    if units == 0 {
        return Ok(());
    }
    for (&faction, &delta) in deltas {
        let next = value(standings, faction) + delta * f64::from(units);
        finite(next)?;
        standings.insert(faction, next);
    }
    Ok(())
}

fn record_gates(
    probe: &mut Probe,
    before: &Standings,
    current: &Standings,
    faction: i64,
    range: StandingRange,
    cap: Option<AwardCap>,
) {
    let shift = value(current, faction) - value(before, faction);
    let cap_boundary = match cap {
        Some(AwardCap::PreEventInclusive { maximum } | AwardCap::HardCeiling { maximum }) => Some(maximum),
        Some(AwardCap::PreEventRankInclusive { maximum_rank }) => rank_end(maximum_rank),
        None => None,
    };
    for boundary in [range.minimum, range.maximum, cap_boundary].into_iter().flatten() {
        probe.has_gate = true;
        probe.fractional |= !exact_integer(boundary) || !exact_integer(value(current, faction));
        probe.boundaries.push((faction, boundary - shift));
    }
}

fn segment_units(before: &Standings, probe: &Probe, remaining: u32) -> u32 {
    if probe.varying {
        return 1;
    }
    if !probe.has_gate {
        return remaining;
    }
    // Reassociation can move a fractional total across an inclusive gate and
    // change the number of whole awards. Keep every intermediate slot ordered.
    if probe.fractional {
        return 1;
    }
    let mut result = remaining;
    for &(faction, boundary) in &probe.boundaries {
        let delta = value(&probe.deltas, faction);
        let distance = boundary - value(before, faction);
        if delta != 0.0 && (distance == 0.0 || distance.signum() == delta.signum()) {
            result = result.min((distance / delta).floor().max(1.0) as u32);
        }
    }
    result.max(1)
}

fn record_addition(
    probe: &mut Probe,
    before: &Standings,
    current: &Standings,
    faction: i64,
    amount: f64,
    cap: Option<AwardCap>,
    bounds: StandingRange,
) {
    let standing = value(current, faction);
    let shift = standing - value(before, faction);
    probe.fractional |= !exact_integer(standing) || !exact_integer(amount) || !exact_integer(standing + amount);
    // Integer addition stays exact only while every intermediate slot fits in
    // the f64 integer range. Recheck before a constant segment leaves it.
    probe.boundaries.extend([
        (faction, MAX_EXACT_INTEGER - amount - shift),
        (faction, -MAX_EXACT_INTEGER - amount - shift),
    ]);
    let hard_ceiling = match cap {
        Some(AwardCap::HardCeiling { maximum }) if amount > 0.0 => Some(maximum),
        _ => None,
    };
    if amount > 0.0 {
        for maximum in [hard_ceiling, bounds.maximum].into_iter().flatten() {
            probe.boundaries.push((faction, maximum - amount - shift));
            probe.varying |= standing + amount > maximum;
        }
    } else if amount < 0.0
        && let Some(minimum) = bounds.minimum
    {
        probe.boundaries.push((faction, minimum - amount - shift));
        probe.varying |= standing + amount < minimum;
    }
}

fn exact_integer(value: f64) -> bool {
    value.fract() == 0.0 && value.abs() <= MAX_EXACT_INTEGER
}

fn gate_factions(event: &ReputationEvent) -> BTreeSet<i64> {
    let mut result = BTreeSet::new();
    let bounded = event.effects.standing_bounds != StandingRange::default();
    for award in &event.effects.awards {
        if bounded || award.cap.is_some() || award.standing != StandingRange::default() {
            result.insert(award.faction_id);
        }
    }
    if let SpilloverPolicy::Explicit(rules) = &event.effects.spillover {
        for rule in rules {
            if bounded || rule.cap.is_some() || rule.standing != StandingRange::default() {
                result.insert(rule.recipient_faction_id);
            }
        }
    }
    result
}

fn finite(value: f64) -> Result<(), TransitionError> {
    if value.is_finite() {
        Ok(())
    } else {
        Err(TransitionError::InvalidRule("non-finite standing or reward".into()))
    }
}

fn validate_range(range: StandingRange, cap: Option<AwardCap>) -> Result<(), TransitionError> {
    for bound in [range.minimum, range.maximum].into_iter().flatten() {
        finite(bound)?;
    }
    if let (Some(minimum), Some(maximum)) = (range.minimum, range.maximum)
        && minimum > maximum
    {
        return Err(TransitionError::InvalidRule("inverted standing range".into()));
    }
    match cap {
        Some(AwardCap::PreEventInclusive { maximum } | AwardCap::HardCeiling { maximum }) => finite(maximum)?,
        Some(AwardCap::PreEventRankInclusive { maximum_rank }) if maximum_rank > 7 => {
            return Err(TransitionError::InvalidRule("standing rank must be 0 through 7".into()));
        }
        _ => {}
    }
    Ok(())
}

fn validate(standings: &Standings, event: &ReputationEvent) -> Result<(), TransitionError> {
    for &standing in standings.values() {
        finite(standing)?;
    }
    let modifiers = event.effects.modifiers;
    for multiplier in [
        1.0 + modifiers.human_bonus,
        modifiers.gain_multiplier,
        modifiers.gray_multiplier,
    ] {
        finite(multiplier)?;
        if multiplier < 0.0 {
            return Err(TransitionError::InvalidRule("negative gain multiplier".into()));
        }
    }
    validate_range(event.effects.standing_bounds, None)?;
    for award in &event.effects.awards {
        finite(award.amount)?;
        validate_range(award.standing, award.cap)?;
    }
    if let SpilloverPolicy::Explicit(rules) = &event.effects.spillover {
        for rule in rules {
            finite(rule.rate)?;
            validate_range(rule.standing, rule.cap)?;
        }
    }
    Ok(())
}
