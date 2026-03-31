use std::sync::{Arc, Mutex};

use anyhow::Result;
use prism_history::HistorySnapshot;
use prism_ir::{LineageEvent, LineageId};
use prism_query::HistoryReadBackend;
use prism_store::{SqliteStore, Store};

pub(crate) struct StoreHistoryReadBackend {
    store: Arc<Mutex<SqliteStore>>,
}

impl StoreHistoryReadBackend {
    pub(crate) fn new(store: Arc<Mutex<SqliteStore>>) -> Self {
        Self { store }
    }
}

impl HistoryReadBackend for StoreHistoryReadBackend {
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
