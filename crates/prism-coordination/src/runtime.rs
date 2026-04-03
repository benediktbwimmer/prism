use anyhow::Result;
use prism_ir::{
    AnchorRef, ArtifactId, ArtifactStatus, Capability, ClaimId, ClaimMode, EventMeta, PlanId,
    ReviewId, SessionId, Timestamp, WorkspaceRevision,
};
use serde_json::Value;

use crate::blockers::{completion_blockers, readiness_blockers};
use crate::helpers::{
    anchors_overlap, artifact_matches_worktree_scope, claim_is_active,
    claim_matches_worktree_scope, conflict_between, dedupe_conflicts, editor_capacity_conflicts,
    expire_claims_locked, plan_policy_for_task, simulate_conflicts, task_matches_worktree_scope,
};
use crate::lease::claim_blocks_new_work;
use crate::mutations::{
    accept_handoff_mutation, acquire_claim_mutation, create_plan_mutation, create_task_mutation,
    handoff_mutation, heartbeat_task_mutation, propose_artifact_mutation, reclaim_task_mutation,
    release_claim_mutation, renew_claim_mutation, resume_task_mutation, review_artifact_mutation,
    set_plan_scheduling_mutation, supersede_artifact_mutation, update_plan_mutation,
    update_task_mutation, update_task_mutation_with_options,
};
use crate::state::CoordinationState;
use crate::types::{
    Artifact, ArtifactProposeInput, ArtifactReview, ArtifactReviewInput, ArtifactSupersedeInput,
    ClaimAcquireInput, CoordinationConflict, CoordinationEvent, CoordinationSnapshot,
    CoordinationTask, HandoffAcceptInput, HandoffInput, Plan, PlanCreateInput, PlanScheduling,
    PlanUpdateInput, PolicyViolation, PolicyViolationRecord, TaskBlocker, TaskCreateInput,
    TaskReclaimInput, TaskResumeInput, TaskUpdateInput, WorkClaim,
};

pub struct CoordinationRuntimeState {
    state: CoordinationState,
}

impl CoordinationRuntimeState {
    pub fn from_snapshot(snapshot: CoordinationSnapshot) -> Self {
        Self {
            state: CoordinationState::from_raw_snapshot(snapshot),
        }
    }

    pub fn replace_from_snapshot(&mut self, snapshot: CoordinationSnapshot) {
        self.state = CoordinationState::from_raw_snapshot(snapshot);
    }

    pub fn snapshot(&self) -> CoordinationSnapshot {
        self.state.snapshot()
    }

    pub fn plan(&self, id: &PlanId) -> Option<Plan> {
        self.state.plans.get(id).cloned()
    }

    pub fn create_plan(
        &mut self,
        meta: EventMeta,
        input: PlanCreateInput,
    ) -> Result<(PlanId, Plan)> {
        create_plan_mutation(&mut self.state, meta, input)
    }

    pub fn update_plan(&mut self, meta: EventMeta, input: PlanUpdateInput) -> Result<Plan> {
        update_plan_mutation(&mut self.state, meta, input)
    }

    pub fn set_plan_scheduling(
        &mut self,
        meta: EventMeta,
        plan_id: PlanId,
        scheduling: PlanScheduling,
    ) -> Result<Plan> {
        set_plan_scheduling_mutation(&mut self.state, meta, plan_id, scheduling)
    }

    pub fn task(&self, id: &prism_ir::CoordinationTaskId) -> Option<CoordinationTask> {
        self.state.tasks.get(id).cloned()
    }

    pub fn create_task(
        &mut self,
        meta: EventMeta,
        input: TaskCreateInput,
    ) -> Result<(prism_ir::CoordinationTaskId, CoordinationTask)> {
        create_task_mutation(&mut self.state, meta, input)
    }

    pub fn update_task(
        &mut self,
        meta: EventMeta,
        input: TaskUpdateInput,
        current_revision: WorkspaceRevision,
        now: Timestamp,
    ) -> Result<CoordinationTask> {
        update_task_mutation(&mut self.state, meta, input, current_revision, now)
    }

    pub fn update_task_authoritative_only(
        &mut self,
        meta: EventMeta,
        input: TaskUpdateInput,
        current_revision: WorkspaceRevision,
        now: Timestamp,
    ) -> Result<CoordinationTask> {
        update_task_mutation_with_options(&mut self.state, meta, input, current_revision, now, true)
    }

