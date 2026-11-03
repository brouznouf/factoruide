#!/bin/sh
# The base every test fixture starts from (crates/fg-route/tests/fixtures/base-<edition>.sqlite.zst):
# the real schema, empty, with the game's constants (classes, races, zones, factions, base stats,
# class spells) and the races' start NPCs. A fixture adds its quests, NPCs and items to it
# (`fg_route::fixture`). Rebuild it when the database's schema changes or an edition's races do:
#   scripts/test-base.sh
set -eu
cd "$(dirname "$0")/.."
out=crates/fg-route/tests/fixtures
mkdir -p "$out"
for edition in classic tbc forever; do
    db=data/$edition.sqlite
    [ -f "$db" ] || zstd -d -q -f "$db.zst" -o "$db"
    tmp=$(mktemp -d)
    sqlite3 "$db" .schema | sqlite3 "$tmp/base.sqlite"
    npcs=$(cat overrides/races.toml "overrides/$edition/races.toml" 2>/dev/null | sed -n 's/^start_npc *= *\([0-9][0-9]*\).*/\1/p' | sort -u | paste -sd, -)
    spells=$(cat overrides/*.toml "overrides/$edition"/*.toml | grep -v '^ *#' | grep -oE '[0-9]{3,6}' | sort -un | paste -sd, -)
    sqlite3 "$tmp/base.sqlite" <<SQL
attach '$db' as r;
insert into client_chrclasses select * from r.client_chrclasses;
insert into client_chrraces select * from r.client_chrraces;
insert into client_charbaseinfo select * from r.client_charbaseinfo;
insert into client_areatable select * from r.client_areatable;
insert into client_factiontemplate select * from r.client_factiontemplate;
insert into client_faction select * from r.client_faction;
insert into client_map select * from r.client_map;
insert into client_questxp select * from r.client_questxp;
insert into client_uimap select * from r.client_uimap;
insert into client_worldmapoverlay select * from r.client_worldmapoverlay;
insert into client_uimapxmapart select * from r.client_uimapxmapart;
insert into client_uimapart select * from r.client_uimapart;
insert into client_uimapartstylelayer select * from r.client_uimapartstylelayer;
insert into client_uimapassignment select * from r.client_uimapassignment;
insert into client_taxinodes select * from r.client_taxinodes;
insert into client_taxipath select * from r.client_taxipath;
insert into client_taxipathnode select * from r.client_taxipathnode;
insert into m_player_stats select * from r.m_player_stats;
insert into m_class_spell select * from r.m_class_spell;
insert into m_class_requirement select * from r.m_class_requirement;
insert into m_class_requirement_quest select * from r.m_class_requirement_quest;
insert into client_spellname select * from r.client_spellname where ID in (select spell_id from m_class_spell union select reward_spell from r.m_quest where reward_spell > 0 union select SpellID from r.client_talent union select value from json_each('[$spells]'));
insert into client_talent select * from r.client_talent;
insert into m_npc select * from r.m_npc where id in ($npcs);
insert into m_spawn select * from r.m_spawn where entity_type = 'npc' and entity_id in ($npcs);
detach r;
update m_npc set sources = 'fixture';
update m_spawn set sources = 'fixture';
vacuum;
SQL
    zstd -19 -q -f "$tmp/base.sqlite" -o "$out/base-$edition.sqlite.zst"
    rm -r "$tmp"
done
ls -l "$out"
