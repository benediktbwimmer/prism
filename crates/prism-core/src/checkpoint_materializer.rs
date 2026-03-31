use std::collections::BTreeMap;
use std::sync::mpsc::{self, RecvTimeoutError, SyncSender};
use std::sync::{Arc, Mutex};
use std::thread;
use std::time::Duration;

use anyhow::{anyhow, Result};
use prism_agent::InferenceSnapshot;
use prism_memory::{EpisodicMemorySnapshot, OutcomeMemorySnapshot};
use prism_projections::ValidationDelta;
use prism_store::{MaterializationStore, SqliteStore};
use tracing::warn;

const VALIDATION_COALESCE_WINDOW: Duration = Duration::from_millis(25);

pub(crate) struct CheckpointMaterializerHandle {
    tx: Option<mpsc::Sender<CheckpointMaterializerMessage>>,
    handle: Option<thread::JoinHandle<()>>,
}

#[derive(Default)]
struct PendingMaterializations {
    validation_deltas: Vec<ValidationDelta>,
    outcome_snapshot: Option<OutcomeMemorySnapshot>,
    episodic_snapshot: Option<EpisodicMemorySnapshot>,
    inference_snapshot: Option<InferenceSnapshot>,
}

enum CheckpointMaterializerMessage {
    ValidationDeltas(Vec<ValidationDelta>),
    OutcomeSnapshot(OutcomeMemorySnapshot),
    EpisodicSnapshot(EpisodicMemorySnapshot),
    InferenceSnapshot(InferenceSnapshot),
    Flush(SyncSender<Result<()>>),
    Stop,
}

impl CheckpointMaterializerHandle {
    pub(crate) fn new(store: Arc<Mutex<SqliteStore>>) -> Self {
        let (tx, rx) = mpsc::channel::<CheckpointMaterializerMessage>();
        let handle = thread::spawn(move || {
            let mut pending = PendingMaterializations::default();
            loop {
                let message = match rx.recv() {
                    Ok(message) => message,
                    Err(_) => {
                        flush_pending_materializations(&store, &mut pending);
                        break;
                    }
                };
                match message {
                    CheckpointMaterializerMessage::ValidationDeltas(deltas) => {
                        pending.validation_deltas.extend(deltas);
                    }
                    CheckpointMaterializerMessage::OutcomeSnapshot(snapshot) => {
                        pending.outcome_snapshot = Some(snapshot);
                    }
                    CheckpointMaterializerMessage::EpisodicSnapshot(snapshot) => {
                        pending.episodic_snapshot = Some(snapshot);
                    }
                    CheckpointMaterializerMessage::InferenceSnapshot(snapshot) => {
                        pending.inference_snapshot = Some(snapshot);
                    }
                    CheckpointMaterializerMessage::Flush(reply) => {
                        let result = flush_pending_materializations_result(&store, &mut pending);
                        let _ = reply.send(result);
                        continue;
                    }
                    CheckpointMaterializerMessage::Stop => {
                        flush_pending_materializations(&store, &mut pending);
                        break;
                    }
                }
                loop {
                    match rx.recv_timeout(VALIDATION_COALESCE_WINDOW) {
                        Ok(CheckpointMaterializerMessage::ValidationDeltas(deltas)) => {
                            pending.validation_deltas.extend(deltas);
                        }
                        Ok(CheckpointMaterializerMessage::OutcomeSnapshot(snapshot)) => {
                            pending.outcome_snapshot = Some(snapshot);
                        }
                        Ok(CheckpointMaterializerMessage::EpisodicSnapshot(snapshot)) => {
                            pending.episodic_snapshot = Some(snapshot);
                        }
                        Ok(CheckpointMaterializerMessage::InferenceSnapshot(snapshot)) => {
                            pending.inference_snapshot = Some(snapshot);
                        }
                        Ok(CheckpointMaterializerMessage::Flush(reply)) => {
                            let result =
                                flush_pending_materializations_result(&store, &mut pending);
                            let _ = reply.send(result);
                            break;
                        }
                        Ok(CheckpointMaterializerMessage::Stop) => {
                            flush_pending_materializations(&store, &mut pending);
                            return;
                        }
                        Err(RecvTimeoutError::Timeout) => {
                            flush_pending_materializations(&store, &mut pending);
                            break;
                        }
                        Err(RecvTimeoutError::Disconnected) => {
                            flush_pending_materializations(&store, &mut pending);
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

    pub(crate) fn enqueue_inference_snapshot(&self, snapshot: InferenceSnapshot) -> Result<()> {
        let Some(tx) = &self.tx else {
            return Err(anyhow!("checkpoint materializer is unavailable"));
        };
        tx.send(CheckpointMaterializerMessage::InferenceSnapshot(snapshot))
            .map_err(|_| anyhow!("checkpoint materializer dropped inference snapshot flush"))
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
        if let Some(tx) = self.tx.take() {
            let _ = tx.send(CheckpointMaterializerMessage::Stop);
        }
        if let Some(handle) = self.handle.take() {
            let _ = handle.join();
        }
    }
}

fn flush_pending_materializations(
    store: &Arc<Mutex<SqliteStore>>,
    pending: &mut PendingMaterializations,
) {
    if let Err(error) = flush_pending_materializations_result(store, pending) {
        warn!(error = %error, "checkpoint materializer flush failed");
    }
}

fn flush_pending_materializations_result(
    store: &Arc<Mutex<SqliteStore>>,
    pending: &mut PendingMaterializations,
) -> Result<()> {
    let validation_deltas = take_coalesced_validation_deltas(&mut pending.validation_deltas);
    let outcome_snapshot = pending.outcome_snapshot.take();
    let episodic_snapshot = pending.episodic_snapshot.take();
    let inference_snapshot = pending.inference_snapshot.take();
    if validation_deltas.is_empty()
        && outcome_snapshot.is_none()
        && episodic_snapshot.is_none()
        && inference_snapshot.is_none()
    {
        return Ok(());
    }
    let mut store = store.lock().expect("workspace store lock poisoned");
    if let Some(snapshot) = outcome_snapshot {
        store.save_outcome_snapshot(&snapshot)?;
    }
    if let Some(snapshot) = episodic_snapshot {
        store.save_episodic_snapshot(&snapshot)?;
    }
    if let Some(snapshot) = inference_snapshot {
        store.save_inference_snapshot(&snapshot)?;
    }
    store.apply_validation_deltas(&validation_deltas)
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
