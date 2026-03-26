use prism_js::TaskJournalView;
use rmcp::schemars::JsonSchema;
use serde::Deserialize;
use serde_json::Value;

use crate::SessionView;

#[derive(Debug, Clone, Deserialize, JsonSchema)]
#[serde(rename_all = "lowercase")]
pub(crate) enum QueryLanguage {
    Ts,
}

#[derive(Debug, Deserialize, JsonSchema)]
pub(crate) struct PrismQueryArgs {
    #[schemars(description = "TypeScript snippet evaluated with a global `prism` object.")]
    pub(crate) code: String,
    #[schemars(description = "Query language. Only `ts` is currently supported.")]
    pub(crate) language: Option<QueryLanguage>,
}

#[derive(Debug, Clone, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub(crate) struct NodeIdInput {
    #[serde(alias = "crate_name")]
    #[serde(alias = "crateName")]
    pub(crate) crate_name: String,
    pub(crate) path: String,
    pub(crate) kind: String,
}

#[derive(Debug, Clone, Deserialize, JsonSchema)]
#[serde(tag = "type", rename_all = "snake_case")]
pub(crate) enum AnchorRefInput {
    Node {
        #[serde(alias = "crate_name")]
        #[serde(alias = "crateName")]
        crate_name: String,
        path: String,
        kind: String,
    },
    Lineage {
        #[serde(rename = "lineageId", alias = "lineage_id")]
        lineage_id: String,
    },
    File {
        #[serde(rename = "fileId", alias = "file_id")]
        file_id: u32,
    },
    Kind {
        kind: String,
    },
}

#[derive(Debug, Clone, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub(crate) enum OutcomeKindInput {
    NoteAdded,
    HypothesisProposed,
    PlanCreated,
    BuildRan,
    TestRan,
    ReviewFeedback,
    FailureObserved,
    RegressionObserved,
    FixValidated,
    RollbackPerformed,
    MigrationRequired,
    IncidentLinked,
    PerfSignalObserved,
}

#[derive(Debug, Clone, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub(crate) enum OutcomeResultInput {
    Success,
    Failure,
    Partial,
    Unknown,
}

#[derive(Debug, Clone, serde::Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub(crate) enum MemoryKindInput {
    Episodic,
    Structural,
    Semantic,
}

#[derive(Debug, Clone, serde::Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub(crate) enum MemorySourceInput {
    Agent,
    User,
    System,
}

#[derive(Debug, Clone, Deserialize, JsonSchema)]
#[serde(tag = "type", rename_all = "snake_case")]
pub(crate) enum OutcomeEvidenceInput {
    Commit { sha: String },
    Test { name: String, passed: bool },
    Build { target: String, passed: bool },
    Reviewer { author: String },
    Issue { id: String },
    StackTrace { hash: String },
    DiffSummary { text: String },
}

#[derive(Debug, Clone, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub(crate) enum InferredEdgeScopeInput {
    SessionOnly,
    Persisted,
    Rejected,
    Expired,
}

#[derive(Debug, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub(crate) struct PrismOutcomeArgs {
    pub(crate) kind: OutcomeKindInput,
    pub(crate) anchors: Vec<AnchorRefInput>,
    pub(crate) summary: String,
    pub(crate) result: Option<OutcomeResultInput>,
    pub(crate) evidence: Option<Vec<OutcomeEvidenceInput>>,
    #[serde(alias = "task_id")]
    pub(crate) task_id: Option<String>,
}

#[derive(Debug, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub(crate) struct MemoryStorePayload {
    pub(crate) anchors: Vec<AnchorRefInput>,
    pub(crate) kind: MemoryKindInput,
    pub(crate) content: String,
    pub(crate) trust: Option<f32>,
    pub(crate) source: Option<MemorySourceInput>,
    pub(crate) metadata: Option<Value>,
}

#[derive(Debug, Clone, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub(crate) enum MemoryMutationActionInput {
    Store,
}

