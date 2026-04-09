mod admission;
mod checkpoint_materializer;
mod concept_events;
mod concept_relation_events;
mod contract_events;
mod coordination_authority_api;
mod coordination_authority_store;
mod coordination_authority_sync;
mod coordination_materialized_store;
mod coordination_mutation_error;
mod coordination_persistence;
mod coordination_reads;
mod coordination_snapshot_sanitization;
mod coordination_startup_checkpoint;
mod curator;
mod curator_support;
mod history_backend;
mod indexer;
mod indexer_support;
mod invalidation;
mod layout;
mod local_credentials;
mod local_principal_registry;
mod materialization;
mod memory_events;
mod memory_refresh;
pub mod mutation_trace;
mod observed_change_tracker;
mod outcome_backend;
mod parse_pipeline;
mod patch_outcomes;
mod path_identity;
mod path_identity_repair;
mod peer_runtime;
mod principal_registry;
mod prism_doc;
mod prism_paths;
mod projection_hydration;
mod protected_state;
mod published_knowledge;
mod published_plans;
mod reanchor;
mod repo_patch_events;
mod resolution;
pub mod runtime_engine;
mod session;
mod session_bootstrap;
mod shared_coordination_ref;
mod shared_coordination_schema;
mod shared_runtime;
mod shared_runtime_backend;
mod snapshot_artifact_repair;
mod snapshot_restoration;
mod tracked_snapshot;
mod util;
mod validation_feedback;
mod watch;
mod workspace_identity;
mod workspace_runtime_state;
mod workspace_session_defaults;
mod workspace_startup_checkpoint;
mod workspace_tree;
mod worktree_inventory;
mod worktree_mutator_slot;
mod worktree_principal;
mod worktree_registration;

use std::sync::Arc;

use anyhow::Result;
use prism_curator::CuratorBackend;
use prism_query::Prism;
use session_bootstrap::{
    hydrate_workspace_session_with_options as bootstrap_workspace_session,
    index_workspace_session_with_options as bootstrap_indexed_workspace_session,
};

