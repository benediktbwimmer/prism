use std::collections::{HashMap, HashSet};

use anyhow::{anyhow, Result};
use prism_ir::{
    AnchorRef, ArtifactId, Capability, ClaimId, ClaimMode, ClaimStatus, ConflictOverlapKind,
    ConflictSeverity, CoordinationTaskId, CoordinationTaskStatus, EventId, EventMeta, PlanId,
    PlanStatus, SessionId, Timestamp, WorkspaceRevision,
};
use serde_json::{json, Value};

use crate::lease::{
    claim_blocks_new_work_with_runtime_descriptors, claim_is_live_with_runtime_descriptors,
};
use crate::state::CoordinationState;
use crate::types::{
    AcceptanceCriterion, Artifact, BlockerKind, CoordinationConflict, CoordinationEvent,
    CoordinationPolicy, PolicyViolation, PolicyViolationCode, TaskBlocker, WorkClaim,
};

pub(crate) fn plan_policy_for_task<'a>(
    state: &'a CoordinationState,
    task_id: Option<&CoordinationTaskId>,
) -> Result<Option<&'a CoordinationPolicy>> {
    let Some(task_id) = task_id else {
        return Ok(None);
    };
    let task = state
        .tasks
        .get(task_id)
        .ok_or_else(|| anyhow!("unknown coordination task `{}`", task_id.0))?;
    Ok(state.plans.get(&task.plan).map(|plan| &plan.policy))
}

pub(crate) fn validate_task_transition(
    previous: CoordinationTaskStatus,
    next: CoordinationTaskStatus,
) -> Result<()> {
    use CoordinationTaskStatus::*;

    let allowed = match previous {
        Proposed => matches!(next, Proposed | Ready | Blocked | Abandoned),
        Ready => matches!(
            next,
            Ready | InProgress | Blocked | InReview | Validating | Completed | Abandoned
        ),
        InProgress => matches!(
            next,
            InProgress | Ready | Blocked | InReview | Validating | Completed | Abandoned
        ),
        Blocked => matches!(next, Blocked | Ready | InProgress | Abandoned),
        InReview => matches!(
            next,
            InReview | Ready | InProgress | Blocked | Validating | Completed | Abandoned
        ),
        Validating => matches!(
            next,
            Validating | Ready | InProgress | Blocked | InReview | Completed | Abandoned
        ),
        Completed => matches!(next, Completed),
        Abandoned => matches!(next, Abandoned),
    };

    if allowed {
        Ok(())
    } else {
        Err(anyhow!(
            "invalid coordination task transition from {:?} to {:?}",
            previous,
            next
        ))
    }
}

pub(crate) fn validate_plan_transition(previous: PlanStatus, next: PlanStatus) -> Result<()> {
    use PlanStatus::*;

    let allowed = match previous {
        Draft => matches!(next, Draft | Active | Abandoned),
        Active => matches!(next, Active | Blocked | Completed | Abandoned),
        Blocked => matches!(next, Blocked | Active | Abandoned),
        Completed => matches!(next, Completed | Archived),
        Abandoned => matches!(next, Abandoned | Archived),
        Archived => matches!(next, Archived),
    };

    if allowed {
        Ok(())
    } else {
        Err(anyhow!(
            "invalid coordination plan transition from {:?} to {:?}",
            previous,
            next
        ))
    }
}

pub(crate) fn plan_status_is_closed(status: PlanStatus) -> bool {
    matches!(
        status,
        PlanStatus::Completed | PlanStatus::Abandoned | PlanStatus::Archived
    )
}

pub(crate) fn policy_violation(
    code: PolicyViolationCode,
    summary: impl Into<String>,
    plan_id: Option<PlanId>,
    task_id: Option<CoordinationTaskId>,
    claim_id: Option<ClaimId>,
    artifact_id: Option<ArtifactId>,
    details: Value,
) -> PolicyViolation {
    PolicyViolation {
        code,
        summary: summary.into(),
        plan_id,
        task_id,
        claim_id,
        artifact_id,
        details,
    }
}

