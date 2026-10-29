//! Tuning knobs of the time model. Defaults can be overridden by `overrides/routing.toml`.

use serde::{Deserialize, Serialize};

/// The planner's settings. New ones are added over time: outside this crate, start from
/// `Params::default()` and set the fields to change.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
#[non_exhaustive]
pub struct Params {
    /// Running speed in yards/second.
    pub run_speed: f64,
    /// Level at which the character gets a mount, and its speed bonus.
    pub mount_level: i64,
    pub mount_bonus: f64,
    /// Flight speed in yards/second.
    pub flight_speed: f64,
    /// Walks are this much longer than the terrain's path (`World::walk_distance_at`): what the
    /// terrain does not model (trees, buildings, mobs run around, mounting). 1: that path alone
    /// already makes the walks of 1-60 guides 1.27 to 1.31 times the straight line (1.30 to
    /// 1.41 before optimizing; Forever and Classic, several races and classes), as much as the
    /// 1.3 of the straight-line model this factor comes from (it was counted twice). And its
    /// 8-direction cells make the walks it bends about 5 % longer than the line they follow,
    /// about what it leaves out.
    pub detour: f64,
    /// Overhead of each stop (talking, looting, reading) in seconds.
    pub stop_overhead: f64,
    /// Overhead of taking a flight (talk to flight master, landing).
    pub flight_overhead: f64,
    /// Time to kill one same-level mob, including finding it and resting, with the floor gear
    /// and every class spell (see `power`; calibrated by hand).
    pub kill_time: f64,
    /// Time per mob when grinding (no quest mob packs, more searching and resting).
    pub grind_kill_time: f64,
    /// QuestXP difficulty column used when a quest's XP is unknown.
    pub default_xp_difficulty: u8,
    pub default_kill_count: f64,
    pub default_item_count: f64,
    pub default_drop_chance: f64,
    /// Quests started by an item dropping less often than this (a treasure map at 0.75 %) are not
    /// planned: farming it is luck, and it is looted anyway when it drops.
    pub min_start_chance: f64,
    /// Time to use/loot one quest object.
    pub object_time: f64,
    /// Quests up to this many levels above the target level stay candidates (picked up on the
    /// way, turned in before the target when worth it). 60: every quest that can be taken before
    /// the target; the power they require and the optimizer leave out what does not pay.
    pub max_quest_above: i64,
    /// How far above its power the character goes (see `Params::reach`).
    pub progression: Progression,
    /// Margin of the progression profile in levels, instead of the profile's own (tuning).
    pub progression_margin: Option<f64>,
    /// Levels of power an objective in a camp of elites requires per extra elite expected in a
    /// pull (1: three elites per pull need two levels more than a lone elite).
    pub elite_pull_levels: f64,
    /// Levels of power normal mobs require per extra mob coming in each pull at their spot (1:
    /// mobs standing in pairs need one level more than mobs standing alone).
    pub pull_levels: f64,
    /// Power a pack (three kills or more) requires on top of a lone target of the same level,
    /// instead of the progression profile's own (tuning).
    pub pack_power: Option<f64>,
    /// Quests the log holds at once (40 on WoW Forever, 20 on Classic Era).
    pub quest_log_size: usize,
    /// Distance at which a flight master is picked up on the way.
    pub flight_learn_radius: f64,
    /// Kill the normal mobs met on every walk, not only those of the log's quests (as guides
    /// ask): those within `farm_corridor` of the path, from `farm_below` levels under the
    /// character to `farm_above` over it (lower ones give little XP, higher ones are slow and
    /// risky). It replaces grinding: limited to `farm_cap` of each level, weighed like grinding.
    /// Of the mobs along the corridor, `farm_share` are killed (some are dead, out of reach or
    /// fighting someone), fewer on a crowded server (see `farm_share_at`). The mobs of the log's
    /// quests are not farmed: their objectives count them where they are done.
    pub farm_on_way: bool,
    pub farm_corridor: f64,
    pub farm_share: f64,
    pub farm_below: i64,
    pub farm_above: i64,
    /// Quests of the route whose NPC is within this distance of a stop are taken and turned in
    /// there, as a player does at a quest hub (0: only at their own stop). Off by default: the
    /// route says every take and turn-in itself, as the guide editor shows them.
    pub camp_radius: f64,
    /// Allow quests whose objectives involve elite mobs.
    pub allow_elite: bool,
    /// Every class quest of the character's class is required, not only the ones that teach a
    /// spell or are listed in class_quests.toml (repeatable and too-high-level ones left out).
    pub all_class_quests: bool,
    /// Battleground (PvP) quests (marks of honor...), each mark or objective taking
    /// `pvp_mark_time` seconds of battleground.
    pub pvp_quests: bool,
    pub pvp_mark_time: f64,
    /// How crowded the server is, from 0 (nobody else) to 1 (a launch): escorts and events wait
    /// up to `crowd_event_time` for their turn, and up to `crowd_players` other players farm the
    /// same mobs and objects, leaving fewer of them (see `respawn_time`). Times
    /// `crowd_by_level` for the quest's level. `true`/`false` (before the scale) read as 1 and 0.
    #[serde(deserialize_with = "crowd_share")]
    pub crowded: f64,
    pub crowd_event_time: f64,
    pub crowd_players: f64,
    /// Share of the crowd by quest level, one value per 10 levels (1-10, 11-20...; the last one
    /// above): players spread out as they level.
    pub crowd_by_level: Vec<f64>,
    /// Seconds before a killed mob or a used object is back, for those whose own time the data
    /// does not know: an objective asking for more than its spot holds waits for them (shared
    /// with the other players of a crowded server).
    pub respawn_time: f64,
    /// How much faster mobs and objects come back with players around (the game shortens
    /// respawns with the population of an area): respawns are divided by 1 + this times the
    /// other players at the spot. 0: fixed respawns, the crowd waits its turn; 1: respawns keep
    /// up with the crowd, each player waits as if alone.
    pub dynamic_respawn: f64,
    /// Without `allow_elite`, elite objectives of at most this many kills are kept anyway: the
    /// lone elites a player kills solo (Hogger, Tharil'zun, Mor'Ladim...). 0 = none.
    pub solo_elite_kills: f64,
    /// Construction plan drawn at random for each candidate of the race (zone_stay,
    /// continent_stay, fit and efficiency weights); off: the values below are used as they are.
    pub construction_random: bool,
    /// Greedy construction: one zone change done otherwise, as [changes, level, zone]: after
    /// that many zone changes, stay in the zone until the level, then go to the zone (empty =
    /// none). The other stops weigh `zone_detour_pull` more meanwhile.
    pub zone_detour: Vec<i64>,
    pub zone_detour_pull: f64,
    /// Race: the first candidates after the first begin in this many of the zones richest in
    /// quests (`Planner::start_zones`) rather than where the character stands; 0 = none.
    pub forced_starts: usize,
    /// Greedy construction: the zone the route begins in (an AreaTable zone): it goes there first,
    /// whatever is nearer (the choice made on the starts' preview, `job::preview_starts`); 0 = the
    /// zone the planner finds best.
    pub start_zone: i64,
    /// Race: candidates built after those above as the best of them with one zone change done
    /// otherwise (`zone_detour`), fewer with a low `effort`.
    pub race_mutants: usize,
    /// Race: only the best this many candidates by their built route are improved (0 = every
    /// one): the ranking of the built routes already tells the winner, building is cheap.
    pub screen: usize,
    /// Greedy construction: weight of the stops on another continent (1 = no preference).
    pub continent_stay: f64,
    /// Greedy construction: weight of the quests of a dungeon close in level (lower = the
    /// dungeon is planned more readily; forced dungeons use 0.25).
    pub dungeon_quest_weight: f64,
    /// Race: routes built, each its own seed and construction plan, and briefly improved
    /// (`qualify_tries`); the best `threads` go on (`threads` or fewer: no race). Fewer with a
    /// low `effort`.
    pub candidates: usize,
    /// Tries of each candidate of the race, per stop of its route (times `effort`)...
    pub qualify_tries: f64,
    /// ...and at least this many (times `effort`): on a short guide, a few tries per stop leave
    /// the routes on the steep start of their improvement, where the race ranks them by chance.
    pub qualify_min_tries: f64,
    /// Tries of each search in a round, per stop of its route (times `effort`). After each round
    /// the better half of the routes goes on, each with twice the searches. 0: no rounds, all the
    /// searches improve the best start (measured better: the leader of the race won every time,
    /// and the tries count on the route that wins).
    pub round_tries: f64,
    /// Routes in the final, and tries of each of its searches per stop (times `effort`).
    pub final_routes: usize,
    pub final_tries: f64,
    /// How much the optimization tries (all the tries above): a longer calculation gives a
    /// shorter guide, the same one on any machine.
    pub effort: f64,
    /// Greedy construction: weight of the stops outside the current zone (1 = nearest stop first,
    /// higher values clear the zone before leaving it).
    pub zone_stay: f64,
    /// Greedy construction: stops left in the current zone (worth doing at this level) from
    /// which `zone_stay` applies in full; with fewer, the weight of leaving fades to 1 (a zone
    /// with nothing left holds nobody). 0 = always in full.
    pub zone_stay_quests: f64,
    /// Objects looting more different items than this (world treasure chests) are not sources
    /// of quest items (0 = every object counts).
    pub max_object_loot: usize,
    /// Items dropped by at least this many mob types are common loot (cloth, gems, potions)...
    pub common_item_sources: usize,
    /// ...gathered on the way or bought, in seconds per item, rather than farmed.
    pub common_item_time: f64,
    /// Use off-road shortcuts (climbs, jumps) declared in `passes.toml`.
    pub shortcuts: bool,
    /// Levels the median hostile mob of a zone may be above the character (the top of its band of
    /// 5 levels) for walks to go into it (0: any zone is walked into). Walks go round the zones
    /// above (Stranglethorn at 17 on the way to Booty Bay's boat), and a walk across one costs
    /// five times more.
    pub zone_gap: f64,
    /// Average wait before boarding a boat, zeppelin or tram (seconds).
    pub link_wait: f64,
    /// Kill time of an elite compared with a normal mob of the same level.
    pub elite_kill_factor: f64,
    /// Keep quests whose objective location is unknown, placing it near the quest giver
    /// (for comparisons on databases with gaps).
    pub approx_sources: bool,
    /// Items no creature, object or vendor gives (made by a profession) that quests ask for:
    /// bought at the auction house (`overrides/routing.toml`).
    pub auction_items: Vec<i64>,
    /// Greedy construction: levels below the character a quest can be without penalty, and
    /// the penalty per level beyond (quests turned in late give less XP).
    pub fit_free_levels: i64,
    pub fit_penalty: f64,
    /// Penalty per level a quest is above the character.
    pub fit_above_penalty: f64,
    /// Greedy construction: how much the quest efficiency (XP/s) sways the choice (0 = off).
    pub efficiency_power: f64,
    /// Local search temperature, as a share of the route time (0 = only improvements).
    pub anneal: f64,
    /// Share of local search moves that ruin and rebuild a part of the route (0 = off).
    pub alns: f64,
    /// Late acceptance history length (0 = only improvements).
    pub lahc: usize,
    /// Stops replayed past a move's last changed stop before its cost is estimated from the
    /// current route (its time at the same XP); only the moves the estimate keeps are replayed
    /// to the end (0 = every move replayed to the end).
    pub eval_window: usize,
    /// Quest sorts (negative zoneOrSort) never planned: holidays, professions, events...
    pub excluded_sorts: Vec<i64>,
    /// Quests never planned (manual exceptions).
    pub excluded_quests: Vec<i64>,
    /// Zones (AreaTable IDs) to favour, and zones never gone to for quest givers, turn-ins or
    /// with the hearthstone.
    pub preferred_zones: Vec<i64>,
    pub excluded_zones: Vec<i64>,
    /// Zones whose every quest is done, worth it or not (those reachable before the target
    /// level, dungeons left to their setting): a quest filed under the zone or one of its
    /// subzones, else given there.
    pub forced_zones: Vec<i64>,
    /// Extra weight of the time spent outside `preferred_zones` (0: they only steer the
    /// construction).
    pub preferred_zone_weight: f64,
    /// Reputations to reach by the end of the guide (none: reputation weighs nothing).
    pub reputation_goals: Vec<ReputationGoal>,
    /// Seconds of the guide each reputation point missing from `reputation_goals` at its end is
    /// worth: quests giving the reputation are planned until the goal is reached.
    pub reputation_weight: f64,
    /// Minimum quest efficiency, relative to grinding, to be worth accepting.
    pub min_efficiency: f64,
    /// Hearthstone cooldown and cast time.
    pub hearth_cooldown: f64,
    pub hearth_cast: f64,
    /// The hearthstone is worth its hour of cooldown only when it saves at least this (seconds).
    pub hearth_min_saving: f64,
    /// Cooldown of the shaman's Astral Recall (a second hearthstone), and the cast of the class
    /// teleports (a mage's "Teleport: Stormwind").
    pub recall_cooldown: f64,
    pub teleport_cast: f64,
    /// Die on purpose to respawn at the nearest spirit healer when that is faster ("death
    /// skips"), up to `DEATH_SKIP_UNTIL` (no resurrection sickness before level 11), and what one
    /// takes (dying, releasing, the spirit healer).
    pub death_skips: bool,
    pub death_skip_time: f64,
    /// New class spells every `train_every` levels; a trainer within `train_radius` is visited on
    /// the way, otherwise a dedicated trip is made once training is `train_max_delay` levels late.
    pub train_every: i64,
    pub train_radius: f64,
    pub train_max_delay: i64,
    pub train_time: f64,
    /// With class spell data: train on the way (trainer within `train_radius`) once the new
    /// spells make fights `train_min_gain` faster (0.03 = 3%), make a trip once
    /// `train_trip_gain`.
    pub train_min_gain: f64,
    pub train_trip_gain: f64,
    /// Bags (`bag_slots`): a kill's loot takes `loot_per_kill` slots until sold, each item that
    /// no longer fits costs `bag_weight` seconds (0 = the bags are not counted). A vendor within
    /// `sell_radius` is visited on the way once the loot fills half the room, `sell_time` there.
    pub bag_weight: f64,
    pub loot_per_kill: f64,
    pub sell_radius: f64,
    pub sell_time: f64,
    /// Share of the bag slots each gathering profession's materials keep (or the bag holding them).
    pub gather_share: f64,
    /// Character power (see `power`): the floor gear the character has anyway, as a share of
    /// the typical uncommon item of its level; a scale on the levels gear and spells add or
    /// take (0 = power is the level); the share of a dungeon's loot the character wins; how
    /// much a quest whose reward is an upgrade is worth (levels of XP per level of power gained:
    /// it makes every fight that follows faster).
    pub gear_floor: f64,
    pub power_scale: f64,
    pub dungeon_loot_share: f64,
    pub gear_value: f64,
    /// Class quests giving a pet or a form are done as soon as taken when that costs at most
    /// this many seconds: the route only counts their power in faster fights, not the safety.
    pub class_power_slack: f64,
    /// How the character's class fares at each level, in levels of power over the average class
    /// (`overrides/class_power.toml`, by level, empty: 0), and how much its gear counts for what it
    /// takes on (1 = average). Set from the class of the request, not read from routing.toml.
    #[serde(skip)]
    pub class_curve: Vec<f64>,
    #[serde(skip)]
    pub gear_dependence: f64,
    /// The player's own class curve and gear dependence (advanced settings), instead of the
    /// class's of class_power.toml: (level, levels of power) points, empty for the class's.
    pub power_curve: Vec<(i64, f64)>,
    pub power_gear: Option<f64>,
    /// Run dungeons with a group when they are worth it (their quests included).
    pub dungeons: bool,
    /// Dungeons (instance area IDs) always run, dungeons on or off...
    pub forced_dungeons: Vec<i64>,
    /// ...run with every quest of theirs the character can take (required as class quests are)...
    pub forced_dungeon_quests: Vec<i64>,
    /// ...and never run.
    pub excluded_dungeons: Vec<i64>,
    /// Dungeon quests are picked up when passing by, up to this many levels before the dungeon.
    pub dungeon_pickup_ahead: i64,
    /// Levels a dungeon is run at, from its monsters' level (`model::dungeon_levels`): from
    /// `dungeon_from` to `dungeon_to` (-3 and +3: Ruins of Lordaeron's monsters at 18, run from 15
    /// to 21), never out of them. The progression profile sets them (`Progression::dungeon_window`).
    pub dungeon_from: i64,
    pub dungeon_to: i64,
    /// Share of the solo XP of a dungeon's kills earned in a 5-man group (elites included).
    pub dungeon_xp_share: f64,
    /// How much a minute of grinding weighs against a minute of questing for the optimizer
    /// (1 = only total time matters; higher values trade time for less grinding).
    pub grind_weight: f64,
    /// Share of each level's XP grinding may give (1 = no limit): grinding beyond weighs
    /// `grind_over_weight` times its duration, so the route takes quests instead whenever it can.
    pub grind_cap: f64,
    /// Share of each level's XP farming on the way may give (1 = no limit), apart from
    /// `grind_cap`: farming on the way stops there. Forever has more quests: Classic and TBC
    /// allow more in `overrides/<edition>/routing.toml`.
    pub farm_cap: f64,
    pub grind_over_weight: f64,
    /// Share of a kill (resting, looking for the next mob) during which quest objects close by
    /// are gathered: a gathering objective stop right before or after a kill objective stop,
    /// within `gather_radius`, is done in part during the kills (see `plan::state::Together`).
    pub gather_overlap: f64,
    pub gather_radius: f64,
    /// Other places an objective can be done besides its main one (other groups of its mobs or
    /// objects): the route goes to the cheapest from where the character is. 0: only the main.
    pub objective_alternatives: usize,
    /// Gray quests that lead to nothing better are taken and kept like the others (turned in when
    /// passing by); by default they are left out and given up when they go gray.
    pub keep_gray: bool,
    /// Grinding allowed: when off, it weighs `NO_GRIND_WEIGHT` times its duration, so any quest
    /// is taken first; the guide still grinds when no quest is left.
    pub allow_grind: bool,
    /// Same for flights (1 = only their time counts): higher values walk or reorder the route
    /// rather than take a flight that saves little time.
    pub flight_weight: f64,
    /// Seconds a gold piece spent on flights is worth for the optimizer up to level
    /// `GOLD_WEIGHT_LEVEL` (0 = flights are free); less above, as quests pay more
    /// (`gold_weight_at`).
    pub gold_weight: f64,
    /// Seconds a change of zone costs the optimizer on top of the trip (0 = only the trip): finding
    /// one's way and the NPCs of another zone, which makes a few quests far away not worth it.
    pub zone_change_weight: f64,
    /// Cost of a class quest left out (keeps them in the route).
    pub mandatory_penalty: f64,
    /// Seconds each quest the route takes and gives up costs the optimizer (gone gray, its
    /// dungeon run without it): taking a quest, killing for one of its objectives and dropping it
    /// hours later is grinding in disguise in a guide. Finishing it or leaving it out wins
    /// unless it saves more than that.
    pub abandon_weight: f64,
    /// Seconds each quest of a leveled profession left out costs the optimizer (Camping 101 at
    /// the profession's trainer): their reward is the profession's, not XP, so they are planned
    /// whenever the route passes by, not at the cost of a long detour.
    pub profession_quest_weight: f64,
    /// How the first route is built.
    pub construction: Construction,
    /// Searches at once (different seeds), whatever the machine's processors: the same guide on
    /// any of them, only slower with fewer.
    pub threads: usize,
    pub seed: u64,
    /// The searches improving the same route meet every this many tries, and those behind go
    /// on from the best (0: each on its own).
    pub share_every: u64,
    /// Players leveling together (1 = solo, up to 5). Set from the request's group.
    pub group_size: i64,
    /// Damage each extra group member adds, as a share of one player's.
    pub group_kill_speed: f64,
    /// Quest items drop for every group member (else each one loots their own).
    pub group_shared_drops: bool,
    /// Levels a group's players may be apart freely: the guide gets them within it first.
    pub group_sync_window: f64,
    /// Score of each second the group's players are a level further apart than
    /// `group_sync_window` (seconds).
    pub group_sync_weight: f64,
    /// Levels a group's players are apart at most once together: further, the players behind
    /// kill mobs on their own to catch up (`Event::CatchUp`).
    pub group_sync_max: f64,
    /// Distance from an area's center at which passing by discovers it (exploration XP).
    pub explore_radius: f64,
    /// Game version: experience rules, level cap.
    pub edition: crate::xp::Edition,
}

