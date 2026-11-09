//! Quantitative earning rules bound to the current edition's entities and material supply.

use super::sources::{LinkKind, ObjectiveKind, QuestRow, Role, SourceKind, Sources};
use super::{EntityKind, Profile};
use crate::params::Params;
use crate::reputation::{
    AcquisitionKind, AwardCap, FactionStandingRequirement, GroupPolicy, InputItem, ReputationEffects, ResetKind,
    RewardEncoding, RewardModifiers, RoundingPolicy, SignedAward, SourceAcquisition, SourceConfidence, SourceCosts,
    SourceDescriptor, SourceEligibility, SourceIdentity, SourceLocation, SourceProvenance, SourceReset, SpilloverGate,
    SpilloverPolicy, SpilloverRule, StandingRange,
};
use crate::world::World;
use crate::xp::Edition;
use anyhow::Result;
use rusqlite::Connection;
use serde_json::Value;
use std::collections::{HashMap, HashSet};
use std::sync::OnceLock;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum BoundSourceKind {
    Kill,
    Turnin,
}

/// Only verified Classic quest bounds belong here; other editions need their own evidence.
pub(crate) fn quest_standing_bounds(edition: Edition) -> StandingRange {
    if edition == Edition::Classic {
        StandingRange {
            minimum: classic()["rules"]["global_min"].as_f64(),
            maximum: classic()["rules"]["global_max"].as_f64(),
        }
    } else {
        StandingRange::default()
    }
}

#[derive(Debug, Clone)]
pub(crate) struct BoundCreature {
    pub npc_id: i64,
    pub min_level: i64,
    pub max_level: i64,
    pub rank: i64,
    pub respawn_seconds: Option<f64>,
    pub spawn_count: u32,
}

#[derive(Debug, Clone)]
pub(crate) struct BoundAcquisition {
    pub descriptor: SourceAcquisition,
    pub locations: Vec<SourceLocation>,
    pub creature: Option<BoundCreature>,
    /// Explicit limited stock: maximum count and restock seconds. None means no recorded limit.
    pub stock: Option<(u32, f64)>,
}

#[derive(Debug, Clone)]
pub(crate) struct BoundReputationSource {
    pub descriptor: SourceDescriptor,
    pub kind: BoundSourceKind,
    pub locations: Vec<SourceLocation>,
    pub giver_locations: Vec<SourceLocation>,
    pub creature: Option<BoundCreature>,
    pub acquisition: Vec<BoundAcquisition>,
    pub prerequisite_any: Vec<i64>,
    pub prerequisite_count: usize,
    /// A real delivery/kill binding and supported eligibility; holdings can supply missing materials.
    pub supported: bool,
    /// Only current normal outdoor creatures explicitly identified by the audited candidate list.
    pub automatic_farming: bool,
    pub warnings: Vec<String>,
    #[cfg_attr(
        not(test),
        expect(dead_code, reason = "retain source flags for binding audits and regression fixtures")
    )]
    pub quest_flags: i64,
    #[expect(dead_code, reason = "retain raw source metadata for binding audits")]
    pub quest_flags_ex: i64,
    #[expect(dead_code, reason = "retain raw source metadata for binding audits")]
    pub quest_flags_ex2: i64,
    #[expect(dead_code, reason = "retain selected catalog patch for binding audits")]
    pub selected_patch: Option<i64>,
}

#[derive(Debug, Clone, Default)]
pub(crate) struct BoundReputationCatalog {
    pub sources: Vec<BoundReputationSource>,
    pub warnings: Vec<String>,
    /// Exact base-route material inputs, consumed once per ordinary quest delivery.
    pub quest_inputs: HashMap<i64, Vec<InputItem>>,
}

impl BoundReputationCatalog {
    pub(super) fn load(
        conn: &Connection,
        world: &World,
        sources: &Sources,
        profile: &Profile,
        params: &Params,
    ) -> Result<Self> {
        let mut out = Self::default();
        for (&quest, objectives) in &sources.objectives {
            let mut inputs = Vec::new();
            for (kind, target, _, _, amount) in objectives {
                if *kind == ObjectiveKind::Item
                    && let (Some(item), Some(amount)) = (*target, *amount)
                {
                    add_inputs(&mut inputs, item, u32::try_from(amount).unwrap_or(0), false);
                }
            }
            if !inputs.is_empty() {
                out.quest_inputs.insert(quest, inputs);
            }
        }
        for row in &sources.rows {
            if let Some(starter) = row.source_item.filter(|s| *s > 0) {
                add_inputs(out.quest_inputs.entry(row.id).or_default(), starter, 1, true);
            }
        }
        let metadata = Metadata::read(conn)?;
        if params.edition == Edition::Classic || params.edition == Edition::Forever && params.reputation_estimates {
            if params.edition == Edition::Forever {
                out.warnings.push(estimate_warning().to_owned());
            }
            for record in array(&classic()["kill_records"]) {
                out.sources
                    .push(bind_kill(record, conn, world, sources, profile, params));
            }
        }
        let mut recorded = HashSet::new();
        if params.edition == Edition::Forever {
            for category in ["daily_records", "imported_repeatables", "one_time_records"] {
                for record in array(&forever()[category]) {
                    let id = integer(record, "quest");
                    recorded.insert(id);
                    let row = sources.rows.iter().find(|r| r.id == id);
                    out.sources
                        .push(bind_quest(record, row, &metadata, world, sources, profile, params));
                }
            }
            out.warnings.push("Durotar daily writ supply and first-reset availability require known inventory or a supported acquisition path; daily flags do not supply materials. No verified direct repeatable Horde (67) earning source is known.".into());
        }
        for row in &sources.rows {
            if row.special_flags.unwrap_or(0) & 1 == 0 || recorded.contains(&row.id) || row.reputations().is_empty() {
                continue;
            }
            let record = serde_json::json!({"source_id":format!("{}:repeatable:{}",params.edition.key(),row.id),"quest":row.id,"schedule":"repeatable"});
            out.sources.push(bind_quest(
                &record,
                Some(row),
                &metadata,
                world,
                sources,
                profile,
                params,
            ));
        }
        Ok(out)
    }
}

pub(super) fn corrected_rewards(quest_id: i64, edition: Edition, rewards: Vec<(i64, f64)>) -> Vec<(i64, f64)> {
    if edition == Edition::Forever
        && let Some(record) = array(&forever()["source_corrections"])
            .iter()
            .find(|r| integer(r, "quest") == quest_id)
    {
        return reward_vector(&record["reward"]);
    }
    rewards
}

fn classic() -> &'static Value {
    static CATALOG: OnceLock<Value> = OnceLock::new();
    CATALOG.get_or_init(|| {
        serde_json::from_str(include_str!("../../../../data/reputation/classic.json"))
            .expect("maintained Classic reputation catalog")
    })
}

