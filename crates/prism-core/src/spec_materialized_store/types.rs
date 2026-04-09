use crate::ParsedSpecDocument;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SpecMaterializedBackendKind {
    Sqlite,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SpecMaterializedCapabilities {
    pub supports_replace_from_parsed_batch: bool,
    pub supports_checklist_items: bool,
    pub supports_dependencies: bool,
    pub supports_source_metadata: bool,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SpecMaterializationMetadata {
    pub backend_kind: SpecMaterializedBackendKind,
    pub materialized_at: Option<u64>,
    pub spec_count: usize,
    pub checklist_item_count: usize,
    pub dependency_count: usize,
}

#[derive(Debug, Clone, PartialEq)]
pub struct SpecMaterializedReadEnvelope<T> {
    pub metadata: SpecMaterializationMetadata,
    pub value: T,
}

impl<T> SpecMaterializedReadEnvelope<T> {
    pub fn new(metadata: SpecMaterializationMetadata, value: T) -> Self {
        Self { metadata, value }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MaterializedSpecRecord {
    pub spec_id: String,
    pub source_path: String,
    pub title: String,
    pub declared_status: String,
    pub created: String,
    pub content_digest: String,
    pub git_revision: Option<String>,
    pub body: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct StoredSpecDependencyRecord {
    pub spec_id: String,
    pub position: usize,
    pub dependency_spec_id: String,
}

#[derive(Debug, Clone, PartialEq)]
pub struct SpecMaterializedReplaceRequest {
    pub parsed: Vec<ParsedSpecDocument>,
}

#[derive(Debug, Clone, PartialEq)]
pub struct SpecMaterializedWriteResult {
    pub metadata: SpecMaterializationMetadata,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct SpecMaterializedClearRequest {
    pub clear_specs: bool,
    pub clear_checklist_items: bool,
    pub clear_dependencies: bool,
    pub clear_metadata: bool,
}

impl SpecMaterializedClearRequest {
    pub const fn all() -> Self {
        Self {
            clear_specs: true,
            clear_checklist_items: true,
            clear_dependencies: true,
            clear_metadata: true,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::{SpecMaterializedClearRequest, SpecMaterializedReadEnvelope};

    #[test]
    fn clear_request_all_sets_every_flag() {
        let request = SpecMaterializedClearRequest::all();
        assert!(request.clear_specs);
        assert!(request.clear_checklist_items);
        assert!(request.clear_dependencies);
        assert!(request.clear_metadata);
    }

    #[test]
    fn read_envelope_wraps_value_with_metadata() {
        let metadata = super::SpecMaterializationMetadata {
            backend_kind: super::SpecMaterializedBackendKind::Sqlite,
            materialized_at: Some(1),
            spec_count: 2,
            checklist_item_count: 3,
            dependency_count: 4,
        };
        let envelope = SpecMaterializedReadEnvelope::new(metadata.clone(), vec![1usize, 2usize]);
        assert_eq!(envelope.metadata, metadata);
        assert_eq!(envelope.value, vec![1, 2]);
    }
}
