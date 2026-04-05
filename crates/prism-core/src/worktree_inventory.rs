use std::fs;
use std::path::{Path, PathBuf};

use anyhow::{Context, Result};

use crate::prism_paths::WORKTREE_METADATA_FILE_NAME;
use crate::worktree_registration::{read_worktree_metadata, WorktreeMode};

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RegisteredWorktreeSummary {
    pub repo_id: String,
    pub storage_worktree_id: String,
    pub registered_worktree_id: String,
    pub canonical_root: PathBuf,
    pub branch_ref: Option<String>,
    pub agent_label: String,
    pub mode: WorktreeMode,
    pub registered_at: u64,
    pub last_registered_at: u64,
}

pub fn list_registered_worktrees(home_root: &Path) -> Result<Vec<RegisteredWorktreeSummary>> {
    let repos_dir = home_root.join("repos");
    if !repos_dir.exists() {
        return Ok(Vec::new());
    }

    let mut worktrees = Vec::new();
    for repo_entry in fs::read_dir(&repos_dir)
        .with_context(|| format!("failed to read {}", repos_dir.display()))?
    {
        let repo_entry =
            repo_entry.with_context(|| format!("failed to read {}", repos_dir.display()))?;
        let worktrees_dir = repo_entry.path().join("worktrees");
        if !worktrees_dir.exists() {
            continue;
        }
        for worktree_entry in fs::read_dir(&worktrees_dir)
            .with_context(|| format!("failed to read {}", worktrees_dir.display()))?
        {
            let worktree_entry = worktree_entry
                .with_context(|| format!("failed to read {}", worktrees_dir.display()))?;
            let metadata_path = worktree_entry.path().join(WORKTREE_METADATA_FILE_NAME);
            let Some(metadata) = read_worktree_metadata(&metadata_path)? else {
                continue;
            };
            let Some(registration) = metadata.registration() else {
                continue;
            };
            worktrees.push(RegisteredWorktreeSummary {
                repo_id: metadata.repo_id,
                storage_worktree_id: metadata.worktree_id,
                registered_worktree_id: registration.worktree_id,
                canonical_root: PathBuf::from(metadata.canonical_root),
                branch_ref: metadata.branch_ref,
                agent_label: registration.agent_label,
                mode: registration.mode,
                registered_at: registration.registered_at,
                last_registered_at: registration.last_registered_at,
            });
        }
    }

    worktrees.sort_by(|left, right| {
        left.agent_label
            .cmp(&right.agent_label)
            .then_with(|| left.canonical_root.cmp(&right.canonical_root))
    });
    Ok(worktrees)
}
