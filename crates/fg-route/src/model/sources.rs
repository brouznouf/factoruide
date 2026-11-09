//! Everything read from the database to build the quests: the quest rows, who gives and takes
//! them, their objectives, where items come from, and what the character can reach.

use super::dungeons::load_dungeons;
use super::entities::{EliteSpawns, Entities};
use super::pvp::pvp_quests;
use super::types::{Dungeon, EntityKind, FloorPoint, Loc, Profile};
use crate::params::Params;
use crate::world::{Pos, World};
use anyhow::{Context, Result};
use rusqlite::Connection;
use std::collections::{HashMap, HashSet};
use std::sync::Arc;

/// Objective rows by quest: (kind, target, text, extra, amount).
pub(super) type ObjectiveRows =
    HashMap<i64, Vec<(ObjectiveKind, Option<i64>, Option<String>, Option<String>, Option<i64>)>>;

/// Who gives (start) or takes (end) a quest.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum Role {
    Start,
    End,
}

/// Where an item comes from.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum SourceKind {
    Npc,
    Object,
    Vendor,
    /// A container item it is found in.
    Item,
}

impl SourceKind {
    fn parse(name: &str) -> Option<Self> {
        match name {
            "npc" => Some(Self::Npc),
            "object" => Some(Self::Object),
            "vendor" => Some(Self::Vendor),
            "item" => Some(Self::Item),
            _ => None,
        }
    }

    /// The entity to find in the world: the mob or the vendor, the object.
    pub(super) fn entity(self) -> Option<EntityKind> {
        match self {
            Self::Npc | Self::Vendor => Some(EntityKind::Npc),
            Self::Object => Some(EntityKind::Object),
            Self::Item => None,
        }
    }
}

/// What a quest objective asks.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum ObjectiveKind {
    /// Kill a creature (or talk to a friendly NPC).
    Creature,
    /// Kill one of several creatures giving credit.
    KillCredit,
    /// Act on a creature without killing it: talk to it, heal it, use a spell on it.
    Talk,
    Object,
    Item,
    /// Reach an area.
    Trigger,
    /// Reputation, spell...: not planned.
    Other,
}

impl ObjectiveKind {
    /// The entity it is about: the object, the item, else the creature.
    pub(super) fn entity(self) -> EntityKind {
        match self {
            Self::Object => EntityKind::Object,
            Self::Item => EntityKind::Item,
            _ => EntityKind::Npc,
        }
    }

    fn parse(name: &str) -> Self {
        match name {
            "creature" => Self::Creature,
            "killcredit" => Self::KillCredit,
            "talk" => Self::Talk,
            "object" => Self::Object,
            "item" => Self::Item,
            "trigger" => Self::Trigger,
            _ => Self::Other,
        }
    }
}

/// How a quest is linked to another.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum LinkKind {
    /// Every one of these first.
    PreAll,
    /// One of these first.
    PreAny,
    /// Taking one closes the other.
    Exclusive,
    /// A quest handed in on the way to this one's turn-in, the parent's (Distracting Jarven for
    /// Bitter Rivals: the barrel is unguarded once Jarven has his ale).
    Child,
    Other,
}

/// (role, entity kind, entity id) of the quest's givers and turn-ins.
pub(super) type Relations = HashMap<i64, Vec<(Role, EntityKind, i64)>>;

pub(super) struct Sources {
    pub(super) entities: Entities,
    pub(super) elites: EliteSpawns,
    /// Hostile mobs around places (givers, turn-ins, objectives).
    pub(super) guards: super::Guards,
    /// Where the character starts, and the continents it can reach from there.
    pub(super) start: Loc,
    pub(super) reachable: HashSet<i64>,
    pub(super) dungeons: Vec<Dungeon>,
    pub(super) dungeon_by_area: HashMap<i64, usize>,
    pub(super) inside: HashMap<(EntityKind, i64), usize>,
    /// Where the creatures and objects inside dungeons stand on their floors (`dungeon_spawn`).
    pub(super) floors: Arc<HashMap<(EntityKind, i64), FloorPoint>>,
    pub(super) rows: Vec<QuestRow>,
    pub(super) class_quest_ids: HashSet<i64>,
    pub(super) relations: Relations,
    pub(super) links: HashMap<i64, Vec<(LinkKind, i64)>>,
    pub(super) objectives: ObjectiveRows,
    pub(super) item_origins: Arc<HashMap<i64, Vec<(SourceKind, i64)>>>,
    pub(super) drop_chance: Arc<HashMap<(i64, i64), f64>>,
    /// Seconds the items that do not last are kept, by item.
    pub(super) item_durations: HashMap<i64, f64>,
    /// XP of quests the quest data has no value for: the most common reward column for their level.
    pub(super) default_xp: HashMap<i64, i64>,
    pub(super) pvp: HashSet<i64>,
    /// Quests whose reward is an item (an item that starts another quest is got there).
    pub(super) reward_of: HashMap<i64, Vec<i64>>,
    /// Items without any known source that several quests ask for (marks of honor, tokens,
    /// crafted goods): not a gap in the drop data, never approximated by mobs.
    pub(super) tokens: HashSet<i64>,
    /// Parent of each area (AreaTable), when zones are forced (`Params::forced_zones`).
    pub(super) area_parents: HashMap<i64, i64>,
}

