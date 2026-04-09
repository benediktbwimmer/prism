use prism_coordination::{
    Artifact, ArtifactReview, CanonicalTaskRecord, CoordinationDerivations, TaskBlocker,
    TaskExecutorCaller,
};
use prism_ir::{
    BlockerCause, BlockerCauseSource, CoordinationTaskId, DerivedPlanStatus, NodeRef, NodeRefKind,
    PlanId, PlanStatus, PrincipalId, ReviewVerdict, TaskId, Timestamp,
};

use crate::common::current_timestamp;
use crate::{
    CoordinationPlanV2, CoordinationTaskV2, PlanSummary, Prism, TaskEvidenceArtifactStatus,
    TaskEvidenceStatus, TaskReviewStatus,
};

pub(crate) struct CoordinationQueryEngine<'a> {
    prism: &'a Prism,
}

impl<'a> CoordinationQueryEngine<'a> {
    pub(crate) fn new(prism: &'a Prism) -> Self {
        Self { prism }
    }

    pub(crate) fn coordination_plan_v2(&self, plan_id: &PlanId) -> Option<CoordinationPlanV2> {
        let snapshot = self.prism.coordination_snapshot_v2();
        let derivations = snapshot.derive_statuses().ok()?;
        let graph = snapshot.graph().ok()?;
        let plan = snapshot
            .plans
            .iter()
            .find(|plan| plan.id == *plan_id)
            .cloned()?;
        let derived = derivations.plan_state(plan_id)?;
        Some(CoordinationPlanV2 {
            plan: plan.clone(),
            status: derived.derived_status,
            children: graph.children_of_plan(plan_id),
            dependencies: graph.dependency_targets(&NodeRef::plan(plan_id.clone())),
            dependents: graph.dependency_sources(&NodeRef::plan(plan_id.clone())),
            estimated_minutes_total: derived.estimated_minutes_total,
            remaining_estimated_minutes: derived.remaining_estimated_minutes,
        })
    }

    pub(crate) fn coordination_task_v2(&self, task_id: &TaskId) -> Option<CoordinationTaskV2> {
        let snapshot = self.prism.coordination_snapshot_v2();
        let derivations = snapshot.derive_statuses().ok()?;
        let graph = snapshot.graph().ok()?;
        let task = snapshot
            .tasks
            .iter()
            .find(|task| task.id == *task_id)
            .cloned()?;
        let derived = derivations.task_state(task_id)?;
        Some(CoordinationTaskV2 {
            task: task.clone(),
            status: derived.effective_status,
            graph_actionable: derived.graph_actionable,
            blocker_causes: derived.blocker_causes.clone(),
            dependencies: graph.dependency_targets(&NodeRef::task(task_id.clone())),
            dependents: graph.dependency_sources(&NodeRef::task(task_id.clone())),
        })
    }

    pub(crate) fn coordination_tasks_v2(&self) -> Vec<CoordinationTaskV2> {
        let snapshot = self.prism.coordination_snapshot_v2();
        let Some(derivations) = snapshot.derive_statuses().ok() else {
            return Vec::new();
        };
        let Ok(graph) = snapshot.graph() else {
            return Vec::new();
        };
        let mut tasks = snapshot
            .tasks
            .iter()
            .filter_map(|task| {
                let derived = derivations.task_state(&task.id)?;
                Some(CoordinationTaskV2 {
                    task: task.clone(),
                    status: derived.effective_status,
                    graph_actionable: derived.graph_actionable,
                    blocker_causes: derived.blocker_causes.clone(),
                    dependencies: graph.dependency_targets(&NodeRef::task(task.id.clone())),
                    dependents: graph.dependency_sources(&NodeRef::task(task.id.clone())),
                })
            })
            .collect::<Vec<_>>();
        tasks.sort_by(|left, right| left.task.id.0.cmp(&right.task.id.0));
        tasks
    }

    pub(crate) fn graph_actionable_tasks_v2(&self) -> Vec<CoordinationTaskV2> {
        let snapshot = self.prism.coordination_snapshot_v2();
        actionable_task_views_from_snapshot(&snapshot, snapshot.derive_statuses().ok(), None)
    }

