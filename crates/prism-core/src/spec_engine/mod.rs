mod discovery;
mod parse;
mod types;

pub use discovery::{discover_spec_sources, resolve_spec_root};
pub use parse::parse_spec_source;
pub use types::{
    DiscoveredSpecSource, ParsedSpecDocument, SpecChecklistIdentitySource,
    SpecChecklistItem, SpecChecklistRequirementLevel, SpecDeclaredStatus, SpecParseDiagnostic,
    SpecParseDiagnosticKind, SpecRootResolution, SpecRootSource,
};
