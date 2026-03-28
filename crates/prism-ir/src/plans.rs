use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

use crate::{
    AgentId, AnchorRef, ArtifactId, PlanEdgeId, PlanId, PlanNodeId, SessionId, WorkspaceRevision,
};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
pub enum PlanScope {
    Local,
    Session,
    Repo,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
pub enum PlanKind {
    TaskExecution,
    Investigation,
    Refactor,
    Migration,
    Release,
    IncidentResponse,
    Maintenance,
    Custom,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
pub enum PlanNodeKind {
    Investigate,
    Decide,
    Edit,
    Validate,
    Review,
    Handoff,
    Merge,
    Release,
    Note,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
pub enum PlanNodeStatus {
    Proposed,
    Ready,
    InProgress,
    Blocked,
    Waiting,
    InReview,
    Validating,
    Completed,
    Abandoned,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
pub enum PlanEdgeKind {
    DependsOn,
    Blocks,
    Informs,
    Validates,
    HandoffTo,
    ChildOf,
    RelatedTo,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
pub struct ValidationRef {
    pub id: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema, Default)]
pub struct PlanBinding {
    pub anchors: Vec<AnchorRef>,
    pub concept_handles: Vec<String>,
    pub artifact_refs: Vec<String>,
    pub memory_refs: Vec<String>,
    pub outcome_refs: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema, Default)]
pub struct HydratedPlanBindingOverlay {
    pub handles: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
pub struct PlanAcceptanceCriterion {
    pub label: String,
    pub anchors: Vec<AnchorRef>,
    pub required_checks: Vec<ValidationRef>,
    pub evidence_policy: AcceptanceEvidencePolicy,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
pub enum AcceptanceEvidencePolicy {
    Any,
    All,
    ReviewOnly,
    ValidationOnly,
    ReviewAndValidation,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
pub enum PlanNodeBlockerKind {
    Dependency,
    BlockingNode,
    ChildIncomplete,
    ValidationGate,
    Handoff,
    ClaimConflict,
    ReviewRequired,
    RiskReviewRequired,
    ValidationRequired,
    StaleRevision,
    ArtifactStale,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct PlanNodeBlocker {
    pub kind: PlanNodeBlockerKind,
    pub summary: String,
    pub related_node_id: Option<PlanNodeId>,
    pub related_artifact_id: Option<ArtifactId>,
    pub risk_score: Option<f32>,
    pub validation_checks: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct PlanNode {
    pub id: PlanNodeId,
    pub plan_id: PlanId,
    pub kind: PlanNodeKind,
    pub title: String,
    pub summary: Option<String>,
    pub status: PlanNodeStatus,
    pub bindings: PlanBinding,
    pub acceptance: Vec<PlanAcceptanceCriterion>,
    #[serde(default)]
    pub validation_refs: Vec<ValidationRef>,
    pub is_abstract: bool,
    pub assignee: Option<AgentId>,
    pub base_revision: WorkspaceRevision,
    pub priority: Option<u8>,
    pub tags: Vec<String>,
    pub metadata: serde_json::Value,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct PlanEdge {
    pub id: PlanEdgeId,
    pub plan_id: PlanId,
    pub from: PlanNodeId,
    pub to: PlanNodeId,
    pub kind: PlanEdgeKind,
    pub summary: Option<String>,
    pub metadata: serde_json::Value,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct PlanExecutionOverlay {
    pub node_id: PlanNodeId,
    pub pending_handoff_to: Option<AgentId>,
    pub session: Option<SessionId>,
    #[serde(default)]
    pub effective_assignee: Option<AgentId>,
    #[serde(default)]
    pub awaiting_handoff_from: Option<PlanNodeId>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct PlanGraph {
    pub id: PlanId,
    pub scope: PlanScope,
    pub kind: PlanKind,
    pub title: String,
    pub goal: String,
    pub status: crate::PlanStatus,
    pub revision: u64,
    pub root_nodes: Vec<PlanNodeId>,
    pub tags: Vec<String>,
    pub created_from: Option<String>,
    pub metadata: serde_json::Value,
    pub nodes: Vec<PlanNode>,
    pub edges: Vec<PlanEdge>,
}
