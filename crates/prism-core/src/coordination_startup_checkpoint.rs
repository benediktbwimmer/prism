use std::collections::BTreeMap;
use std::path::Path;

use anyhow::Result;
use prism_coordination::{
    migrate_legacy_hybrid_snapshot_to_canonical_v2, CoordinationSnapshot, CoordinationSnapshotV2,
};
use prism_ir::{PlanExecutionOverlay, PlanGraph};
use prism_store::{
    CoordinationCheckpointStore, CoordinationJournal, CoordinationStartupCheckpoint,
    CoordinationStartupCheckpointAuthority,
};

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
            let mut plan_graphs = checkpoint.plan_graphs;
            let mut execution_overlays = checkpoint.execution_overlays;
            let runtime_descriptors = checkpoint.runtime_descriptors;
            merge_snapshot_bootstrap_into_plan_state(
                &snapshot,
                &mut plan_graphs,
                &mut execution_overlays,
            );
            let snapshot = merge_shared_coordination_into_snapshot(checkpoint.snapshot, snapshot);
            Some(HydratedCoordinationPlanState {
                canonical_snapshot_v2: migrate_legacy_hybrid_snapshot_to_canonical_v2(
                    &snapshot,
                    &plan_graphs,
                    &execution_overlays,
                )?,
                snapshot,
                plan_graphs,
                execution_overlays,
                runtime_descriptors,
            })
        }
        None => {
            let snapshot = checkpoint.snapshot;
            Some(HydratedCoordinationPlanState {
                canonical_snapshot_v2: checkpoint.canonical_snapshot_v2.unwrap_or(
                    migrate_legacy_hybrid_snapshot_to_canonical_v2(
                        &snapshot,
                        &checkpoint.plan_graphs,
                        &checkpoint.execution_overlays,
                    )?,
                ),
                snapshot,
                plan_graphs: checkpoint.plan_graphs,
                execution_overlays: checkpoint.execution_overlays,
                runtime_descriptors: checkpoint.runtime_descriptors,
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
            let snapshot = merge_shared_coordination_into_snapshot(checkpoint.snapshot, snapshot);
            Some(migrate_legacy_hybrid_snapshot_to_canonical_v2(
                &snapshot,
                &checkpoint.plan_graphs,
                &checkpoint.execution_overlays,
            )?)
        }
        None => Some(checkpoint.canonical_snapshot_v2.unwrap_or(
            migrate_legacy_hybrid_snapshot_to_canonical_v2(
                &checkpoint.snapshot,
                &checkpoint.plan_graphs,
                &checkpoint.execution_overlays,
            )?,
        )),
    })
}

pub(crate) fn save_shared_coordination_startup_checkpoint<S>(
    root: &Path,
    store: &mut S,
    snapshot: &CoordinationSnapshot,
    plan_graphs: &[PlanGraph],
    execution_overlays: &BTreeMap<String, Vec<PlanExecutionOverlay>>,
    runtime_descriptors: &[prism_coordination::RuntimeDescriptor],
) -> Result<()>
where
    S: CoordinationCheckpointStore + CoordinationJournal + ?Sized,
{
    let authority = coordination_startup_authority(root)?;
    let mut checkpoint_snapshot = snapshot.clone();
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
        plan_graphs: plan_graphs.to_vec(),
        execution_overlays: execution_overlays.clone(),
        runtime_descriptors: runtime_descriptors.to_vec(),
    })
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
    let checkpoint = normalize_coordination_startup_checkpoint(checkpoint);
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

