mod spec_engine;
mod spec_materialized_store;

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
    SqliteSpecMaterializedStore, StoredSpecChecklistPosture, StoredSpecDependencyPosture,
    StoredSpecDependencyRecord, StoredSpecStatusRecord,
};
