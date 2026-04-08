mod discovery;
mod parse;
mod types;

pub use discovery::{discover_spec_sources, resolve_spec_root};
pub use parse::{parse_spec_source, parse_spec_sources};
pub use types::{
    DiscoveredSpecSource, ParsedSpecDocument, SpecChecklistIdentitySource,
    ParsedSpecSet, SpecChecklistItem, SpecChecklistRequirementLevel, SpecDeclaredStatus,
    SpecDependency, SpecParseDiagnostic, SpecParseDiagnosticKind, SpecRootResolution,
    SpecRootSource, SpecSourceMetadata,
};
