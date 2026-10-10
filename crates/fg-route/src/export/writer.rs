//! Writes the step of the guide for each event of a replayed route: its kind, its sentence, its
//! target and where it takes place.

use super::language::Language;
use super::text::objective_text;
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
    pub(super) lang: Language<'a>,
    /// Objectives done along the way while doing other quests.
    pub(super) background: HashSet<(u32, u8)>,
    /// Objective stops already announced on the way (indexes in the trace).
    pub(super) announced: HashSet<usize>,
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
    pub(super) fn add_farm(&self, step: &mut Step, kills: u32) {
        let mobs = if kills > 1 { "mobs" } else { "mob" };
        step.text = self.lang.sentence(
            format!("{} (kill ~{kills} {mobs} on the way)", step.text),
            format!("{} (tuer ~{kills} {mobs} en chemin)", step.text),
        );
    }

    /// A checkpoint: the guide goes on from the level (and XP) the route has there, grinding
    /// first when behind (as long as needed) so that the next part of the route fits the
    /// character. `at`: the step where it takes place.
    pub(super) fn checkpoint(&self, at: &Step, checkpoint: i64, level: i64, xp: i64) -> Step {
        let text = if xp > 0 {
            let (p, need) = self.xp_target(level, xp);
            self.lang.sentence(
                format!("Level {checkpoint} checkpoint: be level {level} and {p}% ({need}) before going on (grind here if needed)"),
                format!("Palier niveau {checkpoint} : être niveau {level} et {p} % ({need}) avant de continuer (farmer ici si besoin)"),
            )
        } else {
            self.lang.sentence(
                format!("Checkpoint: be level {level} before going on (grind if needed)"),
                format!("Palier : être niveau {level} avant de continuer (farmer si besoin)"),
            )
        };
        Step {
            kind: StepKind::Grind,
            quest: None,
            quest_name: None,
            alt: Vec::new(),
            bg: false,
            objective: None,
            text,
            target: None,
            skill: None,
            line: None,
            mobs: Vec::new(),
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
        let text = if xp > 0 {
            let (p, need) = self.xp_target(level, xp);
            self.lang.sentence(
                format!("Grind to level {level} and {p}% ({need}) before going on (~{minutes} min)"),
                format!("Farmer jusqu'au niveau {level} et {p} % ({need}) avant de continuer (~{minutes} min)"),
            )
        } else {
            self.lang.sentence(
                format!("Grind to level {level} (~{minutes} min)"),
                format!("Farmer jusqu'au niveau {level} (~{minutes} min)"),
            )
        };
        Step {
            text,
            xp: (xp > 0).then_some(xp),
            level,
            zone: at.zone.clone(),
            map: at.map.clone(),
            world: at.world,
            ..grind.clone()
        }
    }

    /// Percentage of the level and XP count of a target, as written in the guide.
    fn xp_target(&self, level: i64, xp: i64) -> (i64, String) {
        let next = self.planner.rules().to_next_level(level).max(1);
        ((xp * 100 / next), format!("{xp} xp"))
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
        let name = self.lang.name("item", item, english);
        step.text = self.lang.sentence(
            format!("{} (take {name})", step.text),
            format!("{} (prendre {name})", step.text),
        );
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
            target: None,
            zone: None,
            map: None,
            world: None,
            skill: None,
            line: None,
            mobs: Vec::new(),
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

    fn target(&self, loc: &Loc) -> (Target, String) {
        let who = self.lang.name(loc.kind.as_str(), loc.id, &loc.name);
        (
            Target {
                kind: loc.kind.into(),
                id: loc.id,
                name: who.clone(),
            },
            who,
        )
    }

    fn objective(&self, step: &mut Step, o: &Objective, k: u8, who: &str) {
        step.kind = StepKind::Objective;
        step.objective = Some(k);
        step.elite = o.elite;
        step.mob_level = (o.kills > 0.0 && o.mob_level > 0).then_some(o.mob_level);
        for (id, name) in o.mobs.iter().take(4) {
            let name = self.lang.name("npc", *id, name);
            if !step.mobs.contains(&name) {
                step.mobs.push(name);
            }
        }
        step.text = if self.lang.english() || o.dungeon.is_some() || o.loc.kind == EntityKind::Area {
            o.text.clone()
        } else {
            objective_text(o, who, self.lang.french())
        };
    }

    /// Mobs met on the way to the next stop: tracked without stopping the guide.
    fn along(&self, step: &mut Step, quest: usize, objective: u8) -> Place {
        let q = &self.planner.model.quests[quest];
        let o = &q.objectives[objective as usize];
        step.quest = Some(q.id);
        step.quest_name = Some(self.lang.name("quest", q.id, &q.name));
        let (target, who) = self.target(&o.loc);
        step.target = Some(target);
        self.objective(step, o, objective, &who);
        step.bg = true;
        self.place(&o.loc.pos, Some(o.loc.zone))
    }

    fn abandon(&self, step: &mut Step, quest: usize) -> Place {
        let q = &self.planner.model.quests[quest];
        let name = self.lang.name("quest", q.id, &q.name);
        step.kind = StepKind::Abandon;
        step.quest = Some(q.id);
        step.quest_name = Some(name.clone());
        step.text = self.lang.sentence(
            format!("Abandon {name} (can no longer be finished)"),
            format!("Abandonner {name} (ne peut plus être finie)"),
        );
        None
    }

    fn fly(&self, step: &mut Step, from: usize, to: usize) -> Place {
        let nodes = &self.planner.world.taxi_nodes;
        let (a, b) = (&nodes[from], &nodes[to]);
        step.kind = StepKind::Fly;
        step.text = self.lang.sentence(
            format!("Fly from {} to {}", a.name, b.name),
            format!("Vol de {} à {}", a.name, b.name),
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
        step.text = if first == last {
            self.lang
                .sentence(format!("Take {}", a.name), format!("Prendre : {}", a.name))
        } else {
            self.lang.sentence(
                format!("Take {} (then {})", a.name, b.name),
                format!("Prendre : {} (puis {})", a.name, b.name),
            )
        };
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
        step.text = self.lang.sentence(
            format!("Get the flight path at {}", n.name),
            format!("Point de vol : {}", n.name),
        );
        step.target = Some(Target {
            kind: TargetKind::Taxi,
            id: n.id,
            name: n.name.clone(),
        });
        self.place(&n.pos, None)
    }

    fn grind(&self, step: &mut Step, to_level: i64, seconds: f64, near: &Pos, zone: i64) -> Place {
        step.kind = StepKind::Grind;
        let minutes = (seconds / 60.0).ceil();
        step.text = self.lang.sentence(
            format!("Grind to level {to_level} (~{minutes} min)"),
            format!("Farmer jusqu'au niveau {to_level} (~{minutes} min)"),
        );
        self.place(near, Some(zone))
    }

    fn death_skip(&self, step: &mut Step, pos: &Pos, zone: i64) -> Place {
        let zone_name = self.planner.world.zone_name(zone);
        step.kind = StepKind::DeathSkip;
        step.text = self.lang.sentence(
            format!("Die and respawn at the spirit healer ({zone_name})"),
            format!("Mourir et revenir à l'ange de la mort ({zone_name})"),
        );
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
        let place = match town {
            Some(town) if town != zone_name => format!("{town} ({zone_name})"),
            _ => zone_name.to_owned(),
        };
        step.kind = StepKind::Hearth;
        step.text = self.lang.sentence(
            format!("Hearthstone to {place}"),
            format!("Pierre de foyer vers {place}"),
        );
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
        let who = self.lang.name("npc", npc.id, &npc.name);
        let ranks = self
            .planner
            .model
            .power
            .as_ref()
            .map(|p| p.new_ranks(since, level))
            .unwrap_or_default();
        step.spells = ranks.iter().map(|r| r.id).collect();
        let list = ranks
            .iter()
            .map(|r| {
                if r.rank > 0 {
                    format!("{} {}", r.name, r.rank)
                } else {
                    r.name.clone()
                }
            })
            .collect::<Vec<_>>()
            .join(", ");
        step.text = if list.is_empty() {
            self.lang.sentence(
                format!("Train new spells with {who}"),
                format!("Apprendre les sorts auprès de {who}"),
            )
        } else {
            self.lang.sentence(
                format!("Train with {who}: {list}"),
                format!("Apprendre auprès de {who} : {list}"),
            )
        };
        step.target = Some(Target {
            kind: TargetKind::Npc,
            id: npc.id,
            name: npc.name.clone(),
        });
        self.place(&npc.pos, Some(npc.zone))
    }

    /// A visit: a dungeon run, the hearthstone bound, a flight path.
    fn visit(&self, step: &mut Step, stop: Stop, loc: &Loc) -> Place {
        let (target, who) = self.target(loc);
        step.target = Some(target);
        match stop.kind {
            Kind::Dungeon => self.dungeon(step, stop.index as usize),
            Kind::Bind => {
                let zone_name = self.planner.world.zone_name(loc.zone);
                step.kind = StepKind::Bind;
                step.text = self.lang.sentence(
                    format!("Set your hearthstone at {who} ({zone_name})"),
                    format!("Lier la pierre de foyer auprès de {who} ({zone_name})"),
                );
            }
            _ => {
                step.kind = StepKind::FlightMaster;
                step.text = self
                    .lang
                    .sentence(format!("Get the flight path at {who}"), format!("Point de vol : {who}"));
            }
        }
        self.place(&loc.pos, Some(loc.zone))
    }

    /// A dungeon run with a group, and the quests of the route done inside.
    fn dungeon(&self, step: &mut Step, d: usize) {
        let model = &self.planner.model;
        let dungeon = &model.dungeons[d];
        step.kind = StepKind::Dungeon;
        let quests: Vec<String> = model
            .quests
            .iter()
            .filter(|q| q.objectives.iter().any(|o| o.dungeon == Some(d)))
            .filter(|q| {
                self.route
                    .iter()
                    .any(|s| s.kind == Kind::TurnIn && model.quests[s.index as usize].id == q.id)
            })
            .map(|q| self.lang.name("quest", q.id, &q.name))
            .collect();
        let (name, minutes, quests) = (&dungeon.name, dungeon.minutes.round(), quests.join(", "));
        step.text = self.lang.sentence(
            format!("Dungeon: {name} with a group (~{minutes} min) - quests: {quests}"),
            format!("Donjon : {name} en groupe (~{minutes} min) - quêtes : {quests}"),
        );
    }

    /// Accept, objective or turn-in of a quest.
    fn quest_stop(&self, step: &mut Step, n: usize, stop: Stop, loc: &Loc) -> Place {
        let q = &self.planner.model.quests[stop.index as usize];
        step.quest = Some(q.id);
        let quest = self.lang.name("quest", q.id, &q.name);
        step.quest_name = Some(quest.clone());
        step.alt = q
            .exclusive
            .iter()
            .copied()
            .chain(q.breadcrumb_for)
            .filter(|id| *id != q.id)
            .collect();
        let (target, who) = self.target(loc);
        step.target = Some(target);
        match stop.kind {
            Kind::Accept => {
                step.kind = StepKind::Accept;
                step.text = if loc.kind == EntityKind::Item {
                    self.lang.sentence(
                        format!("Loot {who} to start {quest}"),
                        format!("Looter {who} pour démarrer {quest}"),
                    )
                } else {
                    self.lang.sentence(
                        format!("Accept {quest} from {who}"),
                        format!("Prendre {quest} auprès de {who}"),
                    )
                };
                // Profession quest: optional, the addon skips it without the skill.
                if let Some((p, value)) = q.skill {
                    let plan = &self.planner.model.professions[p];
                    step.line = Some(crate::profession::skill_line(&plan.key));
                    step.skill = Some(value);
                    let name = self.lang.profession(&plan.key, &plan.name);
                    step.text = self.lang.sentence(
                        format!("{} (optional: {name} {value})", step.text),
                        format!("{} (facultatif : {name} {value})", step.text),
                    );
                }
            }
            Kind::Objective(k) => {
                self.objective(step, &q.objectives[k as usize], k, &who);
                // Already announced on the way: here is where it is finished.
                step.bg = self.background.contains(&(stop.index, k)) && !self.announced.contains(&n);
            }
            Kind::TurnIn => {
                step.kind = StepKind::TurnIn;
                step.text = self
                    .lang
                    .sentence(format!("Turn in {quest} to {who}"), format!("Rendre {quest} à {who}"));
            }
            Kind::LearnFlight | Kind::Bind | Kind::Dungeon => unreachable!(),
        }
        self.place(&loc.pos, Some(loc.zone))
    }
}
