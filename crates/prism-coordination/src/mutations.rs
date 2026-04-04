use anyhow::{anyhow, Result};
use prism_ir::{
    new_prefixed_id, ArtifactStatus, ClaimId, ClaimMode, ConflictSeverity, CoordinationEventKind,
    CoordinationTaskId, CoordinationTaskStatus, EventId, EventMeta, PlanBinding, PlanId, PlanKind,
    PlanNodeKind, PlanScope, PlanStatus, ReviewId, ReviewVerdict, SessionId, Timestamp,
    WorkspaceRevision,
};
use serde::Serialize;
use serde_json::{json, Value};

use crate::blockers::{completion_blockers, completion_policy_blockers};
use crate::helpers::{
    claim_matches_worktree_scope, dedupe_anchors, dedupe_conflicts, dedupe_event_ids, dedupe_ids,
    dedupe_strings, derived_event_meta, editor_capacity_conflicts, expire_claims_locked,
    missing_validations_for_artifact, normalize_acceptance, plan_policy_for_task,
    plan_status_is_closed, policy_violation, policy_violation_from_blocker, record_rejection,
    simulate_conflicts, validate_plan_transition, validate_task_transition,
};
use crate::lease::{
    authoritative_task_holder, claim_lease_state, claim_renewal_should_refresh,
    clear_task_lease, current_claim_holder, current_task_holder, refresh_claim_lease,
    refresh_task_lease, same_holder,
    task_heartbeat_should_refresh, task_lease_state, LeaseState,
};
use crate::state::CoordinationState;
use crate::state::CoordinationStore;
use crate::types::{
    Artifact, ArtifactProposeInput, ArtifactReview, ArtifactReviewInput, ArtifactSupersedeInput,
    ClaimAcquireInput, CoordinationEvent, CoordinationTask, HandoffAcceptInput, HandoffInput, Plan,
    PlanCreateInput, PlanScheduling, PlanUpdateInput, PolicyViolation, PolicyViolationCode,
    TaskCreateInput, TaskReclaimInput, TaskResumeInput, TaskUpdateInput, WorkClaim,
};

fn push_patch_op(patch: &mut serde_json::Map<String, Value>, field: &str, op: &str) {
    patch.insert(field.to_string(), Value::String(op.to_string()));
}

fn patch_metadata(patch: serde_json::Map<String, Value>) -> Option<Value> {
    (!patch.is_empty()).then_some(Value::Object(patch))
}

fn insert_serialized<T: Serialize>(map: &mut serde_json::Map<String, Value>, key: &str, value: T) {
    map.insert(
        key.to_string(),
        serde_json::to_value(value).expect("coordination metadata serialization should succeed"),
    );
}

fn lease_holder_details(holder: Option<&crate::types::LeaseHolder>) -> Value {
    holder
        .and_then(|holder| serde_json::to_value(holder).ok())
        .unwrap_or(Value::Null)
}

fn enforce_task_lease_for_standard_mutation(
    state: &mut CoordinationState,
    meta: &EventMeta,
    task: &CoordinationTask,
    summary: &str,
) -> Result<()> {
    let lease_state = task_lease_state(task, meta.ts);
    if matches!(lease_state, LeaseState::Unleased) {
        return Ok(());
    }
    let Some(lease_holder) = authoritative_task_holder(task) else {
        return Ok(());
    };
    let current_holder = current_task_holder(meta, task);
    if matches!(lease_state, LeaseState::Active) && same_holder(&lease_holder, &current_holder) {
        return Ok(());
    }

    let (code, violation_summary) = match lease_state {
        LeaseState::Active => (
            PolicyViolationCode::TaskLeaseHeldByOther,
            format!(
                "coordination task `{}` is actively leased by another principal and cannot be mutated",
                task.id.0
            ),
        ),
        LeaseState::Stale | LeaseState::Expired
            if same_holder(&lease_holder, &current_holder) =>
        (
            PolicyViolationCode::TaskResumeRequired,
            format!(
                "coordination task `{}` has a {:?} lease and must be resumed before it can be mutated",
                task.id.0, lease_state
            ),
        ),
        LeaseState::Stale | LeaseState::Expired => (
            PolicyViolationCode::TaskReclaimRequired,
            format!(
                "coordination task `{}` has a {:?} lease owned by another principal and must be reclaimed before it can be mutated",
                task.id.0, lease_state
            ),
        ),
        LeaseState::Unleased => return Ok(()),
    };
    let violations = vec![policy_violation(
        code,
        violation_summary,
        Some(task.plan.clone()),
        Some(task.id.clone()),
        None,
        None,
        json!({
                "leaseState": format!("{lease_state:?}").to_ascii_lowercase(),
                "leaseHolder": lease_holder_details(Some(&lease_holder)),
                "currentHolder": lease_holder_details(Some(&current_holder)),
                "leaseStaleAt": task.lease_stale_at,
                "leaseExpiresAt": task.lease_expires_at,
            }),
        )];
    Err(rejection_error(
        state,
        meta,
        summary,
        Some(task.plan.clone()),
        Some(task.id.clone()),
        None,
        None,
        violations,
    ))
}

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

