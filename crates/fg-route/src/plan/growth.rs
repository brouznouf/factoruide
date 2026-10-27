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

    /// The loot of `kills` mobs into the bags (`Params::bag_weight`): what no longer fits is
    /// left behind, at a cost.
    pub(crate) fn fill_bags(self, s: &mut State, kills: f64) {
        if self.params.bag_weight <= 0.0 || kills <= 0.0 {
            return;
        }
        let room = (self.bag_room(s.level) - f64::from(s.held) - f64::from(s.bags)).max(0.0);
        let loot = kills * self.params.loot_per_kill;
        let kept = loot.min(room);
        s.bags += kept as f32;
        s.spent.bag_overflow += (loot - kept) * self.params.bag_weight;
    }

    /// Bag slots left for loot and quest items at `level` (`Params::bag_weight`): the bags less
    /// what is always carried (hearthstone, food, drink, potions), the class's own (a hunter's
    /// quiver and a warlock's soul pouch take a bag, a shaman's totems a slot each, a rogue's
    /// poisons two) and the gathering professions' materials (`Params::gather_share` each). In a
    /// group, the fullest bags of its players: they go to the vendor together.
    pub(crate) fn bag_room(self, level: i64) -> f64 {
        const CARRIED: f64 = 4.0;
        let slots = crate::params::bag_slots(level);
        let bag = (slots - crate::params::bag_slots(1)) / 4.0;
        let room = |class_id: i64, professions: &[crate::profession::ProfessionGoal]| {
            let class = match class_id {
                3 => bag,
                9 if level >= 10 => bag,
                7 => [4, 10, 20, 30].iter().filter(|&&l| level >= l).count() as f64,
                4 if level >= 20 => 2.0,
                _ => 0.0,
            };
            let gathering = (professions.iter())
                .filter(|g| {
                    crate::profession::find(&g.key).is_some_and(|d| d.kind == crate::profession::Kind::Gathering)
                        && level >= g.start_level.unwrap_or(crate::profession::START_LEVEL)
                })
                .count() as f64;
            slots - CARRIED - class - gathering * self.params.gather_share * slots
        };
        (self.profile.mates.iter())
            .map(|m| room(m.class_id, &m.professions))
            .fold(room(self.profile.class_id, &self.profile.professions), f64::min)
    }

    /// Common loot `kills` normal mobs of `zone` drop (`Model::loot`), into the bags.
    pub(crate) fn gather(self, s: &mut State, zone: i64, kills: f64) {
        self.fill_bags(s, kills);
        if let Some(items) = self.model.loot.by_zone.get(&zone) {
            for &(i, each) in items {
                s.stock[usize::from(i)] += kills as f32 * each;
            }
        }
    }

    /// Levels of power gear, class spells and class quests add (after a level up, new gear,
    /// training, a pet).
    pub(crate) fn refresh_power(self, s: &mut State) {
        let mut bonus = s.class_bonus;
        if let Some(p) = &self.model.power {
            let spells = if p.has_spells() {
                p.spell_bonus(s.trained, s.level, s.abilities)
            } else {
                0.0
            };
            s.gear_bonus = self.params.power_scale * p.gear_bonus(&s.gear, s.level);
            s.gear_quality = p.gear_quality(&s.gear, s.level);
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
        let mut class = match self.quest_at(i).power {
            Some((k, power)) if s.powers & (1 << k) == 0 => Self::class_power_levels(power),
            _ => 0.0,
        };
        let Some(p) = &self.model.power else { return class };
        class += self.params.power_scale
            * p.quest_gain(
                s.abilities,
                s.abilities | self.model.abilities.quest(self.quest_at(i).id),
                s.level.max(self.quest_at(i).min_level),
            );
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

    /// Abilities and power actually granted by completing a class quest.
    pub(crate) fn gain_power(self, s: &mut State, i: u32) {
        s.abilities |= self.model.abilities.quest(self.quest_at(i).id);
        if let Some((k, power)) = self.quest_at(i).power
            && s.powers & (1 << k) == 0
        {
            s.powers |= 1 << k;
            s.class_bonus += Self::class_power_levels(power);
        }
        self.refresh_power(s);
    }

    /// XP of kills: doubled while the character is rested, up to its rested XP. Returns the XP
    /// gained.
    pub(crate) fn gain_kills(self, s: &mut State, amount: i64) -> i64 {
        self.mates_kill(s, amount);
        let bonus = amount.min(s.rested).max(0);
        s.rested -= bonus;
        self.gain(s, amount + bonus);
        amount + bonus
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
            s.farm_used = 0;
            self.refresh_power(s);
        }
    }

    /// The other players of a group guide (`State::mates`).
    pub(crate) fn mates(self) -> usize {
        self.model.initial.mates.len()
    }

    /// The other players' XP of kills worth `amount` to the character: the same mobs, at their
    /// own level (fewer when above it, none once gray), doubled while they are rested.
    pub(crate) fn mates_kill(self, s: &mut State, amount: i64) {
        let n = self.mates();
        if n == 0 || amount <= 0 {
            return;
        }
        let rules = self.rules();
        let content = xp::content_at(s.pos.continent, s.pos.zone);
        let own = rules.mob_xp(s.level, s.level, content);
        for k in 0..n {
            let m = &mut s.mates[k];
            let each = if own > 0.0 {
                rules.mob_xp(m.level, s.level, content) / own
            } else {
                1.0
            };
            let gained = (amount as f64 * each) as i64;
            let bonus = gained.min(m.rested).max(0);
            m.rested -= bonus;
            Self::mate_gain(rules, m, gained + bonus);
        }
    }

    fn mate_gain(rules: &xp::Rules, m: &mut super::state::MateXp, amount: i64) {
        m.xp += amount;
        while m.level < rules.max_level && m.xp >= rules.to_next_level(m.level) {
            m.xp -= rules.to_next_level(m.level);
            m.level += 1;
        }
    }

    /// The XP of turning in quest `i` to each member it is still for (`Initial::need`); returns
    /// the character's.
    pub(crate) fn reward(self, s: &mut State, i: u32) -> i64 {
        let q = self.quest_at(i);
        let need = self.model.initial.need(i as usize);
        for k in 0..self.mates() {
            if need & (1 << s.order[k + 1]) != 0 {
                let m = &mut s.mates[k];
                Self::mate_gain(self.rules(), m, self.quest_reward(q, m.level));
            }
        }
        if need & (1 << s.order[0]) == 0 {
            return 0;
        }
        let reward = self.quest_reward(q, s.level);
        self.gain(s, reward);
        reward
    }

    /// XP of discovering an area of level `area`, to everyone in the group.
    pub(crate) fn explore(self, s: &mut State, area: i64) -> i64 {
        for k in 0..self.mates() {
            let m = &mut s.mates[k];
            Self::mate_gain(self.rules(), m, self.rules().exploration_xp(m.level, area));
        }
        let xp = self.rules().exploration_xp(s.level, area);
        self.gain(s, xp);
        xp
    }

    /// How far apart the group's levels are (0 alone).
    pub(crate) fn spread(self, s: &State) -> f64 {
        let rules = self.rules();
        let at = |level: i64, xp: i64| level as f64 + xp as f64 / rules.to_next_level(level).max(1) as f64;
        let own = at(s.level, s.xp);
        let (mut lo, mut hi) = (own, own);
        for m in &s.mates[..self.mates()] {
            let l = at(m.level, m.xp);
            lo = lo.min(l);
            hi = hi.max(l);
        }
        hi - lo
    }

    /// Lowest level in the group.
    pub(crate) fn lowest_level(self, s: &State) -> i64 {
        s.mates[..self.mates()].iter().map(|m| m.level).fold(s.level, i64::min)
    }

    /// XP grinding may still give in the current level.
    pub(crate) fn grind_budget(self, s: &State) -> i64 {
        self.budget(s, self.params.grind_cap, s.grind_used)
    }

    /// XP farming on the way may still give in the current level.
    pub(crate) fn farm_budget(self, s: &State) -> i64 {
        self.budget(s, self.params.farm_cap, s.farm_used)
    }

    fn budget(self, s: &State, cap: f64, used: i64) -> i64 {
        if cap >= 1.0 {
            return i64::MAX;
        }
        (cap.max(0.0) * self.rules().to_next_level(s.level) as f64).round() as i64 - used
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

    /// Time `grind_to` would take to `level`, without grinding.
    pub(crate) fn grind_time(self, s: &State, level: i64) -> f64 {
        let combat = Combat {
            params: self.params,
            dungeons: &self.model.dungeons,
        };
        let mut xp = s.xp;
        (s.level..level.min(self.rules().max_level))
            .map(|l| {
                let need = self.rules().to_next_level(l) - xp;
                xp = 0;
                need as f64 / combat.grind_rate(l, s.bonus, &s.pos)
            })
            .sum()
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
            // Rested XP doubles the kills' XP: only half of what it covers is killed.
            let killed = need - (need / 2).min(s.rested.max(0));
            let over = (need - self.grind_budget(s).max(0)).max(0);
            let time = killed as f64 / rate;
            let each = self
                .rules()
                .mob_xp(s.level, s.level, xp::content_at(s.pos.continent, s.pos.zone))
                * self.params.group_xp_share();
            if each > 0.0 {
                self.gather(s, s.pos.zone, killed as f64 / each);
            }
            t += time;
            s.spent.grind_over += over as f64 / need as f64 * time;
            s.spent.grind_xp += need;
            s.grind_used += need;
            s.rested -= need - killed;
            self.mates_kill(s, killed);
            self.gain(s, need);
        }
        s.time += t;
        s.spent.grinding += t;
        t
    }

    /// Kill `mobs` mobs of the character's level where it stands, returning the time spent
    /// (counted as grinding, beyond `grind_cap` in `grind_over`).
    pub(crate) fn grind_mobs(self, s: &mut State, mobs: u32) -> f64 {
        let combat = Combat {
            params: self.params,
            dungeons: &self.model.dungeons,
        };
        let content = xp::content_at(s.pos.continent, s.pos.zone);
        let mut t = 0.0;
        for _ in 0..mobs {
            if s.level >= self.rules().max_level {
                break;
            }
            let each = (self.rules().mob_xp(s.level, s.level, content) * self.params.group_xp_share()) as i64;
            let time = each as f64 / combat.grind_rate(s.level, s.bonus, &s.pos);
            if self.grind_budget(s) <= 0 {
                s.spent.grind_over += time;
            }
            t += time;
            self.gather(s, s.pos.zone, 1.0);
            s.grind_used += each;
            s.spent.grind_xp += self.gain_kills(s, each);
        }
        s.time += t;
        s.spent.grinding += t;
        t
    }
}
