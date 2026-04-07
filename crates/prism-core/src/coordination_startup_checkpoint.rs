use std::collections::BTreeMap;
use std::path::Path;

use anyhow::Result;
use prism_coordination::{
    migrate_legacy_hybrid_snapshot_to_canonical_v2, snapshot_plan_graphs, CoordinationSnapshot,
    CoordinationSnapshotV2,
};
use prism_ir::{PlanEdgeKind, PlanExecutionOverlay, PlanGraph, PlanNodeId};
use prism_store::{
    CoordinationCheckpointStore, CoordinationJournal, CoordinationStartupCheckpoint,
    CoordinationStartupCheckpointAuthority,
};

use crate::coordination_snapshot_sanitization::sanitize_persisted_coordination_snapshot;
use crate::published_plans::{
    execution_overlays_by_plan, merge_shared_coordination_into_snapshot,
    merge_snapshot_bootstrap_into_plan_state, HydratedCoordinationPlanState,
};
use crate::shared_coordination_ref::shared_coordination_startup_authority;
use crate::util::current_timestamp;

pub(crate) fn load_materialized_coordination_plan_state<S>(
    root: &Path,
    store: &mut S,
    snapshot: Option<CoordinationSnapshot>,
) -> Result<Option<HydratedCoordinationPlanState>>
where
    S: CoordinationCheckpointStore + CoordinationJournal + ?Sized,
{
    let Some(checkpoint) = load_matching_coordination_startup_checkpoint(root, store)? else {
        return Ok(None);
    };
    Ok(match snapshot {
        Some(snapshot) => {
            let (mut plan_graphs, mut execution_overlays) = compatibility_plan_state(
                &checkpoint.plan_graphs,
                &checkpoint.execution_overlays,
                &checkpoint.snapshot,
            );
            let snapshot = merge_shared_coordination_into_snapshot(checkpoint.snapshot, snapshot);
            merge_snapshot_bootstrap_into_plan_state(
                &snapshot,
                &mut plan_graphs,
                &mut execution_overlays,
            );
            Some(HydratedCoordinationPlanState {
                canonical_snapshot_v2: migrate_legacy_hybrid_snapshot_to_canonical_v2(
                    &snapshot,
                    &plan_graphs,
                    &execution_overlays,
                )?,
                snapshot,
                plan_graphs,
                execution_overlays,
                // Runtime descriptors must come from live shared-coordination refs,
                // never from a local startup checkpoint payload.
                runtime_descriptors: Vec::new(),
            })
        }
        None => {
            let (plan_graphs, execution_overlays) = compatibility_plan_state(
                &checkpoint.plan_graphs,
                &checkpoint.execution_overlays,
                &checkpoint.snapshot,
            );
            let snapshot = checkpoint.snapshot;
            Some(HydratedCoordinationPlanState {
                canonical_snapshot_v2: checkpoint.canonical_snapshot_v2.unwrap_or(
                    migrate_legacy_hybrid_snapshot_to_canonical_v2(
                        &snapshot,
                        &plan_graphs,
                        &execution_overlays,
                    )?,
                ),
                snapshot,
                plan_graphs,
                execution_overlays,
                runtime_descriptors: Vec::new(),
            })
        }
    })
}

pub(crate) fn load_materialized_coordination_snapshot<S>(
    root: &Path,
    store: &mut S,
    snapshot: Option<CoordinationSnapshot>,
) -> Result<Option<CoordinationSnapshot>>
where
    S: CoordinationCheckpointStore + CoordinationJournal + ?Sized,
{
    let Some(checkpoint) = load_matching_coordination_startup_checkpoint(root, store)? else {
        return Ok(None);
    };
    Ok(match snapshot {
        Some(snapshot) => Some(merge_shared_coordination_into_snapshot(
            checkpoint.snapshot,
            snapshot,
        )),
        None => Some(checkpoint.snapshot),
    })
}

pub(crate) fn load_materialized_coordination_snapshot_v2<S>(
    root: &Path,
    store: &mut S,
    snapshot: Option<CoordinationSnapshot>,
) -> Result<Option<CoordinationSnapshotV2>>
where
    S: CoordinationCheckpointStore + CoordinationJournal + ?Sized,
{
    let Some(checkpoint) = load_matching_coordination_startup_checkpoint(root, store)? else {
        return Ok(None);
    };
    Ok(match snapshot {
        Some(snapshot) => {
            let (plan_graphs, execution_overlays) = compatibility_plan_state(
                &checkpoint.plan_graphs,
                &checkpoint.execution_overlays,
                &checkpoint.snapshot,
            );
            let snapshot = merge_shared_coordination_into_snapshot(checkpoint.snapshot, snapshot);
            Some(migrate_legacy_hybrid_snapshot_to_canonical_v2(
                &snapshot,
                &plan_graphs,
                &execution_overlays,
            )?)
        }
        None => {
            let (plan_graphs, execution_overlays) = compatibility_plan_state(
                &checkpoint.plan_graphs,
                &checkpoint.execution_overlays,
                &checkpoint.snapshot,
            );
            Some(checkpoint.canonical_snapshot_v2.unwrap_or(
                migrate_legacy_hybrid_snapshot_to_canonical_v2(
                    &checkpoint.snapshot,
                    &plan_graphs,
                    &execution_overlays,
                )?,
            ))
        }
    })
}