fn forever() -> &'static Value {
    static CATALOG: OnceLock<Value> = OnceLock::new();
    CATALOG.get_or_init(|| {
        serde_json::from_str(include_str!("../../../../data/reputation/forever.json"))
            .expect("maintained Forever reputation catalog")
    })
}

fn array(value: &Value) -> &[Value] {
    value.as_array().map_or(&[], Vec::as_slice)
}
fn integer(value: &Value, key: &str) -> i64 {
    value[key].as_i64().unwrap_or(0)
}
fn text<'a>(value: &'a Value, key: &str) -> &'a str {
    value[key].as_str().unwrap_or("")
}
fn estimate_warning() -> &'static str {
    text(&forever()["classic_fallback"], "warning")
}
fn reward_vector(value: &Value) -> Vec<(i64, f64)> {
    array(value)
        .iter()
        .filter_map(|r| Some((r[0].as_i64()?, r[1].as_f64()?)))
        .collect()
}

fn locations(world: &World, sources: &Sources, npc: i64, params: &Params) -> Vec<SourceLocation> {
    sources
        .entities
        .points(world, EntityKind::Npc, npc)
        .into_iter()
        .filter(|(_, zone)| !params.excluded_zones.contains(zone))
        .map(|(pos, zone)| {
            let map = world.to_map(zone, &pos);
            SourceLocation {
                world_id: u32::try_from(pos.continent).ok(),
                map_id: map.map(|p| p.ui_map),
                zone_id: Some(zone),
                x: Some(pos.x),
                y: Some(pos.y),
                z: pos.z.map(f64::from),
                map_x: map.map(|p| p.x),
                map_y: map.map(|p| p.y),
            }
        })
        .collect()
}

fn creature(sources: &Sources, npc: i64, spawn_count: usize) -> Option<BoundCreature> {
    let (_, min_level, max_level, rank) = sources.entities.npc.get(&npc)?;
    Some(BoundCreature {
        npc_id: npc,
        min_level: *min_level,
        max_level: *max_level,
        rank: *rank,
        respawn_seconds: sources
            .entities
            .respawn
            .get(&(EntityKind::Npc, npc))
            .copied()
            .filter(|s| s.is_finite() && *s > 0.0),
        spawn_count: u32::try_from(spawn_count).unwrap_or(u32::MAX),
    })
}

fn normal_outdoor(conn: &Connection, world: &World, sources: &Sources, npc: i64, at: &[SourceLocation]) -> bool {
    if sources.inside.contains_key(&(EntityKind::Npc, npc)) || at.is_empty() {
        return false;
    }
    let eligible=conn.query_row("SELECT coalesce(rank,0)=0 AND coalesce(npc_flags,0)=0 AND coalesce(friendly_to,'') IN ('','AH') FROM m_npc WHERE id=?1",[npc],|r|r.get::<_,bool>(0)).unwrap_or(false);
    eligible && at.iter().any(|l| l.zone_id.is_some_and(|z| !world.is_capital(z)))
}

fn classic_provenance(patch: i64) -> Vec<SourceProvenance> {
    let p = &classic()["provenance"]["source"];
    vec![SourceProvenance{url:text(p,"asset_url").into(),revision:Some(text(p,"code_revision").into()),sha256:Some(text(p,"database_sha256").into()),license:Some(text(p,"license").into()),build:Some(format!("Classic 1.12; selected creature patch {patch}")),note:Some("Explicit baseline rank gates; ordinary spillover and team halves truncate per kill. Gray-level gains use the pinned baseline and are forecasts.".into())}]
}

fn bind_kill(
    record: &Value,
    conn: &Connection,
    world: &World,
    sources: &Sources,
    profile: &Profile,
    params: &Params,
) -> BoundReputationSource {
    let npc = integer(record, "creature_id");
    let at = locations(world, sources, npc, params);
    let creature = creature(sources, npc, at.len());
    let estimate = params.edition == Edition::Forever;
    let mut awards = Vec::new();
    let mut spillover = Vec::new();
    for slot in array(&record["awards"]) {
        let side = text(slot, "side");
        if !side.is_empty()
            && !side.eq_ignore_ascii_case(profile.faction.name())
            && !side.eq_ignore_ascii_case(profile.faction.letter())
        {
            continue;
        }
        if record["team_dependent"].as_bool() == Some(true)
            && integer(slot, "slot")
                != if profile.faction == crate::faction::Faction::Alliance {
                    1
                } else {
                    2
                }
        {
            continue;
        }
        let faction = integer(slot, "faction");
        awards.push(SignedAward {
            faction_id: faction,
            amount: slot["base_value"].as_f64().unwrap_or(0.0),
            cap: Some(AwardCap::PreEventRankInclusive {
                maximum_rank: integer(slot, "max_standing_rank_inclusive") as u8,
            }),
            rounding: RoundingPolicy::Truncate,
            ..SignedAward::default()
        });
        if let Some(template) = array(&classic()["spillover_templates"])
            .iter()
            .find(|t| integer(t, "faction") == faction)
        {
            for edge in array(&template["effects"]) {
                spillover.push(SpilloverRule {
                    primary_faction_id: faction,
                    recipient_faction_id: integer(edge, "faction"),
                    rate: edge["rate"].as_f64().unwrap_or(0.0),
                    cap: Some(AwardCap::PreEventRankInclusive {
                        maximum_rank: integer(edge, "max_standing_rank_inclusive") as u8,
                    }),
                    rounding: RoundingPolicy::Truncate,
                    ..SpilloverRule::default()
                });
            }
        }
        if slot["is_team_award"].as_bool() == Some(true)
            && let Some(parent) = array(&classic()["faction_metadata"])
                .iter()
                .find(|f| integer(f, "faction") == faction)
                .map(|f| integer(f, "parent"))
                .filter(|f| *f > 0)
        {
            spillover.push(SpilloverRule {
                primary_faction_id: faction,
                recipient_faction_id: parent,
                rate: 0.5,
                gate: SpilloverGate::Independent,
                rounding: RoundingPolicy::Truncate,
                ..SpilloverRule::default()
            });
        }
    }
    let automatic_farming =
        record["outdoor_farming_candidate"].as_bool() == Some(true) && normal_outdoor(conn, world, sources, npc, &at);
    let mut warnings = Vec::new();
    if estimate {
        warnings.push(estimate_warning().into());
    }
    warnings.push("Per-kill gains use explicit Classic gray-level and truncation assumptions; live standings determine completion.".into());
    if creature.is_none() || at.is_empty() {
        warnings.push("Current creature or spawn binding is missing; no automatic farming path is available.".into());
    }
    let group_policy = if params.group_size > 1 {
        GroupPolicy::Unsupported
    } else {
        GroupPolicy::Solo
    };
    if group_policy == GroupPolicy::Unsupported {
        warnings.push("Group kill reputation sharing and party-rate behavior are not verified.".into());
    }
    let patch = integer(record, "selected_patch");
    BoundReputationSource {
        descriptor: SourceDescriptor {
            id: text(record, "source_id").into(),
            name: sources.entities.name(EntityKind::Npc, npc),
            edition: params.edition.key().into(),
            identity: SourceIdentity {
                npc_id: Some(npc),
                npc_name: sources.entities.npc.get(&npc).map(|n| n.0.clone()),
                ..SourceIdentity::default()
            },
            repeatable: true,
            effects: ReputationEffects {
                encoding: RewardEncoding::PrimaryAwards,
                awards,
                spillover: if spillover.is_empty() {
                    SpilloverPolicy::None
                } else {
                    SpilloverPolicy::Explicit(spillover)
                },
                modifiers: RewardModifiers {
                    human_bonus: if params.edition == Edition::Classic && profile.race_id == 1 {
                        0.1
                    } else {
                        0.0
                    },
                    ..RewardModifiers::default()
                },
                standing_bounds: StandingRange {
                    minimum: classic()["rules"]["global_min"].as_f64(),
                    maximum: classic()["rules"]["global_max"].as_f64(),
                },
            },
            group_policy,
            confidence: if estimate {
                SourceConfidence::ClassicEstimate
            } else {
                SourceConfidence::Confirmed
            },
            provenance: classic_provenance(patch),
            location: at.first().cloned(),
            ..SourceDescriptor::default()
        },
        kind: BoundSourceKind::Kill,
        supported: creature.is_some() && !at.is_empty() && group_policy != GroupPolicy::Unsupported,
        locations: at,
        giver_locations: Vec::new(),
        creature,
        acquisition: Vec::new(),
        prerequisite_any: Vec::new(),
        prerequisite_count: 1,
        automatic_farming,
        warnings,
        quest_flags: 0,
        quest_flags_ex: 0,
        quest_flags_ex2: 0,
        selected_patch: Some(patch),
    }
}