fn plan_completion_violations(
    state: &CoordinationState,
    plan_id: &PlanId,
    now: Timestamp,
    allow_abandoned_tasks: bool,
) -> Vec<PolicyViolation> {
    let mut violations = state
        .tasks
        .values()
        .filter(|task| task.plan == *plan_id)
        .filter(|task| {
            !(task.status == CoordinationTaskStatus::Completed
                || (allow_abandoned_tasks && task.status == CoordinationTaskStatus::Abandoned))
        })
        .map(|task| {
            policy_violation(
                PolicyViolationCode::IncompletePlanTasks,
                format!(
                    "coordination task `{}` is still {:?}",
                    task.id.0, task.status
                ),
                Some(plan_id.clone()),
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
        .filter(|claim| matches!(claim_lease_state(claim, now), LeaseState::Active))
        .filter(|claim| {
            claim
                .task
                .as_ref()
                .and_then(|task_id| state.tasks.get(task_id))
                .map(|task| task.plan == *plan_id)
                .unwrap_or(false)
        })
        .map(|claim| {
            policy_violation(
                PolicyViolationCode::ActivePlanClaims,
                format!("claim `{}` is still active for this plan", claim.id.0),
                Some(plan_id.clone()),
                claim.task.clone(),
                Some(claim.id.clone()),
                None,
                Value::Null,
            )
        })
        .collect::<Vec<_>>();
    violations.extend(active_claim_violations);
    violations
}

fn should_auto_complete_execution_plan(
    state: &CoordinationState,
    plan_id: &PlanId,
    now: Timestamp,
) -> bool {
    let Some(plan) = state.plans.get(plan_id) else {
        return false;
    };
    if plan.kind != PlanKind::TaskExecution {
        return false;
    }
    if !matches!(plan.status, PlanStatus::Active | PlanStatus::Blocked) {
        return false;
    }
    let has_tasks = state.tasks.values().any(|task| task.plan == *plan_id);
    has_tasks && plan_completion_violations(state, plan_id, now, false).is_empty()
}

fn auto_complete_execution_plan_if_eligible(
    state: &mut CoordinationState,
    meta: &EventMeta,
    plan_id: &PlanId,
) -> Option<Plan> {
    if !should_auto_complete_execution_plan(state, plan_id, meta.ts) {
        return None;
    }
    let plan = state
        .plans
        .get_mut(plan_id)
        .expect("plan eligibility checked above");
    let previous_status = plan.status;
    plan.status = PlanStatus::Completed;
    let plan = plan.clone();
    state.events.push(CoordinationEvent {
        meta: derived_event_meta(meta, "plan-auto-completed"),
        kind: CoordinationEventKind::PlanUpdated,
        summary: plan.goal.clone(),
        plan: Some(plan.id.clone()),
        task: None,
        claim: None,
        artifact: None,
        review: None,
        metadata: json!({
            "status": plan.status,
            "previousStatus": previous_status,
            "autoTransition": "all_tasks_completed",
            "patch": {
                "status": "set",
            },
            "patchValues": {
                "status": plan.status,
            },
        }),
    });
    Some(plan)
}

pub(crate) fn acquire_claim_mutation(
    state: &mut CoordinationState,
    meta: EventMeta,
    session_id: SessionId,
    input: ClaimAcquireInput,
) -> Result<(
    Option<ClaimId>,
    Vec<crate::types::CoordinationConflict>,
    Option<WorkClaim>,
)> {
    expire_claims_locked(state, meta.ts);
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
        if plan_status_is_closed(plan.status) {
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
                state,
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
    let policy = plan_policy_for_task(state, input.task_id.as_ref())?.cloned();
    if policy
        .as_ref()
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
            state,
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
        .or_else(|| policy.as_ref().map(|policy| policy.default_claim_mode))
        .unwrap_or(ClaimMode::Advisory);
    let mut conflicts = simulate_conflicts(
        state
            .claims
            .values()
            .filter(|claim| claim_matches_worktree_scope(claim, input.worktree_id.as_deref()))
            .filter(|claim| matches!(claim_lease_state(claim, meta.ts), LeaseState::Active)),
        &anchors,
        input.capability,
        mode,
        policy.as_ref(),
        input.task_id.as_ref(),
        input.base_revision.clone(),
        &session_id,
    );
    conflicts.extend(editor_capacity_conflicts(
        state,
        &anchors,
        input.capability,
        input.task_id.as_ref(),
        &session_id,
        policy.as_ref(),
        meta.ts,
        input.worktree_id.as_deref(),
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
    let id = ClaimId::new(new_prefixed_id("claim"));
    let mut claim = WorkClaim {
        id: id.clone(),
        holder: session_id,
        agent: input.agent,
        lease_holder: None,
        worktree_id: input.worktree_id,
        branch_ref: input.branch_ref,
        task: input.task_id,
        anchors,
        capability: input.capability,
        mode,
        since: meta.ts,
        refreshed_at: None,
        stale_at: None,
        expires_at: meta.ts,
        status: if conflicts.is_empty() {
            prism_ir::ClaimStatus::Active
        } else {
            prism_ir::ClaimStatus::Contended
        },
        base_revision: input.base_revision,
    };
    refresh_claim_lease(
        &mut claim,
        &meta,
        meta.ts,
        policy.as_ref(),
        input.ttl_seconds,
    );
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
            "claim": claim.clone(),
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
                execution_context: meta.execution_context.clone(),
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

pub(crate) fn renew_claim_mutation(
    state: &mut CoordinationState,
    meta: EventMeta,
    session_id: &SessionId,
    claim_id: &ClaimId,
    ttl_seconds: Option<u64>,
    renewal_provenance: &str,
) -> Result<WorkClaim> {
    expire_claims_locked(state, meta.ts);
    let claim_snapshot = state
        .claims
        .get(claim_id)
        .cloned()
        .ok_or_else(|| anyhow!("unknown claim `{}`", claim_id.0))?;
    let claim_plan_id = claim_snapshot
        .task
        .as_ref()
        .and_then(|task_id| state.tasks.get(task_id))
        .map(|task| task.plan.clone());
    let current_holder = current_claim_holder(&meta, session_id, &claim_snapshot);
    if claim_snapshot
        .lease_holder
        .as_ref()
        .is_some_and(|lease_holder| !same_holder(lease_holder, &current_holder))
        || (claim_snapshot.lease_holder.is_none() && &claim_snapshot.holder != session_id)
    {
        let violations = vec![policy_violation(
            PolicyViolationCode::ClaimNotOwned,
            format!(
                "claim `{}` is held by `{}` and cannot be renewed by `{}`",
                claim_id.0, claim_snapshot.holder.0, session_id.0
            ),
            claim_plan_id.clone(),
            claim_snapshot.task.clone(),
            Some(claim_snapshot.id.clone()),
            None,
            Value::Null,
        )];
        return Err(rejection_error(
            state,
            &meta,
            "claim renewal rejected",
            claim_plan_id,
            claim_snapshot.task.clone(),
            Some(claim_snapshot.id.clone()),
            None,
            violations,
        ));
    }
    let claim_policy = plan_policy_for_task(state, claim_snapshot.task.as_ref())?.cloned();
    let claim = state
        .claims
        .get_mut(claim_id)
        .ok_or_else(|| anyhow!("unknown claim `{}`", claim_id.0))?;
    if claim.status == prism_ir::ClaimStatus::Released {
        return Err(anyhow!("claim `{}` has already been released", claim_id.0));
    }
    if !claim_renewal_should_refresh(&claim_snapshot, claim_policy.as_ref(), meta.ts, ttl_seconds) {
        return Ok(claim_snapshot);
    }
    claim.status = prism_ir::ClaimStatus::Active;
    refresh_claim_lease(claim, &meta, meta.ts, claim_policy.as_ref(), ttl_seconds);
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
        metadata: json!({
            "claim": claim.clone(),
            "renewalProvenance": renewal_provenance,
        }),
    });
    Ok(claim)
}

pub(crate) fn heartbeat_task_mutation(
    state: &mut CoordinationState,
    meta: EventMeta,
    task_id: &CoordinationTaskId,
    renewal_provenance: &str,
) -> Result<CoordinationTask> {
    let previous = state
        .tasks
        .get(task_id)
        .cloned()
        .ok_or_else(|| anyhow!("unknown coordination task `{}`", task_id.0))?;
    let Some(plan) = state.plans.get(&previous.plan).cloned() else {
        return Err(anyhow!("unknown plan `{}`", previous.plan.0));
    };
    let lease_state = task_lease_state(&previous, meta.ts);
    let current_holder = current_task_holder(&meta, &previous);
    let Some(lease_holder) = authoritative_task_holder(&previous) else {
        return Err(anyhow!(
            "coordination task `{}` does not have an active lease to heartbeat",
            previous.id.0
        ));
    };
    if !same_holder(&lease_holder, &current_holder) {
        let violations = vec![policy_violation(
            PolicyViolationCode::TaskLeaseHeldByOther,
            format!(
                "coordination task `{}` is actively leased by another principal and cannot be heartbeated",
                previous.id.0
            ),
            Some(previous.plan.clone()),
            Some(previous.id.clone()),
            None,
            None,
            json!({
                "leaseState": format!("{lease_state:?}").to_ascii_lowercase(),
                "leaseHolder": lease_holder_details(Some(&lease_holder)),
                "currentHolder": lease_holder_details(Some(&current_holder)),
                "leaseStaleAt": previous.lease_stale_at,
                "leaseExpiresAt": previous.lease_expires_at,
            }),
        )];
        return Err(rejection_error(
            state,
            &meta,
            "coordination task heartbeat rejected",
            Some(previous.plan.clone()),
            Some(previous.id.clone()),
            None,
            None,
            violations,
        ));
    }
    if matches!(lease_state, LeaseState::Stale | LeaseState::Expired) {
        let violations = vec![policy_violation(
            PolicyViolationCode::TaskResumeRequired,
            format!(
                "coordination task `{}` has a {:?} lease and must be resumed before it can be heartbeated",
                previous.id.0, lease_state
            ),
            Some(previous.plan.clone()),
            Some(previous.id.clone()),
            None,
            None,
            json!({
                "leaseState": format!("{lease_state:?}").to_ascii_lowercase(),
                "leaseHolder": lease_holder_details(Some(&lease_holder)),
                "currentHolder": lease_holder_details(Some(&current_holder)),
                "leaseStaleAt": previous.lease_stale_at,
                "leaseExpiresAt": previous.lease_expires_at,
            }),
        )];
        return Err(rejection_error(
            state,
            &meta,
            "coordination task heartbeat rejected",
            Some(previous.plan.clone()),
            Some(previous.id.clone()),
            None,
            None,
            violations,
        ));
    }
    if !task_heartbeat_should_refresh(&previous, &plan.policy, meta.ts) {
        return Ok(previous);
    }

    let task = state.tasks.get_mut(task_id).expect("task validated above");
    refresh_task_lease(task, &meta, meta.ts, &plan.policy);
    let task = task.clone();

    let mut patch = serde_json::Map::new();
    if previous.lease_holder != task.lease_holder {
        push_patch_op(&mut patch, "leaseHolder", "set");
    }
    if previous.lease_started_at != task.lease_started_at {
        push_patch_op(&mut patch, "leaseStartedAt", "set");
    }
    if previous.lease_refreshed_at != task.lease_refreshed_at {
        push_patch_op(&mut patch, "leaseRefreshedAt", "set");
    }
    if previous.lease_stale_at != task.lease_stale_at {
        push_patch_op(&mut patch, "leaseStaleAt", "set");
    }
    if previous.lease_expires_at != task.lease_expires_at {
        push_patch_op(&mut patch, "leaseExpiresAt", "set");
    }

    let mut patch_values = serde_json::Map::new();
    if previous.lease_holder != task.lease_holder {
        insert_serialized(&mut patch_values, "leaseHolder", task.lease_holder.clone());
    }
    if previous.lease_started_at != task.lease_started_at {
        insert_serialized(&mut patch_values, "leaseStartedAt", task.lease_started_at);
    }
    if previous.lease_refreshed_at != task.lease_refreshed_at {
        insert_serialized(
            &mut patch_values,
            "leaseRefreshedAt",
            task.lease_refreshed_at,
        );
    }
    if previous.lease_stale_at != task.lease_stale_at {
        insert_serialized(&mut patch_values, "leaseStaleAt", task.lease_stale_at);
    }
    if previous.lease_expires_at != task.lease_expires_at {
        insert_serialized(&mut patch_values, "leaseExpiresAt", task.lease_expires_at);
    }

    state.events.push(CoordinationEvent {
        meta,
        kind: CoordinationEventKind::TaskHeartbeated,
        summary: format!("task `{}` heartbeat refreshed", task.id.0),
        plan: Some(task.plan.clone()),
        task: Some(task.id.clone()),
        claim: None,
        artifact: None,
        review: None,
        metadata: json!({
            "renewalProvenance": renewal_provenance,
            "leaseRenewalMode": plan.policy.lease_renewal_mode,
            "patch": Value::Object(patch),
            "patchValues": Value::Object(patch_values),
        }),
    });
    Ok(task)
}

pub(crate) fn release_claim_mutation(
    state: &mut CoordinationState,
    meta: EventMeta,
    session_id: &SessionId,
    claim_id: &ClaimId,
) -> Result<WorkClaim> {
    expire_claims_locked(state, meta.ts);
    let claim_snapshot = state
        .claims
        .get(claim_id)
        .cloned()
        .ok_or_else(|| anyhow!("unknown claim `{}`", claim_id.0))?;
    let claim_plan_id = claim_snapshot
        .task
        .as_ref()
        .and_then(|task_id| state.tasks.get(task_id))
        .map(|task| task.plan.clone());
    let current_holder = current_claim_holder(&meta, session_id, &claim_snapshot);
    if claim_snapshot
        .lease_holder
        .as_ref()
        .is_some_and(|lease_holder| !same_holder(lease_holder, &current_holder))
        || (claim_snapshot.lease_holder.is_none() && &claim_snapshot.holder != session_id)
    {
        let violations = vec![policy_violation(
            PolicyViolationCode::ClaimNotOwned,
            format!(
                "claim `{}` is held by `{}` and cannot be released by `{}`",
                claim_id.0, claim_snapshot.holder.0, session_id.0
            ),
            claim_plan_id.clone(),
            claim_snapshot.task.clone(),
            Some(claim_snapshot.id.clone()),
            None,
            Value::Null,
        )];
        return Err(rejection_error(
            state,
            &meta,
            "claim release rejected",
            claim_plan_id,
            claim_snapshot.task.clone(),
            Some(claim_snapshot.id.clone()),
            None,
            violations,
        ));
    }
    let claim = state
        .claims
        .get_mut(claim_id)
        .ok_or_else(|| anyhow!("unknown claim `{}`", claim_id.0))?;
    if claim.status == prism_ir::ClaimStatus::Released {
        return Err(anyhow!("claim `{}` has already been released", claim_id.0));
    }
    claim.status = prism_ir::ClaimStatus::Released;
    let claim = claim.clone();
    let event_meta = meta.clone();
    state.events.push(CoordinationEvent {
        meta: event_meta,
        kind: CoordinationEventKind::ClaimReleased,
        summary: "claim released".to_string(),
        plan: None,
        task: claim.task.clone(),
        claim: Some(claim.id.clone()),
        artifact: None,
        review: None,
        metadata: json!({
            "claim": claim.clone(),
        }),
    });
    if let Some(plan_id) = claim_plan_id.as_ref() {
        auto_complete_execution_plan_if_eligible(state, &meta, plan_id);
    }
    Ok(claim)
}

pub(crate) fn propose_artifact_mutation(
    state: &mut CoordinationState,
    meta: EventMeta,
    input: ArtifactProposeInput,
) -> Result<(prism_ir::ArtifactId, Artifact)> {
    let Some(task) = state.tasks.get(&input.task_id).cloned() else {
        return Err(anyhow!("unknown coordination task `{}`", input.task_id.0));
    };
    let plan = state.plans.get(&task.plan).cloned();
    if let Some(plan) = plan.as_ref() {
        if plan_status_is_closed(plan.status) {
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
                state,
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
        && input.base_revision.graph_version < input.current_revision.graph_version
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
            state,
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
    let id = prism_ir::ArtifactId::new(new_prefixed_id("artifact"));
    let artifact = Artifact {
        id: id.clone(),
        task: input.task_id.clone(),
        worktree_id: input.worktree_id,
        branch_ref: input.branch_ref,
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
            "artifact": artifact.clone(),
        }),
    });
    Ok((id, artifact))
}

pub(crate) fn supersede_artifact_mutation(
    state: &mut CoordinationState,
    meta: EventMeta,
    input: ArtifactSupersedeInput,
) -> Result<Artifact> {
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
        if plan_status_is_closed(plan.status) {
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
                state,
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
        metadata: json!({
            "status": "Superseded",
            "artifact": artifact.clone(),
        }),
    });
    Ok(artifact)
}

pub(crate) fn review_artifact_mutation(
    state: &mut CoordinationState,
    meta: EventMeta,
    input: ArtifactReviewInput,
    current_revision: WorkspaceRevision,
) -> Result<(ReviewId, ArtifactReview, Artifact)> {
    state.next_review += 1;
    let review_id = ReviewId::new(new_prefixed_id("review"));
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
        if plan_status_is_closed(plan.status) {
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
                state,
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
            state,
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
            artifact_mut.required_validations = dedupe_strings(input.required_validations.clone());
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
            state,
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
            "artifact": artifact.clone(),
            "review": review.clone(),
        }),
    });
    Ok((review_id, review, artifact))
}