    pub fn handoff(
        &mut self,
        meta: EventMeta,
        input: HandoffInput,
        current_revision: WorkspaceRevision,
    ) -> Result<CoordinationTask> {
        handoff_mutation(&mut self.state, meta, input, current_revision)
    }

    pub fn accept_handoff(
        &mut self,
        meta: EventMeta,
        input: HandoffAcceptInput,
    ) -> Result<CoordinationTask> {
        accept_handoff_mutation(&mut self.state, meta, input)
    }

    pub fn acquire_claim(
        &mut self,
        meta: EventMeta,
        session_id: SessionId,
        input: ClaimAcquireInput,
    ) -> Result<(
        Option<ClaimId>,
        Vec<CoordinationConflict>,
        Option<WorkClaim>,
    )> {
        acquire_claim_mutation(&mut self.state, meta, session_id, input)
    }

    pub fn resume_task(
        &mut self,
        meta: EventMeta,
        input: TaskResumeInput,
    ) -> Result<CoordinationTask> {
        resume_task_mutation(&mut self.state, meta, input)
    }

    pub fn reclaim_task(
        &mut self,
        meta: EventMeta,
        input: TaskReclaimInput,
    ) -> Result<CoordinationTask> {
        reclaim_task_mutation(&mut self.state, meta, input)
    }

    pub fn heartbeat_task(
        &mut self,
        meta: EventMeta,
        task_id: &prism_ir::CoordinationTaskId,
        renewal_provenance: &str,
    ) -> Result<CoordinationTask> {
        heartbeat_task_mutation(&mut self.state, meta, task_id, renewal_provenance)
    }

    pub fn renew_claim(
        &mut self,
        meta: EventMeta,
        session_id: &SessionId,
        claim_id: &ClaimId,
        ttl_seconds: Option<u64>,
        renewal_provenance: &str,
    ) -> Result<WorkClaim> {
        renew_claim_mutation(
            &mut self.state,
            meta,
            session_id,
            claim_id,
            ttl_seconds,
            renewal_provenance,
        )
    }

    pub fn release_claim(
        &mut self,
        meta: EventMeta,
        session_id: &SessionId,
        claim_id: &ClaimId,
    ) -> Result<WorkClaim> {
        release_claim_mutation(&mut self.state, meta, session_id, claim_id)
    }

    pub fn propose_artifact(
        &mut self,
        meta: EventMeta,
        input: ArtifactProposeInput,
    ) -> Result<(ArtifactId, Artifact)> {
        propose_artifact_mutation(&mut self.state, meta, input)
    }

    pub fn supersede_artifact(
        &mut self,
        meta: EventMeta,
        input: ArtifactSupersedeInput,
    ) -> Result<Artifact> {
        supersede_artifact_mutation(&mut self.state, meta, input)
    }

    pub fn review_artifact(
        &mut self,
        meta: EventMeta,
        input: ArtifactReviewInput,
        current_revision: WorkspaceRevision,
    ) -> Result<(ReviewId, ArtifactReview, Artifact)> {
        review_artifact_mutation(&mut self.state, meta, input, current_revision)
    }

    pub fn claims_for_anchor(&mut self, anchors: &[AnchorRef], now: Timestamp) -> Vec<WorkClaim> {
        self.claims_for_anchor_in_scope(anchors, now, None)
    }

    pub fn claims_for_anchor_in_scope(
        &mut self,
        anchors: &[AnchorRef],
        now: Timestamp,
        worktree_id: Option<&str>,
    ) -> Vec<WorkClaim> {
        expire_claims_locked(&mut self.state, now);
        let mut claims = self
            .state
            .claims
            .values()
            .filter(|claim| claim_is_active(claim, now))
            .filter(|claim| claim_matches_worktree_scope(claim, worktree_id))
            .filter(|claim| anchors_overlap(&claim.anchors, anchors))
            .cloned()
            .collect::<Vec<_>>();
        claims.sort_by(|left, right| left.id.0.cmp(&right.id.0));
        claims
    }

