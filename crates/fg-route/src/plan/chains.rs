//! Bounded insertion of an omitted optional quest and its missing prerequisites.

use super::Planner;
use super::evaluate::Run;
use super::moves::Change;
use super::state::{Kind, State, Stop};
use rand::RngExt;
use rand::rngs::SmallRng;

impl Planner<'_> {
    pub(crate) fn add_optional_chain(&self, route: &mut Vec<Stop>, run: &Run, rng: &mut SmallRng) -> Option<Change> {
        let initial = self.sim().initial_state();
        let present = RouteQuests::new(route, self.model.quests.len());
        let omitted: Vec<u32> = (0..self.model.quests.len() as u32)
            .filter(|&q| {
                !self.quest(q).mandatory
                    && !initial.accepted[q as usize]
                    && !initial.rewarded(q as usize)
                    && !present.present[q as usize]
            })
            .collect();
        let endpoint = *omitted.get(rng.random_range(0..omitted.len().max(1)))?;
        self.insert_optional_chain(route, run, endpoint)
    }

    pub(crate) fn insert_optional_chain(&self, route: &mut Vec<Stop>, run: &Run, endpoint: u32) -> Option<Change> {
        let quest = self.model.quests.get(endpoint as usize)?;
        let initial = self.sim().initial_state();
        let present = RouteQuests::new(route, self.model.quests.len());
        if quest.mandatory
            || initial.accepted[endpoint as usize]
            || initial.rewarded(endpoint as usize)
            || present.present[endpoint as usize]
        {
            return None;
        }
        let collector = Closure {
            planner: self,
            present: &present,
            initial: &initial,
        };
        let quests = collector.solve(
            vec![Need::Quest(endpoint, 0, Dependency::Normal)],
            Collected::default(),
            &mut 0,
        )?;
        let mut candidate = route.clone();
        let mut levels: Vec<i64> = (0..route.len())
            .map(|i| {
                run.states
                    .get(i)
                    .or_else(|| run.states.last())
                    .map_or(initial.level, |s| s.level)
            })
            .collect();
        let mut places = route.iter().map(|&s| self.sim().pos_from_start(&initial, s)).collect();
        let mut first = route.len();
        for q in quests.order {
            // A linked insertion may already have supplied another member of the closure.
            if candidate.iter().any(|s| s.is_quest() && s.index == q) {
                continue;
            }
            let ready = collector.ready(&candidate, q)?;
            let quest = self.quest(q);
            let linked = !quest.pre_held.is_empty() || quest.objectives.iter().any(|o| !o.handed_by.is_empty());
            if linked || ready <= self.after_prerequisites(&candidate, q) {
                first = first.min(self.insert_quest_leveled(&mut candidate, &mut levels, &mut places, &initial, q));
                // Keep the full insertion context whenever its existing floor suffices.
                // Linked insertion also needs it to locate existing partners; discard a
                // linked layout which violates the exact completion floor.
                if Self::position_of(&candidate, q, Kind::Accept)
                    .is_some_and(|at| at < collector.ready(&candidate, q).unwrap_or(usize::MAX))
                {
                    return None;
                }
            } else {
                // The older helper's floor omits completed IDs outside the model. Preserve
                // the exact ready prefix, then use its existing leveled insertion on the suffix.
                let mut suffix = candidate.split_off(ready);
                let mut tail_levels = levels.split_off(ready);
                let mut tail_places = places.split_off(ready);
                let at = self.insert_quest_leveled(&mut suffix, &mut tail_levels, &mut tail_places, &initial, q);
                first = first.min(ready + at);
                candidate.extend(suffix);
                levels.extend(tail_levels);
                places.extend(tail_places);
            }
            let mut added = vec![false; self.model.quests.len()];
            for s in candidate.iter().filter(|s| s.is_quest()) {
                if !present.present[s.index as usize] {
                    added[s.index as usize] = true;
                }
            }
            if added.iter().filter(|&&is_new| is_new).count() > MAX_QUESTS {
                return None;
            }
        }
        // Linked machinery can move an existing parent's later stops too. Compare the final
        // prefix instead of relying solely on the individual insertion's changed index.
        first = first.min(
            route
                .iter()
                .zip(&candidate)
                .position(|(a, b)| a != b)
                .unwrap_or(route.len()),
        );
        let change = Change {
            first,
            last: route.len().max(candidate.len()),
            reordered: false,
        };
        *route = candidate;
        Some(change)
    }
}

