//! Long walks through the tiles' gates, a hierarchy of the floors (as HPA*): searching a walk of
//! thousands of yards floor by floor explores hundreds of thousands of them.
//!
//! The gates of a tile are floors of its border cells a straight step crosses into the next
//! tile from or to: the middle of each run of such crossings joining the same areas on either
//! side, and its ends when the run is long. Both tiles of a border see the same runs, so each
//! gate has its twin across. The walks between the gates of a tile, within it, are found when
//! the tile is first needed (one search a gate over its floors) and kept. A long walk then goes
//! from its start to the gates of its tile, from gate to gate (A* over a few dozen gates a
//! tile), and from the gates of the last tile to its end. Drops go one way only: the walks are
//! not the same both ways.
//!
//! The walk is a little longer than the cheapest floor by floor (it goes through the gates).

use super::layers::DIRECTIONS;
use super::{CELL, Grid, MOVE, MOVE_MORE, MOVE_NO_GO, Node, TileNodes, move_cost};
use crate::sync::LockExt;
use rustc_hash::FxHashMap;
use std::cmp::Reverse;
use std::collections::BinaryHeap;
use std::sync::Arc;

/// Walks farther than this (cells, one way or the other) go through the gates.
pub(super) const FAR: i32 = 96;
/// A run of crossings longer than this has a gate at each end too, not only its middle.
const LONG_RUN: usize = 8;
const NONE: u32 = u32::MAX;
/// A walkable area within a tile smaller than this (floors: a ledge between steep slopes) has
/// no gate of its own.
const SMALL_AREA: u32 = 4;
/// Ends of walks in a tile whose costs to its gates are remembered, beyond which the tile's memo
/// starts over.
const ENDS_MAX: usize = 1 << 12;

/// The walks from a floor of a tile to its gates (or back), by floor and way, shared by the
/// threads.
pub(super) type Ends = FxHashMap<(Node, bool), Arc<[u32]>>;

const FROM_A: (usize, usize) = (usize::MAX, usize::MAX);

thread_local! {
    /// The open floors of `Grid::local`, kept from a search to the next.
    static LOCAL_OPEN: std::cell::RefCell<Vec<Vec<u16>>> =
        std::cell::RefCell::new((0..super::BUCKETS).map(|_| Vec::new()).collect());
}

/// Cheapest cost found to each gate (tile, index) of a walk (`NONE`: none yet), and the gate it
/// came from (`FROM_A`: the walk's start): the gates of a tile side by side, found by one lookup
/// of the tile.
#[derive(Default)]
struct Best {
    tiles: FxHashMap<usize, usize>,
    gates: Vec<(u32, (usize, usize))>,
}

impl Best {
    /// Where the gates of tile `t` (`n` of them) start in `gates`.
    fn row(&mut self, t: usize, n: usize) -> usize {
        let gates = &mut self.gates;
        *self.tiles.entry(t).or_insert_with(|| {
            gates.resize(gates.len() + n, (NONE, FROM_A));
            gates.len() - n
        })
    }

    /// What is known of gate `i` of tile `t`.
    fn get(&self, (t, i): (usize, usize)) -> (u32, (usize, usize)) {
        self.tiles.get(&t).map_or((NONE, FROM_A), |&at| self.gates[at + i])
    }
}

/// The walkable areas within a tile (`Grid::local_areas`): each node's (0: never stood on), the
/// size of each, and the areas each leads to (`next`) or is led to from (`prev`) by a step
/// within the tile.
#[derive(Default)]
pub(super) struct LocalAreas {
    label: Box<[u16]>,
    size: Box<[u32]>,
    next: Box<[Box<[u16]>]>,
    prev: Box<[Box<[u16]>]>,
}

impl LocalAreas {
    /// The areas reached from area `a` within the tile (or reaching it, `back`).
    fn reachable(&self, a: u16, back: bool) -> Vec<bool> {
        let mut seen = vec![false; self.size.len()];
        let ways = if back { &self.prev } else { &self.next };
        if usize::from(a) >= seen.len() || a == 0 {
            return seen;
        }
        seen[usize::from(a)] = true;
        let mut stack = vec![a];
        while let Some(x) = stack.pop() {
            for &y in &ways[usize::from(x)] {
                if !std::mem::replace(&mut seen[usize::from(y)], true) {
                    stack.push(y);
                }
            }
        }
        seen
    }
}

