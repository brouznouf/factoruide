//! `job::prepare` run again and again, to profile what a plan costs before its search
//! (`perf record`): `prepare_loop [rounds] [edition]`.

#![expect(clippy::unwrap_used, reason = "an example that cannot load its data fails")]
use fg_route::job::{self, Overrides, PlanRequest};
use fg_route::xp::Edition;
use rusqlite::Connection;
use std::path::PathBuf;

fn main() {
    let mut args = std::env::args().skip(1);
    let rounds: usize = args.next().and_then(|n| n.parse().ok()).unwrap_or(10);
    let edition = args.next().unwrap_or_else(|| "forever".into());
    let repo = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../..");
    let mut conn = Connection::open(repo.join(format!("data/{edition}.sqlite"))).unwrap();
    let overrides = Overrides::load_edition(&repo.join("overrides"), Edition::Forever).unwrap();
    let request: PlanRequest = serde_json::from_value(serde_json::json!({
        "race": "skyborne_horde", "class": "druid", "from_level": 40, "to_level": 41,
        "required_class_quests": [], "start": {"level": 40}
    }))
    .unwrap();
    // `SLOW=ms` lists the queries (and the statements they run) that take longer.
    if let Some(slow) = std::env::var("SLOW").ok().and_then(|v| v.parse::<u64>().ok()) {
        static SLOW: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);
        SLOW.store(slow, std::sync::atomic::Ordering::Relaxed);
        conn.trace_v2(
            rusqlite::trace::TraceEventCodes::SQLITE_TRACE_PROFILE,
            Some(|event: rusqlite::trace::TraceEvent<'_>| {
                if let rusqlite::trace::TraceEvent::Profile(statement, took) = event
                    && took.as_millis() >= u128::from(SLOW.load(std::sync::atomic::Ordering::Relaxed))
                {
                    let one_line: String = statement.sql().split_whitespace().collect::<Vec<_>>().join(" ");
                    eprintln!(
                        "{:>5} ms  {}",
                        took.as_millis(),
                        one_line.chars().take(110).collect::<String>()
                    );
                }
            }),
        )
        .unwrap();
    }
    for _ in 0..rounds {
        let started = std::time::Instant::now();
        job::prepare(&conn, &overrides, &request, &|_| {}).unwrap();
        eprintln!("{:.0} ms", started.elapsed().as_secs_f64() * 1000.0);
    }
}
