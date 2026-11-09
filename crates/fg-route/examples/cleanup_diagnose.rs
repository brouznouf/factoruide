//! Fixed-attempt regeneration, fixed-snapshot rescoring, or cleanup-only replay; no timing claims.
use anyhow::{Context, Result, ensure};
use fg_route::job::{self, Overrides, PlanRequest, Prepared};
use fg_route::model::Objective;
use fg_route::plan::{Breakdown, Effort, Event, Kind, Planner, Stop, Timed};
use fg_route::xp::Edition;
use rusqlite::{Connection, OpenFlags};
use serde_json::{Value, json};
use std::cell::RefCell;
use std::collections::{BTreeMap, BTreeSet};
use std::io::Write;
use std::path::PathBuf;

fn recorded_counts(logs: &Value) -> Result<(u64, u64)> {
    let lines = logs.as_array().context("search_log must be an array")?;
    let completed: Vec<_> = lines
        .iter()
        .filter_map(Value::as_str)
        .filter(|s| s.starts_with("improved:"))
        .collect();
    ensure!(completed.len() == 1, "expected exactly one completed cleanup log");
    let line = completed[0];
    ensure!(
        line.starts_with("improved: ") && line.contains(" stops, "),
        "malformed cleanup log"
    );
    let (_, counts) = line.rsplit_once(" (").context("cleanup log has no counts")?;
    let counts = counts
        .strip_suffix(" improvements)")
        .context("incomplete cleanup counts")?;
    let (tries, wins) = counts.split_once(" tries, ").context("malformed cleanup counts")?;
    let (tries, wins): (u64, u64) = (tries.parse()?, wins.parse()?);
    ensure!(wins <= tries, "more improvements than attempts");
    Ok((tries, wins))
}

fn validate_replay_contract(artifact: &Value) -> Result<()> {
    ensure!(artifact["schema"] == 1, "unsupported artifact schema");
    ensure!(
        artifact["experiment"] == "single_start_search_kernel_not_production_tournament",
        "only archived direct single-start, unshared comparison runs are supported"
    );
    let request = &artifact["effective_request"];
    ensure!(
        request["params"].is_object() && artifact["effective_params"].is_object(),
        "missing effective parameters"
    );
    for params in [&request["params"], &artifact["effective_params"]] {
        ensure!(
            params["seed"].as_u64().is_some() && params["seed"] == artifact["seed"],
            "seed identity mismatch"
        );
        ensure!(
            params["edition"].as_str().is_some() && params["edition"] == artifact["edition"],
            "edition identity mismatch"
        );
        ensure!(
            params["anneal"].as_f64() == Some(0.0),
            "time-dependent annealing is unsupported"
        );
        ensure!(
            params["lahc"].as_u64() == Some(0),
            "this reproduction contract requires lahc=0"
        );
        ensure!(
            params["construction"] == "greedy" && params["construction_random"] == false,
            "only deterministic greedy construction is supported"
        );
        ensure!(
            params["group_size"].as_u64() == Some(1),
            "only solo archived runs are supported"
        );
    }
    ensure!(
        request["mates"].as_array().is_some_and(Vec::is_empty),
        "only solo archived runs are supported"
    );
    // share_every is inert in the archived harness: it never attaches a sharing member.
    // The experiment identity above is necessary; production tournament logs are not accepted.
    Ok(())
}

fn parse_stop(value: &Value) -> Result<Stop> {
    let index = u32::try_from(value["index"].as_u64().context("stop index must be unsigned")?)?;
    let text = value["kind"].as_str().context("missing stop kind")?;
    let kind = match text {
        "Accept" => Kind::Accept,
        "TurnIn" => Kind::TurnIn,
        "LearnFlight" => Kind::LearnFlight,
        "Bind" => Kind::Bind,
        "Buy" => Kind::Buy,
        "Hearth" => Kind::Hearth,
        "Dungeon" => Kind::Dungeon,
        "Train" => Kind::Train,
        _ => {
            let ordinal = text
                .strip_prefix("Objective(")
                .and_then(|s| s.strip_suffix(')'))
                .context("unknown stop kind")?;
            Kind::Objective(ordinal.parse()?)
        }
    };
    Ok(Stop { index, kind })
}

fn same_callbacks(expected: &[Value], actual: &[Value]) -> Result<()> {
    ensure!(
        expected.len() == actual.len(),
        "callback count: expected {}, got {}",
        expected.len(),
        actual.len()
    );
    ensure!(!expected.is_empty(), "no search callbacks");
    for (i, (left, right)) in expected.iter().zip(actual).enumerate() {
        for field in ["score", "player_seconds"] {
            let want = left[field]
                .as_f64()
                .filter(|x| x.is_finite())
                .context("nonfinite/missing archived callback number")?;
            let got = right[field]
                .as_f64()
                .filter(|x| x.is_finite())
                .context("nonfinite/missing reproduced callback number")?;
            ensure!(
                want.to_bits() == got.to_bits(),
                "callback {i} {field}: expected {want:?}, got {got:?}"
            );
        }
        ensure!(
            left["stops"].as_u64().is_some() && left["stops"] == right["stops"],
            "callback {i} stop count mismatch"
        );
        ensure!(
            left["stage"] == "search" && right["stage"] == "search",
            "callback {i} stage mismatch"
        );
    }
    Ok(())
}