impl Sources {
    pub(super) fn read(
        conn: &Connection,
        world: &World,
        profile: &Profile,
        params: &Params,
        known_rewards: &HashSet<i64>,
        known_prerequisites: &HashSet<i64>,
        reward_quests: &HashSet<i64>,
    ) -> Result<Self> {
        let mut entities = Entities::load(conn, profile.faction)?;
        let start = start_place(&entities, world, profile)?;
        let reachable = world.reachable_continents(start.pos.continent);
        forget_unreachable(&mut entities, world, &reachable);
        entities.borrow_heights(world);
        let (dungeons, dungeon_by_area, inside) =
            load_dungeons(conn, world, profile.faction, &profile.dungeons, &reachable)?;
        let rows = quest_rows(conn)?;
        let objectives = objective_rows(conn)?;
        let (item_origins, drop_chance) = item_origins(conn, params)?;
        let links = links(conn, profile)?;
        Ok(Self {
            elites: EliteSpawns::new(&entities, world),
            guards: super::Guards::load(conn, world, profile.faction)?,
            class_quest_ids: class_quest_ids(
                &rows,
                profile,
                &links,
                known_rewards,
                known_prerequisites,
                reward_quests,
                params.max_quest_above,
            ),
            relations: relations(conn)?,
            links,
            default_xp: default_xp(conn, params)?,
            pvp: pvp_quests(conn, &rows, &objectives)?,
            reward_of: reward_of(conn)?,
            tokens: tokens(&objectives, &item_origins),
            entities,
            start,
            reachable,
            dungeons,
            dungeon_by_area,
            inside,
            floors: floor_points(conn)?,
            rows,
            objectives,
            item_origins,
            item_durations: item_durations(conn)?,
            drop_chance,
            area_parents: if params.forced_zones.is_empty()
                && !(profile.objectives.iter()).any(|o| matches!(o, super::GuideObjective::Zone(_)))
            {
                HashMap::new()
            } else {
                area_parents(conn)?
            },
        })
    }

    /// Whether area `area` is one of `zones` or lies in one of them.
    pub(super) fn in_zones(&self, mut area: i64, zones: &[i64]) -> bool {
        for _ in 0..8 {
            if zones.contains(&area) {
                return true;
            }
            match self.area_parents.get(&area) {
                Some(&parent) if parent > 0 => area = parent,
                _ => return false,
            }
        }
        false
    }
}

/// The first spawn of each creature and object on a dungeon floor; none in a database without
/// them (`dungeon_spawn`, Classic and Forever).
fn floor_points(conn: &Connection) -> Result<Arc<HashMap<(EntityKind, i64), FloorPoint>>> {
    let has: bool = conn.query_row(
        "SELECT count(*) > 0 FROM sqlite_master WHERE name = 'dungeon_spawn'",
        [],
        |r| r.get(0),
    )?;
    if !has {
        return Ok(Arc::default());
    }
    crate::memo::rows(conn, "dungeon spawns", 0, || {
        let mut stmt = conn.prepare("SELECT entity_type, entity_id, ui_map, x, y FROM dungeon_spawn ORDER BY rowid")?;
        let mut out = HashMap::new();
        for row in stmt.query_map([], |r| {
            Ok((
                r.get::<_, String>(0)?,
                r.get::<_, i64>(1)?,
                FloorPoint {
                    ui_map: r.get(2)?,
                    x: r.get(3)?,
                    y: r.get(4)?,
                },
            ))
        })? {
            let (kind, id, point) = row?;
            if let Some(kind) = EntityKind::parse(&kind) {
                out.entry((kind, id)).or_insert(point);
            }
        }
        Ok(out)
    })
}

fn area_parents(conn: &Connection) -> Result<HashMap<i64, i64>> {
    let mut stmt = conn.prepare("SELECT ID, ParentAreaID FROM client_areatable WHERE ParentAreaID > 0")?;
    let rows = stmt.query_map([], |r| Ok((r.get(0)?, r.get(1)?)))?;
    Ok(rows.collect::<rusqlite::Result<_>>()?)
}

fn start_place(entities: &Entities, world: &World, profile: &Profile) -> Result<Loc> {
    let points = entities.points(world, EntityKind::Npc, profile.start_npc);
    let (pos, zone) = *points
        .first()
        .with_context(|| format!("start NPC {} has no known position", profile.start_npc))?;
    Ok(Loc {
        pos,
        zone,
        kind: EntityKind::Npc,
        id: profile.start_npc,
        name: entities.name(EntityKind::Npc, profile.start_npc),
        inside: None,
        floor: None,
    })
}

/// Forget everything on continents the character cannot reach.
fn forget_unreachable(entities: &mut Entities, world: &World, reachable: &HashSet<i64>) {
    for points in entities.spawns.values_mut() {
        points.retain(|(zone, ..)| on_reachable(world, reachable, *zone));
    }
}

pub(super) fn on_reachable(world: &World, reachable: &HashSet<i64>, zone: i64) -> bool {
    world
        .to_world(zone, 50.0, 50.0)
        .is_some_and(|p| reachable.contains(&p.continent))
}

