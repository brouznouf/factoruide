//! Strict checks of a contribution. It comes from anyone (the app is open source, its endpoint
//! public): every field is bounded and checked against what the app and addon can actually
//! produce, and the whole document is rejected at the first anomaly. Error messages only quote
//! our own values (field names, numbers), never the sender's text.

use crate::contribution::{Contribution, FORMAT, VERSION};
use anyhow::{Result, bail, ensure};

/// Largest document accepted.
pub const MAX_BYTES: usize = 16 * 1024 * 1024;
pub const EDITIONS: &[&str] = &["forever", "classic", "tbc"];
pub const LOCALES: &[&str] = &[
    "enUS", "enGB", "frFR", "deDE", "esES", "esMX", "itIT", "ptBR", "ruRU", "koKR", "zhCN", "zhTW",
];
/// What the addon records (Collector.lua).
pub const EVENTS: &[&str] = &[
    "accept", "active", "detail", "kill", "level", "loot", "offer", "removed", "seen", "taxi", "transit", "turnin",
];
pub const ENTITY_TYPES: &[&str] = &["npc", "object", "item"];

const MAX_CACHE_FILES: usize = 12;
const MAX_QUESTS_PER_FILE: usize = 20_000;
const MAX_QUEST_ID: u32 = 100_000;
const MAX_TITLE: usize = 200;
const MAX_LOG: usize = 4_000;
const MAX_OBJECTIVES: usize = 12;
const MAX_CHARACTERS: usize = 50;
const MAX_EVENTS_PER_CHARACTER: usize = 100_000;
const MAX_EVENTS: usize = 300_000;
const MAX_ENTITY_ID: i64 = 10_000_000;
const MAX_UI_MAP: i64 = 10_000;
const MAX_LEVEL: i64 = 80;
const MAX_DATA_BYTES: usize = 2_048;
const MAX_DATA_DEPTH: usize = 3;
const MAX_DATA_STRING: usize = 200;
/// 2023-11-14: nothing older can come from this app.
const MIN_TIME: u64 = 1_700_000_000;
/// Clocks may be a bit ahead.
const CLOCK_SKEW: u64 = 86_400;

fn is_hex(s: &str, len: usize) -> bool {
    s.len() == len && s.bytes().all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b))
}

/// Text without control characters (except line breaks and tabs when `multiline`).
fn clean_text(s: &str, max: usize, multiline: bool) -> bool {
    s.chars().count() <= max
        && s.chars()
            .all(|c| !c.is_control() || (multiline && matches!(c, '\n' | '\r' | '\t')))
}

fn check_data(v: &serde_json::Value, depth: usize) -> Result<()> {
    use serde_json::Value as J;
    ensure!(depth <= MAX_DATA_DEPTH, "event data too deep");
    match v {
        J::Null | J::Bool(_) => {}
        J::Number(n) => ensure!(n.as_f64().is_some_and(f64::is_finite), "event data: bad number"),
        J::String(s) => ensure!(clean_text(s, MAX_DATA_STRING, false), "event data: bad text"),
        J::Array(a) => a.iter().try_for_each(|x| check_data(x, depth + 1))?,
        J::Object(o) => {
            for (k, x) in o {
                ensure!(
                    k.len() <= 32 && k.bytes().all(|b| b.is_ascii_alphanumeric() || b == b'_'),
                    "event data: bad key"
                );
                check_data(x, depth + 1)?;
            }
        }
    }
    Ok(())
}