/// Apply the artifact's existing JSON boundary equally to newly measured numeric values.
/// Equality remains exact; there is no floating-point tolerance.
fn artifact_boundary(value: &Value) -> Result<Value> {
    Ok(serde_json::from_slice(&serde_json::to_vec(value)?)?)
}

fn stop_value(stop: Stop, planner: &Planner<'_>) -> Value {
    json!({"index": stop.index, "kind": format!("{:?}", stop.kind),
        "quest_id": stop.is_quest().then(|| planner.model.quests[stop.index as usize].id)})
}

fn quest_ids(route: &[Stop], planner: &Planner<'_>) -> BTreeSet<i64> {
    route
        .iter()
        .filter(|s| s.is_quest())
        .map(|s| planner.model.quests[s.index as usize].id)
        .collect()
}

fn score_components(score: f64, b: &Breakdown, prepared: &Prepared) -> Value {
    let p = &prepared.params;
    let mut components = json!({
        "player_seconds": b.total,
        "grinding": (p.grind_weight() - 1.0) * b.grinding,
        "excess_grinding": (p.grind_over_weight() - p.grind_weight()) * b.grind_over,
        "farming": (p.grind_weight - 1.0).max(0.0) * b.farming,
        "flights": (p.flight_weight - 1.0) * b.flights,
        "flight_gold": b.flight_gold, "bag_overflow": b.bag_overflow,
        "zone_changes": p.zone_change_weight * f64::from(b.zone_changes),
        "outside_preferred": p.preferred_zone_weight * b.outside_preferred,
        "abandoned": p.abandon_weight * f64::from(b.abandoned),
        "unlock_delay": p.unlock_delay_weight * b.unlock_delay,
        "group_desync": p.group_sync_weight * b.desync,
        "missing_class_zone_dungeon": p.mandatory_penalty
            * (f64::from(b.missing_class_quests) + f64::from(b.missing_zone_quests) + f64::from(b.missing_dungeons)),
        "missing_profession": p.profession_quest_weight * f64::from(b.missing_profession_quests),
        "reputation_points": p.reputation_weight * b.reputation_missing
    });
    let sum: f64 = components
        .as_object()
        .expect("component object")
        .values()
        .filter_map(Value::as_f64)
        .sum();
    // Includes unmet reputation-goal count penalties, when applicable, and summation roundoff.
    components["remaining_goal_penalties_and_roundoff"] = json!(score - sum);
    components
}

fn numeric_delta(before: &Value, after: &Value) -> Value {
    let mut out = serde_json::Map::new();
    if let Some(fields) = after.as_object() {
        for (key, value) in fields {
            if let (Some(a), Some(b)) = (before[key].as_f64(), value.as_f64()) {
                out.insert(key.clone(), json!(b - a));
            }
        }
    }
    Value::Object(out)
}

fn actual_dungeon_entries(trace: &[Timed], planner: &Planner<'_>) -> Vec<Value> {
    let mut seen = BTreeSet::new();
    trace
        .iter()
        .filter_map(|t| {
            let (dungeon, observation_kind) = match &t.event {
                Event::Enter { dungeon } => (*dungeon, "implicit_entry"),
                Event::Stop { stop, .. } if stop.kind == Kind::Dungeon => {
                    (stop.index as usize, "explicit_dungeon_stop")
                }
                _ => return None,
            };
            if !seen.insert(dungeon) {
                return None;
            }
            // An explicit dungeon stop is recorded after its run; this timestamp is
            // the first observed trace event for this dungeon, not an entry/start time.
            Some(
                json!({"dungeon_index": dungeon, "dungeon_id": planner.model.dungeons[dungeon].area,
                "time": t.time, "time_semantics": "observed_trace_event_time_not_run_start",
                "level": t.level, "observation_kind": observation_kind}),
            )
        })
        .collect()
}

