//! Writes the step of the guide for each event of a replayed route: its kind, its sentences
//! (language-neutral phrases with the English names, see `Phrase`), its target and where it
//! takes place.

use super::phrase::{Arg, Phrase};
use super::types::{MapPointOut, Step, StepKind, Target, TargetKind};
use crate::model::{EntityKind, Loc, Objective, Spots};
use crate::plan::{Event, Kind, Planner, ServiceKind, Stop, Timed};
use crate::world::{MapPoint, Pos};
use std::collections::HashSet;

/// Where a step takes place: on its zone's map, and in the world.
type Place = Option<(MapPoint, Pos)>;

pub(super) struct StepWriter<'a> {
    pub(super) planner: &'a Planner<'a>,
    pub(super) route: &'a [Stop],
    /// Objectives done along the way while doing other quests.
    pub(super) background: HashSet<(u32, u8)>,
}

/// A number as a sentence argument.
fn num(n: i64) -> Arg {
    Arg::Num(n)
}

/// What stands at a place, as a sentence argument: an NPC, an object or an item by ID, a
/// dungeon by its area, else its name as it is.
fn entity(loc: &Loc) -> Arg {
    match loc.kind {
        EntityKind::Npc => Arg::Npc(loc.id, loc.name.clone()),
        EntityKind::Object => Arg::Object(loc.id, loc.name.clone()),
        EntityKind::Item => Arg::Item(loc.id, loc.name.clone()),
        EntityKind::Dungeon => Arg::Zone(loc.id, loc.name.clone()),
        EntityKind::Area | EntityKind::Taxi => Arg::Text(loc.name.clone()),
    }
}

/// The sentence of an objective's step.
pub(super) fn objective_phrase(o: &Objective) -> Phrase {
    let n = o.count.round().max(1.0) as i64;
    // The database's text (English) says it best in English, and for dungeons and areas.
    let text = Arg::Text(o.text.clone());
    // Creatures acted on (healed, a spell cast on them: uses, no kills) too: talking is not it.
    let acted_on = o.loc.kind == EntityKind::Npc && o.kills <= 0.0 && o.uses > 0.0;
    if o.dungeon.is_some() || o.loc.kind == EntityKind::Area || acted_on {
        Phrase::new("objective.text").arg("text", text)
    } else {
        let (key, counted) = match o.loc.kind {
            EntityKind::Npc if o.kills > 0.0 => ("objective.kill", true),
            EntityKind::Npc => ("objective.talk", false),
            EntityKind::Object => ("objective.use", true),
            EntityKind::Item if o.text.starts_with("Buy ") => ("objective.buy", true),
            _ => ("objective.get", true),
        };
        let phrase = Phrase::new(key).arg("who", entity(&o.loc)).arg("text", text);
        if counted { phrase.arg("n", num(n)) } else { phrase }
    }
}

