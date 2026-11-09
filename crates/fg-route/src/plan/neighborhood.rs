//! Bounded exact repair of a small, nearby set of optional outdoor quests.

use super::Planner;
use super::evaluate::Run;
use super::moves::Change;
use super::state::{Kind, Stop};
use crate::model::Objective;
use rand::RngExt;
use rand::rngs::SmallRng;
use rustc_hash::FxHashMap;

impl Planner<'_> {
    pub(crate) fn repair_nearby_quests(&self, route: &mut Vec<Stop>, run: &Run, rng: &mut SmallRng) -> Option<Change> {
        let eligible: Vec<(usize, u32)> = route
            .iter()
            .enumerate()
            .filter(|(_, s)| s.kind == Kind::Accept && self.complete_repair_quest(route, s.index))
            .map(|(at, s)| (at, s.index))
            .collect();
        if eligible.len() < 2 {
            return None;
        }
        let (anchor_at, anchor) = eligible[rng.random_range(0..eligible.len())];
        let zone = self.places().zone(Stop::quest(anchor, Kind::Accept));
        let s0 = self.sim().initial_state();
        let anchor_pos = self.sim().pos_from_start(&s0, Stop::quest(anchor, Kind::Accept));
        let mut nearby: Vec<_> = eligible
            .into_iter()
            .filter(|&(at, q)| {
                q != anchor && at.abs_diff(anchor_at) <= 80 && self.places().zone(Stop::quest(q, Kind::Accept)) == zone
            })
            .map(|(at, q)| {
                let pos = self.sim().pos_from_start(&s0, Stop::quest(q, Kind::Accept));
                (anchor_pos.dist(&pos), at, q)
            })
            .filter(|(distance, _, _)| distance.is_finite())
            .collect();
        nearby.sort_by(|a, b| a.0.total_cmp(&b.0).then(a.1.cmp(&b.1)));
        let mut selected = vec![anchor];
        selected.extend(nearby.into_iter().take(2).map(|(_, _, q)| q));
        if selected.len() < 2 {
            return None;
        }

        // Retain the original run's level before each surviving stop; insertion updates both
        // these arrays as it adds stops, without approximating the leaf's final score.
        let mut kept = Vec::with_capacity(route.len());
        let mut levels = Vec::with_capacity(route.len());
        let mut positions = Vec::with_capacity(route.len());
        for (at, &stop) in route.iter().enumerate() {
            if !(stop.is_quest() && selected.contains(&stop.index)) {
                kept.push(stop);
                levels.push(run.states[at.min(run.states.len() - 1)].level);
                positions.push(self.sim().pos_from_start(&s0, stop));
            }
        }
        let leaves = self.repair_orders(&selected).into_iter().filter_map(|order| {
            let (mut candidate, mut levels, mut positions) = (kept.clone(), levels.clone(), positions.clone());
            for q in order {
                self.insert_quest_leveled(&mut candidate, &mut levels, &mut positions, &s0, q);
            }
            same_stops(route, &candidate).then_some(candidate)
        });
        let best = self.best_repair_leaf(leaves, run.total)?;
        let first = route.iter().zip(&best).position(|(a, b)| a != b)?;
        *route = best;
        Some(Change {
            first,
            last: route.len(),
            reordered: false,
        })
    }

    fn complete_repair_quest(&self, route: &[Stop], q: u32) -> bool {
        let quest = self.quest(q);
        let initial = &self.model.initial;
        if quest.mandatory
            || quest.required
            || quest.time_limit.is_some()
            || !quest.pre_held.is_empty()
            || quest.start_dungeon.is_some()
            || quest.starts.iter().chain(&quest.ends).any(|l| l.inside.is_some())
            || quest
                .objectives
                .iter()
                .flat_map(Objective::places)
                .any(|o| o.dungeon_of().is_some() || !o.handed_by.is_empty() || o.lasts.is_some())
            || initial.accepted.contains(&(q as usize))
            || initial.turned.contains(&(q as usize))
            || initial.log.iter().any(|(id, _)| *id == quest.id)
            || initial.progress.iter().any(|&(i, _, _)| i == q as usize)
        {
            return false;
        }
        let mut accepts = 0;
        let mut turn_ins = 0;
        let mut objectives = vec![0; quest.objectives.len()];
        for stop in route.iter().filter(|s| s.is_quest() && s.index == q) {
            match stop.kind {
                Kind::Accept => accepts += 1,
                Kind::TurnIn => turn_ins += 1,
                Kind::Objective(k) => {
                    let Some(count) = objectives.get_mut(usize::from(k)) else {
                        return false;
                    };
                    *count += 1;
                }
                _ => unreachable!("quest stops only"),
            }
        }
        accepts == 1 && turn_ins == 1 && objectives.iter().all(|&count| count == 1)
    }

    fn repair_orders(&self, selected: &[u32]) -> Vec<Vec<u32>> {
        let mut orders = Vec::new();
        for &a in selected {
            for &b in selected.iter().filter(|&&b| b != a) {
                let mut order = vec![a, b];
                order.extend(selected.iter().copied().filter(|&q| q != a && q != b));
                self.prerequisites_first(&mut order);
                if !orders.contains(&order) {
                    orders.push(order);
                }
            }
        }
        orders
    }

    fn best_repair_leaf(&self, leaves: impl IntoIterator<Item = Vec<Stop>>, baseline: f64) -> Option<Vec<Stop>> {
        let mut best = None;
        let mut best_score = baseline - 0.5;
        for candidate in leaves {
            if self.params.start_zone != 0
                && candidate
                    .first()
                    .is_some_and(|&s| self.places().zone(s) != self.params.start_zone)
            {
                continue;
            }
            if let Some(score) = self.simulate(&candidate, None)
                && score.is_finite()
                && score < best_score
            {
                best_score = score;
                best = Some(candidate);
            }
        }
        best
    }
}

