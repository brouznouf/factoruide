//! Strict checks of a contribution. It comes from anyone (the app is open source, its endpoint
//! public): every field is bounded and checked against what the app and addon can actually
//! produce, and the whole document is rejected at the first anomaly. Error messages only quote
//! our own values (field names, numbers), never the sender's text.
//!
//! Event data follows a closed schema per event type (`SCHEMAS`): known keys only, each with
//! its type and bounds, so nothing but what the addon records can travel in a contribution.

use crate::collector::Event;
use crate::contribution::{Contribution, FORMAT, VERSION};
use crate::wdb::{CachedCreature, CachedObject, CachedQuest, LocaleCache};
use anyhow::{Result, bail, ensure};

/// Largest document accepted.
pub const MAX_BYTES: usize = 16 * 1024 * 1024;
pub const EDITIONS: &[&str] = &["forever", "classic", "tbc"];
pub const LOCALES: &[&str] = &[
    "enUS", "enGB", "frFR", "deDE", "esES", "esMX", "itIT", "ptBR", "ruRU", "koKR", "zhCN", "zhTW",
];
pub const ENTITY_TYPES: &[&str] = &["npc", "object", "item"];

const MAX_CACHE_FILES: usize = 12;
const MAX_QUESTS_PER_FILE: usize = 20_000;
const MAX_QUEST_ID: u32 = 100_000;
const MAX_TITLE: usize = 200;
const MAX_LOG: usize = 4_000;
const MAX_OBJECTIVES: usize = 12;
const MAX_ENTITIES_PER_FILE: usize = 100_000;
const MAX_NAME: usize = 100;
const MAX_QUEST_ITEMS: usize = 32;
pub const MAX_CHARACTERS: usize = 50;
const MAX_EVENTS_PER_CHARACTER: usize = 100_000;
pub const MAX_EVENTS: usize = 300_000;
const MAX_ENTITY_ID: i64 = 10_000_000;
const MAX_ITEM_ID: i64 = 10_000_000;
const MAX_UI_MAP: i64 = 10_000;
const MAX_LEVEL: i64 = 80;
const MAX_DATA_BYTES: usize = 2_048;
/// 2023-11-14: nothing older can come from this app.
pub(crate) const MIN_TIME: u64 = 1_700_000_000;
/// Clocks may be a bit ahead.
pub(crate) const CLOCK_SKEW: u64 = 86_400;

/// Whether an event type carries a quest id or an entity.
#[derive(Clone, Copy, PartialEq)]
enum Need {
    Always,
    Never,
    Optional,
}

/// Type and bounds of one data field.
#[derive(Clone, Copy)]
enum Field {
    Int(i64, i64),
    Num(f64, f64),
    Bool,
    /// Game text (quest title, NPC name), at most this many characters.
    Text(usize),
    OneOf(&'static [&'static str]),
    /// List of at most `max` [a, b] integer pairs.
    Pairs {
        max: usize,
        a: (i64, i64),
        b: (i64, i64),
    },
}

struct Schema {
    event: &'static str,
    quest: Need,
    entity: Need,
    fields: &'static [(&'static str, Field)],
}

const QUEST_LEVEL: Field = Field::Int(-1, 90);
const XP: Field = Field::Int(0, 10_000_000);
const PERCENT: Field = Field::Num(0.0, 100.0);
/// Seconds since a creature spawned (its GUID tells), within the hour: older, the spawn of a
/// creature that never died would date the realm's restart, and so the time of day.
const AGE: Field = Field::Int(0, 3_600);

