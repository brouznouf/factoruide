//! Doing one stop of a route: accept, objectives (and the mobs met on the way), turn-in, visits, what is done while there.

use super::state::{Event, Kind, State, Stop, Timed};
use super::{Mode, Planner};
use crate::model::{EntityKind, Loc, Model};
use crate::world::Pos;
use crate::xp;
use std::collections::HashMap;

/// Quest NPCs by cell (continent, x, y): (quest, index in its starts or ends, giver?).
pub(crate) type CampIndex = HashMap<(i64, i64, i64), Vec<(u32, u8, bool)>>;

/// Quest givers (NPCs and objects, not items) and turn-in NPCs by cell of `radius`.
pub(crate) fn camp_index(model: &Model, radius: f64) -> CampIndex {
    let mut index = CampIndex::new();
    if radius <= 0.0 {
        return index;
    }
    let cell = |p: &Pos| {
        (
            p.continent,
            (p.x / radius).floor() as i64,
            (p.y / radius).floor() as i64,
        )
    };
    for (i, q) in model.quests.iter().enumerate() {
        for (at, loc) in q.starts.iter().enumerate().filter(|(_, l)| l.kind != EntityKind::Item) {
            index
                .entry(cell(&loc.pos))
                .or_default()
                .push((i as u32, at as u8, true));
        }
        for (at, loc) in q.ends.iter().enumerate() {
            index
                .entry(cell(&loc.pos))
                .or_default()
                .push((i as u32, at as u8, false));
        }
    }
    index
}

