use prism_ir::{EventActor, EventMeta, SessionId, Timestamp};

use crate::types::{CoordinationPolicy, CoordinationTask, LeaseHolder, WorkClaim};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LeaseState {
    Unleased,
    Active,
    Stale,
    Expired,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LeaseHeartbeatDueState {
    NotDue,
    DueSoon,
    DueNow,
}

fn session_id_from_meta(meta: &EventMeta) -> Option<SessionId> {
    meta.execution_context
        .as_ref()
        .and_then(|context| context.session_id.as_ref())
        .map(|value| SessionId::new(value.clone()))
}

fn principal_from_meta(meta: &EventMeta) -> Option<prism_ir::PrincipalActor> {
    match &meta.actor {
        EventActor::Principal(principal) => Some(principal.clone()),
        _ => None,
    }
}

fn lease_window(
    now: Timestamp,
    policy: &CoordinationPolicy,
    expires_after_seconds: u64,
) -> (u64, u64) {
    let stale_after_seconds = policy.lease_stale_after_seconds.min(expires_after_seconds);
    (
        now.saturating_add(stale_after_seconds),
        now.saturating_add(expires_after_seconds),
    )
}

pub fn heartbeat_due_soon_window(policy: &CoordinationPolicy) -> u64 {
    (policy.lease_stale_after_seconds / 6).clamp(60, 300)
}

pub fn assisted_heartbeat_window(policy: &CoordinationPolicy) -> u64 {
    policy
        .lease_stale_after_seconds
        .min(policy.lease_expires_after_seconds)
}

fn holder_has_identity(holder: &LeaseHolder) -> bool {
    holder.principal.is_some() || holder.session_id.is_some() || holder.agent_id.is_some()
}

pub fn task_lease_state(task: &CoordinationTask, now: Timestamp) -> LeaseState {
    let Some(expires_at) = task.lease_expires_at else {
        return LeaseState::Unleased;
    };
    if expires_at < now {
        return LeaseState::Expired;
    }
    if task.lease_stale_at.is_some_and(|stale_at| stale_at <= now) {
        return LeaseState::Stale;
    }
    LeaseState::Active
}

pub fn claim_lease_state(claim: &WorkClaim, now: Timestamp) -> LeaseState {
    if matches!(
        claim.status,
        prism_ir::ClaimStatus::Released | prism_ir::ClaimStatus::Expired
    ) {
        return LeaseState::Expired;
    }
    if claim.expires_at < now {
        return LeaseState::Expired;
    }
    if claim.stale_at.unwrap_or(claim.expires_at) <= now {
        return LeaseState::Stale;
    }
    LeaseState::Active
}

pub fn task_heartbeat_due_state(
    task: &CoordinationTask,
    policy: &CoordinationPolicy,
    now: Timestamp,
) -> LeaseHeartbeatDueState {
    if !matches!(task_lease_state(task, now), LeaseState::Active) {
        return LeaseHeartbeatDueState::NotDue;
    }
    let Some(stale_at) = task.lease_stale_at else {
        return LeaseHeartbeatDueState::NotDue;
    };
    let remaining = stale_at.saturating_sub(now);
    if remaining <= 60 {
        LeaseHeartbeatDueState::DueNow
    } else if remaining <= heartbeat_due_soon_window(policy) {
        LeaseHeartbeatDueState::DueSoon
    } else {
        LeaseHeartbeatDueState::NotDue
    }
}

pub fn claim_heartbeat_due_state(
    claim: &WorkClaim,
    policy: &CoordinationPolicy,
    now: Timestamp,
) -> LeaseHeartbeatDueState {
    if !matches!(claim_lease_state(claim, now), LeaseState::Active) {
        return LeaseHeartbeatDueState::NotDue;
    }
    let Some(stale_at) = claim.stale_at else {
        return LeaseHeartbeatDueState::NotDue;
    };
    let remaining = stale_at.saturating_sub(now);
    if remaining <= 60 {
        LeaseHeartbeatDueState::DueNow
    } else if remaining <= heartbeat_due_soon_window(policy) {
        LeaseHeartbeatDueState::DueSoon
    } else {
        LeaseHeartbeatDueState::NotDue
    }
}

pub(crate) fn claim_is_live(claim: &WorkClaim, now: Timestamp) -> bool {
    matches!(
        claim.status,
        prism_ir::ClaimStatus::Active | prism_ir::ClaimStatus::Contended
    ) && !matches!(claim_lease_state(claim, now), LeaseState::Expired)
}

pub(crate) fn claim_blocks_new_work(claim: &WorkClaim, now: Timestamp) -> bool {
    matches!(
        claim.status,
        prism_ir::ClaimStatus::Active | prism_ir::ClaimStatus::Contended
    ) && matches!(claim_lease_state(claim, now), LeaseState::Active)
}

pub(crate) fn clear_task_lease(task: &mut CoordinationTask) {
    task.lease_holder = None;
    task.lease_started_at = None;
    task.lease_refreshed_at = None;
    task.lease_stale_at = None;
    task.lease_expires_at = None;
}

pub(crate) fn refresh_task_lease(
    task: &mut CoordinationTask,
    meta: &EventMeta,
    now: Timestamp,
    policy: &CoordinationPolicy,
) {
    let (lease_stale_at, lease_expires_at) =
        lease_window(now, policy, policy.lease_expires_after_seconds);
    let holder = LeaseHolder {
        principal: principal_from_meta(meta),
        session_id: task.session.clone().or_else(|| session_id_from_meta(meta)),
        agent_id: task.assignee.clone(),
    };
    if !holder_has_identity(&holder) {
        clear_task_lease(task);
        return;
    }
    task.lease_holder = Some(holder);
    if task.lease_started_at.is_none() {
        task.lease_started_at = Some(now);
    }
    task.lease_refreshed_at = Some(now);
    task.lease_stale_at = Some(lease_stale_at);
    task.lease_expires_at = Some(lease_expires_at);
}

pub(crate) fn refresh_claim_lease(
    claim: &mut WorkClaim,
    meta: &EventMeta,
    now: Timestamp,
    policy: Option<&CoordinationPolicy>,
    expires_after_seconds: Option<u64>,
) {
    let policy = policy.cloned().unwrap_or_default();
    let expires_after_seconds = expires_after_seconds.unwrap_or(policy.lease_expires_after_seconds);
    let (stale_at, expires_at) = lease_window(now, &policy, expires_after_seconds);
    let holder = LeaseHolder {
        principal: principal_from_meta(meta),
        session_id: Some(claim.holder.clone()),
        agent_id: claim.agent.clone(),
    };
    claim.lease_holder = holder_has_identity(&holder).then_some(holder);
    claim.refreshed_at = Some(now);
    claim.stale_at = Some(stale_at);
    claim.expires_at = expires_at;
}

fn same_principal(left: &prism_ir::PrincipalActor, right: &prism_ir::PrincipalActor) -> bool {
    left.authority_id == right.authority_id && left.principal_id == right.principal_id
}

pub(crate) fn same_holder(left: &LeaseHolder, right: &LeaseHolder) -> bool {
    if let Some(left_principal) = left.principal.as_ref() {
        return right
            .principal
            .as_ref()
            .is_some_and(|right_principal| same_principal(left_principal, right_principal));
    }
    if let (Some(left_session), Some(right_session)) =
        (left.session_id.as_ref(), right.session_id.as_ref())
    {
        return left_session == right_session;
    }
    if let (Some(left_agent), Some(right_agent)) = (left.agent_id.as_ref(), right.agent_id.as_ref())
    {
        return left_agent == right_agent;
    }
    false
}

pub(crate) fn current_task_holder(meta: &EventMeta, task: &CoordinationTask) -> LeaseHolder {
    LeaseHolder {
        principal: principal_from_meta(meta),
        session_id: session_id_from_meta(meta).or_else(|| task.session.clone()),
        agent_id: task.assignee.clone(),
    }
}

pub(crate) fn current_claim_holder(
    meta: &EventMeta,
    session_id: &SessionId,
    claim: &WorkClaim,
) -> LeaseHolder {
    LeaseHolder {
        principal: principal_from_meta(meta),
        session_id: Some(session_id.clone()),
        agent_id: claim.agent.clone(),
    }
}
