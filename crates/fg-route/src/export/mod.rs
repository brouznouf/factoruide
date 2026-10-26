//! Turn a planned route into a guide: steps for humans (JSON) and for the addon (Lua).

mod checkpoints;
pub mod diff;
pub mod fgstring;
pub mod format;
mod grind_spots;
pub mod groups;
mod lua;
mod phrase;
pub mod polestar;
mod templates;
mod types;
mod vendors;
mod writer;

pub use format::{FORMAT, read_route};
pub use lua::{lua_str, to_lua};
pub use phrase::{Arg, Phrase};
pub use templates::Lang;
pub use types::{
    Checkpoint, MapPointOut, Player, ReputationReached, Route, RouteProfession, Step, StepKind, Target, TargetKind,
};

use crate::model::{Names, Profile};
use crate::plan::{Breakdown, Event, Kind, Planner, Stop, Timed, fmt_time};
use std::collections::HashSet;
use writer::StepWriter;

/// The guide written in the language of `names`: its sentences, and the names of its quests,
/// targets, mobs, zones and professions. A guide of format 1 (texts only) keeps its texts.
pub fn render(route: &Route, names: &Names) -> Route {
    let mut out = route.clone();
    let lang = Lang::of(&names.locale);
    for step in out.steps.iter_mut().filter(|s| !s.say.is_empty()) {
        render_step(step, names);
    }
    for p in &mut out.professions {
        if let Some(name) = templates::profession_name(&p.key, lang) {
            p.name = name.to_owned();
        }
    }
    out.locale = Some(names.locale.clone());
    out
}

pub fn build(planner: &Planner<'_>, profile: &Profile, route: &[Stop]) -> Route {
    let mut trace = Vec::new();
    let (total, breakdown) = planner
        .simulate_full(route, Some(&mut trace))
        .unwrap_or((f64::INFINITY, Breakdown::default()));
    // Show the real play time; the optimizer score also counts penalties.
    let total = if total.is_finite() { breakdown.total } else { total };
    build_traced(planner, profile, route, &trace, total, breakdown)
}

