use anyhow::Result;
use prism_ir::LineageEvent;
use prism_memory::SessionMemory;
use prism_store::{AuxiliaryPersistBatch, Store};

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

    let memory = SessionMemory::from_snapshot(snapshot);
    memory.apply_lineage_events(events)?;
    store.commit_auxiliary_persist_batch(&AuxiliaryPersistBatch {
        episodic_snapshot: Some(memory.snapshot()),
        ..AuxiliaryPersistBatch::default()
    })?;
    Ok(())
}
