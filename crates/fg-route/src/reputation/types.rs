//! Portable contracts for reputation rules, forecasts, and executable programs.

use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;

pub type Standings = BTreeMap<i64, f64>;

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SourceConfidence {
    Confirmed,
    BundledResolved,
    ClassicEstimate,
    #[default]
    Unknown,
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct SourceProvenance {
    pub url: String,
    pub build: Option<String>,
    pub revision: Option<String>,
    pub sha256: Option<String>,
    pub license: Option<String>,
    pub note: Option<String>,
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum RewardEncoding {
    /// Already expanded quest vectors: never apply spillover again.
    #[default]
    ResolvedVector,
    PrimaryAwards,
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum RoundingPolicy {
    #[default]
    ExactFractional,
    /// Integer conversion toward zero, performed on each event before batching.
    Truncate,
    /// Expected value of baseline stochastic rounding; not an in-game RNG proof.
    MeanForecast,
    /// Floor the signed amount, avoiding a favorable stochastic gain assumption.
    ConservativeForecast,
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Serialize, Deserialize)]
pub struct StandingRange {
    pub minimum: Option<f64>,
    pub maximum: Option<f64>,
}

#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum AwardCap {
    /// Check the standing before this event, then award the complete amount.
    PreEventInclusive { maximum: f64 },
    /// Classic rank gate, checked before an event, including the entire named rank.
    PreEventRankInclusive { maximum_rank: u8 },
    /// A real numeric ceiling, unlike a pre-event eligibility gate.
    HardCeiling { maximum: f64 },
}

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct SignedAward {
    pub faction_id: i64,
    pub amount: f64,
    pub standing: StandingRange,
    pub cap: Option<AwardCap>,
    pub rounding: RoundingPolicy,
    /// Source-slot no-spillover flag, independent of the event's template.
    pub suppress_spillover: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct RewardModifiers {
    /// Additional positive-gain fraction: human Diplomacy is 0.1.
    pub human_bonus: f64,
    pub gain_multiplier: f64,
    /// Explicit low-level assumption. Never applied to resolved vectors or losses.
    pub gray_multiplier: f64,
}

impl Default for RewardModifiers {
    fn default() -> Self {
        Self {
            human_bonus: 0.0,
            gain_multiplier: 1.0,
            gray_multiplier: 1.0,
        }
    }
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SpilloverGate {
    #[default]
    PrimaryAward,
    /// Separate team award: its own recipient gate can pass after the primary cap.
    Independent,
}

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct SpilloverRule {
    pub primary_faction_id: i64,
    pub recipient_faction_id: i64,
    pub rate: f64,
    pub standing: StandingRange,
    pub cap: Option<AwardCap>,
    pub gate: SpilloverGate,
    pub rounding: RoundingPolicy,
}

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(tag = "kind", content = "rules", rename_all = "snake_case")]
pub enum SpilloverPolicy {
    #[default]
    None,
    /// Only these direct edges run. Recipients never generate another award.
    Explicit(Vec<SpilloverRule>),
}

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct ReputationEffects {
    pub encoding: RewardEncoding,
    pub awards: Vec<SignedAward>,
    pub modifiers: RewardModifiers,
    pub spillover: SpilloverPolicy,
    /// Explicit source-edition bounds, independent of per-award rank gates.
    pub standing_bounds: StandingRange,
}

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct ReputationEvent {
    pub source_id: String,
    pub edition: String,
    pub confidence: SourceConfidence,
    pub effects: ReputationEffects,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct TransitionResult {
    pub source_id: String,
    pub confidence: SourceConfidence,
    /// Signed changes, aggregated by faction. Zero changes are omitted.
    pub deltas: Standings,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct BatchTransitionResult {
    pub source_id: String,
    pub confidence: SourceConfidence,
    pub units: u32,
    pub changed_units: u32,
    pub deltas: Standings,
}

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct SourceLocation {
    /// Planner continent/world identifier, when bound to a loaded world.
    pub world_id: Option<u32>,
    /// UI map identifier, distinct from a continent/world identifier.
    pub map_id: Option<i64>,
    /// AreaTable zone identifier.
    pub zone_id: Option<i64>,
    /// World-yard coordinates; never UI map percentages.
    pub x: Option<f64>,
    pub y: Option<f64>,
    pub z: Option<f64>,
    /// UI map percentages, when independently available.
    pub map_x: Option<f64>,
    pub map_y: Option<f64>,
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct SourceIdentity {
    pub quest_id: Option<i64>,
    pub npc_id: Option<i64>,
    pub giver_npc_id: Option<i64>,
    pub ender_npc_id: Option<i64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub npc_name: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub giver_npc_name: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub ender_npc_name: Option<String>,
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct InputItem {
    pub item_id: i64,
    pub count: u32,
    /// Starter items are consumed on every repetition too.
    pub starter: bool,
}

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct SourceCosts {
    pub travel_seconds: f64,
    pub combat_seconds: f64,
    pub respawn_seconds: f64,
    pub interaction_seconds: f64,
    pub money_cost_copper: i64,
    pub money_reward_copper: i64,
    pub xp_reward: f64,
}

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct FactionStandingRequirement {
    pub faction_id: i64,
    pub standing: StandingRange,
}

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct SourceEligibility {
    pub side: Option<String>,
    pub race_mask: Option<u64>,
    pub class_mask: Option<u64>,
    pub minimum_level: Option<i64>,
    pub maximum_level: Option<i64>,
    pub prerequisite_quests: Vec<i64>,
    pub excluded_quests: Vec<i64>,
    pub standing: Vec<FactionStandingRequirement>,
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ResetKind {
    #[default]
    None,
    Daily,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SourceReset {
    pub kind: ResetKind,
    pub quota_key: Option<String>,
    pub units_per_reset: u32,
}

impl Default for SourceReset {
    fn default() -> Self {
        Self {
            kind: ResetKind::None,
            quota_key: None,
            units_per_reset: 1,
        }
    }
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum GroupPolicy {
    #[default]
    Solo,
    SharedCredit,
    IndividualCredit,
    Unsupported,
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum AcquisitionKind {
    KnownInventory,
    ExactCreatureDrop,
    PricedVendor,
    #[default]
    Unsupported,
}

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct SourceAcquisition {
    pub kind: AcquisitionKind,
    pub item_id: i64,
    pub units_per_acquisition: u32,
    pub npc_id: Option<i64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub npc_name: Option<String>,
    pub location: Option<SourceLocation>,
    pub unit_price_copper: Option<i64>,
    pub drop_chance: Option<f64>,
}

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct SourceDescriptor {
    pub id: String,
    pub name: String,
    pub edition: String,
    pub identity: SourceIdentity,
    pub repeatable: bool,
    pub effects: ReputationEffects,
    pub input_items: Vec<InputItem>,
    pub costs: SourceCosts,
    pub eligibility: SourceEligibility,
    pub reset: SourceReset,
    pub group_policy: GroupPolicy,
    pub confidence: SourceConfidence,
    pub provenance: Vec<SourceProvenance>,
    pub location: Option<SourceLocation>,
    pub acquisition: Vec<SourceAcquisition>,
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum RepActionKind {
    #[default]
    Turnin,
    Kill,
    Acquire,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum CompletionTarget {
    Reputation { faction_id: i64, standing: f64 },
    Inventory { item_id: i64, count: u32 },
}

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct RepAction {
    pub id: String,
    pub source_id: String,
    pub kind: RepActionKind,
    pub units_per_cycle: u32,
    pub total_unit_limit: u32,
    pub location: Option<SourceLocation>,
    pub acquisition: Option<SourceAcquisition>,
    pub completion_targets: Vec<CompletionTarget>,
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum RepPhaseKind {
    #[default]
    Batch,
    DailyCircuit,
}

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct RepPhase {
    pub id: String,
    pub kind: RepPhaseKind,
    pub cycles: u32,
    /// All actions execute in order on each cycle before repeating the circuit.
    pub actions: Vec<RepAction>,
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum RepResultStatus {
    Satisfied,
    #[default]
    BestFound,
}

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct RepProgramResult {
    #[serde(deserialize_with = "deserialize_standings")]
    pub start: Standings,
    #[serde(deserialize_with = "deserialize_standings")]
    pub reached: Standings,
    #[serde(deserialize_with = "deserialize_standings")]
    pub targets: Standings,
    #[serde(deserialize_with = "deserialize_standings")]
    pub deficits: Standings,
    pub status: RepResultStatus,
    pub active_play_seconds: f64,
    pub daily_opportunities: u32,
    pub reset_transitions: u32,
    /// Unknown without a recorded reset clock and first-availability snapshot.
    pub calendar_wait_seconds: Option<f64>,
    pub warnings: Vec<String>,
}

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct RepProgram {
    pub version: u32,
    pub requested_edition: String,
    pub effective_edition: String,
    pub sources: Vec<SourceDescriptor>,
    pub phases: Vec<RepPhase>,
    pub result: RepProgramResult,
}

/// Internally tagged route steps deserialize through serde's content buffer, which does
/// not apply serde_json's numeric map-key adapter. Accept the serialized string IDs there.
fn deserialize_standings<'de, D: serde::Deserializer<'de>>(deserializer: D) -> Result<Standings, D::Error> {
    #[derive(Deserialize, PartialEq, Eq, PartialOrd, Ord)]
    #[serde(untagged)]
    enum Key {
        Number(i64),
        Text(String),
    }
    let values = BTreeMap::<Key, f64>::deserialize(deserializer)?;
    values
        .into_iter()
        .map(|(key, value)| {
            let id = match key {
                Key::Number(id) => id,
                Key::Text(id) => id.parse().map_err(serde::de::Error::custom)?,
            };
            Ok((id, value))
        })
        .collect()
}
