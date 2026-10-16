//! A player's contribution: what their game client and the addon saw, as one JSON document
//! sent by the app and imported into the quest database.
//!
//! Anonymous by construction: no character name nor anything derived from it (each character
//! gets a random id, kept on this computer only), no time of day (event times count from the
//! character's first event in the document, the date is only the day), no free text typed by a
//! player (only game texts: quest titles, NPC names). Only what is new since the last accepted
//! contribution is sent (`SendState`).

use crate::collector::Character;
use crate::wdb::{CacheFile, CreatureCache, ObjectCache};
use anyhow::Result;
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;

pub const FORMAT: &str = "factoruide-contribution";
/// 3: anonymous characters, relative event times, typed event data, incremental documents.
pub const VERSION: u32 = 3;

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Contribution {
    pub format: String,
    pub version: u32,
    /// Game version (forever, classic, tbc).
    pub edition: String,
    pub app_version: String,
    /// Random id of the app install (32 hex digits), the same for all its contributions: tells
    /// how many different players back a fact.
    pub contributor: String,
    /// Day the document was made (days since 1970-01-01).
    pub day: u32,
    /// The client's quest cache, per locale: the quests new or changed since the last send.
    pub quest_cache: Vec<CacheFile>,
    /// The client's creature cache, per locale: the creatures new or changed since the last send.
    #[serde(default)]
    pub creature_cache: Vec<CreatureCache>,
    /// The client's game object cache, per locale, likewise.
    #[serde(default)]
    pub object_cache: Vec<ObjectCache>,
    /// The addon's recordings since the last send, characters anonymized.
    pub collector: Vec<Character>,
}

impl Contribution {
    pub fn quests(&self) -> usize {
        let mut ids: Vec<u32> = self.quest_cache.iter().flat_map(|f| f.quests.keys().copied()).collect();
        ids.sort_unstable();
        ids.dedup();
        ids.len()
    }

    pub fn events(&self) -> usize {
        self.collector.iter().map(|c| c.events.len()).sum()
    }

    /// Creatures and game objects from the client's caches.
    pub fn entities(&self) -> usize {
        let count = |n: &mut Vec<u32>, ids: &mut dyn Iterator<Item = u32>| {
            n.extend(ids);
            n.sort_unstable();
            n.dedup();
            n.len()
        };
        count(
            &mut Vec::new(),
            &mut self.creature_cache.iter().flat_map(|f| f.entries.keys().copied()),
        ) + count(
            &mut Vec::new(),
            &mut self.object_cache.iter().flat_map(|f| f.entries.keys().copied()),
        )
    }

    pub fn is_empty(&self) -> bool {
        self.quests() == 0 && self.events() == 0 && self.entities() == 0
    }
}

/// What was already sent from this computer, kept by the app next to its settings and only
/// updated once the server accepted a contribution. Holds character names: never sent.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct SendState {
    /// "Name-Realm" → its random id and the last event sent.
    #[serde(default)]
    pub characters: BTreeMap<String, SentCharacter>,
    /// Cache records sent, per cache file ("quest:enUS"): record id → fingerprint of its content.
    #[serde(default)]
    pub records: BTreeMap<String, BTreeMap<u32, u64>>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct SentCharacter {
    /// Random, 16 hex digits.
    pub id: String,
    /// Sequence number of the last event sent.
    pub last_seq: i64,
}

/// FNV-1a 64 bits: fingerprint of a cache record, to send it again only when it changed.
pub fn fingerprint(bytes: &[u8]) -> u64 {
    let mut h: u64 = 0xcbf29ce484222325;
    for b in bytes {
        h ^= u64::from(*b);
        h = h.wrapping_mul(0x100000001b3);
    }
    h
}

/// Random lowercase hex id of `bytes` bytes.
#[cfg(feature = "local")]
pub fn random_hex(bytes: usize) -> Result<String> {
    use std::fmt::Write;
    let mut buf = vec![0u8; bytes];
    getrandom::fill(&mut buf).map_err(|e| anyhow::anyhow!("no random source: {e}"))?;
    Ok(buf.iter().fold(String::with_capacity(bytes * 2), |mut s, b| {
        let _ = write!(s, "{b:02x}");
        s
    }))
}

/// Unix seconds now.
pub(crate) fn now() -> Result<u64> {
    Ok(std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)?
        .as_secs())
}

/// Events one document holds at most (about 5 MB): what is left goes in the next one.
pub const MAX_DOCUMENT_EVENTS: usize = 20_000;
/// Records of one cache file a document holds at most.
pub const MAX_DOCUMENT_RECORDS: usize = 20_000;

/// Key of a character in `SendState`: its account folder too, as two accounts may hold
/// characters of the same name (each with its own event numbers).
pub fn state_key(c: &Character) -> String {
    match &c.account {
        Some(account) => format!("{account}/{}", c.character),
        None => c.character.clone(),
    }
}