    pub(crate) fn actionable_tasks_for_executor_v2(
        &self,
        caller: &TaskExecutorCaller,
    ) -> Vec<CoordinationTaskV2> {
        let snapshot = self.prism.coordination_snapshot_v2();
        actionable_task_views_from_snapshot(
            &snapshot,
            snapshot.derive_statuses().ok(),
            Some(caller),
        )
    }

    pub(crate) fn ready_tasks_v2(&self, plan_id: &PlanId) -> Vec<CoordinationTaskV2> {
        let snapshot = self.prism.coordination_snapshot_v2();
        ready_task_views_for_plan_from_snapshot(
            &snapshot,
            snapshot.derive_statuses().ok(),
            plan_id,
            None,
        )
    }

    pub(crate) fn ready_tasks_for_executor_v2(
        &self,
        plan_id: &PlanId,
        caller: &TaskExecutorCaller,
    ) -> Vec<CoordinationTaskV2> {
        let snapshot = self.prism.coordination_snapshot_v2();
        ready_task_views_for_plan_from_snapshot(
            &snapshot,
            snapshot.derive_statuses().ok(),
            plan_id,
            Some(caller),
        )
    }

    pub(crate) fn root_plans_v2(&self) -> Vec<CoordinationPlanV2> {
        let snapshot = self.prism.coordination_snapshot_v2();
        let Some(derivations) = snapshot.derive_statuses().ok() else {
            return Vec::new();
        };
        let Ok(graph) = snapshot.graph() else {
            return Vec::new();
        };
        let mut plans = snapshot
            .plans
            .iter()
            .filter(|plan| plan.parent_plan_id.is_none())
            .filter_map(|plan| {
                let derived = derivations.plan_state(&plan.id)?;
                Some(CoordinationPlanV2 {
                    plan: plan.clone(),
                    status: derived.derived_status,
                    children: graph.children_of_plan(&plan.id),
                    dependencies: graph.dependency_targets(&NodeRef::plan(plan.id.clone())),
                    dependents: graph.dependency_sources(&NodeRef::plan(plan.id.clone())),
                    estimated_minutes_total: derived.estimated_minutes_total,
                    remaining_estimated_minutes: derived.remaining_estimated_minutes,
                })
            })
            .collect::<Vec<_>>();
        plans.sort_by(|left, right| left.plan.id.0.cmp(&right.plan.id.0));
        plans
    }

