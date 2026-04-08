use prism_coordination::{
    Artifact, ArtifactReview, CoordinationConflict, CoordinationEvent, CoordinationTask, Plan,
    TaskBlocker, TaskExecutorCaller, WorkClaim,
};
use prism_ir::{
    AnchorRef, ArtifactId, Capability, ClaimMode, CoordinationTaskId, NodeRef, PlanId, SessionId,
    ReviewId, TaskId, Timestamp, WorkspaceRevision,
};

use crate::common::{anchor_sort_key, sort_node_ids};
use crate::coordination_query_engine::CoordinationQueryEngine;
use crate::{CoordinationPlanV2, CoordinationTaskV2, Prism, TaskEvidenceStatus, TaskReviewStatus};

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
        CoordinationQueryEngine::new(self).coordination_plan_v2(plan_id)
    }

    pub fn coordination_task_v2(&self, task_id: &TaskId) -> Option<CoordinationTaskV2> {
        CoordinationQueryEngine::new(self).coordination_task_v2(task_id)
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
        CoordinationQueryEngine::new(self).graph_actionable_tasks_v2()
    }

    pub fn actionable_tasks_for_executor_v2(
        &self,
        caller: &TaskExecutorCaller,
    ) -> Vec<CoordinationTaskV2> {
        CoordinationQueryEngine::new(self).actionable_tasks_for_executor_v2(caller)
    }

    pub fn root_plans_v2(&self) -> Vec<CoordinationPlanV2> {
        CoordinationQueryEngine::new(self).root_plans_v2()
    }

    pub fn coordination_artifact(&self, artifact_id: &ArtifactId) -> Option<Artifact> {
        let worktree_id = self.coordination_worktree_scope();
        self.with_coordination_runtime(|runtime| {
            runtime.artifact_in_scope(artifact_id, worktree_id.as_deref())
        })
    }

    pub fn coordination_review(&self, review_id: &ReviewId) -> Option<ArtifactReview> {
        let worktree_id = self.coordination_worktree_scope();
        self.with_coordination_runtime(|runtime| runtime.review_in_scope(review_id, worktree_id.as_deref()))
    }

    pub fn coordination_events(&self) -> Vec<CoordinationEvent> {
        self.with_coordination_runtime(|runtime| runtime.events())
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
        CoordinationQueryEngine::new(self).blockers(task_id, now)
    }

    pub fn base_blockers(&self, task_id: &CoordinationTaskId, now: Timestamp) -> Vec<TaskBlocker> {
        CoordinationQueryEngine::new(self).base_blockers(task_id, now)
    }

    pub fn pending_reviews(&self, plan_id: Option<&PlanId>) -> Vec<Artifact> {
        CoordinationQueryEngine::new(self).pending_reviews(plan_id)
    }

    pub fn artifacts(&self, task_id: &CoordinationTaskId) -> Vec<Artifact> {
        CoordinationQueryEngine::new(self).artifacts(task_id)
    }

    pub fn task_evidence_status(
        &self,
        task_id: &CoordinationTaskId,
        now: Timestamp,
    ) -> Option<TaskEvidenceStatus> {
        CoordinationQueryEngine::new(self).task_evidence_status(task_id, now)
    }

    pub fn task_review_status(
        &self,
        task_id: &CoordinationTaskId,
        now: Timestamp,
    ) -> Option<TaskReviewStatus> {
        CoordinationQueryEngine::new(self).task_review_status(task_id, now)
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