fn default_xp(conn: &Connection, params: &Params) -> Result<HashMap<i64, i64>> {
    let col = format!("Difficulty_{}", params.default_xp_difficulty);
    let mut stmt = conn.prepare(&format!("SELECT ID, {col} FROM client_questxp"))?;
    let rows = stmt.query_map([], |r| Ok((r.get::<_, i64>(0)?, r.get::<_, i64>(1)?)))?;
    Ok(rows.collect::<rusqlite::Result<_>>()?)
}

fn quest_rows(conn: &Connection) -> Result<Vec<QuestRow>> {
    // Databases built before the column existed have no observed XP.
    let has_observed: bool = conn.query_row(
        "SELECT count(*) > 0 FROM pragma_table_info('m_quest') WHERE name = 'xp_observed'",
        [],
        |r| r.get(0),
    )?;
    let observed = if has_observed { "coalesce(xp_observed, 0)" } else { "0" };
    // Nor time limits.
    let has_limit: bool = conn.query_row(
        "SELECT count(*) > 0 FROM pragma_table_info('m_quest') WHERE name = 'time_limit'",
        [],
        |r| r.get(0),
    )?;
    let limit = if has_limit { "time_limit" } else { "NULL" };
    // Nor the spell a quest teaches.
    let has_reward_spell: bool = conn.query_row(
        "SELECT count(*) > 0 FROM pragma_table_info('m_quest') WHERE name = 'reward_spell'",
        [],
        |r| r.get(0),
    )?;
    let reward_spell = if has_reward_spell { "reward_spell" } else { "NULL" };
    let mut stmt = conn.prepare(&format!(
        "SELECT id, coalesce(name, ''), level, min_level, coalesce(xp, 0), zone_or_sort, races_mask, classes_mask,
                special_flags, required_skill_id, required_min_rep_faction, required_spell, breadcrumb_for,
                requirements, source_item_id, json_extract(extra, '$.required_source_items'), {observed},
                required_skill_value, reputation_reward, sources, quest_flags, {limit},
                coalesce(objectives_text GLOB '*[0-9]*', 0),
                (SELECT json_group_array(json_extract(t.value, '$[1]'))
                 FROM json_each(extra, '$.extra_objectives') e, json_each(e.value, '$.\"5\"') t
                 WHERE json_extract(t.value, '$[0]') = 'object'),
                {reward_spell}, objectives_text
         FROM m_quest WHERE status = 'available'"
    ))?;
    let rows = stmt.query_map([], |r| {
        Ok(QuestRow {
            id: r.get(0)?,
            name: r.get(1)?,
            level: r.get(2)?,
            min_level: r.get(3)?,
            xp: r.get(4)?,
            sort: r.get(5)?,
            races: r.get(6)?,
            classes: r.get(7)?,
            skill: r.get(9)?,
            rep: r.get(10)?,
            spell: r.get(11)?,
            breadcrumb_for: r.get(12)?,
            requirements: r.get(13)?,
            source_item: r.get(14)?,
            source_items: r.get(15)?,
            xp_observed: r.get(16)?,
            skill_value: r.get(17)?,
            reputation: r.get(18)?,
            sources: r.get(19)?,
            flags: r.get(20)?,
            time_limit: r.get(21)?,
            numbered: r.get(22)?,
            extra_objects: r
                .get::<_, Option<String>>(23)?
                .and_then(|j| serde_json::from_str(&j).ok())
                .unwrap_or_default(),
            reward_spell: r.get(24)?,
            objectives_text: r.get(25)?,
        })
    })?;
    Ok(rows.collect::<rusqlite::Result<_>>()?)
}

/// Retain missing spell-unlock chains; known rewards do not require repeating their quests.
fn class_quest_ids(
    rows: &[QuestRow],
    profile: &Profile,
    links: &HashMap<i64, Vec<(LinkKind, i64)>>,
    known: &HashSet<i64>,
    known_prerequisites: &HashSet<i64>,
    reward_quests: &HashSet<i64>,
    max_above: i64,
) -> HashSet<i64> {
    let mut ids: HashSet<_> = rows
        .iter()
        .filter(|r| profile.class_ids().iter().any(|c| r.of_class(*c)))
        .filter(|r| {
            profile
                .class_requirements
                .iter()
                .any(|c| c.selected && c.quests.contains(&r.id))
                || reward_quests.contains(&r.id)
        })
        .filter(|r| !known.contains(&r.id))
        .filter(|r| {
            reward_quests.contains(&r.id)
                || r.classes.is_some_and(|m| m & profile.class_bit() == 0)
                || profile.within_levels(r.level.unwrap_or(1).max(1), r.min_level.unwrap_or(1).max(1), max_above)
        })
        .filter(|r| r.classes.is_some_and(|m| m != 0) && profile.members_of(r.races, r.classes) != 0)
        .filter(|r| r.min_level.unwrap_or(1) <= profile.to_level)
        .map(|r| r.id)
        .collect();
    let eligible: HashSet<_> = rows
        .iter()
        .filter(|r| profile.members_of(r.races, r.classes) != 0)
        .map(|r| r.id)
        .collect();
    let mut stack: Vec<_> = ids.iter().copied().collect();
    while let Some(id) = stack.pop() {
        for &(kind, pre) in links.get(&id).into_iter().flatten() {
            if matches!(kind, LinkKind::PreAll | LinkKind::PreAny)
                && eligible.contains(&pre)
                && !known_prerequisites.contains(&pre)
                && ids.insert(pre)
            {
                stack.push(pre);
            }
        }
    }
    ids
}

