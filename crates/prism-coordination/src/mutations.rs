use anyhow::{anyhow, Result};
use prism_ir::{
    ArtifactStatus, ClaimId, ClaimMode, ConflictSeverity, CoordinationEventKind,
    CoordinationTaskId, CoordinationTaskStatus, EventId, EventMeta, PlanId, PlanStatus, ReviewId,
    ReviewVerdict, SessionId, Timestamp, WorkspaceRevision,
};
use serde_json::{json, Value};

use crate::helpers::{
    claim_is_active, dedupe_anchors, dedupe_conflicts, dedupe_event_ids, dedupe_ids,
    dedupe_strings, derived_event_meta, editor_capacity_conflicts, expire_claims_locked,
    missing_validations_for_artifact, normalize_acceptance, plan_policy_for_task, policy_violation,
    policy_violation_from_blocker, record_rejection, simulate_conflicts, validate_plan_transition,
    validate_task_transition,
};
use crate::state::CoordinationState;
use crate::state::CoordinationStore;
use crate::types::{
    Artifact, ArtifactProposeInput, ArtifactReview, ArtifactReviewInput, ArtifactSupersedeInput,
    ClaimAcquireInput, CoordinationEvent, CoordinationTask, HandoffInput, Plan, PlanCreateInput,
    PlanUpdateInput, PolicyViolation, PolicyViolationCode, TaskUpdateInput, WorkClaim,
};

fn rejection_error(
    state: &mut CoordinationState,
    meta: &EventMeta,
    summary: impl Into<String>,
    plan_id: Option<PlanId>,
    task_id: Option<CoordinationTaskId>,
    claim_id: Option<ClaimId>,
    artifact_id: Option<prism_ir::ArtifactId>,
    violations: Vec<PolicyViolation>,
) -> anyhow::Error {
    let summary = summary.into();
    record_rejection(
        state,
        meta,
        summary.clone(),
        plan_id,
        task_id,
        claim_id,
        artifact_id,
        &violations,
    );
    anyhow!(
        "{}: {}",
        summary,
        violations
            .iter()
            .map(|violation| violation.summary.as_str())
            .collect::<Vec<_>>()
            .join("; ")
    )
}

impl CoordinationStore {
    pub fn create_plan(&self, meta: EventMeta, input: PlanCreateInput) -> Result<(PlanId, Plan)> {
        let mut state = self
            .state
            .write()
            .expect("coordination store lock poisoned");
        state.next_plan += 1;
        let id = PlanId::new(format!("plan:{}", state.next_plan));
        let plan = Plan {
            id: id.clone(),
            goal: input.goal.clone(),
            status: PlanStatus::Active,
            policy: input.policy.unwrap_or_default(),
            root_tasks: Vec::new(),
        };
        state.plans.insert(id.clone(), plan.clone());
        state.events.push(CoordinationEvent {
            meta,
            kind: CoordinationEventKind::PlanCreated,
            summary: input.goal,
            plan: Some(id.clone()),
            task: None,
            claim: None,
            artifact: None,
            review: None,
            metadata: Value::Null,
        });
        Ok((id, plan))
    }

    pub fn update_plan(&self, meta: EventMeta, input: PlanUpdateInput) -> Result<Plan> {
        let mut state = self
            .state
            .write()
            .expect("coordination store lock poisoned");
        let previous = state
            .plans
            .get(&input.plan_id)
            .cloned()
            .ok_or_else(|| anyhow!("unknown plan `{}`", input.plan_id.0))?;
        if matches!(
            previous.status,
            PlanStatus::Completed | PlanStatus::Abandoned
        ) && (input.goal.is_some() || input.policy.is_some())
        {
            let violations = vec![policy_violation(
                PolicyViolationCode::TerminalPlanEdit,
                format!(
                    "terminal coordination plan `{}` cannot be edited",
                    previous.id.0
                ),
                Some(previous.id.clone()),
                None,
                None,
                None,
                Value::Null,
            )];
            return Err(rejection_error(
                &mut state,
                &meta,
                "coordination plan update rejected",
                Some(previous.id),
                None,
                None,
                None,
                violations,
            ));
        }
        if let Some(next_status) = input.status {
            if let Err(error) = validate_plan_transition(previous.status, next_status) {
                let violations = vec![policy_violation(
                    PolicyViolationCode::InvalidPlanTransition,
                    error.to_string(),
                    Some(previous.id.clone()),
                    None,
                    None,
                    None,
                    json!({
                        "from": format!("{:?}", previous.status),
                        "to": format!("{:?}", next_status),
                    }),
                )];
                return Err(rejection_error(
                    &mut state,
                    &meta,
                    "coordination plan update rejected",
                    Some(previous.id),
                    None,
                    None,
                    None,
                    violations,
                ));
            }
        }
        if matches!(input.status, Some(PlanStatus::Completed)) {
            let mut violations = state
                .tasks
                .values()
                .filter(|task| task.plan == input.plan_id)
                .filter(|task| {
                    !matches!(
                        task.status,
                        CoordinationTaskStatus::Completed | CoordinationTaskStatus::Abandoned
                    )
                })
                .map(|task| {
                    policy_violation(
                        PolicyViolationCode::IncompletePlanTasks,
                        format!(
                            "coordination task `{}` is still {:?}",
                            task.id.0, task.status
                        ),
                        Some(input.plan_id.clone()),
                        Some(task.id.clone()),
                        None,
                        None,
                        Value::Null,
                    )
                })
                .collect::<Vec<_>>();
            let active_claim_violations = state
                .claims
                .values()
                .filter(|claim| claim_is_active(claim, meta.ts))
                .filter(|claim| {
                    claim
                        .task
                        .as_ref()
                        .and_then(|task_id| state.tasks.get(task_id))
                        .map(|task| task.plan == input.plan_id)
                        .unwrap_or(false)
                })
                .map(|claim| {
                    policy_violation(
                        PolicyViolationCode::ActivePlanClaims,
                        format!("claim `{}` is still active for this plan", claim.id.0),
                        Some(input.plan_id.clone()),
                        claim.task.clone(),
                        Some(claim.id.clone()),
                        None,
                        Value::Null,
                    )
                })
                .collect::<Vec<_>>();
            violations.extend(active_claim_violations);
            if !violations.is_empty() {
                return Err(rejection_error(
                    &mut state,
                    &meta,
                    "coordination plan cannot be completed",
                    Some(input.plan_id),
                    None,
                    None,
                    None,
                    violations,
                ));
            }
        }
        let plan = state
            .plans
            .get_mut(&input.plan_id)
            .expect("plan validated above");
        if let Some(goal) = input.goal {
            plan.goal = goal;
        }
        if let Some(status) = input.status {
            plan.status = status;
        }
        if let Some(policy) = input.policy {
            plan.policy = policy;
        }
        let plan = plan.clone();
        state.events.push(CoordinationEvent {
            meta,
            kind: CoordinationEventKind::PlanUpdated,
            summary: plan.goal.clone(),
            plan: Some(plan.id.clone()),
            task: None,
            claim: None,
            artifact: None,
            review: None,
            metadata: json!({
                "status": format!("{:?}", plan.status),
            }),
        });
        Ok(plan)
    }

