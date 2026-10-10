//! A crowded server (a launch, see `Params::crowded`): what many players want at the same time
//! takes waiting. Escorts and events run for one player (or group) at a time; mobs and objects
//! with few spawns are taken by others before you. The wait is added to the objective, so the
//! planner leaves these quests out when others pay better.

use super::builder::QuestBuilder;
use super::sources::QuestRow;
use super::types::{EntityKind, Objective};

/// Spawns under which a target is fought over in full (fewer: the same; more: less wait).
const FEW_SPAWNS: f64 = 4.0;

impl QuestBuilder<'_> {
    /// Add the wait of a crowded server to the quest's objectives.
    pub(super) fn crowd(&self, r: &QuestRow, objectives: &mut [Objective]) {
        let p = self.params;
        if !p.crowded {
            return;
        }
        // Starting zones are the most crowded: full wait up to `crowd_level`, half above.
        let share = if r.level.unwrap_or(1) <= p.crowd_level {
            1.0
        } else {
            0.5
        };
        for o in objectives.iter_mut().filter(|o| o.dungeon.is_none()) {
            if o.loc.kind == EntityKind::Area && is_event(r, &o.text) {
                o.extra += share * p.crowd_event_time;
            } else if o.kills > 0.0 || o.uses > 0.0 {
                let Some(spawns) = self.spawns(o) else { continue };
                let contested = (FEW_SPAWNS / spawns.max(1) as f64).min(1.0);
                o.extra += share * p.crowd_spawn_time * contested * o.count;
            }
        }
    }

    /// How many places the objective's mobs or objects spawn at.
    fn spawns(&self, o: &Objective) -> Option<usize> {
        if let Some(s) = &o.spots {
            return Some(s.points.len());
        }
        let entities = &self.sources.entities;
        if !o.mobs.is_empty() {
            return Some(
                o.mobs
                    .iter()
                    .map(|(id, _)| entities.points(self.world, EntityKind::Npc, *id).len())
                    .sum(),
            );
        }
        (o.loc.kind == EntityKind::Object).then(|| entities.points(self.world, EntityKind::Object, o.loc.id).len())
    }
}

/// An escort or an event (defend someone, overhear a talk): the quest is flagged as one, or its
/// objective says so.
fn is_event(r: &QuestRow, text: &str) -> bool {
    r.flags.is_some_and(|f| f & 2 != 0) || ["Escort", "Protect", "Defend"].iter().any(|w| text.starts_with(w))
}