/// Version of the making of the tiles' graphs and of the steps: graphs stored by another
/// (`Terrain::gate_graphs`) are made again, walking memories of another are dropped. 4: the
/// floors of the game's geometry.
pub const GATES_VERSION: u32 = 4;

/// A tile's gates and the walks between them within it.
#[derive(Clone)]
pub(super) struct TileGraph {
    /// Floors of the border a straight step crosses into the next tile, sorted.
    gates: Vec<Node>,
    /// Cheapest walk (tenths of a cell) from gate `i` to gate `j` within the tile, at
    /// `i * gates + j` (`NONE`: no walk).
    cost: Vec<u32>,
}

impl TileGraph {
    /// Stored: the version, the count of gates, each gate's row and column in its tile and its
    /// floor, then the walks between them, little endian, compressed.
    fn encode(&self, origin: (i32, i32)) -> Vec<u8> {
        let mut bytes = Vec::with_capacity(6 + self.gates.len() * 3 + self.cost.len() * 4);
        bytes.extend(GATES_VERSION.to_le_bytes());
        bytes.extend((self.gates.len() as u16).to_le_bytes());
        for &n in &self.gates {
            bytes.extend([(n.r() - origin.0) as u8, (n.c() - origin.1) as u8, n.f() as u8]);
        }
        for &c in &self.cost {
            bytes.extend(c.to_le_bytes());
        }
        super::layers::compress(&bytes).unwrap_or_default()
    }

    /// A graph stored by this version, None otherwise.
    fn decode(blob: &[u8], origin: (i32, i32)) -> Option<Self> {
        let bytes = zstd::decode_all(blob).ok()?;
        if u32::from_le_bytes(bytes.get(..4)?.try_into().ok()?) != GATES_VERSION {
            return None;
        }
        let n = usize::from(u16::from_le_bytes(bytes.get(4..6)?.try_into().ok()?));
        let gates = (0..n)
            .map(|i| {
                let at = 6 + 3 * i;
                Some(Node::new(
                    origin.0 + i32::from(*bytes.get(at)?),
                    origin.1 + i32::from(*bytes.get(at + 1)?),
                    usize::from(*bytes.get(at + 2)?),
                ))
            })
            .collect::<Option<Vec<_>>>()?;
        let costs = bytes.get(6 + 3 * n..6 + 3 * n + 4 * n * n)?;
        let cost = costs
            .as_chunks::<4>()
            .0
            .iter()
            .map(|c| u32::from_le_bytes(*c))
            .collect();
        Some(Self { gates, cost })
    }
}

impl Grid {
    /// The tiles' graphs (`Terrain::gate_graphs`), each made in this thread or another.
    pub(super) fn all_graphs(&self) -> Vec<(usize, Vec<u8>)> {
        let n = self.tiles.len();
        let workers = std::thread::available_parallelism().map_or(4, std::num::NonZero::get);
        std::thread::scope(|scope| {
            let parts: Vec<_> = (0..workers)
                .map(|w| {
                    scope.spawn(move || {
                        (w..n)
                            .step_by(workers)
                            .filter(|&t| self.tiles[t].is_some())
                            .map(|t| (t, self.graph(t).encode(self.origin(t))))
                            .collect::<Vec<_>>()
                    })
                })
                .collect();
            let mut all: Vec<(usize, Vec<u8>)> = parts.into_iter().flat_map(|p| p.join().unwrap_or_default()).collect();
            all.sort_by_key(|(t, _)| *t);
            all
        })
    }

    /// Use the graph stored for tile `t` (when made by this version).
    pub(super) fn set_graph(&self, t: usize, blob: &[u8]) {
        if let Some(graph) = TileGraph::decode(blob, self.origin(t)) {
            let _ = self.graphs[t].set(graph);
        }
    }

