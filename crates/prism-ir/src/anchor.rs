use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

use crate::{FileId, LineageId, NodeId, NodeKind};

#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize, JsonSchema)]
pub enum AnchorRef {
    Node(NodeId),
    Lineage(LineageId),
    File(FileId),
    WorkspacePath(String),
    Kind(NodeKind),
}

impl From<NodeId> for AnchorRef {
    fn from(value: NodeId) -> Self {
        Self::Node(value)
    }
}

impl From<&NodeId> for AnchorRef {
    fn from(value: &NodeId) -> Self {
        Self::Node(value.clone())
    }
}

impl AnchorRef {
    pub fn requires_graph_resolution(&self) -> bool {
        matches!(self, Self::Lineage(_) | Self::File(_))
    }

    pub fn matches_node_without_graph(&self, target: &NodeId) -> bool {
        match self {
            Self::Node(node) => node == target,
            Self::Kind(kind) => target.kind == *kind,
            Self::Lineage(_) | Self::File(_) | Self::WorkspacePath(_) => false,
        }
    }
}
