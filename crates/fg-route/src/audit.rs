//! Data audits: each check looks for one kind of data problem everywhere in an edition's data,
//! for every race. A problem met once (a player's report) becomes a check, so that the same
//! problem is found in all the game's data, fixed there, and does not come back.
//!
//! Findings known to be right are accepted in `overrides/audit.toml` (with the reason); the
//! others are to fix.

use crate::job::{self, Overrides, PlanRequest};
use crate::model::{EntityKind, Model};
use crate::terrain::LARGE_AREA;
use crate::world::{Pos, World};
use anyhow::{Context, Result};
use rusqlite::Connection;
use serde::Deserialize;
use std::collections::{BTreeMap, HashMap, HashSet, VecDeque};

/// A check: its name, what it looks for, and whether its findings are errors (else a list to
/// review: candidates that are mostly right).
pub struct Check {
    pub name: &'static str,
    pub what: &'static str,
    pub error: bool,
}

/// The checks.
pub const CHECKS: &[Check] = &[
    Check {
        name: "islands",
        what: "places of quests in an area left only by boat, portal, lift or its passages \
               ([[enclosed]] of passes.toml) that none takes to: the planner cannot get there",
        error: true,
    },
    Check {
        name: "source-items",
        what: "items a quest needs (used on its objectives) that no step of the guide gets",
        error: true,
    },
    Check {
        name: "class-quests",
        what: "class quests every guide of the class must include (class_quests.toml) that the \
               planner leaves out for a race of that class: their place, a prerequisite or the \
               way there is unknown",
        error: true,
    },
    Check {
        name: "other-class-quests",
        what: "quests in a guide of a class given or turned in by another class's trainer: a \
               class quest whose class the data does not know, planned for every class",
        error: true,
    },
    Check {
        name: "terrain-gaps",
        what: "areas with places of quests the terrain does not connect to their continent's \
               mainland, even through the passages: the planner walks there in a straight line. \
               Mostly tunnels, bridges and gates, which are buildings (accept them); an island or \
               the top of a cliff left only by boat, portal or lift is to declare [[enclosed]] \
               in passes.toml, with its ways out in travel.toml",
        error: false,
    },
    Check {
        name: "chain-order",
        what: "WoW Forever quests new to it (not in the 1.12 data) with no prerequisite known, \
               given by an NPC who takes back another quest of the guide: the guide may take \
               them before the game offers them (a chain the sources lack). Check the quest's \
               series on Wowhead or in game: a missing prerequisite is fixed in the quest data, \
               a quest offered from the start is accepted",
        error: false,
    },
    Check {
        name: "broken-chains",
        what: "quests left out because the data does not know an objective or its place (an area \
               to reach, a mob, an object, an item's source) while quests of the guide need them \
               first: the rest of the chain is lost with them. Give the place in the quest data, \
               or accept a quest the game no longer offers",
        error: true,
    },
    Check {
        name: "child-quests",
        what: "quests handed in on the way to another's turn-in (its children: Distracting Jarven \
               for Bitter Rivals) that the guide cannot place where they end: the parent is \
               planned without them and its turn-in may be impossible. Give the place in the \
               quest data",
        error: true,
    },
    Check {
        name: "starter-drops",
        what: "quests started by an item dropped by creatures with no drop chance known: the \
               planner assumes 10% when one creature (or only unique ones) drops it, and a rare \
               find, left out of the guide, when several kinds of common mobs do. Give the chance \
               in the quest data ([[drop]]) when it is known, or accept the assumption",
        error: false,
    },
    Check {
        name: "contradictory-prerequisites",
        what: "quests needing themselves first, or a quest that is also exclusive with them \
               (doing it closes the quest): the planner never offers them. Drop the wrong link \
               in the quest data (a list of a quest's alternatives copied in place of its \
               prerequisites, a sequence marked exclusive)",
        error: true,
    },
    Check {
        name: "ambiguous-floors",
        what: "places of quests of unknown height (no spawn of the server data at them, nor of \
               the same NPC or object near) in a cell where floors lie far over each other (a \
               cave under a hill, a bridge over a river, a tower): the planner puts them on the \
               surface or the floor most of the floors around are in, maybe the wrong one. Give \
               the height in the quest data (`[[height]]`) when it is wrong, or accept it",
        error: false,
    },
    Check {
        name: "dungeon-entrances",
        what: "dungeons whose sides enter by different doors (`dungeon_entrance`: City of Dalaran, a \
               tunnel for the Horde, a pipe for the Alliance) with a door given for one faction \
               only (the other would take the shared one, which may be the first's), or for an \
               area that is no dungeon of the data. Nothing in the data says which dungeons have \
               two doors: give the other faction's door in the data overrides \
               (`[[dungeon_entrance]]`, both sides), or accept a door the other side shares",
        error: true,
    },
    Check {
        name: "timed-quests",
        what: "quests of the guide whose objectives give a time (\"in ten minutes\") while the \
               data has no time limit: the planner may do other things between taking and \
               turning in, and the game fails the quest. Give the limit (`time_limit`) in the \
               quest data, or accept a time that is no limit (a spell that lasts 2 minutes)",
        error: false,
    },
];

/// Why a quest is left out when its objectives or their places are unknown to the data (the
/// reasons of the quest builder that say "unknown").
fn unknown_in_data(why: &str) -> bool {
    why.contains("unknown")
}

/// One problem found.
#[derive(Debug, Clone)]
pub struct Finding {
    pub check: &'static str,
    /// What the problem is about, stable from one data build to the next (to accept it).
    pub key: String,
    pub detail: String,
    /// The races whose guides have it.
    pub races: Vec<String>,
}

/// A finding known to be right, in `overrides/audit.toml`.
#[derive(Debug, Clone, Deserialize)]
pub struct Accepted {
    pub check: String,
    pub key: String,
    pub reason: String,
}

/// The accepted findings of an edition: those of `overrides/audit.toml` and of
/// `overrides/<edition>/audit.toml`.
pub fn accepted(dir: &std::path::Path, edition: crate::xp::Edition) -> Result<Vec<Accepted>> {
    #[derive(Default, Deserialize)]
    struct File {
        #[serde(default)]
        accept: Vec<Accepted>,
    }
    let mut accepted = Vec::new();
    let paths = [
        dir.join("audit.toml"),
        dir.join(edition.overrides_subdir()).join("audit.toml"),
    ];
    for path in paths {
        if let Ok(text) = std::fs::read_to_string(&path) {
            let file: File = toml::from_str(&text).with_context(|| format!("parsing {}", path.display()))?;
            accepted.extend(file.accept);
        }
    }
    Ok(accepted)
}

