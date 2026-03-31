use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
pub enum PlanStatus {
    Draft,
    Active,
    Blocked,
    Completed,
    Abandoned,
    Archived,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
pub enum CoordinationTaskStatus {
    Proposed,
    Ready,
    InProgress,
    Blocked,
    InReview,
    Validating,
    Completed,
    Abandoned,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
pub enum ClaimMode {
    Advisory,
    SoftExclusive,
    HardExclusive,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
pub enum Capability {
    Observe,
    Edit,
    Review,
    Validate,
    Merge,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
pub enum ClaimStatus {
    Active,
    Released,
    Expired,
    Contended,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
pub enum ConflictSeverity {
    Info,
    Warn,
    Block,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
pub enum ConflictOverlapKind {
    Node,
    Lineage,
    File,
    Kind,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
pub enum ArtifactStatus {
    Proposed,
    InReview,
    Approved,
    Rejected,
    Superseded,
    Merged,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
pub enum ReviewVerdict {
    Approved,
    ChangesRequested,
    Rejected,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
pub enum CoordinationEventKind {
    PlanCreated,
    PlanUpdated,
    TaskCreated,
    TaskAssigned,
    TaskStatusChanged,
    TaskBlocked,
    TaskUnblocked,
    ClaimAcquired,
    ClaimRenewed,
    ClaimReleased,
    ClaimContended,
    ArtifactProposed,
    ArtifactReviewed,
    ArtifactSuperseded,
    HandoffRequested,
    HandoffAccepted,
    MutationRejected,
}
