//! Protected reputation programmes retain their executable metadata through every export.

use super::phrase::{Arg, Phrase};
use super::{Step, StepKind, Target, TargetKind};
use crate::reputation::{RepProgram, SourceLocation};
use crate::world::{MapPoint, Pos, World};

pub(super) fn render_program(program: &mut RepProgram, names: &crate::model::Names) {
    let localize = |id: Option<i64>, name: &mut Option<String>| {
        if let (Some(id), Some(name)) = (id, name) {
            *name = names.get("npc", id, name).to_owned();
        }
    };
    for source in &mut program.sources {
        if let Some(id) = source.identity.quest_id {
            source.name = names.get("quest", id, &source.name).to_owned();
        } else if let Some(id) = source.identity.npc_id {
            source.name = names.get("npc", id, &source.name).to_owned();
        }
        let identity = &mut source.identity;
        localize(identity.npc_id, &mut identity.npc_name);
        localize(identity.giver_npc_id, &mut identity.giver_npc_name);
        localize(identity.ender_npc_id, &mut identity.ender_npc_name);
        for acquisition in &mut source.acquisition {
            localize(acquisition.npc_id, &mut acquisition.npc_name);
        }
    }
    for action in program.phases.iter_mut().flat_map(|phase| &mut phase.actions) {
        if let Some(acquisition) = &mut action.acquisition {
            localize(acquisition.npc_id, &mut acquisition.npc_name);
        }
    }
}

pub(super) fn write_step(step: &mut Step, program: &RepProgram, world: &World) -> Option<(MapPoint, Pos)> {
    step.kind = StepKind::Reputation;
    step.keep = true;
    step.reputation_program = Some(Box::new(program.clone()));
    let actions = program.phases.iter().map(|p| p.actions.len()).sum::<usize>();
    step.say = vec![Phrase::new("reputation.program").arg("n", Arg::Num(actions as i64))];
    let action = program.phases.iter().flat_map(|p| &p.actions).next()?;
    let source = program.sources.iter().find(|s| s.id == action.source_id)?;
    let identity = &source.identity;
    let (id, name) = if let Some(id) = identity.npc_id {
        (id, identity.npc_name.as_ref().unwrap_or(&source.name))
    } else if let Some(id) = identity.ender_npc_id {
        (id, identity.ender_npc_name.as_ref().unwrap_or(&source.name))
    } else {
        (0, &source.name)
    };
    if id > 0 {
        step.target = Some(Target {
            kind: TargetKind::Npc,
            id,
            name: name.clone(),
        });
    }
    let location = action.location.as_ref().or(source.location.as_ref())?;
    let pos = position(location)?;
    let map = if let (Some(ui_map), Some(x), Some(y)) = (location.map_id, location.map_x, location.map_y) {
        Some(MapPoint {
            zone: location.zone_id.unwrap_or(0),
            ui_map,
            x,
            y,
        })
    } else {
        world.to_map(location.zone_id.unwrap_or(0), &pos)
    }?;
    Some((map, pos))
}

fn position(location: &SourceLocation) -> Option<Pos> {
    Some(Pos {
        continent: i64::from(location.world_id?),
        x: location.x?,
        y: location.y?,
        zone: location.zone_id.unwrap_or(0),
        z: crate::world::height(location.z),
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::reputation::{RepPhase, RepPhaseKind};

    #[test]
    fn reputation_program_survives_json_with_circuit_counts_and_fractional_totals() {
        let mut program = RepProgram {
            version: 1,
            ..RepProgram::default()
        };
        program.phases.push(RepPhase {
            id: "daily".into(),
            kind: RepPhaseKind::DailyCircuit,
            cycles: 3,
            actions: vec![],
        });
        program.result.reached.insert(68, 99.75);
        let mut step = Step::bare(StepKind::Reputation, 60, 123.0);
        step.keep = true;
        step.reputation_program = Some(Box::new(program));
        let saved = serde_json::to_value(&step).unwrap();
        let restored: Step = serde_json::from_value(saved).unwrap();
        let restored = restored.reputation_program.unwrap();
        assert_eq!(restored.phases[0].cycles, 3);
        assert_eq!(restored.phases[0].kind, RepPhaseKind::DailyCircuit);
        assert_eq!(restored.result.reached[&68], 99.75);
        assert!(step.keep);
    }
}
