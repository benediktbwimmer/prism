use anyhow::{anyhow, Result};
use std::path::Path;

use prism_coordination::{
    CoordinationTask, GitExecutionCompletionMode, GitExecutionStartMode, GitPreflightReport,
    GitPublishReport, HandoffAcceptInput, HandoffInput, PolicyViolation, TaskCompletionContext,
    TaskCreateInput, TaskGitExecution, TaskReclaimInput, TaskResumeInput, TaskUpdateInput,
};
use prism_core::{
    AdmissionBusyError, AuthenticatedPrincipal, ValidationFeedbackCategory,
    ValidationFeedbackRecord, ValidationFeedbackVerdict, WorkspaceSession,
};
use prism_curator::{
    CandidateConcept, CandidateConceptOperation, CuratorJobId, CuratorProposal,
    CuratorProposalDisposition,
};
use prism_ir::{
    new_prefixed_id, AgentId, AnchorRef, ArtifactId, ArtifactStatus, ClaimId, CoordinationTaskId,
    Edge, EdgeOrigin, EventId, EventMeta, ObservedChangeCheckpoint,
    ObservedChangeCheckpointTrigger, PlanEdge, PlanEdgeId, PlanEdgeKind, PlanId, PlanNodeId,
    TaskId, WorkContextKind,
};
use prism_js::{CuratorProposalRecordView, TaskJournalView};
use prism_memory::{
    MemoryEntry, MemoryEvent, MemoryEventKind, MemoryKind, MemoryModule, MemoryScope, MemorySource,
    OutcomeEvent, OutcomeEvidence, OutcomeKind, OutcomeResult,
};
use prism_query::{
    canonical_concept_handle, canonical_contract_handle, ConceptEvent, ConceptEventAction,
    ConceptEventPatch, ConceptPacket, ConceptProvenance, ConceptPublication,
    ConceptPublicationStatus, ConceptRelation, ConceptRelationEvent, ConceptRelationEventAction,
    ConceptRelationKind, ConceptScope, ContractCompatibility, ContractEvent, ContractEventAction,
    ContractEventPatch, ContractGuarantee, ContractGuaranteeStrength, ContractKind, ContractPacket,
    ContractStability, ContractStatus, ContractTarget, ContractValidation, Prism,
};
use serde_json::{json, Value};

use crate::git_execution::{
    commit_paths, direct_integrate_published_branch, head_commit, prism_managed_paths,
    push_current_branch, ref_contains_commit, ref_head_commit, refresh_origin,
    restore_prism_managed_paths, restore_prism_managed_roots, run_preflight, user_dirty_paths,
    worktree_dirty_paths,
};
use crate::mutation_trace::MutationRun;
use crate::MutationProvenance;
use crate::{
    artifact_view, claim_view, concept_packet_view, concept_relation_view, conflict_view,
    contract_packet_view, convert_acceptance, convert_anchors, convert_capability,
    convert_claim_mode, convert_completion_context, convert_coordination_task_status,
    convert_inferred_scope, convert_memory_kind, convert_memory_scope, convert_memory_source,
    convert_node_id, convert_outcome_evidence, convert_outcome_kind, convert_outcome_result,
    convert_plan_acceptance, convert_plan_binding, convert_plan_edge_kind, convert_plan_node_kind,
    convert_plan_node_status, convert_plan_scheduling, convert_plan_status, convert_policy,
    convert_review_verdict, convert_validation_refs, coordination_task_view,
    curator_disposition_label, curator_job_status_label, curator_memory_metadata, curator_proposal,
    curator_proposal_state, curator_trigger_label, current_timestamp,
    ensure_repo_publication_metadata, manual_memory_metadata, parse_edge_kind, plan_edge_view,
    plan_node_view, plan_view, retire_repo_publication_metadata, task_journal_memory_metadata,
    ArtifactActionInput, ArtifactMutationResult, ArtifactProposePayload, ArtifactReviewPayload,
    ArtifactSupersedePayload, CheckpointMutationResult, ClaimAcquirePayload, ClaimActionInput,
    ClaimMutationResult, ClaimReleasePayload, ClaimRenewPayload, ConceptMutationOperationInput,
    ConceptMutationResult, ConceptRelationKindInput, ConceptRelationMutationOperationInput,
    ConceptRelationMutationResult, ConceptScopeInput, ConceptVerbosity, ContractCompatibilityInput,
    ContractGuaranteeInput, ContractGuaranteeStrengthInput, ContractKindInput,
    ContractMutationOperationInput, ContractMutationResult, ContractStabilityInput,
    ContractStatusInput, ContractTargetInput, ContractValidationInput,
    CoordinationMutationKindInput, CoordinationMutationResult, CuratorJobView,
    CuratorProposalCreatedResources, CuratorProposalDecision, CuratorProposalDecisionResult,
    EdgeMutationResult, EventMutationResult, HandoffAcceptPayload, HeartbeatLeaseMutationResult,
    MemoryMutationActionInput, MemoryMutationResult, MemoryRetirePayload, MemoryStorePayload,
    MutationViolationView, NodeIdInput, PlanArchivePayload, PlanEdgeCreatePayload,
    PlanEdgeDeletePayload, PlanNodeCreatePayload, PlanUpdatePayload, PrismArtifactArgs,
    PrismCheckpointArgs, PrismClaimArgs, PrismConceptLensInput, PrismConceptMutationArgs,
    PrismConceptRelationMutationArgs, PrismContractMutationArgs, PrismCoordinationArgs,
    PrismCuratorApplyProposalArgs, PrismCuratorPromoteConceptArgs, PrismCuratorPromoteEdgeArgs,
    PrismCuratorPromoteMemoryArgs, PrismCuratorRejectProposalArgs, PrismDeclareWorkArgs,
    PrismFinishTaskArgs, PrismHeartbeatLeaseArgs, PrismInferEdgeArgs, PrismMemoryArgs,
    PrismOutcomeArgs, PrismSessionRepairArgs, PrismValidationFeedbackArgs, QueryHost,
    SessionRepairMutationResult, SessionRepairOperationInput, SessionRepairOperationSchema,
    SessionState, SparsePatch, SparsePatchInput, TaskCompletionContextPayload, TaskCreatePayload,
    TaskReclaimPayload, TaskResumePayload, ValidationFeedbackCategoryInput,
    ValidationFeedbackMutationResult, ValidationFeedbackVerdictInput, WorkDeclarationKindInput,
    WorkDeclarationResult, WorkflowStatusInput, WorkflowUpdatePayload,
    DEFAULT_TASK_JOURNAL_EVENT_LIMIT, DEFAULT_TASK_JOURNAL_MEMORY_LIMIT,
};

fn record_optional_trace_phase(
    trace: Option<&MutationRun>,
    operation: &'static str,
    args: Value,
    started: std::time::Instant,
    success: bool,
    error: Option<String>,
) {
    if let Some(trace) = trace {
        trace.record_phase(operation, &args, started.elapsed(), success, error);
    }
}

fn record_optional_trace_result<T>(
    trace: Option<&MutationRun>,
    operation: &'static str,
    args: Value,
    started: std::time::Instant,
    result: &Result<T>,
) {
    match result {
        Ok(_) => record_optional_trace_phase(trace, operation, args, started, true, None),
        Err(error) => record_optional_trace_phase(
            trace,
            operation,
            args,
            started,
            false,
            Some(error.to_string()),
        ),
    }
}
use crate::{merge_plan_scheduling_payload, merge_policy_payload};

#[derive(Default)]
struct CoordinationAudit {
    event_ids: Vec<String>,
    violations: Vec<MutationViolationView>,
    rejected: bool,
}

#[derive(Clone, serde::Serialize)]
pub(crate) struct TaskClosureMutationResult {
    pub(crate) task_id: String,
    pub(crate) event_id: String,
    pub(crate) memory_id: String,
    pub(crate) journal: TaskJournalView,
}

enum TaskClosureDisposition {
    Completed,
    Abandoned,
}

impl TaskClosureDisposition {
    fn label(&self) -> &'static str {
        match self {
            Self::Completed => "completed",
            Self::Abandoned => "abandoned",
        }
    }

    fn outcome_result(&self) -> OutcomeResult {
        match self {
            Self::Completed => OutcomeResult::Success,
            Self::Abandoned => OutcomeResult::Partial,
        }
    }

    fn trust(&self) -> f32 {
        match self {
            Self::Completed => 0.85,
            Self::Abandoned => 0.7,
        }
    }
}

fn mutation_violation_view(value: PolicyViolation) -> MutationViolationView {
    MutationViolationView {
        code: serde_json::to_string(&value.code)
            .unwrap_or_else(|_| "\"unknown\"".to_string())
            .trim_matches('"')
            .to_string(),
        summary: value.summary,
        plan_id: value.plan_id.map(|id| id.0.to_string()),
        task_id: value.task_id.map(|id| id.0.to_string()),
        claim_id: value.claim_id.map(|id| id.0.to_string()),
        artifact_id: value.artifact_id.map(|id| id.0.to_string()),
        details: value.details,
    }
}

fn git_execution_policy_enabled(policy: &prism_coordination::GitExecutionPolicy) -> bool {
    !matches!(policy.start_mode, GitExecutionStartMode::Off)
        || !matches!(policy.completion_mode, GitExecutionCompletionMode::Off)
}

fn effective_publish_commit(report: Option<&GitPublishReport>) -> Option<String> {
    report.and_then(|report| {
        report
            .coordination_commit
            .clone()
            .or_else(|| report.code_commit.clone())
    })
}

fn task_git_execution_record(
    previous: &TaskGitExecution,
    policy: &prism_coordination::GitExecutionPolicy,
    preflight: &GitPreflightReport,
    status: prism_ir::GitExecutionStatus,
    pending_task_status: Option<prism_ir::CoordinationTaskStatus>,
    last_publish: Option<GitPublishReport>,
    integration_status: prism_ir::GitIntegrationStatus,
) -> TaskGitExecution {
    TaskGitExecution {
        status,
        pending_task_status,
        source_ref: preflight.source_ref.clone(),
        target_ref: preflight.target_ref.clone(),
        publish_ref: preflight.publish_ref.clone(),
        target_branch: Some(policy.target_branch.clone()),
        source_commit: preflight.head_commit.clone(),
        publish_commit: effective_publish_commit(last_publish.as_ref()),
        target_commit_at_publish: preflight.target_commit.clone(),
        review_artifact_ref: previous.review_artifact_ref.clone(),
        integration_commit: previous.integration_commit.clone(),
        integration_evidence: previous.integration_evidence.clone(),
        integration_mode: policy.integration_mode,
        integration_status,
        last_preflight: Some(preflight.clone()),
        last_publish,
    }
}

fn integration_status_after_coordination_publication(
    mode: prism_ir::GitIntegrationMode,
) -> prism_ir::GitIntegrationStatus {
    match mode {
        prism_ir::GitIntegrationMode::ManualPr | prism_ir::GitIntegrationMode::AutoPr => {
            prism_ir::GitIntegrationStatus::IntegrationPending
        }
        _ => prism_ir::GitIntegrationStatus::PublishedToBranch,
    }
}

fn auto_pr_review_diff_ref(publish_ref: &str) -> String {
    format!("patch:{publish_ref}")
}

fn auto_pr_review_artifact_is_current(
    artifact: &prism_coordination::Artifact,
    task_id: &CoordinationTaskId,
    desired_diff_ref: &str,
) -> bool {
    artifact.task == *task_id
        && artifact.diff_ref.as_deref() == Some(desired_diff_ref)
        && matches!(
            artifact.status,
            ArtifactStatus::Proposed | ArtifactStatus::InReview
        )
}

fn completion_context_for_task_update(
    prism: &Prism,
    task_id: &CoordinationTaskId,
    status: Option<prism_ir::CoordinationTaskStatus>,
    meta: &EventMeta,
    payload: Option<TaskCompletionContextPayload>,
) -> Option<TaskCompletionContext> {
    let inferred_risk = status
        .filter(|status| *status == prism_ir::CoordinationTaskStatus::Completed)
        .and_then(|_| prism.task_risk(task_id, meta.ts));

    match convert_completion_context(payload) {
        Some(context) => Some(TaskCompletionContext {
            risk_score: context
                .risk_score
                .or_else(|| inferred_risk.as_ref().map(|risk| risk.risk_score)),
            required_validations: if context.required_validations.is_empty() {
                inferred_risk
                    .as_ref()
                    .map(|risk| risk.likely_validations.clone())
                    .unwrap_or_default()
            } else {
                context.required_validations
            },
            review_artifact_ref: context.review_artifact_ref,
            integration_commit: context.integration_commit,
            integration_evidence: context.integration_evidence,
        }),
        None => inferred_risk.map(|risk| TaskCompletionContext {
            risk_score: Some(risk.risk_score),
            required_validations: risk.likely_validations,
            ..TaskCompletionContext::default()
        }),
    }
}

fn ensure_review_artifact_ready_for_integration(
    prism: &Prism,
    mode: prism_ir::GitIntegrationMode,
    task_id: &CoordinationTaskId,
    review_artifact_ref: &str,
) -> Result<()> {
    let artifact_id = ArtifactId::new(review_artifact_ref.to_string());
    let artifact = prism.coordination_artifact(&artifact_id).ok_or_else(|| {
        anyhow!("{mode:?} integration requires review artifact `{review_artifact_ref}` to exist")
    })?;
    if artifact.task != *task_id {
        return Err(anyhow!(
            "{mode:?} integration review artifact `{review_artifact_ref}` does not belong to task `{}`",
            task_id.0,
        ));
    }
    if !matches!(
        artifact.status,
        prism_ir::ArtifactStatus::Approved | prism_ir::ArtifactStatus::Merged
    ) {
        return Err(anyhow!(
            "{mode:?} integration requires an approved review artifact before recording target landing"
        ));
    }
    Ok(())
}

fn maybe_advance_auto_pr_integration_from_review(
    session: &SessionState,
    prism: &Prism,
    meta: &EventMeta,
    task_id: &CoordinationTaskId,
    artifact: &prism_coordination::Artifact,
) -> Result<()> {
    let Some(task) = prism.coordination_task(task_id) else {
        return Ok(());
    };
    if !matches!(
        task.git_execution.integration_mode,
        prism_ir::GitIntegrationMode::AutoPr
    ) {
        return Ok(());
    }
    if !matches!(
        task.git_execution.status,
        prism_ir::GitExecutionStatus::CoordinationPublished
            | prism_ir::GitExecutionStatus::PublishPending
    ) {
        return Ok(());
    }

    let linked_ref = task.git_execution.review_artifact_ref.as_deref();
    let artifact_ref = Some(artifact.id.0.as_str());
    let next_integration_status = if linked_ref == artifact_ref {
        match artifact.status {
            prism_ir::ArtifactStatus::Approved | prism_ir::ArtifactStatus::Merged => {
                prism_ir::GitIntegrationStatus::IntegrationInProgress
            }
            prism_ir::ArtifactStatus::Rejected => prism_ir::GitIntegrationStatus::IntegrationFailed,
            _ => prism_ir::GitIntegrationStatus::IntegrationPending,
        }
    } else if matches!(
        task.git_execution.integration_status,
        prism_ir::GitIntegrationStatus::IntegratedToTarget
    ) {
        return Ok(());
    } else {
        prism_ir::GitIntegrationStatus::IntegrationPending
    };

    if task.git_execution.integration_status == next_integration_status {
        return Ok(());
    }

    let mut next = task.git_execution.clone();
    next.integration_status = next_integration_status;
    let task_meta = EventMeta {
        id: session.next_event_id("coordination"),
        ..meta.clone()
    };
    prism.update_native_task_authoritative_only(
        task_meta,
        TaskUpdateInput {
            task_id: task_id.clone(),
            kind: None,
            status: None,
            published_task_status: None,
            git_execution: Some(next),
            assignee: None,
            session: None,
            worktree_id: None,
            branch_ref: None,
            title: None,
            summary: None,
            anchors: None,
            bindings: None,
            depends_on: None,
            coordination_depends_on: None,
            integrated_depends_on: None,
            acceptance: None,
            validation_refs: None,
            is_abstract: None,
            base_revision: Some(prism.workspace_revision()),
            priority: None,
            tags: None,
            completion_context: None,
        },
        prism.workspace_revision(),
        current_timestamp(),
    )?;
    Ok(())
}

fn task_git_execution_from_completion_context(
    prism: &Prism,
    task_id: &CoordinationTaskId,
    previous: &TaskGitExecution,
    completion_context: Option<&TaskCompletionContext>,
) -> Result<Option<TaskGitExecution>> {
    let Some(completion_context) = completion_context else {
        return Ok(None);
    };
    if completion_context.review_artifact_ref.is_none()
        && completion_context.integration_commit.is_none()
        && completion_context.integration_evidence.is_none()
    {
        return Ok(None);
    }

    let mut next = previous.clone();
    if let Some(review_artifact_ref) = completion_context.review_artifact_ref.clone() {
        next.review_artifact_ref = Some(review_artifact_ref);
        if matches!(
            next.integration_mode,
            prism_ir::GitIntegrationMode::ManualPr | prism_ir::GitIntegrationMode::AutoPr
        ) && !matches!(
            next.integration_status,
            prism_ir::GitIntegrationStatus::IntegratedToTarget
        ) {
            next.integration_status = prism_ir::GitIntegrationStatus::IntegrationPending;
        }
    }
    if let Some(integration_commit) = completion_context.integration_commit.clone() {
        next.integration_commit = Some(integration_commit);
    }
    if let Some(mut integration_evidence) = completion_context.integration_evidence.clone() {
        if matches!(
            next.integration_mode,
            prism_ir::GitIntegrationMode::ManualPr | prism_ir::GitIntegrationMode::AutoPr
        ) {
            let review_artifact_ref = next
                .review_artifact_ref
                .clone()
                .or_else(|| integration_evidence.review_artifact_ref.clone())
                .ok_or_else(|| {
                    anyhow!(
                        "{:?} integration requires a review artifact reference before recording target landing",
                        next.integration_mode
                    )
                })?;
            ensure_review_artifact_ready_for_integration(
                prism,
                next.integration_mode,
                task_id,
                &review_artifact_ref,
            )?;
            next.review_artifact_ref = Some(review_artifact_ref);
            if integration_evidence.review_artifact_ref.is_none() {
                integration_evidence.review_artifact_ref = next.review_artifact_ref.clone();
            }
        }
        next.integration_evidence = Some(integration_evidence);
        if next.integration_commit.is_none() {
            next.integration_commit = next
                .integration_evidence
                .as_ref()
                .map(|evidence| evidence.target_commit.clone());
        }
        next.integration_status = prism_ir::GitIntegrationStatus::IntegratedToTarget;
    }
    Ok(Some(next))
}

fn task_git_execution_with_direct_integration(
    previous: &TaskGitExecution,
    target_commit: String,
    record_ref: String,
) -> TaskGitExecution {
    let mut next = previous.clone();
    next.integration_commit = Some(target_commit.clone());
    next.integration_evidence = Some(prism_ir::GitIntegrationEvidence {
        kind: prism_ir::GitIntegrationEvidenceKind::TrustedRecord,
        target_commit,
        review_artifact_ref: next.review_artifact_ref.clone(),
        record_ref: Some(record_ref),
    });
    next.integration_status = prism_ir::GitIntegrationStatus::IntegratedToTarget;
    next
}

fn task_git_execution_with_failed_integration(previous: &TaskGitExecution) -> TaskGitExecution {
    let mut next = previous.clone();
    next.integration_status = prism_ir::GitIntegrationStatus::IntegrationFailed;
    next
}

fn task_target_ref(git_execution: &TaskGitExecution) -> Option<String> {
    git_execution.target_ref.clone().or_else(|| {
        git_execution
            .target_branch
            .as_ref()
            .map(|branch| format!("origin/{branch}"))
    })
}

fn validate_explicit_integration_evidence(
    root: &Path,
    prism: &Prism,
    task_id: &CoordinationTaskId,
    git_execution: &TaskGitExecution,
    completion_context: &TaskCompletionContext,
) -> Result<()> {
    let Some(evidence) = completion_context.integration_evidence.as_ref() else {
        return Ok(());
    };
    let target_ref = task_target_ref(git_execution).ok_or_else(|| {
        anyhow!(
            "cannot verify target integration for task `{}` without a target ref",
            task_id.0
        )
    })?;
    if matches!(
        git_execution.integration_mode,
        prism_ir::GitIntegrationMode::ManualPr | prism_ir::GitIntegrationMode::AutoPr
    ) {
        let review_artifact_ref = completion_context
            .review_artifact_ref
            .clone()
            .or_else(|| git_execution.review_artifact_ref.clone())
            .or_else(|| evidence.review_artifact_ref.clone())
            .ok_or_else(|| {
                anyhow!(
                    "{:?} integration requires a review artifact reference before recording target landing",
                    git_execution.integration_mode
                )
            })?;
        ensure_review_artifact_ready_for_integration(
            prism,
            git_execution.integration_mode,
            task_id,
            &review_artifact_ref,
        )?;
    }
    refresh_origin(root)?;
    let verified = match evidence.kind {
        prism_ir::GitIntegrationEvidenceKind::Reachability => {
            let publish_commit = git_execution.publish_commit.as_deref().ok_or_else(|| {
                anyhow!(
                    "cannot verify reachability for task `{}` without a publish commit",
                    task_id.0
                )
            })?;
            ref_contains_commit(root, &target_ref, publish_commit)?
        }
        prism_ir::GitIntegrationEvidenceKind::ReviewArtifact
        | prism_ir::GitIntegrationEvidenceKind::TrustedRecord => {
            ref_contains_commit(root, &target_ref, &evidence.target_commit)?
        }
    };
    if !verified {
        return Err(anyhow!(
            "target ref `{target_ref}` does not contain verified integration evidence for task `{}`",
            task_id.0
        ));
    }
    Ok(())
}

fn observed_integration_git_execution(
    root: &Path,
    prism: &Prism,
    task: &CoordinationTask,
) -> Result<Option<TaskGitExecution>> {
    let artifact_ready_for_integration = |artifact: &prism_coordination::Artifact| {
        matches!(
            artifact.status,
            prism_ir::ArtifactStatus::Approved | prism_ir::ArtifactStatus::Merged
        ) || artifact.reviews.iter().any(|review_id| {
            prism
                .coordination_snapshot()
                .reviews
                .iter()
                .find(|review| review.id == *review_id)
                .is_some_and(|review| review.verdict == prism_ir::ReviewVerdict::Approved)
        })
    };
    if !matches!(
        task.git_execution.status,
        prism_ir::GitExecutionStatus::CoordinationPublished
    ) {
        return Ok(None);
    }
    if matches!(
        task.git_execution.integration_status,
        prism_ir::GitIntegrationStatus::IntegratedToTarget
            | prism_ir::GitIntegrationStatus::IntegrationFailed
    ) {
        return Ok(None);
    }
    let Some(target_ref) = task_target_ref(&task.git_execution) else {
        return Ok(None);
    };
    let review_artifact_ref =
        match task.git_execution.integration_mode {
            prism_ir::GitIntegrationMode::ManualPr | prism_ir::GitIntegrationMode::AutoPr => {
                let linked_artifact = task.git_execution.review_artifact_ref.as_ref().and_then(
                    |review_artifact_ref| {
                        let artifact_id = ArtifactId::new(review_artifact_ref.clone());
                        prism
                            .coordination_artifact(&artifact_id)
                            .and_then(|artifact| {
                                (artifact.task == task.id
                                    && artifact_ready_for_integration(&artifact))
                                .then_some((review_artifact_ref.clone(), artifact))
                            })
                    },
                );
                if let Some((review_artifact_ref, _artifact)) = linked_artifact {
                    Some(review_artifact_ref)
                } else {
                    let mut approved_artifacts = prism
                        .coordination_snapshot()
                        .artifacts
                        .iter()
                        .filter(|artifact| {
                            artifact.task == task.id && artifact_ready_for_integration(artifact)
                        })
                        .map(|artifact| artifact.id.0.to_string())
                        .collect::<Vec<_>>();
                    approved_artifacts.sort();
                    approved_artifacts.dedup();
                    if approved_artifacts.len() == 1 {
                        approved_artifacts.into_iter().next()
                    } else {
                        return Ok(None);
                    }
                }
            }
            _ => task.git_execution.review_artifact_ref.clone(),
        };
    refresh_origin(root)?;
    if let Some(evidence) = task.git_execution.integration_evidence.as_ref() {
        if !ref_contains_commit(root, &target_ref, &evidence.target_commit)? {
            return Ok(None);
        }
        let mut next = task.git_execution.clone();
        let mut verified_evidence = evidence.clone();
        if verified_evidence.review_artifact_ref.is_none() {
            verified_evidence.review_artifact_ref = review_artifact_ref.clone();
        }
        next.review_artifact_ref = review_artifact_ref;
        next.integration_commit = Some(verified_evidence.target_commit.clone());
        next.integration_evidence = Some(verified_evidence);
        next.integration_status = prism_ir::GitIntegrationStatus::IntegratedToTarget;
        return Ok(Some(next));
    }
    let Some(publish_commit) = task.git_execution.publish_commit.as_deref() else {
        return Ok(None);
    };
    if !ref_contains_commit(root, &target_ref, publish_commit)? {
        return Ok(None);
    }
    let target_commit = ref_head_commit(root, &target_ref)?;
    let mut next = task.git_execution.clone();
    next.review_artifact_ref = review_artifact_ref.clone();
    next.integration_commit = Some(target_commit.clone());
    next.integration_evidence = Some(prism_ir::GitIntegrationEvidence {
        kind: prism_ir::GitIntegrationEvidenceKind::Reachability,
        target_commit,
        review_artifact_ref,
        record_ref: None,
    });
    next.integration_status = prism_ir::GitIntegrationStatus::IntegratedToTarget;
    Ok(Some(next))
}

fn maybe_observe_target_integration(
    session: &SessionState,
    prism: &Prism,
    meta: &EventMeta,
    root: &Path,
    task: &CoordinationTask,
) -> Result<Option<CoordinationTask>> {
    let Some(next_git_execution) = observed_integration_git_execution(root, prism, task)? else {
        return Ok(None);
    };
    if next_git_execution == task.git_execution {
        return Ok(None);
    }
    let task_meta = EventMeta {
        id: session.next_event_id("coordination"),
        ..meta.clone()
    };
    let updated = prism.update_native_task_authoritative_only(
        task_meta,
        TaskUpdateInput {
            task_id: task.id.clone(),
            kind: None,
            status: None,
            published_task_status: None,
            git_execution: Some(next_git_execution),
            assignee: None,
            session: None,
            worktree_id: None,
            branch_ref: None,
            title: None,
            summary: None,
            anchors: None,
            bindings: None,
            depends_on: None,
            coordination_depends_on: None,
            integrated_depends_on: None,
            acceptance: None,
            validation_refs: None,
            is_abstract: None,
            base_revision: Some(prism.workspace_revision()),
            priority: None,
            tags: None,
            completion_context: None,
        },
        prism.workspace_revision(),
        current_timestamp(),
    )?;
    Ok(Some(updated))
}

fn maybe_link_review_artifact_to_task_git_execution(
    session: &SessionState,
    prism: &Prism,
    meta: &EventMeta,
    task_id: &CoordinationTaskId,
    artifact_id: &ArtifactId,
) -> Result<()> {
    let Some(task) = prism.coordination_task(task_id) else {
        return Ok(());
    };
    if !matches!(
        task.git_execution.integration_mode,
        prism_ir::GitIntegrationMode::ManualPr | prism_ir::GitIntegrationMode::AutoPr
    ) {
        return Ok(());
    }

    let artifact_ref = artifact_id.0.to_string();
    let mut next = task.git_execution.clone();
    let mut changed = next.review_artifact_ref.as_deref() != Some(artifact_ref.as_str());
    next.review_artifact_ref = Some(artifact_ref);
    if matches!(
        next.status,
        prism_ir::GitExecutionStatus::CoordinationPublished
            | prism_ir::GitExecutionStatus::PublishPending
    ) && !matches!(
        next.integration_status,
        prism_ir::GitIntegrationStatus::IntegratedToTarget
            | prism_ir::GitIntegrationStatus::IntegrationPending
    ) {
        next.integration_status = prism_ir::GitIntegrationStatus::IntegrationPending;
        changed = true;
    }
    if !changed {
        return Ok(());
    }

    let task_meta = EventMeta {
        id: session.next_event_id("coordination"),
        ..meta.clone()
    };
    prism.update_native_task_authoritative_only(
        task_meta,
        TaskUpdateInput {
            task_id: task_id.clone(),
            kind: None,
            status: None,
            published_task_status: None,
            git_execution: Some(next),
            assignee: None,
            session: None,
            worktree_id: None,
            branch_ref: None,
            title: None,
            summary: None,
            anchors: None,
            bindings: None,
            depends_on: None,
            coordination_depends_on: None,
            integrated_depends_on: None,
            acceptance: None,
            validation_refs: None,
            is_abstract: None,
            base_revision: Some(prism.workspace_revision()),
            priority: None,
            tags: None,
            completion_context: None,
        },
        prism.workspace_revision(),
        current_timestamp(),
    )?;
    Ok(())
}

fn ensure_auto_pr_review_artifact(
    host: &QueryHost,
    session: &SessionState,
    authenticated: Option<&AuthenticatedPrincipal>,
    task_id: &CoordinationTaskId,
    trace: Option<&MutationRun>,
) -> Result<Option<ArtifactId>> {
    let prism = host.current_prism();
    let Some(task) = prism.coordination_task(task_id) else {
        return Ok(None);
    };
    if !matches!(
        task.git_execution.integration_mode,
        prism_ir::GitIntegrationMode::AutoPr
    ) {
        return Ok(None);
    }
    let Some(publish_ref) = task.git_execution.publish_ref.clone() else {
        return Ok(None);
    };
    let desired_diff_ref = auto_pr_review_diff_ref(&publish_ref);
    if let Some(existing_ref) = task.git_execution.review_artifact_ref.as_ref() {
        let existing_id = ArtifactId::new(existing_ref.clone());
        if prism
            .coordination_artifact(&existing_id)
            .is_some_and(|artifact| {
                auto_pr_review_artifact_is_current(&artifact, task_id, &desired_diff_ref)
            })
        {
            return Ok(Some(existing_id));
        }
    }

    host.run_workspace_coordination_step(
        session,
        authenticated,
        task_id,
        trace,
        "mutation.gitExecution.ensureReviewArtifact",
        json!({ "taskId": task_id.0.as_str(), "mode": "auto_pr" }),
        true,
        move |prism, meta| {
            let task = prism
                .coordination_task(task_id)
                .ok_or_else(|| anyhow!("unknown coordination task `{}`", task_id.0))?;

            if let Some(existing_ref) = task.git_execution.review_artifact_ref.as_ref() {
                let existing_id = ArtifactId::new(existing_ref.clone());
                if let Some(existing) = prism.coordination_artifact(&existing_id) {
                    if auto_pr_review_artifact_is_current(&existing, task_id, &desired_diff_ref) {
                        return Ok(existing_id);
                    }
                    if existing.status != ArtifactStatus::Superseded {
                        prism.supersede_native_artifact(
                            meta.clone(),
                            prism_coordination::ArtifactSupersedeInput {
                                artifact_id: existing_id,
                            },
                        )?;
                    }
                }
            }

            let recipe = prism.task_validation_recipe(task_id);
            let risk = prism.task_risk(task_id, meta.ts);
            let (artifact_id, artifact) = prism.propose_native_artifact(
                meta.clone(),
                prism_coordination::ArtifactProposeInput {
                    task_id: task_id.clone(),
                    anchors: task.anchors.clone(),
                    diff_ref: Some(desired_diff_ref.clone()),
                    evidence: Vec::new(),
                    base_revision: prism.workspace_revision(),
                    current_revision: prism.workspace_revision(),
                    required_validations: recipe.map(|recipe| recipe.checks).unwrap_or_default(),
                    validated_checks: Vec::new(),
                    risk_score: risk.map(|risk| risk.risk_score),
                    worktree_id: None,
                    branch_ref: task.branch_ref.clone(),
                },
            )?;
            maybe_link_review_artifact_to_task_git_execution(
                session,
                prism,
                &meta,
                &artifact.task,
                &artifact_id,
            )?;
            Ok(artifact_id)
        },
    )
    .map(Some)
}

