use prism_ir::{LineageId, NodeId};
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ConceptDecodeLens {
    Open,
    Workset,
    Validation,
    Timeline,
    Memory,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "snake_case")]
pub enum ConceptScope {
    Local,
    #[default]
    Session,
    Repo,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ConceptPacket {
    pub handle: String,
    pub canonical_name: String,
    pub summary: String,
    pub aliases: Vec<String>,
    pub confidence: f32,
    pub core_members: Vec<NodeId>,
    #[serde(default)]
    pub core_member_lineages: Vec<Option<LineageId>>,
    pub supporting_members: Vec<NodeId>,
    #[serde(default)]
    pub supporting_member_lineages: Vec<Option<LineageId>>,
    pub likely_tests: Vec<NodeId>,
    #[serde(default)]
    pub likely_test_lineages: Vec<Option<LineageId>>,
    pub evidence: Vec<String>,
    pub risk_hint: Option<String>,
    pub decode_lenses: Vec<ConceptDecodeLens>,
    #[serde(default)]
    pub scope: ConceptScope,
    #[serde(default)]
    pub provenance: ConceptProvenance,
    #[serde(default)]
    pub publication: Option<ConceptPublication>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ConceptResolution {
    pub packet: ConceptPacket,
    pub score: i32,
    #[serde(default)]
    pub reasons: Vec<String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ConceptHealthStatus {
    Healthy,
    Drifted,
    NeedsRepair,
    SplitCandidate,
    SupersededCandidate,
}

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ConceptHealthSignals {
    pub live_core_member_ratio: f32,
    pub lineage_coverage_ratio: f32,
    pub rebind_success_ratio: f32,
    pub member_churn_ratio: f32,
    pub validation_coverage_ratio: f32,
    pub ambiguity_ratio: f32,
    pub stale_validation_links: bool,
    pub stale_risk_hint: bool,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ConceptHealth {
    pub handle: String,
    pub status: ConceptHealthStatus,
    pub score: f32,
    pub reasons: Vec<String>,
    pub signals: ConceptHealthSignals,
    #[serde(default)]
    pub superseded_by: Vec<String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "snake_case")]
pub enum ConceptPublicationStatus {
    #[default]
    Active,
    Retired,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ConceptProvenance {
    pub origin: String,
    pub kind: String,
    pub task_id: Option<String>,
}

impl Default for ConceptProvenance {
    fn default() -> Self {
        Self {
            origin: "unknown".to_string(),
            kind: "unknown".to_string(),
            task_id: None,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ConceptPublication {
    pub published_at: u64,
    pub last_reviewed_at: Option<u64>,
    #[serde(default)]
    pub status: ConceptPublicationStatus,
    #[serde(default)]
    pub supersedes: Vec<String>,
    pub retired_at: Option<u64>,
    pub retirement_reason: Option<String>,
}

impl Default for ConceptPublication {
    fn default() -> Self {
        Self {
            published_at: 0,
            last_reviewed_at: None,
            status: ConceptPublicationStatus::Active,
            supersedes: Vec::new(),
            retired_at: None,
            retirement_reason: None,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ConceptEventAction {
    Promote,
    Update,
    Retire,
}

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ConceptEventPatch {
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub set_fields: Vec<String>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub cleared_fields: Vec<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub canonical_name: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub summary: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub aliases: Option<Vec<String>>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub core_members: Option<Vec<NodeId>>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub core_member_lineages: Option<Vec<Option<LineageId>>>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub supporting_members: Option<Vec<NodeId>>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub supporting_member_lineages: Option<Vec<Option<LineageId>>>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub likely_tests: Option<Vec<NodeId>>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub likely_test_lineages: Option<Vec<Option<LineageId>>>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub evidence: Option<Vec<String>>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub risk_hint: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub confidence: Option<f32>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub decode_lenses: Option<Vec<ConceptDecodeLens>>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub scope: Option<ConceptScope>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub supersedes: Option<Vec<String>>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub retirement_reason: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ConceptEvent {
    pub id: String,
    pub recorded_at: u64,
    pub task_id: Option<String>,
    pub action: ConceptEventAction,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub patch: Option<ConceptEventPatch>,
    pub concept: ConceptPacket,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ConceptRelationKind {
    DependsOn,
    Specializes,
    PartOf,
    ValidatedBy,
    OftenUsedWith,
    Supersedes,
    ConfusedWith,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ConceptRelation {
    pub source_handle: String,
    pub target_handle: String,
    pub kind: ConceptRelationKind,
    pub confidence: f32,
    #[serde(default)]
    pub evidence: Vec<String>,
    #[serde(default)]
    pub scope: ConceptScope,
    #[serde(default)]
    pub provenance: ConceptProvenance,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ConceptRelationEventAction {
    Upsert,
    Retire,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ConceptRelationEvent {
    pub id: String,
    pub recorded_at: u64,
    pub task_id: Option<String>,
    pub action: ConceptRelationEventAction,
    pub relation: ConceptRelation,
}

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct ValidationCheck {
    pub label: String,
    pub score: f32,
    pub last_seen: u64,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct CoChangeRecord {
    pub lineage: LineageId,
    pub count: u32,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct CoChangeDelta {
    pub source_lineage: LineageId,
    pub target_lineage: LineageId,
    pub count_delta: u32,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ValidationDelta {
    pub lineage: LineageId,
    pub label: String,
    pub score_delta: f32,
    pub last_seen: u64,
}

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct ProjectionSnapshot {
    pub co_change_by_lineage: Vec<(LineageId, Vec<CoChangeRecord>)>,
    pub validation_by_lineage: Vec<(LineageId, Vec<ValidationCheck>)>,
    #[serde(default)]
    pub curated_concepts: Vec<ConceptPacket>,
    #[serde(default)]
    pub concept_relations: Vec<ConceptRelation>,
}

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct IntentSpecProjection {
    pub spec: NodeId,
    pub implementations: Vec<NodeId>,
    pub validations: Vec<NodeId>,
    pub related: Vec<NodeId>,
}

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct IntentDriftRecord {
    pub spec: NodeId,
    pub implementations: Vec<NodeId>,
    pub validations: Vec<NodeId>,
    pub related: Vec<NodeId>,
    pub reasons: Vec<String>,
}
