use prism_coordination::BlockerKind;
use prism_ir::{
    AnchorRef, ArtifactStatus, Capability, ClaimMode, ClaimStatus, ConflictSeverity,
    CoordinationTaskStatus, EdgeKind, EdgeOrigin, Language, NodeKind, PlanStatus, Span,
};
use prism_memory::OutcomeEvent;
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use serde_json::Value;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct NodeIdView {
    pub crate_name: String,
    pub path: String,
    pub kind: NodeKind,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct SymbolView {
    pub id: NodeIdView,
    pub name: String,
    pub kind: NodeKind,
    pub signature: String,
    pub file_path: Option<String>,
    pub span: Span,
    pub language: Language,
    pub lineage_id: Option<String>,
}

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct RelationsView {
    pub contains: Vec<SymbolView>,
    pub callers: Vec<SymbolView>,
    pub callees: Vec<SymbolView>,
    pub references: Vec<SymbolView>,
    pub imports: Vec<SymbolView>,
    pub implements: Vec<SymbolView>,
    pub specifies: Vec<SymbolView>,
    pub specified_by: Vec<SymbolView>,
    pub validates: Vec<SymbolView>,
    pub validated_by: Vec<SymbolView>,
    pub related: Vec<SymbolView>,
    pub related_by: Vec<SymbolView>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct LineageView {
    pub lineage_id: String,
    pub current: SymbolView,
    pub status: LineageStatus,
    pub history: Vec<LineageEventView>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "lowercase")]
pub enum LineageStatus {
    Active,
    Dead,
    Ambiguous,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct LineageEventView {
    pub event_id: String,
    pub ts: u64,
    pub kind: String,
    pub confidence: f32,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct EdgeView {
    pub kind: EdgeKind,
    pub source: NodeIdView,
    pub target: NodeIdView,
    pub origin: EdgeOrigin,
    pub confidence: f32,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct SubgraphView {
    pub nodes: Vec<SymbolView>,
    pub edges: Vec<EdgeView>,
    pub truncated: bool,
    pub max_depth_reached: Option<usize>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct ChangeImpactView {
    pub direct_nodes: Vec<NodeIdView>,
    pub lineages: Vec<String>,
    pub likely_validations: Vec<String>,
    pub validation_checks: Vec<ValidationCheckView>,
    pub co_change_neighbors: Vec<CoChangeView>,
    pub risk_events: Vec<OutcomeEvent>,
    pub promoted_summaries: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct ValidationCheckView {
    pub label: String,
    pub score: f32,
    pub last_seen: u64,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct CoChangeView {
    pub lineage: String,
    pub count: u32,
    pub nodes: Vec<NodeIdView>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct ValidationRecipeView {
    pub target: NodeIdView,
    pub checks: Vec<String>,
    pub scored_checks: Vec<ValidationCheckView>,
    pub related_nodes: Vec<NodeIdView>,
    pub co_change_neighbors: Vec<CoChangeView>,
    pub recent_failures: Vec<OutcomeEvent>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct TaskValidationRecipeView {
    pub task_id: String,
    pub checks: Vec<String>,
    pub scored_checks: Vec<ValidationCheckView>,
    pub related_nodes: Vec<NodeIdView>,
    pub co_change_neighbors: Vec<CoChangeView>,
    pub recent_failures: Vec<OutcomeEvent>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct TaskRiskView {
    pub task_id: String,
    pub risk_score: f32,
    pub review_required: bool,
    pub stale_task: bool,
    pub has_approved_artifact: bool,
    pub likely_validations: Vec<String>,
    pub missing_validations: Vec<String>,
    pub validation_checks: Vec<ValidationCheckView>,
    pub co_change_neighbors: Vec<CoChangeView>,
    pub risk_events: Vec<OutcomeEvent>,
    pub promoted_summaries: Vec<String>,
    pub approved_artifact_ids: Vec<String>,
    pub stale_artifact_ids: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ArtifactRiskView {
    pub artifact_id: String,
    pub task_id: String,
    pub risk_score: f32,
    pub review_required: bool,
    pub stale: bool,
    pub required_validations: Vec<String>,
    pub validated_checks: Vec<String>,
    pub missing_validations: Vec<String>,
    pub co_change_neighbors: Vec<CoChangeView>,
    pub risk_events: Vec<OutcomeEvent>,
    pub promoted_summaries: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct DriftCandidateView {
    pub spec: NodeIdView,
    pub implementations: Vec<NodeIdView>,
    pub validations: Vec<NodeIdView>,
    pub related: Vec<NodeIdView>,
    pub reasons: Vec<String>,
    pub recent_failures: Vec<OutcomeEvent>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct TaskIntentView {
    pub task_id: String,
    pub specs: Vec<NodeIdView>,
    pub implementations: Vec<NodeIdView>,
    pub validations: Vec<NodeIdView>,
    pub related: Vec<NodeIdView>,
    pub drift_candidates: Vec<DriftCandidateView>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct WorkspaceRevisionView {
    pub graph_version: u64,
    pub git_commit: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct PlanView {
    pub id: String,
    pub goal: String,
    pub status: PlanStatus,
    pub root_task_ids: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct CoordinationTaskView {
    pub id: String,
    pub plan_id: String,
    pub title: String,
    pub status: CoordinationTaskStatus,
    pub assignee: Option<String>,
    pub anchors: Vec<AnchorRef>,
    pub depends_on: Vec<String>,
    pub base_revision: WorkspaceRevisionView,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct ClaimView {
    pub id: String,
    pub holder: String,
    pub task_id: Option<String>,
    pub capability: Capability,
    pub mode: ClaimMode,
    pub status: ClaimStatus,
    pub anchors: Vec<AnchorRef>,
    pub expires_at: u64,
    pub base_revision: WorkspaceRevisionView,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct ConflictView {
    pub severity: ConflictSeverity,
    pub summary: String,
    pub anchors: Vec<AnchorRef>,
    pub blocking_claim_ids: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct BlockerView {
    pub kind: BlockerKind,
    pub summary: String,
    pub related_task_id: Option<String>,
    pub related_artifact_id: Option<String>,
    pub risk_score: Option<f32>,
    pub validation_checks: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct ArtifactView {
    pub id: String,
    pub task_id: String,
    pub status: ArtifactStatus,
    pub anchors: Vec<AnchorRef>,
    pub base_revision: WorkspaceRevisionView,
    pub diff_ref: Option<String>,
    pub required_validations: Vec<String>,
    pub validated_checks: Vec<String>,
    pub risk_score: Option<f32>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct MemoryEntryView {
    pub id: String,
    pub anchors: Vec<AnchorRef>,
    pub kind: String,
    pub content: String,
    pub metadata: Value,
    pub created_at: u64,
    pub source: String,
    pub trust: f32,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct ScoredMemoryView {
    pub id: String,
    pub entry: MemoryEntryView,
    pub score: f32,
    pub source_module: String,
    pub explanation: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct CuratorProposalView {
    pub index: usize,
    pub kind: String,
    pub disposition: String,
    pub payload: Value,
    pub decided_at: Option<u64>,
    pub task_id: Option<String>,
    pub note: Option<String>,
    pub output: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct CuratorJobView {
    pub id: String,
    pub trigger: String,
    pub status: String,
    pub task_id: Option<String>,
    pub focus: Vec<AnchorRef>,
    pub created_at: u64,
    pub started_at: Option<u64>,
    pub finished_at: Option<u64>,
    pub proposals: Vec<CuratorProposalView>,
    pub diagnostics: Vec<QueryDiagnostic>,
    pub error: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct QueryEnvelope {
    pub result: Value,
    pub diagnostics: Vec<QueryDiagnostic>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct QueryDiagnostic {
    pub code: String,
    pub message: String,
    pub data: Option<Value>,
}
