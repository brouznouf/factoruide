//! Getting the items a quest asks for: which mobs to farm (their drop rate), or the objects to
//! loot, the vendor to buy from, common loot gathered on the way, or an approximation.

use super::builder::{QuestBuilder, Skip};
use super::entities::best_cluster;
use super::objectives::QuestContext;
use super::sources::{SourceKind, count_for};
use super::types::{EntityKind, Loc, Objective, Spots};
use crate::world::Pos;

/// Quest givers around which the best drop rate is looked for (yards).
const AROUND: f64 = 1500.0;

/// Where an item can be got.
struct ItemPlaces {
    sources: Vec<(SourceKind, i64)>,
    objects: Vec<(Pos, i64)>,
    vendors: Vec<(Pos, i64)>,
    mob_ids: Vec<i64>,
    mobs: Vec<(Pos, i64)>,
}

impl QuestBuilder<'_> {
    pub(super) fn get_item(
        &self,
        quest: &QuestContext<'_>,
        target: i64,
        text: Option<String>,
        amount: Option<f64>,
    ) -> Result<Option<Objective>, Skip> {
        if quest.row.source_item == Some(target) {
            return Ok(None); // given when accepting
        }
        let name = self.sources.entities.name(EntityKind::Item, target);
        let count = amount.unwrap_or_else(|| count_for(&quest.requirements, &name, self.params.default_item_count));
        let places = self.item_places(quest, target, count);
        let text = text.unwrap_or_else(|| format!("{name} x{count}"));
        let at = |pos, zone| Loc {
            pos,
            zone,
            kind: EntityKind::Item,
            id: target,
            name: name.clone(),
        };
        let common = places.vendors.is_empty()
            && places.sources.iter().filter(|(t, _)| *t == SourceKind::Npc).count() >= self.params.common_item_sources;
        let objective = if common {
            Self::common_loot(at(quest.anchor, quest.zone), &text, count, self.params.common_item_time)
        } else if let Some((pos, zone, _)) = best_cluster(&places.objects, Some(&quest.anchor)) {
            Objective {
                uses: count,
                count,
                spots: Spots::new(&places.objects, &pos),
                ..Objective::plain(at(pos, zone), text, 0.0)
            }
        } else if let Some((pos, zone, _)) = best_cluster(&places.vendors, Some(&quest.anchor)) {
            Objective {
                count,
                ..Objective::plain(at(pos, zone), format!("Buy {text}"), 10.0)
            }
        } else if let Some((pos, zone, _)) = best_cluster(&places.mobs, Some(&quest.anchor)) {
            self.farm(quest, target, &places.mob_ids, count, at(pos, zone), text)
        } else if !self.params.allow_elite
            && places
                .sources
                .iter()
                .any(|(t, id)| *t == SourceKind::Npc && self.sources.entities.is_elite(*id))
        {
            return Err("objective item from elite mobs");
        } else if self.params.approx_sources && !self.sources.tokens.contains(&target) {
            // Unknown source: mobs of the quest level around the quest giver.
            Objective {
                kills: count / self.params.default_drop_chance * self.per_player,
                mob_level: quest.level,
                count,
                ..Objective::plain(at(quest.anchor, quest.zone), text, 0.0)
            }
        } else {
            return Err("objective item source unknown");
        };
        Ok(Some(objective))
    }

    fn item_places(&self, quest: &QuestContext<'_>, target: i64, count: f64) -> ItemPlaces {
        let (entities, world) = (&self.sources.entities, self.world);
        let sources = self.item_sources(target, &quest.parts);
        let points_of = |wanted: SourceKind| -> Vec<(Pos, i64)> {
            sources
                .iter()
                .filter(|(t, _)| *t == wanted)
                .filter_map(|(t, id)| Some(entities.points(world, t.entity()?, *id)))
                .flatten()
                .collect()
        };
        let objects = points_of(SourceKind::Object);
        let vendors = points_of(SourceKind::Vendor);
        let mob_ids = self.item_mobs(quest, target, count, &sources, objects.is_empty());
        let mobs = mob_ids
            .iter()
            .flat_map(|id| entities.points(world, EntityKind::Npc, *id))
            .collect();
        ItemPlaces {
            sources,
            objects,
            vendors,
            mob_ids,
            mobs,
        }
    }

    /// The mobs worth farming for the item: around the quest's level, a lone elite when no
    /// normal mob has it, those dropping it well.
    fn item_mobs(
        &self,
        quest: &QuestContext<'_>,
        target: i64,
        count: f64,
        sources: &[(SourceKind, i64)],
        no_object: bool,
    ) -> Vec<i64> {
        let params = self.params;
        let mut mob_ids = self.npc_sources(quest, sources, params.allow_elite);
        let vendor = sources.iter().any(|(t, _)| *t == SourceKind::Vendor);
        if mob_ids.is_empty() && no_object && !params.allow_elite && !vendor {
            let elites = self.npc_sources(quest, sources, true);
            let chance = elites
                .iter()
                .filter_map(|id| self.sources.drop_chance.get(&(target, *id)).copied())
                .fold(0.0, f64::max);
            let chance = if chance > 0.0 {
                chance
            } else {
                params.default_drop_chance
            };
            if count / chance * self.per_player <= params.solo_elite_kills {
                mob_ids = elites;
            }
        }
        self.keep_good_droppers(quest, target, &mut mob_ids);
        mob_ids
    }

    /// Mobs dropping the item, placed somewhere. Mobs around the quest's level: a rare far above
    /// it (Berserk Trogg, level 19, for a level 12 quest) or the same item on higher mobs
    /// elsewhere do not count.
    fn npc_sources(&self, quest: &QuestContext<'_>, sources: &[(SourceKind, i64)], elite: bool) -> Vec<i64> {
        let entities = &self.sources.entities;
        let ids: Vec<i64> = sources
            .iter()
            .filter(|(t, id)| *t == SourceKind::Npc && (elite || !entities.is_elite(*id)))
            .map(|(_, id)| *id)
            .filter(|id| !entities.points(self.world, EntityKind::Npc, *id).is_empty())
            .collect();
        let fit: Vec<i64> = ids
            .iter()
            .copied()
            .filter(|id| entities.npc_level(*id) <= quest.level + 3)
            .collect();
        if fit.is_empty() { ids } else { fit }
    }

    /// With known drop rates, farm the mobs that drop it well: ignore those that almost never
    /// do, and those far below the best around the quest giver (Defias at 10% when others nearby
    /// give 33%; a better rate in another zone doesn't count).
    fn keep_good_droppers(&self, quest: &QuestContext<'_>, target: i64, mob_ids: &mut Vec<i64>) {
        let chance = |id: &i64| self.sources.drop_chance.get(&(target, *id)).copied();
        if !mob_ids.iter().any(|id| chance(id).is_some()) {
            return;
        }
        let best_where = |near: bool| {
            mob_ids
                .iter()
                .filter(|id| {
                    self.sources
                        .entities
                        .points(self.world, EntityKind::Npc, **id)
                        .iter()
                        .any(|(p, _)| !near || p.dist(&quest.anchor) <= AROUND)
                })
                .filter_map(chance)
                .fold(0.0, f64::max)
        };
        let best = match best_where(true) {
            b if b > 0.0 => b,
            _ => best_where(false),
        };
        let min = (best / 2.0).max(0.03);
        mob_ids.retain(|id| chance(id).is_some_and(|c| c >= min));
    }

    /// Common loot (cloth, gems, potions dropped by hundreds of mob types): gathered on the way
    /// or bought at the auction house, not farmed.
    fn common_loot(loc: Loc, text: &str, count: f64, each: f64) -> Objective {
        Objective {
            count,
            ..Objective::plain(loc, format!("Gather or buy {text}"), count * each)
        }
    }

    /// Farm the mobs found around the chosen spot: their level and mean drop rate.
    fn farm(
        &self,
        quest: &QuestContext<'_>,
        target: i64,
        mob_ids: &[i64],
        count: f64,
        loc: Loc,
        text: String,
    ) -> Objective {
        let entities = &self.sources.entities;
        let pos = loc.pos;
        let here: Vec<i64> = mob_ids
            .iter()
            .copied()
            .filter(|id| {
                entities
                    .points(self.world, EntityKind::Npc, *id)
                    .iter()
                    .any(|(p, _)| p.dist(&pos) <= 250.0)
            })
            .collect();
        let here = if here.is_empty() { mob_ids.to_vec() } else { here };
        let mob_level = here
            .iter()
            .map(|id| entities.npc_level(*id))
            .min()
            .unwrap_or(quest.level);
        let chances: Vec<f64> = here
            .iter()
            .filter_map(|id| self.sources.drop_chance.get(&(target, *id)).copied())
            .collect();
        let chance = if chances.is_empty() {
            self.params.default_drop_chance
        } else {
            (chances.iter().sum::<f64>() / chances.len() as f64).clamp(0.03, 1.0)
        };
        let elite = here.iter().any(|id| entities.is_elite(*id));
        let spots = if elite {
            None
        } else {
            let points: Vec<_> = here
                .iter()
                .flat_map(|id| entities.points(self.world, EntityKind::Npc, *id))
                .collect();
            Spots::new(&points, &pos)
        };
        Objective {
            kills: count / chance * self.per_player,
            mob_level,
            elite: here.iter().all(|id| entities.is_elite(*id)),
            count,
            mobs: here
                .iter()
                .map(|id| (*id, entities.name(EntityKind::Npc, *id)))
                .collect(),
            spots,
            ..Objective::plain(loc, text, 0.0)
        }
    }
}
