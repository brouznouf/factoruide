//! Everything read from the database to build the quests: the quest rows, who gives and takes
//! them, their objectives, where items come from, and what the character can reach.

use super::dungeons::load_dungeons;
use super::entities::{EliteSpawns, Entities};
use super::pvp::pvp_quests;
use super::types::{Dungeon, EntityKind, Loc, Profile};
use crate::params::Params;
use crate::world::World;
use anyhow::{Context, Result};
use rusqlite::Connection;
use std::collections::{HashMap, HashSet};

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
    pub(super) rows: Vec<QuestRow>,
    pub(super) class_quest_ids: HashSet<i64>,
    pub(super) relations: Relations,
    pub(super) links: HashMap<i64, Vec<(LinkKind, i64)>>,
    pub(super) objectives: ObjectiveRows,
    pub(super) item_origins: HashMap<i64, Vec<(SourceKind, i64)>>,
    pub(super) drop_chance: HashMap<(i64, i64), f64>,
    /// XP of quests Questie has no value for: the most common reward column for their level.
    pub(super) default_xp: HashMap<i64, i64>,
    pub(super) pvp: HashSet<i64>,
    /// Quests whose reward is an item (an item that starts another quest is got there).
    pub(super) reward_of: HashMap<i64, Vec<i64>>,
    pub(super) repeatable: HashSet<i64>,
    /// Items without any known source that several quests ask for (marks of honor, tokens,
    /// crafted goods): not a gap in the drop data, never approximated by mobs.
    pub(super) tokens: HashSet<i64>,
}

impl Sources {
    pub(super) fn read(conn: &Connection, world: &World, profile: &Profile, params: &Params) -> Result<Self> {
        let mut entities = Entities::load(conn, profile.faction)?;
        let start = start_place(&entities, world, profile)?;
        let reachable = world.reachable_continents(start.pos.continent);
        forget_unreachable(&mut entities, world, &reachable);
        let (dungeons, dungeon_by_area, inside) = load_dungeons(conn, world, &profile.dungeons, &reachable)?;
        let rows = quest_rows(conn)?;
        let objectives = objective_rows(conn)?;
        let (item_origins, drop_chance) = item_origins(conn, params)?;
        let links = links(conn)?;
        Ok(Self {
            elites: EliteSpawns::new(&entities, world),
            guards: super::Guards::load(conn, world)?,
            class_quest_ids: class_quest_ids(&rows, profile, &links),
            relations: relations(conn)?,
            links,
            default_xp: default_xp(conn, params)?,
            pvp: pvp_quests(conn, &rows, &objectives)?,
            reward_of: reward_of(conn)?,
            repeatable: rows
                .iter()
                .filter(|r| r.special.unwrap_or(0) & 1 != 0)
                .map(|r| r.id)
                .collect(),
            tokens: tokens(&objectives, &item_origins),
            entities,
            start,
            reachable,
            dungeons,
            dungeon_by_area,
            inside,
            rows,
            objectives,
            item_origins,
            drop_chance,
        })
    }
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
    })
}

/// Forget everything on continents the character cannot reach.
fn forget_unreachable(entities: &mut Entities, world: &World, reachable: &HashSet<i64>) {
    for points in entities.spawns.values_mut() {
        points.retain(|(z, _, _)| on_reachable(world, reachable, *z));
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
    let mut stmt = conn.prepare(&format!(
        "SELECT id, coalesce(name, ''), level, min_level, coalesce(xp, 0), zone_or_sort, races_mask, classes_mask,
                special_flags, required_skill_id, required_min_rep_faction, required_spell, breadcrumb_for,
                requirements, source_item_id, json_extract(extra, '$.required_source_items'), {observed},
                required_skill_value, reputation_reward, sources, quest_flags
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
            special: r.get(8)?,
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
        })
    })?;
    Ok(rows.collect::<rusqlite::Result<_>>()?)
}

