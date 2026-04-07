use std::collections::{BTreeMap, BTreeSet};

use anyhow::{anyhow, Result};
use prism_ir::{
    ArtifactStatus, ClaimStatus, CoordinationTaskStatus, GitExecutionStatus, GitIntegrationStatus,
    PlanNodeStatus, PlanStatus, ValidationRef, WorkspaceRevision,
};
use serde_json::{Map, Value};

use crate::git_execution::{GitPreflightReport, GitPublishReport, TaskGitExecution};
use crate::helpers::{validate_plan_transition, validate_task_transition};
use crate::types::{
    AcceptanceCriterion, Artifact, ArtifactReview, CoordinationTask, LeaseHolder, Plan,
    RuntimeDescriptor, WorkClaim,
};

pub fn reconcile_plan_records(
    baseline: &[Plan],
    local: &[Plan],
    remote: &[Plan],
) -> Result<Vec<Plan>> {
    reconcile_records(
        baseline,
        local,
        remote,
        |plan| plan.id.0.as_str(),
        "plan",
        merge_plan,
    )
}

pub fn reconcile_task_records(
    baseline: &[CoordinationTask],
    local: &[CoordinationTask],
    remote: &[CoordinationTask],
) -> Result<Vec<CoordinationTask>> {
    reconcile_records(
        baseline,
        local,
        remote,
        |task| task.id.0.as_str(),
        "task",
        merge_task,
    )
}

pub fn reconcile_claim_records(
    baseline: &[WorkClaim],
    local: &[WorkClaim],
    remote: &[WorkClaim],
) -> Result<Vec<WorkClaim>> {
    reconcile_records(
        baseline,
        local,
        remote,
        |claim| claim.id.0.as_str(),
        "claim",
        merge_claim,
    )
}

pub fn reconcile_artifact_records(
    baseline: &[Artifact],
    local: &[Artifact],
    remote: &[Artifact],
) -> Result<Vec<Artifact>> {
    reconcile_records(
        baseline,
        local,
        remote,
        |artifact| artifact.id.0.as_str(),
        "artifact",
        merge_artifact,
    )
}

pub fn reconcile_review_records(
    baseline: &[ArtifactReview],
    local: &[ArtifactReview],
    remote: &[ArtifactReview],
) -> Result<Vec<ArtifactReview>> {
    reconcile_records(
        baseline,
        local,
        remote,
        |review| review.id.0.as_str(),
        "review",
        merge_review,
    )
}

pub fn reconcile_runtime_descriptor_records(
    baseline: &[RuntimeDescriptor],
    local: &[RuntimeDescriptor],
    remote: &[RuntimeDescriptor],
) -> Result<Vec<RuntimeDescriptor>> {
    reconcile_records(
        baseline,
        local,
        remote,
        |descriptor| descriptor.runtime_id.as_str(),
        "runtime descriptor",
        merge_runtime_descriptor,
    )
}

fn reconcile_records<T, KeyFn, MergeFn>(
    baseline: &[T],
    local: &[T],
    remote: &[T],
    key_for: KeyFn,
    kind: &str,
    merge_fn: MergeFn,
) -> Result<Vec<T>>
where
    T: Clone + PartialEq,
    KeyFn: Fn(&T) -> &str,
    MergeFn: Fn(Option<&T>, &T, &T) -> Result<T>,
{
    let baseline_map = baseline
        .iter()
        .cloned()
        .map(|value| (key_for(&value).to_string(), value))
        .collect::<BTreeMap<_, _>>();
    let local_map = local
        .iter()
        .cloned()
        .map(|value| (key_for(&value).to_string(), value))
        .collect::<BTreeMap<_, _>>();
    let remote_map = remote
        .iter()
        .cloned()
        .map(|value| (key_for(&value).to_string(), value))
        .collect::<BTreeMap<_, _>>();

    let mut result = remote_map.clone();
    let touched_ids = baseline_map
        .keys()
        .chain(local_map.keys())
        .chain(remote_map.keys())
        .cloned()
        .collect::<BTreeSet<_>>();

    for id in touched_ids {
        let baseline_value = baseline_map.get(&id);
        let local_value = local_map.get(&id);
        let remote_value = remote_map.get(&id);
        match (baseline_value, local_value, remote_value) {
            (Some(base), Some(local), Some(remote)) => {
                if local == remote {
                    result.insert(id, local.clone());
                } else if local == base {
                    result.insert(id, remote.clone());
                } else if remote == base {
                    result.insert(id, local.clone());
                } else {
                    result.insert(id, merge_fn(Some(base), local, remote)?);
                }
            }
            (None, Some(local), Some(remote)) => {
                if local == remote {
                    result.insert(id, local.clone());
                } else {
                    result.insert(id, merge_fn(None, local, remote)?);
                }
            }
            (Some(base), Some(local), None) => {
                if local == base {
                    result.remove(&id);
                } else {
                    return Err(semantic_conflict(
                        kind,
                        &id,
                        None,
                        "local deletion conflicts with a concurrent semantic update",
                    ));
                }
            }
            (Some(base), None, Some(remote)) => {
                if remote == base {
                    result.remove(&id);
                } else {
                    return Err(semantic_conflict(
                        kind,
                        &id,
                        None,
                        "remote deletion conflicts with a concurrent semantic update",
                    ));
                }
            }
            (None, Some(local), None) => {
                result.insert(id, local.clone());
            }
            (None, None, Some(remote)) => {
                result.insert(id, remote.clone());
            }
            (Some(_), None, None) | (None, None, None) => {
                result.remove(&id);
            }
        }
    }

    Ok(result.into_values().collect())
}

fn merge_plan(base: Option<&Plan>, local: &Plan, remote: &Plan) -> Result<Plan> {
    let id = local.id.0.as_str();
    ensure_same_identity("plan", id, &local.id.0, &remote.id.0, "id")?;
    ensure_same_identity("plan", id, &local.id.0, &remote.id.0, "id")?;

    Ok(Plan {
        id: local.id.clone(),
        goal: merge_required_scalar(
            base.map(|plan| &plan.goal),
            &local.goal,
            &remote.goal,
            "plan",
            id,
            "goal",
        )?,
        title: merge_required_scalar(
            base.map(|plan| &plan.title),
            &local.title,
            &remote.title,
            "plan",
            id,
            "title",
        )?,
        status: merge_plan_status(
            base.map(|plan| plan.status),
            local.status,
            remote.status,
            id,
        )?,
        policy: merge_required_scalar(
            base.map(|plan| &plan.policy),
            &local.policy,
            &remote.policy,
            "plan",
            id,
            "policy",
        )?,
        scope: merge_required_scalar(
            base.map(|plan| &plan.scope),
            &local.scope,
            &remote.scope,
            "plan",
            id,
            "scope",
        )?,
        kind: merge_required_scalar(
            base.map(|plan| &plan.kind),
            &local.kind,
            &remote.kind,
            "plan",
            id,
            "kind",
        )?,
        revision: local
            .revision
            .max(remote.revision)
            .max(base.map(|plan| plan.revision).unwrap_or(0)),
        scheduling: merge_required_scalar(
            base.map(|plan| &plan.scheduling),
            &local.scheduling,
            &remote.scheduling,
            "plan",
            id,
            "scheduling",
        )?,
        tags: merge_union_vec(
            base.map(|plan| plan.tags.as_slice()),
            &local.tags,
            &remote.tags,
        ),
        created_from: merge_optional_scalar(
            base.and_then(|plan| plan.created_from.as_ref()),
            local.created_from.as_ref(),
            remote.created_from.as_ref(),
            "plan",
            id,
            "created_from",
        )?,
        metadata: merge_json_value(
            base.map(|plan| &plan.metadata),
            &local.metadata,
            &remote.metadata,
        ),
        authored_nodes: merge_plan_nodes(
            base.map(|plan| plan.authored_nodes.as_slice()),
            &local.authored_nodes,
            &remote.authored_nodes,
            id,
        )?,
        authored_edges: merge_plan_edges(
            base.map(|plan| plan.authored_edges.as_slice()),
            &local.authored_edges,
            &remote.authored_edges,
            id,
        )?,
        root_tasks: merge_union_vec(
            base.map(|plan| plan.root_tasks.as_slice()),
            &local.root_tasks,
            &remote.root_tasks,
        ),
    })
}