pub(crate) fn policy_violation_from_blocker(
    blocker: &TaskBlocker,
    plan_id: PlanId,
    task_id: CoordinationTaskId,
) -> PolicyViolation {
    let code = match blocker.kind {
        BlockerKind::Dependency => PolicyViolationCode::IncompletePlanTasks,
        BlockerKind::ClaimConflict => PolicyViolationCode::ClaimConflict,
        BlockerKind::ReviewRequired => PolicyViolationCode::ReviewRequired,
        BlockerKind::RiskReviewRequired => PolicyViolationCode::RiskReviewRequired,
        BlockerKind::ValidationRequired => PolicyViolationCode::ValidationRequired,
        BlockerKind::StaleRevision => PolicyViolationCode::StaleRevision,
        BlockerKind::ArtifactStale => PolicyViolationCode::ArtifactStale,
    };
    policy_violation(
        code,
        blocker.summary.clone(),
        Some(plan_id),
        Some(task_id),
        None,
        blocker.related_artifact_id.clone(),
        json!({
            "relatedTaskId": blocker.related_task_id.as_ref().map(|value| value.0.to_string()),
            "validationChecks": blocker.validation_checks,
            "riskScore": blocker.risk_score,
            "causes": blocker.causes,
        }),
    )
}

pub(crate) fn derived_event_meta(meta: &EventMeta, suffix: &str) -> EventMeta {
    EventMeta {
        id: EventId::new(format!("{}:{suffix}", meta.id.0)),
        ts: meta.ts,
        actor: meta.actor.clone(),
        correlation: meta.correlation.clone(),
        causation: Some(meta.id.clone()),
        execution_context: meta.execution_context.clone(),
    }
}

pub(crate) fn record_rejection(
    state: &mut CoordinationState,
    meta: &EventMeta,
    summary: impl Into<String>,
    plan_id: Option<PlanId>,
    task_id: Option<CoordinationTaskId>,
    claim_id: Option<ClaimId>,
    artifact_id: Option<ArtifactId>,
    violations: &[PolicyViolation],
) -> EventId {
    let rejection_meta = derived_event_meta(meta, "rejected");
    let event_id = rejection_meta.id.clone();
    state.events.push(CoordinationEvent {
        meta: rejection_meta,
        kind: prism_ir::CoordinationEventKind::MutationRejected,
        summary: summary.into(),
        plan: plan_id,
        task: task_id,
        claim: claim_id,
        artifact: artifact_id,
        review: None,
        metadata: json!({ "violations": violations }),
    });
    event_id
}

pub(crate) fn expire_claims_locked(state: &mut CoordinationState, now: Timestamp) {
    for claim in state.claims.values_mut() {
        if matches!(claim.status, ClaimStatus::Active | ClaimStatus::Contended)
            && claim.expires_at < now
        {
            claim.status = ClaimStatus::Expired;
        }
    }
}

pub(crate) fn editor_capacity_conflicts(
    state: &CoordinationState,
    anchors: &[AnchorRef],
    capability: Capability,
    task_id: Option<&CoordinationTaskId>,
    session_id: &SessionId,
    policy: Option<&CoordinationPolicy>,
    now: Timestamp,
    worktree_id: Option<&str>,
) -> Vec<CoordinationConflict> {
    if !matches!(capability, Capability::Edit | Capability::Merge) {
        return Vec::new();
    }
    let requested_limit =
        policy.map(|policy| usize::from(policy.max_parallel_editors_per_anchor.max(1)));
    let candidates = state
        .claims
        .values()
        .filter(|claim| {
            claim_blocks_new_work_with_runtime_descriptors(claim, &state.runtime_descriptors, now)
        })
        .filter(|claim| claim_matches_worktree_scope(claim, worktree_id))
        .filter(|claim| matches!(claim.capability, Capability::Edit | Capability::Merge))
        .filter(|claim| &claim.holder != session_id)
        .filter(|claim| task_id.map_or(true, |task| claim.task.as_ref() != Some(task)))
        .collect::<Vec<_>>();
    let mut conflicts = Vec::new();
    for anchor in anchors {
        let overlapping = candidates
            .iter()
            .copied()
            .filter(|claim| claim.anchors.contains(anchor))
            .collect::<Vec<_>>();
        let limit = overlapping
            .iter()
            .filter_map(|claim| {
                claim
                    .task
                    .as_ref()
                    .and_then(|claim_task_id| {
                        plan_policy_for_task(state, Some(claim_task_id))
                            .ok()
                            .flatten()
                    })
                    .map(|policy| usize::from(policy.max_parallel_editors_per_anchor.max(1)))
            })
            .chain(requested_limit)
            .min();
        let Some(limit) = limit else {
            continue;
        };
        if overlapping.len() >= limit {
            conflicts.push(CoordinationConflict {
                severity: ConflictSeverity::Block,
                anchors: vec![anchor.clone()],
                overlap_kinds: overlap_kinds(&[anchor.clone()]),
                summary: format!("anchor is already at the edit concurrency limit ({limit})"),
                blocking_claims: overlapping
                    .into_iter()
                    .map(|claim| claim.id.clone())
                    .collect(),
            });
        }
    }
    conflicts
}

