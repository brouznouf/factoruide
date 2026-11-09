//! The searches of the final meeting every `share_every` tries: each brings its best route, and
//! those behind go on from the best of all. Left alone, a search spends its time improving a
//! route another one already beats.
//!
//! A meeting waits for every search still running, so that the same tries give the same routes
//! (`Effort::Tries`). A search leaves once it stops trying (its time is over, the generation is
//! stopped, it panicked): the others no longer wait for it.

use super::state::Stop;
use crate::sync::LockExt;
use std::cell::Cell;
use std::sync::{Condvar, Mutex};

pub struct Exchange {
    round: Mutex<Round>,
    met: Condvar,
}

struct Round {
    /// Searches still trying.
    running: usize,
    /// Searches waiting at the current meeting.
    arrived: usize,
    /// Meetings held so far.
    held: u64,
    /// Best route brought to the current meeting: (score, seed, route), the lowest score, then
    /// seed, so that the choice does not depend on the order of arrival.
    brought: Option<(f64, u64, Vec<Stop>, super::HardGoals)>,
    /// Best route of the last meeting.
    best: Option<(f64, Vec<Stop>, super::HardGoals)>,
}

impl Exchange {
    pub fn new(searches: usize) -> Self {
        Self {
            round: Mutex::new(Round {
                running: searches,
                arrived: 0,
                held: 0,
                brought: None,
                best: None,
            }),
            met: Condvar::new(),
        }
    }

    /// A seat for one of the searches (as many as `new` was told).
    pub fn member(&self) -> Member<'_> {
        Member {
            exchange: self,
            left: Cell::new(false),
        }
    }
}

impl Round {
    /// Ends the meeting once everyone still running is there.
    fn close_if_complete(&mut self, met: &Condvar) {
        if self.arrived > 0 && self.arrived >= self.running {
            self.best = self
                .brought
                .take()
                .map(|(score, _, route, goals)| (score, route, goals));
            self.arrived = 0;
            self.held += 1;
            met.notify_all();
        }
    }
}

/// One search's seat at the meetings. It leaves when dropped, unwinding included.
pub struct Member<'e> {
    exchange: &'e Exchange,
    left: Cell<bool>,
}

impl Member<'_> {
    /// Bring `best` (score, route) to the meeting and wait for the others: the best route of
    /// all of them.
    pub(crate) fn meet(
        &self,
        seed: u64,
        best: &(f64, Vec<Stop>, super::HardGoals),
    ) -> Option<(f64, Vec<Stop>, super::HardGoals)> {
        let ex = self.exchange;
        let mut round = ex.round.locked();
        if round.brought.as_ref().is_none_or(|b| {
            best.2
                .cmp(&b.3)
                .then(best.0.total_cmp(&b.0))
                .then(seed.cmp(&b.1))
                .is_lt()
        }) {
            round.brought = Some((best.0, seed, best.1.clone(), best.2));
        }
        round.arrived += 1;
        let meeting = round.held;
        round.close_if_complete(&ex.met);
        while round.held == meeting {
            round = ex.met.wait(round).expect("a thread panicked while holding the lock");
        }
        round.best.clone()
    }

    /// No more meetings for this search.
    pub(crate) fn leave(&self) {
        if self.left.replace(true) {
            return;
        }
        let ex = self.exchange;
        let mut round = ex.round.lock().unwrap_or_else(std::sync::PoisonError::into_inner);
        round.running -= 1;
        round.close_if_complete(&ex.met);
    }
}

impl Drop for Member<'_> {
    fn drop(&mut self) {
        self.leave();
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::plan::state::Kind;

    fn route(n: u32) -> Vec<Stop> {
        vec![Stop::quest(n, Kind::Accept)]
    }

    /// Everyone gets the best route of the meeting; a search that left is not waited for.
    #[test]
    fn meetings_give_the_best_and_do_not_wait_for_those_gone() {
        let ex = Exchange::new(3);
        let got = std::thread::scope(|scope| {
            let gone = scope.spawn(|| drop(ex.member()));
            let a = scope.spawn(|| {
                ex.member()
                    .meet(1, &(10.0, route(1), super::super::HardGoals::default()))
            });
            let b = scope.spawn(|| {
                ex.member()
                    .meet(2, &(5.0, route(2), super::super::HardGoals::default()))
            });
            gone.join().unwrap();
            [a.join().unwrap(), b.join().unwrap()]
        });
        for best in got {
            assert_eq!(best, Some((5.0, route(2), super::super::HardGoals::default())));
        }
    }

    /// Equal scores: the lowest seed, whoever comes first.
    #[test]
    fn ties_go_to_the_lowest_seed() {
        let ex = Exchange::new(2);
        let got = std::thread::scope(|scope| {
            let a = scope.spawn(|| {
                ex.member()
                    .meet(7, &(5.0, route(7), super::super::HardGoals::default()))
            });
            let b = scope.spawn(|| {
                ex.member()
                    .meet(3, &(5.0, route(3), super::super::HardGoals::default()))
            });
            [a.join().unwrap(), b.join().unwrap()]
        });
        for best in got {
            assert_eq!(best, Some((5.0, route(3), super::super::HardGoals::default())));
        }
    }

    #[test]
    fn complete_expensive_member_survives_cheaper_incomplete_member() {
        let ex = Exchange::new(2);
        let complete = super::super::HardGoals::default();
        let incomplete = super::super::HardGoals { gear: 1, ..complete };
        let got = std::thread::scope(|scope| {
            let a = scope.spawn(|| ex.member().meet(1, &(10000.0, route(1), complete)));
            let b = scope.spawn(|| ex.member().meet(2, &(1.0, route(2), incomplete)));
            [a.join().unwrap(), b.join().unwrap()]
        });
        for best in got {
            assert_eq!(best, Some((10000.0, route(1), complete)));
        }
    }
}