fn relations(conn: &Connection) -> Result<Relations> {
    let mut relations: Relations = HashMap::new();
    let mut stmt = conn.prepare("SELECT quest_id, role, entity_type, entity_id FROM m_quest_relation")?;
    for row in stmt.query_map([], |r| {
        Ok((
            r.get::<_, i64>(0)?,
            r.get::<_, String>(1)?,
            r.get::<_, String>(2)?,
            r.get::<_, i64>(3)?,
        ))
    })? {
        let (q, role, kind, id) = row?;
        let role = if role == "start" { Role::Start } else { Role::End };
        if let Some(kind) = EntityKind::parse(&kind) {
            relations.entry(q).or_default().push((role, kind, id));
        }
    }
    Ok(relations)
}

fn links(conn: &Connection, profile: &Profile) -> Result<HashMap<i64, Vec<(LinkKind, i64)>>> {
    let mut links: HashMap<i64, Vec<(LinkKind, i64, bool)>> = HashMap::new();
    let mut stmt = conn.prepare(
        "SELECT l.quest_id,l.kind,l.other_id,q.races_mask,q.classes_mask
         FROM m_quest_link l LEFT JOIN m_quest q ON q.id=l.other_id",
    )?;
    for row in stmt.query_map([], |r| {
        Ok((
            r.get::<_, i64>(0)?,
            r.get::<_, String>(1)?,
            r.get::<_, i64>(2)?,
            r.get::<_, Option<i64>>(3)?,
            r.get::<_, Option<i64>>(4)?,
        ))
    })? {
        let (q, kind, other, races, classes) = row?;
        let kind = match kind.as_str() {
            "pre_all" => LinkKind::PreAll,
            "pre_any" => LinkKind::PreAny,
            "exclusive" => LinkKind::Exclusive,
            "child" => LinkKind::Child,
            _ => LinkKind::Other,
        };
        links
            .entry(q)
            .or_default()
            .push((kind, other, profile.members_of(races, classes) != 0));
    }
    let mut scope = conn.prepare("SELECT races_mask,classes_mask FROM m_quest WHERE id=?1")?;
    let mut known = conn.prepare("SELECT 1 FROM m_quest WHERE id=?1")?;
    for correction in &profile.quest_overrides {
        // A correction of a quest the database does not have (a database of a few quests, in
        // tests) has nothing to correct.
        if !known.exists([correction.id])? {
            continue;
        }
        for (kind, required) in [
            (LinkKind::PreAll, &correction.pre_all),
            (LinkKind::PreAny, &correction.pre_any),
        ] {
            let Some(required) = required else { continue };
            let alternatives = links.entry(correction.id).or_default();
            alternatives.retain(|(existing, _, _)| *existing != kind);
            for id in required {
                let (races, classes) = scope.query_row([id], |r| Ok((r.get(0)?, r.get(1)?))).with_context(|| {
                    format!(
                        "the correction of quest {} requires quest {id}, which the database does not have",
                        correction.id
                    )
                })?;
                alternatives.push((kind, *id, profile.members_of(races, classes) != 0));
            }
        }
    }
    Ok(links
        .into_iter()
        .map(|(quest, alternatives)| {
            // Use eligible race/class alternatives when present; otherwise keep the original links.
            let eligible = alternatives
                .iter()
                .any(|(kind, _, available)| *kind == LinkKind::PreAny && *available);
            let mut kept = Vec::new();
            for (kind, id, _) in alternatives
                .into_iter()
                .filter(|(kind, _, available)| *kind != LinkKind::PreAny || *available || !eligible)
            {
                if !kept.contains(&(kind, id)) {
                    kept.push((kind, id));
                }
            }
            (quest, kept)
        })
        .collect())
}

fn objective_rows(conn: &Connection) -> Result<ObjectiveRows> {
    let mut objectives: ObjectiveRows = HashMap::new();
    let mut stmt = conn.prepare(
        "SELECT quest_id, type, target_id, text, extra, amount FROM m_quest_objective ORDER BY quest_id, idx",
    )?;
    for row in stmt.query_map([], |r| {
        Ok((
            r.get::<_, i64>(0)?,
            (
                ObjectiveKind::parse(&r.get::<_, String>(1)?),
                r.get(2)?,
                r.get(3)?,
                r.get(4)?,
                r.get(5)?,
            ),
        ))
    })? {
        let (q, o) = row?;
        objectives.entry(q).or_default().push(o);
    }
    Ok(objectives)
}

/// Seconds each item that does not last is kept (`m_item.duration`).
fn item_durations(conn: &Connection) -> Result<HashMap<i64, f64>> {
    let mut stmt = conn.prepare("SELECT id, duration FROM m_item WHERE duration > 0")?;
    let rows = stmt.query_map([], |r| Ok((r.get::<_, i64>(0)?, r.get::<_, f64>(1)?)))?;
    Ok(rows.collect::<rusqlite::Result<_>>()?)
}

