use std::collections::HashMap;

use prism_ir::{CoordinationEventKind, CoordinationTaskId, CoordinationTaskStatus, PlanId};
use serde::de::DeserializeOwned;
use serde_json::Value;

use crate::helpers::sorted_values;
use crate::types::{
    Artifact, ArtifactReview, CoordinationEvent, CoordinationSnapshot, CoordinationTask, Plan,
    WorkClaim,
};

pub fn coordination_snapshot_from_events(
    events: &[CoordinationEvent],
    fallback: Option<CoordinationSnapshot>,
) -> Option<CoordinationSnapshot> {
    if events.is_empty() {
        return fallback.map(rehydrate_coordination_snapshot);
    }
    let mut snapshot = fallback.unwrap_or_default();
    snapshot.events = events.to_vec();
    Some(rehydrate_coordination_snapshot(snapshot))
}

pub fn rehydrate_coordination_snapshot(mut snapshot: CoordinationSnapshot) -> CoordinationSnapshot {
    let stored_plans = snapshot
        .plans
        .iter()
        .cloned()
        .map(|plan| (plan.id.clone(), plan))
        .collect::<HashMap<_, _>>();
    let stored_tasks = snapshot
        .tasks
        .iter()
        .cloned()
        .map(|task| (task.id.clone(), task))
        .collect::<HashMap<_, _>>();
    let stored_claims = snapshot
        .claims
        .iter()
        .cloned()
        .map(|claim| (claim.id.clone(), claim))
        .collect::<HashMap<_, _>>();
    let stored_artifacts = snapshot
        .artifacts
        .iter()
        .cloned()
        .map(|artifact| (artifact.id.clone(), artifact))
        .collect::<HashMap<_, _>>();
    let stored_reviews = snapshot
        .reviews
        .iter()
        .cloned()
        .map(|review| (review.id.clone(), review))
        .collect::<HashMap<_, _>>();

    let mut plans = HashMap::<PlanId, Plan>::new();
    let mut tasks = HashMap::<CoordinationTaskId, CoordinationTask>::new();
    let mut claims = HashMap::new();
    let mut artifacts = HashMap::new();
    let mut reviews = HashMap::new();

    for event in &snapshot.events {
        match event.kind {
            CoordinationEventKind::PlanCreated => {
                if let Some(plan) = metadata_field::<Plan>(&event.metadata, "plan") {
                    plans.insert(plan.id.clone(), plan);
                }
            }
            CoordinationEventKind::PlanUpdated => {
                if let Some(plan_id) = event.plan.as_ref() {
                    let mut plan = plans
                        .get(plan_id)
                        .cloned()
                        .or_else(|| stored_plans.get(plan_id).cloned());
                    if let Some(plan) = plan.as_mut() {
                        apply_plan_patch(plan, &event.metadata);
                        plans.insert(plan.id.clone(), plan.clone());
                    }
                }
            }
            CoordinationEventKind::TaskCreated => {
                if let Some(task) = metadata_field::<CoordinationTask>(&event.metadata, "task") {
                    tasks.insert(task.id.clone(), task);
                }
            }
            CoordinationEventKind::TaskAssigned
            | CoordinationEventKind::TaskBlocked
            | CoordinationEventKind::TaskUnblocked
            | CoordinationEventKind::TaskStatusChanged
            | CoordinationEventKind::TaskResumed
            | CoordinationEventKind::TaskReclaimed
            | CoordinationEventKind::HandoffRequested
            | CoordinationEventKind::HandoffAccepted => {
                if let Some(task_id) = event.task.as_ref() {
                    let mut task = tasks
                        .get(task_id)
                        .cloned()
                        .or_else(|| stored_tasks.get(task_id).cloned());
                    if let Some(task) = task.as_mut() {
                        apply_task_patch(task, &event.metadata);
                        tasks.insert(task.id.clone(), task.clone());
                    }
                }
            }
            CoordinationEventKind::ClaimAcquired
            | CoordinationEventKind::ClaimRenewed
            | CoordinationEventKind::ClaimReleased => {
                if let Some(claim) = metadata_field::<WorkClaim>(&event.metadata, "claim") {
                    claims.insert(claim.id.clone(), claim);
                }
            }
            CoordinationEventKind::ArtifactProposed | CoordinationEventKind::ArtifactSuperseded => {
                if let Some(artifact) = metadata_field::<Artifact>(&event.metadata, "artifact") {
                    artifacts.insert(artifact.id.clone(), artifact);
                }
            }
            CoordinationEventKind::ArtifactReviewed => {
                if let Some(artifact) = metadata_field::<Artifact>(&event.metadata, "artifact") {
                    artifacts.insert(artifact.id.clone(), artifact);
                }
                if let Some(review) = metadata_field::<ArtifactReview>(&event.metadata, "review") {
                    reviews.insert(review.id.clone(), review);
                }
            }
            _ => {}
        }
    }

    for (plan_id, stored) in stored_plans {
        match plans.get_mut(&plan_id) {
            Some(plan) => merge_stored_plan_metadata(plan, stored),
            None => {
                plans.insert(plan_id, stored);
            }
        }
    }
    for (task_id, stored) in stored_tasks {
        match tasks.get_mut(&task_id) {
            Some(task) => merge_stored_task_metadata(task, stored),
            None => {
                tasks.insert(task_id, stored);
            }
        }
    }
    for (claim_id, stored) in stored_claims {
        claims.entry(claim_id).or_insert(stored);
    }
    for (artifact_id, stored) in stored_artifacts {
        artifacts.entry(artifact_id).or_insert(stored);
    }
    for (review_id, stored) in stored_reviews {
        reviews.entry(review_id).or_insert(stored);
    }

    recompute_root_tasks(&mut plans, &tasks);

    snapshot.plans = sorted_values(&plans, |plan| plan.id.0.to_string());
    snapshot.tasks = sorted_values(&tasks, |task| task.id.0.to_string());
    snapshot.claims = sorted_values(&claims, |claim| claim.id.0.to_string());
    snapshot.artifacts = sorted_values(&artifacts, |artifact| artifact.id.0.to_string());
    snapshot.reviews = sorted_values(&reviews, |review| review.id.0.to_string());
    snapshot.next_plan = snapshot.next_plan.max(next_counter(
        snapshot.plans.iter().map(|plan| plan.id.0.as_str()),
        "plan:",
    ));
    snapshot.next_task = snapshot.next_task.max(next_counter(
        snapshot.tasks.iter().map(|task| task.id.0.as_str()),
        "coord-task:",
    ));
    snapshot.next_claim = snapshot.next_claim.max(next_counter(
        snapshot.claims.iter().map(|claim| claim.id.0.as_str()),
        "claim:",
    ));
    snapshot.next_artifact = snapshot.next_artifact.max(next_counter(
        snapshot
            .artifacts
            .iter()
            .map(|artifact| artifact.id.0.as_str()),
        "artifact:",
    ));
    snapshot.next_review = snapshot.next_review.max(next_counter(
        snapshot.reviews.iter().map(|review| review.id.0.as_str()),
        "review:",
    ));
    snapshot
}