    pub fn create_task(
        &self,
        meta: EventMeta,
        input: crate::types::TaskCreateInput,
    ) -> Result<(CoordinationTaskId, CoordinationTask)> {
        let mut state = self
            .state
            .write()
            .expect("coordination store lock poisoned");
        let Some(plan) = state.plans.get(&input.plan_id).cloned() else {
            return Err(anyhow!("unknown plan `{}`", input.plan_id.0));
        };
        if matches!(plan.status, PlanStatus::Completed | PlanStatus::Abandoned) {
            let violations = vec![policy_violation(
                PolicyViolationCode::PlanClosed,
                format!(
                    "coordination plan `{}` is {:?} and cannot accept new tasks",
                    plan.id.0, plan.status
                ),
                Some(plan.id.clone()),
                None,
                None,
                None,
                Value::Null,
            )];
            return Err(rejection_error(
                &mut state,
                &meta,
                "coordination task creation rejected",
                Some(plan.id),
                None,
                None,
                None,
                violations,
            ));
        }
        for dependency in &input.depends_on {
            let Some(task) = state.tasks.get(dependency) else {
                let violations = vec![policy_violation(
                    PolicyViolationCode::MissingDependency,
                    format!("unknown dependency task `{}`", dependency.0),
                    Some(input.plan_id.clone()),
                    None,
                    None,
                    None,
                    json!({ "dependencyTaskId": dependency.0 }),
                )];
                return Err(rejection_error(
                    &mut state,
                    &meta,
                    "coordination task creation rejected",
                    Some(input.plan_id.clone()),
                    None,
                    None,
                    None,
                    violations,
                ));
            };
            if task.plan != input.plan_id {
                let violations = vec![policy_violation(
                    PolicyViolationCode::CrossPlanDependency,
                    format!(
                        "dependency task `{}` belongs to a different plan",
                        dependency.0
                    ),
                    Some(input.plan_id.clone()),
                    None,
                    None,
                    None,
                    json!({
                        "dependencyTaskId": dependency.0,
                        "dependencyPlanId": task.plan.0,
                    }),
                )];
                return Err(rejection_error(
                    &mut state,
                    &meta,
                    "coordination task creation rejected",
                    Some(input.plan_id.clone()),
                    None,
                    None,
                    None,
                    violations,
                ));
            }
        }
        state.next_task += 1;
        let id = CoordinationTaskId::new(format!("coord-task:{}", state.next_task));
        let is_root = input.depends_on.is_empty();
        let task = CoordinationTask {
            id: id.clone(),
            plan: input.plan_id.clone(),
            title: input.title.clone(),
            status: input.status.unwrap_or(CoordinationTaskStatus::Ready),
            assignee: input.assignee,
            session: input.session,
            anchors: dedupe_anchors(input.anchors),
            depends_on: dedupe_ids(input.depends_on),
            acceptance: normalize_acceptance(input.acceptance),
            base_revision: input.base_revision,
        };
        if is_root {
            let plan = state
                .plans
                .get_mut(&input.plan_id)
                .expect("plan validated above");
            plan.root_tasks.push(id.clone());
            plan.root_tasks = dedupe_ids(plan.root_tasks.clone());
        }
        state.tasks.insert(id.clone(), task.clone());
        state.events.push(CoordinationEvent {
            meta,
            kind: CoordinationEventKind::TaskCreated,
            summary: input.title,
            plan: Some(input.plan_id),
            task: Some(id.clone()),
            claim: None,
            artifact: None,
            review: None,
            metadata: Value::Null,
        });
        Ok((id, task))
    }

