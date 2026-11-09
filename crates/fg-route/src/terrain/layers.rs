//! The walkable floors of a tile as the databases store them (`client_terrain_layers`, made by
//! the database builder from the game's geometry: the terrain, the buildings, caves, bridges and
//! the models that collide).
//!
//! Each of the tile's 4096 cells (rows from the north, columns from the west) has 0 to
//! `MAX_FLOORS` floors: the walkable surfaces a character stands on in the cell, each joined
//! within it (the terrain's own first when it has one, then by height). Each floor knows the
//! floors of the 8 neighbour cells it walks to (a bit per floor of the neighbour, in the order
//! of `DIRECTIONS`) and those it drops down to, one way (`DROP_DIRECTIONS`: the 4 straight
//! neighbours and its own cell).
//!
//! Stored (`version` 2), zstd-compressed: a byte 2, then streams one after the other, each
//! floor in the order of its cell then of its floors:
//! - the number of floors of each cell (4096 bytes);
//! - the height of each cell's first floor, as its difference with what its west, north and
//!   north-west neighbours' first floors predict (`predict_height`), zigzag varint; then the
//!   height of each other floor as its difference with its cell's first, zigzag varint;
//! - the water of each floor (`WET`, `HAZARD`, `DEEP`, the depth in bits 3 to 7), its flags,
//!   its size (sixteenths of the cell, 1 to 16): a byte each, a stream each;
//! - its straight walks (north, south, west, east), each XOR what `predict_straight` gives;
//! - its diagonal walks, each XOR what the straight walks lead to (`derive_diagonals`);
//! - its drops: a byte of the non-empty slots, then those slots' bytes;
//! - from version 3, its farther drops (`DROP_HEIGHTS`) the same way, a stream of slots and a
//!   stream of bytes for each tier after the first.
//!
//! Most cells have one floor that walks to the one floor of each neighbour: their predictions
//! hold and the streams are mostly zeros. Version 1 (pass 1) stored the plain bytes.

use anyhow::{Result, ensure};

pub const MAX_FLOORS: usize = 8;
/// Bits of a floor's water byte; the depth (yards) in bits 3 to 7.
pub const WET: u8 = 1;
pub const HAZARD: u8 = 2;
pub const DEEP: u8 = 4;
/// Bits of a floor's flags: the terrain's own surface, under it (a cave, a tunnel), above it (a
/// bridge, an upper floor), mostly on a model, in a cell without terrain, in a cell that had
/// more floors than it keeps.
pub const SURFACE: u8 = 1;
pub const UNDER: u8 = 2;
pub const ABOVE: u8 = 4;
pub const MODEL: u8 = 8;
pub const NO_TERRAIN: u8 = 16;
pub const CROWDED: u8 = 32;
/// Directions of the walks (row, column): north, south, west, east, then the diagonals.
pub const DIRECTIONS: [(i32, i32); 8] = [(-1, 0), (1, 0), (0, -1), (0, 1), (-1, -1), (-1, 1), (1, -1), (1, 1)];
/// Drops: the 4 straight directions, then the same cell.
pub const DROPS: usize = 5;
pub const DROP_DIRECTIONS: [(i32, i32); DROPS] = [(-1, 0), (1, 0), (0, -1), (0, 1), (0, 0)];
/// The highest drop (yards) of each tier of drops: `Floor::drops` holds the first tier, the safe
/// fall (no damage); `Floor::farther` each next one (a fall that costs health: about 30 % at
/// 30 yards, two thirds at 50).
pub const DROP_HEIGHTS: [f64; 3] = [15.0, 30.0, 50.0];
/// The tiers of drops after the first.
pub const FARTHER: usize = DROP_HEIGHTS.len() - 1;
/// Index in `DIRECTIONS` of the direction the other way.
pub(crate) const OPPOSITE: [usize; 8] = [1, 0, 3, 2, 7, 6, 5, 4];
/// The two ways to each diagonal neighbour (north-west, north-east, south-west, south-east)
/// through a straight one: the direction to it, then from it (north or south first, then west
/// or east first).
const VIAS: [[(usize, usize); 2]; 4] = [[(0, 2), (2, 0)], [(0, 3), (3, 0)], [(1, 2), (2, 1)], [(1, 3), (3, 1)]];
/// The format `pack` writes.
pub const VERSION: i64 = 3;

