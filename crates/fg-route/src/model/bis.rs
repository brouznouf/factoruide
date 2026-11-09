//! The best item for each slot at each level band (`Params::bis`): where it comes from, and
//! which quests give one. Only the guaranteed sources count — the quest rewards and the
//! vendors' stock, never the drops.

use super::feasibility::{BisQuestAction, BisQuestPlan, Selector};
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
    required_roots: Vec<u32>,
    required_plans: Vec<BisQuestPlan>,
    requires: Vec<bool>,
    fallback_notes: Vec<String>,
    source_actions: Vec<BisQuestAction>,
    source_edges: Vec<(BisQuestAction, BisQuestAction)>,
    source_quests: Vec<u32>,
    source_visits: std::collections::BTreeMap<BisQuestAction, Option<usize>>,
}

impl Bis {
    /// Chosen visit of a residual source action: absent, outside, or inside one dungeon.
    pub fn source_visit(&self, action: BisQuestAction) -> Option<Option<usize>> {
        self.source_visits.get(&action).copied()
    }
    /// Final coherent selected source actions in dependency order (Prefer and Required).
    pub fn source_actions(&self) -> &[BisQuestAction] {
        &self.source_actions
    }
    /// Actual cross-source ordering constraints; satisfied historical edges are absent.
    pub fn source_edges(&self) -> &[(BisQuestAction, BisQuestAction)] {
        &self.source_edges
    }
    /// Source participants, including recorded held parents needing no new action.
    pub fn source_quests(&self) -> &[u32] {
        &self.source_quests
    }
    /// Selected quest sources that Required promises to complete.
    pub fn required_roots(&self) -> &[u32] {
        &self.required_roots
    }
    /// Supported residual actions for those sources.
    pub fn required_plans(&self) -> &[BisQuestPlan] {
        &self.required_plans
    }
    /// Unsatisfied turn-in support; held-only accepts are not completion goals.
    pub fn requires(&self, quest: usize) -> bool {
        self.requires.get(quest).copied().unwrap_or(false)
    }
    /// Explained unsupported preferences, deduplicated across bands and slots.
    pub fn fallback_notes(&self) -> &[String] {
        &self.fallback_notes
    }
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
    skipped: &[(i64, String, String)],
    known: &HashSet<i64>,
    starts: &[&crate::job::StartState],
) -> Result<Bis> {
    let mut bis = Bis {
        entries: Vec::new(),
        gives: vec![false; quests.len()],
        sellers: Vec::new(),
        vendor_items: Vec::new(),
        ..Bis::default()
    };
    if params.bis == BisMode::Off || power.items.is_empty() {
        return Ok(bis);
    }
    bis.requires = vec![false; quests.len()];
    // Preserve every giver until support is checked, with stable game-ID order within an item.
    let mut givers: Vec<Vec<u32>> = vec![vec![]; power.items.len()];
    for (q, (fixed, choices)) in rewards.iter().enumerate() {
        for &i in fixed.iter().chain(choices) {
            givers[i as usize].push(q as u32);
        }
    }
    for g in &mut givers {
        g.sort_by_key(|&i| quests[i as usize].id);
        g.dedup();
    }
    let mut selector = Selector::new(quests, &sources.dungeons, skipped, profile, params, known, starts);
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
            let mut candidates = Vec::new();
            for i in 0..power.items.len() as u16 {
                if !power.slots_of(i as usize).contains(&slot) {
                    continue;
                }
                let value = power.value_in(i as usize, level, slot);
                if value <= 0.0 {
                    continue;
                }
                let gettable = givers[i as usize].iter().any(|&q| quests[q as usize].level <= level);
                if !gettable && sellers[i as usize].is_empty() {
                    continue;
                }
                candidates.push((value, i));
            }
            candidates.sort_by(|a, b| b.0.total_cmp(&a.0).then(a.1.cmp(&b.1)));
            let mut rejected = Vec::new();
            let mut chosen = None;
            'items: for (_, item) in candidates {
                for &giver in &givers[item as usize] {
                    let q = &quests[giver as usize];
                    if q.level > level {
                        continue;
                    }
                    match selector.select(giver) {
                        Ok((plan, closure)) => {
                            selector.committed = closure;
                            bis.gives[giver as usize] = true;
                            if params.bis == BisMode::Required && !bis.required_roots.contains(&giver) {
                                bis.required_roots.push(giver);
                                for a in &plan.actions {
                                    if let BisQuestAction::TurnIn(i) = a {
                                        bis.requires[*i as usize] = true;
                                    }
                                }
                                bis.required_plans.push(plan);
                            }
                            chosen = Some((item, BisSource::Quest(giver), format!("{} ({})", q.name, q.id)));
                            break 'items;
                        }
                        Err(reason) => rejected.push(format!("{} ({}): {reason}", q.name, q.id)),
                    }
                }
                if !sellers[item as usize].is_empty() {
                    chosen = Some((
                        item,
                        BisSource::Vendor(sellers[item as usize].clone()),
                        format!("{} from vendors (optional purchase)", power.items[item as usize].name),
                    ));
                    break;
                }
            }
            let alternative = chosen
                .as_ref()
                .map_or("no supported alternative", |(_, _, name)| name.as_str());
            for rejection in rejected {
                let note = format!("Gear fallback: {rejection}; selected {alternative}.");
                if !bis.fallback_notes.contains(&note) {
                    bis.fallback_notes.push(note);
                }
            }
            if let Some((item, source, _)) = chosen {
                bis.entries.push(BisEntry {
                    level,
                    slot,
                    item,
                    source,
                });
            }
        }
    }
    let graph = selector.finalize().map_err(anyhow::Error::msg)?;
    bis.source_actions = graph.actions;
    bis.source_edges = graph.edges;
    bis.source_quests = graph.quests;
    bis.source_visits = graph.visits;
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

#[cfg(test)]
pub(crate) mod tests;
