use prism_ir::{CoordinationTaskStatus, Timestamp};
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

fn default_target_branch() -> String {
    "main".to_string()
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum GitExecutionStartMode {
    #[default]
    Off,
    Require,
    Auto,
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum GitExecutionCompletionMode {
    #[default]
    Off,
    Require,
    Auto,
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct GitExecutionPolicy {
    #[serde(default)]
    pub start_mode: GitExecutionStartMode,
    #[serde(default)]
    pub completion_mode: GitExecutionCompletionMode,
    #[serde(default = "default_target_branch")]
    pub target_branch: String,
    #[serde(default)]
    pub require_task_branch: bool,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct GitPreflightReport {
    pub checked_at: Timestamp,
    pub target_branch: String,
    #[serde(default)]
    pub current_branch: Option<String>,
    #[serde(default)]
    pub head_commit: Option<String>,
    #[serde(default)]
    pub target_commit: Option<String>,
    #[serde(default)]
    pub merge_base_commit: Option<String>,
    #[serde(default)]
    pub behind_target_commits: u32,
    #[serde(default)]
    pub worktree_dirty: bool,
    #[serde(default)]
    pub dirty_paths: Vec<String>,
    #[serde(default)]
    pub protected_dirty_paths: Vec<String>,
    #[serde(default)]
    pub failure: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct GitPublishReport {
    pub attempted_at: Timestamp,
    #[serde(default)]
    pub code_commit: Option<String>,
    #[serde(default)]
    pub coordination_commit: Option<String>,
    #[serde(default)]
    pub pushed_ref: Option<String>,
    #[serde(default)]
    pub staged_paths: Vec<String>,
    #[serde(default)]
    pub protected_paths: Vec<String>,
    #[serde(default)]
    pub failure: Option<String>,
}

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct TaskGitExecution {
    #[serde(default)]
    pub status: prism_ir::GitExecutionStatus,
    #[serde(default)]
    pub pending_task_status: Option<CoordinationTaskStatus>,
    #[serde(default)]
    pub target_branch: Option<String>,
    #[serde(default)]
    pub last_preflight: Option<GitPreflightReport>,
    #[serde(default)]
    pub last_publish: Option<GitPublishReport>,
}
