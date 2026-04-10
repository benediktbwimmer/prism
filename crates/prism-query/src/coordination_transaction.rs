use std::collections::{BTreeMap, BTreeSet};
use std::fmt;

use anyhow::{anyhow, Result};
use prism_coordination::{
    AcceptanceCriterion, ArtifactProposeInput, ArtifactReviewInput, ArtifactSupersedeInput,
    CoordinationPolicy, CoordinationRuntimeState, CoordinationSnapshot, CoordinationSpecRef,
    CoordinationTaskSpecRef, PlanCreateInput, PlanScheduling, PlanUpdateInput,
    TaskCompletionContext, TaskCreateInput, TaskGitExecution, TaskUpdateInput,
};
use prism_ir::{
    AgentId, AnchorRef, ArtifactId, ClaimId, CoordinationEventKind, CoordinationTaskId,
    CoordinationTaskStatus, EventId, EventMeta, PlanBinding, PlanId, PlanStatus, ReviewId,
    ReviewVerdict, SessionId, ValidationRef, WorkspaceRevision,
};
use serde::Serialize;
use serde_json::Value;

use crate::Prism;

#[derive(Debug, Clone)]
pub enum CoordinationTransactionPlanRef {
    Id(PlanId),
    ClientId(String),
}

#[derive(Debug, Clone)]
pub enum CoordinationTransactionTaskRef {
    Id(CoordinationTaskId),
    ClientId(String),
}

#[derive(Debug, Clone)]
pub enum CoordinationTransactionClaimRef {
    Id(ClaimId),
    ClientId(String),
}

#[derive(Debug, Clone)]
pub enum CoordinationTransactionArtifactRef {
    Id(ArtifactId),
    ClientId(String),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CoordinationDependencyKind {
    DependsOn,
    CoordinationDependsOn,
    IntegratedDependsOn,
}

#[derive(Debug, Clone, Default)]
pub struct CoordinationTransactionGitExecutionPolicyPatch {
    pub start_mode: Option<prism_coordination::GitExecutionStartMode>,
    pub completion_mode: Option<prism_coordination::GitExecutionCompletionMode>,
    pub integration_mode: Option<prism_ir::GitIntegrationMode>,
    pub target_ref: Option<String>,
    pub target_branch: Option<String>,
    pub require_task_branch: Option<bool>,
    pub max_commits_behind_target: Option<u32>,
    pub max_fetch_age_seconds: Option<u64>,
}

#[derive(Debug, Clone, Default)]
pub struct CoordinationTransactionPolicyPatch {
    pub default_claim_mode: Option<prism_ir::ClaimMode>,
    pub max_parallel_editors_per_anchor: Option<u16>,
    pub require_review_for_completion: Option<bool>,
    pub require_validation_for_completion: Option<bool>,
    pub stale_after_graph_change: Option<bool>,
    pub review_required_above_risk_score: Option<f32>,
    pub lease_stale_after_seconds: Option<u64>,
    pub lease_expires_after_seconds: Option<u64>,
    pub lease_renewal_mode: Option<prism_ir::LeaseRenewalMode>,
    pub git_execution: Option<CoordinationTransactionGitExecutionPolicyPatch>,
}

#[derive(Debug, Clone, Default)]
pub struct CoordinationTransactionPlanSchedulingPatch {
    pub importance: Option<u8>,
    pub urgency: Option<u8>,
    pub manual_boost: Option<i16>,
    pub due_at: Option<u64>,
}

#[derive(Debug, Clone)]
pub enum CoordinationTransactionMutation {
    PlanCreate {
        client_plan_id: Option<String>,
        title: String,
        goal: String,
        status: Option<PlanStatus>,
        policy: Option<CoordinationPolicy>,
        scheduling: Option<PlanScheduling>,
        spec_refs: Vec<CoordinationSpecRef>,
    },
    PlanUpdate {
        plan: CoordinationTransactionPlanRef,
        title: Option<String>,
        goal: Option<String>,
        status: Option<PlanStatus>,
        policy: Option<CoordinationTransactionPolicyPatch>,
        scheduling: Option<CoordinationTransactionPlanSchedulingPatch>,
        spec_refs: Option<Vec<CoordinationSpecRef>>,
    },
    PlanArchive {
        plan: CoordinationTransactionPlanRef,
    },
    TaskCreate {
        client_task_id: Option<String>,
        plan: CoordinationTransactionPlanRef,
        title: String,
        status: Option<CoordinationTaskStatus>,
        assignee: Option<AgentId>,
        session: Option<SessionId>,
        worktree_id: Option<String>,
        branch_ref: Option<String>,
        anchors: Vec<AnchorRef>,
        depends_on: Vec<CoordinationTransactionTaskRef>,
        coordination_depends_on: Vec<CoordinationTransactionTaskRef>,
        integrated_depends_on: Vec<CoordinationTransactionTaskRef>,
        acceptance: Vec<AcceptanceCriterion>,
        base_revision: WorkspaceRevision,
        spec_refs: Vec<CoordinationTaskSpecRef>,
        artifact_requirements: Vec<prism_coordination::ArtifactRequirement>,
        review_requirements: Vec<prism_coordination::ReviewRequirement>,
    },
    TaskUpdate {
        task: CoordinationTransactionTaskRef,
        status: Option<CoordinationTaskStatus>,
        published_task_status: Option<Option<CoordinationTaskStatus>>,
        git_execution: Option<TaskGitExecution>,
        assignee: Option<Option<AgentId>>,
        session: Option<Option<SessionId>>,
        worktree_id: Option<Option<String>>,
        branch_ref: Option<Option<String>>,
        title: Option<String>,
        summary: Option<Option<String>>,
        anchors: Option<Vec<AnchorRef>>,
        bindings: Option<PlanBinding>,
        depends_on: Option<Vec<CoordinationTransactionTaskRef>>,
        acceptance: Option<Vec<AcceptanceCriterion>>,
        validation_refs: Option<Vec<ValidationRef>>,
        base_revision: WorkspaceRevision,
        priority: Option<Option<u8>>,
        tags: Option<Vec<String>>,
        completion_context: Option<TaskCompletionContext>,
        spec_refs: Option<Vec<CoordinationTaskSpecRef>>,
        artifact_requirements: Option<Vec<prism_coordination::ArtifactRequirement>>,
        review_requirements: Option<Vec<prism_coordination::ReviewRequirement>>,
    },
    DependencyCreate {
        task: CoordinationTransactionTaskRef,
        depends_on: CoordinationTransactionTaskRef,
        kind: CoordinationDependencyKind,
        base_revision: WorkspaceRevision,
    },
    ClaimAcquire {
        client_claim_id: Option<String>,
        task: Option<CoordinationTransactionTaskRef>,
        anchors: Vec<AnchorRef>,
        capability: prism_ir::Capability,
        mode: Option<prism_ir::ClaimMode>,
        ttl_seconds: Option<u64>,
        agent: Option<AgentId>,
        session: SessionId,
        worktree_id: Option<String>,
        branch_ref: Option<String>,
        base_revision: WorkspaceRevision,
        current_revision: WorkspaceRevision,
    },
    ClaimRenew {
        claim: CoordinationTransactionClaimRef,
        session: SessionId,
        ttl_seconds: Option<u64>,
    },
    ClaimRelease {
        claim: CoordinationTransactionClaimRef,
        session: SessionId,
    },
    ArtifactPropose {
        client_artifact_id: Option<String>,
        task: CoordinationTransactionTaskRef,
        artifact_requirement_id: Option<String>,
        anchors: Option<Vec<AnchorRef>>,
        diff_ref: Option<String>,
        evidence: Vec<EventId>,
        required_validations: Vec<String>,
        validated_checks: Vec<String>,
        risk_score: Option<f32>,
        base_revision: WorkspaceRevision,
        current_revision: WorkspaceRevision,
    },
    ArtifactSupersede {
        artifact: CoordinationTransactionArtifactRef,
    },
    ArtifactReview {
        artifact: CoordinationTransactionArtifactRef,
        review_requirement_id: Option<String>,
        verdict: ReviewVerdict,
        summary: String,
        required_validations: Vec<String>,
        validated_checks: Vec<String>,
        risk_score: Option<f32>,
        current_revision: WorkspaceRevision,
    },
    TaskHandoff {
        task: CoordinationTransactionTaskRef,
        to_agent: Option<AgentId>,
        summary: String,
        base_revision: WorkspaceRevision,
    },
    TaskHandoffAccept {
        task: CoordinationTransactionTaskRef,
        agent: Option<AgentId>,
        worktree_id: Option<String>,
        branch_ref: Option<String>,
    },
    TaskResume {
        task: CoordinationTransactionTaskRef,
        agent: Option<AgentId>,
        worktree_id: Option<String>,
        branch_ref: Option<String>,
    },
    TaskReclaim {
        task: CoordinationTransactionTaskRef,
        agent: Option<AgentId>,
        worktree_id: Option<String>,
        branch_ref: Option<String>,
    },
}

impl CoordinationTransactionMutation {
    fn action_tag(&self) -> &'static str {
        match self {
            Self::PlanCreate { .. } => "plan_create",
            Self::PlanUpdate { .. } => "plan_update",
            Self::PlanArchive { .. } => "plan_archive",
            Self::TaskCreate { .. } => "task_create",
            Self::TaskUpdate { .. } => "task_update",
            Self::DependencyCreate { .. } => "dependency_create",
            Self::ClaimAcquire { .. } => "claim_acquire",
            Self::ClaimRenew { .. } => "claim_renew",
            Self::ClaimRelease { .. } => "claim_release",
            Self::ArtifactPropose { .. } => "artifact_propose",
            Self::ArtifactSupersede { .. } => "artifact_supersede",
            Self::ArtifactReview { .. } => "artifact_review",
            Self::TaskHandoff { .. } => "task_handoff",
            Self::TaskHandoffAccept { .. } => "task_handoff_accept",
            Self::TaskResume { .. } => "task_resume",
            Self::TaskReclaim { .. } => "task_reclaim",
        }
    }
}

