use prism_ir::{LineageEvent, LineageId, NodeId, SymbolFingerprint};
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
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
