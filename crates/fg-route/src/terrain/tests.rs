use super::layers::{ABOVE, DROPS, FARTHER, Floor, SURFACE, UNDER};
use super::*;

/// Highest rise walked between two neighbour floors, and highest drop (yards), as the floors of
/// the tests are made.
const RISE: i32 = 9;
const FALL: i32 = 15;

/// The floors of each cell of tiles of 64 x 64 cells, from `floors` (continent, global row and
/// column: the height and water of each floor, the first the terrain's), the walks and drops
/// between neighbour floors made from their heights (as the database builder finds them over
/// the geometry): a walk both ways within `RISE` yards, a drop one way down to `FALL`, the
/// diagonals through a straight neighbour.
fn floors_terrain(tiles: &[(i64, i32, i32)], floors: impl Fn(i64, i32, i32) -> Vec<(i16, u8)>) -> Terrain {
    floors_terrain_with(tiles, floors, 0)
}

/// `floors_terrain` taking drops up to the tier `tier` of `DROP_HEIGHTS` (a drop of 15 to 30
/// yards is the second tier's, of 30 to 50 the third's).
fn floors_terrain_with(
    tiles: &[(i64, i32, i32)],
    floors: impl Fn(i64, i32, i32) -> Vec<(i16, u8)>,
    tier: usize,
) -> Terrain {
    let conn = Connection::open_in_memory().unwrap();
    conn.execute_batch(
        "CREATE TABLE client_terrain_layers (map_id INTEGER, tile_x INTEGER, tile_y INTEGER,
         version INTEGER, floors INTEGER, data BLOB)",
    )
    .unwrap();
    for &(map, tx, ty) in tiles {
        let (r0, c0) = (ty * 64, tx * 64);
        let mut l = TileLayers {
            counts: vec![0; 4096],
            floors: Vec::new(),
        };
        let at = |r: i32, c: i32| {
            let inside = tiles.contains(&(map, c.div_euclid(64), r.div_euclid(64)));
            if inside { floors(map, r, c) } else { Vec::new() }
        };
        // Straight walks first, then the diagonals through them.
        let straight = |r: i32, c: i32, h: i16| -> [u8; 4] {
            let mut out = [0u8; 4];
            for (d, &(dr, dc)) in DIRECTIONS[..4].iter().enumerate() {
                for (k, &(h2, _)) in at(r + dr, c + dc).iter().enumerate() {
                    if (i32::from(h2) - i32::from(h)).abs() <= RISE {
                        out[d] |= 1 << k;
                    }
                }
            }
            out
        };
        for i in 0..4096usize {
            let (r, c) = (r0 + (i / 64) as i32, c0 + (i % 64) as i32);
            let here = at(r, c);
            l.counts[i] = here.len() as u8;
            for (f, &(h, water)) in here.iter().enumerate() {
                let mut floor = Floor {
                    height: h,
                    water,
                    flags: if f == 0 {
                        SURFACE
                    } else if h < here[0].0 {
                        UNDER
                    } else {
                        ABOVE
                    },
                    size: 16,
                    walks: [0; 8],
                    drops: [0; DROPS],
                    farther: [[0; DROPS]; FARTHER],
                };
                let s = straight(r, c, h);
                floor.walks[..4].copy_from_slice(&s);
                for (d, &(dr, dc)) in DIRECTIONS.iter().enumerate().skip(4) {
                    for (via, rest) in [((dr, 0), (0, dc)), ((0, dc), (dr, 0))] {
                        let dv = DIRECTIONS.iter().position(|&x| x == via).unwrap();
                        let dn = DIRECTIONS.iter().position(|&x| x == rest).unwrap();
                        for (k, &(hv, _)) in at(r + via.0, c + via.1).iter().enumerate() {
                            if s[dv] >> k & 1 != 0 {
                                floor.walks[d] |= straight(r + via.0, c + via.1, hv)[dn];
                            }
                        }
                    }
                }
                for (d, &(dr, dc)) in DIRECTIONS[..4].iter().enumerate() {
                    for (k, &(h2, _)) in at(r + dr, c + dc).iter().enumerate() {
                        let fall = i32::from(h) - i32::from(h2);
                        if fall > RISE && fall <= FALL {
                            floor.drops[d] |= 1 << k;
                        } else if let Some(t) = far_tier(fall) {
                            floor.farther[t][d] |= 1 << k;
                        }
                    }
                }
                for (k, &(h2, _)) in here.iter().enumerate() {
                    let fall = i32::from(h) - i32::from(h2);
                    if k != f && fall > RISE && fall <= FALL {
                        floor.drops[4] |= 1 << k;
                    } else if let (true, Some(t)) = (k != f, far_tier(fall)) {
                        floor.farther[t][4] |= 1 << k;
                    }
                }
                l.floors.push(floor);
            }
        }
        conn.execute(
            "INSERT INTO client_terrain_layers VALUES (?1, ?2, ?3, ?4, ?5, ?6)",
            rusqlite::params![map, tx, ty, layers::VERSION, l.floors.len() as i64, l.pack().unwrap()],
        )
        .unwrap();
    }
    Terrain::load(&conn, tier).unwrap()
}

