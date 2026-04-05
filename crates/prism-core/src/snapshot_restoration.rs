use std::collections::{BTreeMap, BTreeSet};
use std::fs::File;
use std::io::{BufRead, BufReader};
use std::path::{Path, PathBuf};

use anyhow::{bail, Context, Result};
use prism_memory::{MemoryEvent, MemoryEventKind};
use prism_projections::{
    curated_concepts_from_events, curated_contracts_from_events, concept_relations_from_events,
    ConceptEvent, ConceptRelation, ConceptRelationEvent, ContractEvent,
};
use serde::de::DeserializeOwned;

use crate::concept_events::append_repo_concept_event;
use crate::concept_relation_events::append_repo_concept_relation_event;
use crate::contract_events::append_repo_contract_event;
use crate::memory_events::append_repo_memory_event;
use crate::tracked_snapshot::{
    load_concept_snapshots, load_contract_snapshots, load_memory_snapshot_events,
    load_relation_snapshots, regenerate_tracked_snapshot_derived_artifacts,
};

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct LegacyRepoKnowledgeRestoreReport {
    pub source_paths: Vec<PathBuf>,
    pub restored_concepts: usize,
    pub skipped_existing_concepts: usize,
    pub restored_contracts: usize,
    pub skipped_existing_contracts: usize,
    pub restored_relations: usize,
    pub skipped_existing_relations: usize,
    pub restored_memories: usize,
    pub skipped_existing_memories: usize,
}

pub fn restore_legacy_repo_published_knowledge(
    root: impl AsRef<Path>,
) -> Result<LegacyRepoKnowledgeRestoreReport> {
    let root = root.as_ref();
    let current_concepts = load_concept_snapshots(root)?
        .into_iter()
        .map(|packet| packet.handle)
        .collect::<BTreeSet<_>>();
    let current_contracts = load_contract_snapshots(root)?
        .into_iter()
        .map(|packet| packet.handle)
        .collect::<BTreeSet<_>>();
    let current_relations = load_relation_snapshots(root)?
        .into_iter()
        .map(|relation| relation_identity(&relation))
        .collect::<BTreeSet<_>>();
    let current_memories = load_memory_snapshot_events(root)?
        .into_iter()
        .map(|event| event.memory_id.0)
        .collect::<BTreeSet<_>>();

    let mut report = LegacyRepoKnowledgeRestoreReport::default();

    if let Some((path, events)) =
        load_optional_legacy_jsonl::<ConceptEvent>(root, ".prism/concepts/events.jsonl")?
    {
        report.source_paths.push(path);
        let latest_by_handle = latest_concept_events_by_handle(&events);
        for packet in curated_concepts_from_events(&events) {
            if current_concepts.contains(&packet.handle) {
                report.skipped_existing_concepts += 1;
                continue;
            }
            let event = latest_by_handle
                .get(packet.handle.as_str())
                .with_context(|| format!("missing latest legacy concept event for `{}`", packet.handle))?;
            append_repo_concept_event(root, event)?;
            report.restored_concepts += 1;
        }
    }

    if let Some((path, events)) =
        load_optional_legacy_jsonl::<ConceptRelationEvent>(root, ".prism/concepts/relations.jsonl")?
    {
        report.source_paths.push(path);
        let latest_by_relation = latest_relation_events_by_identity(&events);
        for relation in concept_relations_from_events(&events) {
            let identity = relation_identity(&relation);
            if current_relations.contains(&identity) {
                report.skipped_existing_relations += 1;
                continue;
            }
            let event = latest_by_relation
                .get(identity.as_str())
                .with_context(|| format!("missing latest legacy concept relation event for `{identity}`"))?;
            append_repo_concept_relation_event(root, event)?;
            report.restored_relations += 1;
        }
    }

    if let Some((path, events)) =
        load_optional_legacy_jsonl::<ContractEvent>(root, ".prism/contracts/events.jsonl")?
    {
        report.source_paths.push(path);
        let latest_by_handle = latest_contract_events_by_handle(&events);
        for packet in curated_contracts_from_events(&events) {
            if current_contracts.contains(&packet.handle) {
                report.skipped_existing_contracts += 1;
                continue;
            }
            let event = latest_by_handle
                .get(packet.handle.as_str())
                .with_context(|| format!("missing latest legacy contract event for `{}`", packet.handle))?;
            append_repo_contract_event(root, event)?;
            report.restored_contracts += 1;
        }
    }

    if let Some((path, events)) =
        load_optional_legacy_jsonl::<MemoryEvent>(root, ".prism/memory/events.jsonl")?
    {
        report.source_paths.push(path);
        let active_memory_ids = project_active_memory_ids(&events);
        let missing_active_ids = active_memory_ids
            .difference(&current_memories)
            .cloned()
            .collect::<BTreeSet<_>>();

        report.skipped_existing_memories += active_memory_ids
            .intersection(&current_memories)
            .count();

        let mut sorted_events = events;
        sorted_events.sort_by(|left, right| {
            left.recorded_at
                .cmp(&right.recorded_at)
                .then_with(|| left.id.cmp(&right.id))
        });
        for event in sorted_events {
            if !missing_active_ids.contains(&event.memory_id.0) {
                continue;
            }
            append_repo_memory_event(root, &event)?;
        }
        report.restored_memories += missing_active_ids.len();
    }

    if report.source_paths.is_empty() {
        bail!("no legacy repo knowledge backups were found");
    }

    regenerate_tracked_snapshot_derived_artifacts(root)?;
    Ok(report)
}