/// What the addon records (Collector.lua), event by event.
const SCHEMAS: &[Schema] = &[
    Schema {
        event: "accept",
        quest: Need::Always,
        entity: Need::Optional,
        fields: &[("title", Field::Text(MAX_TITLE)), ("level", QUEST_LEVEL)],
    },
    Schema {
        event: "active",
        quest: Need::Always,
        entity: Need::Always,
        fields: &[("complete", Field::Bool)],
    },
    Schema {
        event: "combat",
        quest: Need::Never,
        entity: Need::Optional,
        fields: &[
            ("seconds", Field::Int(0, 3_600)),
            ("kills", Field::Int(0, 100)),
            ("xp", XP),
            ("mobLevel", QUEST_LEVEL),
            ("elite", Field::Bool),
            ("pet", Field::Bool),
            ("died", Field::Bool),
        ],
    },
    Schema {
        event: "detail",
        quest: Need::Always,
        entity: Need::Optional,
        fields: &[("xp", XP)],
    },
    Schema {
        event: "flight",
        quest: Need::Never,
        entity: Need::Never,
        fields: &[
            ("fromMap", Field::Int(1, MAX_UI_MAP)),
            ("fromX", PERCENT),
            ("fromY", PERCENT),
            ("seconds", Field::Int(0, 7_200)),
        ],
    },
    Schema {
        event: "kill",
        quest: Need::Never,
        entity: Need::Always,
        fields: &[("age", AGE)],
    },
    Schema {
        event: "level",
        quest: Need::Never,
        entity: Need::Never,
        fields: &[("level", Field::Int(1, MAX_LEVEL))],
    },
    Schema {
        event: "loot",
        quest: Need::Never,
        entity: Need::Always,
        fields: &[(
            "items",
            Field::Pairs {
                max: 16,
                a: (1, MAX_ITEM_ID),
                b: (1, 1_000),
            },
        )],
    },
    Schema {
        event: "offer",
        quest: Need::Always,
        entity: Need::Always,
        fields: &[
            ("level", QUEST_LEVEL),
            ("trivial", Field::Bool),
            ("repeatable", Field::Bool),
        ],
    },
    Schema {
        event: "progress",
        quest: Need::Always,
        entity: Need::Never,
        fields: &[
            ("index", Field::Int(1, MAX_OBJECTIVES as i64)),
            ("value", Field::Int(0, 9_999)),
        ],
    },
    Schema {
        event: "removed",
        quest: Need::Always,
        entity: Need::Never,
        fields: &[],
    },
    Schema {
        event: "seen",
        quest: Need::Never,
        entity: Need::Always,
        fields: &[
            ("level", QUEST_LEVEL),
            ("name", Field::Text(100)),
            ("reaction", Field::Int(1, 8)),
            ("elite", Field::Bool),
            ("age", AGE),
        ],
    },
    Schema {
        event: "step",
        quest: Need::Optional,
        entity: Need::Never,
        fields: &[("step", Field::Int(1, 100_000)), ("planned", Field::Int(0, 86_400))],
    },
    Schema {
        event: "taxi",
        quest: Need::Never,
        entity: Need::Never,
        fields: &[
            ("map", Field::Int(1, MAX_UI_MAP)),
            (
                "nodes",
                Field::Pairs {
                    max: 128,
                    a: (1, 100_000),
                    b: (0, 2),
                },
            ),
        ],
    },
    Schema {
        event: "transit",
        quest: Need::Never,
        entity: Need::Never,
        fields: &[
            ("fromMap", Field::Int(1, MAX_UI_MAP)),
            ("fromX", PERCENT),
            ("fromY", PERCENT),
            ("seconds", Field::Int(0, 86_400)),
            ("mode", Field::OneOf(&["hearth", "spell", "ride"])),
        ],
    },
    Schema {
        event: "turnin",
        quest: Need::Always,
        entity: Need::Optional,
        fields: &[("xp", XP), ("money", Field::Int(0, 1_000_000_000))],
    },
];