const MAX_QUESTS: usize = 8;
const MAX_DEPTH: usize = 16;
const MAX_VISITS: usize = 64;

#[derive(Clone, Copy)]
enum Need {
    /// A held dependency only needs an active parent's accept, permitting a parent to hand
    /// work to a child which, in turn, hands its objective item back to the parent.
    Quest(u32, usize, Dependency),
    Accepted(u32),
    Finish(u32),
    Any(u32, usize, usize),
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum Dependency {
    Normal,
    Held,
    Handed,
}

#[derive(Clone, Default)]
struct Collected {
    selected: Vec<u32>,
    active: Vec<u32>,
    accepted: Vec<u32>,
    order: Vec<u32>,
}

struct RouteQuests {
    present: Vec<bool>,
    turned: Vec<bool>,
}

impl RouteQuests {
    fn new(route: &[Stop], n: usize) -> Self {
        let mut out = Self {
            present: vec![false; n],
            turned: vec![false; n],
        };
        for s in route.iter().filter(|s| s.is_quest()) {
            out.present[s.index as usize] = true;
            out.turned[s.index as usize] |= s.kind == Kind::TurnIn;
        }
        out
    }
}

struct Closure<'p, 'a> {
    planner: &'p Planner<'a>,
    present: &'p RouteQuests,
    initial: &'p State,
}

impl Closure<'_, '_> {
    /// Structural prerequisite satisfaction follows the simulator's cardinality and initial
    /// completion rules. A route turn-in is reused; its actual feasibility remains the replay's.
    fn members(&self, owner: u32) -> impl Iterator<Item = usize> + '_ {
        let init = &self.planner.model.initial;
        (0..=init.mates.len()).filter(move |m| init.mates.is_empty() || init.need(owner as usize) & (1 << m) != 0)
    }

    fn done(&self, id: i64, owner: u32, collected: &Collected) -> bool {
        self.members(owner)
            .all(|member| self.done_using(id, owner, collected, true, member))
    }

    fn any_met(&self, owner: u32, collected: &Collected) -> bool {
        self.members(owner).all(|member| {
            self.planner
                .quest(owner)
                .pre_any_met(|id| self.done_using(*id, owner, collected, true, member))
        })
    }

    fn done_using(&self, id: i64, owner: u32, collected: &Collected, include_route: bool, member: usize) -> bool {
        let p = self.planner;
        let q = p.quest(owner);
        let known = !q.required
            && !q.zone_forced
            && p.model.known_class_prerequisites.contains(&id)
            && p.model
                .index
                .get(&id)
                .is_none_or(|&i| !p.model.quests[i].required && !p.model.quests[i].zone_forced);
        if known {
            return true;
        }
        if let Some(&i) = p.model.index.get(&id)
            && (self.initial.rewarded(i)
                || include_route && self.present.turned[i]
                || collected.order.contains(&(i as u32)))
        {
            return true;
        }
        let init = &p.model.initial;
        if init.mates.is_empty() {
            !p.model.index.contains_key(&id) && init.done_outside(id, p.profile.from_level)
        } else {
            init.done_by(member, id)
        }
    }

