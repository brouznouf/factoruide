//! Where a quest is taken and turned in: its NPCs and objects, or the item that starts it
//! (looted from an object or a mob, or the reward of another quest).

use super::builder::{QuestBuilder, Skip};
use super::entities::best_cluster;
use super::sources::{QuestRow, Role, SourceKind};
use super::types::{EntityKind, Loc};

pub(super) struct Givers {
    pub(super) starts: Vec<Loc>,
    pub(super) ends: Vec<Loc>,
    /// Work to get the item that starts the quest: kills, at mob level, object uses.
    pub(super) start_work: (f64, i64, f64),
    /// Quests that must be done first to hold the item that starts this one.
    pub(super) item_pre: Vec<i64>,
}

/// How the item starting a quest is got.
enum ItemStart {
    Looted {
        at: Loc,
        work: (f64, i64, f64),
    },
    /// The reward of another quest (a tome handed by the class trainer): got from whoever turns
    /// that quest in, after it (unless it can be asked again).
    Rewarded(Vec<(Loc, Option<i64>)>),
    /// Dropping so rarely that farming it is luck, not a plan.
    Rare,
    Nowhere,
}

impl QuestBuilder<'_> {
    pub(super) fn givers(&self, r: &QuestRow) -> Result<Givers, Skip> {
        let start = &self.sources.start;
        let mut givers = Givers {
            starts: Vec::new(),
            ends: Vec::new(),
            start_work: (0.0, 0, 0.0),
            item_pre: Vec::new(),
        };
        let mut rare_start = false;
        for (role, kind, id) in self.sources.relations.get(&r.id).into_iter().flatten() {
            if *kind != EntityKind::Item {
                if let Some(loc) = self.locate(*kind, *id, Some(&start.pos)) {
                    if *role == Role::Start {
                        givers.starts.push(loc);
                    } else {
                        givers.ends.push(loc);
                    }
                }
                continue;
            }
            if *role != Role::Start {
                continue;
            }
            match self.item_start(r.id, *id) {
                ItemStart::Looted { at, work } => {
                    givers.starts.push(at);
                    givers.start_work = work;
                }
                ItemStart::Rewarded(places) => {
                    for (loc, pre) in places {
                        givers.starts.push(loc);
                        givers.item_pre.extend(pre);
                    }
                }
                ItemStart::Rare => rare_start = true,
                ItemStart::Nowhere => {}
            }
        }
        givers.starts.retain(|l| !self.params.excluded_zones.contains(&l.zone));
        if givers.starts.is_empty() && rare_start {
            return Err("started by a rare drop");
        }
        if givers.starts.is_empty() {
            return Err("no reachable quest giver");
        }
        if givers.ends.is_empty() {
            return Err("no reachable turn-in");
        }
        Ok(givers)
    }

    /// Where the item starting quest `quest` is got: objects first, then the mob dropping it
    /// best (unknown rate: a modest 10%; looting it costs 1 / chance kills).
    fn item_start(&self, quest: i64, item: i64) -> ItemStart {
        let (entities, world, params) = (&self.sources.entities, self.world, self.params);
        let start = &self.sources.start;
        let sources = self.sources.item_origins.get(&item).cloned().unwrap_or_default();
        let objects: Vec<_> = sources
            .iter()
            .filter(|(t, _)| *t == SourceKind::Object)
            .flat_map(|(_, o)| entities.points(world, EntityKind::Object, *o))
            .collect();
        let mobs: Vec<(i64, f64)> = sources
            .iter()
            .filter(|(t, n)| *t == SourceKind::Npc && (params.allow_elite || !entities.is_elite(*n)))
            .map(|(_, n)| (*n, self.sources.drop_chance.get(&(item, *n)).copied().unwrap_or(0.1)))
            .collect();
        let rare = objects.is_empty()
            && !mobs.is_empty()
            && mobs.iter().all(|(_, c)| *c < params.min_start_chance)
            && !self.sources.reward_of.contains_key(&item);
        if rare {
            return ItemStart::Rare;
        }
        let looted = |pos, zone| Loc {
            pos,
            zone,
            kind: EntityKind::Item,
            id: item,
            name: entities.name(EntityKind::Item, item),
        };
        if let Some((pos, zone, _)) = best_cluster(&objects, Some(&start.pos)) {
            return ItemStart::Looted {
                at: looted(pos, zone),
                work: (0.0, 0, 1.0),
            };
        }
        if let Some((npc, chance)) = mobs.iter().copied().max_by(|a, b| a.1.total_cmp(&b.1)) {
            let points = entities.points(world, EntityKind::Npc, npc);
            return match best_cluster(&points, Some(&start.pos)) {
                Some((pos, zone, _)) => ItemStart::Looted {
                    at: looted(pos, zone),
                    work: (1.0 / chance, entities.npc_level(npc), 0.0),
                },
                None => ItemStart::Nowhere,
            };
        }
        self.item_rewarded(quest, item)
    }

    fn item_rewarded(&self, quest: i64, item: i64) -> ItemStart {
        let mut places = Vec::new();
        for &giver in self
            .sources
            .reward_of
            .get(&item)
            .into_iter()
            .flatten()
            .filter(|g| **g != quest)
        {
            for (role, kind, eid) in self.sources.relations.get(&giver).into_iter().flatten() {
                if *role == Role::End
                    && *kind != EntityKind::Item
                    && let Some(loc) = self.locate(*kind, *eid, Some(&self.sources.start.pos))
                {
                    let pre = (!self.sources.repeatable.contains(&giver)).then_some(giver);
                    places.push((loc, pre));
                }
            }
        }
        ItemStart::Rewarded(places)
    }
}