/// A data field of the addon's compact rows (`key=value`, see `collector::parse_row`), typed by
/// its schema: integers and numbers in decimal, booleans as 1/0, texts percent-encoded, pairs as
/// `a:b,a:b`. A key the schema does not know stays a string, a value that does not read becomes
/// null: the event then fails its check.
pub fn decode_field(event: &str, key: &str, raw: &str) -> serde_json::Value {
    use serde_json::Value as J;
    let field = SCHEMAS
        .iter()
        .find(|s| s.event == event)
        .and_then(|s| s.fields.iter().find(|(k, _)| *k == key))
        .map(|&(_, f)| f);
    let decoded = match field {
        Some(Field::Int(..)) => raw.parse::<i64>().ok().map(J::from),
        Some(Field::Num(..)) => raw
            .parse::<f64>()
            .ok()
            .and_then(serde_json::Number::from_f64)
            .map(J::Number),
        Some(Field::Bool) => match raw {
            "1" => Some(J::Bool(true)),
            "0" => Some(J::Bool(false)),
            _ => None,
        },
        Some(Field::Text(_)) => percent_decode(raw).map(J::String),
        Some(Field::OneOf(_)) | None => None,
        Some(Field::Pairs { .. }) => raw
            .split(',')
            .filter(|p| !p.is_empty())
            .map(|p| {
                let (a, b) = p.split_once(':')?;
                Some(serde_json::json!([a.parse::<i64>().ok()?, b.parse::<i64>().ok()?]))
            })
            .collect::<Option<Vec<_>>>()
            .map(J::Array),
    };
    match (field, decoded) {
        (_, Some(v)) => v,
        (Some(Field::OneOf(_)) | None, None) => J::String(raw.to_owned()),
        (Some(_), None) => J::Null,
    }
}

/// `%XX` escapes back to bytes; None when an escape is broken or the text not UTF-8.
fn percent_decode(raw: &str) -> Option<String> {
    let bytes = raw.as_bytes();
    let mut out = Vec::with_capacity(bytes.len());
    let mut i = 0;
    while i < bytes.len() {
        if bytes[i] == b'%' {
            let hex = std::str::from_utf8(bytes.get(i + 1..i + 3)?).ok()?;
            out.push(u8::from_str_radix(hex, 16).ok()?);
            i += 3;
        } else {
            out.push(bytes[i]);
            i += 1;
        }
    }
    String::from_utf8(out).ok()
}

/// The event types a contribution may hold.
pub fn event_types() -> impl Iterator<Item = &'static str> {
    SCHEMAS.iter().map(|s| s.event)
}

pub(crate) fn is_hex(s: &str, len: usize) -> bool {
    s.len() == len && s.bytes().all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b))
}

/// Text without control characters (except line breaks and tabs when `multiline`).
pub(crate) fn clean_text(s: &str, max: usize, multiline: bool) -> bool {
    s.chars().count() <= max
        && s.chars()
            .all(|c| !c.is_control() || (multiline && matches!(c, '\n' | '\r' | '\t')))
}

fn int_in(v: &serde_json::Value, (min, max): (i64, i64)) -> bool {
    v.as_i64().is_some_and(|n| (min..=max).contains(&n))
}

fn check_field(key: &str, field: Field, v: &serde_json::Value) -> Result<()> {
    let ok = match field {
        Field::Int(min, max) => int_in(v, (min, max)),
        Field::Num(min, max) => v.as_f64().is_some_and(|n| n.is_finite() && (min..=max).contains(&n)),
        Field::Bool => v.is_boolean(),
        Field::Text(max) => v.as_str().is_some_and(|s| clean_text(s, max, false)),
        Field::OneOf(values) => v.as_str().is_some_and(|s| values.contains(&s)),
        Field::Pairs { max, a, b } => v.as_array().is_some_and(|list| {
            list.len() <= max
                && list.iter().all(|p| {
                    p.as_array()
                        .is_some_and(|p| p.len() == 2 && int_in(&p[0], a) && int_in(&p[1], b))
                })
        }),
    };
    ensure!(ok, "event data: bad {key}");
    Ok(())
}

/// One recorded quest of a quest cache.
pub fn check_quest(id: u32, q: &CachedQuest) -> Result<()> {
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
    if let Some(xp) = q.xp {
        ensure!(
            (-1..=100).contains(&xp.level) && xp.difficulty <= 10 && (0.05..=20.0).contains(&xp.multiplier),
            "quest {id}: bad XP reward"
        );
    }
    if let Some(info) = &q.info {
        ensure!(
            (-1..=90).contains(&info.min_level) && (-100_000..=100_000).contains(&info.sort),
            "quest {id}: bad level or zone"
        );
        ensure!(
            info.next_quest.is_none_or(|n| (1..=MAX_QUEST_ID).contains(&n)),
            "quest {id}: bad next quest"
        );
        ensure!(
            info.start_item
                .is_none_or(|i| (1..=MAX_ITEM_ID).contains(&i64::from(i))),
            "quest {id}: bad start item"
        );
        ensure!(info.item_drops.len() <= MAX_OBJECTIVES, "quest {id}: too many items");
        for d in &info.item_drops {
            ensure!(
                (1..=MAX_ITEM_ID).contains(&i64::from(d.item)) && (0..10_000).contains(&d.count),
                "quest {id}: bad item"
            );
        }
    }
    Ok(())
}