/// The tier of the farther drops (`Floor::farther`) a fall of `fall` yards is in, if any.
fn far_tier(fall: i32) -> Option<usize> {
    match fall {
        16..=30 => Some(0),
        31..=50 => Some(1),
        _ => None,
    }
}

/// Tile (32, 32) of continent 1, a floor at height 0 on each cell but where `steep` (none).
pub(super) fn flat(steep: impl Fn(usize, usize) -> bool) -> Terrain {
    floors_terrain(&[(1, 32, 32)], |_, r, c| {
        let (r, c) = ((r - 32 * 64) as usize, (c - 32 * 64) as usize);
        if steep(r, c) { Vec::new() } else { vec![(0, 0)] }
    })
}

/// A tile with a wall (no floor) on columns 30 to 35, open at the rows given.
fn terrain(open: &[usize]) -> Terrain {
    flat(|r, c| (30..36).contains(&c) && !open.contains(&r))
}

/// Tile (32, 32) of continent 1 from a height and a water byte for each cell (row, column).
fn shaped(cell: impl Fn(usize, usize) -> (i16, u8)) -> Terrain {
    floors_terrain(&[(1, 32, 32)], |_, r, c| {
        let (h, w) = cell((r - 32 * 64) as usize, (c - 32 * 64) as usize);
        let depth = if w & WET != 0 { i16::from(w >> 3) } else { 0 };
        vec![(h + depth, w)]
    })
}

/// Centre of cell (r, c) of tile (32, 32).
fn at(r: f64, c: f64) -> Pos {
    Pos {
        continent: 1,
        x: ORIGIN - 32.0 * TILE - (r + 0.5) * CELL,
        y: ORIGIN - 32.0 * TILE - (c + 0.5) * CELL,
        zone: 0,
        z: None,
    }
}

#[test]
fn walks_around_a_wall() {
    let t = terrain(&[60, 61, 62, 63]);
    let (a, b) = (at(10.0, 20.0), at(10.0, 44.0));
    let d = t.distance(&a, &b).unwrap();
    assert!(d > 2.0 * 40.0 * CELL, "around the wall through its gap: {d}");
    // Same side: straight.
    let c = at(30.0, 20.0);
    assert!((t.distance(&a, &c).unwrap() - a.dist(&c)).abs() < 1e-6);
}

