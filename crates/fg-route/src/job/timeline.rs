//! A guide replayed to be watched: when each of its steps starts and ends and where the time
//! goes, the way the character moves (walks, flights, boats, the hearthstone), and what it has
//! after each step (quests in the log, flight paths, the inn it is bound to).
//!
//! The guide's steps are replayed as the guide editor replays them (`Planner::edit`), so the
//! times are the guide's own. The guide as saved keeps the last word: its steps give the times,
//! the replay what happens within them (a step it does not find has a straight walk to it).

use super::PlanOutcome;
use super::draft::Request;
use super::edit::Editor;
use crate::export::{self, Step, StepKind};
use crate::model::{EntityKind, Names};
use crate::plan::{Breakdown, Edit, Edited, Event, Kind, Planner, Timed};
use crate::world::Pos;
use serde::Serialize;
use std::collections::{HashMap, HashSet};

/// A guide replayed.
#[derive(Debug, Clone, Serialize)]
pub struct Timeline {
    /// One per step of the guide, in its order.
    pub steps: Vec<TimelineStep>,
    /// The character's moves, in time order (none overlap).
    pub legs: Vec<Leg>,
    /// The character before the first step.
    pub start: StartView,
    /// The quests the steps take, do and turn in (and those of the log at the start), by ID.
    pub quests: Vec<QuestInfo>,
    /// The flight paths the character knows or learns.
    pub nodes: Vec<NodeInfo>,
    /// The inns the hearthstone is bound to.
    pub inns: Vec<InnInfo>,
    /// XP from each level to the next (index: level).
    pub xp_levels: Vec<i64>,
    pub log_size: usize,
    /// From this level the character rides a mount.
    pub mount_level: i64,
    /// Steps of the guide the replay did the same (the others have a straight walk to them):
    /// all of them for a guide replayed as it was computed.
    pub matched: usize,
}

/// A step of the guide as replayed: its time, where the time goes, what changes.
#[derive(Debug, Clone, Serialize)]
pub struct TimelineStep {
    pub start: f64,
    pub end: f64,
    /// When the character is where the step is done (its moves over).
    pub arrive: f64,
    /// Seconds of travel (walks, flights, boats, the hearthstone), combat (kills, grinding,
    /// dungeons), interaction (talking, looting, training) and waiting (respawns, boats).
    #[serde(skip_serializing_if = "is_zero")]
    pub travel: f64,
    #[serde(skip_serializing_if = "is_zero")]
    pub combat: f64,
    #[serde(skip_serializing_if = "is_zero")]
    pub interact: f64,
    #[serde(skip_serializing_if = "is_zero")]
    pub wait: f64,
    /// The replay does not find it (the data changed since the guide was computed): its split
    /// and its moves are guessed.
    #[serde(skip_serializing_if = "std::ops::Not::not")]
    pub estimate: bool,
    /// The named place it is done at (a town, a valley), in the guide's language.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub place: Option<String>,
    /// Quests taken, objectives done (quest, objective), quests out of the log (turned in or
    /// given up), flight paths learned (taxi node IDs).
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub accepted: Vec<i64>,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub done: Vec<(i64, u8)>,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub removed: Vec<i64>,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub flights: Vec<i64>,
    /// The inn the hearthstone is bound to from now on (its innkeeper's ID).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub bind: Option<i64>,
    /// When the hearthstone, and the shaman's Astral Recall, are ready again (used here).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub hearth: Option<f64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub recall: Option<f64>,
    /// Copper paid for flights.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub cost: Option<i64>,
}

/// How the character moves.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum LegMode {
    Walk,
    /// Walking at the level of the mount.
    Mount,
    Flight,
    /// Boats, zeppelins, trams, portals.
    Link,
    /// The hearthstone (or Astral Recall): a jump to the inn once cast.
    Hearth,
    /// A class teleport, or back from the spirit healer: a jump.
    Teleport,
}

/// A move of the character, from `from` to `to` (world points: continent, x, y).
#[derive(Debug, Clone, Serialize)]
pub struct Leg {
    pub mode: LegMode,
    pub start: f64,
    pub end: f64,
    pub from: [f64; 3],
    pub to: [f64; 3],
    /// The points in between (x, y, on the continent of `from`): the way a walk goes around
    /// the terrain and through the passages between zones, a flight's way.
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub via: Vec<[f32; 2]>,
    /// The character's level, for the way a walk takes (the danger on it).
    #[serde(skip)]
    pub level: i64,
}

/// The character before the first step.
#[derive(Debug, Clone, Serialize)]
pub struct StartView {
    pub at: [f64; 3],
    /// Quests in the log and their objectives done (quest, objective).
    pub log: Vec<i64>,
    pub done: Vec<(i64, u8)>,
    /// Flight paths known (taxi node IDs).
    pub flights: Vec<i64>,
    /// The inn the hearthstone is bound to (its innkeeper's ID).
    pub bind: Option<i64>,
    /// Whether the character learns Astral Recall (a second hearthstone).
    pub recall: bool,
}

/// A quest of the guide, named in its language.
#[derive(Debug, Clone, Serialize)]
pub struct QuestInfo {
    pub id: i64,
    pub name: String,
    pub objectives: Vec<ObjectiveInfo>,
}

#[derive(Debug, Clone, Serialize)]
pub struct ObjectiveInfo {
    /// What the step says (Kill Mindless Zombie x8).
    pub text: String,
    /// kill, collect (an item), use (an object), talk, other.
    pub kind: &'static str,
    /// The amount it asks for.
    pub count: u32,
}

#[derive(Debug, Clone, Serialize)]
pub struct NodeInfo {
    pub id: i64,
    pub name: String,
    pub at: [f64; 3],
}

