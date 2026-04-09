use crate::{
    MaterializedSpecRecord, SpecMaterializationMetadata, StoredSpecChecklistItemRecord,
    StoredSpecCoverageRecord, StoredSpecDependencyRecord, StoredSpecStatusRecord,
    StoredSpecSyncProvenanceRecord,
};

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SpecQueryLookup<T> {
    Found(T),
    NotFound,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SpecListEntry {
    pub spec_id: String,
    pub title: String,
    pub source_path: String,
    pub declared_status: String,
    pub overall_status: Option<String>,
    pub created: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SpecDocumentView {
    pub record: MaterializedSpecRecord,
    pub status: Option<StoredSpecStatusRecord>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SpecMetadataView {
    pub materialization: SpecMaterializationMetadata,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SpecChecklistView {
    pub spec_id: String,
    pub items: Vec<StoredSpecChecklistItemRecord>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SpecDependencyView {
    pub spec_id: String,
    pub dependencies: Vec<StoredSpecDependencyRecord>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SpecCoverageView {
    pub spec_id: String,
    pub records: Vec<StoredSpecCoverageRecord>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SpecSyncBriefView {
    pub spec: SpecDocumentView,
    pub required_checklist_items: Vec<StoredSpecChecklistItemRecord>,
    pub coverage: Vec<StoredSpecCoverageRecord>,
    pub linked_coordination_refs: Vec<StoredSpecSyncProvenanceRecord>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SpecSyncProvenanceView {
    pub spec_id: String,
    pub records: Vec<StoredSpecSyncProvenanceRecord>,
}
