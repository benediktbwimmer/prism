use prism_ir::{EventActor, EventMeta, SessionId, Timestamp};

use crate::canonical_graph::CanonicalTaskRecord;
use crate::types::{
    CoordinationPolicy, CoordinationTask, LeaseHolder, RuntimeDescriptor, WorkClaim,
};

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

fn worktree_id_from_meta(meta: &EventMeta) -> Option<String> {
    meta.execution_context
        .as_ref()
        .and_then(|context| context.worktree_id.clone())
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
    holder.principal.is_some()
        || holder.session_id.is_some()
        || holder.worktree_id.is_some()
        || holder.agent_id.is_some()
}

fn matching_runtime_descriptor<'a>(
    runtime_descriptors: &'a [RuntimeDescriptor],
    worktree_id: Option<&str>,
    acquired_at: Option<Timestamp>,
) -> Option<&'a RuntimeDescriptor> {
    let worktree_id = worktree_id?;
    let acquired_at = acquired_at?;
    runtime_descriptors
        .iter()
        .filter(|descriptor| descriptor.worktree_id == worktree_id)
        .filter(|descriptor| descriptor.instance_started_at <= acquired_at)
        .max_by_key(|descriptor| (descriptor.instance_started_at, descriptor.last_seen_at))
}

fn effective_lease_anchor(
    anchor: Option<Timestamp>,
    runtime_descriptors: &[RuntimeDescriptor],
    worktree_id: Option<&str>,
) -> Option<Timestamp> {
    let anchor = anchor?;
    let runtime = matching_runtime_descriptor(runtime_descriptors, worktree_id, Some(anchor));
    Some(
        anchor.max(
            runtime
                .map(|descriptor| descriptor.last_seen_at)
                .unwrap_or(anchor),
        ),
    )
}

fn effective_absolute_deadline(
    anchor: Option<Timestamp>,
    deadline: Option<Timestamp>,
    runtime_descriptors: &[RuntimeDescriptor],
    worktree_id: Option<&str>,
) -> Option<Timestamp> {
    let anchor = anchor?;
    let deadline = deadline?;
    let window = deadline.saturating_sub(anchor);
    effective_lease_anchor(Some(anchor), runtime_descriptors, worktree_id)
        .map(|effective_anchor| effective_anchor.saturating_add(window))
}

pub fn task_lease_state(task: &CoordinationTask, now: Timestamp) -> LeaseState {
    task_lease_state_with_runtime_descriptors(task, &[], now)
}

pub fn canonical_task_lease_state(task: &CanonicalTaskRecord, now: Timestamp) -> LeaseState {
    canonical_task_lease_state_with_runtime_descriptors(task, &[], now)
}

pub fn task_lease_state_with_runtime_descriptors(
    task: &CoordinationTask,
    runtime_descriptors: &[RuntimeDescriptor],
    now: Timestamp,
) -> LeaseState {
    let Some(expires_at) = task.lease_expires_at else {
        return LeaseState::Unleased;
    };
    let anchor = task.lease_refreshed_at.or(task.lease_started_at);
    let worktree_id = task.worktree_id.as_deref().or(task
        .lease_holder
        .as_ref()
        .and_then(|holder| holder.worktree_id.as_deref()));
    let effective_expires_at =
        effective_absolute_deadline(anchor, Some(expires_at), runtime_descriptors, worktree_id)
            .unwrap_or(expires_at);
    if effective_expires_at < now {
        return LeaseState::Expired;
    }
    let effective_stale_at = effective_absolute_deadline(
        anchor,
        task.lease_stale_at,
        runtime_descriptors,
        worktree_id,
    )
    .or(task.lease_stale_at);
    if effective_stale_at.is_some_and(|stale_at| stale_at <= now) {
        return LeaseState::Stale;
    }
    LeaseState::Active
}

