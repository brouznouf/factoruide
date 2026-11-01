//! Geography: zone coordinates <-> world coordinates, and the flight path network.

use crate::faction::Faction;
use anyhow::Result;
use rusqlite::Connection;
use rustc_hash::FxHashMap;
use std::cell::RefCell;
use std::collections::HashMap;
use std::sync::atomic::{AtomicU32, Ordering};

/// A point in world space (yards) on a continent.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Pos {
    pub continent: i64,
    pub x: f64,
    pub y: f64,
    /// AreaTable zone the point is in (0 when unknown).
    pub zone: i64,
    /// Height (yards) when known: the floor the point stands on where several lie over each
    /// other (a cave under a hill, a bridge over a river, the levels of a city).
    pub z: Option<i16>,
}

impl Pos {
    /// The height in a key of the memos (`z`): known heights apart from the unknown one.
    pub(crate) fn height_key(&self) -> u64 {
        self.z.map_or(0, |z| u64::from(z as u16) | 1 << 16)
    }

    pub fn dist(&self, o: &Pos) -> f64 {
        if self.continent != o.continent {
            return f64::INFINITY;
        }
        ((self.x - o.x).powi(2) + (self.y - o.y).powi(2)).sqrt()
    }
}

/// A height of the game's data (yards) as `Pos::z`: None when unknown or out of range.
pub fn height(z: Option<f64>) -> Option<i16> {
    z.filter(|z| z.is_finite() && z.abs() < 30000.0)
        .map(|z| z.round() as i16)
}

/// A point as the game UI shows it: zone (AreaTable), its map, percent coordinates.
#[derive(Debug, Clone, Copy, PartialEq, serde::Serialize)]
pub struct MapPoint {
    pub zone: i64,
    pub ui_map: i64,
    pub x: f64,
    pub y: f64,
}

#[derive(Debug, Clone)]
struct ZoneRect {
    ui_map: i64,
    continent: i64,
    min_x: f64,
    max_x: f64,
    min_y: f64,
    max_y: f64,
}

#[derive(Debug, Clone)]
pub struct TaxiNode {
    pub id: i64,
    pub name: String,
    pub pos: Pos,
}

/// Points of different zones closer than this (yards) may be walked between directly.
const ACROSS_MAX: f64 = 1500.0;
/// Yards around a passage where the terrain areas it joins are looked for: both ends of a
/// tunnel, of a bridge, the foot and the top of a lift, a road the terrain breaks.
pub(crate) const PASS_REACH: f64 = 150.0;
/// A walk between two large areas the terrain does not connect, with no passage between them:
/// the straight line, this much longer. Whatever the way is (a tunnel, a road the terrain
/// misses, around a mountain), it is not shorter, and a walk priced as the crow flies over
/// mountains makes flights look useless.
pub(crate) const UNKNOWN_DETOUR: f64 = 1.5;
/// A leg of `World::walk_path`: drawn straight or around the steep terrain.
#[derive(Clone, Copy, PartialEq, Eq)]
enum Leg {
    Straight,
    Terrain,
}
/// The most a walk through the passages joining two areas the terrain does not connect may
/// cost, times the straight line: farther around, a shorter way the terrain and the passages
/// do not know (a ramp, a path up a cliff) is likelier.
pub(crate) const BRIDGED_DETOUR: f64 = 3.0;

/// How a walk over the ground goes (`World::ground_way`).
#[derive(Debug, Clone, Copy)]
enum Way {
    /// Over the terrain (around the steep ground, or straight when it does not know).
    Terrain,
    /// Through passages.
    Bridge(Bridge),
    /// A way unknown, priced longer than the straight line.
    Straight,
}

/// A walk between two areas through the passages (`World::bridge`): its length, the passage
/// left `a`'s area by and the one entering `b`'s, with the index of the area of each
/// (`Pass::areas`).
#[derive(Debug, Clone, Copy)]
pub struct Bridge {
    pub dist: f64,
    pub from: (usize, usize),
    pub to: (usize, usize),
}

static NEXT_WORLD_ID: AtomicU32 = AtomicU32::new(1);

/// Walks already worked out in this thread (`World::walk_distance_at`), by world
/// (`World::walk_id`) and terrain (`Terrain::id`), level and ends (coordinates, then continent and zone of each): the trips
/// to the flight paths and ports nearest a place are walked again for every trip from it.
type WalkKey = (u64, u8, [u64; 7]);
thread_local! {
    static WALKED: RefCell<FxHashMap<WalkKey, f64>> = RefCell::new(FxHashMap::default());
}
const WALKED_MAX: usize = 1 << 17;

/// Walks between the passages (`World::pass_closed`): shortest distances and the next passage
/// on each.
type PassGraph = (Vec<Vec<f64>>, Vec<Vec<usize>>);

/// Times a walk through a closed zone costs, between two passages.
const CLOSED_COST: f64 = 5.0;
/// Zones with fewer hostile mobs than this have no level (small islands).
const ZONE_MOBS_MIN: usize = 30;
/// Levels above which a mob is a boss or unknown ("??"), not counted in a zone's level.
const ZONE_MOB_LEVEL_MAX: i64 = 70;

/// A lift of `World::area_lifts`: passage and the indices of its two sides.
type LiftSides = (usize, usize, usize);

pub struct World {
    zones: HashMap<i64, ZoneRect>,
    pub zone_names: HashMap<i64, String>,
    /// AreaTable parent of each subzone (a tavern, a valley), to find its zone.
    area_parents: HashMap<i64, i64>,
    /// The capital cities (AreaTable flag 8): towns every route passes through.
    capitals: std::collections::HashSet<i64>,
    pub taxi_nodes: Vec<TaxiNode>,
    /// Direct flight time in seconds between node indexes, INFINITY when no direct path.
    pub direct: Vec<Vec<f64>>,
    /// Price in copper of the direct flights, INFINITY when no direct path.
    pub direct_cost: Vec<Vec<f64>>,
    /// The way each direct flight goes (world x, y, its ends included), by node indexes.
    taxi_paths: HashMap<(usize, usize), Vec<(f64, f64)>>,
    pub links: Vec<Link>,
    /// Quests whose taking opens links (`LinkDef::after_quest`), with whether only while they
    /// are in the log (`LinkDef::while_held`).
    pub link_quests: Vec<(i64, bool)>,
    /// Time to chain links i..j (boarding i, leaving j), walking in between, by set of opened
    /// `link_quests` (bit k: quest k taken, or in the log).
    link_chains: Vec<Vec<Vec<f64>>>,
    /// Walkable passages between zones and the shortest walk between any two of them.
    pub passes: Vec<Pass>,
    pub pass_dist: Vec<Vec<f64>>,
    /// Next passage on the shortest walk from passage i to passage j.
    pass_next: Vec<Vec<usize>>,
    /// Whether the shortest walk from passage i to passage j takes a shortcut (them included).
    pass_shortcut: Vec<Vec<bool>>,
    /// Median level of the hostile mobs of each zone (`add_zone_levels`).
    zone_levels: FxHashMap<i64, u8>,
    /// Levels a zone's median hostile mob may be above the character's (the top of its band of
    /// 5) for walks to go in it (`add_zone_levels`).
    zone_gap: u8,
    /// The walk level of each band of 5 levels (`World::walk_level`).
    band_levels: Vec<u8>,
    /// The same walks between passages for a character of a level (the top of its band of 5),
    /// where crossing the zones too strong for it (`ZONE_GAP`) costs many times more: distances and next
    /// passages. The levels with no zone closed are not here (`pass_graph`).
    pass_closed: FxHashMap<u8, PassGraph>,
    pub zone_passes: FxHashMap<i64, Vec<usize>>,
    /// The passages joining each large terrain area (continent, area) to others: passage and
    /// index in its `areas`. Not the shortcuts nor the one-way passages (taken only on a walk
    /// between their zones).
    area_passes: FxHashMap<(i64, u32), Vec<(usize, usize)>>,
    /// The lifts (`PassDef::lift`) with two sides in the same large area (the floors join them
    /// only by a long way round: `Pass::areas` keeps such sides apart), by area: passage and the
    /// indices of the two sides in its `areas`, both ways.
    area_lifts: FxHashMap<(i64, u32), Vec<LiftSides>>,
    /// Tells this world's walks from another's in the thread memo (`WALKED`): new each time
    /// what they depend on changes (terrain, zone levels, passages).
    walk_id: u32,
    /// Flight master nodes by x (`taxi_near`).
    taxi_by_x: Vec<(f64, usize)>,
    /// Terrain too steep to walk: walks go around it.
    pub terrain: crate::terrain::Terrain,
    /// Named places of the zone maps (their exploration overlays): to tell which zone a point
    /// belongs to where zone maps overlap, and to name towns.
    landmarks: Vec<Landmark>,
}

/// A named place of a zone map (Ratchet, Valley of Trials...), at the center of its overlay.
struct Landmark {
    /// Its area (AreaTable ID).
    id: i64,
    pos: Pos,
    zone: i64,
    name: String,
}

/// A landmark names a point up to this distance (yards).
const LANDMARK_NAME_RADIUS: f64 = 400.0;

