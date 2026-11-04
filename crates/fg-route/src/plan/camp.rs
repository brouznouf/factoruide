//! Where the character grinds: at a camp of mobs of its level (`FarmMobs::camps`), the one of
//! the nearest it gets to fastest, going there first; each level at its own camp. Grinding stays
//! where the character stands only when no camp of its level is on its continent.

use super::Simulator;
use super::state::{Event, State};
use crate::world::Pos;
use rustc_hash::FxHashMap;
use std::rc::Rc;

/// Camps looked at, nearest as the crow flies first: the one the trip to is the fastest.
const CAMP_CHOICES: usize = 3;
/// Places whose nearest camps are remembered, beyond which the memo starts over.
/// What a table of 2^16 buckets holds (7/8 of them): it never doubles for the last ones.
const CAMP_MEMO: usize = (1 << 16) / 8 * 7;

/// The nearest camps of a place for a level: (continent, bits of the coordinates, level).
pub(crate) type NearCamps = std::cell::RefCell<FxHashMap<(i64, [u64; 2], i64), Rc<[Pos]>>>;

impl Simulator<'_> {
    /// Grind until `level` (the game's cap at most), at the camp of each level, recording the
    /// trips there and the grinding at each camp (`Event::Grind`, at the camp). The character
    /// ends at the last camp.
    pub(crate) fn grind_at_camps(&self, s: &mut State, level: i64, record: &mut impl FnMut(&State, Event)) {
        let level = level.min(self.rules().max_level);
        // The camp ground at and the time spent there, recorded on leaving it.
        let mut at: Option<(Pos, i64, f64)> = None;
        while s.level < level {
            let (near, zone) = self.camp(s).map_or((s.pos, s.zone), |c| (c, c.zone));
            if let Some((camp, zone, seconds)) = at.take_if(|c| c.0 != near) {
                Self::ground(s, camp, zone, seconds, record);
            }
            if near != s.pos {
                self.go(s, &near, zone, record);
                if !s.time.is_finite() {
                    break;
                }
                // The mobs farmed on the way there were enough: the trip is still the guide's,
                // a step at the camp (else a replay of the guide would not go there).
                if s.level >= level {
                    at = Some((near, zone, 0.0));
                    break;
                }
            }
            let seconds = self.growth().grind_to(s, s.level + 1);
            at = Some((near, zone, at.map_or(0.0, |c| c.2) + seconds));
        }
        if let Some((camp, zone, seconds)) = at {
            Self::ground(s, camp, zone, seconds, record);
        }
    }

    /// Record the grinding done at a camp.
    fn ground(s: &State, near: Pos, zone: i64, seconds: f64, record: &mut impl FnMut(&State, Event)) {
        let to_level = s.level;
        record(
            s,
            Event::Grind {
                to_level,
                seconds,
                near,
                zone,
            },
        );
    }

    /// The camp where the character grinds its level from where it stands: of the
    /// `CAMP_CHOICES` nearest as the crow flies, the one it gets to fastest (None when none
    /// can be got to).
    pub(crate) fn camp(&self, s: &State) -> Option<Pos> {
        let key = (s.pos.continent, [s.pos.x.to_bits(), s.pos.y.to_bits()], s.level);
        let known = self.memo.near_camps.borrow().get(&key).cloned();
        let near = known.unwrap_or_else(|| {
            let mut camps: Vec<(f64, usize)> = (self.model.farm.camps(s.pos.continent, s.level).iter())
                .enumerate()
                .map(|(i, c)| (s.pos.dist(c), i))
                .collect();
            camps.sort_unstable_by(|a, b| a.0.total_cmp(&b.0).then(a.1.cmp(&b.1)));
            let all = self.model.farm.camps(s.pos.continent, s.level);
            let near: Rc<[Pos]> = camps.iter().take(CAMP_CHOICES).map(|&(_, i)| all[i]).collect();
            let mut memo = self.memo.near_camps.borrow_mut();
            if memo.len() >= CAMP_MEMO {
                memo.clear();
            }
            memo.insert(key, Rc::clone(&near));
            near
        });
        let mut best = None;
        for camp in near.iter() {
            let t = self.trips().fastest(s, camp).0;
            if t.is_finite() && best.is_none_or(|(b, _)| t < b) {
                best = Some((t, *camp));
            }
        }
        best.map(|(_, camp)| camp)
    }
}
