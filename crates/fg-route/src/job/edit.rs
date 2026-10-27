//! Editing a computed guide by hand: its route as steps by game IDs (kept with the guide), moved,
//! removed or added, and replayed after each change to tell whether the guide still holds and how
//! long it takes.

use super::{PlanOutcome, PlanRequest, Prepared, prepare};
use crate::export::{self, MapPointOut, Route, StepKind, TargetKind};
use crate::model::Names;
use crate::plan::{Breakdown, Edit, Edited, Event, Farmed, Kind, Outcome, Planner, Stop, fmt_time};
use crate::terrain::Walks;
use anyhow::{Result, bail};
use rusqlite::Connection;
use serde::{Deserialize, Serialize};
use std::collections::HashSet;
use std::sync::Arc;

/// A step of a guide's route by game IDs: what the guide editor moves, removes and adds.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum RouteStep {
    Accept {
        quest: i64,
    },
    /// `farm`: kill every normal mob met on the way to it (beyond the farming cap).
    Objective {
        quest: i64,
        objective: u8,
        #[serde(default, skip_serializing_if = "std::ops::Not::not")]
        farm: bool,
        /// Where it is done among its places (0 its own, then the others), as computed; none:
        /// the best from where the character is (guides saved before).
        #[serde(default, skip_serializing_if = "Option::is_none")]
        spot: Option<u8>,
    },
    TurnIn {
        quest: i64,
    },
    /// Learn a flight path (taxi node ID).
    FlightMaster {
        node: i64,
    },
    /// Bind the hearthstone at an inn (its innkeeper's ID).
    Bind {
        inn: i64,
    },
    /// Back to the inn: the hearthstone, or the shaman's Astral Recall.
    Hearth {
        #[serde(default, skip_serializing_if = "std::ops::Not::not")]
        recall: bool,
    },
    /// Fly between two known flight paths (taxi node IDs).
    Fly {
        from: i64,
        to: i64,
    },
    /// Run a dungeon (its area ID).
    Dungeon {
        dungeon: i64,
    },
    /// Boats, zeppelins or trams: board `first`, leave `last` (the same when direct).
    Link {
        first: LinkRef,
        last: LinkRef,
    },
    /// Cast a class teleport (its spell name).
    Teleport {
        spell: String,
    },
    /// Die on purpose, back at the nearest spirit healer.
    #[serde(rename = "deathskip")]
    DeathSkip,
    /// Learn the class spells at a trainer (its NPC ID).
    Train {
        trainer: i64,
    },
    /// Sell the bags' loot at a vendor (its NPC ID).
    Sell {
        vendor: i64,
    },
    /// Give up a quest of the log.
    Abandon {
        quest: i64,
    },
    /// Kill mobs of the character's level where it stands.
    Grind {
        mobs: u32,
    },
    /// Kill mobs of the character's level up to a level, at the camps of its levels.
    GrindTo {
        level: i64,
    },
}

/// A transport link one way: its name and the zone it leads to.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct LinkRef {
    pub name: String,
    pub to: i64,
}

/// The steps of a route found by the optimizer: each thing its simulation did, a step.
pub(super) fn route_steps(planner: &Planner<'_>, route: &[Stop]) -> Vec<RouteStep> {
    let edits: Vec<Edit> = (route.iter())
        .map(|&stop| Edit::Stop {
            stop,
            farm: false,
            spot: None,
        })
        .collect();
    planner
        .expand(&edits)
        .into_iter()
        .map(|edit| RouteStep::of(edit, planner))
        .collect()
}