/// The points a walk turns at go through the gap of the wall, not across it; a walk with
/// nothing in the way turns nowhere.
#[test]
fn walk_turns_through_the_gap() {
    let t = terrain(&[60, 61, 62, 63]);
    let (a, b) = (at(10.0, 20.0), at(10.0, 44.0));
    let d = t.distance(&a, &b).unwrap();
    let turns = t.waypoints(&a, &b);
    assert_ne!(turns, []);
    let cells: Vec<(i32, i32)> = turns.iter().map(Terrain::cell).collect();
    let (r, c) = (32 * 64, 32 * 64);
    assert!(
        cells.iter().all(|&(cr, cc)| cr - r >= 10 && (cc - c - 32).abs() <= 12),
        "{cells:?}"
    );
    assert!(
        cells.iter().any(|&(cr, _)| cr - r >= 59),
        "not through the gap: {cells:?}"
    );
    // The polyline is as long as the walk measured.
    let mut points = vec![a];
    points.extend(turns);
    points.push(b);
    let length: f64 = points.windows(2).map(|w| w[0].dist(&w[1])).sum();
    assert!((length - d).abs() < 0.1 * d, "{length} for a walk of {d}");
    assert_eq!(t.waypoints(&a, &at(30.0, 20.0)), []);
}

/// The memory of a terrain, saved (JSON) and given to another load of the same data, spares
/// the search: the walk is the same and no path is added.
#[test]
fn saved_memory_spares_the_search() {
    let first = terrain(&[60, 61, 62, 63]);
    let (a, b) = (at(10.0, 20.0), at(10.0, 44.0));
    let walk = first.distance(&a, &b).unwrap();
    let memory = first.memory();
    assert_eq!(memory.len(), 1);
    let saved: WalkMemory = serde_json::from_str(&serde_json::to_string(&memory).unwrap()).unwrap();
    assert_eq!(saved, memory);
    let walks = Arc::new(Walks::default());
    walks.add(saved);
    let mut second = terrain(&[60, 61, 62, 63]);
    second.share(Arc::clone(&walks));
    assert_eq!(walks.len(), 1);
    assert!((second.distance(&a, &b).unwrap() - walk).abs() < 1e-3);
    assert_eq!(walks.len(), 1, "searched again");
}

/// A memory saved by a version walking the heightmap terrain (format 2) reads as empty: its
/// walks are searched again, the guide it came with still opens.
#[test]
fn memory_of_the_heightmap_terrain_reads_as_empty() {
    let bytes = [0u8; 12];
    let old = format!("\"2:{}\"", BASE64.encode(zstd::encode_all(&bytes[..], 3).unwrap()));
    let memory: WalkMemory = serde_json::from_str(&old).unwrap();
    assert!(memory.is_empty());
}

/// Two large areas the floors do not connect are walked between in a straight line (a lift
/// or a boat may join them), unless one of them is enclosed: then no walk does.
#[test]
fn enclosed_area_is_not_walked_out_of() {
    // Tile (33, 32), east of (32, 32), has its first 6 columns without floor: a cliff between.
    let mut t = floors_terrain(&[(1, 32, 32), (1, 33, 32)], |_, _, c| {
        if (33 * 64..33 * 64 + 6).contains(&c) {
            Vec::new()
        } else {
            vec![(0, 0)]
        }
    });
    let (west, east) = (at(10.0, 20.0), at(10.0, 64.0 + 20.0));
    assert_eq!(t.distance(&west, &east), None, "way unknown: straight line");
    assert!(!t.separated(&west, &east));
    assert!(t.disconnected(&west, &east));
    assert!(t.enclose(&east, 0).is_some());
    assert_eq!(t.distance(&west, &east), Some(f64::INFINITY));
    assert!(t.separated(&west, &east));
    assert!(
        t.distance(&east, &at(30.0, 64.0 + 40.0)).unwrap().is_finite(),
        "walked within"
    );
}

/// A valley the floors join to the mainland, by a way however narrow, is the mainland's area:
/// it is not enclosed (that would cut a way that is there), nor taken out of it.
#[test]
fn valley_joined_by_a_narrow_way_stays_the_mainlands() {
    // Tile (33, 32): a wall on columns 0 to 9 but on the last row, and columns 48 to 63.
    let mut t = floors_terrain(&[(1, 32, 32), (1, 33, 32)], |_, r, c| {
        let (r, c) = (r - 32 * 64, c - 32 * 64);
        let wall = c >= 64 && ((c < 74 && r < 63) || c >= 112);
        if wall { Vec::new() } else { vec![(0, 0)] }
    });
    let (plain, valley) = (at(10.0, 20.0), at(10.0, 64.0 + 20.0));
    assert!(
        t.distance(&plain, &valley).unwrap().is_finite(),
        "through the narrow way"
    );
    assert!(!t.detach(&valley));
    assert!(t.enclose(&valley, 0).is_none(), "one area with the mainland");
    assert!(!t.disconnected(&plain, &valley));
}