/// A floor of a cell.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Floor {
    /// Height of the surface stood or swum on (yards).
    pub height: i16,
    pub water: u8,
    pub flags: u8,
    /// Sixteenths of the cell it covers (1 to 16).
    pub size: u8,
    pub walks: [u8; 8],
    pub drops: [u8; DROPS],
    /// The drops of the next tiers (`DROP_HEIGHTS`), as `drops`.
    pub farther: [[u8; DROPS]; FARTHER],
}

/// The floors of a tile's cells: `counts` for each of the 4096, then `floors` in that order.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct TileLayers {
    pub counts: Vec<u8>,
    pub floors: Vec<Floor>,
}

fn zigzag(v: i32) -> u32 {
    ((v << 1) ^ (v >> 31)) as u32
}

fn unzigzag(v: u32) -> i32 {
    (v >> 1) as i32 ^ -((v & 1) as i32)
}

fn put_varint(out: &mut Vec<u8>, mut v: u32) {
    while v >= 0x80 {
        out.push((v as u8) | 0x80);
        v >>= 7;
    }
    out.push(v as u8);
}

/// Reads bytes in order, failing past the end.
struct Reader<'a> {
    bytes: &'a [u8],
    at: usize,
}

impl Reader<'_> {
    fn byte(&mut self) -> Result<u8> {
        let b = *self
            .bytes
            .get(self.at)
            .ok_or_else(|| anyhow::anyhow!("layers: truncated"))?;
        self.at += 1;
        Ok(b)
    }

    fn take(&mut self, n: usize) -> Result<&[u8]> {
        ensure!(self.at + n <= self.bytes.len(), "layers: truncated");
        let s = &self.bytes[self.at..self.at + n];
        self.at += n;
        Ok(s)
    }

    fn varint(&mut self) -> Result<u32> {
        let (mut v, mut shift) = (0u32, 0);
        loop {
            let b = self.byte()?;
            v |= u32::from(b & 0x7f) << shift;
            if b < 0x80 {
                return Ok(v);
            }
            shift += 7;
            ensure!(shift < 35, "layers: bad varint");
        }
    }
}

/// The height of a cell's first floor from those of its west (`a`), north (`b`) and north-west
/// (`c`) neighbours' (None: no floor, or out of the tile), else `last` (the last one predicted).
fn predict_height(a: Option<i16>, b: Option<i16>, c: Option<i16>, last: i16) -> i16 {
    match (a, b, c) {
        (Some(a), Some(b), Some(c)) => {
            let (lo, hi) = (a.min(b), a.max(b));
            if c >= hi {
                lo
            } else if c <= lo {
                hi
            } else {
                (i32::from(a) + i32::from(b) - i32::from(c)).clamp(-32_768, 32_767) as i16
            }
        }
        (Some(a), _, _) => a,
        (None, Some(b), _) => b,
        _ => last,
    }
}

thread_local! {
    /// The compressor of `compress`, kept in each thread (making one at the highest levels
    /// takes much memory).
    static COMPRESSOR: std::cell::RefCell<Option<zstd::bulk::Compressor<'static>>> =
        const { std::cell::RefCell::new(None) };
}

/// Gives back this thread's compressor (`threads::Release`).
pub(super) fn release_thread_caches() {
    let _ = COMPRESSOR.try_with(|c| c.borrow_mut().take());
}

/// Bytes compressed as the tiles are stored (zstd, level 19).
#[doc(hidden)]
pub fn compress(raw: &[u8]) -> Result<Vec<u8>> {
    COMPRESSOR.with(|c| {
        let mut c = c.borrow_mut();
        if c.is_none() {
            *c = Some(zstd::bulk::Compressor::new(19)?);
        }
        Ok(c.as_mut().map(|c| c.compress(raw)).transpose()?.unwrap_or_default())
    })
}

/// Floors of the cells within 4 yards of a floor in height walked to most of the time: the
/// nearest of the neighbour's.
const PREDICT_RISE: i32 = 4;

impl TileLayers {
    /// Index of the first floor of each cell (and one past the last).
    pub fn offsets(&self) -> Vec<u32> {
        let mut out = Vec::with_capacity(4097);
        let mut n = 0u32;
        for &c in &self.counts {
            out.push(n);
            n += u32::from(c);
        }
        out.push(n);
        out
    }