impl World {
    /// Load zones and the flight network usable by a faction ("Alliance" / "Horde").
    pub fn load(conn: &Connection, faction: Faction, flight_speed: f64, drop_tier: usize) -> Result<Self> {
        let mut zones = HashMap::new();
        let mut stmt = conn.prepare(
            "SELECT AreaID, UiMapID, MapID, Region_0, Region_3, Region_1, Region_4 \
             FROM client_uimapassignment WHERE AreaID != 0 AND OrderIndex = 0",
        )?;
        for row in stmt.query_map([], |r| {
            Ok((
                r.get::<_, i64>(0)?,
                ZoneRect {
                    ui_map: r.get(1)?,
                    continent: r.get(2)?,
                    min_x: r.get(3)?,
                    max_x: r.get(4)?,
                    min_y: r.get(5)?,
                    max_y: r.get(6)?,
                },
            ))
        })? {
            let (area, rect) = row?;
            zones.entry(area).or_insert(rect);
        }

        let mut zone_names = HashMap::new();
        let mut area_parents = HashMap::new();
        let mut capitals = std::collections::HashSet::new();
        let mut stmt = conn.prepare("SELECT ID, AreaName_lang, ParentAreaID, Flags_0 FROM client_areatable")?;
        for row in stmt.query_map([], |r| {
            Ok((
                r.get::<_, i64>(0)?,
                r.get::<_, String>(1)?,
                r.get::<_, i64>(2)?,
                r.get::<_, i64>(3)?,
            ))
        })? {
            let (id, name, parent, flags) = row?;
            if flags & 8 != 0 {
                capitals.insert(id);
            }
            zone_names.insert(id, name);
            if parent != 0 {
                area_parents.insert(id, parent);
            }
        }

        // MountCreatureID_0 is the Horde mount, _1 the Alliance one; Flags 2 shows the node on the
        // Horde's flight map, 1 on the Alliance's (not the quest flights' nodes, nor a duplicate of
        // a town's node for the other faction).
        let (mount_col, side) = if faction == Faction::Horde {
            ("MountCreatureID_0", 2)
        } else {
            ("MountCreatureID_1", 1)
        };
        let mut stmt = conn.prepare(&format!(
            "SELECT ID, Name_lang, ContinentID, Pos_0, Pos_1, Pos_2 FROM client_taxinodes
             WHERE {mount_col} != 0 AND (Flags & {side}) != 0"
        ))?;
        let taxi_nodes: Vec<TaxiNode> = stmt
            .query_map([], |r| {
                Ok(TaxiNode {
                    id: r.get(0)?,
                    name: r.get(1)?,
                    pos: Pos {
                        continent: r.get(2)?,
                        x: r.get(3)?,
                        y: r.get(4)?,
                        zone: 0,
                        z: height(r.get(5)?),
                    },
                })
            })?
            .collect::<rusqlite::Result<_>>()?;
        let index: HashMap<i64, usize> = taxi_nodes.iter().enumerate().map(|(i, n)| (n.id, i)).collect();

        // Direct flights: polyline length of each path.
        let n = taxi_nodes.len();
        let mut flight = vec![vec![f64::INFINITY; n]; n];
        for (i, row) in flight.iter_mut().enumerate() {
            row[i] = 0.0;
        }
        let mut stmt = conn.prepare(
            "SELECT p.FromTaxiNode, p.ToTaxiNode, n.Loc_0, n.Loc_1 FROM client_taxipath p \
             JOIN client_taxipathnode n ON n.PathID = p.ID ORDER BY p.ID, n.NodeIndex",
        )?;
        let mut paths: HashMap<(i64, i64), Vec<(f64, f64)>> = HashMap::new();
        for row in stmt.query_map([], |r| {
            Ok((
                r.get::<_, i64>(0)?,
                r.get::<_, i64>(1)?,
                r.get::<_, f64>(2)?,
                r.get::<_, f64>(3)?,
            ))
        })? {
            let (from, to, x, y) = row?;
            paths.entry((from, to)).or_default().push((x, y));
        }
        let mut taxi_paths = HashMap::new();
        for ((from, to), points) in paths {
            let (Some(&a), Some(&b)) = (index.get(&from), index.get(&to)) else {
                continue;
            };
            let len: f64 = points
                .windows(2)
                .map(|w| ((w[0].0 - w[1].0).powi(2) + (w[0].1 - w[1].1).powi(2)).sqrt())
                .sum();
            if len / flight_speed < flight[a][b] {
                flight[a][b] = len / flight_speed;
                taxi_paths.insert((a, b), points);
            }
        }
        let mut cost = vec![vec![f64::INFINITY; n]; n];
        for (i, row) in cost.iter_mut().enumerate() {
            row[i] = 0.0;
        }
        let mut stmt = conn.prepare("SELECT FromTaxiNode, ToTaxiNode, Cost FROM client_taxipath")?;
        for row in stmt.query_map([], |r| {
            Ok((r.get::<_, i64>(0)?, r.get::<_, i64>(1)?, r.get::<_, f64>(2)?))
        })? {
            let (from, to, price) = row?;
            if let (Some(&a), Some(&b)) = (index.get(&from), index.get(&to)) {
                cost[a][b] = cost[a][b].min(price);
            }
        }
        // A flight master flies to other nodes and back; one-way nodes are scripted flights
        // (quest, battleground, towers) without a flight master.
        let keep = round_trips(&flight);
        let kept = |rows: Vec<Vec<f64>>| -> Vec<Vec<f64>> {
            rows.into_iter()
                .zip(&keep)
                .filter(|(_, k)| **k)
                .map(|(row, _)| {
                    row.into_iter()
                        .zip(&keep)
                        .filter(|(_, k)| **k)
                        .map(|(v, _)| v)
                        .collect()
                })
                .collect()
        };
        let (flight, cost) = (kept(flight), kept(cost));
        // Node indexes once the others are left out.
        let renumbered: Vec<Option<usize>> = keep
            .iter()
            .scan(0, |next, &k| {
                let at = k.then_some(*next);
                *next += usize::from(k);
                Some(at)
            })
            .collect();
        let taxi_paths = taxi_paths
            .into_iter()
            .filter_map(|((a, b), points)| Some(((renumbered[a]?, renumbered[b]?), points)))
            .collect();
        let taxi_nodes: Vec<TaxiNode> = taxi_nodes
            .into_iter()
            .zip(&keep)
            .filter(|(_, k)| **k)
            .map(|(t, _)| t)
            .collect();
        let mut world = Self {
            zones,
            zone_names,
            area_parents,
            capitals,
            taxi_nodes,
            direct: flight,
            direct_cost: cost,
            taxi_paths,
            links: Vec::new(),
            link_quests: Vec::new(),
            link_chains: vec![Vec::new()],
            passes: Vec::new(),
            pass_dist: Vec::new(),
            pass_next: Vec::new(),
            pass_shortcut: Vec::new(),
            zone_levels: FxHashMap::default(),
            zone_gap: 0,
            band_levels: Vec::new(),
            pass_closed: FxHashMap::default(),
            zone_passes: FxHashMap::default(),
            taxi_by_x: Vec::new(),
            terrain: crate::terrain::Terrain::load(conn, drop_tier)?,
            landmarks: Vec::new(),
            area_passes: FxHashMap::default(),
            area_lifts: FxHashMap::default(),
            walk_id: NEXT_WORLD_ID.fetch_add(1, Ordering::Relaxed),
        };
        world.landmarks = world.load_landmarks(conn)?;
        for i in 0..world.taxi_nodes.len() {
            let p = world.taxi_nodes[i].pos;
            world.taxi_nodes[i].pos.zone = world.zone_at(&p).unwrap_or(0);
        }
        world.taxi_by_x = world.taxi_nodes.iter().enumerate().map(|(i, n)| (n.pos.x, i)).collect();
        world.taxi_by_x.sort_by(|a, b| a.0.total_cmp(&b.0));
        Ok(world)
    }

    /// Zone (AreaTable ID) shown by a UiMap.
    /// The map (UiMap ID) of a zone.
    pub(crate) fn ui_map_of_zone(&self, zone: i64) -> Option<i64> {
        self.zones.get(&zone).map(|z| z.ui_map)
    }

    pub fn zone_of_ui_map(&self, ui_map: i64) -> Option<i64> {
        self.zones.iter().find(|(_, z)| z.ui_map == ui_map).map(|(id, _)| *id)
    }

    /// Game map (continent) of a UiMap.
    pub fn continent_of_ui_map(&self, ui_map: i64) -> Option<i64> {
        self.zones.values().find(|z| z.ui_map == ui_map).map(|z| z.continent)
    }

    /// Zone percent coordinates (as the quest data stores them) to world space.
    pub fn to_world(&self, zone: i64, x: f64, y: f64) -> Option<Pos> {
        let z = self.zones.get(&zone)?;
        Some(Pos {
            continent: z.continent,
            x: z.max_x - y / 100.0 * (z.max_x - z.min_x),
            y: z.max_y - x / 100.0 * (z.max_y - z.min_y),
            zone,
            z: None,
        })
    }

    /// World position back to percent coordinates on a given zone's map.
    pub fn to_map(&self, zone: i64, p: &Pos) -> Option<MapPoint> {
        let z = self.zones.get(&zone)?;
        Some(MapPoint {
            zone,
            ui_map: z.ui_map,
            x: ((z.max_y - p.y) / (z.max_y - z.min_y) * 100.0 * 100.0).round() / 100.0,
            y: ((z.max_x - p.x) / (z.max_x - z.min_x) * 100.0 * 100.0).round() / 100.0,
        })
    }

    /// Whether a zone is a capital city.
    pub fn is_capital(&self, zone: i64) -> bool {
        self.capitals.contains(&zone)
    }

    pub fn zone_name(&self, zone: i64) -> &str {
        self.zone_names.get(&zone).map_or("?", String::as_str)
    }

    /// The zone (AreaTable parent, up to the top) an area belongs to: itself for a zone.
    pub(crate) fn area_zone(&self, mut area: i64) -> i64 {
        for _ in 0..8 {
            match self.area_parents.get(&area) {
                Some(&parent) => area = parent,
                None => break,
            }
        }
        area
    }

