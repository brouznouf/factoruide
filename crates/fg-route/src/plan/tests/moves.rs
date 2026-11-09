//! Vendor buys must leave the quest-changing moves reachable through the dispatcher.

use super::{
    Edition, Kind, Overrides, Prepared, Stop, at_start, class_versions, far_and_near_versions, far_version_route,
    planner, quests_at_start, repo,
};
use crate::job;
use rand::{SeedableRng, rngs::SmallRng};

fn with_vendor_item(quests: usize) -> Prepared {
    let conn = crate::fixture::Fixture::new(Edition::Classic)
        .toml(
            r#"
[[npc]]
id = 990001
name = "Gear seller"
npc_flags = 4
friendly_to = "AH"
at = [{ zone = 12, x = 48.0, y = 42.0 }]

[[item]]
id = 990002
name = "Test cap"
level = 1
class = 4
drops = [{ vendor = 990001 }]

[[quest]]
id = 990003
name = "Template"
level = 1
min_level = 1
giver = 990001
xp = 0

[[row]]
table = "m_item_stats"
values = { id = 990002, quality = 1, inventory_type = 1, class = 4, subclass = 1, allowable_class = 0, required_level = 1, str = 1.0, agi = 0.0, sta = 0.0, int = 0.0, spi = 0.0, armor = 0.0, ap = 0.0, rap = 0.0, sp = 0.0, heal = 0.0, crit = 0.0, spell_crit = 0.0, hit = 0.0, mp5 = 0.0, dps = 0.0, speed = 0.0 }
"#,
        )
        .open();
    // The fixture base predates the prices populated by `fg gold`.
    conn.execute_batch(
        "ALTER TABLE m_item_stats ADD COLUMN buy_price INTEGER NOT NULL DEFAULT 0;
         ALTER TABLE m_item_stats ADD COLUMN sell_price INTEGER NOT NULL DEFAULT 0;
         UPDATE m_item_stats SET buy_price = 1 WHERE id = 990002;",
    )
    .unwrap();
    let overrides = Overrides::load_edition(&repo().join("overrides"), Edition::Classic).unwrap();
    let request = serde_json::from_value(serde_json::json!({
        "race": "human", "class": "warrior", "from_level": 1, "to_level": 5,
        "required_class_quests": [], "params": {"edition": "classic", "bis": "prefer"}
    }))
    .unwrap();
    let mut vendor = job::prepare(&conn, &overrides, &request, &|_| {}).unwrap();
    assert_eq!(vendor.model.bis.vendor_items().len(), 1);
    let power = vendor.model.power.take();
    let mut p = at_start(vendor, quests);
    p.model.power = power;
    p.world.terrain = crate::terrain::Terrain::default();
    p.model.initial.money = 1;
    p.params.bis = crate::params::BisMode::Prefer;
    let planner = planner(&p);
    let initial = planner.sim().initial_state();
    assert!(
        planner.availability().can_buy(&initial, p.model.bis.vendor_items()[0]),
        "the fixture item is an affordable, wearable upgrade"
    );
    p
}

fn buy(p: &Prepared) -> Stop {
    Stop {
        index: u32::from(p.model.bis.vendor_items()[0]),
        kind: Kind::Buy,
    }
}

/// Each try starts from the same route and its matching run, as in the search.
/// Other moves may propose invalid orders; only the requested valid change counts.
fn dispatched_route(p: &Prepared, route: &[Stop], wanted: impl Fn(&[Stop]) -> bool) -> Option<Vec<Stop>> {
    let planner = planner(p);
    let run = planner.run(route).expect("a valid starting route");
    let mut rng = SmallRng::seed_from_u64(42);
    for _ in 0..4096 {
        let mut candidate = route.to_vec();
        let change = planner.random_move(&mut candidate, &run, &mut rng);
        let buys = candidate.iter().filter(|s| s.kind == Kind::Buy).count();
        assert!(
            buys <= usize::from(!p.model.bis.vendor_items().is_empty()),
            "no duplicate or unmodeled buys"
        );
        if change.is_some() && wanted(&candidate) && planner.simulate(&candidate, None).is_some() {
            return Some(candidate);
        }
    }
    None
}

fn one_quest_route() -> Vec<Stop> {
    vec![Stop::quest(0, Kind::Accept), Stop::quest(0, Kind::TurnIn)]
}