/// The settings the apps and tools change on top of an edition's defaults, set one by one:
///
/// ```ignore
/// let params = overrides.params.clone().builder().effort(1.0).dungeons(false).build();
/// ```
#[derive(Debug, Clone)]
#[must_use]
pub struct ParamsBuilder(Params);

impl Params {
    /// Settings to change from these.
    pub fn builder(self) -> ParamsBuilder {
        ParamsBuilder(self)
    }
}

impl ParamsBuilder {
    /// How long the search goes on (see `Params::effort`).
    pub fn effort(mut self, effort: f64) -> Self {
        self.0.effort = effort;
        self
    }

    /// The seed of the search.
    pub fn seed(mut self, seed: u64) -> Self {
        self.0.seed = seed;
        self
    }

    /// Whether the route goes to dungeons.
    pub fn dungeons(mut self, on: bool) -> Self {
        self.0.dungeons = on;
        self
    }

    /// Whether elite monsters are fought alone.
    pub fn allow_elite(mut self, on: bool) -> Self {
        self.0.allow_elite = on;
        self
    }

    /// Whether quests asking for player kills are taken.
    pub fn pvp_quests(mut self, on: bool) -> Self {
        self.0.pvp_quests = on;
        self
    }

    /// Whether quest sources are taken as the data approximates them.
    pub fn approx_sources(mut self, on: bool) -> Self {
        self.0.approx_sources = on;
        self
    }

