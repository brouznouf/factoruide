#!/bin/sh
# The terrain the fixtures walk on (crates/fg-route/tests/fixtures/terrain-<edition>.sqlite.zst):
# the tiles of the regions listed in terrain-regions.txt, as the databases store them. Rebuild it
# when the format of the tiles changes or a region is added:
#   scripts/test-terrain.sh
set -eu
cd "$(dirname "$0")/.."
dir=crates/fg-route/tests/fixtures
for edition in classic tbc forever; do
    regions=$(grep "^$edition " "$dir/terrain-regions.txt" || true)
    [ -n "$regions" ] || continue
    db=data/$edition.sqlite
    [ -f "$db" ] || zstd -d -q -f "$db.zst" -o "$db"
    tmp=$(mktemp -d)
    sqlite3 "$db" ".schema client_terrain_layers" ".schema client_terrain_gates" | sqlite3 "$tmp/terrain.sqlite"
    echo "$regions" | while read -r _ map x0 x1 y0 y1; do
        sqlite3 "$tmp/terrain.sqlite" "attach '$db' as r;
          insert or ignore into client_terrain_layers select * from r.client_terrain_layers where map_id = $map and tile_x between $x0 and $x1 and tile_y between $y0 and $y1;
          insert or ignore into client_terrain_gates select * from r.client_terrain_gates where map_id = $map and tile_x between $x0 and $x1 and tile_y between $y0 and $y1;"
    done
    sqlite3 "$tmp/terrain.sqlite" vacuum
    zstd -19 -q -f "$tmp/terrain.sqlite" -o "$dir/terrain-$edition.sqlite.zst"
    rm -r "$tmp"
done
ls -l "$dir"/terrain-*.zst
