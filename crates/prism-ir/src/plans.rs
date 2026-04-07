use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

use crate::{AnchorRef, CoordinationTaskStatus};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
pub enum PlanScope {
    Local,
    Session,
    Repo,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
pub enum PlanKind {
    TaskExecution,
    Investigation,
    Refactor,
    Migration,
    Release,
    IncidentResponse,
    Maintenance,
    Custom,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
pub enum PlanNodeKind {
    Investigate,
    Decide,
    Edit,
    Validate,
    Review,
    Handoff,
    Merge,
    Release,
    Note,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
pub struct ValidationRef {
    pub id: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema, Default)]
pub struct PlanBinding {
    pub anchors: Vec<AnchorRef>,
    pub concept_handles: Vec<String>,
    pub artifact_refs: Vec<String>,
    pub memory_refs: Vec<String>,
    pub outcome_refs: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema, Default)]
pub struct HydratedPlanBindingOverlay {
    pub handles: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
pub struct PlanAcceptanceCriterion {
    pub label: String,
    pub anchors: Vec<AnchorRef>,
    pub required_checks: Vec<ValidationRef>,
    pub evidence_policy: AcceptanceEvidencePolicy,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
pub enum AcceptanceEvidencePolicy {
    Any,
    All,
    ReviewOnly,
    ValidationOnly,
    ReviewAndValidation,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum BlockerCauseSource {
    DependencyGraph,
    RuntimeState,
    PlanPolicy,
    NodeAcceptance,
    ArtifactState,
    DerivedThreshold,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct BlockerCause {
    pub source: BlockerCauseSource,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub code: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub acceptance_label: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub threshold_metric: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub threshold_value: Option<f32>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub observed_value: Option<f32>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum GitExecutionStatus {
    NotStarted,
    PreflightFailed,
    InProgress,
    PublishPending,
    PublishFailed,
    #[serde(alias = "published", alias = "coordination_published")]
    CoordinationPublished,
}

impl Default for GitExecutionStatus {
    fn default() -> Self {
        Self::NotStarted
    }
}

#[cfg(test)]
mod tests {
    use super::GitExecutionStatus;

    #[test]
    fn git_execution_status_accepts_legacy_coordination_published_alias() {
        let status: GitExecutionStatus = serde_json::from_str("\"coordination_published\"")
            .expect("legacy shared-ref status should deserialize");
        assert_eq!(status, GitExecutionStatus::CoordinationPublished);
    }

    #[test]
    fn git_execution_status_accepts_legacy_published_variant() {
        let status: GitExecutionStatus = serde_json::from_str("\"published\"").unwrap();
        assert_eq!(status, GitExecutionStatus::CoordinationPublished);
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum GitIntegrationMode {
    ManualPr,
    AutoPr,
    DirectIntegrate,
    External,
}

impl Default for GitIntegrationMode {
    fn default() -> Self {
        Self::External
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum GitIntegrationStatus {
    NotStarted,
    PublishedToBranch,
    IntegrationPending,
    IntegrationInProgress,
    IntegratedToTarget,
    IntegrationFailed,
}

impl Default for GitIntegrationStatus {
    fn default() -> Self {
        Self::NotStarted
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum GitIntegrationEvidenceKind {
    Reachability,
    ReviewArtifact,
    TrustedRecord,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct GitIntegrationEvidence {
    pub kind: GitIntegrationEvidenceKind,
    pub target_commit: String,
    #[serde(default)]
    pub review_artifact_ref: Option<String>,
    #[serde(default)]
    pub record_ref: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema, Default)]
#[serde(rename_all = "camelCase")]
pub struct GitExecutionOverlay {
    #[serde(default)]
    pub status: GitExecutionStatus,
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
    pub source_commit: Option<String>,
    #[serde(default)]
    pub publish_commit: Option<String>,
    #[serde(default)]
    pub target_commit_at_publish: Option<String>,
    #[serde(default)]
    pub review_artifact_ref: Option<String>,
    #[serde(default)]
    pub integration_commit: Option<String>,
    #[serde(default)]
    pub integration_evidence: Option<GitIntegrationEvidence>,
    #[serde(default)]
    pub integration_mode: GitIntegrationMode,
    #[serde(default)]
    pub integration_status: GitIntegrationStatus,
}