#[derive(Debug, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub(crate) struct PrismMemoryArgs {
    pub(crate) action: MemoryMutationActionInput,
    pub(crate) payload: Value,
    #[serde(alias = "task_id")]
    pub(crate) task_id: Option<String>,
}

#[derive(Debug, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub(crate) struct PrismValidationFeedbackArgs {
    pub(crate) anchors: Vec<AnchorRefInput>,
    pub(crate) context: String,
    pub(crate) prism_said: String,
    pub(crate) actually_true: String,
    pub(crate) category: String,
    pub(crate) verdict: String,
    pub(crate) corrected_manually: Option<bool>,
    pub(crate) correction: Option<String>,
    pub(crate) metadata: Option<Value>,
    #[serde(alias = "task_id")]
    pub(crate) task_id: Option<String>,
}

#[derive(Debug, Deserialize, JsonSchema)]
pub(crate) struct PrismStartTaskArgs {
    pub(crate) description: String,
    pub(crate) tags: Option<Vec<String>>,
}

#[derive(Debug, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub(crate) struct PrismFinishTaskArgs {
    pub(crate) summary: String,
    pub(crate) anchors: Option<Vec<AnchorRefInput>>,
    #[serde(alias = "task_id")]
    pub(crate) task_id: Option<String>,
}

#[derive(Debug, Clone, serde::Serialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub(crate) struct EventMutationResult {
    pub(crate) event_id: String,
    pub(crate) task_id: String,
}

#[derive(Debug, Clone, serde::Serialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub(crate) struct MemoryMutationResult {
    pub(crate) memory_id: String,
    pub(crate) task_id: String,
}

#[derive(Debug, Clone, serde::Serialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub(crate) struct ValidationFeedbackMutationResult {
    pub(crate) entry_id: String,
    pub(crate) task_id: String,
}

#[derive(Debug, Clone, serde::Serialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub(crate) struct EdgeMutationResult {
    pub(crate) edge_id: String,
    pub(crate) task_id: String,
}

#[derive(Debug, Clone, serde::Serialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub(crate) struct CuratorProposalDecisionResult {
    pub(crate) job_id: String,
    pub(crate) proposal: Value,
    pub(crate) memory_id: Option<String>,
    pub(crate) edge_id: Option<String>,
}

#[derive(Debug, Clone, serde::Serialize, JsonSchema)]
pub(crate) struct QueryDiagnosticSchema {
    pub(crate) code: String,
    pub(crate) message: String,
    pub(crate) data: Option<Value>,
}

#[derive(Debug, Clone, serde::Serialize, JsonSchema)]
pub(crate) struct QueryEnvelopeSchema {
    pub(crate) result: Value,
    pub(crate) diagnostics: Vec<QueryDiagnosticSchema>,
}

#[derive(Debug, Clone, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub(crate) struct QueryLimitsInput {
    pub(crate) max_result_nodes: Option<usize>,
    pub(crate) max_call_graph_depth: Option<usize>,
    pub(crate) max_output_json_bytes: Option<usize>,
}

#[derive(Debug, Clone, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub(crate) struct PrismConfigureSessionArgs {
    pub(crate) limits: Option<QueryLimitsInput>,
    #[serde(alias = "current_task_id")]
    pub(crate) current_task_id: Option<String>,
    #[serde(alias = "current_task_description")]
    pub(crate) current_task_description: Option<String>,
    #[serde(alias = "current_task_tags")]
    pub(crate) current_task_tags: Option<Vec<String>>,
    pub(crate) clear_current_task: Option<bool>,
    #[serde(alias = "current_agent")]
    pub(crate) current_agent: Option<String>,
    pub(crate) clear_current_agent: Option<bool>,
}

#[derive(Debug, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case", tag = "action", content = "input")]
pub(crate) enum PrismSessionArgs {
    StartTask(PrismStartTaskArgs),
    Configure(PrismConfigureSessionArgs),
    FinishTask(PrismFinishTaskArgs),
    AbandonTask(PrismFinishTaskArgs),
}

