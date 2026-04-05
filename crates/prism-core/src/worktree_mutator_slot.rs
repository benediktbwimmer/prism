use std::error::Error;
use std::fmt;
use std::fs::{self, File, OpenOptions};
use std::io::ErrorKind;
use std::path::{Path, PathBuf};
use std::thread;
use std::time::Duration;

use anyhow::{anyhow, Context, Result};
use prism_ir::{PrincipalKind, SessionId};
use serde::{Deserialize, Serialize};

use crate::util::current_timestamp_millis;
use crate::workspace_identity::WorkspaceIdentity;
use crate::worktree_principal::BoundWorktreePrincipal;
use crate::{AuthenticatedPrincipal, PrismPaths, WorkspaceSession, WorktreeMode};

const WORKTREE_MUTATOR_SLOT_LOCK_FILE_NAME: &str = "mutator-slot.lock";
const WORKTREE_MUTATOR_SLOT_FILE_VERSION: u32 = 1;
pub const WORKTREE_MUTATOR_SLOT_STALE_AFTER_MS: u64 = 60_000;
const WORKTREE_MUTATOR_SLOT_LOCK_WAIT_MS: u64 = 2_000;
const WORKTREE_MUTATOR_SLOT_LOCK_RETRY_MS: u64 = 10;

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct WorktreeMutatorSlotRecord {
    pub version: u32,
    pub storage_worktree_id: String,
    pub worktree_id: String,
    pub agent_label: Option<String>,
    pub worktree_mode: Option<WorktreeMode>,
    pub session_id: String,
    pub authority_id: String,
    pub principal_id: String,
    pub principal_name: String,
    pub principal_kind: PrincipalKind,
    pub credential_id: String,
    pub acquired_at: u64,
    pub last_heartbeat_at: u64,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub takeover_reason: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct WorktreeMutatorSlotConflict {
    pub worktree_id: String,
    pub current_owner: WorktreeMutatorSlotRecord,
    pub attempted_session_id: String,
    pub attempted_principal: BoundWorktreePrincipal,
    pub stale_at: u64,
}

#[derive(Debug)]
pub enum WorktreeMutatorSlotError {
    Conflict(WorktreeMutatorSlotConflict),
    TakeoverRequiresHuman {
        principal_id: String,
        principal_kind: PrincipalKind,
    },
    Storage(anyhow::Error),
}

impl fmt::Display for WorktreeMutatorSlotConflict {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            f,
            "worktree `{}` already has active mutator session `{}` owned by principal `{}` (`{}`); slot becomes stale at {}",
            self.worktree_id,
            self.current_owner.session_id,
            self.current_owner.principal_id,
            self.current_owner.principal_name,
            self.stale_at,
        )
    }
}

impl Error for WorktreeMutatorSlotConflict {}

impl fmt::Display for WorktreeMutatorSlotError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Conflict(conflict) => conflict.fmt(f),
            Self::TakeoverRequiresHuman {
                principal_id,
                principal_kind,
            } => write!(
                f,
                "principal `{principal_id}` with kind `{:?}` cannot authorize worktree mutator takeover",
                principal_kind
            ),
            Self::Storage(error) => error.fmt(f),
        }
    }
}

impl Error for WorktreeMutatorSlotError {
    fn source(&self) -> Option<&(dyn Error + 'static)> {
        match self {
            Self::Conflict(conflict) => Some(conflict),
            Self::TakeoverRequiresHuman { .. } => None,
            Self::Storage(error) => Some(error.root_cause()),
        }
    }
}

impl From<anyhow::Error> for WorktreeMutatorSlotError {
    fn from(value: anyhow::Error) -> Self {
        Self::Storage(value)
    }
}

