use std::collections::{HashMap, HashSet};

use anyhow::{anyhow, Result};
use prism_ir::{
    AnchorRef, Capability, ClaimMode, ClaimStatus, ConflictSeverity, CoordinationTaskId, EventId,
    SessionId, Timestamp, WorkspaceRevision,
};

use crate::state::CoordinationState;
use crate::types::{
    AcceptanceCriterion, Artifact, CoordinationConflict, CoordinationPolicy, WorkClaim,
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
) -> Vec<CoordinationConflict> {
    if !matches!(capability, Capability::Edit | Capability::Merge) {
        return Vec::new();
    }
    let requested_limit =
        policy.map(|policy| usize::from(policy.max_parallel_editors_per_anchor.max(1)));
    let candidates = state
        .claims
        .values()
        .filter(|claim| claim_is_active(claim, now))
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
    matches!(claim.status, ClaimStatus::Active | ClaimStatus::Contended) && claim.expires_at >= now
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
        .filter_map(|claim| proposal_conflict(claim, anchors, capability, mode, revision.clone()))
        .collect()
}

fn proposal_conflict(
    claim: &WorkClaim,
    anchors: &[AnchorRef],
    capability: Capability,
    mode: ClaimMode,
    revision: WorkspaceRevision,
) -> Option<CoordinationConflict> {
    let overlap = overlapping_anchors(&claim.anchors, anchors);
    if overlap.is_empty() {
        return None;
    }
    let severity = conflict_severity(
        claim.capability,
        claim.mode,
        capability,
        mode,
        claim.base_revision.clone(),
        revision,
    );
    Some(CoordinationConflict {
        severity,
        summary: format!(
            "claim `{}` conflicts with {:?}/{:?} on {} anchor(s)",
            claim.id.0,
            claim.capability,
            claim.mode,
            overlap.len()
        ),
        anchors: overlap,
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
    Some(CoordinationConflict {
        severity: conflict_severity(
            left.capability,
            left.mode,
            right.capability,
            right.mode,
            left.base_revision.clone(),
            right.base_revision.clone(),
        ),
        summary: format!("claims `{}` and `{}` overlap", left.id.0, right.id.0),
        anchors: overlap,
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
    left_revision: WorkspaceRevision,
    right_revision: WorkspaceRevision,
) -> ConflictSeverity {
    let left_write = matches!(left_capability, Capability::Edit | Capability::Merge);
    let right_write = matches!(right_capability, Capability::Edit | Capability::Merge);
    if matches!(left_mode, ClaimMode::HardExclusive)
        || matches!(right_mode, ClaimMode::HardExclusive)
    {
        return ConflictSeverity::Block;
    }
    if left_write && right_write {
        return ConflictSeverity::Warn;
    }
    if left_revision.graph_version != right_revision.graph_version {
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
        AnchorRef::Kind(kind) => (3, format!("{kind:?}")),
    }
}