/// Small optional columns only; large entity/drop maps stay shared in Sources/Entities.
#[derive(Default)]
struct QuestMetadata {
    maximum_level: Option<i64>,
    minimum_rep: Option<(i64, f64)>,
    maximum_rep: Option<(i64, f64)>,
}

#[derive(Default)]
struct Metadata {
    quests: HashMap<i64, QuestMetadata>,
    prices: HashMap<i64, i64>,
}

impl Metadata {
    fn read(conn: &Connection) -> Result<Self> {
        let mut out = Self::default();
        let columns: HashSet<String> = conn
            .prepare("SELECT name FROM pragma_table_info('m_quest')")?
            .query_map([], |r| r.get(0))?
            .collect::<rusqlite::Result<_>>()?;
        let col = |name: &str| {
            if columns.contains(name) {
                name.to_owned()
            } else {
                "NULL".to_owned()
            }
        };
        let catalog_ids = ["daily_records", "imported_repeatables", "one_time_records"]
            .into_iter()
            .flat_map(|key| array(&forever()[key]))
            .map(|record| integer(record, "quest").to_string())
            .collect::<Vec<_>>()
            .join(",");
        let sql = format!(
            "SELECT id,{},{},{},{},{} FROM m_quest WHERE coalesce(special_flags,0)&1!=0 OR id IN ({catalog_ids})",
            col("max_level"),
            col("required_min_rep_faction"),
            col("required_min_rep_value"),
            col("required_max_rep_faction"),
            col("required_max_rep_value")
        );
        for row in conn.prepare(&sql)?.query_map([], |r| {
            let min_f: Option<i64> = r.get(2)?;
            let min_v: Option<f64> = r.get(3)?;
            let max_f: Option<i64> = r.get(4)?;
            let max_v: Option<f64> = r.get(5)?;
            Ok((
                r.get(0)?,
                QuestMetadata {
                    maximum_level: r.get::<_, Option<i64>>(1)?.filter(|l| *l > 0),
                    minimum_rep: min_f.filter(|f| *f > 0).zip(min_v),
                    maximum_rep: max_f.filter(|f| *f > 0).zip(max_v),
                },
            ))
        })? {
            let (id, value) = row?;
            out.quests.insert(id, value);
        }
        let prices: bool = conn.query_row(
            "SELECT count(*)>0 FROM pragma_table_info('m_item_stats') WHERE name='buy_price'",
            [],
            |r| r.get(0),
        )?;
        if prices {
            for row in conn
                .prepare("SELECT id,buy_price FROM m_item_stats WHERE buy_price>0")?
                .query_map([], |r| Ok((r.get(0)?, r.get(1)?)))?
            {
                let (id, price) = row?;
                out.prices.insert(id, price);
            }
        }
        Ok(out)
    }
}

fn quest_provenance(record: &Value, edition: Edition) -> Vec<SourceProvenance> {
    if edition != Edition::Forever {
        return vec![SourceProvenance{url:"https://github.com/prijkes/factoruide/tree/main/data".into(),build:Some(edition.key().into()),note:Some("Resolved vector from the selected edition's merged database; field-level winning-source provenance and live repeat behavior are unavailable.".into()),..SourceProvenance::default()}];
    }
    let p = &forever()["provenance"];
    let build = integer(record, "primary_cache_build");
    if let Some(cache) = array(&p["primary_caches"])
        .iter()
        .find(|c| integer(c, "build") == build)
    {
        vec![SourceProvenance{url:text(cache,"url").into(),build:Some(build.to_string()),revision:Some(text(p,"revision").into()),sha256:Some(text(cache,"sha256").into()),license:Some("CC BY 4.0".into()),note:Some("Efficient Games primary cache; resolved vector from explicit hundredth overrides. Reset flag recorded; second-reset completion not observed.".into())}]
    } else {
        vec![SourceProvenance {
            url: text(p, "datasets_specification_url").into(),
            revision: Some(text(p, "revision").into()),
            note: Some(text(p, "imported_database_confidence").into()),
            ..SourceProvenance::default()
        }]
    }
}

fn add_inputs(items: &mut Vec<InputItem>, item: i64, count: u32, starter: bool) {
    if item <= 0 || count == 0 {
        return;
    }
    if let Some(existing) = items.iter_mut().find(|i| i.item_id == item) {
        existing.count = existing.count.max(count);
        existing.starter |= starter;
    } else {
        items.push(InputItem {
            item_id: item,
            count,
            starter,
        });
    }
}

