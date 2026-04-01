use std::sync::mpsc;
use std::sync::{Arc, Mutex, RwLock};
use std::thread;

use anyhow::Result;
use prism_curator::{
    merge_curator_runs, synthesize_curator_run, CuratorBackend, CuratorBudget, CuratorJob,
    CuratorJobId, CuratorJobRecord, CuratorJobStatus, CuratorProposalState, CuratorRun,
    CuratorSnapshot,
};
use prism_ir::{new_prefixed_id, EventId};
use prism_query::Prism;
use prism_store::{AuxiliaryPersistBatch, SqliteStore, Store};

use crate::curator_support::{
    build_curator_context, curator_job_for_observed, curator_trigger_for_outcome,
    next_curator_sequence,
};
use crate::patch_outcomes::dedupe_anchors;
use crate::util::current_timestamp;

pub(crate) struct CuratorHandle {
    pub(crate) state: Arc<Mutex<CuratorQueueState>>,
    store: Arc<Mutex<SqliteStore>>,
    tx: Option<mpsc::Sender<CuratorMessage>>,
    handle: Option<thread::JoinHandle<()>>,
}

#[derive(Clone)]
pub(crate) struct CuratorHandleRef {
    state: Arc<Mutex<CuratorQueueState>>,
    tx: Option<mpsc::Sender<CuratorMessage>>,
}

#[derive(Default)]
pub(crate) struct CuratorQueueState {
    pub(crate) snapshot: CuratorSnapshot,
    next_sequence: u64,
    pub(crate) loaded: bool,
}

struct CuratorWorkItem {
    id: CuratorJobId,
    job: CuratorJob,
}

enum CuratorMessage {
    Work(CuratorWorkItem),
    Stop,
}