/// Where items come from, and their drop chance from each mob. World treasure chests (Battered
/// Chest: 700 possible items) are not where a quest item is farmed: objects looting more than
/// `max_object_loot` different items do not count as sources of an item found elsewhere too.
#[expect(
    clippy::type_complexity,
    reason = "two maps read by one query, destructured by the caller"
)]
fn item_origins(
    conn: &Connection,
    params: &Params,
) -> Result<(Arc<HashMap<i64, Vec<(SourceKind, i64)>>>, Arc<HashMap<(i64, i64), f64>>)> {
    // The same for a database and the limit on what a container holds.
    let kept = crate::memo::rows(
        conn,
        "item origins",
        crate::memo::setting(params.max_object_loot),
        || {
            let (origins, chances) = read_item_origins(conn, params)?;
            Ok((Arc::new(origins), Arc::new(chances)))
        },
    )?;
    Ok((Arc::clone(&kept.0), Arc::clone(&kept.1)))
}

#[expect(
    clippy::type_complexity,
    reason = "two maps read by one query, destructured by the caller"
)]
fn read_item_origins(
    conn: &Connection,
    params: &Params,
) -> Result<(HashMap<i64, Vec<(SourceKind, i64)>>, HashMap<(i64, i64), f64>)> {
    let mut item_sources: HashMap<i64, Vec<(SourceKind, i64)>> = HashMap::new();
    let mut drop_chance: HashMap<(i64, i64), f64> = HashMap::new();
    let mut stmt = conn.prepare(&format!(
        "WITH loot AS (SELECT from_id, count(DISTINCT item_id) AS n FROM m_item_source
                       WHERE from_type = 'object' GROUP BY from_id),
              chest AS (SELECT from_id FROM loot WHERE {limit} > 0 AND n > {limit}),
              -- Items with a source that is not a chest.
              other AS (SELECT DISTINCT item_id FROM m_item_source
                        WHERE from_type != 'object' OR from_id NOT IN chest)
         SELECT s.item_id, s.from_type, s.from_id, s.chance, coalesce(l.n, 0)
         FROM m_item_source s
         LEFT JOIN loot l ON s.from_type = 'object' AND l.from_id = s.from_id
         WHERE s.from_type != 'object' OR s.from_id NOT IN chest OR s.item_id NOT IN other",
        limit = params.max_object_loot
    ))?;
    for row in stmt.query_map([], |r| {
        Ok((
            r.get::<_, i64>(0)?,
            (r.get::<_, String>(1)?, r.get::<_, i64>(2)?),
            r.get::<_, Option<f64>>(3)?,
            r.get::<_, i64>(4)?,
        ))
    })? {
        let (i, (from, id), chance, loot) = row?;
        let Some(from) = SourceKind::parse(&from) else { continue };
        // A container giving one of several items (Box of Assorted Parts: 9 engineering parts),
        // without a known rate: one chance in their number.
        let chance = match from {
            SourceKind::Object if chance.is_none() && loot > 1 => Some(1.0 / loot as f64),
            _ => chance,
        };
        if let Some(c) = chance {
            drop_chance.insert((i, id), c);
        }
        item_sources.entry(i).or_default().push((from, id));
    }
    Ok((item_sources, drop_chance))
}

fn reward_of(conn: &Connection) -> Result<HashMap<i64, Vec<i64>>> {
    let mut reward_of: HashMap<i64, Vec<i64>> = HashMap::new();
    if let Ok(mut stmt) = conn.prepare("SELECT item_id, quest_id FROM m_quest_reward") {
        for row in stmt.query_map([], |r| Ok((r.get::<_, i64>(0)?, r.get::<_, i64>(1)?)))? {
            let (item, quest) = row?;
            reward_of.entry(item).or_default().push(quest);
        }
    }
    Ok(reward_of)
}

fn tokens(objectives: &ObjectiveRows, item_origins: &HashMap<i64, Vec<(SourceKind, i64)>>) -> HashSet<i64> {
    let mut askers: HashMap<i64, HashSet<i64>> = HashMap::new();
    for (q, objs) in objectives {
        for (kind, target, ..) in objs {
            if *kind == ObjectiveKind::Item
                && let Some(t) = target
                && !item_origins.contains_key(t)
            {
                askers.entry(*t).or_default().insert(*q);
            }
        }
    }
    askers
        .into_iter()
        .filter(|(_, q)| q.len() >= 2)
        .map(|(i, _)| i)
        .collect()
}