pub(crate) fn save_shared_coordination_startup_checkpoint<S>(
    root: &Path,
    store: &mut S,
    snapshot: &CoordinationSnapshot,
    plan_graphs: &[PlanGraph],
    execution_overlays: &BTreeMap<String, Vec<PlanExecutionOverlay>>,
) -> Result<()>
where
    S: CoordinationCheckpointStore + CoordinationJournal + ?Sized,
{
    let authority = coordination_startup_authority(root)?;
    let mut checkpoint_snapshot = sanitize_persisted_coordination_snapshot(
        snapshot_with_compat_authored_plan_graph_state(snapshot.clone(), plan_graphs),
    );
    checkpoint_snapshot.events.clear();
    store.save_coordination_startup_checkpoint(&CoordinationStartupCheckpoint {
        version: CoordinationStartupCheckpoint::VERSION,
        materialized_at: current_timestamp(),
        coordination_revision: store.coordination_revision()?,
        authority,
        snapshot: checkpoint_snapshot.clone(),
        canonical_snapshot_v2: Some(migrate_legacy_hybrid_snapshot_to_canonical_v2(
            &checkpoint_snapshot,
            plan_graphs,
            execution_overlays,
        )?),
        plan_graphs: Vec::new(),
        execution_overlays: BTreeMap::new(),
        runtime_descriptors: Vec::new(),
    })
}

fn snapshot_with_compat_authored_plan_graph_state(
    mut snapshot: CoordinationSnapshot,
    plan_graphs: &[PlanGraph],
) -> CoordinationSnapshot {
    let graphs_by_plan = plan_graphs
        .iter()
        .map(|graph| (graph.id.0.to_string(), graph))
        .collect::<BTreeMap<_, _>>();
    for plan in &mut snapshot.plans {
        let Some(graph) = graphs_by_plan.get(plan.id.0.as_str()) else {
            continue;
        };
        plan.authored_nodes = graph
            .nodes
            .iter()
            .filter(|node| !is_task_backed_plan_node_id(&node.id))
            .cloned()
            .collect();
        plan.authored_edges = graph
            .edges
            .iter()
            .filter(|edge| {
                edge.kind != PlanEdgeKind::DependsOn
                    || !is_task_backed_plan_node_id(&edge.from)
                    || !is_task_backed_plan_node_id(&edge.to)
            })
            .cloned()
            .collect();
        plan.root_tasks = graph
            .root_nodes
            .iter()
            .filter(|node_id| is_task_backed_plan_node_id(node_id))
            .cloned()
            .map(coordination_task_id_from_plan_node_id)
            .collect();
    }
    snapshot
}

fn is_task_backed_plan_node_id(node_id: &PlanNodeId) -> bool {
    node_id.0.starts_with("coord-task:")
}

fn coordination_task_id_from_plan_node_id(node_id: PlanNodeId) -> prism_ir::CoordinationTaskId {
    prism_ir::CoordinationTaskId::new(node_id.0)
}

fn compatibility_plan_state(
    plan_graphs: &[PlanGraph],
    execution_overlays: &BTreeMap<String, Vec<PlanExecutionOverlay>>,
    snapshot: &CoordinationSnapshot,
) -> (Vec<PlanGraph>, BTreeMap<String, Vec<PlanExecutionOverlay>>) {
    if plan_graphs.is_empty() && execution_overlays.is_empty() {
        (
            snapshot_plan_graphs(snapshot),
            execution_overlays_by_plan(&snapshot.tasks),
        )
    } else {
        (plan_graphs.to_vec(), execution_overlays.clone())
    }
}

fn load_matching_coordination_startup_checkpoint<S>(
    root: &Path,
    store: &mut S,
) -> Result<Option<CoordinationStartupCheckpoint>>
where
    S: CoordinationCheckpointStore + CoordinationJournal + ?Sized,
{
    let Some(checkpoint) = store.load_coordination_startup_checkpoint()? else {
        return Ok(None);
    };
    if store.coordination_revision()? > checkpoint.coordination_revision {
        return Ok(None);
    }
    let authority = coordination_startup_authority(root)?;
    if checkpoint.authority.ref_name != authority.ref_name {
        return Ok(None);
    }
    if checkpoint.authority.head_commit.is_some()
        && checkpoint.authority.head_commit != authority.head_commit
    {
        return Ok(None);
    }
    if checkpoint.authority.manifest_digest.is_some()
        && authority.manifest_digest.is_some()
        && checkpoint.authority.manifest_digest != authority.manifest_digest
    {
        return Ok(None);
    }
    Ok(Some(checkpoint))
}

pub(crate) fn coordination_startup_authority(
    root: &Path,
) -> Result<CoordinationStartupCheckpointAuthority> {
    Ok(
        shared_coordination_startup_authority(root)?.unwrap_or_else(|| {
            CoordinationStartupCheckpointAuthority {
                ref_name: "local-worktree".to_string(),
                head_commit: None,
                manifest_digest: None,
            }
        }),
    )
}
