use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

use crate::{FileId, LineageId, NodeId, NodeKind};

#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize, JsonSchema)]
pub enum AnchorRef {
    Node(NodeId),
    Lineage(LineageId),
    File(FileId),
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