    pub fn update_task(
        &self,
        meta: EventMeta,
        input: TaskUpdateInput,
        current_revision: WorkspaceRevision,
        now: Timestamp,
    ) -> Result<CoordinationTask> {
        let completion_context = input.completion_context.clone();
        let mut state = self
            .state
            .write()
            .expect("coordination store lock poisoned");
        let previous = state
            .tasks
            .get(&input.task_id)
            .cloned()
            .ok_or_else(|| anyhow!("unknown coordination task `{}`", input.task_id.0))?;
        let Some(plan) = state.plans.get(&previous.plan).cloned() else {
            return Err(anyhow!("unknown plan `{}`", previous.plan.0));
        };
        if matches!(plan.status, PlanStatus::Completed | PlanStatus::Abandoned) {
            let violations = vec![policy_violation(
                PolicyViolationCode::PlanClosed,
                format!(
                    "coordination plan `{}` is {:?} and cannot mutate task `{}`",
                    plan.id.0, plan.status, previous.id.0
                ),
                Some(plan.id.clone()),
                Some(previous.id.clone()),
                None,
                None,
                Value::Null,
            )];
            return Err(rejection_error(
                &mut state,
                &meta,
                "coordination task update rejected",
                Some(plan.id),
                Some(previous.id),
                None,
                None,
                violations,
            ));
        }
        let stale_writes_enforced = state
            .plans
            .get(&previous.plan)
            .map(|plan| plan.policy.stale_after_graph_change)
            .unwrap_or(false);
        let task_snapshot;
        let status_changed;
        {
            let task = state
                .tasks
                .get_mut(&input.task_id)
                .expect("task validated above");
            if stale_writes_enforced
                && previous.base_revision.graph_version < current_revision.graph_version
                && input.base_revision.is_none()
            {
                let violations = vec![policy_violation(
                    PolicyViolationCode::StaleRevision,
                    format!(
                        "coordination task `{}` is stale against graph version {}; provide an updated base revision before mutating it",
                        previous.id.0, current_revision.graph_version
                    ),
                    Some(previous.plan.clone()),
                    Some(previous.id.clone()),
                    None,
                    None,
                    json!({
                        "taskBaseGraphVersion": previous.base_revision.graph_version,
                        "currentGraphVersion": current_revision.graph_version,
                    }),
                )];
                return Err(rejection_error(
                    &mut state,
                    &meta,
                    "coordination task update rejected",
                    Some(previous.plan.clone()),
                    Some(previous.id.clone()),
                    None,
                    None,
                    violations,
                ));
            }
            if let Some(base_revision) = &input.base_revision {
                if stale_writes_enforced
                    && base_revision.graph_version < current_revision.graph_version
                {
                    let violations = vec![policy_violation(
                        PolicyViolationCode::StaleRevision,
                        format!(
                            "coordination task `{}` cannot use stale base revision {} when current revision is {}",
                            previous.id.0, base_revision.graph_version, current_revision.graph_version
                        ),
                        Some(previous.plan.clone()),
                        Some(previous.id.clone()),
                        None,
                        None,
                        json!({
                            "baseGraphVersion": base_revision.graph_version,
                            "currentGraphVersion": current_revision.graph_version,
                        }),
                    )];
                    return Err(rejection_error(
                        &mut state,
                        &meta,
                        "coordination task update rejected",
                        Some(previous.plan.clone()),
                        Some(previous.id.clone()),
                        None,
                        None,
                        violations,
                    ));
                }
            }
            status_changed = input
                .status
                .map(|status| status != previous.status)
                .unwrap_or(false);
            if let Some(status) = input.status {
                if let Err(error) = validate_task_transition(previous.status, status) {
                    let violations = vec![policy_violation(
                        PolicyViolationCode::InvalidTaskTransition,
                        error.to_string(),
                        Some(previous.plan.clone()),
                        Some(previous.id.clone()),
                        None,
                        None,
                        json!({
                            "from": format!("{:?}", previous.status),
                            "to": format!("{:?}", status),
                        }),
                    )];
                    return Err(rejection_error(
                        &mut state,
                        &meta,
                        "coordination task update rejected",
                        Some(previous.plan.clone()),
                        Some(previous.id.clone()),
                        None,
                        None,
                        violations,
                    ));
                }
            }
            if matches!(
                previous.status,
                CoordinationTaskStatus::Completed | CoordinationTaskStatus::Abandoned
            ) && (input.title.is_some()
                || input.anchors.is_some()
                || input.assignee.is_some()
                || input.session.is_some())
            {
                let violations = vec![policy_violation(
                    PolicyViolationCode::TerminalTaskEdit,
                    format!(
                        "terminal coordination task `{}` cannot be edited",
                        previous.id.0
                    ),
                    Some(previous.plan.clone()),
                    Some(previous.id.clone()),
                    None,
                    None,
                    Value::Null,
                )];
                return Err(rejection_error(
                    &mut state,
                    &meta,
                    "coordination task update rejected",
                    Some(previous.plan.clone()),
                    Some(previous.id.clone()),
                    None,
                    None,
                    violations,
                ));
            }
            if let Some(title) = input.title {
                task.title = title;
            }
            if let Some(status) = input.status {
                task.status = status;
            }
            if let Some(assignee) = input.assignee {
                task.assignee = assignee;
            }
            if let Some(session) = input.session {
                task.session = session;
            }
            if let Some(anchors) = input.anchors {
                task.anchors = dedupe_anchors(anchors);
            }
            if let Some(base_revision) = input.base_revision {
                task.base_revision = base_revision;
            }
            task_snapshot = task.clone();
        }
        let completion_blockers =
            self.completion_blockers_locked(&state, &task_snapshot, current_revision.clone(), now);
        let mut policy_blockers = if task_snapshot.status == CoordinationTaskStatus::Completed {
            self.completion_policy_blockers_locked(
                &state,
                &task_snapshot,
                current_revision,
                completion_context.as_ref(),
            )
        } else {
            Vec::new()
        };
        if task_snapshot.status == CoordinationTaskStatus::Completed
            && (!completion_blockers.is_empty() || !policy_blockers.is_empty())
        {
            let mut blockers = completion_blockers;
            blockers.append(&mut policy_blockers);
            let violations = blockers
                .iter()
                .map(|blocker| {
                    policy_violation_from_blocker(
                        blocker,
                        task_snapshot.plan.clone(),
                        task_snapshot.id.clone(),
                    )
                })
                .collect::<Vec<_>>();
            return Err(rejection_error(
                &mut state,
                &meta,
                format!("coordination task `{}` cannot complete", task_snapshot.id.0),
                Some(task_snapshot.plan.clone()),
                Some(task_snapshot.id.clone()),
                None,
                None,
                violations,
            ));
        }
        let task = task_snapshot;
        let kind = if previous.assignee != task.assignee {
            CoordinationEventKind::TaskAssigned
        } else if status_changed && task.status == CoordinationTaskStatus::Blocked {
            CoordinationEventKind::TaskBlocked
        } else if previous.status == CoordinationTaskStatus::Blocked && status_changed {
            CoordinationEventKind::TaskUnblocked
        } else {
            CoordinationEventKind::TaskStatusChanged
        };
        state.events.push(CoordinationEvent {
            meta,
            kind,
            summary: task.title.clone(),
            plan: Some(task.plan.clone()),
            task: Some(task.id.clone()),
            claim: None,
            artifact: None,
            review: None,
            metadata: json!({
                "status": format!("{:?}", task.status),
                "previousStatus": format!("{:?}", previous.status),
                "assignee": task.assignee.as_ref().map(|agent| agent.0.to_string()),
            }),
        });
        Ok(task)
    }

