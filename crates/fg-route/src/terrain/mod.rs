//! Walking over the game's geometry, from the walkable floors the databases hold
//! (`client_terrain_layers`, see `layers`): cells of 8.33 yards, 64 x 64 a tile, each with the
//! floors a character stands on there (the terrain, a cave under it, a bridge or the streets of
//! a city over it), each floor knowing which floors of the neighbour cells it walks to and which
//! it drops down to (one way: a ledge, a cliff jumped down).
//!
//! A walk goes from floor to floor (a node: a cell and one of its floors). A step costs its
//! length, more swimming. It is never taken into magma nor the open sea (fatigue).
//!
//! A place of known height (`Pos::z`: a spawn of the server data, a flight master) in a cell of
//! several floors stands on the floor of a large walkable area nearest to it in height, when one
//! is within `HEIGHT_GAP` yards: the mine under the hill, the bridge over the river (a place on
//! no such floor, the nearest cells around that have some, `HEIGHT_REACH`). A place of unknown height (or no floor that high) stands on the floor of its
//! cell in the large walkable area most of the floors around it are in (the street of a city
//! rather than the ground under it), else on the nearest floor of that area or of a large one (an
//! NPC in a building, on a ledge).
//!
//! A straight line of plain steps (dry, safe) is the walk; otherwise the cheapest path (A* on
//! the floors, cached). Points it does not connect keep the straight line. The large areas
//! declared enclosed (`[[enclosed]]` of `overrides/passes.toml`: an island, a cliff top) are
//! left only by a boat, a portal or a lift (`overrides/travel.toml`) or their passages; places
//! in a cave the floors miss (`[[cave]]`) are walked to through its mouth.
//!
//! The paths searched are slow to find (the first route of a long guide asks for thousands):
//! they are kept in a walking memory (`Walks`) that the app shares between generations, and
//! saved with each guide (`WalkMemory`).

mod gates;
pub mod layers;

pub use gates::GATES_VERSION;

use crate::sync::{Joined, LockExt, Unpoisoned};
use crate::world::Pos;
use base64::Engine as _;
use base64::engine::general_purpose::STANDARD as BASE64;
use layers::{DEEP, DIRECTIONS, HAZARD, SURFACE, TileLayers, WET};
use rusqlite::Connection;
use rustc_hash::FxHashMap;
use serde::{Deserialize, Deserializer, Serialize, Serializer};
use std::cell::RefCell;
use std::collections::{HashMap, HashSet};
use std::hash::Hasher;
use std::sync::atomic::{AtomicU32, Ordering};
use std::sync::{Arc, Mutex};

const TILE: f64 = 1600.0 / 3.0;
const CELL: f64 = TILE / 64.0;
/// World coordinate of the north-west corner of tile (0, 0).
const ORIGIN: f64 = 32.0 * TILE;
/// Places on no floor of a large walkable area (an NPC in a building, on a ledge) move to the
/// nearest one this close (cells).
const SNAP: i32 = 12;
/// The floors around a place that tell which large area it stands in (cells).
const AROUND: i32 = 3;
/// A place of known height stands on a floor at most this far from it in height (yards): the
/// floors of a cell have one height, its ground may slope (a spawn on a hillside). 94 % of the
/// server data's spawns in cells of floors 10 yards apart or more are within 6 yards of one.
pub(crate) const HEIGHT_GAP: i32 = 8;
/// Cells around a place of known height where its floor may be, when none of its cell is that
/// high (a spawn at the edge of a bridge, of a tower's top).
const HEIGHT_REACH: i32 = 2;
pub const LARGE_AREA: u32 = 2000;
/// The path is searched in the box around both ends, this much larger (cells), then larger.
const MARGINS: [i32; 2] = [60, 240];
const SHARDS: usize = 32;
/// Water at least this deep (yards) is swum.
const SWIM_DEPTH: u8 = 2;
/// Cost of a step in tenths of a cell: straight and diagonal, walked then swum (a third
/// slower than running); a drop within a cell (off a ledge onto the floor under it).
const WALKED: [u32; 2] = [10, 14];
const SWUM: [u32; 2] = [15, 21];
const DROPPED: u32 = 5;
/// What a drop of each tier (`layers::DROP_HEIGHTS`) costs more than the step: the health it
/// takes (`0,018 × yards − 0,2426` of it, past the safe fall) got back by resting, a full heal
/// taking a minute, at a tenth of a cell every 0,12 s: 30 yards take 30 % (18 s), 50 yards two
/// thirds (40 s).
const DROP_COST: [u32; layers::DROP_HEIGHTS.len()] = [0, 150, 330];
/// Buckets of the path search's open floors: more than the largest rise of an estimate (the
/// dearest step, a swum diagonal drop of the farthest tier, plus 14).
const BUCKETS: usize = 512;

/// A floor of a cell (a node of the walks): the cell's row and column (from the north and the
/// west, each under 4096) and the floor's index in the cell, packed (ordered by row, column,
/// floor).
#[derive(Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord, Debug)]
pub(crate) struct Node(u32);

impl Node {
    fn new(r: i32, c: i32, f: usize) -> Self {
        Self((r as u32) << 15 | (c as u32) << 3 | f as u32)
    }

    fn r(self) -> i32 {
        (self.0 >> 15) as i32
    }

    fn c(self) -> i32 {
        (self.0 >> 3 & 0xfff) as i32
    }

    fn f(self) -> usize {
        (self.0 & 7) as usize
    }

    fn cell(self) -> (i32, i32) {
        (self.r(), self.c())
    }
}

/// Continent and pair of floors of a walk.
type Key = (i64, Node, Node);

/// Hash of the terrain data (its fingerprint, saved with the walking memories: must not change).
#[derive(Default)]
struct Packed(u64);

impl Hasher for Packed {
    fn finish(&self) -> u64 {
        self.0
    }
    fn write(&mut self, bytes: &[u8]) {
        for &b in bytes {
            self.0 = (self.0 ^ u64::from(b)).wrapping_mul(0x0100_0000_01b3);
        }
    }
    fn write_u64(&mut self, n: u64) {
        self.0 = (self.0 ^ n).wrapping_mul(0x9e37_79b9_7f4a_7c15);
    }
    fn write_u32(&mut self, n: u32) {
        self.write_u64(u64::from(n));
    }
}

/// Answers already given in this thread, by terrain, continent and pair of cells: the planner
/// asks the same walks millions of times. Negative: the straight line; infinite: not connected;
/// NaN: not known.
type MemoKey = (u32, i64, u128);
type Memo = FxHashMap<MemoKey, f32>;
thread_local! {
    static MEMO: RefCell<Memo> = RefCell::new(Memo::default());
    /// `Terrain::disconnected` already answered, by terrain, continent and pair of cells.
    static APART: RefCell<FxHashMap<(u32, i64, u128), bool>> = RefCell::new(FxHashMap::default());
    /// `Grid::snap` of the cells without a single floor in a large area, by terrain, continent
    /// and cell: the planner asks for the same places again and again.
    static SNAPS: RefCell<FxHashMap<SnapKey, Option<Node>>> = RefCell::new(FxHashMap::default());
}
/// The root of `x` in a union-find over `parent`, the path halved on the way.
fn find(parent: &mut [u32], mut x: u32) -> u32 {
    while parent[x as usize] != x {
        parent[x as usize] = parent[parent[x as usize] as usize];
        x = parent[x as usize];
    }
    x
}

/// A tile's areas (`Grid::label_tile`): each node's and their sizes.
type TileLabels = (Vec<u32>, Vec<u32>);
/// A cell of `SNAPS`: terrain, continent, row, column and height.
type SnapKey = (u32, i64, i32, i32, Option<i16>);
/// Where a place stands for the walks: its cell and its height when known (`Pos::z`).
type Spot = ((i32, i32), Option<i16>);
/// Walks `MEMO` remembers in a thread, beyond which it starts over (some 80 MB).
/// What a table of 2^21 buckets holds (7/8 of them): it never doubles for the last ones.
const MEMO_MAX: usize = (1 << 21) / 8 * 7;
/// Cells `SNAPS` remembers, beyond which it starts over.
/// What a table of 2^18 buckets holds (7/8 of them): it never doubles for the last ones.
const SNAPS_MAX: usize = (1 << 18) / 8 * 7;
/// The turns of the walks already followed (`Terrain::waypoints`), by continent, pair of cells
/// and pair of heights, in shards; a shard is emptied beyond `TURNS_MAX`. A turn: its cell and
/// the height of the floor walked there.
type TurnKey = (i64, i32, i32, i32, i32, Option<i16>, Option<i16>);
type Turns = Vec<Mutex<FxHashMap<TurnKey, Arc<[(i32, i32, Option<i16>)]>>>>;
/// What a table of 2^16 buckets holds (7/8 of them): it never doubles for the last ones.
const TURNS_MAX: usize = (1 << 16) / 8 * 7;
static NEXT_ID: AtomicU32 = AtomicU32::new(1);

/// The path search's work space, kept from a search to the next in each thread: the cheapest
/// cost found for each floor of the box and the search it was found in (valid when it is this
/// one's: nothing to clear), side by side for one memory read, and the open floors by estimate.
struct Scratch {
    best: Vec<(u32, u32)>,
    current: u32,
    open: Vec<Vec<(u32, Node)>>,
}

impl Scratch {
    fn new() -> Self {
        Self {
            best: Vec::new(),
            current: 0,
            open: (0..BUCKETS).map(|_| Vec::new()).collect(),
        }
    }
}

thread_local! {
    static SCRATCH: RefCell<Scratch> = RefCell::new(Scratch::new());
}

/// Gives back the memory of this thread's walks and searches (`threads::Release`).
pub(crate) fn release_thread_caches() {
    let _ = MEMO.try_with(|m| std::mem::take(&mut *m.borrow_mut()));
    let _ = APART.try_with(|m| std::mem::take(&mut *m.borrow_mut()));
    let _ = SNAPS.try_with(|m| std::mem::take(&mut *m.borrow_mut()));
    let _ = SCRATCH.try_with(|s| *s.borrow_mut() = Scratch::new());
    gates::release_thread_caches();
    layers::release_thread_caches();
}

#[cfg(test)]
pub(crate) fn fill_thread_caches_for_tests() {
    MEMO.with(|m| m.borrow_mut().insert((0, 0, 0), 1.0));
}

/// The room this thread's walks take (entries).
#[cfg(test)]
pub(crate) fn thread_cache_capacity() -> usize {
    MEMO.with(|m| m.borrow().capacity())
        + APART.with(|m| m.borrow().capacity())
        + SNAPS.with(|m| m.borrow().capacity())
        + SCRATCH.with(|s| s.borrow().best.capacity())
}

/// Bits of a floor's moves (`TileNodes::moves`), by direction of `DIRECTIONS`: the step to the
/// neighbour's first floor can be taken, it lands in water swum, it is a drop (one way); the
/// floor is never stood on (`MOVE_NO_GO`); it has other steps (`TileNodes::edges`).
const MOVE: u32 = 1;
const MOVE_SWUM: u32 = 1 << 8;
const MOVE_DROP: u32 = 1 << 16;
const MOVE_NO_GO: u32 = 1 << 24;
const MOVE_MORE: u32 = 1 << 25;

/// The direction of each drop slot (`DROP_DIRECTIONS`): as `DIRECTIONS`, 8 for the same cell.
const DROP_DIRECTION: [usize; 5] = [0, 1, 2, 3, 8];
/// Index in `DIRECTIONS` of a step (row, column) + 1, as `(dr + 1) * 3 + dc + 1`.
const DIRECTION_OF: [usize; 9] = [4, 0, 5, 2, usize::MAX, 3, 6, 1, 7];

/// Flags of a floor for a step: never stood on (magma, the open sea), swum.
const NO_GO: u8 = 1;
const SWIM: u8 = 2;

/// A step to a floor of a neighbour cell other than its first, or within the cell: the
/// direction (`DIRECTIONS`, 8: the same cell), the floor, swum, a drop and its tier
/// (`layers::DROP_HEIGHTS`); packed.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct Edge(u16);