/// All calls to this function happen after search and finalization have completed.
fn describe_stage(stage: &str, route: &[Stop], previous: Option<&Value>, prepared: &Prepared) -> Result<Value> {
    let planner = Planner::new(&prepared.model, &prepared.world, &prepared.params, &prepared.profile);
    let mut trace = Vec::new();
    let (score, breakdown) = planner
        .simulate_full(route, Some(&mut trace))
        .with_context(|| format!("invalid {stage} snapshot"))?;
    ensure!(
        score.is_finite() && breakdown.total.is_finite(),
        "nonfinite snapshot score"
    );
    let ids = quest_ids(route, &planner);
    let listed: BTreeSet<_> = route
        .iter()
        .filter(|s| s.kind == Kind::Dungeon)
        .map(|s| planner.model.dungeons[s.index as usize].area)
        .collect();
    let entered = actual_dungeon_entries(&trace, &planner);
    let turnins: Vec<_> = trace
        .iter()
        .filter_map(|t| match &t.event {
            Event::Stop { stop, .. } if stop.kind == Kind::TurnIn => Some(json!({
            "quest_id": planner.model.quests[stop.index as usize].id, "time": t.time, "level": t.level})),
            _ => None,
        })
        .collect();
    let abandoned: Vec<_> = trace
        .iter()
        .filter_map(|t| match &t.event {
            Event::Abandon { quest } => Some(json!({"quest_id": planner.model.quests[*quest].id,
            "time": t.time, "level": t.level})),
            _ => None,
        })
        .collect();
    let before_ids: BTreeSet<_> = previous
        .and_then(|p| p["quest_ids"].as_array())
        .into_iter()
        .flatten()
        .filter_map(Value::as_i64)
        .collect();
    let removed: Vec<_> = before_ids
        .difference(&ids)
        .map(|id| {
            let quest = &prepared.model.quests[prepared.model.index[id]];
            let dungeons: BTreeSet<_> = quest
                .objectives
                .iter()
                .filter_map(Objective::dungeon_of)
                .map(|d| prepared.model.dungeons[d].area)
                .collect();
            let before = previous.expect("removed quest has preceding stage");
            let was_turned = before["actual_turnins"]
                .as_array()
                .is_some_and(|a| a.iter().any(|v| v["quest_id"] == *id));
            let missing: Vec<_> = dungeons
                .iter()
                .filter(|id| {
                    !before["listed_dungeon_ids"]
                        .as_array()
                        .is_some_and(|a| a.iter().any(|v| v.as_i64() == Some(**id)))
                })
                .collect();
            json!({"quest_id": id, "name": quest.name, "required_dungeon_ids": dungeons,
            "missing_listed_dungeon_ids_before": missing, "actually_turned_in_before": was_turned})
        })
        .collect();
    let mut full_breakdown = serde_json::to_value(breakdown)?;
    full_breakdown["flight_gold"] = json!(breakdown.flight_gold);
    full_breakdown["bag_overflow"] = json!(breakdown.bag_overflow);
    full_breakdown["unlock_delay"] = json!(breakdown.unlock_delay);
    let mut value = json!({"stage": stage, "score": score, "player_seconds": breakdown.total,
        "hidden_penalty_seconds": score - breakdown.total, "stop_count": route.len(),
        "stops": route.iter().map(|&s| stop_value(s, &planner)).collect::<Vec<_>>(),
        "quest_ids": ids, "listed_dungeon_ids": listed, "actual_dungeon_entries": entered,
        "actual_turnins": turnins, "actual_abandonments": abandoned,
        "removed_quests": removed, "breakdown": full_breakdown,
        "score_components": score_components(score, &breakdown, prepared),
        "required_validation": planner.validate_required(route).map_or_else(|e| e.to_string(), |()| "passed".to_owned()),
        "trace": trace.iter().map(|t| json!({"time": t.time, "level": t.level, "xp": t.xp,
            "power": t.power, "gear": t.gear, "party": t.party, "event": format!("{:?}", t.event)})).collect::<Vec<_>>()});
    if let Some(before) = previous {
        value["delta"] = json!({"score": score - before["score"].as_f64().context("previous score")?,
            "player_seconds": breakdown.total - before["player_seconds"].as_f64().context("previous play")?,
            "breakdown": numeric_delta(&before["breakdown"], &value["breakdown"]),
            "score_components": numeric_delta(&before["score_components"], &value["score_components"])});
    }
    Ok(value)
}

fn check_result(result: Result<()>) -> Value {
    match result {
        Ok(()) => json!({"equal": true}),
        Err(e) => json!({"equal": false, "detail": e.to_string()}),
    }
}

fn snapshot_routes(artifact: &Value, diagnosis: &Value) -> Result<Vec<(String, Vec<Stop>)>> {
    ensure!(diagnosis["schema"] == 1, "unsupported snapshot schema");
    ensure!(
        diagnosis["experiment"] == "cleanup_loss_fixed_attempt_diagnosis",
        "snapshot input must be an original fixed-attempt diagnosis"
    );
    for field in ["seed", "edition", "effective_request", "effective_params"] {
        ensure!(diagnosis[field] == artifact[field], "snapshot source {field} mismatch");
    }
    let stages = diagnosis["stages"].as_array().context("missing snapshot stages")?;
    let expected = [
        "SelectedInput",
        "PowerFirst",
        "Rebind",
        "PlaceHearths",
        "DropUnfinished",
        "TrimTarget",
        "DropDungeonless",
        "Finalized",
    ];
    ensure!(
        stages.len() == expected.len(),
        "snapshot input must contain all eight stage boundaries"
    );
    stages
        .iter()
        .zip(expected)
        .map(|(stage, name)| {
            ensure!(stage["stage"] == name, "expected snapshot boundary {name}");
            let stops = stage["stops"]
                .as_array()
                .with_context(|| format!("missing {name} stops"))?;
            let route = stops.iter().map(parse_stop).collect::<Result<_>>()?;
            Ok((name.to_owned(), route))
        })
        .collect()
}

fn write_report(output: &std::path::Path, report: &Value) -> Result<()> {
    if let Some(parent) = output.parent().filter(|p| !p.as_os_str().is_empty()) {
        std::fs::create_dir_all(parent)?;
    }
    let mut file = std::fs::OpenOptions::new().write(true).create_new(true).open(output)?;
    file.write_all(&serde_json::to_vec_pretty(report)?)?;
    Ok(())
}

fn validate_modes(opts: &BTreeMap<String, String>) -> Result<()> {
    ensure!(
        !opts.contains_key("--rescore-snapshots") || !opts.contains_key("--tries"),
        "--tries cannot be combined with --rescore-snapshots: rescoring never searches"
    );
    ensure!(
        !opts.contains_key("--cleanup-snapshots")
            || (!opts.contains_key("--tries") && !opts.contains_key("--rescore-snapshots")),
        "--cleanup-snapshots cannot be combined with --tries or --rescore-snapshots: cleanup never searches"
    );
    Ok(())
}