fn coordination_status_bypasses_git_execution(status: prism_ir::CoordinationTaskStatus) -> bool {
    matches!(
        status,
        prism_ir::CoordinationTaskStatus::InProgress
            | prism_ir::CoordinationTaskStatus::InReview
            | prism_ir::CoordinationTaskStatus::Validating
            | prism_ir::CoordinationTaskStatus::Completed
    )
}

fn plan_node_status_bypasses_git_execution(status: prism_ir::PlanNodeStatus) -> bool {
    matches!(
        status,
        prism_ir::PlanNodeStatus::InProgress
            | prism_ir::PlanNodeStatus::InReview
            | prism_ir::PlanNodeStatus::Validating
            | prism_ir::PlanNodeStatus::Completed
    )
}

fn reject_git_execution_bypass_on_create(
    plan: &prism_coordination::Plan,
    requested_status: Option<&str>,
) -> Result<()> {
    if plan.kind != prism_ir::PlanKind::TaskExecution
        || !git_execution_policy_enabled(&plan.policy.git_execution)
    {
        return Ok(());
    }
    if let Some(status) = requested_status {
        return Err(anyhow!(
            "task-execution work under an active git execution policy cannot be created directly in `{status}`; create it as proposed/ready/blocked and transition through `update` instead"
        ));
    }
    Ok(())
}

fn coordination_audit_since(prism: &Prism, before_len: usize) -> CoordinationAudit {
    let mut audit = CoordinationAudit::default();
    for event in prism.coordination_events().into_iter().skip(before_len) {
        audit.event_ids.push(event.meta.id.0.to_string());
        if event.kind == prism_ir::CoordinationEventKind::MutationRejected {
            audit.rejected = true;
        }
        if let Some(value) = event.metadata.get("violations") {
            if let Ok(violations) = serde_json::from_value::<Vec<PolicyViolation>>(value.clone()) {
                audit
                    .violations
                    .extend(violations.into_iter().map(mutation_violation_view));
            }
        }
    }
    audit
}

fn coordination_plan_title(plan: &prism_coordination::Plan) -> String {
    plan.title.clone()
}

fn plan_title_for(prism: &Prism, plan_id: &str) -> Option<String> {
    prism
        .coordination_plan(&PlanId::new(plan_id.to_string()))
        .map(|plan| coordination_plan_title(&plan))
}

fn rebind_current_work_plan(
    host: &QueryHost,
    session: &SessionState,
    prism: &Prism,
    plan_id: &str,
) -> Result<()> {
    let plan_title = plan_title_for(prism, plan_id);
    session.update_current_work(|work| {
        let matches_plan = work
            .plan_id
            .as_deref()
            .is_none_or(|current| current == plan_id);
        if !matches_plan {
            return;
        }
        work.plan_id = Some(plan_id.to_string());
        if let Some(title) = plan_title.clone() {
            work.plan_title = Some(title);
        }
    });
    host.sync_workspace_active_work_context(session);
    host.persist_flushed_observed_change_checkpoints(session, None)?;
    Ok(())
}

fn maybe_bind_current_work_to_coordination_task(
    host: &QueryHost,
    session: &SessionState,
    prism: &Prism,
    task: &prism_coordination::CoordinationTask,
    bind_session_task: bool,
) -> Result<()> {
    let plan_id = task.plan.0.to_string();
    let plan_title = plan_title_for(prism, &plan_id);
    let mut should_bind_work = false;
    if let Some(current_work) = session.current_work_state() {
        let plan_matches = current_work
            .plan_id
            .as_deref()
            .is_none_or(|current| current == plan_id);
        let can_bind = matches!(
            current_work.kind,
            WorkContextKind::Coordination | WorkContextKind::Delegated
        ) && plan_matches;
        if bind_session_task {
            should_bind_work = can_bind;
        } else if current_work.kind == WorkContextKind::Delegated
            && current_work.coordination_task_id.is_none()
            && can_bind
        {
            should_bind_work = true;
        }
    }

    if should_bind_work {
        session.update_current_work(|work| {
            work.coordination_task_id = Some(task.id.0.to_string());
            work.plan_id = Some(plan_id.clone());
            if let Some(title) = plan_title.clone() {
                work.plan_title = Some(title);
            }
        });
    } else if bind_session_task {
        rebind_current_work_plan(host, session, prism, &plan_id)?;
    }

    if bind_session_task || should_bind_work {
        session.set_current_task(
            TaskId::new(task.id.0.to_string()),
            Some(task.title.clone()),
            Vec::new(),
            Some(task.id.0.to_string()),
        );
    }
    host.sync_workspace_active_work_context(session);
    host.persist_flushed_observed_change_checkpoints(session, None)?;
    Ok(())
}

fn clear_current_coordination_binding(
    host: &QueryHost,
    session: &SessionState,
    task_id: &str,
) -> Result<()> {
    if session
        .current_task_state()
        .as_ref()
        .is_some_and(|task| task.id.0 == task_id)
    {
        session.clear_current_task();
    }
    session.update_current_work(|work| {
        if work.coordination_task_id.as_deref() == Some(task_id) {
            work.coordination_task_id = None;
        }
    });
    host.sync_workspace_active_work_context(session);
    host.persist_flushed_observed_change_checkpoints(session, None)?;
    Ok(())
}

fn sync_session_after_coordination_mutation(
    host: &QueryHost,
    session: &SessionState,
    prism: &Prism,
    kind: &CoordinationMutationKindInput,
    state: &Value,
) -> Result<()> {
    match kind {
        CoordinationMutationKindInput::PlanCreate
        | CoordinationMutationKindInput::PlanUpdate
        | CoordinationMutationKindInput::PlanArchive => {
            if let Some(plan_id) = state.get("id").and_then(Value::as_str) {
                rebind_current_work_plan(host, session, prism, plan_id)?;
            }
        }
        CoordinationMutationKindInput::TaskCreate => {
            if let Some(plan_id) = state.get("planId").and_then(Value::as_str) {
                rebind_current_work_plan(host, session, prism, plan_id)?;
            }
            if let Some(task_id) = state.get("id").and_then(Value::as_str) {
                if let Some(task) =
                    prism.coordination_task(&CoordinationTaskId::new(task_id.to_string()))
                {
                    maybe_bind_current_work_to_coordination_task(
                        host, session, prism, &task, false,
                    )?;
                }
            }
        }
        CoordinationMutationKindInput::PlanNodeCreate
        | CoordinationMutationKindInput::PlanEdgeCreate
        | CoordinationMutationKindInput::PlanEdgeDelete
        | CoordinationMutationKindInput::Update => {
            if let Some(plan_id) = state.get("planId").and_then(Value::as_str) {
                rebind_current_work_plan(host, session, prism, plan_id)?;
            }
        }
        CoordinationMutationKindInput::Resume
        | CoordinationMutationKindInput::Reclaim
        | CoordinationMutationKindInput::HandoffAccept => {
            if let Some(task_id) = state.get("id").and_then(Value::as_str) {
                if let Some(task) =
                    prism.coordination_task(&CoordinationTaskId::new(task_id.to_string()))
                {
                    maybe_bind_current_work_to_coordination_task(
                        host, session, prism, &task, true,
                    )?;
                }
            }
        }
        CoordinationMutationKindInput::Handoff => {
            if let Some(task_id) = state.get("id").and_then(Value::as_str) {
                clear_current_coordination_binding(host, session, task_id)?;
            }
        }
    }
    Ok(())
}

fn mutation_provenance(
    host: &QueryHost,
    session: &SessionState,
    authenticated: Option<&AuthenticatedPrincipal>,
) -> MutationProvenance {
    let workspace = host.workspace_session_ref();
    let prism = host.current_prism();
    authenticated.map_or_else(
        || MutationProvenance::fallback(workspace, session, prism.clone()),
        |authenticated| {
            MutationProvenance::authenticated(workspace, session, prism.clone(), authenticated)
        },
    )
}

fn current_plan_node_state(prism: &Prism, plan_id: &PlanId, node_id: &str) -> Result<Value> {
    let graph = prism
        .plan_graph(plan_id)
        .ok_or_else(|| anyhow!("unknown plan `{}`", plan_id.0))?;
    let node = graph
        .nodes
        .into_iter()
        .find(|node| node.id.0 == node_id)
        .ok_or_else(|| anyhow!("unknown plan node `{node_id}`"))?;
    Ok(serde_json::to_value(plan_node_view(node))?)
}

fn current_plan_edge_state(
    prism: &Prism,
    plan_id: &PlanId,
    from_node_id: &str,
    to_node_id: &str,
    kind: PlanEdgeKind,
) -> Result<Value> {
    let graph = prism
        .plan_graph(plan_id)
        .ok_or_else(|| anyhow!("unknown plan `{}`", plan_id.0))?;
    let edge = graph
        .edges
        .into_iter()
        .find(|edge| edge.from.0 == from_node_id && edge.to.0 == to_node_id && edge.kind == kind)
        .ok_or_else(|| {
            anyhow!(
                "unknown plan edge `{}` -> `{}` ({:?})",
                from_node_id,
                to_node_id,
                kind
            )
        })?;
    Ok(serde_json::to_value(plan_edge_view(edge))?)
}

fn deleted_plan_edge_state(
    plan_id: &PlanId,
    from_node_id: &str,
    to_node_id: &str,
    kind: PlanEdgeKind,
) -> Result<Value> {
    Ok(serde_json::to_value(plan_edge_view(PlanEdge {
        id: PlanEdgeId::new(format!(
            "plan-edge:{}:{}:{}",
            from_node_id,
            plan_edge_kind_slug(kind),
            to_node_id
        )),
        plan_id: plan_id.clone(),
        from: PlanNodeId::new(from_node_id.to_string()),
        to: PlanNodeId::new(to_node_id.to_string()),
        kind,
        summary: None,
        metadata: Value::Null,
    }))?)
}

fn plan_edge_kind_slug(kind: PlanEdgeKind) -> &'static str {
    match kind {
        PlanEdgeKind::DependsOn => "depends-on",
        PlanEdgeKind::Blocks => "blocks",
        PlanEdgeKind::Informs => "informs",
        PlanEdgeKind::Validates => "validates",
        PlanEdgeKind::HandoffTo => "handoff-to",
        PlanEdgeKind::ChildOf => "child-of",
        PlanEdgeKind::RelatedTo => "related-to",
    }
}

fn resolve_native_plan_node(prism: &Prism, node_id: &str) -> Option<(PlanId, prism_ir::PlanNode)> {
    prism.plan_graphs().into_iter().find_map(|graph| {
        graph
            .nodes
            .into_iter()
            .find_map(|node| (node.id.0 == node_id).then(|| (graph.id.clone(), node)))
    })
}

enum WorkflowUpdateTarget {
    CoordinationTask(CoordinationTaskId),
    PlanNode {
        plan_id: PlanId,
        node_id: PlanNodeId,
    },
}

#[derive(Debug)]
enum GitExecutionWorkflow {
    Start,
    Complete(prism_ir::CoordinationTaskStatus),
}

struct GitExecutionRequest {
    task_id: CoordinationTaskId,
    workflow: GitExecutionWorkflow,
}

fn git_execution_request(
    prism: &Prism,
    args: &PrismCoordinationArgs,
) -> Result<Option<GitExecutionRequest>> {
    match args.kind {
        CoordinationMutationKindInput::Update => {
            let payload: WorkflowUpdatePayload = serde_json::from_value(args.payload.clone())?;
            let WorkflowUpdateTarget::CoordinationTask(task_id) =
                resolve_workflow_update_target(prism, &payload.id)?
            else {
                return Ok(None);
            };
            let Some(status) = payload
                .status
                .map(convert_workflow_status_for_task)
                .transpose()?
            else {
                return Ok(None);
            };
            let workflow = match status {
                prism_ir::CoordinationTaskStatus::InProgress => GitExecutionWorkflow::Start,
                prism_ir::CoordinationTaskStatus::Completed
                | prism_ir::CoordinationTaskStatus::Abandoned => {
                    GitExecutionWorkflow::Complete(status)
                }
                _ => return Ok(None),
            };
            Ok(Some(GitExecutionRequest { task_id, workflow }))
        }
        CoordinationMutationKindInput::Resume => {
            let payload: TaskResumePayload = serde_json::from_value(args.payload.clone())?;
            Ok(Some(GitExecutionRequest {
                task_id: CoordinationTaskId::new(payload.task_id),
                workflow: GitExecutionWorkflow::Start,
            }))
        }
        CoordinationMutationKindInput::Reclaim => {
            let payload: TaskReclaimPayload = serde_json::from_value(args.payload.clone())?;
            Ok(Some(GitExecutionRequest {
                task_id: CoordinationTaskId::new(payload.task_id),
                workflow: GitExecutionWorkflow::Start,
            }))
        }
        CoordinationMutationKindInput::HandoffAccept => {
            let payload: HandoffAcceptPayload = serde_json::from_value(args.payload.clone())?;
            Ok(Some(GitExecutionRequest {
                task_id: CoordinationTaskId::new(payload.task_id),
                workflow: GitExecutionWorkflow::Start,
            }))
        }
        _ => Ok(None),
    }
}

fn resolve_workflow_update_target(prism: &Prism, id: &str) -> Result<WorkflowUpdateTarget> {
    let task_id = CoordinationTaskId::new(id.to_string());
    if prism.coordination_task(&task_id).is_some() {
        return Ok(WorkflowUpdateTarget::CoordinationTask(task_id));
    }
    if let Some((plan_id, node)) = resolve_native_plan_node(prism, id) {
        return Ok(WorkflowUpdateTarget::PlanNode {
            plan_id,
            node_id: node.id,
        });
    }
    Err(anyhow!("unknown coordination task or plan node `{id}`"))
}

fn convert_workflow_status_for_task(
    value: WorkflowStatusInput,
) -> Result<prism_ir::CoordinationTaskStatus> {
    match value {
        WorkflowStatusInput::Proposed => Ok(prism_ir::CoordinationTaskStatus::Proposed),
        WorkflowStatusInput::Ready => Ok(prism_ir::CoordinationTaskStatus::Ready),
        WorkflowStatusInput::InProgress => Ok(prism_ir::CoordinationTaskStatus::InProgress),
        WorkflowStatusInput::Blocked => Ok(prism_ir::CoordinationTaskStatus::Blocked),
        WorkflowStatusInput::Waiting => Err(anyhow!(
            "status `waiting` is only supported for native plan nodes"
        )),
        WorkflowStatusInput::InReview => Ok(prism_ir::CoordinationTaskStatus::InReview),
        WorkflowStatusInput::Validating => Ok(prism_ir::CoordinationTaskStatus::Validating),
        WorkflowStatusInput::Completed => Ok(prism_ir::CoordinationTaskStatus::Completed),
        WorkflowStatusInput::Abandoned => Ok(prism_ir::CoordinationTaskStatus::Abandoned),
    }
}

fn convert_workflow_status_for_plan_node(value: WorkflowStatusInput) -> prism_ir::PlanNodeStatus {
    match value {
        WorkflowStatusInput::Proposed => prism_ir::PlanNodeStatus::Proposed,
        WorkflowStatusInput::Ready => prism_ir::PlanNodeStatus::Ready,
        WorkflowStatusInput::InProgress => prism_ir::PlanNodeStatus::InProgress,
        WorkflowStatusInput::Blocked => prism_ir::PlanNodeStatus::Blocked,
        WorkflowStatusInput::Waiting => prism_ir::PlanNodeStatus::Waiting,
        WorkflowStatusInput::InReview => prism_ir::PlanNodeStatus::InReview,
        WorkflowStatusInput::Validating => prism_ir::PlanNodeStatus::Validating,
        WorkflowStatusInput::Completed => prism_ir::PlanNodeStatus::Completed,
        WorkflowStatusInput::Abandoned => prism_ir::PlanNodeStatus::Abandoned,
    }
}

impl QueryHost {
    pub(crate) fn declare_work_without_refresh_authenticated(
        &self,
        session: &SessionState,
        args: PrismDeclareWorkArgs,
        authenticated: Option<&AuthenticatedPrincipal>,
    ) -> Result<WorkDeclarationResult> {
        let title = args.title.trim();
        if title.is_empty() {
            return Err(anyhow!("work title cannot be empty"));
        }

        let prism = self.current_prism();
        let inherited_parent_work = match (args.parent_work_id.as_deref(), args.kind.as_ref()) {
            (Some(parent_work_id), _) => session
                .current_work_state()
                .filter(|work| work.id.0 == parent_work_id),
            (None, Some(WorkDeclarationKindInput::Delegated)) => session.current_work_state(),
            _ => None,
        };
        let parent_work_id = args.parent_work_id.clone().or_else(|| {
            inherited_parent_work
                .as_ref()
                .map(|work| work.id.0.to_string())
        });
        let coordination_task_id = args.coordination_task_id.clone().or_else(|| {
            inherited_parent_work
                .as_ref()
                .and_then(|work| work.coordination_task_id.clone())
        });
        let coordination_task = coordination_task_id
            .as_ref()
            .map(|task_id| {
                prism
                    .coordination_task(&CoordinationTaskId::new(task_id.clone()))
                    .ok_or_else(|| anyhow!("unknown coordination task `{task_id}`"))
            })
            .transpose()?;
        let resolved_plan = if let Some(plan_id) = args.plan_id.as_ref() {
            Some(
                prism
                    .coordination_plan(&PlanId::new(plan_id.clone()))
                    .ok_or_else(|| anyhow!("unknown plan `{plan_id}`"))?,
            )
        } else {
            coordination_task
                .as_ref()
                .and_then(|task| prism.coordination_plan(&task.plan))
        };
        let plan_id = resolved_plan
            .as_ref()
            .map(|plan| plan.id.0.to_string())
            .or_else(|| {
                inherited_parent_work
                    .as_ref()
                    .and_then(|work| work.plan_id.clone())
            });
        let plan_title = resolved_plan
            .as_ref()
            .map(coordination_plan_title)
            .or_else(|| {
                inherited_parent_work
                    .as_ref()
                    .and_then(|work| work.plan_title.clone())
            });
        let kind = match args.kind {
            Some(WorkDeclarationKindInput::AdHoc) => WorkContextKind::AdHoc,
            Some(WorkDeclarationKindInput::Coordination) => WorkContextKind::Coordination,
            Some(WorkDeclarationKindInput::Delegated) => WorkContextKind::Delegated,
            None if coordination_task.is_some() => WorkContextKind::Coordination,
            None if parent_work_id.is_some() => WorkContextKind::Delegated,
            None => WorkContextKind::AdHoc,
        };
        let summary = args.summary.clone();
        let work_id = session.declare_work(
            title,
            kind,
            summary.clone(),
            parent_work_id.clone().map(TaskId::new),
            coordination_task_id.clone(),
            plan_id.clone(),
            plan_title.clone(),
        );
        if let Some(coordination_task) = coordination_task {
            session.set_current_task(
                TaskId::new(coordination_task.id.0.to_string()),
                Some(coordination_task.title.clone()),
                Vec::new(),
                Some(coordination_task.id.0.to_string()),
            );
        } else {
            session.clear_current_task();
        }
        self.sync_workspace_active_work_context(session);
        self.persist_flushed_observed_change_checkpoints(session, None)?;

        let provenance = mutation_provenance(self, session, authenticated);
        let event = OutcomeEvent {
            meta: provenance.event_meta(
                session.next_event_id("outcome"),
                Some(work_id.clone()),
                None,
                current_timestamp(),
            ),
            anchors: Vec::new(),
            kind: prism_memory::OutcomeKind::NoteAdded,
            result: prism_memory::OutcomeResult::Success,
            summary: summary
                .clone()
                .unwrap_or_else(|| format!("Declared work: {title}")),
            evidence: Vec::new(),
            metadata: json!({
                "workDeclaration": {
                    "title": title,
                    "kind": kind,
                    "parentWorkId": parent_work_id.clone(),
                    "coordinationTaskId": coordination_task_id.clone(),
                    "planId": plan_id.clone(),
                    "planTitle": plan_title.clone(),
                }
            }),
        };
        if let Some(workspace) = self.workspace_session() {
            workspace.append_outcome(event)?;
            self.sync_workspace_revision(workspace)?;
        } else {
            prism.apply_outcome_event_to_projections(&event);
            let _ = prism.outcome_memory().store_event(event)?;
            self.persist_outcomes()?;
        }

        self.persist_session_seed(session)?;
        Ok(WorkDeclarationResult {
            work_id: work_id.0.to_string(),
            kind,
            title: title.to_string(),
            summary,
            parent_work_id,
            coordination_task_id,
            plan_id,
            plan_title,
            session: self.session_view_without_refresh(session),
        })
    }

    pub(crate) fn store_checkpoint_authenticated(
        &self,
        session: &SessionState,
        args: PrismCheckpointArgs,
        authenticated: Option<&AuthenticatedPrincipal>,
    ) -> Result<CheckpointMutationResult> {
        let summary = args
            .summary
            .as_ref()
            .map(|value| value.trim().to_string())
            .filter(|value| !value.is_empty());
        let task_id = args
            .task_id
            .clone()
            .or_else(|| session.current_work().map(|id| id.0.to_string()))
            .or_else(|| session.current_task().map(|id| id.0.to_string()))
            .ok_or_else(|| anyhow!("checkpoint requires current work or explicit task id"))?;

        let mut event_ids = Vec::new();
        if let Some(workspace) = self.workspace_session_ref() {
            workspace
                .flush_observed_changes(prism_core::ObservedChangeFlushTrigger::ExplicitCheckpoint);
        }
        event_ids.extend(
            self.persist_flushed_observed_change_checkpoints(session, summary.as_deref())?
                .into_iter()
                .map(|id| id.0.to_string()),
        );

        if event_ids.is_empty() {
            let provenance = mutation_provenance(self, session, authenticated);
            let event = OutcomeEvent {
                meta: provenance.event_meta(
                    EventId::new(new_prefixed_id("checkpoint")),
                    Some(TaskId::new(task_id.clone())),
                    None,
                    current_timestamp(),
                ),
                anchors: Vec::new(),
                kind: OutcomeKind::NoteAdded,
                result: OutcomeResult::Success,
                summary: summary
                    .clone()
                    .unwrap_or_else(|| format!("Checkpointed work {task_id}")),
                evidence: Vec::new(),
                metadata: json!({
                    "observedChangeCheckpoint": ObservedChangeCheckpoint {
                        flush_trigger: ObservedChangeCheckpointTrigger::ExplicitCheckpoint,
                        changed_paths: Vec::<String>::new(),
                        entries: Vec::new(),
                        window_started_at: current_timestamp(),
                        window_ended_at: current_timestamp(),
                        summary: summary.clone(),
                    }
                }),
            };
            let event_id = if let Some(workspace) = self.workspace_session() {
                let id = workspace.append_outcome(event)?;
                self.sync_workspace_revision(workspace)?;
                id
            } else {
                let prism = self.current_prism();
                prism.apply_outcome_event_to_projections(&event);
                let id = prism.outcome_memory().store_event(event)?;
                self.persist_outcomes()?;
                id
            };
            event_ids.push(event_id.0.to_string());
        }

        self.persist_session_seed(session)?;
        Ok(CheckpointMutationResult {
            event_ids,
            task_id,
            summary,
            session: self.session_view_without_refresh(session),
        })
    }

    pub(crate) fn ensure_tool_enabled(&self, tool_name: &str, label: &str) -> Result<()> {
        if !self.features.is_tool_enabled(tool_name) {
            return Err(anyhow!(
                "{label} are disabled by the PRISM MCP server feature flags"
            ));
        }
        Ok(())
    }

    pub(crate) fn repair_session_without_refresh(
        &self,
        session: &SessionState,
        args: PrismSessionRepairArgs,
    ) -> Result<SessionRepairMutationResult> {
        match args.operation {
            SessionRepairOperationInput::ClearCurrentTask => {
                let cleared_task_id = session
                    .current_task_state()
                    .map(|task| task.id.0.to_string());
                if cleared_task_id.is_none() {
                    return Err(anyhow!("no current task is set for this session"));
                }
                session.clear_current_task();
                Ok(SessionRepairMutationResult {
                    operation: SessionRepairOperationSchema::ClearCurrentTask,
                    cleared_task_id,
                    session: self.session_view_without_refresh(session),
                })
            }
        }
    }

    #[allow(dead_code)]
    pub(crate) fn start_task(
        &self,
        session: &SessionState,
        description: Option<String>,
        tags: Vec<String>,
        coordination_task_id: Option<String>,
    ) -> Result<TaskId> {
        let (task, description, coordination_task_id) =
            if let Some(coordination_task_id) = coordination_task_id {
                let coordination_task = self
                    .current_prism()
                    .coordination_task(&prism_ir::CoordinationTaskId::new(
                        coordination_task_id.clone(),
                    ))
                    .ok_or_else(|| anyhow!("unknown coordination task `{coordination_task_id}`"))?;
                let description = description
                    .map(|value| value.trim().to_owned())
                    .filter(|value| !value.is_empty())
                    .unwrap_or_else(|| coordination_task.title.clone());
                (
                    session.start_task(
                        &description,
                        &tags,
                        Some(TaskId::new(coordination_task_id.clone())),
                        Some(coordination_task_id.clone()),
                    ),
                    description,
                    Some(coordination_task_id),
                )
            } else {
                let description = description.unwrap_or_default();
                (
                    session.start_task(&description, &tags, None, None),
                    description,
                    None,
                )
            };
        let event = OutcomeEvent {
            meta: mutation_provenance(self, session, None).event_meta(
                session.next_event_id("outcome"),
                Some(task.clone()),
                None,
                current_timestamp(),
            ),
            anchors: Vec::new(),
            kind: prism_memory::OutcomeKind::PlanCreated,
            result: prism_memory::OutcomeResult::Success,
            summary: description,
            evidence: Vec::new(),
            metadata: json!({
                "tags": tags,
                "coordinationTaskId": coordination_task_id,
            }),
        };
        if let Some(workspace) = self.workspace_session() {
            if workspace.try_append_outcome(event)?.is_some() {
                self.sync_workspace_revision(workspace)?;
            }
        } else {
            let prism = self.current_prism();
            prism.apply_outcome_event_to_projections(&event);
            let _ = prism.outcome_memory().store_event(event)?;
            self.persist_outcomes()?;
        }
        self.persist_session_seed(session)?;
        Ok(task)
    }

    #[allow(dead_code)]
    pub(crate) fn finish_task(
        &self,
        session: &SessionState,
        args: PrismFinishTaskArgs,
    ) -> Result<TaskClosureMutationResult> {
        self.close_task_without_refresh(session, args, TaskClosureDisposition::Completed)
    }

    #[allow(dead_code)]
    pub(crate) fn abandon_task(
        &self,
        session: &SessionState,
        args: PrismFinishTaskArgs,
    ) -> Result<TaskClosureMutationResult> {
        self.close_task_without_refresh(session, args, TaskClosureDisposition::Abandoned)
    }

    #[allow(dead_code)]
    pub(crate) fn finish_task_without_refresh(
        &self,
        session: &SessionState,
        args: PrismFinishTaskArgs,
    ) -> Result<TaskClosureMutationResult> {
        self.close_task_without_refresh(session, args, TaskClosureDisposition::Completed)
    }

    #[allow(dead_code)]
    pub(crate) fn abandon_task_without_refresh(
        &self,
        session: &SessionState,
        args: PrismFinishTaskArgs,
    ) -> Result<TaskClosureMutationResult> {
        self.close_task_without_refresh(session, args, TaskClosureDisposition::Abandoned)
    }

    fn close_task_without_refresh(
        &self,
        session: &SessionState,
        args: PrismFinishTaskArgs,
        disposition: TaskClosureDisposition,
    ) -> Result<TaskClosureMutationResult> {
        if args.summary.trim().is_empty() {
            return Err(anyhow!("task summary cannot be empty"));
        }

        let current_task = session.current_task_state();
        let task = args
            .task_id
            .map(TaskId::new)
            .or_else(|| current_task.as_ref().map(|task| task.id.clone()))
            .ok_or_else(|| anyhow!("no active task is set; provide taskId or start a task"))?;
        let metadata_override = current_task
            .as_ref()
            .filter(|state| state.id == task)
            .map(|state| (state.description.clone(), state.tags.clone()));
        let prism = self.current_prism();
        let replay = crate::load_task_replay(self.workspace_session_ref(), prism.as_ref(), &task)
            .unwrap_or_else(|_| prism.resume_task(&task));
        if replay.events.is_empty() && metadata_override.is_none() {
            return Err(anyhow!("unknown task `{}`", task.0));
        }

        let mut anchors = replay
            .events
            .iter()
            .flat_map(|event| event.anchors.iter().cloned())
            .collect::<Vec<_>>();
        if let Some(explicit) = args.anchors {
            anchors.extend(convert_anchors(
                prism.as_ref(),
                self.workspace_session_ref(),
                self.workspace_root(),
                explicit,
            )?);
        }
        let anchors = prism.anchors_for(&anchors);

        let mut entry = MemoryEntry::new(MemoryKind::Episodic, args.summary.clone());
        entry.anchors = anchors.clone();
        entry.source = MemorySource::Agent;
        entry.trust = disposition.trust();
        entry.metadata = task_journal_memory_metadata(Value::Null, &task, disposition.label());
        let memory_id = session.notes.store(entry)?;
        let mut memory_event = session
            .notes
            .entry(&memory_id)
            .map(|entry| {
                MemoryEvent::from_entry(
                    MemoryEventKind::Stored,
                    entry,
                    Some(task.0.to_string()),
                    Vec::new(),
                    Vec::new(),
                )
            })
            .ok_or_else(|| anyhow!("stored memory `{}` could not be reloaded", memory_id.0))?;
        mutation_provenance(self, session, None).stamp_memory_event(&mut memory_event);

        let event = OutcomeEvent {
            meta: mutation_provenance(self, session, None).event_meta(
                session.next_event_id("outcome"),
                Some(task.clone()),
                None,
                current_timestamp(),
            ),
            anchors,
            kind: OutcomeKind::NoteAdded,
            result: disposition.outcome_result(),
            summary: args.summary,
            evidence: Vec::new(),
            metadata: json!({
                "taskLifecycle": {
                    "disposition": disposition.label(),
                    "closed": true,
                    "memoryId": memory_id.0.clone(),
                }
            }),
        };
        let event_id = if let Some(workspace) = self.workspace_session() {
            let event_id =
                workspace.append_outcome_with_auxiliary(event, vec![memory_event], None, None)?;
            self.sync_workspace_revision(workspace)?;
            self.sync_episodic_revision(workspace)?;
            event_id
        } else {
            prism.apply_outcome_event_to_projections(&event);
            let id = prism.outcome_memory().store_event(event)?;
            self.persist_outcomes()?;
            self.persist_notes()?;
            id
        };

        if current_task.as_ref().is_some_and(|state| state.id == task) {
            session.clear_current_task();
        }
        self.persist_session_seed(session)?;

        let replay = crate::load_task_replay(
            self.workspace_session_ref(),
            self.current_prism().as_ref(),
            &task,
        )
        .unwrap_or_else(|_| self.current_prism().resume_task(&task));
        let journal = crate::task_journal_view_from_replay(
            session,
            self.current_prism().as_ref(),
            replay,
            metadata_override,
            DEFAULT_TASK_JOURNAL_EVENT_LIMIT,
            DEFAULT_TASK_JOURNAL_MEMORY_LIMIT,
        )?;

        Ok(TaskClosureMutationResult {
            task_id: task.0.to_string(),
            event_id: event_id.0.to_string(),
            memory_id: memory_id.0,
            journal,
        })
    }