fn bind_quest(
    record: &Value,
    row: Option<&QuestRow>,
    metadata: &Metadata,
    world: &World,
    sources: &Sources,
    profile: &Profile,
    params: &Params,
) -> BoundReputationSource {
    let id = integer(record, "quest");
    let daily = text(record, "schedule") == "daily" || row.is_some_and(|r| r.flags.unwrap_or(0) & 4096 != 0);
    let repeatable = text(record, "schedule") != "once";
    let mut warnings = Vec::new();
    let mut supported = row.is_some();
    let mut inputs = Vec::new();
    let relations = sources.relations.get(&id).map_or(&[][..], Vec::as_slice);
    let starter = record["starting_item_id"]
        .as_i64()
        .or_else(|| {
            relations
                .iter()
                .find(|(role, kind, _)| *role == Role::Start && *kind == EntityKind::Item)
                .map(|(_, _, item)| *item)
        })
        .filter(|i| *i > 0);
    for item in array(&record["required_items"]) {
        let item_id = integer(item, "item");
        add_inputs(
            &mut inputs,
            item_id,
            integer(item, "amount").max(0) as u32,
            Some(item_id) == starter,
        );
    }
    let mut eligibility = SourceEligibility {
        side: Some(profile.faction.name().to_lowercase()),
        minimum_level: record["min_level"].as_i64(),
        ..SourceEligibility::default()
    };
    let mut pre_any = Vec::new();
    let mut flags = integer(record, "flags");
    let mut costs = SourceCosts {
        interaction_seconds: params.object_time * 2.0,
        ..SourceCosts::default()
    };
    if let Some(row) = row {
        eligibility.race_mask = row.races.filter(|m| *m > 0).map(|m| m as u64);
        eligibility.class_mask = row.classes.filter(|m| *m > 0).map(|m| m as u64);
        eligibility.minimum_level = Some(row.min_level.unwrap_or(1).max(eligibility.minimum_level.unwrap_or(1)));
        if profile.members_of(row.races, row.classes) & 1 == 0 {
            supported = false;
            warnings.push("Quest race/class restrictions exclude this character.".into());
        }
        if row
            .sort
            .is_some_and(|sort| sort < 0 && params.excluded_sorts.contains(&-sort))
        {
            supported = false;
            warnings.push("Quest category is excluded by the current request.".into());
        }
        if row.skill.unwrap_or(0) > 0 || row.spell.unwrap_or(0) > 0 || row.time_limit.unwrap_or(0) > 0 {
            supported = false;
            warnings.push(
                "Profession, known-spell or timed-quest eligibility is not supported by this completion source.".into(),
            );
        }
        if row
            .requirements
            .as_deref()
            .is_some_and(|r| r.to_lowercase().contains("money:"))
        {
            supported = false;
            warnings.push("An additional money requirement lacks an exact supported charge.".into());
        }
        if !daily {
            flags = row.flags.unwrap_or(flags);
        }
        costs.money_reward_copper = row.reward_money.max(0);
        costs.money_cost_copper = row.reward_money.saturating_neg().max(0);
        costs.xp_reward = row.xp.max(0) as f64;
        for (kind, target, _, _, count) in sources.objectives.get(&id).into_iter().flatten() {
            if *kind == ObjectiveKind::Item
                && let (Some(item), Some(amount)) = (*target, *count)
            {
                add_inputs(
                    &mut inputs,
                    item,
                    u32::try_from(amount).unwrap_or(0),
                    Some(item) == starter,
                );
            } else if *kind != ObjectiveKind::Other {
                supported = false;
                warnings.push("A non-material or unquantified quest objective needs an unsupported activity.".into());
            } else {
                supported = false;
                warnings.push("An unmodeled quest objective prevents a supported repeated delivery.".into());
            }
        }
        // A named objective with no exact objective rows is not a free hand-in.
        if array(&record["required_items"]).is_empty() && row.numbered && !sources.objectives.contains_key(&id) {
            supported = false;
            warnings.push("Numbered objectives have no exact current target binding.".into());
        }
    } else {
        warnings.push("Quest is absent or unavailable in the current database.".into());
    }
    if let Some(starter) = starter {
        add_inputs(&mut inputs, starter, 1, true);
    }
    if let Some(given) = row
        .and_then(|r| r.source_item)
        .filter(|i| *i > 0 && Some(*i) != starter)
    {
        add_inputs(&mut inputs, given, 1, false);
        warnings.push("The current source item is conservatively reserved as an input; it does not prove item-start availability.".into());
    }
    if row
        .and_then(|r| r.source_items.as_deref())
        .and_then(|json| serde_json::from_str::<Vec<Value>>(json).ok())
        .is_some_and(|items| {
            items
                .iter()
                .filter_map(Value::as_i64)
                .any(|item| !inputs.iter().any(|i| i.item_id == item))
        })
    {
        supported = false;
        warnings.push("Additional required source items lack exact supported per-completion quantities.".into());
    }
    if repeatable && inputs.is_empty() {
        supported = false;
        warnings.push(
            "No exact per-completion inputs are known; an unmodeled donation cannot become a free repeated reward."
                .into(),
        );
    }
    let current = sources.links.get(&id).map_or(&[][..], Vec::as_slice);
    eligibility
        .prerequisite_quests
        .extend(current.iter().filter(|(k, _)| *k == LinkKind::PreAll).map(|(_, q)| *q));
    pre_any.extend(current.iter().filter(|(k, _)| *k == LinkKind::PreAny).map(|(_, q)| *q));
    eligibility.excluded_quests.extend(
        current
            .iter()
            .filter(|(k, _)| *k == LinkKind::Exclusive)
            .map(|(_, q)| *q),
    );
    for q in array(&record["prerequisites"]["all"]).iter().filter_map(Value::as_i64) {
        if !eligibility.prerequisite_quests.contains(&q) {
            eligibility.prerequisite_quests.push(q);
        }
    }
    if pre_any.is_empty() {
        pre_any.extend(array(&record["prerequisites"]["any"]).iter().filter_map(Value::as_i64));
    }
    if sources
        .links
        .values()
        .any(|links| links.iter().any(|(kind, q)| *kind == LinkKind::Child && *q == id))
    {
        supported = false;
        warnings.push("A held parent quest requirement is not supported for repeated delivery.".into());
    }
    if params.excluded_quests.contains(&id) || sources.pvp.contains(&id) && !params.pvp_quests {
        supported = false;
        warnings.push("Quest is excluded by the current request or PvP policy.".into());
    }
    if let Some(m) = metadata.quests.get(&id) {
        eligibility.maximum_level = m.maximum_level;
        if let Some((f, v)) = m.minimum_rep {
            eligibility.standing.push(FactionStandingRequirement {
                faction_id: f,
                standing: StandingRange {
                    minimum: Some(v),
                    maximum: None,
                },
            });
        }
        if let Some((f, v)) = m.maximum_rep {
            eligibility.standing.push(FactionStandingRequirement {
                faction_id: f,
                standing: StandingRange {
                    minimum: None,
                    maximum: Some(v),
                },
            });
        }
    }
    for (key, minimum) in [("min_rep_gate", true), ("max_rep_gate", false)] {
        if let (Some(f), Some(v)) = (record[key][0].as_i64(), record[key][1].as_f64()) {
            eligibility.standing.push(FactionStandingRequirement {
                faction_id: f,
                standing: StandingRange {
                    minimum: minimum.then_some(v),
                    maximum: (!minimum).then_some(v),
                },
            });
        }
    }
    if row.is_some_and(|r| r.rep.unwrap_or(0) > 0)
        && !eligibility
            .standing
            .iter()
            .any(|s| Some(s.faction_id) == row.and_then(|r| r.rep))
    {
        supported = false;
        warnings.push("A current standing requirement has no known threshold.".into());
    }
    let usable = |role| {
        relations
            .iter()
            .filter(move |(r, k, n)| {
                *r == role
                    && *k == EntityKind::Npc
                    && !sources.entities.hostile.contains(n)
                    && !sources.inside.contains_key(&(*k, *n))
            })
            .filter_map(|(_, _, n)| {
                let at = locations(world, sources, *n, params);
                (!at.is_empty()).then_some((*n, at))
            })
            .collect::<Vec<_>>()
    };
    let starts = usable(Role::Start);
    let ends = usable(Role::End);
    // The portable identity names one NPC. Keep all of that NPC's verified spawns,
    // so choosing another spawn never sends a target macro to a different NPC.
    let giver_locations = starts.first().map_or_else(Vec::new, |(_, at)| at.clone());
    let at = ends.first().map_or_else(Vec::new, |(_, at)| at.clone());
    if starts.len() > 1 || ends.len() > 1 {
        warnings.push("Multiple quest NPCs are known; completion uses the named NPC's recorded locations so target instructions remain exact. Other NPC alternatives are not searched.".into());
    }
    if starts.is_empty() && starter.is_none() {
        supported = false;
        warnings.push("No current quest giver or verified starting item is known.".into());
    }
    if ends.is_empty() {
        supported = false;
        warnings.push("No current reachable quest ender is known.".into());
    }
    let conditions = array(&record["acquisition_conditions"]);
    let acquisition = if conditions.is_empty() {
        acquire(&inputs, metadata, world, sources, params)
    } else {
        warnings.push(format!(
            "Material acquisition remains inventory-only: {}.",
            conditions
                .iter()
                .filter_map(Value::as_str)
                .collect::<Vec<_>>()
                .join(", ")
        ));
        Vec::new()
    };
    for input in &inputs {
        if !acquisition.iter().any(|a| a.descriptor.item_id == input.item_id) {
            warnings.push(format!(
                "Item {} has no supported exact drop or priced-vendor supply; only explicitly held stock can fund it.",
                input.item_id
            ));
        }
    }
    if repeatable && !daily {
        warnings.push("Repeatability is imported from special_flags & 1; live repeat behavior and post-cap rewards are unverified.".into());
    }
    if record.get("reputation_cap").is_none_or(Value::is_null) {
        warnings.push("Reputation ceiling is unknown; the resolved reward is a forecast for each supported completion, not proof of unlimited gains.".into());
    }
    if daily {
        warnings.push("Daily flag permits one completion per reset; first availability and future writ replenishment are unknown without recorded state.".into());
    }
    let current_rewards = row
        .map(|r| corrected_rewards(id, params.edition, r.reputations()))
        .unwrap_or_default();
    let rewards = if !current_rewards.is_empty() && text(record, "confidence") != "PrimaryCache" {
        current_rewards
    } else if record["reward"].is_array() {
        reward_vector(&record["reward"])
    } else {
        current_rewards
    };
    let descriptor = SourceDescriptor {
        id: text(record, "source_id").into(),
        name: row.map_or_else(|| format!("Quest {id}"), |r| r.name.clone()),
        edition: params.edition.key().into(),
        identity: SourceIdentity {
            quest_id: Some(id),
            giver_npc_id: starts.first().map(|(id, _)| *id),
            ender_npc_id: ends.first().map(|(id, _)| *id),
            giver_npc_name: starts
                .first()
                .map(|(id, _)| sources.entities.name(EntityKind::Npc, *id)),
            ender_npc_name: ends.first().map(|(id, _)| sources.entities.name(EntityKind::Npc, *id)),
            ..SourceIdentity::default()
        },
        repeatable,
        effects: ReputationEffects {
            encoding: RewardEncoding::ResolvedVector,
            awards: rewards
                .into_iter()
                .map(|(f, a)| SignedAward {
                    faction_id: f,
                    amount: a,
                    ..SignedAward::default()
                })
                .collect(),
            modifiers: RewardModifiers {
                human_bonus: if params.edition == Edition::Classic && profile.race_id == 1 {
                    0.1
                } else {
                    0.0
                },
                ..RewardModifiers::default()
            },
            standing_bounds: quest_standing_bounds(params.edition),
            ..ReputationEffects::default()
        },
        input_items: inputs,
        costs,
        eligibility,
        reset: if daily {
            SourceReset {
                kind: ResetKind::Daily,
                quota_key: Some(format!("quest:{id}")),
                units_per_reset: 1,
            }
        } else {
            SourceReset::default()
        },
        group_policy: GroupPolicy::IndividualCredit,
        confidence: if text(record, "confidence") == "PrimaryCache" {
            SourceConfidence::Confirmed
        } else {
            SourceConfidence::BundledResolved
        },
        provenance: quest_provenance(record, params.edition),
        location: at.first().cloned(),
        acquisition: acquisition.iter().map(|a| a.descriptor.clone()).collect(),
    };
    BoundReputationSource {
        descriptor,
        kind: BoundSourceKind::Turnin,
        locations: at,
        giver_locations,
        creature: None,
        acquisition,
        prerequisite_any: pre_any,
        prerequisite_count: profile
            .quest_overrides
            .iter()
            .find(|o| o.id == id)
            .and_then(|o| o.pre_count)
            .unwrap_or(1)
            .max(1),
        supported,
        automatic_farming: false,
        warnings,
        quest_flags: flags,
        quest_flags_ex: integer(record, "flags_ex"),
        quest_flags_ex2: integer(record, "flags_ex2"),
        selected_patch: None,
    }
}

