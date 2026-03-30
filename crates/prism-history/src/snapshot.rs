use prism_ir::{LineageEvent, LineageId, NodeId, SymbolFingerprint};
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct HistorySnapshot {
    pub node_to_lineage: Vec<(NodeId, LineageId)>,
    pub events: Vec<LineageEvent>,
    #[serde(default)]
    pub tombstones: Vec<LineageTombstone>,
    pub next_lineage: u64,
    pub next_event: u64,
}

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct HistoryPersistDelta {
    pub removed_nodes: Vec<NodeId>,
    pub upserted_node_lineages: Vec<(NodeId, LineageId)>,
    pub appended_events: Vec<LineageEvent>,
    pub upserted_tombstones: Vec<LineageTombstone>,
    pub removed_tombstone_lineages: Vec<LineageId>,
    pub next_lineage: u64,
    pub next_event: u64,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct LineageTombstone {
    pub lineage: LineageId,
    pub nodes: Vec<NodeId>,
    pub fingerprint: SymbolFingerprint,
}
