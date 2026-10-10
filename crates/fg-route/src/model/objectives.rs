//! What a quest asks: one objective per row, built by its kind (kill or talk to an NPC, use an
//! object, get an item, explore an area), inside a dungeon, or won in battlegrounds.

use super::builder::{QuestBuilder, Skip};
use super::entities::best_cluster;
use super::givers::Givers;
use super::sources::{ObjectiveKind, QuestRow, SourceKind, asks_for_work, count_for, on_reachable, parse_requirements};
use super::types::{EntityKind, Loc, Objective, Spots};
use crate::world::Pos;
use std::collections::HashMap;

/// One objective row: kind, target, text, extra data, exact amount.
pub(super) type ObjectiveRow = (ObjectiveKind, Option<i64>, Option<String>, Option<String>, Option<i64>);

/// What every objective of a quest is built from.
pub(super) struct QuestContext<'r> {
    pub(super) row: &'r QuestRow,
    pub(super) level: i64,
    /// Where the quest is taken: objectives found nowhere are put there when approximating.
    pub(super) anchor: Pos,
    pub(super) zone: i64,
    pub(super) approx: Option<(Pos, i64, usize)>,
    pub(super) requirements: Vec<(String, f64)>,
    /// Items an assembled item is made of.
    pub(super) parts: Vec<i64>,
}

impl Objective {
    /// An objective without kills nor uses, with the fields that do not matter left empty.
    pub(super) fn plain(loc: Loc, text: String, extra: f64) -> Self {
        Self {
            loc,
            text,
            kills: 0.0,
            mob_level: 0,
            uses: 0.0,
            extra,
            dungeon: None,
            elite: false,
            count: 1.0,
            mobs: vec![],
            pull: 1.0,
            spots: None,
        }
    }
}