impl RouteStep {
    /// The step of a planner's edit, by game IDs.
    fn of(edit: Edit, planner: &Planner<'_>) -> Self {
        let (model, world) = (planner.model, planner.world);
        let link = |i: usize| LinkRef {
            name: world.links[i].name.clone(),
            to: world.links[i].to_zone,
        };
        match edit {
            Edit::Stop { stop, farm, spot } => {
                let i = stop.index as usize;
                match stop.kind {
                    Kind::Accept => Self::Accept {
                        quest: model.quests[i].id,
                    },
                    Kind::Objective(objective) => Self::Objective {
                        quest: model.quests[i].id,
                        objective,
                        farm,
                        spot,
                    },
                    Kind::TurnIn => Self::TurnIn {
                        quest: model.quests[i].id,
                    },
                    Kind::LearnFlight => Self::FlightMaster {
                        node: world.taxi_nodes[i].id,
                    },
                    Kind::Bind => Self::Bind { inn: model.inns[i].id },
                    Kind::Hearth => Self::Hearth { recall: i == 1 },
                    Kind::Dungeon => Self::Dungeon {
                        dungeon: model.dungeons[i].area,
                    },
                }
            }
            Edit::Fly { from, to } => Self::Fly {
                from: world.taxi_nodes[from].id,
                to: world.taxi_nodes[to].id,
            },
            Edit::Link { first, last } => Self::Link {
                first: link(first),
                last: link(last),
            },
            Edit::Teleport { to } => Self::Teleport {
                spell: model.teleports[to].name.clone(),
            },
            Edit::DeathSkip => Self::DeathSkip,
            Edit::Train { trainer } => Self::Train {
                trainer: model.trainers[trainer].id,
            },
            Edit::Sell { vendor } => Self::Sell {
                vendor: model.vendors[vendor].id,
            },
            Edit::Abandon { quest } => Self::Abandon {
                quest: model.quests[quest as usize].id,
            },
            Edit::Grind { mobs } => Self::Grind { mobs },
            Edit::GrindTo { level } => Self::GrindTo { level },
        }
    }

