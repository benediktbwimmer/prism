use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::mpsc;
use std::sync::{Arc, Mutex, MutexGuard, RwLock};
use std::thread;
use std::time::{Duration, Instant};

use anyhow::Result;
use notify::{recommended_watcher, Event, RecursiveMode, Watcher};
use prism_history::HistoryStore;
use prism_ir::ChangeTrigger;
use prism_memory::OutcomeMemory;
use prism_projections::ProjectionIndex;
use prism_store::{Graph, SqliteStore, WorkspaceTreeSnapshot};
use tracing::{error, warn};

use crate::checkpoint_materializer::CheckpointMaterializerHandle;
use crate::coordination_persistence::CoordinationPersistenceBackend;
use crate::curator::{enqueue_curator_for_observed_locked, CuratorHandleRef};
use crate::indexer::WorkspaceIndexer;
use crate::layout::discover_layout;
use crate::session::{
    published_plan_authority_fingerprint, WorkspaceRefreshResult, WorkspaceRefreshState,
    WorkspaceSession,
};
use crate::shared_runtime::composite_workspace_revision;
use crate::shared_runtime_backend::SharedRuntimeBackend;
use crate::shared_runtime_store::SharedRuntimeStore;
use crate::workspace_runtime_state::{WorkspacePublishedGeneration, WorkspaceRuntimeState};
use crate::workspace_tree::{
    diff_workspace_tree_snapshot, plan_full_refresh, plan_incremental_refresh,
    populate_package_regions, WorkspaceRefreshMode,
};

pub(crate) struct WatchHandle {
    pub(crate) stop: mpsc::Sender<WatchMessage>,
    pub(crate) handle: thread::JoinHandle<()>,
}

pub(crate) enum WatchMessage {
    Fs(notify::Result<Event>),
    Stop,
}