fn load_optional_legacy_jsonl<T>(root: &Path, relative_path: &str) -> Result<Option<(PathBuf, Vec<T>)>>
where
    T: DeserializeOwned,
{
    let live_path = root.join(relative_path);
    let Some(extension) = live_path.extension().and_then(|ext| ext.to_str()) else {
        return Ok(None);
    };
    let backup_path = live_path.with_extension(format!("{extension}.legacy-unsigned.bak"));
    for candidate in [backup_path, live_path] {
        if candidate.exists() {
            return Ok(Some((candidate.clone(), load_jsonl_records(&candidate)?)));
        }
    }
    Ok(None)
}

fn load_jsonl_records<T>(path: &Path) -> Result<Vec<T>>
where
    T: DeserializeOwned,
{
    let file = File::open(path).with_context(|| format!("failed to open {}", path.display()))?;
    let reader = BufReader::new(file);
    let mut values = Vec::new();
    for (index, line) in reader.lines().enumerate() {
        let line = line.with_context(|| format!("failed to read line {} from {}", index + 1, path.display()))?;
        if line.trim().is_empty() {
            continue;
        }
        let value = serde_json::from_str::<T>(&line).with_context(|| {
            format!(
                "failed to parse JSONL record on line {} in {}",
                index + 1,
                path.display()
            )
        })?;
        values.push(value);
    }
    Ok(values)
}

fn latest_concept_events_by_handle(events: &[ConceptEvent]) -> BTreeMap<&str, ConceptEvent> {
    let mut latest = BTreeMap::new();
    for event in events {
        latest.insert(event.concept.handle.as_str(), event.clone());
    }
    latest
}

fn latest_contract_events_by_handle(events: &[ContractEvent]) -> BTreeMap<&str, ContractEvent> {
    let mut latest = BTreeMap::new();
    for event in events {
        latest.insert(event.contract.handle.as_str(), event.clone());
    }
    latest
}

fn latest_relation_events_by_identity(
    events: &[ConceptRelationEvent],
) -> BTreeMap<String, ConceptRelationEvent> {
    let mut latest = BTreeMap::new();
    for event in events {
        latest.insert(relation_identity(&event.relation), event.clone());
    }
    latest
}

fn relation_identity(relation: &ConceptRelation) -> String {
    format!(
        "{}|{}|{:?}",
        relation.source_handle.trim().to_ascii_lowercase(),
        relation.target_handle.trim().to_ascii_lowercase(),
        relation.kind
    )
}

