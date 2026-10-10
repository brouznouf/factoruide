//! Tuning knobs of the time model. Defaults can be overridden by `overrides/routing.toml`.

use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct Params {
    /// Running speed in yards/second.
    pub run_speed: f64,
    /// Level at which the character gets a mount, and its speed bonus.
    pub mount_level: i64,
    pub mount_bonus: f64,
    /// Flight speed in yards/second.
    pub flight_speed: f64,
    /// Ground paths are longer than straight lines.
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
    /// Power a pack (three kills or more) requires on top of a lone target of the same level,
    /// instead of the progression profile's own (tuning).
    pub pack_power: Option<f64>,
    /// Quests the log holds at once (40 on WoW Forever, 20 on Classic Era).
    pub quest_log_size: usize,
    /// Distance at which a flight master is picked up on the way.
    pub flight_learn_radius: f64,
    /// Mobs of the quests in the log within this distance of a walk are killed on the way
    /// (0: never).
    pub along_corridor: f64,
    /// Kill the normal mobs met on every walk, not only those of the log's quests (as guides
    /// ask): those within `farm_corridor` of the path, from `farm_below` levels under the
    /// character to `farm_above` over it (lower ones give little XP, higher ones are slow and
    /// risky). It replaces grinding: counted in `grind_cap`, weighed like grinding.
    pub farm_on_way: bool,
    pub farm_corridor: f64,
    pub farm_below: i64,
    pub farm_above: i64,
    /// Quests of the route whose NPC is within this distance of a stop are taken and turned in
    /// there, as a player does at a quest hub (0: only at their own stop).
    pub camp_radius: f64,
    /// Allow quests whose objectives involve elite mobs.
    pub allow_elite: bool,
    /// Battleground (PvP) quests (marks of honor...), each mark or objective taking
    /// `pvp_mark_time` seconds of battleground.
    pub pvp_quests: bool,
    pub pvp_mark_time: f64,
    /// Without `allow_elite`, elite objectives of at most this many kills are kept anyway: the
    /// lone elites a player kills solo (Hogger, Tharil'zun, Mor'Ladim...). 0 = none.
    pub solo_elite_kills: f64,
    /// Construction plan drawn at random for each candidate of the race (zone_stay,
    /// continent_stay, fit and efficiency weights); off: the values below are used as they are.
    pub construction_random: bool,
    /// Greedy construction: weight of the stops on another continent (1 = no preference).
    pub continent_stay: f64,
    /// Greedy construction: weight of the quests of a dungeon close in level (lower = the
    /// dungeon is planned more readily; forced dungeons use 0.25).
    pub dungeon_quest_weight: f64,
    /// Race: routes built (each its own seed and construction plan) and briefly improved, at most
    /// (no new one once its construction would end after the qualification)...
    pub candidates: usize,
    /// ...during this share of `time_limit_ms` (0 = no race), then the best `threads` (or
    /// `finalists`) are optimized for the rest.
    pub qualify_share: f64,
    /// Routes kept after the qualification (at least `threads`).
    pub finalists: usize,
    /// Greedy construction: weight of the stops outside the current zone (1 = nearest stop first,
    /// higher values clear the zone before leaving it).
    pub zone_stay: f64,
    /// Objects looting more different items than this (world treasure chests) are not sources
    /// of quest items (0 = every object counts).
    pub max_object_loot: usize,
    /// Items dropped by at least this many mob types are common loot (cloth, gems, potions)...
    pub common_item_sources: usize,
    /// ...gathered on the way or bought, in seconds per item, rather than farmed.
    pub common_item_time: f64,
    /// Use off-road shortcuts (climbs, jumps) declared in `passes.toml`.
    pub shortcuts: bool,
    /// Average wait before boarding a boat, zeppelin or tram (seconds).
    pub link_wait: f64,
    /// Kill time of an elite compared with a normal mob of the same level.
    pub elite_kill_factor: f64,
    /// Keep quests whose objective location is unknown, placing it near the quest giver
    /// (for comparisons on databases with gaps).
    pub approx_sources: bool,
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
    /// Quest sorts (negative zoneOrSort) never planned: holidays, professions, events...
    pub excluded_sorts: Vec<i64>,
    /// Quests never planned (manual exceptions).
    pub excluded_quests: Vec<i64>,
    /// Zones (AreaTable IDs) to favour, and zones never used for quest givers.
    pub preferred_zones: Vec<i64>,
    pub excluded_zones: Vec<i64>,
    /// Minimum quest efficiency, relative to grinding, to be worth accepting.
    pub min_efficiency: f64,
    /// Hearthstone cooldown and cast time.
    pub hearth_cooldown: f64,
    pub hearth_cast: f64,
    /// The hearthstone is worth its hour of cooldown only when it saves at least this (seconds).
    pub hearth_min_saving: f64,
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
    /// Run dungeons with a group when they are worth it (their quests included).
    pub dungeons: bool,
    /// Dungeons (instance area IDs) always run, dungeons on or off...
    pub forced_dungeons: Vec<i64>,
    /// ...and never run.
    pub excluded_dungeons: Vec<i64>,
    /// Dungeon quests are picked up when passing by, up to this many levels before the dungeon.
    pub dungeon_pickup_ahead: i64,
    /// Share of the solo XP of a dungeon's kills earned in a 5-man group (elites included).
    pub dungeon_xp_share: f64,
    /// How much a minute of grinding weighs against a minute of questing for the optimizer
    /// (1 = only total time matters; higher values trade time for less grinding).
    pub grind_weight: f64,
    /// Share of each level's XP grinding may give, mobs farmed on the way included (1 = no
    /// limit): farming on the way stops there, grinding beyond weighs `grind_over_weight` times
    /// its duration, so the route takes quests instead whenever it can.
    pub grind_cap: f64,
    pub grind_over_weight: f64,
    /// Share of a kill (resting, looking for the next mob) during which quest objects close by
    /// are gathered: a gathering objective within `gather_radius` of a kill objective of the log
    /// is done with it, in part during the kills.
    pub gather_overlap: f64,
    pub gather_radius: f64,
    /// Gray quests that lead to nothing better are taken and kept like the others (turned in when
    /// passing by); by default they are left out and given up when they go gray.
    pub keep_gray: bool,
    /// Grinding allowed: when off, it weighs `NO_GRIND_WEIGHT` times its duration, so any quest
    /// is taken first; the guide still grinds when no quest is left.
    pub allow_grind: bool,
    /// Same for flights (1 = only their time counts): higher values walk or reorder the route
    /// rather than take a flight that saves little time.
    pub flight_weight: f64,
    /// Seconds a gold piece spent on flights is worth for the optimizer (0 = flights are free).
    pub gold_weight: f64,
    /// Seconds a change of zone costs the optimizer on top of the trip (0 = only the trip): finding
    /// one's way and the NPCs of another zone, which makes a few quests far away not worth it.
    pub zone_change_weight: f64,
    /// Cost of a class quest left out (keeps them in the route).
    pub mandatory_penalty: f64,
    /// How the first route is built.
    pub construction: Construction,
    /// Whole optimization time: the race of candidates, then the search on the best.
    pub time_limit_ms: u64,
    /// Parallel searches (different seeds), the best one wins.
    pub threads: usize,
    pub seed: u64,
    /// Players leveling together (1 = solo, up to 5). Set from the request's group.
    pub group_size: i64,
    /// Damage each extra group member adds, as a share of one player's.
    pub group_kill_speed: f64,
    /// Quest items drop for every group member (else each one loots their own).
    pub group_shared_drops: bool,
    /// Distance from an area's center at which passing by discovers it (exploration XP).
    pub explore_radius: f64,
    /// Game version: experience rules, level cap.
    pub edition: crate::xp::Edition,
}