    /// Nearest flight master node index within `radius` yards.
    pub fn taxi_near(&self, p: &Pos, radius: f64) -> Option<usize> {
        // Asked at every stop: only the nodes about as far along x are looked at (a yard more
        // than `radius`, the exact test below), the first of the nearest kept.
        let from = self.taxi_by_x.partition_point(|n| n.0 < p.x - radius - 1.0);
        let mut best: Option<(f64, usize)> = None;
        for &(x, i) in &self.taxi_by_x[from..] {
            if x > p.x + radius + 1.0 {
                break;
            }
            let n = &self.taxi_nodes[i];
            if (n.pos.x - p.x).abs() > radius || (n.pos.y - p.y).abs() > radius {
                continue;
            }
            let d = n.pos.dist(p);
            if d <= radius && best.is_none_or(|b| d.total_cmp(&b.0).then(i.cmp(&b.1)).is_lt()) {
                best = Some((d, i));
            }
        }
        best.map(|(_, i)| i)
    }
}

impl World {
    /// Flights between nodes when only `known` nodes (bitmask of node indexes) can be used,
    /// including as relays (multi-hop flights only chain through discovered nodes): the time
    /// (seconds) of the fastest itinerary and its price (copper, each hop paid).
    pub fn flights(&self, known: u128) -> (Vec<Vec<f64>>, Vec<Vec<f64>>) {
        fastest_flights(&self.direct, &self.direct_cost, known)
    }

    /// The way the fastest flight from node `from` to node `to` goes through the `known` ones
    /// (as `flights` times it): the flight paths of its direct flights one after the other, its
    /// ends included. Straight between nodes whose path is not known; only the ends when no
    /// flight goes there.
    pub(crate) fn flight_path(&self, from: usize, to: usize, known: u128) -> Vec<Pos> {
        let n = self.taxi_nodes.len();
        let usable = |i: usize| i == from || i == to || known & (1u128 << i) != 0;
        // Dijkstra on the direct flights (few nodes: a scan for the nearest each time).
        let (mut best, mut before, mut done) = (vec![f64::INFINITY; n], vec![usize::MAX; n], vec![false; n]);
        best[from] = 0.0;
        while let Some(i) = (0..n)
            .filter(|&i| !done[i] && best[i].is_finite())
            .min_by(|&a, &b| best[a].total_cmp(&best[b]))
        {
            done[i] = true;
            if i == to {
                break;
            }
            for j in (0..n).filter(|&j| !done[j] && usable(j)) {
                let t = best[i] + self.direct[i][j];
                if t < best[j] {
                    best[j] = t;
                    before[j] = i;
                }
            }
        }
        let at = |i: usize, (x, y): (f64, f64)| Pos {
            x,
            y,
            ..self.taxi_nodes[i].pos
        };
        let node = |i: usize| self.taxi_nodes[i].pos;
        if !best[to].is_finite() || from == to {
            return vec![node(from), node(to)];
        }
        let mut hops = vec![to];
        while let Some(&i) = hops.last().filter(|&&i| i != from) {
            hops.push(before[i]);
        }
        hops.reverse();
        let mut path = vec![node(from)];
        for w in hops.windows(2) {
            if let Some(points) = self.taxi_paths.get(&(w[0], w[1])) {
                path.extend(points.iter().map(|&p| at(w[0], p)));
            }
            path.push(node(w[1]));
        }
        path
    }
}

/// Time and price of the fastest itinerary between each pair of `known` nodes, from the direct
/// flights' times and prices: both of the same itinerary.
fn fastest_flights(time: &[Vec<f64>], price: &[Vec<f64>], known: u128) -> (Vec<Vec<f64>>, Vec<Vec<f64>>) {
    let n = time.len();
    let mut t = vec![vec![f64::INFINITY; n]; n];
    let mut p = vec![vec![f64::INFINITY; n]; n];
    let is_known = |i: usize| known & (1u128 << i) != 0;
    for i in (0..n).filter(|&i| is_known(i)) {
        for j in (0..n).filter(|&j| is_known(j)) {
            t[i][j] = time[i][j];
            p[i][j] = price[i][j];
        }
        t[i][i] = 0.0;
        p[i][i] = 0.0;
    }
    for k in (0..n).filter(|&k| is_known(k)) {
        for i in 0..n {
            for j in 0..n {
                let via = t[i][k] + t[k][j];
                if via < t[i][j] {
                    t[i][j] = via;
                    p[i][j] = p[i][k] + p[k][j];
                }
            }
        }
    }
    (t, p)
}

/// Nodes with a flight to another node and a way back (through others or not).
fn round_trips(direct: &[Vec<f64>]) -> Vec<bool> {
    let n = direct.len();
    let mut reach: Vec<Vec<bool>> = (0..n)
        .map(|i| (0..n).map(|j| i != j && direct[i][j].is_finite()).collect())
        .collect();
    for k in 0..n {
        let via = reach[k].clone();
        for row in reach.iter_mut().filter(|row| row[k]) {
            for (r, v) in row.iter_mut().zip(&via) {
                *r |= *v;
            }
        }
    }
    (0..n)
        .map(|i| (0..n).any(|j| j != i && reach[i][j] && reach[j][i]))
        .collect()
}

#[cfg(test)]
mod tests {
    use super::{fastest_flights, round_trips};

    /// A quest flight (one way) is not a flight master: towns flying to each other are.
    #[test]
    fn one_way_nodes_are_not_flight_masters() {
        let inf = f64::INFINITY;
        let direct = [
            vec![0.0, 10.0, inf, inf],
            vec![inf, 0.0, 10.0, inf],
            vec![10.0, inf, 0.0, inf],
            vec![inf, inf, 10.0, 0.0],
        ];
        assert_eq!(round_trips(&direct), [true, true, true, false]);
    }

    /// Time and price come from the same itinerary: the fast direct flight and its price, not
    /// its time with the price of the slow connection.
    #[test]
    fn flight_time_and_price_of_one_itinerary() {
        let inf = f64::INFINITY;
        let time = [vec![0.0, 50.0, 10.0], vec![inf, 0.0, 50.0], vec![inf, inf, 0.0]];
        let price = [vec![0.0, 0.5, 10.0], vec![inf, 0.0, 0.5], vec![inf, inf, 0.0]];
        let (t, p) = fastest_flights(&time, &price, 0b111);
        assert!((t[0][2] - 10.0).abs() < 1e-9);
        assert!((p[0][2] - 10.0).abs() < 1e-9);
        // The relay unknown: still the direct flight.
        let (t, p) = fastest_flights(&time, &price, 0b101);
        assert!((t[0][2] - 10.0).abs() < 1e-9 && (p[0][2] - 10.0).abs() < 1e-9);
    }
}

impl World {
    /// Zone of a world position. Zone maps are rectangles that overlap (Durotar's covers Ratchet
    /// and the Crossroads): the smallest one wins when it is a city (no named places of its
    /// own), else the zone of the closest named place among the maps containing the point.
    pub fn zone_at(&self, p: &Pos) -> Option<i64> {
        let area = |z: &ZoneRect| (z.max_x - z.min_x) * (z.max_y - z.min_y);
        let mut containing: Vec<(i64, &ZoneRect)> = self
            .zones
            .iter()
            .filter(|(_, z)| {
                z.continent == p.continent && (z.min_x..=z.max_x).contains(&p.x) && (z.min_y..=z.max_y).contains(&p.y)
            })
            .map(|(id, z)| (*id, z))
            .collect();
        containing.sort_by(|a, b| area(a.1).total_cmp(&area(b.1)).then(a.0.cmp(&b.0)));
        let &(smallest, _) = containing.first()?;
        if containing.len() == 1 || !self.landmarks.iter().any(|l| l.zone == smallest) {
            return Some(smallest);
        }
        self.landmarks
            .iter()
            .filter(|l| l.pos.continent == p.continent && containing.iter().any(|(id, _)| *id == l.zone))
            .min_by(|a, b| a.pos.dist(p).total_cmp(&b.pos.dist(p)))
            .map_or(Some(smallest), |l| Some(l.zone))
    }

    /// Name of the place at a world position (a town, a valley), when a named place is close.
    pub fn place_name(&self, p: &Pos) -> Option<&str> {
        self.place(p).map(|(_, name)| name)
    }

    /// The place at a world position (a town, a valley), when a named place is close: its area
    /// (AreaTable ID, to name it in another language) and its English name.
    pub fn place(&self, p: &Pos) -> Option<(i64, &str)> {
        self.landmarks
            .iter()
            .filter(|l| l.pos.continent == p.continent && l.pos.dist(p) <= LANDMARK_NAME_RADIUS)
            .min_by(|a, b| a.pos.dist(p).total_cmp(&b.pos.dist(p)))
            .map(|l| (l.id, l.name.as_str()))
    }

    /// Named places: one per exploration overlay area of the zone maps, at the overlay's center.
    fn load_landmarks(&self, conn: &Connection) -> Result<Vec<Landmark>> {
        let mut stmt = conn.prepare(
            "SELECT t.ID, a.AreaID, t.AreaName_lang,
                    (o.OffsetX + o.TextureWidth / 2.0) * 100.0 / l.LayerWidth,
                    (o.OffsetY + o.TextureHeight / 2.0) * 100.0 / l.LayerHeight
             FROM client_worldmapoverlay o
             JOIN client_uimapxmapart x ON x.UiMapArtID = o.UiMapArtID
             JOIN client_uimapart ua ON ua.ID = o.UiMapArtID
             JOIN client_uimapartstylelayer l ON l.UiMapArtStyleID = ua.UiMapArtStyleID AND l.LayerIndex = 0
             JOIN client_uimapassignment a ON a.UiMapID = x.UiMapID AND a.OrderIndex = 0
             JOIN client_areatable t ON t.ID IN (o.AreaID_0, o.AreaID_1, o.AreaID_2, o.AreaID_3)",
        )?;
        let mut seen = std::collections::HashSet::new();
        let mut out = Vec::new();
        for row in stmt.query_map([], |r| {
            Ok((
                r.get::<_, i64>(0)?,
                r.get::<_, i64>(1)?,
                r.get::<_, String>(2)?,
                r.get::<_, f64>(3)?,
                r.get::<_, f64>(4)?,
            ))
        })? {
            let (id, zone, name, x, y) = row?;
            if seen.insert(id)
                && let Some(pos) = self.to_world(zone, x, y)
            {
                out.push(Landmark { id, pos, zone, name });
            }
        }
        Ok(out)
    }
}

