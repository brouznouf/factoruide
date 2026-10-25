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

Duplicate prerequisite links from different sources count once. Race and class alternatives
keep eligible branches when any exist. Edition-specific `quests.toml` can replace `pre_all`
or `pre_any`; these corrections apply whenever a database is loaded, including after a refresh.

Must-have quests (`PlanRequest.required_quests`, CLI `--required-quest`) resolve by ID or exact
name for the character or group. Their prerequisites stay in the model outside the usual level
window; unsupported or conflicting choices are reported. Required quests keep a route running
past its target level until they are complete. Generated guides and saved edits check completion.

`world.rs` gives the geography: zone coordinates converted to world coordinates, the passes
between zones, boats, zeppelins and tram (`overrides/`), and the flight path network.

A guide can start from a character met in game (`job/start.rs`): the profile the addon saves
(`Profile.lua`, read by the app from the SavedVariables) becomes the simulation's initial state:
quests turned in, position, hearthstone, known flight paths, XP and rested XP, gear worn, last
training (from the spells learned). Profession curves go through the character's skill. The
quests of its log count as never taken: the optimizer takes again those it wants, the export
abandons the others as the guide's first steps.

`xp.rs` holds the rules of each game version (WoW Forever, Classic Era, TBC): XP per level,
quest XP penalty by level difference, mob XP, max level, quest log size, mount level.

## 2. The simulation

A route is an ordered list of **stops**: accept a quest, complete one of its objectives, turn it
in, or a visit (learn a flight path, bind the hearthstone at an inn). `Planner::simulate`
(`plan.rs`) replays a route as a playthrough and returns the time it takes to reach the target
level:

- **Travel**: walking in a straight line within a zone, through the passes between zones; flight
  paths (once learned) and transports when they are faster, a flight to a boat or from where
  it lands included; the hearthstone when it is the fastest way back. Walks jump down cliffs
  up to a height that follows the progression profile (15 yards: no fall damage; 30 for
  "normal", 50 for "risky"); a fall that hurts costs the time to rest the health back
  (`DROP_COST` in `terrain/mod.rs`, a full heal a minute), so the walk goes round when the way
  down saves less than that.
- **Fights and loot**: kill time grows with the level difference and shrinks with the
  character's **power** (below); elites are slower (or need a group); items to loot take as many
  kills as their drop rate requires.
- **XP**: quest XP with the level penalty, mob XP with the classic formula (`5 × level + 45`,
  adjusted by the level difference). Level-ups change everything that follows: which quests are
  worth it, how fast mobs die, how much XP they give.
- **On the way**: class training when the new spells are worth it, the loot sold at a vendor nearby
  once the bags are half full, flight paths picked up nearby,
  dungeon runs, the normal mobs met on foot (`farm_on_way`, mobs near the character's level, up to
  `farm_cap` of each level: 20% on Classic and TBC, 10% on Forever, which has more quests).
  `farm_share` (80%) of the mobs met are killed, fewer on a crowded server (the other players
  take their share, as at an objective's spot); the mobs of the log's quests are left to their
  objectives, which count them where they are done.
  Professions add no time: only their quests are planned, once the skill curve reaches them
  (optional in the guide). Their reward is the profession's, not XP: they are kept once gray,
  and each one left out costs `profession_quest_weight`.
- **Every action is a step**: each take, objective and turn-in of a quest is a stop of the route,
  done at that stop and nowhere else (no quest taken in passing at another stop, no objective
  finished on the way), so that each one can be moved or removed in the guide editor. What doing
  things together saves is counted between consecutive stops: a gathering stop right before or
  after a kill stop at the same place is partly done during the kills (`gather_overlap`,
  `plan::state::Together`), and an objective with several places is done at the one on the way
  to the route's next stop (`Planner::spot`).
- **Rules of the game**: prerequisites, exclusive quests and the quest log size are respected; a
  route breaking them is invalid.
- **When quests run out**, the simulation grinds mobs of the character's level up to the next
  step. Grinding gives at most `grind_cap` of each level (10%, apart from farming on the way);
  beyond, it weighs `grind_over_weight` times its duration, so quests are taken first.
- **Guide export**: each checkpoint gets a step to grind when behind the route, every other level
  one that lets the character be 10% of a level behind (at the last place of the level where its
  mobs live), and grinds are placed at the last step where mobs of the character's level live,
  with an XP target.

### Power

How fast the character fights depends on its **power**, counted in levels: `level + gear +
spells` (`power.rs`). Each level of power above the level makes every fight 10% faster (0.9 of the
time, what a mob one level lower takes). The level difference with the mob keeps its weight in the
kill time, and XP and the moment to turn a quest in follow the real level.

**What the character takes on** compares the power an objective requires with the character's
reach (`Params::reach`, `Planner::objective_need`):

- required power: the quest's level for anything done among its mobs (kills, loot, objects,
  exploring, escorts; talking and delivering require nothing, nor an object used right beside the
  NPC giving or taking back the quest with no elite around, a campfire to sit at in a camp), and the mobs' level: one less for a
  lone target (one or two kills), two more for an elite, times `1 + elite_pull_power` per extra
  elite expected in each pull. The pull is measured from the spawns: the elites within aggro range
  of each spawn of the target (a lone elite like Hogger counts 1, a camp of ogres more, capped at 3).
  Normal mobs farmed at a spot require `pull_levels` more per extra hostile mob within 8 yards of
  each of their spawns (orcs standing in pairs: one level more);