impl Edge {
    fn new(d: usize, f: usize, swum: bool, drop: bool, tier: usize) -> Self {
        Self(d as u16 | (f as u16) << 4 | u16::from(swum) << 7 | u16::from(drop) << 8 | (tier as u16) << 9)
    }

    fn tier(self) -> usize {
        usize::from(self.0 >> 9 & 3)
    }

    fn dir(self) -> usize {
        usize::from(self.0 & 15)
    }

    fn floor(self) -> usize {
        usize::from(self.0 >> 4 & 7)
    }

    fn drop(self) -> bool {
        self.0 >> 8 & 1 != 0
    }

    /// The cell it leads to from (r, c).
    fn to(self, r: i32, c: i32) -> (i32, i32) {
        match self.dir() {
            8 => (r, c),
            d => (r + DIRECTIONS[d].0, c + DIRECTIONS[d].1),
        }
    }

    /// Its cost (tenths of a cell).
    fn cost(self) -> u32 {
        let swum = self.0 >> 7 & 1 != 0;
        DROP_COST[self.tier()]
            + match self.dir() {
                8 => DROPPED,
                d => {
                    let diagonal = usize::from(d >= 4);
                    if swum { SWUM[diagonal] } else { WALKED[diagonal] }
                }
            }
    }
}

/// The cost of a fast move (`MOVE` bits) in direction `d`.
#[inline]
fn move_cost(moves: u32, d: usize) -> u32 {
    let diagonal = usize::from(d >= 4);
    if moves >> d & MOVE_SWUM != 0 {
        SWUM[diagonal]
    } else {
        WALKED[diagonal]
    }
}

/// A tile's floors (its nodes): the first floor of each cell is node `i` (the cell's index, row
/// by row; a cell without floor has its node never stood on), the others follow from 4096.
#[derive(Clone)]
struct TileNodes {
    /// Index of each cell's second floor among the nodes, less 4096 (its other floors follow),
    /// and one past the last cell's.
    more: Box<[u16]>,
    /// Floors of each cell.
    count: Box<[u8]>,
    /// The cell of each node from 4096.
    more_cell: Box<[u16]>,
    /// Height (yards) and water (`layers`) of each node, its layer flags (`SURFACE`...), its
    /// flags for a step (`NO_GO`, `SWIM`).
    height: Box<[i16]>,
    water: Box<[u8]>,
    kind: Box<[u8]>,
    flags: Box<[u8]>,
    /// Each node's moves (`MOVE`...).
    moves: Box<[u32]>,
    /// The other steps of the nodes with `MOVE_MORE`, sorted by node.
    edges: Box<[(u16, Box<[Edge]>)]>,
}

impl TileNodes {
    fn len(&self) -> usize {
        self.height.len()
    }

    /// The node of floor `f` of cell `i`, None when the cell has no such floor.
    #[inline]
    fn local(&self, i: usize, f: usize) -> Option<usize> {
        if f >= usize::from(self.count[i]) {
            None
        } else if f == 0 {
            Some(i)
        } else {
            Some(4096 + usize::from(self.more[i]) + f - 1)
        }
    }

    /// The cell and floor of node `l`.
    fn cell_floor(&self, l: usize) -> (usize, usize) {
        if l < 4096 {
            (l, 0)
        } else {
            let i = usize::from(self.more_cell[l - 4096]);
            (i, l - 4096 - usize::from(self.more[i]) + 1)
        }
    }

    /// Whether node `l` walks (not a drop) in direction `d` to floor `f` of the neighbour cell
    /// there (`Grid::walks_to`, read from this tile only).
    #[inline]
    fn walks_dir(&self, l: usize, d: usize, f: usize) -> bool {
        let moves = self.moves[l];
        if moves & MOVE_NO_GO != 0 {
            false
        } else if f == 0 {
            moves >> d & MOVE != 0 && moves >> d & MOVE_DROP == 0
        } else {
            moves & MOVE_MORE != 0 && (self.edges(l).iter()).any(|e| e.dir() == d && e.floor() == f && !e.drop())
        }
    }

    /// The other steps of node `l` (`MOVE_MORE`).
    fn edges(&self, l: usize) -> &[Edge] {
        match self.edges.binary_search_by_key(&(l as u16), |e| e.0) {
            Ok(k) => &self.edges[k].1,
            Err(_) => &[],
        }
    }
}

/// A tile's moves and other steps (`TileNodes::moves`, `TileNodes::edges`).
type TileLinks = (Box<[u32]>, Box<[(u16, Box<[Edge]>)]>);

/// One continent's floors, over the box of its tiles.
struct Grid {
    r0: i32,
    c0: i32,
    rows: i32,
    cols: i32,
    tile_cols: usize,
    /// The floors of each tile (None: none), by tile of 64 x 64 cells (row by row, `tile_cols`
    /// a row). Shared with the terrain it was copied from (`fork`) until one of them changes a
    /// tile (`tile_mut`): the copy of a whole terrain costs a hundred megabytes.
    tiles: Vec<Option<Arc<TileNodes>>>,
    /// Where the nodes of each tile beyond its first floors start among the continent's
    /// (`Scratch::best`), and their count.
    extra_base: Vec<u32>,
    extras: u32,
    /// Connected walkable area of each node (0: never stood on), by tile, and the size of each
    /// area (nodes).
    areas: Vec<TileAreas>,
    sizes: Vec<u32>,
    /// The gates of each tile and the walks between them, found when first needed (`gates`).
    graphs: Vec<std::sync::OnceLock<gates::TileGraph>>,
    /// The walks from nodes of each tile to its gates and back (`gates`).
    ends: Vec<Mutex<gates::Ends>>,
    /// The walkable areas within each tile, found when first needed (`gates`).
    local_areas: Vec<std::sync::OnceLock<gates::LocalAreas>>,
    /// The caves the floors miss (`Terrain::add_cave`).
    caves: Vec<Cave>,
    /// The tiers of drops taken after the first (`layers::DROP_HEIGHTS`): 0 only the safe fall.
    drop_tier: usize,
}

/// A cave (`[[cave]]` of `overrides/passes.toml`) the floors miss: the places on no large
/// walkable area within `reach` cells of `inside` are walked to through the mouth, a floor of a
/// large area.
#[derive(Clone)]
struct Cave {
    inside: (i32, i32),
    reach: i32,
    mouth: Node,
    mouth_pos: Pos,
}

/// What a cell is for walking (`Terrain::view`), to look at the terrain: its first floor.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct CellView {
    /// Height of its first floor (yards).
    pub height: i16,
    /// No floor stood on.
    pub steep: bool,
    /// Its floors.
    pub floors: u8,
    /// Water depth over the ground (yards, 0: dry); swum from `SWIM_DEPTH`.
    pub depth: u8,
    pub swum: bool,
    /// Open sea (fatigue) or magma, slime: never stood in. `sea_edge`: the client's line of the
    /// open sea, no longer known (false).
    pub deep: bool,
    pub sea_edge: bool,
    pub hazard: bool,
}

/// The areas of a tile's nodes: one for the whole tile, else each node's.
#[derive(Clone)]
enum TileAreas {
    Uniform(u32),
    Nodes(Box<[u32]>),
}

impl Grid {
    /// A copy with the floors, their areas and the stored graphs of the gates; the walks found
    /// since are not copied.
    fn fork(&self) -> Self {
        Self {
            r0: self.r0,
            c0: self.c0,
            rows: self.rows,
            cols: self.cols,
            tile_cols: self.tile_cols,
            tiles: self.tiles.clone(),
            extra_base: self.extra_base.clone(),
            extras: self.extras,
            areas: self.areas.clone(),
            sizes: self.sizes.clone(),
            graphs: (self.graphs.iter())
                .map(|g| g.get().map_or_else(std::sync::OnceLock::new, |g| g.clone().into()))
                .collect(),
            ends: (0..self.ends.len()).map(|_| Mutex::default()).collect(),
            local_areas: (0..self.local_areas.len())
                .map(|_| std::sync::OnceLock::new())
                .collect(),
            caves: self.caves.clone(),
            drop_tier: self.drop_tier,
        }
    }

    /// Tile `t`'s floors to change, copied first when shared with another terrain.
    fn tile_mut(&mut self, t: usize) -> Option<&mut TileNodes> {
        self.tiles[t].as_mut().map(Arc::make_mut)
    }

    /// Index of a cell's tile and its index in it, None out of the box.
    #[inline]
    fn tile(&self, r: i32, c: i32) -> Option<(usize, usize)> {
        let (r, c) = (r - self.r0, c - self.c0);
        if r < 0 || c < 0 || r >= self.rows || c >= self.cols {
            return None;
        }
        let (r, c) = (r as usize, c as usize);
        Some(((r >> 6) * self.tile_cols + (c >> 6), (r & 63) << 6 | (c & 63)))
    }

    /// The floors of tile `t`.
    #[inline]
    fn nodes(&self, t: usize) -> Option<&TileNodes> {
        self.tiles.get(t)?.as_deref()
    }

    /// Tile and index in it of a node, None out of the terrain or when the cell has no such
    /// floor.
    #[inline]
    fn locate(&self, n: Node) -> Option<(usize, usize)> {
        let (t, i) = self.tile(n.r(), n.c())?;
        Some((t, self.nodes(t)?.local(i, n.f())?))
    }

    /// The floors of cell (r, c).
    fn floors(&self, r: i32, c: i32) -> usize {
        self.tile(r, c)
            .and_then(|(t, i)| Some(usize::from(self.nodes(t)?.count[i])))
            .unwrap_or(0)
    }

    /// The moves of a node (`MOVE_NO_GO` out of the terrain).
    #[inline]
    fn moves(&self, n: Node) -> u32 {
        self.locate(n)
            .and_then(|(t, l)| Some(self.nodes(t)?.moves[l]))
            .unwrap_or(MOVE_NO_GO)
    }

    /// Whether a node is never stood on (or not there).
    fn blocked(&self, n: Node) -> bool {
        self.moves(n) & MOVE_NO_GO != 0
    }

    /// Each step from node `n`: the node it leads to, its cost (tenths of a cell) and whether
    /// it is a drop.
    #[inline]
    fn steps(&self, n: Node, mut each: impl FnMut(Node, u32, bool)) {
        let Some((t, l)) = self.locate(n) else { return };
        let Some(tile) = self.nodes(t) else { return };
        let moves = tile.moves[l];
        if moves & MOVE_NO_GO != 0 {
            return;
        }
        let (r, c) = n.cell();
        for (d, (dr, dc)) in DIRECTIONS.into_iter().enumerate() {
            if moves >> d & MOVE != 0 {
                each(
                    Node::new(r + dr, c + dc, 0),
                    move_cost(moves, d),
                    moves >> d & MOVE_DROP != 0,
                );
            }
        }
        if moves & MOVE_MORE != 0 {
            for &e in tile.edges(l) {
                let (nr, nc) = e.to(r, c);
                each(Node::new(nr, nc, e.floor()), e.cost(), e.drop());
            }
        }
    }

    /// The cost of the step from `a` to `b` (None: not taken), the cheapest when several.
    #[inline]
    fn step(&self, a: Node, b: Node) -> Option<u32> {
        let (dr, dc) = (b.r() - a.r(), b.c() - a.c());
        if b.f() == 0 && dr.abs() <= 1 && dc.abs() <= 1 && (dr, dc) != (0, 0) {
            let moves = self.moves(a);
            let d = DIRECTION_OF[((dr + 1) * 3 + dc + 1) as usize];
            if moves & MOVE_NO_GO != 0 || moves >> d & MOVE == 0 {
                return None;
            }
            return Some(move_cost(moves, d));
        }
        let mut best: Option<u32> = None;
        self.steps(a, |n, cost, _| {
            if n == b && best.is_none_or(|x| cost < x) {
                best = Some(cost);
            }
        });
        best
    }