fn merge_task(
    base: Option<&CoordinationTask>,
    local: &CoordinationTask,
    remote: &CoordinationTask,
) -> Result<CoordinationTask> {
    let id = local.id.0.as_str();
    ensure_same_identity("task", id, &local.id.0, &remote.id.0, "id")?;
    ensure_same_identity("task", id, &local.plan.0, &remote.plan.0, "plan")?;

    let lease_holder = merge_lease_holder(
        "task",
        id,
        base.and_then(|task| task.lease_holder.as_ref()),
        local.lease_holder.as_ref(),
        remote.lease_holder.as_ref(),
    )?;

    Ok(CoordinationTask {
        id: local.id.clone(),
        plan: local.plan.clone(),
        kind: merge_required_scalar(
            base.map(|task| &task.kind),
            &local.kind,
            &remote.kind,
            "task",
            id,
            "kind",
        )?,
        title: merge_required_scalar(
            base.map(|task| &task.title),
            &local.title,
            &remote.title,
            "task",
            id,
            "title",
        )?,
        summary: merge_optional_scalar(
            base.and_then(|task| task.summary.as_ref()),
            local.summary.as_ref(),
            remote.summary.as_ref(),
            "task",
            id,
            "summary",
        )?,
        status: merge_task_status(
            base.map(|task| task.status),
            local.status,
            remote.status,
            id,
        )?,
        published_task_status: merge_optional_task_status(
            base.and_then(|task| task.published_task_status),
            local.published_task_status,
            remote.published_task_status,
            id,
            "published_task_status",
        )?,
        assignee: merge_optional_scalar(
            base.and_then(|task| task.assignee.as_ref()),
            local.assignee.as_ref(),
            remote.assignee.as_ref(),
            "task",
            id,
            "assignee",
        )?,
        pending_handoff_to: merge_optional_scalar(
            base.and_then(|task| task.pending_handoff_to.as_ref()),
            local.pending_handoff_to.as_ref(),
            remote.pending_handoff_to.as_ref(),
            "task",
            id,
            "pending_handoff_to",
        )?,
        session: merge_optional_scalar(
            base.and_then(|task| task.session.as_ref()),
            local.session.as_ref(),
            remote.session.as_ref(),
            "task",
            id,
            "session",
        )?,
        lease_holder,
        lease_started_at: merge_max_timestamp(
            base.and_then(|task| task.lease_started_at),
            local.lease_started_at,
            remote.lease_started_at,
        ),
        lease_refreshed_at: merge_max_timestamp(
            base.and_then(|task| task.lease_refreshed_at),
            local.lease_refreshed_at,
            remote.lease_refreshed_at,
        ),
        lease_stale_at: merge_max_timestamp(
            base.and_then(|task| task.lease_stale_at),
            local.lease_stale_at,
            remote.lease_stale_at,
        ),
        lease_expires_at: merge_max_timestamp(
            base.and_then(|task| task.lease_expires_at),
            local.lease_expires_at,
            remote.lease_expires_at,
        ),
        worktree_id: merge_optional_scalar(
            base.and_then(|task| task.worktree_id.as_ref()),
            local.worktree_id.as_ref(),
            remote.worktree_id.as_ref(),
            "task",
            id,
            "worktree_id",
        )?,
        branch_ref: merge_optional_scalar(
            base.and_then(|task| task.branch_ref.as_ref()),
            local.branch_ref.as_ref(),
            remote.branch_ref.as_ref(),
            "task",
            id,
            "branch_ref",
        )?,
        anchors: merge_union_vec(
            base.map(|task| task.anchors.as_slice()),
            &local.anchors,
            &remote.anchors,
        ),
        bindings: merge_plan_binding(
            base.map(|task| &task.bindings),
            &local.bindings,
            &remote.bindings,
        ),
        depends_on: merge_union_vec(
            base.map(|task| task.depends_on.as_slice()),
            &local.depends_on,
            &remote.depends_on,
        ),
        coordination_depends_on: merge_union_vec(
            base.map(|task| task.coordination_depends_on.as_slice()),
            &local.coordination_depends_on,
            &remote.coordination_depends_on,
        ),
        integrated_depends_on: merge_union_vec(
            base.map(|task| task.integrated_depends_on.as_slice()),
            &local.integrated_depends_on,
            &remote.integrated_depends_on,
        ),
        acceptance: merge_acceptance_criteria(
            base.map(|task| task.acceptance.as_slice()),
            &local.acceptance,
            &remote.acceptance,
        ),
        validation_refs: merge_validation_refs(
            base.map(|task| task.validation_refs.as_slice()),
            &local.validation_refs,
            &remote.validation_refs,
        ),
        is_abstract: merge_bool_scalar(
            base.map(|task| task.is_abstract),
            local.is_abstract,
            remote.is_abstract,
        ),
        base_revision: merge_workspace_revision(
            base.map(|task| &task.base_revision),
            &local.base_revision,
            &remote.base_revision,
        ),
        priority: merge_optional_copy(
            base.and_then(|task| task.priority),
            local.priority,
            remote.priority,
            "task",
            id,
            "priority",
        )?,
        tags: merge_union_vec(
            base.map(|task| task.tags.as_slice()),
            &local.tags,
            &remote.tags,
        ),
        metadata: merge_json_value(
            base.map(|task| &task.metadata),
            &local.metadata,
            &remote.metadata,
        ),
        git_execution: merge_task_git_execution(
            base.map(|task| &task.git_execution),
            &local.git_execution,
            &remote.git_execution,
            id,
        )?,
    })
}

fn merge_claim(
    base: Option<&WorkClaim>,
    local: &WorkClaim,
    remote: &WorkClaim,
) -> Result<WorkClaim> {
    let id = local.id.0.as_str();
    ensure_same_identity("claim", id, &local.id.0, &remote.id.0, "id")?;
    ensure_same_identity("claim", id, &local.holder.0, &remote.holder.0, "holder")?;
    ensure_same_identity(
        "claim",
        id,
        &format!("{:?}", local.capability),
        &format!("{:?}", remote.capability),
        "capability",
    )?;
    ensure_same_identity(
        "claim",
        id,
        &format!("{:?}", local.mode),
        &format!("{:?}", remote.mode),
        "mode",
    )?;
    if local.anchors != remote.anchors {
        return Err(semantic_conflict(
            "claim",
            id,
            Some("anchors"),
            "claim scope changed concurrently and cannot be merged safely",
        ));
    }

    Ok(WorkClaim {
        id: local.id.clone(),
        holder: local.holder.clone(),
        agent: merge_optional_scalar(
            base.and_then(|claim| claim.agent.as_ref()),
            local.agent.as_ref(),
            remote.agent.as_ref(),
            "claim",
            id,
            "agent",
        )?,
        lease_holder: merge_lease_holder(
            "claim",
            id,
            base.and_then(|claim| claim.lease_holder.as_ref()),
            local.lease_holder.as_ref(),
            remote.lease_holder.as_ref(),
        )?,
        worktree_id: merge_optional_scalar(
            base.and_then(|claim| claim.worktree_id.as_ref()),
            local.worktree_id.as_ref(),
            remote.worktree_id.as_ref(),
            "claim",
            id,
            "worktree_id",
        )?,
        branch_ref: merge_optional_scalar(
            base.and_then(|claim| claim.branch_ref.as_ref()),
            local.branch_ref.as_ref(),
            remote.branch_ref.as_ref(),
            "claim",
            id,
            "branch_ref",
        )?,
        task: merge_optional_scalar(
            base.and_then(|claim| claim.task.as_ref()),
            local.task.as_ref(),
            remote.task.as_ref(),
            "claim",
            id,
            "task",
        )?,
        anchors: local.anchors.clone(),
        capability: local.capability,
        mode: local.mode,
        since: local
            .since
            .min(remote.since)
            .min(base.map(|claim| claim.since).unwrap_or(local.since)),
        refreshed_at: merge_max_timestamp(
            base.and_then(|claim| claim.refreshed_at),
            local.refreshed_at,
            remote.refreshed_at,
        ),
        stale_at: merge_max_timestamp(
            base.and_then(|claim| claim.stale_at),
            local.stale_at,
            remote.stale_at,
        ),
        expires_at: local
            .expires_at
            .max(remote.expires_at)
            .max(base.map(|claim| claim.expires_at).unwrap_or(0)),
        status: merge_claim_status(
            base.map(|claim| claim.status),
            local.status,
            remote.status,
            id,
        )?,
        base_revision: merge_workspace_revision(
            base.map(|claim| &claim.base_revision),
            &local.base_revision,
            &remote.base_revision,
        ),
    })
}