/// Run the checks named (all when empty) for every race of the edition, a character of each
/// race leveling over the whole level range. Findings are merged over the races.
pub fn audit(
    conn: &Connection,
    overrides: &Overrides,
    checks: &[String],
    progress: &dyn Fn(&str),
) -> Result<Vec<Finding>> {
    let on = |name: &str| checks.is_empty() || checks.iter().any(|c| c == name);
    let mut found: BTreeMap<(&'static str, String), Finding> = BTreeMap::new();
    let source_items = if on("source-items") {
        quest_source_items(conn)?
    } else {
        HashMap::new()
    };
    let class_quests = job::class_quests(conn, overrides)?;
    // Forever only: it reworked zones with quests the sources often know without their chain;
    // TBC's new quests have Questie's.
    let chain_order_on = on("chain-order") && overrides.params.edition == crate::xp::Edition::Forever;
    let old_quests = if chain_order_on {
        old_quests(conn)?
    } else {
        HashSet::new()
    };
    let children = if on("child-quests") {
        child_links(conn)?
    } else {
        Vec::new()
    };
    let starter_drops = if on("starter-drops") {
        starter_drops(conn)?
    } else {
        Vec::new()
    };
    let timed_text = if on("timed-quests") {
        untimed_quests_saying_a_time(conn)?
    } else {
        HashMap::new()
    };
    let doors = if on("dungeon-entrances") {
        dungeon_doors(conn)?
    } else {
        Vec::new()
    };
    let known_dungeons = if on("dungeon-entrances") {
        dungeon_areas(conn)?
    } else {
        HashSet::new()
    };
    let trainer_quests = if on("other-class-quests") {
        trainer_quests(conn)?
    } else {
        HashMap::new()
    };
    for (race, def) in &overrides.races {
        // Classic Era comparison databases have no class list: any class goes.
        let class = job::race_classes(conn, def.id)?
            .into_iter()
            .next()
            .unwrap_or_else(|| "warrior".into());
        let request = PlanRequest {
            race: race.clone(),
            class,
            from_level: 1,
            to_level: overrides.params.edition.rules().max_level,
            name: None,
            params: None,
            required_class_quests: None,
            required_quests: vec![],
            professions: Vec::new(),
            locale: None,
            mates: Vec::new(),
            start: None,
        };
        let prepared = match job::prepare(conn, overrides, &request, &|_| {}) {
            Ok(p) => p,
            Err(e) => {
                progress(&format!("{race}: {e}"));
                continue;
            }
        };
        progress(&format!("{race}: {} quests", prepared.model.quests.len()));
        let mut add = |f: Finding| {
            let races = &mut found
                .entry((f.check, f.key.clone()))
                .or_insert_with(|| Finding { races: Vec::new(), ..f })
                .races;
            if !races.contains(race) {
                races.push(race.clone());
            }
        };
        if on("islands") {
            islands(&prepared.world, &prepared.model).into_iter().for_each(&mut add);
        }
        if on("terrain-gaps") {
            terrain_gaps(&prepared.world, &prepared.model)
                .into_iter()
                .for_each(&mut add);
        }
        if chain_order_on {
            chain_order(&prepared.model, &old_quests).into_iter().for_each(&mut add);
        }
        if on("broken-chains") {
            broken_chains(&prepared.model).into_iter().for_each(&mut add);
        }
        if on("contradictory-prerequisites") {
            contradictory_prerequisites(&prepared.model)
                .into_iter()
                .for_each(&mut add);
        }
        if on("child-quests") {
            children_left_out(&prepared.model, &children)
                .into_iter()
                .for_each(&mut add);
        }
        if on("starter-drops") {
            unknown_starter_drops(&prepared.model, &starter_drops)
                .into_iter()
                .for_each(&mut add);
        }
        if on("ambiguous-floors") {
            ambiguous_floors(&prepared.world, &prepared.model)
                .into_iter()
                .for_each(&mut add);
        }
        if on("dungeon-entrances") {
            dungeon_entrances(&prepared.model, &doors, &known_dungeons)
                .into_iter()
                .for_each(&mut add);
        }
        if on("timed-quests") {
            timed_quests(&prepared.model, &timed_text)
                .into_iter()
                .for_each(&mut add);
        }
        if on("source-items") {
            uncovered_source_items(&prepared.model, &source_items)
                .into_iter()
                .for_each(&mut add);
        }
        if on("class-quests") || on("other-class-quests") {
            for class in job::race_classes(conn, def.id)? {
                let required = on("class-quests") && class_quests.get(&class).is_some_and(|q| !q.is_empty());
                if !required && !on("other-class-quests") {
                    continue;
                }
                let request = PlanRequest {
                    class: class.clone(),
                    ..request.clone()
                };
                let p = match job::prepare(conn, overrides, &request, &|_| {}) {
                    Ok(p) => p,
                    Err(e) => {
                        progress(&format!("{race} {class}: {e}"));
                        continue;
                    }
                };
                if required {
                    let own = class_quest_ids(conn, p.profile.class_id)?;
                    class_quests_left_out(&class, &p.profile.required_class_quests, &own, &p.model)
                        .into_iter()
                        .for_each(&mut add);
                }
                if on("other-class-quests") {
                    other_class_quests(&class, &p.model, &trainer_quests)
                        .into_iter()
                        .for_each(&mut add);
                }
            }
        }
    }
    Ok(found.into_values().collect())
}

/// The quests of a class: filed under it or given to it only (a quest of another kind may have a
/// required class quest's name: Tirion Fordring's Redemption).
fn class_quest_ids(conn: &Connection, class_id: i64) -> Result<HashSet<i64>> {
    let mut stmt = conn.prepare("SELECT id FROM m_quest WHERE classes_mask = ?1 OR zone_or_sort = ?2")?;
    let ids = stmt
        .query_map(
            rusqlite::params![1i64 << (class_id - 1), -crate::model::class_sort(class_id)],
            |r| r.get(0),
        )?
        .collect::<rusqlite::Result<_>>()?;
    Ok(ids)
}

/// Required class quests of a class (by name) the model leaves out: skipped while building it,
/// or no longer required because a prerequisite cannot be planned.
fn class_quests_left_out(class: &str, required: &[String], own: &HashSet<i64>, model: &Model) -> Vec<Finding> {
    let skipped = (model.skipped.iter())
        .filter(|(id, name, why)| required.contains(name) && own.contains(id) && why != "repeatable")
        .map(|(id, name, why)| (*id, name, why.as_str()));
    // Of a name teaching a spell, only those leading to it are required.
    let designated = crate::model::required_by_name(&model.quests);
    let released = (model.quests.iter())
        .filter(|q| required.contains(&q.name) && own.contains(&q.id) && !q.mandatory)
        .filter(|q| designated.contains(&q.id))
        .map(|q| (q.id, &q.name, "a prerequisite cannot be planned"));
    skipped
        .chain(released)
        .map(|(id, name, why)| Finding {
            check: "class-quests",
            key: format!("{class} quest {id}"),
            detail: format!("{name} ({id}): {why}"),
            races: Vec::new(),
        })
        .collect()
}

/// Classes (keys) whose trainers give or take each quest, from the trainers' titles
/// ("Mage Trainer"; "Pet Trainer": the hunter's).
fn trainer_quests(conn: &Connection) -> Result<HashMap<i64, Vec<String>>> {
    let mut stmt = conn.prepare(
        "WITH t AS (SELECT r.quest_id, lower(substr(n.subname, 1, length(n.subname) - 8)) AS title
           FROM m_quest_relation r JOIN m_npc n ON n.id = r.entity_id
           WHERE r.entity_type = 'npc' AND n.subname LIKE '% Trainer')
         SELECT DISTINCT quest_id, CASE title WHEN 'pet' THEN 'hunter' ELSE title END AS class FROM t
         WHERE class IN (SELECT lower(Filename) FROM client_chrclasses)",
    )?;
    let mut out: HashMap<i64, Vec<String>> = HashMap::new();
    for row in stmt.query_map([], |r| Ok((r.get::<_, i64>(0)?, r.get::<_, String>(1)?)))? {
        let (quest, class) = row?;
        out.entry(quest).or_default().push(class);
    }
    Ok(out)
}

