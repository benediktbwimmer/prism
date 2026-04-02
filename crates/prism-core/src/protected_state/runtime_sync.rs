use std::path::Path;

use anyhow::Result;
use prism_projections::{ConceptPacket, ConceptRelation, ContractPacket};

use crate::concept_events::load_repo_curated_concepts;
use crate::concept_relation_events::load_repo_concept_relations;
use crate::contract_events::load_repo_curated_contracts;
use crate::memory_events::load_repo_memory_events;
use crate::repo_patch_events::load_repo_patch_events;

#[derive(Debug, Clone, PartialEq)]
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

pub(crate) fn load_repo_protected_knowledge(root: &Path) -> Result<RepoProtectedKnowledge> {
    Ok(RepoProtectedKnowledge {
        curated_concepts: load_repo_curated_concepts(root)?,
        curated_contracts: load_repo_curated_contracts(root)?,
        concept_relations: load_repo_concept_relations(root)?,
    })
}

pub(crate) fn sync_repo_protected_state<S: prism_store::EventJournalStore>(
    root: &Path,
    store: &mut S,
) -> Result<ProtectedStateSyncReport> {
    let memory_events = load_repo_memory_events(root)?;
    let patch_events = load_repo_patch_events(root)?;
    let imported_memory_events = if memory_events.is_empty() {
        0
    } else {
        prism_store::EventJournalStore::append_memory_events(store, &memory_events)?
    };
    let imported_patch_events = if patch_events.is_empty() {
        0
    } else {
        prism_store::EventJournalStore::append_outcome_events(store, &patch_events, &[])?
    };
    Ok(ProtectedStateSyncReport {
        imported_memory_events,
        imported_patch_events,
    })
}
