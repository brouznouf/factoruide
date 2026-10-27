//! Which quests and stops the character can do now: prerequisites, exclusive quests, gray quests, dungeons, chains.

use super::state::{Kind, State, Stop};
use crate::model::{Model, Profile, Quest};
use crate::params::Params;
use crate::xp;

/// Quests needing each quest first (it is in their `pre_all` or `pre_any`, or hands an item
/// one of their objectives asks for).
pub(crate) fn followers(model: &Model) -> Vec<Vec<u32>> {
    let mut out = vec![Vec::new(); model.quests.len()];
    for (f, q) in model.quests.iter().enumerate() {
        let handing = q.objectives.iter().flat_map(|o| &o.handed_by);
        for id in q.pre_all.iter().chain(&q.pre_any).chain(&q.pre_held).chain(handing) {
            if let Some(&i) = model.index.get(id)
                && i != f
                && !out[i].contains(&(f as u32))
            {
                out[i].push(f as u32);
            }
        }
    }
    out
}

/// Quests each quest blocks once taken: the quests exclusive with it, the breadcrumbs to it.
pub(crate) fn blocks(model: &Model) -> Vec<Vec<u32>> {
    let mut blocks = vec![Vec::new(); model.quests.len()];
    for (y, q) in model.quests.iter().enumerate() {
        for id in q.exclusive.iter().chain(q.breadcrumb_for.iter()) {
            if let Some(&x) = model.index.get(id)
                && x != y
            {
                blocks[x].push(y as u32);
            }
        }
    }
    blocks
}

/// What the character can do now: quests whose prerequisites are done and that nothing
/// excludes, stops in the right state, dungeons allowed.
#[derive(Clone, Copy)]
pub(crate) struct Availability<'v> {
    pub(crate) model: &'v Model,
    pub(crate) params: &'v Params,
    pub(crate) profile: &'v Profile,
    /// Replaying an imposed route: gray quests are taken as it says.
    pub(crate) replaying: bool,
    pub(crate) followers: &'v [Vec<u32>],
}