/// Quests of the model of `class` that only other classes' trainers give or take.
fn other_class_quests(class: &str, model: &Model, trainer_quests: &HashMap<i64, Vec<String>>) -> Vec<Finding> {
    (model.quests.iter())
        .filter_map(|q| {
            let classes = trainer_quests.get(&q.id)?;
            (!classes.iter().any(|c| c == class)).then(|| Finding {
                check: "other-class-quests",
                key: format!("quest {}", q.id),
                detail: format!(
                    "{} ({}): {} trainer, in the {class}'s guide",
                    q.name,
                    q.id,
                    classes.join("/")
                ),
                races: Vec::new(),
            })
        })
        .collect()
}

/// A place of a quest: what is there, and its position.
struct Spot<'m> {
    quest: &'m str,
    what: String,
    pos: Pos,
}

/// A large walkable area of the terrain: continent and area.
type Area = (i64, u32);

/// Places of quests out of reach from `starts`, by area. `walk` tells whether walking goes from
/// an area to another, `passages` the areas joined by each passage; with `links`, boats, portals
/// and lifts (`travel.toml`) go from one to another. One finding per area, named after its zone
/// and the place most of its quests are at.
fn out_of_reach(
    check: &'static str,
    world: &World,
    model: &Model,
    starts: &[Area],
    walk: impl Fn(Area, Area) -> bool,
    passages: &[Vec<Area>],
    links: bool,
) -> Vec<Finding> {
    let area = |p: &Pos| large_area(world, p);
    let mut next: HashMap<Area, Vec<Area>> = HashMap::new();
    for l in world.links.iter().filter(|_| links) {
        if let (Some(a), Some(b)) = (area(&l.from), area(&l.to)) {
            next.entry(a).or_default().push(b);
        }
    }
    for areas in passages {
        for &a in areas {
            next.entry(a).or_default().extend(areas.iter().copied());
        }
    }
    let mut spots: BTreeMap<Area, Vec<Spot<'_>>> = BTreeMap::new();
    for q in &model.quests {
        let places = q
            .starts
            .iter()
            .map(|l| (format!("starts at {}", l.name), l))
            .chain(q.ends.iter().map(|l| (format!("ends at {}", l.name), l)))
            .chain(
                q.objectives
                    .iter()
                    .filter(|o| o.dungeon.is_none())
                    .map(|o| (o.text.clone(), &o.loc)),
            );
        for (what, loc) in places {
            if loc.kind == EntityKind::Dungeon {
                continue;
            }
            if let Some(a) = area(&loc.pos) {
                spots.entry(a).or_default().push(Spot {
                    quest: &q.name,
                    what,
                    pos: loc.pos,
                });
            }
        }
    }
    let mut reached: HashSet<Area> = starts.iter().copied().collect();
    let mut queue: VecDeque<Area> = starts.iter().copied().collect();
    while let Some(a) = queue.pop_front() {
        let walked = spots.keys().copied().filter(|&b| walk(a, b));
        for b in next
            .get(&a)
            .into_iter()
            .flatten()
            .copied()
            .chain(walked)
            .collect::<Vec<_>>()
        {
            if reached.insert(b) {
                queue.push_back(b);
            }
        }
    }
    spots
        .into_iter()
        .filter(|(a, _)| !reached.contains(a))
        .map(|(_, spots)| {
            let mut places: BTreeMap<String, usize> = BTreeMap::new();
            for s in &spots {
                let zone = world.zone_at(&s.pos).map_or("?", |z| world.zone_name(z));
                let place = world.place_name(&s.pos).unwrap_or("");
                *places.entry(format!("{zone}: {place}")).or_default() += 1;
            }
            let key = places
                .iter()
                .max_by_key(|(name, n)| (**n, std::cmp::Reverse((*name).clone())))
                .map(|(name, _)| name.clone())
                .unwrap_or_default();
            let quests: std::collections::BTreeSet<&str> = spots.iter().map(|s| s.quest).collect();
            let s = &spots[0];
            let at = world
                .zone_at(&s.pos)
                .and_then(|z| world.to_map(z, &s.pos))
                .map(|m| format!(" ({} {:.1}, {:.1})", world.zone_name(m.zone), m.x, m.y))
                .unwrap_or_default();
            Finding {
                check,
                key,
                detail: format!(
                    "{} places of {} quests, e.g. {}: {}{at}",
                    spots.len(),
                    quests.len(),
                    s.quest,
                    s.what
                ),
                races: Vec::new(),
            }
        })
        .collect()
}

