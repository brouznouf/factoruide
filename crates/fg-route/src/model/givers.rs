//! Where a quest is taken and turned in: its NPCs and objects, or the item that starts it
//! (looted from an object or a mob, or the reward of another quest).

use super::builder::{QuestBuilder, Skip};
use super::entities::best_cluster;
use super::sources::{ObjectiveKind, QuestRow, Role, SourceKind};
use super::types::{EntityKind, Loc, Spots};

pub(super) struct Givers {
    pub(super) starts: Vec<Loc>,
    pub(super) ends: Vec<Loc>,
    /// Work to get the item that starts the quest: kills, at mob level, object uses.
    pub(super) start_work: (f64, i64, f64),
    /// The mob dropping that item (ID, name), when it is looted from one, and its spawns.
    pub(super) start_mob: Option<(i64, String)>,
    pub(super) start_spots: Option<Spots>,
    /// The dungeon whose boss drops that item: taken during the run.
    pub(super) start_dungeon: Option<usize>,
    /// Quests that must be done first to hold the item that starts this one.
    pub(super) item_pre: Vec<i64>,
    /// The item the giver offers the quest for (`held_item`): looting it is the accept.
    pub(super) held: Option<i64>,
}

/// How the item starting a quest is got.
enum ItemStart {
    Looted {
        at: Loc,
        work: (f64, i64, f64),
        /// The mob it drops from (ID, name) and its spawns; none from an object.
        mob: Option<(i64, String)>,
        spots: Option<Spots>,
        /// Looted during this dungeon's run.
        dungeon: Option<usize>,
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
            start_mob: None,
            start_spots: None,
            start_dungeon: None,
            item_pre: Vec::new(),
            held: None,
        };
        let mut rare_start = false;
        let relations = self.sources.relations.get(&r.id).map(Vec::as_slice).unwrap_or_default();
        if let Some(item) = self.held_item(r.id, relations)
            && let ItemStart::Looted { at, work, .. } = self.item_start(r, item)
        {
            givers.starts.push(at);
            givers.start_work = work;
            givers.held = Some(item);
        }
        for (role, kind, id) in relations {
            if givers.held.is_some() && *role == Role::Start {
                continue;
            }
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
            match self.item_start(r, *id) {
                ItemStart::Looted {
                    at,
                    work,
                    mob,
                    spots,
                    dungeon,
                } => {
                    givers.starts.push(at);
                    givers.start_work = work;
                    givers.start_dungeon = dungeon;
                    givers.start_mob = mob;
                    givers.start_spots = spots;
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
        givers.ends.retain(|l| !self.params.excluded_zones.contains(&l.zone));
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

    /// The item quest `quest`'s NPC offers it for: one of its objectives that the data also
    /// gives as a starter, itself or the object it is looted from (the library's books, "The
    /// Dalaran Digest": the librarian offers it only while the book is in the bags, GitHub #29).
    /// The book is looted first, then the quest is taken and turned in at once at the librarian.
    fn held_item(&self, quest: i64, relations: &[(Role, EntityKind, i64)]) -> Option<i64> {
        let starts = |k: EntityKind| {
            relations
                .iter()
                .filter(move |(r, kind, _)| *r == Role::Start && *kind == k)
                .map(|(_, _, id)| *id)
        };
        starts(EntityKind::Npc).next()?;
        let objects: Vec<i64> = starts(EntityKind::Object).collect();
        let items: Vec<i64> = starts(EntityKind::Item).collect();
        self.sources
            .objectives
            .get(&quest)?
            .iter()
            .filter(|o| o.0 == ObjectiveKind::Item)
            .find_map(|o| {
                let item = o.1?;
                let origins = self.sources.item_origins.get(&item)?;
                let from_starter = (origins.iter()).any(|(t, id)| *t == SourceKind::Object && objects.contains(id));
                (from_starter || items.contains(&item)).then_some(item)
            })
    }

    /// Where the item starting quest `quest` is got: objects first, then the mob dropping it
    /// best (unknown rate: see `assumed_start_chance`; looting it costs 1 / chance kills).
    fn item_start(&self, r: &QuestRow, item: i64) -> ItemStart {
        let quest = r.id;
        let (entities, world, params) = (&self.sources.entities, self.world, self.params);
        let start = &self.sources.start;
        let sources = self.sources.item_origins.get(&item).cloned().unwrap_or_default();
        let objects: Vec<_> = sources
            .iter()
            .filter(|(t, _)| *t == SourceKind::Object)
            .flat_map(|(_, o)| entities.points(world, EntityKind::Object, *o))
            .collect();
        // A boss's drop: looted during the run, with the group (the quest needs it,
        // `add_dungeon_givers`).
        let boss = sources.iter().find_map(|(t, n)| {
            (*t == SourceKind::Npc).then(|| self.dungeon_of(EntityKind::Npc, *n).map(|d| (*n, d)))?
        });
        let in_run = |d: usize, mob: Option<(i64, String)>| ItemStart::Looted {
            at: Loc {
                kind: EntityKind::Item,
                id: item,
                name: entities.name(EntityKind::Item, item),
                ..self.sources.dungeons[d].entrance.clone()
            },
            work: (0.0, 0, 0.0),
            mob,
            spots: None,
            dungeon: Some(d),
        };
        if let Some((npc, d)) = boss {
            return in_run(d, Some((npc, entities.name(EntityKind::Npc, npc))));
        }
        let rank = |n: &i64| entities.npc.get(n).map_or(0, |e| e.3);
        let droppers: Vec<Dropper> = sources
            .iter()
            .filter(|(t, _)| *t == SourceKind::Npc)
            .map(|(_, n)| Dropper {
                rank: rank(n),
                spawns: entities.spawns.get(&(EntityKind::Npc, *n)).map_or(0, Vec::len),
            })
            .collect();
        let unknown = assumed_start_chance(&droppers);
        let mobs: Vec<(i64, f64)> = sources
            .iter()
            .filter(|(t, n)| *t == SourceKind::Npc && reliable_dropper(rank(n)))
            .filter(|(_, n)| params.allow_elite || !entities.is_elite(*n))
            .map(|(_, n)| {
                (
                    *n,
                    self.sources.drop_chance.get(&(item, *n)).copied().unwrap_or(unknown),
                )
            })
            .collect();
        let rare = objects.is_empty()
            && (!mobs.is_empty() || droppers.iter().any(|d| !reliable_dropper(d.rank)))
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
                mob: None,
                spots: None,
                dungeon: None,
            };
        }
        if let Some((npc, chance)) = mobs.iter().copied().max_by(|a, b| a.1.total_cmp(&b.1)) {
            let points = entities.points(world, EntityKind::Npc, npc);
            return match best_cluster(&points, Some(&start.pos)) {
                Some((pos, zone, _)) => ItemStart::Looted {
                    at: looted(pos, zone),
                    work: (1.0 / chance, entities.npc_level(npc), 0.0),
                    mob: Some((npc, entities.name(EntityKind::Npc, npc))),
                    spots: Spots::new(&points, &pos),
                    dungeon: None,
                },
                None => ItemStart::Nowhere,
            };
        }
        match self.item_rewarded(quest, item) {
            // Got nowhere known, for a quest filed under a dungeon: in the run (Forever's Crest
            // of Lordaeron, from an object of Ruins of Lordaeron no source places).
            ItemStart::Rewarded(places) if places.is_empty() => {
                match r.sort.and_then(|s| self.sources.dungeon_by_area.get(&s).copied()) {
                    Some(d) => in_run(d, None),
                    None => ItemStart::Rewarded(places),
                }
            }
            start => start,
        }
    }

    /// Where quest `quest` is turned in (its first ender found), None when nowhere known.
    pub(super) fn ender(&self, quest: i64) -> Option<Loc> {
        self.sources
            .relations
            .get(&quest)?
            .iter()
            .filter(|(role, kind, _)| *role == Role::End && *kind != EntityKind::Item)
            .find_map(|(_, kind, id)| self.locate(*kind, *id, Some(&self.sources.start.pos)))
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

/// A creature dropping an item that starts a quest: its rank (0 normal, 1 elite, 2 rare elite,
/// 3 boss, 4 rare) and how many places it is known at.
#[derive(Debug, Clone, Copy)]
pub struct Dropper {
    pub rank: i64,
    pub spawns: usize,
}

/// The chance per kill assumed for an item starting a quest when no source knows its rate.
/// Dropped by one creature, or only by unique ones (a rare elite, a boss, an NPC standing at one to
/// three places: the rogues' Elegant Letter on each trainer), it is that creature's item, looted
/// on the kill or nearly: a modest 10%. Dropped by several kinds of common mobs, it is a rare
/// find on all of them (Precessive Autocognition Assembly: about one in 900 Harvest Golems):
/// 0, below `min_start_chance`, so the quest is not planned (looted anyway when it drops).
/// Rares are not counted (`reliable_dropper`): dropped only by them, it is not planned either.
pub fn assumed_start_chance(droppers: &[Dropper]) -> f64 {
    let droppers: Vec<&Dropper> = droppers.iter().filter(|d| reliable_dropper(d.rank)).collect();
    let unique = |d: &&Dropper| matches!(d.rank, 2..=3) || (1..=3).contains(&d.spawns);
    if droppers.is_empty() {
        0.0
    } else if droppers.len() == 1 || droppers.iter().all(unique) {
        UNKNOWN_START_CHANCE
    } else {
        0.0
    }
}

/// Whether a creature of this rank is a source of an item starting a quest. Not a rare (4): one
/// spawn, a long respawn, often camped or dead (Gruff Swiftbite's Gold Pickup Schedule): its
/// common droppers are farmed instead at their own chance, if any.
pub(crate) fn reliable_dropper(rank: i64) -> bool {
    rank != 4
}

/// The chance assumed for an item starting a quest that its own creature drops (rate unknown).
pub const UNKNOWN_START_CHANCE: f64 = 0.1;

#[cfg(test)]
mod tests {
    use super::*;

    fn d(rank: i64, spawns: usize) -> Dropper {
        Dropper { rank, spawns }
    }

    #[test]
    fn an_item_of_common_mobs_is_rare() {
        // Harvesting the Harvesters: four kinds of golems and scarecrows, many of each.
        assert_eq!(assumed_start_chance(&[d(0, 29), d(0, 35), d(0, 10), d(0, 16)]), 0.0);
        // A named NPC among common ones does not make the item his (Captain Beld, Dark Iron Spies).
        assert_eq!(assumed_start_chance(&[d(0, 12), d(0, 1)]), 0.0);
        // Creatures of unknown places are not unique ones.
        assert_eq!(assumed_start_chance(&[d(1, 0), d(1, 0)]), 0.0);
    }

    #[test]
    fn an_item_of_one_creature_or_unique_ones_keeps_the_default() {
        assert_eq!(assumed_start_chance(&[d(1, 149)]), UNKNOWN_START_CHANCE);
        assert_eq!(assumed_start_chance(&[d(0, 1)]), UNKNOWN_START_CHANCE);
        // The rogues' Elegant Letter: one trainer at each place.
        assert_eq!(assumed_start_chance(&[d(0, 1); 14]), UNKNOWN_START_CHANCE);
        assert_eq!(assumed_start_chance(&[d(4, 3), d(2, 40)]), UNKNOWN_START_CHANCE);
    }

    /// Ghost Howl's Demon Dust, Blackmoss the Fetid's Fetid Skull: a rare's item is luck.
    #[test]
    fn an_item_of_rares_only_is_rare() {
        assert_eq!(assumed_start_chance(&[d(4, 57)]), 0.0);
        assert_eq!(assumed_start_chance(&[d(4, 7), d(4, 1)]), 0.0);
        // A common creature beside the rare: its own item.
        assert_eq!(assumed_start_chance(&[d(4, 7), d(0, 26)]), UNKNOWN_START_CHANCE);
    }
}