pub(crate) fn spawn_fs_watch(
    root: PathBuf,
    published_generation: Arc<RwLock<WorkspacePublishedGeneration>>,
    runtime_state: Arc<Mutex<WorkspaceRuntimeState>>,
    store: Arc<Mutex<SqliteStore>>,
    cold_query_store: Arc<Mutex<SqliteStore>>,
    shared_runtime_store: Option<Arc<Mutex<SharedRuntimeStore>>>,
    shared_runtime_sqlite: Option<PathBuf>,
    refresh_lock: Arc<Mutex<()>>,
    refresh_state: Arc<WorkspaceRefreshState>,
    loaded_workspace_revision: Arc<AtomicU64>,
    fs_snapshot: Arc<Mutex<WorkspaceTreeSnapshot>>,
    checkpoint_materializer: Option<CheckpointMaterializerHandle>,
    shared_runtime_materializer: Option<CheckpointMaterializerHandle>,
    coordination_enabled: bool,
    curator: Option<CuratorHandleRef>,
) -> Result<WatchHandle> {
    let (msg_tx, msg_rx) = mpsc::channel::<WatchMessage>();
    let (ready_tx, ready_rx) = mpsc::sync_channel::<bool>(1);
    let callback_tx = msg_tx.clone();

    let handle = thread::spawn(move || {
        let mut watcher = match recommended_watcher(move |event| {
            let _ = callback_tx.send(WatchMessage::Fs(event));
        }) {
            Ok(watcher) => watcher,
            Err(error) => {
                warn!(
                    root = %root.display(),
                    error = %error,
                    "failed to initialize prism fs watcher; continuing with fallback refresh checks"
                );
                let _ = ready_tx.send(false);
                return;
            }
        };

        if let Err(error) = watcher.watch(&root, RecursiveMode::Recursive) {
            warn!(
                root = %root.display(),
                error = %error,
                "failed to start prism fs watcher; continuing with fallback refresh checks"
            );
            let _ = ready_tx.send(false);
            return;
        }
        let _ = ready_tx.send(true);

        loop {
            let event = match msg_rx.recv() {
                Ok(WatchMessage::Fs(event)) => event,
                Ok(WatchMessage::Stop) | Err(mpsc::RecvError) => break,
            };

            let Ok(event) = event else {
                continue;
            };
            let mut dirty_paths = relevant_watch_paths(&root, &event);
            let mut published_plan_paths = relevant_published_plan_watch_paths(&root, &event);
            if dirty_paths.is_empty() && published_plan_paths.is_empty() {
                continue;
            }

            while let Ok(next) = msg_rx.recv_timeout(Duration::from_millis(75)) {
                match next {
                    WatchMessage::Fs(Ok(next)) => {
                        let next_paths = relevant_watch_paths(&root, &next);
                        if !next_paths.is_empty() {
                            dirty_paths.extend(next_paths);
                        }
                        let next_published_plan_paths =
                            relevant_published_plan_watch_paths(&root, &next);
                        if !next_published_plan_paths.is_empty() {
                            published_plan_paths.extend(next_published_plan_paths);
                        }
                    }
                    WatchMessage::Fs(Err(_)) => continue,
                    WatchMessage::Stop => return,
                };
            }

            if !dirty_paths.is_empty() {
                refresh_state.mark_fs_dirty_paths(dirty_paths.iter().cloned());

                if let Err(error) = refresh_prism_snapshot(
                    &root,
                    &published_generation,
                    &runtime_state,
                    &store,
                    &cold_query_store,
                    shared_runtime_store.as_ref(),
                    shared_runtime_sqlite.as_deref(),
                    &refresh_lock,
                    &refresh_state,
                    &loaded_workspace_revision,
                    &fs_snapshot,
                    checkpoint_materializer.clone(),
                    shared_runtime_materializer.clone(),
                    coordination_enabled,
                    curator.as_ref(),
                    ChangeTrigger::FsWatch,
                    None,
                    Some(dirty_paths),
                ) {
                    error!(
                        root = %root.display(),
                        error = %error,
                        error_chain = %format_error_chain(&error),
                        "prism fs watch refresh failed"
                    );
                }
            }

            if !published_plan_paths.is_empty() {
                if let Err(error) = sync_published_plan_authority_if_changed(
                    &root,
                    &published_generation,
                    &runtime_state,
                    &store,
                    &cold_query_store,
                    shared_runtime_store.as_ref(),
                    &refresh_lock,
                    &refresh_state,
                    &loaded_workspace_revision,
                    coordination_enabled,
                ) {
                    error!(
                        root = %root.display(),
                        error = %error,
                        error_chain = %format_error_chain(&error),
                        "published plan authority reload failed"
                    );
                }
            }
        }
    });

    let _ = ready_rx.try_recv();

    Ok(WatchHandle {
        stop: msg_tx,
        handle,
    })
}

pub(crate) fn refresh_prism_snapshot(
    root: &Path,
    published_generation: &Arc<RwLock<WorkspacePublishedGeneration>>,
    runtime_state: &Arc<Mutex<WorkspaceRuntimeState>>,
    store: &Arc<Mutex<SqliteStore>>,
    cold_query_store: &Arc<Mutex<SqliteStore>>,
    shared_runtime_store: Option<&Arc<Mutex<SharedRuntimeStore>>>,
    shared_runtime_sqlite: Option<&Path>,
    refresh_lock: &Arc<Mutex<()>>,
    refresh_state: &Arc<WorkspaceRefreshState>,
    loaded_workspace_revision: &Arc<AtomicU64>,
    fs_snapshot: &Arc<Mutex<WorkspaceTreeSnapshot>>,
    checkpoint_materializer: Option<CheckpointMaterializerHandle>,
    shared_runtime_materializer: Option<CheckpointMaterializerHandle>,
    coordination_enabled: bool,
    curator: Option<&CuratorHandleRef>,
    trigger: ChangeTrigger,
    known_fingerprint: Option<WorkspaceTreeSnapshot>,
    dirty_paths_override: Option<Vec<PathBuf>>,
) -> Result<WorkspaceRefreshResult> {
    let guard = refresh_lock
        .lock()
        .expect("workspace refresh lock poisoned");
    refresh_prism_snapshot_with_guard(
        root,
        published_generation,
        runtime_state,
        store,
        cold_query_store,
        shared_runtime_store,
        shared_runtime_sqlite,
        refresh_state,
        loaded_workspace_revision,
        fs_snapshot,
        checkpoint_materializer,
        shared_runtime_materializer,
        coordination_enabled,
        curator,
        trigger,
        known_fingerprint,
        dirty_paths_override,
        guard,
    )
}

