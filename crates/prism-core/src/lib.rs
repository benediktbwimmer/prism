use std::collections::hash_map::DefaultHasher;
use std::collections::{HashMap, HashSet};
use std::fs;
use std::hash::{Hash, Hasher};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::mpsc;
use std::sync::{Arc, Mutex, RwLock};
use std::thread;
use std::time::Duration;
use std::time::{SystemTime, UNIX_EPOCH};

use anyhow::Result;
use notify::{recommended_watcher, Event, RecursiveMode, Watcher};
use prism_agent::InferenceSnapshot;
use prism_coordination::{CoordinationSnapshot, CoordinationStore};
use prism_curator::{
    CuratorBackend, CuratorBudget, CuratorContext, CuratorGraphSlice, CuratorJob, CuratorJobId,
    CuratorJobRecord, CuratorJobStatus, CuratorLineageSlice, CuratorProjectionSlice,
    CuratorProposalDisposition, CuratorProposalState, CuratorRun, CuratorSnapshot, CuratorTrigger,
};
use prism_history::HistoryStore;
use prism_ir::{
    AnchorRef, ChangeTrigger, Edge, EdgeKind, EdgeOrigin, EventActor, EventId, EventMeta,
    GraphChange, Language, Node, NodeId, NodeKind, ObservedChangeSet, Span, TaskId,
};
use prism_lang_json::JsonAdapter;
use prism_lang_markdown::MarkdownAdapter;
use prism_lang_rust::RustAdapter;
use prism_lang_yaml::YamlAdapter;
use prism_memory::{
    EpisodicMemorySnapshot, OutcomeEvent, OutcomeKind, OutcomeMemory, OutcomeResult,
};
use prism_parser::{
    LanguageAdapter, ParseInput, ParseResult, SymbolTarget, UnresolvedCall, UnresolvedImpl,
    UnresolvedImport,
};
use prism_projections::{
    co_change_deltas_for_events, validation_deltas_for_event, CoChangeDelta, ProjectionIndex,
    ValidationDelta,
};
use prism_query::Prism;
use prism_store::{AuxiliaryPersistBatch, FileState, Graph, IndexPersistBatch, SqliteStore, Store};
use smol_str::SmolStr;
use toml::Value;
use walkdir::WalkDir;

pub fn index_workspace(root: impl AsRef<Path>) -> Result<Prism> {
    let mut indexer = WorkspaceIndexer::new(root)?;
    indexer.index()?;
    Ok(indexer.into_prism())
}

pub fn index_workspace_session(root: impl AsRef<Path>) -> Result<WorkspaceSession> {
    let root = root.as_ref().canonicalize()?;
    let mut indexer = WorkspaceIndexer::new(&root)?;
    indexer.index()?;
    indexer.into_session(root, None)
}

pub fn index_workspace_session_with_curator(
    root: impl AsRef<Path>,
    backend: Arc<dyn CuratorBackend>,
) -> Result<WorkspaceSession> {
    let root = root.as_ref().canonicalize()?;
    let mut indexer = WorkspaceIndexer::new(&root)?;
    indexer.index()?;
    indexer.into_session(root, Some(backend))
}

pub struct WorkspaceSession {
    root: PathBuf,
    prism: Arc<RwLock<Arc<Prism>>>,
    store: Arc<Mutex<SqliteStore>>,
    refresh_lock: Arc<Mutex<()>>,
    watch: Option<WatchHandle>,
    curator: Option<CuratorHandle>,
}

impl WorkspaceSession {
    pub fn root(&self) -> &Path {
        &self.root
    }

    pub fn prism(&self) -> Arc<Prism> {
        self.prism_arc()
    }

    pub fn prism_arc(&self) -> Arc<Prism> {
        self.prism
            .read()
            .expect("workspace prism lock poisoned")
            .clone()
    }

    pub fn refresh_fs(&self) -> Result<Vec<ObservedChangeSet>> {
        self.refresh_with_trigger(ChangeTrigger::FsWatch)
    }

    pub fn persist_outcomes(&self) -> Result<()> {
        let _guard = self
            .refresh_lock
            .lock()
            .expect("workspace refresh lock poisoned");
        let prism = self.prism_arc();
        let mut store = self.store.lock().expect("workspace store lock poisoned");
        store.commit_auxiliary_persist_batch(&AuxiliaryPersistBatch {
            outcome_snapshot: Some(prism.outcome_snapshot()),
            ..AuxiliaryPersistBatch::default()
        })
    }

    pub fn persist_history(&self) -> Result<()> {
        let _guard = self
            .refresh_lock
            .lock()
            .expect("workspace refresh lock poisoned");
        let prism = self.prism_arc();
        let mut store = self.store.lock().expect("workspace store lock poisoned");
        store.save_history_snapshot(&prism.history_snapshot())
    }

    pub fn load_episodic_snapshot(&self) -> Result<Option<EpisodicMemorySnapshot>> {
        self.store
            .lock()
            .expect("workspace store lock poisoned")
            .load_episodic_snapshot()
    }

    pub fn persist_episodic(&self, snapshot: &EpisodicMemorySnapshot) -> Result<()> {
        self.store
            .lock()
            .expect("workspace store lock poisoned")
            .commit_auxiliary_persist_batch(&AuxiliaryPersistBatch {
                episodic_snapshot: Some(snapshot.clone()),
                ..AuxiliaryPersistBatch::default()
            })
    }

    pub fn load_inference_snapshot(&self) -> Result<Option<InferenceSnapshot>> {
        self.store
            .lock()
            .expect("workspace store lock poisoned")
            .load_inference_snapshot()
    }

    pub fn persist_inference(&self, snapshot: &InferenceSnapshot) -> Result<()> {
        self.store
            .lock()
            .expect("workspace store lock poisoned")
            .commit_auxiliary_persist_batch(&AuxiliaryPersistBatch {
                inference_snapshot: Some(snapshot.clone()),
                ..AuxiliaryPersistBatch::default()
            })
    }

    pub fn load_coordination_snapshot(&self) -> Result<Option<CoordinationSnapshot>> {
        self.store
            .lock()
            .expect("workspace store lock poisoned")
            .load_coordination_snapshot()
    }

    pub fn persist_coordination(&self, snapshot: &CoordinationSnapshot) -> Result<()> {
        let _guard = self
            .refresh_lock
            .lock()
            .expect("workspace refresh lock poisoned");
        self.store
            .lock()
            .expect("workspace store lock poisoned")
            .commit_auxiliary_persist_batch(&AuxiliaryPersistBatch {
                coordination_snapshot: Some(snapshot.clone()),
                ..AuxiliaryPersistBatch::default()
            })
    }

    pub fn persist_current_coordination(&self) -> Result<()> {
        let _guard = self
            .refresh_lock
            .lock()
            .expect("workspace refresh lock poisoned");
        let prism = self.prism_arc();
        self.store
            .lock()
            .expect("workspace store lock poisoned")
            .commit_auxiliary_persist_batch(&AuxiliaryPersistBatch {
                coordination_snapshot: Some(prism.coordination_snapshot()),
                ..AuxiliaryPersistBatch::default()
            })
    }

    pub fn mutate_coordination<T, F>(&self, mutate: F) -> Result<T>
    where
        F: FnOnce(&Prism) -> Result<T>,
    {
        let _guard = self
            .refresh_lock
            .lock()
            .expect("workspace refresh lock poisoned");
        let prism = self.prism_arc();
        let result = mutate(prism.as_ref())?;
        self.store
            .lock()
            .expect("workspace store lock poisoned")
            .commit_auxiliary_persist_batch(&AuxiliaryPersistBatch {
                coordination_snapshot: Some(prism.coordination_snapshot()),
                ..AuxiliaryPersistBatch::default()
            })?;
        Ok(result)
    }

    pub fn curator_snapshot(&self) -> CuratorSnapshot {
        self.curator
            .as_ref()
            .map(CuratorHandle::snapshot)
            .unwrap_or_default()
    }

    pub fn set_curator_proposal_state(
        &self,
        job_id: &CuratorJobId,
        proposal_index: usize,
        disposition: CuratorProposalDisposition,
        task: Option<TaskId>,
        note: Option<String>,
        output: Option<String>,
    ) -> Result<()> {
        let _guard = self
            .refresh_lock
            .lock()
            .expect("workspace refresh lock poisoned");
        let mut store = self.store.lock().expect("workspace store lock poisoned");
        let Some(curator) = &self.curator else {
            return Ok(());
        };
        let mut state = curator.state.lock().expect("curator state lock poisoned");
        let record = state
            .snapshot
            .records
            .iter_mut()
            .find(|record| &record.id == job_id)
            .ok_or_else(|| anyhow::anyhow!("unknown curator job `{}`", job_id.0))?;
        if record.proposal_states.len() <= proposal_index {
            if let Some(run) = &record.run {
                record
                    .proposal_states
                    .resize(run.proposals.len(), CuratorProposalState::default());
            }
        }
        let proposal_state = record
            .proposal_states
            .get_mut(proposal_index)
            .ok_or_else(|| anyhow::anyhow!("unknown curator proposal index {proposal_index}"))?;
        proposal_state.disposition = disposition;
        proposal_state.decided_at = Some(current_timestamp());
        proposal_state.task = task;
        proposal_state.note = note;
        proposal_state.output = output;
        store.commit_auxiliary_persist_batch(&AuxiliaryPersistBatch {
            curator_snapshot: Some(state.snapshot.clone()),
            ..AuxiliaryPersistBatch::default()
        })?;
        Ok(())
    }

