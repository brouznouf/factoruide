//! Doing one stop of a route: accept, objectives (and the mobs met on the way), turn-in, visits, what is done while there.

use super::state::{Event, Kind, Landed, State, Stop, Timed, Together};
use super::{Mode, Planner};
use crate::model::{EntityKind, Loc, Model, Objective};
use crate::world::Pos;
use crate::xp;
use rustc_hash::FxHashMap;

/// Quest NPCs by cell (continent, x, y): (quest, index in its starts or ends, giver?, and the
/// NPC's x, y, to tell the ones at hand without reading the quest).
pub(crate) type CampIndex = FxHashMap<(i64, i64, i64), Vec<(u32, u8, bool, f64, f64)>>;

/// Quest givers (NPCs and objects, not items) and turn-in NPCs by cell of `radius`.
pub(crate) fn camp_index(model: &Model, radius: f64) -> CampIndex {
    let mut index = CampIndex::default();
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
                .push((i as u32, at as u8, true, loc.pos.x, loc.pos.y));
        }
        for (at, loc) in q.ends.iter().enumerate() {
            index
                .entry(cell(&loc.pos))
                .or_default()
                .push((i as u32, at as u8, false, loc.pos.x, loc.pos.y));
        }
    }
    index
}

/// Share of a timed quest's limit the route may use from taking it to turning it in: the rest
/// is a margin for the time estimates.
const TIMED_SHARE: f64 = 0.75;