#[derive(Debug, Clone, Serialize)]
pub struct InnInfo {
    /// Its innkeeper's ID.
    pub id: i64,
    /// The innkeeper, and the town (or zone) of the inn.
    pub name: String,
    pub place: String,
    pub at: [f64; 3],
}

/// What the replay keeps of the character after each step.
struct Snapshot {
    pos: Pos,
    time: f64,
    level: i64,
    spent: Breakdown,
    /// Quests in the log (indexes) and the objectives done of each.
    log: Vec<(u32, u8)>,
    known: u128,
    bind: usize,
    hearth_ready: f64,
    recall_ready: f64,
}

/// A part of the replay: a move, or time spent at a place.
#[derive(Debug, Clone)]
struct Piece {
    start: f64,
    end: f64,
    /// Seconds of travel, combat, interaction and waiting in it.
    split: [f64; 4],
    /// None: the character stays where it is.
    leg: Option<Leg>,
}

const TRAVEL: usize = 0;
const COMBAT: usize = 1;
const INTERACT: usize = 2;
const WAIT: usize = 3;

/// A walk path keeps the points farther than this from the line between its neighbours (yards).
const SIMPLIFY: f64 = 6.0;

/// A point for the frontend: continent, x, y (a tenth of a yard).
fn point(p: &Pos) -> [f64; 3] {
    [p.continent as f64, round1(p.x), round1(p.y)]
}

// An f64 is "trivially copied" where pointers are 64-bit only (not in the browser's wasm32).
#[cfg_attr(
    target_pointer_width = "64",
    expect(
        clippy::trivially_copy_pass_by_ref,
        reason = "serde's skip_serializing_if passes a reference"
    )
)]
fn is_zero(v: &f64) -> bool {
    *v == 0.0
}

fn round1(v: f64) -> f64 {
    (v * 10.0).round() / 10.0
}

/// Points of the frontend (continent, x, y) on the same continent.
fn same_continent(a: &[f64; 3], b: &[f64; 3]) -> bool {
    a[0] as i64 == b[0] as i64
}

/// Points of the frontend at the same place (a tenth of a yard).
fn same_point(a: &[f64; 3], b: &[f64; 3]) -> bool {
    same_continent(a, b) && (a[1] - b[1]).hypot(a[2] - b[2]) < 0.05
}

/// Two points far enough apart to show a move between them.
fn apart(a: &Pos, b: &Pos) -> bool {
    a.continent != b.continent || a.dist(b) > 1.0
}

impl Editor {
    /// The guide of `outcome` replayed, its texts in the language of `names`.
    pub fn timeline(&self, outcome: &PlanOutcome, names: &Names) -> Timeline {
        let planner = self.planner();
        let (steps, _) = self.steps(outcome);
        let edits: Vec<Edit> = steps.iter().filter_map(|s| s.resolve(&planner).ok()).collect();
        let replay = Replay::run(self, &planner, &edits, names, Lines::Guide).expect("a replay nothing stops ends");
        let mut timeline = replay.timeline(&outcome.route.steps);
        self.add_walk_paths(&mut timeline, None);
        timeline
    }

    /// The ways the walks of `timeline` take around the terrain. False when `request` is
    /// superseded on the way (the timeline is then left half done).
    pub(super) fn add_walk_paths(&self, timeline: &mut Timeline, request: Option<&Request<'_>>) -> bool {
        let Some(paths) = self.walk_paths(&timeline.legs, request) else {
            return false;
        };
        for (leg, path) in timeline.legs.iter_mut().zip(paths) {
            if !path.is_empty() {
                leg.via = path;
            }
        }
        true
    }

    /// The ways the walks of a timeline take around the terrain (simplified), by leg: empty
    /// for the legs that are no walk or go straight. None when `request` is superseded.
    fn walk_paths(&self, legs: &[Leg], request: Option<&Request<'_>>) -> Option<Vec<Vec<[f32; 2]>>> {
        let world = &self.prepared.world;
        let planner = self.planner();
        let at = |p: &[f64; 3]| Pos {
            continent: p[0] as i64,
            x: p[1],
            y: p[2],
            zone: world
                .zone_at(&Pos {
                    continent: p[0] as i64,
                    x: p[1],
                    y: p[2],
                    zone: 0,
                })
                .unwrap_or(0),
        };
        legs.iter()
            .enumerate()
            .map(|(n, leg)| {
                if n % 32 == 0 && request.is_some_and(Request::superseded) {
                    return None;
                }
                if !matches!(leg.mode, LegMode::Walk | LegMode::Mount) || !same_continent(&leg.from, &leg.to) {
                    return Some(Vec::new());
                }
                // The passages it goes through, then the way around the terrain between them.
                let mut stops = vec![at(&leg.from)];
                stops.extend(
                    leg.via
                        .iter()
                        .map(|v| at(&[leg.from[0], f64::from(v[0]), f64::from(v[1])])),
                );
                stops.push(at(&leg.to));
                let level = planner.trips().danger_level(leg.level);
                let mut path: Vec<Pos> = vec![stops[0]];
                for w in stops.windows(2) {
                    path.extend(world.walk_path(&w[0], &w[1], level).into_iter().skip(1));
                }
                let path = simplify(&path, SIMPLIFY);
                if path.len() <= 2 {
                    return Some(Vec::new());
                }
                Some(
                    path[1..path.len() - 1]
                        .iter()
                        .map(|p| [round1(p.x) as f32, round1(p.y) as f32])
                        .collect(),
                )
            })
            .collect()
    }
}