/// A row of `m_quest`.
#[cfg_attr(any(test, feature = "testing"), derive(Default))]
pub(super) struct QuestRow {
    pub(super) id: i64,
    pub(super) name: String,
    pub(super) level: Option<i64>,
    pub(super) min_level: Option<i64>,
    pub(super) xp: i64,
    pub(super) xp_observed: bool,
    pub(super) sort: Option<i64>,
    pub(super) races: Option<i64>,
    pub(super) classes: Option<i64>,
    pub(super) skill: Option<i64>,
    pub(super) skill_value: Option<i64>,
    pub(super) rep: Option<i64>,
    pub(super) spell: Option<i64>,
    pub(super) breadcrumb_for: Option<i64>,
    pub(super) requirements: Option<String>,
    pub(super) source_item: Option<i64>,
    pub(super) source_items: Option<String>,
    /// JSON [[faction, value], ...].
    pub(super) reputation: Option<String>,
    /// Sources of the quest, comma-separated.
    pub(super) sources: Option<String>,
    /// QuestFlags (2: an escort or an event).
    pub(super) flags: Option<i64>,
    /// Seconds from taking the quest to finish it (timed quests).
    pub(super) time_limit: Option<i64>,
    /// Its objectives text has a number ("Capture 5 Deeprun Rats").
    pub(super) numbered: bool,
    /// The quest's objectives text ("Speak with Nara Wildmane.").
    pub(super) objectives_text: Option<String>,
    /// Objects its extra objectives are done at (Questie's): where a creature found nowhere
    /// else is summoned (The Binding's summoning circle).
    pub(super) extra_objects: Vec<i64>,
    /// The spell its turn-in teaches (a pet, a totem...).
    pub(super) reward_spell: Option<i64>,
}

impl QuestRow {
    /// The reputations its turn-in gives: (Faction ID, points).
    pub(super) fn reputations(&self) -> Vec<(i64, i64)> {
        self.reputation
            .as_deref()
            .and_then(|j| serde_json::from_str(j).ok())
            .unwrap_or_default()
    }

    /// A quest of that class: filed under the class, or given to that class only (Forever files
    /// some under their zone: the dwarf's Call of Earth under Dun Morogh).
    pub(super) fn of_class(&self, class_id: i64) -> bool {
        self.sort == Some(-super::chains::class_sort(class_id)) || self.classes == Some(1 << (class_id - 1))
    }

    /// Known only from catalogues (a tooltip's text, a giver): their objectives and turn-in are
    /// unknown (the giver stands for it).
    pub(super) fn catalogued_only(&self) -> bool {
        self.sources
            .as_deref()
            .is_some_and(|s| s.split(',').all(|s| matches!(s.trim(), "wowhead" | "att")))
    }
}

/// "Tough Wolf Meat x 8" -> ("tough wolf meat", 8); Wowhead names one alone ("Sack of
/// \"Picture\" Books" -> 1). Gold and reputations ("Money:2", "The Defilers:") are no count.
pub(super) fn parse_requirements(json: Option<String>) -> Vec<(String, f64)> {
    let list: Vec<String> = json.and_then(|j| serde_json::from_str(&j).ok()).unwrap_or_default();
    list.iter()
        .filter_map(|r| match r.rsplit_once(" x ") {
            Some((name, count)) => Some((name.trim().to_lowercase(), count.trim().parse().ok()?)),
            // Gold ("Money:2") and reputations ("The Defilers:"); a name may hold one
            // ("Picture Book: Fun with Elementals").
            None if r.trim().is_empty() || r.trim_end().ends_with(':') || r.to_lowercase().starts_with("money:") => {
                None
            }
            None => Some((r.trim().to_lowercase(), 1.0)),
        })
        .collect()
}

/// Whether a quest's objectives text only asks to go to an NPC, with word or an item ("Speak with
/// Nara Wildmane.", "Bring the Damp Note to Je'neu Sancrea in Ashenvale."), or to travel to the one
/// handing it in, of names `enders` ("Travel to the Wetlands and meet the Deathstalker Agent").
pub(super) fn reads_as_delivery<'a>(text: &str, mut enders: impl Iterator<Item = &'a str>) -> bool {
    let text = text.trim_start().to_lowercase();
    ["speak ", "talk ", "report ", "bring ", "take ", "return ", "deliver "]
        .iter()
        .any(|verb| text.starts_with(verb))
        || text.starts_with("travel ") && enders.any(|name| !name.is_empty() && text.contains(&name.to_lowercase()))
}

/// Whether a quest's requirements ask for work (mobs, items, an action), not only gold, a
/// reputation or a talk: a quest asking that without objective data cannot be planned.
pub(super) fn asks_for_work(json: Option<&str>) -> bool {
    let list: Vec<String> = json.and_then(|j| serde_json::from_str(j).ok()).unwrap_or_default();
    list.iter().any(|r| {
        let r = r.trim().to_lowercase();
        !(r.is_empty()
            || r.starts_with("money:")
            || r.ends_with(':')
            || ["speak ", "talk ", "report "].iter().any(|p| r.starts_with(p)))
    })
}

/// Whether a requirement asks to talk to `name` ("Speak with Illaya Amberwind", "Listen to
/// Ayessa"): the sources give some talks as creature objectives.
pub(super) fn talks_to(reqs: &[(String, f64)], name: &str) -> bool {
    let name = name.to_lowercase();
    reqs.iter().any(|(r, _)| {
        ["speak with ", "speak to ", "talk to ", "talk with ", "listen to ", "report to "]
            .iter()
            .filter_map(|verb| r.strip_prefix(verb))
            // A first name alone: "Listen to Ayessa".
            .any(|who| who.contains(&name) || (!who.contains(' ') && name.starts_with(&format!("{who} "))))
    })
}

