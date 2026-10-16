//! The hearthstone: where it is bound, and when it is used.
//!
//! Bound: the route is cut into bands of levels (fewer levels each higher up, where each takes
//! longer); for each, the inns closest to where its time is spent, the one among most of its
//! quest NPCs and the closest one next to a flight master are tried, bound during the band, and
//! the best route is kept (one band after the other).
//!
//! Used: a stop of its own (`Kind::Hearth`), placed on the trip it saves the most on among
//! those of the next `WINDOW` seconds once it is ready (not the first one worth it: its hour
//! of cooldown may be worth more a little later, typically back to town once a zone's quests
//! are done). The same for the shaman's Astral Recall.
//!
//! The local search then moves and changes both (`moves`).

use super::Planner;
use super::evaluate::Run;
use super::state::{Kind, State, Stop};
use crate::world::Pos;

/// Inns tried for a band, or drawn from by the local search: the closest ones.
pub(crate) const INNS_TRIED: usize = 3;

/// A use of the hearthstone.
const HEARTHSTONE: Stop = Stop {
    index: 0,
    kind: Kind::Hearth,
};

/// Once the hearthstone is ready, the trips of this many seconds ahead are compared.
const WINDOW: f64 = 1200.0;

/// Quest NPCs this close to an inn make it a hub (yards); a flight master this close is next to it.
const HUB: f64 = 300.0;
const NEXT_TO: f64 = 200.0;