    /// The straight walks of floor `k` of cell `i` predicted (`offsets` given): toward a cell
    /// before it (north, west), the floors of that cell walking back to it; toward a cell after
    /// it, the neighbour's floor nearest in height when near enough; out of the tile, the
    /// first floor from the first.
    fn predict_straight(&self, o: &[u32], i: usize, k: usize, d: usize) -> u8 {
        let (r, c) = ((i / 64) as i32, (i % 64) as i32);
        let (dr, dc) = DIRECTIONS[d];
        let (nr, nc) = (r + dr, c + dc);
        if !(0..64).contains(&nr) || !(0..64).contains(&nc) {
            return u8::from(k == 0);
        }
        let n = (nr * 64 + nc) as usize;
        let (lo, hi) = (o[n] as usize, o[n + 1] as usize);
        if d == 0 || d == 2 {
            // Already known: the neighbour's floors that walk here.
            let mut bits = 0u8;
            for (j, f) in self.floors[lo..hi].iter().enumerate() {
                if f.walks[OPPOSITE[d]] >> k & 1 != 0 {
                    bits |= 1 << j;
                }
            }
            return bits;
        }
        let h = i32::from(self.floors[o[i] as usize + k].height);
        (self.floors[lo..hi].iter().enumerate())
            .map(|(j, f)| ((i32::from(f.height) - h).abs(), j))
            .min()
            .filter(|&(dh, _)| dh <= PREDICT_RISE)
            .map_or(0, |(_, j)| 1 << j)
    }

    /// `predict_straight` in the 4 straight directions; quick for a cell of one floor among
    /// cells of one floor (most of them).
    fn predict_straights(&self, o: &[u32], i: usize, k: usize) -> [u8; 4] {
        let (r, c) = (i / 64, i % 64);
        if self.counts[i] == 1 && r > 0 && r < 63 && c > 0 && c < 63 {
            let (n, s, w, e) = (i - 64, i + 64, i - 1, i + 1);
            if self.counts[n] == 1 && self.counts[s] == 1 && self.counts[w] == 1 && self.counts[e] == 1 {
                let f = |j: usize| &self.floors[o[j] as usize];
                let h = i32::from(f(i).height);
                let near = |j: usize| u8::from((i32::from(f(j).height) - h).abs() <= PREDICT_RISE);
                return [
                    f(n).walks[OPPOSITE[0]] & 1,
                    near(s),
                    f(w).walks[OPPOSITE[2]] & 1,
                    near(e),
                ];
            }
        }
        [0, 1, 2, 3].map(|d| self.predict_straight(o, i, k, d))
    }

    /// The diagonal walks of floor `k` of cell `i` the straight walks lead to: through a floor
    /// of a straight neighbour walked to, that walks on to the diagonal neighbour (as the
    /// database builder makes them).
    fn derive_diagonals(&self, o: &[u32], i: usize, k: usize) -> [u8; 4] {
        let (r, c) = ((i / 64) as i32, (i % 64) as i32);
        let f = &self.floors[o[i] as usize + k];
        // Quick: the straight neighbours of one floor each.
        if (1..63).contains(&r) && (1..63).contains(&c) {
            let (n, s, w, e) = (i - 64, i + 64, i - 1, i + 1);
            if self.counts[n] == 1 && self.counts[s] == 1 && self.counts[w] == 1 && self.counts[e] == 1 {
                let g = |j: usize| &self.floors[o[j] as usize].walks;
                let via = |d: usize, j: usize, dn: usize| if f.walks[d] & 1 != 0 { g(j)[dn] } else { 0 };
                // North-west, north-east, south-west, south-east (`VIAS`).
                return [
                    via(0, n, 2) | via(2, w, 0),
                    via(0, n, 3) | via(3, e, 0),
                    via(1, s, 2) | via(2, w, 1),
                    via(1, s, 3) | via(3, e, 1),
                ];
            }
        }
        let mut out = [0u8; 4];
        for (slot, vias) in VIAS.iter().enumerate() {
            let (dr, dc) = DIRECTIONS[4 + slot];
            let mut bits = 0u8;
            for (w, &(dv, dn)) in vias.iter().enumerate() {
                let (vr, vc) = if w == 0 { (r + dr, c) } else { (r, c + dc) };
                if !(0..64).contains(&vr) || !(0..64).contains(&vc) || f.walks[dv] == 0 {
                    continue;
                }
                let v = (vr * 64 + vc) as usize;
                for (j, g) in self.floors[o[v] as usize..o[v + 1] as usize].iter().enumerate() {
                    if f.walks[dv] >> j & 1 != 0 {
                        bits |= g.walks[dn];
                    }
                }
            }
            out[slot] = bits;
        }
        out
    }