fn large_area(world: &World, p: &Pos) -> Option<Area> {
    world
        .terrain
        .area_of(p)
        .filter(|&(a, size)| a != 0 && size >= LARGE_AREA)
        .map(|(a, _)| (p.continent, a))
}

/// Floors of a cell this far apart in height (yards) or more make a place of unknown height
/// there ambiguous.
const AMBIGUOUS_FLOORS: i16 = 10;

/// Places of quests (givers, turn-ins, objectives) of unknown height whose cell has floors of
/// large areas `AMBIGUOUS_FLOORS` yards apart or more: the planner may put them on the wrong one.
/// One finding per entity and zone.
fn ambiguous_floors(world: &World, model: &Model) -> Vec<Finding> {
    let mut found: BTreeMap<String, Finding> = BTreeMap::new();
    for q in &model.quests {
        let places = (q.starts.iter().map(|l| ("starts at", l)))
            .chain(q.ends.iter().map(|l| ("ends at", l)))
            .chain(
                (q.objectives.iter())
                    .filter(|o| o.dungeon.is_none())
                    .flat_map(|o| std::iter::once(&o.loc).chain(o.alts.iter().map(|a| &a.loc)))
                    .map(|l| ("objective at", l)),
            );
        for (what, loc) in places {
            if loc.pos.z.is_some() || matches!(loc.kind, EntityKind::Dungeon | EntityKind::Taxi) {
                continue;
            }
            let Some((floors, on)) = world.terrain.floors_at(&loc.pos) else {
                continue;
            };
            let (Some(low), Some(high)) = (floors.first(), floors.last()) else {
                continue;
            };
            if high - low < AMBIGUOUS_FLOORS {
                continue;
            }
            let zone = world.zone_at(&loc.pos).map_or("?", |z| world.zone_name(z));
            let key = format!("{zone}: {}", loc.name);
            let at = world
                .zone_at(&loc.pos)
                .and_then(|z| world.to_map(z, &loc.pos))
                .map(|m| format!("{:.1}, {:.1}", m.x, m.y))
                .unwrap_or_default();
            let heights: Vec<String> = floors.iter().map(i16::to_string).collect();
            found.entry(key.clone()).or_insert_with(|| Finding {
                check: "ambiguous-floors",
                key,
                detail: format!(
                    "{} {what} {} ({at}): floors at {} yards, put on {}",
                    q.name,
                    loc.name,
                    heights.join(", "),
                    on.map_or("none".to_owned(), |h| h.to_string())
                ),
                races: Vec::new(),
            });
        }
    }
    found.into_values().collect()
}

/// Places of quests the planner cannot get to: in an area left only by boat, portal, lift or
/// the passages joining it (`[[enclosed]]`) that none takes to, or from one.
fn islands(world: &World, model: &Model) -> Vec<Finding> {
    let enclosed = |a: Area| world.terrain.is_enclosed(a.0, a.1);
    let walk = |a: Area, b: Area| a.0 == b.0 && !enclosed(a) && !enclosed(b);
    let Some(start) = large_area(world, &model.start.pos) else {
        return Vec::new();
    };
    out_of_reach("islands", world, model, &[start], walk, &passage_areas(world), true)
}

/// Yards around a passage where the areas it joins are looked for (the two ends of a tunnel).
const PASSAGE_REACH: f64 = 150.0;

/// Places of quests in an area the terrain does not connect to its continent's mainland (its
/// largest area), even through the passages (the areas around each), and not declared
/// enclosed: the planner walks there in a straight line. Either a tunnel, a bridge or a gate the
/// terrain does not hold (accept it), or an area left only by boat, portal or lift to declare
/// `[[enclosed]]`, with its ways out in `travel.toml`. A boat does not count: it lands on
/// islands.
fn terrain_gaps(world: &World, model: &Model) -> Vec<Finding> {
    let mut mainlands: Vec<Area> = world
        .terrain
        .continents()
        .into_iter()
        .filter_map(|c| world.terrain.main_area(c).map(|a| (c, a)))
        .collect();
    mainlands.extend(world.terrain.enclosed_areas());
    out_of_reach(
        "terrain-gaps",
        world,
        model,
        &mainlands,
        |a, b| a == b,
        &passage_areas(world),
        false,
    )
}

/// The areas each passage joins, as the planner walks between two areas (`World::bridge`): those
/// around it, and those around the passages it is walked to and from (the two ends of a bridge,
/// of a ramp).
fn passage_areas(world: &World) -> Vec<Vec<Area>> {
    let passages: Vec<Vec<Area>> = world
        .passes
        .iter()
        .map(|pass| {
            let mut areas: Vec<Area> = Vec::new();
            for r in [0.0, 50.0, 100.0, PASSAGE_REACH] {
                for k in 0..12 {
                    let angle = f64::from(k) * std::f64::consts::PI / 6.0;
                    let p = Pos {
                        x: pass.pos.x + r * angle.cos(),
                        y: pass.pos.y + r * angle.sin(),
                        ..pass.pos
                    };
                    if let Some(a) = large_area(world, &p).filter(|a| !areas.contains(a)) {
                        areas.push(a);
                    }
                }
            }
            areas
        })
        .collect();
    // From a passage of one to a passage of the other, through the passages between them.
    let ends = |i: usize| !world.passes[i].shortcut && !world.passes[i].one_way;
    (0..passages.len())
        .map(|i| {
            let mut areas = passages[i].clone();
            if ends(i) {
                for j in (0..passages.len()).filter(|&j| ends(j) && world.pass_dist[i][j].is_finite()) {
                    areas.extend(passages[j].iter().filter(|a| !passages[i].contains(a)));
                }
            }
            areas
        })
        .collect()
}

/// What each quest needs on top of its objectives: the items used on them (required source
/// items) and whether each one is found somewhere (dropped, looted, sold; not in what the
/// quest gives on accepting, a bag of them), the one given on
/// accepting, the items its objectives ask for, and whether one of those is made of the others
/// (found nowhere itself, nor is what holds it: Morrowgrain, grown from soil and seeds).
struct SourceItems {
    items: Vec<(i64, String, bool)>,
    given: Option<i64>,
    asked: Vec<i64>,
    assembled: bool,
}