impl WorktreeMutatorSlotRecord {
    fn from_worktree_executor(
        identity: &WorkspaceIdentity,
        session_id: &SessionId,
        now: u64,
    ) -> Self {
        let principal_id = identity.worktree_id.clone();
        Self {
            version: WORKTREE_MUTATOR_SLOT_FILE_VERSION,
            storage_worktree_id: identity.storage_worktree_id.clone(),
            worktree_id: identity.worktree_id.clone(),
            agent_label: identity.agent_label.clone(),
            worktree_mode: identity.worktree_mode,
            session_id: session_id.0.to_string(),
            authority_id: "worktree_executor".to_string(),
            principal_id: principal_id.clone(),
            principal_name: identity
                .agent_label
                .clone()
                .unwrap_or_else(|| principal_id.clone()),
            principal_kind: PrincipalKind::Agent,
            credential_id: format!("worktree-executor:{principal_id}"),
            acquired_at: now,
            last_heartbeat_at: now,
            takeover_reason: None,
        }
    }

    fn from_authenticated(
        identity: &WorkspaceIdentity,
        authenticated: &AuthenticatedPrincipal,
        session_id: &SessionId,
        now: u64,
    ) -> Self {
        Self {
            version: WORKTREE_MUTATOR_SLOT_FILE_VERSION,
            storage_worktree_id: identity.storage_worktree_id.clone(),
            worktree_id: identity.worktree_id.clone(),
            agent_label: identity.agent_label.clone(),
            worktree_mode: identity.worktree_mode,
            session_id: session_id.0.to_string(),
            authority_id: authenticated.principal.authority_id.0.to_string(),
            principal_id: authenticated.principal.principal_id.0.to_string(),
            principal_name: authenticated.principal.name.clone(),
            principal_kind: authenticated.principal.kind,
            credential_id: authenticated.credential.credential_id.0.to_string(),
            acquired_at: now,
            last_heartbeat_at: now,
            takeover_reason: None,
        }
    }

    pub fn stale_at(&self) -> u64 {
        self.last_heartbeat_at
            .saturating_add(WORKTREE_MUTATOR_SLOT_STALE_AFTER_MS)
    }

    pub fn is_stale_at(&self, now: u64) -> bool {
        now >= self.stale_at()
    }

    pub fn bound_principal(&self) -> BoundWorktreePrincipal {
        BoundWorktreePrincipal {
            authority_id: self.authority_id.clone(),
            principal_id: self.principal_id.clone(),
            principal_name: self.principal_name.clone(),
        }
    }
}

impl WorkspaceSession {
    pub fn acquire_or_refresh_agent_worktree_mutator_slot(
        &self,
        session_id: &SessionId,
    ) -> std::result::Result<WorktreeMutatorSlotRecord, WorktreeMutatorSlotError> {
        let paths = PrismPaths::for_workspace_root(&self.root)?;
        let now = current_timestamp_millis();
        let attempted =
            WorktreeMutatorSlotRecord::from_worktree_executor(paths.identity(), session_id, now);
        let _lock = WorktreeMutatorSlotFileLock::acquire(&paths)?;
        let current = load_worktree_mutator_slot(&paths)?;
        let next = match current {
            Some(mut current)
                if current.worktree_id == attempted.worktree_id
                    && current.authority_id == attempted.authority_id
                    && current.principal_id == attempted.principal_id =>
            {
                current.session_id = attempted.session_id.clone();
                current.principal_name = attempted.principal_name.clone();
                current.principal_kind = attempted.principal_kind;
                current.credential_id = attempted.credential_id.clone();
                current.agent_label = attempted.agent_label.clone();
                current.worktree_mode = attempted.worktree_mode;
                current.last_heartbeat_at = now;
                current
            }
            Some(current) if !current.is_stale_at(now) => {
                return Err(WorktreeMutatorSlotError::Conflict(
                    WorktreeMutatorSlotConflict {
                        worktree_id: current.worktree_id.clone(),
                        stale_at: current.stale_at(),
                        current_owner: current,
                        attempted_session_id: attempted.session_id.clone(),
                        attempted_principal: attempted.bound_principal(),
                    },
                ));
            }
            Some(mut current) => {
                current.version = WORKTREE_MUTATOR_SLOT_FILE_VERSION;
                current.storage_worktree_id = attempted.storage_worktree_id.clone();
                current.worktree_id = attempted.worktree_id.clone();
                current.agent_label = attempted.agent_label.clone();
                current.worktree_mode = attempted.worktree_mode;
                current.session_id = attempted.session_id.clone();
                current.authority_id = attempted.authority_id.clone();
                current.principal_id = attempted.principal_id.clone();
                current.principal_name = attempted.principal_name.clone();
                current.principal_kind = attempted.principal_kind;
                current.credential_id = attempted.credential_id.clone();
                current.acquired_at = now;
                current.last_heartbeat_at = now;
                current
            }
            None => attempted,
        };
        save_worktree_mutator_slot(&paths, &next)?;
        self.update_cached_worktree_mutator_slot(Some(next.clone()));
        Ok(next)
    }