/// Douglas-Peucker: the points of `path` that stay within `tolerance` yards of the line drawn
/// through the ones kept (its ends always kept).
fn simplify(path: &[Pos], tolerance: f64) -> Vec<Pos> {
    fn keep(path: &[Pos], tolerance: f64, out: &mut Vec<Pos>) {
        let (a, b) = (path[0], path[path.len() - 1]);
        let (dx, dy) = (b.x - a.x, b.y - a.y);
        let len = dx.hypot(dy);
        let far = (1..path.len() - 1)
            .map(|i| {
                let p = path[i];
                let d = if len < 1e-9 {
                    (p.x - a.x).hypot(p.y - a.y)
                } else {
                    ((p.x - a.x) * dy - (p.y - a.y) * dx).abs() / len
                };
                (i, d)
            })
            .max_by(|x, y| x.1.total_cmp(&y.1));
        match far {
            Some((i, d)) if d > tolerance => {
                keep(&path[..=i], tolerance, out);
                out.pop();
                keep(&path[i..], tolerance, out);
            }
            _ => {
                out.push(a);
                out.push(b);
            }
        }
    }
    if path.len() <= 2 {
        return path.to_vec();
    }
    let mut out = Vec::new();
    keep(path, tolerance, &mut out);
    out
}

/// Which steps a replay writes: the guide's (checkpoints added, grinds moved where mobs of the
/// character's level live), or one per event of the replay (the guide editor's lines).
#[derive(Clone, Copy, PartialEq, Eq)]
pub(super) enum Lines {
    Guide,
    Events,
}

/// A guide replayed by the guide editor's planner, the character kept after each step.
pub(super) struct Replay<'r> {
    planner: &'r Planner<'r>,
    names: &'r Names,
    pub(super) edited: Edited,
    /// The line each event writes (`Lines::Events`; empty otherwise).
    pub(super) event_lines: Vec<Option<Step>>,
    /// Each step's events in the trace, and when it starts and ends (the final grind: a step
    /// of its own, last).
    steps: Vec<(std::ops::Range<usize>, f64, f64)>,
    /// The character before the first step, then after each one (`steps`).
    snaps: Vec<Snapshot>,
    /// The guide's steps written from the replay.
    lines: Vec<Step>,
}

impl<'r> Replay<'r> {
    /// `edits` replayed, the steps written as `lines` says. None when the planner gives up (a
    /// newer replay is asked for: `Planner::abortable`).
    pub(super) fn run(
        editor: &'r Editor,
        planner: &'r Planner<'r>,
        edits: &[Edit],
        names: &'r Names,
        lines: Lines,
    ) -> Option<Self> {
        let mut snaps = Vec::new();
        let edited = planner.edit_observed(edits, &mut |s| {
            let log = s
                .accepted
                .and_not(&s.turned)
                .into_iter()
                .map(|i| (i as u32, s.objectives[i]))
                .collect();
            snaps.push(Snapshot {
                pos: s.pos,
                time: s.time,
                level: s.level,
                spent: s.spent,
                log,
                known: s.known,
                bind: s.bind,
                hearth_ready: s.hearth_ready,
                recall_ready: s.recall_ready,
            });
            true
        })?;
        let mut spans: Vec<_> = edited
            .steps
            .iter()
            .map(|s| (s.events.clone(), s.start, s.end))
            .collect();
        // The final grind, after the last step.
        let tail = spans.last().map_or(0, |s| s.0.end);
        let last = snaps.last().map_or(0.0, |s| s.time);
        spans.push((tail..edited.trace.len(), spans.last().map_or(0.0, |s| s.2), last));
        let p = &editor.prepared;
        let (lines, event_lines) = match lines {
            Lines::Guide => (
                export::build_traced(
                    planner,
                    &p.profile,
                    &edited.route,
                    &edited.trace,
                    edited.breakdown.total,
                    edited.breakdown,
                )
                .steps,
                Vec::new(),
            ),
            Lines::Events => {
                let each = export::event_steps(planner, &edited.route, &edited.trace, names);
                (each.iter().flatten().cloned().collect(), each)
            }
        };
        if planner.aborted() {
            return None;
        }
        Some(Self {
            planner,
            names,
            edited,
            event_lines,
            steps: spans,
            snaps,
            lines,
        })
    }