- reach: `(level + gear, at most +0.5) × factor + margin`, set by the **progression profile**:
  cautious (+1.5, packs of mobs requiring one level more), the hardcore survival style, takes
  yellow quests and lone targets up to two levels above but packs at most one, normal (+2) goes
  up to the yellow content two levels above and never orange, risky (+3) takes orange quests and
  borderline elites. The factor stays at 1: the game's colors are a number of levels, whatever
  the level.

**Spots** are measured once, when the model is built: the spawn points of an objective's mobs (or
objects) within 200 yards of its place, a point shared by mobs taking turns counted once, and the
mobs per pull. An objective asking for more than its spot holds waits for respawns
(`respawn_time`), and on a crowded server (`crowded`, 0 to 1) up to `crowd_players` other players
share the spot, fewer as players spread out with the levels (`crowd_by_level`: all of it up to
level 10, then 90, 80, 60 and 50 % from level 40): Blackrock Menace's 8 orcs for ~22 kills wait
4 minutes alone, about 36 at a launch.

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
- **Required class quests**: those of `overrides/class_quests.toml` and every class quest the data
  says teaches a spell (`m_quest.reward_spell`), by name; the app lists them, the player can take
  some out. Where quests of a name teach a spell, the spell is what is required: those quests and
  the steps of the name leading to them, not a later quest of the same name (the orc's second
  Vile Familiars, in Durotar, is not the undead warlock's). A quest the route takes and gives up
  costs it `abandon_weight`.
- **Class quests** giving a pet or a form (`overrides/class_quests.toml`) add levels of power
  too (a 1.4 times faster fight is 3.2 levels), and count like an upgrade when choosing quests;
  the quest teaching that spell gives it, whatever its name (Piercing the Veil, the undead's
  Imp, like Vile Familiars).
- The data (item stats, quest rewards, boss loot, trainer spells, base stats by level) comes from
  VMaNGOS (Classic, Forever). Without it (TBC), power is the level and training follows
  `train_every`.

Some content is set apart: a quest filed under a dungeon, or whose mobs, objects, giver or turn-in
live only inside one, is done in a run of that dungeon (with dungeons on); in the guide, the run
is a step that takes the player in, followed by a step per quest done inside (`inside`, `di` in
the addon: the run step is then done on entering). Battleground quests
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

### The score

The guide shown is the route with the lowest score: its play time plus these terms
(`Planner::weights`, `Planner::finish`; every value is a setting, most in the app's advanced
settings). They are choices about what a good guide is, not measurements: the play time reported
stays the real one.

| Term | Default | Why |
| --- | --- | --- |
| Grinding: (`grind_weight` − 1) × its time | 1.5 | Most players prefer quests to grinding: a route grinds only where no quest is nearly as fast. At 1 the route grinds wherever it saves a minute; much higher, it makes long trips for one small quest instead. |
| Farming on the way: the same weight | 1.5 | Mobs killed while walking are grinding too, only cheaper (no trip): weighed the same so that they never win over a quest. |
| Grinding beyond the cap: (`grind_over_weight` − `grind_weight`) × its time | 4, cap `grind_cap` 10% of each level's XP | Grinding is allowed but limited: past a tenth of a level, quests first whenever there are any. With the cap and farming on the way (`farm_cap`, its own 10%), a 1-60 route went from about 26 h of pure grinding to under 7 h, and was faster overall. |
| Gold spent on flights: `gold_weight` × gold, divided by (level / 30)³ above level 30 | 3600 s per gold up to level 30, 1520 at 40, 450 at 60 | Flights cost money a leveling character lacks, above all from 10 to 30, when the trainers take most of it: an hour per gold makes such a route walk or reorder when a flight saves only minutes. Above 30, quests and loot pay more and more, so the same gold is fewer minutes of questing: a 10 silver flight weighs 6 minutes up to 30, 2.5 at 40, under 1 at 60. `flight_weight` (1) is left at the flight's real time: raising it only turned flights into walks. |
| Loot left behind with full bags: `bag_weight` per item | 20 s | The bags hold few slots at first, more as bags get bigger (`bag_slots`: the backpack's 16 at level 1, four bags of 6 to 14 slots up to 72 at 60), less what is always carried (4 slots), the class's own (a hunter's quiver and a warlock's soul pouch take a bag, a shaman's totems and a rogue's poisons a few slots) and `gather_share` (10%) for each gathering profession; a kill's loot takes `loot_per_kill` (0.35) slots and each quest item one until its turn-in. Loot that no longer fits is money and time lost: routes by vendors, where it is sold once the bags are half full (`sell_radius`), keep room without any visit forced. |
| Changing zone: `zone_change_weight` per change | 240 s | Finding one's way and the NPCs of another zone: a few quests far away are not worth a detour. |
| Quest taken and given up: `abandon_weight` each | 600 s | Taking a quest, killing for one of its objectives and dropping it hours later is grinding in disguise. Finishing it or leaving it out wins unless keeping it saves more than ten minutes. |
| Profession quest left out: `profession_quest_weight` each | 300 s | Only with professions leveled, for quests whose skill the curve reaches by the target level, once for a group of alternatives: Camping 101 at a trainer the route passes by is planned even when gray and worth no XP, without a long detour for it. |
| Time outside the preferred zones: `preferred_zone_weight` × its time | 0.5 | Only with preferred zones: the guide stays in them unless leaving saves a lot. |
| Missing goal: `mandatory_penalty` each | 7200 s | A required class quest left out or a forced dungeon not run: kept in the route unless impossible. |


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
- move an objective stop where it makes the shortest detour between its quest's take and turn-in
  (between two stops whose way passes by one of its places, or next to a stop done there);
- move a block of consecutive stops, or a whole stay in a zone, earlier or later;
- drop a quest or a visit (class quests stay);
- insert a quest not yet in the route next to the stops closest to its places;
- add a detour to a flight master (before the route first passes close enough to pick it up), or
  to an inn to bind the hearthstone;
- remove a set of quests and put each back where the character has the right level for it
  ("ruin and recreate").

Only the part of the route from the change on is simulated again. A better route is kept; a worse
one is sometimes accepted early on (simulated annealing, late acceptance), because moving a whole
zone earlier only pays once the rest of the route follows. The changes that succeed more often
are tried more often. At the end, quests taken but never turned in are removed when that costs
nothing. Last, on the route kept (and on a guide saved from the editor), objective stops done at
the same place are brought next to each other when that makes the route no worse
(`plan/together.rs`): the addon shows the steps at one place together, so that the player does
them at once.

### Tournament

`job::optimize` runs several searches at once (`threads`, 8, whatever the processors), as a
tournament counted in tries, not in time: the same request gives the same guide on any machine,
only slower with fewer processors. The tries of each search are per stop of its route (a 1-20
needs fewer than a 1-60), times `effort` (the calculation presets). First a race: many candidate
routes, each with its own random seed and construction settings, get a short search. Then all the
searches improve the best one: they meet every `share_every` tries and those behind go on from the
best of them (`plan/share.rs`). Rounds can come in between (`round_tries`, off by default): the
best `threads` go on, one search each, and after each round the better half goes on with twice the
searches each, down to the `final_routes` of the final. The improvement slows down like a
logarithm of the tries, and the leader of the race won every time: the tries count on the route
that wins. A longer calculation gives a shorter guide.

## 4. The guide

`export/` turns the best route into steps: a guide for the app (JSON) and a route for the addon
(Lua). The app shows the route improving live while the search runs. On WoW Forever the guides can
also go to Polestar as a guide pack (`export/polestar/`, its pack format 3, Polestar 0.2.5; `source.rs` writes the same steps as Polestar's own `.guide` source, which the app and `fg polestar --source` write into a pack folder, `job::add_polestar_source`): a goal per step,
the goals at one NPC in one step, and travels and notes in the step of the goal after them, since
Polestar counts a travel done once a later goal of its step is (place names differ by language).

A saved guide has no language. Each step keeps its sentences as phrases: a template key
(`accept`, `objective.kill`, `grind.xp`...) and its arguments (quests, NPCs, objects, items and
zones by ID with their English name, numbers, professions). `render` writes them in one of the
ten game languages (`templates.rs`), with the names of the database's translations (English where
one is missing): the app in its own language, the addon in the one chosen on its page (each guide
computed once). Objectives in English keep the game's own text.

A guide keeps the route it was written from (`PlanOutcome::plan`: its stops by game IDs). The
**guide editor** (`job/edit.rs`, `plan/edit.rs`) changes that list by hand: steps moved, removed or
added. Each step does one thing: a stop, or one of the things the simulation does by itself on
the way (a flight, boats, a class teleport, training, a flight path learned in passing, a quest
given up, grinding up to a level), which `Planner::expand` turns into steps of their own; a number
of mobs to kill where the character stands, and every mob on the way to an objective, can be added
too. The editor's planner (`Planner::explicit`) does nothing the steps do not say, and checks
that each step did only its own action. `Planner::edit` replays the steps like `simulate_full`, the same route giving the
same time, but checks each step instead of giving up on the first impossible one: an impossible step is reported with the reason and left out (the
guide cannot be saved), a step with nothing left to do (done before) is skipped. The edited guide is then written from that replay like any other.
A guide created by hand (`job::manual`) is the editor's save of no step at all, from the
character and settings generation would start from (`prepare`): the grind to the target level,
until steps are added.

A guide's page plays it back (`job/timeline.rs`, `guide_timeline` in the app): its steps are
replayed as the editor replays them (`Planner::edit`, the character kept after each step), the
guide is written again from the replay and tied step by step to the guide as saved (the same
steps, or the same in another order; a step the replay does not find, the data having changed
since, gets a straight walk to it), and the replay is cut at the event of each step: its time
split into travel, combat, interaction and waiting, the character's moves (walks around the
terrain, simplified; flights along their taxi paths; boats; the hearthstone and teleports as
jumps) and what changes (quests taken and turned in, objectives done, flight paths learned, the
inn bound, the hearthstone used). The guide's own times are kept: the replay only fills them in.
The app (`GuideViewer`) rebuilds the character at any moment from those changes.

The steps are edited on that page too (`job/draft.rs`): each change asks for a replay of the
draft in the background, a newer request making the one under way give up between steps
(`job::Requests`, checked by `Planner::edit_observed` and the walks' search), which answers
with a line per step and their timeline. `Editor::suggest` tells what could be added after a
step of the draft: the quests it never takes that the character can take there, and the
objectives and turn-ins of the quests in its log that it forgets.

A guide file carries its format (`format.rs`). Reading an older one migrates it one format after
the other; a migration that cannot carry a guide over refuses it (to compute again), and a guide
of a newer format asks to update the app. Format 1 (before phrases) keeps its texts as they were
written.
