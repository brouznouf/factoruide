//! Selected gear sources are actual completion goals, independent of their soft penalty.

use super::{Kind, Planner, Simulator, State, Stop};
use crate::model::BisQuestAction;
use std::collections::{BTreeMap, BTreeSet};

#[expect(
    clippy::option_option,
    reason = "Distinguish absent source action, outside placement, and selected dungeon"
)]
pub(super) fn source_visit(model: &crate::model::Model, stop: Stop) -> Option<Option<usize>> {
    if model.bis.required_roots().is_empty() {
        return None;
    }
    let action = match stop.kind {
        Kind::Accept => BisQuestAction::Accept(stop.index),
        Kind::TurnIn => BisQuestAction::TurnIn(stop.index),
        Kind::Objective(objective) => BisQuestAction::Objective {
            quest: stop.index,
            objective,
        },
        _ => return None,
    };
    model.bis.source_visit(action)
}

/// A small terminal rank, carried instead of keeping another run's states alive.
/// With no Required sources it is always zero, preserving ordinary scalar ordering.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, PartialOrd, Ord)]
pub(crate) struct HardGoals {
    pub(crate) exact: u32,
    pub(crate) dungeons: u32,
    pub(crate) zones: u32,
    pub(crate) gear: u32,
}

pub(super) fn action_stop(action: BisQuestAction) -> Stop {
    match action {
        BisQuestAction::Accept(q) => Stop::quest(q, Kind::Accept),
        BisQuestAction::Objective { quest, objective } => Stop::quest(quest, Kind::Objective(objective)),
        BisQuestAction::TurnIn(q) => Stop::quest(q, Kind::TurnIn),
    }
}

pub(super) fn action_done(s: &State, action: BisQuestAction) -> bool {
    match action {
        BisQuestAction::Accept(q) => s.accepted[q as usize] || s.rewarded(q as usize),
        BisQuestAction::Objective { quest, objective } => {
            s.rewarded(quest as usize) || s.objectives[quest as usize] & (1 << objective) != 0
        }
        BisQuestAction::TurnIn(q) => s.rewarded(q as usize),
    }
}

fn stop_action(s: Stop) -> Option<BisQuestAction> {
    match s.kind {
        Kind::Accept => Some(BisQuestAction::Accept(s.index)),
        Kind::TurnIn => Some(BisQuestAction::TurnIn(s.index)),
        Kind::Objective(objective) => Some(BisQuestAction::Objective {
            quest: s.index,
            objective,
        }),
        _ => None,
    }
}

fn node_reaches(from: usize, to: usize, edges: &BTreeSet<(usize, usize)>) -> bool {
    let mut stack = vec![from];
    let mut seen = BTreeSet::new();
    while let Some(a) = stack.pop() {
        if a == to {
            return true;
        }
        if seen.insert(a) {
            stack.extend(edges.iter().filter(|&&(p, _)| p == a).map(|&(_, b)| b));
        }
    }
    false
}

fn ordered_nodes(nodes: &[usize], edges: &BTreeSet<(usize, usize)>, priority: &[usize]) -> Option<Vec<usize>> {
    if nodes.len() == 1 {
        return Some(nodes.to_vec());
    }
    let members: BTreeSet<_> = nodes.iter().copied().collect();
    let mut degree = BTreeMap::new();
    let mut children: BTreeMap<usize, Vec<usize>> = BTreeMap::new();
    for &n in nodes {
        degree.insert(n, 0usize);
    }
    for &(a, b) in edges {
        if members.contains(&a) && members.contains(&b) {
            *degree.get_mut(&b)? += 1;
            children.entry(a).or_default().push(b);
        }
    }
    let mut ready: BTreeSet<_> = nodes
        .iter()
        .filter(|n| degree[n] == 0)
        .map(|&n| (priority[n], n))
        .collect();
    let mut out = Vec::new();
    while let Some((_, n)) = ready.pop_first() {
        out.push(n);
        for &b in children.get(&n).into_iter().flatten() {
            let count = degree.get_mut(&b)?;
            *count -= 1;
            if *count == 0 {
                ready.insert((priority[b], b));
            }
        }
    }
    (out.len() == nodes.len()).then_some(out)
}