    /// The steps from node `l` of tile `tile` (cell (r, c)) staying in the tile: the node it
    /// leads to and the cost.
    #[inline]
    fn local_steps(tile: &TileNodes, l: usize, (lr, lc): (i32, i32), mut each: impl FnMut(usize, u32)) {
        let moves = tile.moves[l];
        if moves & MOVE_NO_GO != 0 {
            return;
        }
        let border = lr == 0 || lc == 0 || lr == 63 || lc == 63;
        for (d, (dr, dc)) in DIRECTIONS.into_iter().enumerate() {
            let (nr, nc) = (lr + dr, lc + dc);
            if moves >> d & MOVE != 0 && (!border || ((0..64).contains(&nr) && (0..64).contains(&nc))) {
                each((nr * 64 + nc) as usize, move_cost(moves, d));
            }
        }
        if moves & MOVE_MORE != 0 {
            for &e in tile.edges(l) {
                let (nr, nc) = e.to(lr, lc);
                if (0..64).contains(&nr)
                    && (0..64).contains(&nc)
                    && let Some(k) = tile.local((nr * 64 + nc) as usize, e.floor())
                {
                    each(k, e.cost());
                }
            }
        }
    }

    /// Cheapest walks (tenths of a cell) within tile `t` from node `from` to each of its nodes,
    /// or from each of them to `from` when `back`. Only the nodes of `wanted` are sure to
    /// be found: the search ends once they are.
    fn local(&self, t: usize, from: Node, back: bool, wanted: &[Node]) -> Vec<u32> {
        let Some(tile) = self.nodes(t) else {
            return Vec::new();
        };
        let (tr, tc) = self.origin(t);
        let mut cost = vec![NONE; tile.len()];
        let local = |n: Node| {
            let (r, c) = (n.r() - tr, n.c() - tc);
            ((0..64).contains(&r) && (0..64).contains(&c))
                .then(|| tile.local((r * 64 + c) as usize, n.f()))
                .flatten()
        };
        let Some(start) = local(from) else {
            return cost;
        };
        cost[start] = 0;
        // Only the wanted nodes it may reach (those of other areas none leads to are not
        // waited for).
        let areas = self.local_areas(t);
        let reach = areas.reachable(areas.label.get(start).copied().unwrap_or(0), back);
        let mut left = vec![false; tile.len()];
        let mut count = 0;
        for k in wanted.iter().filter_map(|&n| local(n)) {
            if reach.get(usize::from(areas.label[k])).copied().unwrap_or(false) {
                count += usize::from(!std::mem::replace(&mut left[k], true));
            }
        }
        if count == 0 {
            return cost;
        }
        let cell_of = |k: usize| {
            if k < 4096 {
                k
            } else {
                usize::from(tile.more_cell[k - 4096])
            }
        };
        // Open nodes by cost in a ring of buckets: a step costs less than `BUCKETS`, so the
        // nodes of a bucket all have the cost the search is at (Dijkstra's order).
        LOCAL_OPEN.with(|open| {
            let open = &mut *open.borrow_mut();
            for bucket in open.iter_mut() {
                bucket.clear();
            }
            let (mut g, mut queued) = (0u32, 1usize);
            open[0].push(start as u16);
            macro_rules! relax {
                ($n:expr, $ng:expr) => {{
                    let (n, ng): (usize, u32) = ($n, $ng);
                    if ng < cost[n] {
                        cost[n] = ng;
                        open[ng as usize % super::BUCKETS].push(n as u16);
                        queued += 1;
                    }
                }};
            }
            while queued > 0 {
                let Some(k) = open[g as usize % super::BUCKETS].pop() else {
                    g += 1;
                    continue;
                };
                queued -= 1;
                let k = usize::from(k);
                if g > cost[k] {
                    continue;
                }
                if std::mem::take(&mut left[k]) {
                    count -= 1;
                    if count == 0 {
                        break;
                    }
                }
                let ck = cell_of(k);
                let (lr, lc) = ((ck / 64) as i32, (ck % 64) as i32);
                let border = lr == 0 || lc == 0 || lr == 63 || lc == 63;
                if back {
                    // The steps into `k`: from the floors of the cells around and its own.
                    let fk = if k < 4096 {
                        0
                    } else {
                        k - 4096 - usize::from(tile.more[ck]) + 1
                    };
                    for (d, (dr, dc)) in DIRECTIONS.into_iter().chain([(0, 0)]).enumerate() {
                        let (pr, pc) = (lr - dr, lc - dc);
                        if border && (!(0..64).contains(&pr) || !(0..64).contains(&pc)) {
                            continue;
                        }
                        let j = (pr * 64 + pc) as usize;
                        for f in 0..usize::from(tile.count[j]) {
                            let p = if f == 0 {
                                j
                            } else {
                                4096 + usize::from(tile.more[j]) + f - 1
                            };
                            let moves = tile.moves[p];
                            if moves & MOVE_NO_GO != 0 {
                                continue;
                            }
                            // The steps to a first floor are the moves' bits, the others edges.
                            if fk == 0 && d < 8 && moves >> d & MOVE != 0 {
                                relax!(p, g + move_cost(moves, d));
                            }
                            if moves & MOVE_MORE != 0 {
                                for &e in tile.edges(p) {
                                    if e.dir() == d && e.floor() == fk {
                                        relax!(p, g + e.cost());
                                    }
                                }
                            }
                        }
                    }
                } else {
                    let moves = tile.moves[k];
                    if moves & MOVE_NO_GO != 0 {
                        continue;
                    }
                    for (d, (dr, dc)) in DIRECTIONS.into_iter().enumerate() {
                        let (nr, nc) = (lr + dr, lc + dc);
                        if moves >> d & MOVE == 0 || (border && (!(0..64).contains(&nr) || !(0..64).contains(&nc))) {
                            continue;
                        }
                        let n = (nr * 64 + nc) as usize;
                        relax!(n, g + move_cost(moves, d));
                    }
                    if moves & MOVE_MORE != 0 {
                        for &e in tile.edges(k) {
                            let (nr, nc) = e.to(lr, lc);
                            if !(0..64).contains(&nr) || !(0..64).contains(&nc) {
                                continue;
                            }
                            let i = (nr * 64 + nc) as usize;
                            let Some(n) = tile.local(i, e.floor()) else { continue };
                            relax!(n, g + e.cost());
                        }
                    }
                }
            }
        });
        cost
    }