pub fn canonical_task_lease_state_with_runtime_descriptors(
    task: &CanonicalTaskRecord,
    runtime_descriptors: &[RuntimeDescriptor],
    now: Timestamp,
) -> LeaseState {
    let Some(expires_at) = task.lease_expires_at else {
        return LeaseState::Unleased;
    };
    let anchor = task.lease_refreshed_at.or(task.lease_started_at);
    let worktree_id = task.worktree_id.as_deref().or(task
        .lease_holder
        .as_ref()
        .and_then(|holder| holder.worktree_id.as_deref()));
    let effective_expires_at =
        effective_absolute_deadline(anchor, Some(expires_at), runtime_descriptors, worktree_id)
            .unwrap_or(expires_at);
    if effective_expires_at < now {
        return LeaseState::Expired;
    }
    let effective_stale_at = effective_absolute_deadline(
        anchor,
        task.lease_stale_at,
        runtime_descriptors,
        worktree_id,
    )
    .or(task.lease_stale_at);
    if effective_stale_at.is_some_and(|stale_at| stale_at <= now) {
        return LeaseState::Stale;
    }
    LeaseState::Active
}

pub fn claim_lease_state(claim: &WorkClaim, now: Timestamp) -> LeaseState {
    claim_lease_state_with_runtime_descriptors(claim, &[], now)
}

pub fn claim_lease_state_with_runtime_descriptors(
    claim: &WorkClaim,
    runtime_descriptors: &[RuntimeDescriptor],
    now: Timestamp,
) -> LeaseState {
    if matches!(
        claim.status,
        prism_ir::ClaimStatus::Released | prism_ir::ClaimStatus::Expired
    ) {
        return LeaseState::Expired;
    }
    let anchor = claim.refreshed_at.or(Some(claim.since));
    let worktree_id = claim.worktree_id.as_deref().or(claim
        .lease_holder
        .as_ref()
        .and_then(|holder| holder.worktree_id.as_deref()));
    let effective_expires_at = effective_absolute_deadline(
        anchor,
        Some(claim.expires_at),
        runtime_descriptors,
        worktree_id,
    )
    .unwrap_or(claim.expires_at);
    if effective_expires_at < now {
        return LeaseState::Expired;
    }
    let effective_stale_at = effective_absolute_deadline(
        anchor,
        claim.stale_at.or(Some(claim.expires_at)),
        runtime_descriptors,
        worktree_id,
    )
    .unwrap_or(claim.stale_at.unwrap_or(claim.expires_at));
    if effective_stale_at <= now {
        return LeaseState::Stale;
    }
    LeaseState::Active
}

pub fn task_heartbeat_due_state(
    task: &CoordinationTask,
    policy: &CoordinationPolicy,
    now: Timestamp,
) -> LeaseHeartbeatDueState {
    task_heartbeat_due_state_with_runtime_descriptors(task, policy, &[], now)
}

pub fn canonical_task_heartbeat_due_state(
    task: &CanonicalTaskRecord,
    policy: &CoordinationPolicy,
    now: Timestamp,
) -> LeaseHeartbeatDueState {
    canonical_task_heartbeat_due_state_with_runtime_descriptors(task, policy, &[], now)
}

pub fn task_heartbeat_due_state_with_runtime_descriptors(
    task: &CoordinationTask,
    policy: &CoordinationPolicy,
    runtime_descriptors: &[RuntimeDescriptor],
    now: Timestamp,
) -> LeaseHeartbeatDueState {
    if !matches!(
        task_lease_state_with_runtime_descriptors(task, runtime_descriptors, now),
        LeaseState::Active
    ) {
        return LeaseHeartbeatDueState::NotDue;
    }
    let Some(stale_at) = task.lease_stale_at else {
        return LeaseHeartbeatDueState::NotDue;
    };
    let anchor = task.lease_refreshed_at.or(task.lease_started_at);
    let worktree_id = task.worktree_id.as_deref().or(task
        .lease_holder
        .as_ref()
        .and_then(|holder| holder.worktree_id.as_deref()));
    let effective_stale_at =
        effective_absolute_deadline(anchor, Some(stale_at), runtime_descriptors, worktree_id)
            .unwrap_or(stale_at);
    let remaining = effective_stale_at.saturating_sub(now);
    if remaining <= 60 {
        LeaseHeartbeatDueState::DueNow
    } else if remaining <= heartbeat_due_soon_window(policy) {
        LeaseHeartbeatDueState::DueSoon
    } else {
        LeaseHeartbeatDueState::NotDue
    }
}