/// A place in a cave the floors miss stands where there is no floor (the wall above it), nearer
/// the wall's west side; the cave opens east: walked to from the east through its mouth, from
/// the west around the wall to the mouth.
#[test]
fn cave_is_walked_to_through_its_mouth() {
    let mut t = terrain(&[60, 61, 62, 63]);
    let (inside, mouth) = (at(10.0, 31.0), at(10.0, 37.0));
    let (west, east) = (at(10.0, 20.0), at(10.0, 44.0));
    let before = (t.distance(&west, &inside).unwrap(), t.distance(&east, &inside).unwrap());
    assert!(before.0 < before.1, "nearer the west side: {before:?}");
    assert!(t.add_cave(&mouth, &inside, 3.0 * CELL));
    let from_east = t.distance(&east, &inside).unwrap();
    assert!(
        (from_east - (east.dist(&mouth) + mouth.dist(&inside))).abs() < CELL,
        "{from_east}"
    );
    let from_west = t.distance(&west, &inside).unwrap();
    assert!(from_west > 2.0 * 40.0 * CELL, "around the wall: {from_west}");
    let turns = t.waypoints(&west, &inside);
    assert!(
        turns.last().is_some_and(|p| p.dist(&mouth) < 1e-6),
        "through the mouth: {turns:?}"
    );
    // Two places of the cave: straight between them; places outside it are not moved.
    let deeper = at(12.0, 32.0);
    assert!((t.distance(&inside, &deeper).unwrap() - inside.dist(&deeper)).abs() < 1e-6);
    assert!((t.distance(&east, &at(20.0, 44.0)).unwrap() - 10.0 * CELL).abs() < 1e-6);
}

/// A memory of other terrain data (the game changed) is not used: its walks would be wrong.
#[test]
fn memory_of_other_terrain_is_dropped() {
    let old = terrain(&[60, 61, 62, 63]);
    old.distance(&at(10.0, 20.0), &at(10.0, 44.0)).unwrap();
    let walks = Arc::new(Walks::default());
    walks.add(old.memory());
    let mut new = terrain(&[0, 1, 2, 3]);
    new.share(Arc::clone(&walks));
    assert!(walks.is_empty());
}

/// Two continents share cell numbers: the answers of one are not reused for the other.
#[test]
fn continents_do_not_share_answers() {
    let t = floors_terrain(&[(1, 32, 32), (0, 32, 32)], |map, r, c| {
        let (r, c) = (r - 32 * 64, c - 32 * 64);
        if map == 1 && r < 60 && (30..36).contains(&c) {
            Vec::new()
        } else {
            vec![(0, 0)]
        }
    });
    let on = |continent, (a, b): (Pos, Pos)| {
        let p = |q: Pos| Pos { continent, ..q };
        t.distance(&p(a), &p(b)).unwrap()
    };
    let pair = (at(10.0, 20.0), at(10.0, 44.0));
    let around = on(1, pair);
    let straight = on(0, pair);
    assert!(around > straight + 100.0, "walled {around} vs open {straight}");
    assert!((on(1, pair) - around).abs() < 1e-6);
}

/// Points on both sides of a cell border, west and east of the origin, fall in the cell
/// `floor` gives.
#[test]
fn cell_of_a_point() {
    for v in [
        -3.5 * CELL,
        -CELL,
        -0.25,
        0.0,
        0.25,
        CELL,
        2.0 * CELL - 1e-9,
        70.0 * CELL,
    ] {
        let p = Pos {
            continent: 1,
            x: ORIGIN - v,
            y: ORIGIN - v,
            zone: 0,
            z: None,
        };
        let expected = ((ORIGIN - p.x) / CELL).floor() as i32;
        assert_eq!(Terrain::cell(&p), (expected, expected), "{v}");
    }
}

