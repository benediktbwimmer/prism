use prism_ir::{
    ArtifactStatus, BlockerCause, BlockerCauseSource, Capability, ClaimMode, ConflictSeverity,
    CoordinationTaskStatus, GitExecutionStatus, GitIntegrationStatus, SessionId, Timestamp,
    WorkspaceRevision,
};

use crate::helpers::{dedupe_conflicts, dedupe_strings, simulate_conflicts};
use crate::lease::claim_blocks_new_work_with_runtime_descriptors;
use crate::state::CoordinationState;
use crate::state::CoordinationStore;
use crate::types::{BlockerKind, CoordinationTask, TaskBlocker, TaskCompletionContext};

pub(crate) fn readiness_blockers(
    state: &CoordinationState,
    task: &CoordinationTask,
    current_revision: WorkspaceRevision,
    now: Timestamp,
) -> Vec<TaskBlocker> {
    let mut blockers = dependency_and_revision_blockers(state, task, current_revision);
    blockers.extend(claim_blockers(state, task, now));
    blockers
}

pub(crate) fn completion_blockers(
    state: &CoordinationState,
    task: &CoordinationTask,
    current_revision: WorkspaceRevision,
    now: Timestamp,
) -> Vec<TaskBlocker> {
    let mut blockers = dependency_and_revision_blockers(state, task, current_revision);
    blockers.extend(claim_blockers(state, task, now));
    blockers.extend(review_blockers(state, task));
    blockers
}

pub(crate) fn completion_policy_blockers(
    state: &CoordinationState,
    task: &CoordinationTask,
    current_revision: WorkspaceRevision,
    context: Option<&TaskCompletionContext>,
) -> Vec<TaskBlocker> {
    let Some(plan) = state.plans.get(&task.plan) else {
        return Vec::new();
    };
    let approved_artifacts = state
        .artifacts
        .values()
        .filter(|artifact| artifact.task == task.id)
        .filter(|artifact| {
            matches!(
                artifact.status,
                ArtifactStatus::Approved | ArtifactStatus::Merged
            )
        })
        .collect::<Vec<_>>();
    let mut blockers = Vec::new();

    if plan.policy.stale_after_graph_change {
        let stale = approved_artifacts
            .iter()
            .find(|artifact| artifact.base_revision.graph_version < current_revision.graph_version)
            .map(|artifact| artifact.id.clone());
        if let Some(artifact_id) = stale {
            blockers.push(TaskBlocker {
                kind: BlockerKind::ArtifactStale,
                summary: format!(
                    "approved artifact `{}` is stale against graph version {}",
                    artifact_id.0, current_revision.graph_version
                ),
                related_task_id: Some(task.id.clone()),
                related_artifact_id: Some(artifact_id),
                risk_score: context.and_then(|ctx| ctx.risk_score),
                validation_checks: Vec::new(),
                causes: vec![
                    BlockerCause {
                        source: BlockerCauseSource::PlanPolicy,
                        code: Some("stale_after_graph_change".to_string()),
                        acceptance_label: None,
                        threshold_metric: None,
                        threshold_value: None,
                        observed_value: None,
                    },
                    BlockerCause {
                        source: BlockerCauseSource::ArtifactState,
                        code: Some("approved_artifact_stale".to_string()),
                        acceptance_label: None,
                        threshold_metric: None,
                        threshold_value: None,
                        observed_value: None,
                    },
                ],
            });
        }
    }

    if let Some(context) = context {
        if let Some(threshold) = plan.policy.review_required_above_risk_score {
            if context.risk_score.unwrap_or_default() >= threshold && approved_artifacts.is_empty()
            {
                blockers.push(TaskBlocker {
                    kind: BlockerKind::RiskReviewRequired,
                    summary: format!(
                        "task risk score {:.2} requires review before completion",
                        context.risk_score.unwrap_or_default()
                    ),
                    related_task_id: Some(task.id.clone()),
                    related_artifact_id: None,
                    risk_score: context.risk_score,
                    validation_checks: Vec::new(),
                    causes: vec![BlockerCause {
                        source: BlockerCauseSource::DerivedThreshold,
                        code: Some("review_required_above_risk_score".to_string()),
                        acceptance_label: None,
                        threshold_metric: Some("risk_score".to_string()),
                        threshold_value: Some(threshold),
                        observed_value: context.risk_score,
                    }],
                });
            }
        }

        if plan.policy.require_validation_for_completion && !context.required_validations.is_empty()
        {
            let validated = approved_artifacts
                .iter()
                .flat_map(|artifact| artifact.validated_checks.iter().cloned())
                .collect::<Vec<_>>();
            let validated = dedupe_strings(validated);
            let missing = context
                .required_validations
                .iter()
                .filter(|check| !validated.iter().any(|value| value == *check))
                .cloned()
                .collect::<Vec<_>>();
            if !missing.is_empty() {
                blockers.push(TaskBlocker {
                    kind: BlockerKind::ValidationRequired,
                    summary: format!(
                        "task is missing required validations: {}",
                        missing.join(", ")
                    ),
                    related_task_id: Some(task.id.clone()),
                    related_artifact_id: approved_artifacts
                        .first()
                        .map(|artifact| artifact.id.clone()),
                    risk_score: context.risk_score,
                    validation_checks: missing,
                    causes: vec![BlockerCause {
                        source: BlockerCauseSource::PlanPolicy,
                        code: Some("require_validation_for_completion".to_string()),
                        acceptance_label: None,
                        threshold_metric: None,
                        threshold_value: None,
                        observed_value: None,
                    }],
                });
            }
        }
    }

    blockers
}

