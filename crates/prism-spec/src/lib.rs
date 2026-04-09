mod spec_engine;
mod spec_materialized_store;
mod spec_query;
mod spec_refresh;

pub use spec_engine::{
    discover_spec_sources, parse_spec_source, parse_spec_sources, resolve_spec_root,
    DiscoveredSpecSource, ParsedSpecDocument, ParsedSpecSet, SpecChecklistIdentitySource,
    SpecChecklistItem, SpecChecklistRequirementLevel, SpecDeclaredStatus, SpecDependency,
    SpecParseDiagnostic, SpecParseDiagnosticKind, SpecRootResolution, SpecRootSource,
    SpecSourceMetadata,
};
pub use spec_materialized_store::{
    MaterializedSpecRecord, SpecMaterializationMetadata, SpecMaterializedBackendKind,
    SpecMaterializedCapabilities, SpecMaterializedClearRequest, SpecMaterializedReadEnvelope,
    SpecMaterializedReplaceRequest, SpecMaterializedStore, SpecMaterializedWriteResult,
    SqliteSpecMaterializedStore, StoredSpecChecklistItemRecord, StoredSpecChecklistPosture,
    StoredSpecCoverageRecord, StoredSpecDependencyPosture, StoredSpecDependencyRecord,
    StoredSpecStatusRecord, StoredSpecSyncProvenanceRecord,
};
pub use spec_query::{
    MaterializedSpecQueryEngine, SpecChecklistView, SpecCoverageView, SpecDependencyView,
    SpecDocumentView, SpecListEntry, SpecMetadataView, SpecQueryEngine, SpecQueryLookup,
    SpecSyncBriefView, SpecSyncProvenanceView,
};
pub use spec_refresh::{refresh_spec_materialization, SpecMaterializationRefreshResult};
