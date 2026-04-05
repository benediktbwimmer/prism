use std::path::Path;

use anyhow::Result;
use prism_ir::new_prefixed_id;
use serde::{Deserialize, Serialize};

use crate::util::current_timestamp_millis;
use crate::workspace_identity::WorkspaceIdentity;

pub(crate) const WORKTREE_METADATA_VERSION: u32 = 2;

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
pub enum WorktreeMode {
    Human,
    Agent,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct WorktreeRegistrationRecord {
    pub worktree_id: String,
    pub agent_label: String,
    pub mode: WorktreeMode,
    pub registered_at: u64,
    pub last_registered_at: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub(crate) struct WorktreeMetadata {
    pub(crate) version: u32,
    pub(crate) repo_id: String,
    pub(crate) worktree_id: String,
    pub(crate) canonical_root: String,
    pub(crate) branch_ref: Option<String>,
    pub(crate) created_at: u64,
    pub(crate) last_seen_at: u64,
    #[serde(default)]
    pub(crate) registered_worktree_id: Option<String>,
    #[serde(default)]
    pub(crate) agent_label: Option<String>,
    #[serde(default)]
    pub(crate) worktree_mode: Option<WorktreeMode>,
    #[serde(default)]
    pub(crate) registered_at: Option<u64>,
    #[serde(default)]
    pub(crate) last_registered_at: Option<u64>,
}

impl WorktreeMetadata {
    pub(crate) fn from_identity(
        identity: &WorkspaceIdentity,
        existing: Option<&Self>,
        now: u64,
    ) -> Self {
        let registration = identity.worktree_registration();
        Self {
            version: WORKTREE_METADATA_VERSION,
            repo_id: identity.repo_id.clone(),
            worktree_id: identity.storage_worktree_id.clone(),
            canonical_root: identity.canonical_root.to_string_lossy().to_string(),
            branch_ref: identity.branch_ref.clone(),
            created_at: existing.map_or(now, |metadata| metadata.created_at),
            last_seen_at: now,
            registered_worktree_id: registration
                .as_ref()
                .map(|record| record.worktree_id.clone()),
            agent_label: registration
                .as_ref()
                .map(|record| record.agent_label.clone()),
            worktree_mode: registration.as_ref().map(|record| record.mode),
            registered_at: registration.as_ref().map(|record| record.registered_at),
            last_registered_at: registration
                .as_ref()
                .map(|record| record.last_registered_at),
        }
    }

    pub(crate) fn apply_to_identity(&self, identity: &mut WorkspaceIdentity) {
        if let Some(registration) = self.registration() {
            identity.apply_worktree_registration(registration);
        }
    }

    pub(crate) fn registration(&self) -> Option<WorktreeRegistrationRecord> {
        Some(WorktreeRegistrationRecord {
            worktree_id: self.registered_worktree_id.clone()?,
            agent_label: self.agent_label.clone()?,
            mode: self.worktree_mode?,
            registered_at: self.registered_at?,
            last_registered_at: self.last_registered_at?,
        })
    }
}

pub(crate) fn read_worktree_metadata(path: &Path) -> Result<Option<WorktreeMetadata>> {
    if !path.exists() {
        return Ok(None);
    }
    let bytes = std::fs::read(path)?;
    Ok(Some(serde_json::from_slice(&bytes)?))
}

pub(crate) fn new_worktree_registration(
    label: &str,
    mode: WorktreeMode,
    existing: Option<&WorktreeRegistrationRecord>,
) -> WorktreeRegistrationRecord {
    let now = current_timestamp_millis();
    WorktreeRegistrationRecord {
        worktree_id: existing
            .map(|record| record.worktree_id.clone())
            .unwrap_or_else(|| new_prefixed_id("worktree").to_string()),
        agent_label: label.to_string(),
        mode,
        registered_at: existing.map_or(now, |record| record.registered_at),
        last_registered_at: now,
    }
}
