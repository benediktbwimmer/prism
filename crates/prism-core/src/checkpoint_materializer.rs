use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::sync::mpsc::{self, RecvTimeoutError, SyncSender};
use std::sync::{Arc, Mutex};
use std::thread;
use std::time::Duration;

use anyhow::{anyhow, Result};
use prism_agent::InferenceSnapshot;
use prism_coordination::{
    coordination_queue_read_model_from_snapshot, coordination_read_model_from_snapshot,
    CoordinationSnapshot,
};
use prism_curator::CuratorSnapshot;
use prism_ir::LineageEvent;
use prism_memory::{EpisodicMemorySnapshot, OutcomeMemorySnapshot};
use prism_projections::{CoChangeDelta, ProjectionIndex, ProjectionSnapshot, ValidationDelta};
use prism_store::WorkspaceTreeSnapshot;
use prism_store::{
    AuxiliaryPersistBatch, CoordinationCheckpointStore, CoordinationJournal, Graph, GraphSnapshot,
    MaterializationStore,
};
use tracing::warn;

use crate::coordination_persistence::repo_semantic_coordination_snapshot;
use crate::coordination_startup_checkpoint::save_shared_coordination_startup_checkpoint;
use crate::memory_refresh::reanchor_episodic_snapshot;
use crate::published_plans::execution_overlays_by_plan;
use crate::tracked_snapshot::{sync_coordination_snapshot_state, TrackedSnapshotPublishContext};

const VALIDATION_COALESCE_WINDOW: Duration = Duration::from_millis(25);
const COORDINATION_COMPACTION_SUFFIX_THRESHOLD: usize = 128;

#[derive(Clone)]
pub(crate) struct CoordinationMaterialization {
    pub(crate) authoritative_revision: u64,
    pub(crate) snapshot: CoordinationSnapshot,
    pub(crate) publish_context: Option<TrackedSnapshotPublishContext>,
}

pub(crate) struct CheckpointMaterializerHandle {
    tx: Option<mpsc::Sender<CheckpointMaterializerMessage>>,
    handle: Option<thread::JoinHandle<()>>,
}

impl Clone for CheckpointMaterializerHandle {
    fn clone(&self) -> Self {
        Self {
            tx: self.tx.clone(),
            handle: None,
        }
    }
}

#[derive(Default)]
struct PendingMaterializations {
    co_change_deltas: Vec<CoChangeDelta>,
    validation_deltas: Vec<ValidationDelta>,
    coordination_materialization: Option<CoordinationMaterialization>,
    graph_snapshot: Option<GraphSnapshot>,
    projection_snapshot: Option<ProjectionSnapshot>,
    outcome_snapshot: Option<OutcomeMemorySnapshot>,
    episodic_snapshot: Option<EpisodicMemorySnapshot>,
    episodic_reanchor_events: Vec<LineageEvent>,
    inference_snapshot: Option<InferenceSnapshot>,
    curator_snapshot: Option<CuratorSnapshot>,
    workspace_tree_snapshot: Option<WorkspaceTreeSnapshot>,
}

enum CheckpointMaterializerMessage {
    CoordinationMaterialization(CoordinationMaterialization),
    GraphSnapshot(GraphSnapshot),
    ProjectionDeltas(Vec<CoChangeDelta>, Vec<ValidationDelta>),
    ProjectionSnapshot(ProjectionSnapshot),
    ValidationDeltas(Vec<ValidationDelta>),
    OutcomeSnapshot(OutcomeMemorySnapshot),
    EpisodicSnapshot(EpisodicMemorySnapshot),
    EpisodicReanchorEvents(Vec<LineageEvent>),
    InferenceSnapshot(InferenceSnapshot),
    CuratorSnapshot(CuratorSnapshot),
    WorkspaceTreeSnapshot(WorkspaceTreeSnapshot),
    Flush(SyncSender<Result<()>>),
    Stop,
}

