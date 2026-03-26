use serde::{Deserialize, Serialize};

use crate::{EventMeta, LineageId, NodeId};

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum LineageEventKind {
    Born,
    Updated,
    Renamed,
    Moved,
    Reparented,
    Split,
    Merged,
    Died,
    Revived,
    Ambiguous,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum LineageEvidence {
    ExactNodeId,
    FingerprintMatch,
    SignatureMatch,
    BodyHashMatch,
    SkeletonMatch,
    SameContainerLineage,
    GitRenameHint,
    FileMoveHint,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct LineageEvent {
    pub meta: EventMeta,
    pub lineage: LineageId,
    pub kind: LineageEventKind,
    pub before: Vec<NodeId>,
    pub after: Vec<NodeId>,
    pub confidence: f32,
    pub evidence: Vec<LineageEvidence>,
}
