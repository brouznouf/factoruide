//! Test worlds declared in TOML files: a test names the files of the world it needs (a few quests,
//! the NPCs and items they use) and gets a database of the game's own shape, empty but for them.
//!
//! ```toml
//! [[npc]]
//! id = 1
//! name = "Giver"
//! at = { zone = 14, x = 45.0, y = 60.0 }
//!
//! [[quest]]
//! id = 100
//! name = "Kill the beasts"
//! level = 3
//! giver = 1                                   # starts and ends at the NPC 1
//! objectives = [{ creature = 2, amount = 6 }]
//! ```
//!
//! The database starts from `tests/fixtures/base-<edition>.sqlite.zst` (the real schema, the game's
//! classes, races and zones, the races' start NPCs: `scripts/test-base.sh`). Anything a file does
//! not have a field for is a raw `[[row]]` (`table = "m_npc"`, `values = { id = 1, ... }`).

use crate::xp::Edition;
use anyhow::{Context, Result, bail};
use rusqlite::{Connection, params, types::Value};
use serde::Deserialize;
use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::sync::OnceLock;

/// A place: a zone (AreaTable ID) and its percent coordinates, `z` the floor's height if any.
#[derive(Debug, Clone, Copy, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct At {
    pub zone: i64,
    pub x: f64,
    pub y: f64,
    pub z: Option<i64>,
}

/// An entity of the database: an NPC, an object or an item (one field).
#[derive(Debug, Clone, Copy, Deserialize, Default)]
#[serde(deny_unknown_fields)]
struct Ent {
    npc: Option<i64>,
    object: Option<i64>,
    item: Option<i64>,
}

