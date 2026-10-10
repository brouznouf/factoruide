//! Whether a quest is worth its trip: its cost, its XP and what it unlocks, against grinding.

use super::availability::Availability;
use super::combat::Combat;
use super::growth::Growth;
use super::state::State;
use super::travel::Trips;
use crate::model::{Model, Quest};
use crate::params::Params;
use crate::xp;

/// Whether a quest is worth its trip, from what it costs (trips, fights) and what it brings
/// (XP, rewards, the chain it opens), against grinding.
#[derive(Clone, Copy)]
pub(crate) struct Worth<'w> {
    pub(crate) model: &'w Model,
    pub(crate) params: &'w Params,
    pub(crate) combat: Combat<'w>,
    pub(crate) trips: Trips<'w>,
    pub(crate) growth: Growth<'w>,
    pub(crate) availability: Availability<'w>,
}

impl Worth<'_> {
    fn quest(&self, i: u32) -> &Quest {
        &self.model.quests[i as usize]
    }

    /// Rough standalone cost of a quest, to skip quests not worth the trip.
    pub(crate) fn quest_cost(&self, i: u32, level: i64, bonus: f64) -> f64 {
        let q = self.quest(i);
        let mut path = vec![q.starts[0].pos];
        path.extend(q.objectives.iter().map(|o| o.loc.pos));
        path.push(q.ends[0].pos);
        // Walking or transport links (boats, zeppelins): quests may cross continents.
        let travel: f64 = path.windows(2).map(|w| self.trips.leg(&w[0], &w[1], level, 0).0).sum();
        let work: f64 = q
            .objectives
            .iter()
            .map(|o| o.kills * self.combat.kill_time(level, bonus, o) + o.uses * self.params.object_time + o.extra)
            .sum();
        let start = q.start_kills * self.params.kill_time(level, bonus, q.start_mob_level)
            + q.start_uses * self.params.object_time;
        travel + work + start + self.params.stop_overhead * (2 + q.objectives.len()) as f64
    }

    pub(crate) fn quest_value(&self, i: u32, level: i64) -> f64 {
        let q = self.quest(i);
        let mobs: f64 = q
            .objectives
            .iter()
            .map(|o| o.kills * self.combat.mob_xp(level, o))
            .sum();
        self.growth.quest_reward(q, level) as f64 + mobs
    }

    /// Value used to decide whether to take a quest: its own value plus the chain it unlocks,
    /// reduced like its XP when the quest is far below the character (a starting zone's chain
    /// is not worth coming back for).
    pub(crate) fn quest_worth(&self, i: u32, level: i64) -> f64 {
        let late = xp::quest_xp(1000, self.quest(i).level, level) as f64 / 1000.0;
        self.quest_value(i, level) + self.model.unlocks[i as usize] * late
    }

    /// Whether a quest beats grinding, counting the trip from where we are.
    pub(crate) fn worth_it(&self, s: &State, i: u32, level: i64, trip: f64) -> bool {
        let q = self.quest(i);
        let bonus = s.bonus;
        // Content far beyond our reach would block a quest log slot until we level up.
        let hardest = q
            .objectives
            .iter()
            .filter(|o| o.dungeon.is_none())
            .map(|o| self.combat.need(q, o))
            .fold(0.0, f64::max);
        if hardest > self.params.reach(level + 1, bonus) {
            return false;
        }
        if q.mandatory {
            return true;
        }
        // Dungeon quests: picked up on the way well before the run, kept until it.
        if let Some(d) = q.objectives.iter().find_map(|o| o.dungeon) {
            let dungeon = &self.model.dungeons[d];
            return self.availability.dungeon_allowed(d)
                && level <= dungeon.max_level
                && level + self.params.dungeon_pickup_ahead >= dungeon.min_level;
        }
        let worth = self.quest_worth(i, level) + self.growth.gear_xp(s, i, level);
        let efficiency = worth / (trip + self.quest_cost(i, level, bonus)).max(1.0);
        // Grinding weighs grind_weight times its duration: compare with its effective rate.
        efficiency
            >= self.params.min_efficiency * self.combat.grind_rate(level, bonus, &s.pos)
                / self.params.grind_weight().max(1.0)
    }

    /// Accept weight from the quest's XP per second compared with grinding: the better the
    /// quest, the further we go for it.
    pub(crate) fn efficiency_weight(&self, s: &State, i: u32, level: i64) -> f64 {
        if self.params.efficiency_power <= 0.0 {
            return 1.0;
        }
        let efficiency = (self.quest_worth(i, level) + self.growth.gear_xp(s, i, level))
            / self.quest_cost(i, level, s.bonus).max(1.0);
        let reference = self.combat.grind_rate(level, s.bonus, &s.pos) * 2.0;
        (reference / efficiency.max(1e-6))
            .powf(self.params.efficiency_power)
            .clamp(0.4, 3.0)
    }

    /// Accept weight from the quest level: quests well below our level will be turned in late
    /// with reduced XP (quests are usually done 1-3 levels below the character). Quests above our level
    /// mean slow kills.
    pub(crate) fn level_fit(&self, i: u32, level: i64) -> f64 {
        let below = level - self.quest(i).level - self.params.fit_free_levels;
        let above = self.quest(i).level - level;
        let mut w = 1.0;
        if below > 0 {
            w += self.params.fit_penalty * below as f64;
        }
        if above > 0 {
            w += self.params.fit_above_penalty * above as f64;
        }
        w
    }
}
