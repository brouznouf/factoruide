//! The best item for each slot at each level band (`Params::bis`): where it comes from, and
//! which quests give one. Only the guaranteed sources count — the quest rewards and the
//! vendors' stock, never the drops.

use super::sources::Sources;
use super::types::{EntityKind, Quest, Vendor};
use crate::Profile;
use crate::params::{BisMode, Params};
use crate::power::PowerModel;
use crate::world::World;
use anyhow::Result;
use rusqlite::Connection;
use std::collections::HashSet;

/// The bands the best items are ranked at: 5 levels each (5, 10...), like the flights' gold
/// weight (`Params::gold_weight_at`).
const BAND: i64 = 5;

/// The slots ranked: every armor slot, one ring, one trinket (their twins take the same
/// items), the main hand, the off hand, the two-hand and the ranged.
const RANK_SLOTS: [usize; 16] = [0, 1, 2, 3, 4, 5, 6, 7, 8, 9, 10, 12, 14, 15, 16, 17];

/// The best item of a slot at a level band, and where it comes from.
#[derive(Debug, Clone)]
pub struct BisEntry {
    /// The band's top level (5, 10...).
    pub level: i64,
    /// The slot (`crate::power` equipment slots).
    pub slot: usize,
    /// The item (index in `power.items`).
    pub item: u16,
    /// Where it comes from.
    pub source: BisSource,
}

/// Where a best item comes from: a quest reward, or the vendors selling it.
#[derive(Debug, Clone)]
pub enum BisSource {
    /// A quest (index in `Model::quests`) giving it as a reward.
    Quest(u32),
    /// The vendors selling it (index in the power model's items -> the sellers).
    Vendor(Vec<Vendor>),
}

/// The best items of a class (`Params::bis`).
#[derive(Debug, Clone, Default)]
pub struct Bis {
    /// The best item of each ranked slot at each band, bands first (levels ascending, then the
    /// slots in `RANK_SLOTS`'s order).
    pub entries: Vec<BisEntry>,
    /// Whether each quest (index in `Model::quests`) gives a best item as one of its rewards.
    pub(super) gives: Vec<bool>,
    /// The vendors selling each item (index in the power model's items): the ones of the best
    /// items a vendor sells, empty for the others. Built with the entries.
    sellers: Vec<Vec<Vendor>>,
    /// The best items vendors sell (indexes in the power model's items), in the entries'
    /// order, each once. Built with the entries.
    vendor_items: Vec<u16>,
}

impl Bis {
    /// Whether quest `i` (index in `Model::quests`) gives a best item as one of its rewards
    /// (false without one, and for a quest the list does not know).
    pub fn gives(&self, i: usize) -> bool {
        self.gives.get(i).copied().unwrap_or(false)
    }

    /// The vendors selling best item `i` (index in the power model's items): empty when it is
    /// not one a vendor sells.
    pub fn sellers(&self, i: u16) -> &[Vendor] {
        self.sellers.get(i as usize).map_or(&[], Vec::as_slice)
    }

    /// The best items vendors sell (indexes in the power model's items), in the entries'
    /// order, each once.
    pub fn vendor_items(&self) -> &[u16] {
        &self.vendor_items
    }
}

/// The vendor stock by item (index in the power model's items): each seller and where it
/// stands.
type Stock = Vec<Vec<(i64, Vec<(crate::world::Pos, i64)>)>>;

