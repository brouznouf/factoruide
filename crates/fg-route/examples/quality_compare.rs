//! Reproducible single-start search-kernel comparison. This is NOT the production tournament.
//! Run through scripts/route_quality_compare.py, which owns hashes and memory observations.
use anyhow::{Context, Result, bail};
use fg_route::job::{self, Overrides, PlanRequest};
use fg_route::params::{BisMode, Construction};
use fg_route::plan::{Effort, Event, Kind, Planner, Stop};
use fg_route::xp::Edition;
use rusqlite::{Connection, OpenFlags};
use serde_json::{Value, json};
use std::cell::RefCell;
use std::collections::{BTreeMap, BTreeSet};
use std::path::PathBuf;
use std::time::{Duration, Instant};

fn stop_value(stop: Stop, planner: &Planner<'_>) -> Value {
    json!({"index": stop.index, "kind": format!("{:?}", stop.kind),
           "quest_id": stop.is_quest().then(|| planner.model.quests[stop.index as usize].id)})
}

/// Match the complete model-generated label, including quest name, ID and (for missing
/// quests) minimum level. Never infer an ID from arbitrary numbers in a diagnostic reason.
fn warning_quest_id(warning: &str, labels: impl Iterator<Item = (i64, String)>) -> Result<i64> {
    let mut matches = labels.filter(|(_, label)| warning.starts_with(label));
    let (id, _) = matches.next().context("warning has no known quest label")?;
    anyhow::ensure!(
        matches.next().is_none(),
        "warning has ambiguous quest labels: {warning}"
    );
    Ok(id)
}

fn warning_ids(planner: &Planner<'_>, warnings: &[String], timed: bool) -> Result<Vec<i64>> {
    warnings
        .iter()
        .map(|warning| {
            warning_quest_id(
                warning,
                planner.model.quests.iter().map(|q| {
                    let label = if timed {
                        format!("{} ({}): ", q.name, q.id)
                    } else {
                        format!("{} ({}, level {}): ", q.name, q.id, q.min_level)
                    };
                    (q.id, label)
                }),
            )
        })
        .collect()
}

