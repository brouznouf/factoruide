# How the route is computed

The planner (`crates/fg-route`) answers one question: in which order should this character do
which quests to reach the target level as fast as possible, given the player's choices? It works
in three steps: it **models** what the character can do, **simulates** a route to know how long it
takes, and **searches** for the route with the best simulated time.

## 1. The model

`model.rs` loads from the quest database (`data/<edition>.sqlite.zst`) every quest the character
can do in the game version: its giver and turn-in, its objectives and where to do them (mobs to
kill, items to loot with their drop rates, objects to use), its prerequisites, the quests it
excludes, its level and its XP. The player's choices filter and shape it: excluded zones and
dungeons, forced dungeons, class quests to keep, professions to level.

`world.rs` gives the geography: zone coordinates converted to world coordinates, the passes
between zones, boats, zeppelins and tram (`overrides/`), and the flight path network.

`xp.rs` holds the rules of each game version (WoW Forever, Classic Era, TBC): XP per level,
quest XP penalty by level difference, mob XP, max level, quest log size, mount level.

## 2. The simulation

A route is an ordered list of **stops**: accept a quest, complete one of its objectives, turn it
in, or a visit (learn a flight path, bind the hearthstone at an inn). `Planner::simulate`
(`plan.rs`) replays a route as a playthrough and returns the time it takes to reach the target
level:

- **Travel**: walking in a straight line within a zone, through the passes between zones; flight
  paths (once learned) and transports when they are faster; the hearthstone when it is the
  fastest way back.
- **Fights and loot**: kill time grows with the level difference and shrinks with the
  character's **power** (below); elites are slower (or need a group); items to loot take as many
  kills as their drop rate requires.
- **XP**: quest XP with the level penalty, mob XP with the classic formula (`5 × level + 45`,
  adjusted by the level difference). Level-ups change everything that follows: which quests are
  worth it, how fast mobs die, how much XP they give.
- **On the way**: class training when the new spells are worth it, flight paths picked up nearby,
  dungeon runs, the normal mobs met on foot (`farm_on_way`, mobs near the character's level, up to
  the grinding share of the level), quest objects among another quest's mobs gathered between
  kills (`gather_overlap`). Professions add no time: only their quests are planned, once the
  skill curve reaches them (optional in the guide).
- **Rules of the game**: prerequisites, exclusive quests and the quest log size are respected; a
  route breaking them is invalid.
- **When quests run out**, the simulation grinds mobs of the character's level up to the next
  step. Grinding (mobs on the way included) gives at most `grind_cap` of each level (10%);
  beyond, it weighs `grind_over_weight` times its duration, so quests are taken first.
- **Guide export**: each checkpoint gets a step to grind when behind the route, and grinds are
  placed at the last step where mobs of the character's level live, with an XP target.

### Power

How fast the character fights depends on its **power**, counted in levels: `level + gear +
spells` (`power.rs`). Each level of power above the level makes every fight 10% faster (0.9 of the
time, what a mob one level lower takes). The level difference with the mob keeps its weight in the
kill time, and XP and the moment to turn a quest in follow the real level.

**What the character takes on** compares the power an objective requires with the character's
reach (`Params::reach`, `Planner::objective_need`):

- required power: the quest's level for anything done among its mobs (kills, loot, objects,
  exploring, escorts; talking and delivering require nothing), and the mobs' level: one less for a
  lone target (one or two kills), two more for an elite, times `1 + elite_pull_power` per extra
  elite expected in each pull. The pull is measured from the spawns: the elites within aggro range
  of each spawn of the target (a lone elite like Hogger counts 1, a camp of ogres more, capped at 3);
- reach: `(level + gear, at most +0.5) × factor + margin`, set by the **progression profile**:
  cautious (+1.5, packs of mobs requiring one level more), the hardcore survival style, takes
  yellow quests and lone targets up to two levels above but packs at most one, normal (+2) goes
  up to the yellow content two levels above and never orange, risky (+3) takes orange quests and
  borderline elites. The factor stays at 1: the game's colors are a number of levels, whatever
  the level.

An objective out of reach waits (the simulation grinds when the route goes there anyway), and a
quest whose objectives are far out of reach is not picked up.