fn quest_source_items(conn: &Connection) -> Result<HashMap<i64, SourceItems>> {
    let mut stmt = conn.prepare(
        "SELECT q.id, j.value, q.source_item_id, coalesce(i.name, ''),
                EXISTS (SELECT 1 FROM m_item_source s WHERE s.item_id = j.value)
                  AND NOT EXISTS (SELECT 1 FROM m_item_source s WHERE s.item_id = j.value
                    AND s.from_type = 'item' AND s.from_id IS q.source_item_id),
                (SELECT group_concat(o.target_id) FROM m_quest_objective o
                  WHERE o.quest_id = q.id AND o.type = 'item'),
                EXISTS (SELECT 1 FROM m_quest_objective o
                  WHERE o.quest_id = q.id AND o.type = 'item'
                    AND NOT EXISTS (SELECT 1 FROM m_item_source s
                      LEFT JOIN m_item_source c ON s.from_type = 'item' AND c.item_id = s.from_id
                      WHERE s.item_id = o.target_id
                        AND (s.from_type != 'item' OR c.from_type != 'item')))
         FROM m_quest q, json_each(json_extract(q.extra, '$.required_source_items')) j
         LEFT JOIN m_item i ON i.id = j.value",
    )?;
    let mut out: HashMap<i64, SourceItems> = HashMap::new();
    let rows = stmt.query_map([], |r| {
        Ok((
            r.get::<_, i64>(0)?,
            r.get::<_, i64>(1)?,
            r.get::<_, Option<i64>>(2)?,
            r.get::<_, String>(3)?,
            r.get::<_, bool>(4)?,
            r.get::<_, Option<String>>(5)?,
            r.get::<_, bool>(6)?,
        ))
    })?;
    for row in rows {
        let (quest, item, given, name, found, asked, assembled) = row?;
        let e = out.entry(quest).or_insert_with(|| SourceItems {
            items: Vec::new(),
            given,
            asked: asked
                .unwrap_or_default()
                .split(',')
                .filter_map(|s| s.parse().ok())
                .collect(),
            assembled,
        });
        e.items.push((item, name, found));
    }
    Ok(out)
}

/// Quests of the guide needing an item found somewhere that none of its objectives gets (nor
/// is it given on accepting, nor are they parts of an item asked for, nor got in a dungeon run).
fn uncovered_source_items(model: &Model, source_items: &HashMap<i64, SourceItems>) -> Vec<Finding> {
    let mut out = Vec::new();
    for q in &model.quests {
        let Some(needs) = source_items.get(&q.id) else {
            continue;
        };
        if needs.assembled || q.objectives.iter().any(|o| o.dungeon.is_some()) {
            continue;
        }
        for (item, name, found) in &needs.items {
            let got = |o: &crate::model::Objective| o.loc.kind == EntityKind::Item && o.loc.id == *item;
            if !found
                || needs.given == Some(*item)
                || needs.asked.contains(item)
                || q.objectives.iter().any(|o| o.places().any(got))
            {
                continue;
            }
            out.push(Finding {
                check: "source-items",
                key: format!("quest {} item {item}", q.id),
                detail: format!("{} ({}) needs {name} ({item}): no step gets it", q.name, q.id),
                races: Vec::new(),
            });
        }
    }
    out
}

/// A quest started by an item creatures drop without any known chance (nor an object giving it).
struct StarterDrop {
    quest: i64,
    quest_name: String,
    item: i64,
    item_name: String,
    /// Each creature dropping it: ID, name, and what tells a unique one (rank, places known).
    droppers: Vec<(i64, String, crate::model::Dropper)>,
}

fn starter_drops(conn: &Connection) -> Result<Vec<StarterDrop>> {
    let mut stmt = conn.prepare(
        "SELECT r.quest_id, coalesce(q.name, ''), s.item_id, coalesce(i.name, ''), s.from_id,
                coalesce(n.name, ''), coalesce(n.rank, 0),
                (SELECT count(*) FROM m_spawn p WHERE p.entity_type = 'npc' AND p.entity_id = s.from_id AND p.x >= 0)
         FROM m_quest_relation r
         JOIN m_quest q ON q.id = r.quest_id
         JOIN m_item_source s ON s.item_id = r.entity_id AND s.from_type = 'npc'
         LEFT JOIN m_item i ON i.id = s.item_id
         LEFT JOIN m_npc n ON n.id = s.from_id
         WHERE r.role = 'start' AND r.entity_type = 'item'
           AND NOT EXISTS (SELECT 1 FROM m_item_source k WHERE k.item_id = s.item_id
                             AND (k.chance IS NOT NULL OR k.from_type = 'object'))
         GROUP BY r.quest_id, s.item_id, s.from_id
         ORDER BY r.quest_id, s.item_id, s.from_id",
    )?;
    let rows = stmt.query_map([], |r| {
        Ok((
            r.get::<_, i64>(0)?,
            r.get::<_, String>(1)?,
            r.get::<_, i64>(2)?,
            r.get::<_, String>(3)?,
            r.get::<_, i64>(4)?,
            r.get::<_, String>(5)?,
            r.get::<_, i64>(6)?,
            r.get::<_, i64>(7)?,
        ))
    })?;
    let mut out: Vec<StarterDrop> = Vec::new();
    for row in rows {
        let (quest, quest_name, item, item_name, npc, npc_name, rank, spawns) = row?;
        let dropper = (
            npc,
            npc_name,
            crate::model::Dropper {
                rank,
                spawns: usize::try_from(spawns).unwrap_or(0),
            },
        );
        match out.last_mut() {
            Some(last) if last.quest == quest && last.item == item => last.droppers.push(dropper),
            _ => out.push(StarterDrop {
                quest,
                quest_name,
                item,
                item_name,
                droppers: vec![dropper],
            }),
        }
    }
    Ok(out)
}

