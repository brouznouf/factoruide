//! Bounded structural support for gear sources. This does not promise a schedulable route.
use super::{Dungeon, Objective, Profile, Quest};
use crate::{
    job::StartState,
    params::{Might, Params},
};
use std::collections::{BTreeMap, BTreeSet, HashMap, HashSet};

/// An ordered, model-owned quest action (quest indexes, not game IDs).
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum BisQuestAction {
    Accept(u32),
    Objective { quest: u32, objective: u8 },
    TurnIn(u32),
}

impl BisQuestAction {
    fn quest(self) -> u32 {
        match self {
            Self::Accept(q) | Self::TurnIn(q) | Self::Objective { quest: q, .. } => q,
        }
    }
}

/// A source and its residual support, in legal dependency order.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BisQuestPlan {
    pub root: u32,
    pub quests: Vec<u32>,
    pub actions: Vec<BisQuestAction>,
}

const MAX_WORK: usize = 20_000;
const MAX_ACTIONS: usize = 4096;
const MAX_CHOICES: usize = 128;
const LIMIT: &str = "selection limit reached (support not determined)";

#[derive(Clone, Copy, Default)]
struct Floor {
    minimum: i64,
    combat: i64,
}

#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
enum Visit {
    Outside(BisQuestAction),
    Dungeon(usize),
}
impl Floor {
    fn merge(self, other: Self) -> Self {
        Self {
            minimum: self.minimum.max(other.minimum),
            combat: self.combat.max(other.combat),
        }
    }
}

#[derive(Debug, Clone, Default)]
pub(super) struct Closure {
    nodes: BTreeMap<BisQuestAction, u8>,
    edges: BTreeSet<(BisQuestAction, BisQuestAction)>,
    participants: BTreeSet<u32>,
    local_participants: BTreeMap<BisQuestAction, BTreeSet<u32>>,
    /// Only real gear actions use the strict pre-target horizon in temporary class proofs.
    gear_actions: BTreeSet<BisQuestAction>,
}

pub(super) struct SourceGraph {
    pub(super) actions: Vec<BisQuestAction>,
    pub(super) edges: Vec<(BisQuestAction, BisQuestAction)>,
    pub(super) quests: Vec<u32>,
    pub(super) visits: BTreeMap<BisQuestAction, Option<usize>>,
}

type VisitWitness = (Vec<BisQuestAction>, BTreeMap<BisQuestAction, Option<usize>>);

#[derive(Clone)]
enum Task {
    Action(BisQuestAction, u8),
    ClassChoice {
        name: String,
        roots: Vec<u32>,
    },
    Choose {
        ids: Vec<i64>,
        need: usize,
        members: u8,
        before: BisQuestAction,
    },
}

/// Evidence is kept in member order; the profile's group union is never substituted for it.
pub(super) struct Selector<'a> {
    quests: &'a [Quest],
    dungeons: &'a [Dungeon],
    skipped: &'a [(i64, String, String)],
    profile: &'a Profile,
    params: &'a Params,
    known: &'a HashSet<i64>,
    starts: &'a [&'a StartState],
    index: HashMap<i64, u32>,
    pub(super) committed: Closure,
    class_goals: Result<Vec<Task>, String>,
    unproven_class: Vec<(String, Vec<u32>)>,
}