    /// Weight of the flights in the score.
    pub fn flight_weight(mut self, weight: f64) -> Self {
        self.0.flight_weight = weight;
        self
    }

    pub fn build(self) -> Params {
        self.0
    }
}

impl Default for Params {
    fn default() -> Self {
        Self {
            run_speed: 7.0,
            mount_level: 40,
            mount_bonus: 0.6,
            flight_speed: 30.0,
            detour: 1.0,
            stop_overhead: 8.0,
            flight_overhead: 25.0,
            kill_time: 24.0,
            grind_kill_time: 53.0,
            default_xp_difficulty: 5,
            default_kill_count: 8.0,
            default_item_count: 6.0,
            default_drop_chance: 0.6,
            min_start_chance: 0.1,
            object_time: 12.0,
            max_quest_above: 60,
            progression: Progression::Normal,
            progression_margin: None,
            elite_pull_levels: 1.0,
            pull_levels: 1.0,
            pack_power: None,
            quest_log_size: 40,
            flight_learn_radius: 300.0,
            farm_on_way: true,
            farm_corridor: 25.0,
            farm_share: 0.8,
            farm_below: 3,
            farm_above: 1,
            camp_radius: 0.0,
            allow_elite: false,
            all_class_quests: false,
            pvp_quests: false,
            pvp_mark_time: 900.0,
            crowded: 0.0,
            crowd_event_time: 900.0,
            crowd_players: 3.0,
            crowd_by_level: vec![1.0, 0.9, 0.8, 0.6, 0.5],
            respawn_time: 300.0,
            dynamic_respawn: 0.5,
            solo_elite_kills: 2.0,
            zone_stay: 4.0,
            zone_stay_quests: 0.0,
            construction_random: true,
            zone_detour: vec![],
            zone_detour_pull: 10.0,
            forced_starts: 0,
            start_zone: 0,
            race_mutants: 224,
            screen: 8,
            continent_stay: 1.0,
            dungeon_quest_weight: 0.3,
            candidates: 32,
            qualify_tries: 16.5,
            qualify_min_tries: 30_000.0,
            round_tries: 0.0,
            final_routes: 1,
            final_tries: 195.0,
            effort: 1.0,
            max_object_loot: 20,
            common_item_sources: 100,
            common_item_time: 60.0,
            shortcuts: true,
            zone_gap: 8.0,
            link_wait: 120.0,
            elite_kill_factor: 3.0,
            approx_sources: false,
            auction_items: vec![],
            fit_free_levels: 2,
            fit_penalty: 0.0,
            fit_above_penalty: 0.4,
            efficiency_power: 0.0,
            anneal: 0.0,
            alns: 0.25,
            lahc: 0,
            eval_window: 150,
            excluded_sorts: vec![
                22, 24, 101, 121, 181, 182, 201, 221, 264, 284, 304, 324, 344, 364, 365, 366, 367, 368, 369, 641, 644,
            ],
            excluded_quests: vec![],
            preferred_zones: vec![],
            excluded_zones: vec![],
            forced_zones: vec![],
            preferred_zone_weight: 0.5,
            reputation_goals: vec![],
            reputation_weight: 10.0,
            min_efficiency: 0.5,
            hearth_cooldown: 3600.0,
            hearth_cast: 10.0,
            hearth_min_saving: 180.0,
            recall_cooldown: 900.0,
            teleport_cast: 10.0,
            death_skips: false,
            death_skip_time: 45.0,
            train_every: 2,
            train_radius: 300.0,
            bag_weight: 20.0,
            loot_per_kill: 0.35,
            sell_radius: 150.0,
            sell_time: 15.0,
            gather_share: 0.1,
            train_max_delay: 1,
            train_time: 30.0,
            train_min_gain: 0.03,
            train_trip_gain: 0.12,
            gear_floor: 0.75,
            power_scale: 1.0,
            dungeon_loot_share: 0.35,
            gear_value: 0.5,
            class_power_slack: 600.0,
            class_curve: Vec::new(),
            gear_dependence: 1.0,
            power_curve: Vec::new(),
            power_gear: None,
            mandatory_penalty: 7200.0,
            grind_weight: 1.5,
            grind_cap: 0.1,
            farm_cap: 0.1,
            grind_over_weight: 4.0,
            gather_overlap: 0.4,
            gather_radius: 150.0,
            objective_alternatives: 2,
            allow_grind: true,
            keep_gray: false,
            flight_weight: 1.0,
            gold_weight: 3600.0,
            zone_change_weight: 240.0,
            abandon_weight: 600.0,
            profession_quest_weight: 300.0,
            dungeon_xp_share: 0.35,
            dungeons: true,
            forced_dungeons: vec![],
            forced_dungeon_quests: vec![],
            excluded_dungeons: vec![],
            dungeon_pickup_ahead: 8,
            dungeon_from: -3,
            dungeon_to: 3,
            construction: Construction::Greedy,
            threads: 8,
            seed: 42,
            share_every: 500,
            group_size: 1,
            group_kill_speed: 0.8,
            group_shared_drops: true,
            group_sync_window: 0.5,
            group_sync_weight: 1.0,
            group_sync_max: 1.0,
            explore_radius: 250.0,
            edition: crate::xp::Edition::Forever,
        }
    }
}