fn apply_plan_patch(plan: &mut Plan, metadata: &Value) {
    if !patch_is_set(metadata, "goal")
        && !patch_is_set(metadata, "status")
        && !patch_is_set(metadata, "policy")
    {
        return;
    }
    if patch_is_set(metadata, "goal") {
        if let Some(goal) = metadata_path::<String>(metadata, &["patchValues", "goal"]) {
            if plan.title.is_empty() || plan.title == plan.goal {
                plan.title = goal.clone();
            }
            plan.goal = goal;
        }
    }
    if patch_is_set(metadata, "status") {
        if let Some(status) = metadata_path(metadata, &["patchValues", "status"]) {
            plan.status = status;
        }
    }
    if patch_is_set(metadata, "policy") {
        if let Some(policy) = metadata_path(metadata, &["patchValues", "policy"]) {
            plan.policy = policy;
        }
    }
}

fn apply_task_patch(task: &mut CoordinationTask, metadata: &Value) {
    if patch_is_set(metadata, "kind") {
        if let Some(kind) =
            metadata_path::<prism_ir::PlanNodeKind>(metadata, &["patchValues", "kind"])
        {
            task.kind = kind;
        }
    }
    if patch_is_set(metadata, "title") {
        if let Some(title) = metadata_path::<String>(metadata, &["patchValues", "title"]) {
            task.title = title;
        }
    }
    if patch_is_set(metadata, "summary") || patch_is_clear(metadata, "summary") {
        if let Some(summary) = metadata_optional_path(metadata, &["patchValues", "summary"]) {
            task.summary = summary;
        }
    }
    if patch_is_set(metadata, "status") {
        if let Some(status) =
            metadata_path::<CoordinationTaskStatus>(metadata, &["patchValues", "status"])
        {
            task.status = status;
        }
    }
    if patch_is_set(metadata, "assignee") || patch_is_clear(metadata, "assignee") {
        if let Some(assignee) = metadata_optional_path(metadata, &["patchValues", "assignee"]) {
            task.assignee = assignee;
        }
    }
    if patch_is_set(metadata, "pendingHandoffTo") || patch_is_clear(metadata, "pendingHandoffTo") {
        if let Some(agent) = metadata_optional_path(metadata, &["patchValues", "pendingHandoffTo"])
        {
            task.pending_handoff_to = agent;
        }
    }
    if patch_is_set(metadata, "session") || patch_is_clear(metadata, "session") {
        if let Some(session) = metadata_optional_path(metadata, &["patchValues", "session"]) {
            task.session = session;
        }
    }
    if patch_is_set(metadata, "leaseHolder") || patch_is_clear(metadata, "leaseHolder") {
        if let Some(lease_holder) =
            metadata_optional_path(metadata, &["patchValues", "leaseHolder"])
        {
            task.lease_holder = lease_holder;
        }
    }
    if patch_is_set(metadata, "leaseStartedAt") || patch_is_clear(metadata, "leaseStartedAt") {
        if let Some(lease_started_at) =
            metadata_optional_path(metadata, &["patchValues", "leaseStartedAt"])
        {
            task.lease_started_at = lease_started_at;
        }
    }
    if patch_is_set(metadata, "leaseRefreshedAt") || patch_is_clear(metadata, "leaseRefreshedAt") {
        if let Some(lease_refreshed_at) =
            metadata_optional_path(metadata, &["patchValues", "leaseRefreshedAt"])
        {
            task.lease_refreshed_at = lease_refreshed_at;
        }
    }
    if patch_is_set(metadata, "leaseStaleAt") || patch_is_clear(metadata, "leaseStaleAt") {
        if let Some(lease_stale_at) =
            metadata_optional_path(metadata, &["patchValues", "leaseStaleAt"])
        {
            task.lease_stale_at = lease_stale_at;
        }
    }
    if patch_is_set(metadata, "leaseExpiresAt") || patch_is_clear(metadata, "leaseExpiresAt") {
        if let Some(lease_expires_at) =
            metadata_optional_path(metadata, &["patchValues", "leaseExpiresAt"])
        {
            task.lease_expires_at = lease_expires_at;
        }
    }
    if patch_is_set(metadata, "worktreeId") || patch_is_clear(metadata, "worktreeId") {
        if let Some(worktree_id) = metadata_optional_path(metadata, &["patchValues", "worktreeId"])
        {
            task.worktree_id = worktree_id;
        }
    }
    if patch_is_set(metadata, "branchRef") || patch_is_clear(metadata, "branchRef") {
        if let Some(branch_ref) = metadata_optional_path(metadata, &["patchValues", "branchRef"]) {
            task.branch_ref = branch_ref;
        }
    }
    if patch_is_set(metadata, "anchors") {
        if let Some(anchors) = metadata_path(metadata, &["patchValues", "anchors"]) {
            task.anchors = anchors;
            task.bindings.anchors = task.anchors.clone();
        }
    }
    if patch_is_set(metadata, "bindings") {
        if let Some(bindings) = metadata_path(metadata, &["patchValues", "bindings"]) {
            task.bindings = bindings;
        }
    }
    if patch_is_set(metadata, "dependsOn") {
        if let Some(depends_on) = metadata_path(metadata, &["patchValues", "dependsOn"]) {
            task.depends_on = depends_on;
        }
    }
    if patch_is_set(metadata, "acceptance") {
        if let Some(acceptance) = metadata_path(metadata, &["patchValues", "acceptance"]) {
            task.acceptance = acceptance;
        }
    }
    if patch_is_set(metadata, "validationRefs") {
        if let Some(validation_refs) = metadata_path(metadata, &["patchValues", "validationRefs"]) {
            task.validation_refs = validation_refs;
        }
    }
    if patch_is_set(metadata, "isAbstract") {
        if let Some(is_abstract) = metadata_path(metadata, &["patchValues", "isAbstract"]) {
            task.is_abstract = is_abstract;
        }
    }
    if patch_is_set(metadata, "baseRevision") {
        if let Some(base_revision) = metadata_path(metadata, &["patchValues", "baseRevision"]) {
            task.base_revision = base_revision;
        }
    }
    if patch_is_set(metadata, "priority") || patch_is_clear(metadata, "priority") {
        if let Some(priority) = metadata_optional_path(metadata, &["patchValues", "priority"]) {
            task.priority = priority;
        }
    }
    if patch_is_set(metadata, "tags") {
        if let Some(tags) = metadata_path(metadata, &["patchValues", "tags"]) {
            task.tags = tags;
        }
    }
    if task.bindings.anchors.is_empty() && !task.anchors.is_empty() {
        task.bindings.anchors = task.anchors.clone();
    }
}