    /// The timeline of the guide's steps (`saved`, the guide as it was saved).
    pub(super) fn timeline(&self, saved: &[Step]) -> Timeline {
        // The guide editor's lines are the replay's own: tied by their event.
        let pairs = if self.event_lines.is_empty() {
            align(saved, &self.lines)
        } else {
            let line_of: HashMap<usize, usize> = (self.lines.iter().enumerate())
                .filter_map(|(j, l)| Some((l.event?, j)))
                .collect();
            (saved.iter().enumerate())
                .filter_map(|(i, s)| Some((i, *line_of.get(&s.event?)?)))
                .collect()
        };
        // Replayed line of each saved step.
        let mut of_saved: Vec<Option<usize>> = vec![None; saved.len()];
        for &(i, j) in &pairs {
            of_saved[i] = Some(j);
        }
        let step_of = self.step_of_events();
        let pieces = self.pieces();
        let windows: Vec<(f64, f64)> = (0..saved.len())
            .map(|i| {
                let start = if i == 0 { 0.0 } else { saved[i - 1].time.max(0.0) };
                (start, saved[i].time.max(start))
            })
            .collect();

        // The events the saved steps were written from, in the replay's order: each cuts the
        // replay, the part up to it maps onto the windows of its steps.
        let mut groups: HashMap<usize, Vec<usize>> = HashMap::new();
        for &(i, j) in &pairs {
            if let Some(e) = self.lines[j].event {
                groups.entry(e).or_default().push(i);
            }
        }
        let mut events: Vec<usize> = groups.keys().copied().collect();
        events.sort_unstable();
        // Steps at one place the guide writes in another order than the replay did them (the
        // flight path learned before the quest taken there): no cut between them, their steps
        // share one window, else the moves before the first would land in the window of a step
        // the guide puts later.
        let bounds: Vec<(f64, f64)> = (events.iter())
            .map(|e| {
                let group = &groups[e];
                let start = group.iter().map(|&i| windows[i].0).fold(f64::INFINITY, f64::min);
                let end = group.iter().map(|&i| windows[i].1).fold(f64::NEG_INFINITY, f64::max);
                (start, end)
            })
            .collect();
        let mut later = vec![f64::INFINITY; events.len() + 1];
        for k in (0..events.len()).rev() {
            later[k] = later[k + 1].min(bounds[k].0);
        }
        let mut kept = Vec::with_capacity(events.len());
        let mut carried: Vec<usize> = Vec::new();
        for (k, &e) in events.iter().enumerate() {
            carried.extend(&groups[&e]);
            if bounds[k].1 <= later[k + 1] + 1e-6 {
                groups.insert(e, std::mem::take(&mut carried));
                kept.push(e);
            }
        }
        if let Some(&last) = kept.last() {
            groups.get_mut(&last).expect("a kept cut").extend(carried);
        }
        let events = kept;
        let mut legs: Vec<Leg> = Vec::new();
        let mut placed: Vec<Piece> = Vec::new();
        let mut from = 0.0;
        // The piece being placed, and where its part left starts (a piece across a cut is
        // shared by the steps on both sides).
        let mut next = 0;
        let mut rest = 0.0;
        for &e in &events {
            let cut = self.edited.trace[e].time.max(from);
            let group = &groups[&e];
            let start = group.iter().map(|&i| windows[i].0).fold(f64::INFINITY, f64::min);
            let end = group.iter().map(|&i| windows[i].1).fold(f64::NEG_INFINITY, f64::max);
            let scale = if cut - from > 1e-6 {
                (end - start).max(0.0) / (cut - from)
            } else {
                0.0
            };
            let map = |t: f64| start + (t.clamp(from, cut) - from) * scale;
            while next < pieces.len() && pieces[next].start < cut {
                let p = &pieces[next];
                let begin = p.start.max(rest);
                let (a, b) = (map(begin), map(p.end.min(cut)));
                let share = if p.end - p.start > 1e-9 {
                    (p.end.min(cut) - begin) / (p.end - p.start)
                } else {
                    1.0
                };
                let mut leg = p.leg.clone();
                if let Some(l) = &mut leg {
                    (l.start, l.end) = (a, b);
                }
                placed.push(Piece {
                    start: a,
                    end: b,
                    split: p.split.map(|s| s * share * scale),
                    leg,
                });
                if p.end <= cut {
                    next += 1;
                    rest = 0.0;
                } else {
                    // The rest belongs to the next step.
                    rest = cut;
                    break;
                }
            }
            from = cut;
        }
        placed.sort_by(|a, b| a.start.total_cmp(&b.start));
        for p in &placed {
            if let Some(l) = &p.leg {
                legs.push(l.clone());
            }
        }
        self.straight_walks(saved, &of_saved, &windows, &mut legs);
        // One after the other, each from where the one before ends.
        legs.sort_by(|a, b| a.start.total_cmp(&b.start));
        let mut at = point(&self.snaps[0].pos);
        let mut until = 0.0_f64;
        for l in &mut legs {
            l.start = l.start.max(until);
            l.end = l.end.max(l.start);
            if !same_point(&l.from, &at)
                && same_continent(&l.from, &at)
                && matches!(l.mode, LegMode::Walk | LegMode::Mount)
            {
                l.from = at;
            }
            at = l.to;
            until = l.end;
            (l.start, l.end) = (round1(l.start), round1(l.end));
        }
        legs.retain(|l| !same_point(&l.from, &l.to) || !l.via.is_empty());

        let deltas = self.deltas(saved, &of_saved, &step_of);
        let params = self.planner.params;
        // The first piece that may overlap the step (the windows follow each other).
        let mut first = 0;
        let steps: Vec<TimelineStep> = (0..saved.len())
            .map(|i| {
                let (start, end) = windows[i];
                let mut split = [0.0; 4];
                let mut arrive = start;
                while first < placed.len() && placed[first].end < start {
                    first += 1;
                }
                for p in placed[first..].iter().take_while(|p| p.start <= end) {
                    let overlap = (p.end.min(end) - p.start.max(start)).max(0.0);
                    let length = p.end - p.start;
                    let share = if length > 1e-9 { overlap / length } else { 0.0 };
                    for (k, s) in split.iter_mut().enumerate() {
                        *s += p.split[k] * share;
                    }
                    if p.leg.is_some() && p.end > start && p.end <= end {
                        arrive = arrive.max(p.end);
                    }
                }
                // What the replay does not tell (a step it does not find), by the step's kind.
                let left = (end - start) - split.iter().sum::<f64>();
                if left > 0.5 {
                    split[match saved[i].kind {
                        StepKind::Objective | StepKind::Grind | StepKind::Dungeon => COMBAT,
                        StepKind::Fly | StepKind::Link | StepKind::Hearth | StepKind::Teleport => TRAVEL,
                        _ => INTERACT,
                    }] += left;
                }
                let delta = &deltas[i];
                TimelineStep {
                    start: round1(start),
                    end: round1(end),
                    arrive: round1(arrive),
                    travel: round1(split[TRAVEL]),
                    combat: round1(split[COMBAT]),
                    interact: round1(split[INTERACT]),
                    wait: round1(split[WAIT]),
                    estimate: of_saved[i].is_none(),
                    place: self.place(&saved[i]),
                    accepted: delta.accepted.clone(),
                    done: delta.done.clone(),
                    removed: delta.removed.clone(),
                    flights: delta.flights.clone(),
                    bind: delta.bind,
                    hearth: delta.hearth.then(|| round1(end + params.hearth_cooldown)),
                    recall: delta.recall.then(|| round1(end + params.recall_cooldown)),
                    cost: delta.cost,
                }
            })
            .collect();
        self.timeline_of(steps, legs, pairs.len())
    }

