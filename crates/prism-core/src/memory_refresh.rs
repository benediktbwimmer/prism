use anyhow::Result;
use prism_ir::LineageEvent;
use prism_memory::{EpisodicMemorySnapshot, SessionMemory};
use prism_store::{AuxiliaryPersistBatch, Store};

pub(crate) fn reanchor_episodic_snapshot(
    snapshot: EpisodicMemorySnapshot,
    events: &[LineageEvent],
) -> Result<EpisodicMemorySnapshot> {
    if events.is_empty() {
        return Ok(snapshot);
    }

    let memory = SessionMemory::from_snapshot(snapshot);
    memory.apply_lineage_events(events)?;
    Ok(memory.snapshot())
}

pub(crate) fn reanchor_persisted_memory_snapshot<S: Store>(
    store: &mut S,
    events: &[LineageEvent],
) -> Result<()> {
    if events.is_empty() {
        return Ok(());
    }

    let Some(snapshot) = store.load_episodic_snapshot()? else {
        return Ok(());
    };

    store.commit_auxiliary_persist_batch(&AuxiliaryPersistBatch {
        episodic_snapshot: Some(reanchor_episodic_snapshot(snapshot, events)?),
        ..AuxiliaryPersistBatch::default()
    })?;
    Ok(())
}