fn merge_stored_plan_metadata(plan: &mut Plan, stored: Plan) {
    if !stored.title.is_empty() && stored.title != stored.goal {
        plan.title = stored.title;
    }
    plan.scope = stored.scope;
    plan.kind = stored.kind;
    plan.revision = stored.revision;
    plan.tags = stored.tags;
    plan.created_from = stored.created_from;
    plan.metadata = stored.metadata;
}

fn merge_stored_task_metadata(task: &mut CoordinationTask, stored: CoordinationTask) {
    if stored.kind != prism_ir::PlanNodeKind::Edit {
        task.kind = stored.kind;
    }
    if stored.summary.is_some() {
        task.summary = stored.summary;
    }
    if !stored.bindings.concept_handles.is_empty()
        || !stored.bindings.artifact_refs.is_empty()
        || !stored.bindings.memory_refs.is_empty()
        || !stored.bindings.outcome_refs.is_empty()
    {
        task.bindings = stored.bindings;
        task.bindings.anchors = task.anchors.clone();
    } else if task.bindings.anchors.is_empty() && !task.anchors.is_empty() {
        task.bindings.anchors = task.anchors.clone();
    }
    if !stored.validation_refs.is_empty() {
        task.validation_refs = stored.validation_refs;
    }
    if stored.is_abstract {
        task.is_abstract = true;
    }
    if stored.priority.is_some() {
        task.priority = stored.priority;
    }
    if !stored.tags.is_empty() {
        task.tags = stored.tags;
    }
    if !stored.metadata.is_null() {
        task.metadata = stored.metadata;
    }
    if stored.lease_holder.is_some() {
        task.lease_holder = stored.lease_holder;
    }
    if stored.lease_started_at.is_some() {
        task.lease_started_at = stored.lease_started_at;
    }
    if stored.lease_refreshed_at.is_some() {
        task.lease_refreshed_at = stored.lease_refreshed_at;
    }
    if stored.lease_stale_at.is_some() {
        task.lease_stale_at = stored.lease_stale_at;
    }
    if stored.lease_expires_at.is_some() {
        task.lease_expires_at = stored.lease_expires_at;
    }
}