    /// The steps the replay does not find: a straight walk from where the character is to
    /// where they are done, at the start of their window.
    fn straight_walks(&self, saved: &[Step], of_saved: &[Option<usize>], windows: &[(f64, f64)], legs: &mut Vec<Leg>) {
        let speed = |level: i64| self.planner.params.speed(level) / self.planner.params.detour;
        let mut found = legs.clone();
        found.sort_by(|a, b| a.start.total_cmp(&b.start));
        for i in (0..saved.len()).filter(|&i| of_saved[i].is_none()) {
            let Some(w) = saved[i].world else { continue };
            let (start, end) = windows[i];
            // Where the character is then: the end of the last move before, else the start.
            let at = found
                .iter()
                .rev()
                .find(|l| l.end <= start + 1e-6)
                .map_or_else(|| point(&self.snaps[0].pos), |l| l.to);
            let (a, b) = (pos_of(at), pos_of(w));
            if !apart(&a, &b) || a.continent != b.continent {
                continue;
            }
            let length = (a.dist(&b) / speed(saved[i].level)).min(end - start);
            let leg = Leg {
                mode: self.walk_mode(saved[i].level),
                start,
                end: start + length,
                from: at,
                to: w,
                via: Vec::new(),
                level: saved[i].level,
            };
            let k = found.partition_point(|l| l.start <= start);
            found.insert(k, leg.clone());
            legs.push(leg);
        }
    }

    fn walk_mode(&self, level: i64) -> LegMode {
        if level >= self.planner.params.mount_level {
            LegMode::Mount
        } else {
            LegMode::Walk
        }
    }

    /// The step (index in `steps`) of each event of the trace.
    fn step_of_events(&self) -> Vec<usize> {
        let mut out = vec![0; self.edited.trace.len()];
        for (k, (events, ..)) in self.steps.iter().enumerate() {
            for e in events.clone() {
                out[e] = k;
            }
        }
        out
    }

    /// The parts of the replay one after the other, in its time.
    fn pieces(&self) -> Vec<Piece> {
        let mut out = Vec::new();
        for (k, (events, start, end)) in self.steps.iter().enumerate() {
            let (before, after) = (&self.snaps[k], &self.snaps[(k + 1).min(self.snaps.len() - 1)]);
            self.step_pieces(
                &self.edited.trace[events.clone()],
                (*start, *end),
                before,
                after,
                &mut out,
            );
        }
        out
    }