impl Default for Params {
    fn default() -> Self {
        Self {
            run_speed: 7.0,
            mount_level: 40,
            mount_bonus: 0.6,
            flight_speed: 30.0,
            detour: 1.3,
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
            pack_power: None,
            quest_log_size: 40,
            flight_learn_radius: 300.0,
            along_corridor: 40.0,
            farm_on_way: true,
            farm_corridor: 25.0,
            farm_below: 3,
            farm_above: 1,
            camp_radius: 120.0,
            allow_elite: false,
            pvp_quests: false,
            pvp_mark_time: 900.0,
            solo_elite_kills: 2.0,
            zone_stay: 2.5,
            construction_random: true,
            continent_stay: 1.0,
            dungeon_quest_weight: 0.5,
            candidates: 32,
            qualify_share: 0.25,
            finalists: 0,
            max_object_loot: 20,
            common_item_sources: 100,
            common_item_time: 60.0,
            shortcuts: true,
            link_wait: 120.0,
            elite_kill_factor: 3.0,
            approx_sources: false,
            fit_free_levels: 2,
            fit_penalty: 0.0,
            fit_above_penalty: 0.0,
            efficiency_power: 0.0,
            anneal: 0.0,
            alns: 0.25,
            lahc: 0,
            excluded_sorts: vec![
                22, 24, 101, 121, 181, 182, 201, 221, 264, 284, 304, 324, 344, 364, 365, 366, 367, 368, 369, 641, 644,
            ],
            excluded_quests: vec![],
            preferred_zones: vec![],
            excluded_zones: vec![],
            min_efficiency: 0.5,
            hearth_cooldown: 3600.0,
            hearth_cast: 10.0,
            hearth_min_saving: 180.0,
            train_every: 2,
            train_radius: 300.0,
            train_max_delay: 1,
            train_time: 30.0,
            train_min_gain: 0.03,
            train_trip_gain: 0.12,
            gear_floor: 0.75,
            power_scale: 1.0,
            dungeon_loot_share: 0.35,
            gear_value: 0.5,
            class_power_slack: 600.0,
            mandatory_penalty: 7200.0,
            grind_weight: 1.5,
            grind_cap: 0.1,
            grind_over_weight: 4.0,
            gather_overlap: 0.4,
            gather_radius: 150.0,
            allow_grind: true,
            keep_gray: false,
            flight_weight: 1.0,
            gold_weight: 3600.0,
            zone_change_weight: 240.0,
            dungeon_xp_share: 0.35,
            dungeons: true,
            forced_dungeons: vec![],
            excluded_dungeons: vec![],
            dungeon_pickup_ahead: 8,
            construction: Construction::Greedy,
            time_limit_ms: 120_000,
            threads: std::thread::available_parallelism().map_or(4, |n| n.get().min(8)),
            seed: 42,
            group_size: 1,
            group_kill_speed: 0.8,
            group_shared_drops: true,
            explore_radius: 250.0,
            edition: crate::xp::Edition::Forever,
        }
    }
}

