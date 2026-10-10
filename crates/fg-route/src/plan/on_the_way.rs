//! What happens while the character moves: the trip itself (hearthstone, flight, boat), the
//! areas discovered, the mobs and objects of the log met on the way, and what it does while
//! there (quests turned in and taken around a stop).

use super::Planner;
use super::state::{Event, Kind, Leg, State, Stop};
use crate::model::Loc;
use crate::world::Pos;
use crate::xp;
use std::rc::Rc;

/// Free quest log slots kept when taking a quest at hand (the route takes others before it).
pub(crate) const CAMP_LOG_MARGIN: usize = 2;
/// Quests taken at hand: those the route takes within this many stops anyway.
pub(crate) const CAMP_AHEAD: u32 = 30;

/// A walk: continent and the bits of its ends' coordinates.
pub(crate) type FarmKey = (i64, [u64; 4]);

/// Normal mobs met on each walk, by level: (level, count).
pub(crate) type FarmWalks = std::cell::RefCell<std::collections::HashMap<FarmKey, Rc<[(u8, u16)]>>>;

/// Walks whose farmed mobs are remembered, beyond which the memo starts over.
const FARM_MEMO: usize = 1 << 20;

/// Distance from `p` to the segment `a`-`b` (same continent).
pub(crate) fn segment_dist(p: &Pos, a: &Pos, b: &Pos) -> f64 {
    let (dx, dy) = (b.x - a.x, b.y - a.y);
    let len2 = dx * dx + dy * dy;
    let t = if len2 == 0.0 {
        0.0
    } else {
        (((p.x - a.x) * dx + (p.y - a.y) * dy) / len2).clamp(0.0, 1.0)
    };
    ((p.x - a.x - t * dx).powi(2) + (p.y - a.y - t * dy).powi(2)).sqrt()
}