/// The class quests the profile requires, of the classes in the group; another player's come
/// with the rest of their chain (its prerequisites are of that class too, under other names).
fn class_quest_ids(rows: &[QuestRow], profile: &Profile, links: &HashMap<i64, Vec<(LinkKind, i64)>>) -> HashSet<i64> {
    let mut ids: HashSet<i64> = rows
        .iter()
        .filter(|r| {
            profile
                .class_ids()
                .iter()
                .any(|c| r.sort == Some(-super::chains::class_sort(*c)))
        })
        .filter(|r| profile.required_class_quests.iter().any(|n| n == &r.name))
        .filter(|r| r.classes.is_some_and(|m| m & profile.group_class_mask() != 0))
        // The character's race, or the one deduced for the other player's class.
        .filter(|r| r.races.is_none_or(|m| m == 0 || m & profile.quest_races(r.classes) != 0))
        .filter(|r| r.min_level.unwrap_or(1) <= profile.to_level)
        .map(|r| r.id)
        .collect();
    let others: HashSet<i64> = rows
        .iter()
        .filter(|r| r.classes.is_some_and(|m| m != 0 && m & profile.class_bit() == 0))
        .map(|r| r.id)
        .collect();
    let mut stack: Vec<i64> = ids.iter().copied().filter(|id| others.contains(id)).collect();
    while let Some(id) = stack.pop() {
        for &(kind, pre) in links.get(&id).into_iter().flatten() {
            if matches!(kind, LinkKind::PreAll | LinkKind::PreAny) && others.contains(&pre) && ids.insert(pre) {
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

fn links(conn: &Connection) -> Result<HashMap<i64, Vec<(LinkKind, i64)>>> {
    let mut links: HashMap<i64, Vec<(LinkKind, i64)>> = HashMap::new();
    let mut stmt = conn.prepare("SELECT quest_id, kind, other_id FROM m_quest_link")?;
    for row in stmt.query_map([], |r| {
        Ok((r.get::<_, i64>(0)?, r.get::<_, String>(1)?, r.get::<_, i64>(2)?))
    })? {
        let (q, kind, other) = row?;
        let kind = match kind.as_str() {
            "pre_all" => LinkKind::PreAll,
            "pre_any" => LinkKind::PreAny,
            "exclusive" => LinkKind::Exclusive,
            _ => LinkKind::Other,
        };
        links.entry(q).or_default().push((kind, other));
    }
    Ok(links)
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
) -> Result<(HashMap<i64, Vec<(SourceKind, i64)>>, HashMap<(i64, i64), f64>)> {
    let mut item_sources: HashMap<i64, Vec<(SourceKind, i64)>> = HashMap::new();
    let mut drop_chance: HashMap<(i64, i64), f64> = HashMap::new();
    let mut stmt = conn.prepare(&format!(
        "WITH chest AS (SELECT from_id FROM m_item_source WHERE from_type = 'object' GROUP BY from_id
                        HAVING {limit} > 0 AND count(DISTINCT item_id) > {limit})
         SELECT item_id, from_type, from_id, chance FROM m_item_source s
         WHERE from_type != 'object' OR from_id NOT IN chest OR NOT EXISTS (
           SELECT 1 FROM m_item_source o WHERE o.item_id = s.item_id
             AND NOT (o.from_type = 'object' AND o.from_id IN chest))",
        limit = params.max_object_loot
    ))?;
    for row in stmt.query_map([], |r| {
        Ok((
            r.get::<_, i64>(0)?,
            (r.get::<_, String>(1)?, r.get::<_, i64>(2)?),
            r.get::<_, Option<f64>>(3)?,
        ))
    })? {
        let (i, (from, id), chance) = row?;
        let Some(from) = SourceKind::parse(&from) else { continue };
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
    pub(super) special: Option<i64>,
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
}

impl QuestRow {
    /// Known only from catalogues: Wowhead's tooltip (text) and AllTheThings (givers). Their
    /// objectives and turn-in are unknown (AllTheThings' giver stands for it).
    pub(super) fn catalogued_only(&self) -> bool {
        self.sources
            .as_deref()
            .is_some_and(|s| s.split(',').all(|s| matches!(s.trim(), "wowhead" | "att")))
    }
}

/// "Tough Wolf Meat x 8" -> ("tough wolf meat", 8)
pub(super) fn parse_requirements(json: Option<String>) -> Vec<(String, f64)> {
    let list: Vec<String> = json.and_then(|j| serde_json::from_str(&j).ok()).unwrap_or_default();
    list.iter()
        .filter_map(|r| {
            let (name, count) = r.rsplit_once(" x ")?;
            Some((name.trim().to_lowercase(), count.trim().parse().ok()?))
        })
        .collect()
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

pub(super) fn count_for(reqs: &[(String, f64)], name: &str, default: f64) -> f64 {
    let name = name.to_lowercase();
    reqs.iter()
        .find(|(r, _)| r.contains(&name) || name.contains(r.as_str()))
        .map_or(default, |(_, c)| *c)
}

#[cfg(test)]
mod tests {
    use super::asks_for_work;

    /// Gold, a reputation or a talk ask for nothing to plan; items, mobs and actions do.
    #[test]
    fn requirements_asking_for_work() {
        assert!(!asks_for_work(None));
        assert!(!asks_for_work(Some(r#"["Money:2"]"#)));
        assert!(!asks_for_work(Some(r#"["The Defilers:"]"#)));
        assert!(!asks_for_work(Some(r#"["Speak with Lumina Windsinger"]"#)));
        assert!(asks_for_work(Some(r#"["Nord'el"]"#)));
        assert!(asks_for_work(Some(
            r#"["Dragonmaw Saboteur x 2","Dragonmaw Warder x 4"]"#
        )));
    }
}
