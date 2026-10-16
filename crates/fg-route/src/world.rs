//! Geography: zone coordinates <-> world coordinates, and the flight path network.

use crate::faction::Faction;
use anyhow::Result;
use rusqlite::Connection;
use rustc_hash::FxHashMap;
use std::collections::HashMap;

/// A point in world space (yards) on a continent.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Pos {
    pub continent: i64,
    pub x: f64,
    pub y: f64,
    /// AreaTable zone the point is in (0 when unknown).
    pub zone: i64,
}

impl Pos {
    pub fn dist(&self, o: &Pos) -> f64 {
        if self.continent != o.continent {
            return f64::INFINITY;
        }
        ((self.x - o.x).powi(2) + (self.y - o.y).powi(2)).sqrt()
    }
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

pub struct World {
    zones: HashMap<i64, ZoneRect>,
    pub zone_names: HashMap<i64, String>,
    pub taxi_nodes: Vec<TaxiNode>,
    /// Direct flight time in seconds between node indexes, INFINITY when no direct path.
    pub direct: Vec<Vec<f64>>,
    /// Price in copper of the direct flights, INFINITY when no direct path.
    pub direct_cost: Vec<Vec<f64>>,
    pub links: Vec<Link>,
    /// Quests whose taking opens links (`LinkDef::after_quest`).
    pub link_quests: Vec<i64>,
    /// Time to chain links i..j (boarding i, leaving j), walking in between, by set of opened
    /// `link_quests` (bit k: quest k taken).
    link_chains: Vec<Vec<Vec<f64>>>,
    /// Walkable passages between zones and the shortest walk between any two of them.
    pub passes: Vec<Pass>,
    pub pass_dist: Vec<Vec<f64>>,
    /// Next passage on the shortest walk from passage i to passage j.
    pass_next: Vec<Vec<usize>>,
    /// Whether the shortest walk from passage i to passage j takes a shortcut (them included).
    pass_shortcut: Vec<Vec<bool>>,
    pub zone_passes: FxHashMap<i64, Vec<usize>>,
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
    pos: Pos,
    zone: i64,
    name: String,
}

/// A landmark names a point up to this distance (yards).
const LANDMARK_NAME_RADIUS: f64 = 400.0;

impl World {
    /// Load zones and the flight network usable by a faction ("Alliance" / "Horde").
    pub fn load(conn: &Connection, faction: Faction, flight_speed: f64) -> Result<Self> {
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
        let mut stmt = conn.prepare("SELECT ID, AreaName_lang FROM client_areatable")?;
        for row in stmt.query_map([], |r| Ok((r.get::<_, i64>(0)?, r.get::<_, String>(1)?)))? {
            let (id, name) = row?;
            zone_names.insert(id, name);
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
            "SELECT ID, Name_lang, ContinentID, Pos_0, Pos_1 FROM client_taxinodes
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
        for ((from, to), points) in paths {
            let (Some(&a), Some(&b)) = (index.get(&from), index.get(&to)) else {
                continue;
            };
            let len: f64 = points
                .windows(2)
                .map(|w| ((w[0].0 - w[1].0).powi(2) + (w[0].1 - w[1].1).powi(2)).sqrt())
                .sum();
            flight[a][b] = flight[a][b].min(len / flight_speed);
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
        let taxi_nodes: Vec<TaxiNode> = taxi_nodes
            .into_iter()
            .zip(&keep)
            .filter(|(_, k)| **k)
            .map(|(t, _)| t)
            .collect();
        let mut world = Self {
            zones,
            zone_names,
            taxi_nodes,
            direct: flight,
            direct_cost: cost,
            links: Vec::new(),
            link_quests: Vec::new(),
            link_chains: vec![Vec::new()],
            passes: Vec::new(),
            pass_dist: Vec::new(),
            pass_next: Vec::new(),
            pass_shortcut: Vec::new(),
            zone_passes: FxHashMap::default(),
            taxi_by_x: Vec::new(),
            terrain: crate::terrain::Terrain::load(conn)?,
            landmarks: Vec::new(),
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

    pub fn has_zone(&self, zone: i64) -> bool {
        self.zones.contains_key(&zone)
    }

    /// Zone (AreaTable ID) shown by a UiMap.
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

    pub fn zone_name(&self, zone: i64) -> &str {
        self.zone_names.get(&zone).map_or("?", String::as_str)
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
        self.landmarks
            .iter()
            .filter(|l| l.pos.continent == p.continent && l.pos.dist(p) <= LANDMARK_NAME_RADIUS)
            .min_by(|a, b| a.pos.dist(p).total_cmp(&b.pos.dist(p)))
            .map(|l| l.name.as_str())
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
                out.push(Landmark { pos, zone, name });
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
}

/// At most this many quests open links: the set of opened ones is a byte of the trip cache key.
pub const LINK_QUESTS_MAX: usize = 6;

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
            let make = |from: Pos, fz, to: Pos, tz| Link {
                name: d.name.clone(),
                from,
                from_zone: fz,
                to,
                to_zone: tz,
                seconds: d.seconds + if d.instant { 0.0 } else { wait },
                after_quest: d.after_quest,
            };
            self.links.push(make(a, d.from.zone, b, d.to.zone));
            if !d.one_way {
                self.links.push(make(b, d.to.zone, a, d.from.zone));
            }
        }
        self.link_quests.clear();
        for q in self.links.iter().filter_map(|l| l.after_quest) {
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
            l.after_quest
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
    pub fn link_chain(&self, open: u8) -> &[Vec<f64>] {
        &self.link_chains[usize::from(open) & (self.link_chains.len() - 1)]
    }

    /// Continents reachable from `start` on foot and through links.
    pub fn reachable_continents(&self, start: i64) -> std::collections::HashSet<i64> {
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
}

/// A large walkable area left only by boat, portal or lift (`[[enclosed]]` of
/// `overrides/passes.toml`): the terrain area at `at`.
#[derive(Debug, Clone, serde::Deserialize)]
pub struct EnclosedDef {
    pub name: String,
    pub at: ZonePoint,
}

#[derive(Debug, Clone)]
pub struct Pass {
    pub name: String,
    pub pos: Pos,
    pub zones: [i64; 2],
    pub shortcut: bool,
    pub one_way: bool,
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
    /// Count the danger of the mobs hostile to both factions on walks (`Terrain::add_danger`):
    /// their highest level, 3 more for an elite. Before the passages and links.
    pub fn add_danger(&mut self, conn: &Connection, weight: f64) -> Result<()> {
        let mut stmt = conn.prepare(
            "SELECT s.zone_id, s.x, s.y, coalesce(n.max_level, n.min_level, 1), coalesce(n.rank, 0)
             FROM m_spawn s JOIN m_npc n ON n.id = s.entity_id
             WHERE s.entity_type = 'npc' AND s.x >= 0 AND coalesce(n.friendly_to, '') = ''",
        )?;
        let rows = stmt.query_map([], |r| {
            Ok((
                r.get::<_, i64>(0)?,
                r.get::<_, f64>(1)?,
                r.get::<_, f64>(2)?,
                r.get::<_, i64>(3)?,
                r.get::<_, i64>(4)?,
            ))
        })?;
        let mut spawns = Vec::new();
        for row in rows {
            let (zone, x, y, level, rank) = row?;
            if let Some(p) = self.to_world(zone, x, y) {
                let elite = if rank > 0 { 3 } else { 0 };
                spawns.push((p, (level + elite).clamp(1, 255) as u8));
            }
        }
        self.terrain.add_danger(spawns, weight);
        Ok(())
    }

    /// Areas left only by boat, portal or lift: no walk to or from them. Before the passages
    /// (their walks depend on it).
    pub fn enclose(&mut self, defs: &[EnclosedDef]) -> Vec<String> {
        let mut warnings = Vec::new();
        for d in defs {
            let enclosed = self
                .to_world(d.at.zone, d.at.x, d.at.y)
                .and_then(|p| self.terrain.enclose(&p));
            if enclosed.is_none() && !self.terrain.is_empty() {
                warnings.push(format!("enclosed area {}: not in a large area of the terrain", d.name));
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
                }),
                None => warnings.push(format!("pass {}: zone {zone} has no map", d.name)),
            }
        }
        self.zone_passes.clear();
        for (i, p) in self.passes.iter().enumerate() {
            for z in p.zones {
                self.zone_passes.entry(z).or_default().push(i);
            }
        }
        // Walking from a passage to another taken from the zone it leads into, then all-pairs
        // shortest walks (one-way passages: not the same both ways).
        let n = self.passes.len();
        let mut d = vec![vec![f64::INFINITY; n]; n];
        let mut next: Vec<Vec<usize>> = (0..n).map(|_| (0..n).collect()).collect();
        for (i, (row, a)) in d.iter_mut().zip(&self.passes).enumerate() {
            row[i] = 0.0;
            for (j, (cell, b)) in row.iter_mut().zip(&self.passes).enumerate() {
                if i != j && a.zones.iter().any(|&z| a.leads_to(z) && b.taken_from(z)) {
                    *cell = self.ground(&a.pos, &b.pos, 0);
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

    /// The passages of the shortest walk from passage `i` to passage `j`, both included (none
    /// when no walk goes there).
    pub fn pass_path(&self, i: usize, j: usize) -> Vec<usize> {
        if self.pass_dist[i][j].is_infinite() {
            return Vec::new();
        }
        let mut path = vec![i];
        let mut k = i;
        while k != j {
            k = self.pass_next[k][j];
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

    /// The same for a character of `level`: the danger of the mobs on the way counts (when
    /// `add_danger` set it; 0: never).
    pub fn walk_distance_at(&self, a: &Pos, b: &Pos, level: u8) -> f64 {
        let straight = a.dist(b);
        if !straight.is_finite() {
            return straight;
        }
        if a.zone == b.zone {
            return self.ground(a, b, level);
        }
        let (Some(pa), Some(pb)) = (self.zone_passes.get(&a.zone), self.zone_passes.get(&b.zone)) else {
            return self.unknown_walk(a, b);
        };
        let best = self.through_passes(a, b, pa, pb, level).0;
        let passes = &self.passes;
        if best.is_finite() {
            best
        } else if pa.iter().all(|&i| {
            pb.iter().all(|&j| {
                !passes[i].taken_from(a.zone) || !passes[j].leads_to(b.zone) || self.pass_dist[i][j].is_infinite()
            })
        }) {
            f64::INFINITY
        } else {
            self.unknown_walk(a, b)
        }
    }

    /// The points the walk from `a` to `b` of `walk_distance_at` goes through, ends included: the
    /// passages it takes between zones, the turns around the steep terrain
    /// (`Terrain::waypoints`). Only the ends when it goes straight, or when no walk goes there.
    pub fn walk_path(&self, a: &Pos, b: &Pos, level: u8) -> Vec<Pos> {
        let mut stops = vec![*a];
        if a.zone != b.zone
            && a.continent == b.continent
            && let (Some(pa), Some(pb)) = (self.zone_passes.get(&a.zone), self.zone_passes.get(&b.zone))
            && let (t, Some((i, j))) = self.through_passes(a, b, pa, pb, level)
            && t.is_finite()
        {
            stops.extend(self.pass_path(i, j).into_iter().map(|k| self.passes[k].pos));
        }
        stops.push(*b);
        let mut path = vec![*a];
        let last = stops.len() - 2;
        for (k, leg) in stops.windows(2).enumerate() {
            // Between passages the walks are known at no danger (`add_passes`).
            let at = if k == 0 || k == last { level } else { 0 };
            path.extend(self.terrain.waypoints(&leg[0], &leg[1], at));
            path.push(leg[1]);
        }
        path
    }

    /// Shortest walk from `a` to `b` through the passages of their zones (`pa`, `pb`), or across
    /// the border when shorter, and the first and last passage it takes.
    fn through_passes(&self, a: &Pos, b: &Pos, pa: &[usize], pb: &[usize], level: u8) -> (f64, Option<(usize, usize)>) {
        let mut best = (self.across(a, b, level), None);
        // Each walk to a passage once (the inner loop would ask them for every passage left).
        let to_b: Vec<f64> = (pb.iter())
            .map(|&j| {
                if self.passes[j].leads_to(b.zone) {
                    self.ground(&self.passes[j].pos, b, level)
                } else {
                    f64::INFINITY
                }
            })
            .collect();
        for &i in pa.iter().filter(|&&i| self.passes[i].taken_from(a.zone)) {
            let da = self.ground(a, &self.passes[i].pos, level);
            for (&j, &db) in pb.iter().zip(&to_b) {
                let t = da + self.pass_dist[i][j] + db;
                if t < best.0 {
                    best = (t, Some((i, j)));
                }
            }
        }
        best
    }

    /// A walk the passages do not know: the straight line, unless no walk goes between the two
    /// places (one in an enclosed area: an island, the top of a cliff).
    fn unknown_walk(&self, a: &Pos, b: &Pos) -> f64 {
        if self.terrain.separated(a, b) {
            f64::INFINITY
        } else {
            a.dist(b)
        }
    }

    /// Walk within a zone: around the steep terrain, straight when it does not know the way,
    /// infinite to or from an enclosed area.
    fn ground(&self, a: &Pos, b: &Pos, level: u8) -> f64 {
        self.terrain.distance(a, b, level).unwrap_or_else(|| a.dist(b))
    }

    /// Direct walk between close points of different zones when the terrain knows the way
    /// (a point filed under the neighbouring zone, whose map overlaps), else infinite.
    fn across(&self, a: &Pos, b: &Pos, level: u8) -> f64 {
        if self.terrain.is_empty() || a.dist(b) > ACROSS_MAX {
            return f64::INFINITY;
        }
        self.terrain.distance(a, b, level).unwrap_or(f64::INFINITY)
    }
}
