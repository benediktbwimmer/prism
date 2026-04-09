use std::path::Path;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex, RwLock};

use anyhow::Result;
use prism_ir::WorkspaceRevision;
use prism_store::{CoordinationPersistContext, SqliteStore};
use tracing::info;

use crate::checkpoint_materializer::{
    persist_coordination_materialization, CheckpointMaterializerHandle, CoordinationMaterialization,
};
use crate::coordination_authority_api::{
    poll_coordination_authority_live_sync, CoordinationAuthorityLiveSync,
};
use crate::coordination_authority_store::coordination_materialization_enabled_for_root;
use crate::session::WorkspaceSession;
use crate::tracked_snapshot::TrackedSnapshotPublishContext;
use crate::workspace_identity::coordination_persist_context_for_root;
use crate::workspace_runtime_state::{WorkspacePublishedGeneration, WorkspaceRuntimeState};

pub(crate) fn publish_service_backed_coordination_runtime_state(
    published_generation: &Arc<RwLock<WorkspacePublishedGeneration>>,
    runtime_state: &Arc<Mutex<WorkspaceRuntimeState>>,
    cold_query_store: &Arc<Mutex<SqliteStore>>,
    loaded_workspace_revision: &Arc<AtomicU64>,
    local_workspace_revision: u64,
    workspace_revision: u64,
    coordination_context: Option<CoordinationPersistContext>,
    current_state: &crate::CoordinationCurrentState,
) {
    let mut next_state = runtime_state
        .lock()
        .expect("workspace runtime state lock poisoned")
        .clone();
    next_state.replace_coordination_runtime(
        current_state.snapshot.clone(),
        current_state.runtime_descriptors.clone(),
    );
    let next = next_state.publish_generation(
        WorkspaceRevision {
            graph_version: local_workspace_revision,
            git_commit: None,
        },
        coordination_context,
    );
    WorkspaceSession::attach_cold_query_backends(next.prism_arc().as_ref(), cold_query_store);
    *runtime_state
        .lock()
        .expect("workspace runtime state lock poisoned") = next_state;
    *published_generation
        .write()
        .expect("workspace published generation lock poisoned") = next;
    loaded_workspace_revision.store(workspace_revision, Ordering::Relaxed);
}

pub(crate) fn apply_service_backed_coordination_current_state(
    root: &Path,
    published_generation: &Arc<RwLock<WorkspacePublishedGeneration>>,
    runtime_state: &Arc<Mutex<WorkspaceRuntimeState>>,
    store: &Arc<Mutex<SqliteStore>>,
    cold_query_store: &Arc<Mutex<SqliteStore>>,
    loaded_workspace_revision: &Arc<AtomicU64>,
    coordination_runtime_revision: Option<&Arc<AtomicU64>>,
    checkpoint_materializer: Option<&CheckpointMaterializerHandle>,
    local_workspace_revision: u64,
    workspace_revision: u64,
    coordination_context: Option<CoordinationPersistContext>,
    current_state: &crate::CoordinationCurrentState,
    authoritative_revision: u64,
    publish_context: Option<TrackedSnapshotPublishContext>,
) -> Result<()> {
    publish_service_backed_coordination_runtime_state(
        published_generation,
        runtime_state,
        cold_query_store,
        loaded_workspace_revision,
        local_workspace_revision,
        workspace_revision,
        coordination_context,
        current_state,
    );

    if coordination_materialization_enabled_for_root(root)? {
        let materialization = CoordinationMaterialization {
            authoritative_revision,
            snapshot: current_state.snapshot.clone(),
            canonical_snapshot_v2: Some(current_state.canonical_snapshot_v2.clone()),
            runtime_descriptors: Some(current_state.runtime_descriptors.clone()),
            publish_context,
        };
        if let Some(materializer) = checkpoint_materializer {
            materializer.enqueue_coordination_materialization(materialization)?;
        } else {
            let mut store = store.lock().expect("workspace store lock poisoned");
            persist_coordination_materialization(root, &mut *store, &materialization)?;
        }
    }
    if let Some(coordination_runtime_revision) = coordination_runtime_revision {
        coordination_runtime_revision.store(
            coordination_runtime_revision
                .load(Ordering::Relaxed)
                .max(authoritative_revision),
            Ordering::Relaxed,
        );
    }
    Ok(())
}

pub(crate) fn sync_coordination_authority_update(
    root: &Path,
    published_generation: &Arc<RwLock<WorkspacePublishedGeneration>>,
    runtime_state: &Arc<Mutex<WorkspaceRuntimeState>>,
    store: &Arc<Mutex<SqliteStore>>,
    cold_query_store: &Arc<Mutex<SqliteStore>>,
    refresh_lock: &Arc<Mutex<()>>,
    loaded_workspace_revision: &Arc<AtomicU64>,
    coordination_runtime_revision: &Arc<AtomicU64>,
    coordination_enabled: bool,
) -> Result<()> {
    if !coordination_enabled {
        return Ok(());
    }
    let CoordinationAuthorityLiveSync::Changed(shared) =
        poll_coordination_authority_live_sync(root)?
    else {
        return Ok(());
    };

    apply_coordination_authority_current_state(
        root,
        published_generation,
        runtime_state,
        store,
        cold_query_store,
        refresh_lock,
        loaded_workspace_revision,
        coordination_runtime_revision,
        &shared,
    )
}

pub(crate) fn apply_coordination_authority_current_state(
    root: &Path,
    published_generation: &Arc<RwLock<WorkspacePublishedGeneration>>,
    runtime_state: &Arc<Mutex<WorkspaceRuntimeState>>,
    store: &Arc<Mutex<SqliteStore>>,
    cold_query_store: &Arc<Mutex<SqliteStore>>,
    refresh_lock: &Arc<Mutex<()>>,
    loaded_workspace_revision: &Arc<AtomicU64>,
    coordination_runtime_revision: &Arc<AtomicU64>,
    shared: &crate::CoordinationCurrentState,
) -> Result<()> {
    let _guard = refresh_lock
        .lock()
        .expect("shared coordination ref refresh lock poisoned");

    let local_workspace_revision = store
        .lock()
        .expect("workspace store lock poisoned")
        .workspace_revision()?;
    let persisted_coordination_revision = store
        .lock()
        .expect("workspace store lock poisoned")
        .coordination_revision()?;
    let next_coordination_revision = coordination_runtime_revision
        .load(Ordering::Relaxed)
        .max(persisted_coordination_revision)
        .saturating_add(1);
    apply_service_backed_coordination_current_state(
        root,
        published_generation,
        runtime_state,
        store,
        cold_query_store,
        loaded_workspace_revision,
        Some(coordination_runtime_revision),
        None,
        local_workspace_revision,
        local_workspace_revision,
        Some(coordination_persist_context_for_root(root, None)),
        shared,
        next_coordination_revision,
        None,
    )?;
    info!(
        root = %root.display(),
        plan_count = shared.snapshot.plans.len(),
        task_count = shared.snapshot.tasks.len(),
        claim_count = shared.snapshot.claims.len(),
        artifact_count = shared.snapshot.artifacts.len(),
        review_count = shared.snapshot.reviews.len(),
        "applied coordination authority live sync"
    );
    Ok(())
}
