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
    implicit_principal_identity, inspect_protected_stream, rewrite_protected_stream_events,
};
use crate::protected_state::streams::classify_protected_repo_relative_path;
use crate::shared_coordination_ref::{
    load_shared_coordination_ref_state, sync_shared_coordination_ref_state,
};
use crate::tracked_snapshot::{
    load_tracked_coordination_snapshot_state, remove_obsolete_legacy_tracked_authority_artifacts,
    sync_coordination_snapshot_state, tracked_snapshot_authority_active,
    TrackedSnapshotPublishContext,
};
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

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PublishedPlanArtifactRepairEntry {
    pub plan_id: PlanId,
    pub protected_path: String,
    pub event_count_before: usize,
    pub event_count_after: usize,
    pub redundant_edge_add_count: usize,
    pub repaired: bool,
    pub skipped_legacy_stream: bool,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PublishedPlanArtifactRepairReport {
    pub scanned_plan_count: usize,
    pub repaired_plan_count: usize,
    pub redundant_edge_add_count: usize,
    pub entries: Vec<PublishedPlanArtifactRepairEntry>,
}

#[derive(Debug, Default)]
struct PublishedPlanProjection {
    records: Vec<PublishedPlanRecord>,
    execution_overlays: BTreeMap<String, Vec<PlanExecutionOverlay>>,
    next_plan: u64,
    next_task: u64,
    next_log_sequence: BTreeMap<String, u64>,
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
    publish: Option<&TrackedSnapshotPublishContext>,
) -> Result<()> {
    sync_repo_published_plan_state_observed(
        root,
        snapshot,
        None,
        None,
        snapshot_plan_graphs(snapshot),
        BTreeMap::new(),
        publish,
        |_operation, _duration, _args, _success, _error| {},
    )
}

pub(crate) fn load_repo_published_plan_index(root: &Path) -> Result<Vec<PublishedPlanIndexEntry>> {
    if tracked_snapshot_authority_active(root)? {
        return load_snapshot_published_plan_index(root);
    }
    let index_path = repo_plan_index_path(root);
    if !index_path.exists() {
        return Ok(Vec::new());
    }
    let mut entries = load_jsonl_file::<PublishedPlanIndexEntry>(&index_path)?;
    entries.sort_by(|left, right| left.plan_id.0.cmp(&right.plan_id.0));
    Ok(entries)
}

pub fn regenerate_repo_published_plan_artifacts(root: &Path) -> Result<()> {
    if tracked_snapshot_authority_active(root)? {
        remove_obsolete_legacy_tracked_authority_artifacts(root)?;
        return Ok(());
    }
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

fn load_snapshot_published_plan_index(root: &Path) -> Result<Vec<PublishedPlanIndexEntry>> {
    let Some(state) = load_tracked_coordination_snapshot_state(root)? else {
        return Ok(Vec::new());
    };
    let mut entries = state
        .plan_graphs
        .into_iter()
        .map(|graph| PublishedPlanIndexEntry {
            plan_id: graph.id,
            title: graph.title,
            status: graph.status,
            scope: format!("{:?}", graph.scope),
            kind: format!("{:?}", graph.kind),
            log_path: String::new(),
        })
        .collect::<Vec<_>>();
    entries.sort_by(|left, right| left.plan_id.0.cmp(&right.plan_id.0));
    Ok(entries)
}

pub fn inspect_repo_published_plan_artifacts(
    root: &Path,
) -> Result<PublishedPlanArtifactRepairReport> {
    scan_or_repair_repo_published_plan_artifacts(root, false)
}

pub fn repair_repo_published_plan_artifacts(
    root: &Path,
) -> Result<PublishedPlanArtifactRepairReport> {
    scan_or_repair_repo_published_plan_artifacts(root, true)
}

pub(crate) fn sync_repo_published_plan_state(
    root: &Path,
    snapshot: &CoordinationSnapshot,
    previous_snapshot: Option<&CoordinationSnapshot>,
    previous_graphs: Option<&[PlanGraph]>,
    graphs: Vec<PlanGraph>,
    overlays_by_plan: BTreeMap<String, Vec<PlanExecutionOverlay>>,
    publish: Option<&TrackedSnapshotPublishContext>,
) -> Result<()> {
    sync_repo_published_plan_state_observed(
        root,
        snapshot,
        previous_snapshot,
        previous_graphs,
        graphs,
        overlays_by_plan,
        publish,
        |_operation, _duration, _args, _success, _error| {},
    )
}

pub(crate) fn sync_repo_published_plan_state_observed<O>(
    root: &Path,
    snapshot: &CoordinationSnapshot,
    _previous_snapshot: Option<&CoordinationSnapshot>,
    _previous_graphs: Option<&[PlanGraph]>,
    graphs: Vec<PlanGraph>,
    overlays_by_plan: BTreeMap<String, Vec<PlanExecutionOverlay>>,
    publish: Option<&TrackedSnapshotPublishContext>,
    mut observe_phase: O,
) -> Result<()>
where
    O: FnMut(&str, Duration, Value, bool, Option<String>),
{
    observe_phase(
        "mutation.coordination.publishedPlans.writeLogs",
        Duration::ZERO,
        json!({
            "eventCount": 0,
            "logCount": 0,
            "skipped": true,
            "reason": "snapshot_only_tracked_authority",
        }),
        true,
        None,
    );
    observe_phase(
        "mutation.coordination.publishedPlans.writeIndex",
        Duration::ZERO,
        json!({
            "entryCount": 0,
            "skipped": true,
            "reason": "snapshot_only_tracked_authority",
        }),
        true,
        None,
    );
    observe_phase(
        "mutation.coordination.publishedPlans.cleanupLogs",
        Duration::ZERO,
        json!({
            "expectedLogCount": 0,
            "skipped": true,
            "reason": "snapshot_only_tracked_authority",
        }),
        true,
        None,
    );
    observe_published_plan_step(
        &mut observe_phase,
        "mutation.coordination.publishedPlans.syncSharedCoordinationRef",
        |_| json!({}),
        || sync_shared_coordination_ref_state(root, snapshot, &graphs, &overlays_by_plan, publish),
    )?;
    observe_published_plan_step(
        &mut observe_phase,
        "mutation.coordination.publishedPlans.syncTrackedSnapshot",
        |_| json!({}),
        || sync_coordination_snapshot_state(root, snapshot, &graphs, &overlays_by_plan, publish),
    )
}

pub(crate) fn load_hydrated_coordination_snapshot(
    root: &Path,
    snapshot: Option<CoordinationSnapshot>,
) -> Result<Option<CoordinationSnapshot>> {
    if let Some(shared) = load_shared_coordination_ref_state(root)? {
        return Ok(match snapshot {
            Some(snapshot) => Some(merge_shared_coordination_into_snapshot(
                snapshot,
                shared.snapshot,
            )),
            None => Some(shared.snapshot),
        });
    }
    if let Some(tracked) = load_tracked_coordination_snapshot_state(root)? {
        return Ok(match snapshot {
            Some(snapshot) => Some(merge_published_plans_into_snapshot(
                snapshot,
                tracked.snapshot,
            )),
            None => Some(tracked.snapshot),
        });
    }

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
    if let Some(mut shared) = load_shared_coordination_ref_state(root)? {
        return Ok(match snapshot {
            Some(snapshot) => {
                merge_snapshot_bootstrap_into_plan_state(
                    &snapshot,
                    &mut shared.plan_graphs,
                    &mut shared.execution_overlays,
                );
                let snapshot = merge_shared_coordination_into_snapshot(snapshot, shared.snapshot);
                Some(HydratedCoordinationPlanState {
                    snapshot,
                    plan_graphs: shared.plan_graphs,
                    execution_overlays: shared.execution_overlays,
                })
            }
            None => Some(HydratedCoordinationPlanState {
                snapshot: shared.snapshot,
                plan_graphs: shared.plan_graphs,
                execution_overlays: shared.execution_overlays,
            }),
        });
    }
    if let Some(mut tracked) = load_tracked_coordination_snapshot_state(root)? {
        return Ok(match snapshot {
            Some(snapshot) => {
                merge_snapshot_bootstrap_into_plan_state(
                    &snapshot,
                    &mut tracked.plan_graphs,
                    &mut tracked.execution_overlays,
                );
                let snapshot = merge_published_plans_into_snapshot(snapshot, tracked.snapshot);
                Some(HydratedCoordinationPlanState {
                    snapshot,
                    plan_graphs: tracked.plan_graphs,
                    execution_overlays: tracked.execution_overlays,
                })
            }
            None => Some(HydratedCoordinationPlanState {
                snapshot: tracked.snapshot,
                plan_graphs: tracked.plan_graphs,
                execution_overlays: tracked.execution_overlays,
            }),
        });
    }

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

pub(crate) fn merge_snapshot_bootstrap_into_plan_state(
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

pub(crate) fn merge_shared_coordination_into_snapshot(
    mut snapshot: CoordinationSnapshot,
    shared_snapshot: CoordinationSnapshot,
) -> CoordinationSnapshot {
    let shared_plan_ids = shared_snapshot
        .plans
        .iter()
        .map(|plan| plan.id.0.to_string())
        .collect::<BTreeSet<_>>();
    let shared_task_ids = shared_snapshot
        .tasks
        .iter()
        .map(|task| task.id.0.to_string())
        .collect::<BTreeSet<_>>();
    let shared_artifact_ids = shared_snapshot
        .artifacts
        .iter()
        .map(|artifact| artifact.id.0.to_string())
        .collect::<BTreeSet<_>>();
    let shared_claim_ids = shared_snapshot
        .claims
        .iter()
        .map(|claim| claim.id.0.to_string())
        .collect::<BTreeSet<_>>();
    let shared_review_ids = shared_snapshot
        .reviews
        .iter()
        .map(|review| review.id.0.to_string())
        .collect::<BTreeSet<_>>();
    snapshot
        .plans
        .retain(|plan| !shared_plan_ids.contains(plan.id.0.as_str()));
    snapshot
        .tasks
        .retain(|task| !shared_task_ids.contains(task.id.0.as_str()));
    snapshot
        .artifacts
        .retain(|artifact| !shared_artifact_ids.contains(artifact.id.0.as_str()));
    snapshot
        .claims
        .retain(|claim| !shared_claim_ids.contains(claim.id.0.as_str()));
    snapshot
        .reviews
        .retain(|review| !shared_review_ids.contains(review.id.0.as_str()));
    snapshot.plans.extend(shared_snapshot.plans);
    snapshot.tasks.extend(shared_snapshot.tasks);
    snapshot.artifacts.extend(shared_snapshot.artifacts);
    snapshot.claims.extend(shared_snapshot.claims);
    snapshot.reviews.extend(shared_snapshot.reviews);
    snapshot
        .plans
        .sort_by(|left, right| left.id.0.cmp(&right.id.0));
    snapshot
        .tasks
        .sort_by(|left, right| left.id.0.cmp(&right.id.0));
    snapshot
        .artifacts
        .sort_by(|left, right| left.id.0.cmp(&right.id.0));
    snapshot
        .claims
        .sort_by(|left, right| left.id.0.cmp(&right.id.0));
    snapshot
        .reviews
        .sort_by(|left, right| left.id.0.cmp(&right.id.0));
    snapshot.next_plan = snapshot.next_plan.max(shared_snapshot.next_plan);
    snapshot.next_task = snapshot.next_task.max(shared_snapshot.next_task);
    snapshot.next_claim = snapshot.next_claim.max(shared_snapshot.next_claim);
    snapshot.next_artifact = snapshot.next_artifact.max(shared_snapshot.next_artifact);
    snapshot.next_review = snapshot.next_review.max(shared_snapshot.next_review);
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
            if execution.pending_handoff_to.is_some()
                || execution.session.is_some()
                || execution.git_execution.is_some()
            {
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
                        git_execution: None,
                    },
                );
            } else {
                overlays.remove(node_id.0.as_str());
            }
        }
    }
}

