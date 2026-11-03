//! Items sold in limited supply (the vendor may be out of them): the step at the place the
//! route buys it does not hold the guide (it stays listed along the way until the item is got:
//! the turn-in waits for it), and the other vendors and auction houses of the item the route
//! passes by offer it too, along the way, from the quest's level on (before the quest is taken:
//! the addon then counts the bags) to its turn-in. On a crowded server the auction houses come
//! first (many players selling it), else the vendors.

use super::phrase::{Arg, Phrase};
use super::types::{Step, StepKind};
use super::writer::StepWriter;
use crate::world::Pos;

/// A vendor or auction house is passed by when a step of the route is this close (yards).
const NEAR: f64 = 150.0;
/// Places offered on the way for one item, at most.
const MAX_TRIES: usize = 2;
/// Auction houses are offered from this crowd on (`Params::crowd_at`), before the vendors from
/// `AUCTION_FIRST`.
const AUCTION_CROWD: f64 = 0.25;
const AUCTION_FIRST: f64 = 0.5;

/// A place selling the item: a vendor, or an auction house (its auctioneers).
struct Shop {
    pos: Pos,
    zone: i64,
    who: Vec<(i64, String)>,
    auction: bool,
    stock: Option<(u32, f64)>,
}

/// Add the steps offering the item at the places passed by, with the XP the route has after
/// the step they follow.
pub(super) fn add_tries(writer: &StepWriter<'_>, steps: &mut Vec<Step>, xps: &mut Vec<i64>) {
    let (model, params) = (writer.planner.model, writer.planner.params);
    let mut tries: Vec<(usize, Step)> = Vec::new();
    let mut limited = Vec::new();
    for (i, step) in steps.iter().enumerate() {
        if step.kind != StepKind::Objective || step.bg || !step.buy {
            continue;
        }
        let (Some(q), Some(k)) = (step.quest, step.objective) else {
            continue;
        };
        let Some(quest) = model.index.get(&q).map(|&i| &model.quests[i]) else {
            continue;
        };
        let Some(o) = quest.objectives.get(usize::from(k)) else {
            continue;
        };
        if o.vendors.first().is_none_or(|v| v.stock.is_none()) {
            continue;
        }
        let Some(item) = step.target.as_ref().map(|t| Arg::Item(t.id, t.name.clone())) else {
            continue;
        };
        limited.push(i);
        let crowd = params.crowd_at(quest.level);
        let vendors = (o.vendors.iter()).map(|v| Shop {
            pos: v.pos,
            zone: v.zone,
            who: vec![(v.id, v.name.clone())],
            auction: false,
            stock: v.stock,
        });
        let auctions = (o.alts.iter())
            .filter(|a| !a.auction.is_empty() && crowd >= AUCTION_CROWD)
            .map(|a| Shop {
                pos: a.loc.pos,
                zone: a.loc.zone,
                who: a.auction.clone(),
                auction: true,
                stock: None,
            });
        let mut shops: Vec<Shop> = if crowd >= AUCTION_FIRST {
            auctions.chain(vendors).collect()
        } else {
            vendors.chain(auctions).collect()
        };
        // Not where the route buys it already.
        shops.retain(|shop| !steps[i].world.is_some_and(|w| near(w, &shop.pos)));
        // From the quest's level (or its accept) to its turn-in.
        let accepted = (steps[..i].iter())
            .rposition(|s| s.kind == StepKind::Accept && s.quest == Some(q))
            .map_or(0, |a| a + 1);
        let start = steps[..i]
            .iter()
            .position(|s| s.level >= quest.min_level)
            .unwrap_or(i)
            .min(accepted);
        let end = (steps[i..].iter())
            .position(|s| s.kind == StepKind::TurnIn && s.quest == Some(q))
            .map_or(steps.len(), |t| i + t);
        let n = Arg::Num(o.count.round().max(1.0) as i64);
        for (j, shop) in passed_by(steps, start..end, i, &shops) {
            let index = offer_index(steps, j);
            // The level and time of the step it follows.
            let at = &steps[index - 1];
            let mut offer = Step::bare(StepKind::Objective, at.level, at.time);
            offer.quest = Some(q);
            offer.quest_name.clone_from(&step.quest_name);
            offer.objective = Some(k);
            offer.slot = step.slot;
            offer.target.clone_from(&step.target);
            (offer.bg, offer.try_buy, offer.buy) = (true, true, true);
            for (id, name) in shop.who.iter().take(4) {
                offer.mobs.push(name.clone());
                offer.mob_ids.push(*id);
            }
            offer.say.push(if shop.auction {
                Phrase::new("objective.buy.try.auction")
                    .arg("who", item.clone())
                    .arg("n", n.clone())
            } else {
                Phrase::new("objective.buy.try")
                    .arg("who", item.clone())
                    .arg("vendor", Arg::Npc(shop.who[0].0, shop.who[0].1.clone()))
                    .arg("n", n.clone())
            });
            if let Some((count, restock)) = shop.stock {
                offer.say.push(
                    Phrase::new("suffix.stock")
                        .arg("n", Arg::Text(count.to_string()))
                        .arg("hours", Arg::Num((restock / 3600.0).round().max(1.0) as i64)),
                );
            }
            writer.put_at(&mut offer, &shop.pos, shop.zone);
            tries.push((index, offer));
        }
    }
    for i in limited {
        steps[i].bg = true;
    }
    // From the last, so that the places stay right; those after the same step in route order.
    tries.sort_by_key(|t| t.0);
    for (at, offer) in tries.into_iter().rev() {
        let xp = xps[at - 1];
        steps.insert(at, offer);
        xps.insert(at, xp);
    }
}