/// The level `Params::gold_weight` is for: up to it, money is short (the trainers take most of
/// it); above, a gold piece weighs less, as the cube of the level (quests and loot pay more and
/// more, flights are no longer a worry).
pub(crate) const GOLD_WEIGHT_LEVEL: f64 = 30.0;

/// Bag slots at each level, between these points: the 16 of the backpack alone at first, then
/// four bags of 6 (linen), 8, 10, 12 and 14 slots (runecloth) as the levels go.
const BAG_SLOTS: [(i64, f64); 7] = [
    (1, 16.0),
    (10, 28.0),
    (20, 40.0),
    (30, 48.0),
    (40, 56.0),
    (50, 64.0),
    (60, 72.0),
];

/// The bag slots a character of `level` has (`BAG_SLOTS`).
pub(crate) fn bag_slots(level: i64) -> f64 {
    let at = BAG_SLOTS.partition_point(|&(l, _)| l <= level);
    match (at.checked_sub(1).map(|k| BAG_SLOTS[k]), BAG_SLOTS.get(at)) {
        (Some((l0, s0)), Some(&(l1, s1))) => s0 + (s1 - s0) * (level - l0) as f64 / (l1 - l0) as f64,
        (Some((_, s)), None) | (None, Some(&(_, s))) => s,
        (None, None) => 0.0,
    }
}

