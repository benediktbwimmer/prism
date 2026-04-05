use std::collections::hash_map::DefaultHasher;
use std::collections::HashMap;
use std::fs;
use std::hash::{Hash, Hasher};
use std::path::{Path, PathBuf};
use std::sync::{Mutex, OnceLock};

use prism_ir::SessionId;
use prism_store::CoordinationPersistContext;

use crate::util::current_timestamp_millis;
use crate::worktree_registration::{WorktreeMode, WorktreeRegistrationRecord};

#[derive(Debug)]
struct GitWorkspaceIdentity {
    common_dir: Option<PathBuf>,
    head_ref: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct WorkspaceIdentity {
    pub(crate) canonical_root: PathBuf,
    pub(crate) repo_id: String,
    pub(crate) repo_locator_kind: &'static str,
    pub(crate) repo_locator_path: PathBuf,
    pub(crate) storage_worktree_id: String,
    pub(crate) worktree_id: String,
    pub(crate) registered_worktree_id: Option<String>,
    pub(crate) agent_label: Option<String>,
    pub(crate) worktree_mode: Option<WorktreeMode>,
    pub(crate) registered_at: Option<u64>,
    pub(crate) last_registered_at: Option<u64>,
    pub(crate) branch_ref: Option<String>,
    pub(crate) instance_id: String,
}

pub(crate) fn workspace_identity_for_root(root: &Path) -> WorkspaceIdentity {
    let canonical_root = root.canonicalize().unwrap_or_else(|_| root.to_path_buf());
    let git_identity = discover_git_workspace_identity(&canonical_root);
    let repo_locator_path = git_identity
        .common_dir
        .as_ref()
        .cloned()
        .unwrap_or_else(|| canonical_root.clone());
    let repo_locator_kind = if git_identity.common_dir.is_some() {
        "git_common_dir"
    } else {
        "canonical_root"
    };
    let repo_source = repo_locator_path.to_string_lossy().to_string();
    let storage_worktree_id = scoped_id("worktree", &canonical_root.to_string_lossy());
    WorkspaceIdentity {
        canonical_root: canonical_root.clone(),
        repo_id: scoped_id("repo", &repo_source),
        repo_locator_kind,
        repo_locator_path,
        storage_worktree_id: storage_worktree_id.clone(),
        worktree_id: storage_worktree_id,
        registered_worktree_id: None,
        agent_label: None,
        worktree_mode: None,
        registered_at: None,
        last_registered_at: None,
        branch_ref: git_identity.head_ref,
        instance_id: instance_id_for_root(&canonical_root),
    }
}

pub(crate) fn canonical_root_repo_id(root: &Path) -> String {
    let canonical_root = root.canonicalize().unwrap_or_else(|_| root.to_path_buf());
    scoped_id("repo", &canonical_root.to_string_lossy())
}

pub(crate) fn coordination_persist_context_for_root(
    root: &Path,
    session_id: Option<&SessionId>,
) -> CoordinationPersistContext {
    let identity = workspace_identity_for_root(root);
    CoordinationPersistContext {
        repo_id: identity.repo_id,
        worktree_id: identity.worktree_id,
        branch_ref: identity.branch_ref,
        session_id: session_id.map(|session_id| session_id.0.to_string()),
        instance_id: Some(identity.instance_id),
    }
}

impl WorkspaceIdentity {
    pub(crate) fn is_worktree_registered(&self) -> bool {
        self.registered_worktree_id.is_some()
    }

    pub(crate) fn worktree_registration(&self) -> Option<WorktreeRegistrationRecord> {
        Some(WorktreeRegistrationRecord {
            worktree_id: self.registered_worktree_id.clone()?,
            agent_label: self.agent_label.clone()?,
            mode: self.worktree_mode?,
            registered_at: self.registered_at?,
            last_registered_at: self.last_registered_at?,
        })
    }

    pub(crate) fn apply_worktree_registration(&mut self, registration: WorktreeRegistrationRecord) {
        self.worktree_id = registration.worktree_id.clone();
        self.registered_worktree_id = Some(registration.worktree_id);
        self.agent_label = Some(registration.agent_label);
        self.worktree_mode = Some(registration.mode);
        self.registered_at = Some(registration.registered_at);
        self.last_registered_at = Some(registration.last_registered_at);
    }
}

fn instance_id_for_root(canonical_root: &Path) -> String {
    static INSTANCE_IDS: OnceLock<Mutex<HashMap<PathBuf, String>>> = OnceLock::new();
    let ids = INSTANCE_IDS.get_or_init(|| Mutex::new(HashMap::new()));
    let mut ids = ids
        .lock()
        .expect("workspace instance id cache lock poisoned");
    ids.entry(canonical_root.to_path_buf())
        .or_insert_with(|| {
            format!(
                "instance:{}:{}:{}",
                std::process::id(),
                scoped_id("worktree", &canonical_root.to_string_lossy()),
                current_timestamp_millis()
            )
        })
        .clone()
}

fn scoped_id(prefix: &str, value: &str) -> String {
    let mut hasher = DefaultHasher::new();
    value.hash(&mut hasher);
    format!("{prefix}:{:016x}", hasher.finish())
}

fn discover_git_workspace_identity(root: &Path) -> GitWorkspaceIdentity {
    let Some(git_dir) = resolve_git_dir(root) else {
        return GitWorkspaceIdentity {
            common_dir: None,
            head_ref: None,
        };
    };
    let common_dir = resolve_common_dir(&git_dir);
    let head_ref = fs::read_to_string(git_dir.join("HEAD"))
        .ok()
        .map(|contents| contents.trim().to_string())
        .filter(|value| !value.is_empty())
        .map(|value| {
            value
                .strip_prefix("ref: ")
                .map(str::to_owned)
                .unwrap_or_else(|| format!("detached:{value}"))
        });
    GitWorkspaceIdentity {
        common_dir,
        head_ref,
    }
}

fn resolve_git_dir(root: &Path) -> Option<PathBuf> {
    let dot_git = root.join(".git");
    if dot_git.is_dir() {
        return dot_git.canonicalize().ok().or(Some(dot_git));
    }
    let contents = fs::read_to_string(&dot_git).ok()?;
    let gitdir = contents.strip_prefix("gitdir: ")?.trim();
    let resolved = dot_git
        .parent()
        .unwrap_or(root)
        .join(gitdir)
        .canonicalize()
        .ok()
        .or_else(|| Some(dot_git.parent().unwrap_or(root).join(gitdir)))?;
    Some(resolved)
}

fn resolve_common_dir(git_dir: &Path) -> Option<PathBuf> {
    let common_dir_file = git_dir.join("commondir");
    if let Ok(relative) = fs::read_to_string(common_dir_file) {
        let candidate = git_dir.join(relative.trim());
        return candidate.canonicalize().ok().or(Some(candidate));
    }
    if git_dir.join("HEAD").exists() {
        return git_dir.canonicalize().ok().or(Some(git_dir.to_path_buf()));
    }
    None
}