    pub fn conflicts_for_anchor(
        &mut self,
        anchors: &[AnchorRef],
        now: Timestamp,
    ) -> Vec<CoordinationConflict> {
        self.conflicts_for_anchor_in_scope(anchors, now, None)
    }

    pub fn conflicts_for_anchor_in_scope(
        &mut self,
        anchors: &[AnchorRef],
        now: Timestamp,
        worktree_id: Option<&str>,
    ) -> Vec<CoordinationConflict> {
        expire_claims_locked(&mut self.state, now);
        let relevant = self
            .state
            .claims
            .values()
            .filter(|claim| claim_blocks_new_work(claim, now))
            .filter(|claim| claim_matches_worktree_scope(claim, worktree_id))
            .filter(|claim| anchors_overlap(&claim.anchors, anchors))
            .cloned()
            .collect::<Vec<_>>();
        let mut conflicts = Vec::new();
        for (index, claim) in relevant.iter().enumerate() {
            for other in relevant.iter().skip(index + 1) {
                if let Some(conflict) = conflict_between(claim, other) {
                    conflicts.push(conflict);
                }
            }
        }
        dedupe_conflicts(conflicts)
    }

    pub fn simulate_claim(
        &mut self,
        session_id: &SessionId,
        anchors: &[AnchorRef],
        capability: Capability,
        mode: Option<ClaimMode>,
        task_id: Option<&prism_ir::CoordinationTaskId>,
        revision: WorkspaceRevision,
        now: Timestamp,
    ) -> Vec<CoordinationConflict> {
        self.simulate_claim_in_scope(
            session_id, anchors, capability, mode, task_id, revision, now, None,
        )
    }

    pub fn simulate_claim_in_scope(
        &mut self,
        session_id: &SessionId,
        anchors: &[AnchorRef],
        capability: Capability,
        mode: Option<ClaimMode>,
        task_id: Option<&prism_ir::CoordinationTaskId>,
        revision: WorkspaceRevision,
        now: Timestamp,
        worktree_id: Option<&str>,
    ) -> Vec<CoordinationConflict> {
        expire_claims_locked(&mut self.state, now);
        let policy = plan_policy_for_task(&self.state, task_id).ok().flatten();
        let mode = mode
            .or_else(|| policy.map(|policy| policy.default_claim_mode))
            .unwrap_or(ClaimMode::Advisory);
        let mut conflicts = simulate_conflicts(
            self.state
                .claims
                .values()
                .filter(|claim| claim_blocks_new_work(claim, now))
                .filter(|claim| claim_matches_worktree_scope(claim, worktree_id)),
            anchors,
            capability,
            mode,
            policy,
            task_id,
            revision,
            session_id,
        );
        conflicts.extend(editor_capacity_conflicts(
            &self.state,
            anchors,
            capability,
            task_id,
            session_id,
            policy,
            now,
            worktree_id,
        ));
        dedupe_conflicts(conflicts)
    }

    pub fn pending_reviews(&self, plan_id: Option<&PlanId>) -> Vec<Artifact> {
        self.pending_reviews_in_scope(plan_id, None)
    }

    pub fn pending_reviews_in_scope(
        &self,
        plan_id: Option<&PlanId>,
        worktree_id: Option<&str>,
    ) -> Vec<Artifact> {
        let mut artifacts = self
            .state
            .artifacts
            .values()
            .filter(|artifact| {
                matches!(
                    artifact.status,
                    ArtifactStatus::Proposed | ArtifactStatus::InReview
                )
            })
            .filter(|artifact| artifact_matches_worktree_scope(artifact, worktree_id))
            .filter(|artifact| {
                plan_id.map_or(true, |plan_id| {
                    self.state
                        .tasks
                        .get(&artifact.task)
                        .map(|task| &task.plan == plan_id)
                        .unwrap_or(false)
                })
            })
            .cloned()
            .collect::<Vec<_>>();
        artifacts.sort_by(|left, right| left.id.0.cmp(&right.id.0));
        artifacts
    }

    pub fn ready_tasks(
        &self,
        plan_id: &PlanId,
        current_revision: WorkspaceRevision,
        now: Timestamp,
    ) -> Vec<CoordinationTask> {
        self.ready_tasks_in_scope(plan_id, current_revision, now, None)
    }

