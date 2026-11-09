# Reputation earning catalogs

These catalogs hold quantitative earning rules and their provenance. The selected
edition's database supplies current NPCs, locations, quest relations, prerequisites,
side and class restrictions, material sources, prices and combat suitability.
An earning rule never establishes that its acquisition path is supported.

## Files and confidence

- [Classic catalog](classic.json): 352 latest-applicable-patch creature rules,
  429 signed award slots, 7 explicit spillover templates and 18 faction metadata
  records. `ClassicBaseline` identifies the pinned Classic 1.12 baseline.
- [Forever catalog](forever.json): 150 primary-cache daily writ definitions,
  13 imported repeatable examples, one-time Shipping Label and the quest 94214
  reward correction. `PrimaryCache` verifies the recorded flags and override
  amounts; a second reset's completion has not been observed. `ImportedEstimate`
  identifies repeatability inherited from the merged database's special flags,
  without a verified live Forever repeat or ceiling.
- [Deterministic normalizer](../../scripts/update-reputation-data.py): validates
  pinned audit inputs and generates both JSON files without reading profiles or
  modifying a database. No source SQL, guide prose or player records are included.

Forever's `classic_fallback` defaults to enabled under the accepted estimate
policy. Every applied Classic rule on Forever has confidence `ClassicEstimate`
and requires the catalog's warning in calculation notes and affected instructions.
Disabling that policy does not change the user's faction targets. Prefer verified
edition-specific rules when present.

## Record contract

Both roots have `schema_version: 1`. Source IDs are stable strings:
`classic:kill:<creature>`, `forever:daily:<quest>`,
`forever:repeatable:<quest>` and `forever:quest:<quest>`.
Array rewards are `[faction_id, signed_amount]`; fractional resolved values must
remain fractional. Integer flags preserve the exact source bit fields.

Classic `kill_records` retain the selected patch and ordered award slots. An
award contains its original slot, faction, signed base value, inclusive standing
rank gate, team-award flag and optional side. `team_dependent` preserves the
source switch: when true, slot 1 is Alliance-only and slot 2 is Horde-only.
No current record has that switch set. Do not discard negative slots: 99 of the
429 slots are losses.

`outdoor_farming_candidate` marks the 93 candidates identified by the reviewed
Forever location snapshot. It is a candidate filter, not permission to bypass
current-edition spawn, side, level, difficulty or population checks. Keep all 352
rules available for exact known-creature kill credit, including rules ineligible
for automatic outdoor farming. No unspecified zone grind earns invented credit.

`spillover_templates` are explicit, non-recursive recipient effects. The `rules`
object records the pinned baseline's pre-award inclusive rank check, full final
award across a rank boundary, signed rounding, gray-level formula, default rates
and separate team-half behavior. Truncate spillover and team halves per kill
before batching. Ordinary spillover shares the primary slot's gate; the separate
team award is outside that check and suppresses further spillover. Parent
modifiers and both cap arrays are preserved as metadata; they do not prove an
additional Forever parent-spillover algorithm. There are no relevant faction
reward-rate overrides in this snapshot. Default rates are baseline assumptions.

Forever quest rewards use `reward_encoding: "resolved_vector"`. Apply the vector
once, including negative or fractional components where present, without another
spillover pass. `reward_override_hundredths` preserves the primary cache's exact
integer overrides; `reward` is that amount divided by 100. The raw primary
override is not another reward to apply.

Every daily has `schedule: "daily"` and `max_turn_ins_per_reset: 1` from flag
`0x1000`. Preserve `flags`, `flags_ex` and `flags_ex2`. `starting_item_id` identifies
the required writ; `required_items` includes both it and the crafted item. Reserve
both for every repetition. Actual trade-script consumption was not observed, so
the catalog does not promise which item the script destroys.

The `audit_*` fields describe the audited snapshot, not permanent NPC or source
bindings. Re-query the selected database for actual enders and material paths.
All 150 starting writs lack an item source and vendor; automatic acquisition is
supported for zero writs. Only 24 known enders existed in that snapshot. Existing
inventory can support a delivery only if the items are explicitly present, all
other requirements pass and a real ender is available. Missing inventory means
zero known stock. No free Durotar turn-in or unlimited future writ supply follows
from daily classification.

`null` reputation caps, reset times and global daily limits mean unknown. A null
cap does not mean unlimited reputation. Daily actions form a circuit with each
eligible source once, then a reset before repeating the circuit. Required
completion opportunities and reset transitions are minimum requirements for that
program, not a completion date. First availability needs current server state.
The 45 writs at +75, 60 at +125 and 45 at +200 total 19,875 Durotar reputation per
reset only if every writ, material and delivery can actually be obtained. The
supported automatic rate in the audited data is zero.