    #[allow(dead_code)]
    pub(crate) fn store_outcome(
        &self,
        session: &SessionState,
        args: PrismOutcomeArgs,
    ) -> Result<EventMutationResult> {
        self.store_outcome_without_refresh_authenticated(session, args, None)
    }

    #[cfg_attr(not(test), allow(dead_code))]
    pub(crate) fn store_outcome_without_refresh(
        &self,
        session: &SessionState,
        args: PrismOutcomeArgs,
    ) -> Result<EventMutationResult> {
        self.store_outcome_without_refresh_authenticated(session, args, None)
    }

    pub(crate) fn store_outcome_without_refresh_authenticated(
        &self,
        session: &SessionState,
        args: PrismOutcomeArgs,
        authenticated: Option<&AuthenticatedPrincipal>,
    ) -> Result<EventMutationResult> {
        let prism = self.current_prism();
        let anchors = prism.anchors_for(&convert_anchors(
            prism.as_ref(),
            self.workspace_session_ref(),
            self.workspace_root(),
            args.anchors,
        )?);
        let task_id = session.task_for_mutation(args.task_id.map(TaskId::new));
        let provenance = mutation_provenance(self, session, authenticated);
        let event = OutcomeEvent {
            meta: provenance.event_meta(
                session.next_event_id("outcome"),
                Some(task_id.clone()),
                None,
                current_timestamp(),
            ),
            anchors,
            kind: convert_outcome_kind(args.kind),
            result: args
                .result
                .map(convert_outcome_result)
                .unwrap_or(prism_memory::OutcomeResult::Unknown),
            summary: args.summary,
            evidence: args
                .evidence
                .unwrap_or_default()
                .into_iter()
                .map(convert_outcome_evidence)
                .collect(),
            metadata: Value::Null,
        };
        let event_id = if let Some(workspace) = self.workspace_session() {
            let event_id = workspace.append_outcome(event)?;
            self.sync_workspace_revision(workspace)?;
            event_id
        } else {
            prism.apply_outcome_event_to_projections(&event);
            let id = prism.outcome_memory().store_event(event)?;
            self.persist_outcomes()?;
            id
        };
        Ok(EventMutationResult {
            event_id: event_id.0.to_string(),
            task_id: task_id.0.to_string(),
        })
    }

    #[allow(dead_code)]
    pub(crate) fn store_memory(
        &self,
        session: &SessionState,
        args: PrismMemoryArgs,
    ) -> Result<MemoryMutationResult> {
        self.store_memory_without_refresh_authenticated(session, args, None)
    }

    #[cfg_attr(not(test), allow(dead_code))]
    pub(crate) fn store_memory_without_refresh(
        &self,
        session: &SessionState,
        args: PrismMemoryArgs,
    ) -> Result<MemoryMutationResult> {
        self.store_memory_without_refresh_authenticated(session, args, None)
    }

    pub(crate) fn store_memory_without_refresh_authenticated(
        &self,
        session: &SessionState,
        args: PrismMemoryArgs,
        authenticated: Option<&AuthenticatedPrincipal>,
    ) -> Result<MemoryMutationResult> {
        let task_id = session.task_for_mutation(args.task_id.map(TaskId::new));
        match args.action {
            MemoryMutationActionInput::Store => self.store_memory_payload(
                session,
                task_id,
                serde_json::from_value::<MemoryStorePayload>(args.payload)?,
                authenticated,
            ),
            MemoryMutationActionInput::Retire => self.retire_memory_payload(
                session,
                task_id,
                serde_json::from_value::<MemoryRetirePayload>(args.payload)?,
                authenticated,
            ),
        }
    }

    fn store_memory_payload(
        &self,
        session: &SessionState,
        task_id: TaskId,
        payload: MemoryStorePayload,
        authenticated: Option<&AuthenticatedPrincipal>,
    ) -> Result<MemoryMutationResult> {
        let prism = self.current_prism();
        let anchors = prism.anchors_for(&convert_anchors(
            prism.as_ref(),
            self.workspace_session_ref(),
            self.workspace_root(),
            payload.anchors,
        )?);
        let kind = convert_memory_kind(payload.kind);
        let mut entry = MemoryEntry::new(kind, payload.content);
        entry.anchors = anchors;
        entry.scope = payload
            .scope
            .map(convert_memory_scope)
            .unwrap_or(MemoryScope::Session);
        entry.source = payload
            .source
            .map(convert_memory_source)
            .unwrap_or(MemorySource::Agent);
        entry.trust = payload.trust.unwrap_or(0.5).clamp(0.0, 1.0);
        entry.metadata = manual_memory_metadata(payload.metadata.unwrap_or(Value::Null), &task_id);
        if entry.scope == MemoryScope::Repo {
            entry.metadata = ensure_repo_publication_metadata(entry.metadata, current_timestamp());
        }

        let promoted_from = payload
            .promoted_from
            .clone()
            .unwrap_or_default()
            .into_iter()
            .map(prism_memory::MemoryId)
            .collect::<Vec<_>>();
        let supersedes = payload
            .supersedes
            .clone()
            .unwrap_or_default()
            .into_iter()
            .map(prism_memory::MemoryId)
            .collect::<Vec<_>>();
        ensure_repo_memory_publication_is_not_duplicate(session, &entry, supersedes.as_slice())?;

        let memory_id = session.notes.store(entry)?;
        let stored_entry = session
            .notes
            .entry(&memory_id)
            .ok_or_else(|| anyhow!("stored memory `{}` could not be reloaded", memory_id.0))?;
        if stored_entry.scope != MemoryScope::Local {
            if let Some(workspace) = self.workspace_session() {
                let action =
                    memory_event_kind_for_store(promoted_from.as_slice(), supersedes.as_slice());
                let mut memory_event = MemoryEvent::from_entry(
                    action,
                    stored_entry.clone(),
                    Some(task_id.0.to_string()),
                    promoted_from,
                    supersedes,
                );
                mutation_provenance(self, session, authenticated)
                    .stamp_memory_event(&mut memory_event);
                workspace.append_memory_event(memory_event)?;
                if stored_entry.scope == MemoryScope::Repo {
                    self.reload_episodic_snapshot(workspace)?;
                } else {
                    self.sync_episodic_revision(workspace)?;
                }
            } else if stored_entry.scope == MemoryScope::Repo {
                return Err(anyhow!(
                    "repo-published memory requires a workspace-backed PRISM session"
                ));
            }
        }
        let note_anchors = stored_entry.anchors.clone();
        let note_content = stored_entry.content.clone();
        if kind == MemoryKind::Episodic {
            if stored_entry.scope == MemoryScope::Local {
                return Ok(MemoryMutationResult {
                    memory_id: memory_id.0,
                    task_id: task_id.0.to_string(),
                });
            }
            let note_event = OutcomeEvent {
                meta: mutation_provenance(self, session, authenticated).event_meta(
                    session.next_event_id("outcome"),
                    Some(task_id.clone()),
                    None,
                    current_timestamp(),
                ),
                anchors: note_anchors,
                kind: prism_memory::OutcomeKind::NoteAdded,
                result: prism_memory::OutcomeResult::Success,
                summary: note_content,
                evidence: Vec::new(),
                metadata: Value::Null,
            };
            if let Some(workspace) = self.workspace_session() {
                let _ = workspace.append_outcome(note_event)?;
                self.sync_workspace_revision(workspace)?;
            } else {
                prism.apply_outcome_event_to_projections(&note_event);
                let _ = prism.outcome_memory().store_event(note_event)?;
                self.persist_outcomes()?;
                self.persist_notes()?;
            }
        } else if self.workspace_session().is_none() && stored_entry.scope != MemoryScope::Local {
            self.persist_notes()?;
        }
        Ok(MemoryMutationResult {
            memory_id: memory_id.0,
            task_id: task_id.0.to_string(),
        })
    }

    fn retire_memory_payload(
        &self,
        session: &SessionState,
        task_id: TaskId,
        payload: MemoryRetirePayload,
        authenticated: Option<&AuthenticatedPrincipal>,
    ) -> Result<MemoryMutationResult> {
        let workspace = self.workspace_session().ok_or_else(|| {
            anyhow!("retiring repo-published memory requires a workspace-backed session")
        })?;
        let memory_id = prism_memory::MemoryId(payload.memory_id.clone());
        let existing = session
            .notes
            .entry(&memory_id)
            .ok_or_else(|| anyhow!("unknown memory `{}`", payload.memory_id))?;
        if existing.scope != MemoryScope::Repo {
            return Err(anyhow!(
                "only repo-published memory can be retired through prism_mutate"
            ));
        }
        let already_retired = existing
            .metadata
            .get("publication")
            .and_then(|value| value.get("status"))
            .and_then(Value::as_str)
            .is_some_and(|status| status.eq_ignore_ascii_case("retired"));
        if already_retired {
            return Err(anyhow!("memory `{}` is already retired", payload.memory_id));
        }

        let mut retired_entry = existing;
        retired_entry.metadata = retire_repo_publication_metadata(
            retired_entry.metadata,
            current_timestamp(),
            &payload.retirement_reason,
        );
        let mut memory_event = MemoryEvent::from_entry(
            MemoryEventKind::Retired,
            retired_entry,
            Some(task_id.0.to_string()),
            Vec::new(),
            Vec::new(),
        );
        mutation_provenance(self, session, authenticated).stamp_memory_event(&mut memory_event);
        workspace.append_memory_event(memory_event)?;
        self.reload_episodic_snapshot(workspace)?;
        Ok(MemoryMutationResult {
            memory_id: payload.memory_id,
            task_id: task_id.0.to_string(),
        })
    }

    #[allow(dead_code)]
    pub(crate) fn store_concept(
        &self,
        session: &SessionState,
        args: PrismConceptMutationArgs,
    ) -> Result<ConceptMutationResult> {
        self.store_concept_without_refresh_authenticated(session, args, None)
    }

    #[allow(dead_code)]
    pub(crate) fn store_concept_without_refresh(
        &self,
        session: &SessionState,
        args: PrismConceptMutationArgs,
    ) -> Result<ConceptMutationResult> {
        self.store_concept_without_refresh_authenticated(session, args, None)
    }

    pub(crate) fn store_concept_without_refresh_authenticated(
        &self,
        session: &SessionState,
        args: PrismConceptMutationArgs,
        authenticated: Option<&AuthenticatedPrincipal>,
    ) -> Result<ConceptMutationResult> {
        let workspace = self.workspace_session().ok_or_else(|| {
            anyhow!("concept promotion requires a workspace-backed PRISM session")
        })?;
        let prism = self.current_prism();
        let task_id = session.task_for_mutation(args.task_id.clone().map(TaskId::new));
        let operation = args.operation.clone();
        let recorded_at = current_timestamp();
        let packet = match operation {
            ConceptMutationOperationInput::Promote => {
                build_promoted_concept_packet(prism.as_ref(), &task_id, recorded_at, args.clone())?
            }
            ConceptMutationOperationInput::Update => {
                build_updated_concept_packet(prism.as_ref(), &task_id, recorded_at, args.clone())?
            }
            ConceptMutationOperationInput::Retire => {
                build_retired_concept_packet(prism.as_ref(), &task_id, recorded_at, args.clone())?
            }
        };
        let patch = concept_event_patch(&args, &operation, &packet)?;
        let mut event = ConceptEvent {
            id: next_concept_event_id(),
            recorded_at,
            task_id: Some(task_id.0.to_string()),
            actor: None,
            execution_context: None,
            action: match operation {
                ConceptMutationOperationInput::Promote => ConceptEventAction::Promote,
                ConceptMutationOperationInput::Update => ConceptEventAction::Update,
                ConceptMutationOperationInput::Retire => ConceptEventAction::Retire,
            },
            patch,
            concept: packet.clone(),
        };
        mutation_provenance(self, session, authenticated).stamp_concept_event(&mut event);
        workspace.append_concept_event(event.clone())?;
        self.sync_workspace_revision(workspace)?;
        Ok(ConceptMutationResult {
            event_id: event.id,
            concept_handle: packet.handle.clone(),
            task_id: task_id.0.to_string(),
            packet: concept_packet_view(prism.as_ref(), packet, ConceptVerbosity::Full, true, None),
        })
    }

    #[allow(dead_code)]
    pub(crate) fn store_contract(
        &self,
        session: &SessionState,
        args: PrismContractMutationArgs,
    ) -> Result<ContractMutationResult> {
        self.store_contract_without_refresh_authenticated(session, args, None)
    }

    #[allow(dead_code)]
    pub(crate) fn store_contract_without_refresh(
        &self,
        session: &SessionState,
        args: PrismContractMutationArgs,
    ) -> Result<ContractMutationResult> {
        self.store_contract_without_refresh_authenticated(session, args, None)
    }

    pub(crate) fn store_contract_without_refresh_authenticated(
        &self,
        session: &SessionState,
        args: PrismContractMutationArgs,
        authenticated: Option<&AuthenticatedPrincipal>,
    ) -> Result<ContractMutationResult> {
        let workspace = self.workspace_session().ok_or_else(|| {
            anyhow!("contract mutations require a workspace-backed PRISM session")
        })?;
        let prism = self.current_prism();
        let task_id = session.task_for_mutation(args.task_id.clone().map(TaskId::new));
        let operation = args.operation.clone();
        let recorded_at = current_timestamp();
        let workspace_root = Some(workspace.root());
        let packet = match operation {
            ContractMutationOperationInput::Promote => build_promoted_contract_packet(
                prism.as_ref(),
                Some(workspace),
                workspace_root,
                &task_id,
                recorded_at,
                args.clone(),
            )?,
            ContractMutationOperationInput::Update => build_updated_contract_packet(
                prism.as_ref(),
                Some(workspace),
                workspace_root,
                &task_id,
                recorded_at,
                args.clone(),
            )?,
            ContractMutationOperationInput::Retire => {
                build_retired_contract_packet(prism.as_ref(), &task_id, recorded_at, args.clone())?
            }
            ContractMutationOperationInput::AttachEvidence => {
                build_contract_with_evidence_attached(
                    prism.as_ref(),
                    &task_id,
                    recorded_at,
                    args.clone(),
                )?
            }
            ContractMutationOperationInput::AttachValidation => {
                build_contract_with_validation_attached(
                    prism.as_ref(),
                    Some(workspace),
                    workspace_root,
                    &task_id,
                    recorded_at,
                    args.clone(),
                )?
            }
            ContractMutationOperationInput::RecordConsumer => {
                build_contract_with_consumer_recorded(
                    prism.as_ref(),
                    Some(workspace),
                    workspace_root,
                    &task_id,
                    recorded_at,
                    args.clone(),
                )?
            }
            ContractMutationOperationInput::SetStatus => {
                build_contract_with_status_set(prism.as_ref(), &task_id, recorded_at, args.clone())?
            }
        };
        let patch = contract_event_patch(&args, &operation, &packet)?;
        let mut event = ContractEvent {
            id: next_contract_event_id(),
            recorded_at,
            task_id: Some(task_id.0.to_string()),
            actor: None,
            execution_context: None,
            action: match operation {
                ContractMutationOperationInput::Promote => ContractEventAction::Promote,
                ContractMutationOperationInput::Update => ContractEventAction::Update,
                ContractMutationOperationInput::Retire => ContractEventAction::Retire,
                ContractMutationOperationInput::AttachEvidence => {
                    ContractEventAction::AttachEvidence
                }
                ContractMutationOperationInput::AttachValidation => {
                    ContractEventAction::AttachValidation
                }
                ContractMutationOperationInput::RecordConsumer => {
                    ContractEventAction::RecordConsumer
                }
                ContractMutationOperationInput::SetStatus => ContractEventAction::SetStatus,
            },
            patch,
            contract: packet.clone(),
        };
        mutation_provenance(self, session, authenticated).stamp_contract_event(&mut event);
        workspace.append_contract_event(event.clone())?;
        self.sync_workspace_revision(workspace)?;
        Ok(ContractMutationResult {
            event_id: event.id,
            contract_handle: packet.handle.clone(),
            task_id: task_id.0.to_string(),
            packet: contract_packet_view(prism.as_ref(), self.workspace_root(), packet, None),
        })
    }

    #[cfg_attr(not(test), allow(dead_code))]
    pub(crate) fn store_concept_relation(
        &self,
        session: &SessionState,
        args: PrismConceptRelationMutationArgs,
    ) -> Result<ConceptRelationMutationResult> {
        self.store_concept_relation_authenticated(session, args, None)
    }

    pub(crate) fn store_concept_relation_authenticated(
        &self,
        session: &SessionState,
        args: PrismConceptRelationMutationArgs,
        authenticated: Option<&AuthenticatedPrincipal>,
    ) -> Result<ConceptRelationMutationResult> {
        let workspace = self.workspace_session().ok_or_else(|| {
            anyhow!("concept relation mutations require a workspace-backed PRISM session")
        })?;
        let prism = self.current_prism();
        let task_id = session.task_for_mutation(args.task_id.clone().map(TaskId::new));
        let relation = build_concept_relation(prism.as_ref(), &task_id, &args)?;
        let mut event = ConceptRelationEvent {
            id: next_concept_relation_event_id(),
            recorded_at: current_timestamp(),
            task_id: Some(task_id.0.to_string()),
            actor: None,
            execution_context: None,
            action: match args.operation {
                ConceptRelationMutationOperationInput::Upsert => ConceptRelationEventAction::Upsert,
                ConceptRelationMutationOperationInput::Retire => ConceptRelationEventAction::Retire,
            },
            relation: relation.clone(),
        };
        mutation_provenance(self, session, authenticated).stamp_concept_relation_event(&mut event);
        workspace.append_concept_relation_event(event.clone())?;
        self.sync_workspace_revision(workspace)?;
        let focus_handle = relation.source_handle.clone();
        Ok(ConceptRelationMutationResult {
            event_id: event.id,
            task_id: task_id.0.to_string(),
            relation: concept_relation_view(prism.as_ref(), &focus_handle, relation),
        })
    }

    #[allow(dead_code)]
    pub(crate) fn store_validation_feedback(
        &self,
        session: &SessionState,
        args: PrismValidationFeedbackArgs,
    ) -> Result<ValidationFeedbackMutationResult> {
        self.store_validation_feedback_without_refresh_authenticated(session, args, None)
    }

    #[allow(dead_code)]
    pub(crate) fn store_validation_feedback_without_refresh(
        &self,
        session: &SessionState,
        args: PrismValidationFeedbackArgs,
    ) -> Result<ValidationFeedbackMutationResult> {
        self.store_validation_feedback_without_refresh_authenticated(session, args, None)
    }

    pub(crate) fn store_validation_feedback_without_refresh_authenticated(
        &self,
        session: &SessionState,
        args: PrismValidationFeedbackArgs,
        authenticated: Option<&AuthenticatedPrincipal>,
    ) -> Result<ValidationFeedbackMutationResult> {
        let prism = self.current_prism();
        let task_id = session.task_for_mutation(args.task_id.map(TaskId::new));
        let anchors = prism.anchors_for(&convert_anchors(
            prism.as_ref(),
            self.workspace_session_ref(),
            self.workspace_root(),
            args.anchors.unwrap_or_default(),
        )?);
        let workspace = self.workspace_session().ok_or_else(|| {
            anyhow!("validation feedback logging requires a workspace-backed PRISM session")
        })?;
        let mut record = ValidationFeedbackRecord {
            task_id: Some(task_id.0.to_string()),
            actor: None,
            execution_context: None,
            context: args.context,
            anchors,
            prism_said: args.prism_said,
            actually_true: args.actually_true,
            category: convert_validation_feedback_category(args.category),
            verdict: convert_validation_feedback_verdict(args.verdict),
            corrected_manually: args.corrected_manually.unwrap_or(false),
            correction: args.correction,
            metadata: args.metadata.unwrap_or(Value::Null),
        };
        mutation_provenance(self, session, authenticated)
            .stamp_validation_feedback_record(&mut record);
        let entry = workspace.append_validation_feedback(record)?;
        Ok(ValidationFeedbackMutationResult {
            entry_id: entry.id,
            task_id: task_id.0.to_string(),
        })
    }

    pub(crate) fn store_inferred_edge(
        &self,
        session: &SessionState,
        args: PrismInferEdgeArgs,
    ) -> Result<EdgeMutationResult> {
        let task = session.task_for_mutation(args.task_id.map(TaskId::new));
        let edge = Edge {
            kind: parse_edge_kind(&args.kind)?,
            source: convert_node_id(args.source)?,
            target: convert_node_id(args.target)?,
            origin: EdgeOrigin::Inferred,
            confidence: args.confidence.clamp(0.0, 1.0),
        };
        let scope = args
            .scope
            .map(convert_inferred_scope)
            .unwrap_or(prism_agent::InferredEdgeScope::SessionOnly);
        let id = session.inferred_edges.store_edge(
            edge,
            scope,
            Some(task.clone()),
            args.evidence.unwrap_or_default(),
        );
        if scope != prism_agent::InferredEdgeScope::SessionOnly {
            if let Some(workspace) = self.workspace_session() {
                let record = session.inferred_edges.record(&id).ok_or_else(|| {
                    anyhow!("stored inferred edge `{}` could not be reloaded", id.0)
                })?;
                workspace.append_inference_records(&[record])?;
                self.sync_inference_revision(workspace)?;
            } else {
                self.persist_inferred_edges()?;
            }
        }
        Ok(EdgeMutationResult {
            edge_id: id.0,
            task_id: task.0.to_string(),
        })
    }

    #[cfg_attr(not(test), allow(dead_code))]
    pub(crate) fn store_coordination(
        &self,
        session: &SessionState,
        args: PrismCoordinationArgs,
    ) -> Result<CoordinationMutationResult> {
        self.store_coordination_with_trace_authenticated(session, args, None, None)
    }

    #[cfg_attr(not(test), allow(dead_code))]
    pub(crate) fn store_coordination_traced(
        &self,
        session: &SessionState,
        args: PrismCoordinationArgs,
        trace: &MutationRun,
    ) -> Result<CoordinationMutationResult> {
        self.store_coordination_with_trace_authenticated(session, args, Some(trace), None)
    }

    pub(crate) fn store_coordination_traced_authenticated(
        &self,
        session: &SessionState,
        args: PrismCoordinationArgs,
        trace: &MutationRun,
        authenticated: Option<&AuthenticatedPrincipal>,
    ) -> Result<CoordinationMutationResult> {
        self.store_coordination_with_trace_authenticated(session, args, Some(trace), authenticated)
    }

    fn store_coordination_with_trace_authenticated(
        &self,
        session: &SessionState,
        args: PrismCoordinationArgs,
        trace: Option<&MutationRun>,
        authenticated: Option<&AuthenticatedPrincipal>,
    ) -> Result<CoordinationMutationResult> {
        self.ensure_tool_enabled("prism_coordination", "coordination workflow mutations")?;
        if let Some(result) =
            self.maybe_handle_git_execution_coordination(session, &args, trace, authenticated)?
        {
            return Ok(result);
        }
        let prism = self.current_prism();
        let before_events = prism.coordination_events().len();
        let task = session.task_for_mutation(args.task_id.clone().map(TaskId::new));
        let args_kind = args.kind.clone();
        let event_id = session.next_event_id("coordination");
        let meta = mutation_provenance(self, session, authenticated).event_meta(
            event_id.clone(),
            Some(task),
            None,
            current_timestamp(),
        );
        if let Some(workspace) = self.workspace_session() {
            let result = if let Some(trace) = trace {
                match workspace.mutate_coordination_with_session_wait_observed(
                    Some(&session.session_id()),
                    |prism| {
                        let mutation_kind = format!("{:?}", args.kind);
                        let sync_started = std::time::Instant::now();
                        match self.ensure_coordination_runtime_current(workspace) {
                            Ok(hydrated) => {
                                trace.record_phase(
                                    "mutation.coordination.syncLoadedRevisionBefore",
                                    &json!({
                                        "loadedRevision": self
                                            .loaded_coordination_revision_value(),
                                        "hydrated": hydrated,
                                    }),
                                    sync_started.elapsed(),
                                    true,
                                    None,
                                );
                            }
                            Err(error) => {
                                trace.record_phase(
                                    "mutation.coordination.syncLoadedRevisionBefore",
                                    &json!({}),
                                    sync_started.elapsed(),
                                    false,
                                    Some(error.to_string()),
                                );
                                return Err(error);
                            }
                        }
                        let apply_started = std::time::Instant::now();
                        let state =
                            self.apply_coordination_mutation(session, prism, args, meta.clone());
                        record_optional_trace_result(
                            Some(trace),
                            "mutation.coordination.applyRequestedMutation",
                            json!({
                                "kind": mutation_kind,
                            }),
                            apply_started,
                            &state,
                        );
                        state
                    },
                    |operation, duration, args, success, error| {
                        trace.record_phase(operation, &args, duration, success, error)
                    },
                ) {
                    Ok(Some(state)) => Ok(state),
                    Ok(None) => Err(AdmissionBusyError::refresh_lock("mutateCoordination").into()),
                    Err(error) => Err(error),
                }
            } else {
                match workspace.mutate_coordination_with_session_wait_observed(
                    Some(&session.session_id()),
                    |prism| {
                        let _ = self.ensure_coordination_runtime_current(workspace)?;
                        self.apply_coordination_mutation(session, prism, args, meta.clone())
                    },
                    |_operation, _duration, _args, _success, _error| {},
                ) {
                    Ok(Some(state)) => Ok(state),
                    Ok(None) => Err(AdmissionBusyError::refresh_lock("mutateCoordination").into()),
                    Err(error) => Err(error),
                }
            };
            match result {
                Ok(state) => {
                    let sync_started = std::time::Instant::now();
                    match self.sync_coordination_revision(workspace) {
                        Ok(()) => {
                            if let Some(trace) = trace {
                                trace.record_phase(
                                    "mutation.coordination.syncLoadedRevisionAfter",
                                    &json!({
                                        "loadedRevision": self.loaded_coordination_revision_value(),
                                    }),
                                    sync_started.elapsed(),
                                    true,
                                    None,
                                );
                            }
                        }
                        Err(error) => {
                            if let Some(trace) = trace {
                                trace.record_phase(
                                    "mutation.coordination.syncLoadedRevisionAfter",
                                    &json!({}),
                                    sync_started.elapsed(),
                                    false,
                                    Some(error.to_string()),
                                );
                            }
                            return Err(error);
                        }
                    }
                    let prism = self.current_prism();
                    let sync_session_started = std::time::Instant::now();
                    let sync_session_result = sync_session_after_coordination_mutation(
                        self,
                        session,
                        prism.as_ref(),
                        &args_kind,
                        &state,
                    );
                    record_optional_trace_result(
                        trace,
                        "mutation.coordination.syncSessionAfter",
                        json!({
                            "kind": format!("{:?}", args_kind),
                        }),
                        sync_session_started,
                        &sync_session_result,
                    );
                    sync_session_result?;
                    let persist_seed_started = std::time::Instant::now();
                    let persist_seed_result = self.persist_session_seed(session);
                    record_optional_trace_result(
                        trace,
                        "mutation.coordination.persistSessionSeed",
                        json!({
                            "kind": format!("{:?}", args_kind),
                        }),
                        persist_seed_started,
                        &persist_seed_result,
                    );
                    persist_seed_result?;
                    let audit_started = std::time::Instant::now();
                    let audit = coordination_audit_since(prism.as_ref(), before_events);
                    record_optional_trace_phase(
                        trace,
                        "mutation.coordination.auditSince",
                        json!({
                            "eventCount": audit.event_ids.len(),
                            "violationCount": audit.violations.len(),
                            "rejected": false,
                        }),
                        audit_started,
                        true,
                        None,
                    );
                    return Ok(CoordinationMutationResult {
                        event_id: event_id.0.to_string(),
                        event_ids: audit.event_ids,
                        rejected: false,
                        violations: audit.violations,
                        state,
                    });
                }
                Err(error) => {
                    let prism = self.current_prism();
                    let audit_started = std::time::Instant::now();
                    let audit = coordination_audit_since(prism.as_ref(), before_events);
                    record_optional_trace_phase(
                        trace,
                        "mutation.coordination.auditSince",
                        json!({
                            "eventCount": audit.event_ids.len(),
                            "violationCount": audit.violations.len(),
                            "rejected": audit.rejected,
                        }),
                        audit_started,
                        true,
                        None,
                    );
                    if audit.rejected && !audit.event_ids.is_empty() {
                        let sync_started = std::time::Instant::now();
                        match self.sync_coordination_revision(workspace) {
                            Ok(()) => {
                                if let Some(trace) = trace {
                                    trace.record_phase(
                                        "mutation.coordination.syncLoadedRevisionAfter",
                                        &json!({
                                            "loadedRevision": self.loaded_coordination_revision_value(),
                                        }),
                                        sync_started.elapsed(),
                                        true,
                                        None,
                                    );
                                }
                            }
                            Err(sync_error) => {
                                if let Some(trace) = trace {
                                    trace.record_phase(
                                        "mutation.coordination.syncLoadedRevisionAfter",
                                        &json!({}),
                                        sync_started.elapsed(),
                                        false,
                                        Some(sync_error.to_string()),
                                    );
                                }
                                return Err(sync_error);
                            }
                        }
                        return Ok(CoordinationMutationResult {
                            event_id: audit
                                .event_ids
                                .first()
                                .cloned()
                                .unwrap_or_else(|| event_id.0.to_string()),
                            event_ids: audit.event_ids,
                            rejected: true,
                            violations: audit.violations,
                            state: Value::Null,
                        });
                    }
                    return Err(error);
                }
            }
        }
        let state =
            match self.apply_coordination_mutation(session, prism.as_ref(), args, meta.clone()) {
                Ok(state) => state,
                Err(error) => {
                    let audit = coordination_audit_since(prism.as_ref(), before_events);
                    if audit.rejected && !audit.event_ids.is_empty() {
                        return Ok(CoordinationMutationResult {
                            event_id: audit
                                .event_ids
                                .first()
                                .cloned()
                                .unwrap_or_else(|| event_id.0.to_string()),
                            event_ids: audit.event_ids,
                            rejected: true,
                            violations: audit.violations,
                            state: Value::Null,
                        });
                    }
                    return Err(error);
                }
            };
        let audit = coordination_audit_since(prism.as_ref(), before_events);
        sync_session_after_coordination_mutation(
            self,
            session,
            prism.as_ref(),
            &args_kind,
            &state,
        )?;
        self.persist_session_seed(session)?;
        Ok(CoordinationMutationResult {
            event_id: event_id.0.to_string(),
            event_ids: audit.event_ids,
            rejected: false,
            violations: audit.violations,
            state,
        })
    }