    pub fn handoff(
        &self,
        meta: EventMeta,
        input: HandoffInput,
        current_revision: WorkspaceRevision,
    ) -> Result<CoordinationTask> {
        let mut state = self
            .state
            .write()
            .expect("coordination store lock poisoned");
        let plan = {
            let task = state
                .tasks
                .get(&input.task_id)
                .ok_or_else(|| anyhow!("unknown coordination task `{}`", input.task_id.0))?;
            state.plans.get(&task.plan).cloned()
        };
        if let Some(plan) = plan.as_ref() {
            if matches!(plan.status, PlanStatus::Completed | PlanStatus::Abandoned) {
                let violations = vec![policy_violation(
                    PolicyViolationCode::PlanClosed,
                    format!(
                        "coordination plan `{}` is {:?} and cannot hand off task `{}`",
                        plan.id.0, plan.status, input.task_id.0
                    ),
                    Some(plan.id.clone()),
                    Some(input.task_id.clone()),
                    None,
                    None,
                    Value::Null,
                )];
                return Err(rejection_error(
                    &mut state,
                    &meta,
                    "coordination handoff rejected",
                    Some(plan.id.clone()),
                    Some(input.task_id.clone()),
                    None,
                    None,
                    violations,
                ));
            }
        }
        if input.base_revision.graph_version < current_revision.graph_version {
            let violations = vec![policy_violation(
                PolicyViolationCode::StaleRevision,
                format!(
                    "coordination task `{}` cannot hand off from stale base revision {} when current revision is {}",
                    input.task_id.0, input.base_revision.graph_version, current_revision.graph_version
                ),
                plan.as_ref().map(|plan| plan.id.clone()),
                Some(input.task_id.clone()),
                None,
                None,
                json!({
                    "baseGraphVersion": input.base_revision.graph_version,
                    "currentGraphVersion": current_revision.graph_version,
                }),
            )];
            return Err(rejection_error(
                &mut state,
                &meta,
                "coordination handoff rejected",
                plan.as_ref().map(|plan| plan.id.clone()),
                Some(input.task_id.clone()),
                None,
                None,
                violations,
            ));
        }
        if plan
            .as_ref()
            .map(|plan| plan.policy.stale_after_graph_change)
            .unwrap_or(false)
        {
            let task = state
                .tasks
                .get(&input.task_id)
                .expect("task validated above");
            if task.base_revision.graph_version < current_revision.graph_version {
                let violations = vec![policy_violation(
                    PolicyViolationCode::StaleRevision,
                    format!(
                        "coordination task `{}` is stale against graph version {} and cannot be handed off until refreshed",
                        input.task_id.0, current_revision.graph_version
                    ),
                    plan.as_ref().map(|plan| plan.id.clone()),
                    Some(input.task_id.clone()),
                    None,
                    None,
                    json!({
                        "taskBaseGraphVersion": task.base_revision.graph_version,
                        "currentGraphVersion": current_revision.graph_version,
                    }),
                )];
                return Err(rejection_error(
                    &mut state,
                    &meta,
                    "coordination handoff rejected",
                    plan.as_ref().map(|plan| plan.id.clone()),
                    Some(input.task_id.clone()),
                    None,
                    None,
                    violations,
                ));
            }
        }
        let task = state
            .tasks
            .get_mut(&input.task_id)
            .ok_or_else(|| anyhow!("unknown coordination task `{}`", input.task_id.0))?;
        let target_agent = input.to_agent.clone();
        task.assignee = target_agent.clone();
        task.session = None;
        task.status = CoordinationTaskStatus::Ready;
        task.base_revision = input.base_revision.clone();
        let task = task.clone();
        state.events.push(CoordinationEvent {
            meta: meta.clone(),
            kind: CoordinationEventKind::HandoffRequested,
            summary: input.summary.clone(),
            plan: Some(task.plan.clone()),
            task: Some(task.id.clone()),
            claim: None,
            artifact: None,
            review: None,
            metadata: serde_json::json!({
                "to_agent": target_agent.map(|agent| agent.0.to_string())
            }),
        });
        state.events.push(CoordinationEvent {
            meta: derived_event_meta(&meta, "accepted"),
            kind: CoordinationEventKind::HandoffAccepted,
            summary: input.summary,
            plan: Some(task.plan.clone()),
            task: Some(task.id.clone()),
            claim: None,
            artifact: None,
            review: None,
            metadata: Value::Null,
        });
        Ok(task)
    }

