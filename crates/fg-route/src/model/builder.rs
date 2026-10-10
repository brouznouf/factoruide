//! Turns a quest row of the database into a quest the planner can do, or the reason why not.
//! Each decision lives in its own file: selection, givers, objectives, item, quest.

use super::sources::{QuestRow, SourceKind, Sources};
use super::types::{EntityKind, Loc, Profile, Quest};
use crate::params::Params;
use crate::world::{Pos, World};
use std::collections::HashSet;

/// Why a quest is left out (shown in the planning report).
pub(super) type Skip = &'static str;

pub(super) struct QuestBuilder<'a> {
    pub(super) sources: &'a Sources,
    pub(super) world: &'a World,
    pub(super) profile: &'a Profile,
    pub(super) params: &'a Params,
    pub(super) excluded: HashSet<i64>,
    pub(super) excluded_sorts: HashSet<i64>,
    /// Quest items each group member must loot for themselves (unless drops are shared).
    pub(super) per_player: f64,
}

impl<'a> QuestBuilder<'a> {
    pub(super) fn new(sources: &'a Sources, world: &'a World, profile: &'a Profile, params: &'a Params) -> Self {
        Self {
            sources,
            world,
            profile,
            params,
            excluded: params.excluded_quests.iter().copied().collect(),
            excluded_sorts: params.excluded_sorts.iter().copied().collect(),
            per_player: if params.group_shared_drops {
                1.0
            } else {
                params.group_size.clamp(1, 5) as f64
            },
        }
    }

    /// The quest, None when it does not concern the character (race, class, levels), or why it
    /// is left out.
    pub(super) fn build(&self, row: &QuestRow) -> Result<Option<Quest>, Skip> {
        if !self.concerns(row) {
            return Ok(None);
        }
        let skill = self.skill(row);
        self.check_rules(row, skill)?;
        let givers = self.givers(row)?;
        let objectives = self.objectives(row, &givers)?;
        Ok(Some(self.assemble(row, skill, givers, objectives)))
    }

    /// Dungeon a giver, a mob or an object lives in.
    pub(super) fn dungeon_of(&self, kind: EntityKind, id: i64) -> Option<usize> {
        let kind = if kind == EntityKind::Object {
            EntityKind::Object
        } else {
            EntityKind::Npc
        };
        self.sources.inside.get(&(kind, id)).copied()
    }

    pub(super) fn has_points(&self, kind: EntityKind, id: i64) -> bool {
        !self.sources.entities.points(self.world, kind, id).is_empty()
    }

    /// Where an item comes from: its own sources, the sources of the containers it is found in,
    /// or (for assembled items) those of the parts the quest needs.
    pub(super) fn item_sources(&self, item: i64, parts: &[i64]) -> Vec<(SourceKind, i64)> {
        let sources = &self.sources.item_origins;
        let expand = |item: i64| -> Vec<(SourceKind, i64)> {
            let mut out = Vec::new();
            for (t, id) in sources.get(&item).into_iter().flatten() {
                if *t == SourceKind::Item {
                    out.extend(
                        sources
                            .get(id)
                            .into_iter()
                            .flatten()
                            .filter(|(t, _)| *t != SourceKind::Item)
                            .copied(),
                    );
                } else {
                    out.push((*t, *id));
                }
            }
            out
        };
        let direct = expand(item);
        if !direct.is_empty() {
            return direct;
        }
        parts.iter().filter(|p| **p != item).flat_map(|p| expand(*p)).collect()
    }

    /// Where to meet an NPC or an object: its main spawn cluster nearest to `near`, or the
    /// entrance of the dungeon it lives in. NPCs hostile to the character do not talk to it.
    pub(super) fn locate(&self, kind: EntityKind, id: i64, near: Option<&Pos>) -> Option<Loc> {
        let entities = &self.sources.entities;
        if let Some(d) = self.dungeon_of(kind, id) {
            let mut loc = self.sources.dungeons[d].entrance.clone();
            loc.kind = kind;
            loc.id = id;
            loc.name = entities.name(kind, id);
            return Some(loc);
        }
        if kind == EntityKind::Npc && entities.hostile.contains(&id) {
            return None;
        }
        let points = entities.points(self.world, kind, id);
        let (pos, zone, _) = super::entities::best_cluster(&points, near)?;
        Some(Loc {
            pos,
            zone,
            kind,
            id,
            name: entities.name(kind, id),
        })
    }
}
