//! Costed source work following an ordinary route. Every candidate is replayed on the
//! same resources; the compact program is an instruction stream, not free reputation.

use super::state::{Breakdown, Event, Kind, State, Stop, Timed};
use super::{End, Planner, Simulator};
use crate::job::StartState;
use crate::model::{BoundAcquisition, BoundCreature, BoundReputationSource, BoundSourceKind};
use crate::reputation::{
    AcquisitionKind, CompletionTarget, GroupPolicy, RepAction, RepActionKind, RepPhase, RepPhaseKind, RepProgram,
    RepProgramResult, RepResultStatus, ReputationEffects, ReputationEvent, ResetKind, SourceConfidence, SourceLocation,
    SpilloverPolicy, StandingRange, Standings, apply_batch, apply_event,
};
use crate::world::Pos;
use anyhow::{Context, Result, bail};
use std::collections::{BTreeMap, BTreeSet};

const MAX_UNITS: u32 = 200_000;
const MAX_REPAIR_PASSES: usize = 2_048;
const RUN_UNITS: u32 = 256;
const MAX_ACQUISITION_KILLS: u32 = 4_096;
const MAX_EVALUATED_WORK: u64 = 2_000_000;
const MAX_REPLAY_VISITS: u64 = 2_000_000;

pub struct CompletedReputation {
    pub(crate) state: State,
    pub(crate) score: f64,
    pub trace: Vec<Timed>,
    /// Real active play time, excluding unverified calendar waiting.
    pub total: f64,
    pub breakdown: Breakdown,
    pub end: End,
    pub program: RepProgram,
}

#[derive(Clone, Default)]
struct Resources {
    inventory: BTreeMap<i64, u32>,
    completed: BTreeSet<i64>,
    daily_used: BTreeMap<String, u32>,
    kills: BTreeMap<i64, u32>,
    vendor_bought: BTreeMap<(i64, i64), u32>,
    opportunities: u32,
    resets: u32,
    units: u32,
}