    pub fn acquire_claim(
        &self,
        meta: EventMeta,
        session_id: SessionId,
        input: ClaimAcquireInput,
    ) -> Result<(
        Option<ClaimId>,
        Vec<crate::types::CoordinationConflict>,
        Option<WorkClaim>,
    )> {
        let mut state = self
            .state
            .write()
            .expect("coordination store lock poisoned");
        expire_claims_locked(&mut state, meta.ts);
        let anchors = dedupe_anchors(input.anchors);
        let plan_id = input
            .task_id
            .as_ref()
            .and_then(|task_id| state.tasks.get(task_id))
            .map(|task| task.plan.clone());
        let plan = plan_id
            .as_ref()
            .and_then(|plan_id| state.plans.get(plan_id))
            .cloned();
        if let Some(plan) = plan {
            if matches!(plan.status, PlanStatus::Completed | PlanStatus::Abandoned) {
                let violations = vec![policy_violation(
                    PolicyViolationCode::PlanClosed,
                    format!(
                        "coordination plan `{}` is {:?} and cannot accept new claims",
                        plan.id.0, plan.status
                    ),
                    Some(plan.id.clone()),
                    input.task_id.clone(),
                    None,
                    None,
                    Value::Null,
                )];
                return Err(rejection_error(
                    &mut state,
                    &meta,
                    "claim acquisition rejected",
                    Some(plan.id.clone()),
                    input.task_id.clone(),
                    None,
                    None,
                    violations,
                ));
            }
        }
        let policy = plan_policy_for_task(&state, input.task_id.as_ref())?;
        if policy
            .map(|policy| policy.stale_after_graph_change)
            .unwrap_or(false)
            && input.base_revision.graph_version < input.current_revision.graph_version
        {
            let violations = vec![policy_violation(
                PolicyViolationCode::StaleRevision,
                format!(
                    "claim acquisition cannot use stale base revision {} when current revision is {}",
                    input.base_revision.graph_version, input.current_revision.graph_version
                ),
                plan_id.clone(),
                input.task_id.clone(),
                None,
                None,
                json!({
                    "baseGraphVersion": input.base_revision.graph_version,
                    "currentGraphVersion": input.current_revision.graph_version,
                }),
            )];
            return Err(rejection_error(
                &mut state,
                &meta,
                "claim acquisition rejected",
                plan_id.clone(),
                input.task_id.clone(),
                None,
                None,
                violations,
            ));
        }
        let mode = input
            .mode
            .or_else(|| policy.map(|policy| policy.default_claim_mode))
            .unwrap_or(ClaimMode::Advisory);
        let mut conflicts = simulate_conflicts(
            state.claims.values(),
            &anchors,
            input.capability,
            mode,
            input.task_id.as_ref(),
            input.base_revision.clone(),
            &session_id,
        );
        conflicts.extend(editor_capacity_conflicts(
            &state,
            &anchors,
            input.capability,
            input.task_id.as_ref(),
            &session_id,
            policy,
            meta.ts,
        ));
        let conflicts = dedupe_conflicts(conflicts);
        let has_blocking = conflicts
            .iter()
            .any(|conflict| conflict.severity == ConflictSeverity::Block);
        if has_blocking {
            let violations = conflicts
                .iter()
                .filter(|conflict| conflict.severity == ConflictSeverity::Block)
                .map(|conflict| {
                    policy_violation(
                        PolicyViolationCode::ClaimConflict,
                        conflict.summary.clone(),
                        plan_id.clone(),
                        input.task_id.clone(),
                        None,
                        None,
                        json!({
                            "blockingClaimIds": conflict
                                .blocking_claims
                                .iter()
                                .map(|claim_id| claim_id.0.to_string())
                                .collect::<Vec<_>>(),
                            "overlapKinds": conflict
                                .overlap_kinds
                                .iter()
                                .map(|kind| format!("{kind:?}"))
                                .collect::<Vec<_>>(),
                        }),
                    )
                })
                .collect::<Vec<_>>();
            state.events.push(CoordinationEvent {
                meta,
                kind: CoordinationEventKind::ClaimContended,
                summary: "claim blocked by active contention".to_string(),
                plan: plan_id.clone(),
                task: input.task_id.clone(),
                claim: None,
                artifact: None,
                review: None,
                metadata: json!({
                    "conflicts": conflicts.clone(),
                    "violations": violations.clone(),
                }),
            });
            return Ok((None, conflicts, None));
        }
        state.next_claim += 1;
        let id = ClaimId::new(format!("claim:{}", state.next_claim));
        let claim = WorkClaim {
            id: id.clone(),
            holder: session_id,
            agent: input.agent,
            task: input.task_id,
            anchors,
            capability: input.capability,
            mode,
            since: meta.ts,
            expires_at: meta.ts.saturating_add(input.ttl_seconds.unwrap_or(900)),
            status: if conflicts.is_empty() {
                prism_ir::ClaimStatus::Active
            } else {
                prism_ir::ClaimStatus::Contended
            },
            base_revision: input.base_revision,
        };
        state.claims.insert(id.clone(), claim.clone());
        state.events.push(CoordinationEvent {
            meta: meta.clone(),
            kind: CoordinationEventKind::ClaimAcquired,
            summary: "claim acquired".to_string(),
            plan: plan_id.clone(),
            task: claim.task.clone(),
            claim: Some(id.clone()),
            artifact: None,
            review: None,
            metadata: json!({
                "status": format!("{:?}", claim.status),
            }),
        });
        if !conflicts.is_empty() {
            state.events.push(CoordinationEvent {
                meta: EventMeta {
                    id: EventId::new(format!("{}:contended", id.0)),
                    ts: meta.ts,
                    actor: meta.actor,
                    correlation: meta.correlation.clone(),
                    causation: Some(meta.id.clone()),
                },
                kind: CoordinationEventKind::ClaimContended,
                summary: "claim acquired with contention".to_string(),
                plan: plan_id,
                task: claim.task.clone(),
                claim: Some(id.clone()),
                artifact: None,
                review: None,
                metadata: json!({ "conflicts": conflicts.clone() }),
            });
        }
        Ok((Some(id), conflicts, Some(claim)))
    }

