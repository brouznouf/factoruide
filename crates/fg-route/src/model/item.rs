//! Getting the items a quest asks for: which mobs to farm (their drop rate), or the objects to
//! loot, the vendor to buy from, common loot gathered on the way, or an approximation.

use super::builder::{QuestBuilder, Skip};
use super::entities::{alternatives, best_cluster};
use super::objectives::QuestContext;
use super::sources::{SourceKind, count_for};
use super::types::{EntityKind, Loc, Objective, Spots};
use crate::world::Pos;

/// Quest givers around which the best drop rate is looked for (yards).
const AROUND: f64 = 1500.0;

/// Where an item can be got.
/// Time to buy an item at an auction house, and the more it takes on an empty server (few
/// players selling: waiting for one to put it up, or going without).
const AUCTION_TIME: f64 = 60.0;
const AUCTION_SCARCE: f64 = 1800.0;
/// Auctioneers this close (yards) are one auction house (a city's: Stormwind has two groups of
/// them).
const AUCTION_HOUSE: f64 = 300.0;

/// The Soul Shard item, and the warlock's class ID.
const SOUL_SHARD: i64 = 6265;
const WARLOCK: i64 = 9;

/// An auction house: where it is (middle of its auctioneers, zone), its auctioneers (ID, name).
type AuctionHouse = (Pos, i64, Vec<(i64, String)>);

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
        let in_given_bag =
            |bag: i64| (self.sources.item_origins.get(&target)).is_some_and(|o| o.contains(&(SourceKind::Item, bag)));
        if quest.row.source_item.is_some_and(|s| s == target || in_given_bag(s)) {
            return Ok(None); // given when accepting, or in the bag given then (Call of Fire's satchel)
        }
        // A warlock makes its Soul Shards by draining the creatures it kills (the sources only
        // know a vendor of Outland).
        if target == SOUL_SHARD && self.profile.class_ids().contains(&WARLOCK) {
            return Ok(None);
        }
        if self.bag_quest(quest.row.id, target).is_some() {
            return Ok(None); // in the bag another quest gives, held in the log (`Quest::pre_held`)
        }
        let name = self.sources.entities.name(EntityKind::Item, target);
        let count =
            amount.unwrap_or_else(|| count_for(&quest.requirements, &name).unwrap_or(self.params.default_item_count));
        let places = self.item_places(quest, target, count);
        let text = text.unwrap_or_else(|| format!("{name} x{count}"));
        let at = |pos, zone| Loc {
            pos,
            zone,
            kind: EntityKind::Item,
            id: target,
            name: name.clone(),
            inside: None,
            floor: None,
        };
        // Objects giving it only now and then (random containers): bought instead when sold.
        let object_chance = places
            .sources
            .iter()
            .filter(|(t, id)| *t == SourceKind::Object && self.has_points(EntityKind::Object, *id))
            .map(|(_, id)| self.sources.drop_chance.get(&(target, *id)).copied().unwrap_or(1.0))
            .fold(0.0, f64::max);
        let objects = if object_chance < 0.5 && !places.vendors.is_empty() {
            &[][..]
        } else {
            &places.objects[..]
        };
        let common = places.vendors.is_empty()
            && places.sources.iter().filter(|(t, _)| *t == SourceKind::Npc).count() >= self.params.common_item_sources;
        let objective = if common {
            Self::common_loot(at(quest.anchor, quest.zone), &text, count, self.params.common_item_time)
        } else if let Some((pos, zone, _)) = best_cluster(objects, Some(&quest.anchor)) {
            let loot = |pos: Pos, zone: i64| Objective {
                uses: count / object_chance.max(0.01),
                count,
                spots: Spots::new(objects, &pos),
                ..Objective::plain(at(pos, zone), text.clone(), 0.0)
            };
            Objective {
                alts: alternatives(objects, &pos, &quest.anchor, count, self.params.objective_alternatives)
                    .into_iter()
                    .map(|(p, z)| loot(p, z))
                    .collect(),
                ..loot(pos, zone)
            }
        } else if let Some((pos, zone, _)) = best_cluster(&places.vendors, Some(&quest.anchor)) {
            let vendors = self.vendors(target, &places.sources, &pos);
            // Sold in limited supply: on a crowded server the vendor is often out (half its
            // restock to wait), while the auction house has it from the other players; on an
            // empty one the auction house seldom has it.
            let limited = vendors.first().and_then(|v| v.stock);
            let crowd = self.params.crowd_at(quest.level);
            let wait = limited.map_or(0.0, |(_, restock)| crowd * restock / 2.0);
            let buy = |pos: Pos, zone: i64, extra: f64, auction: Vec<(i64, String)>| Objective {
                count,
                vendors: vendors.clone(),
                auction,
                ..Objective::plain(at(pos, zone), format!("Buy {text}"), extra)
            };
            let alts = if limited.is_some() {
                (self.auction_houses().into_iter())
                    .map(|(pos, zone, auctioneers)| {
                        buy(pos, zone, AUCTION_TIME + (1.0 - crowd) * AUCTION_SCARCE, auctioneers)
                    })
                    .collect()
            } else {
                Vec::new()
            };
            Objective {
                alts,
                ..buy(pos, zone, 10.0 + wait, Vec::new())
            }
        } else if let Some((pos, zone, _)) = best_cluster(&places.mobs, Some(&quest.anchor)) {
            // Dropping too seldom for the player even from its best mob: the quest is left out,
            // unless it is a class quest (never left out).
            let best = (places.mob_ids.iter())
                .filter_map(|id| self.sources.drop_chance.get(&(target, *id)).copied())
                .fold(0.0, f64::max);
            if best > 0.0 && best < self.params.min_drop_chance && !self.sources.class_quest_ids.contains(&quest.row.id)
            {
                return Err("quest item drop rate under the minimum");
            }
            let farm =
                |pos: Pos, zone: i64| self.farm(quest, target, &places.mob_ids, count, at(pos, zone), text.clone());
            let first = farm(pos, zone);
            Objective {
                alts: alternatives(
                    &places.mobs,
                    &pos,
                    &quest.anchor,
                    first.kills,
                    self.params.objective_alternatives,
                )
                .into_iter()
                .map(|(p, z)| farm(p, z))
                .collect(),
                ..first
            }
        } else if let Some(handed) = self.handed_by(quest.row.id, target, &places) {
            handed
        } else if (self.params.auction_items.contains(&target)
            || self.params.auction_house && !self.sources.entities.not_sold.contains(&target))
            && !self.auction_houses().is_empty()
        {
            // Made by a profession: bought, from the auction house nearest the quest giver.
            let (pos, zone, auctioneers) = (self.auction_houses().into_iter())
                .min_by(|a, b| a.0.dist(&quest.anchor).total_cmp(&b.0.dist(&quest.anchor)))
                .expect("an auction house");
            Objective {
                count,
                auction: auctioneers,
                ..Objective::plain(
                    at(pos, zone),
                    format!("Buy {text}"),
                    AUCTION_TIME + (1.0 - self.params.crowd_at(quest.level)) * AUCTION_SCARCE,
                )
            }
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

    /// An item nothing gives but the turn-in of other quests: got there, for no work, once one
    /// of them is turned in (`Objective::handed_by`).
    fn handed_by(&self, quest: i64, item: i64, places: &ItemPlaces) -> Option<Objective> {
        // Questie lists the NPC who hands the quest reward as its source: not one to kill.
        let entities = &self.sources.entities;
        if !(places.sources.iter()).all(|(t, id)| *t == SourceKind::Npc && entities.friendly.contains(id)) {
            return None;
        }
        let givers: Vec<i64> = self
            .sources
            .reward_of
            .get(&item)?
            .iter()
            .copied()
            .filter(|g| *g != quest)
            .collect();
        let name = self.sources.entities.name(EntityKind::Item, item);
        let at = givers.iter().find_map(|g| self.ender(*g))?;
        Some(Objective {
            handed_by: givers,
            ..Objective::plain(
                Loc {
                    kind: EntityKind::Item,
                    id: item,
                    name: name.clone(),
                    ..at
                },
                name,
                0.0,
            )
        })
    }

    /// The auction houses the character can buy from: the middle of each group of auctioneers
    /// (a city's), and their IDs and names.
    fn auction_houses(&self) -> Vec<AuctionHouse> {
        let entities = &self.sources.entities;
        let mut houses: Vec<AuctionHouse> = Vec::new();
        for &id in entities.auctioneers.iter() {
            for (pos, zone) in entities.points(self.world, EntityKind::Npc, id) {
                let name = entities.name(EntityKind::Npc, id);
                match houses.iter_mut().find(|h| h.0.dist(&pos) <= AUCTION_HOUSE) {
                    Some(h) if !h.2.iter().any(|a| a.0 == id) => h.2.push((id, name)),
                    Some(_) => {}
                    None => houses.push((pos, zone, vec![(id, name)])),
                }
            }
        }
        houses
    }

    /// The vendors of the character's side selling `item` (among its `sources`), each at its
    /// spawn nearest `at`: the one at `at` first, then the others from the nearest.
    fn vendors(&self, item: i64, sources: &[(SourceKind, i64)], at: &Pos) -> Vec<super::Vendor> {
        let entities = &self.sources.entities;
        let mut vendors: Vec<(f64, super::Vendor)> = (sources.iter())
            .filter(|&&(t, id)| t == SourceKind::Vendor && !entities.hostile.contains(&id))
            .filter_map(|&(_, id)| {
                let (pos, zone) = (entities.points(self.world, EntityKind::Npc, id).into_iter())
                    .min_by(|a, b| a.0.dist(at).total_cmp(&b.0.dist(at)))?;
                let vendor = super::Vendor {
                    id,
                    name: entities.name(EntityKind::Npc, id),
                    zone,
                    zone_name: self.world.zone_name(zone).to_owned(),
                    pos,
                    stock: entities.vendor_stock.get(&(id, item)).copied(),
                };
                Some((pos.dist(at), vendor))
            })
            .collect();
        vendors.sort_by(|a, b| a.0.total_cmp(&b.0));
        vendors.dedup_by_key(|v| v.1.id);
        vendors.into_iter().map(|(_, v)| v).collect()
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
        // Vendors of the other side do not sell to the character.
        let vendors = (sources.iter())
            .filter(|&&(t, id)| t == SourceKind::Vendor && !entities.hostile.contains(&id))
            .flat_map(|&(_, id)| entities.points(world, EntityKind::Npc, id))
            .collect();
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
            .filter(|(t, id)| {
                *t == SourceKind::Npc && (elite || !entities.is_elite(*id)) && !entities.friendly.contains(id)
            })
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
        // The best dropper always stays, even under the floor (Bloodvenom Essence, 2.9%).
        let min = (best / 2.0).max(0.03).min(best);
        mob_ids.retain(|id| chance(id).is_some_and(|c| c >= min));
    }

    /// Common loot (cloth, gems, potions dropped by hundreds of mob types): gathered on the way
    /// (the route's kills fill the bags, `CommonLoot`) or bought at the auction house, not
    /// farmed.
    fn common_loot(loc: Loc, text: &str, count: f64, each: f64) -> Objective {
        Objective {
            count,
            // Indexed with the others once every quest is built (`CommonLoot::load`).
            stock: Some(0),
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
        // The mobs there are all fought: the middle of their ranges on average, rounded up as
        // one mob's (`npc_level`), not the lowest of them.
        let mob_level = if here.is_empty() {
            quest.level
        } else {
            (here.iter().map(|id| entities.npc_mid(*id)).sum::<f64>() / here.len() as f64).ceil() as i64
        };
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
        let kills = count / chance * self.per_player;
        Objective {
            kills,
            low_drop_kills: kills * (1.0 - chance),
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

#[cfg(test)]
mod tests {
    use crate::{fixture::Fixture, job, xp::Edition};

    fn outdoor_collection() -> job::Prepared {
        let conn = Fixture::new(Edition::Classic)
            .toml(
                r#"
[[npc]]
id = 990001
name = "Collector"
npc_flags = 2
friendly_to = "AH"
at = [{ zone = 40, x = 56.0, y = 47.0 }]
[[npc]]
id = 990002
name = "Outdoor dropper"
level = 18
at = [{ zone = 40, x = 42.0, y = 72.0 }]
[[npc]]
id = 990005
name = "Instance dropper"
level = 18
at = [{ zone = 1581, x = -1.0, y = -1.0 }]
[[object]]
id = 990007
name = "Outdoor container"
at = [{ zone = 40, x = 42.0, y = 72.0 }]
[[object]]
id = 990008
name = "Instance container"
at = [{ zone = 1581, x = -1.0, y = -1.0 }]
[[item]]
id = 990003
name = "Container collection"
class = 12
drops = [{ object = 990007, chance = 0.1 }, { object = 990008, chance = 1.0 }]
[[item]]
id = 990013
name = "Creature collection"
class = 12
drops = [{ npc = 990002, chance = 0.25 }, { npc = 990005, chance = 0.8 }]
[[quest]]
id = 990004
name = "Ordinary outdoor container quest"
level = 18
zone = 40
giver = 990001
objectives = [{ item = 990003, amount = 6 }]
[[quest]]
id = 990014
name = "Ordinary outdoor creature quest"
level = 18
zone = 40
giver = 990001
objectives = [{ item = 990013, amount = 6 }]
[[row]]
table = "dungeon"
values = { area = 1581, name = "The Deadmines", entrance_zone = 40, entrance_x = 42.5, entrance_y = 71.7, source = "fixture" }
"#,
            )
            .open();
        let request = serde_json::from_value(serde_json::json!({
            "race":"human", "class":"warrior", "from_level":18, "to_level":25,
            "params":{"edition":"classic", "all_class_quests":false, "dungeons":false}
        }))
        .unwrap();
        let overrides = job::Overrides::load_edition(
            &std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../overrides"),
            Edition::Classic,
        )
        .unwrap();
        job::prepare(&conn, &overrides, &request, &|_| {}).unwrap()
    }

    #[test]
    fn outdoor_containers_use_their_own_drop_rate_without_instance_providers() {
        let p = outdoor_collection();
        let objective = &p.model.quests[p.model.index[&990004]].objectives[0];
        assert_eq!(objective.dungeon_of(), None, "an ordinary outdoor quest");
        assert_eq!(objective.loc.zone, 40);
        assert_eq!(objective.count, 6.0);
        assert_eq!(objective.uses, 60.0, "six items at ten percent require sixty uses");
        assert_eq!(objective.kills, 0.0);
    }

    #[test]
    fn outdoor_creature_drop_costs_are_unchanged_by_instance_sources() {
        let p = outdoor_collection();
        let objective = &p.model.quests[p.model.index[&990014]].objectives[0];
        assert_eq!(objective.dungeon_of(), None);
        assert_eq!(objective.loc.zone, 40);
        assert_eq!(objective.count, 6.0);
        assert_eq!(
            objective.kills, 24.0,
            "six items at twenty-five percent require twenty-four kills"
        );
        assert_eq!(objective.mobs, vec![(990002, "Outdoor dropper".into())]);
        assert_eq!(objective.uses, 0.0);
        assert_eq!(objective.low_drop_kills, 18.0, "three kills in four drop nothing");
    }

    /// Two kinds of ghouls dropping a rib one time in ten, side by side: 24-25 and 26-27.
    fn ghoul_ribs(params: &serde_json::Value, required: &[i64]) -> job::Prepared {
        let conn = Fixture::new(Edition::Classic)
            .toml(
                r#"
[[npc]]
id = 990101
name = "Giver"
npc_flags = 2
friendly_to = "AH"
at = [{ zone = 40, x = 56.0, y = 47.0 }]
[[npc]]
id = 990102
name = "Rotting Ghoul"
level = 24
max_level = 25
at = [{ zone = 40, x = 42.0, y = 72.0 }]
[[npc]]
id = 990103
name = "Brittle Ghoul"
level = 26
max_level = 27
at = [{ zone = 40, x = 42.2, y = 72.2 }]
[[item]]
id = 990104
name = "Ghoul Rib"
class = 12
drops = [{ npc = 990102, chance = 0.1 }, { npc = 990103, chance = 0.1 }]
[[quest]]
id = 990105
name = "Rib collection"
level = 25
zone = 40
giver = 990101
objectives = [{ item = 990104, amount = 5 }]
[[quest]]
id = 990106
name = "Ghoul cull"
level = 25
zone = 40
giver = 990101
objectives = [{ creature = 990102, amount = 8 }]
"#,
            )
            .open();
        let mut all = serde_json::json!({"edition":"classic", "all_class_quests":false, "dungeons":false});
        all.as_object_mut()
            .expect("an object")
            .extend(params.as_object().cloned().unwrap_or_default());
        let request = serde_json::from_value(serde_json::json!({
            "race":"human", "class":"warrior", "from_level":23, "to_level":30, "params": all,
            "required_quests": required
        }))
        .unwrap();
        let overrides = job::Overrides::load_edition(
            &std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../overrides"),
            Edition::Classic,
        )
        .unwrap();
        job::prepare(&conn, &overrides, &request, &|_| {}).unwrap()
    }

    #[test]
    fn the_kills_that_drop_nothing_are_counted_for_their_weight() {
        let p = ghoul_ribs(&serde_json::json!({}), &[]);
        let ribs = &p.model.quests[p.model.index[&990105]].objectives[0];
        assert_eq!(ribs.kills, 50.0, "five ribs at ten percent");
        assert_eq!(ribs.low_drop_kills, 45.0, "nine kills in ten drop nothing");
        let cull = &p.model.quests[p.model.index[&990106]].objectives[0];
        assert_eq!(cull.low_drop_kills, 0.0, "a kill objective drops nothing to wait for");
    }

    #[test]
    fn a_quest_item_dropping_under_the_minimum_leaves_its_quest_out() {
        let p = ghoul_ribs(&serde_json::json!({"min_drop_chance": 0.2}), &[]);
        assert!(!p.model.index.contains_key(&990105), "ten percent is under twenty");
        assert!(p.model.index.contains_key(&990106), "the kill quest stays");
        let p = ghoul_ribs(&serde_json::json!({"min_drop_chance": 0.1}), &[]);
        assert!(p.model.index.contains_key(&990105), "at the minimum, the quest stays");
        let p = ghoul_ribs(&serde_json::json!({"min_drop_chance": 0.2}), &[990105]);
        assert!(
            p.model.index.contains_key(&990105),
            "asked for by the player, the quest stays"
        );
    }

    /// Two kinds of ghouls dropping a rib one time in ten, side by side: 24-25 and 26-27.
    fn ghoul_ribs(params: &serde_json::Value) -> job::Prepared {
        let conn = Fixture::new(Edition::Classic)
            .toml(
                r#"
[[npc]]
id = 990101
name = "Giver"
npc_flags = 2
friendly_to = "AH"
at = [{ zone = 40, x = 56.0, y = 47.0 }]
[[npc]]
id = 990102
name = "Rotting Ghoul"
level = 24
max_level = 25
at = [{ zone = 40, x = 42.0, y = 72.0 }]
[[npc]]
id = 990103
name = "Brittle Ghoul"
level = 26
max_level = 27
at = [{ zone = 40, x = 42.2, y = 72.2 }]
[[item]]
id = 990104
name = "Ghoul Rib"
class = 12
drops = [{ npc = 990102, chance = 0.1 }, { npc = 990103, chance = 0.1 }]
[[quest]]
id = 990105
name = "Rib collection"
level = 25
zone = 40
giver = 990101
objectives = [{ item = 990104, amount = 5 }]
[[quest]]
id = 990106
name = "Ghoul cull"
level = 25
zone = 40
giver = 990101
objectives = [{ creature = 990102, amount = 8 }]
"#,
            )
            .open();
        let mut all = serde_json::json!({"edition":"classic", "all_class_quests":false, "dungeons":false});
        all.as_object_mut()
            .expect("an object")
            .extend(params.as_object().cloned().unwrap_or_default());
        let request = serde_json::from_value(serde_json::json!({
            "race":"human", "class":"warrior", "from_level":23, "to_level":30, "params": all
        }))
        .unwrap();
        let overrides = job::Overrides::load_edition(
            &std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../overrides"),
            Edition::Classic,
        )
        .unwrap();
        job::prepare(&conn, &overrides, &request, &|_| {}).unwrap()
    }

    #[test]
    fn mobs_are_fought_at_the_middle_of_their_levels_rounded_up() {
        let p = ghoul_ribs(&serde_json::json!({}));
        let cull = &p.model.quests[p.model.index[&990106]].objectives[0];
        assert_eq!(cull.mob_level, 25, "a 24-25 mob");
        let ribs = &p.model.quests[p.model.index[&990105]].objectives[0];
        assert_eq!(ribs.mob_level, 26, "24-25 and 26-27 together, not the lowest");
    }
}