    /// Whether node `a` walks to node `b` (not a drop; within a cell, only drops).
    fn walks_to(&self, a: Node, b: Node) -> bool {
        let (dr, dc) = (b.r() - a.r(), b.c() - a.c());
        if dr.abs() > 1 || dc.abs() > 1 || (dr, dc) == (0, 0) {
            return false;
        }
        let Some((t, l)) = self.locate(a) else { return false };
        let Some(tile) = self.nodes(t) else { return false };
        let moves = tile.moves[l];
        if moves & MOVE_NO_GO != 0 {
            return false;
        }
        let d = DIRECTION_OF[((dr + 1) * 3 + dc + 1) as usize];
        if b.f() == 0 {
            moves >> d & MOVE != 0 && moves >> d & MOVE_DROP == 0
        } else {
            moves & MOVE_MORE != 0 && (tile.edges(l).iter()).any(|e| e.dir() == d && e.floor() == b.f() && !e.drop())
        }
    }

    /// Whether two nodes of straight neighbour cells walk to each other: joined in an area.
    fn joined(&self, a: Node, b: Node) -> bool {
        self.walks_to(a, b) && self.walks_to(b, a)
    }

    /// The area of a node (0: none).
    fn area(&self, n: Node) -> u32 {
        let Some((t, l)) = self.locate(n) else {
            return 0;
        };
        match &self.areas[t] {
            TileAreas::Uniform(a) => *a,
            TileAreas::Nodes(nodes) => nodes[l],
        }
    }

    /// Whether a node stands in a large area.
    fn large(&self, n: Node) -> bool {
        let a = self.area(n);
        a != 0 && self.sizes[a as usize] >= LARGE_AREA
    }

    /// The height of a node (yards; 0 out of the terrain).
    fn height(&self, n: Node) -> i16 {
        self.locate(n)
            .and_then(|(t, l)| Some(self.nodes(t)?.height[l]))
            .unwrap_or(0)
    }

    /// The height of a node when the floors keep it: None on a cell of a single floor
    /// (`TileLayers::lean`).
    fn known_height(&self, n: Node) -> Option<i16> {
        (self.floors(n.r(), n.c()) >= 2).then(|| self.height(n))
    }

    /// The floor of a large area nearest in height to `z`, at most `HEIGHT_GAP` yards from it,
    /// of the nearest cells around (r, c) holding floors of large areas (`HEIGHT_REACH`): None
    /// when they have none that high, or when one of them has a single floor (its height is not
    /// kept, `TileLayers::lean`: the place stands on it, as `snap` says).
    fn floor_at(&self, (r, c): (i32, i32), z: i16) -> Option<Node> {
        for d in 0..=HEIGHT_REACH {
            let (mut best, mut seen): (Option<(i32, Node)>, bool) = (None, false);
            for dr in -d..=d {
                for dc in -d..=d {
                    if dr.abs() != d && dc.abs() != d {
                        continue;
                    }
                    let single = self.floors(r + dr, c + dc) < 2;
                    for n in self.large_floors(r + dr, c + dc) {
                        seen = true;
                        let gap = (i32::from(self.height(n)) - i32::from(z)).abs();
                        if !single && gap <= HEIGHT_GAP && best.is_none_or(|b| (gap, n) < b) {
                            best = Some((gap, n));
                        }
                    }
                }
            }
            if best.is_some() || seen {
                return best.map(|b| b.1);
            }
        }
        None
    }

    /// The node a place of cell (r, c) and height `z` stands on (see the module): the floor
    /// that high when there is one, else `snap`.
    fn snap_at(&self, (r, c): (i32, i32), z: Option<i16>) -> Option<Node> {
        z.and_then(|z| self.floor_at((r, c), z)).or_else(|| self.snap((r, c)))
    }

    /// Whether a node is the terrain's own surface.
    fn is_surface(&self, n: Node) -> bool {
        self.locate(n)
            .and_then(|(t, l)| Some(self.nodes(t)?.kind[l] & SURFACE != 0))
            .unwrap_or(false)
    }