fn merge_artifact(
    base: Option<&Artifact>,
    local: &Artifact,
    remote: &Artifact,
) -> Result<Artifact> {
    let id = local.id.0.as_str();
    ensure_same_identity("artifact", id, &local.id.0, &remote.id.0, "id")?;
    ensure_same_identity("artifact", id, &local.task.0, &remote.task.0, "task")?;

    Ok(Artifact {
        id: local.id.clone(),
        task: local.task.clone(),
        worktree_id: merge_optional_scalar(
            base.and_then(|artifact| artifact.worktree_id.as_ref()),
            local.worktree_id.as_ref(),
            remote.worktree_id.as_ref(),
            "artifact",
            id,
            "worktree_id",
        )?,
        branch_ref: merge_optional_scalar(
            base.and_then(|artifact| artifact.branch_ref.as_ref()),
            local.branch_ref.as_ref(),
            remote.branch_ref.as_ref(),
            "artifact",
            id,
            "branch_ref",
        )?,
        anchors: merge_union_vec(
            base.map(|artifact| artifact.anchors.as_slice()),
            &local.anchors,
            &remote.anchors,
        ),
        base_revision: merge_workspace_revision(
            base.map(|artifact| &artifact.base_revision),
            &local.base_revision,
            &remote.base_revision,
        ),
        diff_ref: merge_optional_scalar(
            base.and_then(|artifact| artifact.diff_ref.as_ref()),
            local.diff_ref.as_ref(),
            remote.diff_ref.as_ref(),
            "artifact",
            id,
            "diff_ref",
        )?,
        status: merge_artifact_status(
            base.map(|artifact| artifact.status),
            local.status,
            remote.status,
            id,
        )?,
        evidence: merge_union_vec(
            base.map(|artifact| artifact.evidence.as_slice()),
            &local.evidence,
            &remote.evidence,
        ),
        reviews: merge_union_vec(
            base.map(|artifact| artifact.reviews.as_slice()),
            &local.reviews,
            &remote.reviews,
        ),
        required_validations: merge_union_vec(
            base.map(|artifact| artifact.required_validations.as_slice()),
            &local.required_validations,
            &remote.required_validations,
        ),
        validated_checks: merge_union_vec(
            base.map(|artifact| artifact.validated_checks.as_slice()),
            &local.validated_checks,
            &remote.validated_checks,
        ),
        risk_score: merge_optional_f32(
            base.and_then(|artifact| artifact.risk_score),
            local.risk_score,
            remote.risk_score,
        ),
    })
}

fn merge_review(
    base: Option<&ArtifactReview>,
    local: &ArtifactReview,
    remote: &ArtifactReview,
) -> Result<ArtifactReview> {
    let id = local.id.0.as_str();
    ensure_same_identity("review", id, &local.id.0, &remote.id.0, "id")?;
    ensure_same_identity(
        "review",
        id,
        &local.artifact.0,
        &remote.artifact.0,
        "artifact",
    )?;

    Ok(ArtifactReview {
        id: local.id.clone(),
        artifact: local.artifact.clone(),
        verdict: merge_required_scalar(
            base.map(|review| &review.verdict),
            &local.verdict,
            &remote.verdict,
            "review",
            id,
            "verdict",
        )?,
        summary: merge_required_scalar(
            base.map(|review| &review.summary),
            &local.summary,
            &remote.summary,
            "review",
            id,
            "summary",
        )?,
        meta: merge_required_scalar(
            base.map(|review| &review.meta),
            &local.meta,
            &remote.meta,
            "review",
            id,
            "meta",
        )?,
    })
}

fn merge_runtime_descriptor(
    base: Option<&RuntimeDescriptor>,
    local: &RuntimeDescriptor,
    remote: &RuntimeDescriptor,
) -> Result<RuntimeDescriptor> {
    let id = local.runtime_id.as_str();
    ensure_same_identity(
        "runtime descriptor",
        id,
        &local.runtime_id,
        &remote.runtime_id,
        "runtime_id",
    )?;
    ensure_same_identity(
        "runtime descriptor",
        id,
        &local.repo_id,
        &remote.repo_id,
        "repo_id",
    )?;
    ensure_same_identity(
        "runtime descriptor",
        id,
        &local.worktree_id,
        &remote.worktree_id,
        "worktree_id",
    )?;
    ensure_same_identity(
        "runtime descriptor",
        id,
        &local.principal_id,
        &remote.principal_id,
        "principal_id",
    )?;

    let preferred = if local.last_seen_at >= remote.last_seen_at {
        local
    } else {
        remote
    };

    Ok(RuntimeDescriptor {
        runtime_id: local.runtime_id.clone(),
        repo_id: local.repo_id.clone(),
        worktree_id: local.worktree_id.clone(),
        principal_id: local.principal_id.clone(),
        instance_started_at: local
            .instance_started_at
            .min(remote.instance_started_at)
            .min(
                base.map(|descriptor| descriptor.instance_started_at)
                    .unwrap_or(local.instance_started_at),
            ),
        last_seen_at: local
            .last_seen_at
            .max(remote.last_seen_at)
            .max(base.map(|descriptor| descriptor.last_seen_at).unwrap_or(0)),
        branch_ref: prefer_fresher_optional(
            base.and_then(|descriptor| descriptor.branch_ref.as_ref()),
            local.branch_ref.as_ref(),
            remote.branch_ref.as_ref(),
            preferred.last_seen_at == local.last_seen_at,
        ),
        checked_out_commit: prefer_fresher_optional(
            base.and_then(|descriptor| descriptor.checked_out_commit.as_ref()),
            local.checked_out_commit.as_ref(),
            remote.checked_out_commit.as_ref(),
            preferred.last_seen_at == local.last_seen_at,
        ),
        capabilities: merge_union_vec(
            base.map(|descriptor| descriptor.capabilities.as_slice()),
            &local.capabilities,
            &remote.capabilities,
        ),
        discovery_mode: if preferred.last_seen_at == local.last_seen_at {
            local.discovery_mode
        } else {
            remote.discovery_mode
        },
        peer_endpoint: prefer_fresher_optional(
            base.and_then(|descriptor| descriptor.peer_endpoint.as_ref()),
            local.peer_endpoint.as_ref(),
            remote.peer_endpoint.as_ref(),
            preferred.last_seen_at == local.last_seen_at,
        ),
        public_endpoint: prefer_fresher_optional(
            base.and_then(|descriptor| descriptor.public_endpoint.as_ref()),
            local.public_endpoint.as_ref(),
            remote.public_endpoint.as_ref(),
            preferred.last_seen_at == local.last_seen_at,
        ),
        peer_transport_identity: prefer_fresher_optional(
            base.and_then(|descriptor| descriptor.peer_transport_identity.as_ref()),
            local.peer_transport_identity.as_ref(),
            remote.peer_transport_identity.as_ref(),
            preferred.last_seen_at == local.last_seen_at,
        ),
        blob_snapshot_head: prefer_fresher_optional(
            base.and_then(|descriptor| descriptor.blob_snapshot_head.as_ref()),
            local.blob_snapshot_head.as_ref(),
            remote.blob_snapshot_head.as_ref(),
            preferred.last_seen_at == local.last_seen_at,
        ),
        export_policy: prefer_fresher_optional(
            base.and_then(|descriptor| descriptor.export_policy.as_ref()),
            local.export_policy.as_ref(),
            remote.export_policy.as_ref(),
            preferred.last_seen_at == local.last_seen_at,
        ),
    })
}

