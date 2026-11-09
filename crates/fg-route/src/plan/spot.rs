//! Where an objective is done when its mobs or objects live in several places (`Objective::alts`):
//! the place that costs the least between where the character is and where it goes next, counting
//! the trips, the work there (the level of its mobs, the wait for respawns, shared with the kills
//! done there just before) and the levels to grind
//! first when its mobs or the guards around are too strong. Where it goes next: the route's next
//! stop when the route is known (`State::next`, a replay: its places come out on the way between
//! two stops), else the quest's turn-in (a route being built stop by stop).

use super::Simulator;
use super::state::{Kind, State, Stop};
use crate::model::{Objective, Quest};

impl Simulator<'_> {
    /// Objective `k` of `quest` at the place the character goes to from `s`.
    pub(crate) fn spot(&self, s: &State, quest: u32, k: u8) -> &Objective {
        let q = self.quest(quest);
        let o = &q.objectives[k as usize];
        let visit = super::gear::source_visit(self.model, Stop::quest(quest, Kind::Objective(k)));
        if o.alts.is_empty() {
            return o;
        }
        if let Some((sq, sk, i)) = s.spot
            && (sq, sk) == (quest, k)
            && let Some(place) = std::iter::once(o).chain(&o.alts).nth(usize::from(i))
            && visit.is_none_or(|v| v == place.dungeon_of())
        {
            return place;
        }
        let open = self.trips().open_links(s);
        let left = if o.shared_kill_target().is_some() {
            1.0 - s
                .started
                .iter()
                .find(|&&(q, ok, _)| q == quest && ok == k)
                .map_or(0.0, |p| p.2)
        } else {
            1.0
        };
        let matching = || o.places().filter(|place| visit.is_none_or(|v| v == place.dungeon_of()));
        let mut best = (
            f64::INFINITY,
            matching().next().expect("selected source placement exists"),
        );
        for place in matching() {
            let there = self.spot_work(s, q, place, left.max(0.0));
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

    /// Time of the remaining work at a place, with the grinding to the level it requires; its
    /// respawns waited for with the kills done there just before (`Together`).
    fn spot_work(&self, s: &State, q: &Quest, o: &Objective, left: f64) -> f64 {
        let gate = self.combat().level_at(q, o, s.might());
        let grind = if gate > s.level {
            self.growth().grind_time(s, gate)
        } else {
            0.0
        };
        let kills = o.kills_left(1.0 - left);
        grind
            + kills * self.combat().kill_time(s.level, s.bonus, o)
            + o.uses * self.params.object_time
            + (o.extra - o.pool.wait) * left
            + self.shared_wait(&s.together, o, kills)
    }
}