pub fn canonical_task_heartbeat_due_state_with_runtime_descriptors(
    task: &CanonicalTaskRecord,
    policy: &CoordinationPolicy,
    runtime_descriptors: &[RuntimeDescriptor],
    now: Timestamp,
) -> LeaseHeartbeatDueState {
    if !matches!(
        canonical_task_lease_state_with_runtime_descriptors(task, runtime_descriptors, now),
        LeaseState::Active
    ) {
        return LeaseHeartbeatDueState::NotDue;
    }
    let Some(stale_at) = task.lease_stale_at else {
        return LeaseHeartbeatDueState::NotDue;
    };
    let anchor = task.lease_refreshed_at.or(task.lease_started_at);
    let worktree_id = task.worktree_id.as_deref().or(task
        .lease_holder
        .as_ref()
        .and_then(|holder| holder.worktree_id.as_deref()));
    let effective_stale_at =
        effective_absolute_deadline(anchor, Some(stale_at), runtime_descriptors, worktree_id)
            .unwrap_or(stale_at);
    let remaining = effective_stale_at.saturating_sub(now);
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
    claim_heartbeat_due_state_with_runtime_descriptors(claim, policy, &[], now)
}

pub fn claim_heartbeat_due_state_with_runtime_descriptors(
    claim: &WorkClaim,
    policy: &CoordinationPolicy,
    runtime_descriptors: &[RuntimeDescriptor],
    now: Timestamp,
) -> LeaseHeartbeatDueState {
    if !matches!(
        claim_lease_state_with_runtime_descriptors(claim, runtime_descriptors, now),
        LeaseState::Active
    ) {
        return LeaseHeartbeatDueState::NotDue;
    }
    let Some(stale_at) = claim.stale_at else {
        return LeaseHeartbeatDueState::NotDue;
    };
    let anchor = claim.refreshed_at.or(Some(claim.since));
    let worktree_id = claim.worktree_id.as_deref().or(claim
        .lease_holder
        .as_ref()
        .and_then(|holder| holder.worktree_id.as_deref()));
    let effective_stale_at =
        effective_absolute_deadline(anchor, Some(stale_at), runtime_descriptors, worktree_id)
            .unwrap_or(stale_at);
    let remaining = effective_stale_at.saturating_sub(now);
    if remaining <= 60 {
        LeaseHeartbeatDueState::DueNow
    } else if remaining <= heartbeat_due_soon_window(policy) {
        LeaseHeartbeatDueState::DueSoon
    } else {
        LeaseHeartbeatDueState::NotDue
    }
}

pub(crate) fn task_heartbeat_should_refresh(
    task: &CoordinationTask,
    policy: &CoordinationPolicy,
    runtime_descriptors: &[RuntimeDescriptor],
    now: Timestamp,
) -> bool {
    matches!(
        task_heartbeat_due_state_with_runtime_descriptors(task, policy, runtime_descriptors, now),
        LeaseHeartbeatDueState::DueSoon | LeaseHeartbeatDueState::DueNow
    )
}

pub(crate) fn claim_renewal_should_refresh(
    claim: &WorkClaim,
    policy: Option<&CoordinationPolicy>,
    runtime_descriptors: &[RuntimeDescriptor],
    now: Timestamp,
    expires_after_seconds: Option<u64>,
) -> bool {
    let policy = policy.cloned().unwrap_or_default();
    if !matches!(
        claim_lease_state_with_runtime_descriptors(claim, runtime_descriptors, now),
        LeaseState::Active
    ) {
        return true;
    }
    if matches!(
        claim_heartbeat_due_state_with_runtime_descriptors(
            claim,
            &policy,
            runtime_descriptors,
            now,
        ),
        LeaseHeartbeatDueState::DueSoon | LeaseHeartbeatDueState::DueNow
    ) {
        return true;
    }
    let Some(requested_expires_after_seconds) = expires_after_seconds else {
        return false;
    };
    requested_expires_after_seconds > claim.expires_at.saturating_sub(now)
}

pub(crate) fn claim_is_live(claim: &WorkClaim, now: Timestamp) -> bool {
    claim_is_live_with_runtime_descriptors(claim, &[], now)
}