fn merge_plan_edges(
    base: Option<&[prism_ir::PlanEdge]>,
    local: &[prism_ir::PlanEdge],
    remote: &[prism_ir::PlanEdge],
    record_id: &str,
) -> Result<Vec<prism_ir::PlanEdge>> {
    reconcile_records(
        base.unwrap_or_default(),
        local,
        remote,
        |edge| edge.id.0.as_str(),
        "plan edge",
        |base, local, remote| {
            let edge_id = local.id.0.as_str();
            ensure_same_identity("plan edge", edge_id, &local.id.0, &remote.id.0, "id")?;
            ensure_same_identity(
                "plan edge",
                edge_id,
                &local.plan_id.0,
                &remote.plan_id.0,
                "plan_id",
            )?;
            ensure_same_identity("plan edge", edge_id, &local.from.0, &remote.from.0, "from")?;
            ensure_same_identity("plan edge", edge_id, &local.to.0, &remote.to.0, "to")?;
            ensure_same_identity(
                "plan edge",
                edge_id,
                &format!("{:?}", local.kind),
                &format!("{:?}", remote.kind),
                "kind",
            )?;
            Ok(prism_ir::PlanEdge {
                id: local.id.clone(),
                plan_id: local.plan_id.clone(),
                from: local.from.clone(),
                to: local.to.clone(),
                kind: local.kind,
                summary: merge_optional_scalar(
                    base.and_then(|edge| edge.summary.as_ref()),
                    local.summary.as_ref(),
                    remote.summary.as_ref(),
                    "plan edge",
                    edge_id,
                    "summary",
                )?,
                metadata: merge_json_value(
                    base.map(|edge| &edge.metadata),
                    &local.metadata,
                    &remote.metadata,
                ),
            })
        },
    )
    .map_err(|error| anyhow!("shared coordination plan `{record_id}` edge merge failed: {error}"))
}

fn merge_plan_nodes(
    base: Option<&[prism_ir::PlanNode]>,
    local: &[prism_ir::PlanNode],
    remote: &[prism_ir::PlanNode],
    record_id: &str,
) -> Result<Vec<prism_ir::PlanNode>> {
    reconcile_records(
        base.unwrap_or_default(),
        local,
        remote,
        |node| node.id.0.as_str(),
        "plan node",
        |base, local, remote| {
            let node_id = local.id.0.as_str();
            ensure_same_identity("plan node", node_id, &local.id.0, &remote.id.0, "id")?;
            ensure_same_identity(
                "plan node",
                node_id,
                &local.plan_id.0,
                &remote.plan_id.0,
                "plan_id",
            )?;
            Ok(prism_ir::PlanNode {
                id: local.id.clone(),
                plan_id: local.plan_id.clone(),
                kind: merge_required_scalar(
                    base.map(|node| &node.kind),
                    &local.kind,
                    &remote.kind,
                    "plan node",
                    node_id,
                    "kind",
                )?,
                title: merge_required_scalar(
                    base.map(|node| &node.title),
                    &local.title,
                    &remote.title,
                    "plan node",
                    node_id,
                    "title",
                )?,
                summary: merge_optional_scalar(
                    base.and_then(|node| node.summary.as_ref()),
                    local.summary.as_ref(),
                    remote.summary.as_ref(),
                    "plan node",
                    node_id,
                    "summary",
                )?,
                status: merge_plan_node_status(
                    base.map(|node| node.status),
                    local.status,
                    remote.status,
                    node_id,
                )?,
                bindings: merge_plan_binding(
                    base.map(|node| &node.bindings),
                    &local.bindings,
                    &remote.bindings,
                ),
                acceptance: merge_union_vec(
                    base.map(|node| node.acceptance.as_slice()),
                    &local.acceptance,
                    &remote.acceptance,
                ),
                validation_refs: merge_validation_refs(
                    base.map(|node| node.validation_refs.as_slice()),
                    &local.validation_refs,
                    &remote.validation_refs,
                ),
                is_abstract: merge_bool_scalar(
                    base.map(|node| node.is_abstract),
                    local.is_abstract,
                    remote.is_abstract,
                ),
                assignee: merge_optional_scalar(
                    base.and_then(|node| node.assignee.as_ref()),
                    local.assignee.as_ref(),
                    remote.assignee.as_ref(),
                    "plan node",
                    node_id,
                    "assignee",
                )?,
                base_revision: merge_workspace_revision(
                    base.map(|node| &node.base_revision),
                    &local.base_revision,
                    &remote.base_revision,
                ),
                priority: merge_optional_copy(
                    base.and_then(|node| node.priority),
                    local.priority,
                    remote.priority,
                    "plan node",
                    node_id,
                    "priority",
                )?,
                tags: merge_union_vec(
                    base.map(|node| node.tags.as_slice()),
                    &local.tags,
                    &remote.tags,
                ),
                metadata: merge_json_value(
                    base.map(|node| &node.metadata),
                    &local.metadata,
                    &remote.metadata,
                ),
            })
        },
    )
    .map_err(|error| anyhow!("shared coordination plan `{record_id}` node merge failed: {error}"))
}

fn merge_task_git_execution(
    base: Option<&TaskGitExecution>,
    local: &TaskGitExecution,
    remote: &TaskGitExecution,
    task_id: &str,
) -> Result<TaskGitExecution> {
    Ok(TaskGitExecution {
        status: merge_git_execution_status(
            base.map(|git| git.status),
            local.status,
            remote.status,
            task_id,
        )?,
        pending_task_status: merge_optional_task_status(
            base.and_then(|git| git.pending_task_status),
            local.pending_task_status,
            remote.pending_task_status,
            task_id,
            "git_execution.pending_task_status",
        )?,
        source_ref: merge_optional_scalar(
            base.and_then(|git| git.source_ref.as_ref()),
            local.source_ref.as_ref(),
            remote.source_ref.as_ref(),
            "task",
            task_id,
            "git_execution.source_ref",
        )?,
        target_ref: merge_optional_scalar(
            base.and_then(|git| git.target_ref.as_ref()),
            local.target_ref.as_ref(),
            remote.target_ref.as_ref(),
            "task",
            task_id,
            "git_execution.target_ref",
        )?,
        publish_ref: merge_optional_scalar(
            base.and_then(|git| git.publish_ref.as_ref()),
            local.publish_ref.as_ref(),
            remote.publish_ref.as_ref(),
            "task",
            task_id,
            "git_execution.publish_ref",
        )?,
        target_branch: merge_optional_scalar(
            base.and_then(|git| git.target_branch.as_ref()),
            local.target_branch.as_ref(),
            remote.target_branch.as_ref(),
            "task",
            task_id,
            "git_execution.target_branch",
        )?,
        source_commit: merge_optional_scalar(
            base.and_then(|git| git.source_commit.as_ref()),
            local.source_commit.as_ref(),
            remote.source_commit.as_ref(),
            "task",
            task_id,
            "git_execution.source_commit",
        )?,
        publish_commit: merge_optional_scalar(
            base.and_then(|git| git.publish_commit.as_ref()),
            local.publish_commit.as_ref(),
            remote.publish_commit.as_ref(),
            "task",
            task_id,
            "git_execution.publish_commit",
        )?,
        target_commit_at_publish: merge_optional_scalar(
            base.and_then(|git| git.target_commit_at_publish.as_ref()),
            local.target_commit_at_publish.as_ref(),
            remote.target_commit_at_publish.as_ref(),
            "task",
            task_id,
            "git_execution.target_commit_at_publish",
        )?,
        review_artifact_ref: merge_optional_scalar(
            base.and_then(|git| git.review_artifact_ref.as_ref()),
            local.review_artifact_ref.as_ref(),
            remote.review_artifact_ref.as_ref(),
            "task",
            task_id,
            "git_execution.review_artifact_ref",
        )?,
        integration_commit: merge_optional_scalar(
            base.and_then(|git| git.integration_commit.as_ref()),
            local.integration_commit.as_ref(),
            remote.integration_commit.as_ref(),
            "task",
            task_id,
            "git_execution.integration_commit",
        )?,
        integration_evidence: merge_optional_scalar(
            base.and_then(|git| git.integration_evidence.as_ref()),
            local.integration_evidence.as_ref(),
            remote.integration_evidence.as_ref(),
            "task",
            task_id,
            "git_execution.integration_evidence",
        )?,
        integration_mode: merge_required_scalar(
            base.map(|git| &git.integration_mode),
            &local.integration_mode,
            &remote.integration_mode,
            "task",
            task_id,
            "git_execution.integration_mode",
        )?,
        integration_status: merge_git_integration_status(
            base.map(|git| git.integration_status),
            local.integration_status,
            remote.integration_status,
            task_id,
        )?,
        last_preflight: merge_preflight_report(
            base.and_then(|git| git.last_preflight.as_ref()),
            local.last_preflight.as_ref(),
            remote.last_preflight.as_ref(),
        ),
        last_publish: merge_publish_report(
            base.and_then(|git| git.last_publish.as_ref()),
            local.last_publish.as_ref(),
            remote.last_publish.as_ref(),
        ),
    })
}