impl StepWriter<'_> {
    /// The step of an event of the trace.
    pub(super) fn step(&self, t: &Timed) -> Step {
        let mut step = self.blank(t);
        let place = match &t.event {
            Event::Reward { .. } | Event::Farm { .. } => None,
            Event::Service { kind, index } => self.service(&mut step, *kind, *index),
            Event::DungeonExit { dungeon } => {
                let d = &self.planner.model.dungeons[*dungeon];
                step.kind = StepKind::DungeonExit;
                step.target = Some(Target {
                    kind: TargetKind::Dungeon,
                    id: d.area,
                    name: d.name.clone(),
                });
                step.say
                    .push(Phrase::new("dungeon.exit").arg("dungeon", Arg::Zone(d.area, d.name.clone())));
                self.place(&d.entrance.pos, Some(d.entrance.zone))
            }
            Event::Abandon { quest } => self.abandon(&mut step, *quest),
            Event::Fly { from, to } => self.fly(&mut step, *from, *to),
            Event::Link { first, last } => self.link(&mut step, *first, *last),
            Event::Shortcut { pass } => self.shortcut(&mut step, *pass),
            Event::LearnFlight { node } => self.learn_flight(&mut step, *node),
            Event::Grind {
                to_level,
                seconds,
                near,
                zone,
            } => self.grind(&mut step, *to_level, *seconds, near, *zone),
            Event::GrindMobs {
                mobs,
                seconds,
                near,
                zone,
            } => self.grind_mobs(&mut step, t, *mobs, *seconds, near, *zone),
            Event::DeathSkip { pos, zone } => self.death_skip(&mut step, pos, *zone),
            Event::Hearth { inn, recall } => self.hearth(&mut step, *inn, *recall),
            Event::Teleport { to } => self.teleport(&mut step, *to),
            Event::Train { trainer, since } => self.train(&mut step, *trainer, *since, t.level),
            Event::Stop { stop, loc } if !stop.is_quest() => self.visit(&mut step, *stop, loc),
            Event::Stop { stop, loc } => self.quest_stop(&mut step, *stop, loc),
        };
        Self::set_place(&mut step, place, self.planner.world);
        step
    }

    /// The step's zone, map point and world position.
    fn set_place(step: &mut Step, place: Place, world: &crate::world::World) {
        if let Some((m, p)) = place {
            step.zone = Some(world.zone_name(m.zone).to_owned());
            step.zone_id = Some(m.zone);
            step.map = Some(m.into());
            step.world = Some([
                p.continent as f64,
                (p.x * 10.0).round() / 10.0,
                (p.y * 10.0).round() / 10.0,
            ]);
        }
    }

    /// The step placed at `pos` in `zone`.
    pub(super) fn put_at(&self, step: &mut Step, pos: &Pos, zone: i64) {
        Self::set_place(step, self.place(pos, Some(zone)), self.planner.world);
    }

    /// The mobs farmed on the way to a step and their XP: the addon counts the XP of the kills,
    /// shown apart from its text.
    pub(super) fn add_farm(step: &mut Step, kills: u32, xp: i64) {
        step.farm = Some(kills);
        step.farm_xp = (xp > 0).then_some(xp);
    }

    /// A checkpoint: the guide goes on from the level (and XP) the route has there, grinding
    /// first when behind (as long as needed) so that the next part of the route fits the
    /// character. `at`: the step where it takes place.
    pub(super) fn checkpoint(&self, at: &Step, checkpoint: i64, level: i64, xp: i64) -> Step {
        let say = if xp > 0 {
            Phrase::new("checkpoint.xp")
                .arg("checkpoint", num(checkpoint))
                .arg("level", num(level))
                .arg("p", num(self.xp_share(level, xp)))
                .arg("xp", num(xp))
        } else {
            Phrase::new("checkpoint").arg("level", num(level))
        };
        Step {
            kind: StepKind::Grind,
            quest: None,
            quest_name: None,
            alt: Vec::new(),
            pre: Vec::new(),
            pre_any: Vec::new(),
            min_level: None,
            bg: false,
            try_buy: false,
            buy: false,
            inside: false,
            held: false,
            objective: None,
            child: None,
            child_name: None,
            text: String::new(),
            say: vec![say],
            target: None,
            skill: None,
            line: None,
            mobs: Vec::new(),
            mob_ids: Vec::new(),
            elite: false,
            mob_level: None,
            reward: None,
            reward_xp: None,
            route_xp: None,
            spells: Vec::new(),
            xp: (xp > 0).then_some(xp),
            farm: None,
            farm_xp: None,
            level,
            keep: true,
            event: None,
            ..at.clone()
        }
    }

    /// The step before `next` (a level that is no checkpoint): a character behind the route by
    /// more than a little catches up to `xp` XP in `level` at `at` before going on.
    pub(super) fn catch_up(&self, at: &Step, next: i64, level: i64, xp: i64) -> Step {
        let mut step = self.checkpoint(at, next, level, xp);
        step.say = vec![
            Phrase::new("catch_up")
                .arg("next", num(next))
                .arg("level", num(level))
                .arg("p", num(self.xp_share(level, xp)))
                .arg("xp", num(xp)),
        ];
        step
    }

    /// A planned grind moved to a place with mobs of the character's level, earlier: it grinds
    /// to `xp` XP in `level` there.
    pub(super) fn grind_before(&self, grind: &Step, at: &Step, level: i64, xp: i64, minutes: f64) -> Step {
        let minutes = num(minutes as i64);
        let say = if xp > 0 {
            Phrase::new("grind.xp")
                .arg("level", num(level))
                .arg("p", num(self.xp_share(level, xp)))
                .arg("xp", num(xp))
                .arg("minutes", minutes)
        } else {
            Phrase::new("grind").arg("level", num(level)).arg("minutes", minutes)
        };
        Step {
            text: String::new(),
            say: vec![say],
            xp: (xp > 0).then_some(xp),
            farm: None,
            farm_xp: None,
            level,
            zone: at.zone.clone(),
            zone_id: at.zone_id,
            map: at.map.clone(),
            world: at.world,
            path: Vec::new(),
            ..grind.clone()
        }
    }

    /// Percentage of the level an XP count is.
    fn xp_share(&self, level: i64, xp: i64) -> i64 {
        xp * 100 / self.planner.rules().to_next_level(level).max(1)
    }

    /// The reward picked, on the quest's turn-in step just before.
    pub(super) fn add_reward(&self, steps: &mut [Step], quest: usize, item: i64) {
        let id = self.planner.model.quests[quest].id;
        let Some(step) = steps
            .iter_mut()
            .rev()
            .find(|s| s.kind == StepKind::TurnIn && s.quest == Some(id))
        else {
            return;
        };
        step.reward = Some(item);
        let english = self.planner.model.power.as_ref().map_or("", |p| p.item_name(item));
        step.say
            .push(Phrase::new("suffix.reward").arg("item", Arg::Item(item, english.to_owned())));
    }

    fn blank(&self, t: &Timed) -> Step {
        let power = self.planner.model.power.as_ref();
        Step {
            kind: StepKind::Objective,
            quest: None,
            quest_name: None,
            alt: Vec::new(),
            pre: Vec::new(),
            pre_any: Vec::new(),
            min_level: None,
            bg: false,
            try_buy: false,
            buy: false,
            inside: false,
            held: false,
            objective: None,
            child: None,
            child_name: None,
            text: String::new(),
            say: Vec::new(),
            target: None,
            zone: None,
            zone_id: None,
            map: None,
            spots: Vec::new(),
            patrol: false,
            world: None,
            path: Vec::new(),
            skill: None,
            line: None,
            mobs: Vec::new(),
            mob_ids: Vec::new(),
            elite: false,
            mob_level: None,
            reward: None,
            reward_xp: None,
            route_xp: None,
            spells: Vec::new(),
            xp: None,
            farm: None,
            farm_xp: None,
            level: t.level,
            time: t.time.round(),
            power: power.map(|_| (t.power * 10.0).round() / 10.0),
            gear: power.map(|_| (t.gear * 10.0).round() / 10.0),
            keep: false,
            event: None,
        }
    }

    /// The other places of mobs whose spawns are `spots`, away from `first`, on the map of the
    /// step's zone `zone` when they are on it (a spawn by the border), else of their own.
    fn spots(&self, spots: Option<&Spots>, first: &Pos, zone: i64) -> Vec<MapPointOut> {
        /// Places shown at most besides the step's own.
        const MAX: usize = 5;
        let Some(spots) = spots.filter(|s| s.continent == first.continent) else {
            return Vec::new();
        };
        spots
            .groups((first.x, first.y), MAX)
            .into_iter()
            .filter_map(|(x, y)| {
                self.map_point(
                    &Pos {
                        continent: spots.continent,
                        x,
                        y,
                        zone: 0,
                    },
                    zone,
                )
            })
            .collect()
    }

    /// A place on the map of the step's zone `zone` when it is on it (by the border), else on
    /// its own zone's.
    fn map_point(&self, pos: &Pos, zone: i64) -> Option<MapPointOut> {
        let inside = |(m, _): &(MapPoint, Pos)| (0.0..=100.0).contains(&m.x) && (0.0..=100.0).contains(&m.y);
        self.place(pos, Some(zone))
            .filter(inside)
            .or_else(|| self.place(pos, None))
            .map(|(m, _)| m.into())
    }

    /// A step at an NPC who patrols (Gordo walking Tirisfal's road): its other places are spread
    /// along the path, away from the step's own, and the step says it. Nothing when the path
    /// stays around the place.
    fn patrol(&self, step: &mut Step, loc: &Loc) {
        /// Points picked along the path, and how far apart the places shown are (yards).
        const SAMPLES: usize = 8;
        const APART: f64 = 100.0;
        let Some(path) = self
            .planner
            .model
            .patrols
            .get(&loc.id)
            .filter(|_| loc.kind == EntityKind::Npc)
        else {
            return;
        };
        let mut shown: Vec<Pos> = vec![loc.pos];
        for p in along(path, SAMPLES) {
            if p.continent == loc.pos.continent && shown.iter().all(|s| s.dist(&p) > APART) {
                shown.push(p);
            }
        }
        // A few steps around the place (Junior Apothecary Holland in Brill): not a patrol.
        let spots: Vec<MapPointOut> = shown[1..].iter().filter_map(|p| self.map_point(p, loc.zone)).collect();
        if spots.is_empty() {
            return;
        }
        step.spots = spots;
        step.patrol = true;
        step.say.push(Phrase::new("suffix.patrol"));
    }

    fn place(&self, p: &Pos, zone: Option<i64>) -> Place {
        let world = self.planner.world;
        let zone = zone.or_else(|| world.zone_at(p))?;
        world.to_map(zone, p).map(|m| (m, *p))
    }

    fn service(&self, step: &mut Step, kind: ServiceKind, index: usize) -> Place {
        let loc = &self.planner.model.services[kind.index()][index];
        let (step_kind, key) = match kind {
            ServiceKind::Vendor => (StepKind::Vendor, "service.vendor"),
            ServiceKind::Mailbox => (StepKind::Mailbox, "service.mailbox"),
        };
        step.kind = step_kind;
        step.target = Some(Self::target(loc));
        step.say.push(Phrase::new(key).arg("who", entity(loc)));
        self.place(&loc.pos, Some(loc.zone))
    }

    fn target(loc: &Loc) -> Target {
        Target {
            kind: loc.kind.into(),
            id: loc.id,
            name: loc.name.clone(),
        }
    }

    fn objective(step: &mut Step, o: &Objective, k: u8) {
        step.kind = StepKind::Objective;
        step.objective = Some(k);
        step.child = o.child;
        step.child_name.clone_from(&o.child_name);
        step.elite = o.elite;
        step.mob_level = (o.kills > 0.0 && o.mob_level > 0).then_some(o.mob_level);
        for (id, name) in o.mobs.iter().take(4) {
            if !step.mob_ids.contains(id) {
                step.mobs.push(name.clone());
                step.mob_ids.push(*id);
            }
        }
        step.say.push(objective_phrase(o));
        Self::vendors(step, o);
    }

    /// An item bought: the vendor at the place named, its supply when limited (it may be sold
    /// out), and a few others selling it, nearest first; all of them to target.
    fn vendors(step: &mut Step, o: &Objective) {
        let Some(first) = o.vendors.first() else { return };
        step.buy = true;
        let npc = |v: &crate::model::Vendor| Arg::Npc(v.id, v.name.clone());
        let n = num(o.count.round().max(1.0) as i64);
        // At an auction house: its auctioneers to target, the vendors as the other way.
        if !o.auction.is_empty() {
            if let Some(say) = step.say.last_mut() {
                *say = Phrase::new("objective.buy.auction")
                    .arg("who", entity(&o.loc))
                    .arg("n", n);
            }
            let vendors: Vec<Arg> = (o.vendors.iter().take(3))
                .map(|v| Arg::At(Box::new(npc(v)), Box::new(Arg::Zone(v.zone, v.zone_name.clone()))))
                .collect();
            step.say
                .push(Phrase::new("suffix.vendors").arg("vendors", Arg::List(vendors)));
            for (id, name) in o.auction.iter().take(4) {
                step.mobs.push(name.clone());
                step.mob_ids.push(*id);
            }
            return;
        }
        if let Some(say) = step.say.last_mut() {
            *say = Phrase::new("objective.buy.from")
                .arg("who", entity(&o.loc))
                .arg("vendor", npc(first))
                .arg("n", n);
        }
        if let Some((n, restock)) = first.stock {
            step.say.push(
                Phrase::new("suffix.stock")
                    .arg("n", Arg::Text(n.to_string()))
                    .arg("hours", num((restock / 3600.0).round().max(1.0) as i64)),
            );
        }
        let others: Vec<Arg> = (o.vendors.iter().skip(1).take(3))
            .map(|v| Arg::At(Box::new(npc(v)), Box::new(Arg::Zone(v.zone, v.zone_name.clone()))))
            .collect();
        if !others.is_empty() {
            step.say
                .push(Phrase::new("suffix.vendors").arg("vendors", Arg::List(others)));
        }
        for v in o.vendors.iter().take(4) {
            if !step.mob_ids.contains(&v.id) {
                step.mobs.push(v.name.clone());
                step.mob_ids.push(v.id);
            }
        }
    }

    /// A quest of the character's log the guide does not take: abandoned at its start (it would
    /// hold a place in the log).
    pub(super) fn abandon_unused(&self, first: &Timed, id: i64, name: &str) -> Step {
        let mut step = self.blank(first);
        step.kind = StepKind::Abandon;
        step.quest = Some(id);
        step.quest_name = Some(name.to_owned());
        step.time = 0.0;
        step.say
            .push(Phrase::new("abandon.unused").arg("quest", Arg::Quest(id, name.to_owned())));
        step
    }

    fn abandon(&self, step: &mut Step, quest: usize) -> Place {
        let q = &self.planner.model.quests[quest];
        step.kind = StepKind::Abandon;
        step.quest = Some(q.id);
        step.quest_name = Some(q.name.clone());
        step.say
            .push(Phrase::new("abandon").arg("quest", Arg::Quest(q.id, q.name.clone())));
        None
    }

    fn fly(&self, step: &mut Step, from: usize, to: usize) -> Place {
        let nodes = &self.planner.world.taxi_nodes;
        let (a, b) = (&nodes[from], &nodes[to]);
        step.kind = StepKind::Fly;
        step.say.push(
            Phrase::new("fly")
                .arg("from", Arg::Text(a.name.clone()))
                .arg("to", Arg::Text(b.name.clone())),
        );
        step.target = Some(Target {
            kind: TargetKind::Taxi,
            id: b.id,
            name: b.name.clone(),
        });
        self.place(&a.pos, None)
    }

    fn link(&self, step: &mut Step, first: usize, last: usize) -> Place {
        let links = &self.planner.world.links;
        let (a, b) = (&links[first], &links[last]);
        step.kind = StepKind::Link;
        let link = Arg::Text(a.name.clone());
        step.say.push(if first == last {
            Phrase::new("link").arg("link", link)
        } else {
            Phrase::new("link.then")
                .arg("link", link)
                .arg("last", Arg::Text(b.name.clone()))
        });
        step.target = Some(Target {
            kind: TargetKind::Link,
            id: last as i64,
            name: b.name.clone(),
        });
        self.place(&a.from, Some(a.from_zone))
    }

    /// An off-road shortcut: shown as a link (done on reaching the next step's map).
    fn shortcut(&self, step: &mut Step, pass: usize) -> Place {
        let pass = &self.planner.world.passes[pass];
        step.kind = StepKind::Link;
        step.say
            .push(Phrase::new("shortcut").arg("pass", Arg::Text(pass.name.clone())));
        self.place(&pass.pos, Some(pass.pos.zone))
    }

    fn learn_flight(&self, step: &mut Step, node: usize) -> Place {
        let n = &self.planner.world.taxi_nodes[node];
        step.kind = StepKind::FlightMaster;
        step.say
            .push(Phrase::new("flight_master").arg("place", Arg::Text(n.name.clone())));
        step.target = Some(Target {
            kind: TargetKind::Taxi,
            id: n.id,
            name: n.name.clone(),
        });
        self.place(&n.pos, None)
    }

    fn grind(&self, step: &mut Step, to_level: i64, seconds: f64, near: &Pos, zone: i64) -> Place {
        step.kind = StepKind::Grind;
        let minutes = (seconds / 60.0).ceil() as i64;
        step.say.push(
            Phrase::new("grind")
                .arg("level", num(to_level))
                .arg("minutes", num(minutes)),
        );
        self.place(near, Some(zone))
    }

    /// Mobs to kill where the character stands (a step added by hand): it then has the XP of
    /// the route there (`xp`, so that the step stays where it is).
    fn grind_mobs(&self, step: &mut Step, t: &Timed, mobs: u32, seconds: f64, near: &Pos, zone: i64) -> Place {
        step.kind = StepKind::Grind;
        step.xp = (t.xp > 0).then_some(t.xp);
        step.say.push(
            Phrase::new("grind.mobs")
                .arg("mobs", num(i64::from(mobs)))
                .arg("minutes", num((seconds / 60.0).ceil() as i64)),
        );
        self.place(near, Some(zone))
    }

    fn zone(&self, zone: i64) -> Arg {
        Arg::Zone(zone, self.planner.world.zone_name(zone).to_owned())
    }

    fn death_skip(&self, step: &mut Step, pos: &Pos, zone: i64) -> Place {
        step.kind = StepKind::DeathSkip;
        step.say.push(Phrase::new("death_skip").arg("zone", self.zone(zone)));
        self.place(pos, Some(zone))
    }

    /// The hearthstone, or Astral Recall (`recall`), to the inn it is bound to.
    fn hearth(&self, step: &mut Step, inn: usize, recall: bool) -> Place {
        let inn = &self.planner.model.inns[inn];
        let world = self.planner.world;
        let zone_name = world.zone_name(inn.zone);
        // The town, not only the zone (Ratchet and the Crossroads are both in the Barrens): its
        // flight master's, else the named place of the map around.
        let town = world
            .taxi_near(&inn.pos, 300.0)
            .and_then(|n| world.taxi_nodes[n].name.split(',').next())
            .or_else(|| world.place_name(&inn.pos));
        step.kind = StepKind::Hearth;
        let key = if recall { "recall" } else { "hearth" };
        step.say.push(match town {
            Some(town) if town != zone_name => Phrase::new(if recall { "recall.town" } else { "hearth.town" })
                .arg("town", Arg::Text(town.to_owned()))
                .arg("zone", self.zone(inn.zone)),
            _ => Phrase::new(key).arg("zone", self.zone(inn.zone)),
        });
        step.target = Some(Target {
            kind: TargetKind::Npc,
            id: inn.id,
            name: inn.name.clone(),
        });
        self.place(&inn.pos, Some(inn.zone))
    }

    /// A class teleport to a capital (a mage's "Teleport: Stormwind").
    fn teleport(&self, step: &mut Step, to: usize) -> Place {
        let tp = &self.planner.model.teleports[to];
        step.kind = StepKind::Teleport;
        step.say
            .push(Phrase::new("teleport").arg("zone", self.zone(tp.to.zone)));
        self.place(&tp.to.pos, Some(tp.to.zone))
    }

    /// Class training: the spells learned since the last visit.
    fn train(&self, step: &mut Step, trainer: usize, since: i64, level: i64) -> Place {
        let npc = &self.planner.model.trainers[trainer];
        step.kind = StepKind::Train;
        let who = Arg::Npc(npc.id, npc.name.clone());
        let ranks = self
            .planner
            .model
            .power
            .as_ref()
            .map(|p| p.new_ranks(since, level))
            .unwrap_or_default();
        step.spells = ranks.iter().map(|r| r.id).collect();
        let spells: Vec<Arg> = ranks
            .iter()
            .map(|r| {
                Arg::Text(if r.rank > 0 {
                    format!("{} {}", r.name, r.rank)
                } else {
                    r.name.clone()
                })
            })
            .collect();
        step.say.push(if spells.is_empty() {
            Phrase::new("train").arg("who", who)
        } else {
            Phrase::new("train.spells")
                .arg("who", who)
                .arg("spells", Arg::List(spells))
        });
        step.target = Some(Target {
            kind: TargetKind::Npc,
            id: npc.id,
            name: npc.name.clone(),
        });
        self.place(&npc.pos, Some(npc.zone))
    }

    /// A visit: a dungeon run, the hearthstone bound, a flight path.
    fn visit(&self, step: &mut Step, stop: Stop, loc: &Loc) -> Place {
        step.target = Some(Self::target(loc));
        match stop.kind {
            Kind::Dungeon => self.dungeon(step, stop.index as usize),
            Kind::Bind => {
                step.kind = StepKind::Bind;
                step.say.push(
                    Phrase::new("bind")
                        .arg("who", entity(loc))
                        .arg("zone", self.zone(loc.zone)),
                );
            }
            _ => {
                step.kind = StepKind::FlightMaster;
                step.say.push(Phrase::new("flight_master").arg("place", entity(loc)));
            }
        }
        self.place(&loc.pos, Some(loc.zone))
    }

    /// A dungeon run with a group (its quests follow as steps of their own, `dungeon_quests`).
    fn dungeon(&self, step: &mut Step, d: usize) {
        let dungeon = &self.planner.model.dungeons[d];
        step.kind = StepKind::Dungeon;
        step.say.push(
            Phrase::new("dungeon.run")
                .arg("dungeon", Arg::Zone(dungeon.area, dungeon.name.clone()))
                .arg("minutes", num(dungeon.minutes.round() as i64)),
        );
    }

    /// The quests of the route done inside dungeon `d`, a step each after the run step `run`
    /// (same place): the quest's objectives inside, done once the quest is complete when they
    /// are all its objectives, else a step per objective inside.
    pub(super) fn dungeon_quests(&self, d: usize, run: &Step) -> Vec<Step> {
        let model = &self.planner.model;
        let turned_in = |id: i64| {
            self.route
                .iter()
                .any(|s| s.kind == Kind::TurnIn && model.quests[s.index as usize].id == id)
        };
        let mut out = Vec::new();
        for q in model.quests.iter().filter(|q| turned_in(q.id)) {
            let inside: Vec<u8> = (0..q.objectives.len() as u8)
                .filter(|&k| q.objectives[k as usize].dungeon == Some(d))
                .collect();
            if inside.is_empty() {
                continue;
            }
            let whole = inside.len() == q.objectives.len();
            let groups: Vec<Vec<u8>> = if whole {
                vec![inside]
            } else {
                inside.into_iter().map(|k| vec![k]).collect()
            };
            for ks in groups {
                let mut step = run.clone();
                step.kind = StepKind::Objective;
                step.quest = Some(q.id);
                step.quest_name = Some(q.name.clone());
                step.target = None;
                step.say.clear();
                step.inside = true;
                // The mobs on the way are the run's, before it.
                (step.farm, step.farm_xp) = (None, None);
                for &k in &ks {
                    Self::objective(&mut step, &q.objectives[k as usize], k);
                }
                // Several objectives: done when the quest is.
                if ks.len() > 1 {
                    step.objective = None;
                }
                // "The Stockade: Defias Prisoner" (the database's text) as "Defias Prisoner x10".
                let prefix = format!("{}: ", model.dungeons[d].name);
                let texts = ks.iter().map(|&k| {
                    let o = &q.objectives[k as usize];
                    let text = o.text.strip_prefix(&prefix).unwrap_or(&o.text);
                    let n = o.count.round() as i64;
                    Arg::Text(if n > 1 { format!("{text} x{n}") } else { text.to_owned() })
                });
                step.say = vec![
                    Phrase::new("dungeon.quest")
                        .arg("quest", Arg::Quest(q.id, q.name.clone()))
                        .arg("objectives", Arg::List(texts.collect())),
                ];
                out.push(step);
            }
        }
        out
    }

    /// Accept, objective or turn-in of a quest.
    fn quest_stop(&self, step: &mut Step, stop: Stop, loc: &Loc) -> Place {
        let q = &self.planner.model.quests[stop.index as usize];
        step.quest = Some(q.id);
        step.quest_name = Some(q.name.clone());
        step.alt = q
            .exclusive
            .iter()
            .copied()
            .chain(q.breadcrumb_for)
            .filter(|id| *id != q.id)
            .collect();
        step.target = Some(Self::target(loc));
        let (quest, who) = (Arg::Quest(q.id, q.name.clone()), entity(loc));
        match stop.kind {
            Kind::Accept => {
                step.kind = StepKind::Accept;
                step.pre.clone_from(&q.pre_all);
                step.pre_any.clone_from(&q.pre_any);
                step.min_level = Some(q.min_level);
                // An item dropped by a mob: named, targeted by the addon's macro, and its other
                // places.
                let say = match &q.start_mob {
                    Some((id, name)) if loc.kind == EntityKind::Item => {
                        step.mobs.push(name.clone());
                        step.mob_ids.push(*id);
                        step.mob_level = (q.start_mob_level > 0).then_some(q.start_mob_level);
                        step.spots = self.spots(q.start_spots.as_ref(), &loc.pos, loc.zone);
                        Phrase::new("accept.item.mob").arg("mob", Arg::Npc(*id, name.clone()))
                    }
                    _ if q.held.is_some() => {
                        step.held = true;
                        let giver = q.ends.first().map_or_else(|| who.clone(), entity);
                        Phrase::new("accept.held").arg("giver", giver)
                    }
                    _ if loc.kind == EntityKind::Item => Phrase::new("accept.item"),
                    _ => Phrase::new("accept"),
                };
                step.say.push(say.arg("quest", quest).arg("who", who));
                self.patrol(step, loc);
                if let Some(limit) = q.time_limit {
                    step.say
                        .push(Phrase::new("suffix.timed").arg("minutes", num((limit / 60.0).round() as i64)));
                }
                // Profession quest: optional, the addon skips it without the skill.
                if let Some((p, value)) = q.skill {
                    let plan = &self.planner.model.professions[p];
                    step.line = Some(crate::profession::skill_line(&plan.key));
                    step.skill = Some(value);
                    step.say.push(
                        Phrase::new("suffix.optional")
                            .arg("profession", Arg::Profession(plan.key.clone(), plan.name.clone()))
                            .arg("skill", num(value)),
                    );
                }
            }
            Kind::Objective(k) => {
                // At the place the route went to (one of its alternatives).
                let o = q.objectives[k as usize].nearest(&loc.pos);
                Self::objective(step, o, k);
                if o.dungeon.is_none() {
                    step.spots = self.spots(o.spots.as_ref(), &loc.pos, loc.zone);
                    self.patrol(
                        step,
                        &Loc {
                            pos: loc.pos,
                            zone: loc.zone,
                            ..o.loc.clone()
                        },
                    );
                }
                step.bg = self.background.contains(&(stop.index, k));
            }
            Kind::TurnIn => {
                step.kind = StepKind::TurnIn;
                step.reward_xp = Some(self.planner.growth().quest_reward(q, step.level));
                let key = if q.held.is_some() { "turnin.held" } else { "turnin" };
                step.say.push(Phrase::new(key).arg("quest", quest).arg("who", who));
                self.patrol(step, loc);
            }
            Kind::LearnFlight | Kind::Bind | Kind::Dungeon | Kind::Hearth => unreachable!(),
        }
        self.place(&loc.pos, Some(loc.zone))
    }
}