    pub fn append_outcome(&self, event: OutcomeEvent) -> Result<EventId> {
        self.append_outcome_with_auxiliary(event, None, None)
    }

    pub fn append_outcome_with_auxiliary(
        &self,
        event: OutcomeEvent,
        episodic_snapshot: Option<EpisodicMemorySnapshot>,
        inference_snapshot: Option<InferenceSnapshot>,
    ) -> Result<EventId> {
        let _guard = self
            .refresh_lock
            .lock()
            .expect("workspace refresh lock poisoned");
        let prism = self.prism_arc();
        let deltas = validation_deltas_for_event(&event, |node| prism.lineage_of(node));
        prism.apply_outcome_event_to_projections(&event);
        let id = prism.outcome_memory().store_event(event)?;
        let mut store = self.store.lock().expect("workspace store lock poisoned");
        store.commit_auxiliary_persist_batch(&AuxiliaryPersistBatch {
            outcome_snapshot: Some(prism.outcome_snapshot()),
            validation_deltas: deltas,
            episodic_snapshot,
            inference_snapshot,
            curator_snapshot: None,
            coordination_snapshot: None,
        })?;
        if let Some(curator) = &self.curator {
            enqueue_curator_for_outcome_locked(curator, prism.as_ref(), &mut store, id.clone())?;
        }
        Ok(id)
    }

    fn refresh_with_trigger(&self, trigger: ChangeTrigger) -> Result<Vec<ObservedChangeSet>> {
        let curator = self.curator.as_ref().map(CuratorHandleRef::from);
        refresh_prism_snapshot(
            &self.root,
            &self.prism,
            &self.store,
            &self.refresh_lock,
            curator.as_ref(),
            trigger,
        )
    }
}

impl Drop for WorkspaceSession {
    fn drop(&mut self) {
        if let Some(watch) = self.watch.take() {
            let _ = watch.stop.send(());
            let _ = watch.handle.join();
        }
        if let Some(mut curator) = self.curator.take() {
            curator.stop();
        }
    }
}

struct WatchHandle {
    stop: mpsc::Sender<()>,
    handle: thread::JoinHandle<()>,
}

struct CuratorHandle {
    state: Arc<Mutex<CuratorQueueState>>,
    tx: Option<mpsc::Sender<CuratorWorkItem>>,
    stop: Option<mpsc::Sender<()>>,
    handle: Option<thread::JoinHandle<()>>,
}

#[derive(Clone)]
struct CuratorHandleRef {
    state: Arc<Mutex<CuratorQueueState>>,
    tx: Option<mpsc::Sender<CuratorWorkItem>>,
}

#[derive(Default)]
struct CuratorQueueState {
    snapshot: CuratorSnapshot,
    next_sequence: u64,
}

struct CuratorWorkItem {
    id: CuratorJobId,
    job: CuratorJob,
    context: CuratorContext,
}

impl CuratorHandle {
    fn new(
        snapshot: CuratorSnapshot,
        backend: Option<Arc<dyn CuratorBackend>>,
        store: Arc<Mutex<SqliteStore>>,
        refresh_lock: Arc<Mutex<()>>,
    ) -> Self {
        let state = Arc::new(Mutex::new(CuratorQueueState {
            next_sequence: next_curator_sequence(&snapshot),
            snapshot,
        }));

        let Some(backend) = backend else {
            return Self {
                state,
                tx: None,
                stop: None,
                handle: None,
            };
        };

        let (tx, rx) = mpsc::channel::<CuratorWorkItem>();
        let (stop_tx, stop_rx) = mpsc::channel::<()>();
        let worker_state = Arc::clone(&state);
        let worker_store = Arc::clone(&store);
        let worker_refresh_lock = Arc::clone(&refresh_lock);
        let handle = thread::spawn(move || loop {
            if stop_rx.try_recv().is_ok() {
                break;
            }

            let item = match rx.recv_timeout(Duration::from_millis(250)) {
                Ok(item) => item,
                Err(mpsc::RecvTimeoutError::Timeout) => continue,
                Err(mpsc::RecvTimeoutError::Disconnected) => break,
            };

            update_curator_record(
                &worker_state,
                &worker_store,
                &worker_refresh_lock,
                &item.id,
                CuratorJobStatus::Running,
                None,
                None,
            );

            match backend.run(&item.job, &item.context) {
                Ok(run) => update_curator_record(
                    &worker_state,
                    &worker_store,
                    &worker_refresh_lock,
                    &item.id,
                    CuratorJobStatus::Completed,
                    Some(run),
                    None,
                ),
                Err(error) => update_curator_record(
                    &worker_state,
                    &worker_store,
                    &worker_refresh_lock,
                    &item.id,
                    CuratorJobStatus::Failed,
                    Some(CuratorRun {
                        proposals: Vec::new(),
                        diagnostics: vec![prism_curator::CuratorDiagnostic {
                            code: "backend_error".to_string(),
                            message: error.to_string(),
                            data: None,
                        }],
                    }),
                    Some(error.to_string()),
                ),
            }
        });

        Self {
            state,
            tx: Some(tx),
            stop: Some(stop_tx),
            handle: Some(handle),
        }
    }

    fn snapshot(&self) -> CuratorSnapshot {
        self.state
            .lock()
            .expect("curator state lock poisoned")
            .snapshot
            .clone()
    }

    fn enqueue_locked(
        &self,
        job: CuratorJob,
        context: CuratorContext,
        store: &mut SqliteStore,
    ) -> Result<CuratorJobId> {
        CuratorHandleRef::from(self).enqueue_locked(job, context, store)
    }

    fn stop(&mut self) {
        if let Some(stop) = self.stop.take() {
            let _ = stop.send(());
        }
        if let Some(handle) = self.handle.take() {
            let _ = handle.join();
        }
    }
}

impl CuratorHandleRef {
    fn enqueue_locked(
        &self,
        job: CuratorJob,
        context: CuratorContext,
        store: &mut SqliteStore,
    ) -> Result<CuratorJobId> {
        let mut state = self.state.lock().expect("curator state lock poisoned");
        let id = CuratorJobId(format!("curator:{}", state.next_sequence + 1));
        state.next_sequence += 1;
        let record = CuratorJobRecord {
            id: id.clone(),
            job: job.clone(),
            status: CuratorJobStatus::Queued,
            created_at: current_timestamp(),
            started_at: None,
            finished_at: None,
            run: None,
            proposal_states: Vec::new(),
            error: None,
        };
        state.snapshot.records.push(record);
        store.commit_auxiliary_persist_batch(&AuxiliaryPersistBatch {
            curator_snapshot: Some(state.snapshot.clone()),
            ..AuxiliaryPersistBatch::default()
        })?;
        drop(state);

        if let Some(tx) = &self.tx {
            let _ = tx.send(CuratorWorkItem {
                id: id.clone(),
                job,
                context,
            });
        }

        Ok(id)
    }
}

impl From<&CuratorHandle> for CuratorHandleRef {
    fn from(value: &CuratorHandle) -> Self {
        Self {
            state: Arc::clone(&value.state),
            tx: value.tx.clone(),
        }
    }
}

fn update_curator_record(
    state: &Arc<Mutex<CuratorQueueState>>,
    store: &Arc<Mutex<SqliteStore>>,
    refresh_lock: &Arc<Mutex<()>>,
    id: &CuratorJobId,
    status: CuratorJobStatus,
    run: Option<CuratorRun>,
    error: Option<String>,
) {
    let _guard = refresh_lock
        .lock()
        .expect("workspace refresh lock poisoned");
    let mut state = state.lock().expect("curator state lock poisoned");
    if let Some(record) = state
        .snapshot
        .records
        .iter_mut()
        .find(|record| &record.id == id)
    {
        record.status = status;
        if matches!(status, CuratorJobStatus::Running) {
            record.started_at = Some(current_timestamp());
        } else if matches!(
            status,
            CuratorJobStatus::Completed | CuratorJobStatus::Failed | CuratorJobStatus::Skipped
        ) {
            if record.started_at.is_none() {
                record.started_at = Some(current_timestamp());
            }
            record.finished_at = Some(current_timestamp());
        }
        if let Some(run) = run {
            if record.proposal_states.is_empty() {
                record
                    .proposal_states
                    .resize(run.proposals.len(), CuratorProposalState::default());
            }
            record.run = Some(run);
        }
        if let Some(error) = error {
            record.error = Some(error);
        }
    }
    if let Ok(mut store) = store.lock() {
        let _ = store.commit_auxiliary_persist_batch(&AuxiliaryPersistBatch {
            curator_snapshot: Some(state.snapshot.clone()),
            ..AuxiliaryPersistBatch::default()
        });
    }
}

