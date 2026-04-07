use prism_coordination::{
    Artifact, CanonicalTaskRecord, CoordinationConflict, CoordinationDerivations,
    CoordinationEvent, CoordinationTask, Plan, TaskBlocker, TaskExecutorCaller, WorkClaim,
};
use prism_ir::{
    AnchorRef, ArtifactId, BlockerCause, BlockerCauseSource, Capability, ClaimMode,
    CoordinationTaskId, NodeRef, PlanExecutionOverlay, PlanGraph, PlanId, PlanNode, PrincipalId,
    SessionId, TaskId, Timestamp, WorkspaceRevision,
};
use std::collections::BTreeMap;

use crate::common::{anchor_sort_key, sort_node_ids};
use crate::plan_completion::current_timestamp;
use crate::{CoordinationPlanV2, CoordinationTaskV2, Prism};

impl Prism {
    fn coordination_worktree_scope(&self) -> Option<String> {
        self.coordination_context()
            .map(|context| context.worktree_id)
    }

    pub fn workspace_revision(&self) -> WorkspaceRevision {
        self.workspace_revision
            .read()
            .expect("workspace revision lock poisoned")
            .clone()
    }

    pub fn coordination_plan(&self, plan_id: &PlanId) -> Option<Plan> {
        self.with_coordination_runtime(|runtime| runtime.plan(plan_id))
    }

    pub fn coordination_task(&self, task_id: &CoordinationTaskId) -> Option<CoordinationTask> {
        self.with_coordination_runtime(|runtime| runtime.task(task_id))
    }