pub(crate) fn try_refresh_prism_snapshot(
    root: &Path,
    published_generation: &Arc<RwLock<WorkspacePublishedGeneration>>,
    runtime_state: &Arc<Mutex<WorkspaceRuntimeState>>,
    store: &Arc<Mutex<SqliteStore>>,
    cold_query_store: &Arc<Mutex<SqliteStore>>,
    shared_runtime_store: Option<&Arc<Mutex<SharedRuntimeStore>>>,
    shared_runtime_sqlite: Option<&Path>,
    refresh_lock: &Arc<Mutex<()>>,
    refresh_state: &Arc<WorkspaceRefreshState>,
    loaded_workspace_revision: &Arc<AtomicU64>,
    fs_snapshot: &Arc<Mutex<WorkspaceTreeSnapshot>>,
    checkpoint_materializer: Option<CheckpointMaterializerHandle>,
    shared_runtime_materializer: Option<CheckpointMaterializerHandle>,
    coordination_enabled: bool,
    curator: Option<&CuratorHandleRef>,
    trigger: ChangeTrigger,
    known_fingerprint: Option<WorkspaceTreeSnapshot>,
    dirty_paths_override: Option<Vec<PathBuf>>,
) -> Result<Option<WorkspaceRefreshResult>> {
    let Ok(guard) = refresh_lock.try_lock() else {
        return Ok(None);
    };
    let observed = refresh_prism_snapshot_with_guard(
        root,
        published_generation,
        runtime_state,
        store,
        cold_query_store,
        shared_runtime_store,
        shared_runtime_sqlite,
        refresh_state,
        loaded_workspace_revision,
        fs_snapshot,
        checkpoint_materializer,
        shared_runtime_materializer,
        coordination_enabled,
        curator,
        trigger,
        known_fingerprint,
        dirty_paths_override,
        guard,
    )?;
    Ok(Some(observed))
}

pub(crate) fn sync_published_plan_authority_if_changed(
    root: &Path,
    published_generation: &Arc<RwLock<WorkspacePublishedGeneration>>,
    runtime_state: &Arc<Mutex<WorkspaceRuntimeState>>,
    store: &Arc<Mutex<SqliteStore>>,
    cold_query_store: &Arc<Mutex<SqliteStore>>,
    shared_runtime_store: Option<&Arc<Mutex<SharedRuntimeStore>>>,
    refresh_lock: &Arc<Mutex<()>>,
    refresh_state: &Arc<WorkspaceRefreshState>,
    loaded_workspace_revision: &Arc<AtomicU64>,
    coordination_enabled: bool,
) -> Result<bool> {
    let fingerprint = published_plan_authority_fingerprint(root)?;
    if !refresh_state.update_published_plan_fingerprint(fingerprint) {
        return Ok(false);
    }
    let guard = refresh_lock
        .lock()
        .expect("workspace refresh lock poisoned");
    sync_published_plan_authority_with_guard(
        root,
        published_generation,
        runtime_state,
        store,
        cold_query_store,
        shared_runtime_store,
        refresh_state,
        loaded_workspace_revision,
        coordination_enabled,
        guard,
    )
}

pub(crate) fn try_sync_published_plan_authority_if_changed(
    root: &Path,
    published_generation: &Arc<RwLock<WorkspacePublishedGeneration>>,
    runtime_state: &Arc<Mutex<WorkspaceRuntimeState>>,
    store: &Arc<Mutex<SqliteStore>>,
    cold_query_store: &Arc<Mutex<SqliteStore>>,
    shared_runtime_store: Option<&Arc<Mutex<SharedRuntimeStore>>>,
    refresh_lock: &Arc<Mutex<()>>,
    refresh_state: &Arc<WorkspaceRefreshState>,
    loaded_workspace_revision: &Arc<AtomicU64>,
    coordination_enabled: bool,
) -> Result<bool> {
    let fingerprint = published_plan_authority_fingerprint(root)?;
    if !refresh_state.update_published_plan_fingerprint(fingerprint) {
        return Ok(false);
    }
    let Ok(guard) = refresh_lock.try_lock() else {
        return Ok(false);
    };
    sync_published_plan_authority_with_guard(
        root,
        published_generation,
        runtime_state,
        store,
        cold_query_store,
        shared_runtime_store,
        refresh_state,
        loaded_workspace_revision,
        coordination_enabled,
        guard,
    )
}