impl Planner<'_> {
    /// Class training: on the way when a trainer is close, or a dedicated trip when late.
    /// With class spell data, training is due when the spells the trainer sells make fights
    /// `train_min_gain` faster, late at `train_trip_gain`; else every `train_every` levels
    /// (and for the other classes of a group).
    pub(crate) fn train(&self, s: &mut State, record: &mut impl FnMut(&State, Event)) {
        // Nothing left to fight for once at the target level (the guide ends there).
        if self.model.trainers.is_empty() || self.explicit || s.level >= self.profile.to_level {
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
        let (near, trainer) = self
            .model
            .trainers
            .iter()
            .enumerate()
            .min_by(|a, b| s.pos.dist(&a.1.pos).total_cmp(&s.pos.dist(&b.1.pos)))
            .unwrap();
        let i = if s.pos.dist(&trainer.pos) <= self.params.train_radius {
            near
        } else if late {
            // Nearest trainer by travel time (it may be worth a flight or the hearthstone).
            self.model
                .trainers
                .iter()
                .enumerate()
                .map(|(i, t)| (i, self.trips().fastest(s, &t.pos).0))
                .min_by(|a, b| a.1.total_cmp(&b.1))
                .map(|(i, _)| i)
                .unwrap()
        } else {
            return;
        };
        self.train_at(s, i, record);
    }

    /// Train at trainer `i`: a detour when it is close (`train_radius`), else a trip there (the
    /// end of the trip just off a flight, boat or teleport step).
    pub(crate) fn train_at(&self, s: &mut State, i: usize, record: &mut impl FnMut(&State, Event)) {
        let trainer = &self.model.trainers[i];
        // Close as the crow flies, but maybe not on foot (another side of the mountains).
        let detour = (s.landed == Landed::No && s.pos.dist(&trainer.pos) <= self.params.train_radius)
            .then(|| 2.0 * self.trips().walk(&s.pos, &trainer.pos, s.level))
            .filter(|d| d.is_finite());
        if let Some(detour) = detour {
            s.time += detour;
            s.spent.travel += detour;
        } else {
            let (pos, zone) = (trainer.pos, trainer.zone);
            self.go(s, &pos, zone, record);
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
    /// Those the route took count against it (`Params::abandon_weight`); a quest of the
    /// character's log it does not take was not its choice.
    pub(crate) fn abandon_hopeless(&self, s: &mut State, record: &mut impl FnMut(&State, Event)) {
        if self.explicit || s.abandon_check == (s.level, s.dungeons) {
            return;
        }
        s.abandon_check = (s.level, s.dungeons);
        for i in s.accepted.and_not(&s.turned) {
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
                if s.planned.flags.get(i).is_some_and(|f| f & 1 != 0) {
                    s.spent.abandoned += 1;
                }
                Self::abandon(s, i as u32, record);
            }
        }
    }

    /// Give up quest `i` of the log.
    pub(crate) fn abandon(s: &mut State, i: u32, record: &mut impl FnMut(&State, Event)) {
        s.started.retain(|&(q, _, _)| q != i);
        s.timers.retain(|&(q, _)| q != i);
        s.accepted.set(i as usize, false);
        s.turned.set(i as usize, true);
        s.dropped.push(i);
        s.log = s.log.saturating_sub(1);
        s.time += 2.0;
        record(s, Event::Abandon { quest: i as usize });
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
            self.growth().gather(s, q.starts[0].zone, q.start_kills);
        }
        s.accepted.set(i as usize, true);
        s.log += 1;
        if let Some(limit) = q.time_limit {
            s.timers.push((i, s.time + limit * TIMED_SHARE));
        }
    }

    /// Turn in quest `i`, wearing its reward items when they are upgrades. Returns the item
    /// picked among the choices.
    pub(crate) fn turn_in(&self, s: &mut State, i: u32) -> Option<i64> {
        let q = self.quest(i);
        s.timers.retain(|&(q, _)| q != i);
        s.turned.set(i as usize, true);
        s.log -= 1;
        s.started.retain(|&(q, _, _)| q != i);
        let reward = self.growth().quest_reward(q, s.level);
        s.spent.quest_xp += reward;
        self.growth().gain(s, reward);
        self.growth().gain_power(s, i);
        if s.forced_left > 0 {
            let closed = (q.exclusive.iter())
                .filter_map(|x| self.model.index.get(x))
                .filter(|&&j| self.model.quests[j].zone_forced && !s.turned[j])
                .count() as u32;
            s.forced_left = s.forced_left.saturating_sub(u32::from(q.zone_forced) + closed);
        }
        for (g, goal) in self.params.reputation_goals.iter().enumerate() {
            for &(faction, points) in &q.reputation {
                if faction == goal.faction {
                    s.reputation[g] += points as f32;
                }
            }
        }
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

    /// A timed quest reached after its time (see `TIMED_SHARE`) has failed: given up instead of
    /// turned in (no reward, no follow-up), which the optimizer avoids. Returns whether it did.
    pub(crate) fn failed_in_time(s: &mut State, i: u32, record: &mut impl FnMut(&State, Event)) -> bool {
        if !s.timers.iter().any(|&(q, deadline)| q == i && s.time > deadline) {
            return false;
        }
        Self::abandon(s, i, record);
        true
    }

    /// Apply a stop (assumed valid). Records events when `trace` is given. The quests gone
    /// hopeless are given up before and after: the next stop is judged (`valid`) on the log left
    /// once the level reached here has made some of them gray, as the player would see it.
    pub(crate) fn apply(&self, s: &mut State, stop: Stop, mut trace: Option<&mut Vec<Timed>>) {
        let mut record = recorder(&mut trace);
        self.abandon_hopeless(s, &mut record);
        let before = s.time;
        if !Self::already_done(s, stop) {
            let target = self.places().pos(s, stop);
            if stop.kind != Kind::Dungeon {
                self.service_visits(s, Some(target), false, true, &mut record);
            }
        }
        if !s.time.is_finite() {
            return;
        }
        self.do_apply(s, stop, &mut record);
        self.service_visits(s, s.next, false, false, &mut record);
        // Counted in the zone the stop ends in.
        let preferred = &self.params.preferred_zones;
        if !preferred.is_empty() && !preferred.contains(&s.zone) {
            s.spent.outside_preferred += s.time - before;
        }
        self.abandon_hopeless(s, &mut record);
    }

    fn do_apply(&self, s: &mut State, stop: Stop, record: &mut impl FnMut(&State, Event)) {
        if Self::already_done(s, stop) {
            return;
        }
        // Done together: objective stops one after the other only.
        if !matches!(stop.kind, Kind::Objective(_)) {
            s.together = Together::default();
        }
        if stop.kind == Kind::Hearth {
            s.landed = Landed::No;
            if self.hearth(s, stop.index == 1, record) {
                self.while_here(s, record);
                self.train(s, record);
            }
            return;
        }
        // Failed already: given up where the character stands, no trip to the turn-in.
        if stop.kind == Kind::TurnIn && Self::failed_in_time(s, stop.index, record) {
            return;
        }
        // Chosen once, before grinding to it: the place the stop is gated by is the one gone to.
        let spot = self.spot_of(s, stop);
        self.grind_to_gate(s, stop, spot, record);
        // Level-gating grinds may take hours. Service immediately before the
        // actual trip to the entrance, after that grinding has finished.
        if stop.kind == Kind::Dungeon {
            self.service_visits(s, None, true, false, record);
            if !s.time.is_finite() {
                return;
            }
        }
        let loc = self.arrive(s, stop, spot, record);
        if !(stop.kind == Kind::TurnIn && Self::failed_in_time(s, stop.index, record)) {
            let picked = self.do_stop(s, stop, spot);
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
        }
        if !self.explicit && stop.kind == Kind::Dungeon && (self.params.vendor_visits || self.params.mailbox_visits) {
            record(
                s,
                Event::DungeonExit {
                    dungeon: stop.index as usize,
                },
            );
            self.service_visits(s, s.next, false, false, record);
        }
        self.while_here(s, record);
        self.train(s, record);
    }

    /// Back to the inn the hearthstone is bound to, with the hearthstone or Astral Recall
    /// (`recall`), when it is learned and ready. Returns whether it was.
    pub(crate) fn hearth(&self, s: &mut State, recall: bool, record: &mut impl FnMut(&State, Event)) -> bool {
        let (ready, cooldown, cast) = if recall {
            match self.model.recall {
                Some(level) if level <= s.trained => {
                    (s.recall_ready, self.params.recall_cooldown, self.params.teleport_cast)
                }
                _ => return false,
            }
        } else {
            (s.hearth_ready, self.params.hearth_cooldown, self.params.hearth_cast)
        };
        if s.time < ready {
            return false;
        }
        record(s, Event::Hearth { inn: s.bind, recall });
        s.time += cast;
        s.spent.travel += cast;
        s.spent.hearths += 1;
        let ready = s.time - cast + cooldown;
        if recall {
            s.recall_ready = ready;
        } else {
            s.hearth_ready = ready;
        }
        let inn = &self.model.inns[s.bind];
        s.pos = inn.pos;
        if inn.zone != s.zone {
            s.spent.zone_changes += 1;
        }
        s.zone = inn.zone;
        true
    }

    /// Done on the way, or taken/turned in on the spot at an earlier stop.
    pub(crate) fn already_done(s: &State, stop: Stop) -> bool {
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
        self.gate_level_at(s, stop, self.spot_of(s, stop))
    }

    /// The same with the objective's place already chosen (`spot`).
    fn gate_level_at(&self, s: &State, stop: Stop, spot: Option<&Objective>) -> i64 {
        let i = stop.index as usize;
        match stop.kind {
            Kind::Accept => {
                let q = self.quest(stop.index);
                q.min_level.max(self.combat().guard_level(&q.start_guard, s.might()))
            }
            Kind::TurnIn => self.combat().guard_level(&self.quest(stop.index).end_guard, s.might()),
            Kind::Objective(k) => self.combat().level_at(
                self.quest(stop.index),
                spot.unwrap_or(&self.quest(stop.index).objectives[k as usize]),
                s.might(),
            ),
            Kind::Dungeon => self.model.dungeons[i].min_level,
            _ => 0,
        }
    }

    fn grind_to_gate(
        &self,
        s: &mut State,
        stop: Stop,
        spot: Option<&Objective>,
        record: &mut impl FnMut(&State, Event),
    ) {
        let gate = self.gate_level_at(s, stop, spot);
        if gate <= s.level || self.mode == Mode::Replay || self.explicit {
            return;
        }
        self.grind_at_camps(s, gate, record);
        self.train(s, record);
    }

    /// Travel to the stop, picking up a flight path close to it on the way.
    fn arrive(
        &self,
        s: &mut State,
        stop: Stop,
        spot: Option<&Objective>,
        record: &mut impl FnMut(&State, Event),
    ) -> Loc {
        let loc = spot.map_or_else(|| self.places().loc(s, stop), |o| o.loc.clone());
        self.go(s, &loc.pos, loc.zone, record);
        s.time += self.params.stop_overhead;
        s.spent.overhead += self.params.stop_overhead;
        if stop.kind != Kind::LearnFlight && !self.explicit {
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
        let detour = 2.0 * self.trips().walk(&s.pos, &self.world.taxi_nodes[node].pos, s.level) + 5.0;
        if !detour.is_finite() {
            return;
        }
        s.known |= 1u128 << node;
        s.time += detour;
        s.spent.travel += detour;
        record(s, Event::LearnFlight { node });
    }

    /// What the stop itself does. Returns the reward item picked on a turn-in.
    fn do_stop(&self, s: &mut State, stop: Stop, spot: Option<&Objective>) -> Option<i64> {
        let i = stop.index as usize;
        match stop.kind {
            Kind::Accept => self.accept(s, stop.index),
            Kind::Objective(k) => {
                let o = spot.unwrap_or(&self.quest(stop.index).objectives[k as usize]);
                self.complete_objective(s, stop.index, k, o);
            }
            Kind::TurnIn => return self.turn_in(s, stop.index),
            Kind::LearnFlight => {
                s.known |= 1u128 << i;
                s.time += 5.0;
            }
            Kind::Bind => {
                s.bind = i;
                s.time += 10.0;
            }
            Kind::Hearth => {}
            Kind::Dungeon => self.run_dungeon(s, i),
        }
        None
    }

    /// Kill or use what objective `k` of `quest` still needs at place `here`. Objects gathered
    /// among the mobs of an objective done just before (or mobs killed among the objects), close
    /// by, take part of the time (`Together`).
    fn complete_objective(&self, s: &mut State, quest: u32, k: u8, here: &Objective) {
        if s.objectives[quest as usize] & (1 << k) != 0 {
            return;
        }
        let (kills, gathers) = self.objective_work(s, quest, k, here);
        self.together(s, here, kills, gathers);
    }

    /// Add an objective done at `here` (`kills`, `gathers`: the time of each) to the objectives
    /// done just before close by, and take off the time that saves (the same for the whole as when
    /// done in one stop: gathering during `gather_overlap` of the kills).
    fn together(&self, s: &mut State, here: &Objective, kills: f64, gathers: f64) {
        let near = s.together.at.is_some_and(|at| {
            at.continent == here.loc.pos.continent && at.dist(&here.loc.pos) <= self.params.gather_radius
        });
        if !near || here.dungeon.is_some() {
            s.together = Together {
                at: (here.dungeon.is_none()).then_some(here.loc.pos),
                ..Together::default()
            };
        }
        let t = &mut s.together;
        t.kills += kills;
        t.gathers += gathers;
        let saved = t.gathers.min(t.kills * self.params.gather_overlap).max(0.0);
        let more = saved - t.saved;
        t.saved = saved;
        s.time -= more;
        s.spent.fighting -= more;
    }

    /// Do what objective `k` of `quest` still needs at place `o` (one of its `alts` or itself),
    /// returning the time of its kills and the rest (objects, fixed extra time).
    fn objective_work(&self, s: &mut State, quest: u32, k: u8, o: &Objective) -> (f64, f64) {
        let i = quest as usize;
        // The share done before the route (a character met in game), whatever the place: this
        // place's kills (or uses) are left for the rest.
        let done = match s.started.iter().position(|&(q, ok, _)| q == quest && ok == k) {
            Some(n) => s.started.swap_remove(n).2,
            None => 0.0,
        };
        let left = (1.0 - done).max(0.0);
        let (kills, uses) = if o.kills > 0.0 {
            (o.kills * left, o.uses)
        } else {
            (0.0, o.uses * left)
        };
        // The fixed time too, by share of the targets left; common loot only for what the bags
        // lack.
        let extra = match o.stock {
            Some(c) => {
                let need = (o.count * left) as f32;
                let have = s.stock[usize::from(c)].min(need).max(0.0);
                s.stock[usize::from(c)] -= have;
                if need > 0.0 {
                    o.extra * left * f64::from((need - have) / need)
                } else {
                    0.0
                }
            }
            None => o.extra * left,
        };
        let kill_work = kills * self.combat().kill_time(s.level, s.bonus, o);
        let other = uses * self.params.object_time + extra;
        self.growth().gather(s, o.loc.zone, kills);
        s.time += kill_work + other;
        s.spent.fighting += kill_work + other;
        s.spent.waiting += extra;
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
        // Dungeon objectives are completed below without re-counting their kills.
        s.add_kills(d.kills);
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
        for &(qi, k) in &self.in_dungeon[i] {
            let qi = qi as usize;
            if s.accepted[qi] && !s.turned[qi] {
                s.objectives[qi] |= 1 << k;
            }
        }
    }

    /// Final grind to the target level and the cost of class quests left out.
    pub(crate) fn finish(&self, s: &mut State, mut trace: Option<&mut Vec<Timed>>) -> f64 {
        let mut record = recorder(&mut trace);
        if s.level < self.profile.to_level {
            self.grind_at_camps(s, self.profile.to_level, &mut record);
        }
        let (mut missing, mut zone) = (0, 0);
        for (i, q) in self.model.quests.iter().enumerate() {
            if q.mandatory && !s.rewarded(i) && !self.availability().alternative_done(s, q) {
                if q.zone_forced { zone += 1 } else { missing += 1 }
            }
        }
        s.spent.missing_class_quests = missing as u32;
        s.spent.missing_zone_quests = zone as u32;
        let dungeons = (0..self.model.dungeons.len())
            .filter(|&d| {
                self.availability().dungeon_forced(d)
                    && s.dungeons & (1u128 << d) == 0
                    && self.model.dungeons[d].min_level <= self.profile.to_level
            })
            .count();
        s.spent.missing_dungeons = dungeons as u32;
        s.spent.reputation_missing = self.reputation_missing(s);
        s.spent.total = s.time;
        // Optimizer score: grinding and flights weigh more than their real time, gold spent on
        // flights and missing goals cost.
        s.time
            + self.weights(&s.spent)
            + (missing + zone + dungeons + self.reputation_goals_unmet(s)) as f64 * self.params.mandatory_penalty
            + s.spent.reputation_missing * self.params.reputation_weight
    }

    /// Reputation goals not met: each costs like a required quest left out, besides its missing
    /// points, so that a goal within reach is met.
    pub(crate) fn reputation_goals_unmet(&self, s: &State) -> usize {
        (self.params.reputation_goals.iter().zip(&s.reputation))
            .filter(|(goal, have)| f64::from(**have) < goal.target as f64)
            .count()
    }

    /// Reputation points the goals miss.
    pub(crate) fn reputation_missing(&self, s: &State) -> f64 {
        (self.params.reputation_goals.iter().zip(&s.reputation))
            .map(|(goal, &have)| (goal.target as f64 - f64::from(have)).max(0.0))
            .sum()
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