    /// The step for the planner, or why it cannot be one (not in this character's data).
    pub(super) fn resolve(&self, planner: &Planner<'_>) -> Result<Edit, &'static str> {
        let (model, world) = (planner.model, planner.world);
        let quest = |id: i64| model.index.get(&id).map(|&i| i as u32).ok_or("quest not available");
        let node = |id: i64| {
            world
                .taxi_nodes
                .iter()
                .position(|n| n.id == id)
                .ok_or("unknown flight path")
        };
        let stop = |index: u32, kind: Kind| Edit::Stop {
            stop: Stop { index, kind },
            farm: false,
            spot: None,
        };
        let link = |l: &LinkRef| {
            world
                .links
                .iter()
                .position(|x| x.name == l.name && x.to_zone == l.to)
                .ok_or("unknown crossing")
        };
        Ok(match *self {
            Self::Accept { quest: id } => stop(quest(id)?, Kind::Accept),
            Self::Objective {
                quest: id,
                objective,
                farm,
                spot,
            } => {
                let index = quest(id)?;
                if usize::from(objective) >= model.quests[index as usize].objectives.len() {
                    return Err("unknown objective");
                }
                Edit::Stop {
                    stop: Stop {
                        index,
                        kind: Kind::Objective(objective),
                    },
                    farm,
                    spot,
                }
            }
            Self::TurnIn { quest: id } => stop(quest(id)?, Kind::TurnIn),
            Self::FlightMaster { node: id } => stop(node(id)? as u32, Kind::LearnFlight),
            Self::Bind { inn } => {
                let i = model
                    .inns
                    .iter()
                    .enumerate()
                    .position(|(i, l)| l.id == inn && i != model.start_inn)
                    .ok_or("unknown inn")?;
                stop(i as u32, Kind::Bind)
            }
            Self::Hearth { recall } => stop(u32::from(recall), Kind::Hearth),
            Self::Fly { from, to } => Edit::Fly {
                from: node(from)?,
                to: node(to)?,
            },
            Self::Dungeon { dungeon } => {
                let i = model
                    .dungeons
                    .iter()
                    .position(|d| d.area == dungeon)
                    .ok_or("unknown dungeon")?;
                stop(i as u32, Kind::Dungeon)
            }
            Self::Link { ref first, ref last } => Edit::Link {
                first: link(first)?,
                last: link(last)?,
            },
            Self::DeathSkip => Edit::DeathSkip,
            Self::Teleport { ref spell } => Edit::Teleport {
                to: model
                    .teleports
                    .iter()
                    .position(|t| t.name == *spell)
                    .ok_or("teleport not learned")?,
            },
            Self::Train { trainer } => Edit::Train {
                trainer: model
                    .trainers
                    .iter()
                    .position(|t| t.id == trainer)
                    .ok_or("unknown trainer")?,
            },
            Self::Sell { vendor } => Edit::Sell {
                vendor: model
                    .vendors
                    .iter()
                    .position(|v| v.id == vendor)
                    .ok_or("unknown vendor")?,
            },
            Self::Abandon { quest: id } => Edit::Abandon { quest: quest(id)? },
            Self::Grind { mobs } => Edit::Grind { mobs },
            Self::GrindTo { level } => Edit::GrindTo { level },
        })
    }
}

/// A computed guide being edited: the character, its settings and the world, loaded once.
pub struct Editor {
    pub(super) prepared: Prepared,
}

/// A guide edited by hand as replayed.
#[derive(Debug, Clone, Serialize)]
pub struct EditReport {
    /// Real play time, final grind included.
    pub total_time: f64,
    pub total_time_text: String,
    pub breakdown: Breakdown,
    pub steps: Vec<StepReport>,
    /// Steps that are not possible: the guide cannot be saved.
    pub errors: usize,
    /// Grinding added at the end to reach the target level (seconds): the steps fall short.
    pub final_grind: f64,
    /// Quests taken but never turned in.
    pub unfinished: Vec<QuestRef>,
}

#[derive(Debug, Clone, Serialize)]
pub struct QuestRef {
    pub id: i64,
    pub name: String,
}

/// What a step of an edited guide did.
#[derive(Debug, Clone, Serialize)]
pub struct StepReport {
    /// done, skipped (nothing to do there) or error (not possible).
    pub status: &'static str,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub reason: Option<&'static str>,
    /// Time before and after it, level after it, and the XP it gave.
    pub start: f64,
    pub end: f64,
    pub level: i64,
    pub xp: i64,
    /// The line of the guide it writes (none when skipped or not possible).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub line: Option<Line>,
    /// Mobs farmed on the way to it, and for an objective without `farm`, what killing every mob
    /// on the way would give.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub farm: Option<Farmed>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub farm_all: Option<Farmed>,
    /// Done, but not as the planner would (a walk through a zone too strong for the character).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub warning: Option<&'static str>,
}

/// A line of the guide.
#[derive(Debug, Clone, Serialize)]
pub struct Line {
    pub kind: StepKind,
    pub text: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub zone: Option<String>,
    #[serde(skip_serializing_if = "std::ops::Not::not")]
    pub bg: bool,
    /// Where it takes place, on its zone's map.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub map: Option<MapPointOut>,
}

/// What can be added to a guide: the character's quests, inns, class trainers, flight paths and
/// dungeons.
#[derive(Debug, Clone, Serialize)]
pub struct EditCatalog {
    pub quests: Vec<CatalogQuest>,
    pub inns: Vec<CatalogPlace>,
    pub trainers: Vec<CatalogPlace>,
    pub nodes: Vec<CatalogPlace>,
    pub dungeons: Vec<CatalogDungeon>,
    /// Boats, zeppelins, trams and portals, each one way.
    pub links: Vec<CatalogLink>,
    /// The character learns Astral Recall (a second hearthstone).
    pub recall: bool,
}

/// A transport link one way: its step, and the zones it goes from and to.
#[derive(Debug, Clone, Serialize)]
pub struct CatalogLink {
    pub link: LinkRef,
    pub from: String,
    pub to: String,
}

#[derive(Debug, Clone, Serialize)]
pub struct CatalogQuest {
    pub id: i64,
    pub name: String,
    pub level: i64,
    pub min_level: i64,
    /// Zone of its giver.
    pub zone: String,
    pub objectives: Vec<String>,
}

#[derive(Debug, Clone, Serialize)]
pub struct CatalogPlace {
    pub id: i64,
    pub name: String,
    pub zone: String,
}

#[derive(Debug, Clone, Serialize)]
pub struct CatalogDungeon {
    pub id: i64,
    pub name: String,
    pub min_level: i64,
    pub max_level: i64,
}

impl Editor {
    /// Load what the guide of `request` was computed from (with `walks`, the walking memory kept
    /// between jobs).
    pub fn open(
        conn: &Connection,
        overrides: &super::Overrides,
        request: &PlanRequest,
        walks: Option<&Arc<Walks>>,
    ) -> Result<Self> {
        let mut prepared = prepare(conn, overrides, request, &|_| {})?;
        if let Some(walks) = walks {
            prepared.world.terrain.share(Arc::clone(walks));
        }
        Ok(Self { prepared })
    }

