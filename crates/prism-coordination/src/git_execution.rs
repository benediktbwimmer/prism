use prism_ir::{CoordinationTaskStatus, Timestamp};
use schemars::JsonSchema;
use serde::{Deserialize, Deserializer, Serialize};

fn default_target_branch() -> String {
    "main".to_string()
}

fn default_max_commits_behind_target() -> u32 {
    0
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum GitExecutionStartMode {
    Off,
    Require,
}

impl Default for GitExecutionStartMode {
    fn default() -> Self {
        Self::Off
    }
}

impl<'de> Deserialize<'de> for GitExecutionStartMode {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        let value = String::deserialize(deserializer)?;
        match value.as_str() {
            "off" => Ok(Self::Off),
            "require" | "auto" => Ok(Self::Require),
            _ => Err(serde::de::Error::unknown_variant(
                &value,
                &["off", "require"],
            )),
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum GitExecutionCompletionMode {
    Off,
    Require,
}

impl Default for GitExecutionCompletionMode {
    fn default() -> Self {
        Self::Off
    }
}

impl<'de> Deserialize<'de> for GitExecutionCompletionMode {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        let value = String::deserialize(deserializer)?;
        match value.as_str() {
            "off" => Ok(Self::Off),
            "require" | "auto" => Ok(Self::Require),
            _ => Err(serde::de::Error::unknown_variant(
                &value,
                &["off", "require"],
            )),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct GitExecutionPolicy {
    #[serde(default)]
    pub start_mode: GitExecutionStartMode,
    #[serde(default)]
    pub completion_mode: GitExecutionCompletionMode,
    #[serde(default)]
    pub target_ref: Option<String>,
    #[serde(default = "default_target_branch")]
    pub target_branch: String,
    #[serde(default)]
    pub require_task_branch: bool,
    #[serde(default = "default_max_commits_behind_target")]
    pub max_commits_behind_target: u32,
    #[serde(default)]
    pub max_fetch_age_seconds: Option<u64>,
}

impl Default for GitExecutionPolicy {
    fn default() -> Self {
        Self {
            start_mode: GitExecutionStartMode::Off,
            completion_mode: GitExecutionCompletionMode::Off,
            target_ref: None,
            target_branch: default_target_branch(),
            require_task_branch: false,
            max_commits_behind_target: default_max_commits_behind_target(),
            max_fetch_age_seconds: None,
        }
    }
}

impl GitExecutionPolicy {
    pub fn effective_target_ref(&self) -> String {
        self.target_ref
            .clone()
            .unwrap_or_else(|| format!("origin/{}", self.target_branch))
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct GitPreflightReport {
    #[serde(default)]
    pub source_ref: Option<String>,
    #[serde(default)]
    pub target_ref: Option<String>,
    #[serde(default)]
    pub publish_ref: Option<String>,
    pub checked_at: Timestamp,
    pub target_branch: String,
    #[serde(default)]
    pub max_commits_behind_target: u32,
    #[serde(default)]
    pub fetch_age_seconds: Option<u64>,
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
    pub publish_ref: Option<String>,
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
    pub source_ref: Option<String>,
    #[serde(default)]
    pub target_ref: Option<String>,
    #[serde(default)]
    pub publish_ref: Option<String>,
    #[serde(default)]
    pub target_branch: Option<String>,
    #[serde(default)]
    pub last_preflight: Option<GitPreflightReport>,
    #[serde(default)]
    pub last_publish: Option<GitPublishReport>,
}

#[cfg(test)]
mod tests {
    use super::{GitExecutionCompletionMode, GitExecutionPolicy, GitExecutionStartMode};

    #[test]
    fn git_execution_policy_defaults_to_off() {
        let policy = GitExecutionPolicy::default();
        assert_eq!(policy.start_mode, GitExecutionStartMode::Off);
        assert_eq!(policy.completion_mode, GitExecutionCompletionMode::Off);
        assert!(!policy.require_task_branch);
        assert_eq!(policy.target_branch, "main");
    }

    #[test]
    fn legacy_auto_modes_deserialize_as_require() {
        let policy: GitExecutionPolicy = serde_json::from_str(
            r#"{
                "startMode":"auto",
                "completionMode":"auto",
                "targetBranch":"main"
            }"#,
        )
        .expect("legacy policy should deserialize");
        assert_eq!(policy.start_mode, GitExecutionStartMode::Require);
        assert_eq!(policy.completion_mode, GitExecutionCompletionMode::Require);
    }
}
