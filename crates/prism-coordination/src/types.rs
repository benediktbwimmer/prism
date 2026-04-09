use prism_ir::{
    AgentId, AnchorRef, ArtifactId, ArtifactStatus, BlockerCause, Capability, ClaimId, ClaimMode,
    ClaimStatus, ConflictOverlapKind, ConflictSeverity, CoordinationEventKind, CoordinationTaskId,
    CoordinationTaskStatus, EventExecutionId, EventExecutionStatus, EventId, EventMeta,
    EventTriggerKind, LeaseRenewalMode, NodeRef, PlanBinding, PlanId, PlanKind, PlanNodeKind,
    PlanScope, PlanStatus, PrincipalActor, ReviewId, ReviewVerdict, SessionId, Timestamp,
    ValidationRef, WorkspaceRevision,
};
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use serde_json::Value;

use crate::git_execution::{GitExecutionPolicy, TaskGitExecution};

fn default_plan_scope() -> PlanScope {
    PlanScope::Repo
}

fn default_plan_kind() -> PlanKind {
    PlanKind::TaskExecution
}

fn default_plan_node_kind() -> PlanNodeKind {
    PlanNodeKind::Edit
}

fn default_runtime_discovery_mode() -> RuntimeDiscoveryMode {
    RuntimeDiscoveryMode::None
}

fn default_lease_stale_after_seconds() -> u64 {
    30 * 60
}

fn default_lease_expires_after_seconds() -> u64 {
    2 * 60 * 60
}