#[derive(Debug, Clone, Copy, serde::Deserialize)]
pub struct ZonePoint {
    pub zone: i64,
    pub x: f64,
    pub y: f64,
    /// Height (yards) when the floor matters (the top of a tower): `Pos::z`.
    #[serde(default)]
    pub z: Option<f64>,
}

/// A transport link as configured in `overrides/travel.toml`.
#[derive(Debug, Clone, serde::Deserialize)]
pub struct LinkDef {
    pub name: String,
    pub faction: Option<Faction>,
    pub from: ZonePoint,
    pub to: ZonePoint,
    pub seconds: f64,
    #[serde(default)]
    pub one_way: bool,
    /// Taken at once (portal, orb, lift): no wait for a departure (`Params::link_wait`).
    #[serde(default)]
    pub instant: bool,
    /// Races allowed (keys of `races.toml`; empty: every race of the faction).
    #[serde(default)]
    pub races: Vec<String>,
    /// Quest ID: usable only once that quest is taken (the teleport its giver offers).
    #[serde(default)]
    pub after_quest: Option<i64>,
    /// With `after_quest`: usable only while that quest is in the log, not once turned in (the
    /// free flight Vesprystus gives to Auberdine with Flight to Auberdine).
    #[serde(default)]
    pub while_held: bool,
}

/// At most this many quests open links: the set of opened ones is a byte of the trip cache key.
pub(crate) const LINK_QUESTS_MAX: usize = 6;

/// A directed transport link.
#[derive(Debug, Clone)]
pub struct Link {
    pub name: String,
    pub from: Pos,
    pub from_zone: i64,
    pub to: Pos,
    pub to_zone: i64,
    pub seconds: f64,
    pub after_quest: Option<i64>,
    /// Open only while `after_quest` is in the log (`LinkDef::while_held`).
    pub while_held: bool,
}

impl Link {
    /// The quest opening it (`after_quest`), and whether only while it is in the log.
    fn opened_by(&self) -> Option<(i64, bool)> {
        self.after_quest.map(|q| (q, self.while_held))
    }
}

impl World {
    /// Register transport links usable by `faction`; `walk_speed` (yards/s, detour included)
    /// is used to chain links together.
    /// `wait`: average wait before boarding (boats and zeppelins leave every few minutes).
    pub fn add_links(&mut self, defs: &[LinkDef], faction: Faction, walk_speed: f64, wait: f64) {
        for d in defs {
            if d.faction.is_some_and(|f| f != faction) {
                continue;
            }
            let (Some(a), Some(b)) = (
                self.to_world(d.from.zone, d.from.x, d.from.y),
                self.to_world(d.to.zone, d.to.x, d.to.y),
            ) else {
                continue;
            };
            let (a, b) = (
                Pos {
                    z: height(d.from.z),
                    ..a
                },
                Pos { z: height(d.to.z), ..b },
            );
            let make = |from: Pos, fz, to: Pos, tz| Link {
                name: d.name.clone(),
                from,
                from_zone: fz,
                to,
                to_zone: tz,
                seconds: d.seconds + if d.instant { 0.0 } else { wait },
                after_quest: d.after_quest,
                while_held: d.while_held && d.after_quest.is_some(),
            };
            self.links.push(make(a, d.from.zone, b, d.to.zone));
            if !d.one_way {
                self.links.push(make(b, d.to.zone, a, d.from.zone));
            }
        }
        self.link_quests.clear();
        for q in self.links.iter().filter_map(Link::opened_by) {
            if !self.link_quests.contains(&q) {
                self.link_quests.push(q);
            }
        }
        // Beyond the cache key's room, the links of the last quests are always open.
        self.link_quests.truncate(LINK_QUESTS_MAX);
        self.link_chains = (0..1usize << self.link_quests.len())
            .map(|open| self.chain_links(open, walk_speed))
            .collect();
    }

    /// The links' chain table (see `link_chains`) with the links of the quests not in `open`
    /// closed.
    fn chain_links(&self, open: usize, walk_speed: f64) -> Vec<Vec<f64>> {
        let closed = |l: &Link| {
            l.opened_by()
                .and_then(|q| self.link_quests.iter().position(|&k| k == q))
                .is_some_and(|k| open & (1 << k) == 0)
        };
        // chain[i][j]: board link i, ..., get off link j.
        let n = self.links.len();
        let mut chain = vec![vec![f64::INFINITY; n]; n];
        for (i, (row, a)) in chain.iter_mut().zip(&self.links).enumerate() {
            if closed(a) {
                continue;
            }
            for (j, (cell, b)) in row.iter_mut().zip(&self.links).enumerate() {
                if i == j {
                    *cell = a.seconds;
                } else if !closed(b) {
                    let walk = self.walk_distance(&a.to, &b.from) / walk_speed;
                    *cell = a.seconds + walk + b.seconds;
                }
            }
        }
        for k in 0..n {
            for i in 0..n {
                for j in 0..n {
                    let via = chain[i][k] + chain[k][j] - self.links[k].seconds;
                    if via < chain[i][j] {
                        chain[i][j] = via;
                    }
                }
            }
        }
        chain
    }

    /// Time to chain links i..j (boarding i, leaving j) with the links of `link_quests` in
    /// `open` (bit k: quest k taken).
    pub(crate) fn link_chain(&self, open: u8) -> &[Vec<f64>] {
        &self.link_chains[usize::from(open) & (self.link_chains.len() - 1)]
    }

    /// Continents reachable from `start` on foot and through links.
    pub(crate) fn reachable_continents(&self, start: i64) -> std::collections::HashSet<i64> {
        let mut seen = std::collections::HashSet::from([start]);
        loop {
            let before = seen.len();
            for l in &self.links {
                if seen.contains(&l.from.continent) {
                    seen.insert(l.to.continent);
                }
            }
            if seen.len() == before {
                return seen;
            }
        }
    }
}

/// A walkable passage between two zones, as configured in `overrides/passes.toml`.
#[derive(Debug, Clone, serde::Deserialize)]
pub struct PassDef {
    pub name: String,
    pub zones: [i64; 2],
    #[serde(default)]
    pub subzone: Option<String>,
    #[serde(default)]
    pub at: Option<ZonePoint>,
    /// Off-road shortcut (mountain climb, jump...): used only when `Params::shortcuts` is on.
    #[serde(default)]
    pub shortcut: bool,
    /// Taken only from the first zone to the second (a drop that is not climbed back).
    #[serde(default)]
    pub one_way: bool,
    /// A lift or an elevator, which the floors do not hold (it moves): its sides in the same
    /// walkable area are still joined through it when the floors' way round is long.
    #[serde(default)]
    pub lift: bool,
}

/// A way the walkable floors hold that the game keeps closed (`[[closed]]` of
/// `overrides/passes.toml`): a locked gate, a door the server places. No walk goes within
/// `radius` yards of it.
#[derive(Debug, Clone, serde::Deserialize)]
pub struct ClosedDef {
    pub name: String,
    /// Where, on a zone's map, or in the world (continent, x, y).
    #[serde(default)]
    pub at: Option<ZonePoint>,
    #[serde(default)]
    pub world: Option<[f64; 3]>,
    pub radius: f64,
}

/// A large walkable area left only by boat, portal or lift (`[[enclosed]]` of
/// `overrides/passes.toml`): the terrain area at `at`.
#[derive(Debug, Clone, serde::Deserialize)]
pub struct EnclosedDef {
    pub name: String,
    pub at: ZonePoint,
}

/// A cave the terrain does not hold (`[[cave]]` of `overrides/passes.toml`): it holds the ground
/// above, so the places inside stand on steep ground, maybe nearer the other side of a ridge
/// than the mouth. The places on no large walkable area within `radius` yards of `inside` are
/// walked to through the mouth `at`.
#[derive(Debug, Clone, serde::Deserialize)]
pub struct CaveDef {
    pub name: String,
    /// The mouth, on the ground it opens on.
    pub at: ZonePoint,
    pub inside: ZonePoint,
    pub radius: f64,
}

#[derive(Debug, Clone)]
pub struct Pass {
    pub name: String,
    pub pos: Pos,
    pub zones: [i64; 2],
    pub shortcut: bool,
    pub one_way: bool,
    /// A lift (`PassDef::lift`).
    pub lift: bool,
    /// Its sides: the large terrain areas within `PASS_REACH` (the two ends of a tunnel, of a
    /// bridge, of a gap in a road), each area and its point nearest to the passage, and the
    /// points of an area the terrain walks to only by a long way around. Walked between freely.
    pub areas: Vec<(u32, Pos)>,
    /// The area of each side as the terrain labels it, before the gaps opened join areas.
    raw_areas: Vec<Option<u32>>,
}

impl Pass {
    /// Whether the passage is taken from zone `z`.
    fn taken_from(&self, z: i64) -> bool {
        if self.one_way {
            self.zones[0] == z
        } else {
            self.zones.contains(&z)
        }
    }