fn refresh_prism_snapshot_with_guard(
    root: &Path,
    published_generation: &Arc<RwLock<WorkspacePublishedGeneration>>,
    runtime_state: &Arc<Mutex<WorkspaceRuntimeState>>,
    store: &Arc<Mutex<SqliteStore>>,
    cold_query_store: &Arc<Mutex<SqliteStore>>,
    shared_runtime_store: Option<&Arc<Mutex<SharedRuntimeStore>>>,
    shared_runtime_sqlite: Option<&Path>,
    refresh_state: &Arc<WorkspaceRefreshState>,
    loaded_workspace_revision: &Arc<AtomicU64>,
    fs_snapshot: &Arc<Mutex<WorkspaceTreeSnapshot>>,
    checkpoint_materializer: Option<CheckpointMaterializerHandle>,
    shared_runtime_materializer: Option<CheckpointMaterializerHandle>,
    coordination_enabled: bool,
    curator: Option<&CuratorHandleRef>,
    trigger: ChangeTrigger,
    known_fingerprint: Option<WorkspaceTreeSnapshot>,
    dirty_paths_override: Option<Vec<PathBuf>>,
    _guard: MutexGuard<'_, ()>,
) -> Result<WorkspaceRefreshResult> {
    let started = Instant::now();
    let observed_revision = refresh_state.observed_fs_revision();
    let dirty_paths = if trigger == ChangeTrigger::FsWatch {
        dirty_paths_override.unwrap_or_else(|| refresh_state.dirty_paths_snapshot())
    } else {
        Vec::new()
    };
    let scoped_watch_refresh = trigger == ChangeTrigger::FsWatch
        && !dirty_paths.is_empty()
        && can_scope_watch_refresh(root, &dirty_paths);
    let cached_snapshot = fs_snapshot
        .lock()
        .expect("workspace tree snapshot lock poisoned")
        .clone();
    let mut plan = if scoped_watch_refresh {
        plan_incremental_refresh(root, &cached_snapshot, &dirty_paths)?
    } else if let Some(next_snapshot) = known_fingerprint {
        crate::workspace_tree::WorkspaceRefreshPlan {
            mode: if cached_snapshot.files.is_empty() && cached_snapshot.directories.is_empty() {
                WorkspaceRefreshMode::Full
            } else {
                WorkspaceRefreshMode::Rescan
            },
            delta: diff_workspace_tree_snapshot(root, &cached_snapshot, &next_snapshot),
            next_snapshot,
        }
    } else {
        plan_full_refresh(root, &cached_snapshot)?
    };
    if plan.delta.is_empty() {
        *fs_snapshot
            .lock()
            .expect("workspace tree snapshot lock poisoned") = plan.next_snapshot;
        refresh_state.mark_refreshed_revision(observed_revision, &dirty_paths);
        return Ok(WorkspaceRefreshResult {
            mode: None,
            observed: Vec::new(),
        });
    }
    let current_prism = published_generation
        .read()
        .expect("workspace published generation lock poisoned")
        .prism_arc();
    let coordination_context = current_prism.coordination_context();
    let runtime_state_value = {
        let mut state = runtime_state
            .lock()
            .expect("workspace runtime state lock poisoned");
        let placeholder = WorkspaceRuntimeState::placeholder_with_layout(state.layout());
        std::mem::replace(&mut *state, placeholder)
    };
    let mut runtime_state_value = runtime_state_value;
    runtime_state_value.overlay_live_projection_knowledge(current_prism.as_ref());
    let cached_layout = runtime_state_value.layout();
    let layout_refresh_required = plan.mode != WorkspaceRefreshMode::Incremental
        || cached_layout.refresh_required_for_paths(plan.delta.changed_files.iter());
    let next_layout = if layout_refresh_required {
        discover_layout(root)?
    } else {
        cached_layout.clone()
    };
    let refresh_runtime_roots = layout_refresh_required
        || next_layout.workspace_manifest != cached_layout.workspace_manifest
        || next_layout.packages.len() != cached_layout.packages.len();
    let reopened_store = store
        .lock()
        .expect("workspace store lock poisoned")
        .reopen_runtime_writer()?;
    let reopened_shared_runtime_store: Option<SharedRuntimeStore> = shared_runtime_store
        .map(|store| {
            store
                .lock()
                .expect("shared runtime store lock poisoned")
                .reopen_runtime_writer()
        })
        .transpose()?;
    let mut indexer = WorkspaceIndexer::with_runtime_state_stores_and_options(
        root,
        reopened_store,
        reopened_shared_runtime_store,
        runtime_state_value,
        next_layout.clone(),
        refresh_runtime_roots,
        Some(cached_snapshot),
        checkpoint_materializer,
        crate::WorkspaceSessionOptions {
            coordination: coordination_enabled,
            shared_runtime: shared_runtime_sqlite
                .map(|path| SharedRuntimeBackend::Sqlite {
                    path: path.to_path_buf(),
                })
                .unwrap_or(SharedRuntimeBackend::Disabled),
            hydrate_persisted_projections: false,
        },
    )?;
    indexer.shared_runtime_materializer = shared_runtime_materializer;
    populate_package_regions(&mut plan.delta, &indexer.layout);
    let observed = match indexer.index_with_refresh_plan(trigger, &plan) {
        Ok(observed) => observed,
        Err(error) => {
            *runtime_state
                .lock()
                .expect("workspace runtime state lock poisoned") = WorkspaceRuntimeState::new(
                next_layout,
                Graph::from_snapshot(current_prism.graph().snapshot()),
                HistoryStore::from_snapshot(current_prism.history_snapshot()),
                OutcomeMemory::from_snapshot(current_prism.outcome_snapshot()),
                current_prism.coordination_snapshot(),
                current_prism.authored_plan_graphs(),
                current_prism.plan_execution_overlays_by_plan(),
                ProjectionIndex::from_snapshot(current_prism.projection_snapshot()),
            );
            return Err(error);
        }
    };
    let local_workspace_revision = indexer.store.workspace_revision()?;
    let workspace_revision = composite_workspace_revision(
        local_workspace_revision,
        indexer
            .shared_runtime_store
            .as_ref()
            .map(SharedRuntimeStore::workspace_revision)
            .transpose()?,
    );
    let next_state = indexer.into_runtime_state();
    let next = next_state.publish_generation(
        prism_ir::WorkspaceRevision {
            graph_version: local_workspace_revision,
            git_commit: None,
        },
        coordination_context,
    );
    *fs_snapshot
        .lock()
        .expect("workspace tree snapshot lock poisoned") = plan.next_snapshot;
    if let Some(curator) = curator {
        let mut store = store.lock().expect("workspace store lock poisoned");
        enqueue_curator_for_observed_locked(
            curator,
            next.prism_arc().as_ref(),
            &mut store,
            &observed,
        )?;
    }
    WorkspaceSession::attach_cold_query_backends(next.prism_arc().as_ref(), cold_query_store);
    *runtime_state
        .lock()
        .expect("workspace runtime state lock poisoned") = next_state;
    *published_generation
        .write()
        .expect("workspace published generation lock poisoned") = next;
    loaded_workspace_revision.store(workspace_revision, Ordering::Relaxed);
    refresh_state.mark_refreshed_revision(observed_revision, &dirty_paths);
    refresh_state.record_refresh(
        plan.mode.as_str(),
        started.elapsed().as_millis() as u64,
        workspace_revision,
        &plan.delta,
    );
    Ok(WorkspaceRefreshResult {
        mode: Some(plan.mode),
        observed,
    })
}