impl<'a> Selector<'a> {
    pub(super) fn new(
        quests: &'a [Quest],
        dungeons: &'a [Dungeon],
        skipped: &'a [(i64, String, String)],
        profile: &'a Profile,
        params: &'a Params,
        known: &'a HashSet<i64>,
        starts: &'a [&'a StartState],
    ) -> Self {
        let mut selector = Self {
            quests,
            dungeons,
            skipped,
            profile,
            params,
            known,
            starts,
            index: quests.iter().enumerate().map(|(i, q)| (q.id, i as u32)).collect(),
            committed: Closure::default(),
            class_goals: Ok(Vec::new()),
            unproven_class: Vec::new(),
        };
        selector.class_goals = selector.supported_class_goals();
        selector
    }

    fn completed(&self, id: i64, member: usize) -> bool {
        self.starts.get(member).map_or_else(
            || self.profile.mates.is_empty() && self.profile.completed_quests.contains(&id),
            |s| s.completed.iter().any(|q| q.0 == id) && !s.log.iter().any(|q| q.id.0 == id),
        )
    }

    fn residual(&self, action: BisQuestAction, members: u8) -> u8 {
        let q = &self.quests[action.quest() as usize];
        let mut remaining = members;
        for m in 0..8 {
            if members & (1 << m) == 0 {
                continue;
            }
            let satisfied = self.completed(q.id, m)
                || self.starts.get(m).is_some_and(|s| {
                    s.log.iter().find(|l| l.id.0 == q.id).is_some_and(|log| match action {
                        BisQuestAction::Accept(_) => true,
                        BisQuestAction::Objective { objective, .. } => crate::job::objective_progress(q, log)
                            .iter()
                            .any(|&(k, v)| k == objective && v >= 1.0),
                        BisQuestAction::TurnIn(_) => false,
                    })
                });
            if satisfied {
                remaining &= !(1 << m);
            }
        }
        remaining
    }

    fn completion(
        &self,
        id: i64,
        members: u8,
        before: BisQuestAction,
        state: &mut Closure,
        tasks: &mut Vec<Task>,
    ) -> Result<(), String> {
        let members = (0..8)
            .filter(|m| members & (1 << m) != 0 && !self.completed(id, *m))
            .fold(0, |mask, m| mask | (1 << m));
        if members == 0
            || self.known.contains(&id)
                && self
                    .index
                    .get(&id)
                    .is_none_or(|&i| !self.quests[i as usize].required && !self.quests[i as usize].zone_forced)
                && !self.quests[before.quest() as usize].required
                && !self.quests[before.quest() as usize].zone_forced
        {
            return Ok(());
        }
        let Some(&i) = self.index.get(&id) else {
            let reason = self
                .skipped
                .iter()
                .find(|(q, _, _)| *q == id)
                .map_or("not modeled", |(_, _, why)| why.as_str());
            return Err(format!(
                "prerequisite {id} is {reason} and has no recorded completion for every participating member"
            ));
        };
        self.dependency(BisQuestAction::TurnIn(i), members, before, state, tasks);
        Ok(())
    }

    fn dependency(
        &self,
        action: BisQuestAction,
        members: u8,
        before: BisQuestAction,
        state: &mut Closure,
        tasks: &mut Vec<Task>,
    ) {
        state.participants.insert(action.quest());
        state
            .local_participants
            .entry(before)
            .or_default()
            .insert(action.quest());
        if self.residual(action, members) != 0 {
            state.edges.insert((action, before));
            tasks.push(Task::Action(action, members));
        }
    }

    pub(super) fn finalize(&self) -> Result<SourceGraph, String> {
        let (actions, visits) = self.order_with_visits(&self.committed)?;
        Ok(SourceGraph {
            actions,
            visits,
            edges: self
                .committed
                .edges
                .iter()
                .copied()
                .filter(|(a, b)| self.committed.nodes.contains_key(a) && self.committed.nodes.contains_key(b))
                .collect(),
            quests: self.committed.participants.iter().copied().collect(),
        })
    }

    pub(super) fn select(&self, root: u32) -> Result<(BisQuestPlan, Closure), String> {
        let mut work = 0;
        let class_goals = self.class_goals.as_ref().map_err(Clone::clone)?;
        let state = self.solve(
            self.committed.clone(),
            vec![Task::Action(BisQuestAction::TurnIn(root), self.participants(root))],
            0,
            &mut work,
            class_goals,
            false,
        )?;
        let ordered = self.order(&state)?;
        let mut needed = BTreeSet::from([BisQuestAction::TurnIn(root)]);
        loop {
            let old = needed.len();
            for &(a, b) in &state.edges {
                if needed.contains(&b) {
                    needed.insert(a);
                }
            }
            if old == needed.len() {
                break;
            }
        }
        let actions: Vec<_> = ordered.into_iter().filter(|a| needed.contains(a)).collect();
        let mut quests: Vec<_> = actions.iter().map(|a| a.quest()).collect();
        for action in &needed {
            quests.extend(state.local_participants.get(action).into_iter().flatten().copied());
        }
        quests.push(root);
        quests.sort_unstable();
        quests.dedup();
        Ok((BisQuestPlan { root, quests, actions }, state))
    }

    fn participants(&self, root: u32) -> u8 {
        let count = self.starts.len().max(self.profile.mates.len() + 1).min(8);
        self.quests[root as usize].members & (((1u16 << count) - 1) as u8)
    }

    /// Preserve class requirements that have a coherent baseline, without letting an already
    /// unsupported independent class goal disable every gear recommendation. No proof actions
    /// are committed to gear, and alternatives are chosen together rather than all reserved.
    fn supported_class_goals(&mut self) -> Result<Vec<Task>, String> {
        let mut goals = Vec::new();
        let mut work = 0;
        for requirement in self.profile.class_requirements.iter().filter(|r| r.selected) {
            let mut roots: Vec<_> = requirement
                .quests
                .iter()
                .filter_map(|id| self.index.get(id).copied())
                .filter(|&i| self.quests[i as usize].mandatory)
                .collect();
            roots.sort_by_key(|&i| self.quests[i as usize].id);
            roots.dedup();
            if roots.is_empty() {
                continue;
            }
            let goal = Task::ClassChoice {
                name: requirement.name.clone(),
                roots: roots.clone(),
            };
            let mut candidate = goals.clone();
            candidate.push(goal);
            match self.solve(Closure::default(), candidate.clone(), 0, &mut work, &[], true) {
                Ok(_) => goals = candidate,
                Err(e) if e == LIMIT => return Err(e),
                Err(_) => self.unproven_class.push((requirement.name.clone(), roots)),
            }
        }
        Ok(goals)
    }

    fn solve(
        &self,
        mut state: Closure,
        mut tasks: Vec<Task>,
        depth: usize,
        work: &mut usize,
        class_goals: &[Task],
        class_support: bool,
    ) -> Result<Closure, String> {
        use BisQuestAction::{Accept, Objective, TurnIn};

        if depth > MAX_CHOICES {
            return Err(LIMIT.into());
        }
        while let Some(task) = tasks.pop() {
            *work += 1;
            if *work > MAX_WORK || state.nodes.len() > MAX_ACTIONS {
                return Err(LIMIT.into());
            }
            let (action, members) = match task {
                Task::Action(a, m) => (a, m),
                Task::ClassChoice { name, roots } => {
                    let mut last = "no supported version".to_owned();
                    for root in roots {
                        let mut pending = tasks.clone();
                        pending.push(Task::Action(TurnIn(root), self.participants(root)));
                        match self.solve(state.clone(), pending, depth + 1, work, class_goals, class_support) {
                            Ok(s) => return Ok(s),
                            Err(e) if e == LIMIT => return Err(e),
                            Err(e) => last = e,
                        }
                    }
                    return Err(format!("conflicts with selected class requirement {name}: {last}"));
                }
                Task::Choose {
                    mut ids,
                    need,
                    members,
                    before,
                } => {
                    ids.sort_unstable();
                    ids.dedup();
                    if need == 0 {
                        continue;
                    }
                    if ids.len() < need {
                        return Err(format!(
                            "requires {need} distinct alternatives but only {} are available",
                            ids.len()
                        ));
                    }
                    // Already committed books first, then cheap alternatives, then stable game ID.
                    ids.sort_by_key(|id| {
                        (
                            self.index
                                .get(id)
                                .is_none_or(|&i| !state.nodes.contains_key(&TurnIn(i)))
                                && !(0..8).all(|m| members & (1 << m) == 0 || self.completed(*id, m)),
                            self.index
                                .get(id)
                                .map_or(usize::MAX, |&i| self.quests[i as usize].objectives.len()),
                            *id,
                        )
                    });
                    let mut last = "no supported alternative".to_owned();
                    for k in 0..=ids.len() - need {
                        let mut branch = state.clone();
                        let mut pending = tasks.clone();
                        pending.push(Task::Choose {
                            ids: ids[k + 1..].to_vec(),
                            need: need - 1,
                            members,
                            before,
                        });
                        match self
                            .completion(ids[k], members, before, &mut branch, &mut pending)
                            .and_then(|()| self.solve(branch, pending, depth + 1, work, class_goals, class_support))
                        {
                            Ok(s) => return Ok(s),
                            Err(e) if e == LIMIT => return Err(e),
                            Err(e) => last = e,
                        }
                    }
                    return Err(last);
                }
            };
            state.participants.insert(action.quest());
            let members = self.residual(action, members);
            if members == 0 {
                continue;
            }
            if !class_support {
                state.gear_actions.insert(action);
            }
            let old = state.nodes.get(&action).copied().unwrap_or(0);
            let members = members & !old;
            if members == 0 {
                continue;
            }
            let q = &self.quests[action.quest() as usize];
            if members & !q.members != 0 {
                return Err(format!(
                    "{} ({}) is unavailable to a participating member",
                    q.name, q.id
                ));
            }
            state.nodes.insert(action, old | members);
            self.compatible(&state, action)?;
            match action {
                Accept(i) => {
                    for &id in &q.pre_all {
                        self.completion(id, members, action, &mut state, &mut tasks)?;
                    }
                    if !q.pre_any.is_empty() {
                        let mut histories = BTreeMap::<Vec<i64>, u8>::new();
                        for m in 0..8 {
                            if members & (1 << m) != 0 {
                                let history = q.pre_any.iter().copied().filter(|id| self.completed(*id, m)).collect();
                                *histories.entry(history).or_default() |= 1 << m;
                            }
                        }
                        for members in histories.into_values() {
                            tasks.push(Task::Choose {
                                ids: q.pre_any.clone(),
                                need: q.pre_need,
                                members,
                                before: action,
                            });
                        }
                    }
                    for id in &q.pre_held {
                        if (0..8).any(|m| members & (1 << m) != 0 && self.completed(*id, m)) {
                            return Err(format!("held prerequisite {id} was already turned in"));
                        }
                        let &parent = self
                            .index
                            .get(id)
                            .ok_or_else(|| format!("held prerequisite {id} is not modeled"))?;
                        self.dependency(Accept(parent), members, Accept(i), &mut state, &mut tasks);
                        // Even if the parent's turn-in is discovered later, it follows the child's accept.
                        state.edges.insert((Accept(i), TurnIn(parent)));
                    }
                }
                Objective { quest: i, objective: k } => {
                    self.dependency(Accept(i), members, action, &mut state, &mut tasks);
                    let o = &q.objectives[k as usize];
                    for before in 0..q.objectives.len() {
                        if o.after & (1 << before) != 0 {
                            self.dependency(
                                Objective {
                                    quest: i,
                                    objective: before as u8,
                                },
                                members,
                                action,
                                &mut state,
                                &mut tasks,
                            );
                        }
                    }
                    if !o.handed_by.is_empty() {
                        tasks.push(Task::Choose {
                            ids: o.handed_by.clone(),
                            need: 1,
                            members,
                            before: action,
                        });
                    }
                }
                TurnIn(i) => {
                    if let Some(&j) = q.breadcrumb_for.as_ref().and_then(|id| self.index.get(id)) {
                        state.edges.insert((action, Accept(j)));
                    }
                    self.dependency(Accept(i), members, action, &mut state, &mut tasks);
                    for k in 0..q.objectives.len() {
                        self.dependency(
                            Objective {
                                quest: i,
                                objective: k as u8,
                            },
                            members,
                            action,
                            &mut state,
                            &mut tasks,
                        );
                    }
                }
            }
        }
        self.order(&state)?;
        if !class_support && !class_goals.is_empty() {
            self.solve(state.clone(), class_goals.to_vec(), depth, work, &[], true)?;
        }
        Ok(state)
    }

    fn compatible(&self, state: &Closure, action: BisQuestAction) -> Result<(), String> {
        let q = &self.quests[action.quest() as usize];
        let taken: HashSet<_> = state.nodes.keys().map(|a| a.quest()).collect();
        for (i, other) in self.quests.iter().enumerate() {
            if i == action.quest() as usize {
                continue;
            }
            let exclusive = q.exclusive.contains(&other.id) || other.exclusive.contains(&q.id);
            if exclusive && (taken.contains(&(i as u32)) || other.required) {
                return Err(format!(
                    "{} ({}) conflicts with {} ({})",
                    q.name, q.id, other.name, other.id
                ));
            }
        }
        for m in 0..8 {
            if q.members & (1 << m) == 0 || self.completed(q.id, m) {
                continue;
            }
            if let Some(s) = self.starts.get(m) {
                for id in s.completed.iter().map(|q| q.0).chain(s.log.iter().map(|q| q.id.0)) {
                    if q.exclusive.contains(&id)
                        || q.breadcrumb_for == Some(id)
                        || self
                            .index
                            .get(&id)
                            .is_some_and(|&i| self.quests[i as usize].exclusive.contains(&q.id))
                    {
                        return Err(format!("{} ({}) conflicts with recorded quest {id}", q.name, q.id));
                    }
                }
            }
        }
        for (name, roots) in &self.unproven_class {
            let mut blocked = true;
            for &root in roots {
                if !self.unavoidable_class_conflict(root, &taken)? {
                    blocked = false;
                    break;
                }
            }
            if blocked {
                return Err(format!(
                    "conflicts with selected class requirement {name}; its full support remains unproven"
                ));
            }
        }
        Ok(())
    }

    /// Incomplete data or a conservative combat failure is not permission to close a selected
    /// class root or its unavoidable residual support. Do not reserve every alternative.
    fn unavoidable_class_conflict(&self, root: u32, taken: &HashSet<u32>) -> Result<bool, String> {
        use BisQuestAction::{Accept, Objective, TurnIn};
        let mut todo = vec![(TurnIn(root), self.participants(root))];
        let mut seen = BTreeMap::<BisQuestAction, u8>::new();
        let mut work = 0;
        while let Some((action, members)) = todo.pop() {
            work += 1;
            if work > MAX_WORK {
                return Err(LIMIT.into());
            }
            let members = self.residual(action, members) & !seen.get(&action).copied().unwrap_or(0);
            if members == 0 {
                continue;
            }
            *seen.entry(action).or_default() |= members;
            let i = action.quest();
            let q = &self.quests[i as usize];
            if taken.iter().any(|&j| {
                i != j
                    && (q.exclusive.contains(&self.quests[j as usize].id)
                        || self.quests[j as usize].exclusive.contains(&q.id))
            }) {
                return Ok(true);
            }
            let mut dependencies = Vec::new();
            match action {
                Accept(_) => {
                    dependencies.extend(q.pre_all.iter().map(|&id| (id, true)));
                    let distinct: HashSet<_> = q.pre_any.iter().copied().collect();
                    if distinct.len() <= q.pre_need {
                        dependencies.extend(distinct.into_iter().map(|id| (id, true)));
                    }
                    dependencies.extend(q.pre_held.iter().map(|&id| (id, false)));
                }
                Objective { quest, objective } => {
                    todo.push((Accept(quest), members));
                    let o = &q.objectives[objective as usize];
                    let distinct: HashSet<_> = o.handed_by.iter().copied().collect();
                    if distinct.len() == 1 {
                        dependencies.extend(distinct.into_iter().map(|id| (id, true)));
                    }
                    for k in 0..q.objectives.len() {
                        if o.after & (1 << k) != 0 {
                            todo.push((
                                Objective {
                                    quest,
                                    objective: k as u8,
                                },
                                members,
                            ));
                        }
                    }
                }
                TurnIn(quest) => {
                    todo.push((Accept(quest), members));
                    todo.extend((0..q.objectives.len()).map(|k| {
                        (
                            Objective {
                                quest,
                                objective: k as u8,
                            },
                            members,
                        )
                    }));
                }
            }
            for (id, complete) in dependencies {
                let relevant = (0..8)
                    .filter(|&m| members & (1 << m) != 0 && !(complete && self.completed(id, m)))
                    .fold(0, |mask, m| mask | (1 << m));
                if relevant == 0 || complete && self.known.contains(&id) {
                    continue;
                }
                if taken.iter().any(|&j| self.quests[j as usize].exclusive.contains(&id)) {
                    return Ok(true);
                }
                if let Some(&j) = self.index.get(&id) {
                    todo.push((if complete { TurnIn(j) } else { Accept(j) }, relevant));
                }
            }
        }
        Ok(false)
    }

    /// Dependency order is independent of placement; windows are checked on each visit witness.
    fn order(&self, state: &Closure) -> Result<Vec<BisQuestAction>, String> {
        self.order_with_visits(state).map(|(actions, _)| actions)
    }

    fn order_with_visits(&self, state: &Closure) -> Result<VisitWitness, String> {
        let mut incoming: BTreeMap<_, usize> = state.nodes.keys().map(|&a| (a, 0)).collect();
        let mut following: BTreeMap<_, Vec<_>> = BTreeMap::new();
        for &(a, b) in &state.edges {
            if state.nodes.contains_key(&a) && state.nodes.contains_key(&b) {
                *incoming.get_mut(&b).expect("present node") += 1;
                following.entry(a).or_default().push(b);
            }
        }
        let mut ready: BTreeSet<_> = incoming.iter().filter_map(|(&a, &n)| (n == 0).then_some(a)).collect();
        let mut ordered = Vec::new();
        while let Some(a) = ready.pop_first() {
            for &b in following.get(&a).into_iter().flatten() {
                let count = incoming.get_mut(&b).expect("present node");
                *count -= 1;
                if *count == 0 {
                    ready.insert(b);
                }
            }
            ordered.push(a);
        }
        if ordered.len() != state.nodes.len() {
            return Err("cyclic quest action dependencies".into());
        }
        self.visit_order(state, &ordered)
    }

    fn check_windows(
        &self,
        state: &Closure,
        ordered: &[BisQuestAction],
        chosen: &BTreeMap<BisQuestAction, Option<usize>>,
    ) -> Result<(), String> {
        let mut following: BTreeMap<_, Vec<_>> = BTreeMap::new();
        for &(a, b) in &state.edges {
            if state.nodes.contains_key(&a) && state.nodes.contains_key(&b) {
                following.entry(a).or_default().push(b);
            }
        }
        let mut floors: BTreeMap<_, Floor> = BTreeMap::new();
        for &a in ordered {
            let gear = state.gear_actions.contains(&a);
            let (min, max) = self.window(a, gear, chosen[&a])?;
            let floor = min.merge(floors.get(&a).copied().unwrap_or_default());
            if floor.minimum.max(floor.combat) > max {
                let reason = if floor.minimum > max {
                    format!(
                        "modeled minimum-level/start/window constraints require level {}",
                        floor.minimum
                    )
                } else {
                    format!(
                        "the planner's conservative combat estimate requires level {} with plain gear and no class-quest power",
                        floor.combat
                    )
                };
                let endpoint = if gear {
                    "; gear actions must occur before the endpoint"
                } else {
                    ""
                };
                return Err(format!(
                    "{} ({}): {reason}, outside the supported window ending at {max}; the route ends at level {}{endpoint}",
                    self.quests[a.quest() as usize].name,
                    self.quests[a.quest() as usize].id,
                    self.profile.to_level,
                ));
            }
            for &b in following.get(&a).into_iter().flatten() {
                let next = floors.entry(b).or_default();
                *next = next.merge(floor);
            }
        }
        Ok(())
    }

    /// Contract each dungeon to one continuous visit. Internal quest order is retained, so
    /// only a path through another place and back is rejected, not sequential inside work.
    fn visit_order(&self, state: &Closure, ordered: &[BisQuestAction]) -> Result<VisitWitness, String> {
        let options: Vec<_> = ordered
            .iter()
            .map(|&a| {
                let q = &self.quests[a.quest() as usize];
                let mut places: Vec<Option<usize>> = match a {
                    BisQuestAction::Accept(_) => q.start_dungeon.map_or_else(
                        || q.starts.iter().map(|l| l.inside.map(usize::from)).collect(),
                        |d| vec![Some(d)],
                    ),
                    BisQuestAction::TurnIn(_) => q.ends.iter().map(|l| l.inside.map(usize::from)).collect(),
                    BisQuestAction::Objective { objective, .. } => {
                        let o = &q.objectives[objective as usize];
                        o.dungeon
                            .map_or_else(|| o.places().map(Objective::dungeon_of).collect(), |d| vec![Some(d)])
                    }
                };
                if places.is_empty() {
                    places.push(None);
                }
                places.sort_unstable();
                places.dedup();
                places
            })
            .collect();
        let mut chosen: BTreeMap<_, _> = ordered.iter().zip(&options).map(|(&a, p)| (a, p[0])).collect();
        if options.iter().all(|p| p == &[None]) {
            self.check_windows(state, ordered, &chosen)?;
            return Ok((ordered.to_vec(), chosen));
        }
        let choices: Vec<_> = ordered
            .iter()
            .copied()
            .zip(options)
            .filter(|(_, p)| p.len() > 1)
            .collect();
        let actions = self.choose_visits(state, ordered, &choices, 0, &mut chosen, &mut 0)?;
        Ok((actions, chosen))
    }

    fn choose_visits(
        &self,
        state: &Closure,
        ordered: &[BisQuestAction],
        choices: &[(BisQuestAction, Vec<Option<usize>>)],
        depth: usize,
        chosen: &mut BTreeMap<BisQuestAction, Option<usize>>,
        work: &mut usize,
    ) -> Result<Vec<BisQuestAction>, String> {
        *work += 1;
        if *work > MAX_WORK || depth > MAX_CHOICES {
            return Err(LIMIT.into());
        }
        if let Some((a, places)) = choices.get(depth) {
            let mut last = "no supported dungeon placement".to_owned();
            for &place in places {
                chosen.insert(*a, place);
                match self.choose_visits(state, ordered, choices, depth + 1, chosen, work) {
                    Ok(order) => return Ok(order),
                    Err(e) if e == LIMIT => return Err(e),
                    Err(e) => last = e,
                }
            }
            return Err(last);
        }
        // A failed location window is a failed branch, not a rejection of every alternative.
        self.check_windows(state, ordered, chosen)?;
        let visit = |a: BisQuestAction| chosen[&a].map_or(Visit::Outside(a), Visit::Dungeon);
        let mut incoming: BTreeMap<Visit, usize> = ordered.iter().map(|&a| (visit(a), 0)).collect();
        let mut edges = BTreeSet::new();
        let mut parents = BTreeMap::<BisQuestAction, Vec<BisQuestAction>>::new();
        for &(a, b) in &state.edges {
            if chosen.contains_key(&a) && chosen.contains_key(&b) {
                parents.entry(b).or_default().push(a);
                if visit(a) != visit(b) {
                    edges.insert((visit(a), visit(b)));
                }
            }
        }
        for &(_, b) in &edges {
            *incoming.get_mut(&b).expect("visit exists") += 1;
        }
        // Entry credit is available only for already accepted quests. Other work performed
        // inside cannot be an unsatisfied predecessor of that entry credit either.
        for &a in ordered {
            let BisQuestAction::Objective { quest, objective } = a else {
                continue;
            };
            let Some(d) = self.quests[quest as usize].objectives[objective as usize].dungeon else {
                continue;
            };
            let mut pending = vec![a];
            let mut ancestors = BTreeSet::new();
            while let Some(b) = pending.pop() {
                for &before in parents.get(&b).into_iter().flatten() {
                    *work += 1;
                    if *work > MAX_WORK {
                        return Err(LIMIT.into());
                    }
                    if !ancestors.insert(before) {
                        continue;
                    }
                    let entry_credit = match before {
                        BisQuestAction::Objective { quest, objective } => {
                            self.quests[quest as usize].objectives[objective as usize].dungeon == Some(d)
                        }
                        _ => false,
                    };
                    if chosen[&before] == Some(d) && !entry_credit {
                        return Err(format!(
                            "{}: entry-credit objectives need acceptance and prerequisites before entry; the planner supports one continuous visit",
                            self.dungeons[d].name
                        ));
                    }
                    pending.push(before);
                }
            }
        }
        let mut ready: BTreeSet<_> = incoming.iter().filter_map(|(&v, &n)| (n == 0).then_some(v)).collect();
        let mut result = Vec::new();
        while let Some(v) = ready.pop_first() {
            result.extend(ordered.iter().copied().filter(|&a| visit(a) == v));
            for &(_, next) in edges.iter().filter(|(from, _)| *from == v) {
                let n = incoming.get_mut(&next).expect("visit exists");
                *n -= 1;
                if *n == 0 {
                    ready.insert(next);
                }
            }
        }
        if result.len() != ordered.len() {
            let names: Vec<_> = incoming
                .iter()
                .filter_map(|(&v, &n)| match v {
                    Visit::Dungeon(d) if n > 0 => Some(self.dungeons[d].name.as_str()),
                    _ => None,
                })
                .collect();
            return Err(format!(
                "{} requires leaving and re-entering a dungeon; the planner supports one continuous visit",
                names.join(", ")
            ));
        }
        Ok(result)
    }

    fn window(&self, a: BisQuestAction, gear: bool, visit: Option<usize>) -> Result<(Floor, i64), String> {
        let q = &self.quests[a.quest() as usize];
        let combat = crate::plan::Combat {
            params: self.params,
            dungeons: self.dungeons,
        };
        let mut min = self.profile.from_level;
        let mut estimate = 0;
        let mut max = self.profile.to_level - i64::from(gear);
        match a {
            BisQuestAction::Accept(_) => {
                min = min.max(q.min_level);
                estimate = combat.guard_level(&q.start_guard, Might::PLAIN);
            }
            BisQuestAction::Objective { objective, .. } => {
                let o = &q.objectives[objective as usize];
                // Black-box entry credit uses the run's window. Direct work, inside or
                // outside, uses combat estimates only from the selected location category.
                if o.dungeon.is_none() {
                    estimate = o
                        .places()
                        .filter(|p| p.dungeon_of() == visit)
                        .map(|p| combat.level_at(q, p, Might::PLAIN))
                        .min()
                        .unwrap_or(0);
                }
            }
            BisQuestAction::TurnIn(_) => {
                min = min.max(q.completion_window.min_level.unwrap_or(0));
                estimate = combat.guard_level(&q.end_guard, Might::PLAIN);
                max = max.min(q.completion_window.max_level.unwrap_or(max));
            }
        }
        if let Some(d) = visit {
            let d = self
                .dungeons
                .get(d)
                .ok_or_else(|| "dungeon is not modeled".to_owned())?;
            min = min.max(d.min_level);
            if !q.implicit_window {
                min = min.max(q.completion_window.min_level.unwrap_or(0));
            }
            max = max.min(d.max_level);
        }
        Ok((
            Floor {
                minimum: min,
                combat: estimate,
            },
            max,
        ))
    }
}

#[cfg(test)]
mod tests;

/// Objectives already satisfied by every participating recorded member must survive filtering.
pub(super) fn recorded_objectives(quests: &[Quest], starts: &[&StartState]) -> HashSet<(i64, u8)> {
    let mut out = HashSet::new();
    for q in quests {
        for k in 0..q.objectives.len() {
            if (0..8).filter(|m| q.members & (1 << m) != 0).all(|m| {
                starts.get(m).is_some_and(|s| {
                    s.completed.iter().any(|id| id.0 == q.id)
                        || s.log.iter().find(|l| l.id.0 == q.id).is_some_and(|l| {
                            crate::job::objective_progress(q, l)
                                .iter()
                                .any(|&(j, v)| j == k as u8 && v >= 1.0)
                        })
                })
            }) {
                out.insert((q.id, k as u8));
            }
        }
    }
    out
}