    fn maybe_handle_git_execution_coordination(
        &self,
        session: &SessionState,
        args: &PrismCoordinationArgs,
        trace: Option<&MutationRun>,
        authenticated: Option<&AuthenticatedPrincipal>,
    ) -> Result<Option<CoordinationMutationResult>> {
        let Some(workspace) = self.workspace_session() else {
            return Ok(None);
        };
        let sync_started = std::time::Instant::now();
        let sync_result = self.ensure_coordination_runtime_current(workspace);
        let sync_args = match &sync_result {
            Ok(hydrated) => json!({ "hydrated": hydrated }),
            Err(_) => json!({}),
        };
        record_optional_trace_result(
            trace,
            "mutation.gitExecution.syncLoadedRevisionBefore",
            sync_args,
            sync_started,
            &sync_result,
        );
        sync_result?;
        let prism = self.current_prism();
        let Some(request) = git_execution_request(prism.as_ref(), args)? else {
            return Ok(None);
        };
        let root = self
            .workspace_root()
            .ok_or_else(|| anyhow!("git execution workflow requires a workspace root"))?;
        let task = prism
            .coordination_task(&request.task_id)
            .ok_or_else(|| anyhow!("unknown coordination task `{}`", request.task_id.0))?;
        let plan = prism
            .coordination_plan(&task.plan)
            .ok_or_else(|| anyhow!("unknown coordination plan `{}`", task.plan.0))?;
        let policy = plan.policy.git_execution.clone();
        let completion_status = match &request.workflow {
            GitExecutionWorkflow::Complete(desired_status) => Some(*desired_status),
            GitExecutionWorkflow::Start => None,
        };
        let mode_enabled = match request.workflow {
            GitExecutionWorkflow::Start => !matches!(policy.start_mode, GitExecutionStartMode::Off),
            GitExecutionWorkflow::Complete(_) => {
                !matches!(policy.completion_mode, GitExecutionCompletionMode::Off)
            }
        };
        if !mode_enabled {
            return Ok(None);
        }

        let before_events = prism.coordination_events().len();
        let now = current_timestamp();
        let require_clean_worktree = matches!(request.workflow, GitExecutionWorkflow::Start);
        let preflight_started = std::time::Instant::now();
        let preflight_result = run_preflight(root, &policy, now, require_clean_worktree);
        record_optional_trace_result(
            trace,
            "mutation.gitExecution.preflight",
            json!({
                "workflow": format!("{:?}", request.workflow),
                "requireCleanWorktree": require_clean_worktree,
            }),
            preflight_started,
            &preflight_result,
        );
        let preflight = preflight_result?;
        let mut post_sync_publish_branch: Option<String> = None;
        if let Some(failure) = preflight.report.failure.clone() {
            let record_started = std::time::Instant::now();
            let record_result = self.record_task_git_execution(
                session,
                authenticated,
                &request.task_id,
                task_git_execution_record(
                    &task.git_execution,
                    &policy,
                    &preflight.report,
                    prism_ir::GitExecutionStatus::PreflightFailed,
                    None,
                    None,
                    prism_ir::GitIntegrationStatus::NotStarted,
                ),
                trace,
            );
            record_optional_trace_result(
                trace,
                "mutation.gitExecution.recordTaskGitExecution",
                json!({
                    "status": "preflight_failed",
                    "taskId": request.task_id.0.as_str(),
                }),
                record_started,
                &record_result,
            );
            record_result?;
            return Err(anyhow!(failure));
        }

        match request.workflow {
            GitExecutionWorkflow::Start => {
                let raw_started = std::time::Instant::now();
                let raw_result = self.run_start_coordination_args_with_git_execution(
                    session,
                    authenticated,
                    &request.task_id,
                    args.clone(),
                    task_git_execution_record(
                        &task.git_execution,
                        &policy,
                        &preflight.report,
                        prism_ir::GitExecutionStatus::InProgress,
                        None,
                        None,
                        prism_ir::GitIntegrationStatus::NotStarted,
                    ),
                    trace,
                );
                record_optional_trace_result(
                    trace,
                    "mutation.gitExecution.applyRequestedMutation",
                    json!({
                        "workflow": "start",
                        "taskId": request.task_id.0.as_str(),
                    }),
                    raw_started,
                    &raw_result,
                );
                if let Err(error) = raw_result {
                    let prism = self.current_prism();
                    let audit = coordination_audit_since(prism.as_ref(), before_events);
                    if audit.rejected && !audit.event_ids.is_empty() {
                        return Ok(Some(CoordinationMutationResult {
                            event_id: audit
                                .event_ids
                                .last()
                                .cloned()
                                .unwrap_or_else(|| format!("coordination:{}", request.task_id.0)),
                            event_ids: audit.event_ids,
                            rejected: true,
                            violations: audit.violations,
                            state: Value::Null,
                        }));
                    }
                    return Err(error);
                }
            }
            GitExecutionWorkflow::Complete(desired_status) => {
                post_sync_publish_branch = Some(preflight.current_branch.clone());
                let dirty_user_paths = user_dirty_paths(&preflight.report.dirty_paths);
                let (code_commit, code_commit_sha) = match policy.completion_mode {
                    GitExecutionCompletionMode::Require => {
                        if !dirty_user_paths.is_empty() {
                            let failure = format!(
                                "completion mode `require` needs user changes committed before completion; dirty user paths: {}",
                                dirty_user_paths.join(", ")
                            );
                            let failed_publish = GitPublishReport {
                                attempted_at: now,
                                publish_ref: Some(preflight.current_branch.clone()),
                                code_commit: None,
                                coordination_commit: None,
                                pushed_ref: None,
                                staged_paths: dirty_user_paths.clone(),
                                protected_paths: Vec::new(),
                                failure: Some(failure.clone()),
                            };
                            let record_started = std::time::Instant::now();
                            let record_result = self.record_task_git_execution(
                                session,
                                authenticated,
                                &request.task_id,
                                task_git_execution_record(
                                    &task.git_execution,
                                    &policy,
                                    &preflight.report,
                                    prism_ir::GitExecutionStatus::PublishFailed,
                                    Some(desired_status),
                                    Some(failed_publish),
                                    prism_ir::GitIntegrationStatus::NotStarted,
                                ),
                                trace,
                            );
                            record_optional_trace_result(
                                trace,
                                "mutation.gitExecution.recordTaskGitExecution",
                                json!({
                                    "status": "publish_failed",
                                    "reason": "dirty_user_paths",
                                    "taskId": request.task_id.0.as_str(),
                                }),
                                record_started,
                                &record_result,
                            );
                            record_result?;
                            return Err(anyhow!(failure));
                        }
                        let current_head = head_commit(root)?;
                        (
                            GitPublishReport {
                                attempted_at: now,
                                publish_ref: Some(preflight.current_branch.clone()),
                                code_commit: Some(current_head.clone()),
                                coordination_commit: None,
                                pushed_ref: None,
                                staged_paths: Vec::new(),
                                protected_paths: Vec::new(),
                                failure: None,
                            },
                            current_head,
                        )
                    }
                    GitExecutionCompletionMode::Off => {
                        unreachable!("completion workflow is gated above")
                    }
                };
                let publish_intent_started = std::time::Instant::now();
                let publish_intent_result = self.record_task_publish_intent(
                    session,
                    authenticated,
                    &request.task_id,
                    desired_status,
                    task_git_execution_record(
                        &task.git_execution,
                        &policy,
                        &preflight.report,
                        prism_ir::GitExecutionStatus::PublishPending,
                        Some(desired_status),
                        Some(code_commit.clone()),
                        prism_ir::GitIntegrationStatus::NotStarted,
                    ),
                    trace,
                );
                record_optional_trace_result(
                    trace,
                    "mutation.gitExecution.recordPublishIntent",
                    json!({
                        "desiredStatus": format!("{desired_status:?}"),
                        "taskId": request.task_id.0.as_str(),
                    }),
                    publish_intent_started,
                    &publish_intent_result,
                );
                if let Err(error) = publish_intent_result {
                    let failure = error.to_string();
                    let mut failed_publish = code_commit.clone();
                    failed_publish.failure = Some(failure.clone());
                    let authoritative_started = std::time::Instant::now();
                    let authoritative_result = self.record_task_git_execution_authoritative_state(
                        session,
                        authenticated,
                        &request.task_id,
                        Some(task.status),
                        Some(None),
                        task_git_execution_record(
                            &task.git_execution,
                            &policy,
                            &preflight.report,
                            prism_ir::GitExecutionStatus::PublishFailed,
                            Some(desired_status),
                            Some(failed_publish),
                            prism_ir::GitIntegrationStatus::NotStarted,
                        ),
                        trace,
                    );
                    record_optional_trace_result(
                        trace,
                        "mutation.gitExecution.recordAuthoritativeState",
                        json!({
                            "status": "publish_failed",
                            "taskId": request.task_id.0.as_str(),
                        }),
                        authoritative_started,
                        &authoritative_result,
                    );
                    authoritative_result?;
                    let dirty_paths = worktree_dirty_paths(root)?;
                    let managed_paths = prism_managed_paths(&dirty_paths);
                    restore_prism_managed_paths(root, &managed_paths)?;
                    if user_dirty_paths(&dirty_paths).is_empty() {
                        restore_prism_managed_roots(root)?;
                    }
                    let prism = self.current_prism();
                    let audit = coordination_audit_since(prism.as_ref(), before_events);
                    if audit.rejected && !audit.event_ids.is_empty() {
                        return Ok(Some(CoordinationMutationResult {
                            event_id: audit
                                .event_ids
                                .last()
                                .cloned()
                                .unwrap_or_else(|| format!("coordination:{}", request.task_id.0)),
                            event_ids: audit.event_ids,
                            rejected: true,
                            violations: audit.violations,
                            state: Value::Null,
                        }));
                    }
                    return Err(anyhow!(failure));
                }
                let coordination_commit_message = match desired_status {
                    prism_ir::CoordinationTaskStatus::Abandoned => {
                        format!("prism: abandon {}", task.title)
                    }
                    _ => format!("prism: complete {}", task.title),
                };
                let mut published = GitPublishReport {
                    attempted_at: current_timestamp(),
                    publish_ref: Some(preflight.current_branch.clone()),
                    code_commit: Some(code_commit_sha),
                    coordination_commit: None,
                    pushed_ref: None,
                    staged_paths: Vec::new(),
                    protected_paths: Vec::new(),
                    failure: None,
                };
                let push_started = std::time::Instant::now();
                let push_result =
                    push_current_branch(root, &preflight.current_branch, &mut published);
                record_optional_trace_result(
                    trace,
                    "mutation.gitExecution.pushBranch",
                    json!({
                        "branch": preflight.current_branch.as_str(),
                        "taskId": request.task_id.0.as_str(),
                    }),
                    push_started,
                    &push_result,
                );
                if let Err(error) = push_result {
                    let failure = error.to_string();
                    published.failure = Some(failure.clone());
                    let authoritative_started = std::time::Instant::now();
                    let authoritative_result = self.record_task_git_execution_authoritative_state(
                        session,
                        authenticated,
                        &request.task_id,
                        Some(task.status),
                        Some(None),
                        task_git_execution_record(
                            &task.git_execution,
                            &policy,
                            &preflight.report,
                            prism_ir::GitExecutionStatus::PublishFailed,
                            Some(desired_status),
                            Some(published),
                            prism_ir::GitIntegrationStatus::NotStarted,
                        ),
                        trace,
                    );
                    record_optional_trace_result(
                        trace,
                        "mutation.gitExecution.recordAuthoritativeState",
                        json!({
                            "status": "publish_failed",
                            "taskId": request.task_id.0.as_str(),
                        }),
                        authoritative_started,
                        &authoritative_result,
                    );
                    authoritative_result?;
                    let final_authoritative_paths = worktree_dirty_paths(root)?;
                    let unexpected_user_paths = user_dirty_paths(&final_authoritative_paths);
                    if !unexpected_user_paths.is_empty() {
                        return Err(anyhow!(
                            "failed publish left unexpected dirty user paths: {}",
                            unexpected_user_paths.join(", ")
                        ));
                    }
                    let final_authoritative_prism_paths =
                        prism_managed_paths(&final_authoritative_paths);
                    if !final_authoritative_prism_paths.is_empty() {
                        let _ = commit_paths(
                            root,
                            &format!("prism: record failed publish {}", task.title),
                            current_timestamp(),
                            &final_authoritative_prism_paths,
                        )?;
                    }
                    return Err(anyhow!(failure));
                }
                let authoritative_started = std::time::Instant::now();
                let authoritative_result = self.record_task_git_execution_authoritative_state(
                    session,
                    authenticated,
                    &request.task_id,
                    Some(desired_status),
                    Some(None),
                    task_git_execution_record(
                        &task.git_execution,
                        &policy,
                        &preflight.report,
                        prism_ir::GitExecutionStatus::CoordinationPublished,
                        None,
                        Some(published.clone()),
                        integration_status_after_coordination_publication(policy.integration_mode),
                    ),
                    trace,
                );
                record_optional_trace_result(
                    trace,
                    "mutation.gitExecution.recordAuthoritativeState",
                    json!({
                        "status": "coordination_published",
                        "taskId": request.task_id.0.as_str(),
                    }),
                    authoritative_started,
                    &authoritative_result,
                );
                authoritative_result?;
                ensure_auto_pr_review_artifact(
                    self,
                    session,
                    authenticated,
                    &request.task_id,
                    trace,
                )?;
                self.flush_workspace_prism_managed_outputs(workspace, &request.task_id, trace)?;
                let final_authoritative_paths = worktree_dirty_paths(root)?;
                let unexpected_user_paths = user_dirty_paths(&final_authoritative_paths);
                if !unexpected_user_paths.is_empty() {
                    return Err(anyhow!(
                        "final authoritative acknowledgement left unexpected dirty user paths: {}",
                        unexpected_user_paths.join(", ")
                    ));
                }
                let final_authoritative_prism_paths =
                    prism_managed_paths(&final_authoritative_paths);
                if !final_authoritative_prism_paths.is_empty() {
                    let finalize_commit_started = std::time::Instant::now();
                    let finalize_commit_result = commit_paths(
                        root,
                        &coordination_commit_message,
                        current_timestamp(),
                        &final_authoritative_prism_paths,
                    );
                    record_optional_trace_result(
                        trace,
                        "mutation.gitExecution.commitProtectedPaths",
                        json!({
                            "finalizeAuthoritative": true,
                            "pathCount": final_authoritative_prism_paths.len(),
                            "taskId": request.task_id.0.as_str(),
                        }),
                        finalize_commit_started,
                        &finalize_commit_result,
                    );
                    let finalize_commit = finalize_commit_result?;
                    let finalize_commit_sha =
                        finalize_commit.code_commit.clone().ok_or_else(|| {
                            anyhow!("missing coordination commit sha after final PRISM git commit")
                        })?;
                    published.coordination_commit = Some(finalize_commit_sha);
                    published.staged_paths.extend(finalize_commit.staged_paths);
                    published
                        .protected_paths
                        .extend(finalize_commit.protected_paths);
                    published.staged_paths.sort();
                    published.staged_paths.dedup();
                    published.protected_paths.sort();
                    published.protected_paths.dedup();
                    let finalize_push_started = std::time::Instant::now();
                    let finalize_push_result =
                        push_current_branch(root, &preflight.current_branch, &mut published);
                    record_optional_trace_result(
                        trace,
                        "mutation.gitExecution.pushBranch",
                        json!({
                            "finalizeAuthoritative": true,
                            "branch": preflight.current_branch.as_str(),
                            "taskId": request.task_id.0.as_str(),
                        }),
                        finalize_push_started,
                        &finalize_push_result,
                    );
                    finalize_push_result?;
                    let finalize_record_started = std::time::Instant::now();
                    let refreshed_task = self
                        .current_prism()
                        .coordination_task(&request.task_id)
                        .ok_or_else(|| {
                        anyhow!(
                            "missing coordination task `{}` after review artifact refresh",
                            request.task_id.0
                        )
                    })?;
                    let finalize_record_result = self
                        .record_task_git_execution_with_materialization(
                            session,
                            authenticated,
                            &request.task_id,
                            task_git_execution_record(
                                &refreshed_task.git_execution,
                                &policy,
                                &preflight.report,
                                prism_ir::GitExecutionStatus::CoordinationPublished,
                                None,
                                Some(published.clone()),
                                integration_status_after_coordination_publication(
                                    policy.integration_mode,
                                ),
                            ),
                            trace,
                            false,
                        );
                    record_optional_trace_result(
                        trace,
                        "mutation.gitExecution.recordTaskGitExecution",
                        json!({
                            "finalizeAuthoritative": true,
                            "status": "published",
                            "taskId": request.task_id.0.as_str(),
                        }),
                        finalize_record_started,
                        &finalize_record_result,
                    );
                    finalize_record_result?;
                    ensure_auto_pr_review_artifact(
                        self,
                        session,
                        authenticated,
                        &request.task_id,
                        trace,
                    )?;
                }
            }
        }

        let prism = self.current_prism();
        let audit_started = std::time::Instant::now();
        let audit = coordination_audit_since(prism.as_ref(), before_events);
        record_optional_trace_phase(
            trace,
            "mutation.gitExecution.auditSince",
            json!({
                "eventCount": audit.event_ids.len(),
                "violationCount": audit.violations.len(),
            }),
            audit_started,
            true,
            None,
        );
        let state = self.current_task_state_value(&request.task_id)?;
        let sync_session_started = std::time::Instant::now();
        let sync_session_result = sync_session_after_coordination_mutation(
            self,
            session,
            prism.as_ref(),
            &args.kind,
            &state,
        );
        record_optional_trace_result(
            trace,
            "mutation.gitExecution.syncSessionAfter",
            json!({
                "taskId": request.task_id.0.as_str(),
            }),
            sync_session_started,
            &sync_session_result,
        );
        sync_session_result?;
        let persist_seed_started = std::time::Instant::now();
        let persist_seed_result = self.persist_session_seed(session);
        record_optional_trace_result(
            trace,
            "mutation.gitExecution.persistSessionSeed",
            json!({
                "taskId": request.task_id.0.as_str(),
            }),
            persist_seed_started,
            &persist_seed_result,
        );
        persist_seed_result?;
        if let Some(branch) = post_sync_publish_branch {
            let post_sync_paths = worktree_dirty_paths(root)?;
            let unexpected_user_paths = user_dirty_paths(&post_sync_paths);
            if !unexpected_user_paths.is_empty() {
                return Err(anyhow!(
                    "post-sync publication left unexpected dirty user paths: {}",
                    unexpected_user_paths.join(", ")
                ));
            }
            let post_sync_prism_paths = prism_managed_paths(&post_sync_paths);
            if !post_sync_prism_paths.is_empty() {
                let post_sync_commit_started = std::time::Instant::now();
                let post_sync_commit_result = commit_paths(
                    root,
                    "prism: finalize coordination publication",
                    current_timestamp(),
                    &post_sync_prism_paths,
                );
                record_optional_trace_result(
                    trace,
                    "mutation.gitExecution.commitProtectedPaths",
                    json!({
                        "postSyncFinalize": true,
                        "pathCount": post_sync_prism_paths.len(),
                        "taskId": request.task_id.0.as_str(),
                    }),
                    post_sync_commit_started,
                    &post_sync_commit_result,
                );
                let _ = post_sync_commit_result?;
                let post_sync_push_started = std::time::Instant::now();
                let post_sync_push_result = push_current_branch(
                    root,
                    &branch,
                    &mut GitPublishReport {
                        attempted_at: current_timestamp(),
                        publish_ref: Some(branch.clone()),
                        code_commit: None,
                        coordination_commit: None,
                        pushed_ref: None,
                        staged_paths: Vec::new(),
                        protected_paths: Vec::new(),
                        failure: None,
                    },
                );
                record_optional_trace_result(
                    trace,
                    "mutation.gitExecution.pushBranch",
                    json!({
                        "postSyncFinalize": true,
                        "branch": branch,
                        "taskId": request.task_id.0.as_str(),
                    }),
                    post_sync_push_started,
                    &post_sync_push_result,
                );
                post_sync_push_result?;
            }
        }
        if let Some(desired_status) = completion_status {
            if matches!(
                policy.integration_mode,
                prism_ir::GitIntegrationMode::DirectIntegrate
            ) {
                let published_commit = head_commit(root)?;
                let direct_integrate_started = std::time::Instant::now();
                let direct_integrate_result = direct_integrate_published_branch(
                    root,
                    &preflight.current_branch,
                    &policy,
                    &published_commit,
                );
                record_optional_trace_result(
                    trace,
                    "mutation.gitExecution.directIntegrate",
                    json!({
                        "branch": preflight.current_branch.as_str(),
                        "targetBranch": policy.target_branch.as_str(),
                        "taskId": request.task_id.0.as_str(),
                    }),
                    direct_integrate_started,
                    &direct_integrate_result,
                );
                match direct_integrate_result {
                    Ok(integration) => {
                        let current_task = self
                            .current_prism()
                            .coordination_task(&request.task_id)
                            .ok_or_else(|| {
                                anyhow!(
                                    "unknown coordination task `{}` after direct integration",
                                    request.task_id.0
                                )
                            })?;
                        let authoritative_started = std::time::Instant::now();
                        let authoritative_result = self
                            .record_task_git_execution_authoritative_state(
                                session,
                                authenticated,
                                &request.task_id,
                                Some(desired_status),
                                Some(None),
                                task_git_execution_with_direct_integration(
                                    &current_task.git_execution,
                                    integration.target_commit,
                                    integration.record_ref,
                                ),
                                trace,
                            );
                        record_optional_trace_result(
                            trace,
                            "mutation.gitExecution.recordAuthoritativeState",
                            json!({
                                "status": "integrated_to_target",
                                "taskId": request.task_id.0.as_str(),
                            }),
                            authoritative_started,
                            &authoritative_result,
                        );
                        authoritative_result?;
                    }
                    Err(error) => {
                        let failure = error.to_string();
                        let current_task = self
                            .current_prism()
                            .coordination_task(&request.task_id)
                            .ok_or_else(|| {
                                anyhow!(
                                    "unknown coordination task `{}` after failed direct integration",
                                    request.task_id.0
                                )
                            })?;
                        let authoritative_started = std::time::Instant::now();
                        let authoritative_result = self
                            .record_task_git_execution_authoritative_state(
                                session,
                                authenticated,
                                &request.task_id,
                                Some(desired_status),
                                Some(None),
                                task_git_execution_with_failed_integration(
                                    &current_task.git_execution,
                                ),
                                trace,
                            );
                        record_optional_trace_result(
                            trace,
                            "mutation.gitExecution.recordAuthoritativeState",
                            json!({
                                "status": "integration_failed",
                                "taskId": request.task_id.0.as_str(),
                            }),
                            authoritative_started,
                            &authoritative_result,
                        );
                        authoritative_result?;
                        let failed_authoritative_paths = worktree_dirty_paths(root)?;
                        let unexpected_user_paths = user_dirty_paths(&failed_authoritative_paths);
                        if !unexpected_user_paths.is_empty() {
                            return Err(anyhow!(
                                "failed direct integration left unexpected dirty user paths: {}",
                                unexpected_user_paths.join(", ")
                            ));
                        }
                        let failed_authoritative_prism_paths =
                            prism_managed_paths(&failed_authoritative_paths);
                        if !failed_authoritative_prism_paths.is_empty() {
                            let _ = commit_paths(
                                root,
                                &format!("prism: record failed integration {}", task.title),
                                current_timestamp(),
                                &failed_authoritative_prism_paths,
                            )?;
                            push_current_branch(
                                root,
                                &preflight.current_branch,
                                &mut GitPublishReport {
                                    attempted_at: current_timestamp(),
                                    publish_ref: Some(preflight.current_branch.clone()),
                                    code_commit: None,
                                    coordination_commit: None,
                                    pushed_ref: None,
                                    staged_paths: Vec::new(),
                                    protected_paths: Vec::new(),
                                    failure: None,
                                },
                            )?;
                        }
                        return Err(anyhow!(failure));
                    }
                }
                let final_authoritative_paths = worktree_dirty_paths(root)?;
                let unexpected_user_paths = user_dirty_paths(&final_authoritative_paths);
                if !unexpected_user_paths.is_empty() {
                    return Err(anyhow!(
                        "direct integration acknowledgement left unexpected dirty user paths: {}",
                        unexpected_user_paths.join(", ")
                    ));
                }
                let final_authoritative_prism_paths =
                    prism_managed_paths(&final_authoritative_paths);
                if !final_authoritative_prism_paths.is_empty() {
                    let finalize_commit_started = std::time::Instant::now();
                    let finalize_commit_result = commit_paths(
                        root,
                        &format!("prism: record direct integration {}", task.title),
                        current_timestamp(),
                        &final_authoritative_prism_paths,
                    );
                    record_optional_trace_result(
                        trace,
                        "mutation.gitExecution.commitProtectedPaths",
                        json!({
                            "directIntegrateFinalize": true,
                            "pathCount": final_authoritative_prism_paths.len(),
                            "taskId": request.task_id.0.as_str(),
                        }),
                        finalize_commit_started,
                        &finalize_commit_result,
                    );
                    let _ = finalize_commit_result?;
                    let finalize_push_started = std::time::Instant::now();
                    let finalize_push_result = push_current_branch(
                        root,
                        &preflight.current_branch,
                        &mut GitPublishReport {
                            attempted_at: current_timestamp(),
                            publish_ref: Some(preflight.current_branch.clone()),
                            code_commit: None,
                            coordination_commit: None,
                            pushed_ref: None,
                            staged_paths: Vec::new(),
                            protected_paths: Vec::new(),
                            failure: None,
                        },
                    );
                    record_optional_trace_result(
                        trace,
                        "mutation.gitExecution.pushBranch",
                        json!({
                            "directIntegrateFinalize": true,
                            "branch": preflight.current_branch.as_str(),
                            "taskId": request.task_id.0.as_str(),
                        }),
                        finalize_push_started,
                        &finalize_push_result,
                    );
                    finalize_push_result?;
                }
            }
        }
        let prism = self.current_prism();
        let audit = coordination_audit_since(prism.as_ref(), before_events);
        let state = self.current_task_state_value(&request.task_id)?;
        Ok(Some(CoordinationMutationResult {
            event_id: audit
                .event_ids
                .last()
                .cloned()
                .unwrap_or_else(|| format!("coordination:{}", request.task_id.0)),
            event_ids: audit.event_ids,
            rejected: false,
            violations: audit.violations,
            state,
        }))
    }

    fn run_start_coordination_args_with_git_execution(
        &self,
        session: &SessionState,
        authenticated: Option<&AuthenticatedPrincipal>,
        task_id: &CoordinationTaskId,
        args: PrismCoordinationArgs,
        git_execution: TaskGitExecution,
        trace: Option<&MutationRun>,
    ) -> Result<Value> {
        let workspace = self
            .workspace_session()
            .ok_or_else(|| anyhow!("git execution workflow requires a workspace-backed session"))?;
        let apply_meta = mutation_provenance(self, session, authenticated).event_meta(
            session.next_event_id("coordination"),
            Some(TaskId::new(task_id.0.clone())),
            None,
            current_timestamp(),
        );
        let record_meta = mutation_provenance(self, session, authenticated).event_meta(
            session.next_event_id("coordination"),
            Some(TaskId::new(task_id.0.clone())),
            None,
            current_timestamp(),
        );
        let task_id = task_id.clone();
        let operation_started = std::time::Instant::now();
        let result = workspace.mutate_coordination_with_session_wait_observed(
            Some(&session.session_id()),
            |prism| {
                let _ = self.ensure_coordination_runtime_current(workspace)?;
                let apply_started = std::time::Instant::now();
                let apply_result =
                    self.apply_coordination_mutation(session, prism, args, apply_meta.clone());
                record_optional_trace_result(
                    trace,
                    "mutation.gitExecution.applyRequestedMutationStep",
                    json!({ "taskId": task_id.0.as_str() }),
                    apply_started,
                    &apply_result,
                );
                let state = apply_result?;
                let record_started = std::time::Instant::now();
                let record_result = prism.update_native_task_authoritative_only(
                    record_meta,
                    TaskUpdateInput {
                        task_id: task_id.clone(),
                        kind: None,
                        status: None,
                        published_task_status: None,
                        git_execution: Some(git_execution.clone()),
                        assignee: None,
                        session: None,
                        worktree_id: None,
                        branch_ref: None,
                        title: None,
                        summary: None,
                        anchors: None,
                        bindings: None,
                        depends_on: None,
                        coordination_depends_on: None,
                        integrated_depends_on: None,
                        acceptance: None,
                        validation_refs: None,
                        is_abstract: None,
                        base_revision: Some(prism.workspace_revision()),
                        priority: None,
                        tags: None,
                        completion_context: None,
                    },
                    prism.workspace_revision(),
                    current_timestamp(),
                );
                record_optional_trace_result(
                    trace,
                    "mutation.gitExecution.recordTaskGitExecutionStep",
                    json!({ "taskId": task_id.0.as_str() }),
                    record_started,
                    &record_result,
                );
                record_result?;
                Ok(state)
            },
            |_operation, _duration, _args, _success, _error| {},
        );
        let final_result = match result {
            Ok(Some(value)) => {
                self.sync_coordination_revision(workspace)?;
                Ok(value)
            }
            Ok(None) => Err(AdmissionBusyError::refresh_lock("mutateCoordination").into()),
            Err(error) => {
                let _ = self.sync_coordination_revision(workspace);
                Err(error)
            }
        };
        record_optional_trace_result(
            trace,
            "mutation.gitExecution.applyAndRecordStartState",
            json!({ "taskId": task_id.0.as_str() }),
            operation_started,
            &final_result,
        );
        final_result
    }

    fn flush_workspace_prism_managed_outputs(
        &self,
        workspace: &WorkspaceSession,
        task_id: &CoordinationTaskId,
        trace: Option<&MutationRun>,
    ) -> Result<()> {
        let flush_started = std::time::Instant::now();
        let flush_result = workspace.flush_materializations();
        record_optional_trace_result(
            trace,
            "mutation.gitExecution.flushMaterializations",
            json!({ "taskId": task_id.0.as_str() }),
            flush_started,
            &flush_result,
        );
        flush_result?;

        let prism_doc_started = std::time::Instant::now();
        let prism_doc_result = workspace.sync_prism_doc();
        record_optional_trace_result(
            trace,
            "mutation.gitExecution.syncPrismDoc",
            json!({ "taskId": task_id.0.as_str() }),
            prism_doc_started,
            &prism_doc_result,
        );
        prism_doc_result?;
        Ok(())
    }

    fn record_task_git_execution(
        &self,
        session: &SessionState,
        authenticated: Option<&AuthenticatedPrincipal>,
        task_id: &CoordinationTaskId,
        git_execution: TaskGitExecution,
        trace: Option<&MutationRun>,
    ) -> Result<prism_coordination::CoordinationTask> {
        self.record_task_git_execution_with_materialization(
            session,
            authenticated,
            task_id,
            git_execution,
            trace,
            true,
        )
    }