fn sync_published_plan_authority_with_guard(
    root: &Path,
    published_generation: &Arc<RwLock<WorkspacePublishedGeneration>>,
    runtime_state: &Arc<Mutex<WorkspaceRuntimeState>>,
    store: &Arc<Mutex<SqliteStore>>,
    cold_query_store: &Arc<Mutex<SqliteStore>>,
    shared_runtime_store: Option<&Arc<Mutex<SharedRuntimeStore>>>,
    refresh_state: &Arc<WorkspaceRefreshState>,
    loaded_workspace_revision: &Arc<AtomicU64>,
    coordination_enabled: bool,
    _guard: MutexGuard<'_, ()>,
) -> Result<bool> {
    if !coordination_enabled {
        return Ok(false);
    }
    let started = Instant::now();
    let state = if let Some(shared_runtime_store) = shared_runtime_store {
        shared_runtime_store
            .lock()
            .expect("shared runtime store lock poisoned")
            .load_hydrated_coordination_plan_state_for_root(root)?
    } else {
        store
            .lock()
            .expect("workspace store lock poisoned")
            .load_hydrated_coordination_plan_state_for_root(root)?
    };
    let snapshot = state
        .as_ref()
        .map(|state| state.snapshot.clone())
        .unwrap_or_default();
    let plan_graphs = state
        .as_ref()
        .map(|state| state.plan_graphs.clone())
        .unwrap_or_default();
    let execution_overlays = state
        .as_ref()
        .map(|state| state.execution_overlays.clone())
        .unwrap_or_default();
    let current_prism = published_generation
        .read()
        .expect("workspace published generation lock poisoned")
        .prism_arc();
    let workspace_revision = current_prism.workspace_revision();
    let coordination_context = current_prism.coordination_context();
    current_prism.replace_coordination_snapshot_and_plan_graphs(
        snapshot.clone(),
        plan_graphs.clone(),
        execution_overlays.clone(),
    );
    let next = {
        let mut runtime_state = runtime_state
            .lock()
            .expect("workspace runtime state lock poisoned");
        runtime_state.replace_coordination_runtime(snapshot, plan_graphs, execution_overlays);
        runtime_state.publish_generation(workspace_revision, coordination_context)
    };
    WorkspaceSession::attach_cold_query_backends(next.prism_arc().as_ref(), cold_query_store);
    *published_generation
        .write()
        .expect("workspace published generation lock poisoned") = next;
    refresh_state.record_runtime_refresh_observation(
        "published_plan_authority_reload",
        started.elapsed().as_millis() as u64,
        loaded_workspace_revision.load(Ordering::Relaxed),
    );
    Ok(true)
}