    pub fn acquire_or_refresh_worktree_mutator_slot(
        &self,
        authenticated: &AuthenticatedPrincipal,
        session_id: &SessionId,
    ) -> std::result::Result<WorktreeMutatorSlotRecord, WorktreeMutatorSlotError> {
        let paths = PrismPaths::for_workspace_root(&self.root)?;
        let now = current_timestamp_millis();
        let attempted = WorktreeMutatorSlotRecord::from_authenticated(
            paths.identity(),
            authenticated,
            session_id,
            now,
        );
        let _lock = WorktreeMutatorSlotFileLock::acquire(&paths)?;
        let current = load_worktree_mutator_slot(&paths)?;
        let next = match current {
            Some(mut current)
                if current.session_id == attempted.session_id
                    && current.worktree_id == attempted.worktree_id
                    && current.authority_id == attempted.authority_id
                    && current.principal_id == attempted.principal_id =>
            {
                current.principal_name = attempted.principal_name.clone();
                current.principal_kind = attempted.principal_kind;
                current.credential_id = attempted.credential_id.clone();
                current.agent_label = attempted.agent_label.clone();
                current.worktree_mode = attempted.worktree_mode;
                current.last_heartbeat_at = now;
                current
            }
            Some(current) if !current.is_stale_at(now) => {
                return Err(WorktreeMutatorSlotError::Conflict(
                    WorktreeMutatorSlotConflict {
                        worktree_id: current.worktree_id.clone(),
                        stale_at: current.stale_at(),
                        current_owner: current,
                        attempted_session_id: attempted.session_id.clone(),
                        attempted_principal: attempted.bound_principal(),
                    },
                ));
            }
            Some(mut current) => {
                current.version = WORKTREE_MUTATOR_SLOT_FILE_VERSION;
                current.storage_worktree_id = attempted.storage_worktree_id.clone();
                current.worktree_id = attempted.worktree_id.clone();
                current.agent_label = attempted.agent_label.clone();
                current.worktree_mode = attempted.worktree_mode;
                current.session_id = attempted.session_id.clone();
                current.authority_id = attempted.authority_id.clone();
                current.principal_id = attempted.principal_id.clone();
                current.principal_name = attempted.principal_name.clone();
                current.principal_kind = attempted.principal_kind;
                current.credential_id = attempted.credential_id.clone();
                current.acquired_at = now;
                current.last_heartbeat_at = now;
                current
            }
            None => attempted,
        };
        save_worktree_mutator_slot(&paths, &next)?;
        self.update_cached_worktree_mutator_slot(Some(next.clone()));
        Ok(next)
    }

    pub fn take_over_worktree_mutator_slot(
        &self,
        authenticated: &AuthenticatedPrincipal,
        session_id: &SessionId,
        reason: Option<&str>,
    ) -> std::result::Result<WorktreeMutatorSlotRecord, WorktreeMutatorSlotError> {
        if authenticated.principal.kind != PrincipalKind::Human {
            return Err(WorktreeMutatorSlotError::TakeoverRequiresHuman {
                principal_id: authenticated.principal.principal_id.0.to_string(),
                principal_kind: authenticated.principal.kind,
            });
        }
        let paths = PrismPaths::for_workspace_root(&self.root)?;
        let now = current_timestamp_millis();
        let next = WorktreeMutatorSlotRecord::from_authenticated(
            paths.identity(),
            authenticated,
            session_id,
            now,
        );
        let mut next = next;
        next.takeover_reason = reason
            .map(str::trim)
            .filter(|value| !value.is_empty())
            .map(ToOwned::to_owned);
        let _lock = WorktreeMutatorSlotFileLock::acquire(&paths)?;
        save_worktree_mutator_slot(&paths, &next)?;
        self.update_cached_worktree_mutator_slot(Some(next.clone()));
        Ok(next)
    }