    fn record_task_git_execution_with_materialization(
        &self,
        session: &SessionState,
        authenticated: Option<&AuthenticatedPrincipal>,
        task_id: &CoordinationTaskId,
        git_execution: TaskGitExecution,
        trace: Option<&MutationRun>,
        schedule_materialization: bool,
    ) -> Result<prism_coordination::CoordinationTask> {
        let task_id = task_id.clone();
        let task_ref = task_id.clone();
        self.run_workspace_coordination_step(
            session,
            authenticated,
            &task_ref,
            trace,
            "mutation.gitExecution.recordTaskGitExecutionStep",
            json!({ "taskId": task_ref.0.as_str() }),
            schedule_materialization,
            move |prism, meta| {
                prism.update_native_task_authoritative_only(
                    meta,
                    TaskUpdateInput {
                        task_id: task_id.clone(),
                        kind: None,
                        status: None,
                        published_task_status: None,
                        git_execution: Some(git_execution),
                        assignee: None,
                        session: None,
                        worktree_id: None,
                        branch_ref: None,
                        title: None,
                        summary: None,
                        anchors: None,
                        bindings: None,
                        depends_on: None,
                        coordination_depends_on: None,
                        integrated_depends_on: None,
                        acceptance: None,
                        validation_refs: None,
                        is_abstract: None,
                        base_revision: Some(prism.workspace_revision()),
                        priority: None,
                        tags: None,
                        completion_context: None,
                    },
                    prism.workspace_revision(),
                    current_timestamp(),
                )
            },
        )
    }

    fn record_task_git_execution_authoritative_state(
        &self,
        session: &SessionState,
        authenticated: Option<&AuthenticatedPrincipal>,
        task_id: &CoordinationTaskId,
        status: Option<prism_ir::CoordinationTaskStatus>,
        published_task_status: Option<Option<prism_ir::CoordinationTaskStatus>>,
        git_execution: TaskGitExecution,
        trace: Option<&MutationRun>,
    ) -> Result<prism_coordination::CoordinationTask> {
        let task_id = task_id.clone();
        let task_ref = task_id.clone();
        self.run_workspace_coordination_step(
            session,
            authenticated,
            &task_ref,
            trace,
            "mutation.gitExecution.recordAuthoritativeStateStep",
            json!({ "taskId": task_ref.0.as_str() }),
            true,
            move |prism, meta| {
                prism.update_native_task_authoritative_only(
                    meta,
                    TaskUpdateInput {
                        task_id: task_id.clone(),
                        kind: None,
                        status,
                        published_task_status,
                        git_execution: Some(git_execution),
                        assignee: None,
                        session: None,
                        worktree_id: None,
                        branch_ref: None,
                        title: None,
                        summary: None,
                        anchors: None,
                        bindings: None,
                        depends_on: None,
                        coordination_depends_on: None,
                        integrated_depends_on: None,
                        acceptance: None,
                        validation_refs: None,
                        is_abstract: None,
                        base_revision: Some(prism.workspace_revision()),
                        priority: None,
                        tags: None,
                        completion_context: None,
                    },
                    prism.workspace_revision(),
                    current_timestamp(),
                )
            },
        )
    }

    fn record_task_publish_intent(
        &self,
        session: &SessionState,
        authenticated: Option<&AuthenticatedPrincipal>,
        task_id: &CoordinationTaskId,
        desired_status: prism_ir::CoordinationTaskStatus,
        git_execution: TaskGitExecution,
        trace: Option<&MutationRun>,
    ) -> Result<prism_coordination::CoordinationTask> {
        let task_id = task_id.clone();
        let task_ref = task_id.clone();
        self.run_workspace_coordination_step(
            session,
            authenticated,
            &task_ref,
            trace,
            "mutation.gitExecution.recordPublishIntentStep",
            json!({
                "desiredStatus": format!("{desired_status:?}"),
                "taskId": task_ref.0.as_str(),
            }),
            true,
            move |prism, meta| {
                prism.update_native_task_authoritative_only(
                    meta,
                    TaskUpdateInput {
                        task_id: task_id.clone(),
                        kind: None,
                        status: None,
                        published_task_status: Some(Some(desired_status)),
                        git_execution: Some(git_execution),
                        assignee: None,
                        session: None,
                        worktree_id: None,
                        branch_ref: None,
                        title: None,
                        summary: None,
                        anchors: None,
                        bindings: None,
                        depends_on: None,
                        coordination_depends_on: None,
                        integrated_depends_on: None,
                        acceptance: None,
                        validation_refs: None,
                        is_abstract: None,
                        base_revision: Some(prism.workspace_revision()),
                        priority: None,
                        tags: None,
                        completion_context: Some(TaskCompletionContext::default()),
                    },
                    prism.workspace_revision(),
                    current_timestamp(),
                )
            },
        )
    }

    fn run_workspace_coordination_step<T, F>(
        &self,
        session: &SessionState,
        authenticated: Option<&AuthenticatedPrincipal>,
        task_id: &CoordinationTaskId,
        trace: Option<&MutationRun>,
        operation: &'static str,
        operation_args: Value,
        schedule_materialization: bool,
        mutate: F,
    ) -> Result<T>
    where
        F: FnOnce(&Prism, EventMeta) -> Result<T>,
    {
        let workspace = self
            .workspace_session()
            .ok_or_else(|| anyhow!("git execution workflow requires a workspace-backed session"))?;
        let event_id = session.next_event_id("coordination");
        let meta = mutation_provenance(self, session, authenticated).event_meta(
            event_id,
            Some(TaskId::new(task_id.0.clone())),
            None,
            current_timestamp(),
        );
        let step_started = std::time::Instant::now();
        let observed = |inner_operation: &str,
                        duration: std::time::Duration,
                        args: Value,
                        success: bool,
                        error: Option<String>| {
            if let Some(trace) = trace {
                let suffix = inner_operation
                    .strip_prefix("mutation.coordination.")
                    .unwrap_or(inner_operation);
                let nested_operation = format!("{operation}.{suffix}");
                trace.record_phase(&nested_operation, &args, duration, success, error);
            }
        };
        let result = if schedule_materialization {
            workspace.mutate_coordination_with_session_wait_observed(
                Some(&session.session_id()),
                |prism| {
                    let _ = self.ensure_coordination_runtime_current(workspace)?;
                    mutate(prism, meta)
                },
                observed,
            )
        } else {
            workspace.mutate_coordination_with_session_wait_observed_no_materialization(
                Some(&session.session_id()),
                |prism| {
                    let _ = self.ensure_coordination_runtime_current(workspace)?;
                    mutate(prism, meta)
                },
                observed,
            )
        };
        let final_result = match result {
            Ok(Some(value)) => {
                self.sync_coordination_revision(workspace)?;
                Ok(value)
            }
            Ok(None) => Err(AdmissionBusyError::refresh_lock("mutateCoordination").into()),
            Err(error) => {
                let _ = self.sync_coordination_revision(workspace);
                Err(error)
            }
        };
        record_optional_trace_result(
            trace,
            operation,
            operation_args,
            step_started,
            &final_result,
        );
        final_result
    }

    fn current_task_state_value(&self, task_id: &CoordinationTaskId) -> Result<Value> {
        let prism = self.current_prism();
        let task = prism
            .coordination_task(task_id)
            .ok_or_else(|| anyhow!("unknown coordination task `{}`", task_id.0))?;
        Ok(serde_json::to_value(coordination_task_view(task))?)
    }

    #[cfg_attr(not(test), allow(dead_code))]
    pub(crate) fn store_claim(
        &self,
        session: &SessionState,
        args: PrismClaimArgs,
    ) -> Result<ClaimMutationResult> {
        self.store_claim_authenticated(session, args, None)
    }

    pub(crate) fn store_claim_authenticated(
        &self,
        session: &SessionState,
        args: PrismClaimArgs,
        authenticated: Option<&AuthenticatedPrincipal>,
    ) -> Result<ClaimMutationResult> {
        self.ensure_tool_enabled("prism_claim", "coordination claim mutations")?;
        if let Some(workspace) = self.workspace_session() {
            self.refresh_workspace_for_mutation()?;
            let _ = self.ensure_coordination_runtime_current(workspace)?;
        }
        let prism = self.current_prism();
        let before_events = prism.coordination_events().len();
        let task = session.task_for_mutation(args.task_id.clone().map(TaskId::new));
        let meta = mutation_provenance(self, session, authenticated).event_meta(
            session.next_event_id("coordination"),
            Some(task),
            None,
            current_timestamp(),
        );
        if let Some(workspace) = self.workspace_session() {
            match workspace.mutate_coordination_with_session_wait_observed(
                Some(&session.session_id()),
                |prism| self.apply_claim_mutation(session, prism, args, meta.clone()),
                |_operation, _duration, _args, _success, _error| {},
            ) {
                Ok(Some(mut result)) => {
                    self.sync_coordination_revision(workspace)?;
                    let prism = self.current_prism();
                    let audit = coordination_audit_since(prism.as_ref(), before_events);
                    result.event_ids = audit.event_ids;
                    result.violations.extend(audit.violations);
                    Ok(result)
                }
                Ok(None) => Err(AdmissionBusyError::refresh_lock("mutateCoordination").into()),
                Err(error) => {
                    let prism = self.current_prism();
                    let audit = coordination_audit_since(prism.as_ref(), before_events);
                    if audit.rejected && !audit.event_ids.is_empty() {
                        self.sync_coordination_revision(workspace)?;
                        return Ok(ClaimMutationResult {
                            claim_id: None,
                            event_ids: audit.event_ids,
                            rejected: true,
                            conflicts: Vec::new(),
                            violations: audit.violations,
                            state: Value::Null,
                        });
                    }
                    Err(error)
                }
            }
        } else {
            match self.apply_claim_mutation(session, prism.as_ref(), args, meta.clone()) {
                Ok(mut result) => {
                    let prism = self.current_prism();
                    let audit = coordination_audit_since(prism.as_ref(), before_events);
                    result.event_ids = audit.event_ids;
                    result.violations.extend(audit.violations);
                    Ok(result)
                }
                Err(error) => {
                    let prism = self.current_prism();
                    let audit = coordination_audit_since(prism.as_ref(), before_events);
                    if audit.rejected && !audit.event_ids.is_empty() {
                        return Ok(ClaimMutationResult {
                            claim_id: None,
                            event_ids: audit.event_ids,
                            rejected: true,
                            conflicts: Vec::new(),
                            violations: audit.violations,
                            state: Value::Null,
                        });
                    }
                    Err(error)
                }
            }
        }
    }

    pub(crate) fn store_heartbeat_lease_authenticated(
        &self,
        session: &SessionState,
        args: PrismHeartbeatLeaseArgs,
        authenticated: Option<&AuthenticatedPrincipal>,
    ) -> Result<HeartbeatLeaseMutationResult> {
        self.ensure_tool_enabled(
            "prism_coordination",
            "coordination lease heartbeat mutations",
        )?;
        if let Some(workspace) = self.workspace_session() {
            self.refresh_workspace_for_mutation()?;
            let _ = self.ensure_coordination_runtime_current(workspace)?;
        }
        let prism = self.current_prism();
        let before_events = prism.coordination_events().len();
        let requested_task_id = args.task_id.clone();
        let requested_claim_id = args.claim_id.clone();
        let task = args
            .task_id
            .clone()
            .map(TaskId::new)
            .or_else(|| Some(session.task_for_mutation(None)));
        let meta = mutation_provenance(self, session, authenticated).event_meta(
            session.next_event_id("coordination"),
            task,
            None,
            current_timestamp(),
        );
        if let Some(workspace) = self.workspace_session() {
            match workspace.mutate_coordination_with_session_wait_observed(
                Some(&session.session_id()),
                |prism| self.apply_heartbeat_lease_mutation(session, prism, args, meta.clone()),
                |_operation, _duration, _args, _success, _error| {},
            ) {
                Ok(Some(mut result)) => {
                    self.sync_coordination_revision(workspace)?;
                    let prism = self.current_prism();
                    let audit = coordination_audit_since(prism.as_ref(), before_events);
                    result.event_ids = audit.event_ids;
                    result.violations.extend(audit.violations);
                    Ok(result)
                }
                Ok(None) => Err(AdmissionBusyError::refresh_lock("mutateCoordination").into()),
                Err(error) => {
                    let prism = self.current_prism();
                    let audit = coordination_audit_since(prism.as_ref(), before_events);
                    if audit.rejected && !audit.event_ids.is_empty() {
                        self.sync_coordination_revision(workspace)?;
                        return Ok(HeartbeatLeaseMutationResult {
                            task_id: requested_task_id,
                            claim_id: requested_claim_id,
                            event_ids: audit.event_ids,
                            rejected: true,
                            violations: audit.violations,
                            state: Value::Null,
                        });
                    }
                    Err(error)
                }
            }
        } else {
            match self.apply_heartbeat_lease_mutation(session, prism.as_ref(), args, meta.clone()) {
                Ok(mut result) => {
                    let prism = self.current_prism();
                    let audit = coordination_audit_since(prism.as_ref(), before_events);
                    result.event_ids = audit.event_ids;
                    result.violations.extend(audit.violations);
                    Ok(result)
                }
                Err(error) => {
                    let prism = self.current_prism();
                    let audit = coordination_audit_since(prism.as_ref(), before_events);
                    if audit.rejected && !audit.event_ids.is_empty() {
                        return Ok(HeartbeatLeaseMutationResult {
                            task_id: requested_task_id,
                            claim_id: requested_claim_id,
                            event_ids: audit.event_ids,
                            rejected: true,
                            violations: audit.violations,
                            state: Value::Null,
                        });
                    }
                    Err(error)
                }
            }
        }
    }

    #[cfg_attr(not(test), allow(dead_code))]
    pub(crate) fn store_artifact(
        &self,
        session: &SessionState,
        args: PrismArtifactArgs,
    ) -> Result<ArtifactMutationResult> {
        self.store_artifact_authenticated(session, args, None)
    }

    pub(crate) fn store_artifact_authenticated(
        &self,
        session: &SessionState,
        args: PrismArtifactArgs,
        authenticated: Option<&AuthenticatedPrincipal>,
    ) -> Result<ArtifactMutationResult> {
        self.ensure_tool_enabled("prism_artifact", "coordination artifact mutations")?;
        if let Some(workspace) = self.workspace_session() {
            self.refresh_workspace_for_mutation()?;
            let _ = self.ensure_coordination_runtime_current(workspace)?;
        }
        let prism = self.current_prism();
        let before_events = prism.coordination_events().len();
        let task = session.task_for_mutation(args.task_id.clone().map(TaskId::new));
        let meta = mutation_provenance(self, session, authenticated).event_meta(
            session.next_event_id("coordination"),
            Some(task),
            None,
            current_timestamp(),
        );
        if let Some(workspace) = self.workspace_session() {
            match workspace.mutate_coordination_with_session_wait_observed(
                Some(&session.session_id()),
                |prism| self.apply_artifact_mutation(session, prism, args, meta.clone()),
                |_operation, _duration, _args, _success, _error| {},
            ) {
                Ok(Some(mut result)) => {
                    self.sync_coordination_revision(workspace)?;
                    let prism = self.current_prism();
                    let audit = coordination_audit_since(prism.as_ref(), before_events);
                    result.event_ids = audit.event_ids;
                    result.violations.extend(audit.violations);
                    Ok(result)
                }
                Ok(None) => Err(AdmissionBusyError::refresh_lock("mutateCoordination").into()),
                Err(error) => {
                    let prism = self.current_prism();
                    let audit = coordination_audit_since(prism.as_ref(), before_events);
                    if audit.rejected && !audit.event_ids.is_empty() {
                        self.sync_coordination_revision(workspace)?;
                        return Ok(ArtifactMutationResult {
                            artifact_id: None,
                            review_id: None,
                            event_ids: audit.event_ids,
                            rejected: true,
                            violations: audit.violations,
                            state: Value::Null,
                        });
                    }
                    Err(error)
                }
            }
        } else {
            match self.apply_artifact_mutation(session, prism.as_ref(), args, meta.clone()) {
                Ok(mut result) => {
                    let prism = self.current_prism();
                    let audit = coordination_audit_since(prism.as_ref(), before_events);
                    result.event_ids = audit.event_ids;
                    result.violations.extend(audit.violations);
                    Ok(result)
                }
                Err(error) => {
                    let prism = self.current_prism();
                    let audit = coordination_audit_since(prism.as_ref(), before_events);
                    if audit.rejected && !audit.event_ids.is_empty() {
                        return Ok(ArtifactMutationResult {
                            artifact_id: None,
                            review_id: None,
                            event_ids: audit.event_ids,
                            rejected: true,
                            violations: audit.violations,
                            state: Value::Null,
                        });
                    }
                    Err(error)
                }
            }
        }
    }

    pub(crate) fn apply_coordination_mutation(
        &self,
        session: &SessionState,
        prism: &Prism,
        args: PrismCoordinationArgs,
        meta: EventMeta,
    ) -> Result<Value> {
        let workspace_root = self.workspace_root();
        match args.kind {
            CoordinationMutationKindInput::PlanCreate => {
                let payload: crate::PlanCreatePayload = serde_json::from_value(args.payload)?;
                let plan_id = prism.create_native_plan_with_scheduling(
                    meta,
                    payload.title,
                    payload.goal,
                    payload.status.map(convert_plan_status),
                    convert_policy(payload.policy)?,
                    convert_plan_scheduling(payload.scheduling),
                )?;
                let plan = prism
                    .coordination_plan(&plan_id)
                    .ok_or_else(|| anyhow!("unknown plan `{}`", plan_id.0))?;
                let root_node_ids = prism
                    .plan_graph(&plan_id)
                    .map(|graph| graph.root_nodes)
                    .unwrap_or_else(|| {
                        plan.root_tasks
                            .iter()
                            .map(|task_id| prism_ir::PlanNodeId::new(task_id.0.clone()))
                            .collect()
                    });
                Ok(serde_json::to_value(plan_view(plan, root_node_ids))?)
            }
            CoordinationMutationKindInput::PlanUpdate => {
                let payload: PlanUpdatePayload = serde_json::from_value(args.payload)?;
                let plan_id = PlanId::new(payload.plan_id);
                let existing_plan = prism
                    .coordination_plan(&plan_id)
                    .ok_or_else(|| anyhow!("unknown plan `{}`", plan_id.0))?;
                let merged_policy = payload
                    .policy
                    .map(|policy| merge_policy_payload(existing_plan.policy.clone(), policy));
                let merged_scheduling = payload.scheduling.map(|scheduling| {
                    merge_plan_scheduling_payload(existing_plan.scheduling, scheduling)
                });
                prism.update_native_plan_with_scheduling(
                    meta,
                    &plan_id,
                    payload.title,
                    payload.status.map(convert_plan_status),
                    payload.goal,
                    merged_policy,
                    merged_scheduling,
                )?;
                let plan = prism
                    .coordination_plan(&plan_id)
                    .ok_or_else(|| anyhow!("unknown plan `{}`", plan_id.0))?;
                let root_node_ids = prism
                    .plan_graph(&plan_id)
                    .map(|graph| graph.root_nodes)
                    .unwrap_or_else(|| {
                        plan.root_tasks
                            .iter()
                            .map(|task_id| prism_ir::PlanNodeId::new(task_id.0.clone()))
                            .collect()
                    });
                Ok(serde_json::to_value(plan_view(plan, root_node_ids))?)
            }
            CoordinationMutationKindInput::PlanArchive => {
                let payload: PlanArchivePayload = serde_json::from_value(args.payload)?;
                let plan_id = PlanId::new(payload.plan_id);
                prism.update_native_plan(
                    meta,
                    &plan_id,
                    None,
                    Some(prism_ir::PlanStatus::Archived),
                    None,
                    None,
                )?;
                let plan = prism
                    .coordination_plan(&plan_id)
                    .ok_or_else(|| anyhow!("unknown plan `{}`", plan_id.0))?;
                let root_node_ids = prism
                    .plan_graph(&plan_id)
                    .map(|graph| graph.root_nodes)
                    .unwrap_or_else(|| {
                        plan.root_tasks
                            .iter()
                            .map(|task_id| prism_ir::PlanNodeId::new(task_id.0.clone()))
                            .collect()
                    });
                Ok(serde_json::to_value(plan_view(plan, root_node_ids))?)
            }
            CoordinationMutationKindInput::TaskCreate => {
                let payload: TaskCreatePayload = serde_json::from_value(args.payload)?;
                let requested_status = payload.status.map(convert_coordination_task_status);
                let plan = prism
                    .coordination_plan(&PlanId::new(payload.plan_id.clone()))
                    .ok_or_else(|| anyhow!("unknown plan `{}`", payload.plan_id))?;
                if requested_status.is_some_and(coordination_status_bypasses_git_execution) {
                    reject_git_execution_bypass_on_create(
                        &plan,
                        requested_status
                            .map(|status| format!("{status:?}").to_ascii_lowercase())
                            .as_deref(),
                    )?;
                }
                let task = prism.create_native_task(
                    meta,
                    TaskCreateInput {
                        plan_id: PlanId::new(payload.plan_id),
                        title: payload.title,
                        status: requested_status,
                        assignee: payload
                            .assignee
                            .map(AgentId::new)
                            .or_else(|| session.current_agent()),
                        session: Some(session.session_id()),
                        worktree_id: None,
                        branch_ref: None,
                        anchors: convert_anchors(
                            prism,
                            self.workspace_session_ref(),
                            workspace_root,
                            payload.anchors.unwrap_or_default(),
                        )?,
                        depends_on: payload
                            .depends_on
                            .unwrap_or_default()
                            .into_iter()
                            .map(CoordinationTaskId::new)
                            .collect(),
                        coordination_depends_on: payload
                            .coordination_depends_on
                            .unwrap_or_default()
                            .into_iter()
                            .map(CoordinationTaskId::new)
                            .collect(),
                        integrated_depends_on: payload
                            .integrated_depends_on
                            .unwrap_or_default()
                            .into_iter()
                            .map(CoordinationTaskId::new)
                            .collect(),
                        acceptance: convert_acceptance(
                            prism,
                            self.workspace_session_ref(),
                            workspace_root,
                            payload.acceptance,
                        )?,
                        base_revision: prism.workspace_revision(),
                    },
                )?;
                Ok(serde_json::to_value(coordination_task_view(task))?)
            }
            CoordinationMutationKindInput::Update => {
                let mut payload: WorkflowUpdatePayload = serde_json::from_value(args.payload.clone())?;
                if payload.completion_context.is_none() {
                    payload.completion_context = args
                        .payload
                        .get("completionContext")
                        .or_else(|| args.payload.get("completion_context"))
                        .map(|value| serde_json::from_value(value.clone()))
                        .transpose()?;
                }
                let WorkflowUpdatePayload {
                    id,
                    kind,
                    status,
                    assignee,
                    is_abstract,
                    title,
                    summary,
                    anchors,
                    bindings,
                    depends_on,
                    coordination_depends_on,
                    integrated_depends_on,
                    acceptance,
                    validation_refs,
                    priority,
                    tags,
                    completion_context,
                } = payload;
                match resolve_workflow_update_target(prism, &id)? {
                    WorkflowUpdateTarget::CoordinationTask(task_id) => {
                        let summary = match parse_sparse_patch(summary, "summary")? {
                            SparsePatch::Keep => None,
                            SparsePatch::Set(value) => Some(Some(value)),
                            SparsePatch::Clear => Some(None),
                        };
                        let priority = match parse_sparse_patch(priority, "priority")? {
                            SparsePatch::Keep => None,
                            SparsePatch::Set(value) => Some(Some(value)),
                            SparsePatch::Clear => Some(None),
                        };
                        let status = status.map(convert_workflow_status_for_task).transpose()?;
                        let assignee = match parse_sparse_patch(assignee, "assignee")? {
                            SparsePatch::Keep => None,
                            SparsePatch::Set(value) => Some(Some(AgentId::new(value))),
                            SparsePatch::Clear => Some(None),
                        };
                        let task_anchors = anchors
                            .clone()
                            .map(|anchors| {
                                convert_anchors(
                                    prism,
                                    self.workspace_session_ref(),
                                    workspace_root,
                                    anchors,
                                )
                            })
                            .transpose()?;
                        let task_bindings = convert_plan_binding(
                            prism,
                            self.workspace_session_ref(),
                            workspace_root,
                            anchors,
                            bindings,
                        )?;
                        let existing_task = prism
                            .coordination_task(&task_id)
                            .ok_or_else(|| anyhow!("unknown coordination task `{}`", task_id.0))?;
                        let completion_context = completion_context_for_task_update(
                            prism,
                            &task_id,
                            status,
                            &meta,
                            completion_context,
                        );
                        if let Some(completion_context) = completion_context.as_ref() {
                            if completion_context.integration_evidence.is_some() {
                                let observation_root = workspace_root.ok_or_else(|| {
                                    anyhow!(
                                        "target integration verification requires a workspace root"
                                    )
                                })?;
                                validate_explicit_integration_evidence(
                                    observation_root,
                                    prism,
                                    &task_id,
                                    &existing_task.git_execution,
                                    completion_context,
                                )?;
                            }
                        }
                        let git_execution = task_git_execution_from_completion_context(
                            prism,
                            &task_id,
                            &existing_task.git_execution,
                            completion_context.as_ref(),
                        )?;
                        let authoritative_git_execution_only_update = git_execution.is_some()
                            && kind.is_none()
                            && status.is_none()
                            && assignee.is_none()
                            && title.is_none()
                            && summary.is_none()
                            && task_anchors.is_none()
                            && task_bindings.is_none()
                            && depends_on.is_none()
                            && coordination_depends_on.is_none()
                            && integrated_depends_on.is_none()
                            && acceptance.is_none()
                            && validation_refs.is_none()
                            && is_abstract.is_none()
                            && priority.is_none()
                            && tags.is_none();
                        let update_input = TaskUpdateInput {
                            task_id,
                            kind: kind.map(convert_plan_node_kind),
                            status,
                            published_task_status: None,
                            git_execution,
                            assignee,
                            session: None,
                            worktree_id: None,
                            branch_ref: None,
                            title,
                            summary,
                            anchors: task_anchors,
                            bindings: task_bindings,
                            depends_on: depends_on.map(|depends_on| {
                                depends_on
                                    .into_iter()
                                    .map(CoordinationTaskId::new)
                                    .collect::<Vec<_>>()
                            }),
                            coordination_depends_on: coordination_depends_on.map(|depends_on| {
                                depends_on
                                    .into_iter()
                                    .map(CoordinationTaskId::new)
                                    .collect::<Vec<_>>()
                            }),
                            integrated_depends_on: integrated_depends_on.map(|depends_on| {
                                depends_on
                                    .into_iter()
                                    .map(CoordinationTaskId::new)
                                    .collect::<Vec<_>>()
                            }),
                            acceptance: acceptance
                                .map(|acceptance| {
                                    convert_acceptance(
                                        prism,
                                        self.workspace_session_ref(),
                                        workspace_root,
                                        Some(acceptance),
                                    )
                                })
                                .transpose()?,
                            validation_refs: validation_refs
                                .map(|refs| convert_validation_refs(Some(refs))),
                            is_abstract,
                            base_revision: Some(prism.workspace_revision()),
                            priority,
                            tags,
                            completion_context: if authoritative_git_execution_only_update {
                                None
                            } else {
                                completion_context
                            },
                        };
                        let task = if authoritative_git_execution_only_update {
                            prism.update_native_task_authoritative_only(
                                meta.clone(),
                                update_input,
                                prism.workspace_revision(),
                                current_timestamp(),
                            )?
                        } else {
                            prism.update_native_task(
                                meta.clone(),
                                update_input,
                                prism.workspace_revision(),
                                current_timestamp(),
                            )?
                        };
                        let task = match workspace_root {
                            Some(observation_root) => maybe_observe_target_integration(
                                session,
                                prism,
                                &meta,
                                observation_root,
                                &task,
                            )?
                            .unwrap_or(task),
                            None => task,
                        };
                        Ok(serde_json::to_value(coordination_task_view(task))?)
                    }
                    WorkflowUpdateTarget::PlanNode { plan_id, node_id } => {
                        let status = status.map(convert_workflow_status_for_plan_node);
                        let assignee = match parse_sparse_patch(assignee, "assignee")? {
                            SparsePatch::Keep => None,
                            SparsePatch::Set(value) => Some(Some(AgentId::new(value))),
                            SparsePatch::Clear => Some(None),
                        };
                        let (summary, clear_summary) = match parse_sparse_patch(summary, "summary")?
                        {
                            SparsePatch::Keep => (None, false),
                            SparsePatch::Set(value) => (Some(value), false),
                            SparsePatch::Clear => (None, true),
                        };
                        let (priority, clear_priority) =
                            match parse_sparse_patch(priority, "priority")? {
                                SparsePatch::Keep => (None, false),
                                SparsePatch::Set(value) => (Some(value), false),
                                SparsePatch::Clear => (None, true),
                            };
                        prism.update_native_plan_node(
                            &node_id,
                            kind.map(convert_plan_node_kind),
                            status,
                            assignee,
                            is_abstract,
                            title,
                            summary,
                            clear_summary,
                            convert_plan_binding(
                                prism,
                                self.workspace_session_ref(),
                                workspace_root,
                                anchors,
                                bindings,
                            )?,
                            depends_on,
                            acceptance
                                .map(|acceptance| {
                                    convert_plan_acceptance(
                                        prism,
                                        self.workspace_session_ref(),
                                        workspace_root,
                                        Some(acceptance),
                                    )
                                })
                                .transpose()?,
                            validation_refs.map(|refs| convert_validation_refs(Some(refs))),
                            Some(prism.workspace_revision()),
                            priority,
                            clear_priority,
                            tags,
                        )?;
                        current_plan_node_state(prism, &plan_id, &node_id.0)
                    }
                }
            }
            CoordinationMutationKindInput::PlanNodeCreate => {
                let payload: PlanNodeCreatePayload = serde_json::from_value(args.payload)?;
                let kind = payload
                    .kind
                    .map(convert_plan_node_kind)
                    .unwrap_or(prism_ir::PlanNodeKind::Edit);
                let status = payload.status.map(convert_plan_node_status);
                let plan_id = PlanId::new(payload.plan_id.clone());
                let plan = prism
                    .coordination_plan(&plan_id)
                    .ok_or_else(|| anyhow!("unknown plan `{}`", plan_id.0))?;
                if status.is_some_and(plan_node_status_bypasses_git_execution) {
                    reject_git_execution_bypass_on_create(
                        &plan,
                        status
                            .map(|status| format!("{status:?}").to_ascii_lowercase())
                            .as_deref(),
                    )?;
                }
                let node_id = prism.create_native_plan_node(
                    &plan_id,
                    kind,
                    payload.title,
                    payload.summary,
                    status,
                    payload
                        .assignee
                        .map(AgentId::new)
                        .or_else(|| session.current_agent()),
                    payload.is_abstract.unwrap_or(false),
                    convert_plan_binding(
                        prism,
                        self.workspace_session_ref(),
                        workspace_root,
                        payload.anchors,
                        payload.bindings,
                    )?
                    .unwrap_or_default(),
                    payload.depends_on.unwrap_or_default(),
                    convert_plan_acceptance(
                        prism,
                        self.workspace_session_ref(),
                        workspace_root,
                        payload.acceptance,
                    )?,
                    convert_validation_refs(payload.validation_refs),
                    prism.workspace_revision(),
                    payload.priority,
                    payload.tags.unwrap_or_default(),
                )?;
                current_plan_node_state(prism, &plan_id, &node_id.0)
            }
            CoordinationMutationKindInput::PlanEdgeCreate => {
                let payload: PlanEdgeCreatePayload = serde_json::from_value(args.payload)?;
                let kind = convert_plan_edge_kind(payload.kind);
                let plan_id = PlanId::new(payload.plan_id.clone());
                prism.create_native_plan_edge(
                    &plan_id,
                    &PlanNodeId::new(payload.from_node_id.clone()),
                    &PlanNodeId::new(payload.to_node_id.clone()),
                    kind,
                )?;
                current_plan_edge_state(
                    prism,
                    &plan_id,
                    &payload.from_node_id,
                    &payload.to_node_id,
                    kind,
                )
            }
            CoordinationMutationKindInput::PlanEdgeDelete => {
                let payload: PlanEdgeDeletePayload = serde_json::from_value(args.payload)?;
                let kind = convert_plan_edge_kind(payload.kind);
                let plan_id = PlanId::new(payload.plan_id.clone());
                prism.delete_native_plan_edge(
                    &plan_id,
                    &PlanNodeId::new(payload.from_node_id.clone()),
                    &PlanNodeId::new(payload.to_node_id.clone()),
                    kind,
                )?;
                deleted_plan_edge_state(&plan_id, &payload.from_node_id, &payload.to_node_id, kind)
            }
            CoordinationMutationKindInput::Handoff => {
                let payload: crate::HandoffPayload = serde_json::from_value(args.payload)?;
                let task = prism.request_native_handoff(
                    meta,
                    HandoffInput {
                        task_id: CoordinationTaskId::new(payload.task_id),
                        to_agent: payload.to_agent.map(AgentId::new),
                        summary: payload.summary,
                        base_revision: prism.workspace_revision(),
                    },
                    prism.workspace_revision(),
                )?;
                Ok(serde_json::to_value(coordination_task_view(task))?)
            }
            CoordinationMutationKindInput::Resume => {
                let payload: TaskResumePayload = serde_json::from_value(args.payload)?;
                let session_agent = session.current_agent();
                if let (Some(expected), Some(current)) =
                    (payload.agent.as_ref(), session_agent.as_ref())
                {
                    if expected != &current.0 {
                        return Err(anyhow!(
                            "task resume agent `{expected}` does not match current session agent `{}`",
                            current.0
                        ));
                    }
                }
                let task = prism.resume_native_task(
                    meta,
                    TaskResumeInput {
                        task_id: CoordinationTaskId::new(payload.task_id),
                        agent: session_agent,
                        worktree_id: None,
                        branch_ref: None,
                    },
                )?;
                Ok(serde_json::to_value(coordination_task_view(task))?)
            }
            CoordinationMutationKindInput::Reclaim => {
                let payload: TaskReclaimPayload = serde_json::from_value(args.payload)?;
                let session_agent = session.current_agent();
                if let (Some(expected), Some(current)) =
                    (payload.agent.as_ref(), session_agent.as_ref())
                {
                    if expected != &current.0 {
                        return Err(anyhow!(
                            "task reclaim agent `{expected}` does not match current session agent `{}`",
                            current.0
                        ));
                    }
                }
                let task = prism.reclaim_native_task(
                    meta,
                    TaskReclaimInput {
                        task_id: CoordinationTaskId::new(payload.task_id),
                        agent: session_agent,
                        worktree_id: None,
                        branch_ref: None,
                    },
                )?;
                Ok(serde_json::to_value(coordination_task_view(task))?)
            }
            CoordinationMutationKindInput::HandoffAccept => {
                let payload: HandoffAcceptPayload = serde_json::from_value(args.payload)?;
                let session_agent = session.current_agent();
                if let (Some(expected), Some(current)) =
                    (payload.agent.as_ref(), session_agent.as_ref())
                {
                    if expected != &current.0 {
                        return Err(anyhow!(
                            "handoff acceptance agent `{expected}` does not match current session agent `{}`",
                            current.0
                        ));
                    }
                }
                let task = prism.accept_native_handoff(
                    meta,
                    HandoffAcceptInput {
                        task_id: CoordinationTaskId::new(payload.task_id),
                        agent: session_agent,
                        worktree_id: None,
                        branch_ref: None,
                    },
                )?;
                Ok(serde_json::to_value(coordination_task_view(task))?)
            }
        }
    }