impl SendState {
    /// Last event sent of each "Name-Realm" for the addon to drop, leaving out a name found in
    /// several accounts (the addon cannot tell their events apart: it keeps them).
    pub fn sent_by_name(&self) -> BTreeMap<String, i64> {
        let mut names: BTreeMap<String, Option<i64>> = BTreeMap::new();
        for (key, sent) in &self.characters {
            let name = key.rsplit('/').next().unwrap_or(key).to_owned();
            names
                .entry(name)
                .and_modify(|v| *v = None)
                .or_insert(Some(sent.last_seq));
        }
        names.into_iter().filter_map(|(n, v)| Some((n, v?))).collect()
    }
}

/// Turn locally read characters into anonymous ones holding only what `state` has not seen:
/// random ids instead of names, events after the last one sent (at most `max_events` in all,
/// oldest first), times relative to the first event kept, events failing validation dropped
/// (one odd row never blocks a contribution). Returns the characters, the state to keep once
/// they are accepted, and whether events were left for a next document.
#[cfg(feature = "local")]
pub fn anonymize(
    characters: Vec<Character>,
    state: &SendState,
    now: u64,
    max_events: usize,
) -> Result<(Vec<Character>, SendState, bool)> {
    let mut next = state.clone();
    let mut out = Vec::new();
    let mut room = max_events;
    let mut more = false;
    let today = (now / 86_400) as i64;
    for mut c in characters {
        let key = state_key(&c);
        let sent = state.characters.get(&key);
        let last_seq = sent.map_or(0, |s| s.last_seq);
        c.events.retain(|e| {
            e.seq.is_some_and(|s| s > last_seq) && e.ts.is_some() && crate::validate::check_local_event(e, now).is_ok()
        });
        c.events.sort_by_key(|e| e.seq);
        if c.events.len() > room {
            c.events.truncate(room);
            more = true;
        }
        room -= c.events.len();
        let Some(max_seq) = c.events.last().and_then(|e| e.seq) else {
            continue;
        };
        let id = match sent {
            Some(s) => s.id.clone(),
            None => random_hex(8)?,
        };
        next.characters.insert(
            key,
            SentCharacter {
                id: id.clone(),
                last_seq: max_seq,
            },
        );
        let base = c.events.iter().filter_map(|e| e.ts).min().unwrap_or(now as i64);
        for e in &mut c.events {
            e.ts = e.ts.map(|t| t - base);
        }
        c.character = id;
        c.account = None;
        // A clock a bit ahead must not date the character after the document.
        c.day = Some((base / 86_400).min(today) as u32);
        out.push(c);
    }
    // A document's order must not tell which character is which across sends.
    out.sort_by(|a, b| a.character.cmp(&b.character));
    Ok((out, next, more))
}

/// Keep the cache records not sent yet (or changed since), at most `max` of them, and note
/// those in `next`. Returns whether records were left for a next document.
pub fn new_records<T: Serialize>(
    key: &str,
    records: &mut BTreeMap<u32, T>,
    state: &SendState,
    next: &mut SendState,
    max: usize,
) -> bool {
    let sent = state.records.get(key);
    let noted = next.records.entry(key.to_owned()).or_default();
    let mut kept = 0;
    let mut more = false;
    records.retain(|id, r| {
        let print = fingerprint(&serde_json::to_vec(r).unwrap_or_default());
        if sent.and_then(|s| s.get(id)) == Some(&print) {
            return false;
        }
        if kept == max {
            more = true;
            return false;
        }
        kept += 1;
        noted.insert(*id, print);
        true
    });
    more
}

