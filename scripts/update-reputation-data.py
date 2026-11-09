#!/usr/bin/env python3
"""Normalize the reviewed public-data audits into the shipped reputation catalogs.

No database, account data, or game profile is read or modified. Inputs are the two
SHA-256-pinned audit JSON files named below; use --check for a canonical-LF check.
--verify-upstream also checks the pinned public Forever caches independently.
"""

import argparse
import collections
import hashlib
import json
from pathlib import Path
import sqlite3
import urllib.request


AUDITS = {
    "classic-estimate-kill-spillover-catalog.json": "3f3c3c43a68d070aad91d8158269af405d04522fd889a46c6d3e227e0319ca54",
    "repeatable-verified-records.json": "9f37f76d6438efb5b5763abcb94bc95a7cb15d32348db5976b23da4bc15d91ea",
}
EG_REVISION = "45f58b070b84501f8001bdcad43864aa929a6a4b"
EG_BASE = f"https://raw.githubusercontent.com/JIVESCORP/eg-link-output-wowf/{EG_REVISION}"
EG_HASHES = {
    70790: "1d338604744f0100ec58ea14c904f1b1c676b826d832bc747c90eae27b569bc7",
    70840: "1a8e5263510de3d9ec98f99dfda56653b1bbe30a77f97268740b28ac466016fd",
}
FLAG_ENUM_REVISION = "76de08086fb77150ab2ee4fc65ac9c832e2c353e"
FLAG_ENUM_URL = f"https://raw.githubusercontent.com/TrinityCore/TrinityCore/{FLAG_ENUM_REVISION}/src/server/game/Quests/QuestDef.h"
FLAG_ENUM_SHA256 = "5ec7f028c9828c235dff8c6c4f3741de0400692e8c6029c0304b0563efab804a"


def cache_url(build):
    return f"{EG_BASE}/data/wowf/1.60.1.{build}/quests.enUS.json"


def read_audit(directory, name):
    raw = (directory / name).read_bytes()
    actual = hashlib.sha256(raw).hexdigest()
    if actual != AUDITS[name]:
        raise ValueError(f"{name}: expected audit SHA-256 {AUDITS[name]}, got {actual}")
    return json.loads(raw)


def compact_json(value):
    """Keep one quantitative record per line without duplicating a raw schema."""
    dump = lambda item: json.dumps(item, ensure_ascii=False, separators=(",", ":"), allow_nan=False)
    lines = []
    for key, item in value.items():
        if isinstance(item, list) and item:
            encoded = "[\n    " + ",\n    ".join(dump(row) for row in item) + "\n  ]"
        else:
            encoded = dump(item)
        lines.append("  " + dump(key) + ": " + encoded)
    return ("{\n" + ",\n".join(lines) + "\n}\n").encode("utf-8")


def classic_catalog(audit):
    source = dict(audit["metadata"]["source"])
    # The archive/member hashes identify the public input; a private local path
    # is unnecessary for provenance or runtime binding.
    source.pop("local_database", None)
    provenance = {
        "audit_sha256": AUDITS["classic-estimate-kill-spillover-catalog.json"],
        "source": source,
        "candidate_classification_database": {
            "edition": "forever",
            "sha256": audit["metadata"]["location_source"]["sha256"],
        },
        "selected_factions": audit["metadata"]["source_factions_including_parents_and_siblings"],
    }
    factions = []
    for row in audit["faction_metadata_classic_client"]:
        factions.append({
            "faction": row["ID"],
            "parent": row["ParentFactionID"],
            "reputation_index": row["ReputationIndex"],
            "parent_mod": [row["ParentFactionMod_0"], row["ParentFactionMod_1"]],
            "parent_cap": [row["ParentFactionCap_0"], row["ParentFactionCap_1"]],
        })
    candidates = set(audit["farming_candidate_ids"])
    kills = []
    for row in sorted(audit["kill_records"], key=lambda item: item["creature_id"]):
        kills.append({
            "source_id": f"classic:kill:{row['creature_id']}",
            "creature_id": row["creature_id"],
            "selected_patch": row["raw"]["patch"],
            "team_dependent": bool(row["raw"]["TeamDependent"]),
            "awards": row["awards"],
            "outdoor_farming_candidate": row["creature_id"] in candidates,
        })
    return {
        "schema_version": 1,
        "edition": "classic",
        "confidence": "ClassicBaseline",
        "forever_confidence": "ClassicEstimate",
        "reward_encoding": "primary_slots",
        "provenance": provenance,
        "rules": audit["rules"],
        "faction_metadata": factions,
        "faction_reward_rate_overrides": audit["faction_reward_rate_overrides"],
        "spillover_templates": [
            {"faction": row["faction"], "effects": row["effects"]}
            for row in audit["spillover_templates"]
        ],
        "kill_records": kills,
    }