- **Gear**: the simulation wears the quest rewards (the best of the choices for the class, which
  the guide then tells the addon to pick) and the dungeon boss loot (each item with the chance the
  character wins it). Items are valued with a small fight model of the class (weapon damage and
  attack power for melee classes, ranged weapons for hunters, spell power, mana and wands for
  casters, health and armor for all), relative to the character's own power at its level. Only
  what beats a floor counts: the gear a character has anyway (vendors, drops), set at
  `gear_floor` × the typical uncommon item of the last levels. A quest whose reward is an upgrade
  is worth more (`gear_value`), so the route favours them, and dungeons.
- **Spells**: `overrides/spells.toml` gives, per class, the share of fighting power each trainer
  spell brings (damage, healing that shortens rests, buffs). A spell not learned loses its share,
  a rank behind loses part of it (rank strength from the game data: damage per cast time,
  healing, buff value). The character trains on the way once the new spells make fights
  `train_min_gain` faster, and makes a trip at `train_trip_gain`; the training steps list the
  spells that count, and the addon buys only those. Spells not listed (and talents) do not count.
- **Class quests** giving a pet or a form (`overrides/class_quests.toml`) add levels of power
  too (a 1.4 times faster fight is 3.2 levels), and count like an upgrade when choosing quests.
- The data (item stats, quest rewards, boss loot, trainer spells, base stats by level) comes from
  VMaNGOS (Classic, Forever). Without it (TBC), power is the level and training follows
  `train_every`.

Some content is set apart: a quest filed under a dungeon, or whose mobs, objects, giver or turn-in
live only inside one, is done in a run of that dungeon (with dungeons on). Battleground quests
(filed under a battleground, asking for its marks of honor, or rewarding a faction that only
battleground quests reward) are left out unless `pvp_quests` is on, each mark or objective then
taking `pvp_mark_time`. Items no source drops but several quests ask for (marks, tokens, crafted
goods) are never given an approximate source.

The **play style** sets the numbers of this model (`params.rs`): time per fight, per loot, per
stop; the progression profile; travel speeds.

The result is a time, plus a score: the optimizer weighs grinding and flights more than their real
time (most players prefer questing to grinding, and flights cost gold), counts a few minutes for
each change of zone (finding one's way in another zone: a few quests far away are not worth it),
and adds a cost for missed goals (a class quest left out, a forced dungeon not run).

A quest gray for the character (no XP) is not taken unless it opens a chain that is not gray, and
one gone gray in the log is given up instead of being turned in many levels later.

## 3. The search

### Construction

A first route is built quickly, in one of two ways:

- **Greedy**: from where the character is, the nearest worthwhile stop next. A quest is worthwhile
  when its XP (plus the chain of quests it unlocks, each follow-up counting less the longer it is
  to do) beats grinding for the same time, trip included.
- **By regions** (`regions.rs`): quests are grouped by quest hub and level band; each group is
  done as a block, and the next group is the one with the best XP per hour, trip included. Quests
  leading to another zone are carried along and finished where the route goes.

### Improvement

The route is then improved by a **local search** (`Planner::improve`) for the time the player
gives to the calculation. At each step a change is tried and the new route is simulated:

- move one stop, mostly near its current place;
- move a block of consecutive stops, or a whole stay in a zone, earlier or later;
- drop a quest or a visit (class quests stay);
- insert a quest not yet in the route next to the stops closest to its places;
- add a detour to a flight master, or to an inn to bind the hearthstone;
- remove a set of quests and put each back where the character has the right level for it
  ("ruin and recreate").

Only the part of the route from the change on is simulated again. A better route is kept; a worse
one is sometimes accepted early on (simulated annealing, late acceptance), because moving a whole
zone earlier only pays once the rest of the route follows. The changes that succeed more often
are tried more often. At the end, quests taken but never turned in are removed when that costs
nothing.

### Parallel race

`job::optimize` runs several searches at once. With a qualification time, it first runs a race:
many candidate routes, each with its own random seed and construction settings, get a short
search; the best ones are then improved for the rest of the time. The best route wins: a longer
calculation gives a shorter guide.

## 4. The guide

`export.rs` turns the best route into steps: a guide for the app (JSON) and a route for the addon
(Lua). The app shows the route improving live while the search runs.
