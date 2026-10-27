//! A guide edited on its own page: the draft replayed in the background (each change asks for a
//! new replay, the one under way gives up), written as the page shows a guide (one line per
//! step of the draft, with its timeline), and what could be added at a moment of it (quests not
//! taken, objectives and turn-ins the draft forgets).

use super::RouteStep;
use super::edit::{EditReport, Editor};
use super::timeline::{Lines, Replay, Timeline};
use crate::export::{Step, StepKind};
use crate::model::{EntityKind, Loc, Names};
use crate::plan::{Edit, Kind, State, Stop};
use crate::world::Pos;
use serde::Serialize;
use std::collections::HashSet;
use std::sync::atomic::{AtomicU64, Ordering};

/// Requests made one after the other (the replays of a draft): a newer one makes the older
/// ones give up.
#[derive(Debug, Default)]
pub struct Requests(AtomicU64);

impl Requests {
    /// A new request: the ones made before it are superseded.
    pub fn next(&self) -> Request<'_> {
        Request {
            requests: self,
            n: self.0.fetch_add(1, Ordering::SeqCst) + 1,
        }
    }
}

/// One of the `Requests`.
#[derive(Debug, Clone, Copy)]
pub struct Request<'r> {
    requests: &'r Requests,
    n: u64,
}

impl Request<'_> {
    /// A newer request was made: this one is no longer wanted.
    pub fn superseded(&self) -> bool {
        self.requests.0.load(Ordering::SeqCst) != self.n
    }
}

/// A draft replayed: its report (without the lines), a line per step (as the guide would write
/// it, without what only the addon reads; the steps not done have a line of their kind, with no
/// place), and the timeline of those lines.
#[derive(Debug, Clone, Serialize)]
pub struct EditView {
    pub report: EditReport,
    pub lines: Vec<Step>,
    pub timeline: Timeline,
}

/// What could be added at a moment of a draft (after one of its steps).
#[derive(Debug, Clone, Default, Serialize)]
pub struct Suggestions {
    /// Quests the draft never takes that the character can take then (level, prerequisites,
    /// room in the log), with their givers: the nearest first.
    pub untaken: Vec<QuestAt>,
    /// Objectives of the quests in the log that the draft does not do, with their places (the
    /// index of a place is the spot to do it at: 0 its own, then the others).
    pub objectives: Vec<ObjectiveAt>,
    /// Quests in the log, their objectives done, that the draft does not turn in, with where
    /// they are turned in.
    pub turn_ins: Vec<QuestAt>,
}

#[derive(Debug, Clone, Serialize)]
pub struct QuestAt {
    pub quest: i64,
    pub name: String,
    pub level: i64,
    /// How hard the quest is for the character then, as the game colors it: grey, green,
    /// yellow, orange or red.
    pub difficulty: &'static str,
    /// The dungeon its objectives are done in, if any (its name).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub dungeon: Option<String>,
    pub places: Vec<PlaceAt>,
}

/// The game's color of a quest of `quest` level for a character of `level`: red five levels
/// above or more, orange three, yellow within two, green down to the grey content, grey below.
fn difficulty(quest: i64, level: i64) -> &'static str {
    match quest - level {
        d if d >= 5 => "red",
        d if d >= 3 => "orange",
        d if d >= -2 => "yellow",
        _ if quest > crate::xp::Rules::gray_level(level) => "green",
        _ => "grey",
    }
}

#[derive(Debug, Clone, Serialize)]
pub struct ObjectiveAt {
    pub quest: i64,
    pub objective: u8,
    /// The quest's name, and what the objective's step says.
    pub name: String,
    pub text: String,
    /// Done in a dungeon run (its area ID and name): its place is the entrance, a run adds it.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub dungeon: Option<(i64, String)>,
    pub places: Vec<PlaceAt>,
}

/// A place in the world (continent, x, y) and who or what stands there.
#[derive(Debug, Clone, Serialize)]
pub struct PlaceAt {
    pub at: [f64; 3],
    pub name: String,
}

