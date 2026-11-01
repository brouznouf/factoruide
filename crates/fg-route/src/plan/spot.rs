//! Where an objective is done when its mobs or objects live in several places (`Objective::alts`):
//! the place that costs the least between where the character is and where it goes next, counting
//! the trips, the work there (the level of its mobs, the wait for respawns) and the levels to grind
//! first when its mobs or the guards around are too strong. Where it goes next: the route's next
//! stop when the route is known (`State::next`, a replay: its places come out on the way between
//! two stops), else the quest's turn-in (a route being built stop by stop).

use super::Planner;
use super::state::{Kind, State, Stop};
use crate::model::{Objective, Quest};

impl Planner<'_> {
    /// Objective `k` of `quest` at the place the character goes to from `s`.
    pub(crate) fn spot(&self, s: &State, quest: u32, k: u8) -> &Objective {
        let q = self.quest(quest);
        let o = &q.objectives[k as usize];
        if o.alts.is_empty() {
            return o;
        }
        if let Some((sq, sk, i)) = s.spot
            && (sq, sk) == (quest, k)
            && let Some(place) = std::iter::once(o).chain(&o.alts).nth(usize::from(i))
        {
            return place;
        }
        let open = self.trips().open_links(s);
        let mut best = (f64::INFINITY, o);
        for place in std::iter::once(o).chain(&o.alts) {
            let there = self.spot_work(s, q, place);
            if there >= best.0 {
                continue;
            }
            // On to the route's next stop when it is known, else to the quest's turn-in.
            let after = match &s.next {
                Some(next) => self.trips().at(s).leg(&place.loc.pos, next, s.level, s.known, open).0,
                None => q
                    .ends
                    .iter()
                    .map(|e| self.trips().at(s).leg(&place.loc.pos, &e.pos, s.level, s.known, open).0)
                    .fold(f64::INFINITY, f64::min),
            };
            let cost = there + after + self.trips().fastest(s, &place.loc.pos).0;
            if cost < best.0 {
                best = (cost, place);
            }
        }
        best.1
    }

    /// Which place of objective `k` of `quest` is at `pos` (0: its own, then its `alts`).
    pub(crate) fn spot_index(&self, quest: u32, k: u8, pos: &crate::world::Pos) -> Option<u8> {
        let o = &self.quest(quest).objectives[k as usize];
        let i = std::iter::once(o).chain(&o.alts).position(|p| p.loc.pos == *pos)?;
        u8::try_from(i).ok()
    }

    /// The objective at its place for an objective stop, None for the other stops.
    pub(crate) fn spot_of(&self, s: &State, stop: Stop) -> Option<&Objective> {
        match stop.kind {
            Kind::Objective(k) => Some(self.spot(s, stop.index, k)),
            _ => None,
        }
    }

    /// Time of the work at a place, with the grinding to the level it requires.
    fn spot_work(&self, s: &State, q: &Quest, o: &Objective) -> f64 {
        let gate = self.combat().level_at(q, o, s.might());
        let grind = if gate > s.level {
            self.growth().grind_time(s, gate)
        } else {
            0.0
        };
        grind + o.kills * self.combat().kill_time(s.level, s.bonus, o) + o.uses * self.params.object_time + o.extra
    }
}