pub(crate) fn create_plan_mutation(
    state: &mut CoordinationState,
    meta: EventMeta,
    input: PlanCreateInput,
) -> Result<(PlanId, Plan)> {
    state.next_plan += 1;
    let id = PlanId::new(new_prefixed_id("plan"));
    let plan = Plan {
        id: id.clone(),
        goal: input.goal.clone(),
        title: input.title.clone(),
        status: input.status.unwrap_or(PlanStatus::Active),
        policy: input.policy.unwrap_or_default(),
        scope: PlanScope::Repo,
        kind: PlanKind::TaskExecution,
        revision: 0,
        scheduling: PlanScheduling::default(),
        tags: Vec::new(),
        created_from: None,
        metadata: Value::Null,
        authored_edges: Vec::new(),
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
        metadata: json!({
            "plan": plan.clone(),
        }),
    });
    Ok((id, plan))
}

pub(crate) fn update_plan_mutation(
    state: &mut CoordinationState,
    meta: EventMeta,
    input: PlanUpdateInput,
) -> Result<Plan> {
    let mut patch = serde_json::Map::new();
    if input.title.is_some() {
        push_patch_op(&mut patch, "title", "set");
    }
    if input.status.is_some() {
        push_patch_op(&mut patch, "status", "set");
    }
    if input.goal.is_some() {
        push_patch_op(&mut patch, "goal", "set");
    }
    if input.policy.is_some() {
        push_patch_op(&mut patch, "policy", "set");
    }
    let patch = patch_metadata(patch);
    let previous = state
        .plans
        .get(&input.plan_id)
        .cloned()
        .ok_or_else(|| anyhow!("unknown plan `{}`", input.plan_id.0))?;
    if plan_status_is_closed(previous.status)
        && (input.title.is_some() || input.goal.is_some() || input.policy.is_some())
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
            state,
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
                state,
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
        let violations = plan_completion_violations(state, &input.plan_id, meta.ts, true);
        if !violations.is_empty() {
            return Err(rejection_error(
                state,
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
    let update_title = input.title.is_some();
    let update_goal = input.goal.is_some();
    let update_status = input.status.is_some();
    let update_policy = input.policy.is_some();
    if let Some(title) = input.title {
        plan.title = title;
    }
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
    let mut metadata = serde_json::Map::new();
    metadata.insert(
        "status".to_string(),
        Value::String(format!("{:?}", plan.status)),
    );
    metadata.insert(
        "previousStatus".to_string(),
        Value::String(format!("{:?}", previous.status)),
    );
    if let Some(patch) = patch {
        metadata.insert("patch".to_string(), patch);
    }
    let mut patch_values = serde_json::Map::new();
    if update_title {
        insert_serialized(&mut patch_values, "title", plan.title.clone());
    }
    if update_goal {
        insert_serialized(&mut patch_values, "goal", plan.goal.clone());
    }
    if update_status {
        insert_serialized(&mut patch_values, "status", plan.status);
    }
    if update_policy {
        insert_serialized(&mut patch_values, "policy", plan.policy.clone());
    }
    if !patch_values.is_empty() {
        metadata.insert("patchValues".to_string(), Value::Object(patch_values));
    }
    state.events.push(CoordinationEvent {
        meta,
        kind: CoordinationEventKind::PlanUpdated,
        summary: plan.goal.clone(),
        plan: Some(plan.id.clone()),
        task: None,
        claim: None,
        artifact: None,
        review: None,
        metadata: Value::Object(metadata),
    });
    Ok(plan)
}

pub(crate) fn set_plan_scheduling_mutation(
    state: &mut CoordinationState,
    meta: EventMeta,
    plan_id: PlanId,
    scheduling: PlanScheduling,
) -> Result<Plan> {
    let previous = state
        .plans
        .get(&plan_id)
        .cloned()
        .ok_or_else(|| anyhow!("unknown plan `{}`", plan_id.0))?;
    if plan_status_is_closed(previous.status) {
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
            state,
            &meta,
            "coordination plan update rejected",
            Some(previous.id),
            None,
            None,
            None,
            violations,
        ));
    }
    let plan = state.plans.get_mut(&plan_id).expect("plan validated above");
    plan.scheduling = scheduling;
    let plan = plan.clone();
    let mut patch = serde_json::Map::new();
    push_patch_op(&mut patch, "scheduling", "set");
    let mut patch_values = serde_json::Map::new();
    insert_serialized(&mut patch_values, "scheduling", plan.scheduling.clone());
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
            "previousStatus": format!("{:?}", previous.status),
            "patch": patch,
            "patchValues": patch_values,
        }),
    });
    Ok(plan)
}