    pub(crate) fn blockers(
        &self,
        task_id: &CoordinationTaskId,
        now: Timestamp,
    ) -> Vec<TaskBlocker> {
        let mut blockers = self.base_blockers(task_id, now);

        if let Some(risk) = self.prism.task_risk(task_id, now) {
            if !risk.stale_artifact_ids.is_empty() {
                blockers.push(TaskBlocker {
                    kind: prism_coordination::BlockerKind::ArtifactStale,
                    summary: format!(
                        "approved artifact is stale against graph version {}",
                        self.prism.workspace_revision().graph_version
                    ),
                    related_task_id: Some(task_id.clone()),
                    related_artifact_id: risk.stale_artifact_ids.first().cloned(),
                    risk_score: Some(risk.risk_score),
                    validation_checks: Vec::new(),
                    causes: vec![BlockerCause {
                        source: BlockerCauseSource::ArtifactState,
                        code: Some("approved_artifact_stale".to_string()),
                        acceptance_label: None,
                        threshold_metric: None,
                        threshold_value: None,
                        observed_value: None,
                    }],
                });
            }
            if risk.review_required && !risk.has_approved_artifact {
                let threshold = self
                    .prism
                    .coordination_task_v2_by_coordination_id(task_id)
                    .and_then(|task| self.prism.coordination_plan_v2(&task.task.parent_plan_id))
                    .and_then(|plan| plan.plan.policy.review_required_above_risk_score);
                blockers.push(TaskBlocker {
                    kind: prism_coordination::BlockerKind::RiskReviewRequired,
                    summary: format!(
                        "task risk score {:.2} requires review before completion",
                        risk.risk_score
                    ),
                    related_task_id: Some(task_id.clone()),
                    related_artifact_id: None,
                    risk_score: Some(risk.risk_score),
                    validation_checks: Vec::new(),
                    causes: vec![
                        BlockerCause {
                            source: BlockerCauseSource::DerivedThreshold,
                            code: Some("review_required_above_risk_score".to_string()),
                            acceptance_label: None,
                            threshold_metric: Some("risk_score".to_string()),
                            threshold_value: threshold,
                            observed_value: Some(risk.risk_score),
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
                });
            }
            if !risk.missing_validations.is_empty() {
                blockers.push(TaskBlocker {
                    kind: prism_coordination::BlockerKind::ValidationRequired,
                    summary: format!(
                        "task is missing required validations: {}",
                        risk.missing_validations.join(", ")
                    ),
                    related_task_id: Some(task_id.clone()),
                    related_artifact_id: risk.approved_artifact_ids.first().cloned(),
                    risk_score: Some(risk.risk_score),
                    validation_checks: risk.missing_validations.clone(),
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

        dedupe_blockers(blockers)
    }

    pub(crate) fn base_blockers(
        &self,
        task_id: &CoordinationTaskId,
        now: Timestamp,
    ) -> Vec<TaskBlocker> {
        self.prism.with_coordination_runtime(|runtime| {
            runtime.blockers(task_id, self.prism.workspace_revision(), now)
        })
    }

    pub(crate) fn plan_summary(&self, plan_id: &PlanId) -> Option<PlanSummary> {
        let snapshot = self.prism.coordination_snapshot_v2();
        let derivations = snapshot.derive_statuses().ok()?;
        let graph = snapshot.graph().ok()?;
        let derived_plan = derivations.plan_state(plan_id)?;
        let task_records = snapshot
            .tasks
            .iter()
            .map(|task| (task.id.0.to_string(), task))
            .collect::<std::collections::BTreeMap<_, _>>();
        let descendant_plan_count = canonical_descendant_plan_ids(&graph, plan_id).len();
        let descendant_task_ids = canonical_descendant_task_ids(&graph, plan_id);
        let now = current_timestamp();

        let mut summary = PlanSummary {
            plan_id: plan_id.clone(),
            status: compatibility_plan_status(derived_plan.derived_status),
            total_nodes: descendant_plan_count + descendant_task_ids.len(),
            completed_nodes: 0,
            abandoned_nodes: 0,
            in_progress_nodes: 0,
            actionable_nodes: 0,
            execution_blocked_nodes: 0,
            completion_gated_nodes: 0,
            review_gated_nodes: 0,
            validation_gated_nodes: 0,
            stale_nodes: 0,
            claim_conflicted_nodes: 0,
        };

        for task_id in descendant_task_ids {
            let task = task_records.get(task_id.0.as_str())?;
            let task_state = derivations.task_state(&task.id)?;
            match task_state.effective_status {
                prism_ir::EffectiveTaskStatus::Completed => {
                    summary.completed_nodes += 1;
                    continue;
                }
                prism_ir::EffectiveTaskStatus::Abandoned => {
                    summary.abandoned_nodes += 1;
                    continue;
                }
                prism_ir::EffectiveTaskStatus::Active => summary.in_progress_nodes += 1,
                _ => {}
            }

            let blockers = self.blockers(&CoordinationTaskId::new(task.id.0.clone()), now);
            let actionable = task_state.graph_actionable && blockers.is_empty();
            if actionable {
                summary.actionable_nodes += 1;
            } else {
                summary.execution_blocked_nodes += 1;
            }
            if blockers
                .iter()
                .any(|blocker| is_task_completion_gate(blocker.kind))
            {
                summary.completion_gated_nodes += 1;
            }
            if blockers
                .iter()
                .any(|blocker| is_task_review_gate(blocker.kind))
            {
                summary.review_gated_nodes += 1;
            }
            if blockers
                .iter()
                .any(|blocker| is_task_validation_gate(blocker.kind))
            {
                summary.validation_gated_nodes += 1;
            }
            if blockers
                .iter()
                .any(|blocker| is_task_stale_gate(blocker.kind))
            {
                summary.stale_nodes += 1;
            }
            if blockers
                .iter()
                .any(|blocker| blocker.kind == prism_coordination::BlockerKind::ClaimConflict)
            {
                summary.claim_conflicted_nodes += 1;
            }
        }

        Some(summary)
    }

    pub(crate) fn pending_reviews(&self, plan_id: Option<&PlanId>) -> Vec<Artifact> {
        let worktree_id = coordination_worktree_scope(self.prism);
        self.prism.with_coordination_runtime(|runtime| {
            runtime.pending_reviews_in_scope(plan_id, worktree_id.as_deref())
        })
    }

    pub(crate) fn artifacts(&self, task_id: &CoordinationTaskId) -> Vec<Artifact> {
        let worktree_id = coordination_worktree_scope(self.prism);
        self.prism.with_coordination_runtime(|runtime| {
            runtime.artifacts_in_scope(task_id, worktree_id.as_deref())
        })
    }

    pub(crate) fn task_evidence_status(
        &self,
        task_id: &CoordinationTaskId,
        now: Timestamp,
    ) -> Option<TaskEvidenceStatus> {
        let artifacts = self.artifacts(task_id);
        let artifact_statuses = self
            .artifacts(task_id)
            .into_iter()
            .map(|artifact| self.task_evidence_artifact_status(artifact))
            .collect::<Vec<_>>();
        let task_risk = self.prism.task_risk(task_id, now)?;
        let blockers = self.blockers(task_id, now);
        let missing_validations = dedupe_strings(
            task_risk
                .missing_validations
                .iter()
                .cloned()
                .chain(artifact_statuses.iter().flat_map(|artifact| {
                    artifact
                        .artifact
                        .required_validations
                        .iter()
                        .filter(|check| {
                            !artifact
                                .artifact
                                .validated_checks
                                .iter()
                                .any(|value| value == *check)
                        })
                        .cloned()
                }))
                .collect(),
        );
        let pending_review_count = artifact_statuses
            .iter()
            .filter(|artifact| artifact.pending_review)
            .count();
        let review_required = task_risk.review_required
            || blockers
                .iter()
                .any(|blocker| is_task_review_gate(blocker.kind));
        let approved_artifact_count = artifact_statuses
            .iter()
            .filter(|artifact| {
                artifact.latest_review_verdict == Some(ReviewVerdict::Approved)
                    || artifact.artifact.status == prism_ir::ArtifactStatus::Approved
            })
            .count();
        let rejected_artifact_count = artifact_statuses
            .iter()
            .filter(|artifact| {
                matches!(
                    artifact.latest_review_verdict,
                    Some(ReviewVerdict::Rejected | ReviewVerdict::ChangesRequested)
                ) || artifact.artifact.status == prism_ir::ArtifactStatus::Rejected
            })
            .count();

        Some(TaskEvidenceStatus {
            task_id: task_id.clone(),
            artifacts: if artifacts.is_empty() {
                Vec::new()
            } else {
                artifact_statuses
            },
            blockers,
            pending_review_count,
            approved_artifact_count,
            rejected_artifact_count,
            missing_validations,
            stale_artifact_ids: task_risk.stale_artifact_ids,
            review_required,
            has_approved_artifact: task_risk.has_approved_artifact,
        })
    }

    pub(crate) fn task_review_status(
        &self,
        task_id: &CoordinationTaskId,
        now: Timestamp,
    ) -> Option<TaskReviewStatus> {
        let evidence = self.task_evidence_status(task_id, now)?;
        Some(TaskReviewStatus {
            task_id: evidence.task_id,
            artifacts: evidence.artifacts,
            pending_review_count: evidence.pending_review_count,
            approved_artifact_count: evidence.approved_artifact_count,
            rejected_artifact_count: evidence.rejected_artifact_count,
        })
    }

    fn task_evidence_artifact_status(&self, artifact: Artifact) -> TaskEvidenceArtifactStatus {
        let reviews = self.reviews_for_artifact(&artifact);
        let latest_review = latest_review(&reviews);
        let latest_review_verdict = latest_review.as_ref().map(|review| review.verdict);
        let pending_review = matches!(
            artifact.status,
            prism_ir::ArtifactStatus::Proposed | prism_ir::ArtifactStatus::InReview
        ) && !matches!(
            latest_review_verdict,
            Some(ReviewVerdict::Approved | ReviewVerdict::Rejected)
        );

        TaskEvidenceArtifactStatus {
            artifact,
            reviews,
            latest_review,
            latest_review_verdict,
            pending_review,
        }
    }

    fn reviews_for_artifact(&self, artifact: &Artifact) -> Vec<ArtifactReview> {
        let review_ids = artifact
            .reviews
            .iter()
            .map(|review_id| review_id.0.to_string())
            .collect::<std::collections::BTreeSet<_>>();
        let snapshot = self.prism.coordination_snapshot_v2();
        let mut reviews = snapshot
            .reviews
            .into_iter()
            .filter(|review| {
                review.artifact == artifact.id || review_ids.contains(review.id.0.as_str())
            })
            .collect::<Vec<_>>();
        reviews.sort_by(|left, right| {
            left.meta
                .ts
                .cmp(&right.meta.ts)
                .then_with(|| left.id.0.cmp(&right.id.0))
        });
        reviews
    }
}

fn actionable_task_views_from_snapshot(
    snapshot: &prism_coordination::CoordinationSnapshotV2,
    derivations: Option<CoordinationDerivations>,
    caller: Option<&TaskExecutorCaller>,
) -> Vec<CoordinationTaskV2> {
    let Some(derivations) = derivations else {
        return Vec::new();
    };
    let Ok(graph) = snapshot.graph() else {
        return Vec::new();
    };
    let actionable_ids = derivations
        .graph_actionable_tasks()
        .iter()
        .map(|task_id| task_id.0.to_string())
        .collect::<std::collections::BTreeSet<_>>();
    let mut tasks = snapshot
        .tasks
        .iter()
        .filter(|task| actionable_ids.contains(task.id.0.as_str()))
        .filter(|task| {
            caller
                .map(|caller| canonical_task_matches_executor(task, caller))
                .unwrap_or(true)
        })
        .filter_map(|task| {
            let derived = derivations.task_state(&task.id)?;
            Some(CoordinationTaskV2 {
                task: task.clone(),
                status: derived.effective_status,
                graph_actionable: derived.graph_actionable,
                blocker_causes: derived.blocker_causes.clone(),
                dependencies: graph.dependency_targets(&NodeRef::task(task.id.clone())),
                dependents: graph.dependency_sources(&NodeRef::task(task.id.clone())),
            })
        })
        .collect::<Vec<_>>();
    tasks.sort_by(|left, right| left.task.id.0.cmp(&right.task.id.0));
    tasks
}

fn ready_task_views_for_plan_from_snapshot(
    snapshot: &prism_coordination::CoordinationSnapshotV2,
    derivations: Option<CoordinationDerivations>,
    plan_id: &PlanId,
    caller: Option<&TaskExecutorCaller>,
) -> Vec<CoordinationTaskV2> {
    let Some(derivations) = derivations else {
        return Vec::new();
    };
    let Ok(graph) = snapshot.graph() else {
        return Vec::new();
    };
    let descendant_task_ids = canonical_descendant_task_ids(&graph, plan_id)
        .into_iter()
        .map(|task_id| task_id.0)
        .collect::<std::collections::BTreeSet<_>>();
    let actionable_ids = derivations
        .graph_actionable_tasks()
        .iter()
        .map(|task_id| task_id.0.to_string())
        .collect::<std::collections::BTreeSet<_>>();
    let mut tasks = snapshot
        .tasks
        .iter()
        .filter(|task| descendant_task_ids.contains(task.id.0.as_str()))
        .filter(|task| actionable_ids.contains(task.id.0.as_str()))
        .filter(|task| {
            caller
                .map(|caller| canonical_task_matches_executor(task, caller))
                .unwrap_or(true)
        })
        .filter_map(|task| {
            let derived = derivations.task_state(&task.id)?;
            Some(CoordinationTaskV2 {
                task: task.clone(),
                status: derived.effective_status,
                graph_actionable: derived.graph_actionable,
                blocker_causes: derived.blocker_causes.clone(),
                dependencies: graph.dependency_targets(&NodeRef::task(task.id.clone())),
                dependents: graph.dependency_sources(&NodeRef::task(task.id.clone())),
            })
        })
        .collect::<Vec<_>>();
    tasks.sort_by(|left, right| left.task.id.0.cmp(&right.task.id.0));
    tasks
}

fn canonical_task_matches_executor(
    task: &CanonicalTaskRecord,
    caller: &TaskExecutorCaller,
) -> bool {
    let policy = &task.executor;
    caller.executor_class == policy.executor_class
        && policy
            .target_label
            .as_ref()
            .is_none_or(|label| caller.target_label.as_ref() == Some(label))
        && principal_allowed(&policy.allowed_principals, caller.principal_id.as_ref())
}

fn principal_allowed(allowed: &[PrincipalId], caller_principal: Option<&PrincipalId>) -> bool {
    allowed.is_empty()
        || caller_principal
            .as_ref()
            .is_some_and(|principal| allowed.contains(*principal))
}

fn latest_review(reviews: &[ArtifactReview]) -> Option<ArtifactReview> {
    reviews
        .iter()
        .max_by(|left, right| {
            left.meta
                .ts
                .cmp(&right.meta.ts)
                .then_with(|| left.id.0.cmp(&right.id.0))
        })
        .cloned()
}

fn coordination_worktree_scope(prism: &Prism) -> Option<String> {
    prism
        .coordination_context()
        .map(|context| context.worktree_id)
}

fn canonical_descendant_plan_ids(
    graph: &prism_coordination::CanonicalCoordinationGraph<'_>,
    plan_id: &PlanId,
) -> Vec<PlanId> {
    let mut plans = Vec::new();
    let mut tasks = Vec::new();
    collect_canonical_descendants(graph, plan_id, &mut plans, &mut tasks);
    plans
}

fn canonical_descendant_task_ids(
    graph: &prism_coordination::CanonicalCoordinationGraph<'_>,
    plan_id: &PlanId,
) -> Vec<TaskId> {
    let mut plans = Vec::new();
    let mut tasks = Vec::new();
    collect_canonical_descendants(graph, plan_id, &mut plans, &mut tasks);
    tasks
}

fn collect_canonical_descendants(
    graph: &prism_coordination::CanonicalCoordinationGraph<'_>,
    plan_id: &PlanId,
    plans: &mut Vec<PlanId>,
    tasks: &mut Vec<TaskId>,
) {
    for child in graph.children_of_plan(plan_id) {
        match child.kind {
            NodeRefKind::Plan => {
                let child_plan = PlanId::new(child.id.clone());
                plans.push(child_plan.clone());
                collect_canonical_descendants(graph, &child_plan, plans, tasks);
            }
            NodeRefKind::Task => tasks.push(TaskId::new(child.id)),
        }
    }
}

fn is_task_completion_gate(kind: prism_coordination::BlockerKind) -> bool {
    !matches!(
        kind,
        prism_coordination::BlockerKind::Dependency
            | prism_coordination::BlockerKind::ClaimConflict
    )
}

fn is_task_review_gate(kind: prism_coordination::BlockerKind) -> bool {
    matches!(
        kind,
        prism_coordination::BlockerKind::ReviewRequired
            | prism_coordination::BlockerKind::RiskReviewRequired
    )
}

fn is_task_validation_gate(kind: prism_coordination::BlockerKind) -> bool {
    kind == prism_coordination::BlockerKind::ValidationRequired
}

fn is_task_stale_gate(kind: prism_coordination::BlockerKind) -> bool {
    matches!(
        kind,
        prism_coordination::BlockerKind::StaleRevision
            | prism_coordination::BlockerKind::ArtifactStale
    )
}

fn compatibility_plan_status(status: DerivedPlanStatus) -> PlanStatus {
    match status {
        DerivedPlanStatus::Pending | DerivedPlanStatus::Active => PlanStatus::Active,
        DerivedPlanStatus::Blocked
        | DerivedPlanStatus::BrokenDependency
        | DerivedPlanStatus::Failed => PlanStatus::Blocked,
        DerivedPlanStatus::Completed => PlanStatus::Completed,
        DerivedPlanStatus::Abandoned => PlanStatus::Abandoned,
        DerivedPlanStatus::Archived => PlanStatus::Archived,
    }
}

fn dedupe_blockers(mut blockers: Vec<TaskBlocker>) -> Vec<TaskBlocker> {
    blockers.sort_by(|left, right| {
        format!("{:?}", left.kind)
            .cmp(&format!("{:?}", right.kind))
            .then_with(|| left.summary.cmp(&right.summary))
    });
    blockers.dedup_by(|left, right| left.kind == right.kind && left.summary == right.summary);
    blockers
}

fn dedupe_strings(values: Vec<String>) -> Vec<String> {
    let mut values = values;
    values.sort();
    values.dedup();
    values
}