/// Route from an already simulated trace (`total`: real play time): a step per event, but the
/// mobs met on the way that deserve no line of their own, the rewards picked added to their
/// turn-ins, the mobs farmed on the way to the next step, and a step at each checkpoint.
pub fn build_traced(
    planner: &Planner<'_>,
    profile: &Profile,
    route: &[Stop],
    trace: &[Timed],
    total: f64,
    breakdown: Breakdown,
) -> Route {
    let writer = StepWriter {
        planner,
        route,
        background: planner.background_objectives(route),
    };
    let mut steps = Vec::new();
    // XP the route has after each step (in its level).
    let mut xps = Vec::new();
    // The quests of the character's log the guide leaves are abandoned first.
    let model = planner.model;
    let left: HashSet<i64> = planner
        .left_at_start(&planner.planned(route))
        .map(|i| model.quests[i].id)
        .collect();
    // Those it holds are kept; one with nothing done (not held) too when the guide takes it itself.
    let planned = planner.planned(route);
    let kept = |id: &i64| {
        model.index.get(id).is_some_and(|i| {
            if model.initial.accepted.contains(i) {
                !left.contains(id)
            } else {
                planned.flags[*i] & 1 != 0
            }
        })
    };
    if let Some(first) = trace.first() {
        for (id, name) in model.initial.log.iter().filter(|(id, _)| !kept(id)) {
            steps.push(writer.abandon_unused(first, *id, name));
            xps.push(planner.model.initial.xp);
        }
    }
    // Mobs farmed on the way to the next step, and their XP.
    let (mut farmed, mut farmed_xp) = (0, 0);
    for (n, t) in trace.iter().enumerate() {
        match &t.event {
            Event::Reward { quest, item } => writer.add_reward(&mut steps, *quest, *item),
            Event::Farm { kills, xp, .. } => {
                farmed += kills;
                farmed_xp += xp;
            }
            _ => {
                let mut step = writer.step(t);
                step.event = Some(n);
                if farmed > 0 {
                    StepWriter::add_farm(&mut step, farmed, farmed_xp);
                    (farmed, farmed_xp) = (0, 0);
                }
                let run = match &t.event {
                    Event::Stop { stop, .. } if stop.kind == Kind::Dungeon => Some(stop.index as usize),
                    _ => None,
                };
                let inside = run.map(|d| writer.dungeon_quests(d, &step)).unwrap_or_default();
                let Some(d) = run.filter(|_| !inside.is_empty()) else {
                    steps.push(step);
                    xps.push(t.xp);
                    continue;
                };
                // The run: entered after the way there with the level and XP of the step before,
                // its quests done in turn until its end.
                let (start, level, xp) = steps
                    .last()
                    .zip(xps.last())
                    .map_or((0.0, t.level, t.xp), |(s, x)| (s.time, s.level, *x));
                let entered = (t.time - planner.model.dungeons[d].minutes * 60.0).max(start);
                (step.time, step.level) = (entered, level);
                // A quest started by a boss's drop is accepted in the run, not before it.
                let from_boss = |s: &Step| {
                    s.kind == StepKind::Accept
                        && (planner.model.quests.iter()).any(|q| Some(q.id) == s.quest && q.start_dungeon == Some(d))
                };
                let mut looted = Vec::new();
                while steps.last().is_some_and(from_boss) {
                    let mut accept = steps.pop().expect("the loop checked the last step");
                    xps.pop();
                    (accept.time, accept.level, accept.inside) = (entered, level, true);
                    looted.insert(0, accept);
                }
                steps.push(step);
                xps.push(xp);
                for accept in looted {
                    steps.push(accept);
                    xps.push(xp);
                }
                let count = inside.len() as f64;
                for (k, mut quest) in inside.into_iter().enumerate() {
                    quest.time = entered + (t.time - entered) * (k as f64 + 1.0) / count;
                    steps.push(quest);
                    xps.push(t.xp);
                }
            }
        }
    }
    // Items in limited supply offered at the other vendors passed by on the way.
    vendors::add_tries(&writer, &mut steps, &mut xps);
    let mut checkpoints = checkpoints::checkpoints(planner, profile, &mut steps);
    // A step at each checkpoint (the character grinds there when behind the route), grinds
    // where mobs of the character's level live.
    grind_spots::place(
        &writer,
        &mut grind_spots::Guide {
            steps: &mut steps,
            xps: &mut xps,
            checkpoints: &mut checkpoints,
        },
    );
    for (step, xp) in steps.iter_mut().zip(&xps) {
        step.route_xp = Some(*xp);
    }
    // The texts in the language the guide is made in (other languages: `render`).
    for step in &mut steps {
        step.text = phrase::render_all(&step.say, &planner.model.names);
    }
    route_of(planner, profile, route, (total, breakdown), checkpoints, steps)
}

/// The sentence an objective's step says, in the language of `names`.
pub(crate) fn objective_text(objective: &crate::model::Objective, names: &Names) -> String {
    writer::objective_phrase(objective).render(names)
}

/// The line of the guide each event of a replayed route writes, in the language of `names`
/// (for the guide editor), with its event and the XP the route has then in its level. None for
/// the events without a line of their own: a reward picked, mobs farmed on the way.
pub(crate) fn event_steps(planner: &Planner<'_>, route: &[Stop], trace: &[Timed], names: &Names) -> Vec<Option<Step>> {
    let writer = StepWriter {
        planner,
        route,
        background: planner.background_objectives(route),
    };
    trace
        .iter()
        .enumerate()
        .map(|(n, t)| {
            if matches!(t.event, Event::Reward { .. } | Event::Farm { .. }) {
                return None;
            }
            let mut step = writer.step(t);
            render_step(&mut step, names);
            step.event = Some(n);
            step.route_xp = Some(t.xp);
            Some(step)
        })
        .collect()
}