    fn ready(&self, route: &[Stop], q: u32) -> Option<usize> {
        let p = self.planner;
        let quest = p.quest(q);
        let mut ready = 0;
        // Each member may meet OR with a different set of prior completions. The route must
        // wait until every needed member meets its own cardinality, rather than intersecting
        // their completed IDs before counting alternatives.
        for member in self.members(q) {
            let at = |id: &i64| {
                if self.done_using(*id, q, &Collected::default(), false, member) {
                    return Some(0);
                }
                let &i = p.model.index.get(id)?;
                Planner::position_of(route, i as u32, Kind::TurnIn).map(|at| at + 1)
            };
            for id in &quest.pre_all {
                ready = ready.max(at(id)?);
            }
            let need = quest.pre_need.min(quest.pre_any.len());
            if need > 0 {
                let mut any: Vec<_> = quest.pre_any.iter().filter_map(at).collect();
                any.sort_unstable();
                ready = ready.max(*any.get(need - 1)?);
            }
        }
        Some(ready)
    }

    fn conflicts(&self, q: u32, collected: &Collected) -> bool {
        let p = self.planner;
        let quest = p.quest(q);
        let present = |i: usize| {
            self.initial.accepted[i]
                || self.initial.rewarded(i)
                || self.present.present[i]
                || collected.selected.contains(&(i as u32))
        };
        quest.exclusive.iter().any(|id| {
            p.model.index.get(id).is_some_and(|&i| present(i))
                || p.model.initial.completed.as_ref().is_some_and(|c| c.contains(id))
        }) || p
            .model
            .quests
            .iter()
            .enumerate()
            .any(|(i, other)| i != q as usize && other.exclusive.contains(&quest.id) && present(i))
    }

    /// Backtracking includes the remaining AND prerequisites and OR combinations, so an early
    /// locally legal alternative cannot hide a later compatible complete closure. Visits are
    /// shared by all branches; every branch works on a clone of at most eight chosen quests.
    fn solve(&self, mut pending: Vec<Need>, mut out: Collected, visits: &mut usize) -> Option<Collected> {
        while let Some(need) = pending.pop() {
            match need {
                Need::Accepted(q) => out.accepted.push(q),
                Need::Finish(q) => {
                    out.active.retain(|&i| i != q);
                    out.order.push(q);
                }
                Need::Any(q, depth, cursor) => {
                    let quest = self.planner.quest(q);
                    if self.any_met(q, &out) {
                        continue;
                    }
                    for (k, id) in quest.pre_any.iter().enumerate().skip(cursor) {
                        if self.done(*id, q, &out) {
                            continue;
                        }
                        let Some(&i) = self.planner.model.index.get(id) else {
                            continue;
                        };
                        let mut branch = pending.clone();
                        branch.push(Need::Any(q, depth, k + 1));
                        branch.push(Need::Quest(i as u32, depth + 1, Dependency::Normal));
                        if let Some(done) = self.solve(branch, out.clone(), visits) {
                            return Some(done);
                        }
                        if *visits >= MAX_VISITS {
                            return None;
                        }
                    }
                    return None;
                }
                Need::Quest(q, depth, dependency) => {
                    // Initial completion does not hold a parent in the quest log. This must
                    // precede the normal prerequisite shortcut: the linked inserter otherwise
                    // recursively adds that parent despite its initial completion.
                    if dependency == Dependency::Held && self.initial.rewarded(q as usize) {
                        return None;
                    }
                    if out.active.contains(&q) {
                        if dependency == Dependency::Held && out.accepted.contains(&q) {
                            continue;
                        }
                        return None;
                    }
                    if out.order.contains(&q)
                        || match dependency {
                            Dependency::Normal => self.done(self.planner.quest(q).id, q, &out),
                            // These are the partner guards of the actual recursive inserter.
                            // A rewarded handed child without a route turn-in still recurses,
                            // so its entire partner graph must be traversed and capped.
                            Dependency::Handed => self.present.turned[q as usize],
                            Dependency::Held => self.present.present[q as usize] && self.present.turned[q as usize],
                        }
                    {
                        continue;
                    }
                    *visits += 1;
                    if *visits > MAX_VISITS || depth > MAX_DEPTH {
                        return None;
                    }
                    // A partial normal/held ancestor is not completed by this move. Handed
                    // children can be explicitly finished from the initial log by insertion.
                    if self.initial.accepted[q as usize] && dependency != Dependency::Handed {
                        return None;
                    }
                    if self.present.present[q as usize] || out.selected.len() >= MAX_QUESTS || self.conflicts(q, &out) {
                        return None;
                    }
                    let quest = self.planner.quest(q);
                    out.selected.push(q);
                    out.active.push(q);
                    pending.push(Need::Finish(q));
                    pending.push(Need::Any(q, depth, 0));
                    // Traverse the same partners the linked inserter can bring in. A parent
                    // held by its child is allowed while active; a turn-in dependency is not.
                    for id in quest.objectives.iter().flat_map(|o| &o.handed_by).rev() {
                        let &i = self.planner.model.index.get(id)?;
                        pending.push(Need::Quest(i as u32, depth + 1, Dependency::Handed));
                    }
                    pending.push(Need::Accepted(q));
                    for id in quest.pre_held.iter().rev() {
                        let &i = self.planner.model.index.get(id)?;
                        pending.push(Need::Quest(i as u32, depth + 1, Dependency::Held));
                    }
                    for id in quest.pre_all.iter().rev() {
                        if !self.done(*id, q, &out) {
                            let &i = self.planner.model.index.get(id)?;
                            pending.push(Need::Quest(i as u32, depth + 1, Dependency::Normal));
                        }
                    }
                }
            }
        }
        Some(out)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::job::Prepared;
    use crate::plan::Kind;
    use crate::plan::tests::quests_at_start;
    use rand::SeedableRng;

    fn planner(p: &Prepared) -> Planner<'_> {
        Planner::new(&p.model, &p.world, &p.params, &p.profile)
    }

