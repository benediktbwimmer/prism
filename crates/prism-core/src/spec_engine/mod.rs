mod discovery;
mod types;

pub use discovery::{discover_spec_sources, resolve_spec_root};
pub use types::{DiscoveredSpecSource, SpecRootResolution, SpecRootSource};