impl Ent {
    fn kind(self) -> Result<(&'static str, i64)> {
        match (self.npc, self.object, self.item) {
            (Some(id), None, None) => Ok(("npc", id)),
            (None, Some(id), None) => Ok(("object", id)),
            (None, None, Some(id)) => Ok(("item", id)),
            _ => bail!("an entity names one of npc, object, item"),
        }
    }
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct Npc {
    id: i64,
    name: Option<String>,
    subname: Option<String>,
    #[serde(default = "one")]
    level: i64,
    max_level: Option<i64>,
    #[serde(default)]
    rank: i64,
    /// Where it stands (several places for a wanderer or a camp).
    #[serde(default)]
    at: Vec<At>,
    /// The game's NPC flags (vendor, trainer, flight master...).
    #[serde(default, rename = "npc_flags")]
    flags: i64,
    #[serde(default)]
    faction: i64,
    /// The sides it talks to: A, H, AH.
    friendly_to: Option<String>,
    respawn: Option<f64>,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct Object {
    id: i64,
    name: Option<String>,
    #[serde(default)]
    at: Vec<At>,
    respawn: Option<f64>,
}

/// What a quest asks for: one of `creature`, `item`, `object`, `talk` (an ID), or a `trigger` text
/// with its place.
#[derive(Debug, Deserialize, Default)]
#[serde(deny_unknown_fields)]
struct Objective {
    creature: Option<i64>,
    item: Option<i64>,
    object: Option<i64>,
    talk: Option<i64>,
    trigger: Option<String>,
    /// The step's words, when the quest has its own.
    text: Option<String>,
    at: Option<At>,
    amount: Option<i64>,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct Reward {
    item: i64,
    #[serde(default = "one")]
    count: i64,
    /// Among the rewards to choose from.
    #[serde(default)]
    choice: bool,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct Quest {
    id: i64,
    name: Option<String>,
    #[serde(default = "one")]
    level: i64,
    min_level: Option<i64>,
    max_level: Option<i64>,
    races_mask: Option<i64>,
    classes_mask: Option<i64>,
    /// The zone it belongs to, or a category.
    zone: Option<i64>,
    xp: Option<i64>,
    /// The NPC that starts and ends it.
    giver: Option<i64>,
    #[serde(default)]
    starts: Vec<Ent>,
    #[serde(default)]
    ends: Vec<Ent>,
    #[serde(default)]
    objectives: Vec<Objective>,
    #[serde(default)]
    rewards: Vec<Reward>,
    #[serde(default)]
    pre_all: Vec<i64>,
    #[serde(default)]
    pre_any: Vec<i64>,
    #[serde(default)]
    exclusive: Vec<i64>,
    #[serde(default)]
    child: Vec<i64>,
    breadcrumb_for: Option<i64>,
    parent: Option<i64>,
    next: Option<i64>,
    reward_spell: Option<i64>,
    time_limit: Option<i64>,
    source_item: Option<i64>,
    required_skill: Option<i64>,
    required_spell: Option<i64>,
    #[serde(rename = "quest_flags")]
    flags: Option<i64>,
    special_flags: Option<i64>,
    /// `available` (default), `removed` or `incomplete`.
    status: Option<String>,
    /// What the quest asks, in words.
    objectives_text: Option<String>,
    /// The JSON the sources add to the quest (an escort's `trigger_end`).
    extra: Option<String>,
    /// The JSON of what the quest requires.
    requirements: Option<String>,
}

/// Where an item comes from: dropped by an NPC, an object or an item, or sold by a vendor (its
/// stock is a `[[vendor]]`).
#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct Drop {
    npc: Option<i64>,
    object: Option<i64>,
    item: Option<i64>,
    vendor: Option<i64>,
    /// Missing: the sources give none.
    chance: Option<f64>,
}

impl Drop {
    fn from(&self) -> Result<(&'static str, i64)> {
        match (self.npc, self.object, self.item, self.vendor) {
            (Some(id), None, None, None) => Ok(("npc", id)),
            (None, Some(id), None, None) => Ok(("object", id)),
            (None, None, Some(id), None) => Ok(("item", id)),
            (None, None, None, Some(id)) => Ok(("vendor", id)),
            _ => bail!("a source names one of npc, object, item, vendor"),
        }
    }
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct Item {
    id: i64,
    name: Option<String>,
    #[serde(default = "one")]
    level: i64,
    /// The game's item class (12 quest items, 15 miscellaneous: not sold at the auction house); 0
    /// when not given, 12 for an item that starts a quest.
    class: Option<i64>,
    /// The quest it starts.
    start_quest: Option<i64>,
    #[serde(default)]
    drops: Vec<Drop>,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct Stock {
    item: i64,
    count: Option<i64>,
    restock: Option<f64>,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct Vendor {
    npc: i64,
    items: Vec<Stock>,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct Dungeon {
    area: i64,
    name: String,
    /// Where its door is.
    at: At,
}

/// A row of any table, columns by name.
#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct Row {
    table: String,
    values: BTreeMap<String, toml::Value>,
}

/// Tiles of the terrain pack (`terrain-<edition>.sqlite.zst`, `scripts/test-terrain.sh`): the part
/// of a map a walk needs.
#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct TerrainRegion {
    map: i64,
    tile_x: [i64; 2],
    tile_y: [i64; 2],
}

#[derive(Debug, Deserialize, Default)]
#[serde(deny_unknown_fields)]
struct File {
    #[serde(default)]
    terrain: Vec<TerrainRegion>,
    #[serde(default)]
    npc: Vec<Npc>,
    #[serde(default)]
    object: Vec<Object>,
    #[serde(default)]
    quest: Vec<Quest>,
    #[serde(default)]
    item: Vec<Item>,
    #[serde(default)]
    vendor: Vec<Vendor>,
    #[serde(default)]
    dungeon: Vec<Dungeon>,
    #[serde(default)]
    row: Vec<Row>,
}

fn one() -> i64 {
    1
}

/// The folder of the fixtures' files.
pub fn folder() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures")
}

/// The database of an edition's test base, as bytes (unpacked once).
fn base(edition: Edition) -> &'static [u8] {
    static BASES: [OnceLock<Vec<u8>>; 3] = [OnceLock::new(), OnceLock::new(), OnceLock::new()];
    let slot = match edition {
        Edition::Classic => 0,
        Edition::Tbc => 1,
        Edition::Forever => 2,
    };
    BASES[slot].get_or_init(|| {
        let path = folder().join(format!("base-{}.sqlite.zst", edition.key()));
        let file = std::fs::File::open(&path).unwrap_or_else(|e| panic!("{}: {e}", path.display()));
        zstd::decode_all(file).unwrap_or_else(|e| panic!("{}: {e}", path.display()))
    })
}

/// The terrain pack of an edition, as bytes (unpacked once).
fn terrain_pack(edition: Edition) -> &'static [u8] {
    static PACKS: [OnceLock<Vec<u8>>; 3] = [OnceLock::new(), OnceLock::new(), OnceLock::new()];
    let slot = match edition {
        Edition::Classic => 0,
        Edition::Tbc => 1,
        Edition::Forever => 2,
    };
    PACKS[slot].get_or_init(|| {
        let path = folder().join(format!("terrain-{}.sqlite.zst", edition.key()));
        let file = std::fs::File::open(&path).unwrap_or_else(|e| panic!("{}: {e}", path.display()));
        zstd::decode_all(file).unwrap_or_else(|e| panic!("{}: {e}", path.display()))
    })
}

/// The files of a world, then its database.
pub struct Fixture {
    edition: Edition,
    files: Vec<(String, String)>,
}

impl Fixture {
    pub fn new(edition: Edition) -> Self {
        Self {
            edition,
            files: Vec::new(),
        }
    }

    /// A file of `tests/fixtures/` (its name without `.toml`).
    #[must_use]
    pub fn file(mut self, name: &str) -> Self {
        let path = folder().join(format!("{name}.toml"));
        let text = std::fs::read_to_string(&path).unwrap_or_else(|e| panic!("{}: {e}", path.display()));
        self.files.push((name.to_owned(), text));
        self
    }

    /// Declarations given in the test itself.
    #[must_use]
    pub fn toml(mut self, text: &str) -> Self {
        self.files.push(("inline".to_owned(), text.to_owned()));
        self
    }

    /// The database: the base, then every file's rows.
    pub fn open(self) -> Connection {
        self.try_open().unwrap_or_else(|e| panic!("{e:#}"))
    }

    pub fn try_open(self) -> Result<Connection> {
        // The database of the same files is built once: the next ones are copies of it.
        static BUILT: std::sync::Mutex<BTreeMap<String, std::sync::Arc<Vec<u8>>>> =
            std::sync::Mutex::new(BTreeMap::new());
        let key = format!(
            "{}\n{}",
            self.edition.key(),
            self.files.iter().fold(String::new(), |mut all, (n, t)| {
                all.push_str(n);
                all.push('\n');
                all.push_str(t);
                all.push('\n');
                all
            })
        );
        let cached = BUILT
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .get(&key)
            .cloned();
        let mut conn = Connection::open_in_memory()?;
        if let Some(bytes) = cached {
            conn.deserialize_read_exact(rusqlite::MAIN_DB, &mut &bytes[..], bytes.len(), false)?;
        } else {
            let bytes = base(self.edition);
            conn.deserialize_read_exact(rusqlite::MAIN_DB, &mut &bytes[..], bytes.len(), false)?;
            for (name, text) in &self.files {
                let file: File = toml::from_str(text).with_context(|| format!("fixture {name}"))?;
                insert(&conn, self.edition, &file).with_context(|| format!("fixture {name}"))?;
            }
            let data = conn.serialize(rusqlite::MAIN_DB)?.to_vec();
            BUILT
                .lock()
                .unwrap_or_else(std::sync::PoisonError::into_inner)
                .insert(key, std::sync::Arc::new(data));
        }
        // `FG_SLOW=5` prints the statements that take 5 ms or more.
        if let Some(ms) = std::env::var("FG_SLOW").ok().and_then(|v| v.parse::<u128>().ok()) {
            static LIMIT: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);
            LIMIT.store(ms as u64, std::sync::atomic::Ordering::Relaxed);
            let _ = conn.trace_v2(
                rusqlite::trace::TraceEventCodes::SQLITE_TRACE_PROFILE,
                Some(|event: rusqlite::trace::TraceEvent<'_>| {
                    if let rusqlite::trace::TraceEvent::Profile(statement, took) = event
                        && took.as_millis() >= u128::from(LIMIT.load(std::sync::atomic::Ordering::Relaxed))
                    {
                        let line: String = statement.sql().split_whitespace().collect::<Vec<_>>().join(" ");
                        eprintln!(
                            "SLOW {:>4} ms  {}",
                            took.as_millis(),
                            line.chars().take(150).collect::<String>()
                        );
                    }
                }),
            );
        }
        // `FG_TRACE=1` prints every statement the code under test runs on it.
        if std::env::var_os("FG_TRACE").is_some() {
            let _ = conn.trace_v2(
                rusqlite::trace::TraceEventCodes::SQLITE_TRACE_STMT,
                Some(|event: rusqlite::trace::TraceEvent<'_>| {
                    if let rusqlite::trace::TraceEvent::Stmt(_, sql) = event {
                        let line: String = sql.split_whitespace().collect::<Vec<_>>().join(" ");
                        eprintln!("SQL {}", line.chars().take(200).collect::<String>());
                    }
                }),
            );
        }
        Ok(conn)
    }
}