def item_requirements(row):
    return [{"item": item["item"], "amount": item["amount"]} for item in row["required_items"]]


def gate(value):
    return None if value[0] is None else value


def forever_catalog(audit):
    daily = []
    for row in sorted(audit["daily_records"], key=lambda item: item["quest"]):
        starts = [relation["entity_id"] for relation in row["turn_in_relations"]
                  if relation["role"] == "start" and relation["entity_type"] == "item"]
        assert len(starts) == 1
        daily.append({
            "source_id": f"forever:daily:{row['quest']}",
            "quest": row["quest"],
            "confidence": "PrimaryCache",
            "primary_cache_build": row["primary_cache_build"],
            "flags": row["flags"],
            "flags_ex": row["flags_ex"],
            "flags_ex2": row["flags_ex2"],
            "schedule": "daily",
            "max_turn_ins_per_reset": row["max_turn_ins_per_reset"],
            "min_level": row["min_level"],
            "required_items": item_requirements(row),
            "starting_item_id": starts[0],
            "reward": row["reward"],
            "reward_override_hundredths": [[faction, int(value * 100)] for faction, value in row["reward"]],
            "reputation_cap": row["reputation_cap"],
            "time_limit_seconds_in_cache": row["time_limit_seconds_in_cache"],
            "audit_known_ender": row["known_ender"],
            "audit_automatic_acquisition_supported": row["automatic_acquisition_supported"],
        })
    imported = []
    for row in sorted(audit["imported_repeatable_examples"], key=lambda item: item["quest"]):
        conditions = []
        if row["quest"] in (5402, 5403, 5404):
            conditions.append("argent_dawn_commission_and_conditional_loot_not_modeled")
        if row["quest"] == 3804:
            conditions.append("morrowgrain_container_production_cost_not_modeled")
        if row["quest"] in (9259, 9267, 9268):
            conditions.append("below_neutral_repair_only_potential_secondary_losses_unverified")
        imported.append({
            "source_id": f"forever:repeatable:{row['quest']}",
            "quest": row["quest"],
            "confidence": "ImportedEstimate",
            "special_flags": row["local_special_flags"],
            "flags": row["local_quest_flags"],
            "schedule": "repeatable",
            "live_repeat_observed": row["primary_observed_repeat_offer"],
            "min_level": row["min_level"],
            "required_items": item_requirements(row),
            "reward": row["reward"],
            "min_rep_gate": gate(row["min_rep_gate"]),
            "max_rep_gate": gate(row["max_rep_gate"]),
            "prerequisites": {
                "all": sorted(link["other_id"] for link in row["links"] if link["kind"] == "pre_all"),
                "any": sorted(link["other_id"] for link in row["links"] if link["kind"] == "pre_any"),
            },
            "acquisition_conditions": conditions,
            "reputation_cap": None,
        })
    corrections = [{
        "quest": row["quest"],
        "field": row["field"],
        "local_reward": row["local"],
        "reward": row["primary_verified"],
        "primary_cache_build": 70840,
        "reward_encoding": "resolved_vector",
    } for row in audit["source_corrections"]]
    return {
        "schema_version": 1,
        "edition": "forever",
        "as_of": audit["as_of"],
        "reward_encoding": "resolved_vector",
        "classic_fallback": {
            "catalog": "classic.json",
            "confidence": "ClassicEstimate",
            "enabled_by_default": True,
            "warning": "Classic VMaNGOS kill, cap, rounding and spillover estimates; Forever server behavior may differ.",
        },
        "provenance": {
            "audit_sha256": AUDITS["repeatable-verified-records.json"],
            "attribution": audit["attribution"],
            "revision": EG_REVISION,
            "datasets_specification_url": f"{EG_BASE}/DATASETS.md",
            "primary_caches": [{"build": build, "url": cache_url(build), "sha256": EG_HASHES[build]}
                               for build in sorted(EG_HASHES)],
            "technical_daily_flag_enum_url": FLAG_ENUM_URL,
            "technical_daily_flag_enum_sha256": FLAG_ENUM_SHA256,
            "imported_database_confidence": "Merged database without field-level winning-source provenance; repeatability is special_flags & 1, not a verified live Forever repeat.",
            "corroboration_urls": [
                f"https://www.wowhead.com/forever/quest={quest}" for quest in (7819, 7825, 7832, 7837, 5402)
            ],
        },
        "daily_policy": {
            "daily_flag_mask": 4096,
            "global_daily_limit": None,
            "server_reset_time": None,
            "observed_second_reset_completion": False,
            "unknown_cap_is_unbounded": False,
            "consumption_observed": False,
            "audit_starting_item_acquisition_supported": False,
            "required_items_policy": "Reserve both the starting writ and every other required item per completion; item consumption by the trade script was not observed.",
            "daily_circuit_policy": "Each eligible daily quest once per reset, then repeat the circuit after a reset; first availability is unknown without live state.",
        },
        "daily_summary": audit["daily_summary"],
        "daily_records": daily,
        "imported_repeatables": imported,
        "one_time_records": [{
            "source_id": "forever:quest:98248",
            "quest": 98248,
            "confidence": "PrimaryCache",
            "primary_cache_build": 70840,
            "flags": 36700160,
            "schedule": "once",
            "min_level": 10,
            "required_items": [{"item": 280180, "amount": 1}],
            "reward": [[2587, 2000]],
            "reward_override_hundredths": [[2587, 200000]],
            "currency_reward": [[3402, 50]],
            "audit_automatic_acquisition_supported": False,
        }],
        "excluded_quests": [{"quest": 93862, "reason": "no_known_giver_no_primary_response_no_verified_repeat_schedule"}],
        "source_corrections": corrections,
        "limitations": audit["limitations"],
    }