fn can_scope_watch_refresh(root: &Path, dirty_paths: &[PathBuf]) -> bool {
    dirty_paths.iter().all(|path| path.starts_with(root))
}

fn relevant_watch_paths(root: &Path, event: &Event) -> Vec<PathBuf> {
    event
        .paths
        .iter()
        .filter_map(|path| {
            let Ok(relative) = path.strip_prefix(root) else {
                return Some(path.clone());
            };
            (!is_ignored_watch_relative_path(relative)).then(|| path.clone())
        })
        .collect()
}

fn relevant_published_plan_watch_paths(root: &Path, event: &Event) -> Vec<PathBuf> {
    event
        .paths
        .iter()
        .filter_map(|path| {
            let Ok(relative) = path.strip_prefix(root) else {
                return None;
            };
            is_published_plan_watch_relative_path(relative).then(|| path.clone())
        })
        .collect()
}

fn is_ignored_watch_relative_path(relative: &Path) -> bool {
    let components = relative
        .components()
        .map(|component| component.as_os_str().to_string_lossy().into_owned())
        .collect::<Vec<_>>();
    if components.is_empty() {
        return false;
    }

    if components.iter().any(|component| {
        matches!(
            component.as_str(),
            ".git" | ".prism" | "target" | "node_modules"
        )
    }) {
        return true;
    }

    matches!(
        components.as_slice(),
        [first, second, ..]
            if first == "benchmarks"
                && matches!(second.as_str(), "external" | "results")
    )
}

fn is_published_plan_watch_relative_path(relative: &Path) -> bool {
    let components = relative
        .components()
        .map(|component| component.as_os_str().to_string_lossy().into_owned())
        .collect::<Vec<_>>();
    matches!(
        components.as_slice(),
        [first, second, ..] if first == ".prism" && second == "plans"
    )
}