fn main() -> Result<()> {
    let mut args = std::env::args().skip(1);
    let mut opts = BTreeMap::new();
    while let Some(key) = args.next() {
        let value = args.next().context("every option needs a value")?;
        opts.insert(key, value);
    }
    let get = |key: &str| opts.get(key).with_context(|| format!("missing {key}"));
    let edition = Edition::from_key(get("--edition")?).context("unknown edition")?;
    let seed: u64 = get("--seed")?.parse()?;
    let variant = get("--variant")?;
    let budget: f64 = get("--budget")?.parse()?;
    let reserve: f64 = get("--reserve")?.parse()?;
    if !budget.is_finite() || !reserve.is_finite() || reserve < 0.0 || budget <= reserve {
        bail!("budget must be finite and greater than nonnegative reserve");
    }
    let repo = PathBuf::from(get("--repo")?);
    let output = PathBuf::from(get("--output")?);
    // Includes input reading, data loading, preparation, construction, cleanup, replay,
    // validation, and guide export. Only artifact serialization/writing is outside this timer.
    let started = Instant::now();
    let overrides = Overrides::load_edition(&repo.join("overrides"), edition)?;
    let mut request: PlanRequest = serde_json::from_slice(&std::fs::read(get("--request")?)?)?;
    let mut params = request.params.take().unwrap_or_else(|| overrides.params.clone());
    params.edition = edition;
    params.seed = seed;
    params.bis = BisMode::Off;
    params.construction = Construction::Greedy;
    params.construction_random = false;
    params.chain_insert = 0.0;
    params.hub_repair = 0.0;
    match variant.as_str() {
        "baseline" => {}
        "chain" => params.chain_insert = 0.05,
        "hub" => params.hub_repair = 0.02,
        "exact" => params.eval_window = 0,
        _ => bail!("unknown variant {variant}"),
    }
    request.params = Some(params);
    let conn = Connection::open_with_flags(
        repo.join("data").join(edition.db_file()),
        OpenFlags::SQLITE_OPEN_READ_ONLY,
    )?;
    let data_seconds = started.elapsed().as_secs_f64();
    let stage = Instant::now();
    let prepared = job::prepare(&conn, &overrides, &request, &|_| {})?;
    let prepare_seconds = stage.elapsed().as_secs_f64();
    let stage = Instant::now();
    let planner = Planner::new(&prepared.model, &prepared.world, &prepared.params, &prepared.profile);
    let initial = planner.construct(|_| {});
    let construction_seconds = stage.elapsed().as_secs_f64();
    let logs = RefCell::new(Vec::new());
    let anytime = RefCell::new(Vec::new());
    let cutoff = Duration::from_secs_f64(budget - reserve);
    let remaining = cutoff.saturating_sub(started.elapsed());
    let stage = Instant::now();
    // keep_going ends search while retaining its cleanup; abortable would skip cleanup.
    let improved = planner.improve_while(
        initial,
        Effort::Time(remaining),
        || started.elapsed() < cutoff,
        |line| logs.borrow_mut().push(line.to_owned()),
        |score, play, route| {
            anytime
                .borrow_mut()
                .push(json!({"seconds": started.elapsed().as_secs_f64(),
            "score": score, "player_seconds": play, "stops": route.len(), "stage": "search"}));
        },
    );
    let search_cleanup_seconds = stage.elapsed().as_secs_f64();
    let stage = Instant::now();
    let final_route = job::finalize_route(&planner, improved)?;
    let finalize_seconds = stage.elapsed().as_secs_f64();
    let stage = Instant::now();
    let mut trace = Vec::new();
    let (score, breakdown) = planner
        .simulate_full(&final_route, Some(&mut trace))
        .context("invalid exact final replay")?;
    let replay_seconds = stage.elapsed().as_secs_f64();
    let stage = Instant::now();
    planner.validate_required(&final_route)?;
    let missing_class = planner.missing_class_quests(&final_route);
    let missing_zone = planner.missing_zone_quests(&final_route);
    let late_timed = planner.late_timed_quests(&final_route);
    let missing_class_ids = warning_ids(&planner, &missing_class, false)?;
    let missing_zone_ids = warning_ids(&planner, &missing_zone, false)?;
    let late_timed_ids = warning_ids(&planner, &late_timed, true)?;
    let validation_seconds = stage.elapsed().as_secs_f64();
    let stage = Instant::now();
    let guide = fg_route::export::build_traced(
        &planner,
        &prepared.profile,
        &final_route,
        &trace,
        breakdown.total,
        breakdown,
    );
    let export_seconds = stage.elapsed().as_secs_f64();
    let compute_seconds = started.elapsed().as_secs_f64();
    anytime
        .borrow_mut()
        .push(json!({"seconds": compute_seconds, "score": score,
        "player_seconds": breakdown.total, "stops": final_route.len(), "stage": "authoritative_final"}));
    let serialization_started = Instant::now();
    let mut complete_breakdown = serde_json::to_value(breakdown)?;
    complete_breakdown["flight_gold"] = json!(breakdown.flight_gold);
    complete_breakdown["bag_overflow"] = json!(breakdown.bag_overflow);
    complete_breakdown["unlock_delay"] = json!(breakdown.unlock_delay);
    let turnins: Vec<_> = trace
        .iter()
        .filter_map(|t| match &t.event {
            Event::Stop { stop, .. } if stop.kind == Kind::TurnIn => Some(json!({
            "quest_id": prepared.model.quests[stop.index as usize].id, "time": t.time, "level": t.level})),
            _ => None,
        })
        .collect();
    let buys = trace
        .iter()
        .filter(|t| matches!(&t.event, Event::Stop { stop, .. } if stop.kind == Kind::Buy))
        .count();
    let zones: BTreeSet<_> = guide.steps.iter().filter_map(|s| s.zone_id).collect();
    let result = json!({"schema": 1, "experiment": "single_start_search_kernel_not_production_tournament",
        "edition": edition, "seed": seed, "arm": variant, "budget_seconds": budget,
        "reserve_seconds": reserve, "compute_seconds": compute_seconds,
        "budget_violation": compute_seconds > budget + 0.25, "valid": score.is_finite(),
        "score": score, "player_seconds": breakdown.total, "hidden_penalty_seconds": score - breakdown.total,
        "breakdown": complete_breakdown, "required_validation": "passed",
        "required_quest_count": prepared.profile.required_quests.len(),
        "warnings": {"notes": prepared.notes, "missing_class": missing_class, "missing_zone": missing_zone, "late_timed": late_timed},
        "warning_quest_ids": {"missing_class": missing_class_ids, "missing_zone": missing_zone_ids, "late_timed": late_timed_ids},
        "completed_turnins": turnins, "buy_count": buys, "zones": zones,
        "copper_end": breakdown.gold_start + breakdown.gold_earned - breakdown.flight_cost - breakdown.training_cost - breakdown.vendor_cost,
        "stops": final_route.iter().map(|&s| stop_value(s, &planner)).collect::<Vec<_>>(),
        "trace": trace.iter().map(|t| json!({"time": t.time, "level": t.level, "xp": t.xp, "event": format!("{:?}", t.event)})).collect::<Vec<_>>(),
        "guide": guide, "effective_request": request, "effective_params": prepared.params,
        "anytime": anytime.into_inner(), "search_log": logs.into_inner(),
        "stages_seconds": {"data": data_seconds, "prepare": prepare_seconds, "construction": construction_seconds,
            "search_including_cleanup": search_cleanup_seconds, "finalize": finalize_seconds,
            "exact_replay": replay_seconds, "validate": validation_seconds, "export": export_seconds}});
    let bytes = serde_json::to_vec(&result)?;
    std::fs::write(&output, bytes)?;
    // Sidecar includes artifact JSON conversion, encoding and disk write; no recursive timing field.
    std::fs::write(
        output.with_extension("timing.json"),
        serde_json::to_vec(&json!({
        "serialization_write_seconds": serialization_started.elapsed().as_secs_f64(),
        "process_elapsed_seconds": started.elapsed().as_secs_f64()}))?,
    )?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::warning_quest_id;

    #[test]
    fn quest_warning_identity_uses_full_label_and_ignores_reason() {
        let labels = || {
            vec![
                (101, "A (strange: name) (101, level 12): ".to_owned()),
                (102, "A (strange: name) (102, level 12): ".to_owned()),
            ]
            .into_iter()
        };
        for why in ["prerequisite 102 is missing", "only after the target level"] {
            let warning = format!("A (strange: name) (101, level 12): {why}");
            assert_eq!(warning_quest_id(&warning, labels()).expect("known quest"), 101);
        }
    }

    #[test]
    fn timed_warning_identity_matches_complete_timed_label() {
        assert_eq!(
            warning_quest_id(
                "A (42): item 999 gone after 5 minutes",
                [(42, "A (42): ".to_owned())].into_iter()
            )
            .expect("known timed quest"),
            42
        );
    }

    #[test]
    fn unknown_and_ambiguous_warning_labels_fail_closed() {
        assert!(warning_quest_id("unknown (99): failure", [(42, "A (42): ".to_owned())].into_iter()).is_err());
        assert!(
            warning_quest_id(
                "A (42): B (43): failure",
                [(42, "A (42): ".to_owned()), (43, "A (42): B (43): ".to_owned())].into_iter()
            )
            .is_err()
        );
    }
}