    fn route(quests: &[u32]) -> Vec<Stop> {
        quests
            .iter()
            .flat_map(|&q| [Stop::quest(q, Kind::Accept), Stop::quest(q, Kind::TurnIn)])
            .collect()
    }

    fn chain(n: usize) -> Prepared {
        let mut p = quests_at_start(n);
        for i in 1..n {
            p.model.quests[i].pre_all = vec![p.model.quests[i - 1].id];
        }
        p
    }

    fn accepts(r: &[Stop]) -> Vec<u32> {
        let mut out: Vec<_> = r.iter().filter(|s| s.kind == Kind::Accept).map(|s| s.index).collect();
        out.sort_unstable();
        out
    }

    #[test]
    fn complete_optional_chain_crosses_a_losing_prefix_valley() {
        let mut p = quests_at_start(6);
        p.world.terrain = crate::terrain::Terrain::default();
        p.model.quests[1].pre_all = vec![p.model.quests[0].id];
        p.model.quests[2].pre_all = vec![p.model.quests[1].id];
        p.model.quests[2].xp = 400;
        p.model.quests[5].xp = 400;
        p.model.quests[5].ends[0].pos.x += 10_000.0;
        let plan = planner(&p);
        let baseline = route(&[3, 4, 5]);
        let run = plan.run(&baseline).unwrap();
        for endpoint in [0, 1] {
            let mut prefix = baseline.clone();
            plan.insert_optional_chain(&mut prefix, &run, endpoint)
                .expect("a valid prefix");
            assert!(plan.simulate(&prefix, None).unwrap() > run.total + 0.5);
        }
        let mut whole = baseline;
        let changed = plan
            .insert_optional_chain(&mut whole, &run, 2)
            .expect("the whole chain");
        assert!(!changed.reordered);
        assert_eq!(accepts(&whole), [0, 1, 2, 3, 4, 5]);
        assert!(plan.simulate(&whole, None).unwrap() < run.total - 0.5);
    }