fn acquire(
    inputs: &[InputItem],
    metadata: &Metadata,
    world: &World,
    sources: &Sources,
    params: &Params,
) -> Vec<BoundAcquisition> {
    let mut out = Vec::new();
    for input in inputs {
        for &(kind, npc) in sources.item_origins.get(&input.item_id).into_iter().flatten() {
            let at = locations(world, sources, npc, params);
            if at.is_empty() || sources.inside.contains_key(&(EntityKind::Npc, npc)) {
                continue;
            }
            let (acquisition_kind, price, chance, mob, stock) = match kind {
                SourceKind::Npc => {
                    let Some(mob) = creature(sources, npc, at.len()).filter(|m| m.rank == 0) else {
                        continue;
                    };
                    if sources.entities.friendly.contains(&npc) || sources.entities.own_side.contains(&npc) {
                        continue;
                    }
                    let Some(chance) = sources
                        .drop_chance
                        .get(&(input.item_id, npc))
                        .copied()
                        .filter(|c| c.is_finite() && *c > 0.0 && *c <= 1.0)
                    else {
                        continue;
                    };
                    (AcquisitionKind::ExactCreatureDrop, None, Some(chance), Some(mob), None)
                }
                SourceKind::Vendor => {
                    if sources.entities.hostile.contains(&npc) {
                        continue;
                    }
                    let Some(price) = metadata.prices.get(&input.item_id).copied().filter(|p| *p > 0) else {
                        continue;
                    };
                    (
                        AcquisitionKind::PricedVendor,
                        Some(price),
                        None,
                        None,
                        sources.entities.vendor_stock.get(&(npc, input.item_id)).copied(),
                    )
                }
                SourceKind::Object | SourceKind::Item => continue,
            };
            out.push(BoundAcquisition {
                descriptor: SourceAcquisition {
                    kind: acquisition_kind,
                    item_id: input.item_id,
                    units_per_acquisition: 1,
                    npc_id: Some(npc),
                    npc_name: Some(sources.entities.name(EntityKind::Npc, npc)),
                    location: at.first().cloned(),
                    unit_price_copper: price,
                    drop_chance: chance,
                },
                locations: at,
                creature: mob,
                stock,
            });
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::reputation::{ReputationEvent, Standings, apply_event};
    use crate::{faction::Faction, fixture::Fixture, xp::Edition};
    use std::collections::HashSet;

    fn database() -> Connection {
        Fixture::new(Edition::Forever)
            .toml(
                r#"
[[npc]]
id = 990001
friendly_to = "AH"
npc_flags = 2
at = [{zone=12,x=40.0,y=30.0}]
[[npc]]
id = 674
level = 45
respawn = 120.0
at = [{zone=12,x=60.0,y=40.0}]
[[npc]]
id = 990002
friendly_to = "AH"
npc_flags = 2
at = [{zone=12,x=80.0,y=30.0}]
[[quest]]
id = 94004
giver = 990001
source_item = 264011
[[quest]]
id = 7819
giver = 990001
min_level = 50
objectives = [{item=14047,amount=20}]
[[quest]]
id = 94214
giver = 990001
[[item]]
id = 14047
drops = [{npc=674,chance=0.4}]
[[item]]
id = 3391
drops = [{vendor=990001}]
[[vendor]]
npc = 990001
items = [{item=3391,count=3,restock=600.0}]
"#,
            )
            .open()
    }

    fn load(conn: &Connection, estimates: bool) -> BoundReputationCatalog {
        load_settings(conn, estimates, |_| {})
    }

    fn load_settings(conn: &Connection, estimates: bool, change: impl FnOnce(&mut Params)) -> BoundReputationCatalog {
        let world = World::flat(&[(12, "Field", 1, 0.0, 0.0)], &[], 0.0);
        let mut profile = super::super::testing::profile(Faction::Horde, 1, 60);
        profile.start_npc = 990001;
        let mut params = Params {
            edition: Edition::Forever,
            reputation_estimates: estimates,
            ..Params::default()
        };
        change(&mut params);
        let sources = Sources::read(
            conn,
            &world,
            &profile,
            &params,
            &HashSet::new(),
            &HashSet::new(),
            &HashSet::new(),
        )
        .unwrap();
        BoundReputationCatalog::load(conn, &world, &sources, &profile, &params).unwrap()
    }

    fn source<'a>(catalog: &'a BoundReputationCatalog, id: &str) -> &'a BoundReputationSource {
        catalog
            .sources
            .iter()
            .find(|s| s.descriptor.id == id)
            .unwrap_or_else(|| panic!("missing {id}"))
    }

    #[test]
    fn reputation_binding_estimate_policy_keeps_primary_dailies() {
        let conn = database();
        let disabled = load(&conn, false);
        assert_eq!(
            source(&disabled, "forever:daily:94004").descriptor.confidence,
            SourceConfidence::Confirmed
        );
        assert!(!disabled.sources.iter().any(|s| s.kind == BoundSourceKind::Kill));
        let enabled = load(&conn, true);
        assert_eq!(
            enabled
                .sources
                .iter()
                .filter(|s| s.kind == BoundSourceKind::Kill)
                .count(),
            352
        );
        let kill = source(&enabled, "classic:kill:674");
        assert_eq!(kill.descriptor.confidence, SourceConfidence::ClassicEstimate);
        assert!(kill.automatic_farming);
        assert!(kill.warnings.iter().any(|warning| warning.contains("Classic")));
        assert_eq!(kill.creature.as_ref().unwrap().respawn_seconds, Some(120.0));
        let loc = &kill.locations[0];
        assert_eq!(loc.map_x, Some(60.0));
        assert_ne!(loc.x, loc.map_x, "world yards are not UI map percentages");
    }

    #[test]
    fn reputation_binding_daily_reserves_material_and_starter_without_free_supply() {
        let catalog = load(&database(), true);
        let daily = source(&catalog, "forever:daily:94004");
        assert!(
            daily.supported,
            "real ender plus the primary-cache starter item supports held materials"
        );
        assert_eq!(daily.descriptor.reset.kind, ResetKind::Daily);
        assert_eq!(daily.descriptor.reset.units_per_reset, 1);
        assert!(
            daily
                .descriptor
                .input_items
                .iter()
                .any(|i| i.item_id == 264011 && i.count == 1 && i.starter)
        );
        assert!(
            daily
                .descriptor
                .input_items
                .iter()
                .any(|i| i.item_id == 3391 && i.count == 1 && !i.starter)
        );
        assert!(
            daily.acquisition.is_empty(),
            "unpriced vendor and absent writ source cannot fund a delivery"
        );
        assert!(
            !source(&catalog, "forever:daily:94212").supported,
            "audit ender flags cannot invent current NPCs"
        );
    }

    #[test]
    fn reputation_binding_exact_drops_and_priced_vendor_preserve_costs() {
        let conn = database();
        conn.execute_batch("ALTER TABLE m_item_stats ADD COLUMN buy_price INTEGER NOT NULL DEFAULT 0; INSERT INTO m_item_stats(id,buy_price) VALUES(3391,321);").unwrap();
        let catalog = load(&conn, true);
        let daily = source(&catalog, "forever:daily:94004");
        let buy = daily.acquisition.iter().find(|a| a.descriptor.item_id == 3391).unwrap();
        assert_eq!(buy.descriptor.unit_price_copper, Some(321));
        assert_eq!(buy.stock, Some((3, 600.0)));
        let repeat = source(&catalog, "forever:repeatable:7819");
        let farm = &repeat.acquisition[0];
        assert_eq!(farm.descriptor.kind, AcquisitionKind::ExactCreatureDrop);
        assert_eq!(farm.descriptor.npc_id, Some(674));
        assert_eq!(farm.descriptor.drop_chance, Some(0.4));
        assert_eq!(farm.creature.as_ref().unwrap().min_level, 45);
        assert_eq!(repeat.prerequisite_any, vec![7818]);
        assert!(matches!(repeat.descriptor.effects.spillover, SpilloverPolicy::None));
    }

    #[test]
    fn reputation_binding_signed_kill_slots_and_caps_survive() {
        let catalog = load(&database(), true);
        assert!(
            catalog
                .sources
                .iter()
                .any(|s| s.descriptor.effects.awards.iter().any(|a| a.amount < 0.0))
        );
        let kill = source(&catalog, "classic:kill:674");
        assert_eq!(
            kill.descriptor.effects.awards[0].cap,
            Some(AwardCap::PreEventRankInclusive { maximum_rank: 5 })
        );
        let SpilloverPolicy::Explicit(edges) = &kill.descriptor.effects.spillover else {
            panic!("missing explicit spillover")
        };
        assert_eq!(edges.len(), 3);
        assert!(edges.iter().all(|e| e.rounding == RoundingPolicy::Truncate));
        assert_eq!(edges.iter().map(|e| e.rate).collect::<Vec<_>>(), vec![0.5; 3]);
        assert!(!source(&catalog, "forever:quest:98248").descriptor.repeatable);
        let repair = source(&catalog, "forever:repeatable:9259");
        assert!(
            repair
                .descriptor
                .eligibility
                .standing
                .iter()
                .any(|r| r.faction_id == 21 && r.standing.maximum == Some(0.0))
        );
    }

    #[test]
    fn reputation_binding_maintained_correction_preserves_resolved_forever_vector() {
        assert_eq!(
            corrected_rewards(94214, Edition::Forever, vec![(2586, 75.0)]),
            vec![(2586, 75.0), (2587, 75.0)]
        );
        assert_eq!(
            corrected_rewards(94214, Edition::Classic, vec![(2586, 75.0)]),
            vec![(2586, 75.0)]
        );
    }

    #[test]
    fn reputation_binding_prefers_current_resolved_imported_vectors() {
        let conn = database();
        conn.execute(
            "UPDATE m_quest SET reputation_reward='[[68,50],[67,12.5],[76,-5]]' WHERE id=7819",
            [],
        )
        .unwrap();
        let catalog = load(&conn, true);
        let source = source(&catalog, "forever:repeatable:7819");
        assert_eq!(
            source
                .descriptor
                .effects
                .awards
                .iter()
                .map(|a| (a.faction_id, a.amount))
                .collect::<Vec<_>>(),
            vec![(68, 50.0), (67, 12.5), (76, -5.0)]
        );
        assert_eq!(source.descriptor.effects.spillover, SpilloverPolicy::None);
    }

    #[test]
    fn reputation_binding_classic_quest_vectors_respect_verified_global_bounds() {
        let conn = database();
        conn.execute(
            "UPDATE m_quest SET special_flags=1,reputation_reward='[[68,50],[67,-50]]' WHERE id=7819",
            [],
        )
        .unwrap();
        let catalog = load_settings(&conn, true, |params| params.edition = Edition::Classic);
        let source = source(&catalog, "classic:repeatable:7819");
        let mut standings = Standings::from([(68, 42990.0), (67, -41990.0)]);
        apply_event(
            &mut standings,
            &ReputationEvent {
                source_id: source.descriptor.id.clone(),
                edition: "classic".into(),
                confidence: source.descriptor.confidence,
                effects: source.descriptor.effects.clone(),
            },
        )
        .unwrap();
        assert_eq!(standings[&68], 42999.0);
        assert_eq!(standings[&67], -42000.0);
        assert_eq!(quest_standing_bounds(Edition::Forever), StandingRange::default());
    }

    #[test]
    fn reputation_binding_named_npc_positions_match_the_exported_identity() {
        let conn = database();
        conn.execute_batch("INSERT INTO m_quest_relation(quest_id,role,entity_type,entity_id) VALUES(7819,'start','npc',990002),(7819,'end','npc',990002);").unwrap();
        let catalog = load(&conn, true);
        let source = source(&catalog, "forever:repeatable:7819");
        assert_eq!(source.descriptor.identity.giver_npc_id, Some(990001));
        assert_eq!(source.descriptor.identity.ender_npc_id, Some(990001));
        assert_eq!(
            source.locations.len(),
            1,
            "delivery positions must belong to the named delivery NPC"
        );
        assert_eq!(
            source.giver_locations.len(),
            1,
            "accept positions must belong to the named giver"
        );
    }

    #[test]
    fn reputation_binding_other_repeatables_obey_current_daily_flags_and_standing() {
        let conn = database();
        conn.execute_batch("INSERT INTO m_quest(id,name,status,special_flags,quest_flags,reputation_reward,required_min_rep_faction,required_min_rep_value,required_max_rep_faction,required_max_rep_value) VALUES(990101,'Other daily','available',1,4096,'[[68,10]]',68,3000,68,9000); INSERT INTO m_quest_relation(quest_id,role,entity_type,entity_id) VALUES(990101,'start','npc',990001),(990101,'end','npc',990001); INSERT INTO m_quest_objective(quest_id,type,target_id,amount) VALUES(990101,'item',14047,2);").unwrap();
        let catalog = load(&conn, true);
        let other = source(&catalog, "forever:repeatable:990101");
        assert!(other.supported);
        assert_eq!(other.descriptor.reset.kind, ResetKind::Daily);
        assert!(
            other
                .descriptor
                .eligibility
                .standing
                .iter()
                .any(|r| r.faction_id == 68 && r.standing.minimum == Some(3000.0))
        );
        assert!(
            other
                .descriptor
                .eligibility
                .standing
                .iter()
                .any(|r| r.faction_id == 68 && r.standing.maximum == Some(9000.0))
        );
    }

    #[test]
    fn reputation_binding_inputless_import_does_not_fabricate_a_free_repeat() {
        let conn = database();
        conn.execute_batch("INSERT INTO m_quest(id,name,status,special_flags,reputation_reward) VALUES(990102,'Unknown donation','available',1,'[[68,10]]'); INSERT INTO m_quest_relation(quest_id,role,entity_type,entity_id) VALUES(990102,'start','npc',990001),(990102,'end','npc',990001);").unwrap();
        let catalog = load(&conn, true);
        assert!(!source(&catalog, "forever:repeatable:990102").supported);
    }

    #[test]
    fn reputation_binding_old_optional_columns_are_not_required() {
        let conn = database();
        conn.execute_batch("ALTER TABLE m_quest DROP COLUMN max_level; ALTER TABLE m_quest DROP COLUMN required_min_rep_value; ALTER TABLE m_quest DROP COLUMN required_max_rep_faction; ALTER TABLE m_quest DROP COLUMN required_max_rep_value;").unwrap();
        let catalog = load(&conn, true);
        assert!(source(&catalog, "forever:daily:94004").supported);
        assert!(source(&catalog, "forever:daily:94004").acquisition.is_empty());
        conn.execute("UPDATE m_quest SET required_min_rep_faction=68 WHERE id=94004", [])
            .unwrap();
        let gated = load(&conn, true);
        assert!(
            !source(&gated, "forever:daily:94004").supported,
            "an unknown threshold is not permission to bypass a standing requirement"
        );
    }

    #[test]
    fn reputation_binding_current_race_and_missing_giver_remain_restrictions() {
        let conn = database();
        conn.execute("UPDATE m_quest SET races_mask=1 WHERE id=7819", [])
            .unwrap();
        let race = load(&conn, true);
        assert!(!source(&race, "forever:repeatable:7819").supported);
        assert_eq!(
            source(&race, "forever:repeatable:7819")
                .descriptor
                .eligibility
                .race_mask,
            Some(1)
        );
        conn.execute_batch("UPDATE m_quest SET races_mask=NULL WHERE id=7819; DELETE FROM m_quest_relation WHERE quest_id=7819 AND role='start';").unwrap();
        let missing = load(&conn, true);
        assert!(!source(&missing, "forever:repeatable:7819").supported);
        conn.execute("UPDATE m_quest SET source_item_id=14047 WHERE id=7819", [])
            .unwrap();
        let merely_given = load(&conn, true);
        assert!(
            !source(&merely_given, "forever:repeatable:7819").supported,
            "a source item is not necessarily the item that starts a quest"
        );
    }

    #[test]
    fn reputation_binding_classic_global_bounds_apply_to_primary_and_spillover() {
        let catalog = load(&database(), true);
        let kill = source(&catalog, "classic:kill:674");
        let event = ReputationEvent {
            source_id: kill.descriptor.id.clone(),
            edition: "forever".into(),
            confidence: kill.descriptor.confidence,
            effects: kill.descriptor.effects.clone(),
        };
        let mut standings = Standings::from([(21, 0.0), (369, 42998.0)]);
        apply_event(&mut standings, &event).unwrap();
        assert_eq!(
            standings[&369], 42999.0,
            "full spillover is clamped only to the baseline global bound"
        );
        let loss = source(&catalog, "classic:kill:9460");
        let event = ReputationEvent {
            source_id: loss.descriptor.id.clone(),
            edition: "forever".into(),
            confidence: loss.descriptor.confidence,
            effects: loss.descriptor.effects.clone(),
        };
        let mut standings = Standings::from([(369, -41999.0)]);
        apply_event(&mut standings, &event).unwrap();
        assert_eq!(standings[&369], -42000.0);
    }

    #[test]
    fn reputation_binding_base_quest_input_ledger_deduplicates_starters() {
        let conn = database();
        conn.execute("UPDATE m_quest SET source_item_id=14047 WHERE id=7819", [])
            .unwrap();
        let catalog = load(&conn, true);
        let inputs = catalog
            .quest_inputs
            .get(&7819)
            .expect("ordinary quest materials must be reserved");
        assert_eq!(
            inputs,
            &vec![InputItem {
                item_id: 14047,
                count: 20,
                starter: true
            }]
        );
    }

    #[test]
    fn reputation_binding_excluded_locations_do_not_supply_or_deliver_materials() {
        let catalog = load_settings(&database(), true, |p| p.excluded_zones = vec![12]);
        let daily = source(&catalog, "forever:daily:94004");
        assert!(!daily.supported);
        assert_eq!(daily.locations.len(), 0);
        assert!(source(&catalog, "forever:repeatable:7819").acquisition.is_empty());
        assert!(!source(&catalog, "classic:kill:674").automatic_farming);
    }

    #[test]
    fn reputation_binding_excluded_sorts_and_unhandled_source_items_are_not_free() {
        let conn = database();
        conn.execute("UPDATE m_quest SET zone_or_sort=-364 WHERE id=7819", [])
            .unwrap();
        let excluded = load(&conn, true);
        assert!(!source(&excluded, "forever:repeatable:7819").supported);
        conn.execute(
            "UPDATE m_quest SET zone_or_sort=NULL,extra='{\"required_source_items\":[990999]}' WHERE id=7819",
            [],
        )
        .unwrap();
        let unhandled = load(&conn, true);
        assert!(!source(&unhandled, "forever:repeatable:7819").supported);
    }

    #[test]
    fn reputation_binding_catalog_import_standing_gate_survives_missing_repeat_flag() {
        let conn = database();
        conn.execute(
            "UPDATE m_quest SET special_flags=0,required_max_rep_faction=68,required_max_rep_value=9000 WHERE id=7819",
            [],
        )
        .unwrap();
        let catalog = load(&conn, true);
        assert!(
            source(&catalog, "forever:repeatable:7819")
                .descriptor
                .eligibility
                .standing
                .iter()
                .any(|r| r.faction_id == 68 && r.standing.maximum == Some(9000.0))
        );
    }

    #[test]
    fn reputation_binding_all_primary_dailies_reserve_writ_and_one_reset_slot() {
        let catalog = load(&database(), true);
        let dailies = catalog
            .sources
            .iter()
            .filter(|s| s.descriptor.id.starts_with("forever:daily:"))
            .collect::<Vec<_>>();
        assert_eq!(dailies.len(), 150);
        for daily in dailies {
            assert_eq!(daily.quest_flags & 4096, 4096);
            assert_eq!(daily.descriptor.reset.kind, ResetKind::Daily);
            assert_eq!(daily.descriptor.reset.units_per_reset, 1);
            assert_eq!(daily.descriptor.input_items.len(), 2);
            assert_eq!(daily.descriptor.input_items.iter().filter(|i| i.starter).count(), 1);
            assert_eq!(daily.descriptor.effects.encoding, RewardEncoding::ResolvedVector);
            assert_eq!(daily.descriptor.effects.spillover, SpilloverPolicy::None);
        }
    }

    #[test]
    fn reputation_binding_team_half_is_separate_from_primary_rank_gate() {
        let catalog = load(&database(), true);
        let kill = source(&catalog, "classic:kill:5615");
        let mut standings = Standings::from([(369, 3000.0), (169, 0.0)]);
        apply_event(
            &mut standings,
            &ReputationEvent {
                source_id: kill.descriptor.id.clone(),
                edition: "forever".into(),
                confidence: kill.descriptor.confidence,
                effects: kill.descriptor.effects.clone(),
            },
        )
        .unwrap();
        assert_eq!(standings[&369], 3000.0);
        assert_eq!(standings[&169], 2.0);
    }
}