impl Planner<'_> {
    /// Levels a band starting at `level` covers.
    fn band_width(level: i64) -> i64 {
        match level {
            ..20 => 5,
            20..40 => 4,
            _ => 3,
        }
    }

    /// The hearthstone bound anew for each band of levels at one of the inns `inns_for` gives,
    /// its other binds removed, or nothing changed: whatever makes the route best. The band's
    /// hearthstone uses are placed anew for each (`pick_uses`): to the inn bound before up to
    /// the bind, to the new one after.
    pub(crate) fn rebind(&self, mut route: Vec<Stop>) -> Vec<Stop> {
        let Some(mut run) = self.run(&route) else {
            return route;
        };
        let mut low = self.profile.from_level;
        while low < self.profile.to_level && !self.aborted() {
            let high = low + Self::band_width(low);
            let band: Vec<usize> = (0..run.states.len() - 1)
                .filter(|&k| (low..high).contains(&run.states[k].level))
                .collect();
            low = high;
            let (Some(&a), Some(&z)) = (band.first(), band.last()) else {
                continue;
            };
            let Some(center) = Self::activity_center(&run, a, z) else {
                continue;
            };
            let mut best: Option<(f64, Vec<Stop>)> = None;
            let mut consider = |candidate: Vec<Stop>| {
                if let Some(t) = self.simulate(&candidate, None)
                    && t < best.as_ref().map_or(run.total - 0.5, |b| b.0)
                {
                    best = Some((t, candidate));
                }
            };
            // The band's stops but its binds and hearthstone uses, chosen anew.
            let kept: Vec<usize> = (a..=z)
                .filter(|&k| route[k].kind != Kind::Bind && route[k] != HEARTHSTONE)
                .collect();
            if kept.is_empty() {
                continue;
            }
            // The band with a bind at `inn` after stop `after` (none: the inn bound before) and
            // the hearthstone used before the stops `uses`.
            let build = |bind: Option<(usize, usize)>, uses: &[usize]| {
                let mut out = route[..a].to_vec();
                for &k in &kept {
                    if uses.contains(&k) {
                        out.push(HEARTHSTONE);
                    }
                    out.push(route[k]);
                    if let Some((inn, after)) = bind
                        && after == k
                    {
                        out.push(Stop {
                            index: inn as u32,
                            kind: Kind::Bind,
                        });
                    }
                }
                out.extend_from_slice(&route[z + 1..]);
                out
            };
            let before = run.states[a].bind;
            if kept.len() < z - a + 1 {
                consider(build(None, &self.pick_uses(&run, &route, &kept, a, |_| before, false)));
            }
            for inn in self.inns_for(&run, &route, a, z, &center) {
                let pos = self.model.inns[inn].pos;
                // After the stop closest to the inn, in the whole band or early in it.
                let nearest = |n: usize| {
                    kept[..n.min(kept.len())]
                        .iter()
                        .copied()
                        .min_by(|&x, &y| {
                            let dx = self.places().pos(&run.states[x], route[x]).dist(&pos);
                            let dy = self.places().pos(&run.states[y], route[y]).dist(&pos);
                            dx.total_cmp(&dy)
                        })
                        .unwrap_or(a)
                };
                let (anywhere, early) = (nearest(kept.len()), nearest(kept.len().div_ceil(3)));
                for after in if early == anywhere {
                    vec![anywhere]
                } else {
                    vec![anywhere, early]
                } {
                    let inn_at = |k: usize| if k > after { inn } else { before };
                    let uses = self.pick_uses(&run, &route, &kept, a, inn_at, false);
                    consider(build(Some((inn, after)), &uses));
                }
            }
            if let Some((_, better)) = best
                && let Some(r) = self.replace_run(&mut run, &route, &better)
            {
                (route, run) = (better, r);
            }
        }
        route
    }

    /// Where the time of stops `a..=z` is spent: the mean place of the stops on the continent
    /// most of it is spent on, weighed by their time.
    fn activity_center(run: &Run, a: usize, z: usize) -> Option<Pos> {
        let spent: Vec<(Pos, f64)> = (a..=z)
            .map(|k| (run.states[k + 1].pos, run.states[k + 1].time - run.states[k].time))
            .filter(|(_, t)| t.is_finite() && *t > 0.0)
            .collect();
        let mut by_continent: Vec<(i64, f64)> = Vec::new();
        for (p, t) in &spent {
            match by_continent.iter_mut().find(|(c, _)| *c == p.continent) {
                Some(c) => c.1 += t,
                None => by_continent.push((p.continent, *t)),
            }
        }
        let continent = by_continent.iter().max_by(|x, y| x.1.total_cmp(&y.1))?.0;
        let here: Vec<&(Pos, f64)> = spent.iter().filter(|(p, _)| p.continent == continent).collect();
        let total: f64 = here.iter().map(|(_, t)| t).sum();
        let first = here.first()?.0;
        Some(Pos {
            x: here.iter().map(|(p, t)| p.x * t).sum::<f64>() / total,
            y: here.iter().map(|(p, t)| p.y * t).sum::<f64>() / total,
            ..first
        })
    }

    /// Inns tried for the band of stops `a..=z` around `center`: the closest ones, the one with
    /// the most of the band's quest NPCs around (a hub: turned in on arrival), and the closest
    /// one next to a flight master (back to town, then a flight anywhere).
    fn inns_for(&self, run: &Run, route: &[Stop], a: usize, z: usize, center: &Pos) -> Vec<usize> {
        let mut inns = self.inns_near(center, INNS_TRIED);
        let near = self.inns_near(center, INNS_TRIED * 4);
        let npcs: Vec<Pos> = (a..=z)
            .filter(|&k| matches!(route[k].kind, Kind::Accept | Kind::TurnIn))
            .map(|k| self.places().pos(&run.states[k], route[k]))
            .collect();
        let hub = near
            .iter()
            .map(|&i| {
                let pos = self.model.inns[i].pos;
                (i, npcs.iter().filter(|p| p.dist(&pos) <= HUB).count())
            })
            .filter(|&(_, n)| n > 0)
            .max_by_key(|&(_, n)| n)
            .map(|(i, _)| i);
        let flights = near
            .iter()
            .copied()
            .find(|&i| self.world.taxi_near(&self.model.inns[i].pos, NEXT_TO).is_some());
        for i in hub.into_iter().chain(flights) {
            if !inns.contains(&i) {
                inns.push(i);
            }
        }
        inns
    }

    /// The hearthstone and Astral Recall used anew (`place_uses`), when that makes the route
    /// better than its uses as they are.
    pub fn place_hearths(&self, route: Vec<Stop>) -> Vec<Stop> {
        let mut placed: Vec<Stop> = route.iter().filter(|s| s.kind != Kind::Hearth).copied().collect();
        placed = self.place_uses(placed, false);
        if self.model.recall.is_some() {
            placed = self.place_uses(placed, true);
        }
        match (self.simulate(&placed, None), self.simulate(&route, None)) {
            (Some(new), Some(old)) if new < old - 0.5 => placed,
            (Some(_), None) => placed,
            _ => route,
        }
    }

    /// Uses of the hearthstone (or Astral Recall: `recall`) inserted in `route` (`pick_uses`).
    fn place_uses(&self, mut route: Vec<Stop>, recall: bool) -> Vec<Stop> {
        let Some(run) = self.run(&route) else {
            return route;
        };
        let all: Vec<usize> = (0..run.states.len() - 1).collect();
        let uses = self.pick_uses(&run, &route, &all, 0, |k| run.states[k].bind, recall);
        for &k in uses.iter().rev() {
            route.insert(
                k,
                Stop {
                    index: u32::from(recall),
                    kind: Kind::Hearth,
                },
            );
        }
        route
    }

    /// Stops among `stops` (indexes in `route`, replayed as `run`, in order) before which the
    /// hearthstone (or Astral Recall: `recall`) takes the character back to the inn `inn_at`
    /// gives: each time it is ready (as it was before stop `from`), on the trip of the next
    /// `WINDOW` seconds it saves the most on, when that is at least `hearth_min_saving`.
    fn pick_uses(
        &self,
        run: &Run,
        route: &[Stop],
        stops: &[usize],
        from: usize,
        inn_at: impl Fn(usize) -> usize,
        recall: bool,
    ) -> Vec<usize> {
        let (cooldown, cast) = if recall {
            (self.params.recall_cooldown, self.params.teleport_cast)
        } else {
            (self.params.hearth_cooldown, self.params.hearth_cast)
        };
        let learned = |s: &State| !recall || self.model.recall.is_some_and(|l| l <= s.trained);
        // Time saved by going back to the inn before stop `k` rather than travelling to it.
        let saving = |k: usize| {
            let s = &run.states[k];
            if route[k].kind == Kind::Hearth
                || !learned(s)
                || (!recall && self.params.excluded_zones.contains(&self.model.inns[inn_at(k)].zone))
            {
                return f64::NEG_INFINITY;
            }
            let target = self.places().pos(s, route[k]);
            let direct = self.trips().fastest(s, &target).0;
            let inn = &self.model.inns[inn_at(k)].pos;
            let open = self.trips().open_links(s);
            direct - cast - self.trips().leg(inn, &target, s.level, s.known, open).0
        };
        let mut uses = Vec::new();
        let mut ready = if recall {
            run.states[from].recall_ready
        } else {
            run.states[from].hearth_ready
        };
        let mut n = 0;
        while n < stops.len() {
            let k = stops[n];
            if run.states[k].time < ready || saving(k) < self.params.hearth_min_saving {
                n += 1;
                continue;
            }
            let until = run.states[k].time + WINDOW;
            let mut best = (saving(k), n);
            for (m, &j) in stops.iter().enumerate().skip(n + 1) {
                if run.states[j].time > until {
                    break;
                }
                let v = saving(j);
                if v > best.0 {
                    best = (v, m);
                }
            }
            let k = stops[best.1];
            uses.push(k);
            // The route replayed has not saved that time: the next use waits for it too, or the
            // hearthstone would not be ready yet.
            ready = run.states[k].time + cooldown + best.0;
            n = best.1 + 1;
        }
        uses
    }

    /// The `n` inns closest to `pos` on its continent the hearthstone can be bound to.
    pub(crate) fn inns_near(&self, pos: &Pos, n: usize) -> Vec<usize> {
        let mut inns: Vec<(usize, f64)> = self
            .model
            .inns
            .iter()
            .enumerate()
            .filter(|(i, l)| *i != self.model.start_inn && l.pos.continent == pos.continent)
            .map(|(i, l)| (i, l.pos.dist(pos)))
            .collect();
        inns.sort_by(|x, y| x.1.total_cmp(&y.1));
        inns.into_iter().take(n).map(|(i, _)| i).collect()
    }
}