/// Quests of the guide (planned or left out) started by an item of unknown drop chance, with
/// what the planner assumes for it. Not the bosses' drops, looted during the dungeon run.
fn unknown_starter_drops(model: &Model, starters: &[StarterDrop]) -> Vec<Finding> {
    let left_out: HashSet<i64> = model.skipped.iter().map(|(id, _, _)| *id).collect();
    let mut out = Vec::new();
    for s in starters {
        let planned = model.index.get(&s.quest).map(|&i| &model.quests[i]);
        if (planned.is_none() && !left_out.contains(&s.quest)) || planned.is_some_and(|q| q.start_dungeon.is_some()) {
            continue;
        }
        let droppers: Vec<crate::model::Dropper> = s.droppers.iter().map(|d| d.2).collect();
        let assumed = if crate::model::assumed_start_chance(&droppers) > 0.0 {
            format!(
                "assumed {:.0}% (its own creature's)",
                crate::model::UNKNOWN_START_CHANCE * 100.0
            )
        } else {
            "assumed rare (common mobs): left out".to_owned()
        };
        let names: Vec<String> = s
            .droppers
            .iter()
            .map(|(id, name, _)| format!("{name} ({id})"))
            .collect();
        out.push(Finding {
            check: "starter-drops",
            key: format!("quest {} item {}", s.quest, s.item),
            detail: format!(
                "{} ({}) starts with {} ({}), dropped by {}: no drop chance known, {assumed}",
                s.quest_name,
                s.quest,
                s.item_name,
                s.item,
                names.join(", ")
            ),
            races: Vec::new(),
        });
    }
    out
}

/// A dungeon's door for one faction (`dungeon_entrance`): dungeon area, faction letter.
type Door = (i64, String);

/// The doors the data gives dungeons per faction (none in a database without the table).
fn dungeon_doors(conn: &Connection) -> Result<Vec<Door>> {
    let has_table: bool = conn.query_row(
        "SELECT count(*) > 0 FROM sqlite_master WHERE type = 'table' AND name = 'dungeon_entrance'",
        [],
        |r| r.get(0),
    )?;
    if !has_table {
        return Ok(Vec::new());
    }
    let mut stmt = conn.prepare("SELECT DISTINCT area, faction FROM dungeon_entrance ORDER BY area, faction")?;
    let doors = stmt
        .query_map([], |r| Ok((r.get(0)?, r.get(1)?)))?
        .collect::<rusqlite::Result<_>>()?;
    Ok(doors)
}

/// The areas of the dungeons the data knows.
fn dungeon_areas(conn: &Connection) -> Result<HashSet<i64>> {
    let mut stmt = conn.prepare("SELECT area FROM dungeon")?;
    let areas = stmt.query_map([], |r| r.get(0))?.collect::<rusqlite::Result<_>>()?;
    Ok(areas)
}

/// Doors given for one faction only, and doors of an area that is no dungeon (a wrong area ID
/// the planner would never use). `known` are the areas of the data's dungeons; the planned
/// ones (those with settings of their own) count too.
fn dungeon_entrances(model: &Model, doors: &[Door], known: &HashSet<i64>) -> Vec<Finding> {
    let mut out = Vec::new();
    let areas: std::collections::BTreeSet<i64> = doors.iter().map(|d| d.0).collect();
    for area in areas {
        let name = model.dungeons.iter().find(|d| d.area == area).map(|d| d.name.as_str());
        if name.is_none() && !known.contains(&area) {
            out.push(Finding {
                check: "dungeon-entrances",
                key: format!("area {area}"),
                detail: format!("a door is given for area {area}, which is no dungeon of the data"),
                races: Vec::new(),
            });
            continue;
        }
        let Some(name) = name else { continue };
        for (letter, faction, other) in [("A", "Alliance", "Horde"), ("H", "Horde", "Alliance")] {
            let has = |l: &str| doors.iter().any(|d| d.0 == area && d.1 == l);
            if has(letter) && !has(if letter == "A" { "H" } else { "A" }) {
                out.push(Finding {
                    check: "dungeon-entrances",
                    key: format!("{name} ({area})"),
                    detail: format!(
                        "{name} has a door for the {faction} only: the {other} enters by the shared one, \
                         which may be the {faction}'s"
                    ),
                    races: Vec::new(),
                });
            }
        }
    }
    out
}

/// The quests of the 1.12 data (VMaNGOS): the server's own prerequisites, complete.
fn old_quests(conn: &Connection) -> Result<HashSet<i64>> {
    let mut stmt = conn.prepare("SELECT id FROM m_quest WHERE ',' || sources || ',' LIKE '%,vmangos,%'")?;
    let ids = stmt.query_map([], |r| r.get(0))?.collect::<rusqlite::Result<_>>()?;
    Ok(ids)
}

/// Quests of the guide not in the 1.12 data, with no prerequisite, given by an NPC who takes back
/// another quest of the guide not linked to them: Walton gave Trouble in the Valley only after The
/// Hermit Tanner, which no source said (the guide took it first).
fn chain_order(model: &Model, old_quests: &HashSet<i64>) -> Vec<Finding> {
    let before = |id: i64| {
        let mut seen = HashSet::new();
        let mut todo = vec![id];
        while let Some(q) = todo.pop() {
            let Some(&i) = model.index.get(&q) else { continue };
            for &p in model.quests[i].pre_all.iter().chain(&model.quests[i].pre_any) {
                if seen.insert(p) {
                    todo.push(p);
                }
            }
        }
        seen
    };
    let npc = |l: &crate::model::Loc| (l.kind == EntityKind::Npc).then_some(l.id);
    let mut out = Vec::new();
    for b in &model.quests {
        if old_quests.contains(&b.id) || !b.pre_all.is_empty() || !b.pre_any.is_empty() {
            continue;
        }
        for start in b.starts.iter().filter_map(|l| npc(l).map(|id| (id, &l.name))) {
            let taken_back: Vec<String> = (model.quests.iter())
                .filter(|a| {
                    let linked = a.id == b.id
                        || a.exclusive.contains(&b.id)
                        || b.exclusive.contains(&a.id)
                        || a.breadcrumb_for == Some(b.id)
                        || b.breadcrumb_for == Some(a.id)
                        || before(a.id).contains(&b.id);
                    !linked && a.min_level <= b.min_level && a.ends.iter().any(|l| npc(l) == Some(start.0))
                })
                .map(|a| format!("{} ({})", a.name, a.id))
                .collect();
            if taken_back.is_empty() {
                continue;
            }
            out.push(Finding {
                check: "chain-order",
                key: format!("quest {}", b.id),
                detail: format!(
                    "{} ({}) from {}, who takes back {}: no prerequisite known",
                    b.name,
                    b.id,
                    start.1,
                    taken_back.join(", ")
                ),
                races: Vec::new(),
            });
        }
    }
    out
}