impl Params {
    /// Seconds a gold piece spent on flights weighs at `level` (see `gold_weight`): an hour up to
    /// level 30 is 25 minutes at 40 and 8 at 60, where a flight costs the minutes of questing
    /// that pay its price, not the hour a level 20 character needs to earn it. By bands of 5
    /// levels (their top), so that the trips remembered serve the whole band (`Trips::leg`).
    pub fn gold_weight_at(&self, level: i64) -> f64 {
        let band = (level.clamp(1, 250) + 4) / 5 * 5;
        let level = (band as f64).max(GOLD_WEIGHT_LEVEL);
        self.gold_weight * (GOLD_WEIGHT_LEVEL / level).powi(3)
    }

    pub fn speed(&self, level: i64) -> f64 {
        if level >= self.mount_level {
            self.run_speed * (1.0 + self.mount_bonus)
        } else {
            self.run_speed
        }
    }

    /// Factor and margin of the progression profile: the character takes on content up to
    /// `power × factor + margin` (`reach`). The factors stay at 1: any other value takes away
    /// (or adds) more levels the higher the character, while the game's colors are a number of
    /// levels whatever the level. Content at the character's power is what the game
    /// shows yellow up to two levels above: "normal" goes there and never orange, "cautious"
    /// (the hardcore survival style) takes yellow quests and lone targets up to two levels
    /// above but packs at most one (`pack_power`), "risky" goes up to orange quests and
    /// borderline elites.
    pub(crate) fn progression_profile(&self) -> (f64, f64) {
        let (factor, margin) = match self.progression {
            Progression::Cautious => (1.0, 1.5),
            Progression::Normal => (1.0, 2.0),
            Progression::Risky => (1.0, 3.0),
        };
        (factor, self.progression_margin.unwrap_or(margin))
    }