    /// Version 3 (see the module).
    pub fn pack(&self) -> Result<Vec<u8>> {
        let mut raw = vec![4u8];
        for (_, stream) in self.streams(false)? {
            raw.extend(stream);
        }
        compress(&raw)
    }

    /// Version 3 without what the planner does not read (`lean`): the heights of the cells of a
    /// single floor and the floors' sizes (the app's databases).
    pub fn pack_lean(&self) -> Result<Vec<u8>> {
        let mut raw = vec![5u8];
        for (_, stream) in self.lean().streams(true)? {
            raw.extend(stream);
        }
        compress(&raw)
    }

    /// The floors without the heights of the cells of a single floor (0: only the heights of the
    /// floors of a cell tell them apart) and without their sizes (0).
    #[must_use]
    pub fn lean(&self) -> Self {
        let mut out = self.clone();
        let o = self.offsets();
        for (&count, &at) in self.counts.iter().zip(o.iter()) {
            if count == 1 {
                out.floors[at as usize].height = 0;
            }
        }
        for f in &mut out.floors {
            f.size = 0;
        }
        out
    }

    /// The streams of version 2 (see the module), named, before compression; `lean`: without the
    /// heights of the cells of a single floor nor the sizes.
    #[doc(hidden)]
    pub fn streams(&self, lean: bool) -> Result<Vec<(&'static str, Vec<u8>)>> {
        ensure!(self.counts.len() == 4096, "layers: {} cells", self.counts.len());
        let o = self.offsets();
        ensure!(
            o[4096] as usize == self.floors.len(),
            "layers: floors and counts differ"
        );
        let mut out = vec![("counts", self.counts.clone())];
        // Heights: first floors predicted from their neighbours', the others from their first.
        let first = |i: usize| {
            (self.counts[i] > 0 && !(lean && self.counts[i] == 1)).then(|| self.floors[o[i] as usize].height)
        };
        let (mut heights, mut last) = (Vec::new(), 0i16);
        for i in 0..4096 {
            let Some(h) = first(i) else { continue };
            let (r, c) = (i / 64, i % 64);
            let a = (c > 0).then(|| first(i - 1)).flatten();
            let b = (r > 0).then(|| first(i - 64)).flatten();
            let nw = (r > 0 && c > 0).then(|| first(i - 65)).flatten();
            let p = predict_height(a, b, nw, last);
            put_varint(&mut heights, zigzag(i32::from(h) - i32::from(p)));
            last = h;
        }
        out.push(("heights", heights));
        let mut upper = Vec::new();
        for i in 0..4096 {
            let (lo, hi) = (o[i] as usize, o[i + 1] as usize);
            for f in &self.floors[(lo + 1).min(hi)..hi] {
                put_varint(
                    &mut upper,
                    zigzag(i32::from(f.height) - i32::from(self.floors[lo].height)),
                );
            }
        }
        out.push(("other heights", upper));
        out.push(("water", self.floors.iter().map(|f| f.water).collect()));
        out.push(("flags", self.floors.iter().map(|f| f.flags).collect()));
        if !lean {
            out.push(("size", self.floors.iter().map(|f| f.size).collect()));
        }
        let (mut straight, mut diagonal) = (Vec::new(), Vec::new());
        for i in 0..4096 {
            for k in 0..usize::from(self.counts[i]) {
                let f = &self.floors[o[i] as usize + k];
                for d in 0..4 {
                    straight.push(f.walks[d] ^ self.predict_straight(&o, i, k, d));
                }
                let derived = self.derive_diagonals(&o, i, k);
                for (&walk, &d) in f.walks[4..8].iter().zip(derived.iter()) {
                    diagonal.push(walk ^ d);
                }
            }
        }
        out.push(("straight walks", straight));
        out.push(("diagonal walks", diagonal));
        let mut slots = Vec::new();
        let mut drops = Vec::new();
        for f in &self.floors {
            let mut s = 0u8;
            for (k, &b) in f.drops.iter().enumerate() {
                if b != 0 {
                    s |= 1 << k;
                    drops.push(b);
                }
            }
            slots.push(s);
        }
        out.push(("drop slots", slots));
        out.push(("drops", drops));
        for tier in 0..FARTHER {
            let (mut slots, mut drops) = (Vec::new(), Vec::new());
            for f in &self.floors {
                let mut s = 0u8;
                for (k, &b) in f.farther[tier].iter().enumerate() {
                    if b != 0 {
                        s |= 1 << k;
                        drops.push(b);
                    }
                }
                slots.push(s);
            }
            out.push(("farther drop slots", slots));
            out.push(("farther drops", drops));
        }
        Ok(out)
    }

