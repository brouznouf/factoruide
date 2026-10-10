//! Writes the step of the guide for each event of a replayed route: its kind, its sentences
//! (language-neutral phrases with the English names, see `Phrase`), its target and where it
//! takes place.

use super::phrase::{Arg, Phrase};
use super::types::{Step, StepKind, Target, TargetKind};
use crate::model::{EntityKind, Loc, Objective};
use crate::plan::{Event, Kind, Planner, Stop, Timed};
use crate::world::{MapPoint, Pos};
use std::collections::HashSet;

/// Where a step takes place: on its zone's map, and in the world.
type Place = Option<(MapPoint, Pos)>;

pub(super) struct StepWriter<'a> {
    pub(super) planner: &'a Planner<'a>,
    pub(super) route: &'a [Stop],
    /// Objectives done along the way while doing other quests.
    pub(super) background: HashSet<(u32, u8)>,
    /// Objective stops already announced on the way (indexes in the trace).
    pub(super) announced: HashSet<usize>,
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

impl StepWriter<'_> {
    /// The step of event `n` of the trace.
    pub(super) fn step(&self, n: usize, t: &Timed) -> Step {
        let mut step = self.blank(t);
        let place = match &t.event {
            Event::Reward { .. } | Event::Farm { .. } => None,
            Event::Along { quest, objective, .. } => self.along(&mut step, *quest, *objective),
            Event::Abandon { quest } => self.abandon(&mut step, *quest),
            Event::Fly { from, to } => self.fly(&mut step, *from, *to),
            Event::Link { first, last } => self.link(&mut step, *first, *last),
            Event::LearnFlight { node } => self.learn_flight(&mut step, *node),
            Event::Grind {
                to_level,
                seconds,
                near,
                zone,
            } => self.grind(&mut step, *to_level, *seconds, near, *zone),
            Event::DeathSkip { pos, zone } => self.death_skip(&mut step, pos, *zone),
            Event::Hearth { inn } => self.hearth(&mut step, *inn),
            Event::Train { trainer, since } => self.train(&mut step, *trainer, *since, t.level),
            Event::Stop { stop, loc } if !stop.is_quest() => self.visit(&mut step, *stop, loc),
            Event::Stop { stop, loc } => self.quest_stop(&mut step, n, *stop, loc),
        };
        if let Some((m, p)) = place {
            step.zone = Some(self.planner.world.zone_name(m.zone).to_owned());
            step.zone_id = Some(m.zone);
            step.map = Some(m.into());
            step.world = Some([
                p.continent as f64,
                (p.x * 10.0).round() / 10.0,
                (p.y * 10.0).round() / 10.0,
            ]);
        }
        step
    }

    /// The mobs farmed on the way to a step, at the end of its text.
    pub(super) fn add_farm(step: &mut Step, kills: u32) {
        let key = if kills > 1 { "suffix.farm" } else { "suffix.farm1" };
        step.say.push(Phrase::new(key).arg("n", num(i64::from(kills))));
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
            bg: false,
            objective: None,
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
            spells: Vec::new(),
            xp: (xp > 0).then_some(xp),
            level,
            keep: true,
            ..at.clone()
        }
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
            level,
            zone: at.zone.clone(),
            zone_id: at.zone_id,
            map: at.map.clone(),
            world: at.world,
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
            bg: false,
            objective: None,
            text: String::new(),
            say: Vec::new(),
            target: None,
            zone: None,
            zone_id: None,
            map: None,
            world: None,
            skill: None,
            line: None,
            mobs: Vec::new(),
            mob_ids: Vec::new(),
            elite: false,
            mob_level: None,
            reward: None,
            spells: Vec::new(),
            xp: None,
            level: t.level,
            time: t.time.round(),
            power: power.map(|_| (t.power * 10.0).round() / 10.0),
            gear: power.map(|_| (t.gear * 10.0).round() / 10.0),
            keep: false,
        }
    }

    fn place(&self, p: &Pos, zone: Option<i64>) -> Place {
        let world = self.planner.world;
        let zone = zone.or_else(|| world.zone_at(p))?;
        world.to_map(zone, p).map(|m| (m, *p))
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
        step.elite = o.elite;
        step.mob_level = (o.kills > 0.0 && o.mob_level > 0).then_some(o.mob_level);
        for (id, name) in o.mobs.iter().take(4) {
            if !step.mob_ids.contains(id) {
                step.mobs.push(name.clone());
                step.mob_ids.push(*id);
            }
        }
        let n = o.count.round().max(1.0) as i64;
        // The database's text (English) says it best in English, and for dungeons and areas.
        let text = Arg::Text(o.text.clone());
        let say = if o.dungeon.is_some() || o.loc.kind == EntityKind::Area {
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
        };
        step.say.push(say);
    }

    /// Mobs met on the way to the next stop: tracked without stopping the guide.
    fn along(&self, step: &mut Step, quest: usize, objective: u8) -> Place {
        let q = &self.planner.model.quests[quest];
        let o = &q.objectives[objective as usize];
        step.quest = Some(q.id);
        step.quest_name = Some(q.name.clone());
        step.target = Some(Self::target(&o.loc));
        Self::objective(step, o, objective);
        step.bg = true;
        self.place(&o.loc.pos, Some(o.loc.zone))
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

    fn zone(&self, zone: i64) -> Arg {
        Arg::Zone(zone, self.planner.world.zone_name(zone).to_owned())
    }

    fn death_skip(&self, step: &mut Step, pos: &Pos, zone: i64) -> Place {
        step.kind = StepKind::DeathSkip;
        step.say.push(Phrase::new("death_skip").arg("zone", self.zone(zone)));
        self.place(pos, Some(zone))
    }

    fn hearth(&self, step: &mut Step, inn: usize) -> Place {
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
        step.say.push(match town {
            Some(town) if town != zone_name => Phrase::new("hearth.town")
                .arg("town", Arg::Text(town.to_owned()))
                .arg("zone", self.zone(inn.zone)),
            _ => Phrase::new("hearth").arg("zone", self.zone(inn.zone)),
        });
        step.target = Some(Target {
            kind: TargetKind::Npc,
            id: inn.id,
            name: inn.name.clone(),
        });
        self.place(&inn.pos, Some(inn.zone))
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

    /// A dungeon run with a group, and the quests of the route done inside.
    fn dungeon(&self, step: &mut Step, d: usize) {
        let model = &self.planner.model;
        let dungeon = &model.dungeons[d];
        step.kind = StepKind::Dungeon;
        let quests: Vec<Arg> = model
            .quests
            .iter()
            .filter(|q| q.objectives.iter().any(|o| o.dungeon == Some(d)))
            .filter(|q| {
                self.route
                    .iter()
                    .any(|s| s.kind == Kind::TurnIn && model.quests[s.index as usize].id == q.id)
            })
            .map(|q| Arg::Quest(q.id, q.name.clone()))
            .collect();
        step.say.push(
            Phrase::new("dungeon")
                .arg("dungeon", Arg::Zone(dungeon.area, dungeon.name.clone()))
                .arg("minutes", num(dungeon.minutes.round() as i64))
                .arg("quests", Arg::List(quests)),
        );
    }

    /// Accept, objective or turn-in of a quest.
    fn quest_stop(&self, step: &mut Step, n: usize, stop: Stop, loc: &Loc) -> Place {
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
                let key = if loc.kind == EntityKind::Item {
                    "accept.item"
                } else {
                    "accept"
                };
                step.say.push(Phrase::new(key).arg("quest", quest).arg("who", who));
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
                Self::objective(step, &q.objectives[k as usize], k);
                // Already announced on the way: here is where it is finished.
                step.bg = self.background.contains(&(stop.index, k)) && !self.announced.contains(&n);
            }
            Kind::TurnIn => {
                step.kind = StepKind::TurnIn;
                step.say.push(Phrase::new("turnin").arg("quest", quest).arg("who", who));
            }
            Kind::LearnFlight | Kind::Bind | Kind::Dungeon => unreachable!(),
        }
        self.place(&loc.pos, Some(loc.zone))
    }
}