fn repo_published_execution_overlays(
    _overlays: Vec<PlanExecutionOverlay>,
) -> Vec<PlanExecutionOverlay> {
    // Repo-published plan artifacts must remain repo-semantic. Live execution bookkeeping like
    // git preflight/publish state stays authoritative-only so task publication can be finalized
    // without creating a follow-up `.prism` commit loop.
    Vec::new()
}

pub(crate) fn execution_overlays_by_plan(
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

fn legacy_header_from_plan(plan: &Plan) -> PublishedPlanHeader {
    PublishedPlanHeader {
        id: plan.id.clone(),
        scope: prism_ir::PlanScope::Repo,
        kind: prism_ir::PlanKind::TaskExecution,
        title: plan.title.clone(),
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

fn scan_or_repair_repo_published_plan_artifacts(
    root: &Path,
    apply_repairs: bool,
) -> Result<PublishedPlanArtifactRepairReport> {
    let plans_dir = repo_plans_dir(root);
    let streams_dir = plans_dir.join("streams");
    if !streams_dir.exists() {
        return Ok(PublishedPlanArtifactRepairReport {
            scanned_plan_count: 0,
            repaired_plan_count: 0,
            redundant_edge_add_count: 0,
            entries: Vec::new(),
        });
    }

    let mut entries = Vec::new();
    let principal = implicit_principal_identity(None, None);
    let mut repaired_plan_count = 0usize;
    let mut redundant_edge_add_count = 0usize;

    for entry in fs::read_dir(&streams_dir)? {
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

        let stored_events = load_stored_plan_events(root, &path)?;
        let Some(plan_id) = stored_events.first().map(stored_plan_event_plan_id) else {
            continue;
        };
        let inspection = inspect_plan_stream_for_repair(&stored_events, &path)?;
        if apply_repairs
            && inspection.redundant_edge_add_count > 0
            && !inspection.skipped_legacy_stream
        {
            rewrite_protected_stream_events(
                root,
                &stream,
                inspection.rewritten_events.clone(),
                &principal,
            )?;
            repaired_plan_count += 1;
        }
        redundant_edge_add_count += inspection.redundant_edge_add_count;
        entries.push(PublishedPlanArtifactRepairEntry {
            plan_id,
            protected_path: normalize_relative_path(relative),
            event_count_before: inspection.event_count_before,
            event_count_after: inspection.event_count_after,
            redundant_edge_add_count: inspection.redundant_edge_add_count,
            repaired: apply_repairs
                && inspection.redundant_edge_add_count > 0
                && !inspection.skipped_legacy_stream,
            skipped_legacy_stream: inspection.skipped_legacy_stream,
        });
    }

    entries.sort_by(|left, right| left.plan_id.0.cmp(&right.plan_id.0));
    if apply_repairs {
        regenerate_repo_published_plan_artifacts(root)?;
    }
    Ok(PublishedPlanArtifactRepairReport {
        scanned_plan_count: entries.len(),
        repaired_plan_count,
        redundant_edge_add_count,
        entries,
    })
}

#[derive(Debug, Clone)]
struct PlanStreamRepairInspection {
    event_count_before: usize,
    event_count_after: usize,
    redundant_edge_add_count: usize,
    skipped_legacy_stream: bool,
    rewritten_events: Vec<(String, PublishedPlanEvent)>,
}

fn inspect_plan_stream_for_repair(
    stored_events: &[StoredPublishedPlanEvent],
    path: &Path,
) -> Result<PlanStreamRepairInspection> {
    let mut edge_state = BTreeMap::<String, PlanEdge>::new();
    let mut rewritten_events = Vec::new();
    let mut redundant_edge_add_count = 0usize;
    let mut skipped_legacy_stream = false;

    for stored_event in stored_events {
        let StoredPublishedPlanEvent::Native(event) = stored_event else {
            skipped_legacy_stream = true;
            continue;
        };

        let redundant_edge_add = matches!(
            (&event.kind, &event.payload),
            (PublishedPlanEventKind::EdgeAdded, PublishedPlanPayload::Edge { edge })
                if edge_state.get(edge.id.0.as_str()) == Some(edge)
        );
        if redundant_edge_add {
            redundant_edge_add_count += 1;
            continue;
        }

        apply_edge_repair_state(event, &mut edge_state);
        rewritten_events.push((event.event_id.clone(), event.clone()));
    }

    if skipped_legacy_stream && redundant_edge_add_count > 0 {
        return Err(anyhow!(
            "refused to repair mixed legacy/native plan stream {} with redundant edge additions",
            path.display()
        ));
    }

    Ok(PlanStreamRepairInspection {
        event_count_before: stored_events.len(),
        event_count_after: if skipped_legacy_stream {
            stored_events.len()
        } else {
            rewritten_events.len()
        },
        redundant_edge_add_count,
        skipped_legacy_stream,
        rewritten_events,
    })
}

fn stored_plan_event_plan_id(event: &StoredPublishedPlanEvent) -> PlanId {
    match event {
        StoredPublishedPlanEvent::Native(event) => event.plan_id.clone(),
        StoredPublishedPlanEvent::Legacy(event) => event.plan_id.clone(),
    }
}

fn apply_edge_repair_state(
    event: &PublishedPlanEvent,
    edge_state: &mut BTreeMap<String, PlanEdge>,
) {
    match (&event.kind, &event.payload) {
        (PublishedPlanEventKind::EdgeAdded, PublishedPlanPayload::Edge { edge }) => {
            edge_state.insert(edge.id.0.to_string(), edge.clone());
        }
        (PublishedPlanEventKind::EdgeRemoved, _) => {
            if let Some(edge_id) = &event.edge_id {
                edge_state.remove(edge_id.0.as_str());
            }
        }
        (PublishedPlanEventKind::NodeRemoved, _) => {
            if let Some(node_id) = &event.node_id {
                edge_state.retain(|_, edge| edge.from != *node_id && edge.to != *node_id);
            }
        }
        _ => {}
    }
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