struct CleanupReplay {
    snapshots: Vec<(String, Vec<Stop>)>,
    callbacks: Vec<Value>,
    logs: Value,
}

fn cleanup_selected_input(planner: &Planner<'_>, initial: &[Stop]) -> Result<CleanupReplay> {
    let callbacks = RefCell::new(Vec::new());
    let callback_routes = RefCell::new(Vec::new());
    let logs = RefCell::new(Vec::new());
    let snapshots = RefCell::new(Vec::new());
    let improved = planner.improve_while_observed(
        initial.to_owned(),
        Effort::Tries(0),
        || true,
        |line| logs.borrow_mut().push(line.to_owned()),
        |score, play, route| {
            callbacks.borrow_mut().push(json!({"score": score,
                "player_seconds": play, "stops": route.len(), "stage": "cleanup_initial"}));
            callback_routes.borrow_mut().push(route.to_vec());
        },
        |stage, route| snapshots.borrow_mut().push((format!("{stage:?}"), route.to_vec())),
    );
    let callbacks = callbacks.into_inner();
    let callback_routes = callback_routes.into_inner();
    let logs = json!(logs.into_inner());
    let mut snapshots = snapshots.into_inner();
    ensure!(
        callbacks.len() == 1,
        "cleanup-only replay must emit exactly one initial best callback"
    );
    ensure!(
        callback_routes[0] == initial,
        "initial best callback changed the typed SelectedInput"
    );
    ensure!(
        recorded_counts(&logs)? == (0, 0),
        "cleanup-only replay performed search attempts or improvements"
    );
    let expected = [
        "SelectedInput",
        "PowerFirst",
        "Rebind",
        "PlaceHearths",
        "DropUnfinished",
        "TrimTarget",
        "DropDungeonless",
    ];
    ensure!(
        snapshots.iter().map(|(stage, _)| stage.as_str()).eq(expected),
        "cleanup-only replay must emit all seven ordered cleanup boundaries"
    );
    ensure!(
        snapshots[0].1 == initial,
        "cleanup changed the typed SelectedInput before its first boundary"
    );
    let final_route = job::finalize_route(planner, improved)?;
    snapshots.push(("Finalized".to_owned(), final_route));
    Ok(CleanupReplay {
        snapshots,
        callbacks,
        logs,
    })
}