impl Planner<'_> {
    pub fn complete_reputation(&self, route: &[Stop], start: Option<&StartState>) -> Result<CompletedReputation> {
        self.reputation_prefix(route, start, None)
    }

    /// Replay the saved occurrences as written. Editing does not run source search again.
    pub fn replay_reputation(
        &self,
        route: &[Stop],
        start: Option<&StartState>,
        program: &RepProgram,
    ) -> Result<CompletedReputation> {
        self.reputation_prefix(route, start, Some(program))
    }

    fn reputation_prefix(
        &self,
        route: &[Stop],
        start: Option<&StartState>,
        stored: Option<&RepProgram>,
    ) -> Result<CompletedReputation> {
        let mut s = self.sim().start_state(self.planned(route));
        self.seed_reputation(&mut s, start);
        let mut trace = Vec::new();
        for (k, &stop) in route.iter().enumerate() {
            if self.sim().ended(&s) {
                break;
            }
            if !self.availability().valid(&s, stop) {
                bail!("invalid ordinary route at stop {k}");
            }
            s.at = k as u32;
            s.next = self.sim().next_pos(&s, route, k);
            self.sim().apply(&mut s, stop, Some(&mut trace));
            if !s.time.is_finite() || s.completion_error.is_some() {
                bail!("ordinary route failed at stop {k}");
            }
        }
        self.sim().finish_level(&mut s, Some(&mut trace));
        self.finish_reputation_state(s, trace, start, stored)
    }

    pub(crate) fn seed_reputation(&self, s: &mut State, start: Option<&StartState>) {
        s.reputation_sources_used = Some(std::rc::Rc::new(BTreeSet::new()));
        s.reputation_all = self.model.initial.reputation_all.clone();
        if let Some(start) = start {
            s.reputation_all
                .extend(start.reputation.iter().map(|r| (r.id, r.value)));
        }
        for (goal, have) in self.params.reputation_goals.iter().zip(&s.reputation) {
            s.reputation_all.insert(goal.faction, *have);
        }
    }

    /// The caller has replayed the prefix and reached its target level, but has not
    /// performed terminal accounting (unfinished objective conversion is not idempotent).
    pub(crate) fn finish_reputation_state(
        &self,
        mut s: State,
        mut trace: Vec<Timed>,
        start: Option<&StartState>,
        stored: Option<&RepProgram>,
    ) -> Result<CompletedReputation> {
        let mut r = Resources::default();
        if let Some(start) = start {
            if start.inventory_valid {
                for item in &start.inventory {
                    *r.inventory.entry(item.item).or_default() = r
                        .inventory
                        .get(&item.item)
                        .copied()
                        .unwrap_or(0)
                        .saturating_add(item.count);
                }
            }
            r.completed.extend(start.completed.iter().map(|q| q.0));
            if start.daily_valid
                && let Some(daily) = &start.daily
            {
                for source in &self.model.reputation_sources.sources {
                    if source.descriptor.reset.kind == ResetKind::Daily
                        && source
                            .descriptor
                            .identity
                            .quest_id
                            .is_some_and(|q| daily.completed.iter().any(|done| done.0 == q))
                    {
                        r.daily_used
                            .insert(quota(source), source.descriptor.reset.units_per_reset);
                    }
                }
            }
        }
        // Ordinary inventory is a different expected-loot model. Never let a real
        // imported item pay for its prefix delivery and again for the completion.
        for row in &trace {
            if let Event::Stop { stop, .. } = &row.event
                && stop.kind == Kind::TurnIn
            {
                let id = self.model.quests[stop.index as usize].id;
                if let Some(inputs) = self.model.reputation_sources.quest_inputs.get(&id) {
                    for item in inputs {
                        let have = r.inventory.entry(item.item_id).or_default();
                        *have = have.saturating_sub(item.count);
                    }
                }
            }
        }
        r.completed.extend(
            (0..self.model.quests.len())
                .filter(|&i| s.rewarded(i))
                .map(|i| self.model.quests[i].id),
        );
        let before = s.reputation_all.clone();
        let active_start = s.time;
        let mut program = stored.cloned().unwrap_or_else(|| RepProgram {
            version: 1,
            requested_edition: self.params.edition.key().into(),
            effective_edition: self.params.edition.key().into(),
            ..RepProgram::default()
        });
        let mut warnings: BTreeSet<String> = BTreeSet::new();
        if stored.is_some() {
            self.replay_program(&mut s, &mut r, &program, &mut warnings);
        } else {
            self.build_program(&mut s, &mut r, &mut program, &mut warnings);
        }
        if let Some(used) = &s.reputation_sources_used {
            for source in &self.model.reputation_sources.sources {
                if used.contains(&source.descriptor.id) {
                    register_source(&mut program, source);
                }
            }
        }
        for source in &program.sources {
            if let Some(bound) = self
                .model
                .reputation_sources
                .sources
                .iter()
                .find(|b| b.descriptor.id == source.id)
            {
                warnings.extend(bound.warnings.iter().cloned());
            }
            if source.confidence == SourceConfidence::ClassicEstimate {
                warnings.insert("Classic-derived reputation rules are forecasts; gray-level scaling, per-kill rounding and live server behavior may differ.".into());
            }
        }
        let targets: Standings = self
            .params
            .reputation_goals
            .iter()
            .map(|g| (g.faction, g.target as f64))
            .collect();
        let deficits: Standings = targets
            .iter()
            .filter_map(|(&f, &target)| {
                let gap = (target - s.reputation_all.get(&f).copied().unwrap_or(0.0)).max(0.0);
                (gap > 1e-9).then_some((f, gap))
            })
            .collect();
        for (&faction, &gap) in &deficits {
            let name = self.model.faction_names.get(&faction).map_or("Faction", String::as_str);
            let supported = self.model.reputation_sources.sources.iter().any(|source| {
                source.supported
                    && source
                        .descriptor
                        .effects
                        .awards
                        .iter()
                        .any(|a| a.faction_id == faction && a.amount > 0.0)
            });
            let limitation = if supported {
                "supported sources are limited by their standing caps, prerequisites or available resources"
            } else {
                "no currently bound direct earning source is supported; any available spillover was considered"
            };
            warnings.insert(format!("Reputation {name} ({faction}): best-found result is {gap:.2} points short; {limitation}. This bounded search does not prove a maximum."));
        }
        if r.opportunities > 0 {
            warnings.insert("Daily opportunities are minimum required completion windows, not a completion date. First availability and calendar waiting are unverified; no midnight reset is assumed.".into());
        }
        if !program.phases.is_empty() {
            warnings.insert("Reputation activity travel uses supported walking paths. Flight, boat and teleport setup is not represented in this compact program; sources requiring those trips are left unsupported.".into());
        }
        if !r.kills.is_empty() {
            warnings.insert("Source farming uses one selected spawn and conservative respawn waits. Intervening travel and deliveries do not offset those waits, so the forecast may be substantially longer than in-game farming.".into());
        }
        if !deficits.is_empty() {
            // The catalog's availability note is policy, not evidence of an applied
            // estimate. Actual estimate warnings travel with used source descriptors.
            warnings.extend(
                self.model
                    .reputation_sources
                    .warnings
                    .iter()
                    .filter(|w| !w.starts_with("Classic VMaNGOS"))
                    .cloned(),
            );
        }
        program.result = RepProgramResult {
            start: before,
            reached: s.reputation_all.clone(),
            targets,
            deficits,
            status: if self
                .params
                .reputation_goals
                .iter()
                .all(|g| s.reputation_all.get(&g.faction).copied().unwrap_or(0.0) + 1e-9 >= g.target as f64)
            {
                RepResultStatus::Satisfied
            } else {
                RepResultStatus::BestFound
            },
            active_play_seconds: s.time - active_start,
            daily_opportunities: r.opportunities,
            reset_transitions: r.resets,
            calendar_wait_seconds: if r.opportunities == 0 { Some(0.0) } else { None },
            warnings: warnings.into_iter().collect(),
        };
        // Prefix stopping follows the ordinary quest vector. Newly identified kill
        // rewards must not cut valid prefix content off early; the combined map is
        // authoritative at the single terminal accounting boundary.
        for (g, goal) in self.params.reputation_goals.iter().enumerate() {
            s.reputation[g] = s.reputation_all.get(&goal.faction).copied().unwrap_or(0.0);
        }
        let score = self.sim().finish_accounting(&mut s);
        if !score.is_finite() || s.completion_error.is_some() {
            bail!("combined route replay is not finite");
        }
        let mut end = self.end_state(&s, r.inventory);
        end.completed_quests = r.completed.into_iter().collect();
        if !program.phases.is_empty() || !self.params.reputation_goals.is_empty() {
            super::step::recorder(&mut Some(&mut trace))(
                &s,
                Event::ReputationProgram {
                    program: Box::new(program.clone()),
                },
            );
        }
        let total = s.spent.total;
        let breakdown = s.spent;
        Ok(CompletedReputation {
            state: s,
            score,
            trace,
            total,
            breakdown,
            end,
            program,
        })
    }

    fn deficit(&self, s: &State) -> f64 {
        self.params
            .reputation_goals
            .iter()
            .map(|g| (g.target as f64 - s.reputation_all.get(&g.faction).copied().unwrap_or(0.0)).max(0.0))
            .sum()
    }

    fn relevant(&self, source: &BoundReputationSource) -> bool {
        let effects = &source.descriptor.effects;
        self.params.reputation_goals.iter().any(|g| effects.awards.iter().any(|a| a.faction_id == g.faction && a.amount > 0.0)
            || matches!(&effects.spillover, SpilloverPolicy::Explicit(edges) if edges.iter().any(|e| e.recipient_faction_id == g.faction && e.rate > 0.0)))
    }

    fn build_program(
        &self,
        s: &mut State,
        r: &mut Resources,
        program: &mut RepProgram,
        warnings: &mut BTreeSet<String>,
    ) {
        let sources: Vec<&BoundReputationSource> = self
            .model
            .reputation_sources
            .sources
            .iter()
            .filter(|source| self.relevant(source))
            .collect();
        let mut evaluated_work = 0u64;
        for pass in 0..MAX_REPAIR_PASSES {
            if self.deficit(s) <= 1e-9 {
                return;
            }
            if r.units >= MAX_UNITS || self.aborted() {
                warnings.insert("Reputation source search stopped at its explicit work limit; retained best-found supported progress.".into());
                return;
            }
            let before = self.deficit(s);
            let mut best: Option<(&BoundReputationSource, f64)> = None;
            for source in sources
                .iter()
                .filter(|source| source.supported && source.descriptor.reset.kind != ResetKind::Daily)
            {
                let (mut candidate, mut resources) = (s.clone(), r.clone());
                let outcome = self.source_unit(&mut candidate, &mut resources, source, true);
                evaluated_work += u64::from(resources.units.saturating_sub(r.units)) + 1;
                if evaluated_work >= MAX_EVALUATED_WORK {
                    warnings.insert("Reputation source search reached its explicit candidate-work limit; retained best-found progress.".into());
                    return;
                }
                match outcome {
                    Ok(_) => {
                        let gain = before - self.deficit(&candidate);
                        if gain > 1e-9 {
                            let value = gain / (candidate.time - s.time).max(0.01);
                            if best.as_ref().is_none_or(|(_, old)| value > *old) {
                                best = Some((source, value));
                            }
                        }
                    }
                    Err(reason) => {
                        warnings.insert(format!("{}: {reason}", source.descriptor.name));
                    }
                }
            }
            if let Some((source, _)) = best {
                for _ in 0..RUN_UNITS {
                    if r.units >= MAX_UNITS {
                        break;
                    }
                    let (mut candidate, mut resources) = (s.clone(), r.clone());
                    let Ok(actions) = self.source_unit(&mut candidate, &mut resources, source, true) else {
                        break;
                    };
                    if self.deficit(&candidate) + 1e-9 >= self.deficit(s) {
                        break;
                    }
                    *s = candidate;
                    *r = resources;
                    record_phase(program, RepPhaseKind::Batch, actions, source);
                    if self.deficit(s) <= 1e-9 {
                        return;
                    }
                }
                continue;
            }
            // A daily opportunity contains every useful eligible source before any reset.
            let (mut candidate, mut resources) = (s.clone(), r.clone());
            let mut actions = Vec::new();
            let mut used = Vec::new();
            for source in sources
                .iter()
                .filter(|source| source.supported && source.descriptor.reset.kind == ResetKind::Daily)
            {
                let (mut next, mut next_resources) = (candidate.clone(), resources.clone());
                match self.source_unit(&mut next, &mut next_resources, source, true) {
                    Ok(more) if self.deficit(&next) + 1e-9 < self.deficit(&candidate) => {
                        candidate = next;
                        resources = next_resources;
                        actions.extend(more);
                        used.push(*source);
                    }
                    Err(reason) => {
                        warnings.insert(format!("{}: {reason}", source.descriptor.name));
                    }
                    _ => {}
                }
            }
            if !actions.is_empty() {
                resources.opportunities += 1;
                *s = candidate;
                *r = resources;
                let first = used[0];
                record_phase(program, RepPhaseKind::DailyCircuit, actions, first);
                for source in used {
                    register_source(program, source);
                }
                continue;
            }
            if !r.daily_used.is_empty() {
                // Do not fabricate future writs/stock: prove a useful funded action exists
                // on the next window before adding a reset transition.
                let mut next_resources = r.clone();
                next_resources.daily_used.clear();
                let useful = sources
                    .iter()
                    .filter(|source| source.descriptor.reset.kind == ResetKind::Daily)
                    .any(|source| {
                        let (mut next, mut resources) = (s.clone(), next_resources.clone());
                        self.source_unit(&mut next, &mut resources, source, true).is_ok()
                            && self.deficit(&next) + 1e-9 < self.deficit(s)
                    });
                if useful {
                    r.daily_used.clear();
                    r.resets += 1;
                    continue;
                }
            }
            if pass + 1 == MAX_REPAIR_PASSES {
                break;
            }
            return;
        }
        warnings.insert("Reputation source search reached its explicit repair-pass limit; retained best-found progress rather than claiming a proven maximum.".into());
    }

    fn eligible(s: &State, r: &Resources, source: &BoundReputationSource) -> Result<()> {
        let d = &source.descriptor;
        if !source.supported {
            bail!("current entity binding, exclusion or eligibility is unsupported");
        }
        if source.kind == BoundSourceKind::Kill && !source.automatic_farming {
            bail!("exact outdoor farming is not supported for this creature");
        }
        let e = &d.eligibility;
        if e.minimum_level.is_some_and(|l| s.level < l) || e.maximum_level.is_some_and(|l| s.level > l) {
            bail!("level eligibility is not met");
        }
        if e.prerequisite_quests.iter().any(|q| !r.completed.contains(q))
            || !source.prerequisite_any.is_empty()
                && source
                    .prerequisite_any
                    .iter()
                    .filter(|q| r.completed.contains(q))
                    .count()
                    < source.prerequisite_count
        {
            bail!("required prerequisite quest is not completed");
        }
        if e.excluded_quests.iter().any(|q| r.completed.contains(q)) {
            bail!("an exclusive quest was already completed");
        }
        if !d.repeatable && d.identity.quest_id.is_some_and(|id| r.completed.contains(&id)) {
            bail!("one-time introduction is already completed");
        }
        for requirement in &e.standing {
            let have = s.reputation_all.get(&requirement.faction_id).copied().unwrap_or(0.0);
            if !within(have, requirement.standing) {
                bail!(
                    "standing prerequisite is not met for faction {}",
                    requirement.faction_id
                );
            }
        }
        if d.reset.kind == ResetKind::Daily
            && r.daily_used.get(&quota(source)).copied().unwrap_or(0) >= d.reset.units_per_reset
        {
            bail!("daily quota is used in this reset window");
        }
        Ok(())
    }

    fn source_unit(
        &self,
        s: &mut State,
        r: &mut Resources,
        source: &BoundReputationSource,
        acquire: bool,
    ) -> Result<Vec<RepAction>> {
        Self::eligible(s, r, source)?;
        let d = &source.descriptor;
        let mut effects = d.effects.clone();
        if let Some(creature) = &source.creature {
            effects.modifiers.gray_multiplier = gray_multiplier(s.level, creature);
        }
        let mut probe = s.reputation_all.clone();
        let delta = apply_event(
            &mut probe,
            &ReputationEvent {
                source_id: d.id.clone(),
                edition: d.edition.clone(),
                confidence: d.confidence,
                effects: effects.clone(),
            },
        )?;
        if delta.deltas.is_empty()
            || self.params.reputation_goals.iter().all(|g| {
                delta.deltas.get(&g.faction).copied().unwrap_or(0.0) <= 0.0
                    || s.reputation_all.get(&g.faction).copied().unwrap_or(0.0) >= g.target as f64
            })
        {
            bail!("standing gate/cap exhausted for selected goals");
        }
        let mut actions = Vec::new();
        for input in &d.input_items {
            let have = r.inventory.get(&input.item_id).copied().unwrap_or(0);
            if have < input.count {
                if !acquire {
                    bail!(
                        "saved program lacks {} units of material {}",
                        input.count - have,
                        input.item_id
                    );
                }
                let missing = input.count - have;
                let mut best = None;
                let mut failures = BTreeSet::new();
                for path in source
                    .acquisition
                    .iter()
                    .filter(|a| a.descriptor.item_id == input.item_id)
                {
                    let (mut next, mut resources) = (s.clone(), r.clone());
                    match self.acquire(&mut next, &mut resources, path, missing) {
                        Ok(())
                            if best
                                .as_ref()
                                .is_none_or(|(old, _, _): &(State, Resources, &BoundAcquisition)| {
                                    next.time < old.time
                                }) =>
                        {
                            best = Some((next, resources, path));
                        }
                        Err(reason) => {
                            failures.insert(reason.to_string());
                        }
                        _ => {}
                    }
                }
                let Some((next, resources, path)) = best else {
                    if !failures.is_empty() {
                        bail!(
                            "material {} acquisition unavailable: {}",
                            input.item_id,
                            failures.into_iter().collect::<Vec<_>>().join("; ")
                        );
                    }
                    if source.acquisition.iter().any(|a| {
                        a.descriptor.item_id == input.item_id && a.descriptor.kind == AcquisitionKind::PricedVendor
                    }) {
                        bail!(
                            "insufficient money or unsupported vendor stock for material {}",
                            input.item_id
                        );
                    }
                    bail!(
                        "material {} (including starter items) has insufficient held stock and no affordable supported acquisition",
                        input.item_id
                    );
                };
                let location = path.locations.iter().find(|l| position(l) == Some(next.pos)).cloned();
                let mut acquisition = path.descriptor.clone();
                acquisition.location.clone_from(&location);
                *s = next;
                *r = resources;
                actions.push(RepAction {
                    source_id: d.id.clone(),
                    kind: RepActionKind::Acquire,
                    units_per_cycle: missing,
                    total_unit_limit: missing,
                    location,
                    acquisition: Some(acquisition),
                    completion_targets: vec![CompletionTarget::Inventory {
                        item_id: input.item_id,
                        count: input.count,
                    }],
                    ..RepAction::default()
                });
            }
        }
        if s.spent.gold() < d.costs.money_cost_copper {
            bail!("insufficient money for the turn-in cost");
        }
        if source.kind == BoundSourceKind::Turnin && !source.giver_locations.is_empty() {
            self.source_travel(s, &source.giver_locations)?;
        }
        let location = self.source_travel(s, &source.locations)?;
        Self::eligible(s, r, source)?;
        if s.spent.gold() < d.costs.money_cost_copper {
            bail!("insufficient money after source travel");
        }
        if let Some(creature) = &source.creature {
            effects.modifiers.gray_multiplier = gray_multiplier(s.level, creature);
            self.exact_kills(s, r, creature, 1)?;
        }
        for input in &d.input_items {
            let held = r.inventory.entry(input.item_id).or_default();
            if *held < input.count {
                bail!("material {} is exhausted", input.item_id);
            }
            *held -= input.count;
        }
        s.time +=
            d.costs.travel_seconds + d.costs.combat_seconds + d.costs.respawn_seconds + d.costs.interaction_seconds;
        s.spent.travel += d.costs.travel_seconds;
        s.spent.fighting += d.costs.combat_seconds + d.costs.respawn_seconds;
        s.spent.waiting += d.costs.respawn_seconds;
        s.spent.overhead += d.costs.interaction_seconds;
        s.spent.vendor_cost += d.costs.money_cost_copper;
        s.spent.gold_earned += d.costs.money_reward_copper;
        if d.costs.xp_reward > 0.0 {
            self.growth().gain(s, d.costs.xp_reward.floor() as i64);
            s.spent.quest_xp += d.costs.xp_reward.floor() as i64;
        }
        // The kill's eligibility and gray amount use the pre-kill level, not its XP reward.
        self.sim().apply_reputation_effects(s, &d.id, d.confidence, effects);
        if let Some(id) = d.identity.quest_id {
            r.completed.insert(id);
        }
        if d.reset.kind == ResetKind::Daily {
            *r.daily_used.entry(quota(source)).or_default() += 1;
        }
        r.units += 1;
        actions.push(RepAction {
            source_id: d.id.clone(),
            kind: if source.kind == BoundSourceKind::Kill {
                RepActionKind::Kill
            } else {
                RepActionKind::Turnin
            },
            units_per_cycle: 1,
            total_unit_limit: 1,
            location: Some(location),
            completion_targets: self
                .params
                .reputation_goals
                .iter()
                .filter(|g| delta.deltas.get(&g.faction).copied().unwrap_or(0.0) > 0.0)
                .map(|g| CompletionTarget::Reputation {
                    faction_id: g.faction,
                    standing: g.target as f64,
                })
                .collect(),
            ..RepAction::default()
        });
        Ok(actions)
    }

    fn source_travel(&self, s: &mut State, locations: &[SourceLocation]) -> Result<SourceLocation> {
        let trips = self.trips().at(s);
        let (target, zone, seconds, location) = locations
            .iter()
            .filter_map(|l| {
                let p = position(l)?;
                let seconds = trips.walk(&s.pos, &p, s.level);
                seconds
                    .is_finite()
                    .then_some((p, l.zone_id.unwrap_or(p.zone), seconds, l))
            })
            .min_by(|a, b| a.2.total_cmp(&b.2))
            .context("no supported walking path; cross-continent flight/boat/teleport setup is not represented")?;
        let from = (s.zone == zone).then_some(s.pos);
        self.sim().explore(s, from, &target, zone);
        s.time += seconds;
        s.spent.travel += seconds;
        s.spent.zone_changes += u32::from(zone != s.zone);
        s.pos = target;
        s.zone = zone;
        s.landed = super::state::Landed::No;
        Ok(location.clone())
    }

    fn acquire(&self, s: &mut State, r: &mut Resources, path: &BoundAcquisition, count: u32) -> Result<()> {
        let a = &path.descriptor;
        self.source_travel(s, &path.locations)?;
        match a.kind {
            AcquisitionKind::PricedVendor => {
                let price = a
                    .unit_price_copper
                    .filter(|p| *p > 0)
                    .context("vendor price is unknown")?;
                let cost = price.checked_mul(i64::from(count)).context("vendor cost overflow")?;
                if s.spent.gold() < cost {
                    bail!("insufficient money");
                }
                let key = (a.npc_id.context("vendor NPC is unknown")?, a.item_id);
                let bought = r.vendor_bought.entry(key).or_default();
                if let Some((stock, restock)) = path.stock {
                    if stock == 0 || !restock.is_finite() || restock <= 0.0 {
                        bail!("vendor replenishment is unsupported");
                    }
                    let old_waits = bought.saturating_sub(stock);
                    let new_waits = bought.saturating_add(count).saturating_sub(stock);
                    let wait = f64::from(new_waits - old_waits) * restock;
                    s.time += wait;
                    s.spent.waiting += wait;
                    s.spent.fighting += wait;
                }
                *bought = bought.saturating_add(count);
                s.spent.vendor_cost += cost;
            }
            AcquisitionKind::ExactCreatureDrop => {
                if self.params.group_size > 1 {
                    bail!(
                        "group material drop credit and individual ownership are unverified; use known inventory or a priced vendor"
                    );
                }
                let chance = a
                    .drop_chance
                    .filter(|c| c.is_finite() && *c > 0.0 && *c <= 1.0)
                    .context("drop probability is unsupported")?;
                let kills = (f64::from(count) / (chance * f64::from(a.units_per_acquisition.max(1)))).ceil();
                if kills > f64::from(MAX_ACQUISITION_KILLS) || kills as u32 >= MAX_UNITS.saturating_sub(r.units) {
                    bail!("material acquisition exceeds the explicit per-cycle or total work limit");
                }
                let creature = path.creature.as_ref().context("exact creature is unknown")?;
                for _ in 0..kills as u32 {
                    if self.aborted() {
                        bail!("material acquisition was interrupted; retained earlier supported progress");
                    }
                    let at_level = s.level;
                    self.exact_kills(s, r, creature, 1)?;
                    self.sim().identified_kill_reputation(s, creature.npc_id, at_level, 1);
                    r.units += 1;
                }
            }
            _ => bail!("material supply is not an exact farm or priced vendor"),
        }
        s.time += self.params.object_time;
        s.spent.overhead += self.params.object_time;
        *r.inventory.entry(a.item_id).or_default() =
            r.inventory.get(&a.item_id).copied().unwrap_or(0).saturating_add(count);
        Ok(())
    }

    fn exact_kills(&self, s: &mut State, r: &mut Resources, creature: &BoundCreature, count: u32) -> Result<()> {
        if creature.spawn_count == 0
            || creature.rank != 0
            || self.params.reach(s.level, s.might()) < creature.max_level as f64
        {
            bail!("creature difficulty exceeds supported normal outdoor combat");
        }
        let prior = r.kills.entry(creature.npc_id).or_default();
        // Other recorded spawn points may be thousands of yards away. The program
        // stands at one selected point and cannot borrow those spawns for free.
        let pool = 1;
        let each = self.params.kill_time(s.level, s.bonus, creature.max_level);
        let old_waves = prior.saturating_sub(1) / pool;
        let new_waves = prior.saturating_add(count).saturating_sub(1) / pool;
        let wait = if new_waves > old_waves {
            let respawn = creature
                .respawn_seconds
                .context("respawn time is unknown beyond currently bound spawns")?;
            f64::from(new_waves - old_waves) * (respawn - each * f64::from(pool)).max(0.0)
        } else {
            0.0
        };
        *prior = prior.saturating_add(count);
        let seconds = f64::from(count) * each + wait;
        s.time += seconds;
        s.spent.fighting += seconds;
        s.spent.waiting += wait;
        let xp = (f64::from(count)
            * self.rules().mob_xp(
                s.level,
                creature.max_level,
                crate::xp::content_at(s.pos.continent, s.zone),
            )
            * self.params.group_xp_share())
        .floor() as i64;
        s.spent.mob_xp += self.growth().gain_kills(s, xp);
        Ok(())
    }

    fn replay_program(&self, s: &mut State, r: &mut Resources, program: &RepProgram, warnings: &mut BTreeSet<String>) {
        let mut counts: BTreeMap<String, u32> = BTreeMap::new();
        let mut had_daily = false;
        let mut visits = 0;
        for phase in &program.phases {
            if self.deficit(s) <= 1e-9 {
                return;
            }
            if !self.replay_visit(&mut visits, warnings) {
                return;
            }
            for _ in 0..phase.cycles {
                if self.deficit(s) <= 1e-9 {
                    return;
                }
                if !self.replay_visit(&mut visits, warnings) {
                    return;
                }
                if r.units >= MAX_UNITS {
                    warnings.insert("Saved reputation program reached the explicit replay work limit; remaining actions were retained as an unmet forecast.".into());
                    return;
                }
                let calendar_before = (r.daily_used.clone(), r.resets, had_daily);
                if phase.kind == RepPhaseKind::DailyCircuit {
                    if had_daily
                        || phase.actions.iter().any(|a| {
                            self.model
                                .reputation_sources
                                .sources
                                .iter()
                                .find(|s| s.descriptor.id == a.source_id)
                                .is_some_and(|s| {
                                    r.daily_used.get(&quota(s)).copied().unwrap_or(0)
                                        >= s.descriptor.reset.units_per_reset
                                })
                        })
                    {
                        r.daily_used.clear();
                        r.resets += 1;
                    }
                    had_daily = true;
                }
                let mut changed_daily = false;
                let mut performed = false;
                for action in &phase.actions {
                    if !self.replay_visit(&mut visits, warnings) {
                        return;
                    }
                    let Some(bound) = self
                        .model
                        .reputation_sources
                        .sources
                        .iter()
                        .find(|source| source.descriptor.id == action.source_id)
                    else {
                        warnings.insert(format!(
                            "Saved source {} is no longer bound to current data.",
                            action.source_id
                        ));
                        continue;
                    };
                    let mut source = bound.clone();
                    if let Some(saved) = program.sources.iter().find(|d| d.id == action.source_id) {
                        source.descriptor = saved.clone();
                    }
                    if action.kind != RepActionKind::Acquire
                        && let Some(location) = &action.location
                    {
                        if source.locations.contains(location) {
                            source.locations = vec![location.clone()];
                        } else {
                            warnings.insert(format!(
                                "Saved source {}: its selected current world position is no longer supported.",
                                action.source_id
                            ));
                            continue;
                        }
                    }
                    let count = counts.entry(action.id.clone()).or_default();
                    let mut units = action
                        .units_per_cycle
                        .min(action.total_unit_limit.saturating_sub(*count));
                    if action.kind == RepActionKind::Acquire {
                        let Some(acquisition) = &action.acquisition else {
                            warnings.insert(format!(
                                "Saved source {} has no acquisition identity.",
                                action.source_id
                            ));
                            continue;
                        };
                        let maximum = bound
                            .descriptor
                            .input_items
                            .iter()
                            .find(|input| input.item_id == acquisition.item_id)
                            .map_or(0, |input| input.count);
                        let target = action
                            .completion_targets
                            .iter()
                            .filter_map(|target| match target {
                                CompletionTarget::Inventory { item_id, count } if *item_id == acquisition.item_id => {
                                    Some(*count)
                                }
                                _ => None,
                            })
                            .max()
                            .unwrap_or(maximum);
                        if maximum == 0 || target > maximum || units > maximum {
                            warnings.insert(format!(
                                "Saved source {} acquisition exceeds its supported one-turn-in carry batch.",
                                action.source_id
                            ));
                            continue;
                        }
                        units = units
                            .min(target.saturating_sub(r.inventory.get(&acquisition.item_id).copied().unwrap_or(0)));
                    }
                    for _ in 0..if action.kind == RepActionKind::Acquire {
                        u32::from(units > 0)
                    } else {
                        units
                    } {
                        if !self.replay_visit(&mut visits, warnings) {
                            return;
                        }
                        if r.units >= MAX_UNITS || self.aborted() {
                            warnings.insert("Saved reputation program stopped at its explicit work or cancellation limit; retained supported progress.".into());
                            return;
                        }
                        let (mut next, mut resources) = (s.clone(), r.clone());
                        let outcome = if action.kind == RepActionKind::Acquire {
                            action
                                .acquisition
                                .as_ref()
                                .and_then(|a| {
                                    source.acquisition.iter().find(|p| {
                                        p.descriptor.item_id == a.item_id
                                            && p.descriptor.npc_id == a.npc_id
                                            && p.descriptor.kind == a.kind
                                    })
                                })
                                .context("saved acquisition path is no longer supported")
                                .and_then(|path| {
                                    let mut path = path.clone();
                                    if let Some(location) = &action.location {
                                        if !path.locations.contains(location) {
                                            bail!("saved acquisition location is no longer supported");
                                        }
                                        path.locations = vec![location.clone()];
                                    }
                                    self.acquire(&mut next, &mut resources, &path, units)
                                })
                        } else {
                            self.source_unit(&mut next, &mut resources, &source, false).map(|_| ())
                        };
                        match outcome {
                            Ok(()) => {
                                performed = true;
                                *s = next;
                                *r = resources;
                                *count += if action.kind == RepActionKind::Acquire {
                                    units
                                } else {
                                    1
                                };
                                changed_daily |= action.kind != RepActionKind::Acquire
                                    && source.descriptor.reset.kind == ResetKind::Daily;
                            }
                            Err(reason) => {
                                warnings.insert(format!("Saved source {}: {reason}", action.source_id));
                                break;
                            }
                        }
                    }
                }
                if phase.kind == RepPhaseKind::DailyCircuit {
                    if changed_daily {
                        r.opportunities += 1;
                    } else {
                        (r.daily_used, r.resets, had_daily) = calendar_before;
                    }
                }
                if !performed {
                    warnings.insert(format!("Saved reputation phase {} has no executable supported actions; remaining cycles were not replayed.", phase.id));
                    break;
                }
            }
        }
    }

    fn replay_visit(&self, visits: &mut u64, warnings: &mut BTreeSet<String>) -> bool {
        *visits += 1;
        if *visits > MAX_REPLAY_VISITS || self.aborted() {
            warnings.insert("Saved reputation program reached its explicit replay visit limit or was interrupted; retained earlier supported progress.".into());
            false
        } else {
            true
        }
    }
}