fn default_lease_renewal_mode() -> LeaseRenewalMode {
    LeaseRenewalMode::Strict
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct LeaseHolder {
    #[serde(default)]
    pub principal: Option<PrincipalActor>,
    #[serde(default)]
    pub session_id: Option<SessionId>,
    #[serde(default)]
    pub worktree_id: Option<String>,
    #[serde(default)]
    pub agent_id: Option<AgentId>,
}

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
    #[serde(default = "default_lease_stale_after_seconds")]
    pub lease_stale_after_seconds: u64,
    #[serde(default = "default_lease_expires_after_seconds")]
    pub lease_expires_after_seconds: u64,
    #[serde(default = "default_lease_renewal_mode")]
    pub lease_renewal_mode: LeaseRenewalMode,
    #[serde(default)]
    pub git_execution: GitExecutionPolicy,
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct PlanScheduling {
    #[serde(default)]
    pub importance: u8,
    #[serde(default)]
    pub urgency: u8,
    #[serde(default)]
    pub manual_boost: i16,
    #[serde(default)]
    pub due_at: Option<Timestamp>,
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
            lease_stale_after_seconds: default_lease_stale_after_seconds(),
            lease_expires_after_seconds: default_lease_expires_after_seconds(),
            lease_renewal_mode: default_lease_renewal_mode(),
            git_execution: GitExecutionPolicy::default(),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct AcceptanceCriterion {
    pub label: String,
    pub anchors: Vec<AnchorRef>,
}

fn default_requirement_min_count() -> u16 {
    1
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum ArtifactRequirementKind {
    CodeChange,
    TestEvidence,
    ExternalEvidence,
    Attestation,
    Note,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum ArtifactEvidenceType {
    GitCommit,
    GitRange,
    GitRef,
    ExternalRun,
    Attestation,
    Note,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum ReviewerClass {
    Agent,
    Human,
    Service,
    System,
    Ci,
    External,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct ArtifactRequirement {
    pub client_artifact_requirement_id: String,
    pub kind: ArtifactRequirementKind,
    #[serde(default = "default_requirement_min_count")]
    pub min_count: u16,
    #[serde(default)]
    pub evidence_types: Vec<ArtifactEvidenceType>,
    #[serde(default)]
    pub stale_after_graph_change: bool,
    #[serde(default)]
    pub required_validations: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct ReviewRequirement {
    pub client_review_requirement_id: String,
    pub artifact_requirement_ref: String,
    #[serde(default)]
    pub allowed_reviewer_classes: Vec<ReviewerClass>,
    #[serde(default = "default_requirement_min_count")]
    pub min_review_count: u16,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct CoordinationSpecRef {
    pub spec_id: String,
    pub source_path: String,
    #[serde(default)]
    pub source_revision: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct CoordinationTaskSpecRef {
    pub spec_id: String,
    pub source_path: String,
    #[serde(default)]
    pub source_revision: Option<String>,
    pub sync_kind: String,
    #[serde(default)]
    pub covered_checklist_items: Vec<String>,
    #[serde(default)]
    pub covered_sections: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Plan {
    pub id: PlanId,
    pub goal: String,
    #[serde(default)]
    pub title: String,
    pub status: PlanStatus,
    pub policy: CoordinationPolicy,
    #[serde(default = "default_plan_scope")]
    pub scope: PlanScope,
    #[serde(default = "default_plan_kind")]
    pub kind: PlanKind,
    #[serde(default)]
    pub revision: u64,
    #[serde(default)]
    pub scheduling: PlanScheduling,
    #[serde(default)]
    pub tags: Vec<String>,
    #[serde(default)]
    pub created_from: Option<String>,
    #[serde(default)]
    pub spec_refs: Vec<CoordinationSpecRef>,
    #[serde(default)]
    pub metadata: Value,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct CoordinationTask {
    pub id: CoordinationTaskId,
    pub plan: PlanId,
    #[serde(default = "default_plan_node_kind")]
    pub kind: PlanNodeKind,
    pub title: String,
    #[serde(default)]
    pub summary: Option<String>,
    pub status: CoordinationTaskStatus,
    #[serde(default)]
    pub published_task_status: Option<CoordinationTaskStatus>,
    pub assignee: Option<AgentId>,
    #[serde(default)]
    pub pending_handoff_to: Option<AgentId>,
    pub session: Option<SessionId>,
    #[serde(default)]
    pub lease_holder: Option<LeaseHolder>,
    #[serde(default)]
    pub lease_started_at: Option<Timestamp>,
    #[serde(default)]
    pub lease_refreshed_at: Option<Timestamp>,
    #[serde(default)]
    pub lease_stale_at: Option<Timestamp>,
    #[serde(default)]
    pub lease_expires_at: Option<Timestamp>,
    #[serde(default)]
    pub worktree_id: Option<String>,
    #[serde(default)]
    pub branch_ref: Option<String>,
    pub anchors: Vec<AnchorRef>,
    #[serde(default)]
    pub bindings: PlanBinding,
    pub depends_on: Vec<CoordinationTaskId>,
    #[serde(default)]
    pub coordination_depends_on: Vec<CoordinationTaskId>,
    #[serde(default)]
    pub integrated_depends_on: Vec<CoordinationTaskId>,
    pub acceptance: Vec<AcceptanceCriterion>,
    #[serde(default)]
    pub validation_refs: Vec<ValidationRef>,
    #[serde(default)]
    pub is_abstract: bool,
    pub base_revision: WorkspaceRevision,
    #[serde(default)]
    pub priority: Option<u8>,
    #[serde(default)]
    pub tags: Vec<String>,
    #[serde(default)]
    pub spec_refs: Vec<CoordinationTaskSpecRef>,
    #[serde(default)]
    pub artifact_requirements: Vec<ArtifactRequirement>,
    #[serde(default)]
    pub review_requirements: Vec<ReviewRequirement>,
    #[serde(default)]
    pub metadata: Value,
    #[serde(default)]
    pub git_execution: TaskGitExecution,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct WorkClaim {
    pub id: ClaimId,
    pub holder: SessionId,
    pub agent: Option<AgentId>,
    #[serde(default)]
    pub lease_holder: Option<LeaseHolder>,
    #[serde(default)]
    pub worktree_id: Option<String>,
    #[serde(default)]
    pub branch_ref: Option<String>,
    pub task: Option<CoordinationTaskId>,
    pub anchors: Vec<AnchorRef>,
    pub capability: Capability,
    pub mode: ClaimMode,
    pub since: Timestamp,
    #[serde(default)]
    pub refreshed_at: Option<Timestamp>,
    #[serde(default)]
    pub stale_at: Option<Timestamp>,
    pub expires_at: Timestamp,
    pub status: ClaimStatus,
    pub base_revision: WorkspaceRevision,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct EventExecutionOwner {
    #[serde(default)]
    pub principal: Option<PrincipalActor>,
    #[serde(default)]
    pub session_id: Option<SessionId>,
    #[serde(default)]
    pub worktree_id: Option<String>,
    #[serde(default)]
    pub service_instance_id: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct EventExecutionRecord {
    pub id: EventExecutionId,
    pub trigger_kind: EventTriggerKind,
    #[serde(default)]
    pub trigger_target: Option<NodeRef>,
    #[serde(default)]
    pub hook_id: Option<String>,
    #[serde(default)]
    pub hook_version_digest: Option<String>,
    #[serde(default)]
    pub authoritative_revision: Option<u64>,
    pub status: EventExecutionStatus,
    #[serde(default)]
    pub owner: Option<EventExecutionOwner>,
    pub claimed_at: Timestamp,
    #[serde(default)]
    pub started_at: Option<Timestamp>,
    #[serde(default)]
    pub finished_at: Option<Timestamp>,
    #[serde(default)]
    pub expires_at: Option<Timestamp>,
    #[serde(default)]
    pub summary: Option<String>,
    #[serde(default)]
    pub metadata: Value,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct CoordinationConflict {
    pub severity: ConflictSeverity,
    pub anchors: Vec<AnchorRef>,
    pub overlap_kinds: Vec<ConflictOverlapKind>,
    pub summary: String,
    pub blocking_claims: Vec<ClaimId>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum PolicyViolationCode {
    InvalidPlanTransition,
    InvalidTaskTransition,
    TerminalPlanEdit,
    TerminalTaskEdit,
    PlanClosed,
    MissingDependency,
    CrossPlanDependency,
    StaleRevision,
    ClaimConflict,
    ArtifactRequired,
    ReviewRequired,
    RiskReviewRequired,
    ValidationRequired,
    ArtifactStale,
    InvalidArtifactRequirement,
    InvalidReviewRequirement,
    ReviewerClassNotAllowed,
    IncompletePlanTasks,
    ActivePlanClaims,
    ClaimNotOwned,
    AgentIdentityRequired,
    HandoffPending,
    HandoffTargetMismatch,
    ExecutorMismatch,
    TaskLeaseHeldByOther,
    TaskResumeRequired,
    TaskReclaimRequired,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct PolicyViolation {
    pub code: PolicyViolationCode,
    pub summary: String,
    pub plan_id: Option<PlanId>,
    pub task_id: Option<CoordinationTaskId>,
    pub claim_id: Option<ClaimId>,
    pub artifact_id: Option<ArtifactId>,
    #[serde(default)]
    pub details: Value,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct PolicyViolationRecord {
    pub event_id: EventId,
    pub ts: Timestamp,
    pub summary: String,
    pub plan_id: Option<PlanId>,
    pub task_id: Option<CoordinationTaskId>,
    pub claim_id: Option<ClaimId>,
    pub artifact_id: Option<ArtifactId>,
    pub violations: Vec<PolicyViolation>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Artifact {
    pub id: ArtifactId,
    pub task: CoordinationTaskId,
    #[serde(default)]
    pub artifact_requirement_id: String,
    #[serde(default)]
    pub worktree_id: Option<String>,
    #[serde(default)]
    pub branch_ref: Option<String>,
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
    #[serde(default)]
    pub review_requirement_id: String,
    #[serde(default)]
    pub reviewer_class: Option<ReviewerClass>,
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
    #[serde(default)]
    pub causes: Vec<BlockerCause>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
pub enum BlockerKind {
    Dependency,
    ClaimConflict,
    ArtifactRequired,
    ReviewRequired,
    RiskReviewRequired,
    ValidationRequired,
    StaleRevision,
    ArtifactStale,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum RuntimeDiscoveryMode {
    None,
    LanDirect,
    PublicUrl,
    Full,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum RuntimeDescriptorCapability {
    CoordinationRefPublisher,
    BoundedPeerReads,
    BundleExports,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct RuntimeDescriptor {
    pub runtime_id: String,
    pub repo_id: String,
    pub worktree_id: String,
    pub principal_id: String,
    pub instance_started_at: u64,
    pub last_seen_at: u64,
    #[serde(default)]
    pub branch_ref: Option<String>,
    #[serde(default)]
    pub checked_out_commit: Option<String>,
    #[serde(default)]
    pub capabilities: Vec<RuntimeDescriptorCapability>,
    #[serde(default = "default_runtime_discovery_mode")]
    pub discovery_mode: RuntimeDiscoveryMode,
    #[serde(default)]
    pub peer_endpoint: Option<String>,
    #[serde(default)]
    pub public_endpoint: Option<String>,
    #[serde(default)]
    pub peer_transport_identity: Option<String>,
    #[serde(default)]
    pub blob_snapshot_head: Option<String>,
    #[serde(default)]
    pub export_policy: Option<String>,
}

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
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
    pub title: String,
    pub goal: String,
    pub status: Option<PlanStatus>,
    pub policy: Option<CoordinationPolicy>,
    pub spec_refs: Vec<CoordinationSpecRef>,

            }

#[derive(Debug, Clone)]
pub struct PlanUpdateInput {
    pub plan_id: PlanId,
    pub title: Option<String>,
    pub status: Option<PlanStatus>,
    pub goal: Option<String>,
    pub policy: Option<CoordinationPolicy>,
    pub spec_refs: Option<Vec<CoordinationSpecRef>>,

            }

#[derive(Debug, Clone)]
pub struct TaskCreateInput {
    pub plan_id: PlanId,
    pub title: String,
    pub status: Option<CoordinationTaskStatus>,
    pub assignee: Option<AgentId>,
    pub session: Option<SessionId>,
    pub worktree_id: Option<String>,
    pub branch_ref: Option<String>,
    pub anchors: Vec<AnchorRef>,
    pub depends_on: Vec<CoordinationTaskId>,
    pub coordination_depends_on: Vec<CoordinationTaskId>,
    pub integrated_depends_on: Vec<CoordinationTaskId>,
    pub acceptance: Vec<AcceptanceCriterion>,
    pub base_revision: WorkspaceRevision,
    pub spec_refs: Vec<CoordinationTaskSpecRef>,
    pub artifact_requirements: Vec<ArtifactRequirement>,
    pub review_requirements: Vec<ReviewRequirement>,
}

#[derive(Debug, Clone)]
pub struct TaskUpdateInput {
    pub task_id: CoordinationTaskId,
    pub kind: Option<PlanNodeKind>,
    pub status: Option<CoordinationTaskStatus>,
    pub published_task_status: Option<Option<CoordinationTaskStatus>>,
    pub git_execution: Option<TaskGitExecution>,
    pub assignee: Option<Option<AgentId>>,
    pub session: Option<Option<SessionId>>,
    pub worktree_id: Option<Option<String>>,
    pub branch_ref: Option<Option<String>>,
    pub title: Option<String>,
    pub summary: Option<Option<String>>,
    pub anchors: Option<Vec<AnchorRef>>,
    pub bindings: Option<PlanBinding>,
    pub depends_on: Option<Vec<CoordinationTaskId>>,
    pub coordination_depends_on: Option<Vec<CoordinationTaskId>>,
    pub integrated_depends_on: Option<Vec<CoordinationTaskId>>,
    pub acceptance: Option<Vec<AcceptanceCriterion>>,
    pub validation_refs: Option<Vec<ValidationRef>>,
    pub is_abstract: Option<bool>,
    pub base_revision: Option<WorkspaceRevision>,
    pub priority: Option<Option<u8>>,
    pub tags: Option<Vec<String>>,
    pub spec_refs: Option<Vec<CoordinationTaskSpecRef>>,
    pub artifact_requirements: Option<Vec<ArtifactRequirement>>,
    pub review_requirements: Option<Vec<ReviewRequirement>>,
    pub completion_context: Option<TaskCompletionContext>,
}

#[derive(Debug, Clone, Default)]
pub struct TaskCompletionContext {
    pub risk_score: Option<f32>,
    pub required_validations: Vec<String>,
    pub validated_checks: Vec<String>,
    pub review_artifact_ref: Option<String>,
    pub integration_commit: Option<String>,
    pub integration_evidence: Option<prism_ir::GitIntegrationEvidence>,
}

#[derive(Debug, Clone)]
pub struct HandoffInput {
    pub task_id: CoordinationTaskId,
    pub to_agent: Option<AgentId>,
    pub summary: String,
    pub base_revision: WorkspaceRevision,
}

#[derive(Debug, Clone)]
pub struct HandoffAcceptInput {
    pub task_id: CoordinationTaskId,
    pub agent: Option<AgentId>,
    pub worktree_id: Option<String>,
    pub branch_ref: Option<String>,
}

#[derive(Debug, Clone)]
pub struct TaskResumeInput {
    pub task_id: CoordinationTaskId,
    pub agent: Option<AgentId>,
    pub worktree_id: Option<String>,
    pub branch_ref: Option<String>,
}

#[derive(Debug, Clone)]
pub struct TaskReclaimInput {
    pub task_id: CoordinationTaskId,
    pub agent: Option<AgentId>,
    pub worktree_id: Option<String>,
    pub branch_ref: Option<String>,
}

#[derive(Debug, Clone)]
pub struct ClaimAcquireInput {
    pub task_id: Option<CoordinationTaskId>,
    pub anchors: Vec<AnchorRef>,
    pub capability: Capability,
    pub mode: Option<ClaimMode>,
    pub ttl_seconds: Option<u64>,
    pub base_revision: WorkspaceRevision,
    pub current_revision: WorkspaceRevision,
    pub agent: Option<AgentId>,
    pub worktree_id: Option<String>,
    pub branch_ref: Option<String>,
}

#[derive(Debug, Clone)]
pub struct ArtifactProposeInput {
    pub task_id: CoordinationTaskId,
    pub artifact_requirement_id: Option<String>,
    pub anchors: Vec<AnchorRef>,
    pub diff_ref: Option<String>,
    pub evidence: Vec<EventId>,
    pub base_revision: WorkspaceRevision,
    pub current_revision: WorkspaceRevision,
    pub required_validations: Vec<String>,
    pub validated_checks: Vec<String>,
    pub risk_score: Option<f32>,
    pub worktree_id: Option<String>,
    pub branch_ref: Option<String>,
}

#[derive(Debug, Clone)]
pub struct ArtifactSupersedeInput {
    pub artifact_id: ArtifactId,
}

#[derive(Debug, Clone)]
pub struct ArtifactReviewInput {
    pub artifact_id: ArtifactId,
    pub review_requirement_id: Option<String>,
    pub verdict: ReviewVerdict,
    pub summary: String,
    pub required_validations: Vec<String>,
    pub validated_checks: Vec<String>,
    pub risk_score: Option<f32>,
}

#[cfg(test)]
mod tests {
    use super::{EventExecutionOwner, EventExecutionRecord};
    use prism_ir::{
        EventExecutionId, EventExecutionStatus, EventTriggerKind, NodeRef, PlanId, PrincipalActor,
        PrincipalAuthorityId, PrincipalId, PrincipalKind,
    };

    #[test]
    fn event_execution_record_serializes_with_contract_shapes() {
        let record = EventExecutionRecord {
            id: EventExecutionId::new("event-exec:test"),
            trigger_kind: EventTriggerKind::RecurringPlanTick,
            trigger_target: Some(NodeRef::plan(PlanId::new("plan:test"))),
            hook_id: Some("hooks/recurring-plan".to_string()),
            hook_version_digest: Some("sha256:test".to_string()),
            authoritative_revision: Some(42),
            status: EventExecutionStatus::Claimed,
            owner: Some(EventExecutionOwner {
                principal: Some(PrincipalActor {
                    authority_id: PrincipalAuthorityId::new("authority:test"),
                    principal_id: PrincipalId::new("principal:test"),
                    kind: Some(PrincipalKind::Service),
                    name: Some("Scheduler".to_string()),
                }),
                session_id: None,
                worktree_id: None,
                service_instance_id: Some("service:test".to_string()),
            }),
            claimed_at: 100,
            started_at: None,
            finished_at: None,
            expires_at: Some(160),
            summary: Some("Recurring plan tick claimed".to_string()),
            metadata: serde_json::json!({
                "recurrencePolicy": "daily",
            }),
        };

        let value = serde_json::to_value(&record).expect("record should serialize");
        assert_eq!(value["triggerKind"], "recurring_plan_tick");
        assert_eq!(value["status"], "claimed");
        assert_eq!(value["triggerTarget"]["kind"], "plan");
        assert_eq!(value["hookId"], "hooks/recurring-plan");
        assert_eq!(value["owner"]["serviceInstanceId"], "service:test");
    }
}
