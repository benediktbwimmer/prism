use serde::{Deserialize, Serialize};
use smol_str::SmolStr;

use crate::{Edge, EventMeta, FileId, Node, NodeId, SymbolFingerprint};

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum GraphChange {
    Added(NodeId),
    Removed(NodeId),
    Modified(NodeId),
    Reanchored { old: NodeId, new: NodeId },
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ObservedNode {
    pub node: Node,
    pub fingerprint: SymbolFingerprint,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum ChangeTrigger {
    ManualReindex,
    FsWatch,
    AgentEdit,
    UserEdit,
    GitCheckout,
    GitCommitImport,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ObservedChangeSet {
    pub meta: EventMeta,
    pub trigger: ChangeTrigger,
    pub files: Vec<FileId>,
    #[serde(default)]
    pub previous_path: Option<SmolStr>,
    #[serde(default)]
    pub current_path: Option<SmolStr>,
    pub added: Vec<ObservedNode>,
    pub removed: Vec<ObservedNode>,
    pub updated: Vec<(ObservedNode, ObservedNode)>,
    pub edge_added: Vec<Edge>,
    pub edge_removed: Vec<Edge>,
}