    pub(crate) fn apply_claim_mutation(
        &self,
        session: &SessionState,
        prism: &Prism,
        args: PrismClaimArgs,
        meta: EventMeta,
    ) -> Result<ClaimMutationResult> {
        let workspace_root = self.workspace_root();
        match args.action {
            ClaimActionInput::Acquire => {
                let payload: ClaimAcquirePayload = serde_json::from_value(args.payload)?;
                let anchors = prism.coordination_scope_anchors(&convert_anchors(
                    prism,
                    self.workspace_session_ref(),
                    workspace_root,
                    payload.anchors,
                )?);
                let (claim_id, conflicts, state) = prism.acquire_native_claim(
                    meta,
                    session.session_id(),
                    prism_coordination::ClaimAcquireInput {
                        task_id: payload.coordination_task_id.map(CoordinationTaskId::new),
                        anchors,
                        capability: convert_capability(payload.capability),
                        mode: payload.mode.map(convert_claim_mode),
                        ttl_seconds: payload.ttl_seconds,
                        base_revision: prism.workspace_revision(),
                        current_revision: prism.workspace_revision(),
                        agent: payload
                            .agent
                            .map(AgentId::new)
                            .or_else(|| session.current_agent()),
                        worktree_id: None,
                        branch_ref: None,
                    },
                )?;
                Ok(ClaimMutationResult {
                    claim_id: claim_id.map(|claim_id| claim_id.0.to_string()),
                    event_ids: Vec::new(),
                    rejected: false,
                    conflicts: conflicts
                        .into_iter()
                        .map(conflict_view)
                        .map(serde_json::to_value)
                        .collect::<Result<Vec<_>, _>>()?,
                    violations: Vec::new(),
                    state: state
                        .map(claim_view)
                        .map(serde_json::to_value)
                        .transpose()?
                        .unwrap_or(Value::Null),
                })
            }
            ClaimActionInput::Renew => {
                let payload: ClaimRenewPayload = serde_json::from_value(args.payload)?;
                let claim = prism.renew_native_claim(
                    meta,
                    &session.session_id(),
                    &ClaimId::new(payload.claim_id.clone()),
                    payload.ttl_seconds,
                    "explicit",
                )?;
                Ok(ClaimMutationResult {
                    claim_id: Some(payload.claim_id),
                    event_ids: Vec::new(),
                    rejected: false,
                    conflicts: Vec::new(),
                    violations: Vec::new(),
                    state: serde_json::to_value(claim_view(claim))?,
                })
            }
            ClaimActionInput::Release => {
                let payload: ClaimReleasePayload = serde_json::from_value(args.payload)?;
                let claim = prism.release_native_claim(
                    meta,
                    &session.session_id(),
                    &ClaimId::new(payload.claim_id.clone()),
                )?;
                Ok(ClaimMutationResult {
                    claim_id: Some(payload.claim_id),
                    event_ids: Vec::new(),
                    rejected: false,
                    conflicts: Vec::new(),
                    violations: Vec::new(),
                    state: serde_json::to_value(claim_view(claim))?,
                })
            }
        }
    }

    pub(crate) fn apply_heartbeat_lease_mutation(
        &self,
        session: &SessionState,
        prism: &Prism,
        args: PrismHeartbeatLeaseArgs,
        meta: EventMeta,
    ) -> Result<HeartbeatLeaseMutationResult> {
        match (args.task_id, args.claim_id) {
            (Some(task_id), None) => {
                let task = prism.heartbeat_native_task(
                    meta,
                    &CoordinationTaskId::new(task_id.clone()),
                    "explicit",
                )?;
                Ok(HeartbeatLeaseMutationResult {
                    task_id: Some(task_id),
                    claim_id: None,
                    event_ids: Vec::new(),
                    rejected: false,
                    violations: Vec::new(),
                    state: serde_json::to_value(coordination_task_view(task))?,
                })
            }
            (None, Some(claim_id)) => {
                let claim = prism.renew_native_claim(
                    meta,
                    &session.session_id(),
                    &ClaimId::new(claim_id.clone()),
                    None,
                    "explicit",
                )?;
                Ok(HeartbeatLeaseMutationResult {
                    task_id: None,
                    claim_id: Some(claim_id),
                    event_ids: Vec::new(),
                    rejected: false,
                    violations: Vec::new(),
                    state: serde_json::to_value(claim_view(claim))?,
                })
            }
            _ => Err(anyhow!(
                "heartbeat_lease requires exactly one of `taskId` or `claimId`"
            )),
        }
    }

    pub(crate) fn apply_artifact_mutation(
        &self,
        session: &SessionState,
        prism: &Prism,
        args: PrismArtifactArgs,
        meta: EventMeta,
    ) -> Result<ArtifactMutationResult> {
        let workspace_root = self.workspace_root();
        match args.action {
            ArtifactActionInput::Propose => {
                let payload: ArtifactProposePayload = serde_json::from_value(args.payload)?;
                let task_id = CoordinationTaskId::new(payload.task_id.clone());
                let anchors = match payload.anchors {
                    Some(anchors) => convert_anchors(
                        prism,
                        self.workspace_session_ref(),
                        workspace_root,
                        anchors,
                    )?,
                    None => prism
                        .coordination_task(&task_id)
                        .map(|task| task.anchors)
                        .unwrap_or_default(),
                };
                let evidence = payload
                    .evidence
                    .unwrap_or_default()
                    .into_iter()
                    .map(EventId::new)
                    .collect::<Vec<_>>();
                let mut inferred_validated_checks = payload.validated_checks.unwrap_or_default();
                for event_id in &evidence {
                    if let Some(event) = prism.outcome_event(event_id) {
                        if matches!(event.result, prism_memory::OutcomeResult::Success) {
                            inferred_validated_checks
                                .extend(outcome_validation_labels(&event.evidence));
                        }
                    }
                }
                inferred_validated_checks.sort();
                inferred_validated_checks.dedup();
                let recipe = prism.task_validation_recipe(&task_id);
                let risk = prism.task_risk(&task_id, meta.ts);
                let (artifact_id, artifact) = prism.propose_native_artifact(
                    meta.clone(),
                    prism_coordination::ArtifactProposeInput {
                        task_id,
                        anchors,
                        diff_ref: payload.diff_ref,
                        evidence: evidence.clone(),
                        base_revision: prism.workspace_revision(),
                        current_revision: prism.workspace_revision(),
                        required_validations: payload.required_validations.unwrap_or_else(|| {
                            recipe.map(|recipe| recipe.checks).unwrap_or_default()
                        }),
                        validated_checks: inferred_validated_checks,
                        risk_score: payload
                            .risk_score
                            .or_else(|| risk.map(|risk| risk.risk_score)),
                        worktree_id: None,
                        branch_ref: None,
                    },
                )?;
                maybe_link_review_artifact_to_task_git_execution(
                    session,
                    prism,
                    &meta,
                    &artifact.task,
                    &artifact_id,
                )?;
                Ok(ArtifactMutationResult {
                    artifact_id: Some(artifact_id.0.to_string()),
                    review_id: None,
                    event_ids: Vec::new(),
                    rejected: false,
                    violations: Vec::new(),
                    state: serde_json::to_value(artifact_view(artifact))?,
                })
            }
            ArtifactActionInput::Supersede => {
                let payload: ArtifactSupersedePayload = serde_json::from_value(args.payload)?;
                let artifact = prism.supersede_native_artifact(
                    meta,
                    prism_coordination::ArtifactSupersedeInput {
                        artifact_id: ArtifactId::new(payload.artifact_id.clone()),
                    },
                )?;
                Ok(ArtifactMutationResult {
                    artifact_id: Some(payload.artifact_id),
                    review_id: None,
                    event_ids: Vec::new(),
                    rejected: false,
                    violations: Vec::new(),
                    state: serde_json::to_value(artifact_view(artifact))?,
                })
            }
            ArtifactActionInput::Review => {
                let payload: ArtifactReviewPayload = serde_json::from_value(args.payload)?;
                let artifact_id = ArtifactId::new(payload.artifact_id.clone());
                let risk = prism.artifact_risk(&artifact_id, meta.ts);
                let mut validated_checks = risk
                    .as_ref()
                    .map(|risk| risk.validated_checks.clone())
                    .unwrap_or_default();
                validated_checks.extend(payload.validated_checks.unwrap_or_default());
                validated_checks.sort();
                validated_checks.dedup();
                let (review_id, _, artifact) = prism.review_native_artifact(
                    meta.clone(),
                    prism_coordination::ArtifactReviewInput {
                        artifact_id,
                        verdict: convert_review_verdict(payload.verdict),
                        summary: payload.summary,
                        required_validations: payload.required_validations.unwrap_or_else(|| {
                            risk.as_ref()
                                .map(|risk| risk.required_validations.clone())
                                .unwrap_or_default()
                        }),
                        validated_checks,
                        risk_score: payload
                            .risk_score
                            .or_else(|| risk.as_ref().map(|risk| risk.risk_score)),
                    },
                    prism.workspace_revision(),
                )?;
                maybe_link_review_artifact_to_task_git_execution(
                    session,
                    prism,
                    &meta,
                    &artifact.task,
                    &artifact.id,
                )?;
                maybe_advance_auto_pr_integration_from_review(
                    session,
                    prism,
                    &meta,
                    &artifact.task,
                    &artifact,
                )?;
                let current_task = prism
                    .coordination_task(&artifact.task)
                    .ok_or_else(|| anyhow!("unknown coordination task `{}`", artifact.task.0))?;
                if let Some(observation_root) = workspace_root {
                    let _ = maybe_observe_target_integration(
                        session,
                        prism,
                        &meta,
                        observation_root,
                        &current_task,
                    )?;
                }
                Ok(ArtifactMutationResult {
                    artifact_id: Some(payload.artifact_id),
                    review_id: Some(review_id.0.to_string()),
                    event_ids: Vec::new(),
                    rejected: false,
                    violations: Vec::new(),
                    state: serde_json::to_value(artifact_view(artifact))?,
                })
            }
        }
    }

    pub(crate) fn promote_curator_edge(
        &self,
        session: &SessionState,
        args: PrismCuratorPromoteEdgeArgs,
    ) -> Result<CuratorProposalDecisionResult> {
        self.promote_curator_edge_authenticated(session, args, None)
    }

    pub(crate) fn promote_curator_edge_authenticated(
        &self,
        session: &SessionState,
        args: PrismCuratorPromoteEdgeArgs,
        _authenticated: Option<&AuthenticatedPrincipal>,
    ) -> Result<CuratorProposalDecisionResult> {
        let workspace = self
            .workspace_session()
            .ok_or_else(|| anyhow!("curator mutations require a workspace-backed session"))?;
        let job_id = CuratorJobId(args.job_id.clone());
        let snapshot = workspace.curator_snapshot()?;
        let record = snapshot
            .records
            .iter()
            .find(|record| record.id == job_id)
            .ok_or_else(|| anyhow!("unknown curator job `{}`", args.job_id))?;
        let proposal_state = curator_proposal_state(record, args.proposal_index)?;
        if proposal_state.disposition != CuratorProposalDisposition::Pending {
            return Err(anyhow!(
                "curator proposal {} for job `{}` is already {}",
                args.proposal_index,
                args.job_id,
                curator_disposition_label(proposal_state.disposition)
            ));
        }
        let proposal = curator_proposal(record, args.proposal_index)?;
        let CuratorProposal::InferredEdge(candidate) = proposal else {
            return Err(anyhow!(
                "curator proposal {} for job `{}` is not an inferred edge",
                args.proposal_index,
                args.job_id
            ));
        };

        let task = session.task_for_mutation(args.task_id.map(TaskId::new));
        let scope =
            args.scope
                .map(convert_inferred_scope)
                .unwrap_or_else(|| match candidate.scope {
                    prism_agent::InferredEdgeScope::SessionOnly => {
                        prism_agent::InferredEdgeScope::Persisted
                    }
                    scope => scope,
                });
        let edge_id = session.inferred_edges.store_edge(
            candidate.edge.clone(),
            scope,
            Some(task.clone()),
            candidate.evidence.clone(),
        );
        if scope != prism_agent::InferredEdgeScope::SessionOnly {
            let record = session.inferred_edges.record(&edge_id).ok_or_else(|| {
                anyhow!("stored inferred edge `{}` could not be reloaded", edge_id.0)
            })?;
            workspace.append_inference_records(&[record])?;
            self.sync_inference_revision(workspace)?;
        }
        let detail = args.note.clone();
        workspace.set_curator_proposal_state(
            &job_id,
            args.proposal_index,
            CuratorProposalDisposition::Applied,
            Some(task),
            detail.clone(),
            Some(edge_id.0.clone()),
        )?;
        let proposal = self
            .curator_job(&args.job_id)?
            .and_then(|job| {
                job.proposals
                    .into_iter()
                    .find(|proposal| proposal.index == args.proposal_index)
            })
            .ok_or_else(|| anyhow!("applied curator proposal could not be reloaded"))?;
        Ok(CuratorProposalDecisionResult {
            job_id: args.job_id,
            proposal_index: args.proposal_index,
            kind: proposal.kind.clone(),
            decision: CuratorProposalDecision::Applied,
            proposal: serde_json::to_value(proposal)?,
            created: CuratorProposalCreatedResources {
                memory_id: None,
                edge_id: Some(edge_id.0.clone()),
                concept_handle: None,
            },
            detail,
            memory_id: None,
            edge_id: Some(edge_id.0),
            concept_handle: None,
        })
    }

    #[cfg_attr(not(test), allow(dead_code))]
    pub(crate) fn promote_curator_concept(
        &self,
        session: &SessionState,
        args: PrismCuratorPromoteConceptArgs,
    ) -> Result<CuratorProposalDecisionResult> {
        self.promote_curator_concept_authenticated(session, args, None)
    }

    pub(crate) fn promote_curator_concept_authenticated(
        &self,
        session: &SessionState,
        args: PrismCuratorPromoteConceptArgs,
        authenticated: Option<&AuthenticatedPrincipal>,
    ) -> Result<CuratorProposalDecisionResult> {
        let workspace = self
            .workspace_session()
            .ok_or_else(|| anyhow!("curator mutations require a workspace-backed session"))?;
        let job_id = CuratorJobId(args.job_id.clone());
        let snapshot = workspace.curator_snapshot()?;
        let record = snapshot
            .records
            .iter()
            .find(|record| record.id == job_id)
            .ok_or_else(|| anyhow!("unknown curator job `{}`", args.job_id))?;
        let proposal_state = curator_proposal_state(record, args.proposal_index)?;
        if proposal_state.disposition != CuratorProposalDisposition::Pending {
            return Err(anyhow!(
                "curator proposal {} for job `{}` is already {}",
                args.proposal_index,
                args.job_id,
                curator_disposition_label(proposal_state.disposition)
            ));
        }
        let proposal = curator_proposal(record, args.proposal_index)?;
        let CuratorProposal::ConceptCandidate(candidate) = proposal else {
            return Err(anyhow!(
                "curator proposal {} for job `{}` is not a concept candidate",
                args.proposal_index,
                args.job_id
            ));
        };

        let task_id = session.task_for_mutation(args.task_id.clone().map(TaskId::new));
        let prism = self.current_prism();
        let recorded_at = current_timestamp();
        let mut packet = build_promoted_concept_packet(
            prism.as_ref(),
            &task_id,
            recorded_at,
            concept_args_from_curator_candidate(candidate, &task_id, args.scope.clone()),
        )?;
        packet.provenance = ConceptProvenance {
            origin: "curator".to_string(),
            kind: "curator_concept_candidate".to_string(),
            task_id: Some(task_id.0.to_string()),
        };
        let mut event = ConceptEvent {
            id: next_concept_event_id(),
            recorded_at,
            task_id: Some(task_id.0.to_string()),
            actor: None,
            execution_context: None,
            action: ConceptEventAction::Promote,
            patch: None,
            concept: packet.clone(),
        };
        mutation_provenance(self, session, authenticated).stamp_concept_event(&mut event);
        workspace.append_concept_event(event)?;
        self.sync_workspace_revision(workspace)?;
        let detail = args.note.clone();
        workspace.set_curator_proposal_state(
            &job_id,
            args.proposal_index,
            CuratorProposalDisposition::Applied,
            Some(task_id),
            detail.clone(),
            Some(packet.handle.clone()),
        )?;
        let proposal = self
            .curator_job(&args.job_id)?
            .and_then(|job| {
                job.proposals
                    .into_iter()
                    .find(|proposal| proposal.index == args.proposal_index)
            })
            .ok_or_else(|| anyhow!("applied curator proposal could not be reloaded"))?;
        Ok(CuratorProposalDecisionResult {
            job_id: args.job_id,
            proposal_index: args.proposal_index,
            kind: proposal.kind.clone(),
            decision: CuratorProposalDecision::Applied,
            proposal: serde_json::to_value(proposal)?,
            created: CuratorProposalCreatedResources {
                memory_id: None,
                edge_id: None,
                concept_handle: Some(packet.handle.clone()),
            },
            detail,
            memory_id: None,
            edge_id: None,
            concept_handle: Some(packet.handle),
        })
    }

    #[cfg_attr(not(test), allow(dead_code))]
    pub(crate) fn apply_curator_proposal(
        &self,
        session: &SessionState,
        args: PrismCuratorApplyProposalArgs,
    ) -> Result<CuratorProposalDecisionResult> {
        self.apply_curator_proposal_authenticated(session, args, None)
    }

    pub(crate) fn apply_curator_proposal_authenticated(
        &self,
        session: &SessionState,
        args: PrismCuratorApplyProposalArgs,
        authenticated: Option<&AuthenticatedPrincipal>,
    ) -> Result<CuratorProposalDecisionResult> {
        let workspace = self
            .workspace_session()
            .ok_or_else(|| anyhow!("curator mutations require a workspace-backed session"))?;
        let job_id = CuratorJobId(args.job_id.clone());
        let snapshot = workspace.curator_snapshot()?;
        let record = snapshot
            .records
            .iter()
            .find(|record| record.id == job_id)
            .ok_or_else(|| anyhow!("unknown curator job `{}`", args.job_id))?;
        let proposal_state = curator_proposal_state(record, args.proposal_index)?;
        if proposal_state.disposition != CuratorProposalDisposition::Pending {
            return Err(anyhow!(
                "curator proposal {} for job `{}` is already {}",
                args.proposal_index,
                args.job_id,
                curator_disposition_label(proposal_state.disposition)
            ));
        }

        let proposal = curator_proposal(record, args.proposal_index)?;
        let options = args.options;

        match proposal {
            CuratorProposal::InferredEdge(_) => self.promote_curator_edge_authenticated(
                session,
                PrismCuratorPromoteEdgeArgs {
                    job_id: args.job_id,
                    proposal_index: args.proposal_index,
                    scope: options
                        .as_ref()
                        .and_then(|options| options.edge_scope.clone()),
                    note: args.note,
                    task_id: args.task_id,
                },
                authenticated,
            ),
            CuratorProposal::ConceptCandidate(_) => self.promote_curator_concept_authenticated(
                session,
                PrismCuratorPromoteConceptArgs {
                    job_id: args.job_id,
                    proposal_index: args.proposal_index,
                    scope: options
                        .as_ref()
                        .and_then(|options| options.concept_scope.clone()),
                    note: args.note,
                    task_id: args.task_id,
                },
                authenticated,
            ),
            CuratorProposal::StructuralMemory(_)
            | CuratorProposal::SemanticMemory(_)
            | CuratorProposal::RiskSummary(_)
            | CuratorProposal::ValidationRecipe(_) => self.promote_curator_memory_authenticated(
                session,
                PrismCuratorPromoteMemoryArgs {
                    job_id: args.job_id,
                    proposal_index: args.proposal_index,
                    trust: options.as_ref().and_then(|options| options.memory_trust),
                    note: args.note,
                    task_id: args.task_id,
                },
                authenticated,
            ),
        }
    }

    #[cfg_attr(not(test), allow(dead_code))]
    pub(crate) fn promote_curator_memory(
        &self,
        session: &SessionState,
        args: PrismCuratorPromoteMemoryArgs,
    ) -> Result<CuratorProposalDecisionResult> {
        self.promote_curator_memory_authenticated(session, args, None)
    }

    pub(crate) fn promote_curator_memory_authenticated(
        &self,
        session: &SessionState,
        args: PrismCuratorPromoteMemoryArgs,
        authenticated: Option<&AuthenticatedPrincipal>,
    ) -> Result<CuratorProposalDecisionResult> {
        let workspace = self
            .workspace_session()
            .ok_or_else(|| anyhow!("curator mutations require a workspace-backed session"))?;
        let job_id = CuratorJobId(args.job_id.clone());
        let snapshot = workspace.curator_snapshot()?;
        let record = snapshot
            .records
            .iter()
            .find(|record| record.id == job_id)
            .ok_or_else(|| anyhow!("unknown curator job `{}`", args.job_id))?;
        let proposal_state = curator_proposal_state(record, args.proposal_index)?;
        if proposal_state.disposition != CuratorProposalDisposition::Pending {
            return Err(anyhow!(
                "curator proposal {} for job `{}` is already {}",
                args.proposal_index,
                args.job_id,
                curator_disposition_label(proposal_state.disposition)
            ));
        }

        let task = session.task_for_mutation(args.task_id.clone().map(TaskId::new));
        let prism = self.current_prism();
        let proposal = curator_proposal(record, args.proposal_index)?;
        let (entry, promoted_from) = match proposal {
            CuratorProposal::StructuralMemory(candidate) => {
                let mut entry = MemoryEntry::new(candidate.kind, candidate.content.clone());
                entry.anchors = prism.anchors_for(&candidate.anchors);
                entry.scope = MemoryScope::Repo;
                entry.source = MemorySource::System;
                entry.trust = args.trust.unwrap_or(candidate.trust).clamp(0.0, 1.0);
                entry.metadata = curator_memory_metadata(
                    proposal,
                    candidate,
                    &task,
                    &args.job_id,
                    args.proposal_index,
                    Value::Null,
                );
                entry.metadata =
                    ensure_repo_publication_metadata(entry.metadata, current_timestamp());
                (entry, candidate.evidence.memory_ids.clone())
            }
            CuratorProposal::SemanticMemory(candidate) => {
                let mut entry = MemoryEntry::new(candidate.kind, candidate.content.clone());
                entry.anchors = prism.anchors_for(&candidate.anchors);
                entry.scope = MemoryScope::Repo;
                entry.source = MemorySource::System;
                entry.trust = args.trust.unwrap_or(candidate.trust).clamp(0.0, 1.0);
                entry.metadata = curator_memory_metadata(
                    proposal,
                    candidate,
                    &task,
                    &args.job_id,
                    args.proposal_index,
                    Value::Null,
                );
                entry.metadata =
                    ensure_repo_publication_metadata(entry.metadata, current_timestamp());
                (entry, candidate.evidence.memory_ids.clone())
            }
            CuratorProposal::RiskSummary(candidate) => {
                let candidate_memory = prism_curator::CandidateMemory {
                    anchors: candidate.anchors.clone(),
                    kind: MemoryKind::Semantic,
                    content: candidate.summary.clone(),
                    trust: match candidate.severity.as_str() {
                        "low" => 0.55,
                        "medium" => 0.7,
                        "high" => 0.85,
                        _ => 0.6,
                    },
                    rationale: "Curator promoted a semantic risk summary.".to_string(),
                    category: Some("risk_summary".to_string()),
                    evidence: prism_curator::CandidateMemoryEvidence {
                        event_ids: candidate.evidence_events.clone(),
                        memory_ids: Vec::new(),
                        validation_checks: Vec::new(),
                        co_change_lineages: Vec::new(),
                    },
                };
                let mut entry =
                    MemoryEntry::new(MemoryKind::Semantic, candidate_memory.content.clone());
                entry.anchors = prism.anchors_for(&candidate.anchors);
                entry.scope = MemoryScope::Repo;
                entry.source = MemorySource::System;
                entry.trust = args.trust.unwrap_or(candidate_memory.trust).clamp(0.0, 1.0);
                entry.metadata = curator_memory_metadata(
                    proposal,
                    &candidate_memory,
                    &task,
                    &args.job_id,
                    args.proposal_index,
                    json!({
                        "severity": candidate.severity,
                        "evidenceEvents": candidate
                            .evidence_events
                            .iter()
                            .map(|event| event.0.clone())
                            .collect::<Vec<_>>(),
                    }),
                );
                entry.metadata =
                    ensure_repo_publication_metadata(entry.metadata, current_timestamp());
                (entry, candidate_memory.evidence.memory_ids.clone())
            }
            CuratorProposal::ValidationRecipe(candidate) => {
                let candidate_memory = prism_curator::CandidateMemory {
                    anchors: vec![AnchorRef::Node(candidate.target.clone())],
                    kind: MemoryKind::Structural,
                    content: format!(
                        "Validation recipe for {}: {}",
                        candidate.target.path,
                        candidate.checks.join(", ")
                    ),
                    trust: 0.8,
                    rationale: candidate.rationale.clone(),
                    category: Some("validation_recipe".to_string()),
                    evidence: prism_curator::CandidateMemoryEvidence {
                        event_ids: Vec::new(),
                        memory_ids: Vec::new(),
                        validation_checks: candidate.checks.clone(),
                        co_change_lineages: Vec::new(),
                    },
                };
                let mut entry =
                    MemoryEntry::new(MemoryKind::Structural, candidate_memory.content.clone());
                entry.anchors = prism.anchors_for(&[AnchorRef::Node(candidate.target.clone())]);
                entry.scope = MemoryScope::Repo;
                entry.source = MemorySource::System;
                entry.trust = args.trust.unwrap_or(0.8).clamp(0.0, 1.0);
                entry.metadata = curator_memory_metadata(
                    proposal,
                    &candidate_memory,
                    &task,
                    &args.job_id,
                    args.proposal_index,
                    json!({
                        "target": candidate.target,
                        "checks": candidate.checks,
                        "evidence": candidate.evidence,
                    }),
                );
                entry.metadata =
                    ensure_repo_publication_metadata(entry.metadata, current_timestamp());
                (entry, candidate_memory.evidence.memory_ids.clone())
            }
            CuratorProposal::InferredEdge(_) => {
                return Err(anyhow!(
                    "curator proposal {} for job `{}` is an inferred edge; use prism_mutate with action `curator_promote_edge`",
                    args.proposal_index,
                    args.job_id
                ));
            }
            CuratorProposal::ConceptCandidate(_) => {
                return Err(anyhow!(
                    "curator proposal {} for job `{}` is a concept candidate; use prism_mutate with action `curator_promote_concept`",
                    args.proposal_index,
                    args.job_id
                ));
            }
        };
        let memory_summary = entry.content.clone();
        let memory_anchors = entry.anchors.clone();
        ensure_repo_memory_publication_is_not_duplicate(session, &entry, &[])?;
        let memory_id = session.notes.store(entry)?;
        let stored_entry = session
            .notes
            .entry(&memory_id)
            .ok_or_else(|| anyhow!("promoted memory `{}` could not be reloaded", memory_id.0))?;
        let mut memory_event = MemoryEvent::from_entry(
            MemoryEventKind::Promoted,
            stored_entry.clone(),
            Some(task.0.to_string()),
            promoted_from,
            Vec::new(),
        );
        mutation_provenance(self, session, authenticated).stamp_memory_event(&mut memory_event);
        workspace.append_memory_event(memory_event)?;
        self.reload_episodic_snapshot(workspace)?;
        let note_event = OutcomeEvent {
            meta: mutation_provenance(self, session, authenticated).event_meta(
                session.next_event_id("outcome"),
                Some(task.clone()),
                None,
                current_timestamp(),
            ),
            anchors: memory_anchors,
            kind: prism_memory::OutcomeKind::NoteAdded,
            result: prism_memory::OutcomeResult::Success,
            summary: memory_summary,
            evidence: Vec::new(),
            metadata: json!({
                "source": "curator",
                "memoryId": memory_id.0.clone(),
                "jobId": args.job_id,
                "proposalIndex": args.proposal_index,
            }),
        };
        if let Some(workspace) = self.workspace_session() {
            let _ = workspace.append_outcome(note_event)?;
            self.sync_workspace_revision(workspace)?;
        } else {
            prism.apply_outcome_event_to_projections(&note_event);
            let _ = prism.outcome_memory().store_event(note_event)?;
            self.persist_outcomes()?;
            self.persist_notes()?;
        }
        let detail = args.note.clone();
        workspace.set_curator_proposal_state(
            &job_id,
            args.proposal_index,
            CuratorProposalDisposition::Applied,
            Some(task),
            detail.clone(),
            Some(memory_id.0.clone()),
        )?;
        let proposal = self
            .curator_job(&args.job_id)?
            .and_then(|job| {
                job.proposals
                    .into_iter()
                    .find(|proposal| proposal.index == args.proposal_index)
            })
            .ok_or_else(|| anyhow!("applied curator proposal could not be reloaded"))?;
        Ok(CuratorProposalDecisionResult {
            job_id: args.job_id,
            proposal_index: args.proposal_index,
            kind: proposal.kind.clone(),
            decision: CuratorProposalDecision::Applied,
            proposal: serde_json::to_value(proposal)?,
            created: CuratorProposalCreatedResources {
                memory_id: Some(memory_id.0.clone()),
                edge_id: None,
                concept_handle: None,
            },
            detail,
            memory_id: Some(memory_id.0),
            edge_id: None,
            concept_handle: None,
        })
    }

