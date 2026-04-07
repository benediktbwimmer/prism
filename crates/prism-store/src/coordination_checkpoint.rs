use prism_coordination::{CoordinationSnapshot, CoordinationSnapshotV2, RuntimeDescriptor};
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct CoordinationStartupCheckpointAuthority {
    pub ref_name: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub head_commit: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub manifest_digest: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct CoordinationStartupCheckpoint {
    pub version: u32,
    pub materialized_at: u64,
    #[serde(default)]
    pub coordination_revision: u64,
    pub authority: CoordinationStartupCheckpointAuthority,
    pub snapshot: CoordinationSnapshot,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub canonical_snapshot_v2: Option<CoordinationSnapshotV2>,
    #[serde(default)]
    pub runtime_descriptors: Vec<RuntimeDescriptor>,
}

impl CoordinationStartupCheckpoint {
    pub const VERSION: u32 = 4;
}
