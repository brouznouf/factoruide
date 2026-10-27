//! What happens while the character moves: the trip itself (teleport, flight, boat), the
//! areas discovered, the mobs and objects of the log met on the way, and what it does while
//! there (quests turned in and taken around a stop).

use super::Planner;
use super::state::{Event, Kind, Landed, Leg, State, Stop};
use crate::model::{Loc, Met};
use crate::world::Pos;
use crate::xp;
use rustc_hash::FxHashMap;
use std::rc::Rc;

/// Free quest log slots kept when taking a quest at hand (the route takes others before it).
pub(crate) const CAMP_LOG_MARGIN: usize = 2;
/// Quests taken at hand: those the route takes within this many stops anyway.
pub(crate) const CAMP_AHEAD: u32 = 30;

/// A walk: in one word the walk level (0: no zone closed), the continent and its ends'
/// zones (the passages taken between them), then the bits of its ends' coordinates and their
/// heights.
pub(crate) type FarmKey = (u64, [u64; 5]);

/// Normal mobs met on each walk, by level (see `FarmMobs::along`).
pub(crate) type FarmWalks = std::cell::RefCell<FxHashMap<FarmKey, Rc<[Met]>>>;

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
        // Off a flight, boat or teleport step: the end of its trip, on foot.
        let landed = std::mem::take(&mut s.landed);
        let (t, via) = if landed == Landed::No {
            self.trips().fastest(s, target)
        } else {
            (
                self.trips().walk(&s.pos, target, s.level),
                super::state::Via {
                    teleport: None,
                    death: None,
                    leg: Leg::Walk,
                },
            )
        };
        // A trip by flight alone counts as flying, walks included; one with boats as travel,
        // its flight apart.
        let flying = landed == Landed::Flight || matches!(via.leg, Leg::Fly(..));
        // Events are recorded when they happen (casting the hearthstone, boarding), so that
        // each step of the guide holds the time up to the next one.
        let spend = |s: &mut State, seconds: f64, flight: bool| {
            s.time += seconds;
            if flying || flight {
                s.spent.flights += seconds;
            } else {
                s.spent.travel += seconds;
            }
        };
        let mut left = t;
        let mut from = start;
        if let Some(to) = via.teleport {
            record(s, Event::Teleport { to });
            spend(s, self.params.teleport_cast, false);
            left -= self.params.teleport_cast;
            from = self.model.teleports[to].to.pos;
        }
        if let Some(g) = via.death {
            let gy = &self.model.graveyards[g];
            record(
                s,
                Event::DeathSkip {
                    pos: gy.pos,
                    zone: gy.zone,
                },
            );
            spend(s, self.params.death_skip_time, false);
            left -= self.params.death_skip_time;
            from = gy.pos;
        }
        // Walked to `at`, at most what is left of the trip.
        let walk_to = |s: &mut State, from: &Pos, at: &Pos, left: &mut f64| {
            let walk = self.trips().walk(from, at, s.level).min(*left);
            spend(s, walk, false);
            *left -= walk;
        };
        // The price of the flight from `a` to `b`, paid on boarding.
        let pay = |s: &mut State, a: usize, b: usize| {
            let price = self.trips().flight_table(s.known).1[a][b];
            if price.is_finite() {
                s.spent.flight_cost += price as i64;
                s.spent.flight_gold += self.params.gold_weight_at(s.level) * price / 10_000.0;
            }
        };
        // The flight itself, when the trip goes on after it.
        let ride = |s: &mut State, seconds: f64, flight: bool, left: &mut f64| {
            let seconds = seconds.min(*left);
            spend(s, seconds, flight);
            *left -= seconds;
        };
        let nodes = &self.world.taxi_nodes;
        let links = &self.world.links;
        match via.leg {
            Leg::Fly(a, b) => {
                walk_to(s, &from, &nodes[a].pos, &mut left);
                pay(s, a, b);
                record(s, Event::Fly { from: a, to: b });
            }
            Leg::Link(first, last) => {
                walk_to(s, &from, &links[first].from, &mut left);
                record(s, Event::Link { first, last });
            }
            Leg::FlyLink(a, b, first, last) => {
                walk_to(s, &from, &nodes[a].pos, &mut left);
                pay(s, a, b);
                record(s, Event::Fly { from: a, to: b });
                let flight = self.trips().flight_table(s.known).0[a][b] + self.params.flight_overhead;
                ride(s, flight, true, &mut left);
                walk_to(s, &nodes[b].pos, &links[first].from, &mut left);
                record(s, Event::Link { first, last });
            }
            Leg::LinkFly(first, last, a, b) => {
                walk_to(s, &from, &links[first].from, &mut left);
                record(s, Event::Link { first, last });
                let boats = self.world.link_chain(self.trips().open_links(s))[first][last];
                ride(s, boats, false, &mut left);
                walk_to(s, &links[last].to, &nodes[a].pos, &mut left);
                pay(s, a, b);
                record(s, Event::Fly { from: a, to: b });
                // What is left: the flight and the walk from its landing.
                let flight = self.trips().flight_table(s.known).0[a][b] + self.params.flight_overhead;
                ride(s, flight, true, &mut left);
            }
            Leg::Walk => {
                // A climb or a jump is a step of the guide, recorded on reaching it.
                if let Some(&pass) = self.world.walk_shortcuts(&from, target).first() {
                    walk_to(s, &from, &self.world.passes[pass].pos, &mut left);
                    record(s, Event::Shortcut { pass });
                }
            }
        }
        spend(s, left, false);
        // Areas discovered: around the arrival, and along a walk inside the zone.
        let walking =
            matches!(via.leg, Leg::Walk) && via.teleport.is_none() && via.death.is_none() && landed == Landed::No;
        let from = (walking && s.zone == zone).then_some(s.pos);
        self.explore(s, from, target, zone);
        if walking {
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
        // Areas out of the box of the segment grown by `r` (and a yard) are farther than `r`.
        let a = from.unwrap_or(*to);
        let (x0, x1) = (a.x.min(to.x) - r - 1.0, a.x.max(to.x) + r + 1.0);
        let (y0, y1) = (a.y.min(to.y) - r - 1.0, a.y.max(to.y) + r + 1.0);
        for &i in areas {
            let c = &self.model.explore[i].pos;
            if c.x < x0 || c.x > x1 || c.y < y0 || c.y > y1 || s.explored[i] {
                continue;
            }
            let d = match from {
                Some(a) => segment_dist(c, &a, to),
                None => c.dist(to),
            };
            if d <= r {
                s.explored.set(i, true);
                s.spent.explore_xp += self.growth().explore(s, self.model.explore[i].level);
            }
        }
    }

    /// On a walk from `a` to `b`, kill the normal mobs met (`Params::farm_on_way`): of those
    /// within `farm_corridor` of the path walked (`World::walk_path`) whose level is close to the
    /// character's, the share left to it (`Params::farm_share_at`), until farming has given its
    /// share of the level (`farm_cap`); beyond it when the walk is asked to (`farm_all`, once).
    /// Never the mobs of the log's quests: their objectives count them, where they are done
    /// (the walk there, the spot itself and the walk away from it).
    pub(crate) fn farm_along(&self, s: &mut State, a: &Pos, b: &Pos, record: &mut impl FnMut(&State, Event)) {
        let p = self.params;
        let all = self.farm_all.replace(false);
        if !(p.farm_on_way || all) || p.farm_corridor <= 0.0 || a.continent != b.continent || a == b {
            return;
        }
        let budget = if all { i64::MAX } else { self.growth().farm_budget(s) };
        if budget <= 0 || s.level >= self.profile.to_level {
            return;
        }
        let mobs = self.farm_walk(a, b, self.walk_level(s.level));
        let content = xp::content_at(a.continent, a.zone);
        let reach = p.reach(s.level, s.might());
        let share = p.farm_share_at(s.level);
        let in_log = |q: u32| s.accepted[q as usize] && !s.turned[q as usize];
        let (mut time, mut gained, mut killed) = (0.0, 0.0, 0.0);
        for m in mobs.iter() {
            let level = i64::from(m.level);
            if level < s.level - p.farm_below || level > s.level + p.farm_above || level as f64 > reach {
                continue;
            }
            if self.model.farm.quests(m.tag).iter().any(|&q| in_log(q)) {
                continue;
            }
            let each = self.rules().mob_xp(s.level, level, content) * p.group_xp_share();
            if each <= 0.0 {
                continue;
            }
            let room = ((budget as f64 - gained) / each).floor();
            if room < 1.0 {
                break;
            }
            let kills = (f64::from(m.count) * share).min(room);
            if kills <= 0.0 {
                continue;
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
        s.farm_used += gained;
        let xp = self.growth().gain_kills(s, gained);
        s.spent.farm_xp += xp;
        self.growth().gather(s, a.zone, killed);
        record(
            s,
            Event::Farm {
                kills: killed.round() as u32,
                xp,
                seconds: time,
            },
        );
    }

    /// Normal mobs by level along a walk (the terrain's path for a character of the walk level `level`), computed once per walk.
    fn farm_walk(&self, a: &Pos, b: &Pos, level: u8) -> Rc<[Met]> {
        let farm = || -> Rc<[Met]> {
            let path = self.world.walk_path(a, b, level);
            self.model.farm.along(&path, self.params.farm_corridor).into()
        };
        // Zones on 20 bits and the continent on 16 (the others are not remembered).
        let (Ok(continent), Ok(za), Ok(zb)) =
            (u16::try_from(a.continent), u32::try_from(a.zone), u32::try_from(b.zone))
        else {
            return farm();
        };
        if za >= 1 << 20 || zb >= 1 << 20 {
            return farm();
        }
        let head = (u64::from(level) << 56) | (u64::from(continent) << 40) | (u64::from(za) << 20) | u64::from(zb);
        let heights = a.height_key() << 32 | b.height_key();
        let key = (
            head,
            [a.x.to_bits(), a.y.to_bits(), b.x.to_bits(), b.y.to_bits(), heights],
        );
        if let Some(mobs) = self.farm_walks.borrow().get(&key) {
            return mobs.clone();
        }
        let mobs = farm();
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
        if radius <= 0.0 || s.planned.flags.is_empty() || self.explicit {
            return;
        }
        let here = s.pos;
        if let Some(heres) = self.heres.borrow_mut().as_mut() {
            heres.push(here);
        }
        let (cx, cy) = ((here.x / radius).floor() as i64, (here.y / radius).floor() as i64);
        loop {
            let mut best: Option<(f64, Stop, &Loc)> = None;
            for dx in -1..=1 {
                for dy in -1..=1 {
                    let Some(cell) = self.camp.get(&(here.continent, cx + dx, cy + dy)) else {
                        continue;
                    };
                    // The cheap tests first: the NPC at hand, nearer than the best so far, a
                    // quest the route plans; the quest itself is read last.
                    for &(i, at, start, x, y) in cell {
                        let npc = Pos { x, y, ..here };
                        let at_hand = here.dist(&npc) <= radius;
                        if !at_hand {
                            continue;
                        }
                        // Nearest to where we are, turn-ins first at the same distance (they
                        // unlock follow-ups).
                        let d = s.pos.dist(&npc) + if start { 0.1 } else { 0.0 };
                        let nearer = best.as_ref().is_none_or(|b| d < b.0);
                        if !nearer {
                            continue;
                        }
                        let iu = i as usize;
                        let planned = if start {
                            s.planned.flags[iu] & 1 != 0 && s.planned.accept_at[iu] <= s.at + CAMP_AHEAD
                        } else {
                            s.planned.flags[iu] & 2 != 0 && s.accepted[iu] && !s.turned[iu]
                        };
                        if !planned {
                            continue;
                        }
                        let q = self.quest(i);
                        let (loc, kind) = if start {
                            (&q.starts[at as usize], Kind::Accept)
                        } else {
                            (&q.ends[at as usize], Kind::TurnIn)
                        };
                        // Quests the route takes soon anyway (at the hub, instead of coming
                        // back). Taking one early must not break the route: keep room in the log
                        // for the quests it takes before it, do not block a quest it takes
                        // later (an exclusive one, the breadcrumb to this one). Nor go where the
                        // route would not yet: its level, a camp to fight through (`gate_level`).
                        let at_level = || self.gate_level(s, Stop::quest(i, kind)) <= s.level;
                        let wanted = if start {
                            s.log + 1 + CAMP_LOG_MARGIN <= self.params.quest_log_size
                                && self.availability().can_accept(s, i)
                                && !self.blocks[iu].iter().any(|&y| {
                                    s.planned.flags[y as usize] & 1 != 0
                                        && !s.accepted[y as usize]
                                        && !s.turned[y as usize]
                                })
                                && q.objectives
                                    .iter()
                                    .all(|o| o.dungeon.is_none_or(|d| self.availability().dungeon_allowed(d)))
                                && at_level()
                        } else {
                            self.availability().objectives_done(s, i) && at_level()
                        };
                        if wanted {
                            best = Some((d, Stop::quest(i, kind), loc));
                        }
                    }
                }
            }
            let Some((_, stop, loc)) = best else { break };
            self.go(s, &loc.pos, loc.zone, record);
            s.time += self.params.stop_overhead;
            s.spent.overhead += self.params.stop_overhead;
            if Self::timed(stop.kind) && self.failed_in_time(s, stop.index, record) {
                continue;
            }
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