    /// Whether the passage leads into zone `z`.
    fn leads_to(&self, z: i64) -> bool {
        if self.one_way {
            self.zones[1] == z
        } else {
            self.zones.contains(&z)
        }
    }
}

impl World {
    /// Close the zones too strong for the character to walks: those whose median level of the
    /// mobs hostile to both factions is more than `gap` levels above the top of its band of 5
    /// (0: none is). No walk goes into a closed zone (a character a flight or a teleport left
    /// in one walks out), a walk across it costs `CLOSED_COST` times more. Before the passages.
    pub fn add_zone_levels(&mut self, conn: &Connection, gap: f64) -> Result<()> {
        let mut levels: FxHashMap<i64, Vec<u8>> = FxHashMap::default();
        if gap > 0.0 {
            let mut stmt = conn.prepare(
                "SELECT s.zone_id, coalesce(n.max_level, n.min_level, 1)
                 FROM m_spawn s JOIN m_npc n ON n.id = s.entity_id
                 WHERE s.entity_type = 'npc' AND s.x >= 0 AND coalesce(n.friendly_to, '') = ''",
            )?;
            for row in stmt.query_map([], |r| Ok((r.get::<_, i64>(0)?, r.get::<_, i64>(1)?)))? {
                let (zone, level) = row?;
                if (1..=ZONE_MOB_LEVEL_MAX).contains(&level) {
                    levels.entry(zone).or_default().push(level as u8);
                }
            }
        }
        self.zone_gap = gap.clamp(0.0, 100.0).round() as u8;
        // The zones with named places of their own: not the cities, whose few hostile mobs say
        // nothing of the walk to their trainers.
        self.zone_levels = (levels.into_iter())
            .filter(|(z, l)| l.len() >= ZONE_MOBS_MIN && self.landmarks.iter().any(|m| m.zone == *z))
            .map(|(z, mut l)| {
                l.sort_unstable();
                (z, l[l.len() / 2])
            })
            .collect();
        // Bands that close the same zones share a walk level (their walks are the same ones).
        let closed_at = |band: u8| self.zone_levels.values().filter(|&&l| l > band + self.zone_gap).count();
        self.band_levels = (1..=ZONE_MOB_LEVEL_MAX as u8 / 5)
            .map(|k| {
                let band = k * 5;
                let closed = closed_at(band);
                if closed == 0 {
                    0
                } else {
                    (5..=band).step_by(5).find(|&b| closed_at(b) == closed).unwrap_or(band)
                }
            })
            .collect();
        self.walk_id = NEXT_WORLD_ID.fetch_add(1, Ordering::Relaxed);
        Ok(())
    }

    /// What a character of `level` needs of the walks (`walk_distance_at`): 0 when no zone is
    /// closed to it, else the level of the first band of 5 closing the same zones.
    pub fn walk_level(&self, level: i64) -> u8 {
        let band = (level.clamp(1, 250) as usize).div_ceil(5);
        self.band_levels.get(band - 1).copied().unwrap_or(0)
    }

    /// Ways the game keeps closed (`Terrain::close`). Before the areas enclosed, the caves and
    /// the passages (their walks depend on it).
    pub fn close(&mut self, defs: &[ClosedDef]) -> Vec<String> {
        self.walk_id = NEXT_WORLD_ID.fetch_add(1, Ordering::Relaxed);
        let mut warnings = Vec::new();
        for d in defs {
            let p = match (&d.at, d.world) {
                (_, Some([continent, x, y])) => Some(Pos {
                    continent: continent as i64,
                    x,
                    y,
                    zone: 0,
                    z: None,
                }),
                (Some(at), None) => self.to_world(at.zone, at.x, at.y),
                (None, None) => None,
            };
            let closed = p.is_some_and(|p| self.terrain.close(&p, d.radius));
            if !closed && !self.terrain.is_empty() {
                warnings.push(format!("closed way {}: no walkable floor there", d.name));
            }
        }
        warnings
    }

    /// Areas left only by boat, portal or lift: no walk to or from them. Before the passages
    /// (their walks depend on it).
    pub fn enclose(&mut self, defs: &[EnclosedDef]) -> Vec<String> {
        self.walk_id = NEXT_WORLD_ID.fetch_add(1, Ordering::Relaxed);
        let mut warnings = Vec::new();
        // A valley the gaps opened join to the mainland is taken out of it first (the areas are
        // numbered anew).
        for d in defs {
            if let Some(p) = self.to_world(d.at.zone, d.at.x, d.at.y) {
                self.terrain.detach(&p);
            }
        }
        // The entries of one name are one place: walks between its areas are not cut.
        let mut groups: Vec<&str> = Vec::new();
        for d in defs {
            let group = groups.iter().position(|&g| g == d.name).unwrap_or_else(|| {
                groups.push(&d.name);
                groups.len() - 1
            }) as u32;
            let enclosed = self
                .to_world(d.at.zone, d.at.x, d.at.y)
                .and_then(|p| self.terrain.enclose(&p, group));
            if enclosed.is_none() && !self.terrain.is_empty() {
                warnings.push(format!("enclosed area {}: not in a large area of the terrain", d.name));
            }
        }
        warnings
    }

    /// Caves the terrain does not hold: walks to the places inside go through their mouth. Before
    /// the passages and links (their walks depend on it).
    pub fn add_caves(&mut self, defs: &[CaveDef]) -> Vec<String> {
        self.walk_id = NEXT_WORLD_ID.fetch_add(1, Ordering::Relaxed);
        let mut warnings = Vec::new();
        for d in defs {
            let added = match (
                self.to_world(d.at.zone, d.at.x, d.at.y),
                self.to_world(d.inside.zone, d.inside.x, d.inside.y),
            ) {
                (Some(mouth), Some(inside)) => self.terrain.add_cave(&mouth, &inside, d.radius),
                _ => false,
            };
            if !added && !self.terrain.is_empty() {
                warnings.push(format!(
                    "cave {}: its mouth is not in a large area of the terrain",
                    d.name
                ));
            }
        }
        warnings
    }

    /// Register passages; subzone positions come from the map exploration overlays.
    pub fn add_passes(&mut self, conn: &Connection, defs: &[PassDef]) -> Result<Vec<String>> {
        // Center of each explorable map area, per zone.
        let mut areas: HashMap<String, Vec<(i64, f64, f64)>> = HashMap::new();
        let query = "SELECT t.AreaName_lang, a.AreaID,
                (o.OffsetX + o.TextureWidth / 2.0) * 100.0 / l.LayerWidth,
                (o.OffsetY + o.TextureHeight / 2.0) * 100.0 / l.LayerHeight
            FROM client_worldmapoverlay o
            JOIN client_uimapxmapart x ON x.UiMapArtID = o.UiMapArtID
            JOIN client_uimapart ua ON ua.ID = o.UiMapArtID
            JOIN client_uimapartstylelayer l ON l.UiMapArtStyleID = ua.UiMapArtStyleID AND l.LayerIndex = 0
            JOIN client_uimapassignment a ON a.UiMapID = x.UiMapID AND a.OrderIndex = 0
            JOIN client_areatable t ON t.ID = o.AreaID_0";
        if let Ok(mut stmt) = conn.prepare(query) {
            for row in stmt.query_map([], |r| Ok((r.get::<_, String>(0)?, r.get(1)?, r.get(2)?, r.get(3)?)))? {
                let (name, zone, x, y) = row?;
                areas.entry(name).or_default().push((zone, x, y));
            }
        }
        let mut warnings = Vec::new();
        for d in defs {
            let point = match (&d.at, &d.subzone) {
                (Some(at), _) => Some((at.zone, at.x, at.y)),
                (None, Some(name)) => areas
                    .get(name)
                    .and_then(|list| list.iter().find(|(z, _, _)| d.zones.contains(z)).or(list.first()))
                    .copied(),
                _ => None,
            };
            let Some((zone, x, y)) = point else {
                warnings.push(format!("pass {}: position unknown", d.name));
                continue;
            };
            match self.to_world(zone, x, y) {
                Some(pos) => self.passes.push(Pass {
                    name: d.name.clone(),
                    pos,
                    zones: d.zones,
                    shortcut: d.shortcut,
                    one_way: d.one_way,
                    lift: d.lift,
                    areas: Vec::new(),
                    raw_areas: Vec::new(),
                }),
                None => warnings.push(format!("pass {}: zone {zone} has no map", d.name)),
            }
        }
        self.zone_passes.clear();
        for (i, p) in self.passes.iter().enumerate() {
            for z in p.zones {
                if !self.zone_passes.get(&z).is_some_and(|l| l.contains(&i)) {
                    self.zone_passes.entry(z).or_default().push(i);
                }
            }
        }
        self.area_passes.clear();
        self.area_lifts.clear();
        for i in 0..self.passes.len() {
            let areas = self.pass_areas(&self.passes[i].pos);
            let pass = &mut self.passes[i];
            pass.raw_areas = areas
                .iter()
                .map(|(_, p)| self.terrain.large_areas_of(p).map(|(raw, _)| raw))
                .collect();
            pass.areas = areas;
            if pass.shortcut || pass.one_way {
                continue;
            }
            for (k, (area, _)) in pass.areas.iter().enumerate() {
                self.area_passes
                    .entry((pass.pos.continent, *area))
                    .or_default()
                    .push((i, k));
                for (j, (other, _)) in pass.areas.iter().enumerate() {
                    if pass.lift && j != k && other == area {
                        self.area_lifts
                            .entry((pass.pos.continent, *area))
                            .or_default()
                            .push((i, k, j));
                    }
                }
            }
        }
        self.walk_id = NEXT_WORLD_ID.fetch_add(1, Ordering::Relaxed);
        // Walking from a passage to another taken from the zone it leads into, then all-pairs
        // shortest walks (one-way passages: not the same both ways). Between two passages of
        // the same terrain area, over the terrain from their points in it (a passage may stand
        // on the other side of a gap the terrain does not cross).
        let n = self.passes.len();
        // The walk from each passage to the next, and the zone(s) it goes through.
        let mut edges = vec![vec![f64::INFINITY; n]; n];
        for (i, (row, a)) in edges.iter_mut().zip(&self.passes).enumerate() {
            for (j, (cell, b)) in row.iter_mut().zip(&self.passes).enumerate() {
                if i != j && a.zones.iter().any(|&z| a.leads_to(z) && b.taken_from(z)) {
                    *cell = self.between_passes(a, b);
                }
            }
        }
        let (d, next) = self.pass_graph(&edges, |_| false);
        self.pass_closed.clear();
        if !self.zone_levels.is_empty() {
            for &level in self.band_levels.iter().filter(|&&l| l != 0) {
                if !self.pass_closed.contains_key(&level) {
                    let graph = self.pass_graph(&edges, |z| self.zone_closed(z, level));
                    self.pass_closed.insert(level, graph);
                }
            }
        }
        self.pass_dist = d;
        self.pass_next = next;
        self.pass_shortcut = (0..n)
            .map(|i| {
                (0..n)
                    .map(|j| self.pass_path(i, j).iter().any(|&k| self.passes[k].shortcut))
                    .collect()
            })
            .collect();
        Ok(warnings)
    }