impl Planner<'_> {
    /// Class training: on the way when a trainer is close, or a dedicated trip when late.
    /// With class spell data, training is due when the spells the trainer sells make fights
    /// `train_min_gain` faster, late at `train_trip_gain`; else every `train_every` levels
    /// (and for the other classes of a group).
    pub(crate) fn train(&self, s: &mut State, record: &mut impl FnMut(&State, Event)) {
        if self.model.trainers.is_empty() {
            return;
        }
        let every = s.level >= s.trained + self.params.train_every;
        let every_late = s.level >= s.trained + self.params.train_every + self.params.train_max_delay;
        let (due, late) = match self.model.power.as_ref().filter(|p| p.has_spells()) {
            Some(p) => {
                let gain = p.train_gain(s.trained, s.level);
                let group = self.profile.group_class_ids.is_empty();
                (
                    gain >= self.params.train_min_gain || (!group && every),
                    gain >= self.params.train_trip_gain || (!group && every_late),
                )
            }
            None => (every, every_late),
        };
        if !due {
            return;
        }
        let (mut i, trainer) = self
            .model
            .trainers
            .iter()
            .enumerate()
            .min_by(|a, b| s.pos.dist(&a.1.pos).total_cmp(&s.pos.dist(&b.1.pos)))
            .unwrap();
        let distance = s.pos.dist(&trainer.pos);
        if distance <= self.params.train_radius {
            let detour = 2.0 * self.trips().walk(&s.pos, &trainer.pos, s.level);
            s.time += detour;
            s.spent.travel += detour;
        } else if late {
            // Nearest trainer by travel time (it may be worth a flight or the hearthstone).
            let (k, trainer) = self
                .model
                .trainers
                .iter()
                .enumerate()
                .map(|(i, t)| (i, t, self.trips().fastest(s, &t.pos).0))
                .min_by(|a, b| a.2.total_cmp(&b.2))
                .map(|(i, t, _)| (i, t))
                .unwrap();
            let (pos, zone) = (trainer.pos, trainer.zone);
            self.go(s, &pos, zone, record);
            i = k;
        } else {
            return;
        }
        s.time += self.growth().train_time();
        s.spent.training += self.growth().train_time();
        let since = s.trained;
        s.trained = s.level;
        self.growth().refresh_power(s);
        record(s, Event::Train { trainer: i, since });
    }

    /// Give up the quests of the log that can no longer be finished: an objective left in a
    /// dungeon already run or outleveled. They would hold a log slot forever. Also those gone
    /// gray leading nowhere (`gray_dead_end`): they are no longer turned in, so the route turns
    /// them in before or loses their XP. Checked when the level or the dungeons done change.
    pub(crate) fn abandon_hopeless(&self, s: &mut State, record: &mut impl FnMut(&State, Event)) {
        if s.abandon_check == (s.level, s.dungeons) {
            return;
        }
        s.abandon_check = (s.level, s.dungeons);
        for i in 0..self.model.quests.len() {
            if !s.accepted[i] || s.turned[i] {
                continue;
            }
            let q = &self.model.quests[i];
            let hopeless = self.availability().gray_dead_end(s, q, i as u32)
                || q.objectives.iter().enumerate().any(|(k, o)| {
                    s.objectives[i] & (1 << k) == 0
                        && o.dungeon.is_some_and(|d| {
                            !self.availability().dungeon_allowed(d)
                                || s.dungeons & (1u128 << d) != 0
                                || s.level > self.model.dungeons[d].max_level
                        })
                });
            if hopeless {
                s.along.retain(|&(q, _, _)| q as usize != i);
                s.accepted[i] = false;
                s.turned[i] = true;
                s.dropped.push(i as u32);
                s.log = s.log.saturating_sub(1);
                s.time += 2.0;
                record(s, Event::Abandon { quest: i });
            }
        }
    }

    /// Accept quest `i` (looting first the item that starts it, if any).
    pub(crate) fn accept(&self, s: &mut State, i: u32) {
        let q = self.quest(i);
        if q.start_kills > 0.0 || q.start_uses > 0.0 {
            let work = q.start_kills * self.params.kill_time(s.level, s.bonus, q.start_mob_level)
                + q.start_uses * self.params.object_time;
            s.time += work;
            s.spent.fighting += work;
            let start = &q.starts[0].pos;
            let content = xp::content_at(start.continent, start.zone);
            let mob_xp = (q.start_kills
                * self.rules().mob_xp(s.level, q.start_mob_level, content)
                * self.params.group_xp_share()) as i64;
            s.spent.mob_xp += self.growth().gain_kills(s, mob_xp);
        }
        s.accepted[i as usize] = true;
        s.log += 1;
        self.open_along(s, i);
    }

    /// Turn in quest `i`, wearing its reward items when they are upgrades. Returns the item
    /// picked among the choices.
    pub(crate) fn turn_in(&self, s: &mut State, i: u32) -> Option<i64> {
        let q = self.quest(i);
        s.turned[i as usize] = true;
        s.log -= 1;
        s.along.retain(|&(q, _, _)| q != i);
        let reward = self.growth().quest_reward(q, s.level);
        s.spent.quest_xp += reward;
        self.growth().gain(s, reward);
        self.growth().gain_power(s, i);
        let p = self.model.power.as_ref()?;
        let (fixed, choices) = &self.model.rewards[i as usize];
        let mut changed = false;
        for &item in fixed {
            changed |= p.equip(&mut s.gear, item, 1.0, s.level);
        }
        let picked = p.best_choice(&s.gear, choices, s.level).map(|(k, _)| choices[k]);
        if let Some(item) = picked {
            changed |= p.equip(&mut s.gear, item, 1.0, s.level);
        }
        if changed {
            self.growth().refresh_power(s);
        }
        picked.map(|item| p.items[item as usize].id)
    }

    /// Apply a stop (assumed valid). Records events when `trace` is given.
    pub(crate) fn apply(&self, s: &mut State, stop: Stop, mut trace: Option<&mut Vec<Timed>>) {
        let mut record = recorder(&mut trace);
        self.abandon_hopeless(s, &mut record);
        if Self::already_done(s, stop) {
            return;
        }
        self.grind_to_gate(s, stop, &mut record);
        let loc = self.arrive(s, stop, &mut record);
        let picked = self.do_stop(s, stop, &mut record);
        record(s, Event::Stop { stop, loc });
        if let Some(item) = picked {
            record(
                s,
                Event::Reward {
                    quest: stop.index as usize,
                    item,
                },
            );
        }
        self.while_here(s, &mut record);
        self.train(s, &mut record);
    }

    /// Done on the way, or taken/turned in on the spot at an earlier stop.
    fn already_done(s: &State, stop: Stop) -> bool {
        let i = stop.index as usize;
        match stop.kind {
            Kind::Objective(k) => s.objectives[i] & (1 << k) != 0,
            Kind::Accept => s.accepted[i] || s.turned[i],
            Kind::TurnIn => s.turned[i],
            _ => false,
        }
    }

    /// Level the character needs before the stop (the optimizer avoids grinding to it when
    /// quests are better): the quest's own, or the one to fight through the camp around its
    /// giver or turn-in. Quests taken or turned in at hand need it too (`while_here`).
    pub(crate) fn gate_level(&self, s: &State, stop: Stop) -> i64 {
        let i = stop.index as usize;
        match stop.kind {
            Kind::Accept => {
                let q = self.quest(stop.index);
                q.min_level.max(self.combat().guard_level(&q.start_guard, s.bonus))
            }
            Kind::TurnIn => self.combat().guard_level(&self.quest(stop.index).end_guard, s.bonus),
            Kind::Objective(k) => self.combat().level(self.quest(stop.index), k, s.bonus),
            Kind::Dungeon => self.model.dungeons[i].min_level,
            _ => 0,
        }
    }

    fn grind_to_gate(&self, s: &mut State, stop: Stop, record: &mut impl FnMut(&State, Event)) {
        let gate = self.gate_level(s, stop);
        if gate <= s.level || self.mode == Mode::Replay {
            return;
        }
        let (near, zone) = (s.pos, s.zone);
        let seconds = self.growth().grind_to(s, gate);
        record(
            s,
            Event::Grind {
                to_level: gate,
                seconds,
                near,
                zone,
            },
        );
        self.train(s, record);
    }

    /// Travel to the stop, picking up a flight path close to it on the way.
    fn arrive(&self, s: &mut State, stop: Stop, record: &mut impl FnMut(&State, Event)) -> Loc {
        let loc = self.places().loc(s, stop);
        self.go(s, &loc.pos, loc.zone, record);
        s.time += self.params.stop_overhead;
        s.spent.overhead += self.params.stop_overhead;
        if stop.kind != Kind::LearnFlight {
            self.learn_flight_nearby(s, record);
        }
        loc
    }

    fn learn_flight_nearby(&self, s: &mut State, record: &mut impl FnMut(&State, Event)) {
        let Some(node) = self.world.taxi_near(&s.pos, self.params.flight_learn_radius) else {
            return;
        };
        if s.known & (1u128 << node) != 0 {
            return;
        }
        s.known |= 1u128 << node;
        let detour = 2.0 * self.trips().walk(&s.pos, &self.world.taxi_nodes[node].pos, s.level) + 5.0;
        s.time += detour;
        s.spent.travel += detour;
        record(s, Event::LearnFlight { node });
    }

    /// What the stop itself does. Returns the reward item picked on a turn-in.
    fn do_stop(&self, s: &mut State, stop: Stop, record: &mut impl FnMut(&State, Event)) -> Option<i64> {
        let i = stop.index as usize;
        match stop.kind {
            Kind::Accept => self.accept(s, stop.index),
            Kind::Objective(k) => self.complete_objective(s, stop.index, k, record),
            Kind::TurnIn => return self.turn_in(s, stop.index),
            Kind::LearnFlight => {
                s.known |= 1u128 << i;
                s.time += 5.0;
            }
            Kind::Bind => {
                s.bind = i;
                s.time += 10.0;
            }
            Kind::Dungeon => self.run_dungeon(s, i),
        }
        None
    }

    /// Kill or use what the mobs and objects met on the way left to do, and with it the
    /// complementary objectives of the log close by: objects gathered between the kills, while
    /// resting and looking for the next mob (`gather_overlap`), or mobs killed among the objects.
    fn complete_objective(&self, s: &mut State, quest: u32, k: u8, record: &mut impl FnMut(&State, Event)) {
        if s.objectives[quest as usize] & (1 << k) != 0 {
            return;
        }
        let partners = self.gathered_with(s, quest, k);
        let (mut kills, mut gathers) = self.objective_work(s, quest, k);
        for (q, ok) in partners {
            let o = &self.quest(q).objectives[ok as usize];
            let total = if o.kills > 0.0 { o.kills } else { o.uses };
            let done = s
                .along
                .iter()
                .find(|&&(i, j, _)| i == q && j == ok)
                .map_or(0.0, |&(_, _, done)| done);
            let left = (total - done).max(0.0);
            let (k_work, g_work) = self.objective_work(s, q, ok);
            kills += k_work;
            gathers += g_work;
            record(
                s,
                Event::Along {
                    quest: q as usize,
                    objective: ok,
                    kills: left,
                    finished: true,
                },
            );
        }
        let saved = gathers.min(kills * self.params.gather_overlap);
        s.time -= saved;
        s.spent.fighting -= saved;
    }

    /// Objectives of the log done together with objective `k` of `quest`: gathering ones close to
    /// a kill objective, kill ones close to a gathering objective.
    fn gathered_with(&self, s: &State, quest: u32, k: u8) -> Vec<(u32, u8)> {
        let o = &self.quest(quest).objectives[k as usize];
        let gathering = |o: &crate::model::Objective| o.kills <= 0.0 && o.uses > 0.0;
        if self.params.gather_overlap <= 0.0 || o.dungeon.is_some() || (o.kills <= 0.0 && !gathering(o)) {
            return Vec::new();
        }
        let reach = self.params.reach(s.level, s.bonus);
        s.along
            .iter()
            .filter(|&&(q, ok, _)| {
                let other = &self.quest(q).objectives[ok as usize];
                // Done together only when the character could do it on its own stop: its mobs,
                // or the guards around its objects (`Combat::need`).
                (q, ok) != (quest, k)
                    && other.loc.pos.continent == o.loc.pos.continent
                    && other.loc.pos.dist(&o.loc.pos) <= self.params.gather_radius
                    && if o.kills > 0.0 {
                        gathering(other)
                    } else {
                        other.kills > 0.0
                    }
                    && self.combat().need(self.quest(q), other) <= reach
            })
            .map(|&(q, ok, _)| (q, ok))
            .collect()
    }

    /// Do what objective `k` of `quest` still needs, returning the time of its kills and the
    /// rest (objects, fixed extra time).
    fn objective_work(&self, s: &mut State, quest: u32, k: u8) -> (f64, f64) {
        let i = quest as usize;
        let o = &self.quest(quest).objectives[k as usize];
        let done = match s.along.iter().position(|&(q, ok, _)| q == quest && ok == k) {
            Some(n) => s.along.swap_remove(n).2,
            None => 0.0,
        };
        let (kills, uses) = if o.kills > 0.0 {
            ((o.kills - done).max(0.0), o.uses)
        } else {
            (0.0, (o.uses - done).max(0.0))
        };
        // The fixed time, by share of the targets left (those met on the way paid theirs).
        let total = if o.kills > 0.0 { o.kills } else { o.uses };
        let left = if total > 0.0 {
            (total - done).max(0.0) / total
        } else {
            1.0
        };
        let kill_work = kills * self.combat().kill_time(s.level, s.bonus, o);
        let other = uses * self.params.object_time + o.extra * left;
        s.time += kill_work + other;
        s.spent.fighting += kill_work + other;
        let mob_xp = (kills * self.combat().mob_xp(s.level, o)) as i64;
        s.spent.mob_xp += self.growth().gain_kills(s, mob_xp);
        s.objectives[i] |= 1 << k;
        (kill_work, uses * self.params.object_time)
    }

    /// A group run: its time, its mobs' XP, the boss loot the character may win, and every
    /// objective inside it of the quests in the log.
    fn run_dungeon(&self, s: &mut State, i: usize) {
        let d = &self.model.dungeons[i];
        let run = d.minutes * 60.0;
        s.time += run;
        s.spent.dungeons += run;
        s.spent.dungeon_runs += 1;
        let gained = (d.kills
            * self.rules().mob_xp(s.level, d.mob_level, d.content)
            * self.rules().dungeon_kill_xp
            * self.params.dungeon_xp_share) as i64;
        s.spent.mob_xp += self.growth().gain_kills(s, gained);
        s.dungeons |= 1u128 << i;
        if let Some(p) = &self.model.power {
            let mut changed = false;
            for &(item, chance) in &self.model.dungeon_loot[i] {
                changed |= p.equip(&mut s.gear, item, chance, s.level);
            }
            if changed {
                self.growth().refresh_power(s);
            }
        }
        for (qi, q) in self.model.quests.iter().enumerate() {
            if s.accepted[qi] && !s.turned[qi] {
                for (k, o) in q.objectives.iter().enumerate() {
                    if o.dungeon == Some(i) {
                        s.objectives[qi] |= 1 << k;
                    }
                }
            }
        }
    }

    /// Final grind to the target level and the cost of class quests left out.
    pub(crate) fn finish(&self, s: &mut State, mut trace: Option<&mut Vec<Timed>>) -> f64 {
        let mut record = recorder(&mut trace);
        if s.level < self.profile.to_level {
            let (near, zone) = (s.pos, s.zone);
            let seconds = self.growth().grind_to(s, self.profile.to_level);
            record(
                s,
                Event::Grind {
                    to_level: self.profile.to_level,
                    seconds,
                    near,
                    zone,
                },
            );
        }
        let missing = self
            .model
            .quests
            .iter()
            .enumerate()
            .filter(|(i, q)| q.mandatory && !s.rewarded(*i) && !self.availability().alternative_done(s, q))
            .count();
        s.spent.missing_class_quests = missing as u32;
        let dungeons = (0..self.model.dungeons.len())
            .filter(|&d| {
                self.availability().dungeon_forced(d)
                    && s.dungeons & (1u128 << d) == 0
                    && self.model.dungeons[d].min_level <= self.profile.to_level
            })
            .count();
        s.spent.missing_dungeons = dungeons as u32;
        s.spent.total = s.time;
        // Optimizer score: grinding and flights weigh more than their real time, gold spent on
        // flights and missing goals cost.
        s.time + self.weights(&s.spent) + (missing + dungeons) as f64 * self.params.mandatory_penalty
    }
}

/// Appends the events of a replay to its trace, when one is kept.
pub(crate) fn recorder<'t>(trace: &'t mut Option<&mut Vec<Timed>>) -> impl FnMut(&State, Event) + 't {
    move |s: &State, event: Event| {
        if let Some(t) = trace.as_deref_mut() {
            t.push(Timed {
                event,
                time: s.time,
                level: s.level,
                xp: s.xp,
                power: s.level as f64 + s.bonus,
                gear: s.gear_bonus,
            });
        }
    }
}