Imported `prerequisites.all` and `prerequisites.any` preserve audited quest links;
the runtime must also enforce current database requirements. `min_rep_gate` is
an inclusive lower gate and `max_rep_gate` is an exclusive upper gate, each
`[faction, standing]` or null. The three +499 repair quests require standing below
Neutral; they are not Exalted grinds. Runecloth setup donations are one-time
prerequisites, not costs or rewards to repeat on every delivery. Scourgestone
loot conditions and Morrowgrain production costs need explicit support; their
`acquisition_conditions` prevent generic item-source rows from proving an
unconditional farm.

Shipping Label 98248 appears only in `one_time_records` with `schedule: "once"`.
Its +2000 Durotar reward and 50 Merchant's Favor do not establish a repeat loop.
Crate Return 93862 is excluded for missing giver and repeat evidence. No direct
repeatable Horde-67 earning rule was verified; city rewards or faction metadata
must not manufacture one.

## Maintained correction

`source_corrections` fixes quest 94214's resolved vector from `[[2586,75]]` to
`[[2586,75],[2587,75]]`, matching build 70840's two explicit overrides of 7500.
Apply this correction to ordinary quest effects as well as daily source rules.
It belongs in maintained catalog data; no packed SQLite file is changed.
The documented [database override format](../../overrides/database/quests.toml)
has no reputation-reward field, and the database builder is absent from this
checkout. Consequently no unsupported TOML key is invented. A future database
builder must add documented support before moving this correction into its
standard quest overrides.

## Pinned public sources and attribution

Classic facts come from the [VMaNGOS September 28, 2026 database snapshot](https://github.com/vmangos/core/releases/tag/db_latest),
asset [db-sqlite-4641790.zip](https://github.com/vmangos/core/releases/download/db_latest/db-sqlite-4641790.zip),
using patch 10 (1.12). Select `max(patch) <= 10` independently per creature;
historical versions are not additive. The archive SHA-256 is
`bb2f1dd69171043a52657574f54948fd30941530fdc6eec12bce62c655428501`;
its SQLite member SHA-256 is
`a8ce8ecc8e9c9d00dfe23321d0763cf8aee19677c836f1e1b9acc15fb1fbd5c9`.
Algorithm references are pinned to VMaNGOS commit
`a88734fb88d19f5e8df6fca766729e419cf6bec0` in the JSON, with file hashes.
These quantitative facts are normalized from VMaNGOS, GPL-2.0-or-later, under the
database distribution's [GPL-3.0 terms](../LICENSE); see [source notices](../../NOTICE.md).

Forever cache facts are from Efficient Games (<https://efficient.games>),
[EG Link output](https://github.com/JIVESCORP/eg-link-output-wowf), licensed under
[CC BY 4.0](https://creativecommons.org/licenses/by/4.0/). Changed: selected
quantitative quest fields only, converted explicit reputation hundredths, joined
local availability facts, and normalized into earning records. The
[dataset specification](https://github.com/JIVESCORP/eg-link-output-wowf/blob/45f58b070b84501f8001bdcad43864aa929a6a4b/DATASETS.md)
and caches are pinned to commit `45f58b070b84501f8001bdcad43864aa929a6a4b`.
Build 70840 supplies 105 writ records and build 70790 the other 45, independently
selecting the latest available record per quest. Both source hashes are in
`provenance.primary_caches`. The technical daily-bit reference is pinned and
hashed separately; it decodes actual Forever flags and does not claim that
Forever runs TrinityCore.

The local merged-data facts retain the confidence limitations documented in
each catalog. Wowhead corroboration links establish the selected published
amounts, not live repeatability, post-cap behavior or source availability.

## Reproduction and checks

Python 3.11 or newer is sufficient. Supply the reviewed audit JSON directory;
the normalizer first checks both complete input hashes, then validates counts,
IDs, exact signed slot fields, flags, required starter items and rewards.

```powershell
python scripts/update-reputation-data.py --audit-dir 'path/to/reviewed-audits' --check
```

The two audit inputs are `classic-estimate-kill-spillover-catalog.json` (SHA-256
`3f3c3c43a68d070aad91d8158269af405d04522fd889a46c6d3e227e0319ca54`) and
`repeatable-verified-records.json` (SHA-256
`9f37f76d6438efb5b5763abcb94bc95a7cb15d32348db5976b23da4bc15d91ea`).
They contain reviewed public-source quantitative observations; the generated
catalogs omit their duplicated raw schemas, prose and location rows.

Add `--verify-upstream` to independently download, hash and compare all 150 daily
records and Shipping Label against the pinned public caches. Add
`--verify-classic-db <extracted-mangos.sqlite>` to hash the public archive member,
open it read-only and independently compare the latest-patch SQL selection with
all 352 raw audited rules. Omit `--check` to regenerate the stable canonical-LF
JSON bytes. The check accepts Git's CRLF checkout conversion only; the reported
SHA-256 and byte count refer to canonical LF bytes. Updating upstream evidence
requires reviewing the input/hash changes rather than silently reading a moving
branch. The catalogs together are less than 200,000 bytes.