fn text(s: Option<&String>, fallback: impl FnOnce() -> String) -> String {
    s.cloned().unwrap_or_else(fallback)
}

fn spawn(conn: &Connection, kind: &str, id: i64, at: &At, respawn: Option<f64>) -> Result<()> {
    conn.execute(
        "INSERT INTO m_spawn (entity_type, entity_id, zone_id, x, y, seen, sources, z) VALUES (?1, ?2, ?3, ?4, ?5, 1, 'fixture', ?6)",
        params![kind, id, at.zone, at.x, at.y, at.z],
    )?;
    if let Some(seconds) = respawn {
        conn.execute(
            "INSERT OR REPLACE INTO m_respawn (entity_type, entity_id, seconds) VALUES (?1, ?2, ?3)",
            params![kind, id, seconds],
        )?;
    }
    Ok(())
}

fn insert(conn: &Connection, edition: Edition, file: &File) -> Result<()> {
    if !file.terrain.is_empty() {
        let bytes = terrain_pack(edition);
        let mut pack = Connection::open_in_memory()?;
        pack.deserialize_read_exact(rusqlite::MAIN_DB, &mut &bytes[..], bytes.len(), true)?;
        for r in &file.terrain {
            let range = params![r.map, r.tile_x[0], r.tile_x[1], r.tile_y[0], r.tile_y[1]];
            let mut layers = pack.prepare(
                "SELECT map_id, tile_x, tile_y, version, floors, data FROM client_terrain_layers
                 WHERE map_id = ?1 AND tile_x BETWEEN ?2 AND ?3 AND tile_y BETWEEN ?4 AND ?5",
            )?;
            let mut rows = layers.query(range)?;
            while let Some(row) = rows.next()? {
                conn.execute(
                    "INSERT OR IGNORE INTO client_terrain_layers VALUES (?1, ?2, ?3, ?4, ?5, ?6)",
                    params![
                        row.get::<_, i64>(0)?,
                        row.get::<_, i64>(1)?,
                        row.get::<_, i64>(2)?,
                        row.get::<_, i64>(3)?,
                        row.get::<_, i64>(4)?,
                        row.get::<_, Vec<u8>>(5)?
                    ],
                )?;
            }
            let mut gates = pack.prepare(
                "SELECT map_id, tile_x, tile_y, version, graph FROM client_terrain_gates
                 WHERE map_id = ?1 AND tile_x BETWEEN ?2 AND ?3 AND tile_y BETWEEN ?4 AND ?5",
            )?;
            let mut rows = gates.query(range)?;
            while let Some(row) = rows.next()? {
                conn.execute(
                    "INSERT OR IGNORE INTO client_terrain_gates VALUES (?1, ?2, ?3, ?4, ?5)",
                    params![
                        row.get::<_, i64>(0)?,
                        row.get::<_, i64>(1)?,
                        row.get::<_, i64>(2)?,
                        row.get::<_, i64>(3)?,
                        row.get::<_, Vec<u8>>(4)?
                    ],
                )?;
            }
        }
    }
    for n in &file.npc {
        // The races' start NPCs are in the base already: their declaration changes nothing.
        if conn.query_row("SELECT count(*) FROM m_npc WHERE id = ?1", [n.id], |r| {
            r.get::<_, i64>(0)
        })? > 0
        {
            continue;
        }
        conn.execute(
            "INSERT INTO m_npc (id, name, subname, min_level, max_level, rank, zone_id, faction_id, friendly_to, npc_flags, sources) \
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, 'fixture')",
            params![
                n.id,
                text(n.name.as_ref(), || format!("Npc {}", n.id)),
                n.subname,
                n.level,
                n.max_level.unwrap_or(n.level),
                n.rank,
                n.at.first().map(|a| a.zone),
                n.faction,
                n.friendly_to,
                n.flags,
            ],
        )?;
        for at in &n.at {
            spawn(conn, "npc", n.id, at, n.respawn)?;
        }
    }
    for o in &file.object {
        if conn.query_row("SELECT count(*) FROM m_object WHERE id = ?1", [o.id], |r| {
            r.get::<_, i64>(0)
        })? > 0
        {
            continue;
        }
        conn.execute(
            "INSERT INTO m_object (id, name, zone_id, faction_id, sources) VALUES (?1, ?2, ?3, 0, 'fixture')",
            params![
                o.id,
                text(o.name.as_ref(), || format!("Object {}", o.id)),
                o.at.first().map(|a| a.zone)
            ],
        )?;
        for at in &o.at {
            spawn(conn, "object", o.id, at, o.respawn)?;
        }
    }
    for i in &file.item {
        if conn.query_row("SELECT count(*) FROM m_item WHERE id = ?1", [i.id], |r| {
            r.get::<_, i64>(0)
        })? > 0
        {
            continue;
        }
        conn.execute(
            "INSERT INTO m_item (id, name, item_level, required_level, class, subclass, start_quest, flags, sources) \
             VALUES (?1, ?2, ?3, ?3, ?4, 0, ?5, ?6, 'fixture')",
            params![
                i.id,
                text(i.name.as_ref(), || format!("Item {}", i.id)),
                i.level,
                i.class.unwrap_or(if i.start_quest.is_some() { 12 } else { 0 }),
                i.start_quest,
                if i.start_quest.is_some() { 2048 } else { 0 },
            ],
        )?;
        for d in &i.drops {
            let (kind, from) = d.from()?;
            conn.execute(
                "INSERT INTO m_item_source (item_id, from_type, from_id, chance, sources) VALUES (?1, ?2, ?3, ?4, 'fixture')",
                params![i.id, kind, from, d.chance],
            )?;
        }
    }
    for v in &file.vendor {
        for s in &v.items {
            conn.execute(
                "INSERT OR IGNORE INTO m_vendor_stock (npc_id, item_id, max_count, restock) VALUES (?1, ?2, ?3, ?4)",
                params![v.npc, s.item, s.count, s.restock],
            )?;
        }
    }
    for d in &file.dungeon {
        conn.execute(
            "INSERT INTO dungeon (area, name, entrance_zone, entrance_x, entrance_y, source) VALUES (?1, ?2, ?3, ?4, ?5, 'fixture')",
            params![d.area, d.name, d.at.zone, d.at.x, d.at.y],
        )?;
    }
    for q in &file.quest {
        // Declared by an earlier file: the first one counts.
        if conn.query_row("SELECT count(*) FROM m_quest WHERE id = ?1", [q.id], |r| {
            r.get::<_, i64>(0)
        })? > 0
        {
            continue;
        }
        quest(conn, q)?;
    }
    for r in &file.row {
        let columns: Vec<&str> = r.values.keys().map(String::as_str).collect();
        let marks: Vec<String> = (1..=columns.len()).map(|i| format!("?{i}")).collect();
        let values: Vec<Value> = r.values.values().map(sql_value).collect::<Result<_>>()?;
        conn.execute(
            &format!(
                "INSERT INTO {} ({}) VALUES ({})",
                r.table,
                columns.join(", "),
                marks.join(", ")
            ),
            rusqlite::params_from_iter(values),
        )?;
    }
    Ok(())
}

