use prism_ir::{AnchorRef, EventActor, EventExecutionContext, LineageId, NodeId};
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

pub type ContractScope = ConceptScope;
pub type ContractProvenance = ConceptProvenance;
pub type ContractPublication = ConceptPublication;
pub type ContractPublicationStatus = ConceptPublicationStatus;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ContractKind {
    Interface,
    Behavioral,
    DataShape,
    DependencyBoundary,
    Lifecycle,
    Protocol,
    Operational,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "snake_case")]
pub enum ContractStatus {
    #[default]
    Candidate,
    Active,
    Deprecated,
    Retired,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "snake_case")]
pub enum ContractStability {
    Experimental,
    #[default]
    Internal,
    Public,
    Deprecated,
    Migrating,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ContractGuaranteeStrength {
    Hard,
    Soft,
    Conditional,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ContractHealthStatus {
    Healthy,
    Watch,
    Degraded,
    Stale,
    Superseded,
    Retired,
}

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ContractHealthSignals {
    pub guarantee_count: usize,
    pub validation_count: usize,
    pub consumer_count: usize,
    pub validation_coverage_ratio: f32,
    pub guarantee_evidence_ratio: f32,
    pub stale_validation_links: bool,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ContractHealth {
    pub handle: String,
    pub status: ContractHealthStatus,
    pub score: f32,
    pub reasons: Vec<String>,
    pub signals: ContractHealthSignals,
    #[serde(default)]
    pub superseded_by: Vec<String>,
}

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ContractTarget {
    #[serde(default)]
    pub anchors: Vec<AnchorRef>,
    #[serde(default)]
    pub concept_handles: Vec<String>,
}

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ContractGuarantee {
    pub id: String,
    pub statement: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub scope: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub strength: Option<ContractGuaranteeStrength>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub evidence_refs: Vec<String>,
}

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ContractValidation {
    pub id: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub summary: Option<String>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub anchors: Vec<AnchorRef>,
}

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ContractCompatibility {
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub compatible: Vec<String>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub additive: Vec<String>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub risky: Vec<String>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub breaking: Vec<String>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub migrating: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ContractPacket {
    pub handle: String,
    pub name: String,
    pub summary: String,
    pub aliases: Vec<String>,
    pub kind: ContractKind,
    pub subject: ContractTarget,
    pub guarantees: Vec<ContractGuarantee>,
    #[serde(default)]
    pub assumptions: Vec<String>,
    #[serde(default)]
    pub consumers: Vec<ContractTarget>,
    #[serde(default)]
    pub validations: Vec<ContractValidation>,
    #[serde(default)]
    pub stability: ContractStability,
    #[serde(default)]
    pub compatibility: ContractCompatibility,
    #[serde(default)]
    pub evidence: Vec<String>,
    #[serde(default)]
    pub status: ContractStatus,
    #[serde(default)]
    pub scope: ContractScope,
    #[serde(default)]
    pub provenance: ContractProvenance,
    #[serde(default)]
    pub publication: Option<ContractPublication>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ContractResolution {
    pub packet: ContractPacket,
    pub score: i32,
    #[serde(default)]
    pub reasons: Vec<String>,
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
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub actor: Option<EventActor>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub execution_context: Option<EventExecutionContext>,
    pub action: ConceptEventAction,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub patch: Option<ConceptEventPatch>,
    pub concept: ConceptPacket,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ContractEventAction {
    Promote,
    Update,
    Retire,
    AttachEvidence,
    AttachValidation,
    RecordConsumer,
    SetStatus,
}

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ContractEventPatch {
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub set_fields: Vec<String>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub cleared_fields: Vec<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub kind: Option<ContractKind>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub name: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub summary: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub aliases: Option<Vec<String>>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub subject: Option<ContractTarget>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub guarantees: Option<Vec<ContractGuarantee>>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub assumptions: Option<Vec<String>>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub consumers: Option<Vec<ContractTarget>>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub validations: Option<Vec<ContractValidation>>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub stability: Option<ContractStability>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub compatibility: Option<ContractCompatibility>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub evidence: Option<Vec<String>>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub status: Option<ContractStatus>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub scope: Option<ContractScope>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub supersedes: Option<Vec<String>>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub retirement_reason: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ContractEvent {
    pub id: String,
    pub recorded_at: u64,
    pub task_id: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub actor: Option<EventActor>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub execution_context: Option<EventExecutionContext>,
    pub action: ContractEventAction,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub patch: Option<ContractEventPatch>,
    pub contract: ContractPacket,
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
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub actor: Option<EventActor>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub execution_context: Option<EventExecutionContext>,
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

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
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