pub(crate) fn dedupe_anchors(mut anchors: Vec<AnchorRef>) -> Vec<AnchorRef> {
    anchors.sort_by_key(anchor_sort_key);
    anchors.dedup();
    anchors
}

pub(crate) fn normalize_acceptance(
    mut acceptance: Vec<AcceptanceCriterion>,
) -> Vec<AcceptanceCriterion> {
    for criterion in &mut acceptance {
        criterion.anchors = dedupe_anchors(criterion.anchors.clone());
    }
    acceptance.sort_by(|left, right| left.label.cmp(&right.label));
    acceptance
}

pub(crate) fn dedupe_ids<T>(mut ids: Vec<T>) -> Vec<T>
where
    T: Ord,
{
    ids.sort();
    ids.dedup();
    ids
}

pub(crate) fn dedupe_event_ids(mut ids: Vec<EventId>) -> Vec<EventId> {
    ids.sort_by(|left, right| left.0.cmp(&right.0));
    ids.dedup_by(|left, right| left.0 == right.0);
    ids
}

pub(crate) fn dedupe_strings(mut values: Vec<String>) -> Vec<String> {
    values.sort();
    values.dedup();
    values
}

pub(crate) fn missing_validations_for_artifact(artifact: &Artifact) -> Vec<String> {
    artifact
        .required_validations
        .iter()
        .filter(|check| {
            !artifact
                .validated_checks
                .iter()
                .any(|value| value == *check)
        })
        .cloned()
        .collect()
}

pub(crate) fn claim_is_active(claim: &WorkClaim, now: Timestamp) -> bool {
    claim_is_live_with_runtime_descriptors(claim, &[], now)
}

pub(crate) fn claim_matches_worktree_scope(claim: &WorkClaim, worktree_id: Option<&str>) -> bool {
    worktree_id.is_none_or(|requested| {
        claim
            .worktree_id
            .as_deref()
            .is_none_or(|claim_scope| claim_scope == requested)
    })
}

pub(crate) fn artifact_matches_worktree_scope(
    artifact: &Artifact,
    worktree_id: Option<&str>,
) -> bool {
    worktree_id.is_none_or(|requested| {
        artifact
            .worktree_id
            .as_deref()
            .is_none_or(|artifact_scope| artifact_scope == requested)
    })
}

pub(crate) fn task_matches_worktree_scope(
    task: &crate::types::CoordinationTask,
    worktree_id: Option<&str>,
) -> bool {
    worktree_id.is_none_or(|requested| {
        task.worktree_id
            .as_deref()
            .is_none_or(|task_scope| task_scope == requested)
    })
}

pub(crate) fn anchors_overlap(left: &[AnchorRef], right: &[AnchorRef]) -> bool {
    if left.is_empty() || right.is_empty() {
        return false;
    }
    let right = right.iter().collect::<HashSet<_>>();
    left.iter().any(|anchor| right.contains(anchor))
}