/// `n` points spread evenly along a path (by its length), its two ends included.
fn along(path: &[Pos], n: usize) -> Vec<Pos> {
    let legs: Vec<f64> = path.windows(2).map(|w| w[0].dist(&w[1])).collect();
    let total: f64 = legs.iter().sum();
    if path.len() < 2 || total <= 0.0 || n < 2 {
        return path.first().copied().into_iter().collect();
    }
    let mut out = Vec::with_capacity(n);
    let (mut leg, mut walked) = (0, 0.0);
    for k in 0..n {
        let at = total * k as f64 / (n - 1) as f64;
        while leg + 1 < legs.len() && walked + legs[leg] < at {
            walked += legs[leg];
            leg += 1;
        }
        let t = if legs[leg] > 0.0 {
            ((at - walked) / legs[leg]).clamp(0.0, 1.0)
        } else {
            0.0
        };
        let (a, b) = (path[leg], path[leg + 1]);
        out.push(Pos {
            x: a.x + (b.x - a.x) * t,
            y: a.y + (b.y - a.y) * t,
            ..a
        });
    }
    out
}

#[cfg(test)]
mod tests {
    use super::along;
    use crate::export::{StepKind, build};
    use crate::plan::tests::quests_at_start;
    use crate::plan::{Kind, Planner, Stop};
    use crate::world::Pos;