impl Params {
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
    pub fn progression_profile(&self) -> (f64, f64) {
        let (factor, margin) = match self.progression {
            Progression::Cautious => (1.0, 1.5),
            Progression::Normal => (1.0, 2.0),
            Progression::Risky => (1.0, 3.0),
        };
        (factor, self.progression_margin.unwrap_or(margin))
    }

    /// How much a minute of grinding weighs for the optimizer (see `allow_grind`).
    pub fn grind_weight(&self) -> f64 {
        if self.allow_grind {
            self.grind_weight
        } else {
            self.grind_weight.max(NO_GRIND_WEIGHT)
        }
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

    /// Highest required power the character takes on at `level` with `bonus` levels of power
    /// from gear and spells (gear counts up to `GEAR_REACH`: it makes fights faster, it does not
    /// remove the danger of higher mobs; missing spells count fully).
    pub fn reach(&self, level: i64, bonus: f64) -> f64 {
        let (factor, margin) = self.progression_profile();
        (level as f64 + bonus.min(GEAR_REACH)) * factor + margin
    }

    /// Lowest level at which the character reaches `need` with `bonus` levels of power.
    pub fn level_for(&self, need: f64, bonus: f64) -> i64 {
        let (factor, margin) = self.progression_profile();
        ((need - margin) / factor - bonus.min(GEAR_REACH) - 1e-9).ceil() as i64
    }

    /// Kill time against a mob of `mob` level for a character of `level` with `bonus` levels of
    /// power (what gear and spells add, see `power`). The level difference keeps its weight (gear
    /// does not remove misses, resists and the danger of mobs above the character): power only
    /// makes every fight faster, by 10% per level.
    pub fn kill_time(&self, level: i64, bonus: f64, mob: i64) -> f64 {
        self.kill_time * kill_factor((mob - level) as f64) * power_speed(bonus) / self.group_damage()
    }

    /// Damage of the group, in solo players.
    pub fn group_damage(&self) -> f64 {
        1.0 + self.group_kill_speed * (self.group_size.clamp(1, 5) - 1) as f64
    }

    /// Share of a mob's XP each member gets: the game's group bonus, split between members.
    pub fn group_xp_share(&self) -> f64 {
        let n = self.group_size.clamp(1, 5);
        [1.0, 1.0, 1.166, 1.3, 1.4][n as usize - 1] / n as f64
    }
}

impl Params {
    /// Apply `key=value` overrides (any field, value parsed as JSON or taken as a string).
    pub fn with_overrides(&self, pairs: &[String]) -> anyhow::Result<Params> {
        let mut json = serde_json::to_value(self)?;
        for pair in pairs {
            let (key, value) = pair
                .split_once('=')
                .ok_or_else(|| anyhow::anyhow!("expected key=value: {pair}"))?;
            let field = json
                .get_mut(key.trim())
                .ok_or_else(|| anyhow::anyhow!("unknown parameter {key}"))?;
            *field = serde_json::from_str(value.trim())
                .unwrap_or_else(|_| serde_json::Value::String(value.trim().to_owned()));
        }
        Ok(serde_json::from_value(json)?)
    }
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
pub const NO_GRIND_WEIGHT: f64 = 10.0;

/// Levels of gear power that extend which mobs and quests the character takes on.
pub const GEAR_REACH: f64 = 0.5;

/// Fight time of a character `bonus` levels of power above its level, relative to its level alone.
pub fn power_speed(bonus: f64) -> f64 {
    (-bonus * crate::power::LEVEL_LN).exp()
}

/// Kill time against a mob `d` levels above the character, relative to a same-level mob.
pub fn kill_factor(d: f64) -> f64 {
    const TABLE: [f64; 8] = [0.65, 0.8, 0.9, 1.0, 1.2, 1.5, 2.2, 3.5];
    let x = (d + 3.0).clamp(0.0, 7.0);
    let k = (x.floor() as usize).min(6);
    let t = x - k as f64;
    TABLE[k] * (1.0 - t) + TABLE[k + 1] * t
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Redridge: hearth to Darkshire then fly back to Lakeshire, 169 s instead of a 3 min walk.
    #[test]
    fn hearth_not_burnt_for_seconds() {
        let p = Params::default();
        assert!(!p.hearth_worth(185.0, 169.0));
        assert!(p.hearth_worth(1200.0, 300.0));
    }
}
