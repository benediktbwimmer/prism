use std::path::Path;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex, RwLock};

use anyhow::Result;
use prism_ir::WorkspaceRevision;
use prism_store::SqliteStore;
use tracing::info;

use crate::checkpoint_materializer::{persist_coordination_materialization, CoordinationMaterialization};
use crate::coordination_authority_api::{
    poll_coordination_authority_live_sync, CoordinationAuthorityLiveSync,
};
use crate::session::WorkspaceSession;
use crate::workspace_identity::coordination_persist_context_for_root;
use crate::workspace_runtime_state::{WorkspacePublishedGeneration, WorkspaceRuntimeState};

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
    let mut next_state = runtime_state
        .lock()
        .expect("workspace runtime state lock poisoned")
        .clone();
    next_state.replace_coordination_runtime(
        shared.snapshot.clone(),
        shared.runtime_descriptors.clone(),
    );
    let next = next_state.publish_generation(
        WorkspaceRevision {
            graph_version: local_workspace_revision,
            git_commit: None,
        },
        Some(coordination_persist_context_for_root(root, None)),
    );
    WorkspaceSession::attach_cold_query_backends(next.prism_arc().as_ref(), cold_query_store);
    *runtime_state
        .lock()
        .expect("workspace runtime state lock poisoned") = next_state;
    *published_generation
        .write()
        .expect("workspace published generation lock poisoned") = next;
    loaded_workspace_revision.store(local_workspace_revision, Ordering::Relaxed);
    let next_coordination_revision = coordination_runtime_revision
        .load(Ordering::Relaxed)
        .max(persisted_coordination_revision)
        .saturating_add(1);
    {
        let mut store = store.lock().expect("workspace store lock poisoned");
        persist_coordination_materialization(
            root,
            &mut *store,
            &CoordinationMaterialization {
                authoritative_revision: next_coordination_revision,
                snapshot: shared.snapshot.clone(),
                canonical_snapshot_v2: Some(shared.canonical_snapshot_v2.clone()),
                runtime_descriptors: Some(shared.runtime_descriptors.clone()),
                publish_context: None,
            },
        )?;
    }
    coordination_runtime_revision.store(next_coordination_revision, Ordering::Relaxed);
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