#[test]
fn unconnected_sides() {
    let t = terrain(&[]);
    assert!(t.distance(&at(10.0, 20.0), &at(10.0, 44.0)).is_none());
}

/// A cliff (13 yards high from column 32: too high to climb, low enough to jump) is jumped down
/// from its top, not climbed from its foot: the walk up goes round by the ramp at the bottom
/// rows, the walk down is straight.
#[test]
fn cliff_jumped_down_not_climbed() {
    let t = shaped(|r, c| {
        let top = c >= 32 && r < 56;
        // The ramp: from column 32 the bottom rows rise slowly to the top.
        let ramp = c >= 32 && r >= 56;
        (
            if top {
                13
            } else if ramp {
                ((c - 32) * 5).min(13) as i16
            } else {
                0
            },
            0,
        )
    });
    let (foot, top) = (at(10.0, 20.0), at(10.0, 44.0));
    let down = t.distance(&top, &foot).unwrap();
    let up = t.distance(&foot, &top).unwrap();
    assert!((down - top.dist(&foot)).abs() < CELL, "jumped straight down: {down}");
    assert!(up > 2.0 * down, "up by the ramp: {up} vs {down}");
}

/// Water deep enough is swum, slower than walking: the walk goes round a lake rather than
/// across, unless round is much longer. The open sea is never entered.
#[test]
fn lakes_swum_slower_and_open_sea_avoided() {
    let lake = |deep: bool| {
        shaped(move |r, c| {
            if (8..56).contains(&r) && (28..36).contains(&c) {
                (0, WET | (10 << 3) | if deep { DEEP } else { 0 })
            } else {
                (5, 0)
            }
        })
    };
    let (a, b) = (at(30.0, 20.0), at(30.0, 44.0));
    let swum = lake(false).distance(&a, &b).unwrap();
    assert!(
        swum > a.dist(&b) + 2.0 * CELL,
        "the lake costs more than walking: {swum}"
    );
    let round = lake(true).distance(&a, &b).unwrap();
    assert!(round > swum, "the open sea is walked round: {round} vs {swum}");
}

/// A ridge without a floor over it, a tunnel under it (a floor under the terrain): the walk
/// between the valleys on either side goes through the tunnel, straight, not round the ridge.
#[test]
fn tunnel_under_a_ridge_is_walked_through() {
    let t = floors_terrain(&[(1, 32, 32)], |_, r, c| {
        let (r, c) = (r - 32 * 64, c - 32 * 64);
        let ridge = (28..36).contains(&c) && r < 60;
        match (ridge, r == 20) {
            (false, _) => vec![(0, 0)],
            // The tunnel's floor, 30 yards under the ridge's top.
            (true, true) => vec![(-30, 0)],
            (true, false) => Vec::new(),
        }
    });
    // The valleys go down to the tunnel's mouths: its floor is the valley's there.
    let t2 = floors_terrain(&[(1, 32, 32)], |_, r, c| {
        let (r, c) = (r - 32 * 64, c - 32 * 64);
        let ridge = (28..36).contains(&c) && r < 60;
        if !ridge || r == 20 { vec![(0, 0)] } else { Vec::new() }
    });
    let (west, east) = (at(20.0, 20.0), at(20.0, 44.0));
    // A floor 30 yards down is not walked to from the valley: round the ridge.
    assert!(t.distance(&west, &east).unwrap() > 2.0 * west.dist(&east));
    let through = t2.distance(&west, &east).unwrap();
    assert!(
        (through - west.dist(&east)).abs() < 1e-6,
        "straight through the tunnel: {through}"
    );
}

