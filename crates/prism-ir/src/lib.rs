mod anchor;
mod change;
mod coordination;
mod durable_ids;
mod events;
mod graph;
mod history;
mod identity;
mod parse;
mod plans;
mod primitives;
mod principal;

pub use anchor::AnchorRef;
pub use change::{ChangeTrigger, GraphChange, ObservedChangeSet, ObservedNode};
pub use coordination::{
    ArtifactStatus, Capability, ClaimMode, ClaimStatus, ConflictOverlapKind, ConflictSeverity,
    CoordinationEventKind, CoordinationTaskStatus, LeaseRenewalMode, PlanStatus, ReviewVerdict,
};
pub use durable_ids::{new_prefixed_id, new_slugged_id, new_sortable_token, slugify_id_fragment};
pub use events::{
    EventActor, EventExecutionContext, EventMeta, ObservedChangeCheckpoint,
    ObservedChangeCheckpointEntry, ObservedChangeCheckpointTrigger, WorkContextKind,
    WorkContextSnapshot,
};
pub use graph::{Edge, EdgeKind, EdgeOrigin, Node, NodeId, NodeKind, Skeleton, Subgraph};
pub use history::{LineageEvent, LineageEventKind, LineageEvidence};
pub use identity::{
    AgentId, ArtifactId, ClaimId, CoordinationTaskId, CredentialId, EventId, LineageId, PlanEdgeId,
    PlanId, PlanNodeId, PrincipalAuthorityId, PrincipalId, ReviewId, SessionId, TaskId,
    WorkspaceRevision,
};
pub use parse::{
    SymbolFingerprint, UnresolvedCall, UnresolvedImpl, UnresolvedImport, UnresolvedIntent,
};
pub use plans::{
    AcceptanceEvidencePolicy, BlockerCause, BlockerCauseSource, GitExecutionOverlay,
    GitExecutionStatus, GitIntegrationEvidence, GitIntegrationEvidenceKind, GitIntegrationMode,
    GitIntegrationStatus, HydratedPlanBindingOverlay, PlanAcceptanceCriterion, PlanBinding,
    PlanEdge, PlanEdgeKind, PlanExecutionOverlay, PlanGraph, PlanKind, PlanNode, PlanNodeBlocker,
    PlanNodeBlockerKind, PlanNodeKind, PlanNodeStatus, PlanScope, ValidationRef,
};
pub use primitives::{EdgeIndex, FileId, Language, Span, Timestamp};
pub use principal::{
    CredentialCapability, CredentialRecord, CredentialStatus, HumanAttestationAssurance,
    HumanAttestationOperation, HumanAttestationRecord, HumanPrincipalProfile, PrincipalActor,
    PrincipalKind, PrincipalProfile, PrincipalRef, PrincipalRegistrySnapshot, PrincipalStatus,
};