pub(crate) fn dependency_and_revision_blockers(
    state: &CoordinationState,
    task: &CoordinationTask,
    current_revision: WorkspaceRevision,
) -> Vec<TaskBlocker> {
    let mut blockers = Vec::new();
    blockers.extend(lifecycle_dependency_blockers(
        state,
        &task.depends_on,
        "completed",
        "task_dependency_incomplete",
        |dependency| dependency.status == CoordinationTaskStatus::Completed,
        |dependency| {
            format!(
                "dependency `{}` is {:?}",
                dependency.id.0, dependency.status
            )
        },
    ));
    blockers.extend(lifecycle_dependency_blockers(
        state,
        &task.coordination_depends_on,
        "coordination_published",
        "task_dependency_coordination_unpublished",
        |dependency| {
            dependency.status == CoordinationTaskStatus::Completed
                && dependency.git_execution.status == GitExecutionStatus::CoordinationPublished
        },
        |dependency| {
            format!(
                "dependency `{}` is not coordination-published (task={:?}, git={:?})",
                dependency.id.0, dependency.status, dependency.git_execution.status
            )
        },
    ));
    blockers.extend(lifecycle_dependency_blockers(
        state,
        &task.integrated_depends_on,
        "integrated_to_target",
        "task_dependency_not_integrated",
        |dependency| {
            dependency.git_execution.integration_status == GitIntegrationStatus::IntegratedToTarget
        },
        |dependency| {
            format!(
                "dependency `{}` is not integrated to target (integration={:?})",
                dependency.id.0, dependency.git_execution.integration_status
            )
        },
    ));

    if let Some(plan) = state.plans.get(&task.plan) {
        if plan.policy.stale_after_graph_change
            && task_is_workspace_bound(task)
            && task.base_revision.graph_version < current_revision.graph_version
        {
            blockers.push(TaskBlocker {
                kind: BlockerKind::StaleRevision,
                summary: format!(
                    "task is based on graph version {} but current revision is {}",
                    task.base_revision.graph_version, current_revision.graph_version
                ),
                related_task_id: Some(task.id.clone()),
                related_artifact_id: None,
                risk_score: None,
                validation_checks: Vec::new(),
                causes: vec![
                    BlockerCause {
                        source: BlockerCauseSource::PlanPolicy,
                        code: Some("stale_after_graph_change".to_string()),
                        acceptance_label: None,
                        threshold_metric: None,
                        threshold_value: None,
                        observed_value: None,
                    },
                    BlockerCause {
                        source: BlockerCauseSource::RuntimeState,
                        code: Some("workspace_revision_mismatch".to_string()),
                        acceptance_label: None,
                        threshold_metric: None,
                        threshold_value: None,
                        observed_value: None,
                    },
                ],
            });
        }
    }
    blockers
}

fn task_is_workspace_bound(task: &CoordinationTask) -> bool {
    !task.anchors.is_empty()
        || task
            .acceptance
            .iter()
            .any(|criterion| !criterion.anchors.is_empty())
}