fn recompute_root_tasks(
    plans: &mut HashMap<PlanId, Plan>,
    tasks: &HashMap<CoordinationTaskId, CoordinationTask>,
) {
    for plan in plans.values_mut() {
        plan.root_tasks.clear();
    }
    let mut roots = tasks
        .values()
        .filter(|task| task.depends_on.is_empty())
        .map(|task| (task.plan.clone(), task.id.clone()))
        .collect::<Vec<_>>();
    roots.sort_by(|left, right| {
        left.0
             .0
            .cmp(&right.0 .0)
            .then_with(|| left.1 .0.cmp(&right.1 .0))
    });
    for (plan_id, task_id) in roots {
        if let Some(plan) = plans.get_mut(&plan_id) {
            plan.root_tasks.push(task_id);
        }
    }
}

fn patch_is_set(metadata: &Value, field: &str) -> bool {
    metadata
        .get("patch")
        .and_then(Value::as_object)
        .and_then(|patch| patch.get(field))
        .and_then(Value::as_str)
        == Some("set")
}

fn patch_is_clear(metadata: &Value, field: &str) -> bool {
    metadata
        .get("patch")
        .and_then(Value::as_object)
        .and_then(|patch| patch.get(field))
        .and_then(Value::as_str)
        == Some("clear")
}

fn metadata_field<T: DeserializeOwned>(metadata: &Value, key: &str) -> Option<T> {
    metadata
        .as_object()
        .and_then(|object| object.get(key))
        .cloned()
        .and_then(|value| serde_json::from_value(value).ok())
}

fn metadata_path<T: DeserializeOwned>(metadata: &Value, path: &[&str]) -> Option<T> {
    let mut value = metadata;
    for segment in path {
        value = value.get(*segment)?;
    }
    serde_json::from_value(value.clone()).ok()
}

fn metadata_optional_path<T: DeserializeOwned>(
    metadata: &Value,
    path: &[&str],
) -> Option<Option<T>> {
    let mut value = metadata;
    for segment in path {
        value = value.get(*segment)?;
    }
    serde_json::from_value(value.clone()).ok()
}

fn next_counter<'a, I>(ids: I, prefix: &str) -> u64
where
    I: Iterator<Item = &'a str>,
{
    ids.filter_map(|id| id.strip_prefix(prefix))
        .filter_map(|suffix| suffix.parse::<u64>().ok())
        .max()
        .unwrap_or(0)
}
