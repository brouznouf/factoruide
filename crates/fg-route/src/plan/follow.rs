//! Replaying an imposed route (written by hand or imported) step by step under our time model.

use super::Planner;
use super::state::{Breakdown, Event, Kind, Stop, Timed};
use crate::world::Pos;

/// One step of an imposed route.
#[derive(Debug, Clone)]
pub enum Want {
    Stop(Stop),
    /// Grind until this level when below.
    Grind(i64),
    /// Start of a section of the route.
    Section(String),
    /// Die on purpose and respawn at the spirit healer near `pos`.
    DeathSkip {
        pos: Pos,
        zone: i64,
        seconds: f64,
    },
}

/// An imposed route as simulated.
pub struct Followed {
    pub route: Vec<Stop>,
    pub trace: Vec<Timed>,
    pub total: f64,
    pub breakdown: Breakdown,
    /// Steps that could not be done, with the reason.
    pub dropped: Vec<(Stop, &'static str)>,
    /// Sections: name, time, level and XP into the level at their start.
    pub sections: Vec<(String, f64, i64, i64)>,
    /// Quests accepted although their prerequisites (in our data) were not done.
    pub trusted: Vec<u32>,
}

impl Planner<'_> {
    /// Simulate `wants` in order, skipping the steps that are not possible (with the reason)
    /// and doing the objectives of a quest before its turn-in when the route left them implicit.
    pub fn follow(&self, wants: &[Want]) -> Followed {
        self.replaying().replay(wants)
    }

    fn replay(&self, wants: &[Want]) -> Followed {
        let mut s = self.initial_state();
        let mut trace = Vec::new();
        let mut route = Vec::new();
        let mut dropped = Vec::new();
        let mut sections = Vec::new();
        let mut trusted_accepts = Vec::new();
        for want in wants {
            match want {
                Want::Section(name) => sections.push((name.clone(), s.time, s.level, s.xp)),
                Want::DeathSkip { pos, zone, seconds } => {
                    s.time += seconds;
                    s.spent.travel += seconds;
                    s.pos = *pos;
                    s.zone = *zone;
                    trace.push(Timed {
                        event: Event::DeathSkip { pos: *pos, zone: *zone },
                        time: s.time,
                        level: s.level,
                        xp: s.xp,
                        power: s.level as f64 + s.bonus,
                        gear: s.gear_bonus,
                    });
                }
                Want::Grind(level) => {
                    if s.level < *level {
                        let (near, zone) = (s.pos, s.zone);
                        let seconds = self.growth().grind_to(&mut s, *level);
                        trace.push(Timed {
                            event: Event::Grind {
                                to_level: *level,
                                seconds,
                                near,
                                zone,
                            },
                            time: s.time,
                            level: s.level,
                            xp: s.xp,
                            power: s.level as f64 + s.bonus,
                            gear: s.gear_bonus,
                        });
                    }
                }
                Want::Stop(stop) => {
                    let i = stop.index as usize;
                    if stop.kind == Kind::TurnIn && s.accepted[i] && !s.turned[i] {
                        for k in 0..self.quest(stop.index).objectives.len() {
                            let o = Stop::quest(stop.index, Kind::Objective(k as u8));
                            if self.availability().valid(&s, o) {
                                self.apply(&mut s, o, Some(&mut trace));
                                route.push(o);
                            }
                        }
                    }
                    // The route knows the prerequisites better than our data.
                    let trusted = stop.kind == Kind::Accept
                        && !s.accepted[i]
                        && !s.turned[i]
                        && s.log < self.params.quest_log_size
                        && !self.availability().prereqs_met(&s, self.quest(stop.index));
                    if trusted && !self.availability().valid(&s, *stop) {
                        trusted_accepts.push(stop.index);
                    }
                    if self.availability().valid(&s, *stop) || trusted {
                        self.apply(&mut s, *stop, Some(&mut trace));
                        route.push(*stop);
                    } else {
                        dropped.push((*stop, self.why_invalid(&s, *stop)));
                    }
                }
            }
        }
        self.finish(&mut s, Some(&mut trace));
        Followed {
            route,
            trace,
            total: s.spent.total,
            breakdown: s.spent,
            dropped,
            sections,
            trusted: trusted_accepts,
        }
    }
}