fn check_quest_items(items: &[u32]) -> bool {
    items.len() <= MAX_QUEST_ITEMS && items.iter().all(|&i| (1..=MAX_ITEM_ID).contains(&i64::from(i)))
}

/// One creature of a creature cache.
pub fn check_creature(id: u32, c: &CachedCreature) -> Result<()> {
    ensure!((1..=MAX_ENTITY_ID).contains(&i64::from(id)), "bad creature id {id}");
    ensure!(
        !c.name.trim().is_empty() && clean_text(&c.name, MAX_NAME, false),
        "creature {id}: bad name"
    );
    ensure!(
        c.classification <= 10 && c.creature_type <= 30 && (-1..=10_000).contains(&c.family),
        "creature {id}: bad kind"
    );
    ensure!(check_quest_items(&c.quest_items), "creature {id}: bad quest items");
    Ok(())
}

/// One game object of a game object cache.
pub fn check_object(id: u32, o: &CachedObject) -> Result<()> {
    ensure!((1..=MAX_ENTITY_ID).contains(&i64::from(id)), "bad object id {id}");
    ensure!(clean_text(&o.name, MAX_NAME, false), "object {id}: bad name");
    ensure!(o.object_type <= 100, "object {id}: bad type");
    ensure!(
        o.loot.is_none_or(|l| (1..=MAX_ENTITY_ID).contains(&i64::from(l))),
        "object {id}: bad loot"
    );
    ensure!(check_quest_items(&o.quest_items), "object {id}: bad quest items");
    Ok(())
}

/// The locale files of a creature or object cache.
fn check_locale_files<T>(files: &[LocaleCache<T>], check: impl Fn(u32, &T) -> Result<()>) -> Result<()> {
    ensure!(files.len() <= MAX_CACHE_FILES, "too many cache files");
    let mut locales = Vec::new();
    for file in files {
        ensure!(LOCALES.contains(&file.locale.as_str()), "unknown locale");
        ensure!(!locales.contains(&&file.locale), "duplicate locale");
        locales.push(&file.locale);
        ensure!((1..=1_000_000).contains(&file.build), "bad client build");
        ensure!(file.entries.len() <= MAX_ENTITIES_PER_FILE, "too many cache records");
        for (&id, r) in &file.entries {
            check(id, r)?;
        }
    }
    Ok(())
}

/// Everything of an event but its time.
fn check_event_body(e: &Event) -> Result<()> {
    let Some(seq) = e.seq else {
        bail!("event without sequence number")
    };
    ensure!((1..=1_000_000_000).contains(&seq), "bad sequence number");
    let Some(schema) = e.event.as_deref().and_then(|ev| SCHEMAS.iter().find(|s| s.event == ev)) else {
        bail!("unknown event type")
    };
    let need = |need: Need, present: bool, what: &str| {
        ensure!(
            match need {
                Need::Always => present,
                Need::Never => !present,
                Need::Optional => true,
            },
            "{} event: {what} {}",
            schema.event,
            if present { "unexpected" } else { "missing" }
        );
        Ok(())
    };
    need(schema.quest, e.quest_id.is_some(), "quest")?;
    need(schema.entity, e.entity_id.is_some(), "entity")?;
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
        let Some(map) = d.as_object() else {
            bail!("event data is not an object")
        };
        ensure!(!map.is_empty(), "empty event data");
        ensure!(d.to_string().len() <= MAX_DATA_BYTES, "event data too large");
        for (k, v) in map {
            let Some(&(_, field)) = schema.fields.iter().find(|(name, _)| name == k) else {
                bail!("{} event: unknown data field", schema.event)
            };
            check_field(k, field, v)?;
        }
    }
    Ok(())
}

