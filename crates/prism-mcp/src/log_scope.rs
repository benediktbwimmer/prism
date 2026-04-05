use std::fs;
use std::path::{Path, PathBuf};

use anyhow::{Context, Result};
use prism_core::PrismPaths;
use serde::Deserialize;

const WORKTREE_METADATA_FILE_NAME: &str = "worktree.json";
const MCP_LOGS_DIR: &str = "mcp/logs";
const MCP_DAEMON_LOG_FILE_NAME: &str = "prism-mcp-daemon.log";
const MCP_CALL_LOG_FILE_NAME: &str = "prism-mcp-call-log.jsonl";

#[derive(Debug, Clone, Copy, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
pub(crate) enum LogScope {
    Worktree,
    Repo,
    All,
}

impl LogScope {
    pub(crate) fn aggregates_repo_logs(self) -> bool {
        matches!(self, Self::Repo | Self::All)
    }
}

#[derive(Debug, Clone)]
pub(crate) struct RepoLogSource {
    pub(crate) repo_id: String,
    pub(crate) worktree_id: String,
    pub(crate) workspace_root: String,
    pub(crate) daemon_log_path: PathBuf,
    pub(crate) mcp_call_log_path: PathBuf,
}

#[derive(Debug, Clone, Deserialize)]
struct WorktreeMetadata {
    repo_id: String,
    worktree_id: String,
    #[serde(default)]
    registered_worktree_id: Option<String>,
    canonical_root: String,
}

pub(crate) fn select_log_sources(
    root: &Path,
    scope: Option<LogScope>,
    worktree_id: Option<&str>,
) -> Result<Vec<RepoLogSource>> {
    let scope = scope.unwrap_or(LogScope::Worktree);
    let current = current_log_source(root)?;
    let mut sources = if scope.aggregates_repo_logs() {
        repo_log_sources(root)?
    } else {
        vec![current.clone()]
    };
    if let Some(requested_worktree_id) = worktree_id {
        sources.retain(|source| source.worktree_id == requested_worktree_id);
    } else if !scope.aggregates_repo_logs() {
        sources.retain(|source| source.worktree_id == current.worktree_id);
    }
    sources.sort_by(|left, right| left.worktree_id.cmp(&right.worktree_id));
    sources.dedup_by(|left, right| left.worktree_id == right.worktree_id);
    Ok(sources)
}

pub(crate) fn current_log_source(root: &Path) -> Result<RepoLogSource> {
    let prism_paths = PrismPaths::for_workspace_root(root)?;
    let metadata =
        read_worktree_metadata(&prism_paths.worktree_dir().join(WORKTREE_METADATA_FILE_NAME))?
            .unwrap_or_else(|| WorktreeMetadata {
                repo_id: String::new(),
                worktree_id: prism_paths
                    .worktree_dir()
                    .file_name()
                    .and_then(|value| value.to_str())
                    .unwrap_or_default()
                    .to_string(),
                registered_worktree_id: None,
                canonical_root: root.display().to_string(),
            });
    Ok(RepoLogSource {
        repo_id: metadata.repo_id,
        worktree_id: metadata
            .registered_worktree_id
            .unwrap_or(metadata.worktree_id),
        workspace_root: metadata.canonical_root,
        daemon_log_path: prism_paths.mcp_daemon_log_path()?,
        mcp_call_log_path: prism_paths.mcp_call_log_path()?,
    })
}

fn repo_log_sources(root: &Path) -> Result<Vec<RepoLogSource>> {
    let prism_paths = PrismPaths::for_workspace_root(root)?;
    let worktrees_dir = prism_paths.repo_home_dir().join("worktrees");
    if !worktrees_dir.exists() {
        return Ok(Vec::new());
    }

    let mut sources = Vec::new();
    for entry in fs::read_dir(&worktrees_dir)
        .with_context(|| format!("failed to read {}", worktrees_dir.display()))?
    {
        let entry = entry.with_context(|| format!("failed to read {}", worktrees_dir.display()))?;
        let worktree_dir = entry.path();
        let metadata = read_worktree_metadata(&worktree_dir.join(WORKTREE_METADATA_FILE_NAME))?;
        let Some(metadata) = metadata else {
            continue;
        };
        sources.push(RepoLogSource {
            repo_id: metadata.repo_id,
            worktree_id: metadata
                .registered_worktree_id
                .unwrap_or(metadata.worktree_id),
            workspace_root: metadata.canonical_root,
            daemon_log_path: worktree_dir
                .join(MCP_LOGS_DIR)
                .join(MCP_DAEMON_LOG_FILE_NAME),
            mcp_call_log_path: worktree_dir.join(MCP_LOGS_DIR).join(MCP_CALL_LOG_FILE_NAME),
        });
    }
    Ok(sources)
}

fn read_worktree_metadata(path: &Path) -> Result<Option<WorktreeMetadata>> {
    if !path.exists() {
        return Ok(None);
    }
    let bytes = fs::read(path).with_context(|| format!("failed to read {}", path.display()))?;
    let metadata = serde_json::from_slice(&bytes)
        .with_context(|| format!("failed to decode {}", path.display()))?;
    Ok(Some(metadata))
}
