use prism_ir::{
    ArtifactStatus, Capability, ClaimMode, ConflictSeverity, CoordinationTaskStatus, SessionId,
    Timestamp, WorkspaceRevision,
};

use crate::helpers::{claim_is_active, dedupe_conflicts, dedupe_strings, simulate_conflicts};
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
            });
        }
    }

    if let Some(context) = context {
        if let Some(threshold) = plan.policy.review_required_above_risk_score {
            if context.risk_score.unwrap_or_default() >= threshold && approved_artifacts.is_empty() {
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
    for dep in &task.depends_on {
        match state.tasks.get(dep) {
            Some(dependency) if dependency.status == CoordinationTaskStatus::Completed => {}
            Some(dependency) => blockers.push(TaskBlocker {
                kind: BlockerKind::Dependency,
                summary: format!("dependency `{}` is {:?}", dependency.id.0, dependency.status),
                related_task_id: Some(dependency.id.clone()),
                related_artifact_id: None,
                risk_score: None,
                validation_checks: Vec::new(),
            }),
            None => blockers.push(TaskBlocker {
                kind: BlockerKind::Dependency,
                summary: format!("dependency `{}` is missing", dep.0),
                related_task_id: Some(dep.clone()),
                related_artifact_id: None,
                risk_score: None,
                validation_checks: Vec::new(),
            }),
        }
    }

    if let Some(plan) = state.plans.get(&task.plan) {
        if plan.policy.stale_after_graph_change
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
            });
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
            .filter(|claim| claim_is_active(claim, now)),
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