/// Quests without a time limit whose objectives text speaks of minutes or a time limit, with
/// that text: Grace of An'she and Mu'sha ("before the incense burns to ash in ten minutes") had
/// none, the guide did eight steps between taking and turning it in, and the game failed it.
fn untimed_quests_saying_a_time(conn: &Connection) -> Result<HashMap<i64, String>> {
    let has_limit: bool = conn.query_row(
        "SELECT count(*) > 0 FROM pragma_table_info('m_quest') WHERE name = 'time_limit'",
        [],
        |r| r.get(0),
    )?;
    let limit = if has_limit { "coalesce(time_limit, 0)" } else { "0" };
    let mut stmt = conn.prepare(&format!(
        "SELECT id, objectives_text FROM m_quest WHERE {limit} <= 0 AND objectives_text IS NOT NULL"
    ))?;
    let rows = stmt.query_map([], |r| Ok((r.get::<_, i64>(0)?, r.get::<_, String>(1)?)))?;
    let mut out = HashMap::new();
    for row in rows {
        let (id, text) = row?;
        if says_a_time(&text) {
            out.insert(id, text);
        }
    }
    Ok(out)
}

/// The text gives a time to do the quest in.
fn says_a_time(text: &str) -> bool {
    let text = text.to_lowercase();
    ["minute", "time limit", "time runs out", "before time"]
        .iter()
        .any(|w| text.contains(w))
}

fn timed_quests(model: &Model, untimed: &HashMap<i64, String>) -> Vec<Finding> {
    model
        .quests
        .iter()
        .filter_map(|q| {
            let text = untimed.get(&q.id)?;
            let excerpt: String = text.chars().take(120).collect();
            Some(Finding {
                check: "timed-quests",
                key: format!("quest {}", q.id),
                detail: format!(
                    "{} ({}): no time limit known, its objectives say \"{excerpt}\"",
                    q.name, q.id
                ),
                races: Vec::new(),
            })
        })
        .collect()
}

/// (parent, parent's name, child, child's name) of the children the guides hand in on the way
/// (the repeatable ones, see the model's `LinkKind::Child`).
fn child_links(conn: &Connection) -> Result<Vec<(i64, String, i64, String)>> {
    let mut stmt = conn.prepare(
        "SELECT p.id, p.name, c.id, c.name FROM m_quest_link l
         JOIN m_quest p ON p.id = l.quest_id JOIN m_quest c ON c.id = l.other_id
         WHERE l.kind = 'child' AND coalesce(c.special_flags, 0) & 1 != 0 ORDER BY p.id, c.id",
    )?;
    let rows = stmt
        .query_map([], |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?, r.get(3)?)))?
        .collect::<rusqlite::Result<_>>()?;
    Ok(rows)
}

/// Planned quests one of whose children has no objective handing it in.
fn children_left_out(model: &Model, children: &[(i64, String, i64, String)]) -> Vec<Finding> {
    let mut out = Vec::new();
    for (parent, parent_name, child, child_name) in children {
        let Some(&i) = model.index.get(parent) else { continue };
        if model.quests[i].objectives.iter().any(|o| o.child == Some(*child)) {
            continue;
        }
        out.push(Finding {
            check: "child-quests",
            key: format!("quest {parent} child {child}"),
            detail: format!(
                "{parent_name} ({parent}) is planned without {child_name} ({child}): where it is handed in is unknown"
            ),
            races: Vec::new(),
        });
    }
    out
}

/// Quests left out for something the data does not know (`unknown_in_data`) that quests of the
/// guide need first: all of them (`pre_all`), or the only ones of a choice (`pre_any`, all left
/// out). Secure the Mountain's den had no place, which lost the report taken back after it.
fn broken_chains(model: &Model) -> Vec<Finding> {
    let unknown: HashMap<i64, (&str, &str)> = (model.skipped.iter())
        .filter(|(_, _, why)| unknown_in_data(why))
        .map(|(id, name, why)| (*id, (name.as_str(), why.as_str())))
        .collect();
    let mut needed: BTreeMap<i64, Vec<String>> = BTreeMap::new();
    for q in &model.quests {
        let any_lost = !q.pre_any.is_empty() && q.pre_any.iter().all(|p| !model.index.contains_key(p));
        let lost = (q.pre_all.iter())
            .chain(q.pre_any.iter().filter(|_| any_lost))
            .filter(|p| unknown.contains_key(p));
        for p in lost {
            let by = needed.entry(*p).or_default();
            let next = format!("{} ({})", q.name, q.id);
            if !by.contains(&next) {
                by.push(next);
            }
        }
    }
    needed
        .into_iter()
        .map(|(id, by)| {
            let (name, why) = unknown[&id];
            Finding {
                check: "broken-chains",
                key: format!("quest {id}"),
                detail: format!("{name} ({id}) left out: {why}; needed first by {}", by.join(", ")),
                races: Vec::new(),
            }
        })
        .collect()
}