/// An event as the addon recorded it (absolute time), before it goes in a contribution: the
/// app drops those that would not pass.
pub fn check_local_event(e: &Event, now: u64) -> Result<()> {
    check_event_body(e)?;
    if let Some(ts) = e.ts {
        ensure!(
            ts >= MIN_TIME as i64 && ts <= (now + CLOCK_SKEW) as i64,
            "bad event time"
        );
    }
    Ok(())
}

/// Check every field of a contribution (`now`: Unix seconds).
pub fn validate(c: &Contribution, now: u64) -> Result<()> {
    ensure!(c.format == FORMAT, "not a Factoruide contribution");
    ensure!(c.version == VERSION, "unsupported contribution version {}", c.version);
    ensure!(EDITIONS.contains(&c.edition.as_str()), "unknown edition");
    ensure!(
        !c.app_version.is_empty()
            && c.app_version.len() <= 32
            && c.app_version
                .bytes()
                .all(|b| b.is_ascii_alphanumeric() || b"+-.".contains(&b)),
        "bad app_version"
    );
    ensure!(is_hex(&c.contributor, 32), "bad contributor id");
    let (first_day, last_day) = ((MIN_TIME / 86_400) as u32, ((now + CLOCK_SKEW) / 86_400) as u32);
    ensure!((first_day..=last_day).contains(&c.day), "bad day");

    ensure!(c.quest_cache.len() <= MAX_CACHE_FILES, "too many quest caches");
    let mut locales = Vec::new();
    for file in &c.quest_cache {
        ensure!(LOCALES.contains(&file.locale.as_str()), "unknown locale");
        ensure!(!locales.contains(&&file.locale), "duplicate locale");
        locales.push(&file.locale);
        ensure!((1..=1_000_000).contains(&file.build), "bad client build");
        ensure!(file.quests.len() <= MAX_QUESTS_PER_FILE, "too many quests");
        for (&id, q) in &file.quests {
            check_quest(id, q)?;
        }
    }
    check_locale_files(&c.creature_cache, check_creature)?;
    check_locale_files(&c.object_cache, check_object)?;

    ensure!(c.collector.len() <= MAX_CHARACTERS, "too many characters");
    let mut ids = Vec::new();
    let mut total = 0;
    // Event times count from the character's first event: never more than the app's lifetime.
    let max_offset = (now + CLOCK_SKEW - MIN_TIME) as i64;
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
        ensure!(
            ch.day.is_some_and(|d| (first_day..=c.day).contains(&d)),
            "bad character day"
        );
        ensure!(ch.events.len() <= MAX_EVENTS_PER_CHARACTER, "too many events");
        total += ch.events.len();
        ensure!(total <= MAX_EVENTS, "too many events");
        let mut last_seq = 0;
        for e in &ch.events {
            check_event_body(e)?;
            let seq = e.seq.unwrap_or_default();
            ensure!(seq > last_seq, "events out of sequence");
            last_seq = seq;
            ensure!(e.ts.is_some_and(|t| (0..=max_offset).contains(&t)), "bad event time");
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::collector::Character;
    use crate::wdb::{CacheFile, Objective, XpReward};

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
            xp: Some(XpReward {
                level: 10,
                difficulty: 5,
                multiplier: 1.0,
            }),
            info: None,
        };
        let accept = Event {
            seq: Some(1),
            ts: Some(0),
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
        let loot = Event {
            seq: Some(2),
            ts: Some(95),
            event: Some("loot".into()),
            quest_id: None,
            entity_type: Some("npc".into()),
            entity_id: Some(116),
            ui_map_id: Some(1436),
            x: Some(50.0),
            y: Some(40.0),
            level: Some(14),
            data: Some(serde_json::json!({"items": [[2589, 2], [1468, 1]]})),
        };
        Contribution {
            format: FORMAT.into(),
            version: VERSION,
            edition: "classic".into(),
            app_version: "0.1.0".into(),
            contributor: "0123456789abcdef0123456789abcdef".into(),
            day: 20_700,
            quest_cache: vec![CacheFile {
                locale: "enUS".into(),
                build: 61582,
                quests: [(155, quest)].into(),
            }],
            creature_cache: vec![LocaleCache {
                locale: "enUS".into(),
                build: 61582,
                entries: [(
                    116,
                    CachedCreature {
                        name: "Defias Bandit".into(),
                        classification: 0,
                        creature_type: 7,
                        family: 0,
                        quest_items: vec![2589],
                    },
                )]
                .into(),
            }],
            object_cache: Vec::new(),
            collector: vec![Character {
                character: "0123456789abcdef".into(),
                race: Some("Human".into()),
                class: Some("WARRIOR".into()),
                day: Some(20_699),
                account: None,
                events: vec![accept, loot],
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
        assert!(rejected(|c| c.version = 2));
        assert!(rejected(|c| c.edition = "retail".into()));
        assert!(rejected(|c| c.contributor = "../../etc/passwd".into()));
        assert!(rejected(|c| c.app_version = "<script>".into()));
        assert!(rejected(|c| c.day = (NOW / 86_400) as u32 + 10));
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
        assert!(rejected(|c| c.quest_cache[0].quests.get_mut(&155).unwrap().xp =
            Some(XpReward {
                level: 10,
                difficulty: 5,
                multiplier: f32::NAN,
            })));
        assert!(rejected(
            |c| c.creature_cache[0].entries.get_mut(&116).unwrap().name = "a\u{7}".into()
        ));
        assert!(rejected(|c| c.creature_cache[0]
            .entries
            .get_mut(&116)
            .unwrap()
            .quest_items = vec![0]));
        assert!(rejected(|c| c.collector[0].character = "Brouz-Realm".into()));
        assert!(rejected(|c| c.collector[0].day = None));
        assert!(rejected(|c| c.collector[0].events[0].event = Some("hack".into())));
        assert!(rejected(|c| c.collector[0].events[0].x = Some(f64::NAN)));
        assert!(rejected(|c| c.collector[0].events[0].x = Some(150.0)));
        assert!(rejected(|c| c.collector[0].events[0].quest_id = Some(-3)));
        assert!(rejected(|c| c.collector[0].events[0].ts = Some(1_790_000_000)));
        assert!(rejected(|c| c.collector[0].events[0].ts = Some(-1)));
        assert!(rejected(|c| {
            let e = c.collector[0].events[0].clone();
            c.collector[0].events.push(e);
        }));
    }

    #[test]
    fn event_data_follows_its_schema() {
        let data = |c: &mut Contribution, d| c.collector[0].events[1].data = Some(d);
        // Unknown key, wrong type, out of bounds, too long a list, nested free data.
        assert!(rejected(|c| data(
            c,
            serde_json::json!({"items": [[2589, 2]], "note": "hi"})
        )));
        assert!(rejected(|c| data(c, serde_json::json!({"items": "2589"}))));
        assert!(rejected(|c| data(c, serde_json::json!({"items": [[2589, 0]]}))));
        assert!(rejected(|c| data(c, serde_json::json!({"items": vec![[1, 1]; 17]}))));
        assert!(rejected(|c| data(c, serde_json::json!({"items": [[2589, {"a": 1}]]}))));
        assert!(rejected(|c| data(c, serde_json::json!({}))));
        // A loot always names its source; a level event never has a quest.
        assert!(rejected(|c| {
            c.collector[0].events[1].entity_type = None;
            c.collector[0].events[1].entity_id = None;
        }));
        assert!(rejected(|c| {
            let e = &mut c.collector[0].events[1];
            e.event = Some("level".into());
            e.entity_type = None;
            e.entity_id = None;
            e.quest_id = Some(155);
            e.data = Some(serde_json::json!({"level": 15}));
        }));
    }

    #[test]
    fn a_local_event_keeps_its_absolute_time() {
        let mut e = sample().collector[0].events[0].clone();
        e.ts = Some(1_790_000_000);
        check_local_event(&e, NOW).unwrap();
        e.ts = Some(5);
        assert!(check_local_event(&e, NOW).is_err());
    }
}
