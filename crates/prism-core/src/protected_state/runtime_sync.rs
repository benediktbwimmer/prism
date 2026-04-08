use std::path::Path;

use anyhow::Result;
use prism_ir::PrismRuntimeCapabilities;
use prism_projections::{ConceptPacket, ConceptRelation, ContractPacket};
use prism_store::{CoordinationCheckpointStore, CoordinationJournal};

use crate::concept_events::load_repo_curated_concepts;
use crate::concept_relation_events::load_repo_concept_relations;
use crate::contract_events::load_repo_curated_contracts;
use crate::coordination_reads::load_eventual_coordination_plan_state_for_root;
use crate::memory_events::load_repo_memory_events;
use crate::protected_state::streams::ProtectedRepoStream;
use crate::published_plans::HydratedCoordinationPlanState;
use crate::repo_patch_events::load_repo_patch_events;
use crate::tracked_snapshot::{
    load_concept_snapshots, load_contract_snapshots, load_memory_snapshot_events,
    load_relation_snapshots,
};

#[derive(Debug, Clone, PartialEq, Default)]
pub(crate) struct RepoProtectedKnowledge {
    pub(crate) curated_concepts: Vec<ConceptPacket>,
    pub(crate) curated_contracts: Vec<ContractPacket>,
    pub(crate) concept_relations: Vec<ConceptRelation>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub(crate) struct ProtectedStateSyncReport {
    pub(crate) imported_memory_events: usize,
    pub(crate) imported_patch_events: usize,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub(crate) struct ProtectedStateImportSelection {
    pub(crate) memory: bool,
    pub(crate) patch_events: bool,
    pub(crate) concepts: bool,
    pub(crate) concept_relations: bool,
    pub(crate) contracts: bool,
}

impl ProtectedStateImportSelection {
    pub(crate) fn from_streams<'a>(
        streams: impl IntoIterator<Item = &'a ProtectedRepoStream>,
    ) -> Self {
        let mut selection = Self::default();
        for stream in streams {
            match stream.stream() {
                "repo_memory_events" => selection.memory = true,
                "repo_patch_events" => selection.patch_events = true,
                "repo_concept_events" => selection.concepts = true,
                "repo_concept_relations" => selection.concept_relations = true,
                "repo_contract_events" => selection.contracts = true,
                _ => {}
            }
        }
        selection
    }

    pub(crate) fn reloads_projection_knowledge(self) -> bool {
        self.concepts || self.concept_relations || self.contracts
    }

    pub(crate) fn reloads_coordination(self) -> bool {
        false
    }

    pub(crate) fn is_empty(self) -> bool {
        !self.memory
            && !self.patch_events
            && !self.concepts
            && !self.concept_relations
            && !self.contracts
    }

    pub(crate) fn filtered_for_runtime(self, runtime: PrismRuntimeCapabilities) -> Self {
        Self {
            memory: self.memory && runtime.knowledge_storage_enabled(),
            patch_events: self.patch_events && runtime.knowledge_storage_enabled(),
            concepts: self.concepts && runtime.knowledge_storage_enabled(),
            concept_relations: self.concept_relations && runtime.knowledge_storage_enabled(),
            contracts: self.contracts && runtime.knowledge_storage_enabled(),
        }
    }
}

pub(crate) fn load_repo_protected_knowledge(root: &Path) -> Result<RepoProtectedKnowledge> {
    if !has_repo_protected_state(root) {
        return Ok(RepoProtectedKnowledge {
            curated_concepts: Vec::new(),
            curated_contracts: Vec::new(),
            concept_relations: Vec::new(),
        });
    }
    let snapshot_concepts = load_concept_snapshots(root)?;
    let snapshot_contracts = load_contract_snapshots(root)?;
    let snapshot_relations = load_relation_snapshots(root)?;
    if !snapshot_concepts.is_empty()
        || !snapshot_contracts.is_empty()
        || !snapshot_relations.is_empty()
    {
        return Ok(RepoProtectedKnowledge {
            curated_concepts: snapshot_concepts,
            curated_contracts: snapshot_contracts,
            concept_relations: snapshot_relations,
        });
    }
    Ok(RepoProtectedKnowledge {
        curated_concepts: load_repo_concept_stream(root)?,
        curated_contracts: load_repo_contract_stream(root)?,
        concept_relations: load_repo_concept_relation_stream(root)?,
    })
}

pub(crate) fn load_repo_protected_knowledge_for_runtime(
    root: &Path,
    runtime: PrismRuntimeCapabilities,
) -> Result<RepoProtectedKnowledge> {
    if !runtime.knowledge_storage_enabled() {
        return Ok(RepoProtectedKnowledge::default());
    }
    load_repo_protected_knowledge(root)
}

pub(crate) fn sync_repo_protected_state<S: prism_store::EventJournalStore>(
    root: &Path,
    store: &mut S,
    runtime: PrismRuntimeCapabilities,
) -> Result<ProtectedStateSyncReport> {
    sync_selected_repo_protected_state(
        root,
        store,
        ProtectedStateImportSelection {
            memory: true,
            patch_events: true,
            concepts: true,
            concept_relations: true,
            contracts: true,
        }
        .filtered_for_runtime(runtime),
    )
}

pub(crate) fn sync_selected_repo_protected_state<S: prism_store::EventJournalStore>(
    root: &Path,
    store: &mut S,
    selection: ProtectedStateImportSelection,
) -> Result<ProtectedStateSyncReport> {
    if selection.is_empty() || !has_repo_protected_state(root) {
        return Ok(ProtectedStateSyncReport::default());
    }
    let imported_memory_events = if selection.memory {
        sync_repo_memory_stream(root, store)?
    } else {
        0
    };
    let imported_patch_events = if selection.patch_events {
        sync_repo_patch_stream(root, store)?
    } else {
        0
    };
    Ok(ProtectedStateSyncReport {
        imported_memory_events,
        imported_patch_events,
    })
}

pub(crate) fn load_repo_protected_plan_state<S>(
    root: &Path,
    store: &mut S,
) -> Result<Option<HydratedCoordinationPlanState>>
where
    S: CoordinationJournal + CoordinationCheckpointStore + ?Sized,
{
    let _ = store;
    load_eventual_coordination_plan_state_for_root(root)
}

fn sync_repo_memory_stream<S: prism_store::EventJournalStore>(
    root: &Path,
    store: &mut S,
) -> Result<usize> {
    let memory_events = load_repo_memory_stream(root)?;
    if memory_events.is_empty() {
        return Ok(0);
    }
    prism_store::EventJournalStore::append_memory_events(store, &memory_events)
}

fn sync_repo_patch_stream<S: prism_store::EventJournalStore>(
    root: &Path,
    store: &mut S,
) -> Result<usize> {
    let patch_events = load_repo_patch_stream(root)?;
    if patch_events.is_empty() {
        return Ok(0);
    }
    prism_store::EventJournalStore::append_outcome_events(store, &patch_events, &[])
}

fn load_repo_memory_stream(root: &Path) -> Result<Vec<prism_memory::MemoryEvent>> {
    let snapshots = load_memory_snapshot_events(root)?;
    if !snapshots.is_empty() {
        return Ok(snapshots);
    }
    load_repo_memory_events(root)
}

fn load_repo_patch_stream(root: &Path) -> Result<Vec<prism_memory::OutcomeEvent>> {
    load_repo_patch_events(root)
}

fn load_repo_concept_stream(root: &Path) -> Result<Vec<ConceptPacket>> {
    load_repo_curated_concepts(root)
}

fn load_repo_contract_stream(root: &Path) -> Result<Vec<ContractPacket>> {
    load_repo_curated_contracts(root)
}

fn load_repo_concept_relation_stream(root: &Path) -> Result<Vec<ConceptRelation>> {
    load_repo_concept_relations(root)
}

fn has_repo_protected_state(root: &Path) -> bool {
    root.join(".prism").is_dir()
}

#[cfg(test)]
mod tests {
    use std::fs;

