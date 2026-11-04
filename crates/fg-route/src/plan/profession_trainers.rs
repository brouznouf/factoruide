//! Profession trainer visits (`Params::profession_trainers`): the character learns each rank of
//! its professions before the skill the curve asks needs it. A trainer close to the route is a
//! detour as soon as the game lets the rank be learned; a trainer far away is a trip, once the
//! skill has passed what the rank before allows. The time is the route's.

use super::Planner;
use super::state::{Event, Landed, State};

/// A rank of a profession (apprentice to artisan): what the game asks to learn it, the skill cap it
/// gives, and the skill the curve may reach without it.
pub(crate) struct Rank {
    /// Character level and skill to learn it (the classic ranks; the data has no others).
    pub(crate) level: i64,
    pub(crate) skill: i64,
    pub(crate) cap: i64,
    /// Needed once the curve passes this: the cap of the rank before, and for the first rank
    /// the skill at which the second can be learned (the profession is learned by then).
    pub(crate) due: i64,
}

pub(crate) const RANKS: [Rank; 4] = [
    Rank {
        level: 1,
        skill: 0,
        cap: 75,
        due: 49,
    },
    Rank {
        level: 10,
        skill: 50,
        cap: 150,
        due: 75,
    },
    Rank {
        level: 20,
        skill: 125,
        cap: 225,
        due: 150,
    },
    Rank {
        level: 35,
        skill: 200,
        cap: 300,
        due: 225,
    },
];

impl Planner<'_> {
    /// The ranks a character starting at `level` already knows: its skill (the curve at that level,
    /// which goes through the skill of a character met in game) asks the first rank once it has
    /// any, and each next rank once it is past the cap of the one before. A new character knows none.
    pub(crate) fn ranks_known_at_start(&self, level: i64) -> [u8; crate::profession::PROFESSIONS.len()] {
        let mut ranks = [0u8; crate::profession::PROFESSIONS.len()];
        for (p, plan) in self.model.professions.iter().enumerate().take(ranks.len()) {
            let skill = plan.skill_at(level);
            ranks[p] = RANKS
                .iter()
                .enumerate()
                .filter(|(k, rank)| if *k == 0 { skill > 0 } else { skill > rank.due })
                .fold(0, |bits, (k, _)| bits | 1 << k);
        }
        ranks
    }

    /// Visit the profession trainers the character needs, one after the other (the trainers of a
    /// town are a few steps apart).
    pub(crate) fn train_professions(&self, s: &mut State, record: &mut impl FnMut(&State, Event)) {
        if self.explicit || self.model.profession_trainers.is_empty() || s.level >= self.profile.to_level {
            return;
        }
        while let Some((profession, trainer, learns)) = self.profession_visit(s) {
            self.train_profession_at(s, profession, trainer, learns, record);
        }
    }

    /// The next visit: the profession, the trainer, and the ranks (a bit each) it teaches now.
    /// Close trainers first, as the class ones are; a far one only when a rank is needed.
    fn profession_visit(&self, s: &mut State) -> Option<(usize, usize, u8)> {
        for (p, plan) in self.model.professions.iter().enumerate().take(s.ranks.len()) {
            let line = crate::profession::skill_line(&plan.key);
            let skill = plan.curve.get(s.level.clamp(0, 60) as usize).copied().unwrap_or(0);
            // The ranks open (the game lets them be learned), not learned yet, and needed.
            let mut open = 0u8;
            let mut needed = 0u8;
            for (r, rank) in RANKS.iter().enumerate() {
                if s.ranks[p] & 1 << r == 0 && s.level >= rank.level.max(plan.start) && skill >= rank.skill {
                    open |= 1 << r;
                    if skill > rank.due {
                        needed |= 1 << r;
                    }
                }
            }
            if open == 0 {
                continue;
            }
            let teaching = |ranks: u8| {
                (self.model.profession_trainers.iter().enumerate())
                    .filter(move |(_, t)| t.line == line && ranks & Self::taught_by(t.cap) != 0)
                    .map(|(k, t)| (k, t.loc.pos))
            };
            let close = teaching(open)
                .map(|(k, pos)| (k, s.pos.dist(&pos)))
                .filter(|(_, d)| *d <= self.params.train_radius)
                .min_by(|a, b| a.1.total_cmp(&b.1));
            let trainer = match close {
                Some((k, _)) => Some(k),
                // Nearest by travel time (it may be worth a flight or the hearthstone).
                None => teaching(needed)
                    .map(|(k, pos)| (k, self.trips().fastest(s, &pos).0))
                    .filter(|(_, time)| time.is_finite())
                    .min_by(|a, b| a.1.total_cmp(&b.1))
                    .map(|(k, _)| k),
            };
            if let Some(k) = trainer {
                return Some((p, k, open & Self::taught_by(self.model.profession_trainers[k].cap)));
            }
        }
        None
    }

    /// The ranks (a bit each) a trainer whose title says `cap` teaches: those up to it.
    fn taught_by(cap: i64) -> u8 {
        RANKS
            .iter()
            .enumerate()
            .filter(|(_, r)| r.cap <= cap)
            .fold(0, |bits, (r, _)| bits | 1 << r)
    }

    /// Learn `learns` of profession `profession` at its trainer `trainer`: a detour when it is
    /// close (`train_radius`), else a trip there.
    fn train_profession_at(
        &self,
        s: &mut State,
        profession: usize,
        trainer: usize,
        learns: u8,
        record: &mut impl FnMut(&State, Event),
    ) {
        let loc = &self.model.profession_trainers[trainer].loc;
        let outbound = (s.landed == Landed::No && s.pos.dist(&loc.pos) <= self.params.train_radius)
            .then(|| {
                let trips = self.trips().at(s);
                (
                    trips.walk(&s.pos, &loc.pos, s.level),
                    trips.walk(&loc.pos, &s.pos, s.level),
                )
            })
            .filter(|(out, back)| out.is_finite() && back.is_finite());
        if let Some((out, back)) = outbound {
            s.time += out + back;
            s.spent.travel += out + back;
        } else {
            let (pos, zone) = (loc.pos, loc.zone);
            self.go(s, &pos, zone, record);
        }
        s.time += self.params.train_time;
        s.spent.training += self.params.train_time;
        s.ranks[profession] |= learns;
        record(
            s,
            Event::ProfTrain {
                profession,
                trainer,
                outbound: outbound.map_or(0.0, |(out, _)| out),
                ranks: learns,
            },
        );
    }
}