    /// The sides of a passage (`Pass::areas`): the large terrain areas around it, its own
    /// first, each with its point nearest to it; and in an area, the points the terrain walks
    /// to only by a long way around (the other end of a bridge, of a tunnel, of a gate the
    /// terrain joins elsewhere).
    fn pass_areas(&self, at: &Pos) -> Vec<(u32, Pos)> {
        let mut sides: Vec<(u32, Pos)> = Vec::new();
        for r in [0.0, 50.0, 100.0, PASS_REACH] {
            for k in 0..16 {
                let angle = f64::from(k) * std::f64::consts::PI / 8.0;
                let p = Pos {
                    x: at.x + r * angle.cos(),
                    y: at.y + r * angle.sin(),
                    ..*at
                };
                if let Some((area, size)) = self.terrain.area_of(&p)
                    && area != 0
                    && size >= crate::terrain::LARGE_AREA
                    && sides.iter().filter(|&&(a, _)| a == area).all(|(_, q)| {
                        let near = 2.0 * q.dist(&p) + 50.0;
                        !self.terrain.distance(q, &p).is_some_and(|d| d <= near)
                            || !self.terrain.distance(&p, q).is_some_and(|d| d <= near)
                    })
                {
                    sides.push((area, p));
                }
                if r == 0.0 {
                    break;
                }
            }
        }
        sides
    }

    /// The sides of passage `i` a walk from or to `p` goes through (`Pass::areas`), nearest
    /// first, each with its distance to the passage: those in `p`'s terrain area as labelled,
    /// else those in its area once the gaps are opened, else all (`p` in no large area, or in
    /// one the passage does not stand in: walked to through other passages, `bridge`).
    fn sides_near(&self, p: &Pos, i: usize) -> Vec<(f64, usize, f64)> {
        let pass = &self.passes[i];
        let mut keep: Vec<usize> = Vec::new();
        if let Some((raw, area)) = self.terrain.large_areas_of(p) {
            keep.extend((0..pass.areas.len()).filter(|&k| pass.raw_areas[k] == Some(raw)));
            if keep.is_empty() {
                keep.extend((0..pass.areas.len()).filter(|&k| pass.areas[k].0 == area));
            }
        }
        // In an area no side is in (a valley left by a tunnel): walked to through the passages
        // joining them (`ground`, `bridge`).
        if keep.is_empty() {
            keep.extend(0..pass.areas.len());
        }
        let mut sides: Vec<(f64, usize, f64)> = keep
            .into_iter()
            .map(|k| {
                let off = pass.areas[k].1.dist(&pass.pos);
                (p.dist(&pass.areas[k].1) + off, k, off)
            })
            .collect();
        sides.sort_by(|x, y| x.0.total_cmp(&y.0).then(x.1.cmp(&y.1)));
        sides
    }

    /// Walk from `a` to passage `i`, to the nearest of its sides (`sides_near`), and the side
    /// (None: to its point, when it has no side: no large area around).
    fn walk_to_pass(&self, a: &Pos, i: usize, level: u8) -> (f64, Option<usize>) {
        let pass = &self.passes[i];
        if pass.areas.is_empty() {
            return (self.ground(a, &pass.pos, level), None);
        }
        let mut best = (f64::INFINITY, None);
        for (least, k, off) in self.sides_near(a, i) {
            if least >= best.0 {
                break;
            }
            let d = self.ground(a, &pass.areas[k].1, level) + off;
            if d < best.0 {
                best = (d, Some(k));
            }
        }
        best
    }

    /// Walk from passage `j` to `b`, the same way.
    fn walk_from_pass(&self, j: usize, b: &Pos, level: u8) -> (f64, Option<usize>) {
        let pass = &self.passes[j];
        if pass.areas.is_empty() {
            return (self.ground(&pass.pos, b, level), None);
        }
        let mut best = (f64::INFINITY, None);
        for (least, k, off) in self.sides_near(b, j) {
            if least >= best.0 {
                break;
            }
            let d = self.ground(&pass.areas[k].1, b, level) + off;
            if d < best.0 {
                best = (d, Some(k));
            }
        }
        best
    }

    /// Walk between two passages of a zone: over the terrain from their points in an area they
    /// share, else around the steep terrain, else in a straight line (a road the terrain
    /// breaks). Never more than `UNKNOWN_DETOUR` times the straight line: the passages of a
    /// zone are joined by its roads, which the terrain may break into pieces it joins by a long
    /// way around (Deadwind Pass's road between Duskwood and the Swamp of Sorrows).
    fn between_passes(&self, a: &Pass, b: &Pass) -> f64 {
        let straight = a.pos.dist(&b.pos);
        let mut best = self.terrain.distance(&a.pos, &b.pos).unwrap_or(straight);
        for (area, pa) in &a.areas {
            for (other, pb) in &b.areas {
                if area == other
                    && let Some(d) = self.terrain.distance(pa, pb)
                    && d.is_finite()
                {
                    best = best.min(a.pos.dist(pa) + d + pb.dist(&b.pos));
                }
            }
        }
        best.min(straight * UNKNOWN_DETOUR)
    }

    /// All-pairs shortest walks between passages from the walks `edges` between those that
    /// join, `CLOSED_COST` times longer through the zones `closed` says (a walk from a passage
    /// into a zone and out of it through the next goes through the zone).
    fn pass_graph(&self, edges: &[Vec<f64>], closed: impl Fn(i64) -> bool) -> PassGraph {
        let n = self.passes.len();
        let mut d = vec![vec![f64::INFINITY; n]; n];
        let mut next: Vec<Vec<usize>> = (0..n).map(|_| (0..n).collect()).collect();
        for (i, a) in self.passes.iter().enumerate() {
            d[i][i] = 0.0;
            for (j, b) in self.passes.iter().enumerate() {
                // Through a closed zone a walk costs many times its length, not nothing: a
                // character a teleport or a flight left in one (Moonglade) gets out of it.
                let through = a.zones.iter().filter(|&&z| a.leads_to(z) && b.taken_from(z));
                let factor = through
                    .map(|&z| if closed(z) { CLOSED_COST } else { 1.0 })
                    .min_by(f64::total_cmp);
                if i != j
                    && let Some(factor) = factor
                {
                    d[i][j] = edges[i][j] * factor;
                }
            }
        }
        for k in 0..n {
            for i in 0..n {
                for j in 0..n {
                    let via = d[i][k] + d[k][j];
                    if via < d[i][j] {
                        d[i][j] = via;
                        next[i][j] = next[i][k];
                    }
                }
            }
        }
        (d, next)
    }

    /// Whether walks do not go into zone `z` at walk level `level` (0: no zone is closed): its
    /// median hostile mob is more than `zone_gap` levels above.
    pub fn zone_closed(&self, z: i64, level: u8) -> bool {
        level > 0
            && self
                .zone_levels
                .get(&z)
                .is_some_and(|&l| l > level.saturating_add(self.zone_gap))
    }

    /// The walks between passages for a character of `level` (0: no zone is closed).
    fn pass_graph_at(&self, level: u8) -> (&Vec<Vec<f64>>, &Vec<Vec<usize>>) {
        match self.pass_closed.get(&level) {
            Some((d, next)) => (d, next),
            None => (&self.pass_dist, &self.pass_next),
        }
    }

    /// The passages of the shortest walk from passage `i` to passage `j`, both included (none
    /// when no walk goes there).
    pub(crate) fn pass_path(&self, i: usize, j: usize) -> Vec<usize> {
        self.pass_path_at(i, j, 0)
    }

    /// The same for a character of `level` (`pass_graph_at`).
    fn pass_path_at(&self, i: usize, j: usize, level: u8) -> Vec<usize> {
        let (dist, next) = self.pass_graph_at(level);
        if dist[i][j].is_infinite() {
            return Vec::new();
        }
        let mut path = vec![i];
        let mut k = i;
        while k != j {
            k = next[k][j];
            path.push(k);
        }
        path
    }

