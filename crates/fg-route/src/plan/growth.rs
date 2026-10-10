//! How the character grows: XP, quest rewards, gear and class power, training.

use super::combat::Combat;
use super::state::State;
use crate::model::{Model, Profile, Quest};
use crate::params::Params;
use crate::xp;

/// How the character grows: XP and levels, quest rewards worn, class powers.
#[derive(Clone, Copy)]
pub(crate) struct Growth<'g> {
    pub(crate) model: &'g Model,
    pub(crate) params: &'g Params,
    pub(crate) profile: &'g Profile,
}

impl<'g> Growth<'g> {
    fn rules(self) -> &'static xp::Rules {
        self.params.edition.rules()
    }

    fn quest_at(self, i: u32) -> &'g Quest {
        &self.model.quests[i as usize]
    }

    /// Levels of power gear, class spells and class quests add (after a level up, new gear,
    /// training, a pet).
    pub(crate) fn refresh_power(self, s: &mut State) {
        let mut bonus = s.class_bonus;
        if let Some(p) = &self.model.power {
            let spells = if p.has_spells() {
                p.spell_bonus(s.trained, s.level)
            } else {
                0.0
            };
            s.gear_bonus = self.params.power_scale * p.gear_bonus(&s.gear, s.level);
            bonus += s.gear_bonus + self.params.power_scale * spells;
        }
        s.bonus = bonus;
    }

    /// Levels of power a class quest's multiplier (`overrides/class_quests.toml`) is worth: the
    /// same fight speed (1.4 = 3.2 levels).
    pub(crate) fn class_power_levels(power: f64) -> f64 {
        power.max(1e-6).ln() / crate::power::LEVEL_LN
    }

    /// Levels of power the rewards of quest `i` would add (the best choice, and a class power
    /// not gained yet), quickly.
    pub(crate) fn reward_gain(self, s: &State, i: u32) -> f64 {
        let class = match self.quest_at(i).power {
            Some((k, power)) if s.powers & (1 << k) == 0 => Self::class_power_levels(power),
            _ => 0.0,
        };
        let Some(p) = &self.model.power else { return class };
        let (fixed, choices) = &self.model.rewards[i as usize];
        if fixed.is_empty() && choices.is_empty() {
            return class;
        }
        let level = s.level.max(self.quest_at(i).level);
        let best = choices
            .iter()
            .map(|&c| p.quick_gain(&s.gear, c, level))
            .fold(0.0, f64::max);
        class + self.params.power_scale * (fixed.iter().map(|&c| p.quick_gain(&s.gear, c, level)).sum::<f64>() + best)
    }

    /// The power a class quest gives (once per quest name).
    pub(crate) fn gain_power(self, s: &mut State, i: u32) {
        if let Some((k, power)) = self.quest_at(i).power
            && s.powers & (1 << k) == 0
        {
            s.powers |= 1 << k;
            s.class_bonus += Self::class_power_levels(power);
            self.refresh_power(s);
        }
    }

    pub(crate) fn gain(self, s: &mut State, amount: i64) {
        s.xp += amount;
        let before = s.level;
        while s.level < self.rules().max_level && s.xp >= self.rules().to_next_level(s.level) {
            s.xp -= self.rules().to_next_level(s.level);
            s.level += 1;
        }
        if s.level != before {
            s.grind_used = 0;
            self.refresh_power(s);
        }
    }

    /// XP grinding (farming on the way included) may still give in the current level.
    pub(crate) fn grind_budget(self, s: &State) -> i64 {
        if self.params.grind_cap >= 1.0 {
            return i64::MAX;
        }
        let cap = self.params.grind_cap.max(0.0) * self.rules().to_next_level(s.level) as f64;
        cap.round() as i64 - s.grind_used
    }

    /// XP the power a quest's rewards add is worth: `gear_value` levels of XP per level of
    /// power (it makes every fight that follows faster).
    pub(crate) fn gear_xp(self, s: &State, i: u32, level: i64) -> f64 {
        if self.params.gear_value <= 0.0 {
            return 0.0;
        }
        let gain = self.reward_gain(s, i);
        if gain <= 0.0 {
            return 0.0;
        }
        gain * self.params.gear_value * self.rules().to_next_level(level) as f64
    }

    /// Time at the class trainers: every class of the group trains (one after the other).
    pub(crate) fn train_time(self) -> f64 {
        self.params.train_time * self.profile.class_ids().len() as f64
    }

    /// XP of turning in a quest at `level`, with the edition's factor for dungeon quests (unless
    /// their XP was received in game, the factor already in).
    pub(crate) fn quest_reward(self, q: &Quest, level: i64) -> i64 {
        let reward = xp::quest_xp(q.xp, q.level, level);
        if !q.xp_observed && q.objectives.iter().any(|o| o.dungeon.is_some()) {
            (reward as f64 * self.rules().dungeon_quest_xp).round() as i64
        } else {
            reward
        }
    }

    /// Grind mobs until `level`, returning the time spent (what goes beyond `grind_cap` is
    /// counted in `grind_over`).
    pub(crate) fn grind_to(self, s: &mut State, level: i64) -> f64 {
        let level = level.min(self.rules().max_level);
        let mut t = 0.0;
        while s.level < level {
            let need = self.rules().to_next_level(s.level) - s.xp;
            let combat = Combat {
                params: self.params,
                dungeons: &self.model.dungeons,
            };
            let rate = combat.grind_rate(s.level, s.bonus, &s.pos);
            let over = (need - self.grind_budget(s).max(0)).max(0);
            t += need as f64 / rate;
            s.spent.grind_over += over as f64 / rate;
            s.spent.grind_xp += need;
            s.grind_used += need;
            self.gain(s, need);
        }
        s.time += t;
        s.spent.grinding += t;
        t
    }
}