/// The best items of the character's class over its levels (`profile`): for each band of
/// `BAND` levels and each ranked slot, the best item among the quests' rewards and the
/// vendors' stock (drops are never guaranteed: they do not count), and which quests give
/// one. Ranked by the value of the item in the slot (`PowerModel::value_in`), ties to the
/// lowest item index. `Params::bis` `Off`: nothing is looked for.
#[expect(clippy::too_many_arguments, reason = "a private loader read once by model::load")]
pub(super) fn load(
    conn: &Connection,
    power: &PowerModel,
    quests: &[Quest],
    rewards: &[(Vec<u16>, Vec<u16>)],
    profile: &Profile,
    world: &World,
    sources: &Sources,
    params: &Params,
) -> Result<Bis> {
    let mut bis = Bis {
        entries: Vec::new(),
        gives: vec![false; quests.len()],
        sellers: Vec::new(),
        vendor_items: Vec::new(),
    };
    if params.bis == BisMode::Off || power.items.is_empty() {
        return Ok(bis);
    }
    // Which quest gives which item: the lowest quest index of each, so that the choice does not
    // depend on the order the quests were built in.
    let mut giver: Vec<u32> = vec![u32::MAX; power.items.len()];
    for (q, (fixed, choices)) in rewards.iter().enumerate() {
        for &i in fixed.iter().chain(choices) {
            giver[i as usize] = giver[i as usize].min(q as u32);
        }
    }
    // The vendors' stock: which NPC sells which item, the character's side only. Vendors of the
    // other side do not sell to it.
    let hostile: &HashSet<i64> = &sources.entities.hostile;
    let mut stock: Stock = vec![Vec::new(); power.items.len()];
    let mut stmt = conn.prepare("SELECT from_id, item_id FROM m_item_source WHERE from_type = 'vendor'")?;
    let rows = stmt.query_map([], |r| Ok((r.get::<_, i64>(0)?, r.get::<_, i64>(1)?)))?;
    for row in rows {
        let (npc, id) = row?;
        let Some(i) = power.item(id) else { continue };
        if !hostile.contains(&npc) {
            let sellers = &mut stock[i as usize];
            if !sellers.iter().any(|&(sold, _)| sold == npc) {
                let points = sources.entities.points(world, EntityKind::Npc, npc);
                sellers.push((npc, points));
            }
        }
    }
    for sellers in &mut stock {
        sellers.sort_by_key(|&(npc, _)| npc);
    }
    // The vendors the character can go to for each item: those with a position (some are not
    // placed by the data), each as the quest objectives' item places have them
    // (`ItemModel::vendors`).
    let sellers: Vec<Vec<Vendor>> = (0..power.items.len())
        .map(|i| {
            stock[i]
                .iter()
                .filter_map(|&(npc, ref points)| {
                    let (pos, zone) = points.first().copied()?;
                    Some(Vendor {
                        id: npc,
                        name: sources.entities.name(EntityKind::Npc, npc),
                        zone,
                        zone_name: world.zone_name(zone).to_owned(),
                        pos,
                        stock: sources.entities.vendor_stock.get(&(npc, power.items[i].id)).copied(),
                    })
                })
                .collect()
        })
        .collect();
    // The bands of the character's levels, at their top level.
    let top = profile.to_level.min(power.top_level());
    let first = (profile.from_level + BAND - 1) / BAND * BAND;
    for level in (first..=top).step_by(BAND as usize) {
        for slot in RANK_SLOTS {
            // The best item of the slot at the band: among the quests' rewards and the vendors'
            // stock, the highest value in the slot (`PowerModel::value_in`), ties to the lowest
            // item index. An item counts when it goes to the slot and can be had at the band:
            // given by a quest of the band's level or below, or sold by a vendor.
            let mut best: Option<(f64, u16)> = None;
            for i in 0..power.items.len() as u16 {
                if !power.slots_of(i as usize).contains(&slot) {
                    continue;
                }
                let value = power.value_in(i as usize, level, slot);
                if value <= 0.0 {
                    continue;
                }
                let giver = giver[i as usize];
                let gettable = giver < u32::MAX && quests[giver as usize].level <= level;
                if (!gettable && sellers[i as usize].is_empty()) || best.is_some_and(|(b, _)| value <= b) {
                    continue;
                }
                best = Some((value, i));
            }
            let Some((_, item)) = best else { continue };
            let giver = giver[item as usize];
            let source = if giver < u32::MAX && quests[giver as usize].level <= level {
                bis.gives[giver as usize] = true;
                BisSource::Quest(giver)
            } else {
                BisSource::Vendor(sellers[item as usize].clone())
            };
            bis.entries.push(BisEntry {
                level,
                slot,
                item,
                source,
            });
        }
    }
    // The best items vendors sell, each once, in the entries' order: the stops the search may
    // add (`Kind::Buy`), with their sellers ready.
    let mut vendor_items = Vec::new();
    for entry in &bis.entries {
        if let BisSource::Vendor(sellers) = &entry.source
            && !sellers.is_empty()
            && !vendor_items.contains(&entry.item)
        {
            vendor_items.push(entry.item);
        }
    }
    bis.sellers = sellers;
    bis.vendor_items = vendor_items;
    Ok(bis)
}