    /// A tile's floors stored by `version` (1 to 3).
    pub fn unpack(data: &[u8], version: i64) -> Result<Self> {
        let raw = zstd::decode_all(data)?;
        match version {
            1 => Self::unpack_v1(&raw),
            2 | 3 => Self::unpack_v2(&raw, version == 3),
            v => anyhow::bail!("layers: version {v}"),
        }
    }

    fn unpack_v1(raw: &[u8]) -> Result<Self> {
        ensure!(raw.len() >= 4096, "layers: {} bytes", raw.len());
        let counts = raw[..4096].to_vec();
        let n: usize = counts.iter().map(|&c| usize::from(c)).sum();
        ensure!(
            raw.len() == 4096 + n * (2 + 3 + 8 + DROPS),
            "layers: {} bytes for {n} floors",
            raw.len()
        );
        let mut floors = vec![Floor::default(); n];
        let mut at = 4096;
        let mut previous = 0i16;
        for f in &mut floors {
            previous = previous.wrapping_add(i16::from_le_bytes([raw[at], raw[at + 1]]));
            f.height = previous;
            at += 2;
        }
        for (k, f) in floors.iter_mut().enumerate() {
            f.water = raw[at + k];
            f.flags = raw[at + n + k];
            f.size = raw[at + 2 * n + k];
        }
        at += 3 * n;
        for f in &mut floors {
            f.walks.copy_from_slice(&raw[at..at + 8]);
            at += 8;
        }
        for f in &mut floors {
            f.drops.copy_from_slice(&raw[at..at + DROPS]);
            at += DROPS;
        }
        Ok(Self { counts, floors })
    }