pub(crate) fn simulate_conflicts<'a, I>(
    claims: I,
    anchors: &[AnchorRef],
    capability: Capability,
    mode: ClaimMode,
    policy: Option<&CoordinationPolicy>,
    task_id: Option<&CoordinationTaskId>,
    revision: WorkspaceRevision,
    session_id: &SessionId,
) -> Vec<CoordinationConflict>
where
    I: IntoIterator<Item = &'a WorkClaim>,
{
    claims
        .into_iter()
        .filter(|claim| anchors_overlap(&claim.anchors, anchors))
        .filter(|claim| &claim.holder != session_id)
        .filter(|claim| task_id.map_or(true, |task| claim.task.as_ref() != Some(task)))
        .filter_map(|claim| {
            proposal_conflict(claim, anchors, capability, mode, policy, revision.clone())
        })
        .collect()
}

fn proposal_conflict(
    claim: &WorkClaim,
    anchors: &[AnchorRef],
    capability: Capability,
    mode: ClaimMode,
    policy: Option<&CoordinationPolicy>,
    revision: WorkspaceRevision,
) -> Option<CoordinationConflict> {
    let overlap = overlapping_anchors(&claim.anchors, anchors);
    if overlap.is_empty() {
        return None;
    }
    let overlap_kinds = overlap_kinds(&overlap);
    let severity = conflict_severity(
        claim.capability,
        claim.mode,
        capability,
        mode,
        &overlap_kinds,
        policy,
        claim.base_revision.clone(),
        revision,
    );
    Some(CoordinationConflict {
        severity,
        summary: format!(
            "claim `{}` conflicts with {:?}/{:?} across {} scope(s)",
            claim.id.0,
            claim.capability,
            claim.mode,
            overlap.len()
        ),
        anchors: overlap,
        overlap_kinds,
        blocking_claims: vec![claim.id.clone()],
    })
}

pub(crate) fn conflict_between(
    left: &WorkClaim,
    right: &WorkClaim,
) -> Option<CoordinationConflict> {
    let overlap = overlapping_anchors(&left.anchors, &right.anchors);
    if overlap.is_empty() {
        return None;
    }
    let overlap_kinds = overlap_kinds(&overlap);
    Some(CoordinationConflict {
        severity: conflict_severity(
            left.capability,
            left.mode,
            right.capability,
            right.mode,
            &overlap_kinds,
            None,
            left.base_revision.clone(),
            right.base_revision.clone(),
        ),
        summary: format!("claims `{}` and `{}` overlap", left.id.0, right.id.0),
        anchors: overlap,
        overlap_kinds,
        blocking_claims: vec![left.id.clone(), right.id.clone()],
    })
}

fn overlapping_anchors(left: &[AnchorRef], right: &[AnchorRef]) -> Vec<AnchorRef> {
    let right = right.iter().collect::<HashSet<_>>();
    let mut overlap = left
        .iter()
        .filter(|anchor| right.contains(anchor))
        .cloned()
        .collect::<Vec<_>>();
    overlap.sort_by_key(anchor_sort_key);
    overlap.dedup();
    overlap
}