fn lifecycle_dependency_blockers<F, G>(
    state: &CoordinationState,
    dependencies: &[prism_ir::CoordinationTaskId],
    required_stage: &str,
    incomplete_code: &str,
    is_satisfied: F,
    summary: G,
) -> Vec<TaskBlocker>
where
    F: Fn(&CoordinationTask) -> bool,
    G: Fn(&CoordinationTask) -> String,
{
    let mut blockers = Vec::new();
    for dep in dependencies {
        match state.tasks.get(dep) {
            Some(dependency) if is_satisfied(dependency) => {}
            Some(dependency) => blockers.push(TaskBlocker {
                kind: BlockerKind::Dependency,
                summary: summary(dependency),
                related_task_id: Some(dependency.id.clone()),
                related_artifact_id: None,
                risk_score: None,
                validation_checks: Vec::new(),
                causes: vec![BlockerCause {
                    source: BlockerCauseSource::DependencyGraph,
                    code: Some(incomplete_code.to_string()),
                    acceptance_label: Some(required_stage.to_string()),
                    threshold_metric: Some("dependency_delivery_stage".to_string()),
                    threshold_value: None,
                    observed_value: None,
                }],
            }),
            None => blockers.push(TaskBlocker {
                kind: BlockerKind::Dependency,
                summary: format!("dependency `{}` is missing", dep.0),
                related_task_id: Some(dep.clone()),
                related_artifact_id: None,
                risk_score: None,
                validation_checks: Vec::new(),
                causes: vec![BlockerCause {
                    source: BlockerCauseSource::DependencyGraph,
                    code: Some("task_dependency_missing".to_string()),
                    acceptance_label: Some(required_stage.to_string()),
                    threshold_metric: Some("dependency_delivery_stage".to_string()),
                    threshold_value: None,
                    observed_value: None,
                }],
            }),
        }
    }
    blockers
}

pub(crate) fn review_blockers(
    state: &CoordinationState,
    task: &CoordinationTask,
) -> Vec<TaskBlocker> {
    let Some(plan) = state.plans.get(&task.plan) else {
        return Vec::new();
    };
    if !plan.policy.require_review_for_completion {
        return Vec::new();
    }
    let has_approved = state.artifacts.values().any(|artifact| {
        artifact.task == task.id
            && matches!(
                artifact.status,
                ArtifactStatus::Approved | ArtifactStatus::Merged
            )
    });
    if has_approved {
        return Vec::new();
    }
    let pending_artifact = state
        .artifacts
        .values()
        .find(|artifact| artifact.task == task.id)
        .map(|artifact| artifact.id.clone());
    vec![TaskBlocker {
        kind: BlockerKind::ReviewRequired,
        summary: "task requires an approved artifact review".to_string(),
        related_task_id: Some(task.id.clone()),
        related_artifact_id: pending_artifact,
        risk_score: None,
        validation_checks: Vec::new(),
        causes: vec![
            BlockerCause {
                source: BlockerCauseSource::PlanPolicy,
                code: Some("require_review_for_completion".to_string()),
                acceptance_label: None,
                threshold_metric: None,
                threshold_value: None,
                observed_value: None,
            },
            BlockerCause {
                source: BlockerCauseSource::ArtifactState,
                code: Some("missing_approved_artifact".to_string()),
                acceptance_label: None,
                threshold_metric: None,
                threshold_value: None,
                observed_value: None,
            },
        ],
    }]
}

pub(crate) fn claim_blockers(
    state: &CoordinationState,
    task: &CoordinationTask,
    now: Timestamp,
) -> Vec<TaskBlocker> {
    let claim_conflicts = dedupe_conflicts(simulate_conflicts(
        state
            .claims
            .values()
            .filter(|claim| {
                claim_blocks_new_work_with_runtime_descriptors(
                    claim,
                    &state.runtime_descriptors,
                    now,
                )
            }),
        &task.anchors,
        Capability::Edit,
        state
            .plans
            .get(&task.plan)
            .map(|plan| plan.policy.default_claim_mode)
            .unwrap_or(ClaimMode::SoftExclusive),
        state.plans.get(&task.plan).map(|plan| &plan.policy),
        Some(&task.id),
        task.base_revision.clone(),
        task.session
            .as_ref()
            .unwrap_or(&SessionId::new("session:none")),
    ));
    let mut blockers = Vec::new();
    for conflict in claim_conflicts {
        if conflict.severity != ConflictSeverity::Block {
            continue;
        }
        blockers.push(TaskBlocker {
            kind: BlockerKind::ClaimConflict,
            summary: conflict.summary,
            related_task_id: Some(task.id.clone()),
            related_artifact_id: None,
            risk_score: None,
            validation_checks: Vec::new(),
            causes: vec![BlockerCause {
                source: BlockerCauseSource::RuntimeState,
                code: Some("claim_conflict".to_string()),
                acceptance_label: None,
                threshold_metric: None,
                threshold_value: None,
                observed_value: None,
            }],
        });
    }
    blockers
}

impl CoordinationStore {
    pub(crate) fn readiness_blockers_locked(
        &self,
        state: &CoordinationState,
        task: &CoordinationTask,
        current_revision: WorkspaceRevision,
        now: Timestamp,
    ) -> Vec<TaskBlocker> {
        readiness_blockers(state, task, current_revision, now)
    }

    pub(crate) fn completion_blockers_locked(
        &self,
        state: &CoordinationState,
        task: &CoordinationTask,
        current_revision: WorkspaceRevision,
        now: Timestamp,
    ) -> Vec<TaskBlocker> {
        completion_blockers(state, task, current_revision, now)
    }
}