fn visit_source_block(i: usize, parents: &[Vec<usize>], seen: &mut [u8], order: &mut Vec<usize>) -> Option<()> {
    let mut stack = vec![(i, false)];
    while let Some((i, exiting)) = stack.pop() {
        if exiting {
            seen[i] = 2;
            order.push(i);
            continue;
        }
        if seen[i] == 2 {
            continue;
        }
        if seen[i] == 1 {
            return None;
        }
        seen[i] = 1;
        stack.push((i, true));
        stack.extend(parents[i].iter().rev().map(|&p| (p, false)));
    }
    Some(())
}

impl Simulator<'_> {
    /// Automatic nearby actions must respect residual cross-source held/breadcrumb edges.
    pub(super) fn source_action_ready(&self, s: &State, stop: Stop) -> bool {
        if self.model.bis.required_roots().is_empty() {
            return true;
        }
        let action = match stop.kind {
            Kind::Accept => BisQuestAction::Accept(stop.index),
            Kind::TurnIn => BisQuestAction::TurnIn(stop.index),
            Kind::Objective(objective) => BisQuestAction::Objective {
                quest: stop.index,
                objective,
            },
            _ => return true,
        };
        self.model
            .bis
            .source_edges()
            .iter()
            .filter(|(_, after)| *after == action)
            .all(|(before, _)| action_done(s, *before))
    }
}