    /// The floors of cell (r, c) stood on in a large area.
    fn large_floors(&self, r: i32, c: i32) -> impl Iterator<Item = Node> + '_ {
        (0..self.floors(r, c))
            .map(move |f| Node::new(r, c, f))
            .filter(|&n| !self.blocked(n) && self.large(n))
    }

    /// The cave a cell is in: no floor of a large area, within a cave's reach.
    fn cave_at(&self, (r, c): (i32, i32)) -> Option<&Cave> {
        if self.caves.is_empty() || self.large_floors(r, c).next().is_some() {
            return None;
        }
        (self.caves.iter()).find(|k| (r - k.inside.0).pow(2) + (c - k.inside.1).pow(2) <= k.reach.pow(2))
    }

    /// The node a place in cell (r, c) stands on (see the module): None when no floor stood on
    /// is near.
    fn snap(&self, (r, c): (i32, i32)) -> Option<Node> {
        let here: Vec<Node> = self.large_floors(r, c).collect();
        if here.len() == 1 {
            return here.first().copied();
        }
        if here.is_empty()
            && let Some(cave) = self.cave_at((r, c))
        {
            return Some(cave.mouth);
        }
        // The large areas of the floors around, by how many of them.
        let mut votes: Vec<(u32, u32)> = Vec::new();
        for dr in -AROUND..=AROUND {
            for dc in -AROUND..=AROUND {
                for n in self.large_floors(r + dr, c + dc) {
                    let a = self.area(n);
                    match votes.iter_mut().find(|v| v.0 == a) {
                        Some(v) => v.1 += 1,
                        None => votes.push((a, 1)),
                    }
                }
            }
        }
        let wanted = votes
            .iter()
            .max_by_key(|&&(a, n)| (n, std::cmp::Reverse(a)))
            .map(|v| v.0);
        // Of floors, the wanted area's (any when None), the terrain's surface first.
        let pick = |floors: &mut dyn Iterator<Item = Node>, area: Option<u32>| {
            floors
                .filter(|&n| area.is_none_or(|a| self.area(n) == a))
                .min_by_key(|&n| (!self.is_surface(n), n.f()))
        };
        if !here.is_empty() {
            return pick(&mut here.iter().copied(), wanted).or_else(|| pick(&mut here.iter().copied(), None));
        }
        // The nearest floor of that area, else of any large one.
        for area in [wanted, None] {
            for d in 1..=SNAP {
                for dr in -d..=d {
                    for dc in -d..=d {
                        if dr.abs() != d && dc.abs() != d {
                            continue;
                        }
                        if let Some(n) = pick(&mut self.large_floors(r + dr, c + dc), area) {
                            return Some(n);
                        }
                    }
                }
            }
            if area.is_none() {
                break;
            }
        }
        // No large area near: any floor stood on, of the cell, else the nearest.
        let walkable = |r: i32, c: i32| {
            (0..self.floors(r, c))
                .map(move |f| Node::new(r, c, f))
                .find(|&n| !self.blocked(n))
        };
        if let Some(n) = walkable(r, c) {
            return Some(n);
        }
        for d in 1..=SNAP {
            for dr in -d..=d {
                for dc in -d..=d {
                    if (dr.abs() == d || dc.abs() == d)
                        && let Some(n) = walkable(r + dr, c + dc)
                    {
                        return Some(n);
                    }
                }
            }
        }
        None
    }

    /// The node `l` of tile `t`.
    fn node_of(&self, t: usize, l: usize) -> Node {
        let (tr, tc) = self.origin(t);
        let (i, f) = self.nodes(t).map_or((l, 0), |x| x.cell_floor(l));
        Node::new(tr + (i / 64) as i32, tc + (i % 64) as i32, f)
    }

    /// First row and column of tile `t`.
    fn origin(&self, t: usize) -> (i32, i32) {
        (
            self.r0 + (t / self.tile_cols) as i32 * 64,
            self.c0 + (t % self.tile_cols) as i32 * 64,
        )
    }

    /// Label the walkable areas: nodes of straight neighbour cells walking to each other (as
    /// paths do not cut corners), a drop down a cliff does not join its top to its foot. Each
    /// tile's own areas side by side, then joined across the tiles' borders; numbered by their
    /// first node (tile by tile, node by node).
    fn label(&mut self) {
        let workers = std::thread::available_parallelism()
            .map_or(4, std::num::NonZero::get)
            .min(8);
        let this = &*self;
        let n = self.tiles.len();
        let mut locals: Vec<(Vec<u32>, Vec<u32>)> = std::thread::scope(|scope| {
            let parts: Vec<_> = (0..workers)
                .map(|w| {
                    scope.spawn(move || {
                        let _release = crate::threads::Release;
                        (w..n)
                            .step_by(workers)
                            .map(|t| (t, this.label_tile(t)))
                            .collect::<Vec<_>>()
                    })
                })
                .collect();
            let mut all: Vec<(usize, TileLabels)> = parts.into_iter().flat_map(|p| p.join().joined()).collect();
            all.sort_unstable_by_key(|x| x.0);
            all.into_iter().map(|x| x.1).collect()
        });
        // Each tile's areas among all (`base[t] + area - 1`), joined across the borders.
        let mut base = vec![0usize; n + 1];
        for t in 0..n {
            base[t + 1] = base[t] + locals[t].1.len();
        }
        let mut parent: Vec<u32> = (0..base[n] as u32).collect();
        for t in 0..n {
            let Some(tile) = self.nodes(t) else { continue };
            let (tr, tc) = self.origin(t);
            // The steps south out of the last row and east out of the last column: each border
            // once (the joins go both ways).
            for (d, cells) in [
                (1usize, (63 * 64..4096).collect::<Vec<_>>()),
                (3, (0..64).map(|r| r * 64 + 63).collect()),
            ] {
                let (dr, dc) = DIRECTIONS[d];
                for i in cells {
                    let (r, c) = (tr + (i / 64) as i32 + dr, tc + (i % 64) as i32 + dc);
                    let Some((u, j)) = self.tile(r, c) else { continue };
                    let Some(next) = self.nodes(u) else { continue };
                    for f in 0..usize::from(tile.count[i]) {
                        let Some(l) = tile.local(i, f) else { continue };
                        let a = locals[t].0[l];
                        if a == 0 {
                            continue;
                        }
                        for g in 0..usize::from(next.count[j]) {
                            let Some(k) = next.local(j, g) else { continue };
                            let b = locals[u].0[k];
                            if b != 0 && tile.walks_dir(l, d, g) && next.walks_dir(k, layers::OPPOSITE[d], f) {
                                let (x, y) = (
                                    find(&mut parent, (base[t] + a as usize - 1) as u32),
                                    find(&mut parent, (base[u] + b as usize - 1) as u32),
                                );
                                if x != y {
                                    parent[x.max(y) as usize] = x.min(y);
                                }
                            }
                        }
                    }
                }
            }
        }
        // Numbered in the order of their first area.
        let mut id = vec![0u32; base[n]];
        let mut sizes = vec![0u32];
        for t in 0..n {
            for (a, &size) in locals[t].1.iter().enumerate() {
                let root = find(&mut parent, (base[t] + a) as u32) as usize;
                if id[root] == 0 {
                    id[root] = sizes.len() as u32;
                    sizes.push(0);
                }
                sizes[id[root] as usize] += size;
            }
        }
        let numbered: Vec<u32> = (0..base[n]).map(|x| id[find(&mut parent, x as u32) as usize]).collect();
        self.areas = (0..n)
            .map(|t| {
                let (labels, _) = std::mem::take(&mut locals[t]);
                let global = |a: u32| if a == 0 { 0 } else { numbered[base[t] + a as usize - 1] };
                let first = labels.first().copied().map_or(0, global);
                if labels.iter().all(|&a| global(a) == first) {
                    TileAreas::Uniform(first)
                } else {
                    TileAreas::Nodes(labels.iter().map(|&a| global(a)).collect())
                }
            })
            .collect();
        self.sizes = sizes;
    }

    /// The walkable areas within tile `t` (`label`): each node's (0: never stood on, else from
    /// 1, by first node) and the size of each.
    fn label_tile(&self, t: usize) -> (Vec<u32>, Vec<u32>) {
        let Some(tile) = self.nodes(t) else {
            return (Vec::new(), Vec::new());
        };
        let mut area = vec![0u32; tile.len()];
        let mut sizes: Vec<u32> = Vec::new();
        let mut stack: Vec<usize> = Vec::new();
        for l0 in 0..tile.len() {
            if area[l0] != 0 || tile.moves[l0] & MOVE_NO_GO != 0 {
                continue;
            }
            sizes.push(0);
            let id = sizes.len() as u32;
            area[l0] = id;
            stack.push(l0);
            while let Some(l) = stack.pop() {
                sizes[id as usize - 1] += 1;
                let (i, f) = tile.cell_floor(l);
                let (r, c) = ((i / 64) as i32, (i % 64) as i32);
                for (d, &(dr, dc)) in DIRECTIONS.iter().enumerate().take(4) {
                    let (nr, nc) = (r + dr, c + dc);
                    if !(0..64).contains(&nr) || !(0..64).contains(&nc) {
                        continue;
                    }
                    let j = (nr * 64 + nc) as usize;
                    for g in 0..usize::from(tile.count[j]) {
                        let Some(k) = tile.local(j, g) else { continue };
                        if area[k] == 0 && tile.walks_dir(l, d, g) && tile.walks_dir(k, layers::OPPOSITE[d], f) {
                            area[k] = id;
                            stack.push(k);
                        }
                    }
                }
            }
        }
        (area, sizes)
    }

    /// The moves and steps of every node from the tiles' layers (by tile, as `tiles`).
    fn link(&mut self, layers: &[Option<TileLayers>]) {
        // Side by side, by tile.
        let workers = std::thread::available_parallelism()
            .map_or(4, std::num::NonZero::get)
            .min(8);
        let this = &*self;
        let all: Vec<TileLinks> = std::thread::scope(|scope| {
            let parts: Vec<_> = (0..workers)
                .map(|w| {
                    scope.spawn(move || {
                        let _release = crate::threads::Release;
                        (w..layers.len())
                            .step_by(workers)
                            .map(|t| (t, this.link_tile(t, layers[t].as_ref())))
                            .collect::<Vec<_>>()
                    })
                })
                .collect();
            let mut all: Vec<(usize, TileLinks)> = parts.into_iter().flat_map(|p| p.join().joined()).collect();
            all.sort_unstable_by_key(|x| x.0);
            all.into_iter().map(|x| x.1).collect()
        });
        for (t, (moves, edges)) in all.into_iter().enumerate() {
            if let Some(tile) = self.tile_mut(t) {
                tile.moves = moves;
                tile.edges = edges;
            }
        }
    }

    /// The moves and steps of the nodes of tile `t` from its layers (`link`).
    fn link_tile(&self, t: usize, l: Option<&TileLayers>) -> TileLinks {
        {
            let (Some(l), Some(tile)) = (l, self.nodes(t)) else {
                return (Box::new([]), Box::new([]));
            };
            let (tr, tc) = self.origin(t);
            let o = l.offsets();
            let mut moves = vec![MOVE_NO_GO; tile.len()];
            let mut edges: Vec<(u16, Box<[Edge]>)> = Vec::new();
            for (i, (&count, &first)) in l.counts.iter().zip(o.iter()).enumerate() {
                let (r, c) = (tr + (i / 64) as i32, tc + (i % 64) as i32);
                for f in 0..usize::from(count) {
                    let at = tile.local(i, f).unwrap_or(i);
                    if tile.flags[at] & NO_GO != 0 {
                        continue;
                    }
                    let floor = &l.floors[first as usize + f];
                    let mut m = 0u32;
                    let mut more: Vec<Edge> = Vec::new();
                    let mut add = |d: usize, bits: u8, drop: bool, tier: usize| {
                        if bits == 0 {
                            return;
                        }
                        let (dr, dc) = if d == 8 { (0, 0) } else { DIRECTIONS[d] };
                        // The floors stepped to: there (in this tile, else the next), and stood on.
                        let (lr, lc) = ((i / 64) as i32 + dr, (i % 64) as i32 + dc);
                        let (target, j) = if (0..64).contains(&lr) && (0..64).contains(&lc) {
                            (tile, (lr * 64 + lc) as usize)
                        } else {
                            let Some((u, j)) = self.tile(r + dr, c + dc) else {
                                return;
                            };
                            let Some(target) = self.nodes(u) else { return };
                            (target, j)
                        };
                        for b in 0..usize::from(target.count[j]).min(8) {
                            if bits >> b & 1 == 0 {
                                continue;
                            }
                            let k = if b == 0 {
                                j
                            } else {
                                4096 + usize::from(target.more[j]) + b - 1
                            };
                            if target.flags[k] & NO_GO != 0 {
                                continue;
                            }
                            let swum = target.flags[k] & SWIM != 0;
                            if b == 0 && d < 8 && tier == 0 {
                                if m >> d & MOVE == 0 {
                                    m |= MOVE << d;
                                    if swum {
                                        m |= MOVE_SWUM << d;
                                    }
                                    if drop {
                                        m |= MOVE_DROP << d;
                                    }
                                }
                            } else {
                                more.push(Edge::new(d, b, swum, drop, tier));
                            }
                        }
                    };
                    for (d, &bits) in floor.walks.iter().enumerate() {
                        add(d, bits, false, 0);
                    }
                    for (s, &bits) in floor.drops.iter().enumerate() {
                        add(DROP_DIRECTION[s], bits, true, 0);
                    }
                    for tier in 1..=self.drop_tier {
                        for (s, &bits) in floor.farther[tier - 1].iter().enumerate() {
                            add(DROP_DIRECTION[s], bits, true, tier);
                        }
                    }
                    if !more.is_empty() {
                        m |= MOVE_MORE;
                        edges.push((at as u16, more.into()));
                    }
                    moves[at] = m;
                }
            }
            edges.sort_by_key(|e| e.0);
            (moves.into(), edges.into())
        }
    }

    /// Never stand on `closed` nor step to them (`Terrain::close`): their moves and the steps
    /// to them taken out, the areas labelled again, the tiles' gates around made anew.
    fn close(&mut self, closed: &[Node]) {
        let shut: HashSet<Node> = closed.iter().copied().collect();
        let mut tiles: std::collections::BTreeSet<usize> = std::collections::BTreeSet::new();
        for &n in closed {
            let Some((t, l)) = self.locate(n) else { continue };
            if let Some(tile) = self.tile_mut(t) {
                tile.moves[l] = MOVE_NO_GO;
                tile.flags[l] |= NO_GO;
            }
            // The steps to it from the floors around.
            for (d, (dr, dc)) in DIRECTIONS.into_iter().enumerate() {
                let (pr, pc) = (n.r() - dr, n.c() - dc);
                for f in 0..self.floors(pr, pc) {
                    let m = Node::new(pr, pc, f);
                    let Some((u, k)) = self.locate(m) else { continue };
                    tiles.insert(u);
                    let Some(tile) = self.tile_mut(u) else {
                        continue;
                    };
                    if n.f() == 0 {
                        tile.moves[k] &= !(MOVE << d | MOVE_SWUM << d | MOVE_DROP << d);
                    }
                }
            }
            tiles.insert(t);
        }
        // The other steps to them (edges).
        for &t in &tiles {
            let origin = self.origin(t);
            let Some(tile) = self.tile_mut(t) else {
                continue;
            };
            let mut edges = std::mem::take(&mut tile.edges).into_vec();
            for (l, list) in &mut edges {
                let i = if usize::from(*l) < 4096 {
                    usize::from(*l)
                } else {
                    usize::from(tile.more_cell[usize::from(*l) - 4096])
                };
                let (r, c) = (origin.0 + (i / 64) as i32, origin.1 + (i % 64) as i32);
                let kept: Vec<Edge> = list
                    .iter()
                    .copied()
                    .filter(|e| {
                        let (nr, nc) = e.to(r, c);
                        !shut.contains(&Node::new(nr, nc, e.floor()))
                    })
                    .collect();
                if kept.is_empty() {
                    tile.moves[usize::from(*l)] &= !MOVE_MORE;
                }
                *list = kept.into();
            }
            edges.retain(|(_, list)| !list.is_empty());
            tile.edges = edges.into();
        }
        self.label();
        // The gates of the tiles around are another's.
        let around: std::collections::BTreeSet<usize> = tiles
            .iter()
            .flat_map(|&t| {
                let (r, c) = self.origin(t);
                DIRECTIONS
                    .into_iter()
                    .chain([(0, 0)])
                    .map(move |(dr, dc)| (r + dr * 64, c + dc * 64))
                    .collect::<Vec<_>>()
            })
            .filter_map(|(r, c)| self.tile(r, c).map(|(t, _)| t))
            .collect();
        for t in around {
            self.graphs[t] = std::sync::OnceLock::new();
            self.local_areas[t] = std::sync::OnceLock::new();
            self.ends[t].get_mut().unpoisoned().clear();
        }
    }

    /// Index of a node in `Scratch::best` for a search over the box of first row and column
    /// `r0`, `c0`, `cols` columns and `cells` cells: its cell's for a first floor, else after
    /// the cells.
    #[inline]
    fn best_index(&self, n: Node, (r0, c0, cols, cells): (i32, i32, i32, usize)) -> Option<usize> {
        if n.f() == 0 {
            return Some(((n.r() - r0) * cols + (n.c() - c0)) as usize);
        }
        let (t, l) = self.locate(n)?;
        Some(cells + self.extra_base[t] as usize + l - 4096)
    }
}

/// Shard of a pair of floors in the sharded maps.
fn shard((_, a, b): Key) -> usize {
    ((a.0 ^ b.0.rotate_left(13)).wrapping_mul(0x9e37_79b9) >> 20) as usize % SHARDS
}

fn turn_shard((_, r1, c1, r2, c2, ..): TurnKey) -> usize {
    (r1 ^ r2 ^ c1.wrapping_mul(31) ^ c2.wrapping_mul(17)).unsigned_abs() as usize % SHARDS
}

/// Walking memory: the paths already searched (walking distance between two floors, infinite
/// when not connected), shared by every route built on the same terrain data. The app keeps one
/// between generations, filled with the memories saved with the guides.
pub struct Walks {
    /// Fingerprint of the terrain data the paths were searched on (None: not known yet).
    terrain: Mutex<Option<u64>>,
    shards: Vec<Mutex<HashMap<Key, Walk>>>,
    /// Memories added before the terrain was known, merged once it is.
    waiting: Mutex<Vec<WalkMemory>>,
}

/// A path of the walking memory: its distance, or the search one thread is making, which the
/// others needing it wait for (the routes built side by side ask the same walks at the same
/// time).
enum Walk {
    Known(f32),
    Searching(Arc<std::sync::OnceLock<f32>>),
}

impl Default for Walks {
    fn default() -> Self {
        Self {
            terrain: Mutex::new(None),
            shards: (0..SHARDS).map(|_| Mutex::new(HashMap::new())).collect(),
            waiting: Mutex::new(Vec::new()),
        }
    }
}

impl Walks {
    /// Remember the paths of a saved memory (dropped when searched on other terrain data).
    pub fn add(&self, memory: WalkMemory) {
        let terrain = *self.terrain.locked();
        match terrain {
            Some(t) if t == memory.terrain => self.insert_all(&memory),
            Some(_) => {}
            None => self.waiting.locked().push(memory),
        }
    }

    /// Paths remembered.
    pub fn len(&self) -> usize {
        self.shards.iter().map(|s| s.locked().len()).sum()
    }

    pub fn is_empty(&self) -> bool {
        self.len() == 0
    }

    /// Ties the memory to the terrain data of fingerprint `terrain`: the paths of other data are
    /// forgotten, the memories waiting for this one merged.
    fn bind(&self, terrain: u64) {
        let mut bound = self.terrain.locked();
        if *bound == Some(terrain) {
            return;
        }
        if bound.is_some() {
            for s in &self.shards {
                s.locked().clear();
            }
        }
        *bound = Some(terrain);
        for memory in std::mem::take(&mut *self.waiting.locked()) {
            if memory.terrain == terrain {
                self.insert_all(&memory);
            }
        }
    }

    fn insert_all(&self, memory: &WalkMemory) {
        for &(key, d) in &memory.paths {
            self.shards[shard(key)].locked().insert(key, Walk::Known(d));
        }
    }

    fn get(&self, key: Key) -> Option<f32> {
        match self.shards[shard(key)].locked().get(&key)? {
            Walk::Known(d) => Some(*d),
            Walk::Searching(_) => None,
        }
    }