#[derive(Debug, Clone, serde::Serialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub(crate) enum SessionMutationActionSchema {
    StartTask,
    Configure,
    FinishTask,
    AbandonTask,
}

#[derive(Debug, Clone, serde::Serialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub(crate) struct PrismSessionMutationResult {
    pub(crate) action: SessionMutationActionSchema,
    pub(crate) task_id: Option<String>,
    pub(crate) event_id: Option<String>,
    pub(crate) memory_id: Option<String>,
    pub(crate) journal: Option<TaskJournalView>,
    pub(crate) session: SessionView,
}

#[derive(Debug, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub(crate) struct PrismInferEdgeArgs {
    pub(crate) source: NodeIdInput,
    pub(crate) target: NodeIdInput,
    pub(crate) kind: String,
    pub(crate) confidence: f32,
    pub(crate) scope: Option<InferredEdgeScopeInput>,
    pub(crate) evidence: Option<Vec<String>>,
    #[serde(alias = "task_id")]
    pub(crate) task_id: Option<String>,
}

#[derive(Debug, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case", tag = "action", content = "input")]
pub(crate) enum PrismMutationArgs {
    Outcome(PrismOutcomeArgs),
    Memory(PrismMemoryArgs),
    ValidationFeedback(PrismValidationFeedbackArgs),
    InferEdge(PrismInferEdgeArgs),
    Coordination(PrismCoordinationArgs),
    Claim(PrismClaimArgs),
    Artifact(PrismArtifactArgs),
    TestRan(PrismTestRanArgs),
    FailureObserved(PrismFailureObservedArgs),
    FixValidated(PrismFixValidatedArgs),
    CuratorPromoteEdge(PrismCuratorPromoteEdgeArgs),
    CuratorPromoteMemory(PrismCuratorPromoteMemoryArgs),
    CuratorRejectProposal(PrismCuratorRejectProposalArgs),
}

#[derive(Debug, Clone, serde::Serialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub(crate) enum PrismMutationActionSchema {
    Outcome,
    Memory,
    ValidationFeedback,
    InferEdge,
    Coordination,
    Claim,
    Artifact,
    TestRan,
    FailureObserved,
    FixValidated,
    CuratorPromoteEdge,
    CuratorPromoteMemory,
    CuratorRejectProposal,
}

#[derive(Debug, Clone, serde::Serialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub(crate) struct PrismMutationResult {
    pub(crate) action: PrismMutationActionSchema,
    pub(crate) result: Value,
}

#[derive(Debug, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub(crate) struct PrismTestRanArgs {
    pub(crate) anchors: Vec<AnchorRefInput>,
    pub(crate) test: String,
    pub(crate) passed: bool,
    #[serde(alias = "task_id")]
    pub(crate) task_id: Option<String>,
}

#[derive(Debug, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub(crate) struct PrismFailureObservedArgs {
    pub(crate) anchors: Vec<AnchorRefInput>,
    pub(crate) summary: String,
    pub(crate) trace: Option<String>,
    #[serde(alias = "task_id")]
    pub(crate) task_id: Option<String>,
}

#[derive(Debug, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub(crate) struct PrismFixValidatedArgs {
    pub(crate) anchors: Vec<AnchorRefInput>,
    pub(crate) summary: String,
    #[serde(alias = "task_id")]
    pub(crate) task_id: Option<String>,
}

#[derive(Debug, Clone, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub(crate) enum CoordinationMutationKindInput {
    PlanCreate,
    PlanUpdate,
    TaskCreate,
    TaskUpdate,
    Handoff,
    HandoffAccept,
}

#[derive(Debug, Clone, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub(crate) enum ClaimActionInput {
    Acquire,
    Renew,
    Release,
}

#[derive(Debug, Clone, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub(crate) enum ArtifactActionInput {
    Propose,
    Supersede,
    Review,
}

