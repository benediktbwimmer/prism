use std::sync::{Arc, Mutex};

use anyhow::Result;
use prism_history::HistorySnapshot;
use prism_ir::{LineageEvent, LineageId};
use prism_query::HistoryReadBackend;
use prism_store::ColdQueryStore;

#[allow(dead_code)]
pub(crate) struct StoreHistoryReadBackend<S> {
    store: Arc<Mutex<S>>,
}

impl<S> StoreHistoryReadBackend<S> {
    #[allow(dead_code)]
    pub(crate) fn new(store: Arc<Mutex<S>>) -> Self {
        Self { store }
    }
}

impl<S> HistoryReadBackend for StoreHistoryReadBackend<S>
where
    S: ColdQueryStore + Send,
{
    fn load_lineage_history(&self, lineage: &LineageId) -> Result<Vec<LineageEvent>> {
        self.store
            .lock()
            .expect("workspace store lock poisoned")
            .load_lineage_history(lineage)
    }

    fn load_history_snapshot(&self) -> Result<Option<HistorySnapshot>> {
        self.store
            .lock()
            .expect("workspace store lock poisoned")
            .load_history_snapshot_with_options(true)
    }
}
