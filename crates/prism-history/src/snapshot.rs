use prism_ir::{LineageEvent, LineageId, NodeId, SymbolFingerprint};
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct HistorySnapshot {
    pub node_to_lineage: Vec<(NodeId, LineageId)>,
    pub events: Vec<LineageEvent>,
    #[serde(default)]
    pub co_change_counts: Vec<(LineageId, LineageId, u32)>,
    #[serde(default)]
    pub tombstones: Vec<LineageTombstone>,
    pub next_lineage: u64,
    pub next_event: u64,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct HistoryCoChangeDelta {
    pub source_lineage: LineageId,
    pub target_lineage: LineageId,
    pub count_delta: u32,
}

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct HistoryPersistDelta {
    pub removed_nodes: Vec<NodeId>,
    pub upserted_node_lineages: Vec<(NodeId, LineageId)>,
    pub appended_events: Vec<LineageEvent>,
    pub co_change_deltas: Vec<HistoryCoChangeDelta>,
    pub upserted_tombstones: Vec<LineageTombstone>,
    pub removed_tombstone_lineages: Vec<LineageId>,
    pub next_lineage: u64,
    pub next_event: u64,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct CoChangeNeighbor {
    pub lineage: LineageId,
    pub count: u32,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct LineageTombstone {
    pub lineage: LineageId,
    pub nodes: Vec<NodeId>,
    pub fingerprint: SymbolFingerprint,
}