    pub fn coordination_plan_v2(&self, plan_id: &PlanId) -> Option<CoordinationPlanV2> {
        let snapshot = self.coordination_snapshot_v2();
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

    pub fn coordination_task_v2(&self, task_id: &TaskId) -> Option<CoordinationTaskV2> {
        let snapshot = self.coordination_snapshot_v2();
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

    pub fn plan_children_v2(&self, plan_id: &PlanId) -> Vec<NodeRef> {
        let snapshot = self.coordination_snapshot_v2();
        let Ok(graph) = snapshot.graph() else {
            return Vec::new();
        };
        graph.children_of_plan(plan_id)
    }

    pub fn node_dependencies_v2(&self, node_ref: &NodeRef) -> Vec<NodeRef> {
        let snapshot = self.coordination_snapshot_v2();
        let Ok(graph) = snapshot.graph() else {
            return Vec::new();
        };
        graph.dependency_targets(node_ref)
    }

    pub fn node_dependents_v2(&self, node_ref: &NodeRef) -> Vec<NodeRef> {
        let snapshot = self.coordination_snapshot_v2();
        let Ok(graph) = snapshot.graph() else {
            return Vec::new();
        };
        graph.dependency_sources(node_ref)
    }

    pub fn graph_actionable_tasks_v2(&self) -> Vec<CoordinationTaskV2> {
        let snapshot = self.coordination_snapshot_v2();
        actionable_task_views_from_snapshot(&snapshot, snapshot.derive_statuses().ok(), None)
    }

    pub fn actionable_tasks_for_executor_v2(
        &self,
        caller: &TaskExecutorCaller,
    ) -> Vec<CoordinationTaskV2> {
        let snapshot = self.coordination_snapshot_v2();
        actionable_task_views_from_snapshot(
            &snapshot,
            snapshot.derive_statuses().ok(),
            Some(caller),
        )
    }

    pub fn root_plans_v2(&self) -> Vec<CoordinationPlanV2> {
        let snapshot = self.coordination_snapshot_v2();
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

    pub fn coordination_artifact(&self, artifact_id: &ArtifactId) -> Option<Artifact> {
        let worktree_id = self.coordination_worktree_scope();
        self.with_coordination_runtime(|runtime| {
            runtime.artifact_in_scope(artifact_id, worktree_id.as_deref())
        })
    }

    pub fn coordination_events(&self) -> Vec<CoordinationEvent> {
        self.with_coordination_runtime(|runtime| runtime.events())
    }

    pub fn plan_graph(&self, plan_id: &PlanId) -> Option<PlanGraph> {
        let runtime = self.plan_runtime_state();
        self.hydrated_plan_graph_for_runtime(&runtime, plan_id)
    }

    pub fn plan_execution(&self, plan_id: &PlanId) -> Vec<PlanExecutionOverlay> {
        self.plan_runtime_state().plan_execution(plan_id)
    }

    pub fn plan_graphs(&self) -> Vec<PlanGraph> {
        let runtime = self.plan_runtime_state();
        self.hydrated_plan_graphs_for_runtime(&runtime)
    }

    pub fn authored_plan_graphs(&self) -> Vec<PlanGraph> {
        let runtime = self.plan_runtime_state();
        self.stabilized_plan_graphs_for_persist(&runtime)
    }

    pub fn plan_execution_overlays_by_plan(&self) -> BTreeMap<String, Vec<PlanExecutionOverlay>> {
        self.plan_runtime_state().execution_overlays_by_plan()
    }

    pub fn plan_ready_nodes(&self, plan_id: &PlanId) -> Vec<PlanNode> {
        let runtime = self.plan_runtime_state();
        self.actionable_plan_nodes_for_runtime(&runtime, plan_id, current_timestamp())
    }

    pub fn ready_tasks(&self, plan_id: &PlanId, now: Timestamp) -> Vec<CoordinationTask> {
        let worktree_id = self.coordination_worktree_scope();
        self.with_coordination_runtime(|runtime| {
            runtime.ready_tasks_in_scope(
                plan_id,
                self.workspace_revision(),
                now,
                worktree_id.as_deref(),
            )
        })
    }

    pub fn ready_tasks_for_executor(
        &self,
        plan_id: &PlanId,
        now: Timestamp,
        caller: &TaskExecutorCaller,
    ) -> Vec<CoordinationTask> {
        let worktree_id = self.coordination_worktree_scope();
        self.with_coordination_runtime(|runtime| {
            runtime.ready_tasks_for_executor_in_scope(
                plan_id,
                self.workspace_revision(),
                now,
                worktree_id.as_deref(),
                caller,
            )
        })
    }

    pub fn claims(&self, anchors: &[AnchorRef], now: Timestamp) -> Vec<WorkClaim> {
        let anchors = self.coordination_scope_anchors(anchors);
        let worktree_id = self.coordination_worktree_scope();
        self.with_coordination_runtime_mut(|runtime| {
            runtime.claims_for_anchor_in_scope(&anchors, now, worktree_id.as_deref())
        })
    }

    pub fn task_claim_history(
        &self,
        task_id: &CoordinationTaskId,
        now: Timestamp,
    ) -> Vec<WorkClaim> {
        let worktree_id = self.coordination_worktree_scope();
        self.with_coordination_runtime_mut(|runtime| {
            runtime.claims_for_task_in_scope(task_id, now, worktree_id.as_deref())
        })
    }

    pub fn conflicts(&self, anchors: &[AnchorRef], now: Timestamp) -> Vec<CoordinationConflict> {
        let anchors = self.coordination_scope_anchors(anchors);
        let worktree_id = self.coordination_worktree_scope();
        self.with_coordination_runtime_mut(|runtime| {
            runtime.conflicts_for_anchor_in_scope(&anchors, now, worktree_id.as_deref())
        })
    }

    pub fn blockers(&self, task_id: &CoordinationTaskId, now: Timestamp) -> Vec<TaskBlocker> {
        let mut blockers = self.with_coordination_runtime(|runtime| {
            runtime.blockers(task_id, self.workspace_revision(), now)
        });
        if let Some(risk) = self.task_risk(task_id, now) {
            if !risk.stale_artifact_ids.is_empty() {
                blockers.push(TaskBlocker {
                    kind: prism_coordination::BlockerKind::ArtifactStale,
                    summary: format!(
                        "approved artifact is stale against graph version {}",
                        self.workspace_revision().graph_version
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
                    .coordination_task(task_id)
                    .and_then(|task| self.coordination_plan(&task.plan))
                    .and_then(|plan| plan.policy.review_required_above_risk_score);
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

    pub fn base_blockers(&self, task_id: &CoordinationTaskId, now: Timestamp) -> Vec<TaskBlocker> {
        self.with_coordination_runtime(|runtime| {
            runtime.blockers(task_id, self.workspace_revision(), now)
        })
    }

    pub fn pending_reviews(&self, plan_id: Option<&PlanId>) -> Vec<Artifact> {
        let worktree_id = self.coordination_worktree_scope();
        self.with_coordination_runtime(|runtime| {
            runtime.pending_reviews_in_scope(plan_id, worktree_id.as_deref())
        })
    }

    pub fn artifacts(&self, task_id: &CoordinationTaskId) -> Vec<Artifact> {
        let worktree_id = self.coordination_worktree_scope();
        self.with_coordination_runtime(|runtime| {
            runtime.artifacts_in_scope(task_id, worktree_id.as_deref())
        })
    }

    pub fn policy_violations(
        &self,
        plan_id: Option<&PlanId>,
        task_id: Option<&CoordinationTaskId>,
        limit: usize,
    ) -> Vec<prism_coordination::PolicyViolationRecord> {
        self.with_coordination_runtime(|runtime| runtime.policy_violations(plan_id, task_id, limit))
    }

    pub fn simulate_claim(
        &self,
        session_id: &SessionId,
        anchors: &[AnchorRef],
        capability: Capability,
        mode: Option<ClaimMode>,
        task_id: Option<&CoordinationTaskId>,
        now: Timestamp,
    ) -> Vec<CoordinationConflict> {
        let anchors = self.coordination_scope_anchors(anchors);
        let worktree_id = self.coordination_worktree_scope();
        self.with_coordination_runtime_mut(|runtime| {
            runtime.simulate_claim_in_scope(
                session_id,
                &anchors,
                capability,
                mode,
                task_id,
                self.workspace_revision(),
                now,
                worktree_id.as_deref(),
            )
        })
    }

    pub fn coordination_scope_anchors(&self, anchors: &[AnchorRef]) -> Vec<AnchorRef> {
        let mut scoped = self.expand_anchors(anchors);
        let seed_nodes = self.resolve_anchor_nodes(&scoped);
        let mut processed_nodes = seed_nodes.into_iter().take(24).collect::<Vec<_>>();
        sort_node_ids(&mut processed_nodes);
        processed_nodes.dedup();

        for node in processed_nodes {
            scoped.push(AnchorRef::Node(node.clone()));
            if let Some(graph_node) = self.graph().node(&node) {
                scoped.push(AnchorRef::File(graph_node.file));
            }
            if let Some(lineage) = self.lineage_of(&node) {
                scoped.push(AnchorRef::Lineage(lineage));
            }

            for neighbor in self.graph_neighbors(&node).into_iter().take(8) {
                scoped.push(AnchorRef::Node(neighbor.clone()));
                if let Some(graph_node) = self.graph().node(&neighbor) {
                    scoped.push(AnchorRef::File(graph_node.file));
                }
                if let Some(lineage) = self.lineage_of(&neighbor) {
                    scoped.push(AnchorRef::Lineage(lineage));
                }
            }

            for neighbor in self.co_change_neighbors(&node, 4) {
                scoped.push(AnchorRef::Lineage(neighbor.lineage.clone()));
                for current in neighbor.nodes.into_iter().take(4) {
                    scoped.push(AnchorRef::Node(current));
                }
            }
        }

        scoped.sort_by(anchor_sort_key);
        scoped.dedup();
        scoped
    }

    pub(crate) fn coordinating_artifact(&self, artifact_id: &ArtifactId) -> Option<Artifact> {
        self.coordination_artifact(artifact_id)
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

fn dedupe_blockers(mut blockers: Vec<TaskBlocker>) -> Vec<TaskBlocker> {
    blockers.sort_by(|left, right| {
        format!("{:?}", left.kind)
            .cmp(&format!("{:?}", right.kind))
            .then_with(|| left.summary.cmp(&right.summary))
    });
    blockers.dedup_by(|left, right| left.kind == right.kind && left.summary == right.summary);
    blockers
}