impl QuestBuilder<'_> {
    pub(super) fn objectives(&self, r: &QuestRow, givers: &Givers) -> Result<Vec<Objective>, Skip> {
        let quest = self.context(r, givers);
        let rows = self.sources.objectives.get(&r.id).cloned().unwrap_or_default();
        // Without them the quest would look free (some recent quests are only described).
        if rows.is_empty() && asks_for_work(r.requirements.as_deref()) {
            return Err("objectives unknown (only described)");
        }
        if rows.is_empty() && r.catalogued_only() {
            return Err("objectives and turn-in unknown (catalogue only)");
        }
        let mut objs = Vec::new();
        for row in rows {
            if let Some(o) = self.objective(&quest, row)? {
                objs.push(o);
            }
        }
        for o in objs.iter_mut().filter(|o| o.elite) {
            o.pull = self.sources.elites.pull(o.mobs.iter().map(|(id, _)| *id));
        }
        self.add_dungeon_givers(r, &mut objs);
        Ok(objs)
    }

    fn context<'r>(&self, r: &'r QuestRow, givers: &Givers) -> QuestContext<'r> {
        let (anchor, zone) = (givers.starts[0].pos, givers.starts[0].zone);
        QuestContext {
            row: r,
            level: r.level.unwrap_or(1).max(1),
            anchor,
            zone,
            approx: self.params.approx_sources.then_some((anchor, zone, 0usize)),
            requirements: parse_requirements(r.requirements.clone()),
            parts: r
                .source_items
                .as_deref()
                .and_then(|j| serde_json::from_str::<Vec<serde_json::Value>>(j).ok())
                .unwrap_or_default()
                .iter()
                .filter_map(serde_json::Value::as_i64)
                .collect(),
        }
    }

    /// One objective; None when there is nothing to do for it (the item is given on accepting).
    fn objective(&self, quest: &QuestContext<'_>, row: ObjectiveRow) -> Result<Option<Objective>, Skip> {
        if self.sources.pvp.contains(&quest.row.id) {
            return Ok(Some(self.battleground(quest, row)));
        }
        if let Some(o) = self.inside_dungeon(quest, &row) {
            return Ok(Some(o));
        }
        let (kind, target, text, extra, amount) = row;
        let amount = amount.filter(|a| *a > 0).map(|a| a as f64);
        let target = target.unwrap_or(0);
        match kind {
            ObjectiveKind::Creature | ObjectiveKind::KillCredit => self
                .kill_or_talk(quest, kind, target, text, extra.as_deref(), amount)
                .map(Some),
            ObjectiveKind::Object => self.use_object(quest, target, text, amount).map(Some),
            ObjectiveKind::Item => self.get_item(quest, target, text, amount),
            ObjectiveKind::Trigger => self.explore(quest, text, extra.as_deref()).map(Some),
            ObjectiveKind::Other => Err("reputation/spell objective"),
        }
    }

    /// Battleground quests: marks and objectives are won in games, `pvp_mark_time` each, queued
    /// from where the quest is taken.
    fn battleground(&self, quest: &QuestContext<'_>, (kind, target, text, _, amount): ObjectiveRow) -> Objective {
        let count = amount.filter(|a| *a > 0).unwrap_or(1) as f64;
        let name = self.sources.entities.name(
            if kind == ObjectiveKind::Item {
                EntityKind::Item
            } else {
                EntityKind::Npc
            },
            target.unwrap_or(0),
        );
        let loc = Loc {
            pos: quest.anchor,
            zone: quest.zone,
            kind: EntityKind::Area,
            id: 0,
            name: name.clone(),
        };
        let text = format!("Battleground: {}", text.unwrap_or_else(|| format!("{name} x{count}")));
        Objective {
            count,
            ..Objective::plain(loc, text, count * self.params.pvp_mark_time)
        }
    }

    /// Objectives inside a dungeon are done by running it with a group. A quest filed under a
    /// dungeon is done there, even when its mob or object is also placed outside (the sources put
    /// some dungeon mobs at the entrance, or in the mine before the instance); otherwise only
    /// targets that live nowhere else.
    fn inside_dungeon(&self, quest: &QuestContext<'_>, row: &ObjectiveRow) -> Option<Objective> {
        let (kind, target, text, ..) = row;
        let target = target.unwrap_or(0);
        let quest_dungeon = quest
            .row
            .sort
            .and_then(|s| self.sources.dungeon_by_area.get(&s).copied());
        let kind = *kind;
        let d = match kind {
            ObjectiveKind::Creature | ObjectiveKind::KillCredit | ObjectiveKind::Object | ObjectiveKind::Item
                if quest_dungeon.is_some() =>
            {
                quest_dungeon
            }
            ObjectiveKind::Creature | ObjectiveKind::KillCredit | ObjectiveKind::Object => {
                if self.has_points(kind.entity(), target) {
                    None
                } else {
                    self.dungeon_of(kind.entity(), target).or(quest_dungeon)
                }
            }
            ObjectiveKind::Item => {
                let sources = self.item_sources(target, &quest.parts);
                let outside = sources
                    .iter()
                    .any(|(t, id)| *t != SourceKind::Item && t.entity().is_some_and(|e| self.has_points(e, *id)));
                if outside {
                    None
                } else {
                    sources
                        .iter()
                        .find_map(|(t, id)| self.dungeon_of(t.entity().unwrap_or(EntityKind::Npc), *id))
                        .or(quest_dungeon)
                }
            }
            ObjectiveKind::Trigger | ObjectiveKind::Other => None,
        }?;
        let dungeon = &self.sources.dungeons[d];
        let text = text.clone().unwrap_or_else(|| {
            format!(
                "{}: {}",
                dungeon.name,
                self.sources.entities.name(kind.entity(), target)
            )
        });
        Some(Objective {
            dungeon: Some(d),
            ..Objective::plain(dungeon.entrance.clone(), text, 0.0)
        })
    }

    /// Kill mobs (several kinds give credit for a killcredit objective), or talk to a friendly
    /// NPC. Elite objectives are left out solo unless only a few kills.
    fn kill_or_talk(
        &self,
        quest: &QuestContext<'_>,
        kind: ObjectiveKind,
        target: i64,
        text: Option<String>,
        extra: Option<&str>,
        amount: Option<f64>,
    ) -> Result<Objective, Skip> {
        let (entities, params) = (&self.sources.entities, self.params);
        let mut ids = vec![target];
        if kind == ObjectiveKind::KillCredit {
            let alt: Vec<i64> = extra.and_then(|e| serde_json::from_str(e).ok()).unwrap_or_default();
            ids.extend(alt);
        }
        let name = entities.name(EntityKind::Npc, target);
        let needed = amount.unwrap_or_else(|| count_for(&quest.requirements, &name, params.default_kill_count));
        let friendly = entities.friendly.contains(&target);
        let elite = ids.iter().any(|id| entities.is_elite(*id));
        if !params.allow_elite && needed > params.solo_elite_kills && !friendly && elite {
            return Err("elite objective");
        }
        let points: Vec<_> = ids
            .iter()
            .flat_map(|id| entities.points(self.world, EntityKind::Npc, *id))
            .collect();
        let (pos, zone, _) = best_cluster(&points, Some(&quest.anchor))
            .or(quest.approx)
            .ok_or("objective mob location unknown")?;
        let loc = Loc {
            pos,
            zone,
            kind: EntityKind::Npc,
            id: target,
            name: name.clone(),
        };
        if friendly {
            return Ok(Objective::plain(
                loc,
                text.unwrap_or_else(|| format!("Talk to {name}")),
                20.0,
            ));
        }
        let kills = needed;
        let credited = if kind == ObjectiveKind::KillCredit && ids.len() > 1 {
            &ids[1..]
        } else {
            &ids[..1]
        };
        Ok(Objective {
            text: text.unwrap_or_else(|| format!("Kill {name} x{kills}")),
            kills,
            mob_level: entities.npc_level(target),
            elite,
            count: kills,
            mobs: credited
                .iter()
                .map(|id| (*id, entities.name(EntityKind::Npc, *id)))
                .collect(),
            spots: if elite { None } else { Spots::new(&points, &pos) },
            ..Objective::plain(loc, String::new(), 0.0)
        })
    }

    fn use_object(
        &self,
        quest: &QuestContext<'_>,
        target: i64,
        text: Option<String>,
        amount: Option<f64>,
    ) -> Result<Objective, Skip> {
        let entities = &self.sources.entities;
        let points = entities.points(self.world, EntityKind::Object, target);
        let (pos, zone, _) = best_cluster(&points, Some(&quest.anchor))
            .or(quest.approx)
            .ok_or("objective object location unknown")?;
        let name = entities.name(EntityKind::Object, target);
        let uses = amount.unwrap_or_else(|| count_for(&quest.requirements, &name, 1.0));
        let loc = Loc {
            pos,
            zone,
            kind: EntityKind::Object,
            id: target,
            name: name.clone(),
        };
        Ok(Objective {
            uses,
            count: uses,
            spots: Spots::new(&points, &pos),
            ..Objective::plain(loc, text.unwrap_or_else(|| format!("Use {name}")), 0.0)
        })
    }

    /// Reach an area. Its points: `extra` = {zone: [[x, y], ...]}.
    fn explore(&self, quest: &QuestContext<'_>, text: Option<String>, extra: Option<&str>) -> Result<Objective, Skip> {
        let area: HashMap<String, Vec<Vec<f64>>> = extra.and_then(|e| serde_json::from_str(e).ok()).unwrap_or_default();
        let points: Vec<_> = area
            .iter()
            .flat_map(|(z, pts)| {
                let z: i64 = z.parse().unwrap_or(0);
                let ok = on_reachable(self.world, &self.sources.reachable, z);
                pts.iter()
                    .filter(move |_| ok)
                    .filter_map(move |p| self.world.to_world(z, *p.first()?, *p.get(1)?).map(|w| (w, z)))
            })
            .collect();
        let (pos, zone, _) = best_cluster(&points, Some(&quest.anchor)).ok_or("exploration area unknown")?;
        let loc = Loc {
            pos,
            zone,
            kind: EntityKind::Area,
            id: 0,
            name: text.clone().unwrap_or_default(),
        };
        Ok(Objective::plain(
            loc,
            text.unwrap_or_else(|| "Explore the area".into()),
            5.0,
        ))
    }

    /// A giver or turn-in that lives inside a dungeon (a corpse, a scout in the instance) is
    /// only met during a run of it.
    fn add_dungeon_givers(&self, r: &QuestRow, objs: &mut Vec<Objective>) {
        for (_, kind, id) in self.sources.relations.get(&r.id).into_iter().flatten() {
            if *kind == EntityKind::Item {
                continue;
            }
            if let Some(d) = self.dungeon_of(*kind, *id)
                && !objs.iter().any(|o| o.dungeon == Some(d))
            {
                let dungeon = &self.sources.dungeons[d];
                let text = format!("{}: {}", dungeon.name, self.sources.entities.name(*kind, *id));
                objs.push(Objective {
                    dungeon: Some(d),
                    ..Objective::plain(dungeon.entrance.clone(), text, 0.0)
                });
            }
        }
    }
}