/// A step written in the language of `names`: its sentence, and the names of its quest,
/// target, mobs and zone.
fn render_step(step: &mut Step, names: &Names) {
    step.text = phrase::render_all(&step.say, names);
    if let (Some(id), Some(name)) = (step.quest, &step.quest_name) {
        step.quest_name = Some(names.get("quest", id, name).to_owned());
    }
    if let (Some(id), Some(name)) = (step.child, &step.child_name) {
        step.child_name = Some(names.get("quest", id, name).to_owned());
    }
    if let Some(t) = &mut step.target {
        let kind = match t.kind {
            TargetKind::Npc => "npc",
            TargetKind::Object => "object",
            TargetKind::Item => "item",
            TargetKind::Dungeon => "zone",
            _ => "",
        };
        t.name = names.get(kind, t.id, &t.name).to_owned();
    }
    for (name, id) in step.mobs.iter_mut().zip(&step.mob_ids) {
        *name = names.get("npc", *id, name).to_owned();
    }
    if let (Some(id), Some(zone)) = (step.zone_id, &step.zone) {
        step.zone = Some(names.get("zone", id, zone).to_owned());
    }
}

fn route_of(
    planner: &Planner<'_>,
    profile: &Profile,
    route: &[Stop],
    (total, breakdown): (f64, Breakdown),
    checkpoints: Vec<Checkpoint>,
    steps: Vec<Step>,
) -> Route {
    let model = &planner.model;
    Route {
        format: FORMAT,
        name: profile.name.clone(),
        race_id: profile.race_id,
        class_id: profile.class_id,
        class: profile.class_name.clone(),
        faction: profile.faction,
        from_level: profile.from_level,
        to_level: profile.to_level,
        total_time: total.round(),
        total_time_text: fmt_time(total),
        quests: route.iter().filter(|s| s.kind == Kind::TurnIn).count(),
        breakdown,
        locale: Some(model.names.locale.clone()),
        spells: {
            let mut spells: Vec<_> = model
                .power
                .as_ref()
                .map(|p| p.ranks().map(|r| r.id).collect())
                .unwrap_or_default();
            spells.extend(model.abilities.spells());
            spells.sort_unstable();
            spells.dedup();
            spells
        },
        professions: model
            .professions
            .iter()
            .map(|p| RouteProfession {
                key: p.key.clone(),
                name: p.name.clone(),
                skill_line: crate::profession::skill_line(&p.key),
                target: p.target,
                start: p.start,
                curve: p.curve.clone(),
            })
            .collect(),
        checkpoints,
        reputation: reputation_reached(planner, route),
        players: if profile.mates.is_empty() {
            Vec::new()
        } else {
            std::iter::once(Player {
                name: profile.player.clone(),
                race_id: profile.race_id,
                class_id: profile.class_id,
            })
            .chain(profile.mates.iter().map(|m| Player {
                name: m.name.clone(),
                race_id: m.race_id,
                class_id: m.class_id,
            }))
            .collect()
        },
        sync_max: (!profile.mates.is_empty()).then_some(planner.params.group_sync_max),
        class_spells: {
            let mut out: Vec<(i64, Vec<i64>)> = Vec::new();
            for (m, spells) in profile.mates.iter().zip(&model.mate_spells) {
                if m.class_id != profile.class_id && !spells.is_empty() && out.iter().all(|(c, _)| *c != m.class_id) {
                    out.push((m.class_id, spells.clone()));
                }
            }
            out
        },
        steps,
    }
}

fn reputation_reached(planner: &Planner<'_>, route: &[Stop]) -> Vec<ReputationReached> {
    let goals = &planner.params.reputation_goals;
    if goals.is_empty() {
        return Vec::new();
    }
    let reached = planner.reputation_reached(route);
    let model = &planner.model;
    goals
        .iter()
        .enumerate()
        .map(|(g, goal)| {
            let start = model.initial.reputation.get(g).copied().unwrap_or(0.0);
            ReputationReached {
                faction: goal.faction,
                name: model.faction_names.get(&goal.faction).cloned().unwrap_or_default(),
                start: start.round() as i64,
                reached: reached.get(g).copied().unwrap_or(start).round() as i64,
                target: goal.target,
            }
        })
        .collect()
}