    use prism_ir::{new_sortable_token, PrismRuntimeMode};
    use prism_store::MemoryStore;

    use super::*;

    #[test]
    fn missing_prism_dir_skips_protected_state_imports() {
        let root =
            std::env::temp_dir().join(format!("prism-runtime-sync-{}", new_sortable_token()));
        let _ = fs::remove_dir_all(&root);
        fs::create_dir_all(&root).expect("temp workspace should be created");

        let knowledge =
            load_repo_protected_knowledge(&root).expect("protected knowledge load should succeed");
        assert!(knowledge.curated_concepts.is_empty());
        assert!(knowledge.curated_contracts.is_empty());
        assert!(knowledge.concept_relations.is_empty());

        let mut store = MemoryStore::default();
        let report =
            sync_repo_protected_state(&root, &mut store, PrismRuntimeMode::Full.capabilities())
                .expect("protected state sync should succeed");
        assert_eq!(report, ProtectedStateSyncReport::default());

        let _ = fs::remove_dir_all(root);
    }

    #[test]
    fn runtime_filtering_disables_protected_state_imports_for_coordination_only() {
        let selection = ProtectedStateImportSelection {
            memory: true,
            patch_events: true,
            concepts: true,
            concept_relations: true,
            contracts: true,
        }
        .filtered_for_runtime(PrismRuntimeMode::CoordinationOnly.capabilities());

        assert!(selection.is_empty());
    }
}