pub(crate) fn create_task_mutation(
    state: &mut CoordinationState,
    meta: EventMeta,
    input: TaskCreateInput,
) -> Result<(CoordinationTaskId, CoordinationTask)> {
    let Some(plan) = state.plans.get(&input.plan_id).cloned() else {
        return Err(anyhow!("unknown plan `{}`", input.plan_id.0));
    };
    if plan_status_is_closed(plan.status) {
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
            state,
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
                state,
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
                state,
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
    let id = CoordinationTaskId::new(new_prefixed_id("coord-task"));
    let is_root = input.depends_on.is_empty();
    let anchors = dedupe_anchors(input.anchors);
    let mut task = CoordinationTask {
        id: id.clone(),
        plan: input.plan_id.clone(),
        kind: PlanNodeKind::Edit,
        title: input.title.clone(),
        summary: None,
        status: input.status.unwrap_or(CoordinationTaskStatus::Ready),
        published_task_status: None,
        assignee: input.assignee,
        pending_handoff_to: None,
        session: input.session,
        lease_holder: None,
        lease_started_at: None,
        lease_refreshed_at: None,
        lease_stale_at: None,
        lease_expires_at: None,
        worktree_id: input.worktree_id,
        branch_ref: input.branch_ref,
        anchors: anchors.clone(),
        bindings: PlanBinding {
            anchors,
            ..PlanBinding::default()
        },
        depends_on: dedupe_ids(input.depends_on),
        acceptance: normalize_acceptance(input.acceptance),
        validation_refs: Vec::new(),
        is_abstract: false,
        base_revision: input.base_revision,
        priority: None,
        tags: Vec::new(),
        metadata: Value::Null,
        git_execution: crate::TaskGitExecution::default(),
    };
    if !matches!(task.status, CoordinationTaskStatus::Proposed) {
        refresh_task_lease(&mut task, &meta, meta.ts, &plan.policy);
    }
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
        metadata: json!({
            "task": task.clone(),
        }),
    });
    Ok((id, task))
}

pub(crate) fn update_task_mutation(
    state: &mut CoordinationState,
    meta: EventMeta,
    input: TaskUpdateInput,
    current_revision: WorkspaceRevision,
    now: Timestamp,
) -> Result<CoordinationTask> {
    update_task_mutation_with_options(state, meta, input, current_revision, now, false)
}