fn main() -> Result<()> {
    let mut args = std::env::args().skip(1);
    let mut opts = BTreeMap::new();
    while let Some(key) = args.next() {
        ensure!(
            [
                "--artifact",
                "--output",
                "--repo",
                "--tries",
                "--rescore-snapshots",
                "--cleanup-snapshots"
            ]
            .contains(&key.as_str()),
            "unknown option {key}"
        );
        let value = args.next().with_context(|| format!("missing value for {key}"))?;
        ensure!(opts.insert(key, value).is_none(), "duplicate option");
    }
    let artifact_path = PathBuf::from(opts.get("--artifact").context("missing --artifact")?);
    let output = PathBuf::from(opts.get("--output").context("missing --output")?);
    ensure!(
        !output.exists(),
        "output already exists; archived and previous artifacts are never overwritten"
    );
    let repo = opts.get("--repo").map_or_else(
        || PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../.."),
        PathBuf::from,
    );
    let artifact: Value = serde_json::from_slice(&std::fs::read(&artifact_path)?)?;
    validate_replay_contract(&artifact)?;
    validate_modes(&opts)?;
    let (recorded_tries, recorded_wins) = recorded_counts(&artifact["search_log"])?;
    let tries = opts
        .get("--tries")
        .map(|v| v.parse::<u64>())
        .transpose()?
        .unwrap_or(recorded_tries);
    let request: PlanRequest = serde_json::from_value(artifact["effective_request"].clone())?;
    let request_equal = artifact_boundary(&serde_json::to_value(&request)?)? == artifact["effective_request"];
    let edition = Edition::from_key(artifact["edition"].as_str().context("edition")?).context("unsupported edition")?;
    let overrides = Overrides::load_edition(&repo.join("overrides"), edition)?;
    let conn = Connection::open_with_flags(
        repo.join("data").join(edition.db_file()),
        OpenFlags::SQLITE_OPEN_READ_ONLY,
    )?;
    let prepared = job::prepare(&conn, &overrides, &request, &|_| {})?;
    let params_equal = artifact_boundary(&serde_json::to_value(&prepared.params)?)? == artifact["effective_params"];
    if let Some(source) = opts.get("--rescore-snapshots") {
        ensure!(
            request_equal && params_equal,
            "prepared rescore configuration differs from source artifact"
        );
        let diagnosis: Value = serde_json::from_slice(&std::fs::read(source)?)?;
        let routes = snapshot_routes(&artifact, &diagnosis)?;
        let mut stages = Vec::new();
        for (i, (stage, route)) in routes.into_iter().enumerate() {
            let mut value = describe_stage(&stage, &route, stages.last(), &prepared)?;
            let old = &diagnosis["stages"][i];
            ensure!(
                value["stops"] == old["stops"],
                "snapshot {stage} quest/stop identity changed in the prepared model"
            );
            value["source_score"] = old["score"].clone();
            value["source_player_seconds"] = old["player_seconds"].clone();
            value["model_rescore_delta"] = json!({
                "score": value["score"].as_f64().context("new snapshot score")?
                    - old["score"].as_f64().context("source snapshot score")?,
                "player_seconds": value["player_seconds"].as_f64().context("new snapshot play")?
                    - old["player_seconds"].as_f64().context("source snapshot play")?,
                "breakdown": numeric_delta(&old["breakdown"], &value["breakdown"]),
                "score_components": numeric_delta(&old["score_components"], &value["score_components"])
            });
            stages.push(value);
        }
        let report = json!({"schema": 1, "experiment": "cleanup_loss_snapshot_rescore",
            "mode": "rescore_snapshots", "archived_artifact": artifact_path,
            "source_diagnosis": source, "repo": repo, "edition": edition, "seed": artifact["seed"],
            "arm": artifact["arm"], "search_performed": false, "callback_comparison_performed": false,
            "configuration_identity_verified": true, "fixed_snapshot_route_identity_verified": true,
            "provenance_requirement": "Verify source/data/overrides and identify the accounting-model change externally; rescoring makes no search-reproduction claim.",
            "timing_claims": false, "effective_request": request, "effective_params": prepared.params,
            "stages": stages});
        write_report(&output, &report)?;
        println!(
            "{}: rescored eight fixed snapshots without search or callback comparison",
            output.display()
        );
        return Ok(());
    }
    if let Some(source) = opts.get("--cleanup-snapshots") {
        ensure!(
            request_equal && params_equal,
            "prepared cleanup configuration differs from source artifact"
        );
        let diagnosis: Value = serde_json::from_slice(&std::fs::read(source)?)?;
        let routes = snapshot_routes(&artifact, &diagnosis)?;
        let initial = routes.into_iter().next().context("missing SelectedInput")?.1;
        let planner = Planner::new(&prepared.model, &prepared.world, &prepared.params, &prepared.profile);
        let replay = cleanup_selected_input(&planner, &initial)?;
        // Describe snapshots only after ordinary cleanup and finalization have completed.
        let mut stages = Vec::new();
        for (stage, route) in replay.snapshots {
            stages.push(describe_stage(&stage, &route, stages.last(), &prepared)?);
        }
        ensure!(
            stages[0]["stops"] == diagnosis["stages"][0]["stops"],
            "SelectedInput quest/stop identity changed in the prepared model"
        );
        let report = json!({"schema": 1, "experiment": "cleanup_training_cleanup_only_replay",
            "mode": "cleanup_snapshots", "archived_artifact": artifact_path,
            "source_diagnosis": source, "repo": repo, "edition": edition, "seed": artifact["seed"],
            "arm": artifact["arm"], "search_performed": false, "search_attempts": 0,
            "search_improvements": 0, "callback_comparison_performed": false,
            "configuration_identity_verified": true, "selected_input_typed_identity_verified": true,
            "selected_input_stop_identity_records_verified": true,
            "initial_best_callback_count": replay.callbacks.len(), "cleanup_boundary_count": stages.len() - 1,
            "initial_best_callbacks": replay.callbacks, "cleanup_log": replay.logs,
            "provenance_requirement": "Verify source/data/overrides externally and compare archived boundaries through TrimTarget before attributing later cleanup differences; this mode makes no search-reproduction claim.",
            "timing_claims": false, "effective_request": request, "effective_params": prepared.params,
            "stages": stages});
        write_report(&output, &report)?;
        println!(
            "{}: replayed cleanup from fixed SelectedInput with zero search attempts",
            output.display()
        );
        return Ok(());
    }
    let planner = Planner::new(&prepared.model, &prepared.world, &prepared.params, &prepared.profile);
    let initial = planner.construct(|_| {});
    let callbacks = RefCell::new(Vec::new());
    let logs = RefCell::new(Vec::new());
    let snapshots = RefCell::new(Vec::new());
    let improved = planner.improve_while_observed(
        initial,
        Effort::Tries(tries),
        || true,
        |line| logs.borrow_mut().push(line.to_owned()),
        |score, play, route| {
            callbacks.borrow_mut().push(json!({"score": score,
            "player_seconds": play, "stops": route.len(), "stage": "search"}));
        },
        |stage, route| snapshots.borrow_mut().push((format!("{stage:?}"), route.to_vec())),
    );
    let final_route = job::finalize_route(&planner, improved)?;
    snapshots
        .borrow_mut()
        .push(("Finalized".to_owned(), final_route.clone()));
    // No snapshot replay is allowed before the normal transformations above finish.
    let mut stages = Vec::new();
    for (stage, route) in snapshots.into_inner() {
        let description = describe_stage(&stage, &route, stages.last(), &prepared)?;
        stages.push(description);
    }
    let actual_callbacks: Vec<Value> = serde_json::from_value(artifact_boundary(&json!(callbacks.into_inner()))?)?;
    let expected_callbacks: Vec<_> = artifact["anytime"]
        .as_array()
        .context("missing archived callbacks")?
        .iter()
        .filter(|v| v["stage"] == "search")
        .cloned()
        .collect();
    let actual_logs = json!(logs.into_inner());
    let (actual_tries, actual_wins) = recorded_counts(&actual_logs)?;
    let expected_stops: Vec<_> = artifact["stops"]
        .as_array()
        .context("missing archived stops")?
        .iter()
        .map(parse_stop)
        .collect::<Result<_>>()?;
    let final_stage = stages.last().context("no final snapshot")?;
    let mut gates = json!({
        "request": {"equal": request_equal}, "effective_params": {"equal": params_equal},
        "attempt_count": {"equal": actual_tries == recorded_tries && tries == recorded_tries,
            "expected": recorded_tries, "reproduced": actual_tries},
        "improvement_count": {"equal": actual_wins == recorded_wins, "expected": recorded_wins, "reproduced": actual_wins},
        "initial_callback": check_result(same_callbacks(&expected_callbacks[..expected_callbacks.len().min(1)],
            &actual_callbacks[..actual_callbacks.len().min(1)])),
        "complete_callbacks": check_result(same_callbacks(&expected_callbacks, &actual_callbacks)),
        "final_typed_stops": {"equal": expected_stops == final_route},
        "final_stop_identity_records": {"equal": final_stage["stops"] == artifact["stops"]}
    });
    for field in ["score", "player_seconds"] {
        let got = artifact_boundary(&final_stage[field])?;
        gates[format!("final_{field}")] = json!({"equal": artifact[field].as_f64().is_some_and(f64::is_finite)
            && got == artifact[field], "expected": artifact[field], "reproduced": got});
    }
    let equal = gates
        .as_object()
        .context("gate object")?
        .values()
        .all(|g| g["equal"] == true);
    let report = json!({"schema": 1, "experiment": "cleanup_loss_fixed_attempt_diagnosis",
        "archived_artifact": artifact_path, "repo": repo, "edition": edition, "seed": artifact["seed"],
        "arm": artifact["arm"], "tries_override": opts.get("--tries"),
        "recorded_observables_equal": equal,
        "replay_classification": if equal { "recorded_observables_match_pending_provenance" } else { "diagnostic_reproduction" },
        "provenance_requirement": "Verify archived source, Cargo.lock, data and overrides externally before claiming exact replay.",
        "timing_claims": false, "gates": gates,
        "expected_search_callbacks": expected_callbacks, "reproduced_search_callbacks": actual_callbacks,
        "expected_search_log": artifact["search_log"], "reproduced_search_log": actual_logs,
        "effective_request": request, "effective_params": prepared.params, "stages": stages});
    write_report(&output, &report)?;
    println!(
        "{}: recorded observables equal={equal}; provenance must be verified separately",
        output.display()
    );
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use fg_route::testing::{At, Obj, Q, Scenario};

    fn dungeon_scenario() -> Prepared {
        let mut prepared = Scenario::new()
            .levels(18, 20)
            .dungeon("Implicit run", 1, 55.0, 50.0)
            .dungeon("Explicit run", 1, 60.0, 50.0)
            .quest(
                Q::new(100, "Inside the first dungeon").level(18).objective(
                    Obj::visit(
                        "Visit inside",
                        At {
                            zone: 1,
                            x: 55.0,
                            y: 50.0,
                        },
                    )
                    .inside(0),
                ),
            )
            .quest(Q::new(200, "Finish after the runs").level(18).xp(100_000))
            .params(|p| {
                p.farm_on_way = false;
                p.explore_radius = 0.0;
            })
            .build();
        // Keep every dungeon event before the final outdoor reward, without relying on
        // dungeon XP or on fallback camps beyond this deliberately small fixture.
        for dungeon in &mut prepared.model.dungeons {
            (dungeon.min_level, dungeon.max_level, dungeon.mob_level) = (18, 20, 18);
            (dungeon.kills, dungeon.minutes) = (0.0, 0.0);
        }
        prepared
    }

    #[test]
    fn actual_dungeons_include_an_explicit_only_run_without_enter_event() {
        let prepared = dungeon_scenario();
        let planner = Planner::new(&prepared.model, &prepared.world, &prepared.params, &prepared.profile);
        let mut trace = Vec::new();
        planner
            .simulate_full(
                &[
                    Stop {
                        index: 1,
                        kind: Kind::Dungeon,
                    },
                    Stop::quest(1, Kind::Accept),
                    Stop::quest(1, Kind::TurnIn),
                ],
                Some(&mut trace),
            )
            .unwrap();
        assert!(!trace.iter().any(|t| matches!(t.event, Event::Enter { .. })));
        assert!(
            trace
                .iter()
                .any(|t| matches!(t.event, Event::Stop { stop, .. } if stop.kind == Kind::Dungeon && stop.index == 1))
        );
        let entries = actual_dungeon_entries(&trace, &planner);
        assert_eq!(entries.len(), 1);
        assert_eq!(entries[0]["dungeon_index"], 1);
        assert_eq!(entries[0]["dungeon_id"], 901);
        assert_eq!(entries[0]["observation_kind"], "explicit_dungeon_stop");
    }

    #[test]
    fn actual_dungeons_keep_first_occurrence_when_implicit_entry_precedes_explicit_stop() {
        let prepared = dungeon_scenario();
        let planner = Planner::new(&prepared.model, &prepared.world, &prepared.params, &prepared.profile);
        let mut trace = Vec::new();
        let route = [
            Stop::quest(0, Kind::Accept),
            Stop::quest(0, Kind::Objective(0)),
            Stop {
                index: 0,
                kind: Kind::Dungeon,
            },
            Stop {
                index: 1,
                kind: Kind::Dungeon,
            },
            Stop::quest(1, Kind::Accept),
            Stop::quest(1, Kind::TurnIn),
        ];
        planner.simulate_full(&route, Some(&mut trace)).unwrap();
        let implicit = trace
            .iter()
            .position(|t| matches!(t.event, Event::Enter { dungeon: 0 }))
            .unwrap();
        let explicit = trace
            .iter()
            .position(|t| matches!(t.event, Event::Stop { stop, .. } if stop.kind == Kind::Dungeon && stop.index == 0))
            .unwrap();
        assert!(implicit < explicit);
        let entries = actual_dungeon_entries(&trace, &planner);
        let identities: Vec<_> = entries
            .iter()
            .map(|v| (v["dungeon_index"].as_u64().unwrap(), v["dungeon_id"].as_i64().unwrap()))
            .collect();
        assert_eq!(identities, vec![(0, 900), (1, 901)]);
        assert_eq!(entries[0]["level"], trace[implicit].level);
        assert_eq!(entries[0]["observation_kind"], "implicit_entry");
        assert_eq!(
            entries[0]["time"].as_f64().unwrap().to_bits(),
            trace[implicit].time.to_bits()
        );
    }

    #[test]
    fn recorded_counts_require_one_complete_cleanup_log() {
        let logs = json!([
            "start: 981 stops, 69h58",
            "improved: 916 stops, 63h06 (59526 tries, 367 improvements)"
        ]);
        assert_eq!(recorded_counts(&logs).unwrap(), (59526, 367));
        assert!(recorded_counts(&json!(["improved: broken"])).is_err());
        assert!(recorded_counts(&json!([])).is_err());
        assert!(recorded_counts(&json!([logs[1], logs[1]])).is_err());
    }

    fn contract() -> Value {
        json!({"schema": 1, "experiment": "single_start_search_kernel_not_production_tournament",
            "seed": 20261107, "edition": "forever", "effective_request": {
                "mates": [], "params": {"seed": 20261107, "edition": "forever",
                "anneal": 0.0, "lahc": 0, "group_size": 1, "construction": "greedy",
                "construction_random": false}}, "effective_params": {"seed": 20261107,
                "edition": "forever", "anneal": 0.0, "lahc": 0, "group_size": 1,
                "construction": "greedy", "construction_random": false}})
    }

    #[test]
    fn replay_contract_rejects_time_acceptance_seed_drift_and_non_solo_origins() {
        assert!(validate_replay_contract(&contract()).is_ok());
        for (field, bad) in [
            ("anneal", json!(0.01)),
            ("lahc", json!(10)),
            ("group_size", json!(2)),
            ("seed", json!(42)),
            ("construction_random", json!(true)),
        ] {
            let mut artifact = contract();
            artifact["effective_request"]["params"][field] = bad.clone();
            artifact["effective_params"][field] = bad;
            assert!(validate_replay_contract(&artifact).is_err(), "accepted {field}");
        }
        let mut artifact = contract();
        artifact["experiment"] = json!("production_tournament");
        assert!(validate_replay_contract(&artifact).is_err());
        let mut artifact = contract();
        artifact["effective_request"]["mates"] = json!([{"class": "mage"}]);
        assert!(validate_replay_contract(&artifact).is_err());
        let mut artifact = contract();
        artifact["effective_params"]["anneal"] = json!(0.2);
        assert!(validate_replay_contract(&artifact).is_err());
    }

    #[test]
    fn archive_stop_decoder_preserves_objective_ordinal_and_nonquest_kind() {
        assert_eq!(
            parse_stop(&json!({"index": 42, "kind": "Objective(3)"})).unwrap(),
            Stop {
                index: 42,
                kind: Kind::Objective(3)
            }
        );
        assert_eq!(
            parse_stop(&json!({"index": 7, "kind": "Dungeon"})).unwrap(),
            Stop {
                index: 7,
                kind: Kind::Dungeon
            }
        );
        for bad in [
            json!({"index": 0, "kind": "Objective(256)"}),
            json!({"index": 0, "kind": "Unknown"}),
            json!({"index": 4294967296_u64, "kind": "Accept"}),
        ] {
            assert!(parse_stop(&bad).is_err());
        }
    }

    #[test]
    fn archive_stop_decoder_preserves_independent_training() {
        assert_eq!(
            parse_stop(&json!({"index": 7, "kind": "Train", "quest_id": null})).unwrap(),
            Stop {
                index: 7,
                kind: Kind::Train
            }
        );
    }

    #[test]
    fn cleanup_mode_excludes_search_override_and_snapshot_rescoring() {
        for keys in [
            vec!["--cleanup-snapshots", "--tries"],
            vec!["--cleanup-snapshots", "--rescore-snapshots"],
            vec!["--rescore-snapshots", "--tries"],
        ] {
            let opts = keys.iter().map(|key| ((*key).to_owned(), "0".to_owned())).collect();
            assert!(validate_modes(&opts).is_err(), "accepted {keys:?}");
        }
        for key in ["--cleanup-snapshots", "--rescore-snapshots", "--tries"] {
            assert!(validate_modes(&BTreeMap::from([(key.to_owned(), "0".to_owned())])).is_ok());
        }
        assert!(validate_modes(&BTreeMap::new()).is_ok());
    }

    #[test]
    fn cleanup_only_uses_fixed_input_with_one_callback_and_zero_attempts() {
        let prepared = Scenario::new()
            .levels(18, 20)
            .quest(Q::new(100, "First fixed visit").level(18).xp(100))
            .quest(Q::new(200, "Finish the fixed route").level(18).xp(100_000))
            .build();
        let planner = Planner::new(&prepared.model, &prepared.world, &prepared.params, &prepared.profile);
        // More than two stops makes a nonzero Effort enter the search loop, so the
        // zero-count assertion detects an accidental search attempt.
        let initial = vec![
            Stop::quest(0, Kind::Accept),
            Stop::quest(0, Kind::TurnIn),
            Stop::quest(1, Kind::Accept),
            Stop::quest(1, Kind::TurnIn),
        ];
        let replay = cleanup_selected_input(&planner, &initial).unwrap();
        assert_eq!(recorded_counts(&replay.logs).unwrap(), (0, 0));
        assert_eq!(replay.callbacks.len(), 1);
        assert_eq!(replay.callbacks[0]["stops"], 4);
        assert_eq!(replay.snapshots[0].1, initial);
        assert_eq!(
            replay
                .snapshots
                .iter()
                .map(|(stage, _)| stage.as_str())
                .collect::<Vec<_>>(),
            [
                "SelectedInput",
                "PowerFirst",
                "Rebind",
                "PlaceHearths",
                "DropUnfinished",
                "TrimTarget",
                "DropDungeonless",
                "Finalized"
            ]
        );
        assert_eq!(replay.snapshots[7].1, initial);
        assert!(planner.validate_required(&replay.snapshots[7].1).is_ok());
    }

    #[test]
    fn callback_equality_ignores_timestamps_but_detects_sequence_and_player_drift() {
        let expected = vec![json!({"seconds": 0.1, "score": 100.0, "player_seconds": 80.0,
            "stops": 9, "stage": "search"})];
        let mut actual = expected.clone();
        actual[0]["seconds"] = json!(999.0);
        assert!(same_callbacks(&expected, &actual).is_ok());
        actual[0]["player_seconds"] = json!(81.0);
        assert!(same_callbacks(&expected, &actual).is_err());
        assert!(same_callbacks(&expected, &[]).is_err());
    }

    fn snapshot_fixture() -> (Value, Value) {
        let artifact = contract();
        let stages: Vec<_> = [
            "SelectedInput",
            "PowerFirst",
            "Rebind",
            "PlaceHearths",
            "DropUnfinished",
            "TrimTarget",
            "DropDungeonless",
            "Finalized",
        ]
        .iter()
        .map(|stage| {
            json!({"stage": stage,
            "stops": [{"index": 42, "kind": "Objective(3)", "quest_id": 100}]})
        })
        .collect();
        let diagnosis = json!({"schema": 1, "experiment": "cleanup_loss_fixed_attempt_diagnosis",
            "seed": artifact["seed"], "edition": artifact["edition"],
            "effective_request": artifact["effective_request"], "effective_params": artifact["effective_params"],
            "stages": stages});
        (artifact, diagnosis)
    }

    #[test]
    fn snapshot_rescoring_rejects_configuration_drift() {
        let (artifact, diagnosis) = snapshot_fixture();
        assert_eq!(snapshot_routes(&artifact, &diagnosis).unwrap().len(), 8);
        for field in ["seed", "edition", "effective_request", "effective_params"] {
            let mut changed = diagnosis.clone();
            changed[field] = Value::Null;
            assert!(
                snapshot_routes(&artifact, &changed).is_err(),
                "accepted changed {field}"
            );
        }
    }

    #[test]
    fn snapshot_source_requires_original_schema_and_experiment() {
        let (artifact, diagnosis) = snapshot_fixture();
        for (field, value) in [
            ("schema", json!(2)),
            ("experiment", json!("cleanup_training_cleanup_only_replay")),
            ("experiment", json!("cleanup_loss_snapshot_rescore")),
        ] {
            let mut changed = diagnosis.clone();
            changed[field] = value;
            assert!(
                snapshot_routes(&artifact, &changed).is_err(),
                "accepted changed {field}"
            );
        }
    }

    #[test]
    fn snapshot_rescoring_requires_all_ordered_boundaries_and_decodes_fixed_stops() {
        let (artifact, diagnosis) = snapshot_fixture();
        let routes = snapshot_routes(&artifact, &diagnosis).unwrap();
        assert_eq!(routes[0].0, "SelectedInput");
        assert_eq!(routes[7].0, "Finalized");
        assert_eq!(
            routes[0].1,
            vec![Stop {
                index: 42,
                kind: Kind::Objective(3)
            }]
        );
        let mut reordered = diagnosis.clone();
        reordered["stages"].as_array_mut().unwrap().swap(0, 7);
        assert!(snapshot_routes(&artifact, &reordered).is_err());
        let mut incomplete = diagnosis.clone();
        incomplete["stages"].as_array_mut().unwrap().remove(3);
        assert!(snapshot_routes(&artifact, &incomplete).is_err());
        let mut malformed = diagnosis;
        malformed["stages"][0]["stops"][0]["kind"] = json!("Objective(256)");
        assert!(snapshot_routes(&artifact, &malformed).is_err());
    }
}