impl Simulator<'_> {
    pub(crate) fn apply_reputation_effects(
        &self,
        s: &mut State,
        id: &str,
        confidence: SourceConfidence,
        effects: ReputationEffects,
    ) {
        let event = ReputationEvent {
            source_id: id.into(),
            edition: self.params.edition.key().into(),
            confidence,
            effects,
        };
        if let Ok(result) = apply_event(&mut s.reputation_all, &event) {
            for (g, goal) in self.params.reputation_goals.iter().enumerate() {
                s.reputation[g] += result.deltas.get(&goal.faction).copied().unwrap_or(0.0);
            }
        }
    }

    /// Only an identified creature death can select a creature earning rule.
    pub(crate) fn identified_kill_reputation(&self, s: &mut State, npc: i64, level: i64, count: u32) {
        if s.reputation_all.is_empty() {
            return;
        }
        for source in &self.model.reputation_sources.sources {
            if source.kind != BoundSourceKind::Kill
                || source.descriptor.identity.npc_id != Some(npc)
                || source.descriptor.group_policy == GroupPolicy::Unsupported
                || source.creature.is_none()
            {
                continue;
            }
            let mut effects = source.descriptor.effects.clone();
            if let Some(creature) = &source.creature {
                effects.modifiers.gray_multiplier = gray_multiplier(level, creature);
            }
            let event = ReputationEvent {
                source_id: source.descriptor.id.clone(),
                edition: self.params.edition.key().into(),
                confidence: source.descriptor.confidence,
                effects,
            };
            if let Ok(result) = apply_batch(&mut s.reputation_all, &event, count)
                && !result.deltas.is_empty()
                && let Some(used) = &mut s.reputation_sources_used
            {
                std::rc::Rc::make_mut(used).insert(source.descriptor.id.clone());
            }
        }
    }
}