pub use admission::AdmissionBusyError;
pub use coordination_authority_api::{
    shared_coordination_ref_diagnostics, sync_live_runtime_descriptor,
};
pub use coordination_authority_store::{
    CoordinationAuthorityBackendDetails, CoordinationAuthorityBackendKind,
    CoordinationAuthorityCapabilities, CoordinationAuthorityDiagnostics,
    CoordinationAuthorityProvenance, CoordinationAuthorityStamp, CoordinationAuthorityStore,
    CoordinationConflictInfo, CoordinationCurrentState, CoordinationDerivedStateMode,
    CoordinationDiagnosticsRequest, CoordinationHistoryEntry, CoordinationHistoryEnvelope,
    CoordinationHistoryRequest, CoordinationReadEnvelope, CoordinationReadRequest,
    CoordinationStateView, CoordinationTransactionBase, CoordinationTransactionDiagnostic,
    CoordinationTransactionRequest, CoordinationTransactionResult, CoordinationTransactionStatus,
    GitSharedRefsCoordinationAuthorityStore, RuntimeDescriptorClearRequest,
    RuntimeDescriptorPublishRequest, RuntimeDescriptorQuery,
};
pub use coordination_materialized_store::{
    CoordinationCompactionWriteRequest, CoordinationMaterializationMetadata,
    CoordinationMaterializedBackendKind, CoordinationMaterializedCapabilities,
    CoordinationMaterializedClearRequest, CoordinationMaterializedReadEnvelope,
    CoordinationMaterializedState, CoordinationMaterializedStore,
    CoordinationMaterializedWriteResult, CoordinationReadModelsWriteRequest,
    CoordinationStartupCheckpointWriteRequest, SqliteCoordinationMaterializedStore,
};
pub use coordination_mutation_error::{
    CoordinationAuthorityMutationError, CoordinationAuthorityMutationStatus,
};
pub use coordination_reads::{
    CoordinationReadConsistency, CoordinationReadFreshness, CoordinationReadResult,
};
pub(crate) use indexer::PendingFileParse;
pub use indexer::WorkspaceIndexer;
pub use local_credentials::{
    CredentialProfile, CredentialProfileCredentialMetadata, CredentialProfilePrincipalMetadata,
    CredentialsFile, EncryptedCredentialSecret, HumanSessionFile, HumanSessionRecord,
};
pub use local_principal_registry::{
    ensure_local_principal_registry_snapshot,
    ensure_local_principal_registry_snapshot_with_unlocked_profile,
};
pub use materialization::{
    WorkspaceBoundaryRegion, WorkspaceMaterializationCoverage, WorkspaceMaterializationSummary,
};
pub use observed_change_tracker::{
    AccumulatedObservedChange, ActiveWorkContextBinding, FlushedObservedChangeSet,
    ObservedChangeFlushTrigger,
};
pub use path_identity_repair::{
    inspect_legacy_path_identity_state, repair_legacy_path_identity_state,
    LegacyPathIdentityRepairReport, LegacyPathIdentityRepairTargetReport,
};
pub use peer_runtime::{local_runtime_id, runtime_query_endpoint, PEER_RUNTIME_QUERY_PATH};
pub use principal_registry::{
    authenticate_principal_credential_in_registry, bootstrap_owner_principal_in_registry,
    mint_principal_credential_in_registry, recover_owner_principal_in_registry,
    AttestedHumanPrincipalInput, AuthenticatedPrincipal, BootstrapOwnerInput, MintPrincipalRequest,
    MintedPrincipalCredential,
};
pub use prism_doc::{
    render_repo_published_plan_markdown, PrismDocBundleFormat, PrismDocExportBundle,
    PrismDocExportResult, PrismDocSyncResult, PrismDocSyncStatus,
};
pub use prism_ir::{PrismLayerSet, PrismRuntimeCapabilities, PrismRuntimeLayer, PrismRuntimeMode};
pub use prism_paths::PrismPaths;
pub use prism_spec::{
    discover_spec_sources, parse_spec_source, parse_spec_sources, refresh_spec_materialization,
    resolve_spec_root, DiscoveredSpecSource, MaterializedSpecQueryEngine, MaterializedSpecRecord,
    ParsedSpecDocument, ParsedSpecSet, SpecChecklistIdentitySource, SpecChecklistItem,
    SpecChecklistRequirementLevel, SpecChecklistView, SpecCoverageView, SpecDeclaredStatus,
    SpecDependency, SpecDependencyView, SpecDocumentView, SpecListEntry,
    SpecMaterializationMetadata, SpecMaterializationRefreshResult, SpecMaterializedBackendKind,
    SpecMaterializedCapabilities, SpecMaterializedClearRequest, SpecMaterializedReadEnvelope,
    SpecMaterializedReplaceRequest, SpecMaterializedStore, SpecMaterializedWriteResult,
    SpecMetadataView, SpecParseDiagnostic, SpecParseDiagnosticKind, SpecQueryEngine,
    SpecQueryLookup, SpecRootResolution, SpecRootSource, SpecSourceMetadata, SpecSyncBriefView,
    SpecSyncProvenanceView, SqliteSpecMaterializedStore, StoredSpecChecklistItemRecord,
    StoredSpecChecklistPosture, StoredSpecCoverageRecord, StoredSpecDependencyPosture,
    StoredSpecDependencyRecord, StoredSpecStatusRecord, StoredSpecSyncProvenanceRecord,
};
pub use protected_state::migration::{
    migrate_legacy_protected_repo_state, ProtectedStateMigrationReport,
};
pub use protected_state::operators::{
    diagnose_protected_state, export_protected_state_trust_material,
    import_protected_state_trust_material, quarantine_protected_state_stream,
    reconcile_protected_state_stream, repair_protected_state_stream_to_last_valid,
    verify_protected_state, ProtectedStateQuarantineReport, ProtectedStateReconcileReport,
    ProtectedStateRepairReport, ProtectedStateStreamReport, ProtectedStateTrustExport,
    ProtectedStateTrustImportReport, ProtectedStateVerifyReport,
};
pub use published_plans::regenerate_repo_published_plan_artifacts;
pub use session::{
    CoordinationPlanState, FsRefreshStatus, PersistedObservedChangeCheckpointResult,
    WorkspaceFsRefreshOutcome, WorkspaceRefreshBreakdown, WorkspaceRefreshWork, WorkspaceSession,
    WorkspaceSnapshotRevisions,
};
pub use shared_coordination_ref::SharedCoordinationRefDiagnostics;
pub use shared_runtime_backend::SharedRuntimeBackend;
pub use snapshot_artifact_repair::regenerate_repo_snapshot_derived_artifacts;
pub use snapshot_restoration::{
    restore_legacy_repo_published_knowledge, LegacyRepoKnowledgeRestoreReport,
};
pub use validation_feedback::{
    ValidationFeedbackCategory, ValidationFeedbackEntry, ValidationFeedbackRecord,
    ValidationFeedbackVerdict,
};
pub use watch::{assisted_lease_renewal_diagnostics, AssistedLeaseRenewalDiagnostics};
pub use workspace_session_defaults::{
    default_workspace_session_options, default_workspace_shared_runtime,
};
pub use worktree_inventory::{list_registered_worktrees, RegisteredWorktreeSummary};
pub use worktree_mutator_slot::{
    WorktreeMutatorSlotConflict, WorktreeMutatorSlotError, WorktreeMutatorSlotRecord,
    WORKTREE_MUTATOR_SLOT_STALE_AFTER_MS,
};
pub use worktree_principal::{BoundWorktreePrincipal, WorktreePrincipalConflict};
pub use worktree_registration::{WorktreeMode, WorktreeRegistrationRecord};