    #[test]
    fn chain_reuses_a_completed_ancestor_and_an_existing_turn_in() {
        let mut p = chain(3);
        let id = p.model.quests[0].id;
        p.model.initial.completed = Some([id].into_iter().collect());
        p.model.initial.turned = vec![0];
        let plan = planner(&p);
        let mut r = route(&[1]);
        let run = plan.run(&r).unwrap();
        plan.insert_optional_chain(&mut r, &run, 2).expect("reuse ancestry");
        assert_eq!(accepts(&r), [1, 2]);
        assert_eq!(r.iter().filter(|s| **s == Stop::quest(1, Kind::TurnIn)).count(), 1);
        assert!(plan.simulate(&r, None).is_some());
    }

    #[test]
    fn chain_counts_completed_ids_missing_from_the_model_in_or_thresholds() {
        let mut p = quests_at_start(3);
        p.model.initial.completed = Some([42].into_iter().collect());
        p.model.quests[2].pre_any = vec![42, p.model.quests[0].id, p.model.quests[1].id];
        p.model.quests[2].pre_need = 2;
        let plan = planner(&p);
        let mut r = Vec::new();
        let run = plan.run(&r).unwrap();
        plan.insert_optional_chain(&mut r, &run, 2)
            .expect("one completed, one new");
        assert_eq!(accepts(&r), [0, 2]);
        assert!(plan.simulate(&r, None).is_some());
    }

    #[test]
    fn group_members_may_satisfy_or_with_different_completed_alternatives() {
        for need in [1, 2] {
            let mut p = quests_at_start(2);
            p.model.initial.completed = Some([42].into_iter().collect());
            p.model.initial.mates = vec![crate::model::MateStart {
                level: 1,
                completed: [43].into_iter().collect(),
                ..crate::model::MateStart::default()
            }];
            p.model.initial.need = vec![0b11, 0b11];
            p.model.quests[1].pre_any = vec![42, 43, p.model.quests[0].id];
            p.model.quests[1].pre_need = need;
            let plan = planner(&p);
            let mut r = Vec::new();
            let run = plan.run(&r).unwrap();
            plan.insert_optional_chain(&mut r, &run, 1)
                .expect("each member meets its own OR threshold");
            assert_eq!(accepts(&r), if need == 1 { vec![1] } else { vec![0, 1] });
            assert!(plan.simulate(&r, None).is_some(), "need {need}: {r:?}");
        }
    }

    #[test]
    fn chain_backtracks_past_a_conflicting_or_alternative() {
        let mut p = quests_at_start(4);
        p.model.quests[3].pre_all = vec![p.model.quests[0].id];
        p.model.quests[3].pre_any = vec![p.model.quests[1].id, p.model.quests[2].id];
        p.model.quests[3].pre_need = 1;
        p.model.quests[1].exclusive = vec![p.model.quests[0].id];
        let plan = planner(&p);
        let mut r = Vec::new();
        let run = plan.run(&r).unwrap();
        plan.insert_optional_chain(&mut r, &run, 3)
            .expect("second alternative works");
        assert_eq!(accepts(&r), [0, 2, 3]);
        assert!(plan.simulate(&r, None).is_some());
    }

    #[test]
    fn chain_backtracks_over_or_combinations_to_meet_cardinality() {
        let mut p = quests_at_start(4);
        p.model.quests[3].pre_any = p.model.quests[..3].iter().map(|q| q.id).collect();
        p.model.quests[3].pre_need = 2;
        p.model.quests[0].exclusive = vec![p.model.quests[1].id, p.model.quests[2].id];
        let plan = planner(&p);
        let mut r = Vec::new();
        let run = plan.run(&r).unwrap();
        plan.insert_optional_chain(&mut r, &run, 3)
            .expect("compatible second and third choices");
        assert_eq!(accepts(&r), [1, 2, 3]);
        assert!(plan.simulate(&r, None).is_some());
    }

    #[test]
    fn chain_requires_the_full_or_cardinality_even_with_missing_ids() {
        let mut p = quests_at_start(2);
        p.model.quests[1].pre_any = vec![42, p.model.quests[0].id];
        p.model.quests[1].pre_need = 2;
        let plan = planner(&p);
        let mut r = Vec::new();
        let run = plan.run(&r).unwrap();
        assert!(plan.insert_optional_chain(&mut r, &run, 1).is_none());
        assert_eq!(r, []);
    }