/// The kind of guide step a step of the draft writes.
fn kind_of(step: &RouteStep) -> StepKind {
    match step {
        RouteStep::Accept { .. } => StepKind::Accept,
        RouteStep::Objective { .. } => StepKind::Objective,
        RouteStep::TurnIn { .. } => StepKind::TurnIn,
        RouteStep::FlightMaster { .. } => StepKind::FlightMaster,
        RouteStep::Bind { .. } => StepKind::Bind,
        RouteStep::Hearth { .. } => StepKind::Hearth,
        RouteStep::Fly { .. } => StepKind::Fly,
        RouteStep::Link { .. } => StepKind::Link,
        RouteStep::Teleport { .. } => StepKind::Teleport,
        RouteStep::DeathSkip => StepKind::DeathSkip,
        RouteStep::Train { .. } => StepKind::Train,
        RouteStep::Sell { .. } => StepKind::Sell,
        RouteStep::Dungeon { .. } => StepKind::Dungeon,
        RouteStep::Abandon { .. } => StepKind::Abandon,
        RouteStep::Grind { .. } | RouteStep::GrindTo { .. } => StepKind::Grind,
    }
}

fn point(p: &Pos) -> [f64; 3] {
    [
        p.continent as f64,
        (p.x * 10.0).round() / 10.0,
        (p.y * 10.0).round() / 10.0,
    ]
}

impl Editor {
    /// The draft `steps` replayed in the background, written in the language of `names`. None
    /// when `request` is superseded before it is done.
    pub fn view(&self, steps: &[RouteStep], names: &Names, request: &Request<'_>) -> Option<EditView> {
        let abort = || request.superseded();
        let planner = self.planner().abortable(&abort);
        let resolved: Vec<Result<Edit, &'static str>> = steps.iter().map(|s| s.resolve(&planner)).collect();
        let edits: Vec<Edit> = resolved.iter().filter_map(|r| r.ok()).collect();
        let replay = Replay::run(self, &planner, &edits, names, Lines::Events)?;
        let report = self.report_of(&resolved, &replay.edited, &replay.event_lines, names);
        // A line per step of the draft: its own event's, else one of its kind where the step
        // before ends.
        let mut done = replay.edited.steps.iter();
        let (mut time, mut level) = (0.0, self.prepared.profile.from_level);
        let lines: Vec<Step> = steps
            .iter()
            .zip(&resolved)
            .map(|(step, r)| {
                let line = r
                    .ok()
                    .and_then(|_| done.next())
                    .and_then(|s| s.event)
                    .and_then(|n| replay.event_lines[n].clone());
                let mut line = line.unwrap_or_else(|| {
                    let mut bare = Step::bare(kind_of(step), level, time);
                    if let RouteStep::Accept { quest }
                    | RouteStep::TurnIn { quest }
                    | RouteStep::Abandon { quest }
                    | RouteStep::Objective { quest, .. } = *step
                    {
                        bare.quest = Some(quest);
                    }
                    if let RouteStep::Objective { objective, .. } = *step {
                        bare.objective = Some(objective);
                    }
                    bare
                });
                line.time = line.time.max(time);
                (time, level) = (line.time, line.level);
                line
            })
            .collect();
        let mut timeline = replay.timeline(&lines);
        if !self.add_walk_paths(&mut timeline, Some(request)) {
            return None;
        }
        // What the page does not show (the addon's: sentences as phrases, prerequisites, other
        // places), and the report's lines (`lines` has them).
        let mut lines = lines;
        for l in &mut lines {
            (l.say, l.alt, l.pre, l.pre_any, l.spots, l.spells, l.mob_ids) = Default::default();
        }
        let mut report = report;
        for r in &mut report.steps {
            r.line = None;
        }
        Some(EditView {
            report,
            lines,
            timeline,
        })
    }

    /// What could be added to the draft `steps` after its step `after` (none: before the
    /// first), named in the language of `names`. None when `request` is superseded.
    pub fn suggest(
        &self,
        steps: &[RouteStep],
        after: Option<usize>,
        names: &Names,
        request: &Request<'_>,
    ) -> Option<Suggestions> {
        let abort = || request.superseded();
        let planner = self.planner().abortable(&abort);
        let resolved: Vec<Result<Edit, &'static str>> = steps.iter().map(|s| s.resolve(&planner)).collect();
        let edits: Vec<Edit> = resolved.iter().filter_map(|r| r.ok()).collect();
        // The character after `after`: the replay stopped there.
        let before = after.map_or(0, |a| {
            resolved[..=a.min(resolved.len().saturating_sub(1))]
                .iter()
                .filter(|r| r.is_ok())
                .count()
        });
        let mut seen = 0;
        let mut there: Option<State> = None;
        let _ = planner.edit_observed(&edits, &mut |s| {
            if seen == before {
                there = Some(s.clone());
                return false;
            }
            seen += 1;
            true
        });
        let s = there?;
        if request.superseded() {
            return None;
        }
        Some(self.suggestions_at(&planner, &s, &edits, before, names))
    }

