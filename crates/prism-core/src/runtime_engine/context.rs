use std::path::{Path, PathBuf};

use crate::workspace_identity::{workspace_identity_for_root, WorkspaceIdentity};

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct WorkspaceRuntimeContext {
    root: PathBuf,
    repo_id: String,
    worktree_id: String,
    branch_ref: Option<String>,
    instance_id: String,
}

impl WorkspaceRuntimeContext {
    pub fn from_root(root: &Path) -> Self {
        Self::from_identity(workspace_identity_for_root(root))
    }

    pub(crate) fn from_identity(identity: WorkspaceIdentity) -> Self {
        Self {
            root: identity.canonical_root,
            repo_id: identity.repo_id,
            worktree_id: identity.worktree_id,
            branch_ref: identity.branch_ref,
            instance_id: identity.instance_id,
        }
    }

    pub fn root(&self) -> &Path {
        self.root.as_path()
    }

    pub fn repo_id(&self) -> &str {
        self.repo_id.as_str()
    }

    pub fn worktree_id(&self) -> &str {
        self.worktree_id.as_str()
    }

    pub fn branch_ref(&self) -> Option<&str> {
        self.branch_ref.as_deref()
    }

    pub fn instance_id(&self) -> &str {
        self.instance_id.as_str()
    }
}
