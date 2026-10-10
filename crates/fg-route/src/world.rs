//! Geography: zone coordinates <-> world coordinates, and the flight path network.

use crate::faction::Faction;
use anyhow::Result;
use rusqlite::Connection;
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
    /// Time to chain links i..j (boarding i, leaving j), walking in between.
    pub link_chain: Vec<Vec<f64>>,
    /// Walkable passages between zones and the shortest walk between any two of them.
    pub passes: Vec<Pass>,
    pub pass_dist: Vec<Vec<f64>>,
    pub zone_passes: HashMap<i64, Vec<usize>>,
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
            link_chain: Vec::new(),
            passes: Vec::new(),
            pass_dist: Vec::new(),
            zone_passes: HashMap::new(),
            terrain: crate::terrain::Terrain::load(conn)?,
            landmarks: Vec::new(),
        };
        world.landmarks = world.load_landmarks(conn)?;
        for i in 0..world.taxi_nodes.len() {
            let p = world.taxi_nodes[i].pos;
            world.taxi_nodes[i].pos.zone = world.zone_at(&p).unwrap_or(0);
        }
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

    /// Zone percent coordinates (as Questie stores them) to world space.
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
        self.taxi_nodes
            .iter()
            .enumerate()
            .map(|(i, n)| (i, n.pos.dist(p)))
            .filter(|(_, d)| *d <= radius)
            .min_by(|a, b| a.1.total_cmp(&b.1))
            .map(|(i, _)| i)
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
}

/// A directed transport link.
#[derive(Debug, Clone)]
pub struct Link {
    pub name: String,
    pub from: Pos,
    pub from_zone: i64,
    pub to: Pos,
    pub to_zone: i64,
    pub seconds: f64,
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
                seconds: d.seconds + wait,
            };
            self.links.push(make(a, d.from.zone, b, d.to.zone));
            if !d.one_way {
                self.links.push(make(b, d.to.zone, a, d.from.zone));
            }
        }
        // chain[i][j]: board link i, ..., get off link j.
        let n = self.links.len();
        let mut chain = vec![vec![f64::INFINITY; n]; n];
        for (i, (row, a)) in chain.iter_mut().zip(&self.links).enumerate() {
            for (j, (cell, b)) in row.iter_mut().zip(&self.links).enumerate() {
                if i == j {
                    *cell = a.seconds;
                } else {
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
        self.link_chain = chain;
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
}

#[derive(Debug, Clone)]
pub struct Pass {
    pub name: String,
    pub pos: Pos,
    pub zones: [i64; 2],
}

impl World {
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
        // Walking between passages that share a zone, then all-pairs shortest walks.
        let n = self.passes.len();
        let mut d = vec![vec![f64::INFINITY; n]; n];
        for (i, (row, a)) in d.iter_mut().zip(&self.passes).enumerate() {
            row[i] = 0.0;
            for (j, (cell, b)) in row.iter_mut().zip(&self.passes).enumerate() {
                if i != j && a.zones.iter().any(|z| b.zones.contains(z)) {
                    *cell = self.ground(&a.pos, &b.pos);
                }
            }
        }
        for k in 0..n {
            for i in 0..n {
                for j in 0..n {
                    let via = d[i][k] + d[k][j];
                    if via < d[i][j] {
                        d[i][j] = via;
                    }
                }
            }
        }
        self.pass_dist = d;
        Ok(warnings)
    }

    /// Walking distance (yards): around the steep terrain within a zone, through passages between
    /// zones.
    /// Zones the passage graph does not cover fall back to a straight line.
    /// Walking distance and the passages used to leave `a`'s zone and enter `b`'s (debugging).
    pub fn walk_route(&self, a: &Pos, b: &Pos) -> (f64, Option<(usize, usize)>) {
        if a.zone == b.zone {
            return (self.ground(a, b), None);
        }
        let (Some(pa), Some(pb)) = (self.zone_passes.get(&a.zone), self.zone_passes.get(&b.zone)) else {
            return (self.ground(a, b), None);
        };
        let mut best = (self.across(a, b), None);
        for &i in pa {
            let da = self.ground(a, &self.passes[i].pos);
            for &j in pb {
                let t = da + self.pass_dist[i][j] + self.ground(&self.passes[j].pos, b);
                if t < best.0 {
                    best = (t, Some((i, j)));
                }
            }
        }
        best
    }

    pub fn walk_distance(&self, a: &Pos, b: &Pos) -> f64 {
        let straight = a.dist(b);
        if !straight.is_finite() {
            return straight;
        }
        if a.zone == b.zone {
            return self.ground(a, b);
        }
        let (Some(pa), Some(pb)) = (self.zone_passes.get(&a.zone), self.zone_passes.get(&b.zone)) else {
            return straight;
        };
        let mut best = self.across(a, b);
        for &i in pa {
            let da = self.ground(a, &self.passes[i].pos);
            for &j in pb {
                let t = da + self.pass_dist[i][j] + self.ground(&self.passes[j].pos, b);
                if t < best {
                    best = t;
                }
            }
        }
        if best.is_finite() { best } else { straight }
    }

    /// Walk within a zone: around the steep terrain, straight when it does not know the way.
    fn ground(&self, a: &Pos, b: &Pos) -> f64 {
        self.terrain.distance(a, b).unwrap_or_else(|| a.dist(b))
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