/// A road under a bridge: the cells under it have two floors, the road's and the bridge's 12
/// yards above. Each is walked along on its own floor; the bridge is dropped from onto the
/// road, never climbed onto from it.
#[test]
fn bridge_over_a_road() {
    let t = floors_terrain(&[(1, 32, 32)], |_, r, c| {
        let (r, c) = (r - 32 * 64, c - 32 * 64);
        // The road: every cell, at 0; the bridge: row 30 from column 20 to 44 at 12, joined
        // to the ground at its ends by ramps (columns 16 to 19 and 45 to 48).
        let mut floors = vec![(0, 0)];
        if r == 30 {
            match c {
                20..=44 => floors.push((12, 0)),
                16..=19 => floors = vec![((c - 15) as i16 * 3, 0)],
                45..=48 => floors = vec![((49 - c) as i16 * 3, 0)],
                _ => {}
            }
        }
        floors
    });
    let grid = &t.grids[&1];
    let (r, c) = (32 * 64 + 30, 32 * 64 + 32);
    assert_eq!(grid.floors(r, c), 2);
    let road = Node::new(r, c, 0);
    let deck = Node::new(r, c, 1);
    assert!(grid.joined(road, Node::new(r + 1, c, 0)), "the road goes on under it");
    assert!(grid.joined(deck, Node::new(r, c + 1, 1)), "the deck goes on");
    assert!(
        !grid.walks_to(road, Node::new(r, c + 1, 1)),
        "not climbed onto from the road"
    );
    assert!(
        grid.step(deck, Node::new(r + 1, c, 0)).is_some(),
        "dropped from onto the road"
    );
    assert_eq!(grid.area(road), grid.area(deck), "one area: the ramps join them");
    // Along the road under the bridge: straight.
    let (a, b) = (at(20.0, 32.0), at(40.0, 32.0));
    assert!((t.distance(&a, &b).unwrap() - a.dist(&b)).abs() < 1e-6);
}

/// A cell with two floors in two large areas, the ground and the streets of a city over it: a
/// place there stands on the floor of the area most of the floors around it are in (by the
/// city's edge, the street that goes on outside; by the other edge, the ground that does).
#[test]
fn place_stands_on_the_floor_of_the_area_around() {
    let t = floors_terrain(&[(1, 32, 32)], |_, r, c| {
        let r = r - 32 * 64;
        let _ = c;
        // North of row 20 the ground is at the streets' height (20), south of row 50 at the
        // ground's (0); in between both.
        if r < 20 {
            vec![(20, 0)]
        } else if r <= 50 {
            vec![(0, 0), (20, 0)]
        } else {
            vec![(0, 0)]
        }
    });
    let grid = &t.grids[&1];
    let street = grid.snap(Terrain::cell(&at(21.0, 32.0))).unwrap();
    let ground = grid.snap(Terrain::cell(&at(49.0, 32.0))).unwrap();
    assert_eq!(street.f(), 1, "on the street, by the north edge");
    assert_eq!(ground.f(), 0, "on the ground, by the south edge");
    assert_ne!(grid.area(street), grid.area(ground));
    assert_eq!(
        grid.area(street),
        grid.area(grid.snap(Terrain::cell(&at(5.0, 32.0))).unwrap())
    );
}