    /// The tier of drops the character jumps down (`terrain::layers::DROP_HEIGHTS`): "cautious"
    /// only what the game does not hurt for (15 yards), "normal" what takes a third of its health
    /// at most (30 yards), "risky" up to two thirds (50 yards).
    pub fn drop_tier(&self) -> usize {
        match self.progression {
            Progression::Cautious => 0,
            Progression::Normal => 1,
            Progression::Risky => 2,
        }
    }

    /// How much a minute of grinding weighs for the optimizer (see `allow_grind`).
    pub fn grind_weight(&self) -> f64 {
        if self.allow_grind {
            self.grind_weight
        } else {
            self.grind_weight.max(NO_GRIND_WEIGHT)
        }
    }

    /// Share of the crowd at quests of `level` (`crowd_by_level`), times `crowded`.
    pub fn crowd_at(&self, level: i64) -> f64 {
        let k = usize::try_from((level.max(1) - 1) / 10).unwrap_or(0);
        let share = self
            .crowd_by_level
            .get(k)
            .or(self.crowd_by_level.last())
            .copied()
            .unwrap_or(1.0);
        self.crowded * share.clamp(0.0, 1.0)
    }

    /// Share of the mobs along a walk a character of `level` kills on the way: `farm_share`,
    /// split with the other players around as the crowd shares an objective's spot (one in
    /// 1 + `crowd_at` × `crowd_players` is left to the character).
    pub(crate) fn farm_share_at(&self, level: i64) -> f64 {
        self.farm_share.clamp(0.0, 1.0) / (1.0 + self.crowd_at(level) * self.crowd_players.max(0.0))
    }

    /// How much a minute of grinding beyond `grind_cap` weighs for the optimizer.
    pub fn grind_over_weight(&self) -> f64 {
        self.grind_over_weight.max(self.grind_weight())
    }

    /// Whether to hearth rather than travel: `by_hearth` saves enough of the `direct` trip.
    pub fn hearth_worth(&self, direct: f64, by_hearth: f64) -> bool {
        by_hearth + self.hearth_min_saving < direct
    }

    /// Power a pack (three kills or more) requires on top of a lone target of the same level:
    /// several mobs at a time hurt more than one, which the cautious profile weighs.
    pub fn pack_extra(&self) -> f64 {
        self.pack_power.unwrap_or(if self.progression == Progression::Cautious {
            1.0
        } else {
            0.0
        })
    }

    /// Highest required power the character takes on at `level` with `might`: its class at that
    /// level (`class_curve`), its gear (`gear_reach`) and what class quests and spells add (up to
    /// `GEAR_REACH`: they make fights faster, they do not remove the danger of higher mobs;
    /// missing spells count fully).
    pub fn reach(&self, level: i64, might: Might) -> f64 {
        let (factor, margin) = self.progression_profile();
        let power =
            level as f64 + self.class_offset(level) + self.gear_reach(level, might.gear) + might.other.min(GEAR_REACH);
        power * factor + margin
    }

