use std::collections::BTreeMap;

use prism_coordination::CoordinationSnapshot;
use prism_ir::{PlanExecutionOverlay, PlanGraph};
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
    pub authority: CoordinationStartupCheckpointAuthority,
    pub snapshot: CoordinationSnapshot,
    pub plan_graphs: Vec<PlanGraph>,
    pub execution_overlays: BTreeMap<String, Vec<PlanExecutionOverlay>>,
}

impl CoordinationStartupCheckpoint {
    pub const VERSION: u32 = 1;
}