/// Far walks go through the tiles' gates, floors of other levels too (a bridge across a tile's
/// border): about as long as the cheapest floor by floor, never shorter.
#[test]
fn far_walks_through_the_gates() {
    let tiles: Vec<(i64, i32, i32)> = (32..35).flat_map(|x| (32..34).map(move |y| (1, x, y))).collect();
    let t = floors_terrain(&tiles, |_, r, c| {
        let (r, c) = (r - 32 * 64, c - 32 * 64);
        // A river (no floor) along column 100, crossed by a bridge on row 70 (12 yards up,
        // its ramps from column 90 and to 110), and a wall on row 70 west of the bridge.
        if c == 100 && r != 70 {
            return Vec::new();
        }
        if r == 70 {
            return match c {
                95..=105 => vec![(12, 0)],
                91..=94 => vec![((c - 90) as i16 * 3, 0)],
                106..=109 => vec![((110 - c) as i16 * 3, 0)],
                c if c < 60 => Vec::new(),
                _ => vec![(0, 0)],
            };
        }
        vec![(0, 0)]
    });
    let grid = &t.grids[&1];
    let (a, b) = (at(10.0, 10.0), at(120.0, 180.0));
    let d = t.distance(&a, &b).unwrap();
    let (na, nb) = (
        grid.snap(Terrain::cell(&a)).unwrap(),
        grid.snap(Terrain::cell(&b)).unwrap(),
    );
    let mut trace = Vec::new();
    let cheapest = Terrain::path(grid, na, nb, 240, Some(&mut trace)).unwrap();
    assert!(d >= cheapest - 1e-3 && d <= cheapest * GATES_SLACK, "{d} vs {cheapest}");
    let bridge = Node::new(32 * 64 + 70, 32 * 64 + 100, 0);
    assert!(trace.contains(&bridge), "over the bridge: {trace:?}");
    let mut gates = Vec::new();
    let through = grid.through_gates(na, nb, Some(&mut gates)).unwrap();
    assert!((through - d).abs() < 1e-3);
    // Its legs from gate to gate are walks within a tile: the one across the river's tiles
    // goes from a side of the river to the other.
    assert!(gates.iter().any(|n| n.c() < 32 * 64 + 100) && gates.iter().any(|n| n.c() > 32 * 64 + 100));
}

/// How much longer than the cheapest a walk through the gates is, at most (on these tests).
const GATES_SLACK: f64 = 1.25;

/// A mine under a hill: the ground at 10 yards, a hill at 40 (rows and columns 20 to 44) climbed
/// by a ramp from the east, a mine under its west half at 10 (rows 28 to 36, columns 20 to 31)
/// that opens on the ground west of the hill.
fn mine_under_a_hill() -> Terrain {
    floors_terrain(&[(1, 32, 32)], |_, r, c| {
        let (r, c) = (r - 32 * 64, c - 32 * 64);
        let hill = (20..=44).contains(&r) && (20..=44).contains(&c);
        let mine = (28..=36).contains(&r) && (20..=31).contains(&c);
        let ramp = (20..=44).contains(&r) && (45..=54).contains(&c);
        if mine {
            vec![(40, 0), (10, 0)]
        } else if hill {
            vec![(40, 0)]
        } else if ramp {
            vec![(40 - (c - 44) as i16 * 3, 0)]
        } else {
            vec![(10, 0)]
        }
    })
}

/// A place of known height in a cell of several floors stands on the floor of its height: the
/// kobold in the mine is walked to through the mine's mouth, not up the hill above it; without
/// a height, or a height of no floor there, on the surface as before. A cell of a single floor
/// keeps it whatever the height.
#[test]
fn place_of_known_height_stands_on_its_floor() {
    let t = mine_under_a_hill();
    let west = at(32.0, 10.0);
    let kobold = |z: Option<i16>| Pos { z, ..at(32.0, 26.0) };
    let floor = |p: &Pos| t.floors_at(p).unwrap().1;
    // Asked first without a height: the answers given for the cell are not those of a height.
    let over = t.distance(&west, &kobold(None)).unwrap();
    let inside = t.distance(&west, &kobold(Some(11))).unwrap();
    assert_eq!(floor(&kobold(None)), Some(40), "on the hill without a height");
    assert_eq!(floor(&kobold(Some(11))), Some(10), "in the mine");
    assert_eq!(
        floor(&kobold(Some(25))),
        Some(40),
        "no floor that high: as without a height"
    );
    assert!(
        (inside - west.dist(&kobold(None))).abs() < 1e-6,
        "straight in through the mouth: {inside}"
    );
    assert!(over > 2.0 * inside, "up the ramp and over the hill: {over} vs {inside}");
    assert_eq!(t.distance(&kobold(Some(11)), &west), Some(inside), "and back out");
    // On the ground (one floor), a height changes nothing.
    let ground = |z| Pos { z, ..at(10.0, 10.0) };
    assert_eq!(t.distance(&west, &ground(Some(90))), t.distance(&west, &ground(None)));
}