fn conflict_severity(
    left_capability: Capability,
    left_mode: ClaimMode,
    right_capability: Capability,
    right_mode: ClaimMode,
    overlap_kinds: &[ConflictOverlapKind],
    policy: Option<&CoordinationPolicy>,
    left_revision: WorkspaceRevision,
    right_revision: WorkspaceRevision,
) -> ConflictSeverity {
    let left_write = matches!(left_capability, Capability::Edit | Capability::Merge);
    let right_write = matches!(right_capability, Capability::Edit | Capability::Merge);
    let node_overlap = overlap_kinds.contains(&ConflictOverlapKind::Node);
    let lineage_overlap = overlap_kinds.contains(&ConflictOverlapKind::Lineage);
    let file_overlap = overlap_kinds.contains(&ConflictOverlapKind::File);
    let kind_only = !node_overlap && !lineage_overlap && !file_overlap;
    let revision_mismatch = left_revision.graph_version != right_revision.graph_version;
    let serialized_by_policy = policy
        .map(|policy| policy.max_parallel_editors_per_anchor <= 1)
        .unwrap_or(false);
    let soft_exclusive = matches!(left_mode, ClaimMode::SoftExclusive)
        || matches!(right_mode, ClaimMode::SoftExclusive);
    if matches!(left_mode, ClaimMode::HardExclusive)
        || matches!(right_mode, ClaimMode::HardExclusive)
    {
        return ConflictSeverity::Block;
    }
    if left_write && right_write {
        if node_overlap && (soft_exclusive || serialized_by_policy) {
            return ConflictSeverity::Block;
        }
        if lineage_overlap && serialized_by_policy {
            return ConflictSeverity::Block;
        }
        if node_overlap || lineage_overlap {
            return ConflictSeverity::Warn;
        }
        if file_overlap {
            return if soft_exclusive || revision_mismatch {
                ConflictSeverity::Warn
            } else {
                ConflictSeverity::Info
            };
        }
        return if kind_only && !revision_mismatch {
            ConflictSeverity::Info
        } else {
            ConflictSeverity::Warn
        };
    }
    if left_write || right_write {
        if node_overlap || lineage_overlap {
            return ConflictSeverity::Warn;
        }
        if file_overlap {
            return if revision_mismatch {
                ConflictSeverity::Warn
            } else {
                ConflictSeverity::Info
            };
        }
    }
    if revision_mismatch {
        return ConflictSeverity::Warn;
    }
    ConflictSeverity::Info
}

pub(crate) fn dedupe_conflicts(
    mut conflicts: Vec<CoordinationConflict>,
) -> Vec<CoordinationConflict> {
    conflicts.sort_by(|left, right| {
        severity_rank(left.severity)
            .cmp(&severity_rank(right.severity))
            .then_with(|| left.summary.cmp(&right.summary))
    });
    conflicts.dedup_by(|left, right| {
        left.severity == right.severity
            && left.summary == right.summary
            && left.blocking_claims == right.blocking_claims
    });
    conflicts
}

fn severity_rank(severity: ConflictSeverity) -> u8 {
    match severity {
        ConflictSeverity::Info => 0,
        ConflictSeverity::Warn => 1,
        ConflictSeverity::Block => 2,
    }
}

fn overlap_kinds(anchors: &[AnchorRef]) -> Vec<ConflictOverlapKind> {
    let mut kinds = anchors
        .iter()
        .map(|anchor| match anchor {
            AnchorRef::Node(_) => ConflictOverlapKind::Node,
            AnchorRef::Lineage(_) => ConflictOverlapKind::Lineage,
            AnchorRef::File(_) | AnchorRef::WorkspacePath(_) => ConflictOverlapKind::File,
            AnchorRef::Kind(_) => ConflictOverlapKind::Kind,
        })
        .collect::<Vec<_>>();
    kinds.sort_by_key(|kind| match kind {
        ConflictOverlapKind::Node => 0,
        ConflictOverlapKind::Lineage => 1,
        ConflictOverlapKind::File => 2,
        ConflictOverlapKind::Kind => 3,
    });
    kinds.dedup();
    kinds
}

pub(crate) fn sorted_values<K, V, F>(values: &HashMap<K, V>, key: F) -> Vec<V>
where
    K: Eq + std::hash::Hash,
    V: Clone,
    F: Fn(&V) -> String,
{
    let mut items = values.values().cloned().collect::<Vec<_>>();
    items.sort_by(|left, right| key(left).cmp(&key(right)));
    items
}

fn anchor_sort_key(anchor: &AnchorRef) -> (u8, String) {
    match anchor {
        AnchorRef::Node(node) => (
            0,
            format!("{}::{}::{:?}", node.crate_name, node.path, node.kind),
        ),
        AnchorRef::Lineage(lineage) => (1, lineage.0.to_string()),
        AnchorRef::File(file_id) => (2, file_id.0.to_string()),
        AnchorRef::WorkspacePath(path) => (2, path.clone()),
        AnchorRef::Kind(kind) => (3, format!("{kind:?}")),
    }
}