def validate(classic, forever, classic_audit, repeatable_audit):
    kills = classic["kill_records"]
    assert len(kills) == len({row["creature_id"] for row in kills}) == 352
    assert len(classic["spillover_templates"]) == 7
    assert sum(row["outdoor_farming_candidate"] for row in kills) == 93
    assert {row["creature_id"] for row in kills if row["outdoor_farming_candidate"]} == set(classic_audit["farming_candidate_ids"])
    assert sum(len(row["awards"]) for row in kills) == 429
    assert sum(award["base_value"] < 0 for row in kills for award in row["awards"]) == 99
    assert collections.Counter(row["selected_patch"] for row in kills) == {0: 345, 2: 1, 3: 1, 6: 1, 9: 4}
    for row, original in zip(classic["spillover_templates"], classic_audit["spillover_templates"]):
        raw = original["raw"]
        assert row["faction"] == raw["faction"]
        assert row["effects"] == [
            {"faction": raw[f"faction{slot}"], "rate": raw[f"rate_{slot}"],
             "max_standing_rank_inclusive": raw[f"rank_{slot}"]}
            for slot in range(1, 5) if raw[f"faction{slot}"]
        ]
    by_creature = {row["creature_id"]: row for row in classic_audit["kill_records"]}
    for row in kills:
        source = by_creature[row["creature_id"]]
        assert row["selected_patch"] == source["raw"]["patch"] <= 10
        assert row["awards"] == source["awards"]
        assert row["team_dependent"] == bool(source["raw"]["TeamDependent"])
        for award in row["awards"]:
            slot = award["slot"]
            assert award["faction"] == source["raw"][f"RewOnKillRepFaction{slot}"]
            assert award["base_value"] == source["raw"][f"RewOnKillRepValue{slot}"]
            assert award["max_standing_rank_inclusive"] == source["raw"][f"MaxStanding{slot}"]
            assert award["is_team_award"] == bool(source["raw"][f"IsTeamAward{slot}"])
    daily = forever["daily_records"]
    assert len(daily) == len({row["quest"] for row in daily}) == 150
    assert collections.Counter(dict(row["reward"])[2587] for row in daily) == {75: 45, 125: 60, 200: 45}
    assert sum(dict(row["reward"])[2587] for row in daily) == 19875
    assert collections.Counter(row["primary_cache_build"] for row in daily) == {70840: 105, 70790: 45}
    assert sum(row["audit_known_ender"] for row in daily) == 24
    assert not any(row["audit_automatic_acquisition_supported"] for row in daily)
    by_quest = {row["quest"]: row for row in repeatable_audit["daily_records"]}
    for row in daily:
        source = by_quest[row["quest"]]
        assert (row["flags"], row["flags_ex"], row["flags_ex2"]) == (3149824, 33281, 268435456)
        assert row["flags"] & 4096 and row["max_turn_ins_per_reset"] == 1
        assert row["reward"] == source["reward"]
        assert row["required_items"] == item_requirements(source)
        assert row["starting_item_id"] in [item["item"] for item in row["required_items"]]
        assert len(row["required_items"]) == 2
        assert not forever["daily_policy"]["audit_starting_item_acquisition_supported"]
        starter = next(item for item in source["required_items"] if item["item"] == row["starting_item_id"])
        assert not starter["sources_by_kind"] and not starter["vendors"]
        assert not starter["acquisition_supported_by_local_data"]
    assert {row["quest"] for row in forever["imported_repeatables"]} == {
        822, 1878, 3804, 5402, 5403, 5404, 7819, 7825, 7832, 7837, 9259, 9267, 9268,
    }
    correction = forever["source_corrections"][0]
    assert correction["quest"] == 94214 and correction["reward"] == [[2586, 75], [2587, 75]]
    assert forever["one_time_records"][0]["schedule"] == "once"