fn sql_value(v: &toml::Value) -> Result<Value> {
    Ok(match v {
        toml::Value::Integer(i) => Value::Integer(*i),
        toml::Value::Float(f) => Value::Real(*f),
        toml::Value::String(s) => Value::Text(s.clone()),
        toml::Value::Boolean(b) => Value::Integer(i64::from(*b)),
        other => bail!("a column takes a number, a string or a boolean, not {other}"),
    })
}

fn quest(conn: &Connection, q: &Quest) -> Result<()> {
    let min_level = q.min_level.unwrap_or(1.min(q.level));
    conn.execute(
        "INSERT INTO m_quest (id, name, level, min_level, max_level, races_mask, classes_mask, zone_or_sort, source_item_id, next_in_chain, \
         breadcrumb_for, parent_quest, quest_flags, special_flags, required_skill_id, required_spell, sources, status, hidden, in_client, \
         time_limit, xp, reward_spell, xp_observed, extra, requirements, objectives_text) \
         VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13, ?14, ?15, ?16, 'fixture', ?17, 0, 1, ?18, ?19, ?20, 1, ?21, ?22, ?23)",
        params![
            q.id,
            text(q.name.as_ref(), || format!("Quest {}", q.id)),
            q.level,
            min_level,
            q.max_level,
            q.races_mask,
            q.classes_mask,
            q.zone,
            q.source_item,
            q.next,
            q.breadcrumb_for,
            q.parent,
            q.flags.unwrap_or(0),
            q.special_flags.unwrap_or(0),
            q.required_skill,
            q.required_spell,
            q.status.as_deref().unwrap_or("available"),
            q.time_limit,
            q.xp.unwrap_or(100 * q.level),
            q.reward_spell,
            q.extra,
            q.requirements,
            q.objectives_text,
        ],
    )?;
    let relation = |role: &str, kind: &str, id: i64| {
        conn.execute(
            "INSERT INTO m_quest_relation (quest_id, role, entity_type, entity_id, sources) VALUES (?1, ?2, ?3, ?4, 'fixture')",
            params![q.id, role, kind, id],
        )
    };
    if let Some(npc) = q.giver {
        relation("start", "npc", npc)?;
        relation("end", "npc", npc)?;
    }
    for e in &q.starts {
        let (kind, id) = e.kind()?;
        relation("start", kind, id)?;
    }
    for e in &q.ends {
        let (kind, id) = e.kind()?;
        relation("end", kind, id)?;
    }
    for (idx, o) in q.objectives.iter().enumerate() {
        let (kind, target) = match (o.creature, o.item, o.object, o.talk) {
            (Some(id), None, None, None) => ("creature", Some(id)),
            (None, Some(id), None, None) => ("item", Some(id)),
            (None, None, Some(id), None) => ("object", Some(id)),
            (None, None, None, Some(id)) => ("talk", Some(id)),
            (None, None, None, None) if o.trigger.is_some() => ("trigger", None),
            _ => bail!(
                "quest {}: an objective names one of creature, item, object, talk, trigger",
                q.id
            ),
        };
        let extra = o.at.as_ref().map(|at| {
            let z = at.z.map(|z| format!(",{z}")).unwrap_or_default();
            format!("{{\"{}\":[[{},{}{z}]]}}", at.zone, at.x, at.y)
        });
        conn.execute(
            "INSERT INTO m_quest_objective (quest_id, idx, type, target_id, amount, text, extra, source) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, 'fixture')",
            params![q.id, idx as i64 + 1, kind, target, o.amount, o.text.as_ref().or(o.trigger.as_ref()), extra],
        )?;
    }
    for r in &q.rewards {
        conn.execute(
            "INSERT INTO m_quest_reward (quest_id, item_id, count, choice) VALUES (?1, ?2, ?3, ?4)",
            params![q.id, r.item, r.count, i64::from(r.choice)],
        )?;
    }
    for (kind, ids) in [
        ("pre_all", &q.pre_all),
        ("pre_any", &q.pre_any),
        ("exclusive", &q.exclusive),
        ("child", &q.child),
    ] {
        for other in ids {
            conn.execute(
                "INSERT INTO m_quest_link (quest_id, kind, other_id, sources) VALUES (?1, ?2, ?3, 'fixture')",
                params![q.id, kind, other],
            )?;
        }
    }
    Ok(())
}