impl CheckpointMaterializerHandle {
    pub(crate) fn new<T>(root: PathBuf, store: Arc<Mutex<T>>) -> Self
    where
        T: CoordinationJournal
            + CoordinationCheckpointStore
            + MaterializationStore
            + Send
            + 'static,
    {
        let (tx, rx) = mpsc::channel::<CheckpointMaterializerMessage>();
        let handle = thread::spawn(move || {
            let mut pending = PendingMaterializations::default();
            loop {
                let message = match rx.recv() {
                    Ok(message) => message,
                    Err(_) => {
                        flush_pending_materializations(&root, &store, &mut pending);
                        break;
                    }
                };
                match message {
                    CheckpointMaterializerMessage::CoordinationMaterialization(materialization) => {
                        pending.coordination_materialization = Some(materialization);
                    }
                    CheckpointMaterializerMessage::GraphSnapshot(snapshot) => {
                        pending.graph_snapshot = Some(snapshot);
                    }
                    CheckpointMaterializerMessage::ProjectionDeltas(
                        co_change_deltas,
                        validation_deltas,
                    ) => {
                        pending.co_change_deltas.extend(co_change_deltas);
                        pending.validation_deltas.extend(validation_deltas);
                    }
                    CheckpointMaterializerMessage::ProjectionSnapshot(snapshot) => {
                        pending.projection_snapshot = Some(snapshot);
                    }
                    CheckpointMaterializerMessage::ValidationDeltas(deltas) => {
                        pending.validation_deltas.extend(deltas);
                    }
                    CheckpointMaterializerMessage::OutcomeSnapshot(snapshot) => {
                        pending.outcome_snapshot = Some(snapshot);
                    }
                    CheckpointMaterializerMessage::EpisodicSnapshot(snapshot) => {
                        pending.episodic_snapshot = Some(snapshot);
                    }
                    CheckpointMaterializerMessage::EpisodicReanchorEvents(events) => {
                        pending.episodic_reanchor_events.extend(events);
                    }
                    CheckpointMaterializerMessage::InferenceSnapshot(snapshot) => {
                        pending.inference_snapshot = Some(snapshot);
                    }
                    CheckpointMaterializerMessage::CuratorSnapshot(snapshot) => {
                        pending.curator_snapshot = Some(snapshot);
                    }
                    CheckpointMaterializerMessage::WorkspaceTreeSnapshot(snapshot) => {
                        pending.workspace_tree_snapshot = Some(snapshot);
                    }
                    CheckpointMaterializerMessage::Flush(reply) => {
                        let result =
                            flush_pending_materializations_result(&root, &store, &mut pending);
                        let _ = reply.send(result);
                        continue;
                    }
                    CheckpointMaterializerMessage::Stop => {
                        flush_pending_materializations(&root, &store, &mut pending);
                        break;
                    }
                }
                loop {
                    match rx.recv_timeout(VALIDATION_COALESCE_WINDOW) {
                        Ok(CheckpointMaterializerMessage::CoordinationMaterialization(
                            materialization,
                        )) => {
                            pending.coordination_materialization = Some(materialization);
                        }
                        Ok(CheckpointMaterializerMessage::GraphSnapshot(snapshot)) => {
                            pending.graph_snapshot = Some(snapshot);
                        }
                        Ok(CheckpointMaterializerMessage::ProjectionDeltas(
                            co_change_deltas,
                            validation_deltas,
                        )) => {
                            pending.co_change_deltas.extend(co_change_deltas);
                            pending.validation_deltas.extend(validation_deltas);
                        }
                        Ok(CheckpointMaterializerMessage::ProjectionSnapshot(snapshot)) => {
                            pending.projection_snapshot = Some(snapshot);
                        }
                        Ok(CheckpointMaterializerMessage::ValidationDeltas(deltas)) => {
                            pending.validation_deltas.extend(deltas);
                        }
                        Ok(CheckpointMaterializerMessage::OutcomeSnapshot(snapshot)) => {
                            pending.outcome_snapshot = Some(snapshot);
                        }
                        Ok(CheckpointMaterializerMessage::EpisodicSnapshot(snapshot)) => {
                            pending.episodic_snapshot = Some(snapshot);
                        }
                        Ok(CheckpointMaterializerMessage::EpisodicReanchorEvents(events)) => {
                            pending.episodic_reanchor_events.extend(events);
                        }
                        Ok(CheckpointMaterializerMessage::InferenceSnapshot(snapshot)) => {
                            pending.inference_snapshot = Some(snapshot);
                        }
                        Ok(CheckpointMaterializerMessage::CuratorSnapshot(snapshot)) => {
                            pending.curator_snapshot = Some(snapshot);
                        }
                        Ok(CheckpointMaterializerMessage::WorkspaceTreeSnapshot(snapshot)) => {
                            pending.workspace_tree_snapshot = Some(snapshot);
                        }
                        Ok(CheckpointMaterializerMessage::Flush(reply)) => {
                            let result =
                                flush_pending_materializations_result(&root, &store, &mut pending);
                            let _ = reply.send(result);
                            break;
                        }
                        Ok(CheckpointMaterializerMessage::Stop) => {
                            flush_pending_materializations(&root, &store, &mut pending);
                            return;
                        }
                        Err(RecvTimeoutError::Timeout) => {
                            flush_pending_materializations(&root, &store, &mut pending);
                            break;
                        }
                        Err(RecvTimeoutError::Disconnected) => {
                            flush_pending_materializations(&root, &store, &mut pending);
                            return;
                        }
                    }
                }
            }
        });
        Self {
            tx: Some(tx),
            handle: Some(handle),
        }
    }