    /// The distance of a path, from `search` when not known: by one thread, the others needing
    /// it meanwhile wait for its answer.
    fn get_or_search(&self, key: Key, search: impl FnOnce() -> f32) -> f32 {
        let shard = &self.shards[shard(key)];
        let cell = {
            let mut map = shard.locked();
            match map.get(&key) {
                Some(Walk::Known(d)) => return *d,
                Some(Walk::Searching(cell)) => Arc::clone(cell),
                None => {
                    let cell = Arc::new(std::sync::OnceLock::new());
                    map.insert(key, Walk::Searching(Arc::clone(&cell)));
                    cell
                }
            }
        };
        let d = *cell.get_or_init(search);
        shard.locked().insert(key, Walk::Known(d));
        d
    }
}

/// The paths a guide needed, saved with it (and in the files it is shared as) so that computing
/// it again, or other guides, do not search them again. In JSON: a string (sorted, zstd, base64).
#[derive(Debug, Clone, Default, PartialEq)]
pub struct WalkMemory {
    /// Fingerprint of the terrain data the paths were searched on.
    terrain: u64,
    paths: Vec<(Key, f32)>,
}

impl WalkMemory {
    pub fn len(&self) -> usize {
        self.paths.len()
    }

    pub fn is_empty(&self) -> bool {
        self.paths.is_empty()
    }

    /// Format 4: terrain fingerprint, count, then each path (continent, floors, distance),
    /// little endian, compressed. Formats 1 to 3 (cells of the heightmap terrain; walks with the
    /// character's level) are not read: their walks are searched again.
    fn encode(&self) -> String {
        let mut bytes = Vec::with_capacity(12 + self.paths.len() * 16);
        bytes.extend(self.terrain.to_le_bytes());
        bytes.extend((self.paths.len() as u32).to_le_bytes());
        for &((map, a, b), d) in &self.paths {
            bytes.extend((map as i32).to_le_bytes());
            bytes.extend(a.0.to_le_bytes());
            bytes.extend(b.0.to_le_bytes());
            bytes.extend(d.to_le_bytes());
        }
        let packed = zstd::encode_all(bytes.as_slice(), 9).unwrap_or_default();
        format!("4:{}", BASE64.encode(packed))
    }

    fn decode(text: &str) -> Option<Self> {
        let bytes = zstd::decode_all(BASE64.decode(text.strip_prefix("4:")?).ok()?.as_slice()).ok()?;
        let terrain = u64::from_le_bytes(bytes.get(..8)?.try_into().ok()?);
        let count = u32::from_le_bytes(bytes.get(8..12)?.try_into().ok()?) as usize;
        let word = |at: usize| Some(u32::from_le_bytes(bytes.get(at..at + 4)?.try_into().ok()?));
        let paths = (0..count)
            .map(|i| {
                let at = 12 + i * 16;
                let key = (i64::from(word(at)? as i32), Node(word(at + 4)?), Node(word(at + 8)?));
                Some((key, f32::from_le_bytes(bytes.get(at + 12..at + 16)?.try_into().ok()?)))
            })
            .collect::<Option<Vec<_>>>()?;
        Some(Self { terrain, paths })
    }
}

impl Serialize for WalkMemory {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        serializer.serialize_str(&self.encode())
    }
}

/// A memory that cannot be read (another format) is empty: it only saves time.
impl<'de> Deserialize<'de> for WalkMemory {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        Ok(Self::decode(&String::deserialize(deserializer)?).unwrap_or_default())
    }
}

#[derive(Default)]
pub struct Terrain {
    /// Tells this terrain's answers from another's in the thread caches.
    id: u32,
    /// Hash of the terrain data: tells which data a saved memory belongs to.
    fingerprint: u64,
    grids: FxHashMap<i64, Grid>,
    walks: Arc<Walks>,
    /// Paths this terrain asked its memory (or searched), for the memory of its guide.
    used: Vec<Mutex<HashSet<Key>>>,
    /// Large areas left only by boat, portal, lift or the passages joining them (continent,
    /// area), and the place each belongs to: `Terrain::enclose`.
    enclosed: HashMap<(i64, u32), u32>,
    /// The turns of the walks followed (`waypoints`), shared by the threads.
    turns: Turns,
}

/// A tile's floors as read: its x and y and its layers.
type TileRow = (i32, i32, TileLayers);

/// The nodes of a tile from its layers, their moves left for `Grid::link` (they need the tiles
/// around).
fn tile_nodes(l: &TileLayers) -> Arc<TileNodes> {
    let mut more = Vec::with_capacity(4097);
    let mut more_cell = Vec::new();
    let mut k = 0u16;
    for (i, &n) in l.counts.iter().enumerate() {
        more.push(k);
        for _ in 1..n {
            more_cell.push(i as u16);
            k += 1;
        }
    }
    more.push(k);
    let n = 4096 + usize::from(k);
    let mut nodes = TileNodes {
        more: more.into(),
        count: l.counts.clone().into(),
        more_cell: more_cell.into(),
        height: vec![0; n].into(),
        water: vec![0; n].into(),
        kind: vec![0; n].into(),
        flags: vec![NO_GO; n].into(),
        moves: vec![MOVE_NO_GO; n].into(),
        edges: Box::new([]),
    };
    let o = l.offsets();
    for (i, (&count, &first)) in l.counts.iter().zip(o.iter()).enumerate() {
        for f in 0..usize::from(count) {
            let floor = &l.floors[first as usize + f];
            let at = nodes.local(i, f).unwrap_or(i);
            nodes.height[at] = floor.height;
            nodes.water[at] = floor.water;
            nodes.kind[at] = floor.flags;
            let mut flags = 0;
            if floor.water & (HAZARD | DEEP) != 0 {
                flags |= NO_GO;
            }
            if floor.water & WET != 0 && floor.water >> 3 >= SWIM_DEPTH {
                flags |= SWIM;
            }
            nodes.flags[at] = flags;
        }
    }
    Arc::new(nodes)
}

impl Terrain {
    /// The terrain of the database's floors, taking drops up to the tier `drop_tier` of
    /// `layers::DROP_HEIGHTS` (0: only the safe fall).
    /// A terrain without any floor: every walk is the straight line (times the detour).
    pub fn empty() -> Self {
        let terrain = Self {
            id: NEXT_ID.fetch_add(1, Ordering::Relaxed),
            fingerprint: 0,
            grids: FxHashMap::default(),
            walks: Arc::new(Walks::default()),
            used: (0..SHARDS).map(|_| Mutex::new(HashSet::new())).collect(),
            enclosed: HashMap::new(),
            turns: (0..SHARDS).map(|_| Mutex::default()).collect(),
        };
        terrain.walks.bind(terrain.fingerprint);
        terrain
    }

    /// The terrain of a database: built from its tiles the first time (about a fifth of a second
    /// of work on every core), then a copy of what was built.
    pub fn load(conn: &Connection, drop_tier: usize) -> anyhow::Result<Self> {
        let drop_tier = drop_tier.min(layers::FARTHER);
        let built = crate::memo::rows(conn, "terrain", crate::memo::setting(drop_tier), || {
            Ok(Self::read(conn, drop_tier)?)
        })?;
        Ok(built.fork())
    }

    /// A copy to work on with its own memory of walks, to close and join areas of: the floors and
    /// the stored graphs of the gates, none of what is found while walking.
    fn fork(&self) -> Self {
        let terrain = Self {
            id: NEXT_ID.fetch_add(1, Ordering::Relaxed),
            fingerprint: self.fingerprint,
            grids: self.grids.iter().map(|(&map, grid)| (map, grid.fork())).collect(),
            walks: Arc::new(Walks::default()),
            used: (0..SHARDS).map(|_| Mutex::new(HashSet::new())).collect(),
            enclosed: self.enclosed.clone(),
            turns: (0..SHARDS).map(|_| Mutex::default()).collect(),
        };
        terrain.walks.bind(terrain.fingerprint);
        terrain
    }