def verify_upstream(forever):
    caches = {}
    for build in sorted(EG_HASHES):
        with urllib.request.urlopen(cache_url(build), timeout=40) as response:
            raw = response.read()
        actual = hashlib.sha256(raw).hexdigest()
        if actual != EG_HASHES[build]:
            raise ValueError(f"build {build}: upstream SHA-256 mismatch: {actual}")
        caches[build] = json.loads(raw)["quests"]
    for row in forever["daily_records"] + forever["one_time_records"]:
        build = row["primary_cache_build"]
        source = caches[build][str(row["quest"])]
        # Keep the newest selected cache for each quest, never sum cache versions.
        assert not any(str(row["quest"]) in cache for newer, cache in caches.items() if newer > build)
        assert row["flags"] == source["flags"]
        for catalog_key, source_key in (("flags_ex", "flagsEx"), ("flags_ex2", "flagsEx2")):
            if catalog_key in row:
                assert row[catalog_key] == source[source_key]
        assert row["min_level"] == source["minLevel"]
        items = [{"item": item["objectId"], "amount": item["amount"]}
                 for item in source["objectives"] if item["type"] == "item"]
        assert row["required_items"] == items
        overrides = [[award["factionId"], award["override"]] for award in source["rewards"]["reputation"]]
        assert row["reward_override_hundredths"] == overrides
        assert row["reward"] == [[faction, value / 100] for faction, value in overrides]