#[derive(Debug, Clone, Default)]
pub struct CoordinationTransactionInput {
    pub mutations: Vec<CoordinationTransactionMutation>,
    pub intent_metadata: Option<Value>,
    pub structured_transaction: Option<Value>,
    pub optimistic_preconditions: Option<Value>,
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
struct CoordinationTransactionOptimisticPreconditions {
    expected_revision: Option<u64>,
    expected_event_count: Option<usize>,
    expected_last_event_id: Option<EventId>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CoordinationTransactionValidationStage {
    InputShape,
    Authorization,
    ObjectIdentity,
    Domain,
    Conflict,
    Commit,
}

impl CoordinationTransactionValidationStage {
    pub fn tag(self) -> &'static str {
        match self {
            Self::InputShape => "input_shape",
            Self::Authorization => "authorization",
            Self::ObjectIdentity => "object_identity",
            Self::Conflict => "conflict",
            Self::Domain => "domain",
            Self::Commit => "commit",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CoordinationTransactionRejectionCategory {
    InvalidInput,
    Unauthorized,
    NotFound,
    DomainViolation,
    Conflict,
    Unsupported,
}

impl CoordinationTransactionRejectionCategory {
    pub fn tag(self) -> &'static str {
        match self {
            Self::InvalidInput => "invalid_input",
            Self::Unauthorized => "unauthorized",
            Self::NotFound => "not_found",
            Self::DomainViolation => "domain_violation",
            Self::Conflict => "conflict",
            Self::Unsupported => "unsupported",
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CoordinationTransactionRejection {
    pub stage: CoordinationTransactionValidationStage,
    pub category: CoordinationTransactionRejectionCategory,
    pub reason_code: &'static str,
    pub message: String,
}

#[derive(Debug, Clone)]
pub enum CoordinationTransactionError {
    Rejected(CoordinationTransactionRejection),
    Indeterminate {
        reason_code: &'static str,
        message: String,
    },
}

impl CoordinationTransactionError {
    fn rejected(
        stage: CoordinationTransactionValidationStage,
        category: CoordinationTransactionRejectionCategory,
        reason_code: &'static str,
        message: impl Into<String>,
    ) -> Self {
        Self::Rejected(CoordinationTransactionRejection {
            stage,
            category,
            reason_code,
            message: message.into(),
        })
    }

    fn domain(error: anyhow::Error) -> Self {
        Self::rejected(
            CoordinationTransactionValidationStage::Domain,
            CoordinationTransactionRejectionCategory::DomainViolation,
            "domain_validation_failed",
            error.to_string(),
        )
    }

    pub fn protocol_state(&self) -> CoordinationTransactionProtocolState {
        match self {
            Self::Rejected(rejection) => CoordinationTransactionProtocolState {
                outcome: "Rejected".to_string(),
                commit: None,
                authority_version: None,
                intent_metadata: None,
                structured_transaction: None,
                rejection: Some(CoordinationTransactionProtocolRejection {
                    stage: rejection.stage.tag().to_string(),
                    category: rejection.category.tag().to_string(),
                    reason_code: rejection.reason_code.to_string(),
                    message: rejection.message.clone(),
                }),
                indeterminate: None,
            },
            Self::Indeterminate {
                reason_code,
                message,
            } => CoordinationTransactionProtocolState {
                outcome: "Indeterminate".to_string(),
                commit: None,
                authority_version: None,
                intent_metadata: None,
                structured_transaction: None,
                rejection: None,
                indeterminate: Some(CoordinationTransactionProtocolIndeterminate {
                    reason_code: reason_code.to_string(),
                    message: message.clone(),
                }),
            },
        }
    }
}

impl fmt::Display for CoordinationTransactionError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Rejected(rejection) => write!(
                f,
                "{} [{}::{:?}]",
                rejection.message, rejection.reason_code, rejection.category
            ),
            Self::Indeterminate {
                reason_code,
                message,
            } => write!(f, "{message} [{reason_code}]"),
        }
    }
}

impl std::error::Error for CoordinationTransactionError {}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CoordinationTransactionOutcome {
    Committed,
}

impl CoordinationTransactionOutcome {
    pub fn tag(self) -> &'static str {
        match self {
            Self::Committed => "Committed",
        }
    }
}

#[derive(Debug, Clone, Default)]
pub struct CoordinationTransactionCommitMetadata {
    pub event_ids: Vec<EventId>,
    pub event_count: usize,
    pub last_event_id: Option<EventId>,
    pub committed_at: Option<u64>,
}

#[derive(Debug, Clone, Default)]
pub struct CoordinationTransactionAuthorityVersion {
    pub total_event_count: usize,
    pub last_event_id: Option<EventId>,
    pub committed_at: Option<u64>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct CoordinationTransactionProtocolCommit {
    pub event_ids: Vec<String>,
    pub event_count: usize,
    pub last_event_id: Option<String>,
    pub committed_at: Option<u64>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct CoordinationTransactionProtocolAuthorityVersion {
    pub event_count: usize,
    pub last_event_id: Option<String>,
    pub committed_at: Option<u64>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct CoordinationTransactionProtocolRejection {
    pub stage: String,
    pub category: String,
    pub reason_code: String,
    pub message: String,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct CoordinationTransactionProtocolIndeterminate {
    pub reason_code: String,
    pub message: String,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct CoordinationTransactionProtocolState {
    pub outcome: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub commit: Option<CoordinationTransactionProtocolCommit>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub authority_version: Option<CoordinationTransactionProtocolAuthorityVersion>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub intent_metadata: Option<Value>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub structured_transaction: Option<Value>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub rejection: Option<CoordinationTransactionProtocolRejection>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub indeterminate: Option<CoordinationTransactionProtocolIndeterminate>,
}

#[derive(Debug, Clone)]
pub struct CoordinationTransactionResult {
    pub outcome: CoordinationTransactionOutcome,
    pub commit: CoordinationTransactionCommitMetadata,
    pub authority_version: CoordinationTransactionAuthorityVersion,
    pub intent_metadata: Option<Value>,
    pub structured_transaction: Option<Value>,
    pub plan_ids_by_client_id: BTreeMap<String, PlanId>,
    pub task_ids_by_client_id: BTreeMap<String, CoordinationTaskId>,
    pub claim_ids_by_client_id: BTreeMap<String, ClaimId>,
    pub artifact_ids_by_client_id: BTreeMap<String, ArtifactId>,
    pub touched_plan_ids: Vec<PlanId>,
    pub touched_task_ids: Vec<CoordinationTaskId>,
    pub touched_claim_ids: Vec<ClaimId>,
    pub touched_artifact_ids: Vec<ArtifactId>,
    pub touched_review_ids: Vec<ReviewId>,
}

impl CoordinationTransactionCommitMetadata {
    fn protocol_state(&self) -> CoordinationTransactionProtocolCommit {
        CoordinationTransactionProtocolCommit {
            event_ids: self
                .event_ids
                .iter()
                .map(|event_id| event_id.0.to_string())
                .collect(),
            event_count: self.event_count,
            last_event_id: self
                .last_event_id
                .as_ref()
                .map(|event_id| event_id.0.to_string()),
            committed_at: self.committed_at,
        }
    }
}

impl CoordinationTransactionAuthorityVersion {
    fn protocol_state(&self) -> CoordinationTransactionProtocolAuthorityVersion {
        CoordinationTransactionProtocolAuthorityVersion {
            event_count: self.total_event_count,
            last_event_id: self
                .last_event_id
                .as_ref()
                .map(|event_id| event_id.0.to_string()),
            committed_at: self.committed_at,
        }
    }
}

impl CoordinationTransactionResult {
    pub fn protocol_state(&self) -> CoordinationTransactionProtocolState {
        CoordinationTransactionProtocolState {
            outcome: self.outcome.tag().to_string(),
            commit: Some(self.commit.protocol_state()),
            authority_version: Some(self.authority_version.protocol_state()),
            intent_metadata: self.intent_metadata.clone(),
            structured_transaction: self.structured_transaction.clone(),
            rejection: None,
            indeterminate: None,
        }
    }
}

impl Prism {
    pub fn execute_coordination_transaction(
        &self,
        meta: EventMeta,
        input: CoordinationTransactionInput,
    ) -> std::result::Result<CoordinationTransactionResult, CoordinationTransactionError> {
        validate_transaction_input_shape(&input)?;
        validate_transaction_authorization(&input)?;
        let optimistic_preconditions =
            parse_optimistic_preconditions(input.optimistic_preconditions.as_ref())?;
        self.coordination_transaction(|coordination_runtime| {
            validate_transaction_identity(coordination_runtime, &input)?;
            validate_transaction_conflict(coordination_runtime, optimistic_preconditions.as_ref())?;
            apply_coordination_transaction(coordination_runtime, meta.clone(), input)
                .map_err(CoordinationTransactionError::domain)
        })
    }

    pub fn execute_coordination_mutation(
        &self,
        meta: EventMeta,
        mutation: CoordinationTransactionMutation,
    ) -> std::result::Result<CoordinationTransactionResult, CoordinationTransactionError> {
        self.execute_coordination_transaction(
            meta,
            CoordinationTransactionInput {
                mutations: vec![mutation],
                ..CoordinationTransactionInput::default()
            },
        )
    }

    pub(crate) fn coordination_transaction<T, F>(
        &self,
        mutate: F,
    ) -> std::result::Result<T, CoordinationTransactionError>
    where
        F: FnOnce(
            &mut CoordinationRuntimeState,
        ) -> std::result::Result<T, CoordinationTransactionError>,
    {
        let mut runtime = self
            .materialized_runtime
            .write()
            .expect("materialized runtime lock poisoned");
        let before_snapshot = runtime.snapshot();
        let result = {
            let coordination_runtime = runtime.continuity_runtime_mut();
            match mutate(coordination_runtime) {
                Ok(value) => {
                    let snapshot = coordination_runtime.snapshot();
                    snapshot
                        .validate_canonical_projection()
                        .map_err(CoordinationTransactionError::domain)?;
                    snapshot
                        .to_canonical_snapshot_v2()
                        .validate_graph()
                        .map_err(CoordinationTransactionError::domain)?;
                    Ok(value)
                }
                Err(error) => Err(error),
            }
        };

        match result {
            Ok(value) => {
                runtime.refresh_canonical_snapshot_v2();
                drop(runtime);
                self.invalidate_plan_discovery_cache();
                Ok(value)
            }
            Err(error) => {
                let failed_snapshot = runtime.snapshot();
                let rollback_snapshot =
                    rollback_snapshot_with_rejections(before_snapshot, &failed_snapshot);
                let rollback_snapshot_v2 = rollback_snapshot.to_canonical_snapshot_v2();
                runtime.replace(rollback_snapshot, rollback_snapshot_v2);
                Err(error)
            }
        }
    }
}

fn validate_transaction_input_shape(
    input: &CoordinationTransactionInput,
) -> std::result::Result<(), CoordinationTransactionError> {
    if input.mutations.is_empty() {
        return Err(CoordinationTransactionError::rejected(
            CoordinationTransactionValidationStage::InputShape,
            CoordinationTransactionRejectionCategory::InvalidInput,
            "empty_transaction",
            "coordination_transaction requires at least one staged mutation",
        ));
    }
    if let Some(intent_metadata) = input.intent_metadata.as_ref() {
        if !intent_metadata.is_object() {
            return Err(CoordinationTransactionError::rejected(
                CoordinationTransactionValidationStage::InputShape,
                CoordinationTransactionRejectionCategory::InvalidInput,
                "invalid_intent_metadata",
                "coordination_transaction intentMetadata must be an object when provided",
            ));
        }
    }
    if let Some(structured_transaction) = input.structured_transaction.as_ref() {
        if !structured_transaction.is_object() {
            return Err(CoordinationTransactionError::rejected(
                CoordinationTransactionValidationStage::InputShape,
                CoordinationTransactionRejectionCategory::InvalidInput,
                "invalid_structured_transaction",
                "coordination_transaction structuredTransaction must be an object when provided",
            ));
        }
    }

    let mut seen_plan_client_ids = BTreeSet::new();
    let mut seen_task_client_ids = BTreeSet::new();
    let mut seen_claim_client_ids = BTreeSet::new();
    let mut seen_artifact_client_ids = BTreeSet::new();
    for mutation in &input.mutations {
        match mutation {
            CoordinationTransactionMutation::PlanCreate {
                client_plan_id: Some(client_plan_id),
                ..
            } => {
                if !seen_plan_client_ids.insert(client_plan_id.clone()) {
                    return Err(CoordinationTransactionError::rejected(
                        CoordinationTransactionValidationStage::InputShape,
                        CoordinationTransactionRejectionCategory::InvalidInput,
                        "duplicate_plan_client_id",
                        format!(
                            "coordination transaction plan client id `{client_plan_id}` was used more than once"
                        ),
                    ));
                }
            }
            CoordinationTransactionMutation::TaskCreate {
                client_task_id: Some(client_task_id),
                ..
            } => {
                if !seen_task_client_ids.insert(client_task_id.clone()) {
                    return Err(CoordinationTransactionError::rejected(
                        CoordinationTransactionValidationStage::InputShape,
                        CoordinationTransactionRejectionCategory::InvalidInput,
                        "duplicate_task_client_id",
                        format!(
                            "coordination transaction task client id `{client_task_id}` was used more than once"
                        ),
                    ));
                }
            }
            CoordinationTransactionMutation::ClaimAcquire {
                client_claim_id: Some(client_claim_id),
                ..
            } => {
                if !seen_claim_client_ids.insert(client_claim_id.clone()) {
                    return Err(CoordinationTransactionError::rejected(
                        CoordinationTransactionValidationStage::InputShape,
                        CoordinationTransactionRejectionCategory::InvalidInput,
                        "duplicate_claim_client_id",
                        format!(
                            "coordination transaction claim client id `{client_claim_id}` was used more than once"
                        ),
                    ));
                }
            }
            CoordinationTransactionMutation::ArtifactPropose {
                client_artifact_id: Some(client_artifact_id),
                ..
            } => {
                if !seen_artifact_client_ids.insert(client_artifact_id.clone()) {
                    return Err(CoordinationTransactionError::rejected(
                        CoordinationTransactionValidationStage::InputShape,
                        CoordinationTransactionRejectionCategory::InvalidInput,
                        "duplicate_artifact_client_id",
                        format!(
                            "coordination transaction artifact client id `{client_artifact_id}` was used more than once"
                        ),
                    ));
                }
            }
            _ => {}
        }
    }

    Ok(())
}

fn parse_optimistic_preconditions(
    optimistic_preconditions: Option<&Value>,
) -> std::result::Result<
    Option<CoordinationTransactionOptimisticPreconditions>,
    CoordinationTransactionError,
> {
    let Some(value) = optimistic_preconditions else {
        return Ok(None);
    };
    let Value::Object(map) = value else {
        return Err(CoordinationTransactionError::rejected(
            CoordinationTransactionValidationStage::InputShape,
            CoordinationTransactionRejectionCategory::Unsupported,
            "unsupported_optimistic_preconditions",
            "coordination_transaction optimisticPreconditions must be an object when provided",
        ));
    };

    let mut expected_revision = None;
    let mut expected_event_count = None;
    let mut expected_last_event_id = None;
    for (key, value) in map {
        match key.as_str() {
            "expectedRevision" => {
                let Value::Number(number) = value else {
                    return Err(CoordinationTransactionError::rejected(
                        CoordinationTransactionValidationStage::InputShape,
                        CoordinationTransactionRejectionCategory::InvalidInput,
                        "invalid_expected_revision",
                        "coordination_transaction optimisticPreconditions.expectedRevision must be a non-negative integer",
                    ));
                };
                let Some(parsed) = number.as_u64() else {
                    return Err(CoordinationTransactionError::rejected(
                        CoordinationTransactionValidationStage::InputShape,
                        CoordinationTransactionRejectionCategory::InvalidInput,
                        "invalid_expected_revision",
                        "coordination_transaction optimisticPreconditions.expectedRevision must be a non-negative integer",
                    ));
                };
                expected_revision = Some(parsed);
            }
            "expectedEventCount" => {
                let Value::Number(number) = value else {
                    return Err(CoordinationTransactionError::rejected(
                        CoordinationTransactionValidationStage::InputShape,
                        CoordinationTransactionRejectionCategory::InvalidInput,
                        "invalid_expected_event_count",
                        "coordination_transaction optimisticPreconditions.expectedEventCount must be a non-negative integer",
                    ));
                };
                let Some(parsed) = number
                    .as_u64()
                    .and_then(|value| usize::try_from(value).ok())
                else {
                    return Err(CoordinationTransactionError::rejected(
                        CoordinationTransactionValidationStage::InputShape,
                        CoordinationTransactionRejectionCategory::InvalidInput,
                        "invalid_expected_event_count",
                        "coordination_transaction optimisticPreconditions.expectedEventCount must be a non-negative integer",
                    ));
                };
                expected_event_count = Some(parsed);
            }
            "expectedLastEventId" => {
                let Value::String(event_id) = value else {
                    return Err(CoordinationTransactionError::rejected(
                        CoordinationTransactionValidationStage::InputShape,
                        CoordinationTransactionRejectionCategory::InvalidInput,
                        "invalid_expected_last_event_id",
                        "coordination_transaction optimisticPreconditions.expectedLastEventId must be a string",
                    ));
                };
                expected_last_event_id = Some(EventId::new(event_id.clone()));
            }
            _ => {
                return Err(CoordinationTransactionError::rejected(
                    CoordinationTransactionValidationStage::InputShape,
                    CoordinationTransactionRejectionCategory::Unsupported,
                    "unsupported_optimistic_preconditions",
                    format!(
                        "coordination_transaction optimisticPreconditions field `{key}` is not supported yet"
                    ),
                ));
            }
        }
    }

    if expected_revision.is_none()
        && expected_event_count.is_none()
        && expected_last_event_id.is_none()
    {
        return Err(CoordinationTransactionError::rejected(
            CoordinationTransactionValidationStage::InputShape,
            CoordinationTransactionRejectionCategory::Unsupported,
            "unsupported_optimistic_preconditions",
            "coordination_transaction optimisticPreconditions must include at least one supported field",
        ));
    }

    Ok(Some(CoordinationTransactionOptimisticPreconditions {
        expected_revision,
        expected_event_count,
        expected_last_event_id,
    }))
}

fn validate_transaction_authorization(
    _input: &CoordinationTransactionInput,
) -> std::result::Result<(), CoordinationTransactionError> {
    Ok(())
}

fn validate_transaction_identity(
    coordination_runtime: &CoordinationRuntimeState,
    input: &CoordinationTransactionInput,
) -> std::result::Result<(), CoordinationTransactionError> {
    let mut declared_plan_client_ids = BTreeSet::new();
    let mut declared_task_client_ids = BTreeSet::new();
    let mut declared_claim_client_ids = BTreeSet::new();
    let mut declared_artifact_client_ids = BTreeSet::new();
    for mutation in &input.mutations {
        match mutation {
            CoordinationTransactionMutation::PlanCreate {
                client_plan_id: Some(client_plan_id),
                ..
            } => {
                declared_plan_client_ids.insert(client_plan_id.clone());
            }
            CoordinationTransactionMutation::TaskCreate {
                client_task_id: Some(client_task_id),
                ..
            } => {
                declared_task_client_ids.insert(client_task_id.clone());
            }
            CoordinationTransactionMutation::ClaimAcquire {
                client_claim_id: Some(client_claim_id),
                ..
            } => {
                declared_claim_client_ids.insert(client_claim_id.clone());
            }
            CoordinationTransactionMutation::ArtifactPropose {
                client_artifact_id: Some(client_artifact_id),
                ..
            } => {
                declared_artifact_client_ids.insert(client_artifact_id.clone());
            }
            _ => {}
        }
    }

    let mut seen_plan_client_ids = BTreeSet::new();
    let mut seen_task_client_ids = BTreeSet::new();
    let mut seen_claim_client_ids = BTreeSet::new();
    let mut seen_artifact_client_ids = BTreeSet::new();
    for mutation in &input.mutations {
        match mutation {
            CoordinationTransactionMutation::PlanCreate { client_plan_id, .. } => {
                if let Some(client_plan_id) = client_plan_id {
                    seen_plan_client_ids.insert(client_plan_id.clone());
                }
            }
            CoordinationTransactionMutation::PlanUpdate { plan, .. }
            | CoordinationTransactionMutation::PlanArchive { plan } => {
                validate_plan_ref(
                    coordination_runtime,
                    plan,
                    &seen_plan_client_ids,
                    &declared_plan_client_ids,
                )?;
            }
            CoordinationTransactionMutation::TaskCreate {
                client_task_id,
                plan,
                depends_on,
                coordination_depends_on,
                integrated_depends_on,
                ..
            } => {
                validate_plan_ref(
                    coordination_runtime,
                    plan,
                    &seen_plan_client_ids,
                    &declared_plan_client_ids,
                )?;
                validate_task_refs(
                    coordination_runtime,
                    depends_on,
                    &seen_task_client_ids,
                    &declared_task_client_ids,
                )?;
                validate_task_refs(
                    coordination_runtime,
                    coordination_depends_on,
                    &seen_task_client_ids,
                    &declared_task_client_ids,
                )?;
                validate_task_refs(
                    coordination_runtime,
                    integrated_depends_on,
                    &seen_task_client_ids,
                    &declared_task_client_ids,
                )?;
                if let Some(client_task_id) = client_task_id {
                    seen_task_client_ids.insert(client_task_id.clone());
                }
            }
            CoordinationTransactionMutation::TaskUpdate { task, .. } => {
                validate_task_ref(
                    coordination_runtime,
                    task,
                    &seen_task_client_ids,
                    &declared_task_client_ids,
                )?;
            }
            CoordinationTransactionMutation::DependencyCreate {
                task, depends_on, ..
            } => {
                validate_task_ref(
                    coordination_runtime,
                    task,
                    &seen_task_client_ids,
                    &declared_task_client_ids,
                )?;
                validate_task_ref(
                    coordination_runtime,
                    depends_on,
                    &seen_task_client_ids,
                    &declared_task_client_ids,
                )?;
            }
            CoordinationTransactionMutation::ClaimAcquire {
                client_claim_id,
                task,
                ..
            } => {
                if let Some(task) = task {
                    validate_task_ref(
                        coordination_runtime,
                        task,
                        &seen_task_client_ids,
                        &declared_task_client_ids,
                    )?;
                }
                if let Some(client_claim_id) = client_claim_id {
                    seen_claim_client_ids.insert(client_claim_id.clone());
                }
            }
            CoordinationTransactionMutation::ClaimRenew { claim, .. }
            | CoordinationTransactionMutation::ClaimRelease { claim, .. } => {
                validate_claim_ref(
                    coordination_runtime,
                    claim,
                    &seen_claim_client_ids,
                    &declared_claim_client_ids,
                )?;
            }
            CoordinationTransactionMutation::ArtifactPropose {
                client_artifact_id,
                task,
                ..
            } => {
                validate_task_ref(
                    coordination_runtime,
                    task,
                    &seen_task_client_ids,
                    &declared_task_client_ids,
                )?;
                if let Some(client_artifact_id) = client_artifact_id {
                    seen_artifact_client_ids.insert(client_artifact_id.clone());
                }
            }
            CoordinationTransactionMutation::ArtifactSupersede { artifact }
            | CoordinationTransactionMutation::ArtifactReview { artifact, .. } => {
                validate_artifact_ref(
                    coordination_runtime,
                    artifact,
                    &seen_artifact_client_ids,
                    &declared_artifact_client_ids,
                )?;
            }
            CoordinationTransactionMutation::TaskHandoff { task, .. }
            | CoordinationTransactionMutation::TaskHandoffAccept { task, .. }
            | CoordinationTransactionMutation::TaskResume { task, .. }
            | CoordinationTransactionMutation::TaskReclaim { task, .. } => {
                validate_task_ref(
                    coordination_runtime,
                    task,
                    &seen_task_client_ids,
                    &declared_task_client_ids,
                )?;
            }
        }
    }

    Ok(())
}

fn validate_transaction_conflict(
    coordination_runtime: &CoordinationRuntimeState,
    optimistic_preconditions: Option<&CoordinationTransactionOptimisticPreconditions>,
) -> std::result::Result<(), CoordinationTransactionError> {
    let Some(optimistic_preconditions) = optimistic_preconditions else {
        return Ok(());
    };
    let snapshot = coordination_runtime.snapshot();
    if let Some(expected_revision) = optimistic_preconditions.expected_revision {
        let actual_revision = u64::try_from(snapshot.events.len()).unwrap_or(u64::MAX);
        if actual_revision != expected_revision {
            return Err(CoordinationTransactionError::rejected(
                CoordinationTransactionValidationStage::Conflict,
                CoordinationTransactionRejectionCategory::Conflict,
                "stale_revision",
                format!(
                    "coordination transaction optimisticPreconditions.expectedRevision expected `{expected_revision}` but current revision is `{actual_revision}`"
                ),
            ));
        }
    }
    if let Some(expected_event_count) = optimistic_preconditions.expected_event_count {
        let actual_event_count = snapshot.events.len();
        if actual_event_count != expected_event_count {
            return Err(CoordinationTransactionError::rejected(
                CoordinationTransactionValidationStage::Conflict,
                CoordinationTransactionRejectionCategory::Conflict,
                "stale_event_count",
                format!(
                    "coordination transaction optimisticPreconditions.expectedEventCount expected `{expected_event_count}` but current event count is `{actual_event_count}`"
                ),
            ));
        }
    }
    if let Some(expected_last_event_id) = optimistic_preconditions.expected_last_event_id.as_ref() {
        let actual_last_event_id = snapshot.events.last().map(|event| event.meta.id.clone());
        if actual_last_event_id.as_ref() != Some(expected_last_event_id) {
            let actual_last_event_id_label = actual_last_event_id
                .as_ref()
                .map(|value| value.0.as_str())
                .unwrap_or("<none>");
            return Err(CoordinationTransactionError::rejected(
                CoordinationTransactionValidationStage::Conflict,
                CoordinationTransactionRejectionCategory::Conflict,
                "stale_last_event_id",
                format!(
                    "coordination transaction optimisticPreconditions.expectedLastEventId expected `{}` but current last event id is `{}`",
                    expected_last_event_id.0,
                    actual_last_event_id_label
                ),
            ));
        }
    }
    Ok(())
}

fn validate_plan_ref(
    coordination_runtime: &CoordinationRuntimeState,
    plan: &CoordinationTransactionPlanRef,
    seen_plan_client_ids: &BTreeSet<String>,
    declared_plan_client_ids: &BTreeSet<String>,
) -> std::result::Result<(), CoordinationTransactionError> {
    match plan {
        CoordinationTransactionPlanRef::Id(plan_id) => {
            if coordination_runtime.plan(plan_id).is_none() {
                return Err(CoordinationTransactionError::rejected(
                    CoordinationTransactionValidationStage::ObjectIdentity,
                    CoordinationTransactionRejectionCategory::NotFound,
                    "unknown_plan",
                    format!("unknown plan `{}`", plan_id.0),
                ));
            }
            Ok(())
        }
        CoordinationTransactionPlanRef::ClientId(client_id) => {
            if seen_plan_client_ids.contains(client_id) {
                return Ok(());
            }
            let reason_code = if declared_plan_client_ids.contains(client_id) {
                "forward_plan_client_reference"
            } else {
                "unknown_plan_client_id"
            };
            let message = if declared_plan_client_ids.contains(client_id) {
                format!(
                    "coordination transaction plan client id `{client_id}` was referenced before it was created"
                )
            } else {
                format!("unknown coordination transaction plan client id `{client_id}`")
            };
            Err(CoordinationTransactionError::rejected(
                CoordinationTransactionValidationStage::ObjectIdentity,
                CoordinationTransactionRejectionCategory::NotFound,
                reason_code,
                message,
            ))
        }
    }
}

fn validate_task_ref(
    coordination_runtime: &CoordinationRuntimeState,
    task: &CoordinationTransactionTaskRef,
    seen_task_client_ids: &BTreeSet<String>,
    declared_task_client_ids: &BTreeSet<String>,
) -> std::result::Result<(), CoordinationTransactionError> {
    match task {
        CoordinationTransactionTaskRef::Id(task_id) => {
            if coordination_runtime.task(task_id).is_none() {
                return Err(CoordinationTransactionError::rejected(
                    CoordinationTransactionValidationStage::ObjectIdentity,
                    CoordinationTransactionRejectionCategory::NotFound,
                    "unknown_task",
                    format!("unknown task `{}`", task_id.0),
                ));
            }
            Ok(())
        }
        CoordinationTransactionTaskRef::ClientId(client_id) => {
            if seen_task_client_ids.contains(client_id) {
                return Ok(());
            }
            let reason_code = if declared_task_client_ids.contains(client_id) {
                "forward_task_client_reference"
            } else {
                "unknown_task_client_id"
            };
            let message = if declared_task_client_ids.contains(client_id) {
                format!(
                    "coordination transaction task client id `{client_id}` was referenced before it was created"
                )
            } else {
                format!("unknown coordination transaction task client id `{client_id}`")
            };
            Err(CoordinationTransactionError::rejected(
                CoordinationTransactionValidationStage::ObjectIdentity,
                CoordinationTransactionRejectionCategory::NotFound,
                reason_code,
                message,
            ))
        }
    }
}

fn validate_task_refs(
    coordination_runtime: &CoordinationRuntimeState,
    refs: &[CoordinationTransactionTaskRef],
    seen_task_client_ids: &BTreeSet<String>,
    declared_task_client_ids: &BTreeSet<String>,
) -> std::result::Result<(), CoordinationTransactionError> {
    for task_ref in refs {
        validate_task_ref(
            coordination_runtime,
            task_ref,
            seen_task_client_ids,
            declared_task_client_ids,
        )?;
    }
    Ok(())
}

fn validate_claim_ref(
    coordination_runtime: &CoordinationRuntimeState,
    claim: &CoordinationTransactionClaimRef,
    seen_claim_client_ids: &BTreeSet<String>,
    declared_claim_client_ids: &BTreeSet<String>,
) -> std::result::Result<(), CoordinationTransactionError> {
    match claim {
        CoordinationTransactionClaimRef::Id(claim_id) => {
            if coordination_runtime
                .claims_in_scope(None)
                .into_iter()
                .all(|claim| claim.id != *claim_id)
            {
                return Err(CoordinationTransactionError::rejected(
                    CoordinationTransactionValidationStage::ObjectIdentity,
                    CoordinationTransactionRejectionCategory::NotFound,
                    "unknown_claim",
                    format!("unknown claim `{}`", claim_id.0),
                ));
            }
            Ok(())
        }
        CoordinationTransactionClaimRef::ClientId(client_id) => {
            if seen_claim_client_ids.contains(client_id) {
                return Ok(());
            }
            let reason_code = if declared_claim_client_ids.contains(client_id) {
                "forward_claim_client_reference"
            } else {
                "unknown_claim_client_id"
            };
            let message = if declared_claim_client_ids.contains(client_id) {
                format!(
                    "coordination transaction claim client id `{client_id}` was referenced before it was created"
                )
            } else {
                format!("unknown coordination transaction claim client id `{client_id}`")
            };
            Err(CoordinationTransactionError::rejected(
                CoordinationTransactionValidationStage::ObjectIdentity,
                CoordinationTransactionRejectionCategory::NotFound,
                reason_code,
                message,
            ))
        }
    }
}

fn validate_artifact_ref(
    coordination_runtime: &CoordinationRuntimeState,
    artifact: &CoordinationTransactionArtifactRef,
    seen_artifact_client_ids: &BTreeSet<String>,
    declared_artifact_client_ids: &BTreeSet<String>,
) -> std::result::Result<(), CoordinationTransactionError> {
    match artifact {
        CoordinationTransactionArtifactRef::Id(artifact_id) => {
            if coordination_runtime.artifact(artifact_id).is_none() {
                return Err(CoordinationTransactionError::rejected(
                    CoordinationTransactionValidationStage::ObjectIdentity,
                    CoordinationTransactionRejectionCategory::NotFound,
                    "unknown_artifact",
                    format!("unknown artifact `{}`", artifact_id.0),
                ));
            }
            Ok(())
        }
        CoordinationTransactionArtifactRef::ClientId(client_id) => {
            if seen_artifact_client_ids.contains(client_id) {
                return Ok(());
            }
            let reason_code = if declared_artifact_client_ids.contains(client_id) {
                "forward_artifact_client_reference"
            } else {
                "unknown_artifact_client_id"
            };
            let message = if declared_artifact_client_ids.contains(client_id) {
                format!(
                    "coordination transaction artifact client id `{client_id}` was referenced before it was created"
                )
            } else {
                format!("unknown coordination transaction artifact client id `{client_id}`")
            };
            Err(CoordinationTransactionError::rejected(
                CoordinationTransactionValidationStage::ObjectIdentity,
                CoordinationTransactionRejectionCategory::NotFound,
                reason_code,
                message,
            ))
        }
    }
}

fn apply_coordination_transaction(
    coordination_runtime: &mut CoordinationRuntimeState,
    meta: EventMeta,
    input: CoordinationTransactionInput,
) -> Result<CoordinationTransactionResult> {
    let CoordinationTransactionInput {
        mutations,
        intent_metadata,
        structured_transaction,
        optimistic_preconditions: _,
    } = input;
    let before_event_len = coordination_runtime.snapshot().events.len();
    let mut seen_plan_client_ids = BTreeSet::new();
    let mut seen_task_client_ids = BTreeSet::new();
    let mut seen_claim_client_ids = BTreeSet::new();
    let mut seen_artifact_client_ids = BTreeSet::new();
    let mut plan_ids_by_client_id = BTreeMap::new();
    let mut task_ids_by_client_id = BTreeMap::new();
    let mut claim_ids_by_client_id = BTreeMap::new();
    let mut artifact_ids_by_client_id = BTreeMap::new();
    let mut touched_plan_ids = Vec::new();
    let mut touched_task_ids = Vec::new();
    let mut touched_claim_ids = Vec::new();
    let mut touched_artifact_ids = Vec::new();
    let mut touched_review_ids = Vec::new();
    let mut touched_plan_seen = BTreeSet::new();
    let mut touched_task_seen = BTreeSet::new();
    let mut touched_claim_seen = BTreeSet::new();
    let mut touched_artifact_seen = BTreeSet::new();
    let mut touched_review_seen = BTreeSet::new();

    for (index, mutation) in mutations.into_iter().enumerate() {
        let step_meta = transaction_meta(&meta, index, mutation.action_tag());
        match mutation {
            CoordinationTransactionMutation::PlanCreate {
                client_plan_id,
                title,
                goal,
                status,
                policy,
                scheduling,
                spec_refs,
            } => {
                if let Some(client_plan_id) = client_plan_id.as_deref() {
                    ensure_unique_client_id(
                        &mut seen_plan_client_ids,
                        client_plan_id,
                        "coordination transaction plan",
                    )?;
                }
                let (plan_id, _) = coordination_runtime.create_plan(
                    step_meta.clone(),
                    PlanCreateInput {
                        title,
                        goal,
                        status,
                        policy,
                        spec_refs,
                    },
                )?;
                if let Some(scheduling) = scheduling {
                    coordination_runtime.set_plan_scheduling(
                        transaction_meta(&step_meta, index, "plan_scheduling"),
                        plan_id.clone(),
                        scheduling,
                    )?;
                }
                if let Some(client_plan_id) = client_plan_id {
                    plan_ids_by_client_id.insert(client_plan_id, plan_id.clone());
                }
                touch_plan(&mut touched_plan_ids, &mut touched_plan_seen, plan_id);
            }
            CoordinationTransactionMutation::PlanUpdate {
                plan,
                title,
                goal,
                status,
                policy,
                scheduling,
                spec_refs,
            } => {
                let plan_id = resolve_plan_ref(&plan_ids_by_client_id, &plan)?;
                let existing_plan = coordination_runtime
                    .plan(&plan_id)
                    .ok_or_else(|| anyhow!("unknown plan `{}`", plan_id.0))?;
                if title.is_some() || goal.is_some() || status.is_some() || policy.is_some() {
                    coordination_runtime.update_plan(
                        step_meta.clone(),
                        PlanUpdateInput {
                            plan_id: plan_id.clone(),
                            title,
                            goal,
                            status,
                            policy: policy.map(|patch| merge_policy(existing_plan.policy, patch)),
                            spec_refs,
                        },
                    )?;
                }
                if let Some(scheduling) = scheduling {
                    coordination_runtime.set_plan_scheduling(
                        transaction_meta(&step_meta, index, "plan_scheduling"),
                        plan_id.clone(),
                        merge_plan_scheduling(existing_plan.scheduling, scheduling),
                    )?;
                }
                touch_plan(&mut touched_plan_ids, &mut touched_plan_seen, plan_id);
            }
            CoordinationTransactionMutation::PlanArchive { plan } => {
                let plan_id = resolve_plan_ref(&plan_ids_by_client_id, &plan)?;
                let existing_plan = coordination_runtime
                    .plan(&plan_id)
                    .ok_or_else(|| anyhow!("unknown plan `{}`", plan_id.0))?;
                let mut archive_meta = step_meta.clone();
                if !matches!(
                    existing_plan.status,
                    PlanStatus::Archived | PlanStatus::Completed | PlanStatus::Abandoned
                ) {
                    coordination_runtime.update_plan(
                        archive_meta.clone(),
                        PlanUpdateInput {
                            plan_id: plan_id.clone(),
                            title: None,
                            goal: None,
                            status: Some(PlanStatus::Abandoned),
                            policy: None,
                            spec_refs: None,
                        },
                    )?;
                    archive_meta = EventMeta {
                        id: EventId::new(format!("{}:archive", step_meta.id.0)),
                        ts: meta.ts,
                        causation: Some(step_meta.id.clone()),
                        ..meta.clone()
                    };
                }
                if existing_plan.status != PlanStatus::Archived {
                    coordination_runtime.update_plan(
                        archive_meta,
                        PlanUpdateInput {
                            plan_id: plan_id.clone(),
                            title: None,
                            goal: None,
                            status: Some(PlanStatus::Archived),
                            policy: None,
                            spec_refs: None,
                        },
                    )?;
                }
                touch_plan(&mut touched_plan_ids, &mut touched_plan_seen, plan_id);
            }
            CoordinationTransactionMutation::TaskCreate {
                client_task_id,
                plan,
                title,
                status,
                assignee,
                session,
                worktree_id,
                branch_ref,
                anchors,
                depends_on,
                coordination_depends_on,
                integrated_depends_on,
                acceptance,
                base_revision,
                spec_refs,
                artifact_requirements,
                review_requirements,
            } => {
                if let Some(client_task_id) = client_task_id.as_deref() {
                    ensure_unique_client_id(
                        &mut seen_task_client_ids,
                        client_task_id,
                        "coordination transaction task",
                    )?;
                }
                let plan_id = resolve_plan_ref(&plan_ids_by_client_id, &plan)?;
                let plan = coordination_runtime
                    .plan(&plan_id)
                    .ok_or_else(|| anyhow!("unknown plan `{}`", plan_id.0))?;
                reject_git_execution_bypass_on_create(&plan, status)?;
                let (task_id, _) = coordination_runtime.create_task(
                    step_meta.clone(),
                    TaskCreateInput {
                        plan_id: plan_id.clone(),
                        title,
                        status,
                        assignee,
                        session,
                        worktree_id,
                        branch_ref,
                        anchors,
                        depends_on: resolve_task_refs(&task_ids_by_client_id, &depends_on)?,
                        coordination_depends_on: resolve_task_refs(
                            &task_ids_by_client_id,
                            &coordination_depends_on,
                        )?,
                        integrated_depends_on: resolve_task_refs(
                            &task_ids_by_client_id,
                            &integrated_depends_on,
                        )?,
                        acceptance,
                        base_revision: base_revision.clone(),
                        spec_refs,
                        artifact_requirements,
                        review_requirements,
                    },
                )?;
                if let Some(client_task_id) = client_task_id {
                    task_ids_by_client_id.insert(client_task_id, task_id.clone());
                }
                touch_plan(
                    &mut touched_plan_ids,
                    &mut touched_plan_seen,
                    plan_id.clone(),
                );
                touch_task(
                    &mut touched_task_ids,
                    &mut touched_task_seen,
                    task_id.clone(),
                );
            }
            CoordinationTransactionMutation::TaskUpdate {
                task,
                status,
                published_task_status,
                git_execution,
                assignee,
                session,
                worktree_id,
                branch_ref,
                title,
                summary,
                anchors,
                bindings,
                depends_on,
                acceptance,
                validation_refs,
                base_revision,
                priority,
                tags,
                completion_context,
                spec_refs,
                artifact_requirements,
                review_requirements,
            } => {
                let task_id = resolve_task_ref(&task_ids_by_client_id, &task)?;
                let existing_task = coordination_runtime
                    .task(&task_id)
                    .ok_or_else(|| anyhow!("unknown coordination task `{}`", task_id.0))?;
                let update_input = TaskUpdateInput {
                    task_id: task_id.clone(),
                    kind: None,
                    status,
                    published_task_status,
                    git_execution,
                    assignee,
                    session,
                    worktree_id,
                    branch_ref,
                    title,
                    summary,
                    anchors,
                    bindings,
                    depends_on: depends_on
                        .map(|refs| resolve_task_refs(&task_ids_by_client_id, &refs))
                        .transpose()?,
                    coordination_depends_on: None,
                    integrated_depends_on: None,
                    acceptance,
                    validation_refs,
                    is_abstract: None,
                    base_revision: Some(base_revision.clone()),
                    priority,
                    tags,
                    completion_context,
                    spec_refs,
                    artifact_requirements,
                    review_requirements,
                };
                if is_authoritative_git_execution_only_update(&update_input) {
                    coordination_runtime.update_task_authoritative_only(
                        step_meta,
                        update_input,
                        base_revision,
                        meta.ts,
                    )?;
                } else {
                    coordination_runtime.update_task(
                        step_meta,
                        update_input,
                        base_revision,
                        meta.ts,
                    )?;
                }
                touch_plan(
                    &mut touched_plan_ids,
                    &mut touched_plan_seen,
                    existing_task.plan.clone(),
                );
                touch_task(&mut touched_task_ids, &mut touched_task_seen, task_id);
            }
            CoordinationTransactionMutation::DependencyCreate {
                task,
                depends_on,
                kind,
                base_revision,
            } => {
                let task_id = resolve_task_ref(&task_ids_by_client_id, &task)?;
                let dependency_id = resolve_task_ref(&task_ids_by_client_id, &depends_on)?;
                let existing_task = coordination_runtime
                    .task(&task_id)
                    .ok_or_else(|| anyhow!("unknown coordination task `{}`", task_id.0))?;
                let mut direct = existing_task.depends_on.clone();
                let mut coordination = existing_task.coordination_depends_on.clone();
                let mut integrated = existing_task.integrated_depends_on.clone();
                match kind {
                    CoordinationDependencyKind::DependsOn => {
                        push_unique_dependency(&mut direct, dependency_id.clone())
                    }
                    CoordinationDependencyKind::CoordinationDependsOn => {
                        push_unique_dependency(&mut coordination, dependency_id.clone())
                    }
                    CoordinationDependencyKind::IntegratedDependsOn => {
                        push_unique_dependency(&mut integrated, dependency_id.clone())
                    }
                }
                coordination_runtime.update_task(
                    step_meta,
                    TaskUpdateInput {
                        task_id: task_id.clone(),
                        kind: None,
                        status: None,
                        published_task_status: None,
                        git_execution: None,
                        assignee: None,
                        session: None,
                        worktree_id: None,
                        branch_ref: None,
                        title: None,
                        summary: None,
                        anchors: None,
                        bindings: None,
                        depends_on: Some(direct),
                        coordination_depends_on: Some(coordination),
                        integrated_depends_on: Some(integrated),
                        acceptance: None,
                        validation_refs: None,
                        is_abstract: None,
                        base_revision: Some(base_revision.clone()),
                        priority: None,
                        tags: None,
                        completion_context: None,
                        spec_refs: None,
                        artifact_requirements: None,
                        review_requirements: None,
                    },
                    base_revision,
                    meta.ts,
                )?;
                touch_task(&mut touched_task_ids, &mut touched_task_seen, task_id);
            }
            CoordinationTransactionMutation::ClaimAcquire {
                client_claim_id,
                task,
                anchors,
                capability,
                mode,
                ttl_seconds,
                agent,
                session,
                worktree_id,
                branch_ref,
                base_revision,
                current_revision,
            } => {
                if let Some(client_claim_id) = client_claim_id.as_deref() {
                    ensure_unique_client_id(
                        &mut seen_claim_client_ids,
                        client_claim_id,
                        "coordination transaction claim",
                    )?;
                }
                let (claim_id, _conflicts, claim) = coordination_runtime.acquire_claim(
                    step_meta,
                    session,
                    prism_coordination::ClaimAcquireInput {
                        task_id: task
                            .as_ref()
                            .map(|task| resolve_task_ref(&task_ids_by_client_id, task))
                            .transpose()?,
                        anchors,
                        capability,
                        mode,
                        ttl_seconds,
                        base_revision,
                        current_revision,
                        agent,
                        worktree_id,
                        branch_ref,
                    },
                )?;
                if let Some(client_claim_id) = client_claim_id {
                    if let Some(claim_id) = claim_id.as_ref() {
                        claim_ids_by_client_id.insert(client_claim_id, claim_id.clone());
                    }
                }
                if let Some(claim_id) = claim_id {
                    touch_claim(
                        &mut touched_claim_ids,
                        &mut touched_claim_seen,
                        claim_id.clone(),
                    );
                }
                if let Some(task_id) = claim.and_then(|claim| claim.task) {
                    touch_task(&mut touched_task_ids, &mut touched_task_seen, task_id);
                }
            }
            CoordinationTransactionMutation::ClaimRenew {
                claim,
                session,
                ttl_seconds,
            } => {
                let claim_id = resolve_claim_ref(&claim_ids_by_client_id, &claim)?;
                let renewed = coordination_runtime.renew_claim(
                    step_meta,
                    &session,
                    &claim_id,
                    ttl_seconds,
                    "coordination_transaction",
                )?;
                touch_claim(&mut touched_claim_ids, &mut touched_claim_seen, claim_id);
                if let Some(task_id) = renewed.task {
                    touch_task(&mut touched_task_ids, &mut touched_task_seen, task_id);
                }
            }
            CoordinationTransactionMutation::ClaimRelease { claim, session } => {
                let claim_id = resolve_claim_ref(&claim_ids_by_client_id, &claim)?;
                let released = coordination_runtime.release_claim(
                    step_meta,
                    &session,
                    &claim_id,
                )?;
                touch_claim(&mut touched_claim_ids, &mut touched_claim_seen, claim_id);
                if let Some(task_id) = released.task {
                    touch_task(&mut touched_task_ids, &mut touched_task_seen, task_id);
                }
            }
            CoordinationTransactionMutation::ArtifactPropose {
                client_artifact_id,
                task,
                artifact_requirement_id,
                anchors,
                diff_ref,
                evidence,
                required_validations,
                validated_checks,
                risk_score,
                base_revision,
                current_revision,
            } => {
                if let Some(client_artifact_id) = client_artifact_id.as_deref() {
                    ensure_unique_client_id(
                        &mut seen_artifact_client_ids,
                        client_artifact_id,
                        "coordination transaction artifact",
                    )?;
                }
                let task_id = resolve_task_ref(&task_ids_by_client_id, &task)?;
                let task_record = coordination_runtime
                    .task(&task_id)
                    .ok_or_else(|| anyhow!("unknown coordination task `{}`", task_id.0))?;
                let (artifact_id, artifact) = coordination_runtime.propose_artifact(
                    step_meta,
                    ArtifactProposeInput {
                        task_id: task_id.clone(),
                        artifact_requirement_id,
                        anchors: anchors.unwrap_or_else(|| task_record.anchors.clone()),
                        diff_ref,
                        evidence,
                        base_revision,
                        current_revision,
                        required_validations,
                        validated_checks,
                        risk_score,
                        worktree_id: task_record.worktree_id.clone(),
                        branch_ref: task_record.branch_ref.clone(),
                    },
                )?;
                if let Some(client_artifact_id) = client_artifact_id {
                    artifact_ids_by_client_id.insert(client_artifact_id, artifact_id.clone());
                }
                touch_artifact(
                    &mut touched_artifact_ids,
                    &mut touched_artifact_seen,
                    artifact_id,
                );
                touch_task(&mut touched_task_ids, &mut touched_task_seen, artifact.task.clone());
            }
            CoordinationTransactionMutation::ArtifactSupersede { artifact } => {
                let artifact_id = resolve_artifact_ref(&artifact_ids_by_client_id, &artifact)?;
                let artifact = coordination_runtime.supersede_artifact(
                    step_meta,
                    ArtifactSupersedeInput { artifact_id: artifact_id.clone() },
                )?;
                touch_artifact(
                    &mut touched_artifact_ids,
                    &mut touched_artifact_seen,
                    artifact_id,
                );
                touch_task(&mut touched_task_ids, &mut touched_task_seen, artifact.task);
            }
            CoordinationTransactionMutation::ArtifactReview {
                artifact,
                review_requirement_id,
                verdict,
                summary,
                required_validations,
                validated_checks,
                risk_score,
                current_revision,
            } => {
                let artifact_id = resolve_artifact_ref(&artifact_ids_by_client_id, &artifact)?;
                let (review_id, _review, artifact) = coordination_runtime.review_artifact(
                    step_meta,
                    ArtifactReviewInput {
                        artifact_id: artifact_id.clone(),
                        review_requirement_id,
                        verdict,
                        summary,
                        required_validations,
                        validated_checks,
                        risk_score,
                    },
                    current_revision,
                )?;
                touch_review(&mut touched_review_ids, &mut touched_review_seen, review_id);
                touch_artifact(
                    &mut touched_artifact_ids,
                    &mut touched_artifact_seen,
                    artifact_id,
                );
                touch_task(&mut touched_task_ids, &mut touched_task_seen, artifact.task);
            }
            CoordinationTransactionMutation::TaskHandoff {
                task,
                to_agent,
                summary,
                base_revision,
            } => {
                let task_id = resolve_task_ref(&task_ids_by_client_id, &task)?;
                let updated = coordination_runtime.handoff(
                    step_meta,
                    prism_coordination::HandoffInput {
                        task_id: task_id.clone(),
                        to_agent,
                        summary,
                        base_revision: base_revision.clone(),
                    },
                    base_revision,
                )?;
                touch_plan(
                    &mut touched_plan_ids,
                    &mut touched_plan_seen,
                    updated.plan.clone(),
                );
                touch_task(&mut touched_task_ids, &mut touched_task_seen, task_id);
            }
            CoordinationTransactionMutation::TaskHandoffAccept {
                task,
                agent,
                worktree_id,
                branch_ref,
            } => {
                let task_id = resolve_task_ref(&task_ids_by_client_id, &task)?;
                let updated = coordination_runtime.accept_handoff(
                    step_meta,
                    prism_coordination::HandoffAcceptInput {
                        task_id: task_id.clone(),
                        agent,
                        worktree_id,
                        branch_ref,
                    },
                )?;
                touch_plan(
                    &mut touched_plan_ids,
                    &mut touched_plan_seen,
                    updated.plan.clone(),
                );
                touch_task(&mut touched_task_ids, &mut touched_task_seen, task_id);
            }
            CoordinationTransactionMutation::TaskResume {
                task,
                agent,
                worktree_id,
                branch_ref,
            } => {
                let task_id = resolve_task_ref(&task_ids_by_client_id, &task)?;
                let updated = coordination_runtime.resume_task(
                    step_meta,
                    prism_coordination::TaskResumeInput {
                        task_id: task_id.clone(),
                        agent,
                        worktree_id,
                        branch_ref,
                    },
                )?;
                touch_plan(
                    &mut touched_plan_ids,
                    &mut touched_plan_seen,
                    updated.plan.clone(),
                );
                touch_task(&mut touched_task_ids, &mut touched_task_seen, task_id);
            }
            CoordinationTransactionMutation::TaskReclaim {
                task,
                agent,
                worktree_id,
                branch_ref,
            } => {
                let task_id = resolve_task_ref(&task_ids_by_client_id, &task)?;
                let updated = coordination_runtime.reclaim_task(
                    step_meta,
                    prism_coordination::TaskReclaimInput {
                        task_id: task_id.clone(),
                        agent,
                        worktree_id,
                        branch_ref,
                    },
                )?;
                touch_plan(
                    &mut touched_plan_ids,
                    &mut touched_plan_seen,
                    updated.plan.clone(),
                );
                touch_task(&mut touched_task_ids, &mut touched_task_seen, task_id);
            }
        }
    }

    if let Some(intent_metadata) = intent_metadata.as_ref() {
        coordination_runtime.annotate_recent_events_with_metadata(
            before_event_len,
            "transactionIntent",
            intent_metadata.clone(),
        );
    }
    if let Some(structured_transaction) = structured_transaction.as_ref() {
        coordination_runtime.annotate_recent_events_with_metadata(
            before_event_len,
            "transactionStructure",
            structured_transaction.clone(),
        );
    }

    let committed_events = coordination_runtime.snapshot().events;
    let committed_event_ids = committed_events
        .iter()
        .skip(before_event_len)
        .map(|event| event.meta.id.clone())
        .collect::<Vec<_>>();
    let committed_at = committed_events
        .iter()
        .skip(before_event_len)
        .map(|event| event.meta.ts)
        .max();

    Ok(CoordinationTransactionResult {
        outcome: CoordinationTransactionOutcome::Committed,
        commit: CoordinationTransactionCommitMetadata {
            event_count: committed_event_ids.len(),
            last_event_id: committed_event_ids.last().cloned(),
            event_ids: committed_event_ids,
            committed_at,
        },
        authority_version: CoordinationTransactionAuthorityVersion {
            total_event_count: committed_events.len(),
            last_event_id: committed_events.last().map(|event| event.meta.id.clone()),
            committed_at: committed_events.last().map(|event| event.meta.ts),
        },
        intent_metadata,
        structured_transaction,
        plan_ids_by_client_id,
        task_ids_by_client_id,
        claim_ids_by_client_id,
        artifact_ids_by_client_id,
        touched_plan_ids,
        touched_task_ids,
        touched_claim_ids,
        touched_artifact_ids,
        touched_review_ids,
    })
}

fn transaction_meta(meta: &EventMeta, index: usize, action: &str) -> EventMeta {
    let mut derived = meta.clone();
    derived.id = EventId::new(format!("{}:tx:{index}:{action}", meta.id.0));
    derived
}

fn ensure_unique_client_id(
    seen: &mut BTreeSet<String>,
    client_id: &str,
    entity_name: &str,
) -> Result<()> {
    if client_id.is_empty() {
        return Err(anyhow!("{entity_name} client ids must be non-empty"));
    }
    if !seen.insert(client_id.to_string()) {
        return Err(anyhow!(
            "{entity_name} client id `{client_id}` is duplicated"
        ));
    }
    Ok(())
}

fn resolve_plan_ref(
    plan_ids_by_client_id: &BTreeMap<String, PlanId>,
    plan: &CoordinationTransactionPlanRef,
) -> Result<PlanId> {
    match plan {
        CoordinationTransactionPlanRef::Id(plan_id) => Ok(plan_id.clone()),
        CoordinationTransactionPlanRef::ClientId(client_id) => plan_ids_by_client_id
            .get(client_id)
            .cloned()
            .ok_or_else(|| {
                anyhow!("unknown coordination transaction plan client id `{client_id}`")
            }),
    }
}

fn resolve_task_ref(
    task_ids_by_client_id: &BTreeMap<String, CoordinationTaskId>,
    task: &CoordinationTransactionTaskRef,
) -> Result<CoordinationTaskId> {
    match task {
        CoordinationTransactionTaskRef::Id(task_id) => Ok(task_id.clone()),
        CoordinationTransactionTaskRef::ClientId(client_id) => task_ids_by_client_id
            .get(client_id)
            .cloned()
            .ok_or_else(|| {
                anyhow!("unknown coordination transaction task client id `{client_id}`")
            }),
    }
}

fn resolve_task_refs(
    task_ids_by_client_id: &BTreeMap<String, CoordinationTaskId>,
    refs: &[CoordinationTransactionTaskRef],
) -> Result<Vec<CoordinationTaskId>> {
    refs.iter()
        .map(|task_ref| resolve_task_ref(task_ids_by_client_id, task_ref))
        .collect()
}

fn resolve_claim_ref(
    claim_ids_by_client_id: &BTreeMap<String, ClaimId>,
    claim: &CoordinationTransactionClaimRef,
) -> Result<ClaimId> {
    match claim {
        CoordinationTransactionClaimRef::Id(claim_id) => Ok(claim_id.clone()),
        CoordinationTransactionClaimRef::ClientId(client_id) => claim_ids_by_client_id
            .get(client_id)
            .cloned()
            .ok_or_else(|| {
                anyhow!("unknown coordination transaction claim client id `{client_id}`")
            }),
    }
}

fn resolve_artifact_ref(
    artifact_ids_by_client_id: &BTreeMap<String, ArtifactId>,
    artifact: &CoordinationTransactionArtifactRef,
) -> Result<ArtifactId> {
    match artifact {
        CoordinationTransactionArtifactRef::Id(artifact_id) => Ok(artifact_id.clone()),
        CoordinationTransactionArtifactRef::ClientId(client_id) => artifact_ids_by_client_id
            .get(client_id)
            .cloned()
            .ok_or_else(|| {
                anyhow!("unknown coordination transaction artifact client id `{client_id}`")
            }),
    }
}

fn touch_plan(
    touched_plan_ids: &mut Vec<PlanId>,
    touched_plan_seen: &mut BTreeSet<String>,
    plan_id: PlanId,
) {
    if touched_plan_seen.insert(plan_id.0.to_string()) {
        touched_plan_ids.push(plan_id);
    }
}

fn touch_task(
    touched_task_ids: &mut Vec<CoordinationTaskId>,
    touched_task_seen: &mut BTreeSet<String>,
    task_id: CoordinationTaskId,
) {
    if touched_task_seen.insert(task_id.0.to_string()) {
        touched_task_ids.push(task_id);
    }
}

fn touch_claim(
    touched_claim_ids: &mut Vec<ClaimId>,
    touched_claim_seen: &mut BTreeSet<String>,
    claim_id: ClaimId,
) {
    if touched_claim_seen.insert(claim_id.0.to_string()) {
        touched_claim_ids.push(claim_id);
    }
}

fn touch_artifact(
    touched_artifact_ids: &mut Vec<ArtifactId>,
    touched_artifact_seen: &mut BTreeSet<String>,
    artifact_id: ArtifactId,
) {
    if touched_artifact_seen.insert(artifact_id.0.to_string()) {
        touched_artifact_ids.push(artifact_id);
    }
}

fn touch_review(
    touched_review_ids: &mut Vec<ReviewId>,
    touched_review_seen: &mut BTreeSet<String>,
    review_id: ReviewId,
) {
    if touched_review_seen.insert(review_id.0.to_string()) {
        touched_review_ids.push(review_id);
    }
}

fn push_unique_dependency(
    dependencies: &mut Vec<CoordinationTaskId>,
    dependency_id: CoordinationTaskId,
) {
    if dependencies
        .iter()
        .all(|existing| existing != &dependency_id)
    {
        dependencies.push(dependency_id);
    }
}

fn is_authoritative_git_execution_only_update(input: &TaskUpdateInput) -> bool {
    input.git_execution.is_some()
        && input.status.is_none()
        && input.published_task_status.is_none()
        && input.assignee.is_none()
        && input.session.is_none()
        && input.worktree_id.is_none()
        && input.branch_ref.is_none()
        && input.title.is_none()
        && input.summary.is_none()
        && input.anchors.is_none()
        && input.bindings.is_none()
        && input.depends_on.is_none()
        && input.coordination_depends_on.is_none()
        && input.integrated_depends_on.is_none()
        && input.acceptance.is_none()
        && input.validation_refs.is_none()
        && input.is_abstract.is_none()
        && input.priority.is_none()
        && input.tags.is_none()
        && input.completion_context.is_none()
}

fn merge_policy(
    mut existing: CoordinationPolicy,
    patch: CoordinationTransactionPolicyPatch,
) -> CoordinationPolicy {
    if let Some(value) = patch.default_claim_mode {
        existing.default_claim_mode = value;
    }
    if let Some(value) = patch.max_parallel_editors_per_anchor {
        existing.max_parallel_editors_per_anchor = value;
    }
    if let Some(value) = patch.require_review_for_completion {
        existing.require_review_for_completion = value;
    }
    if let Some(value) = patch.require_validation_for_completion {
        existing.require_validation_for_completion = value;
    }
    if let Some(value) = patch.stale_after_graph_change {
        existing.stale_after_graph_change = value;
    }
    if let Some(value) = patch.review_required_above_risk_score {
        existing.review_required_above_risk_score = Some(value);
    }
    if let Some(value) = patch.lease_stale_after_seconds {
        existing.lease_stale_after_seconds = value;
    }
    if let Some(value) = patch.lease_expires_after_seconds {
        existing.lease_expires_after_seconds = value;
    }
    if let Some(value) = patch.lease_renewal_mode {
        existing.lease_renewal_mode = value;
    }
    if let Some(value) = patch.git_execution {
        if let Some(start_mode) = value.start_mode {
            existing.git_execution.start_mode = start_mode;
        }
        if let Some(completion_mode) = value.completion_mode {
            existing.git_execution.completion_mode = completion_mode;
        }
        if let Some(integration_mode) = value.integration_mode {
            existing.git_execution.integration_mode = integration_mode;
        }
        if let Some(target_ref) = value.target_ref {
            existing.git_execution.target_ref = Some(target_ref);
        }
        if let Some(target_branch) = value.target_branch {
            existing.git_execution.target_branch = target_branch;
        }
        if let Some(require_task_branch) = value.require_task_branch {
            existing.git_execution.require_task_branch = require_task_branch;
        }
        if let Some(max_commits_behind_target) = value.max_commits_behind_target {
            existing.git_execution.max_commits_behind_target = max_commits_behind_target;
        }
        if let Some(max_fetch_age_seconds) = value.max_fetch_age_seconds {
            existing.git_execution.max_fetch_age_seconds = Some(max_fetch_age_seconds);
        }
    }
    existing
}

fn merge_plan_scheduling(
    mut existing: PlanScheduling,
    patch: CoordinationTransactionPlanSchedulingPatch,
) -> PlanScheduling {
    if let Some(value) = patch.importance {
        existing.importance = value;
    }
    if let Some(value) = patch.urgency {
        existing.urgency = value;
    }
    if let Some(value) = patch.manual_boost {
        existing.manual_boost = value;
    }
    if let Some(value) = patch.due_at {
        existing.due_at = Some(value);
    }
    existing
}

fn git_execution_policy_enabled(policy: &prism_coordination::GitExecutionPolicy) -> bool {
    !matches!(
        policy.start_mode,
        prism_coordination::GitExecutionStartMode::Off
    ) || !matches!(
        policy.completion_mode,
        prism_coordination::GitExecutionCompletionMode::Off
    )
}

fn coordination_status_bypasses_git_execution(status: CoordinationTaskStatus) -> bool {
    matches!(
        status,
        CoordinationTaskStatus::InProgress
            | CoordinationTaskStatus::InReview
            | CoordinationTaskStatus::Validating
            | CoordinationTaskStatus::Completed
            | CoordinationTaskStatus::Abandoned
    )
}

fn reject_git_execution_bypass_on_create(
    plan: &prism_coordination::Plan,
    requested_status: Option<CoordinationTaskStatus>,
) -> Result<()> {
    if requested_status.is_none()
        || !git_execution_policy_enabled(&plan.policy.git_execution)
        || !requested_status.is_some_and(coordination_status_bypasses_git_execution)
    {
        return Ok(());
    }
    let requested_status = requested_status
        .map(|status| format!("{status:?}").to_ascii_lowercase())
        .unwrap_or_default();
    Err(anyhow!(
        "task-execution work under an active git execution policy cannot be created directly in `{requested_status}`; create it as proposed/ready/blocked and transition through `update` instead"
    ))
}

fn rollback_snapshot_with_rejections(
    mut before_snapshot: CoordinationSnapshot,
    failed_snapshot: &CoordinationSnapshot,
) -> CoordinationSnapshot {
    let rejection_events = failed_snapshot
        .events
        .iter()
        .skip(before_snapshot.events.len())
        .filter(|event| event.kind == CoordinationEventKind::MutationRejected)
        .cloned();
    before_snapshot.events.extend(rejection_events);
    before_snapshot
}