impl Availability<'_> {
    /// The prerequisites of quest `i` are done: in a group, by each member it is for (before the
    /// route, or turned in on it). The quests to hold in the log are the character's own.
    pub(crate) fn prereqs_met(&self, s: &State, i: u32) -> bool {
        let q = self.quest_at(i);
        let init = &self.model.initial;
        // In the log: taken, not turned in nor given up.
        let held = |id: &i64| self.model.index.get(id).is_some_and(|&j| s.accepted[j] && !s.turned[j]);
        if init.mates.is_empty() {
            let done = |id: &i64| match self.model.index.get(id) {
                Some(&i) => s.rewarded(i),
                // Not plannable (other class, removed, gray...): known for a character met in
                // game, else assumed done only when starting mid-way.
                None => init.done_outside(*id, self.profile.from_level),
            };
            return q.pre_all.iter().all(done)
                && (q.pre_any.is_empty() || q.pre_any.iter().any(done))
                && q.pre_held.iter().all(held);
        }
        let need = init.need(i as usize);
        (0..=init.mates.len()).filter(|m| need & (1 << m) != 0).all(|m| {
            let done = |id: &i64| init.done_by(m, *id) || self.model.index.get(id).is_some_and(|&j| s.rewarded(j));
            q.pre_all.iter().all(done) && (q.pre_any.is_empty() || q.pre_any.iter().any(done))
        }) && q.pre_held.iter().all(held)
    }

    pub(crate) fn blocked(&self, s: &State, q: &Quest) -> bool {
        // A quest given up no longer holds its exclusive group.
        // In a group, one done before the route closes it to that member only (`Initial::need`).
        let group = !self.model.initial.mates.is_empty();
        let taken = |id: &i64| match self.model.index.get(id) {
            Some(&i) => s.accepted[i] || s.rewarded(i),
            None => !group && self.model.initial.completed.as_ref().is_some_and(|c| c.contains(id)),
        };
        q.exclusive.iter().any(taken) || q.breadcrumb_for.as_ref().is_some_and(taken)
    }

    /// An exclusive alternative (often the same quest for another race) was done instead.
    pub(crate) fn alternative_done(&self, s: &State, q: &Quest) -> bool {
        q.exclusive.iter().any(|id| match self.model.index.get(id) {
            Some(&i) => s.rewarded(i),
            None => self.model.initial.completed.as_ref().is_some_and(|c| c.contains(id)),
        })
    }

    pub(crate) fn can_accept(&self, s: &State, i: u32) -> bool {
        let q = self.quest_at(i);
        !s.accepted[i as usize]
            && !s.turned[i as usize]
            && s.log < self.params.quest_log_size
            && self.prereqs_met(s, i)
            && !self.blocked(s, q)
            && q.skill
                .is_none_or(|(p, value)| self.model.professions[p].skill_at(s.level) >= value)
            && !self.gray_dead_end(s, q, i)
    }

    /// Whether quest `q` gives reputation a goal still misses.
    fn missing_reputation_of(&self, s: &State, q: &Quest) -> bool {
        (self.params.reputation_goals.iter().zip(&s.reputation)).any(|(goal, &have)| {
            f64::from(have) < goal.target as f64 && q.reputation.iter().any(|(f, v)| *f == goal.faction && *v > 0.0)
        })
    }

    /// A quest gray for the character (no XP worth the trip, turned in whenever the route passes
    /// by) that leads to nothing better: not taken, and given up when it goes gray in the log. A
    /// gray quest opening a chain that is not gray stays; class quests always do, profession
    /// quests too (their reward is the profession's, e.g. Camping 101's recipe, handed in at a
    /// trainer the route often reaches only once they are gray), and those giving reputation a
    /// goal still misses. An imposed route being replayed takes what it says.
    pub(crate) fn gray_dead_end(&self, s: &State, q: &Quest, i: u32) -> bool {
        let gray = xp::Rules::gray_level(s.level);
        !q.mandatory
            && q.skill.is_none()
            && !self.missing_reputation_of(s, q)
            && !self.replaying
            && !self.params.keep_gray
            && q.level <= gray
            && self.model.chain_top[i as usize] <= gray
    }

    pub(crate) fn objectives_done(&self, s: &State, i: u32) -> bool {
        let n = self.quest_at(i).objectives.len();
        s.objectives[i as usize].count_ones() as usize == n
    }

    /// Objective `k` of quest `i` is still to do and can be done now (the objectives it comes
    /// after are done).
    pub(crate) fn objective_open(&self, s: &State, i: u32, k: usize) -> bool {
        let done = s.objectives[i as usize];
        let o = &self.quest_at(i).objectives[k];
        done & (1 << k) == 0 && done & o.after == o.after && self.handed(s, o)
    }

    /// The item of objective `o` has been handed: one of the quests giving it is turned in (or
    /// nothing hands it).
    fn handed(&self, s: &State, o: &crate::model::Objective) -> bool {
        o.handed_by.is_empty()
            || o.handed_by
                .iter()
                .any(|id| self.model.index.get(id).is_some_and(|&j| s.turned[j]))
    }

    pub(crate) fn valid(&self, s: &State, stop: Stop) -> bool {
        let i = stop.index as usize;
        match stop.kind {
            // Taken or turned in on the spot already: the stop is skipped.
            Kind::Accept if s.accepted[i] || s.turned[i] => true,
            Kind::TurnIn if s.turned[i] => true,
            Kind::Accept => {
                self.can_accept(s, stop.index)
                    && self
                        .quest_at(stop.index)
                        .objectives
                        .iter()
                        .all(|o| o.dungeon.is_none_or(|d| self.dungeon_allowed(d)))
            }
            // Done on the way already (even turned in since): the stop is skipped.
            Kind::Objective(k) => {
                let done = s.objectives[i] & (1 << k) != 0;
                let o = &self.quest_at(stop.index).objectives[k as usize];
                s.accepted[i]
                    && (!s.turned[i] || done)
                    && o.dungeon.is_none()
                    && (done || s.objectives[i] & o.after == o.after && self.handed(s, o))
            }
            Kind::TurnIn => s.accepted[i] && !s.turned[i] && self.objectives_done(s, stop.index),
            Kind::LearnFlight => s.known & (1u128 << i) == 0,
            // A group bound apart binds anywhere, even where the character is.
            Kind::Bind => (s.bind != i || s.hearth_ready.is_infinite()) && i != self.model.start_inn,
            Kind::Hearth => {
                (i == 0 && !self.params.excluded_zones.contains(&self.model.inns[s.bind].zone))
                    || (i == 1 && self.model.recall.is_some())
            }
            // Never past its levels (`Params::dungeon_to`); below them, the guide grinds to them.
            Kind::Dungeon => {
                self.dungeon_allowed(i)
                    && s.dungeons & (1u128 << i) == 0
                    && self.model.dungeons[i].min_level <= self.profile.to_level
                    && s.level <= self.model.dungeons[i].max_level
            }
        }
    }

    /// Quest `q` and the quests of `route` that need it, directly or through others: what
    /// leaves the route with it. None when a class quest would go.
    pub(crate) fn chain_after(&self, route: &[Stop], q: u32) -> Option<Vec<u32>> {
        let in_route = |i: u32| route.iter().any(|s| s.is_quest() && s.index == i);
        let mut chain = vec![q];
        let mut k = 0;
        while k < chain.len() {
            let i = chain[k];
            if self.quest_at(i).mandatory {
                return None;
            }
            for &f in &self.followers[i as usize] {
                if chain.contains(&f) || !in_route(f) {
                    continue;
                }
                // A quest needing any of several still has the others.
                let fq = self.quest_at(f);
                let needs_it = (fq.pre_all.iter().chain(&fq.pre_held))
                    .any(|id| self.model.index.get(id) == Some(&(i as usize)))
                    || fq
                        .pre_any
                        .iter()
                        .filter_map(|id| self.model.index.get(id))
                        .all(|&j| chain.contains(&(j as u32)) || !in_route(j as u32));
                if needs_it {
                    chain.push(f);
                }
            }
            k += 1;
        }
        Some(chain)
    }

    /// Quests with objectives inside dungeon `d`.
    pub(crate) fn dungeon_quests(&self, d: usize) -> impl Iterator<Item = u32> + '_ {
        (0..self.model.quests.len() as u32)
            .filter(move |&i| self.quest_at(i).objectives.iter().any(|o| o.dungeon == Some(d)))
    }

    /// Dungeon `d` may be run: dungeons on (or this one forced), and not excluded.
    pub(crate) fn dungeon_allowed(&self, d: usize) -> bool {
        let area = self.model.dungeons[d].area;
        !self.params.excluded_dungeons.contains(&area) && (self.params.dungeons || self.dungeon_forced(d))
    }

    /// Dungeon `d` must be run (when the route reaches its levels), with its quests or not.
    pub(crate) fn dungeon_forced(&self, d: usize) -> bool {
        let area = self.model.dungeons[d].area;
        (self.params.forced_dungeons.contains(&area) || self.params.forced_dungeon_quests.contains(&area))
            && !self.params.excluded_dungeons.contains(&area)
    }

    fn quest_at(&self, i: u32) -> &Quest {
        &self.model.quests[i as usize]
    }
}