impl Planner<'_> {
    pub(crate) fn hard_goals(&self, s: &State) -> HardGoals {
        if self.model.bis.required_roots().is_empty() {
            return HardGoals::default();
        }
        HardGoals {
            exact: s.required_left,
            zones: s.forced_left,
            dungeons: (0..self.model.dungeons.len())
                .filter(|&d| {
                    self.availability().dungeon_forced(d)
                        && self.model.dungeons[d].min_level < self.profile.to_level
                        && s.dungeons & (1u128 << d) == 0
                })
                .count() as u32,
            gear: self
                .model
                .bis
                .required_roots()
                .iter()
                .filter(|&&q| !s.rewarded(q as usize))
                .count() as u32,
        }
    }

    /// Accepted specific sources not actually completed, including recorded initial history.
    pub fn missing_required_gear(&self, route: &[Stop]) -> Vec<u32> {
        if self.model.bis.required_roots().is_empty() {
            return Vec::new();
        }
        let state = self.simulate_state(route, None).map(|(_, s)| s);
        self.model
            .bis
            .required_roots()
            .iter()
            .copied()
            .filter(|&q| state.as_ref().is_none_or(|s| !s.rewarded(q as usize)))
            .collect()
    }

    pub(crate) fn scored_goals(&self, route: &[Stop]) -> Option<(f64, f64, HardGoals)> {
        self.simulate_state(route, None)
            .map(|(score, s)| (score, s.spent.total, self.hard_goals(&s)))
    }

    /// Whether a transformation preserves every previously satisfied hard goal.
    fn preserves_goals(&self, before: &State, after: &State) -> bool {
        after.forced_left <= before.forced_left
            && self
                .model
                .quests
                .iter()
                .enumerate()
                .all(|(q, quest)| !quest.required || !before.rewarded(q) || after.rewarded(q))
            && self
                .model
                .bis
                .required_roots()
                .iter()
                .all(|&q| !before.rewarded(q as usize) || after.rewarded(q as usize))
            && (0..self.model.dungeons.len()).all(|d| {
                !self.availability().dungeon_forced(d)
                    || before.dungeons & (1u128 << d) == 0
                    || after.dungeons & (1u128 << d) != 0
            })
    }

    /// Keep a known hard-feasible route across cosmetic cleanup; Off does no extra replay.
    pub(crate) fn keep_hard_goals(&self, before: Vec<Stop>, after: Vec<Stop>) -> Vec<Stop> {
        if self.model.bis.required_roots().is_empty() || before == after {
            return after;
        }
        let Some((_, old)) = self.simulate_state(&before, None) else {
            return after;
        };
        if self
            .simulate_state(&after, None)
            .is_some_and(|(_, new)| self.preserves_goals(&old, &new))
        {
            after
        } else {
            before
        }
    }

    /// Insert selected residual closures transactionally. Global edges and the contiguous
    /// dungeon witness govern ordering; local root plans can predate cross-root held edges.
    pub fn repair_required_gear(&self, mut route: Vec<Stop>) -> Vec<Stop> {
        if self.model.bis.required_roots().is_empty() || self.aborted() {
            return route;
        }
        let Some((_, mut end)) = self.simulate_state(&route, None) else {
            return route;
        };
        if self.hard_goals(&end).gear == 0 {
            return route;
        }
        if let Some(candidate) = self.merge_sources(&route, &end)
            && let Some((_, next)) = self.simulate_state(&candidate, None)
            && self.preserves_goals(&end, &next)
            && self.hard_goals(&next).gear < self.hard_goals(&end).gear
        {
            (route, end) = (candidate, next);
        }
        // Try the complete graph first, then individual residual ancestors when only part can
        // be scheduled. A dungeon's whole selected visit is kept together in either case.
        let roots = self.model.bis.required_roots();
        let attempts = std::iter::once(roots.to_vec()).chain(roots.iter().map(|&q| vec![q]));
        for goals in attempts {
            if self.aborted() || self.hard_goals(&end).gear == 0 {
                break;
            }
            if goals.iter().all(|&q| end.rewarded(q as usize)) {
                continue;
            }
            let mut actions = self.source_closure(&goals);
            let visits: BTreeSet<_> = actions.iter().filter_map(|&a| self.source_dungeon(a)).collect();
            let mut edges = self.model.bis.source_edges().to_vec();
            // Existing work in a visit travels with that visit, especially exact dungeon
            // quests. Keep its own action sequence while allowing outside accepts before
            // the shared entry and outside hand-ins afterwards.
            let companions: BTreeSet<_> = route
                .iter()
                .filter(|s| s.is_quest())
                .filter_map(|s| {
                    let q = self.quest(s.index);
                    (q.objectives
                        .iter()
                        .filter_map(crate::model::Objective::dungeon_of)
                        .any(|d| visits.contains(&d))
                        || q.starts
                            .iter()
                            .chain(&q.ends)
                            .filter_map(|l| l.inside)
                            .any(|d| visits.contains(&usize::from(d))))
                    .then_some(s.index)
                })
                .collect();
            let mut previous = BTreeMap::new();
            for stop in route.iter().filter(|s| s.is_quest() && companions.contains(&s.index)) {
                let action = match stop.kind {
                    Kind::Accept => BisQuestAction::Accept(stop.index),
                    Kind::TurnIn => BisQuestAction::TurnIn(stop.index),
                    Kind::Objective(objective) => BisQuestAction::Objective {
                        quest: stop.index,
                        objective,
                    },
                    _ => unreachable!(),
                };
                actions.insert(action);
                if let Some(before) = previous.insert(stop.index, action)
                    && before != action
                {
                    edges.push((before, action));
                }
            }
            let quests: BTreeSet<_> = actions.iter().map(|&a| action_stop(a).index).collect();
            let Some(ordered) = self.source_order(&actions, &edges) else {
                continue;
            };
            let mut inserted = Vec::new();
            let mut dungeons = BTreeSet::new();
            for action in ordered {
                let stop = action_stop(action);
                if let Kind::Objective(k) = stop.kind
                    && let Some(d) = self.quest(stop.index).objectives[k as usize].dungeon
                {
                    if dungeons.insert(d) {
                        inserted.push(Stop {
                            index: d as u32,
                            kind: Kind::Dungeon,
                        });
                    }
                } else {
                    if let Some(d) = self.places().usual_inside(stop) {
                        dungeons.insert(d);
                    }
                    inserted.push(stop);
                }
            }
            // Recorded held-only parents have no residual Accept node, but an explicit no-op
            // anchor retains their real log state through start_state's abandonment policy.
            let anchors: Vec<_> = self
                .model
                .bis
                .source_quests()
                .iter()
                .copied()
                .filter(|q| self.in_log_at_start(*q) && !quests.contains(q))
                .map(|q| Stop::quest(q, Kind::Accept))
                .collect();
            inserted.splice(0..0, anchors);
            let removed = |s: &Stop| {
                s.is_quest() && quests.contains(&s.index)
                    || s.kind == Kind::Dungeon && dungeons.contains(&(s.index as usize))
            };
            let original_at = route.iter().take_while(|s| !removed(s)).count();
            let base: Vec<_> = route.iter().copied().filter(|s| !removed(s)).collect();
            // Try the original neighborhood, the beginning, and bounded route landmarks.
            // Insertion never relies on post-target intent.
            // Compact replay states are discarded between attempts; no second Run is retained.
            let mut placements = vec![original_at.min(base.len()), 0];
            placements.extend((1..base.len()).step_by(base.len().div_ceil(24).max(1)));
            placements.push(base.len());
            let mut tried = BTreeSet::new();
            placements.retain(|at| tried.insert(*at));
            for at in placements {
                let mut candidate = base.clone();
                candidate.splice(at..at, inserted.iter().copied());
                let Some((_, next)) = self.simulate_state(&candidate, None) else {
                    continue;
                };
                if self.preserves_goals(&end, &next) && self.hard_goals(&next).gear < self.hard_goals(&end).gear {
                    (route, end) = (candidate, next);
                    break;
                }
            }
        }
        route
    }

    /// Merge the source DAG with existing work instead of relocating its entire ancestry.
    /// Visits and standalone timed quests stay together; other outside work keeps
    /// independent positions. Priorities are compact route landmarks, not retained simulation states.
    fn merge_sources(&self, route: &[Stop], end: &State) -> Option<Vec<Stop>> {
        use BisQuestAction as A;

        // Replay with the original plan so automatic actions and level gains match the
        // route whose completed goals the transaction must preserve.
        let mut state = self.sim().start_state(self.planned(route));
        let mut levels = Vec::with_capacity(route.len());
        let mut realized = Vec::with_capacity(route.len());
        for (at, &stop) in route.iter().enumerate() {
            levels.push(state.level);
            realized.push(!Simulator::already_done(&state, stop));
            if self.sim().ended(&state) {
                continue;
            }
            if !self.availability().valid(&state, stop) {
                return None;
            }
            state.at = at as u32;
            state.next = self.sim().next_pos(&state, route, at);
            self.sim().apply(&mut state, stop, None);
            if !state.time.is_finite() || state.completion_error.is_some() {
                return None;
            }
        }
        drop(state);
        let missing: Vec<_> = self
            .model
            .bis
            .required_roots()
            .iter()
            .copied()
            .filter(|&q| !end.rewarded(q as usize))
            .collect();
        let active = self.source_closure(&missing);
        let landmark = |level| levels.iter().position(|&l| l >= level).unwrap_or(route.len());
        let initial = self.sim().initial_state();
        let mut stops = Vec::new();
        let mut actions = BTreeMap::new();
        let mut dungeon_stops = BTreeMap::new();
        let mut visits = Vec::new();
        let mut priorities = Vec::new();
        let mut floors = Vec::new();
        let mut added = Vec::new();
        for (original, stop) in route.iter().copied().enumerate().map(|(i, s)| (Some(i), s)).chain(
            self.model
                .bis
                .source_actions()
                .iter()
                .filter(|a| active.contains(a))
                .map(|&a| (None, action_stop(a))),
        ) {
            let a = stop_action(stop);
            if a.is_some_and(|a| actions.contains_key(&a)) {
                continue;
            }
            let visit = a
                .and_then(|a| self.source_dungeon(a))
                .or_else(|| (stop.kind == Kind::Dungeon).then_some(stop.index as usize));
            let blackbox = if let Kind::Objective(k) = stop.kind {
                self.quest(stop.index).objectives[k as usize].dungeon
            } else {
                None
            };
            let run = blackbox.or_else(|| (stop.kind == Kind::Dungeon).then_some(stop.index as usize));
            if let Some(&node) = run.and_then(|d| dungeon_stops.get(&d)) {
                if let Some(a) = a {
                    actions.insert(a, node);
                }
                continue;
            }
            let actual = blackbox.map_or(stop, |d| Stop {
                index: d as u32,
                kind: Kind::Dungeon,
            });
            let floor = if stop.is_quest() {
                let q = self.quest(stop.index);
                q.min_level.max(if stop.kind == Kind::TurnIn {
                    q.completion_window.min_level.unwrap_or(1)
                } else {
                    1
                })
            } else if let Some(d) = run {
                self.model.dungeons[d].min_level
            } else {
                1
            };
            let preferred = if stop.is_quest() {
                let q = self.quest(stop.index);
                let level = (q.level - 3).max(floor).max(self.sim().gate_level(&initial, actual));
                q.completion_window.max_level.map_or(level, |max| level.min(max))
            } else {
                floor
            };
            let node = stops.len();
            stops.push(actual);
            added.push(original.is_none());
            priorities.push(original.unwrap_or_else(|| landmark(preferred)));
            floors.push(landmark(floor));
            visits.push(visit);
            if let Some(a) = a {
                actions.insert(a, node);
            }
            if let Some(d) = run {
                dungeon_stops.insert(d, node);
            }
        }
        drop(initial);
        let mut edges = BTreeSet::new();
        let mut edge = |a: A, b: A| {
            if let (Some(&a), Some(&b)) = (actions.get(&a), actions.get(&b))
                && a != b
            {
                edges.insert((a, b));
            }
        };
        for &(a, b) in self.model.bis.source_edges() {
            if active.contains(&a) && active.contains(&b) {
                edge(a, b);
            }
        }
        // Keep ordinary work's realized order, including objectives performed in an order
        // chosen by the original route. Cross-quest constraints must survive import too.
        let mut previous = BTreeMap::new();
        for (at, &stop) in route.iter().enumerate() {
            // Recorded Accept anchors and automatic completions are no-ops, not new
            // reverse dependencies after the quest was already rewarded.
            if !realized[at] {
                continue;
            }
            if let Some(a) = stop_action(stop)
                && let Some(before) = previous.insert(stop.index, a)
            {
                edge(before, a);
            }
        }
        for (i, q) in self.model.quests.iter().enumerate() {
            let i = i as u32;
            edge(A::Accept(i), A::TurnIn(i));
            for (k, o) in q.objectives.iter().enumerate() {
                let a = A::Objective {
                    quest: i,
                    objective: k as u8,
                };
                edge(A::Accept(i), a);
                edge(a, A::TurnIn(i));
                for before in 0..q.objectives.len() {
                    if o.after & (1 << before) != 0 {
                        edge(
                            A::Objective {
                                quest: i,
                                objective: before as u8,
                            },
                            a,
                        );
                    }
                }
                for id in &o.handed_by {
                    if let Some(&p) = self.model.index.get(id) {
                        edge(A::TurnIn(p as u32), a);
                    }
                }
            }
            for id in &q.pre_all {
                if let Some(&p) = self.model.index.get(id) {
                    edge(A::TurnIn(p as u32), A::Accept(i));
                }
            }
            // Selected Any choices already have exact witness edges. For ordinary work,
            // retain the earliest represented alternatives from the valid original route.
            if !active.contains(&A::Accept(i)) {
                let mut parents: Vec<_> = q
                    .pre_any
                    .iter()
                    .filter_map(|id| self.model.index.get(id))
                    .filter_map(|&p| actions.get(&A::TurnIn(p as u32)).map(|&n| (priorities[n], p)))
                    .collect();
                parents.sort_unstable();
                for &(_, p) in parents.iter().take(q.pre_any_needed(q.pre_any.len())) {
                    edge(A::TurnIn(p as u32), A::Accept(i));
                }
            }
            for id in &q.pre_held {
                if let Some(&p) = self.model.index.get(id) {
                    edge(A::Accept(p as u32), A::Accept(i));
                    edge(A::Accept(i), A::TurnIn(p as u32));
                }
            }
            if let Some(&target) = q.breadcrumb_for.as_ref().and_then(|id| self.model.index.get(id)) {
                edge(A::Accept(i), A::Accept(target as u32));
            }
        }
        // The selector's chosen visit witness also fixes the internal source order.
        let mut last_inside = BTreeMap::new();
        for &a in self.model.bis.source_actions().iter().filter(|a| active.contains(a)) {
            if let Some(d) = self.source_dungeon(a)
                && let Some(before) = last_inside.insert(d, a)
            {
                edge(before, a);
            }
        }
        // An explicit run is the entry of its visit, not another run marker after
        // an inside objective already entered implicitly. Outside prerequisites still
        // precede the contracted visit, including every black-box quest acceptance.
        for (&d, &entry) in &dungeon_stops {
            for (n, visit) in visits.iter().enumerate() {
                if *visit == Some(d) && n != entry {
                    edges.insert((entry, n));
                }
            }
        }
        let timed: BTreeSet<_> = active
            .iter()
            .map(|&a| action_stop(a).index)
            .filter(|&q| {
                let quest = self.quest(q);
                quest.time_limit.is_some()
                    && quest.objectives.iter().all(|o| o.handed_by.is_empty())
                    && !self.model.quests.iter().any(|child| child.pre_held.contains(&quest.id))
                    && stops
                        .iter()
                        .zip(&visits)
                        .filter(|(s, _)| s.is_quest() && s.index == q)
                        .all(|(_, v)| v.is_none())
            })
            .collect();
        let mut timed_blocks = BTreeMap::new();
        let mut blocks: Vec<Vec<usize>> = Vec::new();
        let mut visit_blocks = BTreeMap::new();
        let mut block_of = Vec::new();
        for (n, visit) in visits.into_iter().enumerate() {
            let block = if let Some(d) = visit {
                *visit_blocks.entry(d).or_insert_with(|| {
                    blocks.push(Vec::new());
                    blocks.len() - 1
                })
            } else if stops[n].is_quest() && timed.contains(&stops[n].index) {
                *timed_blocks.entry(stops[n].index).or_insert_with(|| {
                    blocks.push(Vec::new());
                    blocks.len() - 1
                })
            } else {
                blocks.push(Vec::new());
                blocks.len() - 1
            };
            blocks[block].push(n);
            block_of.push(block);
        }
        let mut block_edges = BTreeSet::new();
        for &(a, b) in &edges {
            if block_of[a] != block_of[b] {
                block_edges.insert((block_of[a], block_of[b]));
            }
        }
        // Newly inserted higher-level work must not consume an earlier forced visit's
        // window. Use the normal quest placement band (nominal level + 2), since
        // combat and travel can advance beyond the nominal level. Preserve those
        // visits first unless the work is itself a prerequisite of that visit.
        // The actual DAG and replay still govern those prerequisite relations.
        for (n, &stop) in stops.iter().enumerate().filter(|(n, s)| added[*n] && s.is_quest()) {
            for (&d, &visit) in &visit_blocks {
                if self.availability().dungeon_forced(d)
                    && end.dungeons & (1u128 << d) != 0
                    && self.model.dungeons[d].max_level < (self.quest(stop.index).level + 2).min(self.profile.to_level)
                    && !node_reaches(block_of[n], visit, &block_edges)
                {
                    block_edges.insert((visit, block_of[n]));
                }
            }
        }
        let mut priority: Vec<_> = blocks
            .iter()
            .map(|nodes| {
                nodes
                    .iter()
                    .map(|&n| priorities[n])
                    .min()
                    .expect("each block contains a stop")
            })
            .collect();
        let floor: Vec<_> = blocks
            .iter()
            .map(|nodes| {
                nodes
                    .iter()
                    .map(|&n| floors[n])
                    .max()
                    .expect("each block contains a stop")
            })
            .collect();
        for (p, &f) in priority.iter_mut().zip(&floor) {
            *p = (*p).max(f);
        }
        // A prerequisite originally completed late moves only as far back as its child
        // needs it, and never before its modeled minimum level landmark.
        for _ in 0..blocks.len() {
            let mut changed = false;
            for &(a, b) in &block_edges {
                let next = priority[a].min(priority[b]).max(floor[a]);
                changed |= next != priority[a];
                priority[a] = next;
            }
            if !changed {
                break;
            }
        }

        let order = ordered_nodes(&(0..blocks.len()).collect::<Vec<_>>(), &block_edges, &priority)?;
        let mut merged = Vec::with_capacity(stops.len());
        for b in order {
            let inside = ordered_nodes(&blocks[b], &edges, &priorities)?;
            for n in inside {
                merged.push(stops[n]);
            }
        }
        // Ordinary gray work can cease to be useful after inserting genuine XP gains.
        // The existing bounded replay cleanup handles that; the caller commits only an
        // actual improvement preserving every already-satisfied hard goal.
        Some(self.settle(merged))
    }

    fn source_closure(&self, roots: &[u32]) -> BTreeSet<BisQuestAction> {
        let mut selected: BTreeSet<_> = roots.iter().map(|&q| BisQuestAction::TurnIn(q)).collect();
        loop {
            let old = selected.len();
            for &(before, after) in self.model.bis.source_edges() {
                if selected.contains(&after) {
                    selected.insert(before);
                }
            }
            let visits: BTreeSet<_> = selected.iter().filter_map(|&a| self.source_dungeon(a)).collect();
            for &a in self.model.bis.source_actions() {
                if self.source_dungeon(a).is_some_and(|d| visits.contains(&d)) {
                    selected.insert(a);
                }
            }
            if old == selected.len() {
                break;
            }
        }
        selected.retain(|a| self.model.bis.source_actions().contains(a));
        selected
    }

    fn source_dungeon(&self, action: BisQuestAction) -> Option<usize> {
        if let Some(visit) = self.model.bis.source_visit(action) {
            return visit;
        }
        let stop = action_stop(action);
        if let Kind::Objective(k) = stop.kind {
            return self.quest(stop.index).objectives[k as usize].dungeon_of();
        }
        self.places().usual_inside(stop)
    }

    /// Dependency-first traversal avoids opening every independent quest before finishing
    /// any. Contracted visit blocks retain the selector's internal witness order.
    fn source_order(
        &self,
        selected: &BTreeSet<BisQuestAction>,
        edges: &[(BisQuestAction, BisQuestAction)],
    ) -> Option<Vec<BisQuestAction>> {
        let mut blocks: Vec<Vec<BisQuestAction>> = Vec::new();
        let mut visits = BTreeMap::new();
        let mut block_of = BTreeMap::new();
        let mut witness: Vec<_> = self
            .model
            .bis
            .source_actions()
            .iter()
            .copied()
            .filter(|a| selected.contains(a))
            .collect();
        witness.extend(
            selected
                .iter()
                .copied()
                .filter(|a| !self.model.bis.source_actions().contains(a)),
        );
        for action in witness {
            let block = if let Some(d) = self.source_dungeon(action) {
                *visits.entry(d).or_insert_with(|| {
                    blocks.push(Vec::new());
                    blocks.len() - 1
                })
            } else {
                blocks.push(Vec::new());
                blocks.len() - 1
            };
            blocks[block].push(action);
            block_of.insert(action, block);
        }
        let mut parents = vec![Vec::new(); blocks.len()];
        for &(before, after) in edges {
            if let (Some(&a), Some(&b)) = (block_of.get(&before), block_of.get(&after))
                && a != b
            {
                parents[b].push(a);
            }
        }

        let mut seen = vec![0; blocks.len()];
        let mut order = Vec::new();
        for &q in self.model.bis.required_roots() {
            if let Some(&i) = block_of.get(&BisQuestAction::TurnIn(q)) {
                visit_source_block(i, &parents, &mut seen, &mut order)?;
            }
        }
        for i in 0..blocks.len() {
            visit_source_block(i, &parents, &mut seen, &mut order)?;
        }
        Some(order.into_iter().flat_map(|i| blocks[i].iter().copied()).collect())
    }
}