    pub fn current_worktree_mutator_slot(&self) -> Option<WorktreeMutatorSlotRecord> {
        self.worktree_mutator_slot
            .lock()
            .expect("worktree mutator slot lock poisoned")
            .clone()
    }

    fn update_cached_worktree_mutator_slot(&self, slot: Option<WorktreeMutatorSlotRecord>) {
        *self
            .worktree_mutator_slot
            .lock()
            .expect("worktree mutator slot lock poisoned") = slot.clone();
        if let Some(slot) = slot {
            *self
                .worktree_principal_binding
                .lock()
                .expect("worktree principal binding lock poisoned") = Some(slot.bound_principal());
            self.schedule_pending_repo_patch_provenance_for_active_work();
        }
    }
}

pub(crate) fn load_worktree_mutator_slot(
    paths: &PrismPaths,
) -> Result<Option<WorktreeMutatorSlotRecord>> {
    let path = paths.worktree_mutator_slot_path();
    if !path.exists() {
        return Ok(None);
    }
    let bytes = fs::read(&path)
        .with_context(|| format!("failed to read worktree mutator slot at {}", path.display()))?;
    Ok(Some(serde_json::from_slice(&bytes).with_context(|| {
        format!(
            "failed to parse worktree mutator slot at {}",
            path.display()
        )
    })?))
}

pub(crate) fn save_worktree_mutator_slot(
    paths: &PrismPaths,
    slot: &WorktreeMutatorSlotRecord,
) -> Result<()> {
    let path = paths.worktree_mutator_slot_path();
    write_json_file(&path, slot)
}

struct WorktreeMutatorSlotFileLock {
    path: PathBuf,
    _file: File,
}

impl WorktreeMutatorSlotFileLock {
    fn acquire(paths: &PrismPaths) -> Result<Self> {
        fs::create_dir_all(paths.worktree_dir()).with_context(|| {
            format!(
                "failed to create worktree directory {}",
                paths.worktree_dir().display()
            )
        })?;
        let path = paths
            .worktree_dir()
            .join(WORKTREE_MUTATOR_SLOT_LOCK_FILE_NAME);
        let mut waited_ms = 0;
        loop {
            match OpenOptions::new().write(true).create_new(true).open(&path) {
                Ok(file) => {
                    return Ok(Self { path, _file: file });
                }
                Err(error) if error.kind() == ErrorKind::AlreadyExists => {
                    if waited_ms >= WORKTREE_MUTATOR_SLOT_LOCK_WAIT_MS {
                        return Err(anyhow!(
                            "timed out waiting for worktree mutator slot lock {}",
                            path.display()
                        ));
                    }
                    thread::sleep(Duration::from_millis(WORKTREE_MUTATOR_SLOT_LOCK_RETRY_MS));
                    waited_ms += WORKTREE_MUTATOR_SLOT_LOCK_RETRY_MS;
                }
                Err(error) => {
                    return Err(error)
                        .with_context(|| format!("failed to acquire {}", path.display()));
                }
            }
        }
    }
}

impl Drop for WorktreeMutatorSlotFileLock {
    fn drop(&mut self) {
        let _ = fs::remove_file(&self.path);
    }
}

fn write_json_file<T>(path: &Path, value: &T) -> Result<()>
where
    T: Serialize,
{
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)
            .with_context(|| format!("failed to create {}", parent.display()))?;
    }
    let mut bytes =
        serde_json::to_vec_pretty(value).context("failed to serialize worktree mutator slot")?;
    bytes.push(b'\n');
    let tmp_path = path.with_extension(format!("tmp-{}", prism_ir::new_sortable_token()));
    fs::write(&tmp_path, &bytes)
        .with_context(|| format!("failed to write {}", tmp_path.display()))?;
    fs::rename(&tmp_path, path).with_context(|| format!("failed to replace {}", path.display()))
}