    /// Gordo's case (GitHub #20): a quest given and taken back by an NPC who walks ~700 yards
    /// along a road. Its steps say he patrols and give places along his path, away from the
    /// step's own; the steps of NPCs who stand still have none.
    #[test]
    fn steps_at_a_patrolling_npc_say_it_and_show_places_along_the_path() {
        let mut p = quests_at_start(1);
        let here = p.model.start.clone();
        let route = [Stop::quest(0, Kind::Accept), Stop::quest(0, Kind::TurnIn)];
        let steps = |p: &crate::job::Prepared| {
            let planner = Planner::new(&p.model, &p.world, &p.params, &p.profile);
            build(&planner, &p.profile, &route).steps
        };
        let at_npc = |s: &crate::export::Step| matches!(s.kind, StepKind::Accept | StepKind::TurnIn);
        let still = |p: &crate::job::Prepared| {
            steps(p)
                .iter()
                .filter(|s| at_npc(s))
                .all(|s| !s.patrol && s.spots.is_empty())
        };
        assert!(still(&p), "standing still");
        // A few steps around the place: not a patrol.
        let around = |d: f64| Pos {
            x: here.pos.x + d,
            ..here.pos
        };
        p.model
            .patrols
            .insert(here.id, vec![around(0.0), around(40.0), around(-30.0)]);
        assert!(still(&p), "walking around its post");
        let path: Vec<Pos> = (0..=7)
            .map(|k| Pos {
                x: here.pos.x + 100.0 * f64::from(k),
                ..here.pos
            })
            .collect();
        p.model.patrols.insert(here.id, path);
        let steps = steps(&p);
        let at = steps.iter().filter(|s| at_npc(s)).collect::<Vec<_>>();
        assert_eq!(at.len(), 2, "{steps:?}");
        for s in at {
            assert!(s.patrol, "{s:?}");
            assert!(s.say.iter().any(|ph| ph.key == "suffix.patrol"), "{:?}", s.say);
            assert!(
                (3..=7).contains(&s.spots.len()),
                "places along 700 yards: {:?}",
                s.spots
            );
        }
    }

    /// Points spread by length: the ends and the middle of a straight path.
    #[test]
    fn points_along_a_path_are_spread_by_its_length() {
        let p = |x: f64| Pos {
            continent: 0,
            x,
            y: 0.0,
            zone: 0,
        };
        let points = along(&[p(0.0), p(100.0), p(400.0)], 3);
        let xs: Vec<f64> = points.iter().map(|q| q.x).collect();
        assert_eq!(xs, [0.0, 200.0, 400.0]);
    }
}