    /// The planner of the edited guides: it does only what the steps say (`Planner::explicit`).
    pub(super) fn planner(&self) -> Planner<'_> {
        let p = &self.prepared;
        Planner::new(&p.model, &p.world, &p.params, &p.profile).explicit()
    }

    /// The route of a computed guide as steps to edit, and whether it was rebuilt from the
    /// guide's steps (a guide computed before the editor: its time may differ a little).
    pub fn steps(&self, outcome: &PlanOutcome) -> (Vec<RouteStep>, bool) {
        if outcome.explicit {
            return (outcome.plan.clone(), false);
        }
        let (steps, rebuilt) = if outcome.plan.is_empty() {
            (self.read_guide(&outcome.route), true)
        } else {
            (outcome.plan.clone(), false)
        };
        (self.expand(&steps), rebuilt)
    }

    /// Steps of a guide saved before each thing done was a step (the route's stops, and its
    /// flights): replayed as the optimizer does, each thing its simulation did becomes a step.
    fn expand(&self, steps: &[RouteStep]) -> Vec<RouteStep> {
        let p = &self.prepared;
        let planner = Planner::new(&p.model, &p.world, &p.params, &p.profile);
        let edits: Vec<Edit> = steps.iter().filter_map(|s| s.resolve(&planner).ok()).collect();
        planner
            .expand(&edits)
            .into_iter()
            .map(|edit| RouteStep::of(edit, &planner))
            .collect()
    }

    /// The route's steps read back from a guide's: quest stops (an objective where it is
    /// finished), flight paths, inns, the hearthstone and dungeons.
    fn read_guide(&self, route: &Route) -> Vec<RouteStep> {
        let world = &self.prepared.world;
        let mut out: Vec<RouteStep> = Vec::new();
        // The quests of a dungeon run follow its step: done by the run.
        let mut run = false;
        for step in &route.steps {
            run = match step.kind {
                StepKind::Dungeon => true,
                StepKind::Objective => run || step.inside,
                _ => false,
            };
            if run && step.kind == StepKind::Objective {
                continue;
            }
            let quest = step.quest;
            let target = step.target.as_ref();
            let next = match step.kind {
                StepKind::Accept => quest.map(|quest| RouteStep::Accept { quest }),
                StepKind::TurnIn => quest.map(|quest| RouteStep::TurnIn { quest }),
                StepKind::Objective => quest.zip(step.objective).map(|(quest, objective)| {
                    // Where it is finished: its last line (mobs met on the way come before).
                    out.retain(|s| {
                        !matches!(s, RouteStep::Objective { quest: q, objective: o, .. } if *q == quest && *o == objective)
                    });
                    RouteStep::Objective {
                        quest,
                        objective,
                        farm: false,
                        spot: None,
                    }
                }),
                StepKind::Hearth => Some(RouteStep::Hearth {
                    recall: step.say.iter().any(|p| p.key.starts_with("recall")),
                }),
                StepKind::Bind => target.map(|t| RouteStep::Bind { inn: t.id }),
                StepKind::Dungeon => target
                    .filter(|t| t.kind == TargetKind::Dungeon)
                    .map(|t| RouteStep::Dungeon { dungeon: t.id }),
                StepKind::FlightMaster => step
                    .world
                    .and_then(|[continent, x, y]| {
                        let pos = crate::world::Pos {
                            continent: continent as i64,
                            x,
                            y,
                            zone: 0,
                            z: None,
                        };
                        world.taxi_near(&pos, 100.0)
                    })
                    .map(|n| RouteStep::FlightMaster {
                        node: world.taxi_nodes[n].id,
                    }),
                _ => None,
            };
            out.extend(next);
        }
        out
    }

