//! The planning report: how many quests can be planned, and why the others are left out.

use super::{JobEvent, Watch};
use crate::diagnostics::Diagnostics;
use crate::model::Model;
use crate::world::World;
use std::collections::BTreeMap;

pub(super) fn model_notes(model: &Model, world: &World) -> Vec<String> {
    let start = match model.initial.pos {
        Some((pos, zone)) => format!(
            "where the character stands ({})",
            world.place_name(&pos).unwrap_or_else(|| world.zone_name(zone))
        ),
        None => format!("at {} ({})", model.start.name, world.zone_name(model.start.zone)),
    };
    let mut notes = vec![format!("{} plannable quests, start {start}", model.quests.len())];
    let mut reasons: BTreeMap<&str, usize> = BTreeMap::new();
    for (_, _, reason) in &model.skipped {
        *reasons.entry(reason).or_default() += 1;
    }
    for (reason, n) in reasons {
        notes.push(format!("skipped {n}: {reason}"));
    }
    notes
}

/// The quests left out for the reason `diagnostics.skipped_for` asks, and why those of
/// `diagnostics.skipped` are.
pub(super) fn list_skipped(model: &Model, diagnostics: &Diagnostics, progress: Watch<'_>) {
    if let Some(reason) = &diagnostics.skipped_for {
        for (id, name, why) in &model.skipped {
            if why.contains(reason.as_str()) {
                progress(&JobEvent::Status(&format!("skipped {id} {name}: {why}")));
            }
        }
    }
    for (id, name, reason) in &model.skipped {
        if diagnostics.skipped.contains(id) {
            progress(&JobEvent::Status(&format!("skipped {name} ({id}): {reason}")));
        }
    }
}