    pub fn ready_tasks_in_scope(
        &self,
        plan_id: &PlanId,
        current_revision: WorkspaceRevision,
        now: Timestamp,
        worktree_id: Option<&str>,
    ) -> Vec<CoordinationTask> {
        if !self
            .state
            .plans
            .get(plan_id)
            .is_some_and(|plan| plan.status == prism_ir::PlanStatus::Active)
        {
            return Vec::new();
        }
        let mut tasks = self
            .state
            .tasks
            .values()
            .filter(|task| &task.plan == plan_id)
            .filter(|task| task_matches_worktree_scope(task, worktree_id))
            .filter(|task| {
                matches!(
                    task.status,
                    prism_ir::CoordinationTaskStatus::Ready
                        | prism_ir::CoordinationTaskStatus::InProgress
                )
            })
            .filter(|task| {
                readiness_blockers(&self.state, task, current_revision.clone(), now).is_empty()
            })
            .cloned()
            .collect::<Vec<_>>();
        tasks.sort_by(|left, right| left.id.0.cmp(&right.id.0));
        tasks
    }

    pub fn artifacts(&self, task_id: &prism_ir::CoordinationTaskId) -> Vec<Artifact> {
        self.artifacts_in_scope(task_id, None)
    }

    pub fn artifacts_in_scope(
        &self,
        task_id: &prism_ir::CoordinationTaskId,
        worktree_id: Option<&str>,
    ) -> Vec<Artifact> {
        let mut artifacts = self
            .state
            .artifacts
            .values()
            .filter(|artifact| &artifact.task == task_id)
            .filter(|artifact| artifact_matches_worktree_scope(artifact, worktree_id))
            .cloned()
            .collect::<Vec<_>>();
        artifacts.sort_by(|left, right| left.id.0.cmp(&right.id.0));
        artifacts
    }

    pub fn blockers(
        &self,
        task_id: &prism_ir::CoordinationTaskId,
        current_revision: WorkspaceRevision,
        now: Timestamp,
    ) -> Vec<TaskBlocker> {
        let Some(task) = self.state.tasks.get(task_id) else {
            return Vec::new();
        };
        completion_blockers(&self.state, task, current_revision, now)
    }

    pub fn events(&self) -> Vec<CoordinationEvent> {
        self.state.events.clone()
    }

    pub fn policy_violations(
        &self,
        plan_id: Option<&PlanId>,
        task_id: Option<&prism_ir::CoordinationTaskId>,
        limit: usize,
    ) -> Vec<PolicyViolationRecord> {
        let mut records = self
            .state
            .events
            .iter()
            .filter(|event| event.kind == prism_ir::CoordinationEventKind::MutationRejected)
            .filter(|event| plan_id.is_none_or(|plan_id| event.plan.as_ref() == Some(plan_id)))
            .filter(|event| task_id.is_none_or(|task_id| event.task.as_ref() == Some(task_id)))
            .filter_map(|event| {
                let violations = event
                    .metadata
                    .get("violations")
                    .and_then(|value| {
                        serde_json::from_value::<Vec<PolicyViolation>>(value.clone()).ok()
                    })
                    .unwrap_or_default();
                if violations.is_empty() && event.metadata == Value::Null {
                    return None;
                }
                Some(PolicyViolationRecord {
                    event_id: event.meta.id.clone(),
                    ts: event.meta.ts,
                    summary: event.summary.clone(),
                    plan_id: event.plan.clone(),
                    task_id: event.task.clone(),
                    claim_id: event.claim.clone(),
                    artifact_id: event.artifact.clone(),
                    violations,
                })
            })
            .collect::<Vec<_>>();
        records.sort_by(|left, right| {
            right
                .ts
                .cmp(&left.ts)
                .then_with(|| left.event_id.0.cmp(&right.event_id.0))
        });
        records.truncate(limit);
        records
    }

    pub fn artifact(&self, artifact_id: &ArtifactId) -> Option<Artifact> {
        self.artifact_in_scope(artifact_id, None)
    }

    pub fn artifact_in_scope(
        &self,
        artifact_id: &ArtifactId,
        worktree_id: Option<&str>,
    ) -> Option<Artifact> {
        let artifact = self.state.artifacts.get(artifact_id)?;
        artifact_matches_worktree_scope(artifact, worktree_id).then(|| artifact.clone())
    }
}