impl Planner<'_> {
    /// Move to `target`, recording how.
    pub(crate) fn go(&self, s: &mut State, target: &Pos, zone: i64, record: &mut impl FnMut(&State, Event)) {
        let start = s.pos;
        let (t, via) = self.trips().fastest(s, target);
        let flying = matches!(via.leg, Leg::Fly(..));
        // Events are recorded when they happen (casting the hearthstone, boarding), so that
        // each step of the guide holds the time up to the next one.
        let spend = |s: &mut State, seconds: f64| {
            s.time += seconds;
            if flying {
                s.spent.flights += seconds;
            } else {
                s.spent.travel += seconds;
            }
        };
        let mut left = t;
        let mut from = start;
        if via.hearth {
            s.hearth_ready = s.time + self.params.hearth_cooldown;
            s.spent.hearths += 1;
            record(s, Event::Hearth { inn: s.bind });
            spend(s, self.params.hearth_cast);
            left -= self.params.hearth_cast;
            from = self.model.inns[s.bind].pos;
        }
        let boarding = match via.leg {
            Leg::Fly(a, _) => Some(self.world.taxi_nodes[a].pos),
            Leg::Link(a, _) => Some(self.world.links[a].from),
            Leg::Walk => None,
        };
        if let Some(at) = boarding {
            let walk = self.trips().walk(&from, &at, s.level).min(left);
            spend(s, walk);
            left -= walk;
        }
        match via.leg {
            Leg::Fly(from, to) => {
                let price = self.trips().flight_table(s.known).1[from][to];
                if price.is_finite() {
                    s.spent.flight_cost += price as i64;
                }
                record(s, Event::Fly { from, to });
            }
            Leg::Link(first, last) => record(s, Event::Link { first, last }),
            Leg::Walk => {}
        }
        spend(s, left);
        // Areas discovered: around the arrival, and along a walk inside the zone.
        let from = (matches!(via.leg, Leg::Walk) && !via.hearth && s.zone == zone).then_some(s.pos);
        self.explore(s, from, target, zone);
        if matches!(via.leg, Leg::Walk) && !via.hearth {
            self.kill_along(s, &start, target, record);
            self.farm_along(s, &start, target, record);
        }
        s.pos = *target;
        if zone != s.zone {
            s.spent.zone_changes += 1;
        }
        s.zone = zone;
    }

    /// XP of the areas of `zone` within reach of the segment `from`-`to` (or of `to`).
    pub(crate) fn explore(&self, s: &mut State, from: Option<Pos>, to: &Pos, zone: i64) {
        let Some(areas) = self.model.explore_by_zone.get(&zone) else {
            return;
        };
        let r = self.params.explore_radius;
        for &i in areas {
            if s.explored[i] {
                continue;
            }
            let c = &self.model.explore[i].pos;
            let d = match from {
                Some(a) => segment_dist(c, &a, to),
                None => c.dist(to),
            };
            if d <= r {
                s.explored[i] = true;
                let xp = self.rules().exploration_xp(s.level, self.model.explore[i].level);
                s.spent.explore_xp += xp;
                self.growth().gain(s, xp);
            }
        }
    }

    /// The objectives of quest `i`, just accepted, that can be done on the way.
    pub(crate) fn open_along(&self, s: &mut State, i: u32) {
        for (k, o) in self.quest(i).objectives.iter().enumerate() {
            if o.spots.is_some()
                && (o.kills > 0.0 || o.uses > 0.0)
                && o.dungeon.is_none()
                && s.objectives[i as usize] & (1 << k) == 0
            {
                s.along.push((i, k as u8, 0.0));
            }
        }
    }

    /// On a walk from `a` to `b`, kill the mobs and loot the objects of the log's objectives met
    /// on the way: those within `along_corridor` of the path, up to what the objective still
    /// needs (weapons lying around the valley are picked up while doing the other quests).
    pub(crate) fn kill_along(&self, s: &mut State, a: &Pos, b: &Pos, record: &mut impl FnMut(&State, Event)) {
        let corridor = self.params.along_corridor;
        if corridor <= 0.0 || s.along.is_empty() || a.continent != b.continent {
            return;
        }
        let lo = (a.x.min(b.x) - corridor, a.y.min(b.y) - corridor);
        let hi = (a.x.max(b.x) + corridor, a.y.max(b.y) + corridor);
        let mut n = 0;
        while n < s.along.len() {
            let (qi, k, done) = s.along[n];
            let o = &self.quest(qi).objectives[k as usize];
            let Some(spots) = &o.spots else {
                n += 1;
                continue;
            };
            let apart = spots.continent != a.continent
                || spots.max.0 < lo.0
                || spots.min.0 > hi.0
                || spots.max.1 < lo.1
                || spots.min.1 > hi.1;
            if apart || self.combat().need(self.quest(qi), o) > self.params.reach(s.level, s.bonus) {
                n += 1;
                continue;
            }
            let met = spots
                .points
                .iter()
                .filter(|&&(x, y)| {
                    let p = Pos {
                        continent: a.continent,
                        x,
                        y,
                        zone: 0,
                    };
                    segment_dist(&p, a, b) <= corridor
                })
                .count() as f64;
            let total = if o.kills > 0.0 { o.kills } else { o.uses };
            let kills = met.min(total - done);
            if kills < 1.0 {
                n += 1;
                continue;
            }
            if o.kills > 0.0 {
                let work = kills * self.combat().kill_time(s.level, s.bonus, o);
                s.time += work;
                s.spent.fighting += work;
                let mob_xp = (kills * self.combat().mob_xp(s.level, o)) as i64;
                s.spent.mob_xp += mob_xp;
                self.growth().gain(s, mob_xp);
            } else {
                let work = kills * self.params.object_time;
                s.time += work;
                s.spent.fighting += work;
            }
            let finished = done + kills >= total - 1e-9;
            if finished {
                s.objectives[qi as usize] |= 1 << k;
                s.along.swap_remove(n);
            } else {
                s.along[n].2 += kills;
                n += 1;
            }
            record(
                s,
                Event::Along {
                    quest: qi as usize,
                    objective: k,
                    kills,
                    finished,
                },
            );
        }
    }

    /// On a walk from `a` to `b`, kill the normal mobs met (`Params::farm_on_way`): those within
    /// `farm_corridor` of the path whose level is close to the character's, until grinding has
    /// given its share of the level (`grind_cap`).
    pub(crate) fn farm_along(&self, s: &mut State, a: &Pos, b: &Pos, record: &mut impl FnMut(&State, Event)) {
        let p = self.params;
        if !p.farm_on_way || p.farm_corridor <= 0.0 || a.continent != b.continent || a == b {
            return;
        }
        let budget = self.growth().grind_budget(s);
        if budget <= 0 || s.level >= self.profile.to_level {
            return;
        }
        let mobs = self.farm_walk(a, b);
        let content = xp::content_at(a.continent, a.zone);
        let reach = p.reach(s.level, s.bonus);
        let (mut time, mut gained, mut killed) = (0.0, 0.0, 0.0);
        for &(level, count) in mobs.iter() {
            let level = i64::from(level);
            if level < s.level - p.farm_below || level > s.level + p.farm_above || level as f64 > reach {
                continue;
            }
            let each = self.rules().mob_xp(s.level, level, content) * p.group_xp_share();
            if each <= 0.0 {
                continue;
            }
            let kills = f64::from(count).min(((budget as f64 - gained) / each).floor());
            if kills < 1.0 {
                break;
            }
            time += kills * p.kill_time(s.level, s.bonus, level);
            gained += kills * each;
            killed += kills;
        }
        if gained <= 0.0 {
            return;
        }
        let gained = gained as i64;
        s.time += time;
        s.spent.farming += time;
        s.spent.farm_xp += gained;
        s.grind_used += gained;
        self.growth().gain(s, gained);
        record(
            s,
            Event::Farm {
                kills: killed as u32,
                seconds: time,
            },
        );
    }

    /// Normal mobs by level along a walk, computed once per walk.
    fn farm_walk(&self, a: &Pos, b: &Pos) -> Rc<[(u8, u16)]> {
        let key = (
            a.continent,
            [a.x.to_bits(), a.y.to_bits(), b.x.to_bits(), b.y.to_bits()],
        );
        if let Some(mobs) = self.farm_walks.borrow().get(&key) {
            return mobs.clone();
        }
        let mobs: Rc<[(u8, u16)]> = self.model.farm.along(a, b, self.params.farm_corridor).into();
        let mut memo = self.farm_walks.borrow_mut();
        if memo.len() >= FARM_MEMO {
            memo.clear();
        }
        memo.insert(key, mobs.clone());
        mobs
    }

    /// While here: turn in the finished quests and take the quests of the route whose NPCs
    /// are within `camp_radius` of this stop, nearest first, as a player does instead of coming
    /// back later.
    pub(crate) fn while_here(&self, s: &mut State, record: &mut impl FnMut(&State, Event)) {
        let radius = self.params.camp_radius;
        if radius <= 0.0 || s.planned.flags.is_empty() {
            return;
        }
        let here = s.pos;
        let (cx, cy) = ((here.x / radius).floor() as i64, (here.y / radius).floor() as i64);
        loop {
            let mut best: Option<(f64, Stop, &Loc)> = None;
            for dx in -1..=1 {
                for dy in -1..=1 {
                    let Some(cell) = self.camp.get(&(here.continent, cx + dx, cy + dy)) else {
                        continue;
                    };
                    for &(i, at, start) in cell {
                        let q = self.quest(i);
                        let (loc, kind) = if start {
                            (&q.starts[at as usize], Kind::Accept)
                        } else {
                            (&q.ends[at as usize], Kind::TurnIn)
                        };
                        // Quests the route takes soon anyway (at the hub, instead of coming
                        // back). Taking one early must not break the route: keep room in the log
                        // for the quests it takes before it, do not block a quest it takes
                        // later (an exclusive one, the breadcrumb to this one).
                        let wanted = if start {
                            s.planned.flags[i as usize] & 1 != 0
                                && s.planned.accept_at[i as usize] <= s.at + CAMP_AHEAD
                                && q.min_level <= s.level
                                && s.log + 1 + CAMP_LOG_MARGIN <= self.params.quest_log_size
                                && self.availability().can_accept(s, i)
                                && !self.blocks[i as usize].iter().any(|&y| {
                                    s.planned.flags[y as usize] & 1 != 0
                                        && !s.accepted[y as usize]
                                        && !s.turned[y as usize]
                                })
                                && q.objectives
                                    .iter()
                                    .all(|o| o.dungeon.is_none_or(|d| self.availability().dungeon_allowed(d)))
                        } else {
                            s.planned.flags[i as usize] & 2 != 0
                                && s.accepted[i as usize]
                                && !s.turned[i as usize]
                                && self.availability().objectives_done(s, i)
                        };
                        // Nearest to where we are, turn-ins first at the same distance (they
                        // unlock follow-ups).
                        let d = s.pos.dist(&loc.pos) + if start { 0.1 } else { 0.0 };
                        if wanted && here.dist(&loc.pos) <= radius && best.as_ref().is_none_or(|b| d < b.0) {
                            best = Some((d, Stop::quest(i, kind), loc));
                        }
                    }
                }
            }
            let Some((_, stop, loc)) = best else { break };
            self.go(s, &loc.pos, loc.zone, record);
            s.time += self.params.stop_overhead;
            s.spent.overhead += self.params.stop_overhead;
            let picked = if stop.kind == Kind::Accept {
                self.accept(s, stop.index);
                None
            } else {
                self.turn_in(s, stop.index)
            };
            record(s, Event::Stop { stop, loc: loc.clone() });
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
    }
}