fn merge_preflight_report(
    base: Option<&GitPreflightReport>,
    local: Option<&GitPreflightReport>,
    remote: Option<&GitPreflightReport>,
) -> Option<GitPreflightReport> {
    merge_observation_report(base, local, remote, |report| report.checked_at)
}

fn merge_publish_report(
    base: Option<&GitPublishReport>,
    local: Option<&GitPublishReport>,
    remote: Option<&GitPublishReport>,
) -> Option<GitPublishReport> {
    merge_observation_report(base, local, remote, |report| report.attempted_at)
}

fn merge_observation_report<T, F>(
    base: Option<&T>,
    local: Option<&T>,
    remote: Option<&T>,
    timestamp_for: F,
) -> Option<T>
where
    T: Clone + PartialEq,
    F: Fn(&T) -> u64,
{
    match (base, local, remote) {
        (_, Some(local), Some(remote)) if local == remote => Some(local.clone()),
        (Some(base), Some(local), Some(remote)) if local == base => Some(remote.clone()),
        (Some(base), Some(local), Some(remote)) if remote == base => Some(local.clone()),
        (_, Some(local), Some(remote)) => {
            if timestamp_for(local) >= timestamp_for(remote) {
                Some(local.clone())
            } else {
                Some(remote.clone())
            }
        }
        (_, Some(local), None) => Some(local.clone()),
        (_, None, Some(remote)) => Some(remote.clone()),
        _ => None,
    }
}

fn merge_plan_binding(
    base: Option<&prism_ir::PlanBinding>,
    local: &prism_ir::PlanBinding,
    remote: &prism_ir::PlanBinding,
) -> prism_ir::PlanBinding {
    prism_ir::PlanBinding {
        anchors: merge_union_vec(
            base.map(|binding| binding.anchors.as_slice()),
            &local.anchors,
            &remote.anchors,
        ),
        concept_handles: merge_union_vec(
            base.map(|binding| binding.concept_handles.as_slice()),
            &local.concept_handles,
            &remote.concept_handles,
        ),
        artifact_refs: merge_union_vec(
            base.map(|binding| binding.artifact_refs.as_slice()),
            &local.artifact_refs,
            &remote.artifact_refs,
        ),
        memory_refs: merge_union_vec(
            base.map(|binding| binding.memory_refs.as_slice()),
            &local.memory_refs,
            &remote.memory_refs,
        ),
        outcome_refs: merge_union_vec(
            base.map(|binding| binding.outcome_refs.as_slice()),
            &local.outcome_refs,
            &remote.outcome_refs,
        ),
    }
}

fn merge_acceptance_criteria(
    base: Option<&[AcceptanceCriterion]>,
    local: &[AcceptanceCriterion],
    remote: &[AcceptanceCriterion],
) -> Vec<AcceptanceCriterion> {
    let mut merged = BTreeMap::<String, AcceptanceCriterion>::new();
    for criterion in base.unwrap_or_default() {
        merged.insert(criterion.label.clone(), criterion.clone());
    }
    for criterion in local {
        merged
            .entry(criterion.label.clone())
            .and_modify(|existing| {
                existing.anchors = merge_union_vec(
                    Some(existing.anchors.as_slice()),
                    &existing.anchors,
                    &criterion.anchors,
                );
            })
            .or_insert_with(|| criterion.clone());
    }
    for criterion in remote {
        merged
            .entry(criterion.label.clone())
            .and_modify(|existing| {
                existing.anchors = merge_union_vec(
                    Some(existing.anchors.as_slice()),
                    &existing.anchors,
                    &criterion.anchors,
                );
            })
            .or_insert_with(|| criterion.clone());
    }
    merged.into_values().collect()
}

fn merge_validation_refs(
    base: Option<&[ValidationRef]>,
    local: &[ValidationRef],
    remote: &[ValidationRef],
) -> Vec<ValidationRef> {
    merge_union_vec(base, local, remote)
}

fn merge_workspace_revision(
    base: Option<&WorkspaceRevision>,
    local: &WorkspaceRevision,
    remote: &WorkspaceRevision,
) -> WorkspaceRevision {
    if local == remote {
        return local.clone();
    }
    if base.is_some_and(|base| local == base) {
        return remote.clone();
    }
    if base.is_some_and(|base| remote == base) {
        return local.clone();
    }
    if local.graph_version > remote.graph_version {
        return local.clone();
    }
    if remote.graph_version > local.graph_version {
        return remote.clone();
    }
    if local.git_commit.is_some() && remote.git_commit.is_none() {
        return local.clone();
    }
    if remote.git_commit.is_some() && local.git_commit.is_none() {
        return remote.clone();
    }
    local.clone()
}

fn merge_json_value(base: Option<&Value>, local: &Value, remote: &Value) -> Value {
    if local == remote {
        return local.clone();
    }
    if base.is_some_and(|base| local == base) {
        return remote.clone();
    }
    if base.is_some_and(|base| remote == base) {
        return local.clone();
    }
    match (
        base.and_then(Value::as_object),
        local.as_object(),
        remote.as_object(),
    ) {
        (base_map, Some(local_map), Some(remote_map)) => {
            Value::Object(merge_json_object(base_map, local_map, remote_map))
        }
        _ => local.clone(),
    }
}

fn merge_json_object(
    base: Option<&Map<String, Value>>,
    local: &Map<String, Value>,
    remote: &Map<String, Value>,
) -> Map<String, Value> {
    let mut merged = Map::new();
    let keys = base
        .into_iter()
        .flat_map(|map| map.keys())
        .chain(local.keys())
        .chain(remote.keys())
        .cloned()
        .collect::<BTreeSet<_>>();
    for key in keys {
        let base_value = base.and_then(|map| map.get(&key));
        match (base_value, local.get(&key), remote.get(&key)) {
            (_, Some(local_value), Some(remote_value)) => {
                merged.insert(key, merge_json_value(base_value, local_value, remote_value));
            }
            (_, Some(local_value), None) => {
                merged.insert(key, local_value.clone());
            }
            (_, None, Some(remote_value)) => {
                merged.insert(key, remote_value.clone());
            }
            _ => {}
        }
    }
    merged
}

fn merge_union_vec<T>(base: Option<&[T]>, local: &[T], remote: &[T]) -> Vec<T>
where
    T: Clone + PartialEq,
{
    let mut merged = Vec::new();
    for value in base.unwrap_or_default() {
        if !merged.contains(value) {
            merged.push(value.clone());
        }
    }
    for source in [local, remote] {
        for value in source {
            if !merged.contains(value) {
                merged.push(value.clone());
            }
        }
    }
    merged
}