    /// The shortcuts (off-road passages) the walk from `a` to `b` takes, in order.
    pub fn walk_shortcuts(&self, a: &Pos, b: &Pos) -> Vec<usize> {
        if a.zone == b.zone {
            return Vec::new();
        }
        let (Some(pa), Some(pb)) = (self.zone_passes.get(&a.zone), self.zone_passes.get(&b.zone)) else {
            return Vec::new();
        };
        // Most walks: no shortcut between these zones, the walk itself is not looked for.
        if !pa.iter().any(|&i| pb.iter().any(|&j| self.pass_shortcut[i][j])) {
            return Vec::new();
        }
        match self.through_passes(a, b, pa, pb, 0) {
            (t, Some((i, j))) if t.is_finite() => (self.pass_path(i, j).into_iter())
                .filter(|&k| self.passes[k].shortcut)
                .collect(),
            _ => Vec::new(),
        }
    }

    /// Walking distance (yards): around the steep terrain within a zone, through passages between
    /// zones.
    /// Zones the passage graph does not cover fall back to a straight line, unless one of the
    /// places is in an enclosed area. Two zones it covers with no passage leading from one to the
    /// other have no walk between them (only links and flights: the tram, a boat).
    /// Walking distance and the passages used to leave `a`'s zone and enter `b`'s (debugging).
    pub fn walk_route(&self, a: &Pos, b: &Pos) -> (f64, Option<(usize, usize)>) {
        if a.zone == b.zone {
            return (self.ground(a, b, 0), None);
        }
        let (Some(pa), Some(pb)) = (self.zone_passes.get(&a.zone), self.zone_passes.get(&b.zone)) else {
            return (self.ground(a, b, 0), None);
        };
        self.through_passes(a, b, pa, pb, 0)
    }

    pub fn walk_distance(&self, a: &Pos, b: &Pos) -> f64 {
        self.walk_distance_at(a, b, 0)
    }

    /// The same for a character with the walk level `level` (`walk_level`): the zones too strong
    /// for it are closed.
    pub fn walk_distance_at(&self, a: &Pos, b: &Pos, level: u8) -> f64 {
        let head =
            |p: &Pos| Some(u64::from(u32::try_from(p.continent).ok()?) << 32 | u64::from(u32::try_from(p.zone).ok()?));
        let (Some(ha), Some(hb)) = (head(a), head(b)) else {
            return self.compute_walk(a, b, level);
        };
        let key = (
            u64::from(self.walk_id) << 32 | u64::from(self.terrain.id()),
            level,
            [
                a.x.to_bits(),
                a.y.to_bits(),
                b.x.to_bits(),
                b.y.to_bits(),
                ha,
                hb,
                a.height_key() << 32 | b.height_key(),
            ],
        );
        if let Some(d) = WALKED.with(|m| m.borrow().get(&key).copied()) {
            return d;
        }
        let d = self.compute_walk(a, b, level);
        WALKED.with(|m| {
            let mut m = m.borrow_mut();
            if m.len() >= WALKED_MAX {
                m.clear();
            }
            m.insert(key, d);
        });
        d
    }

    fn compute_walk(&self, a: &Pos, b: &Pos, level: u8) -> f64 {
        let straight = a.dist(b);
        if !straight.is_finite() {
            return straight;
        }
        if a.zone == b.zone {
            return self.ground(a, b, level);
        }
        // No walk into a zone too strong for the character (out of one: yes).
        if self.zone_closed(b.zone, level) {
            return f64::INFINITY;
        }
        let (Some(pa), Some(pb)) = (self.zone_passes.get(&a.zone), self.zone_passes.get(&b.zone)) else {
            return self.ground(a, b, level);
        };
        let best = self.through_passes(a, b, pa, pb, level).0;
        let passes = &self.passes;
        let dist = self.pass_graph_at(level).0;
        if best.is_finite() {
            best
        } else if pa.iter().all(|&i| {
            pb.iter()
                .all(|&j| !passes[i].taken_from(a.zone) || !passes[j].leads_to(b.zone) || dist[i][j].is_infinite())
        }) {
            f64::INFINITY
        } else {
            self.ground(a, b, level)
        }
    }

    /// The points the walk from `a` to `b` of `walk_distance_at` goes through, ends included: the
    /// passages it takes between zones, the turns around the steep terrain
    /// (`Terrain::waypoints`). Only the ends when it goes straight, or when no walk goes there.
    pub fn walk_path(&self, a: &Pos, b: &Pos, level: u8) -> Vec<Pos> {
        self.path_points(a, self.walk_legs(a, b, level))
    }

    /// Outdoor ground known along the whole walk, excluding passages and estimated links.
    pub fn exposed_walk(&self, a: &Pos, b: &Pos, level: u8) -> bool {
        let legs = self.walk_legs(a, b, level);
        if legs.iter().any(|(_, _, leg)| *leg != Leg::Terrain) {
            return false;
        }
        let path = self.path_points(a, legs);
        path.len() >= 2 && path.windows(2).all(|p| self.terrain.exposed_ground_line(&p[0], &p[1]))
    }

    fn walk_legs(&self, a: &Pos, b: &Pos, level: u8) -> Vec<(Pos, Pos, Leg)> {
        // The legs: those drawn straight (to or through a passage) or over the terrain.
        let mut legs: Vec<(Pos, Pos, Leg)> = Vec::new();
        if a.zone != b.zone
            && a.continent == b.continent
            && let (Some(pa), Some(pb)) = (self.zone_passes.get(&a.zone), self.zone_passes.get(&b.zone))
            && let (t, Some((i, j))) = self.through_passes(a, b, pa, pb, level)
            && t.is_finite()
        {
            let chain = self.pass_path_at(i, j, level);
            let side = |k: usize, s: Option<usize>| s.map_or(self.passes[k].pos, |s| self.passes[k].areas[s].1);
            let first = side(i, self.walk_to_pass(a, i, level).1);
            self.ground_legs(a, &first, level, &mut legs);
            legs.push((first, self.passes[i].pos, Leg::Straight));
            self.chain_legs(&chain, &mut legs);
            let last = side(j, self.walk_from_pass(j, b, level).1);
            legs.push((self.passes[j].pos, last, Leg::Straight));
            self.ground_legs(&last, b, level, &mut legs);
        } else {
            self.ground_legs(a, b, level, &mut legs);
        }
        legs
    }

    fn path_points(&self, a: &Pos, legs: Vec<(Pos, Pos, Leg)>) -> Vec<Pos> {
        let mut path = vec![*a];
        for (from, to, leg) in legs {
            if leg == Leg::Terrain {
                path.extend(self.terrain.waypoints(&from, &to));
            }
            path.push(to);
        }
        path.dedup_by(|p, q| p.dist(q) < 1e-6);
        path
    }

    /// The legs of a walk over the ground (`ground`): straight or around the steep terrain, or
    /// through the passages joining two areas the terrain does not connect.
    fn ground_legs(&self, a: &Pos, b: &Pos, level: u8, legs: &mut Vec<(Pos, Pos, Leg)>) {
        match self.ground_way(a, b, level).1 {
            Way::Bridge(bridge) => {
                let (i, ka) = bridge.from;
                let (j, kb) = bridge.to;
                let (near_a, near_b) = (self.passes[i].areas[ka].1, self.passes[j].areas[kb].1);
                legs.push((*a, near_a, Leg::Terrain));
                legs.push((near_a, self.passes[i].pos, Leg::Straight));
                self.chain_legs(&self.pass_path_at(i, j, level), legs);
                legs.push((self.passes[j].pos, near_b, Leg::Straight));
                legs.push((near_b, *b, Leg::Terrain));
            }
            Way::Terrain => legs.push((*a, *b, Leg::Terrain)),
            Way::Straight => legs.push((*a, *b, Leg::Straight)),
        }
    }

    /// The legs from passage to passage of a chain (`pass_path`): through their points in an
    /// area they share (`between_passes`), else straight.
    fn chain_legs(&self, chain: &[usize], legs: &mut Vec<(Pos, Pos, Leg)>) {
        for w in chain.windows(2) {
            let (a, b) = (&self.passes[w[0]], &self.passes[w[1]]);
            let straight = a.pos.dist(&b.pos) * UNKNOWN_DETOUR;
            let direct = self
                .terrain
                .distance(&a.pos, &b.pos)
                .filter(|d| d.is_finite() && *d <= straight)
                .map(|d| (d, a.pos, b.pos));
            let shared = (a.areas.iter())
                .flat_map(|(area, pa)| {
                    (b.areas.iter())
                        .filter(move |(o, _)| o == area)
                        .map(move |(_, pb)| (pa, pb))
                })
                .filter_map(|(pa, pb)| {
                    let d = self.terrain.distance(pa, pb).filter(|d| d.is_finite())?;
                    Some((a.pos.dist(pa) + d + pb.dist(&b.pos), *pa, *pb))
                })
                .chain(direct)
                .filter(|w| w.0 <= straight)
                .min_by(|x, y| x.0.total_cmp(&y.0));
            match shared {
                Some((_, pa, pb)) => {
                    legs.push((a.pos, pa, Leg::Straight));
                    legs.push((pa, pb, Leg::Terrain));
                    legs.push((pb, b.pos, Leg::Straight));
                }
                None => legs.push((a.pos, b.pos, Leg::Straight)),
            }
        }
    }

