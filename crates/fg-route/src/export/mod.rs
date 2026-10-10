//! Turn a planned route into a guide: steps for humans (JSON) and for the addon (Lua).

mod along;
mod checkpoints;
mod grind_spots;
mod language;
mod lua;
mod text;
mod types;
mod writer;

pub use lua::to_lua;
pub use types::{Checkpoint, MapPointOut, Route, RouteProfession, Step, StepKind, Target, TargetKind};

use crate::model::Profile;
use crate::plan::{Breakdown, Event, Kind, Planner, Stop, Timed, fmt_time};
use language::Language;
use writer::StepWriter;

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
    let (hidden, announced) = along::hidden_along(trace);
    let writer = StepWriter {
        planner,
        route,
        lang: Language::new(&planner.model.names),
        background: planner.background_objectives(route),
        announced,
    };
    let mut steps = Vec::new();
    // XP the route has after each step (in its level).
    let mut xps = Vec::new();
    let mut farmed = 0;
    for (n, t) in trace.iter().enumerate() {
        if hidden.contains(&n) {
            continue;
        }
        match &t.event {
            Event::Reward { quest, item } => writer.add_reward(&mut steps, *quest, *item),
            Event::Farm { kills, .. } => farmed += kills,
            _ => {
                let mut step = writer.step(n, t);
                if farmed > 0 {
                    writer.add_farm(&mut step, farmed);
                    farmed = 0;
                }
                steps.push(step);
                xps.push(t.xp);
            }
        }
    }
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
    route_of(planner, profile, route, (total, breakdown), checkpoints, steps)
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
        spells: model
            .power
            .as_ref()
            .map(|p| p.ranks().map(|r| r.id).collect())
            .unwrap_or_default(),
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
        steps,
    }
}