pub(crate) fn claim_is_live_with_runtime_descriptors(
    claim: &WorkClaim,
    runtime_descriptors: &[RuntimeDescriptor],
    now: Timestamp,
) -> bool {
    matches!(
        claim.status,
        prism_ir::ClaimStatus::Active | prism_ir::ClaimStatus::Contended
    ) && !matches!(
        claim_lease_state_with_runtime_descriptors(claim, runtime_descriptors, now),
        LeaseState::Expired
    )
}

pub(crate) fn claim_blocks_new_work(claim: &WorkClaim, now: Timestamp) -> bool {
    claim_blocks_new_work_with_runtime_descriptors(claim, &[], now)
}

pub(crate) fn claim_blocks_new_work_with_runtime_descriptors(
    claim: &WorkClaim,
    runtime_descriptors: &[RuntimeDescriptor],
    now: Timestamp,
) -> bool {
    matches!(
        claim.status,
        prism_ir::ClaimStatus::Active | prism_ir::ClaimStatus::Contended
    ) && matches!(
        claim_lease_state_with_runtime_descriptors(claim, runtime_descriptors, now),
        LeaseState::Active
    )
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
        worktree_id: task
            .worktree_id
            .clone()
            .or_else(|| worktree_id_from_meta(meta)),
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
        worktree_id: claim
            .worktree_id
            .clone()
            .or_else(|| worktree_id_from_meta(meta)),
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

pub fn same_holder(left: &LeaseHolder, right: &LeaseHolder) -> bool {
    if let (Some(left_worktree), Some(right_worktree)) =
        (left.worktree_id.as_ref(), right.worktree_id.as_ref())
    {
        return left_worktree == right_worktree;
    }
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

fn authoritative_task_holder_fields(
    lease_holder: Option<&LeaseHolder>,
    session: Option<&SessionId>,
    worktree_id: Option<&str>,
    assignee: Option<&prism_ir::AgentId>,
) -> Option<LeaseHolder> {
    let mut holder = lease_holder.cloned().unwrap_or(LeaseHolder {
        principal: None,
        session_id: None,
        worktree_id: None,
        agent_id: None,
    });
    if holder.session_id.is_none() {
        holder.session_id = session.cloned();
    }
    if holder.worktree_id.is_none() {
        holder.worktree_id = worktree_id.map(ToOwned::to_owned);
    }
    if holder.agent_id.is_none() {
        holder.agent_id = assignee.cloned();
    }
    holder_has_identity(&holder).then_some(holder)
}

pub fn canonical_authoritative_task_holder(task: &CanonicalTaskRecord) -> Option<LeaseHolder> {
    authoritative_task_holder_fields(
        task.lease_holder.as_ref(),
        task.session.as_ref(),
        task.worktree_id.as_deref(),
        task.assignee.as_ref(),
    )
}

pub(crate) fn authoritative_task_holder(task: &CoordinationTask) -> Option<LeaseHolder> {
    authoritative_task_holder_fields(
        task.lease_holder.as_ref(),
        task.session.as_ref(),
        task.worktree_id.as_deref(),
        task.assignee.as_ref(),
    )
}

pub fn canonical_current_task_holder(meta: &EventMeta, task: &CanonicalTaskRecord) -> LeaseHolder {
    LeaseHolder {
        principal: principal_from_meta(meta),
        session_id: session_id_from_meta(meta).or_else(|| task.session.clone()),
        worktree_id: task
            .worktree_id
            .clone()
            .or_else(|| worktree_id_from_meta(meta)),
        agent_id: task.assignee.clone(),
    }
}

pub(crate) fn current_task_holder(meta: &EventMeta, task: &CoordinationTask) -> LeaseHolder {
    LeaseHolder {
        principal: principal_from_meta(meta),
        session_id: session_id_from_meta(meta).or_else(|| task.session.clone()),
        worktree_id: task
            .worktree_id
            .clone()
            .or_else(|| worktree_id_from_meta(meta)),
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
        worktree_id: claim
            .worktree_id
            .clone()
            .or_else(|| worktree_id_from_meta(meta)),
        agent_id: claim.agent.clone(),
    }
}