    pub fn renew_claim(
        &self,
        meta: EventMeta,
        claim_id: &ClaimId,
        ttl_seconds: Option<u64>,
    ) -> Result<WorkClaim> {
        let mut state = self
            .state
            .write()
            .expect("coordination store lock poisoned");
        expire_claims_locked(&mut state, meta.ts);
        let claim = state
            .claims
            .get_mut(claim_id)
            .ok_or_else(|| anyhow!("unknown claim `{}`", claim_id.0))?;
        if claim.status == prism_ir::ClaimStatus::Expired {
            return Err(anyhow!("claim `{}` has expired", claim_id.0));
        }
        claim.expires_at = meta.ts.saturating_add(ttl_seconds.unwrap_or(900));
        claim.status = prism_ir::ClaimStatus::Active;
        let claim = claim.clone();
        state.events.push(CoordinationEvent {
            meta,
            kind: CoordinationEventKind::ClaimRenewed,
            summary: "claim renewed".to_string(),
            plan: None,
            task: claim.task.clone(),
            claim: Some(claim.id.clone()),
            artifact: None,
            review: None,
            metadata: Value::Null,
        });
        Ok(claim)
    }

    pub fn release_claim(&self, meta: EventMeta, claim_id: &ClaimId) -> Result<WorkClaim> {
        let mut state = self
            .state
            .write()
            .expect("coordination store lock poisoned");
        expire_claims_locked(&mut state, meta.ts);
        let claim = state
            .claims
            .get_mut(claim_id)
            .ok_or_else(|| anyhow!("unknown claim `{}`", claim_id.0))?;
        if claim.status == prism_ir::ClaimStatus::Expired {
            return Err(anyhow!("claim `{}` has expired", claim_id.0));
        }
        claim.status = prism_ir::ClaimStatus::Released;
        let claim = claim.clone();
        state.events.push(CoordinationEvent {
            meta,
            kind: CoordinationEventKind::ClaimReleased,
            summary: "claim released".to_string(),
            plan: None,
            task: claim.task.clone(),
            claim: Some(claim.id.clone()),
            artifact: None,
            review: None,
            metadata: Value::Null,
        });
        Ok(claim)
    }