fn has_new_quest(route: &[Stop]) -> bool {
    [Kind::Accept, Kind::TurnIn]
        .iter()
        .all(|&kind| route.contains(&Stop::quest(1, kind)))
}

fn has_other_version(route: &[Stop]) -> bool {
    let quests: Vec<_> = route.iter().filter(|s| s.is_quest()).copied().collect();
    quests.len() == 4
        && [2, 3].iter().all(|&i| {
            [Kind::Accept, Kind::TurnIn]
                .iter()
                .all(|&kind| quests.contains(&Stop::quest(i, kind)))
        })
}

#[test]
fn vendor_buys_remain_reachable() {
    let p = with_vendor_item(1);
    let purchase = buy(&p);
    let candidate = dispatched_route(&p, &one_quest_route(), |route| route.contains(&purchase))
        .expect("the dispatcher still proposes a valid vendor buy");
    let (_, spent) = planner(&p).simulate_full(&candidate, None).unwrap();
    assert_eq!(spent.vendor_cost, 1);
}

#[test]
fn vendor_candidates_leave_omitted_quests_reachable() {
    let p = with_vendor_item(2);
    for already_bought in [false, true] {
        let mut route = one_quest_route();
        if already_bought {
            route.insert(0, buy(&p));
        }
        assert!(
            dispatched_route(&p, &route, has_new_quest).is_some(),
            "an omitted quest remains reachable with vendor candidates (buy present: {already_bought})"
        );
    }
}

#[test]
fn vendor_candidates_leave_class_versions_reachable() {
    let p = class_versions(with_vendor_item(4));
    for already_bought in [false, true] {
        let mut route = far_version_route();
        if already_bought {
            route.insert(0, buy(&p));
        }
        let candidate = dispatched_route(&p, &route, has_other_version)
            .unwrap_or_else(|| panic!("the whole alternative remains reachable (buy present: {already_bought})"));
        let (_, spent) = planner(&p).simulate_full(&candidate, None).unwrap();
        assert_eq!(spent.missing_class_quests, 0);
    }
}

#[test]
fn without_vendor_candidates_omitted_quests_remain_reachable() {
    let p = quests_at_start(2);
    assert_eq!(p.model.bis.vendor_items().len(), 0);
    assert!(dispatched_route(&p, &one_quest_route(), has_new_quest).is_some());
}

#[test]
fn without_vendor_candidates_class_versions_remain_reachable() {
    let p = far_and_near_versions();
    assert_eq!(p.model.bis.vendor_items().len(), 0);
    assert!(dispatched_route(&p, &far_version_route(), has_other_version).is_some());
}

/// A compact snapshot of proposals and RNG state from the corrected dispatcher.
/// Experimental operators disabled must preserve both, including vendor draws.
#[test]
fn experimental_moves_off_preserve_dispatch_and_rng() {
    use rand::RngExt;
    let mut fingerprints = Vec::new();
    for p in [quests_at_start(2), with_vendor_item(2)] {
        let planner = planner(&p);
        let route = one_quest_route();
        let run = planner.run(&route).unwrap();
        let mut rng = SmallRng::seed_from_u64(20261107);
        let mut fingerprint = 0xcbf29ce484222325_u64;
        let mut record = |value: u64| {
            for byte in value.to_le_bytes() {
                fingerprint = (fingerprint ^ u64::from(byte)).wrapping_mul(0x100000001b3);
            }
        };
        for _ in 0..128 {
            let mut candidate = route.clone();
            if let Some(change) = planner.random_move(&mut candidate, &run, &mut rng) {
                record(1);
                record(change.first as u64);
                record(change.last as u64);
                record(u64::from(change.reordered));
            } else {
                record(0);
            }
            record(candidate.len() as u64);
            for stop in candidate {
                record(u64::from(stop.index));
                let kind = match stop.kind {
                    Kind::Accept => 1,
                    Kind::TurnIn => 2,
                    Kind::Objective(i) => 100 + u64::from(i),
                    _ => 1000,
                };
                record(kind);
            }
            record(rng.random::<u64>());
        }
        fingerprints.push(fingerprint);
    }
    assert_eq!(fingerprints, [8630436502940397851, 8474989223404584315]);
}