#[derive(Debug, Clone)]
pub struct WorkspaceSessionOptions {
    pub runtime_mode: PrismRuntimeMode,
    pub shared_runtime: SharedRuntimeBackend,
    pub hydrate_persisted_projections: bool,
    pub hydrate_persisted_co_change: bool,
}

impl Default for WorkspaceSessionOptions {
    fn default() -> Self {
        Self {
            runtime_mode: PrismRuntimeMode::Full,
            shared_runtime: SharedRuntimeBackend::Disabled,
            hydrate_persisted_projections: false,
            hydrate_persisted_co_change: true,
        }
    }
}

impl WorkspaceSessionOptions {
    pub fn runtime_capabilities(&self) -> PrismRuntimeCapabilities {
        self.runtime_mode.capabilities()
    }

    pub fn coordination_enabled(&self) -> bool {
        self.runtime_capabilities().coordination_enabled()
    }

    pub fn knowledge_storage_enabled(&self) -> bool {
        self.runtime_capabilities().knowledge_storage_enabled()
    }

    pub fn cognition_enabled(&self) -> bool {
        self.runtime_capabilities().cognition_enabled()
    }
}

pub fn index_workspace(root: impl AsRef<std::path::Path>) -> Result<Prism> {
    let mut indexer = WorkspaceIndexer::new(root)?;
    indexer.index()?;
    Ok(indexer.into_prism())
}

pub fn index_workspace_session(root: impl AsRef<std::path::Path>) -> Result<WorkspaceSession> {
    let root = root.as_ref();
    index_workspace_session_with_options(root, default_workspace_session_options(root)?)
}

pub fn hydrate_workspace_session(root: impl AsRef<std::path::Path>) -> Result<WorkspaceSession> {
    let root = root.as_ref();
    hydrate_workspace_session_with_options(root, default_workspace_session_options(root)?)
}

pub fn hydrate_workspace_session_with_options(
    root: impl AsRef<std::path::Path>,
    options: WorkspaceSessionOptions,
) -> Result<WorkspaceSession> {
    bootstrap_workspace_session(root, options)
}

pub fn index_workspace_session_with_options(
    root: impl AsRef<std::path::Path>,
    options: WorkspaceSessionOptions,
) -> Result<WorkspaceSession> {
    bootstrap_indexed_workspace_session(root, options, None)
}

pub fn index_workspace_session_with_curator(
    root: impl AsRef<std::path::Path>,
    backend: Arc<dyn CuratorBackend>,
) -> Result<WorkspaceSession> {
    let root = root.as_ref();
    index_workspace_session_with_curator_and_options(
        root,
        backend,
        default_workspace_session_options(root)?,
    )
}

pub fn index_workspace_session_with_curator_and_options(
    root: impl AsRef<std::path::Path>,
    backend: Arc<dyn CuratorBackend>,
    options: WorkspaceSessionOptions,
) -> Result<WorkspaceSession> {
    bootstrap_indexed_workspace_session(root, options, Some(backend))
}

#[cfg(test)]
mod tests;
