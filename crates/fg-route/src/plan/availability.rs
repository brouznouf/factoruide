//! Which quests and stops the character can do now: prerequisites, exclusive quests, gray quests, dungeons, chains.

use super::state::{Kind, State, Stop};
use crate::model::{Model, Profile, Quest};
use crate::params::Params;
use crate::xp;

/// Quests needing each quest first (it is in their `pre_all` or `pre_any`).
pub(crate) fn followers(model: &Model) -> Vec<Vec<u32>> {
    let mut out = vec![Vec::new(); model.quests.len()];
    for (f, q) in model.quests.iter().enumerate() {
        for id in q.pre_all.iter().chain(&q.pre_any) {
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
    pub(crate) fn prereqs_met(&self, s: &State, q: &Quest) -> bool {
        let done = |id: &i64| match self.model.index.get(id) {
            Some(&i) => s.rewarded(i),
            // Not plannable (other class, removed, gray...): known for a character met in game,
            // else assumed done only when starting mid-way.
            None => self.model.initial.done_outside(*id, self.profile.from_level),
        };
        q.pre_all.iter().all(done) && (q.pre_any.is_empty() || q.pre_any.iter().any(done))
    }

    pub(crate) fn blocked(&self, s: &State, q: &Quest) -> bool {
        // A quest given up no longer holds its exclusive group.
        let taken = |id: &i64| match self.model.index.get(id) {
            Some(&i) => s.accepted[i] || s.rewarded(i),
            None => self.model.initial.completed.as_ref().is_some_and(|c| c.contains(id)),
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
            && self.prereqs_met(s, q)
            && !self.blocked(s, q)
            && q.skill
                .is_none_or(|(p, value)| self.model.professions[p].skill_at(s.level) >= value)
            && !self.gray_dead_end(s, q, i)
    }

    /// A quest gray for the character (no XP worth the trip, turned in whenever the route passes
    /// by) that leads to nothing better: not taken, and given up when it goes gray in the log. A
    /// gray quest opening a chain that is not gray stays; class quests always do. An imposed
    /// route being replayed takes what it says.
    pub(crate) fn gray_dead_end(&self, s: &State, q: &Quest, i: u32) -> bool {
        let gray = xp::Rules::gray_level(s.level);
        !q.mandatory
            && !self.replaying
            && !self.params.keep_gray
            && q.level <= gray
            && self.model.chain_top[i as usize] <= gray
    }

    pub(crate) fn objectives_done(&self, s: &State, i: u32) -> bool {
        let n = self.quest_at(i).objectives.len();
        s.objectives[i as usize].count_ones() as usize == n
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
                s.accepted[i]
                    && (!s.turned[i] || s.objectives[i] & (1 << k) != 0)
                    && self.quest_at(stop.index).objectives[k as usize].dungeon.is_none()
            }
            Kind::TurnIn => s.accepted[i] && !s.turned[i] && self.objectives_done(s, stop.index),
            Kind::LearnFlight => s.known & (1u128 << i) == 0,
            Kind::Bind => s.bind != i && i != self.model.start_inn,
            Kind::Dungeon => {
                self.dungeon_allowed(i)
                    && s.dungeons & (1u128 << i) == 0
                    && self.model.dungeons[i].min_level <= self.profile.to_level
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
                let needs_it = fq
                    .pre_all
                    .iter()
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
        !self.params.excluded_dungeons.contains(&area)
            && (self.params.dungeons || self.params.forced_dungeons.contains(&area))
    }

    /// Dungeon `d` must be run (when the route reaches its levels).
    pub(crate) fn dungeon_forced(&self, d: usize) -> bool {
        let area = self.model.dungeons[d].area;
        self.params.forced_dungeons.contains(&area) && !self.params.excluded_dungeons.contains(&area)
    }

    fn quest_at(&self, i: u32) -> &Quest {
        &self.model.quests[i as usize]
    }
}