fn next_curator_sequence(snapshot: &CuratorSnapshot) -> u64 {
    snapshot
        .records
        .iter()
        .filter_map(|record| record.id.0.rsplit(':').next()?.parse::<u64>().ok())
        .max()
        .unwrap_or(0)
}

pub struct WorkspaceIndexer<S: Store> {
    root: PathBuf,
    layout: WorkspaceLayout,
    graph: Graph,
    history: HistoryStore,
    outcomes: OutcomeMemory,
    coordination: CoordinationStore,
    projections: ProjectionIndex,
    had_prior_snapshot: bool,
    had_projection_snapshot: bool,
    adapters: Vec<Box<dyn LanguageAdapter>>,
    store: S,
}

#[derive(Debug, Clone)]
struct WorkspaceLayout {
    workspace_name: String,
    workspace_display_name: String,
    workspace_manifest: PathBuf,
    packages: Vec<PackageInfo>,
}

#[derive(Debug, Clone)]
struct PackageInfo {
    package_name: String,
    crate_name: String,
    root: PathBuf,
    manifest_path: PathBuf,
    node_id: NodeId,
}

#[derive(Debug, Clone)]
struct PendingFileParse {
    path: PathBuf,
    source: String,
    hash: u64,
    previous_path: Option<PathBuf>,
}