fn gray_multiplier(level: i64, creature: &BoundCreature) -> f64 {
    // Use the lower bound when a level range straddles the gray boundary.
    if creature.min_level <= crate::xp::Rules::gray_level(level) {
        0.2
    } else {
        1.0
    }
}
fn within(value: f64, range: StandingRange) -> bool {
    range.minimum.is_none_or(|min| value >= min) && range.maximum.is_none_or(|max| value < max)
}
fn quota(source: &BoundReputationSource) -> String {
    source
        .descriptor
        .reset
        .quota_key
        .clone()
        .unwrap_or_else(|| source.descriptor.id.clone())
}
fn position(l: &SourceLocation) -> Option<Pos> {
    Some(Pos {
        continent: i64::from(l.world_id?),
        zone: l.zone_id.unwrap_or(0),
        x: l.x?,
        y: l.y?,
        z: crate::world::height(l.z),
    })
}
fn register_source(program: &mut RepProgram, source: &BoundReputationSource) {
    if !program.sources.iter().any(|d| d.id == source.descriptor.id) {
        program.sources.push(source.descriptor.clone());
    }
}
fn record_phase(
    program: &mut RepProgram,
    kind: RepPhaseKind,
    mut actions: Vec<RepAction>,
    source: &BoundReputationSource,
) {
    register_source(program, source);
    if let Some(last) = program.phases.last_mut()
        && last.kind == kind
        && last.actions.len() == actions.len()
        && last.actions.iter().zip(&actions).all(|(a, b)| {
            a.source_id == b.source_id
                && a.kind == b.kind
                && a.units_per_cycle == b.units_per_cycle
                && a.acquisition == b.acquisition
        })
    {
        last.cycles += 1;
        for (a, b) in last.actions.iter_mut().zip(actions) {
            a.total_unit_limit = a.total_unit_limit.saturating_add(b.total_unit_limit);
        }
        return;
    }
    let id = format!("rep-phase-{}", program.phases.len() + 1);
    for (index, action) in actions.iter_mut().enumerate() {
        action.id = format!("{id}-action-{}", index + 1);
    }
    program.phases.push(RepPhase {
        id,
        kind,
        cycles: 1,
        actions,
    });
}