def verify_classic_db(path, audit):
    expected = audit["metadata"]["source"]["database_sha256"]
    with path.open("rb") as source:
        actual = hashlib.file_digest(source, "sha256").hexdigest()
    if actual != expected:
        raise ValueError(f"Classic database: expected SHA-256 {expected}, got {actual}")
    factions = audit["metadata"]["source_factions_including_parents_and_siblings"]
    placeholders = ",".join("?" for _ in factions)
    sql = f"""SELECT * FROM creature_onkill_reputation AS r
        WHERE r.patch = (SELECT MAX(p.patch) FROM creature_onkill_reputation AS p
                         WHERE p.creature_id = r.creature_id AND p.patch <= 10)
          AND (r.RewOnKillRepFaction1 IN ({placeholders})
               OR r.RewOnKillRepFaction2 IN ({placeholders}))
        ORDER BY r.creature_id"""
    with sqlite3.connect(path.resolve().as_uri() + "?mode=ro", uri=True) as db:
        db.row_factory = sqlite3.Row
        selected = [dict(row) for row in db.execute(sql, factions + factions)]
        assert selected == [row["raw"] for row in sorted(audit["kill_records"], key=lambda row: row["creature_id"])]
        assert len(selected) == 352


def profile_daily_ids(path, forever, check):
    """Keep the profile's queried daily IDs in sync without embedding quest prose."""
    import re

    ids = sorted(row["quest"] for row in forever["daily_records"])
    content = path.read_text(encoding="utf-8")
    pattern = re.compile(r"(local DAILY_QUESTS = \{ -- DAILY_QUEST_IDS\n)(.*?)(\n\})", re.S)
    match = pattern.search(content)
    if not match:
        raise ValueError(f"{path}: missing daily quest ID markers")
    if check:
        if [int(value) for value in re.findall(r"\d+", match.group(2))] != ids:
            raise ValueError(f"{path}: daily IDs differ from the catalog")
    else:
        lines = "\n".join(f"    {quest}," for quest in ids)
        path.write_text(content[:match.start(2)] + lines + content[match.end(2):], encoding="utf-8")
    print(f"Validated {len(ids)} profile daily quest IDs.")


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--audit-dir", type=Path, required=True)
    parser.add_argument("--output-dir", type=Path, default=Path(__file__).resolve().parents[1] / "data" / "reputation")
    parser.add_argument("--check", action="store_true")
    parser.add_argument("--verify-upstream", action="store_true")
    parser.add_argument("--verify-classic-db", type=Path)
    parser.add_argument("--profile-daily-ids", type=Path, help="update/check the profile's numeric daily ID list")
    args = parser.parse_args()
    classic_audit = read_audit(args.audit_dir, "classic-estimate-kill-spillover-catalog.json")
    repeatable_audit = read_audit(args.audit_dir, "repeatable-verified-records.json")
    classic = classic_catalog(classic_audit)
    forever = forever_catalog(repeatable_audit)
    validate(classic, forever, classic_audit, repeatable_audit)
    if args.profile_daily_ids:
        profile_daily_ids(args.profile_daily_ids, forever, args.check)
    if args.verify_upstream:
        verify_upstream(forever)
    if args.verify_classic_db:
        verify_classic_db(args.verify_classic_db, classic_audit)
    for name, catalog in (("classic.json", classic), ("forever.json", forever)):
        path = args.output_dir / name
        raw = compact_json(catalog)
        if args.check:
            # Git's Windows checkout policy may materialize CRLF. No other
            # whitespace or content difference is accepted.
            if path.read_bytes().replace(b"\r\n", b"\n") != raw:
                raise ValueError(f"{path}: catalog differs from deterministic generation")
        else:
            args.output_dir.mkdir(parents=True, exist_ok=True)
            path.write_bytes(raw)
        print(f"{name}: {len(raw)} bytes; SHA-256 {hashlib.sha256(raw).hexdigest()}")
    print("Validated 352 latest-patch kills, 93 outdoor candidates, 429 signed slots, 7 templates, 150 daily writs, 13 imported repeatables, and one-time Shipping Label.")


if __name__ == "__main__":
    main()
