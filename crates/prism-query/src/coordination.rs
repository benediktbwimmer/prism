use prism_coordination::{
    Artifact, CoordinationConflict, CoordinationTask, Plan, TaskBlocker, WorkClaim,
};
use prism_ir::{
    AnchorRef, ArtifactId, Capability, ClaimMode, CoordinationTaskId, PlanId, SessionId, Timestamp,
    WorkspaceRevision,
};

use crate::common::{anchor_sort_key, sort_node_ids};
use crate::Prism;

impl Prism {
    pub fn workspace_revision(&self) -> WorkspaceRevision {
        WorkspaceRevision {
            graph_version: self.history_snapshot().events.len() as u64,
            git_commit: None,
        }
    }

    pub fn coordination_plan(&self, plan_id: &PlanId) -> Option<Plan> {
        self.coordination.plan(plan_id)
    }

    pub fn coordination_task(&self, task_id: &CoordinationTaskId) -> Option<CoordinationTask> {
        self.coordination.task(task_id)
    }

    pub fn ready_tasks(&self, plan_id: &PlanId, now: Timestamp) -> Vec<CoordinationTask> {
        self.coordination
            .ready_tasks(plan_id, self.workspace_revision(), now)
    }

    pub fn claims(&self, anchors: &[AnchorRef], now: Timestamp) -> Vec<WorkClaim> {
        let anchors = self.coordination_scope_anchors(anchors);
        self.coordination.claims_for_anchor(&anchors, now)
    }

    pub fn conflicts(&self, anchors: &[AnchorRef], now: Timestamp) -> Vec<CoordinationConflict> {
        let anchors = self.coordination_scope_anchors(anchors);
        self.coordination.conflicts_for_anchor(&anchors, now)
    }

    pub fn blockers(&self, task_id: &CoordinationTaskId, now: Timestamp) -> Vec<TaskBlocker> {
        let mut blockers = self
            .coordination
            .blockers(task_id, self.workspace_revision(), now);
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
                });
            }
            if risk.review_required && !risk.has_approved_artifact {
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
                });
            }
        }

        dedupe_blockers(blockers)
    }

    pub fn pending_reviews(&self, plan_id: Option<&PlanId>) -> Vec<Artifact> {
        self.coordination.pending_reviews(plan_id)
    }

    pub fn artifacts(&self, task_id: &CoordinationTaskId) -> Vec<Artifact> {
        self.coordination.artifacts(task_id)
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
        self.coordination.simulate_claim(
            session_id,
            &anchors,
            capability,
            mode,
            task_id,
            self.workspace_revision(),
            now,
        )
    }

    pub fn coordination_scope_anchors(&self, anchors: &[AnchorRef]) -> Vec<AnchorRef> {
        let mut scoped = self.expand_anchors(anchors);
        let seed_nodes = self.resolve_anchor_nodes(&scoped);
        let mut processed_nodes = seed_nodes.into_iter().take(24).collect::<Vec<_>>();
        sort_node_ids(&mut processed_nodes);
        processed_nodes.dedup();

        for node in processed_nodes {
            scoped.push(AnchorRef::Node(node.clone()));
            if let Some(lineage) = self.lineage_of(&node) {
                scoped.push(AnchorRef::Lineage(lineage));
            }

            for neighbor in self.graph_neighbors(&node).into_iter().take(8) {
                scoped.push(AnchorRef::Node(neighbor.clone()));
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
        self.coordination.events();
        self.coordination
            .snapshot()
            .artifacts
            .into_iter()
            .find(|artifact| &artifact.id == artifact_id)
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
