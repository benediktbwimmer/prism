use std::collections::HashSet;
use std::sync::mpsc;
use std::sync::{Arc, Mutex};
use std::thread;
use std::time::Duration;

use anyhow::Result;
use prism_curator::{
    CuratorBackend, CuratorBudget, CuratorContext, CuratorGraphSlice, CuratorJob, CuratorJobId,
    CuratorJobRecord, CuratorJobStatus, CuratorLineageSlice, CuratorProjectionSlice,
    CuratorProposalState, CuratorRun, CuratorSnapshot, CuratorTrigger,
};
use prism_ir::{AnchorRef, EventId, Node, NodeId};
use prism_memory::{EpisodicMemorySnapshot, OutcomeEvent, OutcomeKind, OutcomeResult};
use prism_query::Prism;
use prism_store::{AuxiliaryPersistBatch, SqliteStore, Store};

use crate::patch_outcomes::{dedupe_anchors, observed_is_empty};
use crate::util::current_timestamp;

pub(crate) struct CuratorHandle {
    pub(crate) state: Arc<Mutex<CuratorQueueState>>,
    tx: Option<mpsc::Sender<CuratorWorkItem>>,
    stop: Option<mpsc::Sender<()>>,
    handle: Option<thread::JoinHandle<()>>,
}

#[derive(Clone)]
pub(crate) struct CuratorHandleRef {
    state: Arc<Mutex<CuratorQueueState>>,
    tx: Option<mpsc::Sender<CuratorWorkItem>>,
}

#[derive(Default)]
pub(crate) struct CuratorQueueState {
    pub(crate) snapshot: CuratorSnapshot,
    next_sequence: u64,
}

struct CuratorWorkItem {
    id: CuratorJobId,
    job: CuratorJob,
    context: CuratorContext,
}

impl CuratorHandle {
    pub(crate) fn new(
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

    pub(crate) fn snapshot(&self) -> CuratorSnapshot {
        self.state
            .lock()
            .expect("curator state lock poisoned")
            .snapshot
            .clone()
    }

    pub(crate) fn enqueue_locked(
        &self,
        job: CuratorJob,
        context: CuratorContext,
        store: &mut SqliteStore,
    ) -> Result<CuratorJobId> {
        CuratorHandleRef::from(self).enqueue_locked(job, context, store)
    }

    pub(crate) fn stop(&mut self) {
        if let Some(stop) = self.stop.take() {
            let _ = stop.send(());
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

pub(crate) fn enqueue_curator_for_observed_locked(
    curator: &CuratorHandleRef,
    prism: &Prism,
    store: &mut SqliteStore,
    observed: &[prism_ir::ObservedChangeSet],
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
    observed: &prism_ir::ObservedChangeSet,
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