    pub fn propose_artifact(
        &self,
        meta: EventMeta,
        input: ArtifactProposeInput,
    ) -> Result<(prism_ir::ArtifactId, Artifact)> {
        let mut state = self
            .state
            .write()
            .expect("coordination store lock poisoned");
        let Some(task) = state.tasks.get(&input.task_id).cloned() else {
            return Err(anyhow!("unknown coordination task `{}`", input.task_id.0));
        };
        let plan = state.plans.get(&task.plan).cloned();
        if let Some(plan) = plan.as_ref() {
            if matches!(plan.status, PlanStatus::Completed | PlanStatus::Abandoned) {
                let violations = vec![policy_violation(
                    PolicyViolationCode::PlanClosed,
                    format!(
                        "coordination plan `{}` is {:?} and cannot accept new artifacts",
                        plan.id.0, plan.status
                    ),
                    Some(plan.id.clone()),
                    Some(input.task_id.clone()),
                    None,
                    None,
                    Value::Null,
                )];
                return Err(rejection_error(
                    &mut state,
                    &meta,
                    "artifact proposal rejected",
                    Some(plan.id.clone()),
                    Some(input.task_id.clone()),
                    None,
                    None,
                    violations,
                ));
            }
        }
        if plan
            .as_ref()
            .map(|plan| plan.policy.stale_after_graph_change)
            .unwrap_or(false)
            && (input.base_revision.graph_version < input.current_revision.graph_version
                || task.base_revision.graph_version < input.current_revision.graph_version)
        {
            let violations = vec![policy_violation(
                PolicyViolationCode::ArtifactStale,
                format!(
                    "artifact proposal for task `{}` is stale against graph version {}",
                    input.task_id.0, input.current_revision.graph_version
                ),
                plan.as_ref().map(|plan| plan.id.clone()),
                Some(input.task_id.clone()),
                None,
                None,
                json!({
                    "artifactBaseGraphVersion": input.base_revision.graph_version,
                    "taskBaseGraphVersion": task.base_revision.graph_version,
                    "currentGraphVersion": input.current_revision.graph_version,
                }),
            )];
            return Err(rejection_error(
                &mut state,
                &meta,
                "artifact proposal rejected",
                plan.as_ref().map(|plan| plan.id.clone()),
                Some(input.task_id.clone()),
                None,
                None,
                violations,
            ));
        }
        state.next_artifact += 1;
        let id = prism_ir::ArtifactId::new(format!("artifact:{}", state.next_artifact));
        let artifact = Artifact {
            id: id.clone(),
            task: input.task_id.clone(),
            anchors: dedupe_anchors(input.anchors),
            base_revision: input.base_revision,
            diff_ref: input.diff_ref,
            status: ArtifactStatus::Proposed,
            evidence: dedupe_event_ids(input.evidence),
            reviews: Vec::new(),
            required_validations: dedupe_strings(input.required_validations),
            validated_checks: dedupe_strings(input.validated_checks),
            risk_score: input.risk_score,
        };
        state.artifacts.insert(id.clone(), artifact.clone());
        let plan_id = state
            .tasks
            .get(&input.task_id)
            .map(|task| task.plan.clone());
        state.events.push(CoordinationEvent {
            meta,
            kind: CoordinationEventKind::ArtifactProposed,
            summary: "artifact proposed".to_string(),
            plan: plan_id,
            task: Some(input.task_id),
            claim: None,
            artifact: Some(id.clone()),
            review: None,
            metadata: json!({
                "requiredValidations": artifact.required_validations.clone(),
                "validatedChecks": artifact.validated_checks.clone(),
                "riskScore": artifact.risk_score,
            }),
        });
        Ok((id, artifact))
    }

    pub fn supersede_artifact(
        &self,
        meta: EventMeta,
        input: ArtifactSupersedeInput,
    ) -> Result<Artifact> {
        let mut state = self
            .state
            .write()
            .expect("coordination store lock poisoned");
        let plan_id = state
            .artifacts
            .get(&input.artifact_id)
            .and_then(|artifact| state.tasks.get(&artifact.task))
            .map(|task| task.plan.clone());
        let plan = plan_id
            .as_ref()
            .and_then(|plan_id| state.plans.get(plan_id))
            .cloned();
        if let Some(plan) = plan {
            if matches!(plan.status, PlanStatus::Completed | PlanStatus::Abandoned) {
                let violations = vec![policy_violation(
                    PolicyViolationCode::PlanClosed,
                    format!(
                        "coordination plan `{}` is {:?} and cannot supersede artifacts",
                        plan.id.0, plan.status
                    ),
                    Some(plan.id.clone()),
                    None,
                    None,
                    Some(input.artifact_id.clone()),
                    Value::Null,
                )];
                return Err(rejection_error(
                    &mut state,
                    &meta,
                    "artifact supersede rejected",
                    Some(plan.id.clone()),
                    None,
                    None,
                    Some(input.artifact_id.clone()),
                    violations,
                ));
            }
        }
        let artifact = state
            .artifacts
            .get_mut(&input.artifact_id)
            .ok_or_else(|| anyhow!("unknown artifact `{}`", input.artifact_id.0))?;
        artifact.status = ArtifactStatus::Superseded;
        let artifact = artifact.clone();
        let plan_id = state
            .tasks
            .get(&artifact.task)
            .map(|task| task.plan.clone());
        state.events.push(CoordinationEvent {
            meta,
            kind: CoordinationEventKind::ArtifactSuperseded,
            summary: "artifact superseded".to_string(),
            plan: plan_id.clone(),
            task: Some(artifact.task.clone()),
            claim: None,
            artifact: Some(artifact.id.clone()),
            review: None,
            metadata: json!({ "status": "Superseded" }),
        });
        Ok(artifact)
    }