    /// The parts of one step: its events (`events`) between `start` and `end`, the character
    /// `before` and `after` it.
    fn step_pieces(
        &self,
        events: &[Timed],
        (start, end): (f64, f64),
        before: &Snapshot,
        after: &Snapshot,
        out: &mut Vec<Piece>,
    ) {
        let (world, model, params) = (self.planner.world, self.planner.model, self.planner.params);
        let d = |f: fn(&Breakdown) -> f64| (f(&after.spent) - f(&before.spent)).max(0.0);
        // Walking to the place of a stop (on foot off a flight, killing mobs on the way).
        let walking = d(|b| b.travel) + d(|b| b.flights) + d(|b| b.farming);
        let farming = d(|b| b.farming);
        let mut t = start;
        let mut at = before.pos;
        let mut via: Vec<[f32; 2]> = Vec::new();
        let level = before.level;
        let walk_mode = self.walk_mode(level);
        let leg = |mode, from: &Pos, to: &Pos, via: Vec<[f32; 2]>| Leg {
            mode,
            start: 0.0,
            end: 0.0,
            from: point(from),
            to: point(to),
            via,
            level,
        };
        let walk_split = |length: f64| [(length - farming).max(0.0), farming.min(length), 0.0, 0.0];
        for (n, e) in events.iter().enumerate() {
            // When the next event of the step happens (or the step ends).
            let until = events[n + 1..].iter().map(|x| x.time).next().unwrap_or(end).max(e.time);
            match &e.event {
                Event::Shortcut { pass } => {
                    let p = world.passes[*pass].pos;
                    via.push([p.x as f32, p.y as f32]);
                }
                Event::Farm { .. } | Event::Reward { .. } => {}
                Event::Stop { stop, loc } => {
                    let walk = walking.min(e.time - t).max(0.0);
                    if apart(&at, &loc.pos) {
                        let l = leg(walk_mode, &at, &loc.pos, std::mem::take(&mut via));
                        push(out, walk, &mut t, walk_split(walk), Some(l));
                    }
                    let fight = d(|b| b.fighting) - d(|b| b.waiting) + d(|b| b.grinding) + d(|b| b.dungeons);
                    let split = match stop.kind {
                        Kind::Objective(_) | Kind::Dungeon | Kind::Accept => {
                            [0.0, fight.max(0.0), d(|b| b.overhead), d(|b| b.waiting)]
                        }
                        _ => [0.0, fight.max(0.0), (e.time - t - fight).max(0.0), 0.0],
                    };
                    push(out, e.time - t, &mut t, split, None);
                    at = loc.pos;
                }
                Event::Fly { from, to } => {
                    let (a, b) = (world.taxi_nodes[*from].pos, world.taxi_nodes[*to].pos);
                    if apart(&at, &a) {
                        let l = leg(walk_mode, &at, &a, std::mem::take(&mut via));
                        push(out, e.time - t, &mut t, [1.0, 0.0, 0.0, 0.0], Some(l));
                    }
                    let path = world.flight_path(*from, *to, before.known | after.known);
                    let inner = path[1..path.len() - 1]
                        .iter()
                        .map(|p| [p.x as f32, p.y as f32])
                        .collect();
                    push(
                        out,
                        until - t,
                        &mut t,
                        [1.0, 0.0, 0.0, 0.0],
                        Some(leg(LegMode::Flight, &a, &b, inner)),
                    );
                    at = b;
                }
                Event::Link { first, last } => {
                    let (a, b) = (world.links[*first].from, world.links[*last].to);
                    if apart(&at, &a) {
                        let l = leg(walk_mode, &at, &a, std::mem::take(&mut via));
                        push(out, e.time - t, &mut t, [1.0, 0.0, 0.0, 0.0], Some(l));
                    }
                    let ride = until - t;
                    // Waiting for the departure, unless it leaves at once (a portal, a lift).
                    let wait = if ride > 1.5 * params.link_wait {
                        params.link_wait
                    } else {
                        0.0
                    };
                    if wait > 0.0 {
                        push(out, wait, &mut t, [0.0, 0.0, 0.0, 1.0], None);
                    }
                    let mut inner = Vec::new();
                    if first != last && a.continent == world.links[*first].to.continent {
                        let (x, y) = (world.links[*first].to, world.links[*last].from);
                        inner.push([x.x as f32, x.y as f32]);
                        if y.continent == a.continent {
                            inner.push([y.x as f32, y.y as f32]);
                        }
                    }
                    push(
                        out,
                        ride - wait,
                        &mut t,
                        [1.0, 0.0, 0.0, 0.0],
                        Some(leg(LegMode::Link, &a, &b, inner)),
                    );
                    at = b;
                }
                Event::Hearth { inn, .. } => {
                    push(out, e.time - t, &mut t, [0.0, 0.0, 1.0, 0.0], None);
                    push(out, until - t, &mut t, [1.0, 0.0, 0.0, 0.0], None);
                    let to = model.inns[*inn].pos;
                    push(
                        out,
                        0.0,
                        &mut t,
                        [1.0, 0.0, 0.0, 0.0],
                        Some(leg(LegMode::Hearth, &at, &to, Vec::new())),
                    );
                    at = to;
                }
                Event::Teleport { to } => {
                    push(out, e.time - t, &mut t, [0.0, 0.0, 1.0, 0.0], None);
                    push(out, until - t, &mut t, [1.0, 0.0, 0.0, 0.0], None);
                    let to = model.teleports[*to].to.pos;
                    push(
                        out,
                        0.0,
                        &mut t,
                        [1.0, 0.0, 0.0, 0.0],
                        Some(leg(LegMode::Teleport, &at, &to, Vec::new())),
                    );
                    at = to;
                }
                Event::DeathSkip { pos, .. } => {
                    push(out, e.time - t, &mut t, [1.0, 0.0, 0.0, 0.0], None);
                    push(
                        out,
                        0.0,
                        &mut t,
                        [1.0, 0.0, 0.0, 0.0],
                        Some(leg(LegMode::Teleport, &at, pos, Vec::new())),
                    );
                    at = *pos;
                }
                Event::LearnFlight { node } => {
                    // A detour to a flight master at hand, and back.
                    let p = world.taxi_nodes[*node].pos;
                    let walk = (e.time - t - 5.0).max(0.0);
                    if apart(&at, &p) {
                        push(
                            out,
                            walk / 2.0,
                            &mut t,
                            [1.0, 0.0, 0.0, 0.0],
                            Some(leg(walk_mode, &at, &p, Vec::new())),
                        );
                        push(out, 5.0, &mut t, [0.0, 0.0, 1.0, 0.0], None);
                        push(
                            out,
                            e.time - t,
                            &mut t,
                            [1.0, 0.0, 0.0, 0.0],
                            Some(leg(walk_mode, &p, &at, Vec::new())),
                        );
                    } else {
                        push(out, e.time - t, &mut t, [0.0, 0.0, 1.0, 0.0], None);
                    }
                }
                Event::Service { kind, index } => {
                    let p = model.services[kind.index()][*index].pos;
                    let visit = self.planner.service_settings(*kind).3.min((e.time - t).max(0.0));
                    let walk = (e.time - t - visit).max(0.0);
                    if apart(&at, &p) {
                        let l = leg(walk_mode, &at, &p, std::mem::take(&mut via));
                        push(out, walk, &mut t, walk_split(walk), Some(l));
                        at = p;
                    }
                    push(out, e.time - t, &mut t, [0.0, 0.0, 1.0, 0.0], None);
                }
                Event::DungeonExit { .. } => {
                    push(out, e.time - t, &mut t, [0.0, 0.0, 1.0, 0.0], None);
                }
                Event::Train { trainer, .. } => {
                    let p = model.trainers[*trainer].pos;
                    let train = d(|b| b.training).min(e.time - t);
                    let walk = (e.time - t - train).max(0.0);
                    if !apart(&at, &p) {
                        push(out, e.time - t, &mut t, [0.0, 0.0, 1.0, 0.0], None);
                    } else if !apart(&after.pos, &at) {
                        // A detour: there and back.
                        push(
                            out,
                            walk / 2.0,
                            &mut t,
                            [1.0, 0.0, 0.0, 0.0],
                            Some(leg(walk_mode, &at, &p, Vec::new())),
                        );
                        push(out, train, &mut t, [0.0, 0.0, 1.0, 0.0], None);
                        push(
                            out,
                            e.time - t,
                            &mut t,
                            [1.0, 0.0, 0.0, 0.0],
                            Some(leg(walk_mode, &p, &at, Vec::new())),
                        );
                    } else {
                        let l = leg(walk_mode, &at, &p, std::mem::take(&mut via));
                        push(out, walk, &mut t, [1.0, 0.0, 0.0, 0.0], Some(l));
                        push(out, e.time - t, &mut t, [0.0, 0.0, 1.0, 0.0], None);
                        at = p;
                    }
                }
                Event::Grind { seconds, near, .. } | Event::GrindMobs { seconds, near, .. } => {
                    let walk = (e.time - t - seconds).max(0.0);
                    if apart(&at, near) {
                        let l = leg(walk_mode, &at, near, std::mem::take(&mut via));
                        push(out, walk, &mut t, [1.0, 0.0, 0.0, 0.0], Some(l));
                        at = *near;
                    }
                    push(out, e.time - t, &mut t, [0.0, 1.0, 0.0, 0.0], None);
                }
                Event::Abandon { .. } => push(out, e.time - t, &mut t, [0.0, 0.0, 1.0, 0.0], None),
            }
        }
        if end > t + 1e-6 {
            push(out, end - t, &mut t, [0.0, 0.0, 1.0, 0.0], None);
        }
    }