    pub(crate) fn enqueue_validation_deltas(&self, deltas: Vec<ValidationDelta>) -> Result<()> {
        if deltas.is_empty() {
            return Ok(());
        }
        let Some(tx) = &self.tx else {
            return Err(anyhow!("checkpoint materializer is unavailable"));
        };
        tx.send(CheckpointMaterializerMessage::ValidationDeltas(deltas))
            .map_err(|_| anyhow!("checkpoint materializer dropped validation delta flush"))
    }

    pub(crate) fn enqueue_coordination_materialization(
        &self,
        materialization: CoordinationMaterialization,
    ) -> Result<()> {
        let Some(tx) = &self.tx else {
            return Err(anyhow!("checkpoint materializer is unavailable"));
        };
        tx.send(CheckpointMaterializerMessage::CoordinationMaterialization(
            materialization,
        ))
        .map_err(|_| anyhow!("checkpoint materializer dropped coordination materialization flush"))
    }

    pub(crate) fn enqueue_graph_snapshot(&self, snapshot: GraphSnapshot) -> Result<()> {
        let Some(tx) = &self.tx else {
            return Err(anyhow!("checkpoint materializer is unavailable"));
        };
        tx.send(CheckpointMaterializerMessage::GraphSnapshot(snapshot))
            .map_err(|_| anyhow!("checkpoint materializer dropped graph snapshot flush"))
    }

    pub(crate) fn enqueue_projection_deltas(
        &self,
        co_change_deltas: Vec<CoChangeDelta>,
        validation_deltas: Vec<ValidationDelta>,
    ) -> Result<()> {
        if co_change_deltas.is_empty() && validation_deltas.is_empty() {
            return Ok(());
        }
        let Some(tx) = &self.tx else {
            return Err(anyhow!("checkpoint materializer is unavailable"));
        };
        tx.send(CheckpointMaterializerMessage::ProjectionDeltas(
            co_change_deltas,
            validation_deltas,
        ))
        .map_err(|_| anyhow!("checkpoint materializer dropped projection delta flush"))
    }

    pub(crate) fn enqueue_projection_snapshot(&self, snapshot: ProjectionSnapshot) -> Result<()> {
        let Some(tx) = &self.tx else {
            return Err(anyhow!("checkpoint materializer is unavailable"));
        };
        tx.send(CheckpointMaterializerMessage::ProjectionSnapshot(snapshot))
            .map_err(|_| anyhow!("checkpoint materializer dropped projection snapshot flush"))
    }

    pub(crate) fn enqueue_episodic_snapshot(&self, snapshot: EpisodicMemorySnapshot) -> Result<()> {
        let Some(tx) = &self.tx else {
            return Err(anyhow!("checkpoint materializer is unavailable"));
        };
        tx.send(CheckpointMaterializerMessage::EpisodicSnapshot(snapshot))
            .map_err(|_| anyhow!("checkpoint materializer dropped episodic snapshot flush"))
    }

    pub(crate) fn enqueue_outcome_snapshot(&self, snapshot: OutcomeMemorySnapshot) -> Result<()> {
        let Some(tx) = &self.tx else {
            return Err(anyhow!("checkpoint materializer is unavailable"));
        };
        tx.send(CheckpointMaterializerMessage::OutcomeSnapshot(snapshot))
            .map_err(|_| anyhow!("checkpoint materializer dropped outcome snapshot flush"))
    }