fn normalize_coordination_startup_checkpoint(
    mut checkpoint: CoordinationStartupCheckpoint,
) -> CoordinationStartupCheckpoint {
    if checkpoint.plan_graphs.is_empty() {
        checkpoint.plan_graphs = prism_coordination::snapshot_plan_graphs(&checkpoint.snapshot);
    }
    if checkpoint.execution_overlays.is_empty() {
        checkpoint.execution_overlays = execution_overlays_by_plan(&checkpoint.snapshot.tasks);
    }
    checkpoint
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

#[cfg(test)]
mod tests {
    use super::normalize_coordination_startup_checkpoint;
    use crate::published_plans::execution_overlays_by_plan;
    use prism_coordination::{CoordinationPolicy, CoordinationSnapshot, Plan, PlanScheduling};
    use prism_ir::{
        CoordinationTaskId, CoordinationTaskStatus, PlanId, PlanKind, PlanScope, PlanStatus,
        SessionId, WorkspaceRevision,
    };
    use prism_store::{CoordinationStartupCheckpoint, CoordinationStartupCheckpointAuthority};

    #[test]
    fn legacy_checkpoint_without_plan_graphs_rebuilds_derived_plan_state() {
        let plan_id = PlanId::new("plan:legacy-checkpoint");
        let task_id = CoordinationTaskId::new("coord-task:legacy-checkpoint");
        let snapshot = CoordinationSnapshot {
            plans: vec![Plan {
                id: plan_id.clone(),
                goal: "ship".to_string(),
                title: "ship".to_string(),
                status: PlanStatus::Active,
                policy: CoordinationPolicy::default(),
                scope: PlanScope::Repo,
                kind: PlanKind::TaskExecution,
                revision: 1,
                scheduling: PlanScheduling::default(),
                tags: Vec::new(),
                created_from: None,
                metadata: serde_json::Value::Null,
                authored_nodes: Vec::new(),
                authored_edges: Vec::new(),
                root_tasks: vec![task_id.clone()],
            }],
            tasks: vec![prism_coordination::CoordinationTask {
                id: task_id,
                plan: plan_id.clone(),
                kind: prism_ir::PlanNodeKind::Edit,
                title: "ship it".to_string(),
                summary: None,
                status: CoordinationTaskStatus::InProgress,
                published_task_status: None,
                assignee: None,
                pending_handoff_to: None,
                session: Some(SessionId::new("session:test".to_string())),
                lease_holder: None,
                lease_started_at: None,
                lease_refreshed_at: None,
                lease_stale_at: None,
                lease_expires_at: None,
                worktree_id: None,
                branch_ref: None,
                anchors: Vec::new(),
                bindings: prism_ir::PlanBinding::default(),
                depends_on: Vec::new(),
                coordination_depends_on: Vec::new(),
                integrated_depends_on: Vec::new(),
                acceptance: Vec::new(),
                validation_refs: Vec::new(),
                is_abstract: false,
                base_revision: WorkspaceRevision::default(),
                priority: None,
                tags: Vec::new(),
                metadata: serde_json::Value::Null,
                git_execution: prism_coordination::TaskGitExecution::default(),
            }],
            claims: Vec::new(),
            artifacts: Vec::new(),
            reviews: Vec::new(),
            events: Vec::new(),
            next_plan: 1,
            next_task: 1,
            next_claim: 0,
            next_artifact: 0,
            next_review: 0,
        };
        let expected_graph = prism_coordination::snapshot_plan_graphs(&snapshot);
        let expected_overlays = execution_overlays_by_plan(&snapshot.tasks);
        let checkpoint = CoordinationStartupCheckpoint {
            version: CoordinationStartupCheckpoint::VERSION,
            materialized_at: 1,
            coordination_revision: 1,
            authority: CoordinationStartupCheckpointAuthority {
                ref_name: "local-worktree".to_string(),
                head_commit: None,
                manifest_digest: None,
            },
            snapshot,
            canonical_snapshot_v2: None,
            plan_graphs: Vec::new(),
            execution_overlays: Default::default(),
            runtime_descriptors: Vec::new(),
        };

        let normalized = normalize_coordination_startup_checkpoint(checkpoint);

        assert_eq!(normalized.plan_graphs, expected_graph);
        assert_eq!(normalized.execution_overlays, expected_overlays);
    }
}