/// Quests whose prerequisites can never be met: themselves (Call of Earth listing its group,
/// itself included, as what it needs first), or a quest exclusive with them (taking it closes
/// the quest).
fn contradictory_prerequisites(model: &Model) -> Vec<Finding> {
    let exclusive = |a: &crate::model::Quest, b: i64| {
        a.exclusive.contains(&b)
            || model
                .index
                .get(&b)
                .is_some_and(|&j| model.quests[j].exclusive.contains(&a.id))
    };
    let mut out = Vec::new();
    for q in &model.quests {
        let mut wrong: Vec<i64> = Vec::new();
        for &p in q.pre_all.iter().chain(&q.pre_any) {
            if (p == q.id || exclusive(q, p)) && !wrong.contains(&p) {
                wrong.push(p);
            }
        }
        if wrong.is_empty() {
            continue;
        }
        let what: Vec<String> = (wrong.iter())
            .map(|&p| {
                if p == q.id {
                    "itself".to_owned()
                } else {
                    format!("{p} (exclusive with it)")
                }
            })
            .collect();
        out.push(Finding {
            check: "contradictory-prerequisites",
            key: format!("quest {}", q.id),
            detail: format!("{} ({}) needs first {}", q.name, q.id, what.join(", ")),
            races: Vec::new(),
        });
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::plan::tests::quests_at_start;

    /// The accepted findings of the repository's three editions read (a bad line stops `fg audit`).
    #[test]
    fn the_accepted_findings_files_read() {
        let dir = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../overrides");
        for edition in crate::xp::Edition::ALL {
            let found = accepted(&dir, edition).unwrap();
            assert!(!found.is_empty(), "{edition:?}");
        }
    }

    /// Secure the Mountain's case: a quest left out because an objective has no place, needed
    /// first by a quest of the guide. Not found for a reason the data does not cause (elites),
    /// nor when another quest of a choice stays.
    #[test]
    fn a_quest_left_out_for_an_unknown_place_that_a_kept_quest_needs_is_found() {
        let mut p = quests_at_start(2);
        let first = p.model.quests[0].id;
        let lost = 990_001;
        p.model.skipped = vec![
            (lost, "Secure the Mountain".into(), "exploration area unknown".into()),
            (990_002, "Elites".into(), "elite objective".into()),
        ];
        let keys = |m: &Model| -> Vec<String> { broken_chains(m).into_iter().map(|f| f.key).collect() };
        assert!(keys(&p.model).is_empty(), "nothing needs it");
        p.model.quests[0].pre_all = vec![lost];
        assert_eq!(keys(&p.model), [format!("quest {lost}")]);
        p.model.quests[0].pre_all = vec![990_002];
        assert!(keys(&p.model).is_empty(), "left out for an elite, not the data");
        p.model.quests[0].pre_all.clear();
        p.model.quests[1].pre_any = vec![lost, first];
        assert!(keys(&p.model).is_empty(), "the other quest of the choice stays");
        p.model.quests[1].pre_any = vec![lost];
        assert_eq!(keys(&p.model), [format!("quest {lost}")]);
    }

    /// Walton's case: two quests of one NPC, one he also takes back, no link between them. Found
    /// until the data links them, or when the quest is in the 1.12 data.
    #[test]
    fn a_quest_given_by_the_npc_taking_back_another_is_found_until_linked() {
        let mut p = quests_at_start(2);
        let (hermit, trouble) = (p.model.quests[0].id, p.model.quests[1].id);
        let found = |model: &Model, old: &HashSet<i64>| -> Vec<String> {
            chain_order(model, old).into_iter().map(|f| f.key).collect()
        };
        // Each is given by the NPC who takes back the other.
        assert_eq!(
            found(&p.model, &HashSet::new()),
            [format!("quest {hermit}"), format!("quest {trouble}")]
        );
        assert_eq!(found(&p.model, &HashSet::from([hermit])), [format!("quest {trouble}")]);
        p.model.quests[1].pre_all = vec![hermit];
        assert!(found(&p.model, &HashSet::new()).is_empty(), "linked both ways");
        // A quest asking a lower level cannot need one asking a higher one.
        p.model.quests[1].pre_all.clear();
        p.model.quests[0].min_level = 10;
        assert_eq!(found(&p.model, &HashSet::new()), [format!("quest {hermit}")]);
    }

    /// Call of Earth's case: a quest needing itself, or a quest exclusive with it, first. Not
    /// found for a prerequisite that is only exclusive with another quest.
    #[test]
    fn a_quest_needing_itself_or_an_exclusive_quest_first_is_found() {
        let mut p = quests_at_start(3);
        let (a, b, c) = (p.model.quests[0].id, p.model.quests[1].id, p.model.quests[2].id);
        let keys = |m: &Model| -> Vec<String> { contradictory_prerequisites(m).into_iter().map(|f| f.key).collect() };
        p.model.quests[1].pre_any = vec![a];
        p.model.quests[0].exclusive = vec![c];
        assert!(keys(&p.model).is_empty(), "a sequence");
        p.model.quests[0].pre_any = vec![a, b];
        assert_eq!(keys(&p.model), [format!("quest {a}")], "itself");
        p.model.quests[0].pre_any.clear();
        p.model.quests[2].pre_all = vec![a];
        assert_eq!(keys(&p.model), [format!("quest {c}")], "exclusive the other way");
    }

    /// A quest of the guide whose objectives give a time and that has no limit is found; with
    /// a limit, or not in the guide, it is not.
    #[test]
    fn a_quest_saying_a_time_without_a_limit_is_found() {
        let p = quests_at_start(2);
        let id = p.model.quests[0].id;
        let text = "Carry the Smoldering Incense to the shrine before the incense burns to ash in ten minutes!";
        assert!(says_a_time(text));
        assert!(!says_a_time("Kill 10 Plainstriders."));
        let untimed = HashMap::from([(id, text.to_owned()), (990_002, text.to_owned())]);
        let keys: Vec<String> = timed_quests(&p.model, &untimed).into_iter().map(|f| f.key).collect();
        assert_eq!(keys, [format!("quest {id}")]);
        assert!(timed_quests(&p.model, &HashMap::new()).is_empty());
    }
    /// A door given for one faction only is found (the other would take the shared one), and a
    /// door of an area that is no dungeon; both sides given, or a dungeon not planned, are not.
    #[test]
    fn a_dungeon_door_for_one_faction_only_is_found() {
        let mut p = quests_at_start(1);
        let mut dungeon = |area: i64, name: &str| {
            let here = p.model.start.clone();
            p.model.dungeons.push(crate::model::Dungeon {
                area,
                name: name.into(),
                entrance: here,
                min_level: 1,
                max_level: 60,
                minutes: 60.0,
                kills: 100.0,
                mob_level: 30,
                content: 0,
            });
        };
        dungeon(16544, "City of Dalaran");
        dungeon(719, "Blackfathom Deeps");
        let known = HashSet::from([16544, 719, 990_001]);
        let keys = |doors: &[Door]| -> Vec<String> {
            dungeon_entrances(&p.model, doors, &known)
                .into_iter()
                .map(|f| f.key)
                .collect()
        };
        let door = |area: i64, faction: &str| (area, faction.to_owned());
        assert!(keys(&[]).is_empty(), "no door given");
        assert_eq!(
            keys(&[door(16544, "A")]),
            ["City of Dalaran (16544)"],
            "the Horde has none"
        );
        assert_eq!(
            keys(&[door(16544, "H")]),
            ["City of Dalaran (16544)"],
            "the Alliance has none"
        );
        assert!(keys(&[door(16544, "A"), door(16544, "H")]).is_empty(), "both given");
        assert!(
            keys(&[door(990_001, "A")]).is_empty(),
            "a dungeon this guide does not plan"
        );
        assert_eq!(keys(&[door(999, "A"), door(999, "H")]), ["area 999"], "no such dungeon");
    }
}