    fn suggestions_at(
        &self,
        planner: &crate::plan::Planner<'_>,
        s: &State,
        edits: &[Edit],
        before: usize,
        names: &Names,
    ) -> Suggestions {
        let model = &self.prepared.model;
        // The dungeons run from then on: their quests' objectives are done there.
        let dungeons: HashSet<u32> = edits[before.min(edits.len())..]
            .iter()
            .filter_map(|e| match e {
                Edit::Stop { stop, .. } if stop.kind == Kind::Dungeon => Some(stop.index),
                _ => None,
            })
            .collect();
        let dungeon_name = |d: usize| {
            names
                .get("zone", model.dungeons[d].area, &model.dungeons[d].name)
                .to_owned()
        };
        let planned = |kind: Kind| -> HashSet<u32> {
            edits
                .iter()
                .filter_map(|e| match e {
                    Edit::Stop { stop, .. } if stop.kind == kind => Some(stop.index),
                    _ => None,
                })
                .collect()
        };
        let (accepts, turn_ins) = (planned(Kind::Accept), planned(Kind::TurnIn));
        let objectives: HashSet<(u32, u8)> = edits
            .iter()
            .filter_map(|e| match e {
                Edit::Stop {
                    stop:
                        Stop {
                            index,
                            kind: Kind::Objective(k),
                        },
                    ..
                } => Some((*index, *k)),
                _ => None,
            })
            .collect();
        let place = |l: &Loc| PlaceAt {
            at: point(&l.pos),
            name: names
                .get(
                    match l.kind {
                        EntityKind::Npc => "npc",
                        EntityKind::Object => "object",
                        EntityKind::Item => "item",
                        _ => "zone",
                    },
                    l.id,
                    &l.name,
                )
                .to_owned(),
        };
        let availability = planner.availability();
        let mut out = Suggestions::default();
        for (i, q) in model.quests.iter().enumerate() {
            let n = i as u32;
            let name = || names.get("quest", q.id, &q.name).to_owned();
            if s.accepted[i] && !s.turned[i] {
                for (k, o) in q.objectives.iter().enumerate() {
                    let k8 = k as u8;
                    let in_run = o.dungeon.is_some_and(|d| dungeons.contains(&(d as u32)));
                    let allowed = o.dungeon.is_none_or(|d| availability.dungeon_allowed(d));
                    if s.objectives[i] & (1 << k) == 0 && allowed && !in_run && !objectives.contains(&(n, k8)) {
                        out.objectives.push(ObjectiveAt {
                            quest: q.id,
                            objective: k8,
                            name: name(),
                            text: crate::export::objective_text(o, names),
                            dungeon: o.dungeon.map(|d| (model.dungeons[d].area, dungeon_name(d))),
                            places: match o.dungeon {
                                Some(d) => vec![place(&model.dungeons[d].entrance)],
                                None => o.places().map(|p| place(&p.loc)).collect(),
                            },
                        });
                    }
                }
                if !turn_ins.contains(&n) && availability.objectives_done(s, n) {
                    out.turn_ins.push(QuestAt {
                        quest: q.id,
                        name: name(),
                        level: q.level,
                        difficulty: difficulty(q.level, s.level),
                        dungeon: None,
                        places: q.ends.iter().map(place).collect(),
                    });
                }
            } else if !accepts.contains(&n)
                && q.min_level <= s.level
                && availability.valid(s, Stop::quest(n, Kind::Accept))
                && !s.accepted[i]
                && !s.turned[i]
            {
                let givers: Vec<PlaceAt> = q
                    .starts
                    .iter()
                    .filter(|l| l.kind != EntityKind::Item)
                    .map(place)
                    .collect();
                if !givers.is_empty() {
                    out.untaken.push(QuestAt {
                        quest: q.id,
                        name: name(),
                        level: q.level,
                        difficulty: difficulty(q.level, s.level),
                        dungeon: q.objectives.iter().find_map(|o| o.dungeon).map(dungeon_name),
                        places: givers,
                    });
                }
            }
        }
        // The nearest first (as the crow flies; those of other continents last).
        let away = |q: &QuestAt| {
            (q.places.iter())
                .map(|p| {
                    let at = Pos {
                        continent: p.at[0] as i64,
                        x: p.at[1],
                        y: p.at[2],
                        zone: 0,
                        z: None,
                    };
                    s.pos.dist(&at)
                })
                .fold(f64::INFINITY, f64::min)
        };
        out.untaken.sort_by(|a, b| away(a).total_cmp(&away(b)));
        out
    }
}