fn merge_required_scalar<T>(
    base: Option<&T>,
    local: &T,
    remote: &T,
    kind: &str,
    id: &str,
    field: &str,
) -> Result<T>
where
    T: Clone + PartialEq,
{
    if local == remote {
        Ok(local.clone())
    } else if base.is_some_and(|base| local == base) {
        Ok(remote.clone())
    } else if base.is_some_and(|base| remote == base) {
        Ok(local.clone())
    } else {
        Err(semantic_conflict(
            kind,
            id,
            Some(field),
            "concurrent edits changed this field incompatibly",
        ))
    }
}

fn merge_optional_scalar<T>(
    base: Option<&T>,
    local: Option<&T>,
    remote: Option<&T>,
    kind: &str,
    id: &str,
    field: &str,
) -> Result<Option<T>>
where
    T: Clone + PartialEq,
{
    match (base, local, remote) {
        (_, Some(local), Some(remote)) if local == remote => Ok(Some(local.clone())),
        (Some(base), Some(local), Some(remote)) if local == base => Ok(Some(remote.clone())),
        (Some(base), Some(local), Some(remote)) if remote == base => Ok(Some(local.clone())),
        (None, Some(local), None) => Ok(Some(local.clone())),
        (None, None, Some(remote)) => Ok(Some(remote.clone())),
        (Some(base), None, None) => Ok(Some(base.clone())).and_then(|_| Ok(None)),
        (Some(base), None, Some(remote)) if remote == base => Ok(None),
        (Some(base), Some(local), None) if local == base => Ok(None),
        (None, None, None) => Ok(None),
        _ => Err(semantic_conflict(
            kind,
            id,
            Some(field),
            "concurrent edits changed this optional field incompatibly",
        )),
    }
}

fn merge_optional_copy<T>(
    base: Option<T>,
    local: Option<T>,
    remote: Option<T>,
    kind: &str,
    id: &str,
    field: &str,
) -> Result<Option<T>>
where
    T: Copy + PartialEq,
{
    match (base, local, remote) {
        (_, Some(local), Some(remote)) if local == remote => Ok(Some(local)),
        (Some(base), Some(local), Some(remote)) if local == base => Ok(Some(remote)),
        (Some(base), Some(local), Some(remote)) if remote == base => Ok(Some(local)),
        (None, Some(local), None) => Ok(Some(local)),
        (None, None, Some(remote)) => Ok(Some(remote)),
        (Some(base), None, Some(remote)) if remote == base => Ok(None),
        (Some(base), Some(local), None) if local == base => Ok(None),
        (Some(_), None, None) | (None, None, None) => Ok(None),
        _ => Err(semantic_conflict(
            kind,
            id,
            Some(field),
            "concurrent edits changed this optional field incompatibly",
        )),
    }
}

fn merge_bool_scalar(base: Option<bool>, local: bool, remote: bool) -> bool {
    if local == remote {
        local
    } else if base.is_some_and(|base| local == base) {
        remote
    } else {
        local
    }
}

fn merge_optional_f32(base: Option<f32>, local: Option<f32>, remote: Option<f32>) -> Option<f32> {
    match (base, local, remote) {
        (_, Some(local), Some(remote)) if (local - remote).abs() < f32::EPSILON => Some(local),
        (_, Some(local), Some(remote)) => Some(local.max(remote)),
        (_, Some(local), None) => Some(local),
        (_, None, Some(remote)) => Some(remote),
        _ => None,
    }
}

fn merge_max_timestamp(base: Option<u64>, local: Option<u64>, remote: Option<u64>) -> Option<u64> {
    base.into_iter().chain(local).chain(remote).max()
}

fn merge_plan_status(
    base: Option<PlanStatus>,
    local: PlanStatus,
    remote: PlanStatus,
    id: &str,
) -> Result<PlanStatus> {
    merge_lifecycle_status(
        "plan",
        id,
        "status",
        base,
        local,
        remote,
        |from, to| validate_plan_transition(from, to).is_ok(),
        plan_status_rank,
    )
}

fn merge_plan_node_status(
    base: Option<PlanNodeStatus>,
    local: PlanNodeStatus,
    remote: PlanNodeStatus,
    id: &str,
) -> Result<PlanNodeStatus> {
    merge_precedence_status(
        "plan node",
        id,
        "status",
        base,
        local,
        remote,
        plan_node_status_rank,
        |left, right| plan_node_status_rank(left) <= plan_node_status_rank(right),
    )
}

fn merge_task_status(
    base: Option<CoordinationTaskStatus>,
    local: CoordinationTaskStatus,
    remote: CoordinationTaskStatus,
    id: &str,
) -> Result<CoordinationTaskStatus> {
    merge_lifecycle_status(
        "task",
        id,
        "status",
        base,
        local,
        remote,
        |from, to| validate_task_transition(from, to).is_ok(),
        task_status_rank,
    )
}

fn merge_optional_task_status(
    base: Option<CoordinationTaskStatus>,
    local: Option<CoordinationTaskStatus>,
    remote: Option<CoordinationTaskStatus>,
    id: &str,
    field: &str,
) -> Result<Option<CoordinationTaskStatus>> {
    match (local, remote) {
        (Some(local), Some(remote)) => merge_task_status(base, local, remote, id).map(Some),
        _ => merge_optional_copy(base, local, remote, "task", id, field),
    }
}

fn merge_claim_status(
    base: Option<ClaimStatus>,
    local: ClaimStatus,
    remote: ClaimStatus,
    id: &str,
) -> Result<ClaimStatus> {
    merge_precedence_status(
        "claim",
        id,
        "status",
        base,
        local,
        remote,
        claim_status_rank,
        |left, right| {
            matches!(
                (left, right),
                (ClaimStatus::Active, ClaimStatus::Contended)
                    | (ClaimStatus::Active, ClaimStatus::Released)
                    | (ClaimStatus::Active, ClaimStatus::Expired)
                    | (ClaimStatus::Contended, ClaimStatus::Released)
                    | (ClaimStatus::Contended, ClaimStatus::Expired)
                    | (ClaimStatus::Expired, ClaimStatus::Released)
            )
        },
    )
}

fn merge_artifact_status(
    base: Option<ArtifactStatus>,
    local: ArtifactStatus,
    remote: ArtifactStatus,
    id: &str,
) -> Result<ArtifactStatus> {
    merge_precedence_status(
        "artifact",
        id,
        "status",
        base,
        local,
        remote,
        artifact_status_rank,
        |left, right| {
            matches!(
                (left, right),
                (ArtifactStatus::Proposed, ArtifactStatus::InReview)
                    | (ArtifactStatus::Proposed, ArtifactStatus::Approved)
                    | (ArtifactStatus::Proposed, ArtifactStatus::Rejected)
                    | (ArtifactStatus::Proposed, ArtifactStatus::Superseded)
                    | (ArtifactStatus::Proposed, ArtifactStatus::Merged)
                    | (ArtifactStatus::InReview, ArtifactStatus::Approved)
                    | (ArtifactStatus::InReview, ArtifactStatus::Rejected)
                    | (ArtifactStatus::InReview, ArtifactStatus::Superseded)
                    | (ArtifactStatus::InReview, ArtifactStatus::Merged)
                    | (ArtifactStatus::Approved, ArtifactStatus::Merged)
                    | (ArtifactStatus::Approved, ArtifactStatus::Superseded)
            )
        },
    )
}

fn merge_git_execution_status(
    base: Option<GitExecutionStatus>,
    local: GitExecutionStatus,
    remote: GitExecutionStatus,
    id: &str,
) -> Result<GitExecutionStatus> {
    merge_precedence_status(
        "task",
        id,
        "git_execution.status",
        base,
        local,
        remote,
        git_execution_status_rank,
        |left, right| git_execution_status_rank(left) <= git_execution_status_rank(right),
    )
}