    /// What changes at each saved step: the character after the replayed step it is tied to,
    /// against the one after the last step tied before it.
    fn deltas(&self, saved: &[Step], of_saved: &[Option<usize>], step_of: &[usize]) -> Vec<Delta> {
        let mut out: Vec<Delta> = (0..saved.len()).map(|_| Delta::default()).collect();
        // Saved steps by the event they were written from, in the replay's order.
        let mut order: Vec<(usize, usize)> = (0..saved.len())
            .filter_map(|i| Some((self.lines[of_saved[i]?].event?, i)))
            .collect();
        order.sort_unstable();
        let mut last = 0;
        for (e, i) in order {
            let k = step_of[e];
            if k < last {
                continue;
            }
            let (a, b) = (&self.snaps[last], &self.snaps[(k + 1).min(self.snaps.len() - 1)]);
            out[i] = self.delta(a, b);
            last = k + 1;
        }
        out
    }

    fn delta(&self, a: &Snapshot, b: &Snapshot) -> Delta {
        let (model, world) = (self.planner.model, self.planner.world);
        let id = |q: u32| model.quests[q as usize].id;
        let before: HashMap<u32, u8> = a.log.iter().copied().collect();
        let now: HashMap<u32, u8> = b.log.iter().copied().collect();
        let mut done = Vec::new();
        for &(q, bits) in &b.log {
            let had = before.get(&q).copied().unwrap_or(0);
            for k in 0..8 {
                if bits & !had & (1 << k) != 0 {
                    done.push((id(q), k));
                }
            }
        }
        let learned = b.known & !a.known;
        Delta {
            accepted: b
                .log
                .iter()
                .filter(|(q, _)| !before.contains_key(q))
                .map(|&(q, _)| id(q))
                .collect(),
            removed: a
                .log
                .iter()
                .filter(|(q, _)| !now.contains_key(q))
                .map(|&(q, _)| id(q))
                .collect(),
            done,
            flights: (0..world.taxi_nodes.len())
                .filter(|&n| learned & (1u128 << n) != 0)
                .map(|n| world.taxi_nodes[n].id)
                .collect(),
            bind: (b.bind != a.bind).then(|| model.inns[b.bind].id),
            hearth: (b.hearth_ready - a.hearth_ready).abs() > 1e-9,
            recall: (b.recall_ready - a.recall_ready).abs() > 1e-9,
            cost: Some(b.spent.flight_cost - a.spent.flight_cost).filter(|&c| c > 0),
        }
    }

    /// The named place a step is done at, in the guide's language.
    fn place(&self, step: &Step) -> Option<String> {
        let w = step.world?;
        let (id, name) = self.planner.world.place(&pos_of(w))?;
        Some(self.names.get("zone", id, name).to_owned())
    }

    /// The timeline with what its steps refer to.
    fn timeline_of(&self, steps: Vec<TimelineStep>, legs: Vec<Leg>, matched: usize) -> Timeline {
        let (model, world, params) = (self.planner.model, self.planner.world, self.planner.params);
        let first = &self.snaps[0];
        let mut quests: Vec<i64> = first.log.iter().map(|&(q, _)| model.quests[q as usize].id).collect();
        for s in &steps {
            quests.extend(&s.accepted);
            quests.extend(s.done.iter().map(|d| d.0));
            quests.extend(&s.removed);
        }
        let mut seen = HashSet::new();
        quests.retain(|q| seen.insert(*q));
        let mut known = first.known;
        for s in &steps {
            for id in &s.flights {
                if let Some(n) = world.taxi_nodes.iter().position(|n| n.id == *id) {
                    known |= 1u128 << n;
                }
            }
        }
        let mut inns: Vec<usize> = vec![first.bind];
        for s in &steps {
            if let Some(id) = s.bind
                && let Some(i) = model.inns.iter().position(|l| l.id == id)
            {
                inns.push(i);
            }
        }
        let mut seen = HashSet::new();
        inns.retain(|i| seen.insert(*i));
        let rules = self.planner.rules();
        Timeline {
            start: StartView {
                at: point(&first.pos),
                log: first.log.iter().map(|&(q, _)| model.quests[q as usize].id).collect(),
                done: first
                    .log
                    .iter()
                    .flat_map(|&(q, bits)| {
                        (0..8u8)
                            .filter(move |k| bits & (1 << k) != 0)
                            .map(move |k| (model.quests[q as usize].id, k))
                    })
                    .collect(),
                flights: (0..world.taxi_nodes.len())
                    .filter(|&n| first.known & (1u128 << n) != 0)
                    .map(|n| world.taxi_nodes[n].id)
                    .collect(),
                bind: model.inns.get(first.bind).map(|l| l.id),
                recall: model.recall.is_some(),
            },
            quests: quests.iter().filter_map(|id| self.quest_info(*id)).collect(),
            nodes: (0..world.taxi_nodes.len())
                .filter(|&n| known & (1u128 << n) != 0)
                .map(|n| {
                    let node = &world.taxi_nodes[n];
                    NodeInfo {
                        id: node.id,
                        name: node.name.clone(),
                        at: point(&node.pos),
                    }
                })
                .collect(),
            inns: inns
                .iter()
                .filter_map(|&i| model.inns.get(i))
                .map(|l| InnInfo {
                    id: l.id,
                    name: self.names.get("npc", l.id, &l.name).to_owned(),
                    place: world.place(&l.pos).map_or_else(
                        || self.names.get("zone", l.zone, world.zone_name(l.zone)).to_owned(),
                        |(id, name)| self.names.get("zone", id, name).to_owned(),
                    ),
                    at: point(&l.pos),
                })
                .collect(),
            xp_levels: (0..=rules.max_level).map(|l| rules.to_next_level(l.max(1))).collect(),
            log_size: params.quest_log_size,
            mount_level: params.mount_level,
            matched,
            steps,
            legs,
        }
    }