    pub(crate) fn enqueue_episodic_reanchor_events(&self, events: Vec<LineageEvent>) -> Result<()> {
        if events.is_empty() {
            return Ok(());
        }
        let Some(tx) = &self.tx else {
            return Err(anyhow!("checkpoint materializer is unavailable"));
        };
        tx.send(CheckpointMaterializerMessage::EpisodicReanchorEvents(
            events,
        ))
        .map_err(|_| anyhow!("checkpoint materializer dropped episodic reanchor flush"))
    }

    pub(crate) fn enqueue_inference_snapshot(&self, snapshot: InferenceSnapshot) -> Result<()> {
        let Some(tx) = &self.tx else {
            return Err(anyhow!("checkpoint materializer is unavailable"));
        };
        tx.send(CheckpointMaterializerMessage::InferenceSnapshot(snapshot))
            .map_err(|_| anyhow!("checkpoint materializer dropped inference snapshot flush"))
    }

    pub(crate) fn enqueue_curator_snapshot(&self, snapshot: CuratorSnapshot) -> Result<()> {
        let Some(tx) = &self.tx else {
            return Err(anyhow!("checkpoint materializer is unavailable"));
        };
        tx.send(CheckpointMaterializerMessage::CuratorSnapshot(snapshot))
            .map_err(|_| anyhow!("checkpoint materializer dropped curator snapshot flush"))
    }

    pub(crate) fn enqueue_workspace_tree_snapshot(
        &self,
        snapshot: WorkspaceTreeSnapshot,
    ) -> Result<()> {
        let Some(tx) = &self.tx else {
            return Err(anyhow!("checkpoint materializer is unavailable"));
        };
        tx.send(CheckpointMaterializerMessage::WorkspaceTreeSnapshot(
            snapshot,
        ))
        .map_err(|_| anyhow!("checkpoint materializer dropped workspace tree snapshot flush"))
    }

    pub(crate) fn flush(&self) -> Result<()> {
        let Some(tx) = &self.tx else {
            return Ok(());
        };
        let (reply_tx, reply_rx) = mpsc::sync_channel::<Result<()>>(1);
        tx.send(CheckpointMaterializerMessage::Flush(reply_tx))
            .map_err(|_| anyhow!("checkpoint materializer is unavailable"))?;
        reply_rx
            .recv()
            .map_err(|_| anyhow!("checkpoint materializer dropped flush response"))?
    }

    pub(crate) fn stop(&mut self) {
        let _ = self.flush();
        if let Some(tx) = self.tx.take() {
            let _ = tx.send(CheckpointMaterializerMessage::Stop);
        }
        if let Some(handle) = self.handle.take() {
            let _ = handle.join();
        }
    }
}

fn flush_pending_materializations<T>(
    root: &Path,
    store: &Arc<Mutex<T>>,
    pending: &mut PendingMaterializations,
) where
    T: CoordinationJournal + CoordinationCheckpointStore + MaterializationStore + Send + 'static,
{
    if let Err(error) = flush_pending_materializations_result(root, store, pending) {
        warn!(error = %error, "checkpoint materializer flush failed");
    }
}