impl CuratorHandle {
    pub(crate) fn new(
        backend: Option<Arc<dyn CuratorBackend>>,
        prism: Arc<RwLock<Arc<Prism>>>,
        store: Arc<Mutex<SqliteStore>>,
        context_store: Arc<Mutex<SqliteStore>>,
        refresh_lock: Arc<Mutex<()>>,
    ) -> Self {
        let state = Arc::new(Mutex::new(CuratorQueueState::default()));

        let (tx, rx) = mpsc::channel::<CuratorMessage>();
        let worker_state = Arc::clone(&state);
        let worker_store = Arc::clone(&store);
        let worker_context_store = Arc::clone(&context_store);
        let worker_prism = Arc::clone(&prism);
        let worker_refresh_lock = Arc::clone(&refresh_lock);
        let worker_backend = backend.clone();
        let handle = thread::spawn(move || loop {
            let item = match rx.recv() {
                Ok(CuratorMessage::Work(item)) => item,
                Ok(CuratorMessage::Stop) | Err(mpsc::RecvError) => break,
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

            let context = {
                let prism = worker_prism
                    .read()
                    .expect("workspace prism lock poisoned")
                    .clone();
                let mut store = worker_context_store
                    .lock()
                    .expect("curator context store lock poisoned");
                build_curator_context(
                    prism.as_ref(),
                    &mut store,
                    &item.job.focus,
                    &item.job.budget,
                )
            };

            let Ok(context) = context else {
                let error = context.err().expect("context error must exist");
                update_curator_record(
                    &worker_state,
                    &worker_store,
                    &worker_refresh_lock,
                    &item.id,
                    CuratorJobStatus::Failed,
                    None,
                    Some(error.to_string()),
                );
                continue;
            };

            let synthesized = synthesize_curator_run(&item.job, &context);
            let backend_result = worker_backend
                .as_ref()
                .map(|backend| backend.run(&item.job, &context))
                .transpose();

            match backend_result {
                Ok(run) => {
                    let merged =
                        merge_curator_runs(synthesized, run, item.job.budget.max_proposals, None);
                    update_curator_record(
                        &worker_state,
                        &worker_store,
                        &worker_refresh_lock,
                        &item.id,
                        CuratorJobStatus::Completed,
                        Some(merged),
                        None,
                    )
                }
                Err(error) => {
                    let merged = merge_curator_runs(
                        synthesized,
                        None,
                        item.job.budget.max_proposals,
                        Some(error.to_string()),
                    );
                    let status = if merged.proposals.is_empty() {
                        CuratorJobStatus::Failed
                    } else {
                        CuratorJobStatus::Completed
                    };
                    update_curator_record(
                        &worker_state,
                        &worker_store,
                        &worker_refresh_lock,
                        &item.id,
                        status,
                        Some(merged),
                        Some(error.to_string()),
                    )
                }
            }
        });

        Self {
            state,
            store,
            tx: Some(tx),
            handle: Some(handle),
        }
    }

    pub(crate) fn snapshot(&self) -> Result<CuratorSnapshot> {
        let mut store = self.store.lock().expect("workspace store lock poisoned");
        let mut state = self.state.lock().expect("curator state lock poisoned");
        state.ensure_loaded(&mut *store)?;
        Ok(state.snapshot.clone())
    }

    pub(crate) fn enqueue_locked(
        &self,
        job: CuratorJob,
        store: &mut SqliteStore,
    ) -> Result<CuratorJobId> {
        CuratorHandleRef::from(self).enqueue_locked(job, store)
    }

    pub(crate) fn stop(&mut self) {
        if let Some(tx) = self.tx.take() {
            let _ = tx.send(CuratorMessage::Stop);
        }
        if let Some(handle) = self.handle.take() {
            let _ = handle.join();
        }
    }
}

impl CuratorHandleRef {
    pub(crate) fn enqueue_locked(
        &self,
        job: CuratorJob,
        store: &mut SqliteStore,
    ) -> Result<CuratorJobId> {
        let mut state = self.state.lock().expect("curator state lock poisoned");
        state.ensure_loaded(store)?;
        state.next_sequence += 1;
        let id = CuratorJobId(new_prefixed_id("curator").to_string());
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
            let _ = tx.send(CuratorMessage::Work(CuratorWorkItem {
                id: id.clone(),
                job,
            }));
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

pub(crate) fn update_curator_record(
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
    if let Ok(mut store) = store.lock() {
        let mut state = state.lock().expect("curator state lock poisoned");
        if state.ensure_loaded(&mut *store).is_err() {
            return;
        }
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
        let _ = store.commit_auxiliary_persist_batch(&AuxiliaryPersistBatch {
            curator_snapshot: Some(state.snapshot.clone()),
            ..AuxiliaryPersistBatch::default()
        });
    }
}

pub(crate) fn enqueue_curator_for_observed_locked(
    curator: &CuratorHandleRef,
    prism: &Prism,
    store: &mut SqliteStore,
    observed: &[prism_ir::ObservedChangeSet],
) -> Result<()> {
    for change in observed {
        if let Some((trigger, focus)) = curator_job_for_observed(change, prism) {
            let budget = CuratorBudget::default();
            let job = CuratorJob {
                id: CuratorJobId("pending".to_string()),
                trigger,
                task: change.meta.correlation.clone(),
                focus,
                budget,
            };
            let _ = curator.enqueue_locked(job, store)?;
        }
    }
    Ok(())
}

pub(crate) fn enqueue_curator_for_outcome_locked(
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
    let Some(trigger) = curator_trigger_for_outcome(prism, store, &event)? else {
        return Ok(());
    };
    let focus = dedupe_anchors(prism.anchors_for(&event.anchors));
    if focus.is_empty() {
        return Ok(());
    }
    let budget = CuratorBudget::default();
    let job = CuratorJob {
        id: CuratorJobId("curator:pending".to_string()),
        trigger,
        task: event.meta.correlation.clone(),
        focus,
        budget,
    };
    let _ = curator.enqueue_locked(job, store)?;
    Ok(())
}

impl CuratorQueueState {
    pub(crate) fn ensure_loaded(&mut self, store: &mut SqliteStore) -> Result<()> {
        if self.loaded {
            return Ok(());
        }
        self.snapshot = store.load_curator_snapshot()?.unwrap_or_default();
        self.next_sequence = next_curator_sequence(&self.snapshot);
        self.loaded = true;
        Ok(())
    }
}
