//! The planning report: how many quests can be planned, and why the others are left out.

use crate::model::Model;
use crate::world::World;
use std::collections::BTreeMap;

pub(super) fn model_notes(model: &Model, world: &World, progress: &(dyn Fn(&str) + Sync)) -> Vec<String> {
    list_skipped_for_debugging(model, progress);
    let mut notes = vec![format!(
        "{} plannable quests, start at {} ({})",
        model.quests.len(),
        model.start.name,
        world.zone_name(model.start.zone)
    )];
    let mut reasons: BTreeMap<&str, usize> = BTreeMap::new();
    for (_, _, reason) in &model.skipped {
        *reasons.entry(reason).or_default() += 1;
    }
    for (reason, n) in reasons {
        notes.push(format!("skipped {n}: {reason}"));
    }
    notes
}

/// FG_LIST_SKIPPED=<reason> lists the quests left out for that reason, FG_DEBUG_SKIP=<ids> why
/// these quests are.
fn list_skipped_for_debugging(model: &Model, progress: &(dyn Fn(&str) + Sync)) {
    if let Ok(reason) = std::env::var("FG_LIST_SKIPPED") {
        for (id, name, why) in &model.skipped {
            if why.contains(reason.as_str()) {
                progress(&format!("skipped {id} {name}: {why}"));
            }
        }
    }
    if let Ok(ids) = std::env::var("FG_DEBUG_SKIP") {
        for (id, name, reason) in &model.skipped {
            if ids.split(',').any(|x| x.trim() == id.to_string()) {
                progress(&format!("skipped {name} ({id}): {reason}"));
            }
        }
    }
}