/// What is new in a game client folder since `state`, in one contribution within the server's
/// limits: the contribution, the state to keep once the server accepted it, and whether more
/// is left for a next one.
#[cfg(feature = "local")]
pub fn collect(
    client_dir: &std::path::Path,
    edition: &str,
    app_version: &str,
    contributor: &str,
    state: &SendState,
) -> Result<(Contribution, SendState, bool)> {
    use crate::{collector, wdb};
    let now = now()?;
    let characters = collector::read(&collector::find_saved_variables(client_dir))?;
    let (collector, mut next, mut more) = anonymize(characters, state, now, MAX_DOCUMENT_EVENTS)?;
    let mut quest_cache = wdb::read_cache(client_dir);
    for file in &mut quest_cache {
        file.quests.retain(|&id, q| crate::validate::check_quest(id, q).is_ok());
        let key = format!("quest:{}", file.locale);
        more |= new_records(&key, &mut file.quests, state, &mut next, MAX_DOCUMENT_RECORDS);
    }
    quest_cache.retain(|f| !f.quests.is_empty());
    let mut creature_cache = wdb::read_creature_cache(client_dir);
    for file in &mut creature_cache {
        file.entries
            .retain(|&id, c| crate::validate::check_creature(id, c).is_ok());
        let key = format!("creature:{}", file.locale);
        more |= new_records(&key, &mut file.entries, state, &mut next, MAX_DOCUMENT_RECORDS);
    }
    creature_cache.retain(|f| !f.entries.is_empty());
    let mut object_cache = wdb::read_object_cache(client_dir);
    for file in &mut object_cache {
        file.entries
            .retain(|&id, o| crate::validate::check_object(id, o).is_ok());
        let key = format!("object:{}", file.locale);
        more |= new_records(&key, &mut file.entries, state, &mut next, MAX_DOCUMENT_RECORDS);
    }
    object_cache.retain(|f| !f.entries.is_empty());
    let contribution = Contribution {
        format: FORMAT.into(),
        version: VERSION,
        edition: edition.into(),
        app_version: app_version.into(),
        contributor: contributor.into(),
        day: (now / 86_400) as u32,
        quest_cache,
        creature_cache,
        object_cache,
        collector,
    };
    // What the server would refuse must not leave: it would be refused at every try.
    crate::validate::validate(&contribution, now)?;
    anyhow::ensure!(
        serde_json::to_vec(&contribution)?.len() <= crate::validate::MAX_BYTES / 2,
        "contribution too large"
    );
    Ok((contribution, next, more))
}

/// Read a contribution document and check every field of it (`validate`): what comes from
/// outside is never trusted, whoever reads it (server, import).
pub fn parse(json: &[u8]) -> Result<Contribution> {
    anyhow::ensure!(json.len() <= crate::validate::MAX_BYTES, "contribution too large");
    let c: Contribution = serde_json::from_slice(json)
        // serde's messages may quote the document: only say where it went wrong.
        .map_err(|e| anyhow::anyhow!("invalid contribution document (line {}, column {})", e.line(), e.column()))?;
    crate::validate::validate(&c, now()?)?;
    Ok(c)
}

#[cfg(all(test, feature = "local"))]
mod tests {
    use super::*;
    use crate::collector::Event;

    fn event(seq: i64, ts: i64) -> Event {
        Event {
            seq: Some(seq),
            ts: Some(ts),
            event: Some("kill".into()),
            quest_id: None,
            entity_type: Some("npc".into()),
            entity_id: Some(6),
            ui_map_id: Some(1429),
            x: Some(40.0),
            y: Some(60.0),
            level: Some(5),
            data: None,
        }
    }

    fn character(name: &str, events: Vec<Event>) -> Character {
        Character {
            character: name.into(),
            race: Some("Human".into()),
            class: Some("WARRIOR".into()),
            day: None,
            account: Some("ACCOUNT".into()),
            events,
        }
    }

    const NOW: u64 = 1_800_000_000;

    #[test]
    fn characters_lose_their_name_and_time_of_day() {
        let (out, next, _) = anonymize(
            vec![character(
                "Brouz-Realm",
                vec![event(1, 1_790_000_100), event(2, 1_790_000_160)],
            )],
            &SendState::default(),
            NOW,
            MAX_DOCUMENT_EVENTS,
        )
        .unwrap();
        let c = &out[0];
        assert_ne!(c.character, "Brouz-Realm");
        assert_eq!(c.character.len(), 16);
        assert_eq!(c.events.iter().map(|e| e.ts).collect::<Vec<_>>(), [Some(0), Some(60)]);
        assert_eq!(c.day, Some(1_790_000_100 / 86_400));
        assert_eq!(next.characters["ACCOUNT/Brouz-Realm"].id, c.character);
        assert_eq!(next.characters["ACCOUNT/Brouz-Realm"].last_seq, 2);
    }

    #[test]
    fn only_events_after_the_last_send_go_and_the_id_stays() {
        let first = anonymize(
            vec![character("Brouz-Realm", vec![event(1, 1_790_000_000)])],
            &SendState::default(),
            NOW,
            MAX_DOCUMENT_EVENTS,
        )
        .unwrap();
        let (out, next, _) = anonymize(
            vec![character(
                "Brouz-Realm",
                vec![event(1, 1_790_000_000), event(2, 1_790_000_050)],
            )],
            &first.1,
            NOW,
            MAX_DOCUMENT_EVENTS,
        )
        .unwrap();
        assert_eq!(out[0].events.len(), 1);
        assert_eq!(out[0].events[0].seq, Some(2));
        assert_eq!(out[0].character, first.0[0].character);
        assert_eq!(next.characters["ACCOUNT/Brouz-Realm"].last_seq, 2);
        // Nothing new: the character is left out.
        let (out, _, _) = anonymize(
            vec![character(
                "Brouz-Realm",
                vec![event(1, 1_790_000_000), event(2, 1_790_000_050)],
            )],
            &next,
            NOW,
            MAX_DOCUMENT_EVENTS,
        )
        .unwrap();
        assert!(out.is_empty());
    }