    #[test]
    fn rejected_chains_leave_every_original_stop_unchanged() {
        for reason in ["partial", "cycle", "missing", "exclusive", "mandatory"] {
            let mut p = quests_at_start(3);
            p.model.quests[2].pre_all = vec![p.model.quests[0].id];
            let r = if reason == "partial" {
                vec![Stop::quest(0, Kind::Accept)]
            } else {
                route(&[1])
            };
            match reason {
                "cycle" => p.model.quests[0].pre_all = vec![p.model.quests[2].id],
                "missing" => p.model.quests[0].pre_all = vec![42],
                "exclusive" => p.model.quests[1].exclusive = vec![p.model.quests[0].id],
                "mandatory" => p.model.quests[2].mandatory = true,
                _ => {}
            }
            let plan = planner(&p);
            let run = plan.run(&r).unwrap();
            let mut candidate = r.clone();
            assert!(
                plan.insert_optional_chain(&mut candidate, &run, 2).is_none(),
                "{reason}"
            );
            assert_eq!(candidate, r, "{reason}");
        }
    }

    #[test]
    fn held_parent_cycles_are_rejected_before_recursive_insertion() {
        let mut p = quests_at_start(2);
        p.model.quests[0].pre_held = vec![p.model.quests[1].id];
        p.model.quests[1].pre_held = vec![p.model.quests[0].id];
        let plan = planner(&p);
        let initial = plan.sim().initial_state();
        let present = RouteQuests::new(&[], p.model.quests.len());
        let collector = Closure {
            planner: &plan,
            present: &present,
            initial: &initial,
        };
        assert!(
            collector
                .solve(
                    vec![Need::Quest(0, 0, Dependency::Normal)],
                    Collected::default(),
                    &mut 0
                )
                .is_none()
        );
    }

    #[test]
    fn rewarded_held_parent_cannot_hide_a_recursive_partner_cycle() {
        let mut p = quests_at_start(2);
        p.model.quests[0].pre_held = vec![p.model.quests[1].id];
        p.model.quests[1].pre_held = vec![p.model.quests[0].id];
        p.model.initial.turned = vec![1];
        let plan = planner(&p);
        let initial = plan.sim().initial_state();
        let present = RouteQuests::new(&[], p.model.quests.len());
        let collector = Closure {
            planner: &plan,
            present: &present,
            initial: &initial,
        };
        assert!(
            collector
                .solve(
                    vec![Need::Quest(0, 0, Dependency::Normal)],
                    Collected::default(),
                    &mut 0
                )
                .is_none()
        );
    }

    fn handed_pair(cyclic: bool) -> Prepared {
        use crate::model::testing::At;
        use crate::testing::{Obj, Q, Scenario};
        let at = At {
            zone: 1,
            x: 50.0,
            y: 50.0,
        };
        let parent = Q::new(1, "Parent").objective(Obj::visit("Child's item", at).handed_by(&[2]));
        let child = if cyclic {
            Q::new(2, "Child").objective(Obj::visit("Parent's item", at).handed_by(&[1]))
        } else {
            Q::new(2, "Child")
        };
        let mut p = Scenario::new().levels(1, 2).quest(parent).quest(child).build();
        p.params.child_glue = 1.0;
        p.model.initial.turned = vec![1];
        p
    }

    #[test]
    fn rewarded_handed_child_cannot_hide_a_recursive_partner_cycle() {
        let p = handed_pair(true);
        let plan = planner(&p);
        let initial = plan.sim().initial_state();
        let present = RouteQuests::new(&[], p.model.quests.len());
        let collector = Closure {
            planner: &plan,
            present: &present,
            initial: &initial,
        };
        assert!(
            collector
                .solve(
                    vec![Need::Quest(0, 0, Dependency::Normal)],
                    Collected::default(),
                    &mut 0
                )
                .is_none()
        );
    }