fn project_active_memory_ids(events: &[MemoryEvent]) -> BTreeSet<String> {
    let mut sorted = events.to_vec();
    sorted.sort_by(|left, right| {
        left.recorded_at
            .cmp(&right.recorded_at)
            .then_with(|| left.id.cmp(&right.id))
    });

    let mut active = BTreeSet::new();
    for event in sorted {
        for superseded in &event.supersedes {
            active.remove(&superseded.0);
        }
        match event.action {
            MemoryEventKind::Stored | MemoryEventKind::Promoted | MemoryEventKind::Superseded => {
                if event.entry.is_some() {
                    active.insert(event.memory_id.0.clone());
                }
            }
            MemoryEventKind::Retired => {
                active.remove(&event.memory_id.0);
            }
        }
    }
    active
}

#[cfg(test)]
mod tests {
    use std::fs;
    use std::path::PathBuf;

    use prism_memory::{
        MemoryEntry, MemoryEvent, MemoryEventKind, MemoryId, MemoryKind, MemoryScope,
        MemorySource,
    };
    use prism_projections::{
        ConceptEvent, ConceptEventAction, ConceptPacket, ConceptProvenance, ConceptPublication,
        ConceptPublicationStatus, ConceptRelation, ConceptRelationEvent,
        ConceptRelationEventAction, ConceptRelationKind, ConceptScope, ContractCompatibility,
        ContractEvent, ContractEventAction, ContractKind, ContractPacket, ContractPublication,
        ContractPublicationStatus, ContractScope, ContractStatus, ContractTarget,
    };

    use super::restore_legacy_repo_published_knowledge;
    use crate::concept_events::append_repo_concept_event;
    use crate::concept_relation_events::append_repo_concept_relation_event;
    use crate::contract_events::append_repo_contract_event;
    use crate::memory_events::append_repo_memory_event;
    use crate::tracked_snapshot::{
        load_concept_snapshots, load_contract_snapshots, load_memory_snapshot_events,
        load_relation_snapshots,
    };

    #[test]
    fn restores_legacy_repo_knowledge_backups_into_snapshot_state() {
        let root = temp_workspace();

        append_repo_concept_event(&root, &current_concept_event()).unwrap();
        append_repo_contract_event(&root, &current_contract_event()).unwrap();
        append_repo_concept_relation_event(&root, &current_relation_event()).unwrap();
        append_repo_memory_event(&root, &current_memory_event()).unwrap();

        write_jsonl(
            root.join(".prism/concepts/events.jsonl.legacy-unsigned.bak"),
            &[legacy_concept_event()],
        );
        write_jsonl(
            root.join(".prism/concepts/relations.jsonl.legacy-unsigned.bak"),
            &[legacy_relation_event()],
        );
        write_jsonl(
            root.join(".prism/contracts/events.jsonl.legacy-unsigned.bak"),
            &[legacy_contract_event()],
        );
        write_jsonl(
            root.join(".prism/memory/events.jsonl.legacy-unsigned.bak"),
            &[legacy_memory_event()],
        );

        let report = restore_legacy_repo_published_knowledge(&root).unwrap();
        assert_eq!(report.restored_concepts, 1);
        assert_eq!(report.restored_contracts, 1);
        assert_eq!(report.restored_relations, 1);
        assert_eq!(report.restored_memories, 1);
        assert_eq!(report.skipped_existing_concepts, 0);
        assert_eq!(report.skipped_existing_contracts, 0);
        assert_eq!(report.skipped_existing_relations, 0);
        assert_eq!(report.skipped_existing_memories, 0);

        let concepts = load_concept_snapshots(&root).unwrap();
        assert_eq!(concepts.len(), 2);
        assert!(concepts.iter().any(|packet| packet.handle == "concept://legacy-alpha"));
        assert!(concepts.iter().any(|packet| packet.handle == "concept://current-alpha"));

        let contracts = load_contract_snapshots(&root).unwrap();
        assert_eq!(contracts.len(), 2);
        assert!(contracts
            .iter()
            .any(|packet| packet.handle == "contract://legacy-alpha"));

        let relations = load_relation_snapshots(&root).unwrap();
        assert_eq!(relations.len(), 2);
        assert!(relations.iter().any(|relation| {
            relation.source_handle == "concept://legacy-alpha"
                && relation.target_handle == "concept://legacy-platform"
        }));

        let memories = load_memory_snapshot_events(&root).unwrap();
        assert_eq!(memories.len(), 2);
        assert!(memories
            .iter()
            .any(|event| event.memory_id.0 == "memory:legacy-alpha"));

        let _ = fs::remove_dir_all(root);
    }

