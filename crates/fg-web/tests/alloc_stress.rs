//! The browser's allocator (`fg_web::alloc`) under the planner's threads, natively.
#![cfg(any(target_os = "wasi", unix))]

use fg_route::job;
use std::path::Path;

#[global_allocator]
static ALLOCATOR: fg_web::alloc::Sharded = fg_web::alloc::Sharded;

#[test]
fn many_threads_allocating_and_freeing_keep_their_blocks_apart() {
    let handles: Vec<_> = (0..16u64)
        .map(|t| {
            std::thread::spawn(move || {
                let mut kept: Vec<Vec<u64>> = Vec::new();
                for i in 0..6_000u64 {
                    let n = ((i * 7919 + t) % 300) as usize;
                    kept.push(vec![t * 1_000_000 + i; n]);
                    if kept.len() > 500 {
                        let v = kept.swap_remove((i % 500) as usize);
                        assert!(v.iter().all(|&x| x == v[0]), "a block changed under its owner");
                    }
                    if i % 1000 == 0 {
                        let mut v = kept.pop().unwrap_or_default();
                        let x = v.first().copied().unwrap_or(t);
                        v.resize(v.len() * 3 + 1, x);
                        kept.push(v);
                    }
                }
                kept.len()
            })
        })
        .collect();
    for h in handles {
        h.join().unwrap();
    }
}

#[test]
#[ignore = "a full guide: run with --ignored"]
fn a_guide_is_planned_with_it() {
    let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("../..");
    let db = root.join("data/classic.sqlite");
    let conn = rusqlite::Connection::open(&db).unwrap();
    let overrides = job::Overrides::load_edition(&root.join("overrides"), fg_route::xp::Edition::Classic).unwrap();
    let request: job::PlanRequest =
        serde_json::from_str(r#"{"race":"dwarf","class":"warrior","from_level":1,"to_level":60,"name":"t"}"#).unwrap();
    let outcome = job::plan(&conn, &overrides, &request, &|_| {}).unwrap();
    assert!(outcome.route.steps.len() > 100);
}