    fn read(conn: &Connection, drop_tier: usize) -> rusqlite::Result<Self> {
        let mut terrain = Self::empty();
        let exists: bool = conn.query_row(
            "SELECT count(*) > 0 FROM sqlite_schema WHERE name = 'client_terrain_layers'",
            [],
            |r| r.get(0),
        )?;
        if !exists {
            terrain.walks.bind(terrain.fingerprint);
            return Ok(terrain);
        }
        let mut rows: Vec<(i64, i32, i32, i64, Vec<u8>)> = Vec::new();
        let mut stmt = conn.prepare("SELECT map_id, tile_x, tile_y, version, data FROM client_terrain_layers")?;
        for row in stmt.query_map([], |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?, r.get(3)?, r.get(4)?)))? {
            let (map, x, y, version, data): (i64, i32, i32, i64, Vec<u8>) = row?;
            // Whatever order the rows come in: a sum of the tiles' hashes.
            let mut h = Packed::default();
            h.write_u64(map as u64);
            h.write_u32(x as u32);
            h.write_u32(y as u32);
            h.write_u64(version as u64);
            h.write(&data);
            terrain.fingerprint = terrain.fingerprint.wrapping_add(h.finish());
            rows.push((map, x, y, version, data));
        }
        // Unpacked side by side.
        let workers = std::thread::available_parallelism()
            .map_or(4, std::num::NonZero::get)
            .min(8);
        let stored = rows.len();
        let unpacked: Vec<(i64, TileRow)> = std::thread::scope(|scope| {
            let parts: Vec<_> = rows
                .chunks(rows.len().div_ceil(workers).max(1))
                .map(|part| {
                    scope.spawn(move || {
                        let _release = crate::threads::Release;
                        part.iter()
                            .filter_map(|(map, x, y, version, data)| {
                                let l = TileLayers::unpack(data, *version).ok()?;
                                (l.counts.len() == 4096).then_some((*map, (*x, *y, l)))
                            })
                            .collect::<Vec<_>>()
                    })
                })
                .collect();
            parts.into_iter().flat_map(|p| p.join().unwrap_or_default()).collect()
        });
        drop(rows);
        if unpacked.len() < stored {
            // Without them the walks cross what they should not: never silently.
            eprintln!(
                "terrain: {} of {stored} tiles of floors unreadable (left out)",
                stored - unpacked.len()
            );
        }
        let mut tiles: HashMap<i64, Vec<TileRow>> = HashMap::new();
        for (map, tile) in unpacked {
            tiles.entry(map).or_default().push(tile);
        }
        for (map, list) in tiles {
            let tx0 = list.iter().map(|t| t.0).min().unwrap_or(0);
            let tx1 = list.iter().map(|t| t.0).max().unwrap_or(0);
            let ty0 = list.iter().map(|t| t.1).min().unwrap_or(0);
            let ty1 = list.iter().map(|t| t.1).max().unwrap_or(0);
            let (rows, cols) = ((ty1 - ty0 + 1) * 64, (tx1 - tx0 + 1) * 64);
            let tile_cols = cols as usize / 64;
            let count = rows as usize / 64 * tile_cols;
            let mut layers: Vec<Option<TileLayers>> = (0..count).map(|_| None).collect();
            let mut nodes: Vec<Option<Arc<TileNodes>>> = (0..count).map(|_| None).collect();
            for (x, y, l) in list {
                let t = (y - ty0) as usize * tile_cols + (x - tx0) as usize;
                nodes[t] = Some(tile_nodes(&l));
                layers[t] = Some(l);
            }
            let mut extra_base = Vec::with_capacity(count);
            let mut extras = 0u32;
            for n in &nodes {
                extra_base.push(extras);
                extras += n.as_ref().map_or(0, |n| (n.len() - 4096) as u32);
            }
            let mut grid = Grid {
                r0: ty0 * 64,
                c0: tx0 * 64,
                rows,
                cols,
                tile_cols,
                tiles: nodes,
                extra_base,
                extras,
                areas: Vec::new(),
                sizes: Vec::new(),
                graphs: (0..count).map(|_| std::sync::OnceLock::new()).collect(),
                ends: (0..count).map(|_| Mutex::default()).collect(),
                local_areas: (0..count).map(|_| std::sync::OnceLock::new()).collect(),
                caves: Vec::new(),
                drop_tier,
            };
            grid.link(&layers);
            drop(layers);
            grid.label();
            terrain.grids.insert(map, grid);
        }
        // The gates' graphs the databases hold (made when they were built): none to make. Each
        // tier of drops has its own (databases from before them have the first tier's only).
        let stored: bool = conn.query_row(
            "SELECT count(*) > 0 FROM sqlite_schema WHERE name = 'client_terrain_gates'",
            [],
            |r| r.get(0),
        )?;
        let tiered: bool = stored
            && conn.query_row(
                "SELECT count(*) > 0 FROM pragma_table_info('client_terrain_gates') WHERE name = 'tier'",
                [],
                |r| r.get(0),
            )?;
        if stored && (tiered || drop_tier == 0) {
            let mut stmt = conn.prepare(&format!(
                "SELECT map_id, tile_x, tile_y, graph FROM client_terrain_gates{}",
                if tiered { " WHERE tier = ?1" } else { "" }
            ))?;
            let row = |r: &rusqlite::Row<'_>| {
                Ok((
                    r.get::<_, i64>(0)?,
                    r.get::<_, i32>(1)?,
                    r.get::<_, i32>(2)?,
                    r.get::<_, Vec<u8>>(3)?,
                ))
            };
            let rows: Vec<_> = if tiered {
                stmt.query_map([drop_tier as i64], row)?
                    .collect::<rusqlite::Result<_>>()?
            } else {
                stmt.query_map([], row)?.collect::<rusqlite::Result<_>>()?
            };
            for (map, x, y, blob) in rows {
                if let Some(grid) = terrain.grids.get(&map)
                    && let Some((t, _)) = grid.tile(y * 64, x * 64)
                {
                    grid.set_graph(t, &blob);
                }
            }
        }
        // The walks depend on how the steps are made too: memories of another making are not
        // this terrain's.
        terrain.fingerprint = terrain
            .fingerprint
            .wrapping_add(u64::from(GATES_VERSION).wrapping_mul(0x9e37_79b9_7f4a_7c15));
        // Memories of walks with other drops are not this terrain's (the first tier's are as
        // they were before the others).
        if drop_tier > 0 {
            terrain.fingerprint = terrain
                .fingerprint
                .wrapping_add((drop_tier as u64).wrapping_mul(0xc2b2_ae3d_27d4_eb4f));
        }
        terrain.walks.bind(terrain.fingerprint);
        Ok(terrain)
    }

    /// The graphs of the gates of every tile with floors (map, tile x and y, stored graph): for
    /// the databases to hold them, made once (see `gates`).
    pub fn gate_graphs(&self) -> Vec<(i64, i32, i32, Vec<u8>)> {
        let mut out = Vec::new();
        for (&map, grid) in &self.grids {
            for (t, blob) in grid.all_graphs() {
                let (r, c) = grid.origin(t);
                out.push((map, c / 64, r / 64, blob));
            }
        }
        out
    }

    /// Use `walks` as memory (the app's, kept between generations): paths already searched on
    /// this terrain data are not searched again, and the new ones stay for the next routes.
    pub fn share(&mut self, walks: Arc<Walks>) {
        walks.bind(self.fingerprint);
        self.walks = walks;
    }

    /// The paths this terrain needed so far: to save with the guide.
    pub fn memory(&self) -> WalkMemory {
        let mut paths: Vec<(Key, f32)> = self
            .used
            .iter()
            .flat_map(|s| s.locked().iter().copied().collect::<Vec<_>>())
            .filter_map(|key| Some((key, self.walks.get(key)?)))
            .collect();
        paths.sort_by_key(|p| p.0);
        WalkMemory {
            terrain: self.fingerprint,
            paths,
        }
    }

    /// Tells this terrain from another (a new one each time one is made).
    pub(crate) fn id(&self) -> u32 {
        self.id
    }

    pub fn is_empty(&self) -> bool {
        self.grids.is_empty()
    }

    /// Cell (row from the north, column from the west) of a world position.
    fn cell(p: &Pos) -> (i32, i32) {
        // `f64::floor` is a call into the C library here: truncation, one less when negative.
        let floor = |v: f64| {
            let t = v as i32;
            if f64::from(t) > v { t - 1 } else { t }
        };
        (floor((ORIGIN - p.x) / CELL), floor((ORIGIN - p.y) / CELL))
    }

    /// Where a place stands for the walks.
    fn spot(p: &Pos) -> Spot {
        (Self::cell(p), p.z)
    }

    /// Whether the whole segment stays on known outdoor terrain without swimming.
    pub(crate) fn exposed_ground_line(&self, a: &Pos, b: &Pos) -> bool {
        let Some(grid) = self.grids.get(&a.continent) else {
            return false;
        };
        if a.continent != b.continent {
            return false;
        }
        let (Some(from), Some(to)) = (
            self.snapped(grid, a.continent, Self::spot(a)),
            self.snapped(grid, b.continent, Self::spot(b)),
        ) else {
            return false;
        };
        if from.cell() != Self::cell(a) || to.cell() != Self::cell(b) {
            return false;
        }
        Self::line_with(grid, from, to, |n| {
            let Some((tile, local)) = grid.locate(n) else {
                return false;
            };
            let Some(nodes) = grid.nodes(tile) else { return false };
            nodes.flags[local] & (NO_GO | SWIM) == 0
                && nodes.kind[local] & (SURFACE | layers::UNDER | layers::MODEL) == SURFACE
                && n.f() + 1 == grid.floors(n.r(), n.c())
        })
        .is_some()
    }

    /// `Grid::snap_at` of a place of continent `map`, remembered in the thread.
    fn snapped(&self, grid: &Grid, map: i64, ((r, c), z): Spot) -> Option<Node> {
        // Most places: a single floor, in a large area (whatever their height: the floors keep
        // none for a cell of one).
        if grid.floors(r, c) == 1 {
            let n = Node::new(r, c, 0);
            if !grid.blocked(n) && grid.large(n) {
                return Some(n);
            }
        }
        let key = (self.id, map, r, c, z);
        if let Some(known) = SNAPS.with(|m| m.borrow().get(&key).copied()) {
            return known;
        }
        let node = grid.snap_at((r, c), z);
        SNAPS.with(|m| {
            let mut m = m.borrow_mut();
            if m.len() >= SNAPS_MAX {
                m.clear();
            }
            m.insert(key, node);
        });
        node
    }

    /// The straight line between two floors when each of its steps can be taken, from floor to
    /// floor of the cells it crosses (`b` in its cell, else the nearest in height, walked rather than dropped to):
    /// -1 when all are walked, else what its swum steps make it cost (yards). None when it
    /// cannot be followed.
    fn line(grid: &Grid, a: Node, b: Node) -> Option<f64> {
        Self::line_with(grid, a, b, |_| true)
    }

    fn line_with(grid: &Grid, a: Node, b: Node, allowed: impl Fn(Node) -> bool) -> Option<f64> {
        let (ca, cb) = (a.cell(), b.cell());
        let (dr, dc) = (cb.0 - ca.0, cb.1 - ca.1);
        let n = dr.abs().max(dc.abs()) * 2;
        if grid.blocked(a) || !allowed(a) {
            return None;
        }
        let height = |x: Node| {
            grid.locate(x)
                .and_then(|(t, l)| Some(i32::from(grid.nodes(t)?.height[l])))
                .unwrap_or(0)
        };
        let (mut cost, mut swum, mut at, mut previous) = (0u32, false, a, ca);
        for k in 1..=n {
            let t = f64::from(k) / f64::from(n);
            let r = (f64::from(ca.0) + t * f64::from(dr)).round() as i32;
            let c = (f64::from(ca.1) + t * f64::from(dc)).round() as i32;
            if (r, c) == previous {
                continue;
            }
            let from = std::mem::replace(&mut previous, (r, c));
            let diagonal = usize::from(from.0 != r && from.1 != c);
            // Mostly a single floor there: the step to it, when taken.
            let (m, step) = if grid.floors(r, c) == 1 {
                let m = Node::new(r, c, 0);
                (m, grid.step(at, m)?)
            } else {
                let h = height(at);
                let mut next: Option<(Node, u32, (bool, bool, i32))> = None;
                grid.steps(at, |m, step, drop| {
                    if m.cell() != (r, c) {
                        return;
                    }
                    let rank = (m != b, drop, (height(m) - h).abs());
                    if next.is_none_or(|x| rank < x.2) {
                        next = Some((m, step, rank));
                    }
                });
                let (m, step, _) = next?;
                (m, step)
            };
            if !allowed(m) {
                return None;
            }
            swum |= step != WALKED[diagonal];
            cost += step;
            at = m;
        }
        (at == b).then_some(if swum { f64::from(cost) / 10.0 * CELL } else { -1.0 })
    }

    /// The key of a walk between two places in the thread memos.
    fn memo_key(&self, map: i64, a: Spot, b: Spot) -> MemoKey {
        let pack = |((r, c), z): Spot| {
            let z = z.map_or(0, |z| u64::from(z as u16) | 1 << 16);
            (u128::from(z) << 32) | (u128::from(r as u16) << 16) | u128::from(c as u16)
        };
        (self.id, map, (pack(a) << 64) | pack(b))
    }

    /// Walking distance (yards) between two points of the same continent (see the module): the
    /// straight line when nothing is in the way, infinite when no walk goes between them (`separated`), None when
    /// it does not know the way.
    pub fn distance(&self, a: &Pos, b: &Pos) -> Option<f64> {
        let straight = a.dist(b);
        let Some(grid) = self.grids.get(&a.continent) else {
            return Some(straight);
        };
        if a.continent != b.continent || !straight.is_finite() {
            return Some(straight);
        }
        let (sa, sb) = (Self::spot(a), Self::spot(b));
        let key = self.memo_key(a.continent, sa, sb);
        let known = MEMO.with(|m| m.borrow().get(&key).copied());
        let walk = known.unwrap_or_else(|| {
            let w = self
                .walk_cells(grid, a.continent, sa, sb)
                .map_or(f32::NAN, |d| d as f32);
            MEMO.with(|m| {
                let mut m = m.borrow_mut();
                // The walks of older terrains stay too (their id is in the key): start over.
                if m.len() >= MEMO_MAX {
                    m.clear();
                }
                m.insert(key, w);
            });
            w
        });
        // From or to a cave: through its mouth (two places of the same cave: straight).
        let (mouth_a, mouth_b) = (Self::mouth_of(grid, a), Self::mouth_of(grid, b));
        if mouth_a.is_some() || mouth_b.is_some() {
            let (ea, eb) = (mouth_a.unwrap_or(*a), mouth_b.unwrap_or(*b));
            if mouth_a.is_some() && ea.dist(&eb) < 1e-6 {
                return Some(straight);
            }
            let (between, legs) = (ea.dist(&eb), a.dist(&ea) + eb.dist(b));
            return if walk.is_nan() {
                None
            } else if walk < 0.0 {
                Some(between + legs)
            } else {
                Some(f64::from(walk).max(between) + legs)
            };
        }
        if walk.is_nan() {
            None
        } else if walk < 0.0 {
            Some(straight)
        } else {
            Some(f64::from(walk).max(straight))
        }
    }

    /// The mouth of the cave `p` stands in (`add_cave`), None outside the caves.
    fn mouth_of(grid: &Grid, p: &Pos) -> Option<Pos> {
        grid.cave_at(Self::cell(p)).map(|k| k.mouth_pos)
    }

    /// A cave the floors miss, opening at `mouth`: the places on no large walkable area within
    /// `reach` yards of `inside` are walked to through it. False when the mouth is in no large
    /// area.
    pub(crate) fn add_cave(&mut self, mouth: &Pos, inside: &Pos, reach: f64) -> bool {
        let Some(grid) = self.grids.get_mut(&mouth.continent) else {
            return false;
        };
        let Some(node) = grid.snap_at(Self::cell(mouth), mouth.z) else {
            return false;
        };
        if !grid.large(node) || inside.continent != mouth.continent {
            return false;
        }
        grid.caves.push(Cave {
            inside: Self::cell(inside),
            reach: (reach / CELL).round() as i32,
            mouth: node,
            mouth_pos: *mouth,
        });
        // Answers given before are kept by terrain: this one is another.
        self.id = NEXT_ID.fetch_add(1, Ordering::Relaxed);
        self.turns.iter_mut().for_each(|t| t.get_mut().unpoisoned().clear());
        true
    }

    /// The walkable area a point stands in (its floor, `Grid::snap`) and its size in floors:
    /// None without terrain for its continent, area 0 on no floor stood on.
    pub fn area_of(&self, p: &Pos) -> Option<(u32, u32)> {
        let grid = self.grids.get(&p.continent)?;
        let area = self
            .snapped(grid, p.continent, Self::spot(p))
            .map_or(0, |n| grid.area(n));
        Some((area, grid.sizes.get(area as usize).copied().unwrap_or(0)))
    }

    /// The heights of the floors of large areas in the cell of `p` (yards, lowest first; none
    /// for a cell of a single floor, whose height the floors do not keep), and the height of
    /// the one it stands on (`Grid::snap_at`, when kept): None without terrain for its
    /// continent.
    pub fn floors_at(&self, p: &Pos) -> Option<(Vec<i16>, Option<i16>)> {
        let grid = self.grids.get(&p.continent)?;
        let (r, c) = Self::cell(p);
        let mut heights: Vec<i16> = grid.large_floors(r, c).filter_map(|n| grid.known_height(n)).collect();
        heights.sort_unstable();
        let on = self
            .snapped(grid, p.continent, Self::spot(p))
            .and_then(|n| grid.known_height(n));
        Some((heights, on))
    }

    /// The large walkable area a point stands in, twice (once the areas the gaps of the
    /// heightmap terrain joined, now the floors' own): None without terrain or in no large
    /// area.
    pub(crate) fn large_areas_of(&self, p: &Pos) -> Option<(u32, u32)> {
        self.area_of(p)
            .filter(|&(a, size)| a != 0 && size >= LARGE_AREA)
            .map(|(a, _)| (a, a))
    }

    /// Forget every walk worked out, in this thread and shared (benchmarks: each run from cold
    /// memories).
    #[doc(hidden)]
    pub fn forget_walks(&self) {
        MEMO.with(|m| m.borrow_mut().clear());
        APART.with(|m| m.borrow_mut().clear());
        SNAPS.with(|m| m.borrow_mut().clear());
        self.walks.shards.iter().for_each(|s| s.locked().clear());
        self.used.iter().for_each(|u| u.locked().clear());
        self.turns.iter().for_each(|t| t.locked().clear());
    }

    /// Took a valley out of the mainland the gaps of the heightmap terrain joined it to; the
    /// floors open no gaps: nothing to take out (false).
    pub fn detach(&mut self, _p: &Pos) -> bool {
        false
    }

    /// A way the floors hold that the game keeps closed (a locked gate, `[[closed]]` of
    /// `overrides/passes.toml`): the floors within `radius` yards of `p` are never stood on.
    /// Before `enclose` (the areas are labelled again). Whether there were floors there.
    pub(crate) fn close(&mut self, p: &Pos, radius: f64) -> bool {
        let Some(grid) = self.grids.get_mut(&p.continent) else {
            return false;
        };
        let (r, c) = Self::cell(p);
        let k = (radius / CELL).ceil() as i32;
        let mut closed: Vec<Node> = Vec::new();
        for dr in -k..=k {
            for dc in -k..=k {
                if f64::from(dr * dr + dc * dc).sqrt() * CELL > radius + CELL / 2.0 {
                    continue;
                }
                closed.extend((0..grid.floors(r + dr, c + dc)).map(|f| Node::new(r + dr, c + dc, f)));
            }
        }
        if closed.is_empty() {
            return false;
        }
        grid.close(&closed);
        let mut h = Packed::default();
        for n in &closed {
            h.write_u32(n.0);
        }
        self.fingerprint = self.fingerprint.wrapping_add(h.finish());
        self.walks.bind(self.fingerprint);
        self.id = NEXT_ID.fetch_add(1, Ordering::Relaxed);
        self.turns.iter_mut().for_each(|t| t.get_mut().unpoisoned().clear());
        true
    }

    /// The large area at `p` is left only by boat, portal or lift (Rut'theran Village, at the
    /// foot of Teldrassil): no walk between it and the other areas. Its area and size, None when
    /// `p` is not in a large area.
    pub(crate) fn enclose(&mut self, p: &Pos, place: u32) -> Option<(u32, u32)> {
        let (area, size) = self.area_of(p).filter(|&(a, size)| a != 0 && size >= LARGE_AREA)?;
        // Swimming joins the shore to the mainland: enclosing it would enclose the continent.
        if self.main_area(p.continent) == Some(area) {
            return None;
        }
        self.enclosed.insert((p.continent, area), place);
        // Answers given before are kept by terrain: this one is another.
        self.id = NEXT_ID.fetch_add(1, Ordering::Relaxed);
        self.turns.iter_mut().for_each(|t| t.get_mut().unpoisoned().clear());
        Some((area, size))
    }

    /// The box of a continent's cells: first row and column, rows and columns (rows from the
    /// north, columns from the west).
    pub fn bounds(&self, continent: i64) -> Option<(i32, i32, i32, i32)> {
        let g = self.grids.get(&continent)?;
        Some((g.r0, g.c0, g.rows, g.cols))
    }

    /// A cell of a continent (its first floor), None out of the tiles with floors.
    pub fn view(&self, continent: i64, r: i32, c: i32) -> Option<CellView> {
        let grid = self.grids.get(&continent)?;
        let (t, i) = grid.tile(r, c)?;
        let tile = grid.nodes(t)?;
        let floors = tile.count[i];
        let water = if floors > 0 { tile.water[i] } else { 0 };
        let depth = if water & WET != 0 { water >> 3 } else { 0 };
        Some(CellView {
            height: if floors > 0 { tile.height[i] } else { 0 },
            steep: floors == 0,
            floors,
            depth,
            swum: depth >= SWIM_DEPTH,
            deep: water & DEEP != 0,
            sea_edge: false,
            hazard: water & HAZARD != 0,
        })
    }

    /// Continents the terrain covers.
    pub fn continents(&self) -> Vec<i64> {
        let mut c: Vec<i64> = self.grids.keys().copied().collect();
        c.sort_unstable();
        c
    }

    /// The areas declared enclosed (continent, area).
    pub(crate) fn enclosed_areas(&self) -> impl Iterator<Item = (i64, u32)> + '_ {
        self.enclosed.keys().copied()
    }

    /// The largest walkable area of a continent (its mainland).
    pub fn main_area(&self, continent: i64) -> Option<u32> {
        let grid = self.grids.get(&continent)?;
        (1..grid.sizes.len()).max_by_key(|&a| grid.sizes[a]).map(|a| a as u32)
    }

    /// Whether area `a` (of continent `map`) is left only by boat, portal or lift.
    pub(crate) fn is_enclosed(&self, map: i64, a: u32) -> bool {
        self.enclosed.contains_key(&(map, a))
    }

    /// Two areas the floors do not connect, large, one of them enclosed (not both of the same
    /// place): no walk between them, but through the passages joining them.
    fn apart(&self, grid: &Grid, map: i64, area: u32, other: u32) -> bool {
        let large = |a: u32| a != 0 && grid.sizes[a as usize] >= LARGE_AREA;
        let (pa, po) = (self.enclosed.get(&(map, area)), self.enclosed.get(&(map, other)));
        area != other && large(area) && large(other) && (pa.is_some() || po.is_some()) && pa != po
    }

    /// Whether `a` and `b` stand in two large areas the floors do not join (whatever is
    /// enclosed): a lift, a boat or a portal may.
    pub fn disconnected(&self, a: &Pos, b: &Pos) -> bool {
        if a.continent != b.continent {
            return false;
        }
        let Some(grid) = self.grids.get(&a.continent) else {
            return false;
        };
        let (sa, sb) = (Self::spot(a), Self::spot(b));
        let (id, map, cells) = self.memo_key(a.continent, sa, sb);
        if let Some(known) = APART.with(|m| m.borrow().get(&(id, map, cells)).copied()) {
            return known;
        }
        let area = |c| self.snapped(grid, map, c).map_or(0, |n| grid.area(n));
        let (area, other) = (area(sa), area(sb));
        let large = |a: u32| a != 0 && grid.sizes[a as usize] >= LARGE_AREA;
        let apart = area != other && large(area) && large(other);
        APART.with(|m| {
            let mut m = m.borrow_mut();
            if m.len() >= 1 << 20 {
                m.clear();
            }
            m.insert((id, map, cells), apart);
        });
        apart
    }

    /// The cell of a world position (row from the north, column from the west).
    pub fn cell_of(p: &Pos) -> (i32, i32) {
        Self::cell(p)
    }

    /// Whether no walk goes between `a` and `b`: two areas the floors do not connect, one of
    /// them enclosed (no path searched).
    pub fn separated(&self, a: &Pos, b: &Pos) -> bool {
        if self.enclosed.is_empty() || a.continent != b.continent {
            return false;
        }
        let Some(grid) = self.grids.get(&a.continent) else {
            return false;
        };
        let area = |p: &Pos| {
            self.snapped(grid, a.continent, Self::spot(p))
                .map_or(0, |n| grid.area(n))
        };
        self.apart(grid, a.continent, area(a), area(b))
    }

    /// Walk between two places (yards), negative for the straight line, infinite when no walk
    /// goes between them (`separated`), None when the floors do not know the way (two areas
    /// they do not connect, which a lift or a boat may; no path found).
    fn walk_cells(&self, grid: &Grid, map: i64, sa: Spot, sb: Spot) -> Option<f64> {
        let (a, b) = self.walk_ends(grid, map, sa, sb)?;
        if a == b {
            return Some(-1.0);
        }
        // A river or a lake on the way: swum straight across, as long as that can be done.
        if let Some(d) = Self::line(grid, a, b) {
            return Some(d);
        }
        let (area, other) = (grid.area(a), grid.area(b));
        if area == 0 || other == 0 {
            return None;
        }
        if area != other {
            return self.apart(grid, map, area, other).then_some(f64::INFINITY);
        }
        // The cheapest path is not the same both ways (drops): the key keeps the direction.
        let key = (map, a, b);
        if let Some(used) = self.used.get(shard(key)) {
            used.locked().insert(key);
        }
        let d = self.walks.get_or_search(key, || {
            Self::search_walk(grid, a, b, None).map_or(f32::INFINITY, |d| d as f32)
        });
        d.is_finite().then_some(f64::from(d))
    }

    /// The floors a walk between two places goes from and to (`Grid::snap_at`). None when an end
    /// stands on no floor.
    fn walk_ends(&self, grid: &Grid, map: i64, sa: Spot, sb: Spot) -> Option<(Node, Node)> {
        Some((self.snapped(grid, map, sa)?, self.snapped(grid, map, sb)?))
    }

    /// The walk between two floors of the same area that the straight line does not follow
    /// (yards), and its floors in `trace` (from `a` to `b`, given): far apart, through the
    /// tiles' gates; else floor by floor in the box around them, else through the gates.
    fn search_walk(grid: &Grid, a: Node, b: Node, mut trace: Option<&mut Vec<Node>>) -> Option<f64> {
        let far = (a.r() - b.r()).abs().max((a.c() - b.c()).abs()) > gates::FAR;
        if far {
            match grid.through_gates(a, b, trace.as_deref_mut()) {
                Some(d) => Some(d),
                None => Self::path(grid, a, b, MARGINS[MARGINS.len() - 1], trace),
            }
        } else {
            // Not in the box around: a long way round, through the gates rather than over a much
            // larger box.
            Self::path(grid, a, b, MARGINS[0], trace.as_deref_mut())
                .or_else(|| grid.through_gates(a, b, trace.as_deref_mut()))
                .or_else(|| Self::path(grid, a, b, MARGINS[1], trace))
        }
    }

    /// The points a walk between two points of the same continent turns at, in order, its ends
    /// left out: the cells where it turns (the tiles' gates it goes through for a long walk).
    /// Empty when it goes straight: nothing in the way (a river swum across), or the floors do
    /// not know the way. Searched once for all the threads (the walking memories keep distances
    /// only).
    pub fn waypoints(&self, a: &Pos, b: &Pos) -> Vec<Pos> {
        let Some(grid) = self.grids.get(&a.continent) else {
            return Vec::new();
        };
        if a.continent != b.continent {
            return Vec::new();
        }
        // From or to a cave: through its mouth.
        let (mouth_a, mouth_b) = (Self::mouth_of(grid, a), Self::mouth_of(grid, b));
        if mouth_a.is_some() || mouth_b.is_some() {
            let (ea, eb) = (mouth_a.unwrap_or(*a), mouth_b.unwrap_or(*b));
            if mouth_a.is_some() && ea.dist(&eb) < 1e-6 {
                return Vec::new();
            }
            let mut points: Vec<Pos> = mouth_a.into_iter().collect();
            points.extend(self.waypoints_between(grid, a, b));
            points.extend(mouth_b);
            return points;
        }
        self.waypoints_between(grid, a, b)
    }

    /// The floors a walk between two points goes over, each cell's centre and the floor's
    /// index, searched floor by floor (to draw it: the planner's walks only keep their turns).
    /// Empty when the floors do not know the way.
    #[doc(hidden)]
    pub fn cell_path(&self, a: &Pos, b: &Pos) -> Vec<(Pos, u8)> {
        let Some(grid) = self.grids.get(&a.continent) else {
            return Vec::new();
        };
        let Some((na, nb)) = self.walk_ends(grid, a.continent, Self::spot(a), Self::spot(b)) else {
            return Vec::new();
        };
        let mut nodes = Vec::new();
        let span = (na.r() - nb.r()).abs().max((na.c() - nb.c()).abs());
        if na != nb && Self::path(grid, na, nb, MARGINS[1].max(span), Some(&mut nodes)).is_none() {
            return Vec::new();
        }
        (nodes.iter())
            .map(|n| {
                let p = Pos {
                    x: ORIGIN - (f64::from(n.r()) + 0.5) * CELL,
                    y: ORIGIN - (f64::from(n.c()) + 0.5) * CELL,
                    z: grid.known_height(*n),
                    ..*a
                };
                (p, n.f() as u8)
            })
            .collect()
    }

    /// `waypoints` from a place to another (as snapped).
    fn waypoints_between(&self, grid: &Grid, a: &Pos, b: &Pos) -> Vec<Pos> {
        let (sa, sb) = (Self::spot(a), Self::spot(b));
        let (ca, cb) = (sa.0, sb.0);
        // Most walks go straight, or the floors do not know the way: the distance asked before
        // tells it.
        let distance = self.memo_key(a.continent, sa, sb);
        if MEMO
            .with(|m| m.borrow().get(&distance).copied())
            .is_some_and(|w| !(w >= 0.0 && w.is_finite()))
        {
            return Vec::new();
        }
        let key = (a.continent, ca.0, ca.1, cb.0, cb.1, a.z, b.z);
        let memo = self.turns.get(turn_shard(key));
        let known = memo.and_then(|m| m.locked().get(&key).cloned());
        let turns = known.unwrap_or_else(|| {
            let turns = self.turns(grid, a.continent, sa, sb);
            if let Some(memo) = memo {
                let mut memo = memo.locked();
                if memo.len() >= TURNS_MAX {
                    memo.clear();
                }
                memo.insert(key, Arc::clone(&turns));
            }
            turns
        });
        (turns.iter())
            .map(|&(r, c, z)| Pos {
                x: ORIGIN - (f64::from(r) + 0.5) * CELL,
                y: ORIGIN - (f64::from(c) + 0.5) * CELL,
                z,
                ..*a
            })
            .collect()
    }

    /// The cells a walk between two places turns at (`waypoints`), and the height of the floor
    /// walked there.
    fn turns(&self, grid: &Grid, map: i64, sa: Spot, sb: Spot) -> Arc<[(i32, i32, Option<i16>)]> {
        let Some((a, b)) = self.walk_ends(grid, map, sa, sb) else {
            return Arc::new([]);
        };
        if a == b || Self::line(grid, a, b).is_some() {
            return Arc::new([]);
        }
        let area = grid.area(a);
        if area == 0 || area != grid.area(b) {
            return Arc::new([]);
        }
        let mut nodes = Vec::new();
        if Self::search_walk(grid, a, b, Some(&mut nodes)).is_none() {
            return Arc::new([]);
        }
        let nodes = Self::between_gates(grid, &nodes);
        let mut cells: Vec<((i32, i32), Option<i16>)> =
            nodes.iter().map(|&n| (n.cell(), grid.known_height(n))).collect();
        cells.dedup_by_key(|c| c.0);
        // Where the direction changes (the walk's ends are the points themselves).
        (1..cells.len().saturating_sub(1))
            .filter(|&k| {
                let (p, q, r) = (cells[k - 1].0, cells[k].0, cells[k + 1].0);
                (q.0 - p.0, q.1 - p.1) != (r.0 - q.0, r.1 - q.1)
            })
            .map(|k| (cells[k].0.0, cells[k].0.1, cells[k].1))
            .collect()
    }

    /// The floors of a walk found through the tiles' gates (`a`, the gates, `b`) with the cells
    /// between two gates, searched floor by floor: the walk as it goes, not as lines from gate to
    /// gate. Two floors it cannot join stay as they are.
    fn between_gates(grid: &Grid, gates: &[Node]) -> Vec<Node> {
        let mut out: Vec<Node> = gates.first().copied().into_iter().collect();
        for w in gates.windows(2) {
            let (p, q) = (w[0], w[1]);
            let gap = (p.r() - q.r()).abs().max((p.c() - q.c()).abs());
            if gap > 1 {
                let mut sub = Vec::new();
                if MARGINS
                    .iter()
                    .any(|&m| Self::path(grid, p, q, m.max(gap), Some(&mut sub)).is_some())
                {
                    out.extend(sub.into_iter().skip(1));
                    continue;
                }
            }
            out.push(q);
        }
        out
    }

    /// Cheapest walk between two floors (yards), within their
    /// box grown by `margin` cells, and its floors in `trace` (from `a` to `b`).
    fn path(grid: &Grid, a: Node, b: Node, margin: i32, trace: Option<&mut Vec<Node>>) -> Option<f64> {
        let (r0, c0) = (a.r().min(b.r()) - margin, a.c().min(b.c()) - margin);
        let (rows, cols) = (a.r().max(b.r()) + margin - r0 + 1, a.c().max(b.c()) + margin - c0 + 1);
        let cells = (rows * cols) as usize;
        let space = (r0, c0, cols, cells);
        SCRATCH.with(|scratch| {
            let scratch = &mut *scratch.borrow_mut();
            let n = cells + grid.extras as usize;
            if scratch.best.len() < n {
                scratch.best.resize(n, (u32::MAX, 0));
            }
            scratch.current = scratch.current.wrapping_add(1);
            if scratch.current == 0 {
                scratch.best.fill((u32::MAX, 0));
                scratch.current = 1;
            }
            for bucket in &mut scratch.open {
                bucket.clear();
            }
            let found = Self::search(grid, a, b, (r0, c0, rows, cols), scratch)?;
            if let Some(trace) = trace {
                trace.clear();
                trace.push(b);
                // Back from `b`: each floor reached from a floor whose cost and step make its
                // own (the first found when several), costs going down to `a`.
                let current = scratch.current;
                let inside = |n: Node| n.r() >= r0 && n.c() >= c0 && n.r() < r0 + rows && n.c() < c0 + cols;
                let cost_of = |n: Node| {
                    if !inside(n) {
                        return None;
                    }
                    let k = grid.best_index(n, space)?;
                    (scratch.best[k].1 == current).then(|| scratch.best[k].0)
                };
                let mut q = b;
                while q != a {
                    let g = cost_of(q)?;
                    let mut from = None;
                    'cells: for (dr, dc) in DIRECTIONS.into_iter().chain([(0, 0)]) {
                        let (pr, pc) = (q.r() - dr, q.c() - dc);
                        for f in 0..grid.floors(pr, pc) {
                            let p = Node::new(pr, pc, f);
                            let (Some(before), Some(step)) = (cost_of(p), grid.step(p, q)) else {
                                continue;
                            };
                            if before + step == g {
                                from = Some(p);
                                break 'cells;
                            }
                        }
                    }
                    let from = from?;
                    trace.push(from);
                    q = from;
                }
                trace.reverse();
            }
            Some(found)
        })
    }

    /// The A* of `path` in `scratch`, over the box of first row and column `r0`, `c0`, `rows` and
    /// `cols`.
    fn search(
        grid: &Grid,
        a: Node,
        b: Node,
        (r0, c0, rows, cols): (i32, i32, i32, i32),
        scratch: &mut Scratch,
    ) -> Option<f64> {
        let space = (r0, c0, cols, (rows * cols) as usize);
        let current = scratch.current;
        let (best, open) = (&mut scratch.best, &mut scratch.open);
        let get = |best: &[(u32, u32)], k: usize| {
            let (cost, round) = best[k];
            if round == current { cost } else { u32::MAX }
        };
        // Costs in tenths of a cell: 10 straight, 14 diagonal, more swimming.
        let (br, bc) = b.cell();
        let h = |r: i32, c: i32| {
            let (dr, dc) = ((r - br).unsigned_abs(), (c - bc).unsigned_abs());
            10 * dr.max(dc) + 4 * dr.min(dc)
        };
        // Open floors by estimate, in a ring of buckets: the heuristic is consistent (no step
        // costs less than its length), so a step raises the estimate by 0 to its cost plus 14
        // (fewer than `BUCKETS`) and the first time the goal comes out is the cheapest.
        let (mut f, mut queued) = (h(a.r(), a.c()), 1usize);
        best[grid.best_index(a, space)?] = (0, current);
        open[f as usize % BUCKETS].push((0, a));
        while queued > 0 {
            let Some((g, n)) = open[f as usize % BUCKETS].pop() else {
                f += 1;
                continue;
            };
            queued -= 1;
            if n == b {
                return Some(f64::from(g) / 10.0 * CELL);
            }
            let Some((t, l)) = grid.locate(n) else { continue };
            let k = if l < 4096 {
                ((n.r() - r0) * cols + (n.c() - c0)) as usize
            } else {
                space.3 + grid.extra_base[t] as usize + l - 4096
            };
            if g > get(best, k) {
                continue;
            }
            let Some(tile) = grid.nodes(t) else { continue };
            let (r, c) = n.cell();
            let moves = tile.moves[l];
            // The steps to the first floors of the neighbours, inline (most of them).
            for (d, (dr, dc)) in DIRECTIONS.into_iter().enumerate() {
                if moves >> d & MOVE == 0 {
                    continue;
                }
                let (nr, nc) = (r + dr, c + dc);
                if nr < r0 || nc < c0 || nr >= r0 + rows || nc >= c0 + cols {
                    continue;
                }
                let ng = g + move_cost(moves, d);
                let k = ((nr - r0) * cols + (nc - c0)) as usize;
                if ng < get(best, k) {
                    best[k] = (ng, current);
                    open[(ng + h(nr, nc)) as usize % BUCKETS].push((ng, Node::new(nr, nc, 0)));
                    queued += 1;
                }
            }
            if moves & MOVE_MORE != 0 {
                for &e in tile.edges(l) {
                    let (nr, nc) = e.to(r, c);
                    if nr < r0 || nc < c0 || nr >= r0 + rows || nc >= c0 + cols {
                        continue;
                    }
                    let m = Node::new(nr, nc, e.floor());
                    let ng = g + e.cost();
                    let Some(k) = grid.best_index(m, space) else { continue };
                    if ng < get(best, k) {
                        best[k] = (ng, current);
                        open[(ng + h(nr, nc)) as usize % BUCKETS].push((ng, m));
                        queued += 1;
                    }
                }
            }
        }
        None
    }
}

#[cfg(test)]
mod tests;

#[cfg(test)]
#[derive(Clone, Copy)]
pub(crate) enum TestGround {
    Dry,
    Shallow,
    Mixed,
    Covered,
    Indoor,
    Passage,
    Unknown,
}

#[cfg(test)]
pub(crate) fn movement_test_ground(kind: TestGround) -> Terrain {
    tests::movement_ground(kind)
}
