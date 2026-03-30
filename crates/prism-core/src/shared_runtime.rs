use std::collections::BTreeMap;

use prism_history::HistorySnapshot;
use prism_memory::{
    EpisodicMemorySnapshot, MemoryEntry, MemoryEvent, MemoryScope, OutcomeMemorySnapshot,
};
use prism_projections::{
    ConceptPacket, ConceptRelation, ConceptScope, ProjectionIndex, ProjectionSnapshot,
};

pub(crate) fn composite_workspace_revision(
    local_revision: u64,
    shared_revision: Option<u64>,
) -> u64 {
    shared_revision.map_or(local_revision, |shared| local_revision.max(shared))
}

pub(crate) fn merge_episodic_snapshots(
    local: Option<EpisodicMemorySnapshot>,
    shared: Option<EpisodicMemorySnapshot>,
) -> Option<EpisodicMemorySnapshot> {
    let mut entries = BTreeMap::<String, MemoryEntry>::new();
    for snapshot in [local, shared].into_iter().flatten() {
        for entry in snapshot.entries {
            entries.insert(entry.id.0.clone(), entry);
        }
    }
    if entries.is_empty() {
        None
    } else {
        Some(EpisodicMemorySnapshot {
            entries: entries.into_values().collect(),
        })
    }
}

pub(crate) fn merge_memory_events(
    local: Vec<MemoryEvent>,
    shared: Vec<MemoryEvent>,
) -> Vec<MemoryEvent> {
    let mut events = BTreeMap::<String, MemoryEvent>::new();
    for event in local.into_iter().chain(shared) {
        events.insert(event.id.clone(), event);
    }
    let mut merged = events.into_values().collect::<Vec<_>>();
    merged.sort_by(|left, right| {
        left.recorded_at
            .cmp(&right.recorded_at)
            .then_with(|| left.id.cmp(&right.id))
    });
    merged
}

pub(crate) fn local_projection_snapshot_for_persist(
    snapshot: &ProjectionSnapshot,
) -> ProjectionSnapshot {
    ProjectionSnapshot {
        co_change_by_lineage: snapshot.co_change_by_lineage.clone(),
        validation_by_lineage: snapshot.validation_by_lineage.clone(),
        curated_concepts: snapshot
            .curated_concepts
            .iter()
            .filter(|concept| concept.scope == ConceptScope::Local)
            .cloned()
            .collect(),
        concept_relations: snapshot
            .concept_relations
            .iter()
            .filter(|relation| relation.scope == ConceptScope::Local)
            .cloned()
            .collect(),
    }
}

pub(crate) fn overlay_persisted_projection_knowledge(
    projections: &mut ProjectionIndex,
    snapshots: impl IntoIterator<Item = ProjectionSnapshot>,
) {
    let mut combined_concepts = projections.curated_concepts().to_vec();
    let mut combined_relations = projections.concept_relations().to_vec();
    for snapshot in snapshots {
        combined_concepts.extend(snapshot.curated_concepts);
        combined_relations.extend(snapshot.concept_relations);
    }
    projections.replace_curated_concepts(combined_concepts);
    projections.replace_concept_relations(combined_relations);
}

pub(crate) fn merged_projection_index(
    local_snapshot: Option<ProjectionSnapshot>,
    shared_snapshot: Option<ProjectionSnapshot>,
    repo_concepts: Vec<ConceptPacket>,
    repo_contracts: Vec<prism_projections::ContractPacket>,
    repo_relations: Vec<ConceptRelation>,
    history: &HistorySnapshot,
    outcomes: &OutcomeMemorySnapshot,
) -> ProjectionIndex {
    let mut projections = local_snapshot
        .map(|snapshot| ProjectionIndex::from_snapshot_with_history(snapshot, Some(history)))
        .unwrap_or_else(|| ProjectionIndex::derive(history, outcomes));
    let keep_local_only = shared_snapshot.is_some();
    let local_concepts = projections
        .curated_concepts()
        .iter()
        .filter(|concept| {
            if keep_local_only {
                concept.scope == ConceptScope::Local
            } else {
                concept.scope != ConceptScope::Repo
            }
        })
        .cloned()
        .collect::<Vec<_>>();
    let local_relations = projections
        .concept_relations()
        .iter()
        .filter(|relation| {
            if keep_local_only {
                relation.scope == ConceptScope::Local
            } else {
                relation.scope != ConceptScope::Repo
            }
        })
        .cloned()
        .collect::<Vec<_>>();

    let mut combined_concepts = repo_concepts;
    combined_concepts.extend(local_concepts);
    if let Some(shared_snapshot) = &shared_snapshot {
        combined_concepts.extend(shared_snapshot.curated_concepts.clone());
    }
    projections.replace_curated_concepts(combined_concepts);

    let mut combined_relations = repo_relations;
    combined_relations.extend(local_relations);
    if let Some(shared_snapshot) = shared_snapshot {
        combined_relations.extend(shared_snapshot.concept_relations);
    }
    projections.replace_concept_relations(combined_relations);
    projections.replace_curated_contracts(repo_contracts);
    projections
}

pub(crate) fn split_episodic_snapshot_for_persist(
    snapshot: &EpisodicMemorySnapshot,
) -> (EpisodicMemorySnapshot, EpisodicMemorySnapshot) {
    let mut local_entries = Vec::new();
    let mut shared_entries = Vec::new();
    for entry in &snapshot.entries {
        match entry.scope {
            MemoryScope::Local => local_entries.push(entry.clone()),
            MemoryScope::Session | MemoryScope::Repo => shared_entries.push(entry.clone()),
        }
    }
    (
        EpisodicMemorySnapshot {
            entries: local_entries,
        },
        EpisodicMemorySnapshot {
            entries: shared_entries,
        },
    )
}