    fn temp_workspace() -> PathBuf {
        let root = std::env::temp_dir().join(format!(
            "prism-legacy-restore-test-{}",
            prism_ir::new_sortable_token()
        ));
        fs::create_dir_all(root.join("src")).unwrap();
        fs::write(
            root.join("Cargo.toml"),
            "[package]\nname = \"demo\"\nversion = \"0.1.0\"\n",
        )
        .unwrap();
        fs::write(root.join("src/lib.rs"), "pub fn demo() {}\n").unwrap();
        root
    }

    fn write_jsonl<T: serde::Serialize>(path: PathBuf, records: &[T]) {
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent).unwrap();
        }
        let mut body = String::new();
        for record in records {
            body.push_str(&serde_json::to_string(record).unwrap());
            body.push('\n');
        }
        fs::write(path, body).unwrap();
    }

    fn current_concept_event() -> ConceptEvent {
        ConceptEvent {
            id: "concept-event:current".to_string(),
            recorded_at: 20,
            task_id: Some("task:current-concept".to_string()),
            actor: None,
            execution_context: None,
            action: ConceptEventAction::Promote,
            patch: None,
            concept: concept_packet("concept://current-alpha", "current alpha", 20),
        }
    }

    fn legacy_concept_event() -> ConceptEvent {
        ConceptEvent {
            id: "concept-event:legacy".to_string(),
            recorded_at: 10,
            task_id: Some("task:legacy-concept".to_string()),
            actor: None,
            execution_context: None,
            action: ConceptEventAction::Promote,
            patch: None,
            concept: concept_packet("concept://legacy-alpha", "legacy alpha", 10),
        }
    }

    fn concept_packet(handle: &str, name: &str, published_at: u64) -> ConceptPacket {
        ConceptPacket {
            handle: handle.to_string(),
            canonical_name: name.to_string(),
            summary: format!("{name} summary"),
            aliases: vec![name.to_string()],
            confidence: 0.9,
            core_members: Vec::new(),
            core_member_lineages: Vec::new(),
            supporting_members: Vec::new(),
            supporting_member_lineages: Vec::new(),
            likely_tests: Vec::new(),
            likely_test_lineages: Vec::new(),
            evidence: vec!["test evidence".to_string()],
            risk_hint: None,
            decode_lenses: Vec::new(),
            scope: ConceptScope::Repo,
            provenance: ConceptProvenance {
                origin: "test".to_string(),
                kind: "legacy_restore".to_string(),
                task_id: None,
            },
            publication: Some(ConceptPublication {
                published_at,
                last_reviewed_at: Some(published_at),
                status: ConceptPublicationStatus::Active,
                supersedes: Vec::new(),
                retired_at: None,
                retirement_reason: None,
            }),
        }
    }

    fn current_relation_event() -> ConceptRelationEvent {
        ConceptRelationEvent {
            id: "concept-relation-event:current".to_string(),
            recorded_at: 20,
            task_id: Some("task:current-relation".to_string()),
            actor: None,
            execution_context: None,
            action: ConceptRelationEventAction::Upsert,
            relation: ConceptRelation {
                source_handle: "concept://current-alpha".to_string(),
                target_handle: "concept://current-platform".to_string(),
                kind: ConceptRelationKind::DependsOn,
                confidence: 0.8,
                evidence: vec!["current relation".to_string()],
                scope: ConceptScope::Repo,
                provenance: prism_projections::ConceptProvenance {
                    origin: "test".to_string(),
                    kind: "legacy_restore".to_string(),
                    task_id: None,
                },
            },
        }
    }

    fn legacy_relation_event() -> ConceptRelationEvent {
        ConceptRelationEvent {
            id: "concept-relation-event:legacy".to_string(),
            recorded_at: 10,
            task_id: Some("task:legacy-relation".to_string()),
            actor: None,
            execution_context: None,
            action: ConceptRelationEventAction::Upsert,
            relation: ConceptRelation {
                source_handle: "concept://legacy-alpha".to_string(),
                target_handle: "concept://legacy-platform".to_string(),
                kind: ConceptRelationKind::PartOf,
                confidence: 0.8,
                evidence: vec!["legacy relation".to_string()],
                scope: ConceptScope::Repo,
                provenance: prism_projections::ConceptProvenance {
                    origin: "test".to_string(),
                    kind: "legacy_restore".to_string(),
                    task_id: None,
                },
            },
        }
    }

    fn current_contract_event() -> ContractEvent {
        ContractEvent {
            id: "contract-event:current".to_string(),
            recorded_at: 20,
            task_id: Some("task:current-contract".to_string()),
            actor: None,
            execution_context: None,
            action: ContractEventAction::Promote,
            patch: None,
            contract: contract_packet("contract://current-alpha", 20),
        }
    }

    fn legacy_contract_event() -> ContractEvent {
        ContractEvent {
            id: "contract-event:legacy".to_string(),
            recorded_at: 10,
            task_id: Some("task:legacy-contract".to_string()),
            actor: None,
            execution_context: None,
            action: ContractEventAction::Promote,
            patch: None,
            contract: contract_packet("contract://legacy-alpha", 10),
        }
    }

    fn contract_packet(handle: &str, published_at: u64) -> ContractPacket {
        ContractPacket {
            handle: handle.to_string(),
            name: handle.trim_start_matches("contract://").to_string(),
            summary: format!("{handle} summary"),
            aliases: vec![handle.to_string()],
            kind: ContractKind::Lifecycle,
            subject: ContractTarget {
                anchors: Vec::new(),
                concept_handles: Vec::new(),
            },
            guarantees: Vec::new(),
            assumptions: Vec::new(),
            consumers: Vec::new(),
            validations: Vec::new(),
            stability: prism_query::ContractStability::Internal,
            compatibility: ContractCompatibility::default(),
            evidence: vec!["test evidence".to_string()],
            status: ContractStatus::Active,
            scope: ContractScope::Repo,
            provenance: prism_query::ContractProvenance {
                origin: "test".to_string(),
                kind: "legacy_restore".to_string(),
                task_id: None,
            },
            publication: Some(ContractPublication {
                published_at,
                last_reviewed_at: Some(published_at),
                status: ContractPublicationStatus::Active,
                supersedes: Vec::new(),
                retired_at: None,
                retirement_reason: None,
            }),
        }
    }

    fn current_memory_event() -> MemoryEvent {
        let mut entry = MemoryEntry::new(MemoryKind::Structural, "current memory");
        entry.id = MemoryId("memory:current-alpha".to_string());
        entry.scope = MemoryScope::Repo;
        entry.source = MemorySource::Agent;
        entry.trust = 0.9;
        let mut event = MemoryEvent::from_entry(
            MemoryEventKind::Promoted,
            entry,
            Some("task:current-memory".to_string()),
            Vec::new(),
            Vec::new(),
        );
        event.id = "memory-event:current".to_string();
        event.recorded_at = 20;
        event
    }

    fn legacy_memory_event() -> MemoryEvent {
        let mut entry = MemoryEntry::new(MemoryKind::Episodic, "legacy memory");
        entry.id = MemoryId("memory:legacy-alpha".to_string());
        entry.scope = MemoryScope::Repo;
        entry.source = MemorySource::Agent;
        entry.trust = 0.9;
        let mut event = MemoryEvent::from_entry(
            MemoryEventKind::Promoted,
            entry,
            Some("task:legacy-memory".to_string()),
            Vec::new(),
            Vec::new(),
        );
        event.id = "memory-event:legacy".to_string();
        event.recorded_at = 10;
        event
    }
}