    #[cfg_attr(not(test), allow(dead_code))]
    pub(crate) fn reject_curator_proposal(
        &self,
        session: &SessionState,
        args: PrismCuratorRejectProposalArgs,
    ) -> Result<CuratorProposalDecisionResult> {
        self.reject_curator_proposal_authenticated(session, args, None)
    }

    pub(crate) fn reject_curator_proposal_authenticated(
        &self,
        session: &SessionState,
        args: PrismCuratorRejectProposalArgs,
        _authenticated: Option<&AuthenticatedPrincipal>,
    ) -> Result<CuratorProposalDecisionResult> {
        let workspace = self
            .workspace_session()
            .ok_or_else(|| anyhow!("curator mutations require a workspace-backed session"))?;
        let job_id = CuratorJobId(args.job_id.clone());
        let snapshot = workspace.curator_snapshot()?;
        let record = snapshot
            .records
            .iter()
            .find(|record| record.id == job_id)
            .ok_or_else(|| anyhow!("unknown curator job `{}`", args.job_id))?;
        let proposal_state = curator_proposal_state(record, args.proposal_index)?;
        if proposal_state.disposition != CuratorProposalDisposition::Pending {
            return Err(anyhow!(
                "curator proposal {} for job `{}` is already {}",
                args.proposal_index,
                args.job_id,
                curator_disposition_label(proposal_state.disposition)
            ));
        }

        let task = session.task_for_mutation(args.task_id.map(TaskId::new));
        let detail = args.reason.clone();
        workspace.set_curator_proposal_state(
            &job_id,
            args.proposal_index,
            CuratorProposalDisposition::Rejected,
            Some(task),
            detail.clone(),
            None,
        )?;
        let proposal = self
            .curator_job(&args.job_id)?
            .and_then(|job| {
                job.proposals
                    .into_iter()
                    .find(|proposal| proposal.index == args.proposal_index)
            })
            .ok_or_else(|| anyhow!("rejected curator proposal could not be reloaded"))?;
        Ok(CuratorProposalDecisionResult {
            job_id: args.job_id,
            proposal_index: args.proposal_index,
            kind: proposal.kind.clone(),
            decision: CuratorProposalDecision::Rejected,
            proposal: serde_json::to_value(proposal)?,
            created: CuratorProposalCreatedResources::default(),
            detail,
            memory_id: None,
            edge_id: None,
            concept_handle: None,
        })
    }

    pub(crate) fn curator_jobs(&self, args: crate::CuratorJobsArgs) -> Result<Vec<CuratorJobView>> {
        self.refresh_workspace()?;
        let Some(workspace) = self.workspace_session() else {
            return Ok(Vec::new());
        };
        let mut jobs = workspace
            .curator_snapshot()?
            .records
            .into_iter()
            .filter(|record| {
                args.status
                    .as_deref()
                    .is_none_or(|status| curator_job_status_label(record) == status)
                    && args
                        .trigger
                        .as_deref()
                        .is_none_or(|trigger| curator_trigger_label(&record.job.trigger) == trigger)
            })
            .map(crate::curator_job_view)
            .collect::<Result<Vec<_>>>()?;

        jobs.sort_by(|left, right| right.created_at.cmp(&left.created_at));
        if let Some(limit) = args.limit {
            jobs.truncate(limit);
        }
        Ok(jobs)
    }

    pub(crate) fn curator_proposals(
        &self,
        args: crate::CuratorProposalsArgs,
    ) -> Result<Vec<CuratorProposalRecordView>> {
        self.refresh_workspace()?;
        let Some(workspace) = self.workspace_session() else {
            return Ok(Vec::new());
        };
        let mut proposals = Vec::new();
        for record in workspace.curator_snapshot()?.records {
            if args
                .status
                .as_deref()
                .is_some_and(|status| curator_job_status_label(&record) != status)
                || args
                    .trigger
                    .as_deref()
                    .is_some_and(|trigger| curator_trigger_label(&record.job.trigger) != trigger)
            {
                continue;
            }
            let run = record.run.clone().unwrap_or_default();
            for (index, proposal) in run.proposals.into_iter().enumerate() {
                let state = record
                    .proposal_states
                    .get(index)
                    .cloned()
                    .unwrap_or_default();
                if args.disposition.as_deref().is_some_and(|disposition| {
                    curator_disposition_label(state.disposition) != disposition
                }) || args.task_id.as_deref().is_some_and(|task_id| {
                    record.job.task.as_ref().map(|task| task.0.as_str()) != Some(task_id)
                        && state.task.as_ref().map(|task| task.0.as_str()) != Some(task_id)
                }) {
                    continue;
                }
                let proposal_view =
                    crate::curator_proposal_record_view(&record, index, proposal, state)?;
                if args
                    .kind
                    .as_deref()
                    .is_none_or(|kind| proposal_view.kind == kind)
                {
                    proposals.push(proposal_view);
                }
            }
        }

        proposals.sort_by(|left, right| {
            right
                .job_created_at
                .cmp(&left.job_created_at)
                .then_with(|| left.index.cmp(&right.index))
        });
        if let Some(limit) = args.limit {
            proposals.truncate(limit);
        }
        Ok(proposals)
    }

    pub(crate) fn curator_job(&self, job_id: &str) -> Result<Option<CuratorJobView>> {
        self.refresh_workspace()?;
        let Some(workspace) = self.workspace_session() else {
            return Ok(None);
        };
        workspace
            .curator_snapshot()?
            .records
            .into_iter()
            .find(|record| record.id.0 == job_id)
            .map(crate::curator_job_view)
            .transpose()
    }
}

fn memory_event_kind_for_store(
    promoted_from: &[prism_memory::MemoryId],
    supersedes: &[prism_memory::MemoryId],
) -> MemoryEventKind {
    if !supersedes.is_empty() && promoted_from.is_empty() {
        MemoryEventKind::Superseded
    } else if !promoted_from.is_empty() || !supersedes.is_empty() {
        MemoryEventKind::Promoted
    } else {
        MemoryEventKind::Stored
    }
}

fn ensure_repo_memory_publication_is_not_duplicate(
    session: &SessionState,
    entry: &MemoryEntry,
    supersedes: &[prism_memory::MemoryId],
) -> Result<()> {
    if entry.scope != MemoryScope::Repo {
        return Ok(());
    }
    let duplicate_ids = session
        .notes
        .snapshot()
        .entries
        .into_iter()
        .filter(|existing| existing.scope == MemoryScope::Repo)
        .filter(|existing| existing.kind == entry.kind)
        .filter(|existing| !supersedes.iter().any(|id| id == &existing.id))
        .filter(|existing| memory_publication_status(existing) != Some("retired"))
        .filter(|existing| entries_share_anchor(existing, entry))
        .filter(|existing| {
            normalize_memory_content(&existing.content) == normalize_memory_content(&entry.content)
        })
        .map(|existing| existing.id.0)
        .collect::<Vec<_>>();
    if duplicate_ids.is_empty() {
        return Ok(());
    }
    Err(anyhow!(
        "repo-published memory duplicates active published memory {}. Add `supersedes` to publish a reviewed replacement, or retire the older memory first.",
        duplicate_ids.join(", ")
    ))
}

fn entries_share_anchor(left: &MemoryEntry, right: &MemoryEntry) -> bool {
    left.anchors
        .iter()
        .any(|anchor| right.anchors.iter().any(|candidate| candidate == anchor))
}

fn memory_publication_status(entry: &MemoryEntry) -> Option<&str> {
    entry
        .metadata
        .get("publication")
        .and_then(|value| value.get("status"))
        .and_then(Value::as_str)
}

fn normalize_memory_content(content: &str) -> String {
    content
        .split_whitespace()
        .collect::<Vec<_>>()
        .join(" ")
        .to_ascii_lowercase()
}

fn outcome_validation_labels(evidence: &[OutcomeEvidence]) -> Vec<String> {
    let mut labels = evidence
        .iter()
        .filter_map(|evidence| match evidence {
            OutcomeEvidence::Test { name, .. } => Some(normalize_validation_label(name, "test:")),
            OutcomeEvidence::Build { target, .. } => {
                Some(normalize_validation_label(target, "build:"))
            }
            OutcomeEvidence::Command { argv, passed } if *passed => match argv.as_slice() {
                [tool, subcommand, ..] if tool == "cargo" && subcommand == "test" => {
                    Some(format!("test:{}", argv.join(" ")))
                }
                [tool, subcommand, ..] if tool == "cargo" && subcommand == "build" => {
                    Some(format!("build:{}", argv.join(" ")))
                }
                _ => None,
            },
            _ => None,
        })
        .collect::<Vec<_>>();
    labels.sort();
    labels.dedup();
    labels
}

fn normalize_validation_label(value: &str, default_prefix: &str) -> String {
    let value = value.trim();
    if value.starts_with("test:")
        || value.starts_with("build:")
        || value.starts_with("validation:")
        || value.starts_with("command:")
    {
        value.to_string()
    } else {
        format!("{default_prefix}{value}")
    }
}

fn build_promoted_concept_packet(
    prism: &Prism,
    task_id: &TaskId,
    recorded_at: u64,
    args: PrismConceptMutationArgs,
) -> Result<ConceptPacket> {
    let scope = args
        .scope
        .clone()
        .map(convert_concept_scope)
        .unwrap_or(ConceptScope::Session);
    let canonical_name = args
        .canonical_name
        .map(|value| value.trim().to_string())
        .filter(|value| !value.is_empty())
        .ok_or_else(|| anyhow!("concept promote requires canonicalName"))?;
    let summary = args
        .summary
        .map(|value| value.trim().to_string())
        .filter(|value| !value.is_empty())
        .ok_or_else(|| anyhow!("concept promote requires summary"))?;
    let core_members = args
        .core_members
        .ok_or_else(|| anyhow!("concept promote requires coreMembers"))?;

    let core_members = convert_concept_nodes(prism, core_members, "coreMembers")?;
    let supporting_members =
        convert_optional_concept_nodes(prism, args.supporting_members, "supportingMembers")?;
    let likely_tests = convert_optional_concept_nodes(prism, args.likely_tests, "likelyTests")?;
    let risk_hint = match parse_sparse_patch(args.risk_hint, "riskHint")? {
        SparsePatch::Keep | SparsePatch::Clear => None,
        SparsePatch::Set(value) => {
            let risk_hint = value.trim().to_string();
            if risk_hint.is_empty() {
                return Err(anyhow!("concept riskHint cannot be empty"));
            }
            Some(risk_hint)
        }
    };

    let packet = ConceptPacket {
        handle: normalize_concept_handle(args.handle.as_deref(), &canonical_name),
        canonical_name,
        summary,
        aliases: sanitize_strings(args.aliases.unwrap_or_default()),
        confidence: args.confidence.unwrap_or(0.88).clamp(0.0, 1.0),
        core_members: core_members.clone(),
        core_member_lineages: concept_member_lineages(prism, &core_members),
        supporting_members: supporting_members.clone(),
        supporting_member_lineages: concept_member_lineages(prism, &supporting_members),
        likely_tests: likely_tests.clone(),
        likely_test_lineages: concept_member_lineages(prism, &likely_tests),
        evidence: sanitize_strings(args.evidence.unwrap_or_else(|| {
            vec!["Promoted from live repo work through prism_mutate.".to_string()]
        })),
        risk_hint,
        decode_lenses: convert_concept_lenses(args.decode_lenses),
        scope,
        provenance: ConceptProvenance {
            origin: match scope {
                ConceptScope::Local => "local_mutation".to_string(),
                ConceptScope::Session => "session_mutation".to_string(),
                ConceptScope::Repo => "repo_mutation".to_string(),
            },
            kind: "manual_concept_promote".to_string(),
            task_id: Some(task_id.0.to_string()),
        },
        publication: (scope == ConceptScope::Repo).then_some(ConceptPublication {
            published_at: recorded_at,
            last_reviewed_at: Some(recorded_at),
            status: ConceptPublicationStatus::Active,
            supersedes: normalize_concept_handles(args.supersedes.unwrap_or_default()),
            retired_at: None,
            retirement_reason: None,
        }),
    };
    validate_concept_packet(&packet)?;
    Ok(packet)
}

fn build_promoted_contract_packet(
    prism: &Prism,
    workspace: Option<&WorkspaceSession>,
    workspace_root: Option<&std::path::Path>,
    task_id: &TaskId,
    recorded_at: u64,
    args: PrismContractMutationArgs,
) -> Result<ContractPacket> {
    let scope = args
        .scope
        .clone()
        .map(convert_concept_scope)
        .unwrap_or(ConceptScope::Session);
    let name = args
        .name
        .map(|value| value.trim().to_string())
        .filter(|value| !value.is_empty())
        .ok_or_else(|| anyhow!("contract promote requires name"))?;
    let summary = args
        .summary
        .map(|value| value.trim().to_string())
        .filter(|value| !value.is_empty())
        .ok_or_else(|| anyhow!("contract promote requires summary"))?;
    let kind = args
        .kind
        .clone()
        .map(convert_contract_kind)
        .ok_or_else(|| anyhow!("contract promote requires kind"))?;
    let subject = convert_contract_target(
        prism,
        workspace,
        workspace_root,
        args.subject
            .clone()
            .ok_or_else(|| anyhow!("contract promote requires subject"))?,
    )?;
    let guarantees = convert_contract_guarantees(
        args.guarantees
            .clone()
            .ok_or_else(|| anyhow!("contract promote requires guarantees"))?,
    )?;
    let status = args
        .status
        .clone()
        .map(convert_contract_status)
        .unwrap_or_else(|| {
            if scope == ConceptScope::Repo {
                ContractStatus::Active
            } else {
                ContractStatus::Candidate
            }
        });

    let packet = ContractPacket {
        handle: normalize_contract_handle(args.handle.as_deref(), &name),
        name,
        summary,
        aliases: sanitize_strings(args.aliases.unwrap_or_default()),
        kind,
        subject,
        guarantees,
        assumptions: sanitize_strings(args.assumptions.unwrap_or_default()),
        consumers: convert_contract_targets(prism, workspace, workspace_root, args.consumers)?,
        validations: convert_contract_validations(
            prism,
            workspace,
            workspace_root,
            args.validations,
        )?,
        stability: args
            .stability
            .clone()
            .map(convert_contract_stability)
            .unwrap_or(ContractStability::Internal),
        compatibility: args
            .compatibility
            .map(convert_contract_compatibility)
            .unwrap_or_default(),
        evidence: sanitize_strings(args.evidence.unwrap_or_else(|| {
            vec!["Promoted from live repo work through prism_mutate.".to_string()]
        })),
        status,
        scope,
        provenance: ConceptProvenance {
            origin: match scope {
                ConceptScope::Local => "local_mutation".to_string(),
                ConceptScope::Session => "session_mutation".to_string(),
                ConceptScope::Repo => "repo_mutation".to_string(),
            },
            kind: "manual_contract_promote".to_string(),
            task_id: Some(task_id.0.to_string()),
        },
        publication: (scope == ConceptScope::Repo).then_some(ConceptPublication {
            published_at: recorded_at,
            last_reviewed_at: Some(recorded_at),
            status: if status == ContractStatus::Retired {
                ConceptPublicationStatus::Retired
            } else {
                ConceptPublicationStatus::Active
            },
            supersedes: normalize_contract_handles(args.supersedes.unwrap_or_default()),
            retired_at: (status == ContractStatus::Retired).then_some(recorded_at),
            retirement_reason: args.retirement_reason.clone(),
        }),
    };
    validate_contract_packet(&packet)?;
    Ok(packet)
}

fn build_updated_contract_packet(
    prism: &Prism,
    workspace: Option<&WorkspaceSession>,
    workspace_root: Option<&std::path::Path>,
    task_id: &TaskId,
    recorded_at: u64,
    args: PrismContractMutationArgs,
) -> Result<ContractPacket> {
    let handle =
        required_contract_handle(args.handle.as_deref(), "contract update requires handle")?;
    let mut packet = current_contract(prism, &handle)?;
    let mut changed = false;

    if let Some(name) = args
        .name
        .map(|value| value.trim().to_string())
        .filter(|value| !value.is_empty())
    {
        packet.name = name;
        changed = true;
    }
    if let Some(summary) = args
        .summary
        .map(|value| value.trim().to_string())
        .filter(|value| !value.is_empty())
    {
        packet.summary = summary;
        changed = true;
    }
    if let Some(aliases) = args.aliases {
        packet.aliases = sanitize_strings(aliases);
        changed = true;
    }
    if let Some(kind) = args.kind {
        packet.kind = convert_contract_kind(kind);
        changed = true;
    }
    if let Some(subject) = args.subject {
        packet.subject = convert_contract_target(prism, workspace, workspace_root, subject)?;
        changed = true;
    }
    if let Some(guarantees) = args.guarantees {
        packet.guarantees = convert_contract_guarantees(guarantees)?;
        changed = true;
    }
    if let Some(assumptions) = args.assumptions {
        packet.assumptions = sanitize_strings(assumptions);
        changed = true;
    }
    if let Some(consumers) = args.consumers {
        packet.consumers =
            convert_contract_targets(prism, workspace, workspace_root, Some(consumers))?;
        changed = true;
    }
    if let Some(validations) = args.validations {
        packet.validations =
            convert_contract_validations(prism, workspace, workspace_root, Some(validations))?;
        changed = true;
    }
    if let Some(stability) = args.stability {
        packet.stability = convert_contract_stability(stability);
        changed = true;
    }
    if let Some(compatibility) = args.compatibility {
        packet.compatibility = convert_contract_compatibility(compatibility);
        changed = true;
    }
    if let Some(evidence) = args.evidence {
        packet.evidence = sanitize_strings(evidence);
        changed = true;
    }
    if let Some(status) = args.status {
        packet.status = convert_contract_status(status);
        changed = true;
    }
    if let Some(scope) = args.scope.map(convert_concept_scope) {
        packet.scope = scope;
        changed = true;
    }
    if let Some(supersedes) = args.supersedes {
        let publication = packet
            .publication
            .get_or_insert_with(ConceptPublication::default);
        publication.supersedes = normalize_contract_handles(supersedes);
        changed = true;
    }
    if !changed {
        return Err(anyhow!(
            "contract update requires at least one changed field"
        ));
    }
    packet.provenance = ConceptProvenance {
        origin: match packet.scope {
            ConceptScope::Local => "local_mutation".to_string(),
            ConceptScope::Session => "session_mutation".to_string(),
            ConceptScope::Repo => "repo_mutation".to_string(),
        },
        kind: "manual_contract_update".to_string(),
        task_id: Some(task_id.0.to_string()),
    };
    packet.publication = update_contract_publication(
        packet.publication,
        packet.scope,
        packet.status,
        recorded_at,
        None,
    );
    validate_contract_packet(&packet)?;
    Ok(packet)
}

fn build_retired_contract_packet(
    prism: &Prism,
    task_id: &TaskId,
    recorded_at: u64,
    args: PrismContractMutationArgs,
) -> Result<ContractPacket> {
    let handle =
        required_contract_handle(args.handle.as_deref(), "contract retire requires handle")?;
    let mut packet = current_contract(prism, &handle)?;
    packet.status = ContractStatus::Retired;
    packet.provenance = ConceptProvenance {
        origin: match packet.scope {
            ConceptScope::Local => "local_mutation".to_string(),
            ConceptScope::Session => "session_mutation".to_string(),
            ConceptScope::Repo => "repo_mutation".to_string(),
        },
        kind: "manual_contract_retire".to_string(),
        task_id: Some(task_id.0.to_string()),
    };
    packet.publication = update_contract_publication(
        packet.publication,
        packet.scope,
        packet.status,
        recorded_at,
        args.retirement_reason.clone(),
    );
    validate_contract_packet(&packet)?;
    Ok(packet)
}

fn build_contract_with_evidence_attached(
    prism: &Prism,
    task_id: &TaskId,
    recorded_at: u64,
    args: PrismContractMutationArgs,
) -> Result<ContractPacket> {
    let additions = sanitize_strings(
        args.evidence
            .clone()
            .ok_or_else(|| anyhow!("attach_evidence requires evidence"))?,
    );
    if additions.is_empty() {
        return Err(anyhow!("attach_evidence requires non-empty evidence"));
    }
    let handle =
        required_contract_handle(args.handle.as_deref(), "attach_evidence requires handle")?;
    let mut packet = current_contract(prism, &handle)?;
    packet.evidence = merge_unique_strings(packet.evidence, additions);
    packet.provenance = ConceptProvenance {
        origin: origin_for_scope(packet.scope).to_string(),
        kind: "manual_contract_attach_evidence".to_string(),
        task_id: Some(task_id.0.to_string()),
    };
    packet.publication = update_contract_publication(
        packet.publication,
        packet.scope,
        packet.status,
        recorded_at,
        None,
    );
    validate_contract_packet(&packet)?;
    Ok(packet)
}

fn build_contract_with_validation_attached(
    prism: &Prism,
    workspace: Option<&WorkspaceSession>,
    workspace_root: Option<&std::path::Path>,
    task_id: &TaskId,
    recorded_at: u64,
    args: PrismContractMutationArgs,
) -> Result<ContractPacket> {
    let additions =
        convert_contract_validations(prism, workspace, workspace_root, args.validations.clone())?;
    if additions.is_empty() {
        return Err(anyhow!("attach_validation requires validations"));
    }
    let handle =
        required_contract_handle(args.handle.as_deref(), "attach_validation requires handle")?;
    let mut packet = current_contract(prism, &handle)?;
    packet.validations = merge_contract_validations(packet.validations, additions);
    packet.provenance = ConceptProvenance {
        origin: origin_for_scope(packet.scope).to_string(),
        kind: "manual_contract_attach_validation".to_string(),
        task_id: Some(task_id.0.to_string()),
    };
    packet.publication = update_contract_publication(
        packet.publication,
        packet.scope,
        packet.status,
        recorded_at,
        None,
    );
    validate_contract_packet(&packet)?;
    Ok(packet)
}

fn build_contract_with_consumer_recorded(
    prism: &Prism,
    workspace: Option<&WorkspaceSession>,
    workspace_root: Option<&std::path::Path>,
    task_id: &TaskId,
    recorded_at: u64,
    args: PrismContractMutationArgs,
) -> Result<ContractPacket> {
    let additions =
        convert_contract_targets(prism, workspace, workspace_root, args.consumers.clone())?;
    if additions.is_empty() {
        return Err(anyhow!("record_consumer requires consumers"));
    }
    let handle =
        required_contract_handle(args.handle.as_deref(), "record_consumer requires handle")?;
    let mut packet = current_contract(prism, &handle)?;
    packet.consumers = merge_contract_targets(packet.consumers, additions);
    packet.provenance = ConceptProvenance {
        origin: origin_for_scope(packet.scope).to_string(),
        kind: "manual_contract_record_consumer".to_string(),
        task_id: Some(task_id.0.to_string()),
    };
    packet.publication = update_contract_publication(
        packet.publication,
        packet.scope,
        packet.status,
        recorded_at,
        None,
    );
    validate_contract_packet(&packet)?;
    Ok(packet)
}

fn build_contract_with_status_set(
    prism: &Prism,
    task_id: &TaskId,
    recorded_at: u64,
    args: PrismContractMutationArgs,
) -> Result<ContractPacket> {
    let status = args
        .status
        .clone()
        .map(convert_contract_status)
        .ok_or_else(|| anyhow!("set_status requires status"))?;
    let handle = required_contract_handle(args.handle.as_deref(), "set_status requires handle")?;
    let mut packet = current_contract(prism, &handle)?;
    packet.status = status;
    packet.provenance = ConceptProvenance {
        origin: origin_for_scope(packet.scope).to_string(),
        kind: "manual_contract_set_status".to_string(),
        task_id: Some(task_id.0.to_string()),
    };
    packet.publication = update_contract_publication(
        packet.publication,
        packet.scope,
        packet.status,
        recorded_at,
        None,
    );
    validate_contract_packet(&packet)?;
    Ok(packet)
}

fn concept_args_from_curator_candidate(
    candidate: &CandidateConcept,
    task_id: &TaskId,
    scope: Option<ConceptScopeInput>,
) -> PrismConceptMutationArgs {
    PrismConceptMutationArgs {
        operation: match candidate.recommended_operation {
            CandidateConceptOperation::Promote => ConceptMutationOperationInput::Promote,
        },
        handle: None,
        canonical_name: Some(candidate.canonical_name.clone()),
        summary: Some(candidate.summary.clone()),
        aliases: (!candidate.aliases.is_empty()).then_some(candidate.aliases.clone()),
        core_members: Some(
            candidate
                .core_members
                .iter()
                .cloned()
                .map(node_id_input)
                .collect(),
        ),
        supporting_members: (!candidate.supporting_members.is_empty()).then_some(
            candidate
                .supporting_members
                .iter()
                .cloned()
                .map(node_id_input)
                .collect(),
        ),
        likely_tests: (!candidate.likely_tests.is_empty()).then_some(
            candidate
                .likely_tests
                .iter()
                .cloned()
                .map(node_id_input)
                .collect(),
        ),
        evidence: (!candidate.evidence.is_empty()).then_some(candidate.evidence.clone()),
        risk_hint: None,
        confidence: Some(candidate.confidence),
        decode_lenses: Some(vec![
            PrismConceptLensInput::Open,
            PrismConceptLensInput::Workset,
            PrismConceptLensInput::Validation,
        ]),
        scope: Some(scope.unwrap_or(ConceptScopeInput::Session)),
        supersedes: None,
        retirement_reason: None,
        task_id: Some(task_id.0.to_string()),
    }
}

fn build_concept_relation(
    prism: &Prism,
    task_id: &TaskId,
    args: &PrismConceptRelationMutationArgs,
) -> Result<ConceptRelation> {
    let source_handle = normalize_concept_handle(Some(&args.source_handle), &args.source_handle);
    let target_handle = normalize_concept_handle(Some(&args.target_handle), &args.target_handle);
    if source_handle == target_handle {
        return Err(anyhow!(
            "concept relations require distinct source and target handles"
        ));
    }
    prism
        .concept_by_handle(&source_handle)
        .ok_or_else(|| anyhow!("no concept packet matched `{source_handle}`"))?;
    prism
        .concept_by_handle(&target_handle)
        .ok_or_else(|| anyhow!("no concept packet matched `{target_handle}`"))?;
    let kind = convert_concept_relation_kind(args.kind.clone());
    match args.operation {
        ConceptRelationMutationOperationInput::Upsert => Ok(ConceptRelation {
            source_handle,
            target_handle,
            kind,
            confidence: args.confidence.unwrap_or(0.78).clamp(0.0, 1.0),
            evidence: sanitize_strings(args.evidence.clone().unwrap_or_default()),
            scope: args
                .scope
                .clone()
                .map(convert_concept_scope)
                .unwrap_or(ConceptScope::Session),
            provenance: ConceptProvenance {
                origin: "manual_concept_relation".to_string(),
                kind: "manual_concept_relation".to_string(),
                task_id: Some(task_id.0.to_string()),
            },
        }),
        ConceptRelationMutationOperationInput::Retire => prism
            .concept_relations_for_handle(&source_handle)
            .into_iter()
            .find(|relation| {
                relation.source_handle.eq_ignore_ascii_case(&source_handle)
                    && relation.target_handle.eq_ignore_ascii_case(&target_handle)
                    && relation.kind == kind
            })
            .ok_or_else(|| {
                anyhow!(
                    "no concept relation matched `{}` -> `{}` ({:?})",
                    source_handle,
                    target_handle,
                    kind
                )
            }),
    }
}

fn node_id_input(id: prism_ir::NodeId) -> NodeIdInput {
    NodeIdInput {
        crate_name: id.crate_name.to_string(),
        path: id.path.to_string(),
        kind: id.kind.to_string(),
    }
}

fn parse_sparse_patch<T>(
    value: Option<SparsePatchInput<T>>,
    field: &str,
) -> Result<SparsePatch<T>> {
    value
        .map(|patch| patch.into_patch(field))
        .transpose()
        .map_err(|error| anyhow!(error))?
        .map_or(Ok(SparsePatch::Keep), Ok)
}

fn concept_event_patch(
    args: &PrismConceptMutationArgs,
    operation: &ConceptMutationOperationInput,
    packet: &ConceptPacket,
) -> Result<Option<ConceptEventPatch>> {
    let mut patch = ConceptEventPatch::default();
    match operation {
        ConceptMutationOperationInput::Promote => return Ok(None),
        ConceptMutationOperationInput::Update => {
            if args.canonical_name.is_some() {
                patch.set_fields.push("canonicalName".to_string());
                patch.canonical_name = Some(packet.canonical_name.clone());
            }
            if args.summary.is_some() {
                patch.set_fields.push("summary".to_string());
                patch.summary = Some(packet.summary.clone());
            }
            if args.aliases.is_some() {
                patch.set_fields.push("aliases".to_string());
                patch.aliases = Some(packet.aliases.clone());
            }
            if args.core_members.is_some() {
                patch.set_fields.push("coreMembers".to_string());
                patch.core_members = Some(packet.core_members.clone());
                patch.core_member_lineages = Some(packet.core_member_lineages.clone());
            }
            if args.supporting_members.is_some() {
                patch.set_fields.push("supportingMembers".to_string());
                patch.supporting_members = Some(packet.supporting_members.clone());
                patch.supporting_member_lineages = Some(packet.supporting_member_lineages.clone());
            }
            if args.likely_tests.is_some() {
                patch.set_fields.push("likelyTests".to_string());
                patch.likely_tests = Some(packet.likely_tests.clone());
                patch.likely_test_lineages = Some(packet.likely_test_lineages.clone());
            }
            if args.evidence.is_some() {
                patch.set_fields.push("evidence".to_string());
                patch.evidence = Some(packet.evidence.clone());
            }
            match parse_sparse_patch(args.risk_hint.clone(), "riskHint")? {
                SparsePatch::Keep => {}
                SparsePatch::Set(_) => {
                    patch.set_fields.push("riskHint".to_string());
                    patch.risk_hint = packet.risk_hint.clone();
                }
                SparsePatch::Clear => patch.cleared_fields.push("riskHint".to_string()),
            }
            if args.confidence.is_some() {
                patch.set_fields.push("confidence".to_string());
                patch.confidence = Some(packet.confidence);
            }
            if args.decode_lenses.is_some() {
                patch.set_fields.push("decodeLenses".to_string());
                patch.decode_lenses = Some(packet.decode_lenses.clone());
            }
            if args.scope.is_some() {
                patch.set_fields.push("scope".to_string());
                patch.scope = Some(packet.scope);
            }
            if args.supersedes.is_some() {
                patch.set_fields.push("supersedes".to_string());
                patch.supersedes = Some(
                    packet
                        .publication
                        .as_ref()
                        .map(|publication| publication.supersedes.clone())
                        .unwrap_or_default(),
                );
            }
        }
        ConceptMutationOperationInput::Retire => {
            if args.retirement_reason.is_some() {
                patch.set_fields.push("retirementReason".to_string());
                patch.retirement_reason = packet
                    .publication
                    .as_ref()
                    .and_then(|publication| publication.retirement_reason.clone());
            }
        }
    }
    if patch.set_fields.is_empty() && patch.cleared_fields.is_empty() {
        Ok(None)
    } else {
        Ok(Some(patch))
    }
}