    fn quest_info(&self, id: i64) -> Option<QuestInfo> {
        let model = self.planner.model;
        let q = &model.quests[*model.index.get(&id)?];
        Some(QuestInfo {
            id,
            name: self.names.get("quest", id, &q.name).to_owned(),
            objectives: q
                .objectives
                .iter()
                .map(|o| ObjectiveInfo {
                    text: export::objective_text(o, self.names),
                    kind: match o.loc.kind {
                        EntityKind::Npc if o.kills > 0.0 => "kill",
                        EntityKind::Npc => "talk",
                        EntityKind::Item => "collect",
                        EntityKind::Object => "use",
                        _ => "other",
                    },
                    count: o.count.round().max(1.0) as u32,
                })
                .collect(),
        })
    }
}

/// What changes at a step (see `TimelineStep`): the hearthstone and Astral Recall used, cast
/// at its end (their events are recorded when cast).
#[derive(Default)]
struct Delta {
    accepted: Vec<i64>,
    removed: Vec<i64>,
    done: Vec<(i64, u8)>,
    flights: Vec<i64>,
    bind: Option<i64>,
    hearth: bool,
    recall: bool,
    cost: Option<i64>,
}

/// Steps found this far from where they would be are the same done in another order.
const SWAP: isize = 3;

/// What tells a step from another: its kind, quest, objective and target.
fn key(s: &Step) -> (StepKind, Option<i64>, Option<u8>, Option<i64>, bool) {
    (s.kind, s.quest, s.objective, s.target.as_ref().map(|t| t.id), s.inside)
}

/// The saved steps and the replayed ones that are the same (index pairs, in order): all of them
/// when the replay wrote the guide again, else the longest common sequence.
fn align(saved: &[Step], lines: &[Step]) -> Vec<(usize, usize)> {
    if saved.len() == lines.len() && saved.iter().zip(lines).all(|(a, b)| key(a) == key(b)) {
        return (0..saved.len()).map(|i| (i, i)).collect();
    }
    // The same start and end, then the longest common sequence of the rest.
    let head = saved.iter().zip(lines).take_while(|(a, b)| key(a) == key(b)).count();
    let tail = saved[head..]
        .iter()
        .rev()
        .zip(lines[head..].iter().rev())
        .take_while(|(a, b)| key(a) == key(b))
        .count();
    let (a, b) = (&saved[head..saved.len() - tail], &lines[head..lines.len() - tail]);
    let mut pairs: Vec<(usize, usize)> = (0..head).map(|i| (i, i)).collect();
    let (n, m) = (a.len(), b.len());
    if n * m <= 30_000_000 {
        let mut table = vec![0u16; (n + 1) * (m + 1)];
        let at = |i: usize, j: usize| i * (m + 1) + j;
        for i in (0..n).rev() {
            for j in (0..m).rev() {
                table[at(i, j)] = if key(&a[i]) == key(&b[j]) {
                    table[at(i + 1, j + 1)] + 1
                } else {
                    table[at(i + 1, j)].max(table[at(i, j + 1)])
                };
            }
        }
        let (mut i, mut j) = (0, 0);
        while i < n && j < m {
            if key(&a[i]) == key(&b[j]) {
                pairs.push((head + i, head + j));
                (i, j) = (i + 1, j + 1);
            } else if table[at(i + 1, j)] >= table[at(i, j + 1)] {
                i += 1;
            } else {
                j += 1;
            }
        }
    }
    pairs.extend((0..tail).map(|k| (saved.len() - tail + k, lines.len() - tail + k)));
    // Steps done at the same time in another order (a flight path learned in passing, before
    // or after the stop it is next to).
    let (mut a, mut b) = (vec![false; saved.len()], vec![false; lines.len()]);
    for &(i, j) in &pairs {
        (a[i], b[j]) = (true, true);
    }
    pairs.sort_unstable();
    let mut swapped = Vec::new();
    for i in (0..saved.len()).filter(|&i| !a[i]) {
        // Where it would be in the replay: as far from the step tied before it.
        let before = pairs.partition_point(|p| p.0 < i);
        let expected = before.checked_sub(1).map_or(i, |k| pairs[k].1 + (i - pairs[k].0)) as isize;
        let same = (expected - SWAP..=expected + SWAP)
            .filter_map(|j| usize::try_from(j).ok())
            .filter(|&j| j < lines.len() && !b[j] && key(&lines[j]) == key(&saved[i]))
            .min_by_key(|&j| (j as isize - expected).abs());
        if let Some(j) = same {
            b[j] = true;
            swapped.push((i, j));
        }
    }
    pairs.extend(swapped);
    pairs.sort_unstable();
    pairs
}

/// Add a piece of `length` seconds at `t` (moved to its end), its time split as `split` says
/// (shares).
fn push(out: &mut Vec<Piece>, length: f64, t: &mut f64, split: [f64; 4], leg: Option<Leg>) {
    let length = length.max(0.0);
    let total: f64 = split.iter().sum();
    let split = if total > 1e-9 {
        split.map(|s| s * length / total)
    } else {
        [0.0, 0.0, length, 0.0]
    };
    out.push(Piece {
        start: *t,
        end: *t + length,
        split,
        leg: leg.map(|mut l| {
            (l.start, l.end) = (*t, *t + length);
            l
        }),
    });
    *t += length;
}

/// A world point as the guide's steps give it (continent, x, y).
fn pos_of(w: [f64; 3]) -> Pos {
    Pos {
        continent: w[0] as i64,
        x: w[1],
        y: w[2],
        zone: 0,
    }
}