    pub fn review_artifact(
        &self,
        meta: EventMeta,
        input: ArtifactReviewInput,
        current_revision: WorkspaceRevision,
    ) -> Result<(ReviewId, ArtifactReview, Artifact)> {
        let mut state = self
            .state
            .write()
            .expect("coordination store lock poisoned");
        state.next_review += 1;
        let review_id = ReviewId::new(format!("review:{}", state.next_review));
        let (plan, mut artifact): (Option<Plan>, Artifact) = {
            let artifact = state
                .artifacts
                .get(&input.artifact_id)
                .ok_or_else(|| anyhow!("unknown artifact `{}`", input.artifact_id.0))?
                .clone();
            let plan = state
                .tasks
                .get(&artifact.task)
                .and_then(|task| state.plans.get(&task.plan))
                .cloned();
            (plan, artifact)
        };
        if let Some(plan) = plan.as_ref() {
            if matches!(plan.status, PlanStatus::Completed | PlanStatus::Abandoned) {
                let violations = vec![policy_violation(
                    PolicyViolationCode::PlanClosed,
                    format!(
                        "coordination plan `{}` is {:?} and cannot review artifact `{}`",
                        plan.id.0, plan.status, input.artifact_id.0
                    ),
                    Some(plan.id.clone()),
                    Some(artifact.task.clone()),
                    None,
                    Some(input.artifact_id.clone()),
                    Value::Null,
                )];
                return Err(rejection_error(
                    &mut state,
                    &meta,
                    "artifact review rejected",
                    Some(plan.id.clone()),
                    Some(artifact.task.clone()),
                    None,
                    Some(input.artifact_id.clone()),
                    violations,
                ));
            }
        }
        if matches!(input.verdict, ReviewVerdict::Approved)
            && plan
                .as_ref()
                .map(|plan| plan.policy.stale_after_graph_change)
                .unwrap_or(false)
            && artifact.base_revision.graph_version < current_revision.graph_version
        {
            let violations = vec![policy_violation(
                PolicyViolationCode::ArtifactStale,
                format!(
                    "artifact `{}` is stale against graph version {}",
                    artifact.id.0, current_revision.graph_version
                ),
                plan.as_ref().map(|plan| plan.id.clone()),
                Some(artifact.task.clone()),
                None,
                Some(artifact.id.clone()),
                json!({
                    "artifactBaseGraphVersion": artifact.base_revision.graph_version,
                    "currentGraphVersion": current_revision.graph_version,
                }),
            )];
            return Err(rejection_error(
                &mut state,
                &meta,
                "artifact review rejected",
                plan.as_ref().map(|plan| plan.id.clone()),
                Some(artifact.task.clone()),
                None,
                Some(artifact.id.clone()),
                violations,
            ));
        }
        let review = ArtifactReview {
            id: review_id.clone(),
            artifact: artifact.id.clone(),
            verdict: input.verdict,
            summary: input.summary.clone(),
            meta: meta.clone(),
        };
        let mut review_rejection = None;
        {
            let artifact_mut = state
                .artifacts
                .get_mut(&input.artifact_id)
                .ok_or_else(|| anyhow!("unknown artifact `{}`", input.artifact_id.0))?;
            if !input.required_validations.is_empty() {
                artifact_mut.required_validations =
                    dedupe_strings(input.required_validations.clone());
            }
            if !input.validated_checks.is_empty() {
                let mut checks = artifact_mut.validated_checks.clone();
                checks.extend(input.validated_checks.clone());
                artifact_mut.validated_checks = dedupe_strings(checks);
            }
            if input.risk_score.is_some() {
                artifact_mut.risk_score = input.risk_score;
            }
            if matches!(input.verdict, ReviewVerdict::Approved) {
                let missing = missing_validations_for_artifact(artifact_mut);
                if !missing.is_empty() {
                    review_rejection =
                        Some((artifact_mut.task.clone(), artifact_mut.id.clone(), missing));
                }
            }
            if review_rejection.is_none() {
                artifact_mut.reviews.push(review_id.clone());
                artifact_mut.status = match input.verdict {
                    ReviewVerdict::Approved => ArtifactStatus::Approved,
                    ReviewVerdict::ChangesRequested => ArtifactStatus::InReview,
                    ReviewVerdict::Rejected => ArtifactStatus::Rejected,
                };
                artifact = artifact_mut.clone();
            }
        }
        if let Some((task_id, artifact_id, missing)) = review_rejection {
            let violations = vec![policy_violation(
                PolicyViolationCode::ValidationRequired,
                format!(
                    "artifact `{}` is missing required validations: {}",
                    artifact_id.0,
                    missing.join(", ")
                ),
                plan.as_ref().map(|plan| plan.id.clone()),
                Some(task_id.clone()),
                None,
                Some(artifact_id.clone()),
                json!({ "missingValidations": missing }),
            )];
            return Err(rejection_error(
                &mut state,
                &meta,
                "artifact review rejected",
                plan.as_ref().map(|plan| plan.id.clone()),
                Some(task_id),
                None,
                Some(artifact_id),
                violations,
            ));
        }
        state.reviews.insert(review_id.clone(), review.clone());
        let plan_id = state
            .tasks
            .get(&artifact.task)
            .map(|task| task.plan.clone());
        state.events.push(CoordinationEvent {
            meta,
            kind: CoordinationEventKind::ArtifactReviewed,
            summary: input.summary,
            plan: plan_id,
            task: Some(artifact.task.clone()),
            claim: None,
            artifact: Some(artifact.id.clone()),
            review: Some(review_id.clone()),
            metadata: json!({
                "verdict": format!("{:?}", review.verdict),
                "requiredValidations": artifact.required_validations.clone(),
                "validatedChecks": artifact.validated_checks.clone(),
                "riskScore": artifact.risk_score,
            }),
        });
        Ok((review_id, review, artifact))
    }
}