fn spawn_fs_watch(
    root: PathBuf,
    prism: Arc<RwLock<Arc<Prism>>>,
    store: Arc<Mutex<SqliteStore>>,
    refresh_lock: Arc<Mutex<()>>,
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
                curator.as_ref(),
                ChangeTrigger::FsWatch,
            ) {
                eprintln!("prism fs watch refresh failed: {error}");
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

impl WorkspaceIndexer<SqliteStore> {
    pub fn new(root: impl AsRef<Path>) -> Result<Self> {
        let root = root.as_ref().canonicalize()?;
        cleanup_legacy_cache(&root)?;
        let store = SqliteStore::open(cache_path(&root))?;
        Self::with_store(root, store)
    }

    pub fn into_session(
        self,
        root: PathBuf,
        backend: Option<Arc<dyn CuratorBackend>>,
    ) -> Result<WorkspaceSession> {
        let prism = Arc::new(Prism::with_history_outcomes_coordination_and_projections(
            self.graph,
            self.history,
            self.outcomes,
            self.coordination,
            self.projections,
        ));
        let prism = Arc::new(RwLock::new(prism));
        let store = Arc::new(Mutex::new(self.store));
        let refresh_lock = Arc::new(Mutex::new(()));
        let curator_snapshot = {
            let mut store = store.lock().expect("workspace store lock poisoned");
            store.load_curator_snapshot()?.unwrap_or_default()
        };
        let curator = CuratorHandle::new(
            curator_snapshot,
            backend,
            Arc::clone(&store),
            Arc::clone(&refresh_lock),
        );
        let watch = Some(spawn_fs_watch(
            root.clone(),
            Arc::clone(&prism),
            Arc::clone(&store),
            Arc::clone(&refresh_lock),
            Some(CuratorHandleRef::from(&curator)),
        )?);
        Ok(WorkspaceSession {
            root,
            prism,
            store,
            refresh_lock,
            watch,
            curator: Some(curator),
        })
    }
}

impl<S: Store> WorkspaceIndexer<S> {
    pub fn with_store(root: impl AsRef<Path>, mut store: S) -> Result<Self> {
        let root = root.as_ref().canonicalize()?;
        let layout = discover_layout(&root)?;
        let stored_graph = store.load_graph()?;
        let had_prior_snapshot = stored_graph.is_some();
        let mut graph = stored_graph.unwrap_or_default();
        sync_root_nodes(&mut graph, &layout);
        let mut history = store
            .load_history_snapshot()?
            .map(HistoryStore::from_snapshot)
            .unwrap_or_else(HistoryStore::new);
        history.seed_nodes(graph.all_nodes().map(|node| node.id.clone()));
        let outcomes = store
            .load_outcome_snapshot()?
            .map(OutcomeMemory::from_snapshot)
            .unwrap_or_else(OutcomeMemory::new);
        let coordination = store
            .load_coordination_snapshot()?
            .map(CoordinationStore::from_snapshot)
            .unwrap_or_else(CoordinationStore::new);
        let stored_projection_snapshot = store.load_projection_snapshot()?;
        let had_projection_snapshot = stored_projection_snapshot.is_some();
        let projections = stored_projection_snapshot
            .map(ProjectionIndex::from_snapshot)
            .unwrap_or_else(|| ProjectionIndex::derive(&history.snapshot(), &outcomes.snapshot()));

        Ok(Self {
            root,
            layout,
            graph,
            history,
            outcomes,
            coordination,
            projections,
            had_prior_snapshot,
            had_projection_snapshot,
            adapters: default_adapters(),
            store,
        })
    }

    pub fn index(&mut self) -> Result<()> {
        let _ = self.index_with_observed_changes()?;
        Ok(())
    }

    pub fn index_with_changes(&mut self) -> Result<Vec<GraphChange>> {
        let (_, changes) = self.index_impl(ChangeTrigger::ManualReindex)?;
        Ok(changes)
    }

    pub fn index_with_observed_changes(&mut self) -> Result<Vec<ObservedChangeSet>> {
        self.index_with_trigger(ChangeTrigger::ManualReindex)
    }

    pub fn index_with_trigger(&mut self, trigger: ChangeTrigger) -> Result<Vec<ObservedChangeSet>> {
        let (observed, _) = self.index_impl(trigger)?;
        Ok(observed)
    }

    fn index_impl(
        &mut self,
        trigger: ChangeTrigger,
    ) -> Result<(Vec<ObservedChangeSet>, Vec<GraphChange>)> {
        let mut pending = Vec::<PendingFileParse>::new();
        let mut seen_files = HashSet::<PathBuf>::new();
        let mut observed_changes = Vec::<ObservedChangeSet>::new();
        let mut changes = Vec::<GraphChange>::new();
        let mut co_change_deltas = Vec::<CoChangeDelta>::new();
        let mut validation_deltas = Vec::<ValidationDelta>::new();
        let mut upserted_paths = Vec::<PathBuf>::new();
        let mut removed_paths = Vec::<PathBuf>::new();
        let walk_root = self.root.clone();

        for entry in WalkDir::new(&walk_root)
            .into_iter()
            .filter_entry(|entry| should_walk(entry.path(), &walk_root))
            .filter_map(Result::ok)
        {
            if !entry.file_type().is_file() {
                continue;
            }

            let path = entry.path();
            let Some(_adapter) = self
                .adapters
                .iter()
                .find(|adapter| adapter.supports_path(path))
            else {
                continue;
            };

            let canonical_path = path.to_path_buf();
            seen_files.insert(canonical_path.clone());
            let source = fs::read_to_string(path)?;
            let hash = stable_hash(&source);
            pending.push(PendingFileParse {
                path: canonical_path,
                source,
                hash,
                previous_path: None,
            });
        }

        let moved_paths = detect_moved_files(&self.graph, &seen_files, &mut pending);

        for pending_file in pending {
            if pending_file.previous_path.is_none()
                && self
                    .graph
                    .file_record(&pending_file.path)
                    .map(|record| record.hash == pending_file.hash)
                    .unwrap_or(false)
            {
                continue;
            }

            let Some(adapter) = self
                .adapters
                .iter()
                .find(|adapter| adapter.supports_path(&pending_file.path))
            else {
                continue;
            };

            let previous_path = pending_file.previous_path.as_deref();
            let file_id = previous_path
                .and_then(|path| self.graph.file_record(path).map(|record| record.file_id))
                .unwrap_or_else(|| self.graph.ensure_file(&pending_file.path));
            let package = self.layout.package_for(&pending_file.path).clone();
            let input = ParseInput {
                package_name: &package.package_name,
                crate_name: &package.crate_name,
                package_root: &package.root,
                path: &pending_file.path,
                file_id,
                source: &pending_file.source,
            };
            let parsed = adapter.parse(&input)?;
            let update = self.upsert_parsed_file(
                previous_path,
                &pending_file.path,
                pending_file.hash,
                &package,
                parsed,
                trigger.clone(),
            );
            let lineage_events = self.history.apply(&update.observed);
            let change_set_deltas = co_change_deltas_for_events(&lineage_events);
            self.projections.apply_lineage_events(&lineage_events);
            co_change_deltas.extend(change_set_deltas);
            self.outcomes.apply_lineage(&lineage_events)?;
            validation_deltas.extend(self.record_patch_outcome(&update.observed));
            observed_changes.push(update.observed.clone());
            changes.extend(update.changes);
            upserted_paths.push(pending_file.path.clone());
        }

        for tracked in self.graph.tracked_files() {
            if !seen_files.contains(&tracked) && !moved_paths.contains(&tracked) {
                let update = self.graph.remove_file_with_observed(
                    &tracked,
                    default_outcome_meta("observed"),
                    trigger.clone(),
                );
                let lineage_events = self.history.apply(&update.observed);
                let change_set_deltas = co_change_deltas_for_events(&lineage_events);
                self.projections.apply_lineage_events(&lineage_events);
                co_change_deltas.extend(change_set_deltas);
                self.outcomes.apply_lineage(&lineage_events)?;
                validation_deltas.extend(self.record_patch_outcome(&update.observed));
                observed_changes.push(update.observed.clone());
                changes.extend(update.changes);
                removed_paths.push(tracked.clone());
            }
        }

        self.resolve_all_edges();
        self.history
            .seed_nodes(self.graph.all_nodes().map(|node| node.id.clone()));
        let projection_snapshot =
            (!self.had_projection_snapshot).then(|| self.projections.snapshot());
        let batch = IndexPersistBatch {
            upserted_paths,
            removed_paths,
            history_snapshot: self.history.snapshot(),
            outcome_snapshot: self.outcomes.snapshot(),
            co_change_deltas,
            validation_deltas,
            projection_snapshot,
        };
        self.store.commit_index_persist_batch(&self.graph, &batch)?;
        self.had_prior_snapshot = true;
        self.had_projection_snapshot = true;
        Ok((observed_changes, changes))
    }

    pub fn graph(&self) -> &Graph {
        &self.graph
    }

    pub fn into_prism(self) -> Prism {
        Prism::with_history_outcomes_coordination_and_projections(
            self.graph,
            self.history,
            self.outcomes,
            self.coordination,
            self.projections,
        )
    }

    fn upsert_parsed_file(
        &mut self,
        previous_path: Option<&Path>,
        path: &Path,
        hash: u64,
        package: &PackageInfo,
        parsed: ParseResult,
        trigger: ChangeTrigger,
    ) -> prism_store::FileUpdate {
        let previous_state = previous_path
            .or(Some(path))
            .and_then(|candidate| self.graph.file_state(candidate));
        let reanchors = previous_state
            .as_ref()
            .map(|state| infer_reanchors(state, &parsed))
            .unwrap_or_default();
        let package_id = package.node_id.clone();
        let contained_nodes = parsed
            .edges
            .iter()
            .filter(|edge| edge.kind == EdgeKind::Contains)
            .map(|edge| edge.target.clone())
            .collect::<HashSet<_>>();
        let package_edges = parsed
            .nodes
            .iter()
            .filter(|node| !contained_nodes.contains(&node.id))
            .map(|node| Edge {
                kind: EdgeKind::Contains,
                source: package_id.clone(),
                target: node.id.clone(),
                origin: EdgeOrigin::Static,
                confidence: 1.0,
            })
            .collect::<Vec<_>>();

        let mut edges = parsed.edges;
        edges.extend(package_edges);
        self.graph.upsert_file_from_with_observed(
            previous_path,
            path,
            hash,
            parsed.nodes,
            edges,
            parsed.fingerprints,
            parsed.unresolved_calls,
            parsed.unresolved_imports,
            parsed.unresolved_impls,
            &reanchors,
            default_outcome_meta("observed"),
            trigger,
        )
    }

    fn resolve_all_edges(&mut self) {
        self.graph
            .clear_edges_by_kind(&[EdgeKind::Calls, EdgeKind::Imports, EdgeKind::Implements]);
        let unresolved_calls = self.graph.unresolved_calls();
        let unresolved_imports = self.graph.unresolved_imports();
        let unresolved_impls = self.graph.unresolved_impls();
        resolve_calls(&mut self.graph, unresolved_calls);
        resolve_imports(&mut self.graph, unresolved_imports);
        resolve_impls(&mut self.graph, unresolved_impls);
    }

    fn record_patch_outcome(&mut self, observed: &ObservedChangeSet) -> Vec<ValidationDelta> {
        if !self.had_prior_snapshot || observed_is_empty(observed) {
            return Vec::new();
        }

        let mut anchors = observed
            .files
            .iter()
            .copied()
            .filter(|file_id| file_id.0 != 0)
            .map(AnchorRef::File)
            .collect::<Vec<_>>();
        anchors.extend(
            observed
                .added
                .iter()
                .map(|node| AnchorRef::Node(node.node.id.clone())),
        );
        anchors.extend(
            observed
                .removed
                .iter()
                .map(|node| AnchorRef::Node(node.node.id.clone())),
        );
        anchors.extend(observed.updated.iter().flat_map(|(before, after)| {
            [
                AnchorRef::Node(before.node.id.clone()),
                AnchorRef::Node(after.node.id.clone()),
            ]
        }));

        let event = OutcomeEvent {
            meta: EventMeta {
                id: auto_outcome_event_id("outcome"),
                ts: observed.meta.ts,
                actor: EventActor::System,
                correlation: observed.meta.correlation.clone(),
                causation: Some(observed.meta.id.clone()),
            },
            anchors: dedupe_anchors(anchors),
            kind: OutcomeKind::PatchApplied,
            result: OutcomeResult::Success,
            summary: patch_summary(observed),
            evidence: Vec::new(),
            metadata: serde_json::json!({
                "trigger": format!("{:?}", observed.trigger),
                "files": observed.files.iter().map(|file_id| file_id.0).collect::<Vec<_>>(),
            }),
        };
        let deltas = validation_deltas_for_event(&event, |node| self.history.lineage_of(node));
        self.projections
            .apply_outcome_event(&event, |node| self.history.lineage_of(node));
        let _ = self.outcomes.store_event(event);
        deltas
    }
}

static NEXT_AUTO_OUTCOME_ID: AtomicU64 = AtomicU64::new(1);

fn refresh_prism_snapshot(
    root: &Path,
    prism: &Arc<RwLock<Arc<Prism>>>,
    store: &Arc<Mutex<SqliteStore>>,
    refresh_lock: &Arc<Mutex<()>>,
    curator: Option<&CuratorHandleRef>,
    trigger: ChangeTrigger,
) -> Result<Vec<ObservedChangeSet>> {
    let _guard = refresh_lock
        .lock()
        .expect("workspace refresh lock poisoned");
    let mut indexer = WorkspaceIndexer::new(root)?;
    let observed = indexer.index_with_trigger(trigger)?;
    let next = Arc::new(indexer.into_prism());
    *prism.write().expect("workspace prism lock poisoned") = Arc::clone(&next);
    if let Some(curator) = curator {
        let mut store = store.lock().expect("workspace store lock poisoned");
        enqueue_curator_for_observed_locked(curator, next.as_ref(), &mut store, &observed)?;
    }
    Ok(observed)
}

fn enqueue_curator_for_observed_locked(
    curator: &CuratorHandleRef,
    prism: &Prism,
    store: &mut SqliteStore,
    observed: &[ObservedChangeSet],
) -> Result<()> {
    for change in observed {
        if let Some((trigger, focus)) = curator_job_for_observed(change, prism) {
            let budget = CuratorBudget::default();
            let context = build_curator_context(prism, store, &focus, &budget)?;
            let job = CuratorJob {
                id: CuratorJobId("pending".to_string()),
                trigger,
                task: change.meta.correlation.clone(),
                focus,
                budget,
            };
            let _ = curator.enqueue_locked(job, context, store)?;
        }
    }
    Ok(())
}

fn curator_job_for_observed(
    observed: &ObservedChangeSet,
    prism: &Prism,
) -> Option<(CuratorTrigger, Vec<AnchorRef>)> {
    if observed_is_empty(observed) {
        return None;
    }

    let changed_nodes = observed.added.len() + observed.removed.len() + observed.updated.len() * 2;
    let mut focus = Vec::new();
    focus.extend(
        observed
            .updated
            .iter()
            .map(|(_, after)| AnchorRef::Node(after.node.id.clone())),
    );
    focus.extend(
        observed
            .added
            .iter()
            .map(|node| AnchorRef::Node(node.node.id.clone())),
    );
    focus.extend(
        observed
            .removed
            .iter()
            .map(|node| AnchorRef::Node(node.node.id.clone())),
    );
    let focus = dedupe_anchors(prism.anchors_for(&focus));
    if focus.is_empty() {
        return None;
    }

    let has_related_failures = focus.iter().any(|anchor| match anchor {
        AnchorRef::Node(id) => !prism.related_failures(id).is_empty(),
        _ => false,
    });
    if changed_nodes < 3 && observed.files.len() < 2 && !has_related_failures {
        return None;
    }

    let trigger = if changed_nodes >= 6 || observed.files.len() >= 2 {
        CuratorTrigger::HotspotChanged
    } else {
        CuratorTrigger::PostChange
    };
    Some((trigger, focus))
}

fn curator_trigger_for_outcome(prism: &Prism, event: &OutcomeEvent) -> Option<CuratorTrigger> {
    match event.kind {
        OutcomeKind::FailureObserved | OutcomeKind::RegressionObserved => {
            let failures = prism.outcomes_for(&event.anchors, 8);
            if failures
                .iter()
                .filter(|candidate| {
                    matches!(
                        candidate.kind,
                        OutcomeKind::FailureObserved | OutcomeKind::RegressionObserved
                    )
                })
                .count()
                >= 2
            {
                Some(CuratorTrigger::RepeatedFailure)
            } else {
                None
            }
        }
        OutcomeKind::FixValidated => Some(CuratorTrigger::TaskCompleted),
        OutcomeKind::BuildRan | OutcomeKind::TestRan
            if matches!(event.result, OutcomeResult::Failure) =>
        {
            Some(CuratorTrigger::RepeatedFailure)
        }
        _ => None,
    }
}

fn build_curator_context(
    prism: &Prism,
    store: &mut SqliteStore,
    focus: &[AnchorRef],
    budget: &CuratorBudget,
) -> Result<CuratorContext> {
    let focus = prism.anchors_for(focus);
    let lineages = focus_lineages(prism, &focus);
    let nodes = focus_nodes(prism, &focus, budget.max_context_nodes);
    let node_set = nodes
        .iter()
        .map(|node| node.id.clone())
        .collect::<HashSet<_>>();
    let max_edges = budget.max_context_nodes.saturating_mul(4).max(1);
    let mut edges = prism
        .graph()
        .edges
        .iter()
        .filter(|edge| node_set.contains(&edge.source) || node_set.contains(&edge.target))
        .cloned()
        .collect::<Vec<_>>();
    if edges.len() > max_edges {
        edges.truncate(max_edges);
    }

    let mut lineage_events = lineages
        .iter()
        .flat_map(|lineage| prism.lineage_history(lineage))
        .collect::<Vec<_>>();
    lineage_events.sort_by(|left, right| {
        left.meta
            .ts
            .cmp(&right.meta.ts)
            .then_with(|| left.meta.id.0.cmp(&right.meta.id.0))
    });

    let mut co_change = Vec::new();
    let mut validation_checks = Vec::new();
    let projection_snapshot = prism.projection_snapshot();
    for (lineage, records) in projection_snapshot.co_change_by_lineage {
        if lineages.contains(&lineage) {
            co_change.extend(records);
        }
    }
    for (lineage, checks) in projection_snapshot.validation_by_lineage {
        if lineages.contains(&lineage) {
            validation_checks.extend(checks);
        }
    }

    let outcomes = prism.outcomes_for(&focus, budget.max_outcomes);
    let memories = store
        .load_episodic_snapshot()?
        .unwrap_or(EpisodicMemorySnapshot {
            entries: Vec::new(),
        })
        .entries
        .into_iter()
        .filter(|entry| entry.anchors.iter().any(|anchor| focus.contains(anchor)))
        .take(budget.max_memories)
        .collect();

    Ok(CuratorContext {
        graph: CuratorGraphSlice { nodes, edges },
        lineage: CuratorLineageSlice {
            events: lineage_events,
        },
        outcomes,
        memories,
        projections: CuratorProjectionSlice {
            co_change,
            validation_checks,
        },
    })
}

fn enqueue_curator_for_outcome_locked(
    curator: &CuratorHandle,
    prism: &Prism,
    store: &mut SqliteStore,
    outcome_id: EventId,
) -> Result<()> {
    let Some(event) = prism
        .outcome_snapshot()
        .events
        .into_iter()
        .find(|candidate| candidate.meta.id == outcome_id)
    else {
        return Ok(());
    };
    let Some(trigger) = curator_trigger_for_outcome(prism, &event) else {
        return Ok(());
    };
    let focus = dedupe_anchors(prism.anchors_for(&event.anchors));
    if focus.is_empty() {
        return Ok(());
    }
    let budget = CuratorBudget::default();
    let context = build_curator_context(prism, store, &focus, &budget)?;
    let job = CuratorJob {
        id: CuratorJobId("curator:pending".to_string()),
        trigger,
        task: event.meta.correlation.clone(),
        focus,
        budget,
    };
    let _ = curator.enqueue_locked(job, context, store)?;
    Ok(())
}

fn focus_nodes(prism: &Prism, focus: &[AnchorRef], limit: usize) -> Vec<Node> {
    let mut node_ids = HashSet::<NodeId>::new();
    for anchor in focus {
        match anchor {
            AnchorRef::Node(id) => {
                node_ids.insert(id.clone());
            }
            AnchorRef::Lineage(lineage) => {
                for node in
                    prism
                        .history_snapshot()
                        .node_to_lineage
                        .iter()
                        .filter_map(|(id, candidate)| {
                            if candidate == lineage {
                                Some(id.clone())
                            } else {
                                None
                            }
                        })
                {
                    node_ids.insert(node);
                }
            }
            _ => {}
        }
    }

    let mut nodes = node_ids
        .into_iter()
        .filter_map(|id| prism.graph().node(&id).cloned())
        .collect::<Vec<_>>();
    nodes.sort_by(|left, right| left.id.path.cmp(&right.id.path));
    if nodes.len() > limit {
        nodes.truncate(limit);
    }
    nodes
}

fn focus_lineages(prism: &Prism, focus: &[AnchorRef]) -> HashSet<prism_ir::LineageId> {
    let mut lineages = HashSet::new();
    for anchor in focus {
        match anchor {
            AnchorRef::Lineage(lineage) => {
                lineages.insert(lineage.clone());
            }
            AnchorRef::Node(node) => {
                if let Some(lineage) = prism.lineage_of(node) {
                    lineages.insert(lineage);
                }
            }
            _ => {}
        }
    }
    lineages
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

fn default_outcome_meta(prefix: &str) -> EventMeta {
    let sequence = NEXT_AUTO_OUTCOME_ID.fetch_add(1, Ordering::Relaxed);
    EventMeta {
        id: EventId::new(format!("{prefix}:{sequence}")),
        ts: current_timestamp(),
        actor: EventActor::System,
        correlation: None,
        causation: None,
    }
}

fn auto_outcome_event_id(prefix: &str) -> EventId {
    let sequence = NEXT_AUTO_OUTCOME_ID.fetch_add(1, Ordering::Relaxed);
    EventId::new(format!("{prefix}:{sequence}"))
}

fn observed_is_empty(observed: &ObservedChangeSet) -> bool {
    observed.added.is_empty()
        && observed.removed.is_empty()
        && observed.updated.is_empty()
        && observed.edge_added.is_empty()
        && observed.edge_removed.is_empty()
}

fn patch_summary(observed: &ObservedChangeSet) -> String {
    format!(
        "observed file change: {} added, {} removed, {} updated symbols",
        observed.added.len(),
        observed.removed.len(),
        observed.updated.len(),
    )
}

fn dedupe_anchors(anchors: Vec<AnchorRef>) -> Vec<AnchorRef> {
    let mut seen = HashSet::new();
    let mut deduped = Vec::new();
    for anchor in anchors {
        if seen.insert(anchor.clone()) {
            deduped.push(anchor);
        }
    }
    deduped
}

fn current_timestamp() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .expect("system clock should be after unix epoch")
        .as_secs()
}

fn detect_moved_files(
    graph: &Graph,
    seen_files: &HashSet<PathBuf>,
    pending: &mut [PendingFileParse],
) -> HashSet<PathBuf> {
    let mut old_by_hash = HashMap::<u64, Vec<PathBuf>>::new();
    for tracked in graph.tracked_files() {
        if seen_files.contains(&tracked) {
            continue;
        }
        if let Some(record) = graph.file_record(&tracked) {
            old_by_hash.entry(record.hash).or_default().push(tracked);
        }
    }

    let mut moved_paths = HashSet::new();
    for pending_file in pending
        .iter_mut()
        .filter(|pending_file| graph.file_record(&pending_file.path).is_none())
    {
        let Some(candidates) = old_by_hash.get(&pending_file.hash) else {
            continue;
        };
        let available = candidates
            .iter()
            .filter(|candidate| !moved_paths.contains(*candidate))
            .collect::<Vec<_>>();
        if available.len() == 1 {
            let previous = (*available[0]).clone();
            pending_file.previous_path = Some(previous.clone());
            moved_paths.insert(previous);
        }
    }

    moved_paths
}

fn infer_reanchors(previous: &FileState, parsed: &ParseResult) -> Vec<(NodeId, NodeId)> {
    let previous_nodes = previous
        .nodes
        .iter()
        .map(|node| (node.id.clone(), node))
        .collect::<HashMap<_, _>>();
    let parsed_nodes = parsed
        .nodes
        .iter()
        .map(|node| (node.id.clone(), node))
        .collect::<HashMap<_, _>>();

    let mut matched_old = HashSet::<NodeId>::new();
    let mut matched_new = HashSet::<NodeId>::new();
    let mut reanchors = Vec::<(NodeId, NodeId)>::new();
    let mut old_by_fingerprint = HashMap::<prism_parser::NodeFingerprint, Vec<NodeId>>::new();
    let mut new_by_fingerprint = HashMap::<prism_parser::NodeFingerprint, Vec<NodeId>>::new();

    for node in previous
        .nodes
        .iter()
        .filter(|node| parsed_nodes.contains_key(&node.id))
    {
        matched_old.insert(node.id.clone());
        matched_new.insert(node.id.clone());
    }

    for (id, fingerprint) in &previous.record.fingerprints {
        if previous_nodes.contains_key(id) {
            old_by_fingerprint
                .entry(fingerprint.clone())
                .or_default()
                .push(id.clone());
        }
    }

    for (id, fingerprint) in &parsed.fingerprints {
        if parsed_nodes.contains_key(id) {
            new_by_fingerprint
                .entry(fingerprint.clone())
                .or_default()
                .push(id.clone());
        }
    }

    for (fingerprint, old_ids) in &old_by_fingerprint {
        let Some(new_ids) = new_by_fingerprint.get(fingerprint) else {
            continue;
        };
        let available_old = old_ids
            .iter()
            .filter(|id| !matched_old.contains(*id))
            .cloned()
            .collect::<Vec<_>>();
        let available_new = new_ids
            .iter()
            .filter(|id| !matched_new.contains(*id))
            .cloned()
            .collect::<Vec<_>>();

        if available_old.len() == 1 && available_new.len() == 1 {
            let old = available_old[0].clone();
            let new = available_new[0].clone();
            matched_old.insert(old.clone());
            matched_new.insert(new.clone());
            if old != new {
                reanchors.push((old, new));
            }
        }
    }

    for (fingerprint, old_ids) in old_by_fingerprint {
        let Some(new_ids) = new_by_fingerprint.get(&fingerprint) else {
            continue;
        };

        for old_id in old_ids {
            if matched_old.contains(&old_id) {
                continue;
            }

            let Some(old_node) = previous_nodes.get(&old_id) else {
                continue;
            };
            let best = new_ids
                .iter()
                .filter(|new_id| !matched_new.contains(*new_id))
                .filter_map(|new_id| {
                    let new_node = parsed_nodes.get(new_id)?;
                    Some((score_reanchor_candidate(old_node, new_node), new_id.clone()))
                })
                .filter(|(score, _)| *score >= 40)
                .max_by_key(|(score, _)| *score);

            if let Some((_, new_id)) = best {
                matched_old.insert(old_id.clone());
                matched_new.insert(new_id.clone());
                if old_id != new_id {
                    reanchors.push((old_id, new_id));
                }
            }
        }
    }

    reanchors
}

fn score_reanchor_candidate(old: &Node, new: &Node) -> i32 {
    if old.kind != new.kind || old.language != new.language {
        return 0;
    }

    let mut score = 0;
    if old.name == new.name {
        score += 20;
    }
    if old.id.crate_name == new.id.crate_name {
        score += 10;
    }
    if parent_path(old.id.path.as_str()) == parent_path(new.id.path.as_str()) {
        score += 10;
    }

    let start_delta = old.span.start.abs_diff(new.span.start);
    score += (20 - start_delta.min(20)) as i32;

    let end_delta = old.span.end.abs_diff(new.span.end);
    score += (20 - end_delta.min(20)) as i32;

    score
}

fn parent_path(path: &str) -> &str {
    path.rsplit_once("::")
        .map(|(parent, _)| parent)
        .unwrap_or(path)
}

fn default_adapters() -> Vec<Box<dyn LanguageAdapter>> {
    vec![
        Box::new(RustAdapter),
        Box::new(MarkdownAdapter),
        Box::new(JsonAdapter),
        Box::new(YamlAdapter),
    ]
}

impl WorkspaceLayout {
    fn package_for(&self, path: &Path) -> &PackageInfo {
        self.packages
            .iter()
            .filter(|package| path.starts_with(&package.root))
            .max_by_key(|package| package.root.components().count())
            .unwrap_or(&self.packages[0])
    }
}

impl PackageInfo {
    fn new(package_name: String, root: PathBuf, manifest_path: PathBuf) -> Self {
        let crate_name = normalize_identifier(&package_name);
        let node_id = NodeId::new(crate_name.clone(), crate_name.clone(), NodeKind::Package);
        Self {
            package_name,
            crate_name,
            root,
            manifest_path,
            node_id,
        }
    }
}

fn sync_root_nodes(graph: &mut Graph, layout: &WorkspaceLayout) -> NodeId {
    let manifest_file = graph.ensure_file(&layout.workspace_manifest);
    let workspace_id = NodeId::new(
        layout.workspace_name.clone(),
        format!("{}::workspace", layout.workspace_name),
        NodeKind::Workspace,
    );
    let allowed_root_ids = std::iter::once(workspace_id.clone())
        .chain(
            layout
                .packages
                .iter()
                .map(|package| package.node_id.clone()),
        )
        .collect::<HashSet<_>>();
    graph.retain_root_nodes(&allowed_root_ids);

    graph.add_node(Node {
        id: workspace_id.clone(),
        name: SmolStr::new(layout.workspace_display_name.clone()),
        kind: NodeKind::Workspace,
        file: manifest_file,
        span: Span::line(1),
        language: Language::Unknown,
    });

    for package in &layout.packages {
        let manifest_file = graph.ensure_file(&package.manifest_path);
        graph.add_node(Node {
            id: package.node_id.clone(),
            name: SmolStr::new(package.package_name.clone()),
            kind: NodeKind::Package,
            file: manifest_file,
            span: Span::line(1),
            language: Language::Unknown,
        });
    }

    graph.clear_root_contains_edges();
    for package in &layout.packages {
        graph.add_edge(Edge {
            kind: EdgeKind::Contains,
            source: workspace_id.clone(),
            target: package.node_id.clone(),
            origin: EdgeOrigin::Static,
            confidence: 1.0,
        });
    }

    workspace_id
}

fn resolve_calls(graph: &mut Graph, unresolved: Vec<UnresolvedCall>) {
    for call in unresolved {
        let Some(target) = resolve_target(
            graph,
            SymbolTarget {
                kind: EdgeKind::Calls,
                source: &call.caller,
                module_path: &call.module_path,
                name: &call.name,
                target_path: "",
            },
        ) else {
            continue;
        };
        graph.add_edge(Edge {
            kind: EdgeKind::Calls,
            source: call.caller.clone(),
            target,
            origin: EdgeOrigin::Static,
            confidence: 0.6,
        });
    }
}

fn resolve_imports(graph: &mut Graph, unresolved: Vec<UnresolvedImport>) {
    for import in unresolved {
        let name = import
            .path
            .rsplit("::")
            .next()
            .unwrap_or(import.path.as_str())
            .to_owned();
        let Some(target) = resolve_target(
            graph,
            SymbolTarget {
                kind: EdgeKind::Imports,
                source: &import.importer,
                module_path: &import.module_path,
                name: &name,
                target_path: &import.path,
            },
        ) else {
            continue;
        };
        graph.add_edge(Edge {
            kind: EdgeKind::Imports,
            source: import.importer.clone(),
            target,
            origin: EdgeOrigin::Static,
            confidence: 0.8,
        });
    }
}

fn resolve_impls(graph: &mut Graph, unresolved: Vec<UnresolvedImpl>) {
    for implementation in unresolved {
        let name = implementation
            .target
            .rsplit("::")
            .next()
            .unwrap_or(implementation.target.as_str())
            .to_owned();
        let Some(target) = resolve_target(
            graph,
            SymbolTarget {
                kind: EdgeKind::Implements,
                source: &implementation.impl_node,
                module_path: &implementation.module_path,
                name: &name,
                target_path: &implementation.target,
            },
        ) else {
            continue;
        };
        graph.add_edge(Edge {
            kind: EdgeKind::Implements,
            source: implementation.impl_node.clone(),
            target,
            origin: EdgeOrigin::Static,
            confidence: 0.8,
        });
    }
}

fn resolve_target(graph: &Graph, target: SymbolTarget<'_>) -> Option<NodeId> {
    let allowed = |kind: NodeKind| match target.kind {
        EdgeKind::Calls => matches!(kind, NodeKind::Function | NodeKind::Method),
        EdgeKind::Implements => kind == NodeKind::Trait,
        EdgeKind::Imports => !matches!(kind, NodeKind::Workspace | NodeKind::Package),
        _ => false,
    };

    if !target.target_path.is_empty() {
        if let Some(node) = graph
            .all_nodes()
            .find(|node| allowed(node.kind) && node.id.path == target.target_path)
        {
            return Some(node.id.clone());
        }
    }

    let exact_path = format!("{}::{}", target.module_path, target.name);
    if let Some(node) = graph
        .all_nodes()
        .find(|node| allowed(node.kind) && node.id.path == exact_path)
    {
        return Some(node.id.clone());
    }

    let mut matches = graph
        .all_nodes()
        .filter(|node| allowed(node.kind))
        .filter(|node| node.name == target.name)
        .map(|node| node.id.clone())
        .collect::<Vec<_>>();

    if matches.len() == 1 {
        return matches.pop();
    }

    None
}

fn stable_hash(source: &str) -> u64 {
    let mut hasher = DefaultHasher::new();
    source.hash(&mut hasher);
    hasher.finish()
}

fn discover_layout(root: &Path) -> Result<WorkspaceLayout> {
    let workspace_display_name = root
        .file_name()
        .and_then(|value| value.to_str())
        .unwrap_or("workspace")
        .to_owned();
    let workspace_name = normalize_identifier(&workspace_display_name);
    let workspace_manifest = root.join("Cargo.toml");
    let root_package_name = manifest_package_name(&workspace_manifest)?
        .unwrap_or_else(|| workspace_display_name.clone());
    let mut packages = vec![PackageInfo::new(
        root_package_name,
        root.to_path_buf(),
        workspace_manifest.clone(),
    )];

    for entry in WalkDir::new(root)
        .into_iter()
        .filter_entry(|entry| should_walk(entry.path(), root))
        .filter_map(Result::ok)
    {
        if !entry.file_type().is_file() || entry.file_name() != "Cargo.toml" {
            continue;
        }

        let manifest_path = entry.path();
        if manifest_path == workspace_manifest {
            continue;
        }

        let Some(package_name) = manifest_package_name(manifest_path)? else {
            continue;
        };
        let package_root = manifest_path
            .parent()
            .unwrap_or(root)
            .canonicalize()
            .unwrap_or_else(|_| manifest_path.parent().unwrap_or(root).to_path_buf());
        packages.push(PackageInfo::new(
            package_name,
            package_root,
            manifest_path.to_path_buf(),
        ));
    }

    Ok(WorkspaceLayout {
        workspace_name,
        workspace_display_name,
        workspace_manifest,
        packages,
    })
}

fn manifest_package_name(path: &Path) -> Result<Option<String>> {
    if !path.exists() {
        return Ok(None);
    }

    let manifest = fs::read_to_string(path)?;
    let value: Value = toml::from_str(&manifest)?;
    Ok(value
        .get("package")
        .and_then(|package| package.get("name"))
        .and_then(Value::as_str)
        .map(ToOwned::to_owned))
}

fn normalize_identifier(value: &str) -> String {
    let mut normalized = String::new();
    let mut previous_underscore = false;

    for ch in value.chars() {
        let ch = ch.to_ascii_lowercase();
        if ch.is_ascii_alphanumeric() {
            normalized.push(ch);
            previous_underscore = false;
        } else if !previous_underscore {
            normalized.push('_');
            previous_underscore = true;
        }
    }

    let normalized = normalized.trim_matches('_').to_owned();
    if normalized.is_empty() {
        "workspace".to_owned()
    } else {
        normalized
    }
}

fn cache_path(root: &Path) -> PathBuf {
    root.join(".prism").join("cache.db")
}

fn cleanup_legacy_cache(root: &Path) -> Result<()> {
    let legacy = root.join(".prism").join("cache.bin");
    if legacy.exists() {
        fs::remove_file(legacy)?;
    }
    Ok(())
}

fn should_walk(path: &Path, root: &Path) -> bool {
    let Ok(relative) = path.strip_prefix(root) else {
        return true;
    };
    let Some(first) = relative.components().next() else {
        return true;
    };
    let first = first.as_os_str().to_string_lossy();
    !matches!(first.as_ref(), ".git" | ".prism" | "target")
}

#[cfg(test)]
mod tests {
    use std::fs;
    use std::path::PathBuf;
    use std::sync::atomic::{AtomicU64, Ordering};
    use std::sync::{Arc, Mutex};
    use std::thread;
    use std::time::Duration;
    use std::time::{SystemTime, UNIX_EPOCH};

    use prism_curator::{
        CandidateRiskSummary, CuratorBackend, CuratorContext, CuratorJob, CuratorProposal,
        CuratorRun,
    };
    use prism_ir::{
        AnchorRef, EdgeKind, EventActor, EventId, EventMeta, GraphChange, NodeId, NodeKind, TaskId,
    };
    use prism_memory::{OutcomeEvent, OutcomeEvidence, OutcomeKind, OutcomeResult};
    use prism_store::MemoryStore;

    use super::{
        index_workspace, index_workspace_session, index_workspace_session_with_curator,
        WorkspaceIndexer,
    };

    static NEXT_TEMP_WORKSPACE: AtomicU64 = AtomicU64::new(0);

    #[test]
    fn reindexes_incrementally_across_file_changes() {
        let root = temp_workspace();
        fs::create_dir_all(root.join("src")).unwrap();
        fs::write(
            root.join("Cargo.toml"),
            "[package]\nname = \"demo\"\nversion = \"0.1.0\"\n",
        )
        .unwrap();
        fs::write(
            root.join("src/lib.rs"),
            "fn alpha() { beta(); }\nfn beta() {}\n",
        )
        .unwrap();

        let mut indexer = WorkspaceIndexer::with_store(&root, MemoryStore::default()).unwrap();
        indexer.index().unwrap();
        assert!(indexer.outcomes.snapshot().events.is_empty());

        let initial_calls = indexer
            .graph()
            .edges
            .iter()
            .filter(|edge| edge.kind == EdgeKind::Calls)
            .count();
        assert_eq!(initial_calls, 1);

        fs::write(
            root.join("src/lib.rs"),
            "fn alpha() { gamma(); }\nfn gamma() {}\n",
        )
        .unwrap();
        indexer.index().unwrap();

        let patch_events = indexer
            .outcomes
            .outcomes_for(
                &[AnchorRef::Node(NodeId::new(
                    "demo",
                    "demo::gamma",
                    NodeKind::Function,
                ))],
                10,
            )
            .into_iter()
            .filter(|event| event.kind == OutcomeKind::PatchApplied)
            .collect::<Vec<_>>();
        assert_eq!(patch_events.len(), 1);

        assert!(indexer
            .graph()
            .nodes_by_name("gamma")
            .into_iter()
            .any(|node| node.id.path == "prism::gamma" || node.id.path.ends_with("::gamma")));
        assert_eq!(
            indexer
                .graph()
                .edges
                .iter()
                .filter(|edge| edge.kind == EdgeKind::Calls)
                .count(),
            1
        );

        fs::remove_file(root.join("src/lib.rs")).unwrap();
        indexer.index().unwrap();

        let removal_patch_events = indexer
            .outcomes
            .snapshot()
            .events
            .into_iter()
            .filter(|event| event.kind == OutcomeKind::PatchApplied)
            .count();
        assert_eq!(removal_patch_events, 2);

        assert!(indexer.graph().nodes_by_name("alpha").is_empty());
        assert!(indexer
            .graph()
            .edges
            .iter()
            .all(|edge| edge.kind != EdgeKind::Calls));

        let _ = fs::remove_dir_all(root);
    }

    #[test]
    fn reloads_graph_from_disk_cache() {
        let root = temp_workspace();
        fs::create_dir_all(root.join("src")).unwrap();
        fs::write(
            root.join("Cargo.toml"),
            "[package]\nname = \"demo\"\nversion = \"0.1.0\"\n",
        )
        .unwrap();
        fs::write(root.join("src/lib.rs"), "fn alpha() {}\n").unwrap();

        let mut first = WorkspaceIndexer::new(&root).unwrap();
        first.index().unwrap();
        drop(first);

        assert!(root.join(".prism/cache.db").exists());

        let second = WorkspaceIndexer::new(&root).unwrap();
        assert!(second
            .graph()
            .nodes_by_name("alpha")
            .into_iter()
            .any(|node| node.id.path.ends_with("::alpha")));

        let _ = fs::remove_dir_all(root);
    }

    #[test]
    fn uses_member_package_identity_and_attaches_workspace_docs() {
        let root = temp_workspace();
        fs::create_dir_all(root.join("crates/alpha/src")).unwrap();
        fs::create_dir_all(root.join("crates/beta/src")).unwrap();
        fs::create_dir_all(root.join("docs")).unwrap();
        fs::write(
            root.join("Cargo.toml"),
            "[workspace]\nmembers = [\"crates/alpha\", \"crates/beta\"]\n",
        )
        .unwrap();
        fs::write(
            root.join("crates/alpha/Cargo.toml"),
            "[package]\nname = \"alpha-pkg\"\nversion = \"0.1.0\"\n",
        )
        .unwrap();
        fs::write(
            root.join("crates/beta/Cargo.toml"),
            "[package]\nname = \"beta-pkg\"\nversion = \"0.1.0\"\n",
        )
        .unwrap();
        fs::write(root.join("crates/alpha/src/lib.rs"), "fn alpha() {}\n").unwrap();
        fs::write(
            root.join("crates/beta/src/lib.rs"),
            "mod outer { mod inner {} }\n",
        )
        .unwrap();
        fs::write(root.join("docs/SPEC.md"), "# Spec\n").unwrap();

        let mut indexer = WorkspaceIndexer::with_store(&root, MemoryStore::default()).unwrap();
        indexer.index().unwrap();

        assert!(indexer
            .graph()
            .nodes_by_name("alpha")
            .into_iter()
            .any(|node| node.id.crate_name == "alpha_pkg" && node.id.path == "alpha_pkg::alpha"));
        assert!(indexer
            .graph()
            .nodes_by_name("inner")
            .into_iter()
            .any(
                |node| node.id.crate_name == "beta_pkg" && node.id.path == "beta_pkg::outer::inner"
            ));

        let inner_module = indexer
            .graph()
            .nodes_by_name("inner")
            .into_iter()
            .find(|node| node.kind == NodeKind::Module)
            .unwrap();
        assert!(!indexer
            .graph()
            .edges_to(&inner_module.id, Some(EdgeKind::Contains))
            .iter()
            .any(|edge| edge.source.kind == NodeKind::Package));

        let spec = indexer
            .graph()
            .nodes_by_name("Spec")
            .into_iter()
            .find(|node| node.kind == NodeKind::MarkdownHeading)
            .unwrap();
        let spec_document = indexer
            .graph()
            .nodes_by_name("docs/SPEC.md")
            .into_iter()
            .find(|node| node.kind == NodeKind::Document)
            .unwrap();
        assert!(indexer
            .graph()
            .edges_to(&spec_document.id, Some(EdgeKind::Contains))
            .iter()
            .any(|edge| edge.source.kind == NodeKind::Package));
        assert!(indexer
            .graph()
            .edges_to(&spec.id, Some(EdgeKind::Contains))
            .iter()
            .any(|edge| edge.source == spec_document.id));

        let _ = fs::remove_dir_all(root);
    }

    #[test]
    fn emits_reanchored_change_for_symbol_rename() {
        let root = temp_workspace();
        fs::create_dir_all(root.join("src")).unwrap();
        fs::write(
            root.join("Cargo.toml"),
            "[package]\nname = \"demo\"\nversion = \"0.1.0\"\n",
        )
        .unwrap();
        fs::write(
            root.join("src/lib.rs"),
            "fn alpha() { helper(); }\nfn helper() {}\n",
        )
        .unwrap();

        let mut indexer = WorkspaceIndexer::with_store(&root, MemoryStore::default()).unwrap();
        indexer.index().unwrap();

        fs::write(
            root.join("src/lib.rs"),
            "fn renamed_alpha() { helper(); }\nfn helper() {}\n",
        )
        .unwrap();

        let changes = indexer.index_with_changes().unwrap();

        assert!(changes.contains(&GraphChange::Reanchored {
            old: NodeId::new("demo", "demo::alpha", NodeKind::Function),
            new: NodeId::new("demo", "demo::renamed_alpha", NodeKind::Function),
        }));

        let _ = fs::remove_dir_all(root);
    }

    #[test]
    fn emits_reanchored_changes_for_file_move_with_same_content() {
        let root = temp_workspace();
        fs::create_dir_all(root.join("src")).unwrap();
        fs::write(
            root.join("Cargo.toml"),
            "[package]\nname = \"demo\"\nversion = \"0.1.0\"\n",
        )
        .unwrap();
        fs::write(
            root.join("src/feature.rs"),
            "pub fn alpha() { helper(); }\nfn helper() {}\n",
        )
        .unwrap();

        let mut indexer = WorkspaceIndexer::with_store(&root, MemoryStore::default()).unwrap();
        indexer.index().unwrap();

        fs::rename(root.join("src/feature.rs"), root.join("src/renamed.rs")).unwrap();

        let changes = indexer.index_with_changes().unwrap();

        assert!(changes.contains(&GraphChange::Reanchored {
            old: NodeId::new("demo", "demo::feature", NodeKind::Module),
            new: NodeId::new("demo", "demo::renamed", NodeKind::Module),
        }));
        assert!(changes.contains(&GraphChange::Reanchored {
            old: NodeId::new("demo", "demo::feature::alpha", NodeKind::Function),
            new: NodeId::new("demo", "demo::renamed::alpha", NodeKind::Function),
        }));

        let _ = fs::remove_dir_all(root);
    }

    #[test]
    fn watcher_refreshes_session_after_external_edit() {
        let root = temp_workspace();
        fs::create_dir_all(root.join("src")).unwrap();
        fs::write(
            root.join("Cargo.toml"),
            "[package]\nname = \"demo\"\nversion = \"0.1.0\"\n",
        )
        .unwrap();
        fs::write(
            root.join("src/lib.rs"),
            "pub fn alpha() { beta(); }\npub fn beta() {}\n",
        )
        .unwrap();

        let session = index_workspace_session(&root).unwrap();
        fs::write(
            root.join("src/lib.rs"),
            "pub fn alpha() { gamma(); }\npub fn gamma() {}\n",
        )
        .unwrap();

        let mut saw_gamma = false;
        for _ in 0..40 {
            if session
                .prism()
                .symbol("gamma")
                .iter()
                .any(|symbol| symbol.id().path == "demo::gamma")
            {
                saw_gamma = true;
                break;
            }
            thread::sleep(Duration::from_millis(50));
        }

        assert!(saw_gamma);
        let patch_events = session
            .prism()
            .outcome_memory()
            .outcomes_for(
                &[AnchorRef::Node(NodeId::new(
                    "demo",
                    "demo::gamma",
                    NodeKind::Function,
                ))],
                10,
            )
            .into_iter()
            .filter(|event| event.kind == OutcomeKind::PatchApplied)
            .count();
        assert_eq!(patch_events, 1);

        let _ = fs::remove_dir_all(root);
    }

    #[test]
    fn appended_outcome_persists_projection_snapshot() {
        let root = temp_workspace();
        fs::create_dir_all(root.join("src")).unwrap();
        fs::write(
            root.join("Cargo.toml"),
            "[package]\nname = \"demo\"\nversion = \"0.1.0\"\n",
        )
        .unwrap();
        fs::write(root.join("src/lib.rs"), "pub fn alpha() {}\n").unwrap();

        let session = index_workspace_session(&root).unwrap();
        let alpha = session
            .prism()
            .symbol("alpha")
            .into_iter()
            .next()
            .unwrap()
            .id()
            .clone();
        session
            .append_outcome(OutcomeEvent {
                meta: EventMeta {
                    id: EventId::new("outcome:test"),
                    ts: 10,
                    actor: EventActor::User,
                    correlation: Some(TaskId::new("task:test")),
                    causation: None,
                },
                anchors: vec![AnchorRef::Node(alpha.clone())],
                kind: OutcomeKind::FailureObserved,
                result: OutcomeResult::Failure,
                summary: "alpha needs integration coverage".into(),
                evidence: vec![OutcomeEvidence::Test {
                    name: "alpha_integration".into(),
                    passed: false,
                }],
                metadata: serde_json::Value::Null,
            })
            .unwrap();
        drop(session);

        let prism = index_workspace(&root).unwrap();
        let recipe = prism.validation_recipe(&alpha);
        assert!(recipe
            .scored_checks
            .iter()
            .any(|check| check.label == "test:alpha_integration" && check.score > 0.0));

        let _ = fs::remove_dir_all(root);
    }

    #[test]
    fn curator_backend_processes_and_persists_task_boundary_jobs() {
        let root = temp_workspace();
        fs::create_dir_all(root.join("src")).unwrap();
        fs::write(
            root.join("Cargo.toml"),
            "[package]\nname = \"demo\"\nversion = \"0.1.0\"\n",
        )
        .unwrap();
        fs::write(root.join("src/lib.rs"), "pub fn alpha() {}\n").unwrap();

        #[derive(Clone, Default)]
        struct FakeCurator {
            seen: Arc<Mutex<Vec<String>>>,
        }

        impl CuratorBackend for FakeCurator {
            fn run(&self, _job: &CuratorJob, ctx: &CuratorContext) -> anyhow::Result<CuratorRun> {
                self.seen
                    .lock()
                    .unwrap()
                    .push(format!("nodes:{}", ctx.graph.nodes.len()));
                Ok(CuratorRun {
                    proposals: vec![CuratorProposal::RiskSummary(CandidateRiskSummary {
                        anchors: Vec::new(),
                        summary: "alpha needs follow-up".into(),
                        severity: "medium".into(),
                        evidence_events: Vec::new(),
                    })],
                    diagnostics: Vec::new(),
                })
            }
        }

        let backend = FakeCurator::default();
        let session =
            index_workspace_session_with_curator(&root, Arc::new(backend.clone())).unwrap();
        let alpha = session
            .prism()
            .symbol("alpha")
            .into_iter()
            .next()
            .unwrap()
            .id()
            .clone();
        session
            .append_outcome(OutcomeEvent {
                meta: EventMeta {
                    id: EventId::new("outcome:validated"),
                    ts: 42,
                    actor: EventActor::User,
                    correlation: Some(TaskId::new("task:alpha")),
                    causation: None,
                },
                anchors: vec![AnchorRef::Node(alpha)],
                kind: OutcomeKind::FixValidated,
                result: OutcomeResult::Success,
                summary: "alpha fix validated".into(),
                evidence: Vec::new(),
                metadata: serde_json::Value::Null,
            })
            .unwrap();

        let mut completed = false;
        for _ in 0..40 {
            let snapshot = session.curator_snapshot();
            if snapshot
                .records
                .iter()
                .any(|record| record.status == prism_curator::CuratorJobStatus::Completed)
            {
                completed = true;
                break;
            }
            thread::sleep(Duration::from_millis(50));
        }

        assert!(completed);
        assert_eq!(backend.seen.lock().unwrap().len(), 1);
        drop(session);

        let reloaded = index_workspace_session(&root).unwrap();
        let snapshot = reloaded.curator_snapshot();
        assert_eq!(snapshot.records.len(), 1);
        assert!(matches!(
            snapshot.records[0].run.as_ref().and_then(|run| run.proposals.first()),
            Some(CuratorProposal::RiskSummary(summary)) if summary.summary == "alpha needs follow-up"
        ));

        let _ = fs::remove_dir_all(root);
    }

    fn temp_workspace() -> PathBuf {
        let stamp = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        let sequence = NEXT_TEMP_WORKSPACE.fetch_add(1, Ordering::Relaxed);
        std::env::temp_dir().join(format!(
            "prism-test-{}-{stamp}-{sequence}",
            std::process::id()
        ))
    }
}