fn merge_git_integration_status(
    base: Option<GitIntegrationStatus>,
    local: GitIntegrationStatus,
    remote: GitIntegrationStatus,
    id: &str,
) -> Result<GitIntegrationStatus> {
    merge_precedence_status(
        "task",
        id,
        "git_execution.integration_status",
        base,
        local,
        remote,
        git_integration_status_rank,
        |left, right| git_integration_status_rank(left) <= git_integration_status_rank(right),
    )
}

fn merge_lifecycle_status<T, CanTransition, Rank>(
    kind: &str,
    id: &str,
    field: &str,
    base: Option<T>,
    local: T,
    remote: T,
    can_transition: CanTransition,
    rank: Rank,
) -> Result<T>
where
    T: Copy + PartialEq,
    CanTransition: Fn(T, T) -> bool,
    Rank: Fn(T) -> u8,
{
    if local == remote {
        return Ok(local);
    }
    if base.is_some_and(|base| local == base) {
        return Ok(remote);
    }
    if base.is_some_and(|base| remote == base) {
        return Ok(local);
    }
    match (can_transition(local, remote), can_transition(remote, local)) {
        (true, false) => Ok(remote),
        (false, true) => Ok(local),
        (true, true) => {
            if rank(local) >= rank(remote) {
                Ok(local)
            } else {
                Ok(remote)
            }
        }
        (false, false) => Err(semantic_conflict(
            kind,
            id,
            Some(field),
            "concurrent lifecycle transitions are semantically incompatible",
        )),
    }
}

fn merge_precedence_status<T, Rank, CanAdvance>(
    kind: &str,
    id: &str,
    field: &str,
    base: Option<T>,
    local: T,
    remote: T,
    rank: Rank,
    can_advance: CanAdvance,
) -> Result<T>
where
    T: Copy + PartialEq,
    Rank: Fn(T) -> u8,
    CanAdvance: Fn(T, T) -> bool,
{
    if local == remote {
        return Ok(local);
    }
    if base.is_some_and(|base| local == base) {
        return Ok(remote);
    }
    if base.is_some_and(|base| remote == base) {
        return Ok(local);
    }
    if can_advance(local, remote) && !can_advance(remote, local) {
        Ok(remote)
    } else if can_advance(remote, local) && !can_advance(local, remote) {
        Ok(local)
    } else if rank(local) == rank(remote) {
        Err(semantic_conflict(
            kind,
            id,
            Some(field),
            "concurrent terminal outcomes are semantically incompatible",
        ))
    } else if rank(local) > rank(remote) {
        Ok(local)
    } else {
        Ok(remote)
    }
}

fn merge_lease_holder(
    kind: &str,
    id: &str,
    base: Option<&LeaseHolder>,
    local: Option<&LeaseHolder>,
    remote: Option<&LeaseHolder>,
) -> Result<Option<LeaseHolder>> {
    match (base, local, remote) {
        (_, Some(local), Some(remote)) if local == remote => Ok(Some(local.clone())),
        (Some(base), Some(local), Some(remote)) if local == base => Ok(Some(remote.clone())),
        (Some(base), Some(local), Some(remote)) if remote == base => Ok(Some(local.clone())),
        (None, Some(local), None) => Ok(Some(local.clone())),
        (None, None, Some(remote)) => Ok(Some(remote.clone())),
        (Some(base), None, Some(remote)) if remote == base => Ok(None),
        (Some(base), Some(local), None) if local == base => Ok(None),
        (Some(_), None, None) | (None, None, None) => Ok(None),
        _ => Err(semantic_conflict(
            kind,
            id,
            Some("lease_holder"),
            "concurrent updates asserted incompatible holders for the same lease epoch",
        )),
    }
}

fn prefer_fresher_optional<T>(
    base: Option<&T>,
    local: Option<&T>,
    remote: Option<&T>,
    prefer_local: bool,
) -> Option<T>
where
    T: Clone + PartialEq,
{
    match (base, local, remote) {
        (_, Some(local), Some(remote)) if local == remote => Some(local.clone()),
        (Some(base), Some(local), Some(remote)) if local == base => Some(remote.clone()),
        (Some(base), Some(local), Some(remote)) if remote == base => Some(local.clone()),
        (_, Some(local), Some(remote)) => {
            if prefer_local {
                Some(local.clone())
            } else {
                Some(remote.clone())
            }
        }
        (_, Some(local), None) => Some(local.clone()),
        (_, None, Some(remote)) => Some(remote.clone()),
        _ => None,
    }
}

fn ensure_same_identity(
    kind: &str,
    id: &str,
    local: &str,
    remote: &str,
    field: &str,
) -> Result<()> {
    if local == remote {
        Ok(())
    } else {
        Err(semantic_conflict(
            kind,
            id,
            Some(field),
            "identity-defining fields changed incompatibly",
        ))
    }
}

fn semantic_conflict(kind: &str, id: &str, field: Option<&str>, detail: &str) -> anyhow::Error {
    match field {
        Some(field) => anyhow!(
            "shared coordination semantic merge rejected for {kind} `{id}` field `{field}`: {detail}"
        ),
        None => anyhow!("shared coordination semantic merge rejected for {kind} `{id}`: {detail}"),
    }
}

fn plan_status_rank(status: PlanStatus) -> u8 {
    match status {
        PlanStatus::Draft => 0,
        PlanStatus::Active => 1,
        PlanStatus::Blocked => 2,
        PlanStatus::Completed => 3,
        PlanStatus::Abandoned => 3,
        PlanStatus::Archived => 4,
    }
}

fn plan_node_status_rank(status: PlanNodeStatus) -> u8 {
    match status {
        PlanNodeStatus::Proposed => 0,
        PlanNodeStatus::Ready => 1,
        PlanNodeStatus::InProgress => 2,
        PlanNodeStatus::Blocked => 3,
        PlanNodeStatus::Waiting => 3,
        PlanNodeStatus::InReview => 4,
        PlanNodeStatus::Validating => 5,
        PlanNodeStatus::Completed => 6,
        PlanNodeStatus::Abandoned => 6,
    }
}

fn task_status_rank(status: CoordinationTaskStatus) -> u8 {
    match status {
        CoordinationTaskStatus::Proposed => 0,
        CoordinationTaskStatus::Ready => 1,
        CoordinationTaskStatus::InProgress => 2,
        CoordinationTaskStatus::Blocked => 3,
        CoordinationTaskStatus::InReview => 4,
        CoordinationTaskStatus::Validating => 5,
        CoordinationTaskStatus::Completed => 6,
        CoordinationTaskStatus::Abandoned => 6,
    }
}

fn claim_status_rank(status: ClaimStatus) -> u8 {
    match status {
        ClaimStatus::Active => 0,
        ClaimStatus::Contended => 1,
        ClaimStatus::Expired => 2,
        ClaimStatus::Released => 3,
    }
}

fn artifact_status_rank(status: ArtifactStatus) -> u8 {
    match status {
        ArtifactStatus::Proposed => 0,
        ArtifactStatus::InReview => 1,
        ArtifactStatus::Approved => 2,
        ArtifactStatus::Rejected => 3,
        ArtifactStatus::Superseded => 3,
        ArtifactStatus::Merged => 4,
    }
}

fn git_execution_status_rank(status: GitExecutionStatus) -> u8 {
    match status {
        GitExecutionStatus::NotStarted => 0,
        GitExecutionStatus::PreflightFailed => 1,
        GitExecutionStatus::InProgress => 2,
        GitExecutionStatus::PublishPending => 3,
        GitExecutionStatus::PublishFailed => 4,
        GitExecutionStatus::CoordinationPublished => 5,
    }
}

fn git_integration_status_rank(status: GitIntegrationStatus) -> u8 {
    match status {
        GitIntegrationStatus::NotStarted => 0,
        GitIntegrationStatus::PublishedToBranch => 1,
        GitIntegrationStatus::IntegrationPending => 2,
        GitIntegrationStatus::IntegrationInProgress => 3,
        GitIntegrationStatus::IntegratedToTarget => 4,
        GitIntegrationStatus::IntegrationFailed => 5,
    }
}

