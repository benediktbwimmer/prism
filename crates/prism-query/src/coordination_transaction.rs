use std::collections::{BTreeMap, BTreeSet};
use std::fmt;

use anyhow::{anyhow, Result};
use prism_coordination::{
    AcceptanceCriterion, CoordinationPolicy, CoordinationRuntimeState, CoordinationSnapshot,
    PlanCreateInput, PlanScheduling, PlanUpdateInput, TaskCompletionContext, TaskCreateInput,
    TaskGitExecution, TaskUpdateInput,
};
use prism_ir::{
    AgentId, AnchorRef, CoordinationEventKind, CoordinationTaskId, CoordinationTaskStatus, EventId,
    EventMeta, PlanBinding, PlanId, PlanStatus, SessionId, ValidationRef, WorkspaceRevision,
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
    },
    PlanUpdate {
        plan: CoordinationTransactionPlanRef,
        title: Option<String>,
        goal: Option<String>,
        status: Option<PlanStatus>,
        policy: Option<CoordinationTransactionPolicyPatch>,
        scheduling: Option<CoordinationTransactionPlanSchedulingPatch>,
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
    },
    DependencyCreate {
        task: CoordinationTransactionTaskRef,
        depends_on: CoordinationTransactionTaskRef,
        kind: CoordinationDependencyKind,
        base_revision: WorkspaceRevision,
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
    pub rejection: Option<CoordinationTransactionProtocolRejection>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub indeterminate: Option<CoordinationTransactionProtocolIndeterminate>,
}

#[derive(Debug, Clone)]
pub struct CoordinationTransactionResult {
    pub outcome: CoordinationTransactionOutcome,
    pub commit: CoordinationTransactionCommitMetadata,
    pub authority_version: CoordinationTransactionAuthorityVersion,
    pub plan_ids_by_client_id: BTreeMap<String, PlanId>,
    pub task_ids_by_client_id: BTreeMap<String, CoordinationTaskId>,
    pub touched_plan_ids: Vec<PlanId>,
    pub touched_task_ids: Vec<CoordinationTaskId>,
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
            validate_transaction_conflict(
                coordination_runtime,
                optimistic_preconditions.as_ref(),
            )?;
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
        F: FnOnce(&mut CoordinationRuntimeState) -> std::result::Result<T, CoordinationTransactionError>,
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
                drop(runtime);
                self.invalidate_plan_discovery_cache();
                Ok(value)
            }
            Err(error) => {
                let failed_snapshot = runtime.snapshot();
                runtime.replace_from_snapshot(rollback_snapshot_with_rejections(
                    before_snapshot,
                    &failed_snapshot,
                ));
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
    if input.intent_metadata.is_some() {
        return Err(CoordinationTransactionError::rejected(
            CoordinationTransactionValidationStage::InputShape,
            CoordinationTransactionRejectionCategory::Unsupported,
            "unsupported_intent_metadata",
            "coordination_transaction intentMetadata is not supported yet",
        ));
    }

    let mut seen_plan_client_ids = BTreeSet::new();
    let mut seen_task_client_ids = BTreeSet::new();
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
                let Some(parsed) = number.as_u64().and_then(|value| usize::try_from(value).ok())
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
            _ => {}
        }
    }

    let mut seen_plan_client_ids = BTreeSet::new();
    let mut seen_task_client_ids = BTreeSet::new();
    for mutation in &input.mutations {
        match mutation {
            CoordinationTransactionMutation::PlanCreate {
                client_plan_id,
                ..
            } => {
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

fn apply_coordination_transaction(
    coordination_runtime: &mut CoordinationRuntimeState,
    meta: EventMeta,
    input: CoordinationTransactionInput,
) -> Result<CoordinationTransactionResult> {
    let before_event_len = coordination_runtime.snapshot().events.len();
    let mut seen_plan_client_ids = BTreeSet::new();
    let mut seen_task_client_ids = BTreeSet::new();
    let mut plan_ids_by_client_id = BTreeMap::new();
    let mut task_ids_by_client_id = BTreeMap::new();
    let mut touched_plan_ids = Vec::new();
    let mut touched_task_ids = Vec::new();
    let mut touched_plan_seen = BTreeSet::new();
    let mut touched_task_seen = BTreeSet::new();

    for (index, mutation) in input.mutations.into_iter().enumerate() {
        let step_meta = transaction_meta(&meta, index, mutation.action_tag());
        match mutation {
            CoordinationTransactionMutation::PlanCreate {
                client_plan_id,
                title,
                goal,
                status,
                policy,
                scheduling,
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
                    },
                    base_revision,
                    meta.ts,
                )?;
                touch_task(&mut touched_task_ids, &mut touched_task_seen, task_id);
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
        plan_ids_by_client_id,
        task_ids_by_client_id,
        touched_plan_ids,
        touched_task_ids,
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