    /// The walkable areas within tile `t` (nodes of straight neighbour cells walking to each
    /// other, as `label` but not leaving the tile), and the steps between them.
    fn local_areas(&self, t: usize) -> &LocalAreas {
        self.local_areas[t].get_or_init(|| {
            let Some(tile) = self.nodes(t) else {
                return LocalAreas::default();
            };
            let mut area = vec![0u16; tile.len()];
            let mut next = 0u16;
            let mut stack = Vec::new();
            let mut found: Vec<usize> = Vec::new();
            for start in 0..tile.len() {
                if area[start] != 0 || tile.moves[start] & MOVE_NO_GO != 0 {
                    continue;
                }
                next += 1;
                area[start] = next;
                stack.push(start);
                while let Some(k) = stack.pop() {
                    let i = tile.cell_floor(k).0;
                    let (lr, lc) = ((i / 64) as i32, (i % 64) as i32);
                    found.clear();
                    Self::local_steps(tile, k, (lr, lc), |n, _| found.push(n));
                    for &n in &found {
                        let j = tile.cell_floor(n).0;
                        let straight = (j / 64 == i / 64) != (j % 64 == i % 64);
                        if area[n] != 0 || !straight {
                            continue;
                        }
                        let (a, b) = (self.node_of(t, k), self.node_of(t, n));
                        if self.joined(a, b) {
                            area[n] = next;
                            stack.push(n);
                        }
                    }
                }
            }
            let mut size = vec![0u32; usize::from(next) + 1];
            for &a in &area {
                size[usize::from(a)] += 1;
            }
            // The steps from an area to another (a drop, a diagonal), both ways.
            let mut ways: Vec<(u16, u16)> = Vec::new();
            for k in 0..tile.len() {
                if area[k] == 0 {
                    continue;
                }
                let i = tile.cell_floor(k).0;
                Self::local_steps(tile, k, ((i / 64) as i32, (i % 64) as i32), |n, _| {
                    if area[n] != 0 && area[n] != area[k] {
                        ways.push((area[k], area[n]));
                    }
                });
            }
            ways.sort_unstable();
            ways.dedup();
            let mut next_of = vec![Vec::new(); usize::from(next) + 1];
            let mut prev_of = vec![Vec::new(); usize::from(next) + 1];
            for (a, b) in ways {
                next_of[usize::from(a)].push(b);
                prev_of[usize::from(b)].push(a);
            }
            LocalAreas {
                label: area.into(),
                size: size.into(),
                next: next_of.into_iter().map(Into::into).collect(),
                prev: prev_of.into_iter().map(Into::into).collect(),
            }
        })
    }