    /// Replay edited steps; the lines of the guide are written in the language of `names`.
    pub fn simulate(&self, steps: &[RouteStep], names: &Names) -> EditReport {
        let planner = self.planner();
        let resolved: Vec<Result<Edit, &'static str>> = steps.iter().map(|s| s.resolve(&planner)).collect();
        let edits: Vec<Edit> = resolved.iter().filter_map(|r| r.ok()).collect();
        let edited = planner.edit(&edits);
        let lines = export::event_steps(&planner, &edited.route, &edited.trace, names);
        self.report_of(&resolved, &edited, &lines, names)
    }

    /// The report of a replay (`edited`) of the steps `resolved` (those not resolved are
    /// errors), with the line each event writes (`lines`).
    pub(super) fn report_of(
        &self,
        resolved: &[Result<Edit, &'static str>],
        edited: &Edited,
        lines: &[Option<export::Step>],
        names: &Names,
    ) -> EditReport {
        let mut done = edited.steps.iter();
        let mut time = 0.0;
        let reports = resolved
            .iter()
            .map(|r| {
                if let Err(why) = r {
                    return StepReport {
                        status: "error",
                        reason: Some(why),
                        start: time,
                        end: time,
                        level: 0,
                        xp: 0,
                        line: None,
                        farm: None,
                        farm_all: None,
                        warning: None,
                    };
                }
                let step = done.next().expect("a replayed step per resolved one");
                time = step.end;
                let line = step.event.and_then(|n| lines[n].as_ref()).map(|line| Line {
                    kind: line.kind,
                    text: line.text.clone(),
                    zone: line.zone.clone(),
                    bg: line.bg,
                    map: line.map.clone(),
                });
                let (status, reason) = match step.outcome {
                    Outcome::Done => ("done", None),
                    Outcome::Skipped(why) => ("skipped", Some(why)),
                    Outcome::Invalid(why) => ("error", Some(why)),
                };
                StepReport {
                    status,
                    reason,
                    start: step.start,
                    end: step.end,
                    level: step.level,
                    xp: step.xp,
                    line,
                    farm: step.farm,
                    farm_all: step.farm_all,
                    warning: step.warning,
                }
            })
            .collect::<Vec<_>>();
        let errors = reports.iter().filter(|r| r.status == "error").count();
        let total = edited.breakdown.total;
        EditReport {
            total_time: total.round(),
            total_time_text: fmt_time(total),
            breakdown: edited.breakdown,
            steps: reports,
            errors,
            final_grind: edited.final_grind,
            unfinished: edited
                .unfinished
                .iter()
                .map(|&i| {
                    let q = &self.prepared.model.quests[i as usize];
                    QuestRef {
                        id: q.id,
                        name: names.get("quest", q.id, &q.name).to_owned(),
                    }
                })
                .collect(),
        }
    }

    /// The guide of edited steps, to save (with `notes`); refused when a step is not possible.
    pub fn save(&self, steps: &[RouteStep], notes: Vec<String>) -> Result<PlanOutcome> {
        let planner = self.planner();
        let edits = steps
            .iter()
            .map(|s| s.resolve(&planner))
            .collect::<Result<Vec<_>, _>>()
            .map_err(anyhow::Error::msg)?;
        let edited = planner.edit(&edits);
        if edited.errors() > 0 {
            bail!("{} steps of the guide are not possible", edited.errors());
        }
        // Objectives at the same place one after the other, done at once, when that is no worse.
        let places = edited
            .steps
            .iter()
            .zip(&edits)
            .map(
                |(step, edit)| match (edit, step.event.map(|n| &edited.trace[n].event)) {
                    (Edit::Stop { stop, .. }, Some(Event::Stop { loc, .. }))
                        if matches!(stop.kind, Kind::Objective(_)) =>
                    {
                        Some(loc.pos)
                    }
                    _ => None,
                },
            )
            .collect();
        let items: Vec<(RouteStep, Edit)> = steps.iter().cloned().zip(edits).collect();
        let together = planner.bring_together(
            items,
            |(_, edit)| match edit {
                Edit::Stop { stop, .. } => Some(*stop),
                _ => None,
            },
            places,
            |items| {
                let edits: Vec<Edit> = items.iter().map(|(_, edit)| *edit).collect();
                let edited = planner.edit(&edits);
                (edited.errors() == 0).then_some(edited.score)
            },
        );
        let (steps, edits): (Vec<RouteStep>, Vec<Edit>) = together.into_iter().unzip();
        let edited = planner.edit(&edits);
        planner.validate_required_trace(&edited.trace)?;
        Ok(self.outcome(steps, &edited, notes))
    }

    /// The empty starting draft of a manual guide; requirements apply when edits are saved.
    pub(super) fn blank(&self, notes: Vec<String>) -> PlanOutcome {
        self.outcome(vec![], &self.planner().edit(&[]), notes)
    }

    fn outcome(&self, steps: Vec<RouteStep>, edited: &Edited, notes: Vec<String>) -> PlanOutcome {
        let planner = self.planner();
        let total = edited.breakdown.total;
        let route = export::build_traced(
            &planner,
            &self.prepared.profile,
            &edited.route,
            &edited.trace,
            total,
            edited.breakdown,
        );
        let mut outcome = PlanOutcome {
            route,
            notes,
            walks: Some(self.prepared.world.terrain.memory()),
            plan: steps,
            explicit: true,
            end: None,
        };
        self.set_walks(&mut outcome);
        outcome
    }

    /// What can be added to the guide, named in the language of `names`.
    pub fn catalog(&self, names: &Names) -> EditCatalog {
        let (model, world) = (&self.prepared.model, &self.prepared.world);
        let zone = |id: i64| names.get("zone", id, world.zone_name(id)).to_owned();
        let mut seen = HashSet::new();
        let mut inns: Vec<CatalogPlace> = model
            .inns
            .iter()
            .enumerate()
            .filter(|&(i, l)| i != model.start_inn && seen.insert(l.id))
            .map(|(_, l)| CatalogPlace {
                id: l.id,
                name: names.get("npc", l.id, &l.name).to_owned(),
                zone: zone(l.zone),
            })
            .collect();
        inns.sort_by(|a, b| a.zone.cmp(&b.zone).then_with(|| a.name.cmp(&b.name)));
        let mut trainers: Vec<CatalogPlace> = model
            .trainers
            .iter()
            .map(|l| CatalogPlace {
                id: l.id,
                name: names.get("npc", l.id, &l.name).to_owned(),
                zone: zone(l.zone),
            })
            .collect();
        trainers.sort_by(|a, b| a.zone.cmp(&b.zone).then_with(|| a.name.cmp(&b.name)));
        let mut nodes: Vec<CatalogPlace> = world
            .taxi_nodes
            .iter()
            .map(|n| CatalogPlace {
                id: n.id,
                name: n.name.clone(),
                zone: world.zone_at(&n.pos).map(zone).unwrap_or_default(),
            })
            .collect();
        nodes.sort_by(|a, b| a.name.cmp(&b.name));
        let mut quests: Vec<CatalogQuest> = model
            .quests
            .iter()
            .map(|q| CatalogQuest {
                id: q.id,
                name: names.get("quest", q.id, &q.name).to_owned(),
                level: q.level,
                min_level: q.min_level,
                zone: q.starts.first().map(|l| zone(l.zone)).unwrap_or_default(),
                objectives: q
                    .objectives
                    .iter()
                    .enumerate()
                    .map(|(k, o)| {
                        if o.text.is_empty() {
                            format!("#{}", k + 1)
                        } else {
                            o.text.clone()
                        }
                    })
                    .collect(),
            })
            .collect();
        quests.sort_by(|a, b| a.level.cmp(&b.level).then_with(|| a.name.cmp(&b.name)));
        EditCatalog {
            quests,
            inns,
            trainers,
            nodes,
            dungeons: model
                .dungeons
                .iter()
                .map(|d| CatalogDungeon {
                    id: d.area,
                    name: names.get("zone", d.area, &d.name).to_owned(),
                    min_level: d.min_level,
                    max_level: d.max_level,
                })
                .collect(),
            links: {
                let mut seen = HashSet::new();
                world
                    .links
                    .iter()
                    .filter(|l| seen.insert((l.name.clone(), l.to_zone)))
                    .map(|l| CatalogLink {
                        link: LinkRef {
                            name: l.name.clone(),
                            to: l.to_zone,
                        },
                        from: zone(l.from_zone),
                        to: zone(l.to_zone),
                    })
                    .collect()
            },
            recall: model.recall.is_some(),
        }
    }
}