/// Whether a requirement asks to tame `name` ("Tame a Vuldren" for the Vuldren Alpha).
pub(super) fn tames(reqs: &[(String, f64)], name: &str) -> bool {
    let name = name.to_lowercase();
    reqs.iter().any(|(r, _)| {
        let Some(beast) = r.strip_prefix("tame ") else {
            return false;
        };
        let beast = (beast.strip_prefix("a ").or_else(|| beast.strip_prefix("an "))).unwrap_or(beast);
        !beast.is_empty() && (name.contains(beast) || beast.contains(&name))
    })
}

pub(super) fn count_for(reqs: &[(String, f64)], name: &str) -> Option<f64> {
    let name = name.to_lowercase();
    reqs.iter()
        .find(|(r, _)| r.contains(&name) || name.contains(r.as_str()))
        .map(|(_, c)| *c)
}

/// Spawn points all within this many yards: one NPC, not a kind of mob.
const ONE_NPC: f64 = 30.0;

/// Whether `points` are those of one NPC standing at one place (Henze Faulk, Rageclaw): an
/// objective on it with no known amount, and no number in the quest's text, asks for it once.
pub(super) fn one_place(points: &[(Pos, i64)]) -> bool {
    points
        .first()
        .is_some_and(|(a, _)| points.iter().all(|(b, _)| a.dist(b) <= ONE_NPC))
}

#[cfg(test)]
mod tests {
    use super::{QuestRow, asks_for_work, parse_requirements, talks_to, tames};

    /// A class quest is filed under its class, or given to that class only: Forever's dwarf
    /// Call of Earth is filed under Dun Morogh.
    #[test]
    fn class_quests_filed_under_a_zone() {
        let (shaman, warrior) = (7, 1);
        let classic = QuestRow {
            sort: Some(-82),
            classes: Some(64),
            ..QuestRow::default()
        };
        let forever = QuestRow {
            sort: Some(1),
            classes: Some(64),
            ..QuestRow::default()
        };
        let shared = QuestRow {
            sort: Some(1),
            classes: Some(64 | 1),
            ..QuestRow::default()
        };
        assert!(classic.of_class(shaman));
        assert!(forever.of_class(shaman));
        assert!(!forever.of_class(warrior));
        assert!(!shared.of_class(shaman));
        assert!(
            !QuestRow {
                sort: Some(1),
                ..QuestRow::default()
            }
            .of_class(shaman)
        );
    }