    /// Levels of power the class is worth at `level` over the average class (`class_curve`).
    pub(crate) fn class_offset(&self, level: i64) -> f64 {
        let n = self.class_curve.len();
        if n == 0 {
            return 0.0;
        }
        self.class_curve[(level.max(0) as usize).min(n - 1)]
    }

    /// Levels of power gear of quality `gear` (`Might::gear`: -2 for none or plain items, 0 for
    /// the usual uncommon items of the level, +1 for very good ones) is worth for what the
    /// character takes on at `level`: little at first, fully at the level cap of Classic, times
    /// the class's dependence on its gear (never beyond `GEAR_WORST` and `GEAR_BEST`).
    pub(crate) fn gear_reach(&self, level: i64, gear: f64) -> f64 {
        let weight = (level as f64 / GEAR_FULL_LEVEL).clamp(GEAR_MIN_WEIGHT, 1.0);
        (self.gear_dependence * weight * gear).clamp(GEAR_WORST, GEAR_BEST)
    }

    /// Lowest level at which the character reaches `need` with `might`.
    pub(crate) fn level_for(&self, need: f64, might: Might) -> i64 {
        let (factor, margin) = self.progression_profile();
        // The level without class and gear, then a few steps to where `reach` crosses `need`
        // (its class and gear parts change slowly with the level).
        let mut level = ((need - margin) / factor - might.other.min(GEAR_REACH) - 1e-9).ceil() as i64;
        for _ in 0..200 {
            if self.reach(level, might) + 1e-9 < need {
                level += 1;
            } else if self.reach(level - 1, might) + 1e-9 >= need {
                level -= 1;
            } else {
                break;
            }
        }
        level
    }

    /// Kill time against a mob of `mob` level for a character of `level` with `bonus` levels of
    /// power (what gear and spells add, see `power`). The level difference keeps its weight (gear
    /// does not remove misses, resists and the danger of mobs above the character): power only
    /// makes every fight faster, by 10% per level.
    pub fn kill_time(&self, level: i64, bonus: f64, mob: i64) -> f64 {
        self.kill_time * kill_factor((mob - level) as f64) * power_speed(bonus) / self.group_damage()
    }

    /// Damage of the group, in solo players.
    pub(crate) fn group_damage(&self) -> f64 {
        1.0 + self.group_kill_speed * (self.group_size.clamp(1, 5) - 1) as f64
    }

    /// Share of a mob's XP each member gets: the game's group bonus, split between members.
    pub(crate) fn group_xp_share(&self) -> f64 {
        let n = self.group_size.clamp(1, 5);
        [1.0, 1.0, 1.166, 1.3, 1.4][n as usize - 1] / n as f64
    }
}

impl Params {
    /// Apply `key=value` overrides (any field, value parsed as JSON or taken as a string).
    /// A progression profile given without the dungeons' levels brings its own
    /// (`Progression::dungeon_window`), as the app's choice of a profile does.
    pub fn with_overrides(&self, pairs: &[String]) -> anyhow::Result<Params> {
        let mut json = serde_json::to_value(self)?;
        let mut keys = Vec::new();
        for pair in pairs {
            let (key, value) = pair
                .split_once('=')
                .ok_or_else(|| anyhow::anyhow!("expected key=value: {pair}"))?;
            let field = json
                .get_mut(key.trim())
                .ok_or_else(|| anyhow::anyhow!("unknown parameter {key}"))?;
            *field = serde_json::from_str(value.trim())
                .unwrap_or_else(|_| serde_json::Value::String(value.trim().to_owned()));
            keys.push(key.trim());
        }
        let mut params: Params = serde_json::from_value(json)?;
        if keys.contains(&"progression")
            && !keys
                .iter()
                .any(|k| k.starts_with("dungeon_from") || k.starts_with("dungeon_to"))
        {
            (params.dungeon_from, params.dungeon_to) = params.progression.dungeon_window();
        }
        Ok(params)
    }
}

/// A reputation to reach: the faction (Faction ID) and the points to have with it, counted
/// from the bottom of Neutral as the game does (Friendly 3000, Honored 9000, Revered 21000,
/// Exalted 42000).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct ReputationGoal {
    pub faction: i64,
    pub target: i64,
}

/// `crowded`: a number from 0 to 1, or a boolean (settings saved before the scale).
fn crowd_share<'de, D: serde::Deserializer<'de>>(d: D) -> Result<f64, D::Error> {
    #[derive(Deserialize)]
    #[serde(untagged)]
    enum Level {
        On(bool),
        Share(f64),
    }
    Ok(match Level::deserialize(d)? {
        Level::On(on) => f64::from(u8::from(on)),
        Level::Share(v) => v.clamp(0.0, 1.0),
    })
}

/// How far above its power the character goes.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Progression {
    /// The hardcore survival style: yellow quests and lone targets up to two levels above, packs
    /// at most one.
    Cautious,
    /// Yellow quests, never orange.
    Normal,
    /// Orange quests and borderline elites.
    Risky,
}

impl Progression {
    /// The levels a dungeon is run at by default, from its monsters' level (`Params::dungeon_from`,
    /// `Params::dungeon_to`): at their level or above when cautious, from 3 below when normal, 5
    /// below when risky.
    pub fn dungeon_window(self) -> (i64, i64) {
        match self {
            Self::Cautious => (0, 5),
            Self::Normal => (-3, 3),
            Self::Risky => (-5, 2),
        }
    }
}

