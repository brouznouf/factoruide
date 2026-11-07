//! What a planner looks up all the time and computes once from the model: quests blocking or
//! following each other, the objectives inside each dungeon, the quest NPCs and the vendors by
//! cell. Fixed once built, shared by the planners of the same model and settings
//! (`Planner::replaying`).

use crate::model::{EntityKind, Model};
use crate::params::Params;
use crate::world::Pos;
use rustc_hash::FxHashMap;

/// Quest NPCs by cell (continent, x, y): (quest, index in its starts or ends, giver?, and the
/// NPC's x, y, to tell the ones at hand without reading the quest).
pub(crate) type CampIndex = FxHashMap<(i64, i64, i64), Vec<(u32, u8, bool, f64, f64)>>;
/// Vendors (index in `Model::vendors`) by cell of `Params::sell_radius`: (continent, x, y).
pub(crate) type VendorIndex = FxHashMap<(i64, i64, i64), Vec<u32>>;

pub(crate) struct Indexes {
    /// Quest givers and turn-in NPCs by cell of `camp_radius` (continent, x, y): (quest,
    /// index in its starts or ends, giver?).
    pub(crate) camp: CampIndex,
    /// Vendors by cell of `sell_radius` (continent, x, y).
    pub(crate) vendors: VendorIndex,
    /// Quests each quest blocks once taken: those exclusive with it, the breadcrumbs to it.
    pub(crate) blocks: Vec<Vec<u32>>,
    /// The reverse: quests whose taking blocks each quest.
    pub(crate) blocked_by: Vec<Vec<u32>>,
    /// Quests that need each quest done first (the rest of its chain, one step at a time).
    pub(crate) followers: Vec<Vec<u32>>,
    /// The breadcrumbs to each quest the route must do (`mandatory`): taken first, the quest
    /// closing them once taken.
    pub(crate) breadcrumbs: Vec<Vec<u32>>,
    /// The objectives inside each dungeon: (quest, objective).
    pub(crate) in_dungeon: Vec<Vec<(u32, u8)>>,
}

impl Indexes {
    pub(crate) fn new(model: &Model, params: &Params) -> Self {
        let blocks = blocks(model);
        let mut blocked_by = vec![Vec::new(); blocks.len()];
        for (x, ys) in blocks.iter().enumerate() {
            for &y in ys {
                blocked_by[y as usize].push(x as u32);
            }
        }
        let mut in_dungeon = vec![Vec::new(); model.dungeons.len()];
        for (i, q) in model.quests.iter().enumerate() {
            for (k, o) in q.objectives.iter().enumerate() {
                if let Some(d) = o.dungeon_of() {
                    in_dungeon[d].push((i as u32, k as u8));
                }
            }
        }
        Self {
            camp: camp_index(model, params.camp_radius),
            vendors: vendor_index(model, params.sell_radius),
            blocks,
            blocked_by,
            followers: followers(model),
            breadcrumbs: mandatory_breadcrumbs(model),
            in_dungeon,
        }
    }
}

/// Quests needing each quest first (it is in their `pre_all` or `pre_any`, or hands an item
/// one of their objectives asks for).
fn followers(model: &Model) -> Vec<Vec<u32>> {
    let mut out = vec![Vec::new(); model.quests.len()];
    for (f, q) in model.quests.iter().enumerate() {
        let handing = q.objectives.iter().flat_map(|o| &o.handed_by);
        for id in q.pre_all.iter().chain(&q.pre_any).chain(&q.pre_held).chain(handing) {
            if let Some(&i) = model.index.get(id)
                && i != f
                && !out[i].contains(&(f as u32))
            {
                out[i].push(f as u32);
            }
        }
    }
    out
}

/// The mandatory breadcrumbs to each quest.
fn mandatory_breadcrumbs(model: &Model) -> Vec<Vec<u32>> {
    let mut out = vec![Vec::new(); model.quests.len()];
    for (b, q) in model.quests.iter().enumerate().filter(|(_, q)| q.mandatory) {
        if let Some(&t) = q.breadcrumb_for.as_ref().and_then(|id| model.index.get(id))
            && t != b
        {
            out[t].push(b as u32);
        }
    }
    out
}

/// Quests each quest blocks once taken: the quests exclusive with it, the breadcrumbs to it.
fn blocks(model: &Model) -> Vec<Vec<u32>> {
    let mut blocks = vec![Vec::new(); model.quests.len()];
    for (y, q) in model.quests.iter().enumerate() {
        for id in q.exclusive.iter().chain(q.breadcrumb_for.iter()) {
            if let Some(&x) = model.index.get(id)
                && x != y
            {
                blocks[x].push(y as u32);
            }
        }
    }
    blocks
}

fn vendor_index(model: &Model, radius: f64) -> VendorIndex {
    let mut index = VendorIndex::default();
    if radius > 0.0 {
        for (i, v) in model.vendors.iter().enumerate() {
            let cell = (
                v.pos.continent,
                (v.pos.x / radius).floor() as i64,
                (v.pos.y / radius).floor() as i64,
            );
            index.entry(cell).or_default().push(i as u32);
        }
    }
    index
}

/// Quest givers (NPCs and objects, not items) and turn-in NPCs by cell of `radius`.
fn camp_index(model: &Model, radius: f64) -> CampIndex {
    let mut index = CampIndex::default();
    if radius <= 0.0 {
        return index;
    }
    let cell = |p: &Pos| {
        (
            p.continent,
            (p.x / radius).floor() as i64,
            (p.y / radius).floor() as i64,
        )
    };
    for (i, q) in model.quests.iter().enumerate() {
        for (at, loc) in q.starts.iter().enumerate().filter(|(_, l)| l.kind != EntityKind::Item) {
            index
                .entry(cell(&loc.pos))
                .or_default()
                .push((i as u32, at as u8, true, loc.pos.x, loc.pos.y));
        }
        for (at, loc) in q.ends.iter().enumerate() {
            index
                .entry(cell(&loc.pos))
                .or_default()
                .push((i as u32, at as u8, false, loc.pos.x, loc.pos.y));
        }
    }
    index
}