    #[test]
    fn an_odd_event_is_dropped_not_the_contribution() {
        let mut bad = event(2, 1_790_000_010);
        bad.event = Some("hack".into());
        let (out, _, _) = anonymize(
            vec![character(
                "Brouz-Realm",
                vec![event(1, 1_790_000_000), bad, event(3, 1_790_000_020)],
            )],
            &SendState::default(),
            NOW,
            MAX_DOCUMENT_EVENTS,
        )
        .unwrap();
        assert_eq!(
            out[0].events.iter().map(|e| e.seq).collect::<Vec<_>>(),
            [Some(1), Some(3)]
        );
    }

    #[test]
    fn a_document_stays_within_its_limit_and_the_rest_follows() {
        let events = (1..=5).map(|i| event(i, 1_790_000_000 + i)).collect::<Vec<_>>();
        let (out, next, more) = anonymize(
            vec![character("Brouz-Realm", events.clone())],
            &SendState::default(),
            NOW,
            3,
        )
        .unwrap();
        assert!(more);
        assert_eq!(out[0].events.len(), 3);
        assert_eq!(next.characters["ACCOUNT/Brouz-Realm"].last_seq, 3);
        let (out, _, more) = anonymize(vec![character("Brouz-Realm", events)], &next, NOW, 3).unwrap();
        assert!(!more);
        assert_eq!(
            out[0].events.iter().map(|e| e.seq).collect::<Vec<_>>(),
            [Some(4), Some(5)]
        );
    }

    #[test]
    fn a_name_in_two_accounts_is_two_characters_the_addon_does_not_purge() {
        let mut other = character("Brouz-Realm", vec![event(1, 1_790_000_000)]);
        other.account = Some("OTHER".into());
        let (out, next, _) = anonymize(
            vec![
                character("Brouz-Realm", vec![event(1, 1_790_000_000), event(2, 1_790_000_001)]),
                other,
            ],
            &SendState::default(),
            NOW,
            MAX_DOCUMENT_EVENTS,
        )
        .unwrap();
        assert_eq!(out.len(), 2);
        assert_ne!(out[0].character, out[1].character);
        assert!(!next.sent_by_name().contains_key("Brouz-Realm"));
        let (_, alone, _) = anonymize(
            vec![character("Solo-Realm", vec![event(7, 1_790_000_000)])],
            &SendState::default(),
            NOW,
            MAX_DOCUMENT_EVENTS,
        )
        .unwrap();
        assert_eq!(alone.sent_by_name()["Solo-Realm"], 7);
    }

    #[test]
    fn cache_records_are_sent_again_only_when_they_change() {
        let mut next = SendState::default();
        let mut records: BTreeMap<u32, String> = [(1, "a".into()), (2, "b".into())].into();
        new_records("quest:enUS", &mut records, &SendState::default(), &mut next, 10);
        assert_eq!(records.len(), 2);
        let state = next.clone();
        let mut records: BTreeMap<u32, String> = [(1, "a".into()), (2, "c".into()), (3, "d".into())].into();
        new_records("quest:enUS", &mut records, &state, &mut next, 10);
        assert_eq!(records.keys().copied().collect::<Vec<_>>(), [2, 3]);
    }
}

/// A real contribution of each game client passes validation: `FG_WOW_DIR=<World of Warcraft
/// folder> cargo test -p fg-contrib -- --ignored --nocapture real_contributions`.
#[cfg(all(test, feature = "local"))]
mod real {
    #[test]
    #[ignore = "needs game clients (FG_WOW_DIR)"]
    fn real_contributions() {
        let root = std::path::PathBuf::from(std::env::var("FG_WOW_DIR").expect("FG_WOW_DIR"));
        for dir in std::fs::read_dir(&root).unwrap().flatten().map(|e| e.path()) {
            if !dir.join("WTF").is_dir() {
                continue;
            }
            let (c, next, _) = super::collect(
                &dir,
                "forever",
                "0.1.0",
                "0123456789abcdef0123456789abcdef",
                &super::SendState::default(),
            )
            .unwrap();
            let json = serde_json::to_vec(&c).unwrap();
            super::parse(&json).unwrap();
            let (again, _, _) =
                super::collect(&dir, "forever", "0.1.0", "0123456789abcdef0123456789abcdef", &next).unwrap();
            println!(
                "{}: {} bytes, {} quests, {} creatures/objects, {} events of {} characters; then {} quests, {} \
                 entities, {} events",
                dir.display(),
                json.len(),
                c.quests(),
                c.entities(),
                c.events(),
                c.collector.len(),
                again.quests(),
                again.entities(),
                again.events()
            );
        }
    }
}