pub(crate) fn update_task_mutation_with_options(
    state: &mut CoordinationState,
    meta: EventMeta,
    input: TaskUpdateInput,
    current_revision: WorkspaceRevision,
    now: Timestamp,
    authoritative_only: bool,
) -> Result<CoordinationTask> {
    let update_kind = input.kind.is_some();
    let update_status = input.status.is_some();
    let update_assignee = input.assignee.is_some();
    let update_session = input.session.is_some();
    let update_worktree = input.worktree_id.is_some();
    let update_branch = input.branch_ref.is_some();
    let update_title = input.title.is_some();
    let update_summary = input.summary.is_some();
    let update_anchors = input.anchors.is_some();
    let update_bindings = input.bindings.is_some();
    let update_depends_on = input.depends_on.is_some();
    let update_acceptance = input.acceptance.is_some();
    let update_validation_refs = input.validation_refs.is_some();
    let update_is_abstract = input.is_abstract.is_some();
    let update_base_revision = input.base_revision.is_some();
    let update_priority = input.priority.is_some();
    let update_tags = input.tags.is_some();
    let update_published_task_status = input.published_task_status.is_some();
    let git_execution_only_update = input.git_execution.is_some()
        && input.kind.is_none()
        && input.status.is_none()
        && input.published_task_status.is_none()
        && input.assignee.is_none()
        && input.session.is_none()
        && input.worktree_id.is_none()
        && input.branch_ref.is_none()
        && input.title.is_none()
        && input.summary.is_none()
        && input.anchors.is_none()
        && input.bindings.is_none()
        && input.depends_on.is_none()
        && input.acceptance.is_none()
        && input.validation_refs.is_none()
        && input.is_abstract.is_none()
        && input.priority.is_none()
        && input.tags.is_none()
        && input.completion_context.is_none();
    let mut patch = serde_json::Map::new();
    if input.kind.is_some() {
        push_patch_op(&mut patch, "kind", "set");
    }
    if input.status.is_some() {
        push_patch_op(&mut patch, "status", "set");
    }
    if let Some(published_task_status) = input.published_task_status.as_ref() {
        push_patch_op(
            &mut patch,
            "publishedTaskStatus",
            if published_task_status.is_some() {
                "set"
            } else {
                "clear"
            },
        );
    }
    if input.git_execution.is_some() {
        push_patch_op(&mut patch, "gitExecution", "set");
    }
    if let Some(assignee) = input.assignee.as_ref() {
        push_patch_op(
            &mut patch,
            "assignee",
            if assignee.is_some() { "set" } else { "clear" },
        );
    }
    if let Some(session) = input.session.as_ref() {
        push_patch_op(
            &mut patch,
            "session",
            if session.is_some() { "set" } else { "clear" },
        );
    }
    if let Some(worktree_id) = input.worktree_id.as_ref() {
        push_patch_op(
            &mut patch,
            "worktreeId",
            if worktree_id.is_some() {
                "set"
            } else {
                "clear"
            },
        );
    }
    if let Some(branch_ref) = input.branch_ref.as_ref() {
        push_patch_op(
            &mut patch,
            "branchRef",
            if branch_ref.is_some() { "set" } else { "clear" },
        );
    }
    if input.title.is_some() {
        push_patch_op(&mut patch, "title", "set");
    }
    if let Some(summary) = input.summary.as_ref() {
        push_patch_op(
            &mut patch,
            "summary",
            if summary.is_some() { "set" } else { "clear" },
        );
    }
    if input.anchors.is_some() {
        push_patch_op(&mut patch, "anchors", "set");
    }
    if input.bindings.is_some() {
        push_patch_op(&mut patch, "bindings", "set");
    }
    if input.depends_on.is_some() {
        push_patch_op(&mut patch, "dependsOn", "set");
    }
    if input.acceptance.is_some() {
        push_patch_op(&mut patch, "acceptance", "set");
    }
    if input.validation_refs.is_some() {
        push_patch_op(&mut patch, "validationRefs", "set");
    }
    if input.is_abstract.is_some() {
        push_patch_op(&mut patch, "isAbstract", "set");
    }
    if input.base_revision.is_some() {
        push_patch_op(&mut patch, "baseRevision", "set");
    }
    if let Some(priority) = input.priority.as_ref() {
        push_patch_op(
            &mut patch,
            "priority",
            if priority.is_some() { "set" } else { "clear" },
        );
    }
    if input.tags.is_some() {
        push_patch_op(&mut patch, "tags", "set");
    }
    let completion_context = input.completion_context.clone();
    let next_dependencies = input.depends_on.clone().map(dedupe_ids);
    let next_acceptance = input.acceptance.clone().map(normalize_acceptance);
    let previous = state
        .tasks
        .get(&input.task_id)
        .cloned()
        .ok_or_else(|| anyhow!("unknown coordination task `{}`", input.task_id.0))?;
    let Some(plan) = state.plans.get(&previous.plan).cloned() else {
        return Err(anyhow!("unknown plan `{}`", previous.plan.0));
    };
    if plan_status_is_closed(plan.status) && !git_execution_only_update {
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
            state,
            &meta,
            "coordination task update rejected",
            Some(plan.id),
            Some(previous.id),
            None,
            None,
            violations,
        ));
    }
    enforce_task_lease_for_standard_mutation(
        state,
        &meta,
        &previous,
        "coordination task update rejected",
    )?;
    let stale_writes_enforced = state
        .plans
        .get(&previous.plan)
        .map(|plan| plan.policy.stale_after_graph_change)
        .unwrap_or(false);
    if let Some(dependencies) = next_dependencies.as_ref() {
        validate_task_dependencies(state, &previous.plan, &previous.id, dependencies, &meta)?;
    }
    let task_snapshot;
    let status_changed;
    let mut root_membership_change = None;
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
                state,
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
            if stale_writes_enforced && base_revision.graph_version < current_revision.graph_version
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
                    state,
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
            let skip_transition_validation = authoritative_only && input.git_execution.is_some();
            if !skip_transition_validation {
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
                        state,
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
        }
        if matches!(
            previous.status,
            CoordinationTaskStatus::Completed | CoordinationTaskStatus::Abandoned
        ) && (input.title.is_some()
            || input.summary.is_some()
            || input.anchors.is_some()
            || input.bindings.is_some()
            || input.depends_on.is_some()
            || input.acceptance.is_some()
            || input.validation_refs.is_some()
            || input.is_abstract.is_some()
            || input.kind.is_some()
            || input.priority.is_some()
            || input.tags.is_some()
            || input.assignee.is_some()
            || input.session.is_some())
            && !git_execution_only_update
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
                state,
                &meta,
                "coordination task update rejected",
                Some(previous.plan.clone()),
                Some(previous.id.clone()),
                None,
                None,
                violations,
            ));
        }
        if previous.pending_handoff_to.is_some()
            && (input.title.is_some()
                || input.summary.is_some()
                || input.anchors.is_some()
                || input.bindings.is_some()
                || input.depends_on.is_some()
                || input.acceptance.is_some()
                || input.validation_refs.is_some()
                || input.is_abstract.is_some()
                || input.kind.is_some()
                || input.priority.is_some()
                || input.tags.is_some()
                || input.assignee.is_some()
                || input.session.is_some()
                || input.status.is_some())
            && !git_execution_only_update
        {
            let violations = vec![policy_violation(
                PolicyViolationCode::HandoffPending,
                format!(
                    "coordination task `{}` has a pending handoff and cannot be updated until it is accepted",
                    previous.id.0
                ),
                Some(previous.plan.clone()),
                Some(previous.id.clone()),
                None,
                None,
                Value::Null,
            )];
            return Err(rejection_error(
                state,
                &meta,
                "coordination task update rejected",
                Some(previous.plan.clone()),
                Some(previous.id.clone()),
                None,
                None,
                violations,
            ));
        }
        if let Some(kind) = input.kind {
            task.kind = kind;
        }
        if let Some(title) = input.title {
            task.title = title;
        }
        if let Some(summary) = input.summary {
            task.summary = summary;
        }
        if let Some(status) = input.status {
            task.status = status;
        }
        if let Some(published_task_status) = input.published_task_status {
            task.published_task_status = published_task_status;
        }
        if let Some(git_execution) = input.git_execution.clone() {
            task.git_execution = git_execution;
        }
        if let Some(assignee) = input.assignee {
            task.assignee = assignee;
        }
        if let Some(session) = input.session {
            task.session = session;
        }
        if let Some(worktree_id) = input.worktree_id {
            task.worktree_id = worktree_id;
        }
        if let Some(branch_ref) = input.branch_ref {
            task.branch_ref = branch_ref;
        }
        if let Some(anchors) = input.anchors {
            task.anchors = dedupe_anchors(anchors);
            task.bindings.anchors = task.anchors.clone();
        }
        if let Some(bindings) = input.bindings {
            task.bindings = bindings;
        }
        if let Some(depends_on) = next_dependencies.clone() {
            let previous_root = task.depends_on.is_empty();
            let next_root = depends_on.is_empty();
            task.depends_on = depends_on;
            if previous_root != next_root {
                root_membership_change = Some(next_root);
            }
        }
        if let Some(acceptance) = next_acceptance.clone() {
            task.acceptance = acceptance;
        }
        if let Some(validation_refs) = input.validation_refs {
            task.validation_refs = dedupe_strings(
                validation_refs
                    .into_iter()
                    .map(|validation| validation.id)
                    .collect::<Vec<_>>(),
            )
            .into_iter()
            .map(|id| prism_ir::ValidationRef { id })
            .collect();
        }
        if let Some(is_abstract) = input.is_abstract {
            task.is_abstract = is_abstract;
        }
        if let Some(base_revision) = input.base_revision {
            task.base_revision = base_revision;
        }
        if let Some(priority) = input.priority {
            task.priority = priority;
        }
        if let Some(tags) = input.tags {
            task.tags = dedupe_strings(tags);
        }
        if task.bindings.anchors.is_empty() && !task.anchors.is_empty() {
            task.bindings.anchors = task.anchors.clone();
        }
        if matches!(
            task.status,
            CoordinationTaskStatus::Completed | CoordinationTaskStatus::Abandoned
        ) {
            clear_task_lease(task);
        } else {
            refresh_task_lease(task, &meta, meta.ts, &plan.policy);
        }
        task_snapshot = task.clone();
    }
    if let Some(next_root) = root_membership_change {
        let plan = state
            .plans
            .get_mut(&previous.plan)
            .expect("task plan validated above");
        if next_root {
            plan.root_tasks.push(previous.id.clone());
            plan.root_tasks = dedupe_ids(plan.root_tasks.clone());
        } else {
            plan.root_tasks.retain(|task_id| task_id != &previous.id);
        }
    }
    let completion_candidate_status = task_snapshot
        .published_task_status
        .unwrap_or(task_snapshot.status);
    let completion_candidate = if completion_candidate_status == task_snapshot.status {
        task_snapshot.clone()
    } else {
        let mut candidate = task_snapshot.clone();
        candidate.status = completion_candidate_status;
        candidate
    };
    let completion_blockers =
        completion_blockers(state, &completion_candidate, current_revision.clone(), now);
    let mut policy_blockers = if completion_candidate.status == CoordinationTaskStatus::Completed {
        completion_policy_blockers(
            state,
            &completion_candidate,
            current_revision,
            completion_context.as_ref(),
        )
    } else {
        Vec::new()
    };
    if completion_candidate.status == CoordinationTaskStatus::Completed
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
            state,
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
    if previous.lease_holder != task.lease_holder {
        push_patch_op(
            &mut patch,
            "leaseHolder",
            if task.lease_holder.is_some() {
                "set"
            } else {
                "clear"
            },
        );
    }
    if previous.lease_started_at != task.lease_started_at {
        push_patch_op(
            &mut patch,
            "leaseStartedAt",
            if task.lease_started_at.is_some() {
                "set"
            } else {
                "clear"
            },
        );
    }
    if previous.lease_refreshed_at != task.lease_refreshed_at {
        push_patch_op(
            &mut patch,
            "leaseRefreshedAt",
            if task.lease_refreshed_at.is_some() {
                "set"
            } else {
                "clear"
            },
        );
    }
    if previous.lease_stale_at != task.lease_stale_at {
        push_patch_op(
            &mut patch,
            "leaseStaleAt",
            if task.lease_stale_at.is_some() {
                "set"
            } else {
                "clear"
            },
        );
    }
    if previous.lease_expires_at != task.lease_expires_at {
        push_patch_op(
            &mut patch,
            "leaseExpiresAt",
            if task.lease_expires_at.is_some() {
                "set"
            } else {
                "clear"
            },
        );
    }
    let patch = patch_metadata(patch);
    let kind = if previous.assignee != task.assignee {
        CoordinationEventKind::TaskAssigned
    } else if status_changed && task.status == CoordinationTaskStatus::Blocked {
        CoordinationEventKind::TaskBlocked
    } else if previous.status == CoordinationTaskStatus::Blocked && status_changed {
        CoordinationEventKind::TaskUnblocked
    } else {
        CoordinationEventKind::TaskStatusChanged
    };
    let mut metadata = serde_json::Map::new();
    metadata.insert(
        "status".to_string(),
        Value::String(format!("{:?}", task.status)),
    );
    metadata.insert(
        "previousStatus".to_string(),
        Value::String(format!("{:?}", previous.status)),
    );
    metadata.insert(
        "assignee".to_string(),
        task.assignee
            .as_ref()
            .map(|agent| Value::String(agent.0.to_string()))
            .unwrap_or(Value::Null),
    );
    if authoritative_only || git_execution_only_update {
        metadata.insert("authoritativeOnly".to_string(), Value::Bool(true));
    }
    if let Some(patch) = patch {
        metadata.insert("patch".to_string(), patch);
    }
    let mut patch_values = serde_json::Map::new();
    if update_kind {
        insert_serialized(&mut patch_values, "kind", task.kind);
    }
    if update_status {
        insert_serialized(&mut patch_values, "status", task.status);
    }
    if update_published_task_status {
        insert_serialized(
            &mut patch_values,
            "publishedTaskStatus",
            task.published_task_status,
        );
    }
    if input.git_execution.is_some() {
        insert_serialized(
            &mut patch_values,
            "gitExecution",
            task.git_execution.clone(),
        );
    }
    if update_assignee {
        insert_serialized(&mut patch_values, "assignee", task.assignee.clone());
    }
    if update_session {
        insert_serialized(&mut patch_values, "session", task.session.clone());
    }
    if update_worktree {
        insert_serialized(&mut patch_values, "worktreeId", task.worktree_id.clone());
    }
    if update_branch {
        insert_serialized(&mut patch_values, "branchRef", task.branch_ref.clone());
    }
    if update_title {
        insert_serialized(&mut patch_values, "title", task.title.clone());
    }
    if update_summary {
        insert_serialized(&mut patch_values, "summary", task.summary.clone());
    }
    if update_anchors {
        insert_serialized(&mut patch_values, "anchors", task.anchors.clone());
    }
    if update_bindings {
        insert_serialized(&mut patch_values, "bindings", task.bindings.clone());
    }
    if update_depends_on {
        insert_serialized(&mut patch_values, "dependsOn", task.depends_on.clone());
    }
    if update_acceptance {
        insert_serialized(&mut patch_values, "acceptance", task.acceptance.clone());
    }
    if update_validation_refs {
        insert_serialized(
            &mut patch_values,
            "validationRefs",
            task.validation_refs.clone(),
        );
    }
    if update_is_abstract {
        insert_serialized(&mut patch_values, "isAbstract", task.is_abstract);
    }
    if update_base_revision {
        insert_serialized(
            &mut patch_values,
            "baseRevision",
            task.base_revision.clone(),
        );
    }
    if update_priority {
        insert_serialized(&mut patch_values, "priority", task.priority);
    }
    if update_tags {
        insert_serialized(&mut patch_values, "tags", task.tags.clone());
    }
    if previous.lease_holder != task.lease_holder {
        insert_serialized(&mut patch_values, "leaseHolder", task.lease_holder.clone());
    }
    if previous.lease_started_at != task.lease_started_at {
        insert_serialized(&mut patch_values, "leaseStartedAt", task.lease_started_at);
    }
    if previous.lease_refreshed_at != task.lease_refreshed_at {
        insert_serialized(
            &mut patch_values,
            "leaseRefreshedAt",
            task.lease_refreshed_at,
        );
    }
    if previous.lease_stale_at != task.lease_stale_at {
        insert_serialized(&mut patch_values, "leaseStaleAt", task.lease_stale_at);
    }
    if previous.lease_expires_at != task.lease_expires_at {
        insert_serialized(&mut patch_values, "leaseExpiresAt", task.lease_expires_at);
    }
    if !patch_values.is_empty() {
        metadata.insert("patchValues".to_string(), Value::Object(patch_values));
    }
    let event_meta = meta.clone();
    state.events.push(CoordinationEvent {
        meta: event_meta,
        kind,
        summary: task.title.clone(),
        plan: Some(task.plan.clone()),
        task: Some(task.id.clone()),
        claim: None,
        artifact: None,
        review: None,
        metadata: Value::Object(metadata),
    });
    if !authoritative_only {
        auto_complete_execution_plan_if_eligible(state, &meta, &task.plan);
    }
    Ok(task)
}