    #[test]
    fn completed_handed_child_still_allows_a_valid_parent_insertion() {
        let p = handed_pair(false);
        let plan = planner(&p);
        let mut r = Vec::new();
        let run = plan.run(&r).unwrap();
        plan.insert_optional_chain(&mut r, &run, 0)
            .expect("a completed handed child is safe");
        assert_eq!(accepts(&r), [0, 1]);
        assert!(plan.simulate(&r, None).is_some(), "{r:?}");
    }

    #[test]
    fn handed_child_reuses_its_active_parent_without_duplicate_stops() {
        use crate::model::testing::At;
        use crate::testing::{Obj, Q, Scenario};
        let mut p = Scenario::new()
            .levels(1, 2)
            .quest(
                Q::new(1, "Parent").objective(
                    Obj::visit(
                        "Child's item",
                        At {
                            zone: 1,
                            x: 50.0,
                            y: 50.0,
                        },
                    )
                    .handed_by(&[2]),
                ),
            )
            .quest(Q::new(2, "Child").while_in_log(&[1]))
            .build();
        p.params.child_glue = 1.0;
        let plan = planner(&p);
        let mut r = Vec::new();
        let run = plan.run(&r).unwrap();
        plan.insert_optional_chain(&mut r, &run, 0)
            .expect("a legitimate linked parent/child");
        assert_eq!(accepts(&r), [0, 1]);
        assert_eq!(r.len(), 5);
        assert!(plan.simulate(&r, None).is_some(), "{r:?}");
    }

    #[test]
    fn eight_new_quests_fit_but_nine_are_rejected_atomically() {
        for (n, allowed) in [(8, true), (9, false)] {
            let p = chain(n);
            let plan = planner(&p);
            let mut r = Vec::new();
            let run = plan.run(&r).unwrap();
            assert_eq!(
                plan.insert_optional_chain(&mut r, &run, n as u32 - 1).is_some(),
                allowed
            );
            assert_eq!(accepts(&r).len(), if allowed { 8 } else { 0 });
        }
    }

    #[test]
    fn linked_parents_count_toward_the_actual_new_quest_cap() {
        for (n, allowed) in [(8, true), (9, false)] {
            let mut p = quests_at_start(n);
            p.params.child_glue = 1.0;
            for i in 1..n {
                p.model.quests[i].pre_held = vec![p.model.quests[i - 1].id];
            }
            let plan = planner(&p);
            let mut r = Vec::new();
            let run = plan.run(&r).unwrap();
            assert_eq!(
                plan.insert_optional_chain(&mut r, &run, n as u32 - 1).is_some(),
                allowed
            );
            assert_eq!(accepts(&r).len(), if allowed { 8 } else { 0 });
            if allowed {
                assert!(plan.simulate(&r, None).is_some(), "{r:?}");
            }
        }
    }

    #[test]
    fn excessive_or_branching_rejects_without_mutation() {
        let mut p = quests_at_start(67);
        for i in 0..65 {
            p.model.quests[i].pre_all = vec![42];
        }
        p.model.quests[66].pre_any = p.model.quests[..66].iter().map(|q| q.id).collect();
        p.model.quests[66].pre_need = 1;
        let plan = planner(&p);
        let mut r = Vec::new();
        let run = plan.run(&r).unwrap();
        assert!(plan.insert_optional_chain(&mut r, &run, 66).is_none());
        assert_eq!(r, []);
    }

    #[test]
    fn random_chain_selection_is_seed_reproducible() {
        let p = chain(3);
        let plan = planner(&p);
        let mut a = Vec::new();
        let mut b = Vec::new();
        let run = plan.run(&a).unwrap();
        let mut ra = SmallRng::seed_from_u64(9);
        let mut rb = SmallRng::seed_from_u64(9);
        assert!(plan.add_optional_chain(&mut a, &run, &mut ra).is_some());
        assert!(plan.add_optional_chain(&mut b, &run, &mut rb).is_some());
        assert_eq!(a, b);
    }
}