#[derive(Debug, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub(crate) struct PrismCoordinationArgs {
    pub(crate) kind: CoordinationMutationKindInput,
    pub(crate) payload: Value,
    #[serde(alias = "task_id")]
    pub(crate) task_id: Option<String>,
}

#[derive(Debug, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub(crate) struct PrismClaimArgs {
    pub(crate) action: ClaimActionInput,
    pub(crate) payload: Value,
    #[serde(alias = "task_id")]
    pub(crate) task_id: Option<String>,
}

#[derive(Debug, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub(crate) struct PrismArtifactArgs {
    pub(crate) action: ArtifactActionInput,
    pub(crate) payload: Value,
    #[serde(alias = "task_id")]
    pub(crate) task_id: Option<String>,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct PlanTargetArgs {
    #[serde(alias = "plan_id")]
    pub(crate) plan_id: String,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct CoordinationTaskTargetArgs {
    #[serde(alias = "task_id")]
    pub(crate) task_id: String,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct PolicyViolationQueryArgs {
    #[serde(alias = "plan_id")]
    pub(crate) plan_id: Option<String>,
    #[serde(alias = "task_id")]
    pub(crate) task_id: Option<String>,
    pub(crate) limit: Option<usize>,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct LimitArgs {
    pub(crate) limit: Option<usize>,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct AnchorListArgs {
    pub(crate) anchors: Vec<AnchorRefInput>,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct PendingReviewsArgs {
    #[serde(alias = "plan_id")]
    pub(crate) plan_id: Option<String>,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct SimulateClaimArgs {
    pub(crate) anchors: Vec<AnchorRefInput>,
    pub(crate) capability: String,
    pub(crate) mode: Option<String>,
    #[serde(alias = "task_id")]
    pub(crate) task_id: Option<String>,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct PlanCreatePayload {
    pub(crate) goal: String,
    pub(crate) status: Option<String>,
    pub(crate) policy: Option<CoordinationPolicyPayload>,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct PlanUpdatePayload {
    pub(crate) plan_id: String,
    pub(crate) status: Option<String>,
    pub(crate) goal: Option<String>,
    pub(crate) policy: Option<CoordinationPolicyPayload>,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct CoordinationPolicyPayload {
    pub(crate) default_claim_mode: Option<String>,
    pub(crate) max_parallel_editors_per_anchor: Option<u16>,
    pub(crate) require_review_for_completion: Option<bool>,
    pub(crate) require_validation_for_completion: Option<bool>,
    pub(crate) stale_after_graph_change: Option<bool>,
    pub(crate) review_required_above_risk_score: Option<f32>,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct AcceptanceCriterionPayload {
    pub(crate) label: String,
    pub(crate) anchors: Option<Vec<AnchorRefInput>>,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct TaskCreatePayload {
    pub(crate) plan_id: String,
    pub(crate) title: String,
    pub(crate) status: Option<String>,
    pub(crate) assignee: Option<String>,
    pub(crate) anchors: Option<Vec<AnchorRefInput>>,
    pub(crate) depends_on: Option<Vec<String>>,
    pub(crate) acceptance: Option<Vec<AcceptanceCriterionPayload>>,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct TaskUpdatePayload {
    pub(crate) task_id: String,
    pub(crate) status: Option<String>,
    pub(crate) assignee: Option<String>,
    pub(crate) title: Option<String>,
    pub(crate) anchors: Option<Vec<AnchorRefInput>>,
    pub(crate) completion_context: Option<TaskCompletionContextPayload>,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct TaskCompletionContextPayload {
    pub(crate) risk_score: Option<f32>,
    pub(crate) required_validations: Option<Vec<String>>,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct HandoffPayload {
    pub(crate) task_id: String,
    pub(crate) to_agent: Option<String>,
    pub(crate) summary: String,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct HandoffAcceptPayload {
    pub(crate) task_id: String,
    pub(crate) agent: Option<String>,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct ClaimAcquirePayload {
    pub(crate) anchors: Vec<AnchorRefInput>,
    pub(crate) capability: String,
    pub(crate) mode: Option<String>,
    pub(crate) ttl_seconds: Option<u64>,
    pub(crate) agent: Option<String>,
    pub(crate) coordination_task_id: Option<String>,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct ClaimRenewPayload {
    pub(crate) claim_id: String,
    pub(crate) ttl_seconds: Option<u64>,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct ClaimReleasePayload {
    pub(crate) claim_id: String,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct ArtifactProposePayload {
    pub(crate) task_id: String,
    pub(crate) anchors: Option<Vec<AnchorRefInput>>,
    pub(crate) diff_ref: Option<String>,
    pub(crate) evidence: Option<Vec<String>>,
    pub(crate) required_validations: Option<Vec<String>>,
    pub(crate) validated_checks: Option<Vec<String>>,
    pub(crate) risk_score: Option<f32>,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct ArtifactSupersedePayload {
    pub(crate) artifact_id: String,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct ArtifactReviewPayload {
    pub(crate) artifact_id: String,
    pub(crate) verdict: String,
    pub(crate) summary: String,
    pub(crate) required_validations: Option<Vec<String>>,
    pub(crate) validated_checks: Option<Vec<String>>,
    pub(crate) risk_score: Option<f32>,
}

#[derive(Debug, Clone, serde::Serialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub(crate) struct MutationViolationView {
    pub(crate) code: String,
    pub(crate) summary: String,
    pub(crate) plan_id: Option<String>,
    pub(crate) task_id: Option<String>,
    pub(crate) claim_id: Option<String>,
    pub(crate) artifact_id: Option<String>,
    pub(crate) details: Value,
}

#[derive(Debug, Clone, serde::Serialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub(crate) struct CoordinationMutationResult {
    pub(crate) event_id: String,
    pub(crate) event_ids: Vec<String>,
    pub(crate) rejected: bool,
    pub(crate) violations: Vec<MutationViolationView>,
    pub(crate) state: Value,
}

#[derive(Debug, Clone, serde::Serialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub(crate) struct ClaimMutationResult {
    pub(crate) claim_id: Option<String>,
    pub(crate) event_ids: Vec<String>,
    pub(crate) rejected: bool,
    pub(crate) conflicts: Vec<Value>,
    pub(crate) violations: Vec<MutationViolationView>,
    pub(crate) state: Value,
}

#[derive(Debug, Clone, serde::Serialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub(crate) struct ArtifactMutationResult {
    pub(crate) artifact_id: Option<String>,
    pub(crate) review_id: Option<String>,
    pub(crate) event_ids: Vec<String>,
    pub(crate) rejected: bool,
    pub(crate) violations: Vec<MutationViolationView>,
    pub(crate) state: Value,
}

#[derive(Debug, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub(crate) struct PrismCuratorPromoteEdgeArgs {
    #[serde(alias = "job_id")]
    pub(crate) job_id: String,
    #[serde(alias = "proposal_index")]
    pub(crate) proposal_index: usize,
    pub(crate) scope: Option<InferredEdgeScopeInput>,
    pub(crate) note: Option<String>,
    #[serde(alias = "task_id")]
    pub(crate) task_id: Option<String>,
}

#[derive(Debug, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub(crate) struct PrismCuratorRejectProposalArgs {
    #[serde(alias = "job_id")]
    pub(crate) job_id: String,
    #[serde(alias = "proposal_index")]
    pub(crate) proposal_index: usize,
    pub(crate) reason: Option<String>,
    #[serde(alias = "task_id")]
    pub(crate) task_id: Option<String>,
}

#[derive(Debug, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub(crate) struct PrismCuratorPromoteMemoryArgs {
    #[serde(alias = "job_id")]
    pub(crate) job_id: String,
    #[serde(alias = "proposal_index")]
    pub(crate) proposal_index: usize,
    pub(crate) trust: Option<f32>,
    pub(crate) note: Option<String>,
    #[serde(alias = "task_id")]
    pub(crate) task_id: Option<String>,
}