    /// The area within its tile of a node, when large enough for gates (0: none, too small, or
    /// out of the terrain).
    fn local_area(&self, n: Node) -> u16 {
        self.locate(n).map_or(0, |(t, l)| {
            let la = self.local_areas(t);
            let a = la.label.get(l).copied().unwrap_or(0);
            if la.size.get(usize::from(a)).is_some_and(|&s| s >= SMALL_AREA) {
                a
            } else {
                0
            }
        })
    }

    /// The gates of tile `t`: a run of crossings ends where the areas met on either side change
    /// (a ledge, the other bank of a river, a bridge over a road: each has its gate).
    fn tile_gates(&self, t: usize) -> Vec<Node> {
        let (tr, tc) = self.origin(t);
        let mut gates = Vec::new();
        for (dr, dc) in DIRECTIONS[..4].iter().copied() {
            let side = |k: i32| match (dr, dc) {
                (-1, 0) => (tr, tc + k),
                (1, 0) => (tr + 63, tc + k),
                (0, -1) => (tr + k, tc),
                _ => (tr + k, tc + 63),
            };
            // Each crossing (along the side, the floor here) and the areas it joins.
            let mut crossings: Vec<((u16, u16), i32, Node)> = Vec::new();
            for k in 0..64 {
                let (r, c) = side(k);
                for f in 0..self.floors(r, c) {
                    let p = Node::new(r, c, f);
                    let ap = self.local_area(p);
                    if ap == 0 {
                        continue;
                    }
                    let mut across: Vec<Node> = Vec::new();
                    self.steps(p, |q, _, _| {
                        if q.cell() == (r + dr, c + dc) {
                            across.push(q);
                        }
                    });
                    for g in 0..self.floors(r + dr, c + dc) {
                        let q = Node::new(r + dr, c + dc, g);
                        if !across.contains(&q) && self.step(q, p).is_some() {
                            across.push(q);
                        }
                    }
                    for q in across {
                        let aq = self.local_area(q);
                        if aq != 0 {
                            crossings.push(((ap, aq), k, p));
                        }
                    }
                }
            }
            crossings.sort_unstable();
            crossings.dedup_by(|x, y| x.0 == y.0 && x.1 == y.1);
            let mut i = 0;
            while i < crossings.len() {
                let mut j = i + 1;
                while j < crossings.len()
                    && crossings[j].0 == crossings[i].0
                    && crossings[j].1 == crossings[j - 1].1 + 1
                {
                    j += 1;
                }
                let run = &crossings[i..j];
                gates.push(run[(run.len() - 1) / 2].2);
                if run.len() > LONG_RUN {
                    gates.push(run[0].2);
                    gates.push(run[run.len() - 1].2);
                }
                i = j;
            }
        }
        gates.sort_unstable();
        gates.dedup();
        gates
    }

    /// The gates of tile `t` and the walks between them, found the first time.
    fn graph(&self, t: usize) -> &TileGraph {
        self.graphs[t].get_or_init(|| {
            if self.tiles[t].is_none() {
                return TileGraph {
                    gates: Vec::new(),
                    cost: Vec::new(),
                };
            }
            let gates = self.tile_gates(t);
            let n = gates.len();
            let mut cost = vec![NONE; n * n];
            for (i, &from) in gates.iter().enumerate() {
                let walks = self.local(t, from, false, &gates);
                for (j, &to) in gates.iter().enumerate() {
                    cost[i * n + j] = self
                        .locate(to)
                        .map_or(NONE, |(_, l)| walks.get(l).copied().unwrap_or(NONE));
                }
            }
            TileGraph { gates, cost }
        })
    }