/// Check every field of a contribution (`now`: Unix seconds).
pub fn validate(c: &Contribution, now: u64) -> Result<()> {
    ensure!(c.format == FORMAT, "not a Factoruide contribution");
    ensure!(
        (1..=VERSION).contains(&c.version),
        "unsupported contribution version {}",
        c.version
    );
    ensure!(EDITIONS.contains(&c.edition.as_str()), "unknown edition");
    ensure!(
        !c.app_version.is_empty()
            && c.app_version.len() <= 32
            && c.app_version
                .bytes()
                .all(|b| b.is_ascii_alphanumeric() || b"+-.".contains(&b)),
        "bad app_version"
    );
    if c.version >= 2 {
        ensure!(is_hex(&c.contributor, 32), "bad contributor id");
    } else {
        ensure!(
            c.contributor.is_empty() || is_hex(&c.contributor, 32),
            "bad contributor id"
        );
    }
    ensure!((MIN_TIME..=now + CLOCK_SKEW).contains(&c.created), "bad creation time");

    ensure!(c.quest_cache.len() <= MAX_CACHE_FILES, "too many quest caches");
    let mut locales = Vec::new();
    for file in &c.quest_cache {
        ensure!(LOCALES.contains(&file.locale.as_str()), "unknown locale");
        ensure!(!locales.contains(&&file.locale), "duplicate locale");
        locales.push(&file.locale);
        ensure!((1..=1_000_000).contains(&file.build), "bad client build");
        ensure!(file.quests.len() <= MAX_QUESTS_PER_FILE, "too many quests");
        for (&id, q) in &file.quests {
            ensure!((1..=MAX_QUEST_ID).contains(&id), "bad quest id {id}");
            ensure!(
                !q.title.trim().is_empty() && clean_text(&q.title, MAX_TITLE, false),
                "quest {id}: bad title"
            );
            ensure!(clean_text(&q.log, MAX_LOG, true), "quest {id}: bad log text");
            ensure!(q.objectives.len() <= MAX_OBJECTIVES, "quest {id}: too many objectives");
            ensure!(
                q.confirmed || q.objectives.is_empty(),
                "quest {id}: unconfirmed objectives"
            );
            for o in &q.objectives {
                ensure!(o.kind <= 30, "quest {id}: bad objective type");
                ensure!(
                    (0..=MAX_ENTITY_ID as i32).contains(&o.target),
                    "quest {id}: bad objective target"
                );
                ensure!((1..10_000).contains(&o.amount), "quest {id}: bad objective amount");
            }
        }
    }

    ensure!(c.collector.len() <= MAX_CHARACTERS, "too many characters");
    let mut ids = Vec::new();
    let mut total = 0;
    for ch in &c.collector {
        ensure!(is_hex(&ch.character, 16), "bad character id");
        ensure!(!ids.contains(&&ch.character), "duplicate character");
        ids.push(&ch.character);
        for v in [&ch.race, &ch.class].into_iter().flatten() {
            ensure!(
                v.len() <= 24 && v.bytes().all(|b| b.is_ascii_alphabetic()),
                "bad race or class"
            );
        }
        ensure!(ch.events.len() <= MAX_EVENTS_PER_CHARACTER, "too many events");
        total += ch.events.len();
        ensure!(total <= MAX_EVENTS, "too many events");
        let mut last_seq = 0;
        for e in &ch.events {
            let Some(seq) = e.seq else {
                bail!("event without sequence number")
            };
            ensure!(seq > last_seq && seq <= 1_000_000_000, "events out of sequence");
            last_seq = seq;
            if let Some(ts) = e.ts {
                ensure!(
                    ts >= MIN_TIME as i64 && ts <= (now + CLOCK_SKEW) as i64,
                    "bad event time"
                );
            }
            ensure!(
                e.event.as_deref().is_some_and(|ev| EVENTS.contains(&ev)),
                "unknown event type"
            );
            if let Some(q) = e.quest_id {
                ensure!((1..=i64::from(MAX_QUEST_ID)).contains(&q), "bad event quest id");
            }
            ensure!(
                e.entity_type.is_some() == e.entity_id.is_some(),
                "event entity incomplete"
            );
            if let Some(t) = &e.entity_type {
                ensure!(ENTITY_TYPES.contains(&t.as_str()), "unknown entity type");
            }
            if let Some(id) = e.entity_id {
                ensure!((1..=MAX_ENTITY_ID).contains(&id), "bad entity id");
            }
            if let Some(m) = e.ui_map_id {
                ensure!((1..=MAX_UI_MAP).contains(&m), "bad map id");
            }
            for v in [e.x, e.y].into_iter().flatten() {
                ensure!(v.is_finite() && (0.0..=100.0).contains(&v), "bad position");
            }
            ensure!(
                e.x.is_some() == e.y.is_some() && (e.x.is_none() || e.ui_map_id.is_some()),
                "position incomplete"
            );
            if let Some(l) = e.level {
                ensure!((1..=MAX_LEVEL).contains(&l), "bad level");
            }
            if let Some(d) = &e.data {
                ensure!(d.is_object(), "event data is not an object");
                ensure!(d.to_string().len() <= MAX_DATA_BYTES, "event data too large");
                check_data(d, 0)?;
            }
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::collector::{Character, Event};
    use crate::wdb::{CacheFile, CachedQuest, Objective};

    const NOW: u64 = 1_800_000_000;

    fn sample() -> Contribution {
        let quest = CachedQuest {
            title: "The Defias Brotherhood".into(),
            log: "Kill them.\nAll.".into(),
            objectives: vec![Objective {
                kind: 0,
                target: 116,
                amount: 10,
            }],
            confirmed: true,
        };
        let event = Event {
            seq: Some(1),
            ts: Some(1_790_000_000),
            event: Some("accept".into()),
            quest_id: Some(155),
            entity_type: Some("npc".into()),
            entity_id: Some(234),
            ui_map_id: Some(1436),
            x: Some(56.3),
            y: Some(47.5),
            level: Some(14),
            data: Some(serde_json::json!({"title": "The Defias Brotherhood", "level": 14})),
        };
        Contribution {
            format: FORMAT.into(),
            version: VERSION,
            edition: "classic".into(),
            app_version: "0.1.0".into(),
            contributor: "0123456789abcdef0123456789abcdef".into(),
            created: 1_790_000_000,
            quest_cache: vec![CacheFile {
                locale: "enUS".into(),
                build: 61582,
                quests: [(155, quest)].into(),
            }],
            collector: vec![Character {
                character: "0123456789abcdef".into(),
                race: Some("Human".into()),
                class: Some("WARRIOR".into()),
                events: vec![event],
            }],
        }
    }

    fn rejected(f: impl FnOnce(&mut Contribution)) -> bool {
        let mut c = sample();
        f(&mut c);
        validate(&c, NOW).is_err()
    }

    #[test]
    fn accepts_a_genuine_contribution() {
        validate(&sample(), NOW).unwrap();
    }

    #[test]
    fn rejects_anomalies() {
        assert!(rejected(|c| c.format = "other".into()));
        assert!(rejected(|c| c.edition = "retail".into()));
        assert!(rejected(|c| c.contributor = "../../etc/passwd".into()));
        assert!(rejected(|c| c.app_version = "<script>".into()));
        assert!(rejected(|c| c.created = NOW + 10 * CLOCK_SKEW));
        assert!(rejected(|c| c.quest_cache[0].locale = "xxXX".into()));
        assert!(rejected(
            |c| c.quest_cache[0].quests.get_mut(&155).unwrap().title = "a\u{0}b".into()
        ));
        assert!(rejected(
            |c| c.quest_cache[0].quests.get_mut(&155).unwrap().title = "x".repeat(500)
        ));
        assert!(rejected(|c| c.quest_cache[0]
            .quests
            .get_mut(&155)
            .unwrap()
            .objectives[0]
            .amount = 0));
        assert!(rejected(|c| c.collector[0].character = "Brouz-Realm".into()));
        assert!(rejected(|c| c.collector[0].events[0].event = Some("hack".into())));
        assert!(rejected(|c| c.collector[0].events[0].x = Some(f64::NAN)));
        assert!(rejected(|c| c.collector[0].events[0].x = Some(150.0)));
        assert!(rejected(|c| c.collector[0].events[0].quest_id = Some(-3)));
        assert!(rejected(
            |c| c.collector[0].events[0].data = Some(serde_json::json!({"a": {"b": {"c": {"d": 1}}}}))
        ));
        assert!(rejected(|c| {
            let e = c.collector[0].events[0].clone();
            c.collector[0].events.push(e);
        }));
    }
}