    fn unpack_v2(raw: &[u8], farther: bool) -> Result<Self> {
        let mut rd = Reader { bytes: raw, at: 0 };
        let lean = match (rd.byte()?, farther) {
            (2, false) | (4, true) => false,
            (3, false) | (5, true) => true,
            _ => anyhow::bail!("layers: not version {}", if farther { 3 } else { 2 }),
        };
        let counts = rd.take(4096)?.to_vec();
        ensure!(
            counts.iter().all(|&c| usize::from(c) <= MAX_FLOORS),
            "layers: bad counts"
        );
        let mut l = Self {
            floors: vec![Floor::default(); counts.iter().map(|&c| usize::from(c)).sum()],
            counts,
        };
        let o = l.offsets();
        let n = l.floors.len();
        let mut last = 0i16;
        for i in 0..4096 {
            if l.counts[i] == 0 || (lean && l.counts[i] == 1) {
                continue;
            }
            let first = |l: &Self, j: usize| {
                (l.counts[j] > 0 && !(lean && l.counts[j] == 1)).then(|| l.floors[o[j] as usize].height)
            };
            let (r, c) = (i / 64, i % 64);
            let a = (c > 0).then(|| first(&l, i - 1)).flatten();
            let b = (r > 0).then(|| first(&l, i - 64)).flatten();
            let nw = (r > 0 && c > 0).then(|| first(&l, i - 65)).flatten();
            let p = predict_height(a, b, nw, last);
            let h = (i32::from(p) + unzigzag(rd.varint()?)).clamp(-32_768, 32_767) as i16;
            l.floors[o[i] as usize].height = h;
            last = h;
        }
        for i in 0..4096 {
            let (lo, hi) = (o[i] as usize, o[i + 1] as usize);
            for k in (lo + 1).min(hi)..hi {
                let base = i32::from(l.floors[lo].height);
                l.floors[k].height = (base + unzigzag(rd.varint()?)).clamp(-32_768, 32_767) as i16;
            }
        }
        let water = rd.take(n)?.to_vec();
        let flags = rd.take(n)?.to_vec();
        let size = if lean { vec![0; n] } else { rd.take(n)?.to_vec() };
        for (k, f) in l.floors.iter_mut().enumerate() {
            (f.water, f.flags, f.size) = (water[k], flags[k], size[k]);
        }
        let straight = rd.take(4 * n)?;
        for i in 0..4096 {
            for k in 0..usize::from(l.counts[i]) {
                let at = o[i] as usize + k;
                let p = l.predict_straights(&o, i, k);
                for d in 0..4 {
                    l.floors[at].walks[d] = straight[4 * at + d] ^ p[d];
                }
            }
        }
        let diagonal = rd.take(4 * n)?;
        for i in 0..4096 {
            for k in 0..usize::from(l.counts[i]) {
                let at = o[i] as usize + k;
                let derived = l.derive_diagonals(&o, i, k);
                for (d, p) in derived.into_iter().enumerate() {
                    l.floors[at].walks[4 + d] = diagonal[4 * at + d] ^ p;
                }
            }
        }
        let slots = rd.take(n)?.to_vec();
        for (f, s) in l.floors.iter_mut().zip(slots) {
            for (k, b) in f.drops.iter_mut().enumerate() {
                if s >> k & 1 != 0 {
                    *b = rd.byte()?;
                }
            }
        }
        if farther {
            for tier in 0..FARTHER {
                let slots = rd.take(n)?.to_vec();
                for (f, s) in l.floors.iter_mut().zip(slots) {
                    for (k, b) in f.farther[tier].iter_mut().enumerate() {
                        if s >> k & 1 != 0 {
                            *b = rd.byte()?;
                        }
                    }
                }
            }
        }
        ensure!(rd.at == raw.len(), "layers: {} bytes left", raw.len() - rd.at);
        Ok(l)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A tile of plain ground (one floor a cell walking to its 8 neighbours) with a bridge over
    /// a few cells and odd walks and drops: packed and unpacked the same, both versions.
    #[test]
    fn layers_round_trip() {
        let mut l = TileLayers {
            counts: vec![1; 4096],
            floors: Vec::new(),
        };
        for i in 0..4096usize {
            let (r, c) = (i / 64, i % 64);
            let bridge = r == 20 && (10..20).contains(&c);
            l.counts[i] = if (r, c) == (63, 63) {
                0
            } else if bridge {
                2
            } else {
                1
            };
            for k in 0..l.counts[i] {
                l.floors.push(Floor {
                    height: (r as i16 * 3 - c as i16) + i16::from(k) * 12,
                    water: if c == 40 { WET | 3 << 3 } else { 0 },
                    flags: if k == 0 { SURFACE } else { ABOVE | MODEL },
                    size: if c == 5 { 7 } else { 16 },
                    walks: if k == 0 { [1; 8] } else { [2, 2, 2, 2, 0, 0, 0, 0] },
                    drops: if c == 7 {
                        [0, 0, 0, 1, 0]
                    } else if k == 1 {
                        [0, 0, 0, 0, 1]
                    } else {
                        [0; DROPS]
                    },
                    farther: if c == 9 {
                        [[0, 0, 1, 0, 0], [0, 2, 0, 0, 0]]
                    } else if k == 1 && c == 12 {
                        [[0; DROPS], [0, 0, 0, 0, 1]]
                    } else {
                        [[0; DROPS]; FARTHER]
                    },
                });
            }
        }
        l.floors[100].walks = [0, 3, 0, 1, 255, 0, 1, 1];
        // What the versions before 3 hold: no farther drops.
        let mut near = l.clone();
        near.floors.iter_mut().for_each(|f| f.farther = [[0; DROPS]; FARTHER]);
        assert_ne!(near, l);
        for version in [1, 2, 3] {
            let packed = if version == 2 {
                let mut raw = vec![2u8];
                let streams = near.streams(false).unwrap();
                for (_, stream) in &streams[..streams.len() - 2 * FARTHER] {
                    raw.extend(stream);
                }
                compress(&raw).unwrap()
            } else if version == 1 {
                let mut raw = l.counts.clone();
                let mut previous = 0i16;
                for f in &l.floors {
                    raw.extend(f.height.wrapping_sub(previous).to_le_bytes());
                    previous = f.height;
                }
                raw.extend(l.floors.iter().map(|f| f.water));
                raw.extend(l.floors.iter().map(|f| f.flags));
                raw.extend(l.floors.iter().map(|f| f.size));
                l.floors.iter().for_each(|f| raw.extend(f.walks));
                l.floors.iter().for_each(|f| raw.extend(f.drops));
                zstd::encode_all(raw.as_slice(), 3).unwrap()
            } else {
                l.pack().unwrap()
            };
            let want = if version == 3 { &l } else { &near };
            assert_eq!(
                &TileLayers::unpack(&packed, version).unwrap(),
                want,
                "version {version}"
            );
        }
        // Lean: the same but the heights of the cells of one floor and the sizes.
        assert_eq!(TileLayers::unpack(&l.pack_lean().unwrap(), 3).unwrap(), l.lean());
    }
}