fn contract_event_patch(
    args: &PrismContractMutationArgs,
    operation: &ContractMutationOperationInput,
    packet: &ContractPacket,
) -> Result<Option<ContractEventPatch>> {
    let mut patch = ContractEventPatch::default();
    match operation {
        ContractMutationOperationInput::Promote => return Ok(None),
        ContractMutationOperationInput::Update => {
            if args.name.is_some() {
                patch.set_fields.push("name".to_string());
                patch.name = Some(packet.name.clone());
            }
            if args.summary.is_some() {
                patch.set_fields.push("summary".to_string());
                patch.summary = Some(packet.summary.clone());
            }
            if args.aliases.is_some() {
                patch.set_fields.push("aliases".to_string());
                patch.aliases = Some(packet.aliases.clone());
            }
            if args.kind.is_some() {
                patch.set_fields.push("kind".to_string());
                patch.kind = Some(packet.kind);
            }
            if args.subject.is_some() {
                patch.set_fields.push("subject".to_string());
                patch.subject = Some(packet.subject.clone());
            }
            if args.guarantees.is_some() {
                patch.set_fields.push("guarantees".to_string());
                patch.guarantees = Some(packet.guarantees.clone());
            }
            if args.assumptions.is_some() {
                patch.set_fields.push("assumptions".to_string());
                patch.assumptions = Some(packet.assumptions.clone());
            }
            if args.consumers.is_some() {
                patch.set_fields.push("consumers".to_string());
                patch.consumers = Some(packet.consumers.clone());
            }
            if args.validations.is_some() {
                patch.set_fields.push("validations".to_string());
                patch.validations = Some(packet.validations.clone());
            }
            if args.stability.is_some() {
                patch.set_fields.push("stability".to_string());
                patch.stability = Some(packet.stability);
            }
            if args.compatibility.is_some() {
                patch.set_fields.push("compatibility".to_string());
                patch.compatibility = Some(packet.compatibility.clone());
            }
            if args.evidence.is_some() {
                patch.set_fields.push("evidence".to_string());
                patch.evidence = Some(packet.evidence.clone());
            }
            if args.status.is_some() {
                patch.set_fields.push("status".to_string());
                patch.status = Some(packet.status);
            }
            if args.scope.is_some() {
                patch.set_fields.push("scope".to_string());
                patch.scope = Some(packet.scope);
            }
            if args.supersedes.is_some() {
                patch.set_fields.push("supersedes".to_string());
                patch.supersedes = Some(
                    packet
                        .publication
                        .as_ref()
                        .map(|publication| publication.supersedes.clone())
                        .unwrap_or_default(),
                );
            }
        }
        ContractMutationOperationInput::Retire => {
            patch.set_fields.push("status".to_string());
            patch.status = Some(packet.status);
            if args.retirement_reason.is_some() {
                patch.set_fields.push("retirementReason".to_string());
                patch.retirement_reason = args.retirement_reason.clone();
            }
        }
        ContractMutationOperationInput::AttachEvidence => {
            patch.set_fields.push("evidence".to_string());
            patch.evidence = Some(packet.evidence.clone());
        }
        ContractMutationOperationInput::AttachValidation => {
            patch.set_fields.push("validations".to_string());
            patch.validations = Some(packet.validations.clone());
        }
        ContractMutationOperationInput::RecordConsumer => {
            patch.set_fields.push("consumers".to_string());
            patch.consumers = Some(packet.consumers.clone());
        }
        ContractMutationOperationInput::SetStatus => {
            patch.set_fields.push("status".to_string());
            patch.status = Some(packet.status);
            if args.retirement_reason.is_some() {
                patch.set_fields.push("retirementReason".to_string());
                patch.retirement_reason = args.retirement_reason.clone();
            }
        }
    }
    if patch.set_fields.is_empty() && patch.cleared_fields.is_empty() {
        Ok(None)
    } else {
        Ok(Some(patch))
    }
}

fn required_contract_handle(handle: Option<&str>, message: &str) -> Result<String> {
    handle
        .map(|value| normalize_contract_handle(Some(value), value))
        .ok_or_else(|| anyhow!("{message}"))
}

fn current_contract(prism: &Prism, handle: &str) -> Result<ContractPacket> {
    prism
        .contract_by_handle(handle)
        .ok_or_else(|| anyhow!("no contract packet matched `{handle}`"))
}

fn normalize_contract_handle(handle: Option<&str>, name: &str) -> String {
    handle
        .map(|value| value.trim())
        .filter(|value| !value.is_empty())
        .map(|value| canonical_contract_handle(value.trim_start_matches("contract://")))
        .unwrap_or_else(|| canonical_contract_handle(name))
}

fn normalize_contract_handles(handles: Vec<String>) -> Vec<String> {
    let mut normalized = sanitize_strings(handles)
        .into_iter()
        .map(|handle| canonical_contract_handle(handle.trim_start_matches("contract://")))
        .collect::<Vec<_>>();
    normalized.sort();
    normalized.dedup();
    normalized
}

fn update_contract_publication(
    publication: Option<ConceptPublication>,
    scope: ConceptScope,
    status: ContractStatus,
    recorded_at: u64,
    retirement_reason: Option<String>,
) -> Option<ConceptPublication> {
    if scope != ConceptScope::Repo && status != ContractStatus::Retired {
        return None;
    }
    let mut publication = publication.unwrap_or_default();
    if publication.published_at == 0 {
        publication.published_at = recorded_at;
    }
    publication.last_reviewed_at = Some(recorded_at);
    if status == ContractStatus::Retired {
        publication.status = ConceptPublicationStatus::Retired;
        publication.retired_at = Some(recorded_at);
        if retirement_reason.is_some() {
            publication.retirement_reason = retirement_reason;
        } else if publication.retirement_reason.is_none() {
            publication.retirement_reason = Some("retired".to_string());
        }
    } else {
        publication.status = ConceptPublicationStatus::Active;
        publication.retired_at = None;
        publication.retirement_reason = None;
    }
    Some(publication)
}

fn origin_for_scope(scope: ConceptScope) -> &'static str {
    match scope {
        ConceptScope::Local => "local_mutation",
        ConceptScope::Session => "session_mutation",
        ConceptScope::Repo => "repo_mutation",
    }
}

fn convert_contract_kind(kind: ContractKindInput) -> ContractKind {
    match kind {
        ContractKindInput::Interface => ContractKind::Interface,
        ContractKindInput::Behavioral => ContractKind::Behavioral,
        ContractKindInput::DataShape => ContractKind::DataShape,
        ContractKindInput::DependencyBoundary => ContractKind::DependencyBoundary,
        ContractKindInput::Lifecycle => ContractKind::Lifecycle,
        ContractKindInput::Protocol => ContractKind::Protocol,
        ContractKindInput::Operational => ContractKind::Operational,
    }
}

fn convert_contract_status(status: ContractStatusInput) -> ContractStatus {
    match status {
        ContractStatusInput::Candidate => ContractStatus::Candidate,
        ContractStatusInput::Active => ContractStatus::Active,
        ContractStatusInput::Deprecated => ContractStatus::Deprecated,
        ContractStatusInput::Retired => ContractStatus::Retired,
    }
}

fn convert_contract_stability(stability: ContractStabilityInput) -> ContractStability {
    match stability {
        ContractStabilityInput::Experimental => ContractStability::Experimental,
        ContractStabilityInput::Internal => ContractStability::Internal,
        ContractStabilityInput::Public => ContractStability::Public,
        ContractStabilityInput::Deprecated => ContractStability::Deprecated,
        ContractStabilityInput::Migrating => ContractStability::Migrating,
    }
}

fn convert_contract_guarantee_strength(
    strength: ContractGuaranteeStrengthInput,
) -> ContractGuaranteeStrength {
    match strength {
        ContractGuaranteeStrengthInput::Hard => ContractGuaranteeStrength::Hard,
        ContractGuaranteeStrengthInput::Soft => ContractGuaranteeStrength::Soft,
        ContractGuaranteeStrengthInput::Conditional => ContractGuaranteeStrength::Conditional,
    }
}

fn convert_contract_target(
    prism: &Prism,
    workspace: Option<&WorkspaceSession>,
    workspace_root: Option<&std::path::Path>,
    target: ContractTargetInput,
) -> Result<ContractTarget> {
    Ok(ContractTarget {
        anchors: convert_anchors(
            prism,
            workspace,
            workspace_root,
            target.anchors.unwrap_or_default(),
        )?,
        concept_handles: normalize_concept_handles(target.concept_handles.unwrap_or_default()),
    })
}

fn convert_contract_targets(
    prism: &Prism,
    workspace: Option<&WorkspaceSession>,
    workspace_root: Option<&std::path::Path>,
    targets: Option<Vec<ContractTargetInput>>,
) -> Result<Vec<ContractTarget>> {
    targets
        .unwrap_or_default()
        .into_iter()
        .map(|target| convert_contract_target(prism, workspace, workspace_root, target))
        .collect()
}

fn convert_contract_guarantees(
    guarantees: Vec<ContractGuaranteeInput>,
) -> Result<Vec<ContractGuarantee>> {
    let guarantees = guarantees
        .into_iter()
        .map(|guarantee| {
            let statement = guarantee.statement.trim().to_string();
            if statement.is_empty() {
                return Err(anyhow!("contract guarantees require non-empty statements"));
            }
            Ok(ContractGuarantee {
                id: guarantee
                    .id
                    .map(|value| value.trim().to_string())
                    .filter(|value| !value.is_empty())
                    .unwrap_or_default(),
                statement,
                scope: guarantee
                    .scope
                    .map(|value| value.trim().to_string())
                    .filter(|value| !value.is_empty()),
                strength: guarantee.strength.map(convert_contract_guarantee_strength),
                evidence_refs: sanitize_strings(guarantee.evidence_refs.unwrap_or_default()),
            })
        })
        .collect::<Result<Vec<_>>>()?;
    let guarantees = normalize_contract_guarantees(guarantees);
    if guarantees.is_empty() {
        return Err(anyhow!("contract guarantees cannot be empty"));
    }
    Ok(guarantees)
}

fn normalize_contract_guarantees(guarantees: Vec<ContractGuarantee>) -> Vec<ContractGuarantee> {
    let mut seen = std::collections::HashMap::<String, usize>::new();
    guarantees
        .into_iter()
        .map(|mut guarantee| {
            let base = normalize_contract_guarantee_id(if guarantee.id.trim().is_empty() {
                &guarantee.statement
            } else {
                &guarantee.id
            });
            let counter = seen.entry(base.clone()).or_insert(0);
            *counter += 1;
            guarantee.id = if *counter == 1 {
                base
            } else {
                format!("{base}_{}", *counter)
            };
            guarantee
        })
        .collect()
}

fn normalize_contract_guarantee_id(value: &str) -> String {
    let mut slug = String::new();
    let mut last_was_sep = false;
    for ch in value.chars() {
        if ch.is_ascii_alphanumeric() {
            slug.push(ch.to_ascii_lowercase());
            last_was_sep = false;
        } else if !last_was_sep && !slug.is_empty() {
            slug.push('_');
            last_was_sep = true;
        }
    }
    while slug.ends_with('_') {
        slug.pop();
    }
    if slug.is_empty() {
        "guarantee".to_string()
    } else {
        slug
    }
}

fn convert_contract_validations(
    prism: &Prism,
    workspace: Option<&WorkspaceSession>,
    workspace_root: Option<&std::path::Path>,
    validations: Option<Vec<ContractValidationInput>>,
) -> Result<Vec<ContractValidation>> {
    validations
        .unwrap_or_default()
        .into_iter()
        .map(|validation| {
            let id = validation.id.trim().to_string();
            if id.is_empty() {
                return Err(anyhow!("contract validations require non-empty ids"));
            }
            Ok(ContractValidation {
                id,
                summary: validation
                    .summary
                    .map(|value| value.trim().to_string())
                    .filter(|value| !value.is_empty()),
                anchors: convert_anchors(
                    prism,
                    workspace,
                    workspace_root,
                    validation.anchors.unwrap_or_default(),
                )?,
            })
        })
        .collect()
}

fn convert_contract_compatibility(
    compatibility: ContractCompatibilityInput,
) -> ContractCompatibility {
    ContractCompatibility {
        compatible: sanitize_strings(compatibility.compatible.unwrap_or_default()),
        additive: sanitize_strings(compatibility.additive.unwrap_or_default()),
        risky: sanitize_strings(compatibility.risky.unwrap_or_default()),
        breaking: sanitize_strings(compatibility.breaking.unwrap_or_default()),
        migrating: sanitize_strings(compatibility.migrating.unwrap_or_default()),
    }
}

fn merge_unique_strings(mut current: Vec<String>, additions: Vec<String>) -> Vec<String> {
    current.extend(additions);
    current = sanitize_strings(current);
    current.sort();
    current.dedup();
    current
}

fn merge_contract_targets(
    current: Vec<ContractTarget>,
    additions: Vec<ContractTarget>,
) -> Vec<ContractTarget> {
    let mut merged = current;
    for target in additions {
        if !merged.iter().any(|existing| existing == &target) {
            merged.push(target);
        }
    }
    merged
}

fn merge_contract_validations(
    current: Vec<ContractValidation>,
    additions: Vec<ContractValidation>,
) -> Vec<ContractValidation> {
    let mut merged = current;
    for validation in additions {
        if let Some(existing) = merged.iter_mut().find(|item| item.id == validation.id) {
            *existing = validation;
        } else {
            merged.push(validation);
        }
    }
    merged.sort_by(|left, right| left.id.cmp(&right.id));
    merged
}

fn build_updated_concept_packet(
    prism: &Prism,
    task_id: &TaskId,
    recorded_at: u64,
    args: PrismConceptMutationArgs,
) -> Result<ConceptPacket> {
    let handle = args
        .handle
        .as_deref()
        .map(|value| normalize_concept_handle(Some(value), value))
        .ok_or_else(|| anyhow!("concept update requires handle"))?;
    let mut packet = prism
        .concept_by_handle(&handle)
        .ok_or_else(|| anyhow!("no concept packet matched `{handle}`"))?;
    let mut changed = false;
    if let Some(scope) = args.scope.clone().map(convert_concept_scope) {
        packet.scope = scope;
        changed = true;
    }
    if let Some(canonical_name) = args
        .canonical_name
        .map(|value| value.trim().to_string())
        .filter(|value| !value.is_empty())
    {
        packet.canonical_name = canonical_name;
        changed = true;
    }
    if let Some(summary) = args
        .summary
        .map(|value| value.trim().to_string())
        .filter(|value| !value.is_empty())
    {
        packet.summary = summary;
        changed = true;
    }
    if let Some(aliases) = args.aliases {
        packet.aliases = sanitize_strings(aliases);
        changed = true;
    }
    if let Some(core_members) = args.core_members {
        packet.core_members = convert_concept_nodes(prism, core_members, "coreMembers")?;
        packet.core_member_lineages = concept_member_lineages(prism, &packet.core_members);
        changed = true;
    }
    if let Some(supporting_members) = args.supporting_members {
        packet.supporting_members =
            convert_concept_nodes(prism, supporting_members, "supportingMembers")?;
        packet.supporting_member_lineages =
            concept_member_lineages(prism, &packet.supporting_members);
        changed = true;
    }
    if let Some(likely_tests) = args.likely_tests {
        packet.likely_tests = convert_concept_nodes(prism, likely_tests, "likelyTests")?;
        packet.likely_test_lineages = concept_member_lineages(prism, &packet.likely_tests);
        changed = true;
    }
    if let Some(evidence) = args.evidence {
        packet.evidence = sanitize_strings(evidence);
        changed = true;
    }
    match parse_sparse_patch(args.risk_hint, "riskHint")? {
        SparsePatch::Keep => {}
        SparsePatch::Set(value) => {
            let risk_hint = value.trim().to_string();
            if risk_hint.is_empty() {
                return Err(anyhow!("concept riskHint cannot be empty"));
            }
            packet.risk_hint = Some(risk_hint);
            changed = true;
        }
        SparsePatch::Clear => {
            packet.risk_hint = None;
            changed = true;
        }
    }
    if let Some(confidence) = args.confidence {
        packet.confidence = confidence.clamp(0.0, 1.0);
        changed = true;
    }
    if let Some(decode_lenses) = args.decode_lenses {
        packet.decode_lenses = convert_concept_lenses(Some(decode_lenses));
        changed = true;
    }
    if let Some(supersedes) = args.supersedes {
        let publication = packet
            .publication
            .get_or_insert_with(|| ConceptPublication {
                published_at: recorded_at,
                ..ConceptPublication::default()
            });
        publication.supersedes = normalize_concept_handles(supersedes);
        changed = true;
    }
    if !changed {
        return Err(anyhow!(
            "concept update requires at least one field to change"
        ));
    }
    if packet.provenance == ConceptProvenance::default() {
        packet.provenance = ConceptProvenance {
            origin: match packet.scope {
                ConceptScope::Local => "local_mutation".to_string(),
                ConceptScope::Session => "session_mutation".to_string(),
                ConceptScope::Repo => "repo_mutation".to_string(),
            },
            kind: "manual_concept_update".to_string(),
            task_id: Some(task_id.0.to_string()),
        };
    }
    if packet.scope == ConceptScope::Repo {
        let publication = packet
            .publication
            .get_or_insert_with(|| ConceptPublication {
                published_at: recorded_at,
                ..ConceptPublication::default()
            });
        if publication.published_at == 0 {
            publication.published_at = recorded_at;
        }
        publication.last_reviewed_at = Some(recorded_at);
        publication.status = ConceptPublicationStatus::Active;
        publication.retired_at = None;
        publication.retirement_reason = None;
    } else {
        packet.publication = None;
    }
    validate_concept_packet(&packet)?;
    Ok(packet)
}

fn build_retired_concept_packet(
    prism: &Prism,
    task_id: &TaskId,
    recorded_at: u64,
    args: PrismConceptMutationArgs,
) -> Result<ConceptPacket> {
    let handle = args
        .handle
        .as_deref()
        .map(|value| normalize_concept_handle(Some(value), value))
        .ok_or_else(|| anyhow!("concept retire requires handle"))?;
    let retirement_reason = args
        .retirement_reason
        .map(|value| value.trim().to_string())
        .filter(|value| !value.is_empty())
        .ok_or_else(|| anyhow!("concept retire requires retirementReason"))?;
    let mut packet = prism
        .concept_by_handle(&handle)
        .ok_or_else(|| anyhow!("no concept packet matched `{handle}`"))?;
    if let Some(scope) = args.scope.clone().map(convert_concept_scope) {
        packet.scope = scope;
    }
    if packet.provenance == ConceptProvenance::default() {
        packet.provenance = ConceptProvenance {
            origin: match packet.scope {
                ConceptScope::Local => "local_mutation".to_string(),
                ConceptScope::Session => "session_mutation".to_string(),
                ConceptScope::Repo => "repo_mutation".to_string(),
            },
            kind: "manual_concept_retire".to_string(),
            task_id: Some(task_id.0.to_string()),
        };
    }
    let publication = packet
        .publication
        .get_or_insert_with(|| ConceptPublication {
            published_at: recorded_at,
            ..ConceptPublication::default()
        });
    if publication.published_at == 0 {
        publication.published_at = recorded_at;
    }
    if let Some(supersedes) = args.supersedes {
        publication.supersedes = normalize_concept_handles(supersedes);
    }
    publication.last_reviewed_at = Some(recorded_at);
    publication.status = ConceptPublicationStatus::Retired;
    publication.retired_at = Some(recorded_at);
    publication.retirement_reason = Some(retirement_reason);
    validate_concept_packet(&packet)?;
    Ok(packet)
}

fn convert_optional_concept_nodes(
    prism: &Prism,
    value: Option<Vec<crate::NodeIdInput>>,
    field: &str,
) -> Result<Vec<prism_ir::NodeId>> {
    value
        .map(|nodes| convert_concept_nodes(prism, nodes, field))
        .transpose()
        .map(Option::unwrap_or_default)
}

fn convert_concept_nodes(
    prism: &Prism,
    nodes: Vec<crate::NodeIdInput>,
    field: &str,
) -> Result<Vec<prism_ir::NodeId>> {
    let mut converted = Vec::new();
    for node in nodes {
        let node_id = convert_node_id(node)?;
        if prism.graph().node(&node_id).is_none() {
            return Err(anyhow!(
                "concept `{field}` references unknown node `{}`",
                node_id.path
            ));
        }
        if !converted.iter().any(|candidate| candidate == &node_id) {
            converted.push(node_id);
        }
    }
    Ok(converted)
}

fn convert_concept_lenses(
    value: Option<Vec<PrismConceptLensInput>>,
) -> Vec<prism_query::ConceptDecodeLens> {
    value
        .unwrap_or_else(|| {
            vec![
                PrismConceptLensInput::Open,
                PrismConceptLensInput::Workset,
                PrismConceptLensInput::Validation,
                PrismConceptLensInput::Timeline,
                PrismConceptLensInput::Memory,
            ]
        })
        .into_iter()
        .map(|lens| match lens {
            PrismConceptLensInput::Open => prism_query::ConceptDecodeLens::Open,
            PrismConceptLensInput::Workset => prism_query::ConceptDecodeLens::Workset,
            PrismConceptLensInput::Validation => prism_query::ConceptDecodeLens::Validation,
            PrismConceptLensInput::Timeline => prism_query::ConceptDecodeLens::Timeline,
            PrismConceptLensInput::Memory => prism_query::ConceptDecodeLens::Memory,
        })
        .collect()
}

fn sanitize_strings(values: Vec<String>) -> Vec<String> {
    let mut sanitized = Vec::new();
    for value in values {
        let value = value.trim();
        if value.is_empty() {
            continue;
        }
        if !sanitized
            .iter()
            .any(|candidate: &String| candidate == value)
        {
            sanitized.push(value.to_string());
        }
    }
    sanitized
}

fn normalize_concept_handles(values: Vec<String>) -> Vec<String> {
    sanitize_strings(values)
        .into_iter()
        .map(|value| canonical_concept_handle(value.trim_start_matches("concept://")))
        .collect()
}

fn normalize_concept_handle(handle: Option<&str>, canonical_name: &str) -> String {
    match handle.map(str::trim).filter(|value| !value.is_empty()) {
        Some(value) => canonical_concept_handle(value.trim_start_matches("concept://")),
        None => canonical_concept_handle(canonical_name),
    }
}

fn concept_member_lineages(
    prism: &Prism,
    members: &[prism_ir::NodeId],
) -> Vec<Option<prism_ir::LineageId>> {
    members
        .iter()
        .map(|member| prism.lineage_of(member))
        .collect()
}

fn validate_concept_packet(packet: &ConceptPacket) -> Result<()> {
    if packet.handle.trim().is_empty() {
        return Err(anyhow!("concept handle cannot be empty"));
    }
    if packet.canonical_name.trim().is_empty() {
        return Err(anyhow!("concept canonical name cannot be empty"));
    }
    if packet.summary.trim().is_empty() {
        return Err(anyhow!("concept summary cannot be empty"));
    }
    let min_core_members = if packet.scope == ConceptScope::Repo {
        2
    } else {
        1
    };
    if packet.core_members.len() < min_core_members {
        return Err(anyhow!(
            "concept coreMembers must contain at least {min_core_members} central member(s)"
        ));
    }
    if packet.core_members.len() > 5 {
        return Err(anyhow!(
            "concept coreMembers cannot contain more than 5 members"
        ));
    }
    if packet.evidence.is_empty() {
        return Err(anyhow!("concept evidence cannot be empty"));
    }
    let min_confidence = if packet.scope == ConceptScope::Repo {
        0.7
    } else {
        0.5
    };
    if packet.confidence < min_confidence {
        return Err(anyhow!(
            "concept confidence must be at least {min_confidence}"
        ));
    }
    if packet.decode_lenses.is_empty() {
        return Err(anyhow!("concept decodeLenses cannot be empty"));
    }
    if packet.scope == ConceptScope::Repo {
        let Some(publication) = packet.publication.as_ref() else {
            return Err(anyhow!(
                "repo-published concept packet must include publication metadata"
            ));
        };
        if publication.published_at == 0 {
            return Err(anyhow!(
                "repo-published concept publication metadata must include publishedAt"
            ));
        }
        if publication.status == ConceptPublicationStatus::Retired
            && publication
                .retirement_reason
                .as_deref()
                .unwrap_or("")
                .trim()
                .is_empty()
        {
            return Err(anyhow!(
                "retired concept publication metadata must include retirementReason"
            ));
        }
    } else if packet
        .publication
        .as_ref()
        .is_some_and(|publication| publication.status == ConceptPublicationStatus::Retired)
        && packet
            .publication
            .as_ref()
            .and_then(|publication| publication.retirement_reason.as_deref())
            .unwrap_or("")
            .trim()
            .is_empty()
    {
        return Err(anyhow!(
            "retired concept publication metadata must include retirementReason"
        ));
    }
    if packet.provenance == ConceptProvenance::default() {
        return Err(anyhow!("concept packet must include provenance metadata"));
    }
    Ok(())
}

fn validate_contract_packet(packet: &ContractPacket) -> Result<()> {
    if packet.handle.trim().is_empty() {
        return Err(anyhow!("contract handle cannot be empty"));
    }
    if packet.name.trim().is_empty() {
        return Err(anyhow!("contract name cannot be empty"));
    }
    if packet.summary.trim().is_empty() {
        return Err(anyhow!("contract summary cannot be empty"));
    }
    if packet.guarantees.is_empty() {
        return Err(anyhow!("contract guarantees cannot be empty"));
    }
    if packet
        .guarantees
        .iter()
        .any(|guarantee| guarantee.statement.trim().is_empty() || guarantee.id.trim().is_empty())
    {
        return Err(anyhow!(
            "contract guarantees must contain non-empty ids and statements"
        ));
    }
    let unique_guarantee_ids = packet
        .guarantees
        .iter()
        .map(|guarantee| guarantee.id.to_ascii_lowercase())
        .collect::<std::collections::HashSet<_>>();
    if unique_guarantee_ids.len() != packet.guarantees.len() {
        return Err(anyhow!(
            "contract guarantee ids must be unique within a packet"
        ));
    }
    if packet.subject.anchors.is_empty() && packet.subject.concept_handles.is_empty() {
        return Err(anyhow!(
            "contract subject must include at least one anchor or concept handle"
        ));
    }
    if packet.evidence.is_empty() {
        return Err(anyhow!("contract evidence cannot be empty"));
    }
    if packet.scope == ConceptScope::Repo {
        let Some(publication) = packet.publication.as_ref() else {
            return Err(anyhow!(
                "repo-published contract packet must include publication metadata"
            ));
        };
        if publication.published_at == 0 {
            return Err(anyhow!(
                "repo-published contract publication metadata must include publishedAt"
            ));
        }
    }
    if packet.status == ContractStatus::Retired
        && packet
            .publication
            .as_ref()
            .and_then(|publication| publication.retirement_reason.as_deref())
            .unwrap_or("")
            .trim()
            .is_empty()
    {
        return Err(anyhow!(
            "retired contract publication metadata must include retirementReason"
        ));
    }
    if packet.provenance == ConceptProvenance::default() {
        return Err(anyhow!("contract packet must include provenance metadata"));
    }
    Ok(())
}

fn convert_concept_scope(scope: crate::ConceptScopeInput) -> ConceptScope {
    match scope {
        crate::ConceptScopeInput::Local => ConceptScope::Local,
        crate::ConceptScopeInput::Session => ConceptScope::Session,
        crate::ConceptScopeInput::Repo => ConceptScope::Repo,
    }
}

fn convert_concept_relation_kind(kind: ConceptRelationKindInput) -> ConceptRelationKind {
    match kind {
        ConceptRelationKindInput::DependsOn => ConceptRelationKind::DependsOn,
        ConceptRelationKindInput::Specializes => ConceptRelationKind::Specializes,
        ConceptRelationKindInput::PartOf => ConceptRelationKind::PartOf,
        ConceptRelationKindInput::ValidatedBy => ConceptRelationKind::ValidatedBy,
        ConceptRelationKindInput::OftenUsedWith => ConceptRelationKind::OftenUsedWith,
        ConceptRelationKindInput::Supersedes => ConceptRelationKind::Supersedes,
        ConceptRelationKindInput::ConfusedWith => ConceptRelationKind::ConfusedWith,
    }
}

fn next_concept_event_id() -> String {
    new_prefixed_id("concept-event").to_string()
}

fn next_contract_event_id() -> String {
    new_prefixed_id("contract-event").to_string()
}

fn next_concept_relation_event_id() -> String {
    new_prefixed_id("concept-relation-event").to_string()
}

fn convert_validation_feedback_category(
    category: ValidationFeedbackCategoryInput,
) -> ValidationFeedbackCategory {
    match category {
        ValidationFeedbackCategoryInput::Structural => ValidationFeedbackCategory::Structural,
        ValidationFeedbackCategoryInput::Lineage => ValidationFeedbackCategory::Lineage,
        ValidationFeedbackCategoryInput::Memory => ValidationFeedbackCategory::Memory,
        ValidationFeedbackCategoryInput::Projection => ValidationFeedbackCategory::Projection,
        ValidationFeedbackCategoryInput::Coordination => ValidationFeedbackCategory::Coordination,
        ValidationFeedbackCategoryInput::Freshness => ValidationFeedbackCategory::Freshness,
        ValidationFeedbackCategoryInput::Other => ValidationFeedbackCategory::Other,
    }
}

fn convert_validation_feedback_verdict(
    verdict: ValidationFeedbackVerdictInput,
) -> ValidationFeedbackVerdict {
    match verdict {
        ValidationFeedbackVerdictInput::Wrong => ValidationFeedbackVerdict::Wrong,
        ValidationFeedbackVerdictInput::Stale => ValidationFeedbackVerdict::Stale,
        ValidationFeedbackVerdictInput::Noisy => ValidationFeedbackVerdict::Noisy,
        ValidationFeedbackVerdictInput::Helpful => ValidationFeedbackVerdict::Helpful,
        ValidationFeedbackVerdictInput::Mixed => ValidationFeedbackVerdict::Mixed,
    }
}