/// The turns of a walk to a place of known height keep the heights of the floors they are on:
/// asked again from a turn, the walk stays on that floor.
#[test]
fn turns_keep_their_floors() {
    let t = mine_under_a_hill();
    let (from, kobold) = (
        at(10.0, 60.0),
        Pos {
            z: Some(10),
            ..at(32.0, 26.0)
        },
    );
    let turns = t.waypoints(&from, &kobold);
    assert!(!turns.is_empty(), "round the hill to the mine's mouth");
    let last = turns.last().unwrap();
    assert!(
        (t.distance(last, &kobold).unwrap() - last.dist(&kobold)).abs() < 1e-6,
        "from the last turn, straight in: {last:?}"
    );
}

/// A plateau `high` yards over the plain from column 32, reached by a ramp on the bottom rows
/// only, as the terrain taking drops up to `tier`.
fn plateau(high: i16, tier: usize) -> Terrain {
    floors_terrain_with(
        &[(1, 32, 32)],
        |_, r, c| {
            let (r, c) = ((r - 32 * 64) as usize, (c - 32 * 64) as usize);
            let h = if c >= 32 && r < 56 {
                high
            } else if c >= 32 {
                (((c - 32) * 5) as i16).min(high)
            } else {
                0
            };
            vec![(h, 0)]
        },
        tier,
    )
}

/// How far down a plateau a walk goes from its top to the foot, and the ramp's way (the foot to
/// the top), with each tier of drops.
fn down_and_up(high: i16, tier: usize) -> (f64, f64) {
    let t = plateau(high, tier);
    let (foot, top) = (at(10.0, 20.0), at(10.0, 44.0));
    (t.distance(&top, &foot).unwrap(), t.distance(&foot, &top).unwrap())
}

/// A drop beyond the safe fall is taken only by a character that takes the next tier, and it
/// costs the rest the health needs: more than the straight way down, less than the ramp's.
#[test]
fn farther_drops_follow_the_tier() {
    let straight = at(10.0, 44.0).dist(&at(10.0, 20.0));
    // 25 yards: the second tier's.
    let (safe, _) = down_and_up(25, 0);
    let (hurt, up) = down_and_up(25, 1);
    assert!(
        safe > 2.0 * straight,
        "round by the ramp without the tier: {safe} vs {straight}"
    );
    assert!(
        hurt > straight + 100.0 && hurt < safe / 2.0,
        "jumped with the tier, resting paid: {hurt}"
    );
    assert!(up > 2.0 * straight, "never climbed back: {up}");
    // 40 yards: only the third tier's.
    let (second, _) = down_and_up(40, 1);
    let (third, _) = down_and_up(40, 2);
    assert!(second > 2.0 * straight, "too far for the second tier: {second}");
    assert!(
        third > hurt && third < 0.7 * second,
        "jumped with the third: {third} vs {hurt} and {second}"
    );
}

/// A wall bends the path into several segments, then a wet strip crosses it between dry ends.
pub(super) fn movement_ground(kind: TestGround) -> Terrain {
    match kind {
        TestGround::Dry => return flat(|_, _| false),
        TestGround::Shallow => return floors_terrain(&[(1, 32, 32)], |_, _, _| vec![(0, WET | 1 << 3)]),
        TestGround::Covered => return floors_terrain(&[(1, 32, 32)], |_, _, _| vec![(0, 0), (20, 0)]),
        TestGround::Unknown => return Terrain::default(),
        TestGround::Mixed => {}
    }
    floors_terrain(&[(1, 32, 32)], |_, r, c| {
        let (r, c) = (r - 32 * 64, c - 32 * 64);
        if (20..24).contains(&r) && c < 18 {
            vec![]
        } else {
            vec![(0, if (32..36).contains(&r) { WET | 3 << 3 } else { 0 })]
        }
    })
}
