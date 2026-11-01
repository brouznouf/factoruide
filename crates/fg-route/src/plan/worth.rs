//! Whether a quest is worth its trip: its cost, its XP and what it unlocks, against grinding.

use super::availability::Availability;
use super::combat::Combat;
use super::growth::Growth;
use super::state::State;
use super::travel::Trips;
use crate::model::{Model, Objective, Quest};
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
    pub(crate) to_level: i64,
}

impl Worth<'_> {
    fn quest(&self, i: u32) -> &Quest {
        &self.model.quests[i as usize]
    }

    /// Rough standalone cost of a quest, to skip quests not worth the trip.
    pub(crate) fn quest_cost(&self, i: u32, level: i64, bonus: f64, abilities: u64) -> f64 {
        let q = self.quest(i);
        let trips = Trips {
            abilities,
            ..self.trips
        };
        let leg = |a: &crate::world::Pos, b: &crate::world::Pos| trips.leg(a, b, level, 0, u8::MAX).0;
        let work_at = |o: &Objective| {
            o.kills * self.combat.kill_time(level, bonus, o) + o.uses * self.params.object_time + o.extra
        };
        // Each objective at its cheapest place, its work and the way from the giver to the
        // turn-in through it (an item bought at an auction house rather than waited for).
        let (start, end) = (q.starts[0].pos, q.ends[0].pos);
        let places: Vec<&Objective> = (q.objectives.iter())
            .map(|o| {
                if o.alts.is_empty() {
                    return o;
                }
                let cost = |p: &Objective| work_at(p) + leg(&start, &p.loc.pos) + leg(&p.loc.pos, &end);
                (o.places()).min_by(|a, b| cost(a).total_cmp(&cost(b))).unwrap_or(o)
            })
            .collect();
        let mut path = vec![start];
        path.extend(places.iter().map(|o| o.loc.pos));
        path.push(end);
        // Walking or transport links (boats, zeppelins): quests may cross continents.
        let travel: f64 = path.windows(2).map(|w| leg(&w[0], &w[1])).sum();
        let work: f64 = places.iter().map(|o| work_at(o)).sum();
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
        let might = s.might();
        // Content far beyond our reach would block a quest log slot until we level up.
        let hardest = q
            .objectives
            .iter()
            .filter(|o| o.dungeon.is_none())
            .map(|o| self.combat.need(q, o))
            .fold(0.0, f64::max);
        if hardest > self.params.reach(level + 1, might) {
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
        // Past the target level (forced zones, reputation goals), XP is worth nothing: only the
        // reputation the goals still miss is.
        let past = s.level >= self.to_level;
        let worth = if past {
            self.reputation_xp(s, i, level)
        } else {
            self.quest_worth(i, level) + self.growth.gear_xp(s, i, level) + self.reputation_xp(s, i, level)
        };
        if past && worth <= 0.0 {
            return false;
        }
        self.beats_grinding(s, worth, trip + self.quest_cost(i, level, s.bonus, s.abilities), level)
    }

    /// Whether `worth` XP for `cost` seconds beats grinding.
    fn beats_grinding(&self, s: &State, worth: f64, cost: f64, level: i64) -> bool {
        let efficiency = worth / cost.max(1.0);
        // Grinding weighs grind_weight times its duration: compare with its effective rate.
        efficiency
            >= self.params.min_efficiency * self.combat.grind_rate(level, s.bonus, &s.pos)
                / self.params.grind_weight().max(1.0)
    }

    /// Whether quest `i`, in the log, is worth finishing from where the character stands: what
    /// is left of it (its objectives not done, its turn-in) against grinding, as `worth_it`
    /// weighs a quest to take.
    pub(crate) fn finish_worth_it(&self, s: &State, i: u32) -> bool {
        let q = self.quest(i);
        let level = s.level;
        let left: Vec<_> = (q.objectives.iter().enumerate())
            .filter(|(k, _)| s.objectives[i as usize] & (1 << k) == 0)
            .map(|(_, o)| o)
            .collect();
        let hardest = (left.iter())
            .filter(|o| o.dungeon.is_none())
            .map(|o| self.combat.need(q, o))
            .fold(0.0, f64::max);
        if hardest > self.params.reach(level + 1, s.might()) {
            return false;
        }
        if q.mandatory {
            return true;
        }
        if let Some(d) = left.iter().find_map(|o| o.dungeon) {
            let dungeon = &self.model.dungeons[d];
            return self.availability.dungeon_allowed(d)
                && level <= dungeon.max_level
                && level + self.params.dungeon_pickup_ahead >= dungeon.min_level;
        }
        let past = level >= self.to_level;
        let mobs: f64 = left.iter().map(|o| o.kills * self.combat.mob_xp(level, o)).sum();
        let worth = if past {
            self.reputation_xp(s, i, level)
        } else {
            self.quest_worth(i, level) + mobs + self.growth.gear_xp(s, i, level) + self.reputation_xp(s, i, level)
        };
        if past && worth <= 0.0 {
            return false;
        }
        let mut path = vec![s.pos];
        path.extend(left.iter().map(|o| o.loc.pos));
        path.push(q.ends[0].pos);
        let travel: f64 = (path.windows(2))
            .map(|w| self.trips.at(s).leg(&w[0], &w[1], level, 0, u8::MAX).0)
            .sum();
        let work: f64 = (left.iter())
            .map(|o| o.kills * self.combat.kill_time(level, s.bonus, o) + o.uses * self.params.object_time + o.extra)
            .sum();
        let cost = travel + work + self.params.stop_overhead * (1 + left.len()) as f64;
        self.beats_grinding(s, worth, cost, level)
    }

    /// What the reputation a quest gives towards the goals not reached yet is worth, in XP: the
    /// score time it saves (`Params::reputation_weight` per point, and a required quest's
    /// penalty when it meets a goal), at the rate grinding fills score time.
    pub(crate) fn reputation_xp(&self, s: &State, i: u32, level: i64) -> f64 {
        let goals = &self.params.reputation_goals;
        if goals.is_empty() {
            return 0.0;
        }
        let seconds: f64 = (goals.iter().zip(&s.reputation))
            .map(|(goal, &have)| {
                let gain: f64 = (self.quest(i).reputation.iter())
                    .filter(|(f, _)| *f == goal.faction)
                    .map(|(_, v)| v)
                    .sum();
                let missing = (goal.target as f64 - f64::from(have)).max(0.0);
                let met = if missing > 0.0 && gain >= missing {
                    self.params.mandatory_penalty
                } else {
                    0.0
                };
                gain.min(missing) * self.params.reputation_weight + met
            })
            .sum();
        seconds * self.combat.grind_rate(level, s.bonus, &s.pos) / self.params.grind_weight().max(1.0)
    }

    /// Accept weight from the quest's XP per second compared with grinding: the better the
    /// quest, the further we go for it.
    pub(crate) fn efficiency_weight(&self, s: &State, i: u32, level: i64) -> f64 {
        if self.params.efficiency_power <= 0.0 {
            return 1.0;
        }
        let efficiency =
            (self.quest_worth(i, level) + self.growth.gear_xp(s, i, level) + self.reputation_xp(s, i, level))
                / self.quest_cost(i, level, s.bonus, s.abilities).max(1.0);
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