    /// Shortest walk from `a` to `b` through the passages of their zones (`pa`, `pb`), or across
    /// the border when shorter, and the first and last passage it takes.
    ///
    /// A walk is never shorter than the straight line: the pairs of passages are weighed from
    /// the least straight trip up, and only walked to while that could beat the best (ties: the
    /// first pair in the order of `pa` then `pb`, after the walk across).
    fn through_passes(&self, a: &Pos, b: &Pos, pa: &[usize], pb: &[usize], level: u8) -> (f64, Option<(usize, usize)>) {
        let mut best = (self.across(a, b), None);
        let dist = self.pass_graph_at(level).0;
        let mut best_order = (0, 0);
        let mut pairs: Vec<(f64, usize, usize)> = Vec::new();
        for (ki, &i) in pa.iter().enumerate() {
            if !self.passes[i].taken_from(a.zone) {
                continue;
            }
            let la = a.dist(&self.passes[i].pos);
            for (kj, &j) in pb.iter().enumerate() {
                if self.passes[j].leads_to(b.zone) {
                    let least = la + dist[i][j] + self.passes[j].pos.dist(b);
                    if least.is_finite() {
                        pairs.push((least, ki, kj));
                    }
                }
            }
        }
        pairs.sort_by(|x, y| x.0.total_cmp(&y.0).then((x.1, x.2).cmp(&(y.1, y.2))));
        // Each walk to a passage once.
        let mut from_a: Vec<Option<f64>> = vec![None; pa.len()];
        let mut to_b: Vec<Option<f64>> = vec![None; pb.len()];
        for (least, ki, kj) in pairs {
            if least > best.0 {
                break;
            }
            let (i, j) = (pa[ki], pb[kj]);
            let da = *from_a[ki].get_or_insert_with(|| self.walk_to_pass(a, i, level).0);
            let db = *to_b[kj].get_or_insert_with(|| self.walk_from_pass(j, b, level).0);
            let t = da + dist[i][j] + db;
            let better = match t.partial_cmp(&best.0) {
                Some(std::cmp::Ordering::Less) => true,
                Some(std::cmp::Ordering::Equal) => best.1.is_some() && (ki, kj) < best_order,
                _ => false,
            };
            if better {
                best = (t, Some((i, j)));
                best_order = (ki, kj);
            }
        }
        best
    }

    /// Walk over the ground: around the steep terrain. Between two large areas the terrain
    /// labels apart (`Terrain::disconnected`), the shortest of its way through the gaps it
    /// opened and the way through the passages joining them (`bridge`), but no more than
    /// `UNKNOWN_DETOUR` times the straight line when no passage joins them, `BRIDGED_DETOUR`
    /// times when one does (farther around, an unknown shorter way is likelier). To or from an
    /// enclosed area, only through the passages (infinite without). The straight line when an
    /// end is in no large area (a building, a cave, a ledge).
    fn ground(&self, a: &Pos, b: &Pos, level: u8) -> f64 {
        self.ground_way(a, b, level).0
    }

    /// `ground` and the way it goes.
    fn ground_way(&self, a: &Pos, b: &Pos, level: u8) -> (f64, Way) {
        let straight = a.dist(b);
        let known = self.terrain.distance(a, b);
        if known.is_some_and(f64::is_infinite) {
            // Another continent, or an enclosed area and another.
            return match self.bridge(a, b, level) {
                Some(bridge) if a.continent == b.continent => (bridge.dist, Way::Bridge(bridge)),
                _ => (f64::INFINITY, Way::Terrain),
            };
        }
        if !self.terrain.disconnected(a, b) {
            let walk = known.unwrap_or(straight);
            // The floors' way round, much longer than a lift the passages know.
            if known.is_some()
                && walk > straight * UNKNOWN_DETOUR
                && let Some(lift) = self.lift(a, b, walk)
            {
                return (lift.dist, Way::Bridge(lift));
            }
            return (walk, Way::Terrain);
        }
        let walk = known.unwrap_or(f64::INFINITY);
        // Through the gaps opened, about as straight as can be: no passage beats it by much.
        if walk <= straight * UNKNOWN_DETOUR {
            return (walk, Way::Terrain);
        }
        let bridge = self.bridge(a, b, level);
        let cap = straight
            * if bridge.is_some() {
                BRIDGED_DETOUR
            } else {
                UNKNOWN_DETOUR
            };
        let via = bridge.map_or(f64::INFINITY, |b| b.dist);
        if walk <= via && walk <= cap {
            (walk, Way::Terrain)
        } else if let Some(bridge) = bridge.filter(|_| via <= cap) {
            (via, Way::Bridge(bridge))
        } else {
            (cap, Way::Straight)
        }
    }

    /// Forget every walk worked out (`Terrain::forget_walks`), through the passages too.
    #[doc(hidden)]
    pub fn forget_walks(&self) {
        self.terrain.forget_walks();
        WALKED.with(|m| m.borrow_mut().clear());
    }

    /// A way through a lift shorter than `walk`, the floors' way between two points of one
    /// area (Undercity's elevators, down from the ruins), when the walk round is long: from `a`
    /// to a side of a lift of its area, out by another side of the same lift and on to `b`.
    fn lift(&self, a: &Pos, b: &Pos, walk: f64) -> Option<Bridge> {
        if a.continent != b.continent || self.area_lifts.is_empty() {
            return None;
        }
        let (area, size) = self.terrain.area_of(a)?;
        if area == 0 || size < crate::terrain::LARGE_AREA {
            return None;
        }
        let lifts = self.area_lifts.get(&(a.continent, area))?;
        // As the crow flies first: walked to only while that could beat the best.
        let mut ways: Vec<(f64, usize, usize, usize)> = (lifts.iter())
            .map(|&(i, k, j)| {
                let pass = &self.passes[i];
                let (near_a, near_b) = (&pass.areas[k].1, &pass.areas[j].1);
                let least = a.dist(near_a) + near_a.dist(&pass.pos) + pass.pos.dist(near_b) + near_b.dist(b);
                (least, i, k, j)
            })
            .filter(|w| w.0 < walk)
            .collect();
        if ways.is_empty() {
            return None;
        }
        ways.sort_by(|x, y| x.0.total_cmp(&y.0).then((x.1, x.2, x.3).cmp(&(y.1, y.2, y.3))));
        let known = |p: &Pos, q: &Pos| match self.terrain.distance(p, q) {
            Some(d) => d,
            None if self.terrain.disconnected(p, q) => f64::INFINITY,
            None => p.dist(q),
        };
        let mut best: Option<Bridge> = None;
        for (least, i, k, j) in ways {
            if least >= best.map_or(walk, |b| b.dist) {
                break;
            }
            let pass = &self.passes[i];
            let (near_a, near_b) = (&pass.areas[k].1, &pass.areas[j].1);
            let t = known(a, near_a) + near_a.dist(&pass.pos) + pass.pos.dist(near_b) + known(near_b, b);
            if t < best.map_or(walk, |b| b.dist) {
                best = Some(Bridge {
                    dist: t,
                    from: (i, k),
                    to: (i, j),
                });
            }
        }
        best
    }

    /// The walk from `a` to `b`, in two large areas the terrain does not connect, through the
    /// passages joining them: over the terrain to a side of a passage in `a`'s area, from
    /// passage to passage (`pass_dist`), and from a side of a passage in `b`'s area. None when
    /// no passage joins the areas.
    pub fn bridge(&self, a: &Pos, b: &Pos, level: u8) -> Option<Bridge> {
        if a.continent != b.continent || self.area_passes.is_empty() {
            return None;
        }
        let large = |p: &Pos| {
            self.terrain
                .area_of(p)
                .filter(|&(area, size)| area != 0 && size >= crate::terrain::LARGE_AREA)
                .map(|(area, _)| area)
        };
        let (area_a, area_b) = (large(a)?, large(b)?);
        let from = self.area_passes.get(&(a.continent, area_a))?;
        let to = self.area_passes.get(&(b.continent, area_b))?;
        let dist = self.pass_graph_at(level).0;
        let point = |(i, k): (usize, usize)| (&self.passes[i].pos, &self.passes[i].areas[k].1);
        // Weighed from the least straight trip up, walked to while that could beat the best.
        let mut pairs: Vec<(f64, usize, usize)> = Vec::new();
        for (ka, &(i, _)) in from.iter().enumerate() {
            let (pass_a, near_a) = point(from[ka]);
            for (kb, &(j, _)) in to.iter().enumerate() {
                let (pass_b, near_b) = point(to[kb]);
                let least = a.dist(near_a) + near_a.dist(pass_a) + dist[i][j] + pass_b.dist(near_b) + near_b.dist(b);
                if least.is_finite() {
                    pairs.push((least, ka, kb));
                }
            }
        }
        pairs.sort_by(|x, y| x.0.total_cmp(&y.0).then((x.1, x.2).cmp(&(y.1, y.2))));
        let mut from_a: Vec<Option<f64>> = vec![None; from.len()];
        let mut to_b: Vec<Option<f64>> = vec![None; to.len()];
        let mut best: Option<Bridge> = None;
        for (least, ka, kb) in pairs {
            if best.is_some_and(|b| least >= b.dist) {
                break;
            }
            let (pass_a, near_a) = point(from[ka]);
            let (pass_b, near_b) = point(to[kb]);
            // Over the terrain; straight from or to a place in no large area (a building).
            let known = |p: &Pos, q: &Pos| match self.terrain.distance(p, q) {
                Some(d) => d,
                None if self.terrain.disconnected(p, q) => f64::INFINITY,
                None => p.dist(q),
            };
            let da = *from_a[ka].get_or_insert_with(|| known(a, near_a));
            let db = *to_b[kb].get_or_insert_with(|| known(near_b, b));
            let t = da + near_a.dist(pass_a) + dist[from[ka].0][to[kb].0] + pass_b.dist(near_b) + db;
            if t.is_finite() && best.is_none_or(|b| t < b.dist) {
                best = Some(Bridge {
                    dist: t,
                    from: from[ka],
                    to: to[kb],
                });
            }
        }
        best
    }

    /// Direct walk between close points of different zones when the terrain knows the way
    /// (a point filed under the neighbouring zone, whose map overlaps), else infinite.
    fn across(&self, a: &Pos, b: &Pos) -> f64 {
        if self.terrain.is_empty() || a.dist(b) > ACROSS_MAX {
            return f64::INFINITY;
        }
        self.terrain.distance(a, b).unwrap_or(f64::INFINITY)
    }
}
