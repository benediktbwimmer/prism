use std::path::{Path, PathBuf};
use std::sync::mpsc;
use std::sync::{Arc, Mutex, RwLock};
use std::thread;
use std::time::Duration;

use anyhow::Result;
use notify::{recommended_watcher, Event, RecursiveMode, Watcher};
use prism_ir::ChangeTrigger;
use prism_query::Prism;
use prism_store::SqliteStore;

use crate::curator::{enqueue_curator_for_observed_locked, CuratorHandleRef};
use crate::indexer::WorkspaceIndexer;
use crate::session::WorkspaceRefreshState;
use crate::util::workspace_fingerprint;

pub(crate) struct WatchHandle {
    pub(crate) stop: mpsc::Sender<()>,
    pub(crate) handle: thread::JoinHandle<()>,
}

pub(crate) fn spawn_fs_watch(
    root: PathBuf,
    prism: Arc<RwLock<Arc<Prism>>>,
    store: Arc<Mutex<SqliteStore>>,
    refresh_lock: Arc<Mutex<()>>,
    refresh_state: Arc<WorkspaceRefreshState>,
    fs_fingerprint: Arc<Mutex<u64>>,
    coordination_enabled: bool,
    curator: Option<CuratorHandleRef>,
) -> Result<WatchHandle> {
    let (stop_tx, stop_rx) = mpsc::channel::<()>();
    let (init_tx, init_rx) = mpsc::sync_channel::<Result<()>>(1);

    let handle = thread::spawn(move || {
        let (event_tx, event_rx) = mpsc::channel::<notify::Result<Event>>();
        let mut watcher = match recommended_watcher(move |event| {
            let _ = event_tx.send(event);
        }) {
            Ok(watcher) => watcher,
            Err(error) => {
                let _ = init_tx.send(Err(error.into()));
                return;
            }
        };

        if let Err(error) = watcher.watch(&root, RecursiveMode::Recursive) {
            let _ = init_tx.send(Err(error.into()));
            return;
        }

        let _ = init_tx.send(Ok(()));

        loop {
            if stop_rx.try_recv().is_ok() {
                break;
            }

            let event = match event_rx.recv_timeout(Duration::from_millis(250)) {
                Ok(event) => event,
                Err(mpsc::RecvTimeoutError::Timeout) => continue,
                Err(mpsc::RecvTimeoutError::Disconnected) => break,
            };

            let Ok(event) = event else {
                continue;
            };
            if !is_relevant_watch_event(&root, &event) {
                continue;
            }
            refresh_state.mark_fs_dirty();

            while let Ok(next) = event_rx.recv_timeout(Duration::from_millis(75)) {
                if stop_rx.try_recv().is_ok() {
                    return;
                }
                if let Ok(next) = next {
                    if !is_relevant_watch_event(&root, &next) {
                        continue;
                    }
                }
            }

            if let Err(error) = refresh_prism_snapshot(
                &root,
                &prism,
                &store,
                &refresh_lock,
                &fs_fingerprint,
                coordination_enabled,
                curator.as_ref(),
                ChangeTrigger::FsWatch,
                None,
            ) {
                eprintln!("prism fs watch refresh failed: {error}");
            } else {
                refresh_state.mark_refreshed();
            }
        }
    });

    init_rx
        .recv()
        .map_err(|_| anyhow::anyhow!("watcher init channel closed"))??;

    Ok(WatchHandle {
        stop: stop_tx,
        handle,
    })
}

pub(crate) fn refresh_prism_snapshot(
    root: &Path,
    prism: &Arc<RwLock<Arc<Prism>>>,
    store: &Arc<Mutex<SqliteStore>>,
    refresh_lock: &Arc<Mutex<()>>,
    fs_fingerprint: &Arc<Mutex<u64>>,
    coordination_enabled: bool,
    curator: Option<&CuratorHandleRef>,
    trigger: ChangeTrigger,
    known_fingerprint: Option<u64>,
) -> Result<Vec<prism_ir::ObservedChangeSet>> {
    let _guard = refresh_lock
        .lock()
        .expect("workspace refresh lock poisoned");
    let next_fingerprint = known_fingerprint.unwrap_or(workspace_fingerprint(root)?);
    {
        let current = *fs_fingerprint
            .lock()
            .expect("workspace fingerprint lock poisoned");
        if current == next_fingerprint {
            return Ok(Vec::new());
        }
    }
    let mut indexer = WorkspaceIndexer::new_with_options(
        root,
        crate::WorkspaceSessionOptions {
            coordination: coordination_enabled,
        },
    )?;
    let observed = indexer.index_with_trigger(trigger)?;
    let next = Arc::new(indexer.into_prism());
    *prism.write().expect("workspace prism lock poisoned") = Arc::clone(&next);
    *fs_fingerprint
        .lock()
        .expect("workspace fingerprint lock poisoned") = next_fingerprint;
    if let Some(curator) = curator {
        let mut store = store.lock().expect("workspace store lock poisoned");
        enqueue_curator_for_observed_locked(curator, next.as_ref(), &mut store, &observed)?;
    }
    Ok(observed)
}

fn is_relevant_watch_event(root: &Path, event: &Event) -> bool {
    if event.paths.is_empty() {
        return false;
    }

    event.paths.iter().any(|path| {
        let Ok(relative) = path.strip_prefix(root) else {
            return true;
        };
        let Some(first) = relative.components().next() else {
            return true;
        };
        let first = first.as_os_str().to_string_lossy();
        !matches!(first.as_ref(), ".git" | ".prism" | "target")
    })
}