#[cfg(test)]
mod tests {
    use prism_ir::{
        AgentId, ClaimId, ClaimMode, CoordinationTaskId, GitIntegrationMode, PlanBinding, PlanId,
        PlanNodeKind, PrincipalActor, PrincipalAuthorityId, PrincipalId, SessionId, ValidationRef,
        WorkspaceRevision,
    };
    use serde_json::json;

    use super::*;
    use crate::git_execution::TaskGitExecution;
    use crate::types::{RuntimeDescriptorCapability, RuntimeDiscoveryMode};

    fn revision(version: u64) -> WorkspaceRevision {
        WorkspaceRevision {
            graph_version: version,
            git_commit: None,
        }
    }

    fn base_task() -> CoordinationTask {
        CoordinationTask {
            id: CoordinationTaskId::new("coord-task:semantic"),
            plan: PlanId::new("plan:semantic"),
            kind: PlanNodeKind::Edit,
            title: "Semantic merge".into(),
            summary: Some("base".into()),
            status: CoordinationTaskStatus::InProgress,
            published_task_status: None,
            assignee: Some(AgentId::new("agent:base")),
            pending_handoff_to: None,
            session: Some(SessionId::new("session:base")),
            lease_holder: Some(LeaseHolder {
                principal: Some(PrincipalActor {
                    authority_id: PrincipalAuthorityId::new("authority:test"),
                    principal_id: PrincipalId::new("principal:base"),
                    kind: None,
                    name: None,
                }),
                session_id: Some(SessionId::new("session:base")),
                worktree_id: Some("worktree:base".into()),
                agent_id: Some(AgentId::new("agent:base")),
            }),
            lease_started_at: Some(10),
            lease_refreshed_at: Some(10),
            lease_stale_at: Some(40),
            lease_expires_at: Some(70),
            worktree_id: Some("worktree:base".into()),
            branch_ref: Some("refs/heads/task/base".into()),
            anchors: Vec::new(),
            bindings: PlanBinding::default(),
            depends_on: vec![CoordinationTaskId::new("coord-task:dep-a")],
            coordination_depends_on: Vec::new(),
            integrated_depends_on: Vec::new(),
            acceptance: vec![AcceptanceCriterion {
                label: "done".into(),
                anchors: Vec::new(),
            }],
            validation_refs: vec![ValidationRef {
                id: "test:base".into(),
            }],
            is_abstract: false,
            base_revision: revision(1),
            priority: Some(1),
            tags: vec!["base".into()],
            metadata: json!({"base": true}),
            git_execution: TaskGitExecution {
                status: GitExecutionStatus::InProgress,
                integration_mode: GitIntegrationMode::External,
                integration_status: GitIntegrationStatus::NotStarted,
                ..TaskGitExecution::default()
            },
        }
    }

    #[test]
    fn reconcile_task_records_merges_lifecycle_and_set_fields() {
        let base = base_task();
        let mut local = base.clone();
        local.status = CoordinationTaskStatus::Completed;
        local.tags.push("local".into());
        local.metadata = json!({"local": true});
        local.git_execution.status = GitExecutionStatus::CoordinationPublished;
        local.git_execution.integration_status = GitIntegrationStatus::PublishedToBranch;

        let mut remote = base.clone();
        remote.status = CoordinationTaskStatus::Validating;
        remote.validation_refs.push(ValidationRef {
            id: "test:remote".into(),
        });
        remote
            .depends_on
            .push(CoordinationTaskId::new("coord-task:dep-b"));
        remote.metadata = json!({"remote": true});

        let merged = reconcile_task_records(&[base], &[local], &[remote]).unwrap();
        let task = merged.into_iter().next().unwrap();
        assert_eq!(task.status, CoordinationTaskStatus::Completed);
        assert!(task.tags.iter().any(|tag| tag == "local"));
        assert!(task
            .validation_refs
            .iter()
            .any(|validation| validation.id == "test:remote"));
        assert!(task
            .depends_on
            .iter()
            .any(|dependency| dependency.0 == "coord-task:dep-b"));
        assert_eq!(
            task.git_execution.status,
            GitExecutionStatus::CoordinationPublished
        );
        assert_eq!(
            task.git_execution.integration_status,
            GitIntegrationStatus::PublishedToBranch
        );
        assert_eq!(task.metadata, json!({"local": true, "remote": true}));
    }

    #[test]
    fn reconcile_task_records_rejects_conflicting_terminal_states() {
        let base = base_task();
        let mut local = base.clone();
        local.status = CoordinationTaskStatus::Completed;
        let mut remote = base.clone();
        remote.status = CoordinationTaskStatus::Abandoned;

        let error = reconcile_task_records(&[base], &[local], &[remote]).unwrap_err();
        assert!(error
            .to_string()
            .contains("concurrent lifecycle transitions are semantically incompatible"));
    }

    #[test]
    fn reconcile_claim_records_rejects_incompatible_lease_holders() {
        let base = WorkClaim {
            id: ClaimId::new("claim:semantic"),
            holder: SessionId::new("session:base"),
            agent: None,
            lease_holder: None,
            worktree_id: Some("worktree:base".into()),
            branch_ref: Some("refs/heads/task/base".into()),
            task: Some(CoordinationTaskId::new("coord-task:semantic")),
            anchors: Vec::new(),
            capability: prism_ir::Capability::Edit,
            mode: ClaimMode::SoftExclusive,
            since: 10,
            refreshed_at: Some(10),
            stale_at: Some(20),
            expires_at: 30,
            status: ClaimStatus::Active,
            base_revision: revision(1),
        };
        let mut local = base.clone();
        local.lease_holder = Some(LeaseHolder {
            principal: None,
            session_id: Some(SessionId::new("session:local")),
            worktree_id: Some("worktree:local".into()),
            agent_id: None,
        });
        let mut remote = base.clone();
        remote.lease_holder = Some(LeaseHolder {
            principal: None,
            session_id: Some(SessionId::new("session:remote")),
            worktree_id: Some("worktree:remote".into()),
            agent_id: None,
        });

        let error = reconcile_claim_records(&[base], &[local], &[remote]).unwrap_err();
        assert!(error
            .to_string()
            .contains("incompatible holders for the same lease epoch"));
    }

    #[test]
    fn reconcile_runtime_descriptor_records_prefers_fresher_observation_fields() {
        let base = RuntimeDescriptor {
            runtime_id: "runtime:test".into(),
            repo_id: "repo:test".into(),
            worktree_id: "worktree:test".into(),
            principal_id: "principal:test".into(),
            instance_started_at: 10,
            last_seen_at: 20,
            branch_ref: Some("refs/heads/main".into()),
            checked_out_commit: Some("aaaa".into()),
            capabilities: vec![RuntimeDescriptorCapability::CoordinationRefPublisher],
            discovery_mode: RuntimeDiscoveryMode::LanDirect,
            peer_endpoint: Some("http://127.0.0.1:1".into()),
            public_endpoint: None,
            peer_transport_identity: None,
            blob_snapshot_head: None,
            export_policy: None,
        };
        let mut local = base.clone();
        local.last_seen_at = 25;
        local.peer_endpoint = Some("http://127.0.0.1:2".into());
        let mut remote = base.clone();
        remote.last_seen_at = 30;
        remote.branch_ref = Some("refs/heads/task/runtime".into());
        remote.checked_out_commit = Some("bbbb".into());

        let merged = reconcile_runtime_descriptor_records(&[base], &[local], &[remote]).unwrap();
        let descriptor = merged.into_iter().next().unwrap();
        assert_eq!(descriptor.last_seen_at, 30);
        assert_eq!(
            descriptor.branch_ref.as_deref(),
            Some("refs/heads/task/runtime")
        );
        assert_eq!(descriptor.checked_out_commit.as_deref(), Some("bbbb"));
    }
}