    /// Cheapest walks from node `p` of tile `t` to each of its gates (or from each of them to
    /// `p`, `back`), remembered for all the threads.
    fn ends(&self, t: usize, p: Node, back: bool) -> Arc<[u32]> {
        let key = (p, back);
        if let Some(known) = self.ends[t].locked().get(&key) {
            return Arc::clone(known);
        }
        let walks = self.local(t, p, back, &self.graph(t).gates);
        let costs: Arc<[u32]> = (self.graph(t).gates.iter())
            .map(|&g| {
                self.locate(g)
                    .map_or(NONE, |(_, l)| walks.get(l).copied().unwrap_or(NONE))
            })
            .collect();
        let mut memo = self.ends[t].locked();
        if memo.len() >= ENDS_MAX {
            memo.clear();
        }
        memo.insert(key, Arc::clone(&costs));
        costs
    }

    /// Walk (yards) from node `a` to node `b` through the gates, None when they do not lead
    /// there; its floors in `trace`: `a`, the gates it goes through, `b`.
    pub(super) fn through_gates(&self, a: Node, b: Node, trace: Option<&mut Vec<Node>>) -> Option<f64> {
        let ((ta, _), (tb, _)) = (self.tile(a.r(), a.c())?, self.tile(b.r(), b.c())?);
        let (start, goal) = (self.ends(ta, a, false), self.ends(tb, b, true));
        // Estimate: the length to `b`, as floor by floor.
        let (br, bc) = b.cell();
        let h = |n: Node| {
            let (dr, dc) = ((n.r() - br).unsigned_abs(), (n.c() - bc).unsigned_abs());
            10 * dr.max(dc) + 4 * dr.min(dc)
        };
        let mut best = Best::default();
        let mut open = BinaryHeap::new();
        // Gate `j` of the tile whose gates start at `row` in `best` reached at cost `g`.
        let reach = |best: &mut Best,
                     open: &mut BinaryHeap<Reverse<(u32, u32, usize, usize)>>,
                     (t, j, row): (usize, usize, usize),
                     g: u32,
                     at: Node,
                     from: (usize, usize)| {
            let known = &mut best.gates[row + j];
            if g < known.0 {
                *known = (g, from);
                open.push(Reverse((g + h(at), g, t, j)));
            }
        };
        let gates = &self.graph(ta).gates;
        let row = best.row(ta, gates.len());
        for (i, &gate) in gates.iter().enumerate() {
            let g = start.get(i).copied().unwrap_or(NONE);
            if g != NONE {
                reach(&mut best, &mut open, (ta, i, row), g, gate, FROM_A);
            }
        }
        let (mut found, mut last) = (NONE, FROM_A);
        let mut across: Vec<(Node, u32)> = Vec::new();
        while let Some(Reverse((f, g, t, i))) = open.pop() {
            if f >= found {
                break;
            }
            let graph = self.graph(t);
            let Some(&row) = best.tiles.get(&t) else {
                continue;
            };
            if g > best.gates[row + i].0 {
                continue;
            }
            let n = graph.gates.len();
            let cost = &graph.cost[i * n..(i + 1) * n];
            let gate = graph.gates[i];
            if t == tb {
                let rest = goal.get(i).copied().unwrap_or(NONE);
                if rest != NONE && g + rest < found {
                    (found, last) = (g + rest, (t, i));
                }
            }
            for (j, &other) in graph.gates.iter().enumerate() {
                let c = cost[j];
                if j != i && c != NONE {
                    reach(&mut best, &mut open, (t, j, row), g + c, other, (t, i));
                }
            }
            // Across the border: the steps into the next tiles' gates.
            across.clear();
            self.steps(gate, |next, step, _| {
                if next.cell() != gate.cell() && (next.r() == gate.r()) != (next.c() == gate.c()) {
                    across.push((next, step));
                }
            });
            for &(next, step) in &across {
                let Some((u, _)) = self.tile(next.r(), next.c()) else {
                    continue;
                };
                if u == t {
                    continue;
                }
                let gates = &self.graph(u).gates;
                if let Ok(j) = gates.binary_search(&next) {
                    let row = best.row(u, gates.len());
                    reach(&mut best, &mut open, (u, j, row), g + step, next, (t, i));
                }
            }
        }
        if found == NONE {
            return None;
        }
        if let Some(trace) = trace {
            trace.clear();
            trace.push(b);
            let mut node = last;
            while node != FROM_A {
                trace.push(self.graph(node.0).gates[node.1]);
                node = best.get(node).1;
            }
            trace.push(a);
            trace.reverse();
        }
        Some(f64::from(found) / 10.0 * CELL)
    }
}
