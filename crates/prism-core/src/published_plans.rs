use std::collections::{BTreeMap, BTreeSet};
use std::fs::{self, File, OpenOptions};
use std::io::{BufRead, BufReader, Write};
use std::path::{Path, PathBuf};

use anyhow::{anyhow, Context, Result};
use prism_coordination::{CoordinationSnapshot, CoordinationTask, Plan};
use prism_ir::{CoordinationTaskId, PlanId, PlanStatus};
use serde::{Deserialize, Serialize};

use crate::util::{
    repo_active_plans_dir, repo_archived_plans_dir, repo_plan_index_path, repo_plans_dir,
};

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
enum PublishedPlanEventKind {
    PlanUpdated,
    NodeUpdated,
    ExecutionUpdated,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
struct PublishedPlanEvent {
    event_id: String,
    kind: PublishedPlanEventKind,
    plan_id: PlanId,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    node_id: Option<CoordinationTaskId>,
    payload: PublishedPlanPayload,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
enum PublishedPlanPayload {
    Plan {
        plan: Plan,
    },
    Node {
        task: PublishedPlanNode,
    },
    Execution {
        execution: PublishedPlanExecutionOverlay,
    },
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
struct PublishedPlanNode {
    id: CoordinationTaskId,
    plan: PlanId,
    title: String,
    status: prism_ir::CoordinationTaskStatus,
    assignee: Option<prism_ir::AgentId>,
    anchors: Vec<prism_ir::AnchorRef>,
    depends_on: Vec<CoordinationTaskId>,
    acceptance: Vec<prism_coordination::AcceptanceCriterion>,
    base_revision: prism_ir::WorkspaceRevision,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
struct PublishedPlanExecutionOverlay {
    pending_handoff_to: Option<prism_ir::AgentId>,
}

impl From<CoordinationTask> for PublishedPlanNode {
    fn from(task: CoordinationTask) -> Self {
        Self {
            id: task.id,
            plan: task.plan,
            title: task.title,
            status: task.status,
            assignee: task.assignee,
            anchors: task.anchors,
            depends_on: task.depends_on,
            acceptance: task.acceptance,
            base_revision: task.base_revision,
        }
    }
}

impl From<PublishedPlanNode> for CoordinationTask {
    fn from(task: PublishedPlanNode) -> Self {
        Self {
            id: task.id,
            plan: task.plan,
            title: task.title,
            status: task.status,
            assignee: task.assignee,
            pending_handoff_to: None,
            session: None,
            anchors: task.anchors,
            depends_on: task.depends_on,
            acceptance: task.acceptance,
            base_revision: task.base_revision,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub(crate) struct PublishedPlanIndexEntry {
    pub(crate) plan_id: PlanId,
    pub(crate) title: String,
    pub(crate) status: PlanStatus,
    pub(crate) scope: String,
    pub(crate) kind: String,
    pub(crate) log_path: String,
}

#[derive(Debug, Default)]
struct PublishedPlanProjection {
    plans: Vec<Plan>,
    tasks: Vec<CoordinationTask>,
    next_plan: u64,
    next_task: u64,
    next_log_sequence: BTreeMap<String, u64>,
}

impl PublishedPlanProjection {
    fn plan(&self, plan_id: &PlanId) -> Option<Plan> {
        self.plans.iter().find(|plan| &plan.id == plan_id).cloned()
    }

    fn tasks_for_plan(&self, plan_id: &PlanId) -> Vec<CoordinationTask> {
        self.tasks
            .iter()
            .filter(|task| &task.plan == plan_id)
            .cloned()
            .collect()
    }

    fn next_log_sequence_for(&self, plan_id: &PlanId) -> u64 {
        self.next_log_sequence
            .get(plan_id.0.as_str())
            .copied()
            .unwrap_or(0)
    }
}

pub(crate) fn sync_repo_published_plans(
    root: &Path,
    snapshot: &CoordinationSnapshot,
) -> Result<()> {
    let plans_dir = repo_plans_dir(root);
    fs::create_dir_all(repo_active_plans_dir(root))?;
    fs::create_dir_all(repo_archived_plans_dir(root))?;

    let existing_entries = load_jsonl_file::<PublishedPlanIndexEntry>(&repo_plan_index_path(root))?;
    let existing_paths = existing_entries
        .into_iter()
        .map(|entry| (entry.plan_id.0.to_string(), entry.log_path))
        .collect::<BTreeMap<_, _>>();
    let existing_projection = load_repo_published_plan_projection(root)?.unwrap_or_default();

    let tasks_by_plan = snapshot.tasks.iter().cloned().fold(
        BTreeMap::<String, Vec<CoordinationTask>>::new(),
        |mut map, task| {
            map.entry(task.plan.0.to_string()).or_default().push(task);
            map
        },
    );

    let mut index_entries = Vec::new();
    let mut expected_logs = BTreeSet::new();
    for plan in snapshot.plans.iter().cloned() {
        let plan_key = plan.id.0.to_string();
        let mut tasks = tasks_by_plan.get(&plan_key).cloned().unwrap_or_default();
        tasks.sort_by(|left, right| left.id.0.cmp(&right.id.0));

        let relative_log_path = relative_plan_log_path(&plan);
        let full_log_path = root.join(&relative_log_path);
        if let Some(previous_path) = existing_paths.get(&plan_key) {
            let previous_path = resolve_log_path(root, previous_path);
            if previous_path != full_log_path && previous_path.exists() {
                if let Some(parent) = full_log_path.parent() {
                    fs::create_dir_all(parent)?;
                }
                fs::rename(previous_path, &full_log_path)?;
            }
        }

        let previous_plan = existing_projection.plan(&plan.id);
        let previous_tasks = existing_projection.tasks_for_plan(&plan.id);
        let starting_sequence = existing_projection.next_log_sequence_for(&plan.id);
        let events =
            if previous_plan.is_none() && previous_tasks.is_empty() && !full_log_path.exists() {
                published_plan_events(starting_sequence, &plan, &tasks)
            } else {
                append_plan_delta_events(
                    starting_sequence,
                    previous_plan.as_ref(),
                    &previous_tasks,
                    &plan,
                    &tasks,
                )
            };
        if !events.is_empty() {
            append_jsonl_file(&full_log_path, &events)?;
        }
        expected_logs.insert(normalize_path(&full_log_path));

        index_entries.push(PublishedPlanIndexEntry {
            plan_id: plan.id.clone(),
            title: plan.goal.clone(),
            status: plan.status,
            scope: "Repo".to_string(),
            kind: "TaskExecution".to_string(),
            log_path: normalize_relative_path(&relative_log_path),
        });
    }

    index_entries.sort_by(|left, right| left.plan_id.0.cmp(&right.plan_id.0));
    write_jsonl_file(&repo_plan_index_path(root), &index_entries)?;
    cleanup_stale_plan_logs(&plans_dir, &expected_logs)?;
    Ok(())
}

pub(crate) fn load_hydrated_coordination_snapshot(
    root: &Path,
    snapshot: Option<CoordinationSnapshot>,
) -> Result<Option<CoordinationSnapshot>> {
    let Some(published) = load_repo_published_plan_projection(root)? else {
        return Ok(snapshot);
    };

    let mut snapshot = snapshot.unwrap_or_default();
    snapshot.plans = published.plans;
    snapshot.tasks = published.tasks;
    snapshot.next_plan = snapshot.next_plan.max(published.next_plan);
    snapshot.next_task = snapshot.next_task.max(published.next_task);
    Ok(Some(snapshot))
}

fn load_repo_published_plan_projection(root: &Path) -> Result<Option<PublishedPlanProjection>> {
    let index_path = repo_plan_index_path(root);
    if !index_path.exists() {
        return Ok(None);
    }

    let entries = load_jsonl_file::<PublishedPlanIndexEntry>(&index_path)?;
    if entries.is_empty() {
        return Ok(None);
    }

    let mut projection = PublishedPlanProjection::default();
    for entry in entries {
        let log_path = resolve_log_path(root, &entry.log_path);
        let events = load_jsonl_file::<PublishedPlanEvent>(&log_path)?;
        let (plan, mut tasks) = project_plan_log(&log_path, &events)?;
        projection.next_plan = projection
            .next_plan
            .max(next_numeric_suffix(&plan.id.0, "plan:"));
        projection.next_task = tasks.iter().fold(projection.next_task, |current, task| {
            current.max(next_numeric_suffix(&task.id.0, "coord-task:"))
        });
        projection.next_log_sequence.insert(
            plan.id.0.to_string(),
            next_log_sequence_from_events(&events),
        );
        projection.plans.push(plan);
        projection.tasks.append(&mut tasks);
    }
    projection
        .plans
        .sort_by(|left, right| left.id.0.cmp(&right.id.0));
    projection
        .tasks
        .sort_by(|left, right| left.id.0.cmp(&right.id.0));
    Ok(Some(projection))
}

fn project_plan_log(
    path: &Path,
    events: &[PublishedPlanEvent],
) -> Result<(Plan, Vec<CoordinationTask>)> {
    let mut plan = None;
    let mut tasks = BTreeMap::<String, CoordinationTask>::new();
    for event in events {
        match &event.payload {
            PublishedPlanPayload::Plan { plan: value } => {
                plan = Some(value.clone());
            }
            PublishedPlanPayload::Node { task } => {
                tasks.insert(task.id.0.to_string(), task.clone().into());
            }
            PublishedPlanPayload::Execution { execution } => {
                let Some(node_id) = &event.node_id else {
                    continue;
                };
                if let Some(task) = tasks.get_mut(node_id.0.as_str()) {
                    task.pending_handoff_to = execution.pending_handoff_to.clone();
                }
            }
        }
    }

    let plan = plan.ok_or_else(|| {
        anyhow!(
            "published plan log {} did not contain a plan record",
            path.display()
        )
    })?;
    let tasks = tasks.into_values().collect::<Vec<_>>();
    Ok((plan, tasks))
}

fn published_plan_events(
    starting_sequence: u64,
    plan: &Plan,
    tasks: &[CoordinationTask],
) -> Vec<PublishedPlanEvent> {
    let mut sequence = starting_sequence;
    let mut events = Vec::with_capacity(tasks.len() + 2);
    events.push(PublishedPlanEvent {
        event_id: next_published_event_id(&plan.id, &mut sequence),
        kind: PublishedPlanEventKind::PlanUpdated,
        plan_id: plan.id.clone(),
        node_id: None,
        payload: PublishedPlanPayload::Plan { plan: plan.clone() },
    });
    for task in tasks {
        events.push(PublishedPlanEvent {
            event_id: next_published_event_id(&plan.id, &mut sequence),
            kind: PublishedPlanEventKind::NodeUpdated,
            plan_id: plan.id.clone(),
            node_id: Some(task.id.clone()),
            payload: PublishedPlanPayload::Node {
                task: task.clone().into(),
            },
        });
        if task.pending_handoff_to.is_some() {
            events.push(PublishedPlanEvent {
                event_id: next_published_event_id(&plan.id, &mut sequence),
                kind: PublishedPlanEventKind::ExecutionUpdated,
                plan_id: plan.id.clone(),
                node_id: Some(task.id.clone()),
                payload: PublishedPlanPayload::Execution {
                    execution: PublishedPlanExecutionOverlay {
                        pending_handoff_to: task.pending_handoff_to.clone(),
                    },
                },
            });
        }
    }
    events
}

fn append_plan_delta_events(
    starting_sequence: u64,
    previous_plan: Option<&Plan>,
    previous_tasks: &[CoordinationTask],
    plan: &Plan,
    tasks: &[CoordinationTask],
) -> Vec<PublishedPlanEvent> {
    let mut sequence = starting_sequence;
    let mut events = Vec::new();
    if previous_plan != Some(plan) {
        events.push(PublishedPlanEvent {
            event_id: next_published_event_id(&plan.id, &mut sequence),
            kind: PublishedPlanEventKind::PlanUpdated,
            plan_id: plan.id.clone(),
            node_id: None,
            payload: PublishedPlanPayload::Plan { plan: plan.clone() },
        });
    }

    let previous_tasks = previous_tasks
        .iter()
        .cloned()
        .map(|task| (task.id.0.to_string(), task))
        .collect::<BTreeMap<_, _>>();
    for task in tasks {
        let current_node: PublishedPlanNode = task.clone().into();
        let previous_task = previous_tasks.get(task.id.0.as_str());
        let previous_node = previous_task.cloned().map(PublishedPlanNode::from);
        if previous_node.as_ref() != Some(&current_node) {
            events.push(PublishedPlanEvent {
                event_id: next_published_event_id(&plan.id, &mut sequence),
                kind: PublishedPlanEventKind::NodeUpdated,
                plan_id: plan.id.clone(),
                node_id: Some(task.id.clone()),
                payload: PublishedPlanPayload::Node { task: current_node },
            });
        }

        let previous_pending = previous_task.and_then(|task| task.pending_handoff_to.clone());
        if previous_pending != task.pending_handoff_to {
            events.push(PublishedPlanEvent {
                event_id: next_published_event_id(&plan.id, &mut sequence),
                kind: PublishedPlanEventKind::ExecutionUpdated,
                plan_id: plan.id.clone(),
                node_id: Some(task.id.clone()),
                payload: PublishedPlanPayload::Execution {
                    execution: PublishedPlanExecutionOverlay {
                        pending_handoff_to: task.pending_handoff_to.clone(),
                    },
                },
            });
        }
    }

    events
}

fn relative_plan_log_path(plan: &Plan) -> PathBuf {
    let base = if matches!(plan.status, PlanStatus::Completed | PlanStatus::Abandoned) {
        PathBuf::from(".prism").join("plans").join("archived")
    } else {
        PathBuf::from(".prism").join("plans").join("active")
    };
    base.join(format!("{}.jsonl", plan.id.0))
}

fn resolve_log_path(root: &Path, raw: &str) -> PathBuf {
    let path = PathBuf::from(raw);
    if path.is_absolute() {
        path
    } else {
        root.join(path)
    }
}

fn cleanup_stale_plan_logs(plans_dir: &Path, expected_logs: &BTreeSet<String>) -> Result<()> {
    for subdir in ["active", "archived"] {
        let dir = plans_dir.join(subdir);
        if !dir.exists() {
            continue;
        }
        for entry in fs::read_dir(&dir)? {
            let entry = entry?;
            let path = entry.path();
            if path.extension().and_then(|ext| ext.to_str()) != Some("jsonl") {
                continue;
            }
            if !expected_logs.contains(&normalize_path(&path)) {
                fs::remove_file(path)?;
            }
        }
    }
    Ok(())
}

fn load_jsonl_file<T>(path: &Path) -> Result<Vec<T>>
where
    T: for<'de> Deserialize<'de>,
{
    if !path.exists() {
        return Ok(Vec::new());
    }
    let file = File::open(path)?;
    let reader = BufReader::new(file);
    let mut values = Vec::new();
    for (index, line) in reader.lines().enumerate() {
        let line = line?;
        if line.trim().is_empty() {
            continue;
        }
        let value = serde_json::from_str::<T>(&line).with_context(|| {
            format!(
                "failed to parse JSONL record on line {} in {}",
                index + 1,
                path.display()
            )
        })?;
        values.push(value);
    }
    Ok(values)
}

fn write_jsonl_file<T>(path: &Path, values: &[T]) -> Result<()>
where
    T: Serialize,
{
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)?;
    }
    let mut next = Vec::new();
    for value in values {
        serde_json::to_writer(&mut next, value)?;
        next.push(b'\n');
    }
    if fs::read(path).ok().as_deref() == Some(next.as_slice()) {
        return Ok(());
    }
    let temp_path = path.with_extension("jsonl.tmp");
    let mut file = File::create(&temp_path)?;
    file.write_all(&next)?;
    file.sync_all()?;
    fs::rename(temp_path, path)?;
    Ok(())
}

fn append_jsonl_file<T>(path: &Path, values: &[T]) -> Result<()>
where
    T: Serialize,
{
    if values.is_empty() {
        return Ok(());
    }
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)?;
    }
    let mut file = OpenOptions::new().create(true).append(true).open(path)?;
    for value in values {
        serde_json::to_writer(&mut file, value)?;
        file.write_all(b"\n")?;
    }
    file.sync_all()?;
    Ok(())
}

fn next_published_event_id(plan_id: &PlanId, sequence: &mut u64) -> String {
    *sequence += 1;
    format!("published:{}:{}", plan_id.0, sequence)
}

fn next_log_sequence_from_events(events: &[PublishedPlanEvent]) -> u64 {
    events
        .iter()
        .filter_map(|event| event.event_id.rsplit(':').next())
        .filter_map(|value| value.parse::<u64>().ok())
        .max()
        .unwrap_or(0)
}

fn next_numeric_suffix(id: &str, prefix: &str) -> u64 {
    id.strip_prefix(prefix)
        .and_then(|value| value.parse::<u64>().ok())
        .map(|value| value.saturating_add(1))
        .unwrap_or(0)
}

fn normalize_path(path: &Path) -> String {
    path.to_string_lossy().replace('\\', "/")
}

fn normalize_relative_path(path: &Path) -> String {
    normalize_path(path)
}