/// Include the objective number as well as the kind discriminant: all objective stops have the
/// same discriminant, and a reconstruction must not replace one objective with another.
fn same_stops(a: &[Stop], b: &[Stop]) -> bool {
    if a.len() != b.len() {
        return false;
    }
    let key = |s: &Stop| {
        (
            s.index,
            std::mem::discriminant(&s.kind),
            match s.kind {
                Kind::Objective(k) => k,
                _ => 0,
            },
        )
    };
    let mut counts = FxHashMap::default();
    for stop in a {
        *counts.entry(key(stop)).or_insert(0_usize) += 1;
    }
    for stop in b {
        let Some(count) = counts.get_mut(&key(stop)) else {
            return false;
        };
        if *count == 0 {
            return false;
        }
        *count -= 1;
    }
    true
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::job::Prepared;
    use crate::testing::{At, Obj, Q, Scenario};
    use rand::SeedableRng;

    fn fixture() -> Prepared {
        let mut p = Scenario::new()
            .levels(1, 10)
            .dungeon("Unused cave", 1, 50.0, 50.0)
            .quest(quest(1, 10.0))
            .quest(quest(2, 90.0))
            .quest(quest(3, 20.0))
            .build();
        p.params.camp_radius = 0.0;
        p.params.farm_on_way = false;
        p
    }

    fn quest(id: i64, x: f64) -> Q {
        Q::new(id, "Visit")
            .level(1)
            .xp(0)
            .giver(1, x, 50.0)
            .objective(Obj::visit("Talk", At { zone: 1, x, y: 50.0 }))
    }

    fn planner(p: &Prepared) -> Planner<'_> {
        Planner::new(&p.model, &p.world, &p.params, &p.profile)
    }

    fn route() -> Vec<Stop> {
        // Valid but wasteful: revisit every hub separately for accept, work, and turn-in.
        [Kind::Accept, Kind::Objective(0), Kind::TurnIn]
            .into_iter()
            .flat_map(|kind| (0..3).map(move |q| Stop::quest(q, kind)))
            .collect()
    }

    fn route_of(quests: &[u32]) -> Vec<Stop> {
        quests
            .iter()
            .copied()
            .flat_map(|q| [Kind::Accept, Kind::Objective(0), Kind::TurnIn].map(|k| Stop::quest(q, k)))
            .collect()
    }

    fn assert_same_stops(a: &[Stop], b: &[Stop]) {
        assert_eq!(a.len(), b.len());
        for stop in a {
            assert_eq!(
                a.iter().filter(|s| *s == stop).count(),
                b.iter().filter(|s| *s == stop).count()
            );
        }
    }

    // A no-op implementation must fail: crossing the zone three times has a cheaper reconstruction.
    #[test]
    fn nearby_repair_improves_exact_score_and_preserves_every_stop() {
        let p = fixture();
        let planner = planner(&p);
        let mut route = route();
        let before = route.clone();
        let run = planner.run(&route).unwrap();
        let change = planner
            .repair_nearby_quests(&mut route, &run, &mut SmallRng::seed_from_u64(42))
            .unwrap();
        assert!(planner.simulate(&route, None).unwrap() < run.total - 0.5);
        assert_same_stops(&before, &route);
        assert!(!change.reordered);
        assert_eq!(change.last, route.len());
        assert_eq!(before[..change.first], route[..change.first]);
    }

    // Rejection must leave the caller's original allocation contents unchanged.
    #[test]
    fn nearby_repair_without_gain_is_atomic() {
        let mut p = fixture();
        let loc = p.model.start.clone();
        for q in &mut p.model.quests {
            q.starts = vec![loc.clone()];
            q.ends = vec![loc.clone()];
            q.objectives[0].loc = loc.clone();
        }
        let planner = planner(&p);
        let mut route = route();
        let before = route.clone();
        let run = planner.run(&route).unwrap();
        assert!(
            planner
                .repair_nearby_quests(&mut route, &run, &mut SmallRng::seed_from_u64(42))
                .is_none()
        );
        assert_eq!(route, before);
    }

    // An incomplete or protected quest cannot be silently rebuilt with extra/missing stops.
    #[test]
    fn repair_eligibility_rejects_partial_duplicate_and_protected_quests() {
        let p = fixture();
        let all = route();
        assert!(planner(&p).complete_repair_quest(&all, 0));
        let own: Vec<usize> = all
            .iter()
            .enumerate()
            .filter(|(_, s)| s.index == 0)
            .map(|(at, _)| at)
            .collect();
        for &missing in &own {
            let mut partial = all.clone();
            partial.remove(missing);
            assert!(!planner(&p).complete_repair_quest(&partial, 0));
        }
        for &duplicate in &own {
            let mut twice = all.clone();
            twice.push(all[duplicate]);
            assert!(!planner(&p).complete_repair_quest(&twice, 0));
        }
        let mut bad_objective = all.clone();
        bad_objective.push(Stop::quest(0, Kind::Objective(1)));
        assert!(!planner(&p).complete_repair_quest(&bad_objective, 0));
        for excluded in 0..14 {
            let mut p = fixture();
            let q = &mut p.model.quests[0];
            match excluded {
                0 => q.mandatory = true,
                1 => q.required = true,
                2 => q.time_limit = Some(1000.0),
                3 => q.pre_held = vec![2],
                4 => q.objectives[0].handed_by = vec![2],
                5 => q.start_dungeon = Some(0),
                6 => q.starts[0].inside = Some(0),
                7 => q.objectives[0].dungeon = Some(0),
                8 => p.model.initial.accepted = vec![0],
                9 => p.model.initial.log = vec![(1, "already held".into())],
                10 => q.ends[0].inside = Some(0),
                11 => p.model.initial.progress = vec![(0, 0, 0.5)],
                12 => q.objectives[0].lasts = Some(300.0),
                _ => p.model.initial.turned = vec![0],
            }
            assert!(!planner(&p).complete_repair_quest(&all, 0), "exclusion {excluded}");
        }
    }

    // A timed/mandatory quest cannot enter the removed set even if it is near the anchor.
    #[test]
    fn ineligible_neighborhood_is_unchanged() {
        let mut p = fixture();
        p.model.quests[0].mandatory = true;
        p.model.quests[1].time_limit = Some(10000.0);
        let planner = planner(&p);
        let mut route = route();
        let before = route.clone();
        let run = planner.run(&route).unwrap();
        assert!(
            planner
                .repair_nearby_quests(&mut route, &run, &mut SmallRng::seed_from_u64(42))
                .is_none()
        );
        assert_eq!(route, before);
    }

    // Prerequisite normalization must not consume the six-leaf budget on duplicate orders.
    #[test]
    fn repair_orders_are_bounded_unique_and_prerequisites_first() {
        let mut p = fixture();
        let plan = planner(&p);
        let orders = plan.repair_orders(&[0, 1, 2]);
        assert_eq!(orders.len(), 6);
        for (i, order) in orders.iter().enumerate() {
            assert!(!orders[..i].contains(order));
            assert_eq!(order.len(), 3);
        }
        p.model.quests[1].pre_all = vec![1];
        p.model.quests[2].pre_all = vec![2];
        assert_eq!(planner(&p).repair_orders(&[2, 1, 0]), vec![vec![0, 1, 2]]);
    }

    // Seeding the operator must control both anchor selection and the resulting route.
    #[test]
    fn nearby_repair_is_seeded_and_reproducible() {
        let p = fixture();
        let planner = planner(&p);
        let mut a = route();
        let mut b = a.clone();
        let run = planner.run(&a).unwrap();
        assert!(
            planner
                .repair_nearby_quests(&mut a, &run, &mut SmallRng::seed_from_u64(1337))
                .is_some()
        );
        assert!(
            planner
                .repair_nearby_quests(&mut b, &run, &mut SmallRng::seed_from_u64(1337))
                .is_some()
        );
        assert_eq!(a, b);
    }

    // Ranking before validity would discard the valid second-best reconstruction.
    #[test]
    fn cheaper_wrong_start_zone_does_not_mask_valid_improvement() {
        let mut p = Scenario::new()
            .levels(1, 10)
            .zone(2, "Other")
            .quest(quest(1, 10.0))
            .quest(quest(2, 10.0).giver(2, 10.0, 50.0))
            .quest(quest(3, 20.0))
            .build();
        // Keep each objective beside its giver, including the quest in zone 2.
        for q in &mut p.model.quests {
            q.objectives[0].loc = q.starts[0].clone();
        }
        p.params.start_zone = 1;
        p.params.camp_radius = 0.0;
        p.params.farm_on_way = false;
        let planner = planner(&p);
        let baseline = planner.simulate(&route(), None).unwrap();
        let invalid = route_of(&[1, 0, 2]);
        let valid = route_of(&[2, 1, 0]);
        assert!(planner.simulate(&invalid, None).unwrap() < planner.simulate(&valid, None).unwrap());
        assert!(planner.simulate(&valid, None).unwrap() < baseline - 0.5);
        assert_eq!(
            planner.best_repair_leaf([invalid, valid.clone()], baseline),
            Some(valid)
        );
    }

    // Moving a selected prerequisite behind a retained follower must be refused by full replay.
    #[test]
    fn nearby_repair_keeps_retained_followers_valid() {
        let mut p = Scenario::new()
            .levels(1, 10)
            .quest(quest(1, 10.0))
            .quest(quest(2, 90.0))
            .quest(quest(3, 20.0).after(&[1]).timed(10000.0))
            .quest(quest(4, 10.0).mandatory())
            .build();
        p.params.camp_radius = 0.0;
        p.params.farm_on_way = false;
        let planner = planner(&p);
        let mut route: Vec<_> = [Kind::Accept, Kind::Objective(0), Kind::TurnIn]
            .into_iter()
            .flat_map(|kind| [Stop::quest(0, kind), Stop::quest(1, kind)])
            .collect();
        route.extend(route_of(&[3, 2]));
        let before = route.clone();
        let run = planner.run(&route).unwrap();
        assert!(planner.complete_repair_quest(&route, 0));
        assert!(planner.complete_repair_quest(&route, 1));
        assert!(!planner.complete_repair_quest(&route, 2));
        assert!(
            planner
                .repair_nearby_quests(&mut route, &run, &mut SmallRng::seed_from_u64(42))
                .is_some()
        );
        assert!(planner.simulate(&route, None).is_some());
        assert_same_stops(&before, &route);
        let parent = route.iter().position(|s| *s == Stop::quest(0, Kind::TurnIn)).unwrap();
        let child = route.iter().position(|s| *s == Stop::quest(2, Kind::Accept)).unwrap();
        assert!(parent < child);
    }

    // Repair may improve outdoor work while retaining mandatory and dungeon content verbatim.
    #[test]
    fn nearby_repair_preserves_unrelated_mandatory_and_dungeon_stops() {
        let mut p = Scenario::new()
            .levels(1, 10)
            .dungeon("Cave", 1, 50.0, 50.0)
            .quest(quest(1, 10.0))
            .quest(quest(2, 90.0))
            .quest(quest(3, 20.0))
            .quest(Q::new(4, "Required").level(1).xp(0).mandatory())
            .quest(
                Q::new(5, "Cave quest").level(1).xp(0).objective(
                    Obj::visit(
                        "Inside",
                        At {
                            zone: 1,
                            x: 50.0,
                            y: 50.0,
                        },
                    )
                    .inside(0),
                ),
            )
            .build();
        p.params.camp_radius = 0.0;
        p.params.farm_on_way = false;
        let planner = planner(&p);
        let mut route = route();
        route.extend([Stop::quest(3, Kind::Accept), Stop::quest(3, Kind::TurnIn)]);
        route.extend(route_of(&[4]));
        let before = route.clone();
        let run = planner.run(&route).unwrap();
        assert!(
            planner
                .repair_nearby_quests(&mut route, &run, &mut SmallRng::seed_from_u64(42))
                .is_some()
        );
        assert_same_stops(&before, &route);
        let retained = |stops: &[Stop]| stops.iter().copied().filter(|s| s.index >= 3).collect::<Vec<_>>();
        assert_eq!(retained(&before), retained(&route));
        assert!(planner.simulate(&route, None).is_some());
    }
}