pub(crate) fn handoff_mutation(
    state: &mut CoordinationState,
    meta: EventMeta,
    input: HandoffInput,
    current_revision: WorkspaceRevision,
) -> Result<CoordinationTask> {
    let plan = {
        let task = state
            .tasks
            .get(&input.task_id)
            .ok_or_else(|| anyhow!("unknown coordination task `{}`", input.task_id.0))?;
        state.plans.get(&task.plan).cloned()
    };
    if let Some(plan) = plan.as_ref() {
        if plan_status_is_closed(plan.status) {
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
                state,
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
    let task_for_lease = state
        .tasks
        .get(&input.task_id)
        .cloned()
        .expect("task validated above");
    enforce_task_lease_for_standard_mutation(
        state,
        &meta,
        &task_for_lease,
        "coordination handoff rejected",
    )?;
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
            state,
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
                state,
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
    let previous = task.clone();
    let target_agent = input.to_agent.clone();
    if let Some(agent) = target_agent.clone() {
        task.pending_handoff_to = Some(agent.clone());
        task.status = CoordinationTaskStatus::Blocked;
    } else {
        task.assignee = None;
        task.session = None;
        task.worktree_id = None;
        task.branch_ref = None;
        task.status = CoordinationTaskStatus::Ready;
        task.pending_handoff_to = None;
    }
    clear_task_lease(task);
    task.base_revision = input.base_revision.clone();
    let task = task.clone();
    let mut patch = serde_json::Map::new();
    push_patch_op(&mut patch, "status", "set");
    push_patch_op(
        &mut patch,
        "pendingHandoffTo",
        if task.pending_handoff_to.is_some() {
            "set"
        } else {
            "clear"
        },
    );
    push_patch_op(&mut patch, "baseRevision", "set");
    if previous.lease_holder != task.lease_holder {
        push_patch_op(&mut patch, "leaseHolder", "clear");
    }
    if previous.lease_started_at != task.lease_started_at {
        push_patch_op(&mut patch, "leaseStartedAt", "clear");
    }
    if previous.lease_refreshed_at != task.lease_refreshed_at {
        push_patch_op(&mut patch, "leaseRefreshedAt", "clear");
    }
    if previous.lease_stale_at != task.lease_stale_at {
        push_patch_op(&mut patch, "leaseStaleAt", "clear");
    }
    if previous.lease_expires_at != task.lease_expires_at {
        push_patch_op(&mut patch, "leaseExpiresAt", "clear");
    }
    if target_agent.is_none() {
        push_patch_op(&mut patch, "assignee", "clear");
        push_patch_op(&mut patch, "session", "clear");
        push_patch_op(&mut patch, "worktreeId", "clear");
        push_patch_op(&mut patch, "branchRef", "clear");
    }
    let mut patch_values = serde_json::Map::new();
    insert_serialized(&mut patch_values, "status", task.status);
    insert_serialized(
        &mut patch_values,
        "pendingHandoffTo",
        task.pending_handoff_to.clone(),
    );
    insert_serialized(
        &mut patch_values,
        "baseRevision",
        task.base_revision.clone(),
    );
    if previous.lease_holder != task.lease_holder {
        insert_serialized(&mut patch_values, "leaseHolder", task.lease_holder.clone());
    }
    if previous.lease_started_at != task.lease_started_at {
        insert_serialized(&mut patch_values, "leaseStartedAt", task.lease_started_at);
    }
    if previous.lease_refreshed_at != task.lease_refreshed_at {
        insert_serialized(
            &mut patch_values,
            "leaseRefreshedAt",
            task.lease_refreshed_at,
        );
    }
    if previous.lease_stale_at != task.lease_stale_at {
        insert_serialized(&mut patch_values, "leaseStaleAt", task.lease_stale_at);
    }
    if previous.lease_expires_at != task.lease_expires_at {
        insert_serialized(&mut patch_values, "leaseExpiresAt", task.lease_expires_at);
    }
    if target_agent.is_none() {
        insert_serialized(&mut patch_values, "assignee", task.assignee.clone());
        insert_serialized(&mut patch_values, "session", task.session.clone());
        insert_serialized(&mut patch_values, "worktreeId", task.worktree_id.clone());
        insert_serialized(&mut patch_values, "branchRef", task.branch_ref.clone());
    }
    state.events.push(CoordinationEvent {
        meta: meta.clone(),
        kind: CoordinationEventKind::HandoffRequested,
        summary: input.summary.clone(),
        plan: Some(task.plan.clone()),
        task: Some(task.id.clone()),
        claim: None,
        artifact: None,
        review: None,
        metadata: json!({
            "to_agent": target_agent.map(|agent| agent.0.to_string()),
            "patch": Value::Object(patch),
            "patchValues": Value::Object(patch_values),
        }),
    });
    Ok(task)
}

pub(crate) fn accept_handoff_mutation(
    state: &mut CoordinationState,
    meta: EventMeta,
    input: HandoffAcceptInput,
) -> Result<CoordinationTask> {
    let previous = state
        .tasks
        .get(&input.task_id)
        .cloned()
        .ok_or_else(|| anyhow!("unknown coordination task `{}`", input.task_id.0))?;
    let Some(plan) = state.plans.get(&previous.plan).cloned() else {
        return Err(anyhow!("unknown plan `{}`", previous.plan.0));
    };
    if plan_status_is_closed(plan.status) {
        let violations = vec![policy_violation(
            PolicyViolationCode::PlanClosed,
            format!(
                "coordination plan `{}` is {:?} and cannot accept handoffs",
                plan.id.0, plan.status
            ),
            Some(plan.id.clone()),
            Some(previous.id.clone()),
            None,
            None,
            Value::Null,
        )];
        return Err(rejection_error(
            state,
            &meta,
            "handoff acceptance rejected",
            Some(plan.id),
            Some(previous.id),
            None,
            None,
            violations,
        ));
    }
    let Some(target) = previous.pending_handoff_to.clone() else {
        let violations = vec![policy_violation(
            PolicyViolationCode::HandoffPending,
            format!(
                "coordination task `{}` does not have a pending handoff",
                previous.id.0
            ),
            Some(previous.plan.clone()),
            Some(previous.id.clone()),
            None,
            None,
            Value::Null,
        )];
        return Err(rejection_error(
            state,
            &meta,
            "handoff acceptance rejected",
            Some(previous.plan),
            Some(previous.id),
            None,
            None,
            violations,
        ));
    };
    let Some(actor) = input.agent.clone() else {
        let violations = vec![policy_violation(
            PolicyViolationCode::AgentIdentityRequired,
            format!(
                "coordination task `{}` requires an acting agent identity to accept a handoff",
                previous.id.0
            ),
            Some(previous.plan.clone()),
            Some(previous.id.clone()),
            None,
            None,
            Value::Null,
        )];
        return Err(rejection_error(
            state,
            &meta,
            "handoff acceptance rejected",
            Some(previous.plan),
            Some(previous.id),
            None,
            None,
            violations,
        ));
    };
    if actor != target {
        let violations = vec![policy_violation(
            PolicyViolationCode::HandoffTargetMismatch,
            format!(
                "handoff for task `{}` is assigned to `{}` and cannot be accepted by `{}`",
                previous.id.0, target.0, actor.0
            ),
            Some(previous.plan.clone()),
            Some(previous.id.clone()),
            None,
            None,
            json!({
                "expectedAgent": target.0,
                "providedAgent": actor.0.to_string(),
            }),
        )];
        return Err(rejection_error(
            state,
            &meta,
            "handoff acceptance rejected",
            Some(previous.plan),
            Some(previous.id),
            None,
            None,
            violations,
        ));
    }
    let task = state
        .tasks
        .get_mut(&input.task_id)
        .expect("task validated above");
    task.assignee = Some(target.clone());
    task.pending_handoff_to = None;
    task.session = None;
    task.worktree_id = input.worktree_id;
    task.branch_ref = input.branch_ref;
    task.status = CoordinationTaskStatus::Ready;
    refresh_task_lease(task, &meta, meta.ts, &plan.policy);
    let task = task.clone();
    let mut patch = serde_json::Map::new();
    push_patch_op(&mut patch, "assignee", "set");
    push_patch_op(&mut patch, "pendingHandoffTo", "clear");
    push_patch_op(&mut patch, "session", "clear");
    push_patch_op(
        &mut patch,
        "worktreeId",
        if task.worktree_id.is_some() {
            "set"
        } else {
            "clear"
        },
    );
    push_patch_op(
        &mut patch,
        "branchRef",
        if task.branch_ref.is_some() {
            "set"
        } else {
            "clear"
        },
    );
    push_patch_op(&mut patch, "status", "set");
    push_patch_op(&mut patch, "leaseHolder", "set");
    push_patch_op(&mut patch, "leaseStartedAt", "set");
    push_patch_op(&mut patch, "leaseRefreshedAt", "set");
    push_patch_op(&mut patch, "leaseStaleAt", "set");
    push_patch_op(&mut patch, "leaseExpiresAt", "set");
    let mut patch_values = serde_json::Map::new();
    insert_serialized(&mut patch_values, "assignee", task.assignee.clone());
    insert_serialized(
        &mut patch_values,
        "pendingHandoffTo",
        task.pending_handoff_to.clone(),
    );
    insert_serialized(&mut patch_values, "session", task.session.clone());
    insert_serialized(&mut patch_values, "worktreeId", task.worktree_id.clone());
    insert_serialized(&mut patch_values, "branchRef", task.branch_ref.clone());
    insert_serialized(&mut patch_values, "status", task.status);
    insert_serialized(&mut patch_values, "leaseHolder", task.lease_holder.clone());
    insert_serialized(&mut patch_values, "leaseStartedAt", task.lease_started_at);
    insert_serialized(
        &mut patch_values,
        "leaseRefreshedAt",
        task.lease_refreshed_at,
    );
    insert_serialized(&mut patch_values, "leaseStaleAt", task.lease_stale_at);
    insert_serialized(&mut patch_values, "leaseExpiresAt", task.lease_expires_at);
    state.events.push(CoordinationEvent {
        meta: derived_event_meta(&meta, "accepted"),
        kind: CoordinationEventKind::HandoffAccepted,
        summary: format!("handoff accepted by `{}`", target.0),
        plan: Some(task.plan.clone()),
        task: Some(task.id.clone()),
        claim: None,
        artifact: None,
        review: None,
        metadata: json!({
            "agent": target.0.to_string(),
            "patch": Value::Object(patch),
            "patchValues": Value::Object(patch_values),
        }),
    });
    Ok(task)
}

pub(crate) fn resume_task_mutation(
    state: &mut CoordinationState,
    meta: EventMeta,
    input: TaskResumeInput,
) -> Result<CoordinationTask> {
    let previous = state
        .tasks
        .get(&input.task_id)
        .cloned()
        .ok_or_else(|| anyhow!("unknown coordination task `{}`", input.task_id.0))?;
    let Some(plan) = state.plans.get(&previous.plan).cloned() else {
        return Err(anyhow!("unknown plan `{}`", previous.plan.0));
    };
    if previous.pending_handoff_to.is_some() {
        let violations = vec![policy_violation(
            PolicyViolationCode::HandoffPending,
            format!(
                "coordination task `{}` has a pending handoff and cannot be resumed",
                previous.id.0
            ),
            Some(previous.plan.clone()),
            Some(previous.id.clone()),
            None,
            None,
            Value::Null,
        )];
        return Err(rejection_error(
            state,
            &meta,
            "coordination task resume rejected",
            Some(previous.plan.clone()),
            Some(previous.id.clone()),
            None,
            None,
            violations,
        ));
    }
    let lease_state = task_lease_state(&previous, meta.ts);
    if !matches!(lease_state, LeaseState::Stale | LeaseState::Expired) {
        return Err(anyhow!(
            "coordination task `{}` does not have a stale or expired lease to resume",
            previous.id.0
        ));
    }
    let current_holder = current_task_holder(&meta, &previous);
    if authoritative_task_holder(&previous)
        .as_ref()
        .is_none_or(|lease_holder| !same_holder(lease_holder, &current_holder))
    {
        let violations = vec![policy_violation(
            PolicyViolationCode::TaskResumeRequired,
            format!(
                "coordination task `{}` cannot be resumed by a different principal",
                previous.id.0
            ),
            Some(previous.plan.clone()),
            Some(previous.id.clone()),
            None,
            None,
            json!({
                "leaseState": format!("{lease_state:?}").to_ascii_lowercase(),
                "leaseHolder": lease_holder_details(previous.lease_holder.as_ref()),
                "currentHolder": lease_holder_details(Some(&current_holder)),
            }),
        )];
        return Err(rejection_error(
            state,
            &meta,
            "coordination task resume rejected",
            Some(previous.plan.clone()),
            Some(previous.id.clone()),
            None,
            None,
            violations,
        ));
    }

    let task = state
        .tasks
        .get_mut(&input.task_id)
        .expect("task validated above");
    if let Some(agent) = input.agent {
        task.assignee = Some(agent);
    }
    task.session = current_holder.session_id.clone();
    task.worktree_id = input.worktree_id;
    task.branch_ref = input.branch_ref;
    refresh_task_lease(task, &meta, meta.ts, &plan.policy);
    let task = task.clone();

    let mut patch = serde_json::Map::new();
    if previous.assignee != task.assignee {
        push_patch_op(
            &mut patch,
            "assignee",
            if task.assignee.is_some() {
                "set"
            } else {
                "clear"
            },
        );
    }
    if previous.session != task.session {
        push_patch_op(
            &mut patch,
            "session",
            if task.session.is_some() {
                "set"
            } else {
                "clear"
            },
        );
    }
    if previous.worktree_id != task.worktree_id {
        push_patch_op(
            &mut patch,
            "worktreeId",
            if task.worktree_id.is_some() {
                "set"
            } else {
                "clear"
            },
        );
    }
    if previous.branch_ref != task.branch_ref {
        push_patch_op(
            &mut patch,
            "branchRef",
            if task.branch_ref.is_some() {
                "set"
            } else {
                "clear"
            },
        );
    }
    push_patch_op(&mut patch, "leaseHolder", "set");
    push_patch_op(&mut patch, "leaseStartedAt", "set");
    push_patch_op(&mut patch, "leaseRefreshedAt", "set");
    push_patch_op(&mut patch, "leaseStaleAt", "set");
    push_patch_op(&mut patch, "leaseExpiresAt", "set");

    let mut patch_values = serde_json::Map::new();
    if previous.assignee != task.assignee {
        insert_serialized(&mut patch_values, "assignee", task.assignee.clone());
    }
    if previous.session != task.session {
        insert_serialized(&mut patch_values, "session", task.session.clone());
    }
    if previous.worktree_id != task.worktree_id {
        insert_serialized(&mut patch_values, "worktreeId", task.worktree_id.clone());
    }
    if previous.branch_ref != task.branch_ref {
        insert_serialized(&mut patch_values, "branchRef", task.branch_ref.clone());
    }
    insert_serialized(&mut patch_values, "leaseHolder", task.lease_holder.clone());
    insert_serialized(&mut patch_values, "leaseStartedAt", task.lease_started_at);
    insert_serialized(
        &mut patch_values,
        "leaseRefreshedAt",
        task.lease_refreshed_at,
    );
    insert_serialized(&mut patch_values, "leaseStaleAt", task.lease_stale_at);
    insert_serialized(&mut patch_values, "leaseExpiresAt", task.lease_expires_at);

    state.events.push(CoordinationEvent {
        meta: meta.clone(),
        kind: CoordinationEventKind::TaskResumed,
        summary: format!("task `{}` resumed", task.id.0),
        plan: Some(task.plan.clone()),
        task: Some(task.id.clone()),
        claim: None,
        artifact: None,
        review: None,
        metadata: json!({
            "leaseState": format!("{lease_state:?}").to_ascii_lowercase(),
            "patch": Value::Object(patch),
            "patchValues": Value::Object(patch_values),
        }),
    });
    Ok(task)
}

pub(crate) fn reclaim_task_mutation(
    state: &mut CoordinationState,
    meta: EventMeta,
    input: TaskReclaimInput,
) -> Result<CoordinationTask> {
    let previous = state
        .tasks
        .get(&input.task_id)
        .cloned()
        .ok_or_else(|| anyhow!("unknown coordination task `{}`", input.task_id.0))?;
    let Some(plan) = state.plans.get(&previous.plan).cloned() else {
        return Err(anyhow!("unknown plan `{}`", previous.plan.0));
    };
    let lease_state = task_lease_state(&previous, meta.ts);
    if !matches!(lease_state, LeaseState::Stale | LeaseState::Expired) {
        return Err(anyhow!(
            "coordination task `{}` does not have a stale or expired lease to reclaim",
            previous.id.0
        ));
    }
    let current_holder = current_task_holder(&meta, &previous);
    if authoritative_task_holder(&previous)
        .as_ref()
        .is_some_and(|lease_holder| same_holder(lease_holder, &current_holder))
    {
        let violations = vec![policy_violation(
            PolicyViolationCode::TaskReclaimRequired,
            format!(
                "coordination task `{}` is still owned by the same principal and should be resumed instead of reclaimed",
                previous.id.0
            ),
            Some(previous.plan.clone()),
            Some(previous.id.clone()),
            None,
            None,
            json!({
                "leaseState": format!("{lease_state:?}").to_ascii_lowercase(),
                "leaseHolder": lease_holder_details(previous.lease_holder.as_ref()),
                "currentHolder": lease_holder_details(Some(&current_holder)),
            }),
        )];
        return Err(rejection_error(
            state,
            &meta,
            "coordination task reclaim rejected",
            Some(previous.plan.clone()),
            Some(previous.id.clone()),
            None,
            None,
            violations,
        ));
    }

    let task = state
        .tasks
        .get_mut(&input.task_id)
        .expect("task validated above");
    if let Some(agent) = input.agent {
        task.assignee = Some(agent);
    }
    task.session = current_holder.session_id.clone();
    task.worktree_id = input.worktree_id;
    task.branch_ref = input.branch_ref;
    if task.pending_handoff_to.is_some() {
        task.pending_handoff_to = None;
        if task.status == CoordinationTaskStatus::Blocked {
            task.status = CoordinationTaskStatus::Ready;
        }
    }
    refresh_task_lease(task, &meta, meta.ts, &plan.policy);
    let task = task.clone();

    let mut patch = serde_json::Map::new();
    if previous.assignee != task.assignee {
        push_patch_op(
            &mut patch,
            "assignee",
            if task.assignee.is_some() {
                "set"
            } else {
                "clear"
            },
        );
    }
    if previous.pending_handoff_to != task.pending_handoff_to {
        push_patch_op(
            &mut patch,
            "pendingHandoffTo",
            if task.pending_handoff_to.is_some() {
                "set"
            } else {
                "clear"
            },
        );
    }
    if previous.session != task.session {
        push_patch_op(
            &mut patch,
            "session",
            if task.session.is_some() {
                "set"
            } else {
                "clear"
            },
        );
    }
    if previous.worktree_id != task.worktree_id {
        push_patch_op(
            &mut patch,
            "worktreeId",
            if task.worktree_id.is_some() {
                "set"
            } else {
                "clear"
            },
        );
    }
    if previous.branch_ref != task.branch_ref {
        push_patch_op(
            &mut patch,
            "branchRef",
            if task.branch_ref.is_some() {
                "set"
            } else {
                "clear"
            },
        );
    }
    if previous.status != task.status {
        push_patch_op(&mut patch, "status", "set");
    }
    push_patch_op(&mut patch, "leaseHolder", "set");
    push_patch_op(&mut patch, "leaseStartedAt", "set");
    push_patch_op(&mut patch, "leaseRefreshedAt", "set");
    push_patch_op(&mut patch, "leaseStaleAt", "set");
    push_patch_op(&mut patch, "leaseExpiresAt", "set");

    let mut patch_values = serde_json::Map::new();
    if previous.assignee != task.assignee {
        insert_serialized(&mut patch_values, "assignee", task.assignee.clone());
    }
    if previous.pending_handoff_to != task.pending_handoff_to {
        insert_serialized(
            &mut patch_values,
            "pendingHandoffTo",
            task.pending_handoff_to.clone(),
        );
    }
    if previous.session != task.session {
        insert_serialized(&mut patch_values, "session", task.session.clone());
    }
    if previous.worktree_id != task.worktree_id {
        insert_serialized(&mut patch_values, "worktreeId", task.worktree_id.clone());
    }
    if previous.branch_ref != task.branch_ref {
        insert_serialized(&mut patch_values, "branchRef", task.branch_ref.clone());
    }
    if previous.status != task.status {
        insert_serialized(&mut patch_values, "status", task.status);
    }
    insert_serialized(&mut patch_values, "leaseHolder", task.lease_holder.clone());
    insert_serialized(&mut patch_values, "leaseStartedAt", task.lease_started_at);
    insert_serialized(
        &mut patch_values,
        "leaseRefreshedAt",
        task.lease_refreshed_at,
    );
    insert_serialized(&mut patch_values, "leaseStaleAt", task.lease_stale_at);
    insert_serialized(&mut patch_values, "leaseExpiresAt", task.lease_expires_at);

    state.events.push(CoordinationEvent {
        meta: meta.clone(),
        kind: CoordinationEventKind::TaskReclaimed,
        summary: format!("task `{}` reclaimed", task.id.0),
        plan: Some(task.plan.clone()),
        task: Some(task.id.clone()),
        claim: None,
        artifact: None,
        review: None,
        metadata: json!({
            "leaseState": format!("{lease_state:?}").to_ascii_lowercase(),
            "patch": Value::Object(patch),
            "patchValues": Value::Object(patch_values),
        }),
    });
    Ok(task)
}

impl CoordinationStore {
    pub fn create_plan(&self, meta: EventMeta, input: PlanCreateInput) -> Result<(PlanId, Plan)> {
        let mut state = self
            .state
            .write()
            .expect("coordination store lock poisoned");
        create_plan_mutation(&mut state, meta, input)
    }

    pub fn update_plan(&self, meta: EventMeta, input: PlanUpdateInput) -> Result<Plan> {
        let mut state = self
            .state
            .write()
            .expect("coordination store lock poisoned");
        update_plan_mutation(&mut state, meta, input)
    }

    pub fn set_plan_scheduling(
        &self,
        meta: EventMeta,
        plan_id: PlanId,
        scheduling: PlanScheduling,
    ) -> Result<Plan> {
        let mut state = self
            .state
            .write()
            .expect("coordination store lock poisoned");
        set_plan_scheduling_mutation(&mut state, meta, plan_id, scheduling)
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
        create_task_mutation(&mut state, meta, input)
    }

    pub fn update_task(
        &self,
        meta: EventMeta,
        input: TaskUpdateInput,
        current_revision: WorkspaceRevision,
        now: Timestamp,
    ) -> Result<CoordinationTask> {
        let mut state = self
            .state
            .write()
            .expect("coordination store lock poisoned");
        update_task_mutation(&mut state, meta, input, current_revision, now)
    }

    pub fn update_task_authoritative_only(
        &self,
        meta: EventMeta,
        input: TaskUpdateInput,
        current_revision: WorkspaceRevision,
        now: Timestamp,
    ) -> Result<CoordinationTask> {
        let mut state = self
            .state
            .write()
            .expect("coordination store lock poisoned");
        update_task_mutation_with_options(&mut state, meta, input, current_revision, now, true)
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
        handoff_mutation(&mut state, meta, input, current_revision)
    }

    pub fn accept_handoff(
        &self,
        meta: EventMeta,
        input: HandoffAcceptInput,
    ) -> Result<CoordinationTask> {
        let mut state = self
            .state
            .write()
            .expect("coordination store lock poisoned");
        accept_handoff_mutation(&mut state, meta, input)
    }

    pub fn resume_task(&self, meta: EventMeta, input: TaskResumeInput) -> Result<CoordinationTask> {
        let mut state = self
            .state
            .write()
            .expect("coordination store lock poisoned");
        resume_task_mutation(&mut state, meta, input)
    }

    pub fn reclaim_task(
        &self,
        meta: EventMeta,
        input: TaskReclaimInput,
    ) -> Result<CoordinationTask> {
        let mut state = self
            .state
            .write()
            .expect("coordination store lock poisoned");
        reclaim_task_mutation(&mut state, meta, input)
    }

    pub fn heartbeat_task(
        &self,
        meta: EventMeta,
        task_id: &CoordinationTaskId,
        renewal_provenance: &str,
    ) -> Result<CoordinationTask> {
        let mut state = self
            .state
            .write()
            .expect("coordination store lock poisoned");
        heartbeat_task_mutation(&mut state, meta, task_id, renewal_provenance)
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
        acquire_claim_mutation(&mut state, meta, session_id, input)
    }

    pub fn renew_claim(
        &self,
        meta: EventMeta,
        session_id: &SessionId,
        claim_id: &ClaimId,
        ttl_seconds: Option<u64>,
        renewal_provenance: &str,
    ) -> Result<WorkClaim> {
        let mut state = self
            .state
            .write()
            .expect("coordination store lock poisoned");
        renew_claim_mutation(
            &mut state,
            meta,
            session_id,
            claim_id,
            ttl_seconds,
            renewal_provenance,
        )
    }

    pub fn release_claim(
        &self,
        meta: EventMeta,
        session_id: &SessionId,
        claim_id: &ClaimId,
    ) -> Result<WorkClaim> {
        let mut state = self
            .state
            .write()
            .expect("coordination store lock poisoned");
        release_claim_mutation(&mut state, meta, session_id, claim_id)
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
        propose_artifact_mutation(&mut state, meta, input)
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
        supersede_artifact_mutation(&mut state, meta, input)
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
        review_artifact_mutation(&mut state, meta, input, current_revision)
    }
}

fn validate_task_dependencies(
    state: &mut CoordinationState,
    plan_id: &PlanId,
    task_id: &CoordinationTaskId,
    dependencies: &[CoordinationTaskId],
    meta: &EventMeta,
) -> Result<()> {
    for dependency in dependencies {
        if dependency == task_id {
            let violations = vec![policy_violation(
                PolicyViolationCode::MissingDependency,
                format!("coordination task `{}` cannot depend on itself", task_id.0),
                Some(plan_id.clone()),
                Some(task_id.clone()),
                None,
                None,
                json!({ "dependencyTaskId": dependency.0 }),
            )];
            return Err(rejection_error(
                state,
                meta,
                "coordination task update rejected",
                Some(plan_id.clone()),
                Some(task_id.clone()),
                None,
                None,
                violations,
            ));
        }
        let Some(task) = state.tasks.get(dependency) else {
            let violations = vec![policy_violation(
                PolicyViolationCode::MissingDependency,
                format!("unknown dependency task `{}`", dependency.0),
                Some(plan_id.clone()),
                Some(task_id.clone()),
                None,
                None,
                json!({ "dependencyTaskId": dependency.0 }),
            )];
            return Err(rejection_error(
                state,
                meta,
                "coordination task update rejected",
                Some(plan_id.clone()),
                Some(task_id.clone()),
                None,
                None,
                violations,
            ));
        };
        if &task.plan != plan_id {
            let violations = vec![policy_violation(
                PolicyViolationCode::CrossPlanDependency,
                format!(
                    "dependency task `{}` belongs to a different plan",
                    dependency.0
                ),
                Some(plan_id.clone()),
                Some(task_id.clone()),
                None,
                None,
                json!({
                    "dependencyTaskId": dependency.0,
                    "dependencyPlanId": task.plan.0,
                }),
            )];
            return Err(rejection_error(
                state,
                meta,
                "coordination task update rejected",
                Some(plan_id.clone()),
                Some(task_id.clone()),
                None,
                None,
                violations,
            ));
        }
    }
    Ok(())
}