fn flush_pending_materializations_result<T>(
    root: &Path,
    store_handle: &Arc<Mutex<T>>,
    pending: &mut PendingMaterializations,
) -> Result<()>
where
    T: CoordinationJournal + CoordinationCheckpointStore + MaterializationStore + Send + 'static,
{
    let co_change_deltas = std::mem::take(&mut pending.co_change_deltas);
    let validation_deltas = take_coalesced_validation_deltas(&mut pending.validation_deltas);
    let coordination_materialization = pending.coordination_materialization.take();
    let graph_snapshot = pending.graph_snapshot.take();
    let projection_snapshot = pending.projection_snapshot.take();
    let outcome_snapshot = pending.outcome_snapshot.take();
    let episodic_snapshot = pending.episodic_snapshot.take();
    let episodic_reanchor_events = std::mem::take(&mut pending.episodic_reanchor_events);
    let inference_snapshot = pending.inference_snapshot.take();
    let curator_snapshot = pending.curator_snapshot.take();
    let workspace_tree_snapshot = pending.workspace_tree_snapshot.take();
    if co_change_deltas.is_empty()
        && validation_deltas.is_empty()
        && coordination_materialization.is_none()
        && graph_snapshot.is_none()
        && projection_snapshot.is_none()
        && outcome_snapshot.is_none()
        && episodic_snapshot.is_none()
        && episodic_reanchor_events.is_empty()
        && inference_snapshot.is_none()
        && curator_snapshot.is_none()
        && workspace_tree_snapshot.is_none()
    {
        return Ok(());
    }
    let mut store = store_handle.lock().expect("workspace store lock poisoned");
    if let Some(materialization) = coordination_materialization {
        persist_coordination_materialization(root, &mut *store, &materialization)?;
    }
    if let Some(snapshot) = graph_snapshot {
        store.save_graph_snapshot(&Graph::from_snapshot(snapshot))?;
    }
    if let Some(snapshot) = projection_snapshot {
        if co_change_deltas.is_empty() && validation_deltas.is_empty() {
            store.save_projection_snapshot(&snapshot)?;
        } else {
            let mut index = ProjectionIndex::from_snapshot(snapshot);
            index.apply_co_change_deltas(&co_change_deltas);
            index.apply_validation_deltas(&validation_deltas);
            store.save_projection_snapshot(&index.snapshot())?;
        }
    } else if !co_change_deltas.is_empty() || !validation_deltas.is_empty() {
        store.apply_projection_deltas(&co_change_deltas, &validation_deltas)?;
    }
    let episodic_snapshot = if let Some(snapshot) = episodic_snapshot {
        Some(reanchor_episodic_snapshot(
            snapshot,
            &episodic_reanchor_events,
        )?)
    } else if !episodic_reanchor_events.is_empty() {
        store
            .load_episodic_snapshot()?
            .map(|snapshot| reanchor_episodic_snapshot(snapshot, &episodic_reanchor_events))
            .transpose()?
    } else {
        None
    };
    if outcome_snapshot.is_some()
        || episodic_snapshot.is_some()
        || inference_snapshot.is_some()
        || curator_snapshot.is_some()
        || !validation_deltas.is_empty()
    {
        store.commit_auxiliary_persist_batch(&AuxiliaryPersistBatch {
            outcome_snapshot,
            validation_deltas,
            episodic_snapshot,
            inference_snapshot,
            curator_snapshot,
            ..AuxiliaryPersistBatch::default()
        })?;
    }
    if let Some(snapshot) = workspace_tree_snapshot {
        store.save_workspace_tree_snapshot(&snapshot)?;
    }
    Ok(())
}

pub(crate) fn persist_coordination_materialization<T>(
    root: &Path,
    store: &mut T,
    materialization: &CoordinationMaterialization,
) -> Result<()>
where
    T: CoordinationJournal + CoordinationCheckpointStore + ?Sized,
{
    let mut read_model = coordination_read_model_from_snapshot(&materialization.snapshot);
    read_model.revision = materialization.authoritative_revision;
    let mut queue_model = coordination_queue_read_model_from_snapshot(&materialization.snapshot);
    queue_model.revision = materialization.authoritative_revision;
    store.save_coordination_read_model(&read_model)?;
    store.save_coordination_queue_read_model(&queue_model)?;
    if store.load_coordination_event_stream()?.suffix_events.len()
        >= COORDINATION_COMPACTION_SUFFIX_THRESHOLD
    {
        store.save_coordination_compaction(&materialization.snapshot)?;
    }
    let repo_semantic_snapshot = repo_semantic_coordination_snapshot(materialization.snapshot.clone());
    let plan_graphs = prism_coordination::snapshot_plan_graphs(&repo_semantic_snapshot);
    let repo_semantic_execution_overlays =
        execution_overlays_by_plan(&repo_semantic_snapshot.tasks);
    sync_coordination_snapshot_state(
        root,
        &repo_semantic_snapshot,
        &plan_graphs,
        &repo_semantic_execution_overlays,
        materialization.publish_context.as_ref(),
        Some(materialization.authoritative_revision),
    )?;
    save_shared_coordination_startup_checkpoint(
        root,
        store,
        &repo_semantic_snapshot,
        &[],
    )?;
    Ok(())
}

fn take_coalesced_validation_deltas(deltas: &mut Vec<ValidationDelta>) -> Vec<ValidationDelta> {
    let pending = std::mem::take(deltas);
    let mut merged = BTreeMap::<(String, String), ValidationDelta>::new();
    for delta in pending {
        let key = (delta.lineage.0.to_string(), delta.label.clone());
        match merged.get_mut(&key) {
            Some(existing) => {
                existing.score_delta += delta.score_delta;
                existing.last_seen = existing.last_seen.max(delta.last_seen);
            }
            None => {
                merged.insert(key, delta);
            }
        }
    }
    merged.into_values().collect()
}