/// How the first route is built.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Construction {
    /// Nearest worthwhile stop first.
    Greedy,
    /// Quest groups, region by region.
    #[serde(alias = "groups")]
    Regions,
    /// Race candidates alternate between both.
    Mixed,
}

/// Weight of a minute of grinding when the player does not want to grind.
pub(crate) const NO_GRIND_WEIGHT: f64 = 10.0;

/// Levels of power from class quests and spells that extend which mobs and quests the
/// character takes on.
pub(crate) const GEAR_REACH: f64 = 0.5;

/// Gear quality (`Might::gear`) of a character in plain items, and with very good ones.
pub(crate) const GEAR_WORST: f64 = -2.0;
pub(crate) const GEAR_BEST: f64 = 1.0;
/// Gear counts fully for what the character takes on from this level, `GEAR_MIN_WEIGHT` of it
/// at the start.
const GEAR_FULL_LEVEL: f64 = 60.0;
const GEAR_MIN_WEIGHT: f64 = 0.1;

/// What the character takes on depends on, besides its level and class (see `Params::reach`):
/// the quality of its gear (`GEAR_WORST` to `GEAR_BEST`), and the levels of power class quests
/// and spells add (`other`, the part of the power that is not gear).
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Might {
    pub other: f64,
    pub gear: f64,
}

impl Might {
    /// A character that has nothing more than its level: plain gear, no class quest.
    pub const PLAIN: Might = Might {
        other: 0.0,
        gear: GEAR_WORST,
    };
}

/// Fight time of a character `bonus` levels of power above its level, relative to its level alone.
pub(crate) fn power_speed(bonus: f64) -> f64 {
    (-bonus * crate::power::LEVEL_LN).exp()
}

/// Kill time against a mob `d` levels above the character, relative to a same-level mob.
pub(crate) fn kill_factor(d: f64) -> f64 {
    const TABLE: [f64; 8] = [0.65, 0.8, 0.9, 1.0, 1.2, 1.5, 2.2, 3.5];
    let x = (d + 3.0).clamp(0.0, 7.0);
    let k = (x.floor() as usize).min(6);
    let t = x - k as f64;
    TABLE[k] * (1.0 - t) + TABLE[k + 1] * t
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The builder changes what it is told to and nothing else.
    #[test]
    fn the_builder_changes_only_what_it_is_told() {
        let base = Params::default();
        let built = base
            .clone()
            .builder()
            .effort(3.0)
            .dungeons(!base.dungeons)
            .seed(7)
            .build();
        assert_eq!((built.effort, built.seed, built.dungeons), (3.0, 7, !base.dungeons));
        assert_eq!(built.allow_elite, base.allow_elite);
        assert_eq!(built.flight_weight, base.flight_weight);
        assert_eq!(base.clone().builder().build().seed, base.seed);
    }

    /// Redridge: hearth to Darkshire then fly back to Lakeshire, 169 s instead of a 3 min walk.
    #[test]
    fn hearth_not_burnt_for_seconds() {
        let p = Params::default();
        assert!(!p.hearth_worth(185.0, 169.0));
        assert!(p.hearth_worth(1200.0, 300.0));
    }

    /// Plain gear holds the character back little at first and two levels at the level cap;
    /// good gear takes it a level further; a class strong at a level goes further there.
    #[test]
    fn gear_counts_more_with_the_level() {
        let mut p = Params::default();
        let plain = Might::PLAIN;
        let usual = Might { other: 0.0, gear: 0.0 };
        let good = Might {
            other: 0.0,
            gear: GEAR_BEST,
        };
        assert!(p.reach(5, usual) - p.reach(5, plain) < 0.5);
        assert!((p.reach(60, usual) - p.reach(60, plain) - 2.0).abs() < 1e-9);
        assert!((p.reach(60, good) - p.reach(60, usual) - 1.0).abs() < 1e-9);
        p.class_curve = vec![1.0; 61];
        assert!((p.reach(30, usual) - (30.0 + 1.0 + 2.0)).abs() < 1e-9);
    }

    /// `level_for` is the lowest level whose reach covers the need, whatever the class and gear.
    #[test]
    fn level_for_inverts_reach() {
        let p = Params {
            class_curve: (0..=60).map(|l| if l < 20 { -1.5 } else { 1.0 }).collect(),
            gear_dependence: 1.5,
            ..Params::default()
        };
        for might in [Might::PLAIN, Might { other: 0.3, gear: 0.5 }] {
            for need in [3.0, 12.5, 21.0, 40.0, 58.0] {
                let level = p.level_for(need, might);
                assert!(p.reach(level, might) >= need - 1e-9, "{need} at {level}");
                assert!(p.reach(level - 1, might) < need, "{need} before {level}");
            }
        }
    }

    /// Gold weighs its full `gold_weight` up to level 30, then less as the cube of the level:
    /// an eighth at 60, by bands of 5 levels.
    #[test]
    fn gold_weighs_less_the_higher_the_level() {
        let p = Params {
            gold_weight: 3600.0,
            ..Params::default()
        };
        assert_eq!(p.gold_weight_at(1), 3600.0);
        assert_eq!(p.gold_weight_at(30), 3600.0);
        assert!(p.gold_weight_at(40) < 3600.0);
        assert!((p.gold_weight_at(60) - 450.0).abs() < 1e-9);
        assert_eq!(p.gold_weight_at(37), p.gold_weight_at(40));
        assert!(p.gold_weight_at(60) < p.gold_weight_at(45));
    }
}