fn format_error_chain(error: &anyhow::Error) -> String {
    error
        .chain()
        .map(ToString::to_string)
        .collect::<Vec<_>>()
        .join(" | ")
}

#[cfg(test)]
mod tests {
    use super::{
        can_scope_watch_refresh, is_ignored_watch_relative_path,
        is_published_plan_watch_relative_path, relevant_published_plan_watch_paths,
        relevant_watch_paths,
    };
    use notify::{
        event::{EventAttributes, ModifyKind},
        Event, EventKind,
    };
    use std::path::PathBuf;

    fn modify_event(path: PathBuf) -> Event {
        Event {
            kind: EventKind::Modify(ModifyKind::Any),
            paths: vec![path],
            attrs: EventAttributes::new(),
        }
    }

    #[test]
    fn ignored_watch_paths_skip_generated_benchmark_results() {
        assert!(is_ignored_watch_relative_path(
            PathBuf::from("benchmarks/results/local/prism/workspaces/demo/repo/src/lib.rs")
                .as_path()
        ));
        assert!(is_ignored_watch_relative_path(
            PathBuf::from("benchmarks/external/demo/src/lib.rs").as_path()
        ));
        assert!(is_ignored_watch_relative_path(
            PathBuf::from("node_modules/pkg/index.json").as_path()
        ));
        assert!(is_ignored_watch_relative_path(
            PathBuf::from(".prism/plans/active/plan:1.jsonl").as_path()
        ));
        assert!(!is_ignored_watch_relative_path(
            PathBuf::from("crates/prism-core/src/watch.rs").as_path()
        ));
    }

    #[test]
    fn relevant_watch_paths_drop_ignored_generated_paths() {
        let root = PathBuf::from("/workspace/prism");
        let event = Event {
            kind: EventKind::Modify(ModifyKind::Any),
            paths: vec![
                root.join(".prism/plans/active/plan:1.jsonl"),
                root.join("benchmarks/results/local/prism/workspaces/demo/repo/src/lib.rs"),
                root.join("crates/prism-core/src/watch.rs"),
            ],
            attrs: EventAttributes::new(),
        };

        let paths = relevant_watch_paths(&root, &event);
        assert_eq!(paths, vec![root.join("crates/prism-core/src/watch.rs")]);
    }

    #[test]
    fn published_plan_watch_paths_are_routed_separately() {
        let root = PathBuf::from("/workspace/prism");
        let event = Event {
            kind: EventKind::Modify(ModifyKind::Any),
            paths: vec![
                root.join(".prism/plans/index.jsonl"),
                root.join(".prism/plans/active/plan:1.jsonl"),
                root.join("crates/prism-core/src/watch.rs"),
            ],
            attrs: EventAttributes::new(),
        };

        assert!(is_published_plan_watch_relative_path(
            PathBuf::from(".prism/plans/active/plan:1.jsonl").as_path()
        ));
        assert!(!is_published_plan_watch_relative_path(
            PathBuf::from(".prism/validation_feedback.jsonl").as_path()
        ));
        assert_eq!(
            relevant_published_plan_watch_paths(&root, &event),
            vec![
                root.join(".prism/plans/index.jsonl"),
                root.join(".prism/plans/active/plan:1.jsonl"),
            ]
        );
    }

    #[test]
    fn relevant_watch_paths_keep_out_of_root_events() {
        let root = PathBuf::from("/workspace/prism");
        let outside = PathBuf::from("/tmp/external.rs");
        let event = modify_event(outside.clone());
        let paths = relevant_watch_paths(&root, &event);
        assert_eq!(paths, vec![outside]);
    }

    #[test]
    fn scoped_watch_refresh_requires_in_root_paths() {
        let root = PathBuf::from("/workspace/prism");
        assert!(can_scope_watch_refresh(
            &root,
            &[root.join("docs/guide.md"), root.join("src/lib.rs")]
        ));
        assert!(!can_scope_watch_refresh(
            &root,
            &[
                root.join("docs/guide.md"),
                PathBuf::from("/tmp/editor-copy.md")
            ]
        ));
    }
}
