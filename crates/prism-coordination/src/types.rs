use prism_ir::{
    AgentId, AnchorRef, ArtifactId, ArtifactStatus, Capability, ClaimId, ClaimMode, ClaimStatus,
    ConflictSeverity, CoordinationEventKind, CoordinationTaskId, CoordinationTaskStatus, EventId,
    EventMeta, PlanId, PlanStatus, ReviewId, ReviewVerdict, SessionId, Timestamp,
    WorkspaceRevision,
};
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use serde_json::Value;

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct CoordinationPolicy {
    pub default_claim_mode: ClaimMode,
    pub max_parallel_editors_per_anchor: u16,
    pub require_review_for_completion: bool,
    #[serde(default)]
    pub require_validation_for_completion: bool,
    pub stale_after_graph_change: bool,
    #[serde(default)]
    pub review_required_above_risk_score: Option<f32>,
}

impl Default for CoordinationPolicy {
    fn default() -> Self {
        Self {
            default_claim_mode: ClaimMode::Advisory,
            max_parallel_editors_per_anchor: 2,
            require_review_for_completion: false,
            require_validation_for_completion: false,
            stale_after_graph_change: true,
            review_required_above_risk_score: None,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct AcceptanceCriterion {
    pub label: String,
    pub anchors: Vec<AnchorRef>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Plan {
    pub id: PlanId,
    pub goal: String,
    pub status: PlanStatus,
    pub policy: CoordinationPolicy,
    pub root_tasks: Vec<CoordinationTaskId>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct CoordinationTask {
    pub id: CoordinationTaskId,
    pub plan: PlanId,
    pub title: String,
    pub status: CoordinationTaskStatus,
    pub assignee: Option<AgentId>,
    pub session: Option<SessionId>,
    pub anchors: Vec<AnchorRef>,
    pub depends_on: Vec<CoordinationTaskId>,
    pub acceptance: Vec<AcceptanceCriterion>,
    pub base_revision: WorkspaceRevision,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct WorkClaim {
    pub id: ClaimId,
    pub holder: SessionId,
    pub agent: Option<AgentId>,
    pub task: Option<CoordinationTaskId>,
    pub anchors: Vec<AnchorRef>,
    pub capability: Capability,
    pub mode: ClaimMode,
    pub since: Timestamp,
    pub expires_at: Timestamp,
    pub status: ClaimStatus,
    pub base_revision: WorkspaceRevision,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct CoordinationConflict {
    pub severity: ConflictSeverity,
    pub anchors: Vec<AnchorRef>,
    pub summary: String,
    pub blocking_claims: Vec<ClaimId>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Artifact {
    pub id: ArtifactId,
    pub task: CoordinationTaskId,
    pub anchors: Vec<AnchorRef>,
    pub base_revision: WorkspaceRevision,
    pub diff_ref: Option<String>,
    pub status: ArtifactStatus,
    pub evidence: Vec<EventId>,
    pub reviews: Vec<ReviewId>,
    #[serde(default)]
    pub required_validations: Vec<String>,
    #[serde(default)]
    pub validated_checks: Vec<String>,
    #[serde(default)]
    pub risk_score: Option<f32>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ArtifactReview {
    pub id: ReviewId,
    pub artifact: ArtifactId,
    pub verdict: ReviewVerdict,
    pub summary: String,
    pub meta: EventMeta,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct CoordinationEvent {
    pub meta: EventMeta,
    pub kind: CoordinationEventKind,
    pub summary: String,
    pub plan: Option<PlanId>,
    pub task: Option<CoordinationTaskId>,
    pub claim: Option<ClaimId>,
    pub artifact: Option<ArtifactId>,
    pub review: Option<ReviewId>,
    pub metadata: Value,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct TaskBlocker {
    pub kind: BlockerKind,
    pub summary: String,
    pub related_task_id: Option<CoordinationTaskId>,
    pub related_artifact_id: Option<ArtifactId>,
    #[serde(default)]
    pub risk_score: Option<f32>,
    #[serde(default)]
    pub validation_checks: Vec<String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
pub enum BlockerKind {
    Dependency,
    ClaimConflict,
    ReviewRequired,
    RiskReviewRequired,
    ValidationRequired,
    StaleRevision,
    ArtifactStale,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct CoordinationSnapshot {
    pub plans: Vec<Plan>,
    pub tasks: Vec<CoordinationTask>,
    pub claims: Vec<WorkClaim>,
    pub artifacts: Vec<Artifact>,
    pub reviews: Vec<ArtifactReview>,
    pub events: Vec<CoordinationEvent>,
    pub next_plan: u64,
    pub next_task: u64,
    pub next_claim: u64,
    pub next_artifact: u64,
    pub next_review: u64,
}

#[derive(Debug, Clone)]
pub struct PlanCreateInput {
    pub goal: String,
    pub policy: Option<CoordinationPolicy>,
}

#[derive(Debug, Clone)]
pub struct TaskCreateInput {
    pub plan_id: PlanId,
    pub title: String,
    pub status: Option<CoordinationTaskStatus>,
    pub assignee: Option<AgentId>,
    pub session: Option<SessionId>,
    pub anchors: Vec<AnchorRef>,
    pub depends_on: Vec<CoordinationTaskId>,
    pub acceptance: Vec<AcceptanceCriterion>,
    pub base_revision: WorkspaceRevision,
}

#[derive(Debug, Clone)]
pub struct TaskUpdateInput {
    pub task_id: CoordinationTaskId,
    pub status: Option<CoordinationTaskStatus>,
    pub assignee: Option<Option<AgentId>>,
    pub session: Option<Option<SessionId>>,
    pub title: Option<String>,
    pub anchors: Option<Vec<AnchorRef>>,
    pub base_revision: Option<WorkspaceRevision>,
    pub completion_context: Option<TaskCompletionContext>,
}

#[derive(Debug, Clone, Default)]
pub struct TaskCompletionContext {
    pub risk_score: Option<f32>,
    pub required_validations: Vec<String>,
}

#[derive(Debug, Clone)]
pub struct HandoffInput {
    pub task_id: CoordinationTaskId,
    pub to_agent: Option<AgentId>,
    pub summary: String,
}

#[derive(Debug, Clone)]
pub struct ClaimAcquireInput {
    pub task_id: Option<CoordinationTaskId>,
    pub anchors: Vec<AnchorRef>,
    pub capability: Capability,
    pub mode: Option<ClaimMode>,
    pub ttl_seconds: Option<u64>,
    pub base_revision: WorkspaceRevision,
    pub agent: Option<AgentId>,
}

#[derive(Debug, Clone)]
pub struct ArtifactProposeInput {
    pub task_id: CoordinationTaskId,
    pub anchors: Vec<AnchorRef>,
    pub diff_ref: Option<String>,
    pub evidence: Vec<EventId>,
    pub base_revision: WorkspaceRevision,
    pub required_validations: Vec<String>,
    pub validated_checks: Vec<String>,
    pub risk_score: Option<f32>,
}

#[derive(Debug, Clone)]
pub struct ArtifactSupersedeInput {
    pub artifact_id: ArtifactId,
}

#[derive(Debug, Clone)]
pub struct ArtifactReviewInput {
    pub artifact_id: ArtifactId,
    pub verdict: ReviewVerdict,
    pub summary: String,
    pub required_validations: Vec<String>,
    pub validated_checks: Vec<String>,
    pub risk_score: Option<f32>,
}
