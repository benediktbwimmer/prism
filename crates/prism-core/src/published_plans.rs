use std::collections::{BTreeMap, BTreeSet};
use std::fs::{self, File};
use std::io::{BufRead, BufReader, Write};
use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

use anyhow::{anyhow, Context, Result};
use prism_coordination::{
    coordination_snapshot_from_plan_graphs, execution_overlays_from_tasks, snapshot_plan_graphs,
    CoordinationSnapshot, CoordinationTask, Plan,
};
use prism_ir::{
    CoordinationTaskId, PlanEdge, PlanEdgeId, PlanExecutionOverlay, PlanGraph, PlanId, PlanNode,
    PlanNodeId, PlanNodeStatus, PlanStatus,
};
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};

use crate::protected_state::repo_streams::{
    append_protected_stream_event, implicit_principal_identity, inspect_protected_stream,
};
use crate::protected_state::streams::{classify_protected_repo_relative_path, ProtectedRepoStream};
use crate::util::{
    repo_active_plans_dir, repo_archived_plans_dir, repo_plan_index_path, repo_plans_dir,
};

fn observe_published_plan_step<T, E, O, F, A>(
    observe_phase: &mut O,
    operation: &str,
    success_args: A,
    step: F,
) -> std::result::Result<T, E>
where
    E: ToString,
    O: FnMut(&str, Duration, Value, bool, Option<String>),
    F: FnOnce() -> std::result::Result<T, E>,
    A: FnOnce(&T) -> Value,
{
    let started = Instant::now();
    match step() {
        Ok(value) => {
            observe_phase(
                operation,
                started.elapsed(),
                success_args(&value),
                true,
                None,
            );
            Ok(value)
        }
        Err(error) => {
            observe_phase(
                operation,
                started.elapsed(),
                json!({}),
                false,
                Some(error.to_string()),
            );
            Err(error)
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
enum PublishedPlanEventKind {
    PlanCreated,
    PlanUpdated,
    PlanArchived,
    NodeAdded,
    NodeUpdated,
    NodeRemoved,
    EdgeAdded,
    EdgeRemoved,
    ExecutionUpdated,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
struct PublishedPlanEvent {
    event_id: String,
    kind: PublishedPlanEventKind,
    plan_id: PlanId,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    node_id: Option<PlanNodeId>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    edge_id: Option<PlanEdgeId>,
    payload: PublishedPlanPayload,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
enum PublishedPlanPayload {
    Empty {},
    Plan { plan: PublishedPlanHeader },
    Node { node: PlanNode },
    Edge { edge: PlanEdge },
    Execution { execution: PlanExecutionOverlay },
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
struct PublishedPlanHeader {
    id: PlanId,
    scope: prism_ir::PlanScope,
    kind: prism_ir::PlanKind,
    title: String,
    goal: String,
    status: PlanStatus,
    revision: u64,
    root_nodes: Vec<PlanNodeId>,
    tags: Vec<String>,
    created_from: Option<String>,
    metadata: Value,
    policy: prism_coordination::CoordinationPolicy,
}

#[derive(Debug, Clone, PartialEq)]
struct PublishedPlanRecord {
    header: PublishedPlanHeader,
    graph: PlanGraph,
}

#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "snake_case")]
enum LegacyPublishedPlanEventKind {
    PlanUpdated,
    NodeUpdated,
    ExecutionUpdated,
}

#[derive(Debug, Clone, PartialEq, Deserialize)]
struct LegacyPublishedPlanEvent {
    event_id: String,
    kind: LegacyPublishedPlanEventKind,
    plan_id: PlanId,
    #[serde(default)]
    node_id: Option<CoordinationTaskId>,
    payload: LegacyPublishedPlanPayload,
}

#[derive(Debug, Clone, PartialEq, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
enum LegacyPublishedPlanPayload {
    Plan {
        plan: Plan,
    },
    Node {
        task: LegacyPublishedPlanNode,
    },
    Execution {
        execution: LegacyPublishedPlanExecutionOverlay,
    },
}

#[derive(Debug, Clone, PartialEq, Deserialize)]
struct LegacyPublishedPlanNode {
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

#[derive(Debug, Clone, PartialEq, Deserialize)]
struct LegacyPublishedPlanExecutionOverlay {
    pending_handoff_to: Option<prism_ir::AgentId>,
}

#[derive(Debug, Clone, PartialEq, Deserialize)]
#[serde(untagged)]
enum StoredPublishedPlanEvent {
    Native(PublishedPlanEvent),
    Legacy(LegacyPublishedPlanEvent),
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
    records: Vec<PublishedPlanRecord>,
    execution_overlays: BTreeMap<String, Vec<PlanExecutionOverlay>>,
    next_plan: u64,
    next_task: u64,
    next_log_sequence: BTreeMap<String, u64>,
}

impl PublishedPlanProjection {
    fn record(&self, plan_id: &PlanId) -> Option<PublishedPlanRecord> {
        self.records
            .iter()
            .find(|record| record.header.id == *plan_id)
            .cloned()
    }

    fn next_log_sequence_for(&self, plan_id: &PlanId) -> u64 {
        self.next_log_sequence
            .get(plan_id.0.as_str())
            .copied()
            .unwrap_or(0)
    }
}

#[derive(Debug, Clone)]
pub(crate) struct HydratedCoordinationPlanState {
    pub(crate) snapshot: CoordinationSnapshot,
    pub(crate) plan_graphs: Vec<PlanGraph>,
    pub(crate) execution_overlays: BTreeMap<String, Vec<PlanExecutionOverlay>>,
}

pub(crate) fn sync_repo_published_plans(
    root: &Path,
    snapshot: &CoordinationSnapshot,
) -> Result<()> {
    sync_repo_published_plan_state_observed(
        root,
        snapshot,
        snapshot_plan_graphs(snapshot),
        BTreeMap::new(),
        |_operation, _duration, _args, _success, _error| {},
    )
}

pub fn regenerate_repo_published_plan_artifacts(root: &Path) -> Result<()> {
    let plans_dir = repo_plans_dir(root);
    if !plans_dir.exists() {
        return Ok(());
    }
    let streams_dir = plans_dir.join("streams");
    fs::create_dir_all(&streams_dir)?;
    fs::create_dir_all(repo_active_plans_dir(root))?;
    fs::create_dir_all(repo_archived_plans_dir(root))?;

    let mut index_entries = Vec::new();
    let mut expected_derived_logs = BTreeSet::new();
    for record in load_authoritative_published_plan_records(root, &streams_dir)? {
        let relative_log_path = authoritative_plan_log_path(&record.header.id);
        let authoritative_path = root.join(&relative_log_path);
        let derived_log_path = root.join(derived_plan_log_path(
            record.header.status,
            &record.header.id,
        ));
        sync_derived_plan_log(&authoritative_path, &derived_log_path)?;
        expected_derived_logs.insert(normalize_path(&derived_log_path));
        index_entries.push(PublishedPlanIndexEntry {
            plan_id: record.header.id.clone(),
            title: record.header.title.clone(),
            status: record.header.status,
            scope: format!("{:?}", record.header.scope),
            kind: format!("{:?}", record.header.kind),
            log_path: normalize_relative_path(&relative_log_path),
        });
    }
    index_entries.sort_by(|left, right| left.plan_id.0.cmp(&right.plan_id.0));
    write_jsonl_file(&repo_plan_index_path(root), &index_entries)?;
    cleanup_stale_derived_plan_logs(&plans_dir, &expected_derived_logs)?;
    Ok(())
}

pub(crate) fn sync_repo_published_plan_state(
    root: &Path,
    snapshot: &CoordinationSnapshot,
    graphs: Vec<PlanGraph>,
    overlays_by_plan: BTreeMap<String, Vec<PlanExecutionOverlay>>,
) -> Result<()> {
    sync_repo_published_plan_state_observed(
        root,
        snapshot,
        graphs,
        overlays_by_plan,
        |_operation, _duration, _args, _success, _error| {},
    )
}

pub(crate) fn sync_repo_published_plan_state_observed<O>(
    root: &Path,
    snapshot: &CoordinationSnapshot,
    mut graphs: Vec<PlanGraph>,
    _overlays_by_plan: BTreeMap<String, Vec<PlanExecutionOverlay>>,
    mut observe_phase: O,
) -> Result<()>
where
    O: FnMut(&str, Duration, Value, bool, Option<String>),
{
    let plans_dir = repo_plans_dir(root);
    let streams_dir = plans_dir.join("streams");
    fs::create_dir_all(&streams_dir)?;
    fs::create_dir_all(repo_active_plans_dir(root))?;
    fs::create_dir_all(repo_archived_plans_dir(root))?;

    let existing_entries = observe_published_plan_step(
        &mut observe_phase,
        "mutation.coordination.publishedPlans.loadIndex",
        |entries: &Vec<PublishedPlanIndexEntry>| json!({ "entryCount": entries.len() }),
        || load_jsonl_file::<PublishedPlanIndexEntry>(&repo_plan_index_path(root)),
    )?;
    let existing_paths = existing_entries
        .into_iter()
        .map(|entry| (entry.plan_id.0.to_string(), entry.log_path))
        .collect::<BTreeMap<_, _>>();
    let existing_projection = observe_published_plan_step(
        &mut observe_phase,
        "mutation.coordination.publishedPlans.loadProjection",
        |projection: &Option<PublishedPlanProjection>| {
            json!({
                "hasProjection": projection.is_some(),
                "recordCount": projection.as_ref().map_or(0, |projection| projection.records.len()),
            })
        },
        || load_repo_published_plan_projection(root),
    )?
    .unwrap_or_default();

    let plan_policies = snapshot
        .plans
        .iter()
        .map(|plan| (plan.id.0.to_string(), plan.policy.clone()))
        .collect::<BTreeMap<_, _>>();

    let mut index_entries = Vec::new();
    let mut expected_logs = BTreeSet::new();
    graphs.sort_by(|left, right| left.id.0.cmp(&right.id.0));
    let write_logs_started = Instant::now();
    let mut logs_written = 0usize;
    let mut events_written = 0usize;
    let write_logs_result: Result<()> = (|| {
        for graph in graphs {
            let plan_key = graph.id.0.to_string();
            let header = PublishedPlanHeader {
                id: graph.id.clone(),
                scope: graph.scope,
                kind: graph.kind,
                title: graph.title.clone(),
                goal: graph.goal.clone(),
                status: graph.status,
                revision: graph.revision,
                root_nodes: graph.root_nodes.clone(),
                tags: graph.tags.clone(),
                created_from: graph.created_from.clone(),
                metadata: graph.metadata.clone(),
                policy: plan_policies
                    .get(plan_key.as_str())
                    .cloned()
                    .unwrap_or_default(),
            };

            let relative_log_path = authoritative_plan_log_path(&header.id);
            let full_log_path = root.join(&relative_log_path);
            let previous_record = if full_log_path.exists() {
                existing_projection.record(&graph.id)
            } else {
                None
            };
            let starting_sequence = if full_log_path.exists() {
                existing_projection.next_log_sequence_for(&graph.id)
            } else {
                0
            };
            let events = if previous_record.is_none() {
                published_plan_events(starting_sequence, &header, &graph)
            } else {
                append_plan_delta_events(
                    starting_sequence,
                    previous_record.as_ref(),
                    &header,
                    &graph,
                )
            };
            if !events.is_empty() {
                let stream = ProtectedRepoStream::plan_stream(&header.id);
                let principal = implicit_principal_identity(None, None);
                for event in &events {
                    append_protected_stream_event(
                        root,
                        &stream,
                        &event.event_id,
                        event,
                        &principal,
                    )?;
                }
                logs_written += 1;
                events_written += events.len();
            }
            expected_logs.insert(normalize_path(&full_log_path));
            let derived_log_path = root.join(derived_plan_log_path(header.status, &header.id));
            sync_derived_plan_log(&full_log_path, &derived_log_path)?;
            expected_logs.insert(normalize_path(&derived_log_path));
            index_entries.push(PublishedPlanIndexEntry {
                plan_id: header.id.clone(),
                title: header.title.clone(),
                status: header.status,
                scope: format!("{:?}", header.scope),
                kind: format!("{:?}", header.kind),
                log_path: normalize_relative_path(&relative_log_path),
            });
            if let Some(previous_path) = existing_paths.get(&plan_key) {
                let previous_path = resolve_log_path(root, previous_path);
                if previous_path != full_log_path && previous_path != derived_log_path {
                    remove_legacy_plan_log_if_stale(
                        &previous_path,
                        &full_log_path,
                        &derived_log_path,
                    )?;
                }
            }
        }
        Ok(())
    })();
    match write_logs_result {
        Ok(()) => observe_phase(
            "mutation.coordination.publishedPlans.writeLogs",
            write_logs_started.elapsed(),
            json!({
                "eventCount": events_written,
                "logCount": logs_written,
            }),
            true,
            None,
        ),
        Err(error) => {
            observe_phase(
                "mutation.coordination.publishedPlans.writeLogs",
                write_logs_started.elapsed(),
                json!({
                    "eventCount": events_written,
                    "logCount": logs_written,
                }),
                false,
                Some(error.to_string()),
            );
            return Err(error);
        }
    }

    index_entries.sort_by(|left, right| left.plan_id.0.cmp(&right.plan_id.0));
    observe_published_plan_step(
        &mut observe_phase,
        "mutation.coordination.publishedPlans.writeIndex",
        |_| json!({ "entryCount": index_entries.len() }),
        || write_jsonl_file(&repo_plan_index_path(root), &index_entries),
    )?;
    observe_published_plan_step(
        &mut observe_phase,
        "mutation.coordination.publishedPlans.cleanupLogs",
        |_| json!({ "expectedLogCount": expected_logs.len() }),
        || cleanup_stale_plan_logs(&plans_dir, &expected_logs),
    )?;
    Ok(())
}

pub(crate) fn load_hydrated_coordination_snapshot(
    root: &Path,
    snapshot: Option<CoordinationSnapshot>,
) -> Result<Option<CoordinationSnapshot>> {
    match (snapshot, load_repo_published_plan_projection(root)?) {
        (Some(snapshot), Some(published)) => Ok(Some(merge_published_plans_into_snapshot(
            snapshot,
            hydrated_plan_state_from_projection(published).snapshot,
        ))),
        (Some(snapshot), None) => Ok(Some(snapshot)),
        (None, Some(published)) => Ok(Some(
            hydrated_plan_state_from_projection(published).snapshot,
        )),
        (None, None) => Ok(None),
    }
}

pub(crate) fn load_hydrated_coordination_plan_state(
    root: &Path,
    snapshot: Option<CoordinationSnapshot>,
) -> Result<Option<HydratedCoordinationPlanState>> {
    match (snapshot, load_repo_published_plan_projection(root)?) {
        (Some(snapshot), Some(published)) => {
            let mut state = hydrated_plan_state_from_projection(published);
            merge_snapshot_bootstrap_into_plan_state(
                &snapshot,
                &mut state.plan_graphs,
                &mut state.execution_overlays,
            );
            let snapshot = merge_published_plans_into_snapshot(snapshot, state.snapshot);
            Ok(Some(HydratedCoordinationPlanState {
                snapshot,
                plan_graphs: state.plan_graphs,
                execution_overlays: state.execution_overlays,
            }))
        }
        (Some(snapshot), None) => Ok(Some(HydratedCoordinationPlanState {
            plan_graphs: snapshot_plan_graphs(&snapshot),
            execution_overlays: execution_overlays_by_plan(&snapshot.tasks),
            snapshot,
        })),
        (None, Some(published)) => Ok(Some(hydrated_plan_state_from_projection(published))),
        (None, None) => Ok(None),
    }
}

fn hydrated_plan_state_from_projection(
    published: PublishedPlanProjection,
) -> HydratedCoordinationPlanState {
    let mut graphs = published
        .records
        .iter()
        .map(|record| record.graph.clone())
        .collect::<Vec<_>>();
    let execution_overlays = published.execution_overlays.clone();
    let mut snapshot = coordination_snapshot_from_plan_graphs(&graphs, &execution_overlays);
    let policies = published
        .records
        .into_iter()
        .map(|record| (record.header.id.0.to_string(), record.header.policy))
        .collect::<BTreeMap<_, _>>();
    for plan in &mut snapshot.plans {
        if let Some(policy) = policies.get(plan.id.0.as_str()) {
            plan.policy = policy.clone();
        }
    }
    snapshot.next_plan = snapshot.next_plan.max(published.next_plan);
    snapshot.next_task = snapshot.next_task.max(published.next_task);
    graphs.sort_by(|left, right| left.id.0.cmp(&right.id.0));
    HydratedCoordinationPlanState {
        snapshot,
        plan_graphs: graphs,
        execution_overlays,
    }
}

fn merge_snapshot_bootstrap_into_plan_state(
    snapshot: &CoordinationSnapshot,
    graphs: &mut Vec<PlanGraph>,
    execution_overlays: &mut BTreeMap<String, Vec<PlanExecutionOverlay>>,
) {
    let snapshot_graphs = snapshot_plan_graphs(snapshot);
    let snapshot_execution = execution_overlays_by_plan(&snapshot.tasks);
    let existing_plan_ids = graphs
        .iter()
        .map(|graph| graph.id.0.to_string())
        .collect::<BTreeSet<_>>();
    for graph in snapshot_graphs {
        if existing_plan_ids.contains(graph.id.0.as_str()) {
            continue;
        }
        execution_overlays
            .entry(graph.id.0.to_string())
            .or_insert_with(|| {
                snapshot_execution
                    .get(graph.id.0.as_str())
                    .cloned()
                    .unwrap_or_default()
            });
        graphs.push(graph);
    }
    graphs.sort_by(|left, right| left.id.0.cmp(&right.id.0));
}

fn merge_published_plans_into_snapshot(
    mut snapshot: CoordinationSnapshot,
    published_snapshot: CoordinationSnapshot,
) -> CoordinationSnapshot {
    let published_plan_ids = published_snapshot
        .plans
        .iter()
        .map(|plan| plan.id.0.to_string())
        .collect::<BTreeSet<_>>();
    let task_backed_plan_ids = published_snapshot
        .plans
        .iter()
        .filter(|plan| plan.kind == prism_ir::PlanKind::TaskExecution)
        .map(|plan| plan.id.0.to_string())
        .collect::<BTreeSet<_>>();
    let runtime_scope_by_task = snapshot
        .tasks
        .iter()
        .map(|task| {
            (
                task.id.clone(),
                (
                    task.pending_handoff_to.clone(),
                    task.session.clone(),
                    task.worktree_id.clone(),
                    task.branch_ref.clone(),
                ),
            )
        })
        .collect::<BTreeMap<_, _>>();
    snapshot
        .plans
        .retain(|plan| !published_plan_ids.contains(plan.id.0.as_str()));
    snapshot
        .tasks
        .retain(|task| !task_backed_plan_ids.contains(task.plan.0.as_str()));
    snapshot.plans.extend(published_snapshot.plans);
    snapshot.tasks.extend(
        published_snapshot
            .tasks
            .into_iter()
            .filter(|task| task_backed_plan_ids.contains(task.plan.0.as_str()))
            .filter(|task| task.id.0.starts_with("coord-task:"))
            .map(|mut task| {
                if let Some((pending_handoff_to, session, worktree_id, branch_ref)) =
                    runtime_scope_by_task.get(&task.id)
                {
                    task.pending_handoff_to = pending_handoff_to.clone();
                    task.session = session.clone();
                    task.worktree_id = worktree_id.clone();
                    task.branch_ref = branch_ref.clone();
                }
                task
            }),
    );
    snapshot
        .plans
        .sort_by(|left, right| left.id.0.cmp(&right.id.0));
    snapshot
        .tasks
        .sort_by(|left, right| left.id.0.cmp(&right.id.0));
    snapshot.next_plan = snapshot.next_plan.max(published_snapshot.next_plan);
    snapshot.next_task = snapshot.next_task.max(published_snapshot.next_task);
    snapshot.next_claim = snapshot.next_claim.max(published_snapshot.next_claim);
    snapshot.next_artifact = snapshot.next_artifact.max(published_snapshot.next_artifact);
    snapshot.next_review = snapshot.next_review.max(published_snapshot.next_review);
    snapshot
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
        let events = load_stored_plan_events(root, &log_path)?;
        let (record, overlays) = project_plan_log(&log_path, &events)?;
        projection.next_plan = projection
            .next_plan
            .max(next_numeric_suffix(&record.header.id.0, "plan:"));
        projection.next_task = record
            .graph
            .nodes
            .iter()
            .fold(projection.next_task, |current, node| {
                current.max(next_numeric_suffix(&node.id.0, "coord-task:"))
            });
        projection.next_log_sequence.insert(
            record.header.id.0.to_string(),
            next_log_sequence_from_events(&events),
        );
        projection
            .execution_overlays
            .insert(record.header.id.0.to_string(), overlays);
        projection.records.push(record);
    }
    projection
        .records
        .sort_by(|left, right| left.header.id.0.cmp(&right.header.id.0));
    Ok(Some(projection))
}

fn project_plan_log(
    path: &Path,
    events: &[StoredPublishedPlanEvent],
) -> Result<(PublishedPlanRecord, Vec<PlanExecutionOverlay>)> {
    let mut header = None;
    let mut nodes = BTreeMap::<String, PlanNode>::new();
    let mut edges = BTreeMap::<String, PlanEdge>::new();
    let mut overlays = BTreeMap::<String, PlanExecutionOverlay>::new();
    for event in events {
        match event {
            StoredPublishedPlanEvent::Native(event) => {
                apply_native_event(event, &mut header, &mut nodes, &mut edges, &mut overlays);
            }
            StoredPublishedPlanEvent::Legacy(event) => {
                apply_legacy_event(event, &mut header, &mut nodes, &mut edges, &mut overlays);
            }
        }
    }

    let header = header.ok_or_else(|| {
        anyhow!(
            "published plan log {} did not contain a plan record",
            path.display()
        )
    })?;
    let graph = PlanGraph {
        id: header.id.clone(),
        scope: header.scope,
        kind: header.kind,
        title: header.title.clone(),
        goal: header.goal.clone(),
        status: header.status,
        revision: header.revision,
        root_nodes: header.root_nodes.clone(),
        tags: header.tags.clone(),
        created_from: header.created_from.clone(),
        metadata: header.metadata.clone(),
        nodes: nodes.into_values().collect(),
        edges: edges.into_values().collect(),
    };
    let overlays = overlays.into_values().collect::<Vec<_>>();
    Ok((PublishedPlanRecord { header, graph }, overlays))
}

fn apply_native_event(
    event: &PublishedPlanEvent,
    header: &mut Option<PublishedPlanHeader>,
    nodes: &mut BTreeMap<String, PlanNode>,
    edges: &mut BTreeMap<String, PlanEdge>,
    overlays: &mut BTreeMap<String, PlanExecutionOverlay>,
) {
    match &event.payload {
        PublishedPlanPayload::Plan { plan } => {
            *header = Some(plan.clone());
        }
        PublishedPlanPayload::Node { node } => {
            nodes.insert(node.id.0.to_string(), node.clone());
        }
        PublishedPlanPayload::Edge { edge } => {
            edges.insert(edge.id.0.to_string(), edge.clone());
        }
        PublishedPlanPayload::Execution { execution } => {
            if execution.pending_handoff_to.is_some() || execution.session.is_some() {
                overlays.insert(execution.node_id.0.to_string(), execution.clone());
            } else {
                overlays.remove(execution.node_id.0.as_str());
            }
        }
        PublishedPlanPayload::Empty {} => {}
    }

    match event.kind {
        PublishedPlanEventKind::NodeRemoved => {
            if let Some(node_id) = &event.node_id {
                nodes.remove(node_id.0.as_str());
                overlays.remove(node_id.0.as_str());
                edges.retain(|_, edge| edge.from != *node_id && edge.to != *node_id);
            }
        }
        PublishedPlanEventKind::EdgeRemoved => {
            if let Some(edge_id) = &event.edge_id {
                edges.remove(edge_id.0.as_str());
            }
        }
        _ => {}
    }
}

fn apply_legacy_event(
    event: &LegacyPublishedPlanEvent,
    header: &mut Option<PublishedPlanHeader>,
    nodes: &mut BTreeMap<String, PlanNode>,
    edges: &mut BTreeMap<String, PlanEdge>,
    overlays: &mut BTreeMap<String, PlanExecutionOverlay>,
) {
    match &event.payload {
        LegacyPublishedPlanPayload::Plan { plan } => {
            *header = Some(legacy_header_from_plan(plan));
        }
        LegacyPublishedPlanPayload::Node { task } => {
            let node = legacy_node_from_task(task);
            nodes.insert(node.id.0.to_string(), node);
            let from = PlanNodeId::new(task.id.0.to_string());
            edges.retain(|_, edge| {
                !(edge.from == from && edge.kind == prism_ir::PlanEdgeKind::DependsOn)
            });
            for edge in legacy_dependency_edges_for_task(task) {
                edges.insert(edge.id.0.to_string(), edge);
            }
        }
        LegacyPublishedPlanPayload::Execution { execution } => {
            let Some(node_id) = &event.node_id else {
                return;
            };
            if execution.pending_handoff_to.is_some() {
                overlays.insert(
                    node_id.0.to_string(),
                    PlanExecutionOverlay {
                        node_id: PlanNodeId::new(node_id.0.to_string()),
                        pending_handoff_to: execution.pending_handoff_to.clone(),
                        session: None,
                        worktree_id: None,
                        branch_ref: None,
                        effective_assignee: None,
                        awaiting_handoff_from: None,
                    },
                );
            } else {
                overlays.remove(node_id.0.as_str());
            }
        }
    }
}

fn published_plan_events(
    starting_sequence: u64,
    header: &PublishedPlanHeader,
    graph: &PlanGraph,
) -> Vec<PublishedPlanEvent> {
    let mut sequence = starting_sequence;
    let mut events = Vec::with_capacity(graph.nodes.len() + graph.edges.len() + 1);
    events.push(PublishedPlanEvent {
        event_id: next_published_event_id(&header.id, &mut sequence),
        kind: PublishedPlanEventKind::PlanCreated,
        plan_id: header.id.clone(),
        node_id: None,
        edge_id: None,
        payload: PublishedPlanPayload::Plan {
            plan: header.clone(),
        },
    });
    for node in sorted_nodes(&graph.nodes) {
        events.push(PublishedPlanEvent {
            event_id: next_published_event_id(&header.id, &mut sequence),
            kind: PublishedPlanEventKind::NodeAdded,
            plan_id: header.id.clone(),
            node_id: Some(node.id.clone()),
            edge_id: None,
            payload: PublishedPlanPayload::Node { node },
        });
    }
    for edge in sorted_edges(&graph.edges) {
        events.push(PublishedPlanEvent {
            event_id: next_published_event_id(&header.id, &mut sequence),
            kind: PublishedPlanEventKind::EdgeAdded,
            plan_id: header.id.clone(),
            node_id: None,
            edge_id: Some(edge.id.clone()),
            payload: PublishedPlanPayload::Edge { edge },
        });
    }
    events
}

fn append_plan_delta_events(
    starting_sequence: u64,
    previous_record: Option<&PublishedPlanRecord>,
    header: &PublishedPlanHeader,
    graph: &PlanGraph,
) -> Vec<PublishedPlanEvent> {
    let mut sequence = starting_sequence;
    let mut events = Vec::new();
    let previous_header = previous_record.map(|record| &record.header);
    if previous_header != Some(header) {
        let kind = if previous_header
            .map(|previous| previous.status != PlanStatus::Archived)
            .unwrap_or(false)
            && header.status == PlanStatus::Archived
        {
            PublishedPlanEventKind::PlanArchived
        } else {
            PublishedPlanEventKind::PlanUpdated
        };
        events.push(PublishedPlanEvent {
            event_id: next_published_event_id(&header.id, &mut sequence),
            kind,
            plan_id: header.id.clone(),
            node_id: None,
            edge_id: None,
            payload: PublishedPlanPayload::Plan {
                plan: header.clone(),
            },
        });
    }

    let previous_nodes = previous_record
        .map(|record| {
            record
                .graph
                .nodes
                .iter()
                .cloned()
                .map(|node| (node.id.0.to_string(), node))
                .collect::<BTreeMap<_, _>>()
        })
        .unwrap_or_default();
    let current_nodes = graph
        .nodes
        .iter()
        .cloned()
        .map(|node| (node.id.0.to_string(), node))
        .collect::<BTreeMap<_, _>>();
    for node_id in previous_nodes
        .keys()
        .filter(|node_id| !current_nodes.contains_key(*node_id))
    {
        events.push(PublishedPlanEvent {
            event_id: next_published_event_id(&header.id, &mut sequence),
            kind: PublishedPlanEventKind::NodeRemoved,
            plan_id: header.id.clone(),
            node_id: Some(PlanNodeId::new((*node_id).clone())),
            edge_id: None,
            payload: PublishedPlanPayload::Empty {},
        });
    }
    for node in current_nodes.values() {
        match previous_nodes.get(node.id.0.as_str()) {
            None => events.push(PublishedPlanEvent {
                event_id: next_published_event_id(&header.id, &mut sequence),
                kind: PublishedPlanEventKind::NodeAdded,
                plan_id: header.id.clone(),
                node_id: Some(node.id.clone()),
                edge_id: None,
                payload: PublishedPlanPayload::Node { node: node.clone() },
            }),
            Some(previous) if previous != node => events.push(PublishedPlanEvent {
                event_id: next_published_event_id(&header.id, &mut sequence),
                kind: PublishedPlanEventKind::NodeUpdated,
                plan_id: header.id.clone(),
                node_id: Some(node.id.clone()),
                edge_id: None,
                payload: PublishedPlanPayload::Node { node: node.clone() },
            }),
            _ => {}
        }
    }

    let previous_edges = previous_record
        .map(|record| {
            record
                .graph
                .edges
                .iter()
                .cloned()
                .map(|edge| (edge.id.0.to_string(), edge))
                .collect::<BTreeMap<_, _>>()
        })
        .unwrap_or_default();
    let current_edges = graph
        .edges
        .iter()
        .cloned()
        .map(|edge| (edge.id.0.to_string(), edge))
        .collect::<BTreeMap<_, _>>();
    for edge in previous_edges.values() {
        match current_edges.get(edge.id.0.as_str()) {
            None => events.push(PublishedPlanEvent {
                event_id: next_published_event_id(&header.id, &mut sequence),
                kind: PublishedPlanEventKind::EdgeRemoved,
                plan_id: header.id.clone(),
                node_id: None,
                edge_id: Some(edge.id.clone()),
                payload: PublishedPlanPayload::Empty {},
            }),
            Some(current) if current != edge => {
                events.push(PublishedPlanEvent {
                    event_id: next_published_event_id(&header.id, &mut sequence),
                    kind: PublishedPlanEventKind::EdgeRemoved,
                    plan_id: header.id.clone(),
                    node_id: None,
                    edge_id: Some(edge.id.clone()),
                    payload: PublishedPlanPayload::Empty {},
                });
                events.push(PublishedPlanEvent {
                    event_id: next_published_event_id(&header.id, &mut sequence),
                    kind: PublishedPlanEventKind::EdgeAdded,
                    plan_id: header.id.clone(),
                    node_id: None,
                    edge_id: Some(current.id.clone()),
                    payload: PublishedPlanPayload::Edge {
                        edge: current.clone(),
                    },
                });
            }
            _ => {}
        }
    }
    for edge in current_edges.values() {
        if !previous_edges.contains_key(edge.id.0.as_str()) {
            events.push(PublishedPlanEvent {
                event_id: next_published_event_id(&header.id, &mut sequence),
                kind: PublishedPlanEventKind::EdgeAdded,
                plan_id: header.id.clone(),
                node_id: None,
                edge_id: Some(edge.id.clone()),
                payload: PublishedPlanPayload::Edge { edge: edge.clone() },
            });
        }
    }

    events
}

fn repo_published_execution_overlays(
    overlays: Vec<PlanExecutionOverlay>,
) -> Vec<PlanExecutionOverlay> {
    let mut overlays = overlays
        .into_iter()
        // Repo-published plan streams must stay self-contained and repo-semantic. Runtime
        // correlation like session/worktree/branch remains in the shared runtime snapshot and is
        // never serialized into `.prism` plan logs.
        .map(|overlay| PlanExecutionOverlay {
            node_id: overlay.node_id,
            pending_handoff_to: overlay.pending_handoff_to,
            session: None,
            worktree_id: None,
            branch_ref: None,
            effective_assignee: None,
            awaiting_handoff_from: None,
        })
        .filter(|overlay| overlay.pending_handoff_to.is_some())
        .collect::<Vec<_>>();
    overlays.sort_by(|left, right| left.node_id.0.cmp(&right.node_id.0));
    overlays
}

fn execution_overlays_by_plan(
    tasks: &[CoordinationTask],
) -> BTreeMap<String, Vec<PlanExecutionOverlay>> {
    tasks
        .iter()
        .cloned()
        .fold(
            BTreeMap::<String, Vec<CoordinationTask>>::new(),
            |mut map, task| {
                map.entry(task.plan.0.to_string()).or_default().push(task);
                map
            },
        )
        .into_iter()
        .map(|(plan_id, tasks)| {
            (
                plan_id,
                repo_published_execution_overlays(execution_overlays_from_tasks(&tasks)),
            )
        })
        .collect()
}

fn sorted_nodes(nodes: &[PlanNode]) -> Vec<PlanNode> {
    let mut nodes = nodes.to_vec();
    nodes.sort_by(|left, right| left.id.0.cmp(&right.id.0));
    nodes
}

fn sorted_edges(edges: &[PlanEdge]) -> Vec<PlanEdge> {
    let mut edges = edges.to_vec();
    edges.sort_by(|left, right| left.id.0.cmp(&right.id.0));
    edges
}

fn legacy_header_from_plan(plan: &Plan) -> PublishedPlanHeader {
    PublishedPlanHeader {
        id: plan.id.clone(),
        scope: prism_ir::PlanScope::Repo,
        kind: prism_ir::PlanKind::TaskExecution,
        title: plan.goal.clone(),
        goal: plan.goal.clone(),
        status: plan.status,
        revision: 0,
        root_nodes: plan
            .root_tasks
            .iter()
            .cloned()
            .map(|task_id| PlanNodeId::new(task_id.0))
            .collect(),
        tags: Vec::new(),
        created_from: None,
        metadata: Value::Null,
        policy: plan.policy.clone(),
    }
}

fn legacy_node_from_task(task: &LegacyPublishedPlanNode) -> PlanNode {
    PlanNode {
        id: PlanNodeId::new(task.id.0.to_string()),
        plan_id: task.plan.clone(),
        kind: prism_ir::PlanNodeKind::Edit,
        title: task.title.clone(),
        summary: None,
        status: match task.status {
            prism_ir::CoordinationTaskStatus::Proposed => PlanNodeStatus::Proposed,
            prism_ir::CoordinationTaskStatus::Ready => PlanNodeStatus::Ready,
            prism_ir::CoordinationTaskStatus::InProgress => PlanNodeStatus::InProgress,
            prism_ir::CoordinationTaskStatus::Blocked => PlanNodeStatus::Blocked,
            prism_ir::CoordinationTaskStatus::InReview => PlanNodeStatus::InReview,
            prism_ir::CoordinationTaskStatus::Validating => PlanNodeStatus::Validating,
            prism_ir::CoordinationTaskStatus::Completed => PlanNodeStatus::Completed,
            prism_ir::CoordinationTaskStatus::Abandoned => PlanNodeStatus::Abandoned,
        },
        bindings: prism_ir::PlanBinding {
            anchors: task.anchors.clone(),
            concept_handles: Vec::new(),
            artifact_refs: Vec::new(),
            memory_refs: Vec::new(),
            outcome_refs: Vec::new(),
        },
        acceptance: task
            .acceptance
            .iter()
            .cloned()
            .map(|criterion| prism_ir::PlanAcceptanceCriterion {
                label: criterion.label,
                anchors: criterion.anchors,
                required_checks: Vec::new(),
                evidence_policy: prism_ir::AcceptanceEvidencePolicy::Any,
            })
            .collect(),
        validation_refs: Vec::new(),
        is_abstract: false,
        assignee: task.assignee.clone(),
        base_revision: task.base_revision.clone(),
        priority: None,
        tags: Vec::new(),
        metadata: Value::Null,
    }
}

fn legacy_dependency_edges_for_task(task: &LegacyPublishedPlanNode) -> Vec<PlanEdge> {
    let mut seen = BTreeSet::new();
    let mut edges = Vec::new();
    for dependency in &task.depends_on {
        if !seen.insert(dependency.0.to_string()) {
            continue;
        }
        edges.push(PlanEdge {
            id: PlanEdgeId::new(format!(
                "plan-edge:{}:depends-on:{}",
                task.id.0, dependency.0
            )),
            plan_id: task.plan.clone(),
            from: PlanNodeId::new(task.id.0.to_string()),
            to: PlanNodeId::new(dependency.0.to_string()),
            kind: prism_ir::PlanEdgeKind::DependsOn,
            summary: None,
            metadata: Value::Null,
        });
    }
    edges
}

fn authoritative_plan_log_path(plan_id: &PlanId) -> PathBuf {
    PathBuf::from(".prism")
        .join("plans")
        .join("streams")
        .join(format!("{}.jsonl", plan_id.0))
}

fn derived_plan_log_path(status: PlanStatus, plan_id: &PlanId) -> PathBuf {
    let base = if matches!(status, PlanStatus::Archived) {
        PathBuf::from(".prism").join("plans").join("archived")
    } else {
        PathBuf::from(".prism").join("plans").join("active")
    };
    base.join(format!("{}.jsonl", plan_id.0))
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
    for subdir in ["active", "archived", "streams"] {
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

fn cleanup_stale_derived_plan_logs(
    plans_dir: &Path,
    expected_derived_logs: &BTreeSet<String>,
) -> Result<()> {
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
            if !expected_derived_logs.contains(&normalize_path(&path)) {
                fs::remove_file(path)?;
            }
        }
    }
    Ok(())
}

fn load_authoritative_published_plan_records(
    root: &Path,
    streams_dir: &Path,
) -> Result<Vec<PublishedPlanRecord>> {
    let mut records = Vec::new();
    if !streams_dir.exists() {
        return Ok(records);
    }
    for entry in fs::read_dir(streams_dir)? {
        let entry = entry?;
        let path = entry.path();
        if path.extension().and_then(|ext| ext.to_str()) != Some("jsonl") {
            continue;
        }
        let Ok(relative) = path.strip_prefix(root) else {
            continue;
        };
        let Some(stream) = classify_protected_repo_relative_path(relative) else {
            continue;
        };
        if stream.stream() != "repo_plan_events" {
            continue;
        }
        let events = load_stored_plan_events(root, &path)?;
        let (record, _) = project_plan_log(&path, &events)?;
        records.push(record);
    }
    records.sort_by(|left, right| left.header.id.0.cmp(&right.header.id.0));
    Ok(records)
}

fn load_stored_plan_events(root: &Path, log_path: &Path) -> Result<Vec<StoredPublishedPlanEvent>> {
    let relative = log_path
        .strip_prefix(root)
        .ok()
        .map(Path::to_path_buf)
        .unwrap_or_else(|| log_path.to_path_buf());
    match classify_protected_repo_relative_path(&relative) {
        Some(stream) if stream.stream() == "repo_plan_events" => {
            let inspection = inspect_protected_stream::<PublishedPlanEvent>(root, &stream)?;
            if inspection.verification.verification_status
                != crate::protected_state::streams::ProtectedVerificationStatus::Verified
            {
                return Err(anyhow!(
                    "refused to hydrate protected plan stream {} because verification status is {:?}: {}",
                    log_path.display(),
                    inspection.verification.verification_status,
                    inspection
                        .verification
                        .diagnostic_summary
                        .as_deref()
                        .unwrap_or("verification failed"),
                ));
            }
            Ok(inspection
                .payloads
                .into_iter()
                .map(StoredPublishedPlanEvent::Native)
                .collect())
        }
        _ => load_jsonl_file::<StoredPublishedPlanEvent>(log_path),
    }
}

fn sync_derived_plan_log(authoritative_path: &Path, derived_path: &Path) -> Result<()> {
    if let Some(parent) = derived_path.parent() {
        fs::create_dir_all(parent)?;
    }
    let bytes = fs::read(authoritative_path)?;
    if fs::read(derived_path).ok().as_deref() != Some(bytes.as_slice()) {
        fs::write(derived_path, bytes)?;
    }
    let opposite_path = if derived_path
        .components()
        .any(|component| component.as_os_str() == "archived")
    {
        derived_path
            .parent()
            .and_then(Path::parent)
            .map(|plans_dir| {
                plans_dir
                    .join("active")
                    .join(derived_path.file_name().unwrap())
            })
    } else {
        derived_path
            .parent()
            .and_then(Path::parent)
            .map(|plans_dir| {
                plans_dir
                    .join("archived")
                    .join(derived_path.file_name().unwrap())
            })
    };
    if let Some(opposite_path) = opposite_path {
        if opposite_path.exists() {
            fs::remove_file(opposite_path)?;
        }
    }
    Ok(())
}

fn remove_legacy_plan_log_if_stale(
    previous_path: &Path,
    authoritative_path: &Path,
    derived_path: &Path,
) -> Result<()> {
    if previous_path == authoritative_path || previous_path == derived_path {
        return Ok(());
    }
    if previous_path.exists() {
        fs::remove_file(previous_path)?;
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

fn next_published_event_id(plan_id: &PlanId, sequence: &mut u64) -> String {
    *sequence += 1;
    format!("published:{}:{}", plan_id.0, sequence)
}

fn next_log_sequence_from_events(events: &[StoredPublishedPlanEvent]) -> u64 {
    events
        .iter()
        .filter_map(|event| match event {
            StoredPublishedPlanEvent::Native(event) => event.event_id.rsplit(':').next(),
            StoredPublishedPlanEvent::Legacy(event) => event.event_id.rsplit(':').next(),
        })
        .filter_map(|value| value.parse::<u64>().ok())
        .max()
        .unwrap_or(0)
}

fn next_numeric_suffix(value: &str, prefix: &str) -> u64 {
    value
        .strip_prefix(prefix)
        .and_then(|value| value.parse::<u64>().ok())
        .unwrap_or(0)
}

fn normalize_path(path: &Path) -> String {
    normalize_relative_path(path)
}

fn normalize_relative_path(path: impl AsRef<Path>) -> String {
    path.as_ref().to_string_lossy().replace('\\', "/")
}