/// The places the steps of `window` (but the buy step `own`) pass by, each with the first step
/// passing by it, `MAX_TRIES` at most, in the order of `shops`.
fn passed_by<'s>(
    steps: &[Step],
    window: std::ops::Range<usize>,
    own: usize,
    shops: &'s [Shop],
) -> Vec<(usize, &'s Shop)> {
    (shops.iter())
        .filter_map(|shop| {
            let j = window
                .clone()
                .find(|&j| j != own && steps[j].world.is_some_and(|w| near(w, &shop.pos)))?;
            Some((j, shop))
        })
        .take(MAX_TRIES)
        .collect()
}

/// Where the offer for the shop passed by at step `j` goes in the guide: after it, but before a
/// flight or a link (boat, zeppelin, shortcut), whose place is where it starts: the character
/// has left by the time it is done.
fn offer_index(steps: &[Step], j: usize) -> usize {
    match steps[j].kind {
        StepKind::Fly | StepKind::Link if j > 0 => j,
        _ => j + 1,
    }
}

/// Whether a step at `world` ([continent, x, y]) passes by `pos`.
fn near(world: [f64; 3], pos: &Pos) -> bool {
    world[0] as i64 == pos.continent && (world[1] - pos.x).hypot(world[2] - pos.y) <= NEAR
}

#[cfg(test)]
mod tests {
    use super::*;

    fn at(x: f64, y: f64) -> Step {
        let mut s = Step::bare(StepKind::Objective, 20, 0.0);
        s.world = Some([0.0, x, y]);
        s
    }

    fn vendor(id: i64, x: f64, y: f64) -> Shop {
        Shop {
            pos: Pos {
                continent: 0,
                x,
                y,
                zone: 0,
                z: None,
            },
            zone: 0,
            who: vec![(id, String::new())],
            auction: false,
            stock: Some((2, 10800.0)),
        }
    }

    /// The places passed by between the accept and the turn-in, the first step near each, two
    /// at most; not the buy step itself, nor the other continent, nor far away.
    #[test]
    fn vendors_passed_by_on_the_way() {
        let steps = vec![
            at(0.0, 0.0),
            at(1000.0, 0.0),
            at(2000.0, 0.0),
            at(2050.0, 0.0),
            at(5000.0, 0.0),
        ];
        let vendors = [
            vendor(1, 2060.0, 30.0),
            vendor(2, 5000.0, 0.0),
            vendor(3, 990.0, 0.0),
            vendor(4, 9000.0, 0.0),
        ];
        let found: Vec<(usize, i64)> = passed_by(&steps, 1..5, 4, &vendors)
            .iter()
            .map(|(j, v)| (*j, v.who[0].0))
            .collect();
        assert_eq!(found, vec![(2, 1), (1, 3)]);
    }

