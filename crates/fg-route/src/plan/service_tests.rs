//! Expected service kills and the dungeon boundary, independent of optimizer ordering.
use super::tests::quests_at_start;
use super::{Edit, Event, Kind, Planner, ServiceKind, Stop};
use crate::{
    export,
    job::Prepared,
    model::{EntityKind, Names},
    params::Params,
};

fn fixture() -> Prepared {
    let mut p = quests_at_start(2);
    p.params.vendor_visits = true;
    p.params.mailbox_visits = true;
    let mut vendor = p.model.start.clone();
    vendor.id = 910_001;
    vendor.name = "Test vendor".into();
    vendor.kind = EntityKind::Npc;
    let mut mailbox = vendor.clone();
    mailbox.id = 910_002;
    mailbox.name = "Test mailbox".into();
    mailbox.kind = EntityKind::Object;
    p.model.services = [vec![vendor], vec![mailbox]];
    p
}

fn planner(p: &Prepared) -> Planner<'_> {
    Planner::new(&p.model, &p.world, &p.params, &p.profile)
}

#[test]
fn service_defaults_and_old_settings() {
    let p: Params = serde_json::from_str("{}").unwrap();
    assert!(!p.vendor_visits && !p.mailbox_visits);
    assert_eq!(
        (p.vendor_min_kills, p.vendor_max_kills, p.vendor_visit_seconds),
        (50.0, 100.0, 30.0)
    );
    assert_eq!(
        (p.mailbox_min_kills, p.mailbox_max_kills, p.mailbox_visit_seconds),
        (50.0, 100.0, 30.0)
    );
    let old: Params = serde_json::from_str(r#"{"vendor_visits":true,"vendor_min_minutes":2,"vendor_max_minutes":7,"mailbox_min_minutes":4,"mailbox_max_minutes":8,"mailbox_visit_seconds":47}"#).unwrap();
    assert!(old.vendor_visits);
    assert_eq!((old.vendor_min_kills, old.vendor_max_kills), (50.0, 100.0));
    assert_eq!((old.mailbox_min_kills, old.mailbox_max_kills), (50.0, 100.0));
    assert_eq!(old.mailbox_visit_seconds, 47.0);
    assert!(!serde_json::to_string(&old).unwrap().contains("_minutes"));
    let mut invalid = p;
    invalid.vendor_visits = true;
    invalid.vendor_min_kills = 101.0;
    assert!(invalid.validate_services().is_err());
    invalid.vendor_min_kills = 50.0;
    invalid.vendor_visit_seconds = -1.0;
    assert!(invalid.validate_services().is_err());
}

#[test]
fn service_catalog_loads_real_faction_appropriate_locations() {
    use crate::job::{self, Overrides, PlanRequest};
    use crate::xp::Edition;
    let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../..");
    for edition in [Edition::Classic, Edition::Forever, Edition::Tbc] {
        let db = if edition == Edition::Classic {
            super::tests::database()
        } else {
            let path = root.join(format!("data/{}.service-test.sqlite", edition.key()));
            if !path.exists() {
                let data = zstd::decode_all(
                    std::fs::File::open(root.join(format!("data/{}.sqlite.zst", edition.key()))).unwrap(),
                )
                .unwrap();
                std::fs::write(&path, data).unwrap();
            }
            rusqlite::Connection::open(path).unwrap()
        };
        for race in ["human", "orc"] {
            let overrides = Overrides::load_edition(&root.join("overrides"), edition).unwrap();
            let mut params = overrides.params.clone();
            params.vendor_visits = true;
            params.mailbox_visits = true;
            let request = PlanRequest {
                race: race.into(),
                class: "warrior".into(),
                from_level: 1,
                to_level: 2,
                name: None,
                params: Some(params),
                required_class_quests: None,
                professions: vec![],
                locale: None,
                group: vec![],
                start: None,
            };
            let p = job::prepare(&db, &overrides, &request, &|_| {}).unwrap();
            assert!(!p.model.services[0].is_empty(), "{race} vendors");
            assert!(!p.model.services[1].is_empty(), "{race} mailboxes");
            for vendor in &p.model.services[0] {
                let flags: (i64, String) = db
                    .query_row(
                        "SELECT npc_flags, friendly_to FROM m_npc WHERE id=?1",
                        [vendor.id],
                        |r| Ok((r.get(0)?, r.get(1)?)),
                    )
                    .unwrap();
                assert_ne!(flags.0 & if edition == Edition::Tbc { 128 } else { 4 }, 0);
                assert!(flags.1.contains(p.profile.faction.letter()));
            }
        }
    }
}

#[test]
fn service_kills_are_part_of_incremental_evaluation() {
    let mut p = fixture();
    p.model.quests[0].start_kills = 6.0;
    p.model.quests[1].start_kills = 2.0;
    p.params.vendor_min_kills = 5.0;
    p.params.vendor_max_kills = 5.0;
    let planner = planner(&p);
    let route = [
        Stop::quest(0, Kind::Accept),
        Stop::quest(1, Kind::Accept),
        Stop::quest(1, Kind::TurnIn),
        Stop::quest(0, Kind::TurnIn),
    ];
    let swapped = [route[1], route[0], route[2], route[3]];
    let run = planner.run(&route).unwrap();
    let incremental = planner.evaluate(&run, &swapped, 0, 1, true).unwrap();
    let full = planner.simulate(&swapped, None).unwrap();
    assert!((incremental - full).abs() < 1e-6);
}

#[test]
fn service_long_grinding_is_interrupted_by_due_visits() {
    let mut p = fixture();
    // Cross several boundaries before reaching level two.
    p.params.vendor_min_kills = 5.0;
    p.params.vendor_max_kills = 5.0;
    p.params.mailbox_min_kills = 5.0;
    p.params.mailbox_max_kills = 5.0;
    let planner = planner(&p);
    let mut s = planner.initial_state();
    let mut events = Vec::new();
    planner.grind_at_camps(&mut s, 2, &mut |state, event| events.push((state.time, event)));
    assert!(s.time.is_finite() && s.level == 2);
    let first = events
        .iter()
        .position(|(_, e)| matches!(e, Event::Service { .. }))
        .unwrap();
    assert!(
        events[..first]
            .iter()
            .any(|(_, e)| matches!(e, Event::GrindMobs { .. }))
    );
    let killed: u32 = events[..first]
        .iter()
        .filter_map(|(_, e)| match e {
            Event::GrindMobs { mobs, .. } => Some(mobs),
            _ => None,
        })
        .sum();
    assert_eq!(killed, 5);
    assert!(
        events[first + 1..]
            .iter()
            .any(|(_, e)| matches!(e, Event::GrindMobs { .. })),
        "grinding resumes after service"
    );
}

#[test]
fn service_minimum_and_independent_resets_and_durations() {
    let mut p = fixture();
    p.params.mailbox_visit_seconds = 47.0;
    let planner = planner(&p);
    let mut s = planner.initial_state();
    // Even a very long route with no kills does not make service due.
    s.time = 100_000.0;
    let mut events = Vec::new();
    planner.service_visits(&mut s, Some(p.model.start.pos), false, true, &mut |_, e| events.push(e));
    assert!(events.is_empty());
    s.service_kills = [49.5, 20.5];
    s.add_kills(0.5);
    planner.service_visits(&mut s, Some(p.model.start.pos), false, true, &mut |_, e| events.push(e));
    assert_eq!(events.len(), 1);
    assert!(matches!(
        events[0],
        Event::Service {
            kind: ServiceKind::Vendor,
            ..
        }
    ));
    assert_eq!(s.time, 100_030.0);
    assert_eq!(s.service_kills, [0.0, 21.0]);
    planner.service_at(&mut s, ServiceKind::Mailbox, 0, &mut |_, _| {});
    assert_eq!(s.time, 100_077.0);
    assert_eq!(s.service_kills, [0.0, 0.0]);
    assert_eq!(s.spent.overhead, 77.0);
}

#[test]
fn service_long_visit_durations_do_not_retrigger_visits() {
    let mut p = fixture();
    p.params.vendor_min_kills = 0.0;
    p.params.mailbox_min_kills = 0.0;
    p.params.vendor_max_kills = 0.001;
    p.params.mailbox_max_kills = 0.001;
    p.params.vendor_visit_seconds = 5000.0;
    p.params.mailbox_visit_seconds = 5000.0;
    let planner = planner(&p);
    let mut s = planner.initial_state();
    let mut events = Vec::new();
    planner.grind_at_camps(&mut s, 2, &mut |_, e| events.push(e));
    assert!(s.time.is_finite() && s.level == 2);
    assert!(events.len() < 200);
}

#[test]
fn service_final_grind_edits_preserve_visits_locations_and_time() {
    let mut p = fixture();
    p.params.vendor_min_kills = 5.0;
    p.params.vendor_max_kills = 5.0;
    p.params.mailbox_min_kills = 5.0;
    p.params.mailbox_max_kills = 5.0;
    let planner = planner(&p);
    let base = [Edit::Stop {
        stop: Stop::quest(0, Kind::Accept),
        farm: false,
        spot: None,
    }];
    let original = planner.edit(&base);
    let count = original
        .trace
        .iter()
        .filter(|t| matches!(t.event, Event::Service { .. }))
        .count();
    assert!(count > 0, "fixture must service during the final grind");
    let expanded = planner.expand(&base);
    assert!(expanded.iter().any(|s| matches!(s, Edit::CampGrind { .. })));
    let replayed = planner.explicit().edit(&expanded);
    assert_eq!(replayed.errors(), 0);
    assert_eq!(
        replayed
            .trace
            .iter()
            .filter(|t| matches!(t.event, Event::Service { .. }))
            .count(),
        count
    );
    assert!(
        (original.breakdown.total - replayed.breakdown.total).abs() < 1.0,
        "saved camp locations retain trip and service time: {} vs {}",
        original.breakdown.total,
        replayed.breakdown.total
    );
}

#[test]
fn service_maximum_commits_to_travel_instead_of_an_arrival_deadline() {
    let mut p = fixture();
    p.params.mailbox_visits = false;
    p.model.services[0][0].pos.x += 5000.0;
    let planner = planner(&p);
    let mut s = planner.initial_state();
    s.service_kills[0] = 50.0;
    let mut events = Vec::new();
    planner.service_visits(&mut s, Some(p.model.start.pos), false, true, &mut |_, e| events.push(e));
    assert!(events.is_empty(), "an expensive detour may wait after minimum");
    s.service_kills[0] = 100.0;
    planner.service_visits(&mut s, None, false, false, &mut |_, e| events.push(e));
    assert!(s.time.is_finite() && s.time > 30.0);
    assert_eq!(s.pos, p.model.services[0][0].pos);
    assert_eq!(s.service_kills[0], 0.0);
    assert_eq!(events.iter().filter(|e| matches!(e, Event::Service { .. })).count(), 1);
}

fn dungeon_fixture(minutes: f64) -> Prepared {
    let mut p = fixture();
    p.params.dungeons = true;
    let d = &mut p.model.dungeons[0];
    d.entrance = p.model.start.clone();
    d.min_level = 1;
    d.max_level = 60;
    d.minutes = minutes;
    d.kills = 120.0;
    p
}

#[test]
fn service_forced_before_short_dungeon_even_below_minimum() {
    let mut p = dungeon_fixture(5.0);
    p.model.dungeons[0].kills = 10.0;
    let planner = planner(&p);
    let mut s = planner.initial_state();
    let mut trace = Vec::new();
    planner.apply(
        &mut s,
        Stop {
            kind: Kind::Dungeon,
            index: 0,
        },
        Some(&mut trace),
    );
    let services: Vec<_> = trace
        .iter()
        .filter(|t| matches!(t.event, Event::Service { .. }))
        .collect();
    assert_eq!(services.len(), 2);
    let run = trace
        .iter()
        .position(|t| matches!(t.event, Event::Stop { stop, .. } if stop.kind == Kind::Dungeon))
        .unwrap();
    assert_eq!(
        trace[..run]
            .iter()
            .filter(|t| matches!(t.event, Event::Service { .. }))
            .count(),
        2
    );
    assert!(
        trace[run + 1..]
            .iter()
            .any(|t| matches!(t.event, Event::DungeonExit { .. }))
    );
}

#[test]
fn service_due_inside_dungeon_waits_until_exit_and_survives_guide_export() {
    let p = dungeon_fixture(40.0);
    let planner = planner(&p);
    let stop = Stop {
        kind: Kind::Dungeon,
        index: 0,
    };
    let mut s = planner.initial_state();
    let mut trace = Vec::new();
    planner.apply(&mut s, stop, Some(&mut trace));
    let kinds: Vec<_> = trace
        .iter()
        .filter_map(|t| match t.event {
            Event::Service {
                kind: ServiceKind::Vendor,
                ..
            } => Some("vendor"),
            Event::Service {
                kind: ServiceKind::Mailbox,
                ..
            } => Some("mailbox"),
            Event::Stop { stop, .. } if stop.kind == Kind::Dungeon => Some("dungeon"),
            Event::DungeonExit { .. } => Some("exit"),
            _ => None,
        })
        .collect();
    assert_eq!(kinds, ["vendor", "mailbox", "dungeon", "exit", "vendor", "mailbox"]);
    let guide = export::build_traced(&planner, &p.profile, &[stop], &trace, s.time, s.spent);
    let guide = export::render(&guide, &Names::default());
    let json = serde_json::to_value(&guide).unwrap();
    let restored = export::read_route(json).unwrap();
    assert!(restored.steps.iter().any(|s| s.kind == export::StepKind::DungeonExit));
    let lua = export::to_lua(&[restored]);
    assert!(lua.contains("dungeon_exit") && lua.contains("mailbox") && lua.contains("vendor"));
    assert_eq!(s.spent.dungeons, 2400.0);
}

#[test]
fn service_expansion_replays_without_duplicate_automatic_visits() {
    let p = dungeon_fixture(40.0);
    let planner = planner(&p);
    let edits = planner.expand(&[Edit::Stop {
        stop: Stop {
            kind: Kind::Dungeon,
            index: 0,
        },
        farm: false,
        spot: None,
    }]);
    let service_count = edits.iter().filter(|e| matches!(e, Edit::Service { .. })).count();
    assert_eq!(service_count, 4);
    let explicit = planner.explicit().edit(&edits);
    assert_eq!(explicit.errors(), 0);
    assert_eq!(
        explicit
            .trace
            .iter()
            .filter(|t| matches!(t.event, Event::Service { .. }))
            .count(),
        service_count
    );
}

#[test]
fn service_quest_and_start_drop_kills_count_fractional_remaining_work() {
    use crate::model::{Guard, Objective};
    let mut p = fixture();
    p.params.group_size = 3;
    p.model.quests[0].start_kills = 3.5;
    p.model.quests[0].start_mob_level = 1;
    let loc = p.model.start.clone();
    p.model.quests[0].objectives = vec![Objective {
        loc,
        text: "Loot five items".into(),
        kills: 12.5,
        mob_level: 1,
        uses: 0.0,
        extra: 0.0,
        dungeon: None,
        elite: true,
        count: 5.0,
        mobs: vec![],
        spots: None,
        pull: 1.0,
        spawns: 0,
        pack: 1.0,
        guard: Guard::default(),
        alts: vec![],
        after: 0,
        handed_by: vec![],
        stock: None,
        child: None,
        child_name: None,
        vendors: vec![],
        auction: vec![],
    }];
    let planner = planner(&p);
    let mut s = planner.initial_state();
    s.rested = 1000;
    planner.accept(&mut s, 0);
    assert_eq!(s.service_kills, [3.5; 2]);
    // Already-completed work is not new inventory pressure.
    s.started.push((0, 0, 0.25));
    planner.apply(&mut s, Stop::quest(0, Kind::Objective(0)), None);
    assert_eq!(s.service_kills, [3.5 + 12.5 * 0.75; 2]);
    // Reapplying a completed objective must not double-count.
    planner.apply(&mut s, Stop::quest(0, Kind::Objective(0)), None);
    assert_eq!(s.service_kills, [12.875; 2]);
}

#[test]
fn service_grinding_counts_mobs_without_rest_or_party_multiplier() {
    let mut p = fixture();
    p.params.group_size = 3;
    let planner = planner(&p);
    let mut s = planner.initial_state();
    s.rested = 10000;
    planner.growth().grind_mobs(&mut s, 3);
    assert_eq!(s.service_kills, [3.0; 2]);
    let need = planner.rules().to_next_level(s.level) - s.xp;
    let base_xp = planner
        .rules()
        .mob_xp(s.level, s.level, crate::xp::content_at(s.pos.continent, s.pos.zone))
        * p.params.group_xp_share();
    let expected = (need - (need / 2).min(s.rested.max(0))) as f64 / base_xp;
    let next_level = s.level + 1;
    planner.growth().grind_to(&mut s, next_level);
    assert!((s.service_kills[0] - 3.0 - expected).abs() < 1e-9);
    assert_eq!(s.service_kills[0], s.service_kills[1]);
}

#[test]
fn service_travel_farming_counts_the_farm_event_once() {
    let mut p = fixture();
    p.params.farm_on_way = true;
    p.params.farm_cap = 1.0;
    p.params.crowded = 0.0;
    p.profile.to_level = 60;
    let mut full_kills = 0.0;
    for (full, share) in [(true, 1.0), (false, 0.8)] {
        p.params.farm_share = share;
        let planner = planner(&p);
        let mut s = planner.initial_state();
        let camps = p.model.farm.camps(s.pos.continent, 1);
        assert_ne!(camps, []);
        let b = camps[0];
        let mut a = b;
        a.x -= 1000.0;
        let mut kills = 0;
        planner.farm_along(&mut s, &a, &b, &mut |_, e| {
            if let Event::Farm { kills: count, .. } = e {
                kills += count;
            }
        });
        assert!(kills > 0);
        if full {
            full_kills = f64::from(kills);
            assert_eq!(s.service_kills, [full_kills; 2]);
        } else {
            // v13 rounds the display event; inventory retains fractional expected kills.
            let expected = full_kills * share;
            assert!((expected - expected.round()).abs() > 1e-9);
            assert!((s.service_kills[0] - expected).abs() < 1e-9);
            assert_eq!(s.service_kills[0], s.service_kills[1]);
            assert_eq!(kills, expected.round() as u32);
        }
    }
}

#[test]
fn service_dungeon_uses_whole_run_kill_estimate() {
    let mut p = dungeon_fixture(1.0);
    p.model.dungeons[0].kills = 70.5;
    let planner = planner(&p);
    let mut s = planner.initial_state();
    let mut trace = Vec::new();
    planner.apply(
        &mut s,
        Stop {
            kind: Kind::Dungeon,
            index: 0,
        },
        Some(&mut trace),
    );
    assert_eq!(s.service_kills, [70.5; 2]);
    // Below maximum: only the two pre-entry services, despite being above minimum.
    assert_eq!(
        trace
            .iter()
            .filter(|t| matches!(t.event, Event::Service { .. }))
            .count(),
        2
    );
}

#[test]
fn service_forced_dungeon_visits_follow_the_level_gating_grind() {
    let mut p = dungeon_fixture(1.0);
    p.model.dungeons[0].min_level = 2;
    p.model.dungeons[0].kills = 0.0;
    p.params.vendor_min_kills = 10_000.0;
    p.params.vendor_max_kills = 10_000.0;
    p.params.mailbox_min_kills = 10_000.0;
    p.params.mailbox_max_kills = 10_000.0;
    let planner = planner(&p);
    let mut s = planner.initial_state();
    let mut trace = Vec::new();
    planner.apply(
        &mut s,
        Stop {
            kind: Kind::Dungeon,
            index: 0,
        },
        Some(&mut trace),
    );
    let grind = trace
        .iter()
        .rposition(|t| matches!(t.event, Event::Grind { .. } | Event::GrindMobs { .. }))
        .unwrap();
    let services: Vec<_> = trace
        .iter()
        .enumerate()
        .filter(|(_, t)| matches!(t.event, Event::Service { .. }))
        .collect();
    assert_eq!(services.len(), 2);
    assert!(services.iter().all(|(i, t)| *i > grind && t.level >= 2));
    assert_eq!(s.service_kills, [0.0; 2]);
}

#[test]
fn service_skyborne_warriors_complete_initial_construction() {
    use crate::job::{self, Overrides, PlanRequest};
    use crate::params::Construction;
    use crate::xp::Edition;
    use std::cell::Cell;
    let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../..");
    let path = root.join("data/forever.service-test.sqlite");
    if !path.exists() {
        let data = zstd::decode_all(std::fs::File::open(root.join("data/forever.sqlite.zst")).unwrap()).unwrap();
        std::fs::write(&path, data).unwrap();
    }
    let conn = rusqlite::Connection::open(path).unwrap();
    let overrides = Overrides::load_edition(&root.join("overrides"), Edition::Forever).unwrap();
    for race in ["skyborne", "skyborne_horde"] {
        for (vendor, mailbox) in [(false, false), (true, false), (false, true), (true, true)] {
            for construction in [Construction::Greedy, Construction::Regions] {
                let mut params = overrides.params.clone();
                params.vendor_visits = vendor;
                params.mailbox_visits = mailbox;
                params.construction = construction;
                params.construction_random = false;
                params.dungeons = false;
                let req = PlanRequest {
                    race: race.into(),
                    class: "warrior".into(),
                    from_level: 1,
                    to_level: 10,
                    name: None,
                    params: Some(params),
                    required_class_quests: None,
                    professions: vec![],
                    locale: None,
                    group: vec![],
                    start: None,
                };
                let p = job::prepare(&conn, &overrides, &req, &|_| {}).unwrap();
                if mailbox {
                    assert!(
                        p.model.services[1]
                            .iter()
                            .any(|loc| loc.zone == 16593 && loc.pos.continent == 2991),
                        "{race}: Skyborne starter island must have reachable mailbox hubs"
                    );
                }
                let checks = Cell::new(0);
                let abort = || {
                    checks.set(checks.get() + 1);
                    checks.get() > 10_000
                };
                let planner = planner(&p).abortable(&abort);
                let route = planner.construct(|_| {});
                assert!(
                    checks.get() <= 10_000,
                    "{race} {vendor}/{mailbox} {construction:?}: construction did not finish"
                );
                let mut trace = Vec::new();
                assert!(
                    planner.simulate(&route, Some(&mut trace)).is_some_and(f64::is_finite),
                    "{race} {vendor}/{mailbox} {construction:?}: initial route is invalid"
                );
                for (enabled, kind) in [(vendor, ServiceKind::Vendor), (mailbox, ServiceKind::Mailbox)] {
                    assert_eq!(
                        trace
                            .iter()
                            .any(|t| matches!(t.event, Event::Service { kind: k, .. } if k == kind)),
                        enabled,
                        "{race} {construction:?}: scheduled {kind:?} visits"
                    );
                }
            }
        }
    }
}

#[test]
fn service_unreachable_mandatory_visit_terminates_construction() {
    use crate::params::Construction;
    use std::cell::Cell;
    for construction in [Construction::Greedy, Construction::Regions] {
        let mut p = fixture();
        p.model.quests[0].mandatory = true;
        p.model.quests[0].xp = 100;
        p.model.quests[0].start_kills = 2.0;
        p.params.mailbox_min_kills = 1.0;
        p.params.mailbox_max_kills = 1.0;
        // No route can satisfy the next mandatory mailbox visit, or the finishing grind.
        p.model.services[1].clear();
        p.params.construction = construction;
        p.params.construction_random = false;
        let checks = Cell::new(0);
        let abort = || {
            checks.set(checks.get() + 1);
            checks.get() > 1000
        };
        let planner = planner(&p).abortable(&abort);
        let route = planner.construct(|_| {});
        assert!(
            checks.get() <= 1000,
            "{construction:?}: invalid state was repeatedly retried"
        );
        assert!(
            planner.simulate(&route, None).is_none_or(|time| !time.is_finite()),
            "{construction:?}: an impossible mandatory visit must not be silently omitted"
        );
    }
}
