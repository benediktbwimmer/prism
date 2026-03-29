use std::collections::HashMap;

use prism_ir::{CoordinationEventKind, CoordinationTaskId, CoordinationTaskStatus, PlanId};
use serde::de::DeserializeOwned;
use serde_json::Value;

use crate::helpers::sorted_values;
use crate::types::{CoordinationSnapshot, CoordinationTask, Plan};

pub(crate) fn rehydrate_plan_task_state(
    mut snapshot: CoordinationSnapshot,
) -> CoordinationSnapshot {
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

    let mut plans = HashMap::<PlanId, Plan>::new();
    let mut tasks = HashMap::<CoordinationTaskId, CoordinationTask>::new();

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
            _ => {}
        }
    }

    for (plan_id, plan) in stored_plans {
        plans.entry(plan_id).or_insert(plan);
    }
    for (task_id, task) in stored_tasks {
        tasks.entry(task_id).or_insert(task);
    }

    recompute_root_tasks(&mut plans, &tasks);

    snapshot.plans = sorted_values(&plans, |plan| plan.id.0.to_string());
    snapshot.tasks = sorted_values(&tasks, |task| task.id.0.to_string());
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
    if patch_is_set(metadata, "title") {
        if let Some(title) = metadata_path::<String>(metadata, &["patchValues", "title"]) {
            task.title = title;
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
    if patch_is_set(metadata, "anchors") {
        if let Some(anchors) = metadata_path(metadata, &["patchValues", "anchors"]) {
            task.anchors = anchors;
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
    if patch_is_set(metadata, "baseRevision") {
        if let Some(base_revision) = metadata_path(metadata, &["patchValues", "baseRevision"]) {
            task.base_revision = base_revision;
        }
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