    /// A flight or a link is placed where it starts and done where it ends: the offer for a shop
    /// at its start comes before it. Any other step is done where it is placed: the offer follows.
    #[test]
    fn offers_come_before_the_trips_they_pass_by() {
        let kinds = [
            StepKind::Accept,
            StepKind::Fly,
            StepKind::Link,
            StepKind::Hearth,
            StepKind::Teleport,
            StepKind::TurnIn,
        ];
        let steps: Vec<Step> = kinds.iter().map(|&k| Step::bare(k, 20, 0.0)).collect();
        let index: Vec<usize> = (0..steps.len()).map(|j| offer_index(&steps, j)).collect();
        // The hearthstone and the teleport are placed where they end: the offer follows them.
        assert_eq!(index, vec![1, 1, 2, 4, 5, 6]);
        // Nothing before a trip that starts the guide.
        let first = [Step::bare(StepKind::Fly, 20, 0.0)];
        assert_eq!(offer_index(&first, 0), 1);
    }

    /// An item of limited supply bought at a vendor, an auction house beside the flight master
    /// the route flies from before it: the offer for the auction house comes before the flight,
    /// not after it (GitHub #115). The offer takes the time of the step before it: the flight
    /// keeps its own, the time of its arrival.
    #[test]
    fn the_offer_comes_before_the_flight_leaving_its_city() {
        use crate::export::{Target, TargetKind};
        use crate::model::testing::{At, Obj, Q, Scenario};
        use crate::plan::Planner;
        use std::collections::HashSet;
        let place = |zone, x, y| At { zone, x, y };
        let p = Scenario::new()
            .levels(1, 12)
            .zone(2, "Durotar")
            .zone(3, "The Barrens")
            .params(|p| p.crowded = 0.3)
            .quest(
                Q::new(900, "Supplies")
                    .level(10)
                    .min_level(8)
                    .giver(3, 90.0, 90.0)
                    .objective(Obj::bought(
                        "Thick Bandage",
                        5,
                        place(3, 88.0, 88.0),
                        place(3, 50.0, 50.0),
                    )),
            )
            .build();
        let planner = Planner::new(&p.model, &p.world, &p.params, &p.profile);
        let writer = StepWriter {
            planner: &planner,
            route: &[],
            background: HashSet::new(),
        };
        let objective = &p.model.quests[p.model.index[&900]].objectives[0];
        let world = |pos: &Pos| Some([pos.continent as f64, pos.x, pos.y]);
        let (vendor, auction) = (objective.vendors[0].pos, objective.alts[0].loc.pos);

        let mut accept = Step::bare(StepKind::Accept, 8, 10.0);
        accept.quest = Some(900);
        accept.world = Some([vendor.continent as f64, 5000.0, 5000.0]);
        // Placed at the flight master where it starts, done when it lands.
        let mut fly = Step::bare(StepKind::Fly, 8, 200.0);
        fly.world = world(&auction);
        let mut buy = Step::bare(StepKind::Objective, 8, 260.0);
        buy.quest = Some(900);
        buy.objective = Some(0);
        buy.buy = true;
        buy.world = world(&vendor);
        buy.target = Some(Target {
            kind: TargetKind::Item,
            id: 1,
            name: "Thick Bandage".to_owned(),
        });
        let mut turn_in = Step::bare(StepKind::TurnIn, 8, 300.0);
        turn_in.quest = Some(900);
        turn_in.world = Some([vendor.continent as f64, 5000.0, 5000.0]);
        let mut steps = vec![accept, fly, buy, turn_in];
        let mut xps = vec![0; steps.len()];

        add_tries(&writer, &mut steps, &mut xps);

        let kinds: Vec<(StepKind, bool)> = steps.iter().map(|s| (s.kind, s.try_buy)).collect();
        assert_eq!(
            kinds,
            vec![
                (StepKind::Accept, false),
                (StepKind::Objective, true),
                (StepKind::Fly, false),
                (StepKind::Objective, false),
                (StepKind::TurnIn, false),
            ]
        );
        // The offer is done when the step before it is; the flight keeps the time it lands.
        assert_eq!(steps[1].time, 10.0);
        assert_eq!(steps[2].time, 200.0);
    }
}