    /// Gold, a reputation or a talk ask for nothing to plan; items, mobs and actions do.
    #[test]
    fn requirements_asking_for_work() {
        assert!(!asks_for_work(None));
        assert!(!asks_for_work(Some(r#"["Money:2"]"#)));
        assert!(!asks_for_work(Some(r#"["The Defilers:"]"#)));
        assert!(!asks_for_work(Some(r#"["Speak with Lumina Windsinger"]"#)));
        assert!(asks_for_work(Some(r#"["Nord'el"]"#)));
    }

    /// One of an item whose name holds a colon (Forever's "Picture Book: Fun with Elementals"),
    /// not gold nor a reputation.
    #[test]
    fn requirements_with_a_colon() {
        let reqs = |json: &str| parse_requirements(Some(json.to_owned()));
        assert_eq!(
            reqs(r#"["Picture Book: Fun with Elementals","Lost Book x 6","Money:2","The Defilers:"]"#),
            [
                ("picture book: fun with elementals".to_owned(), 1.0),
                ("lost book".to_owned(), 6.0)
            ]
        );
    }

    /// Talks the sources give as creature objectives: named in a requirement after a verb.
    #[test]
    fn requirements_talking_to() {
        let reqs = |json: &str| parse_requirements(Some(json.to_owned()));
        assert!(talks_to(&reqs(r#"["Listen to Ayessa"]"#), "Ayessa Dawnsinger"));
        assert!(talks_to(
            &reqs(r#"["Find Aamelia Windfield","Listen to Alvarion Windfield's Story"]"#),
            "Alvarion Windfield"
        ));
        assert!(!talks_to(&reqs(r#"["Find Aamelia Windfield"]"#), "Aamelia Windfield"));
        assert!(!talks_to(&reqs(r#"["Speak with the Innkeeper"]"#), "Thelma"));
        assert!(!talks_to(&reqs(r#"["Al'Aketh Convert x 7"]"#), "Al'Aketh Convert"));
        assert!(asks_for_work(Some(
            r#"["Dragonmaw Saboteur x 2","Dragonmaw Warder x 4"]"#
        )));
    }

    /// Taming the Beast: the creature credit comes from taming one.
    #[test]
    fn requirements_taming() {
        let reqs = |json: &str| parse_requirements(Some(json.to_owned()));
        assert!(tames(&reqs(r#"["Tame a Large Crag Boar"]"#), "Large Crag Boar"));
        assert!(tames(&reqs(r#"["Tame an Adult Plainstrider"]"#), "Adult Plainstrider"));
        assert!(tames(&reqs(r#"["Tame a Vuldren"]"#), "Vuldren Alpha"));
        assert!(!tames(&reqs(r#"["Large Crag Boar x 8"]"#), "Large Crag Boar"));
        assert!(!tames(&reqs(r#"["Tame a Snow Leopard"]"#), "Ice Claw Bear"));
    }
}

#[cfg(test)]
mod links_tests {
    use super::*;
    use crate::faction::Faction;
    use crate::model::Mate;
    use crate::model::testing::profile;

    const ORC: i64 = 2;
    const HUMAN: i64 = 1;
    const WARRIOR: i64 = 1;
    const WARLOCK: i64 = 9;

    fn character(faction: Faction, race_id: i64, class_id: i64) -> Profile {
        Profile {
            race_id,
            class_id,
            ..profile(faction, 1, 30)
        }
    }

    /// The quests (id, races mask, classes mask) and their links (quest, kind, other).
    fn db(quests: &[(i64, i64, i64)], links: &[(i64, &str, i64)]) -> Connection {
        let conn = Connection::open_in_memory().unwrap();
        conn.execute_batch(
            "CREATE TABLE m_quest(id INTEGER, races_mask INTEGER, classes_mask INTEGER);
             CREATE TABLE m_quest_link(quest_id INTEGER, kind TEXT, other_id INTEGER);",
        )
        .unwrap();
        for q in quests {
            conn.execute(
                "INSERT INTO m_quest VALUES (?1, ?2, ?3)",
                rusqlite::params![q.0, q.1, q.2],
            )
            .unwrap();
        }
        for l in links {
            conn.execute(
                "INSERT INTO m_quest_link VALUES (?1, ?2, ?3)",
                rusqlite::params![l.0, l.1, l.2],
            )
            .unwrap();
        }
        conn
    }

    fn any_of(links: &HashMap<i64, Vec<(LinkKind, i64)>>, quest: i64) -> Vec<i64> {
        let mut ids: Vec<i64> = links[&quest]
            .iter()
            .filter(|(k, _)| *k == LinkKind::PreAny)
            .map(|l| l.1)
            .collect();
        ids.sort_unstable();
        ids
    }

    /// Vile Familiars' follow-up (794) has a branch for each class: the warrior's quest (792) and
    /// the warlock's (1499). A character keeps the branch it can do; one that can do none, or both
    /// (a group), keeps them all.
    #[test]
    fn a_follow_up_keeps_the_branch_of_the_character() {
        let orc = 1 << (ORC - 1);
        let warrior = 1 << (WARRIOR - 1);
        let warlock = 1 << (WARLOCK - 1);
        let conn = db(
            &[(792, orc, warrior), (1499, orc, warlock), (794, orc, 0)],
            &[(794, "pre_any", 792), (794, "pre_any", 1499)],
        );
        let links_of = |profile: &Profile| links(&conn, profile).unwrap();
        assert_eq!(any_of(&links_of(&character(Faction::Horde, ORC, WARRIOR)), 794), [792]);
        assert_eq!(any_of(&links_of(&character(Faction::Horde, ORC, WARLOCK)), 794), [1499]);
        // A human can do neither: the links stay as they are.
        assert_eq!(
            any_of(&links_of(&character(Faction::Alliance, HUMAN, WARRIOR)), 794),
            [792, 1499]
        );
        // A group of a warrior and a warlock needs both branches.
        let mut group = character(Faction::Horde, ORC, WARRIOR);
        group.mates = vec![Mate {
            class_name: "Warlock".into(),
            name: "Warlock-Realm".into(),
            race_id: ORC,
            class_id: WARLOCK,
            spell_weights: Vec::new(),
            professions: Vec::new(),
        }];
        assert_eq!(any_of(&links_of(&group), 794), [792, 1499]);
    }

    /// Forever's Windborne Cat quest has a predecessor for each side (the Horde's 98340, the
    /// Alliance's 98393): each side keeps its own, and both stay when the quest's race scope is
    /// the same.
    #[test]
    fn each_side_keeps_its_own_predecessor() {
        let (horde, alliance) = (1 << 32, 1 << (HUMAN - 1));
        let conn = db(
            &[(98340, horde, 0), (98393, alliance, 0), (98400, 0, 0)],
            &[(98400, "pre_any", 98340), (98400, "pre_any", 98393)],
        );
        let skyborne_horde = character(Faction::Horde, 95, WARRIOR);
        assert_eq!(any_of(&links(&conn, &skyborne_horde).unwrap(), 98400), [98340]);
        let human = character(Faction::Alliance, HUMAN, WARRIOR);
        assert_eq!(any_of(&links(&conn, &human).unwrap(), 98400), [98393]);
    }

    /// Links from the quest data are kept once each, with their kind.
    #[test]
    fn a_link_is_kept_once_with_its_kind() {
        let conn = db(
            &[(1, 0, 0), (2, 0, 0), (3, 0, 0)],
            &[
                (3, "pre_all", 1),
                (3, "pre_all", 1),
                (3, "exclusive", 2),
                (3, "child", 2),
                (3, "breadcrumb", 1),
            ],
        );
        let kept = &links(&conn, &character(Faction::Horde, ORC, WARRIOR)).unwrap()[&3];
        assert_eq!(kept.iter().filter(|l| **l == (LinkKind::PreAll, 1)).count(), 1);
        assert!(kept.contains(&(LinkKind::Exclusive, 2)));
        assert!(kept.contains(&(LinkKind::Child, 2)));
        assert!(kept.contains(&(LinkKind::Other, 1)));
    }
}
