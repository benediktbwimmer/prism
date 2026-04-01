use prism_ir::{
    AnchorRef, ArtifactStatus, Capability, ClaimMode, CoordinationTaskId, PlanId, SessionId,
    Timestamp, WorkspaceRevision,
};
use serde_json::Value;

use crate::helpers::{
    anchors_overlap, claim_is_active, conflict_between, dedupe_conflicts,
    editor_capacity_conflicts, plan_policy_for_task, simulate_conflicts,
};
use crate::lease::claim_blocks_new_work;
use crate::state::CoordinationStore;
use crate::types::{
    Artifact, CoordinationConflict, CoordinationEvent, CoordinationTask, Plan, PolicyViolation,
    PolicyViolationRecord, TaskBlocker, WorkClaim,
};

impl CoordinationStore {
    pub fn plan(&self, id: &PlanId) -> Option<Plan> {
        self.state
            .read()
            .expect("coordination store lock poisoned")
            .plans
            .get(id)
            .cloned()
    }

    pub fn task(&self, id: &CoordinationTaskId) -> Option<CoordinationTask> {
        self.state
            .read()
            .expect("coordination store lock poisoned")
            .tasks
            .get(id)
            .cloned()
    }

    pub fn ready_tasks(
        &self,
        plan_id: &PlanId,
        current_revision: WorkspaceRevision,
        now: Timestamp,
    ) -> Vec<CoordinationTask> {
        let state = self.state.read().expect("coordination store lock poisoned");
        if !state
            .plans
            .get(plan_id)
            .is_some_and(|plan| plan.status == prism_ir::PlanStatus::Active)
        {
            return Vec::new();
        }
        let mut tasks = state
            .tasks
            .values()
            .filter(|task| &task.plan == plan_id)
            .filter(|task| {
                matches!(
                    task.status,
                    prism_ir::CoordinationTaskStatus::Ready
                        | prism_ir::CoordinationTaskStatus::InProgress
                )
            })
            .filter(|task| {
                self.readiness_blockers_locked(&state, task, current_revision.clone(), now)
                    .is_empty()
            })
            .cloned()
            .collect::<Vec<_>>();
        tasks.sort_by(|left, right| left.id.0.cmp(&right.id.0));
        tasks
    }

    pub fn claims_for_anchor(&self, anchors: &[AnchorRef], now: Timestamp) -> Vec<WorkClaim> {
        let state = self.state.read().expect("coordination store lock poisoned");
        let mut claims = state
            .claims
            .values()
            .filter(|claim| claim_is_active(claim, now))
            .filter(|claim| anchors_overlap(&claim.anchors, anchors))
            .cloned()
            .collect::<Vec<_>>();
        claims.sort_by(|left, right| left.id.0.cmp(&right.id.0));
        claims
    }

    pub fn conflicts_for_anchor(
        &self,
        anchors: &[AnchorRef],
        now: Timestamp,
    ) -> Vec<CoordinationConflict> {
        let state = self.state.read().expect("coordination store lock poisoned");
        let relevant = state
            .claims
            .values()
            .filter(|claim| claim_blocks_new_work(claim, now))
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
        &self,
        session_id: &SessionId,
        anchors: &[AnchorRef],
        capability: Capability,
        mode: Option<ClaimMode>,
        task_id: Option<&CoordinationTaskId>,
        revision: WorkspaceRevision,
        now: Timestamp,
    ) -> Vec<CoordinationConflict> {
        let state = self.state.read().expect("coordination store lock poisoned");
        let policy = plan_policy_for_task(&state, task_id).ok().flatten();
        let mode = mode
            .or_else(|| policy.map(|policy| policy.default_claim_mode))
            .unwrap_or(ClaimMode::Advisory);
        let mut conflicts = simulate_conflicts(
            state
                .claims
                .values()
                .filter(|claim| claim_blocks_new_work(claim, now)),
            anchors,
            capability,
            mode,
            policy,
            task_id,
            revision,
            session_id,
        );
        conflicts.extend(editor_capacity_conflicts(
            &state, anchors, capability, task_id, session_id, policy, now, None,
        ));
        dedupe_conflicts(conflicts)
    }

    pub fn blockers(
        &self,
        task_id: &CoordinationTaskId,
        current_revision: WorkspaceRevision,
        now: Timestamp,
    ) -> Vec<TaskBlocker> {
        let state = self.state.read().expect("coordination store lock poisoned");
        let Some(task) = state.tasks.get(task_id) else {
            return Vec::new();
        };
        self.completion_blockers_locked(&state, task, current_revision, now)
    }

    pub fn pending_reviews(&self, plan_id: Option<&PlanId>) -> Vec<Artifact> {
        let state = self.state.read().expect("coordination store lock poisoned");
        let mut artifacts = state
            .artifacts
            .values()
            .filter(|artifact| {
                matches!(
                    artifact.status,
                    ArtifactStatus::Proposed | ArtifactStatus::InReview
                )
            })
            .filter(|artifact| {
                plan_id.map_or(true, |plan_id| {
                    state
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

    pub fn artifacts(&self, task_id: &CoordinationTaskId) -> Vec<Artifact> {
        let state = self.state.read().expect("coordination store lock poisoned");
        let mut artifacts = state
            .artifacts
            .values()
            .filter(|artifact| &artifact.task == task_id)
            .cloned()
            .collect::<Vec<_>>();
        artifacts.sort_by(|left, right| left.id.0.cmp(&right.id.0));
        artifacts
    }

    pub fn events(&self) -> Vec<CoordinationEvent> {
        self.state
            .read()
            .expect("coordination store lock poisoned")
            .events
            .clone()
    }

    pub fn policy_violations(
        &self,
        plan_id: Option<&PlanId>,
        task_id: Option<&CoordinationTaskId>,
        limit: usize,
    ) -> Vec<PolicyViolationRecord> {
        let state = self.state.read().expect("coordination store lock poisoned");
        let mut records = state
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
}
