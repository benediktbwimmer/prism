use std::collections::{BTreeMap, BTreeSet};
use std::fs;
use std::path::{Path, PathBuf};

use anyhow::{Context, Result};
use base64::engine::general_purpose::STANDARD as BASE64_STANDARD;
use base64::Engine;
use ed25519_dalek::Signer;
use prism_coordination::{
    coordination_snapshot_from_plan_graphs, CoordinationEvent, CoordinationSnapshot, Plan,
};
use prism_ir::{
    EventActor, EventExecutionContext, PlanExecutionOverlay, PlanGraph, WorkContextKind,
    WorkContextSnapshot,
};
use prism_memory::{MemoryEntry, MemoryEvent, MemoryEventKind};
use prism_projections::{
    ConceptPacket, ConceptRelation, ConceptRelationEvent, ConceptRelationEventAction,
    ContractPacket,
};
use serde::{Deserialize, Serialize};

use crate::protected_state::canonical::{canonical_json_bytes, sha256_prefixed};
use crate::protected_state::envelope::ProtectedSignatureAlgorithm;
use crate::protected_state::repo_streams::{
    implicit_principal_identity, ProtectedPrincipalIdentity,
};
use crate::protected_state::trust::load_active_runtime_signing_key;
use crate::PrismPaths;

const SNAPSHOT_MANIFEST_VERSION: u32 = 1;
#[derive(Debug, Clone)]
pub(crate) struct TrackedSnapshotPublishContext {
    pub(crate) published_at: u64,
    pub(crate) principal: ProtectedPrincipalIdentity,
    pub(crate) work_context: Option<WorkContextSnapshot>,
    pub(crate) publish_summary: Option<SnapshotManifestPublishSummary>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct TrackedCoordinationMaterializationStatus {
    pub(crate) coordination_revision: u64,
    pub(crate) materialized_at: u64,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
struct SnapshotManifestPublisher {
    principal_authority_id: String,
    principal_id: String,
    credential_id: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
struct SnapshotManifestFile {
    path: String,
    sha256: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
struct SnapshotRetiredAuthority {
    authority: String,
    digest: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct SnapshotManifestPublishSummary {
    title: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    summary: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
struct SnapshotManifestSignature {
    algorithm: ProtectedSignatureAlgorithm,
    runtime_authority_id: String,
    runtime_key_id: String,
    trust_bundle_id: String,
    value: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
struct SnapshotManifest {
    version: u32,
    published_at: u64,
    publisher: SnapshotManifestPublisher,
    work_context: WorkContextSnapshot,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    publish_summary: Option<SnapshotManifestPublishSummary>,
    files: BTreeMap<String, SnapshotManifestFile>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    previous_manifest_digest: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    migration_source_digest: Option<String>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    retired_authorities: Vec<SnapshotRetiredAuthority>,
    signature: SnapshotManifestSignature,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct SnapshotManifestSigningView<'a> {
    version: u32,
    published_at: u64,
    publisher: &'a SnapshotManifestPublisher,
    work_context: &'a WorkContextSnapshot,
    publish_summary: &'a Option<SnapshotManifestPublishSummary>,
    files: &'a BTreeMap<String, SnapshotManifestFile>,
    previous_manifest_digest: &'a Option<String>,
    migration_source_digest: &'a Option<String>,
    retired_authorities: &'a [SnapshotRetiredAuthority],
    signature: SnapshotManifestSignatureMetadata<'a>,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct SnapshotManifestSignatureMetadata<'a> {
    algorithm: ProtectedSignatureAlgorithm,
    runtime_authority_id: &'a str,
    runtime_key_id: &'a str,
    trust_bundle_id: &'a str,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
struct SnapshotIndexEntry {
    id: String,
    title: String,
    status: String,
    path: String,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
struct SnapshotMemoryRecord {
    entry: MemoryEntry,
    latest_event_id: String,
    latest_recorded_at: u64,
    event_count: usize,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    actor: Option<EventActor>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    execution_context: Option<EventExecutionContext>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    task_id: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
struct SnapshotPlanRecord {
    plan: Plan,
    graph: PlanGraph,
    execution_overlays: Vec<PlanExecutionOverlay>,
}

#[derive(Debug, Clone)]
pub(crate) struct TrackedCoordinationSnapshotState {
    pub(crate) snapshot: CoordinationSnapshot,
    pub(crate) plan_graphs: Vec<PlanGraph>,
    pub(crate) execution_overlays: BTreeMap<String, Vec<PlanExecutionOverlay>>,
}

fn snapshot_root(root: &Path) -> PathBuf {
    root.join(".prism").join("state")
}

fn snapshot_manifest_path(root: &Path) -> PathBuf {
    snapshot_root(root).join("manifest.json")
}

fn load_snapshot_manifest(root: &Path) -> Result<Option<SnapshotManifest>> {
    let path = snapshot_manifest_path(root);
    if !path.exists() {
        return Ok(None);
    }
    Ok(Some(read_json_file::<SnapshotManifest>(&path)?))
}

pub(crate) fn tracked_snapshot_authority_active(root: &Path) -> Result<bool> {
    Ok(load_snapshot_manifest(root)?.is_some())
}

pub(crate) fn legacy_tracked_stream_bridge_active(root: &Path) -> Result<bool> {
    Ok(!tracked_snapshot_authority_active(root)?)
}

pub(crate) fn remove_obsolete_legacy_tracked_authority_artifacts(root: &Path) -> Result<()> {
    for relative in [
        ".prism/memory/events.jsonl",
        ".prism/concepts/events.jsonl",
        ".prism/concepts/relations.jsonl",
        ".prism/contracts/events.jsonl",
        ".prism/changes/events.jsonl",
        ".prism/plans/index.jsonl",
    ] {
        remove_file_if_exists(&root.join(relative))?;
    }
    for relative in [
        ".prism/plans/streams",
        ".prism/plans/active",
        ".prism/plans/archived",
    ] {
        remove_dir_all_if_exists(&root.join(relative))?;
    }
    for relative in [
        ".prism/changes",
        ".prism/memory",
        ".prism/contracts",
        ".prism/concepts",
        ".prism/plans",
    ] {
        remove_dir_if_empty(&root.join(relative))?;
    }
    Ok(())
}

fn snapshot_indexes_dir(root: &Path) -> PathBuf {
    snapshot_root(root).join("indexes")
}

fn snapshot_concepts_dir(root: &Path) -> PathBuf {
    snapshot_root(root).join("concepts")
}

fn snapshot_contracts_dir(root: &Path) -> PathBuf {
    snapshot_root(root).join("contracts")
}

fn snapshot_relations_dir(root: &Path) -> PathBuf {
    snapshot_root(root).join("relations")
}

fn snapshot_memory_dir(root: &Path) -> PathBuf {
    snapshot_root(root).join("memory")
}

fn snapshot_changes_dir(root: &Path) -> PathBuf {
    snapshot_root(root).join("changes")
}

fn snapshot_plans_dir(root: &Path) -> PathBuf {
    snapshot_root(root).join("plans")
}

fn snapshot_coordination_tasks_dir(root: &Path) -> PathBuf {
    snapshot_root(root).join("coordination").join("tasks")
}

fn snapshot_coordination_artifacts_dir(root: &Path) -> PathBuf {
    snapshot_root(root).join("coordination").join("artifacts")
}

fn relation_identity(relation: &ConceptRelation) -> String {
    format!(
        "{}|{}|{:?}",
        relation.source_handle.trim().to_ascii_lowercase(),
        relation.target_handle.trim().to_ascii_lowercase(),
        relation.kind
    )
}

fn snapshot_file_name(identity: &str) -> String {
    let mut stem = identity
        .chars()
        .map(|ch| {
            if ch.is_ascii_alphanumeric() || ch == '-' || ch == '_' {
                ch.to_ascii_lowercase()
            } else {
                '-'
            }
        })
        .collect::<String>();
    while stem.contains("--") {
        stem = stem.replace("--", "-");
    }
    let stem = stem.trim_matches('-');
    let stem = if stem.is_empty() { "snapshot" } else { stem };
    let digest = crate::util::stable_hash_bytes(identity.as_bytes());
    format!("{stem}-{digest:016x}.json")
}

fn concept_snapshot_path(root: &Path, handle: &str) -> PathBuf {
    snapshot_concepts_dir(root).join(snapshot_file_name(handle))
}

fn contract_snapshot_path(root: &Path, handle: &str) -> PathBuf {
    snapshot_contracts_dir(root).join(snapshot_file_name(handle))
}

fn relation_snapshot_path(root: &Path, relation: &ConceptRelation) -> PathBuf {
    snapshot_relations_dir(root).join(snapshot_file_name(&relation_identity(relation)))
}

fn memory_snapshot_path(root: &Path, memory_id: &str) -> PathBuf {
    snapshot_memory_dir(root).join(snapshot_file_name(memory_id))
}

pub(crate) fn publish_context_from_event(
    actor: Option<&EventActor>,
    execution_context: Option<&EventExecutionContext>,
    published_at: u64,
) -> TrackedSnapshotPublishContext {
    TrackedSnapshotPublishContext {
        published_at,
        principal: implicit_principal_identity(actor, execution_context),
        work_context: execution_context
            .and_then(|context| context.work_context.clone())
            .or_else(|| Some(implicit_work_context())),
        publish_summary: execution_context
            .and_then(|context| context.work_context.clone())
            .or_else(|| Some(implicit_work_context()))
            .map(|work_context| publish_summary_from_work_context(&work_context)),
    }
}

pub(crate) fn publish_context_from_coordination_events(
    appended_events: &[CoordinationEvent],
) -> Option<TrackedSnapshotPublishContext> {
    appended_events
        .last()
        .map(|event| TrackedSnapshotPublishContext {
            published_at: event.meta.ts,
            principal: implicit_principal_identity(
                Some(&event.meta.actor),
                event.meta.execution_context.as_ref(),
            ),
            work_context: event
                .meta
                .execution_context
                .as_ref()
                .and_then(|context| context.work_context.clone())
                .or_else(|| Some(implicit_work_context())),
            publish_summary: event
                .meta
                .execution_context
                .as_ref()
                .and_then(|context| context.work_context.clone())
                .or_else(|| Some(implicit_work_context()))
                .map(|work_context| publish_summary_from_work_context(&work_context)),
        })
}

pub(crate) fn sync_concept_snapshot(
    root: &Path,
    concept: &ConceptPacket,
    publish: &TrackedSnapshotPublishContext,
) -> Result<()> {
    let path = concept_snapshot_path(root, &concept.handle);
    if concept.publication.as_ref().is_some_and(|publication| {
        publication.status == prism_projections::ConceptPublicationStatus::Retired
    }) {
        remove_file_if_exists(&path)?;
    } else {
        write_json_file(&path, concept)?;
    }
    rebuild_concept_index(root)?;
    refresh_manifest(root, Some(publish))
}

pub(crate) fn sync_contract_snapshot(
    root: &Path,
    contract: &ContractPacket,
    publish: &TrackedSnapshotPublishContext,
) -> Result<()> {
    write_json_file(&contract_snapshot_path(root, &contract.handle), contract)?;
    rebuild_contract_index(root)?;
    refresh_manifest(root, Some(publish))
}

pub(crate) fn apply_concept_relation_snapshot(
    root: &Path,
    event: &ConceptRelationEvent,
    publish: &TrackedSnapshotPublishContext,
) -> Result<()> {
    match event.action {
        ConceptRelationEventAction::Upsert => {
            write_json_file(
                &relation_snapshot_path(root, &event.relation),
                &event.relation,
            )?;
        }
        ConceptRelationEventAction::Retire => {
            remove_file_if_exists(&relation_snapshot_path(root, &event.relation))?;
        }
    }
    rebuild_relation_index(root)?;
    refresh_manifest(root, Some(publish))
}

pub(crate) fn apply_memory_snapshot(
    root: &Path,
    event: &MemoryEvent,
    publish: &TrackedSnapshotPublishContext,
) -> Result<()> {
    for superseded in &event.supersedes {
        remove_file_if_exists(&memory_snapshot_path(root, &superseded.0))?;
    }
    let path = memory_snapshot_path(root, &event.memory_id.0);
    match event.action {
        MemoryEventKind::Stored | MemoryEventKind::Promoted | MemoryEventKind::Superseded => {
            if let Some(entry) = event.entry.clone() {
                let previous_count = read_json_file::<SnapshotMemoryRecord>(&path)
                    .ok()
                    .map(|record| record.event_count)
                    .unwrap_or(0);
                write_json_file(
                    &path,
                    &SnapshotMemoryRecord {
                        entry,
                        latest_event_id: event.id.clone(),
                        latest_recorded_at: event.recorded_at,
                        event_count: previous_count.saturating_add(1),
                        actor: event.actor.clone(),
                        execution_context: event.execution_context.clone(),
                        task_id: event.task_id.clone(),
                    },
                )?;
            }
        }
        MemoryEventKind::Retired => {
            remove_file_if_exists(&path)?;
        }
    }
    rebuild_memory_index(root)?;
    refresh_manifest(root, Some(publish))
}

pub(crate) fn sync_coordination_snapshot_state(
    root: &Path,
    _snapshot: &CoordinationSnapshot,
    _plan_graphs: &[PlanGraph],
    _execution_overlays: &BTreeMap<String, Vec<PlanExecutionOverlay>>,
    _publish: Option<&TrackedSnapshotPublishContext>,
    _coordination_revision: Option<u64>,
) -> Result<()> {
    // Coordination state now lives in shared refs and runtime-local read models rather than the
    // tracked snapshot authority. Keep the normal mutation path side-effect free so coordination
    // work does not republish unrelated tracked `.prism/state` manifests for concepts/memory.
    let _ = root;
    Ok(())
}

pub(crate) fn regenerate_tracked_snapshot_derived_artifacts(root: &Path) -> Result<()> {
    rebuild_concept_index(root)?;
    rebuild_contract_index(root)?;
    rebuild_relation_index(root)?;
    rebuild_memory_index(root)?;
    cleanup_tracked_plan_snapshot_exports(root)?;
    cleanup_shared_coordination_mirror_exports(root)?;
    refresh_manifest(root, None)
}

pub(crate) fn load_tracked_coordination_materialization_status(
    root: &Path,
) -> Result<Option<TrackedCoordinationMaterializationStatus>> {
    let path = snapshot_indexes_dir(root).join("coordination_materialization.json");
    if !path.exists() {
        return Ok(None);
    }
    Ok(Some(read_json_file::<
        TrackedCoordinationMaterializationStatus,
    >(&path)?))
}

pub(crate) fn load_concept_snapshots(root: &Path) -> Result<Vec<ConceptPacket>> {
    load_json_records::<ConceptPacket>(&snapshot_concepts_dir(root))
        .map(|records| records.into_iter().map(|(_, packet)| packet).collect())
}

pub(crate) fn load_contract_snapshots(root: &Path) -> Result<Vec<ContractPacket>> {
    load_json_records::<ContractPacket>(&snapshot_contracts_dir(root))
        .map(|records| records.into_iter().map(|(_, packet)| packet).collect())
}

pub(crate) fn load_relation_snapshots(root: &Path) -> Result<Vec<ConceptRelation>> {
    load_json_records::<ConceptRelation>(&snapshot_relations_dir(root))
        .map(|records| records.into_iter().map(|(_, relation)| relation).collect())
}

pub(crate) fn load_memory_snapshot_events(root: &Path) -> Result<Vec<MemoryEvent>> {
    let mut events = load_json_records::<SnapshotMemoryRecord>(&snapshot_memory_dir(root))?
        .into_iter()
        .map(|(_, record)| MemoryEvent {
            id: record.latest_event_id,
            memory_id: record.entry.id.clone(),
            recorded_at: record.latest_recorded_at,
            task_id: record.task_id,
            actor: record.actor,
            execution_context: record.execution_context,
            action: MemoryEventKind::Promoted,
            scope: record.entry.scope,
            promoted_from: Vec::new(),
            supersedes: Vec::new(),
            entry: Some(record.entry),
        })
        .collect::<Vec<_>>();
    events.sort_by(|left, right| {
        left.recorded_at
            .cmp(&right.recorded_at)
            .then_with(|| left.id.cmp(&right.id))
    });
    Ok(events)
}

pub(crate) fn load_tracked_coordination_snapshot_state(
    root: &Path,
) -> Result<Option<TrackedCoordinationSnapshotState>> {
    let plan_records = load_json_records::<SnapshotPlanRecord>(&snapshot_plans_dir(root))?
        .into_iter()
        .map(|(_, record)| record)
        .collect::<Vec<_>>();

    if plan_records.is_empty() {
        return Ok(None);
    }

    let mut plan_graphs = plan_records
        .iter()
        .map(|record| record.graph.clone())
        .collect::<Vec<_>>();
    let execution_overlays = plan_records
        .iter()
        .map(|record| {
            (
                record.plan.id.0.to_string(),
                record.execution_overlays.clone(),
            )
        })
        .collect::<BTreeMap<_, _>>();
    plan_graphs.sort_by(|left, right| left.id.0.cmp(&right.id.0));

    let stored_plans = plan_records
        .iter()
        .map(|record| (record.plan.id.0.to_string(), record.plan.clone()))
        .collect::<BTreeMap<_, _>>();
    let mut snapshot = coordination_snapshot_from_plan_graphs(&plan_graphs, &execution_overlays);
    for plan in &mut snapshot.plans {
        if let Some(stored) = stored_plans.get(plan.id.0.as_str()) {
            *plan = stored.clone();
        }
    }
    snapshot
        .plans
        .sort_by(|left, right| left.id.0.cmp(&right.id.0));
    snapshot
        .tasks
        .sort_by(|left, right| left.id.0.cmp(&right.id.0));

    Ok(Some(TrackedCoordinationSnapshotState {
        snapshot,
        plan_graphs,
        execution_overlays,
    }))
}

fn cleanup_tracked_plan_snapshot_exports(root: &Path) -> Result<()> {
    cleanup_directory_json_files(&snapshot_plans_dir(root), &BTreeSet::new())?;
    remove_file_if_exists(&snapshot_indexes_dir(root).join("plans.json"))?;
    remove_dir_if_empty(&snapshot_plans_dir(root))?;
    Ok(())
}

fn rebuild_concept_index(root: &Path) -> Result<()> {
    let entries = load_json_records::<ConceptPacket>(&snapshot_concepts_dir(root))?
        .into_iter()
        .map(|(path, packet)| SnapshotIndexEntry {
            id: packet.handle,
            title: packet.canonical_name,
            status: packet
                .publication
                .as_ref()
                .map(|publication| format!("{:?}", publication.status))
                .unwrap_or_else(|| "unpublished".to_string()),
            path,
        })
        .collect::<Vec<_>>();
    write_json_file(&snapshot_indexes_dir(root).join("concepts.json"), &entries)
}

fn rebuild_contract_index(root: &Path) -> Result<()> {
    let entries = load_json_records::<ContractPacket>(&snapshot_contracts_dir(root))?
        .into_iter()
        .map(|(path, packet)| SnapshotIndexEntry {
            id: packet.handle,
            title: packet.summary.clone(),
            status: format!("{:?}", packet.status),
            path,
        })
        .collect::<Vec<_>>();
    write_json_file(&snapshot_indexes_dir(root).join("contracts.json"), &entries)
}

fn rebuild_relation_index(root: &Path) -> Result<()> {
    let entries = load_json_records::<ConceptRelation>(&snapshot_relations_dir(root))?
        .into_iter()
        .map(|(path, relation)| SnapshotIndexEntry {
            id: relation_identity(&relation),
            title: format!(
                "{} {:?} {}",
                relation.source_handle, relation.kind, relation.target_handle
            ),
            status: "active".to_string(),
            path,
        })
        .collect::<Vec<_>>();
    write_json_file(&snapshot_indexes_dir(root).join("relations.json"), &entries)
}

fn rebuild_memory_index(root: &Path) -> Result<()> {
    let entries = load_json_records::<SnapshotMemoryRecord>(&snapshot_memory_dir(root))?
        .into_iter()
        .map(|(path, record)| SnapshotIndexEntry {
            id: record.entry.id.0.clone(),
            title: record.entry.content.clone(),
            status: format!("{:?}", record.entry.kind),
            path,
        })
        .collect::<Vec<_>>();
    write_json_file(&snapshot_indexes_dir(root).join("memory.json"), &entries)
}

fn cleanup_shared_coordination_mirror_exports(root: &Path) -> Result<()> {
    cleanup_directory_json_files(&snapshot_coordination_tasks_dir(root), &BTreeSet::new())?;
    cleanup_directory_json_files(&snapshot_coordination_artifacts_dir(root), &BTreeSet::new())?;
    remove_file_if_exists(&snapshot_indexes_dir(root).join("coordination_tasks.json"))?;
    remove_file_if_exists(&snapshot_indexes_dir(root).join("coordination_artifacts.json"))?;
    remove_dir_if_empty(&snapshot_coordination_tasks_dir(root))?;
    remove_dir_if_empty(&snapshot_coordination_artifacts_dir(root))?;
    remove_dir_if_empty(&snapshot_root(root).join("coordination"))?;
    Ok(())
}

fn refresh_manifest(root: &Path, publish: Option<&TrackedSnapshotPublishContext>) -> Result<()> {
    fs::create_dir_all(snapshot_root(root))?;
    let previous_manifest = load_snapshot_manifest(root)?;
    let previous_manifest_digest = previous_manifest
        .as_ref()
        .map(canonical_manifest_digest)
        .transpose()?;
    let migration_source_digest = match previous_manifest
        .as_ref()
        .and_then(|manifest| manifest.migration_source_digest.clone())
    {
        Some(digest) => Some(digest),
        None => previous_manifest
            .as_ref()
            .and_then(|manifest| retired_authority_digest(manifest, "legacy_tracked_repo_state"))
            .or(legacy_authoritative_migration_source_digest(root)?),
    };
    let retired_authorities = retained_tracked_authority_digests(root, previous_manifest.as_ref())?;
    remove_obsolete_tracked_change_snapshot_artifacts(root)?;
    remove_obsolete_legacy_tracked_authority_artifacts(root)?;
    let file_map = collect_snapshot_file_map(root)?;
    if file_map.is_empty() {
        remove_file_if_exists(&snapshot_manifest_path(root))?;
        return Ok(());
    }
    let publish = publish
        .cloned()
        .or_else(|| {
            previous_manifest
                .as_ref()
                .map(publish_context_from_manifest)
        })
        .unwrap_or_else(|| TrackedSnapshotPublishContext {
            published_at: crate::util::current_timestamp(),
            principal: implicit_principal_identity(None, None),
            work_context: Some(implicit_work_context()),
            publish_summary: None,
        });
    let work_context = publish.work_context.unwrap_or_else(implicit_work_context);
    let publish_summary = Some(
        publish
            .publish_summary
            .unwrap_or_else(|| publish_summary_from_work_context(&work_context)),
    );
    let paths = PrismPaths::for_workspace_root(root)?;
    let active_key = load_active_runtime_signing_key(&paths)?;
    let mut manifest = SnapshotManifest {
        version: SNAPSHOT_MANIFEST_VERSION,
        published_at: publish.published_at,
        publisher: SnapshotManifestPublisher {
            principal_authority_id: publish.principal.principal_authority_id,
            principal_id: publish.principal.principal_id,
            credential_id: publish.principal.credential_id,
        },
        work_context,
        publish_summary,
        files: file_map,
        previous_manifest_digest,
        migration_source_digest,
        retired_authorities,
        signature: SnapshotManifestSignature {
            algorithm: ProtectedSignatureAlgorithm::Ed25519,
            runtime_authority_id: active_key.state.runtime_authority_id.clone(),
            runtime_key_id: active_key.runtime_key.runtime_key_id.clone(),
            trust_bundle_id: active_key.bundle.bundle_id.clone(),
            value: String::new(),
        },
    };
    let signature =
        active_key
            .signing_key
            .sign(&canonical_json_bytes(&SnapshotManifestSigningView {
                version: manifest.version,
                published_at: manifest.published_at,
                publisher: &manifest.publisher,
                work_context: &manifest.work_context,
                publish_summary: &manifest.publish_summary,
                files: &manifest.files,
                previous_manifest_digest: &manifest.previous_manifest_digest,
                migration_source_digest: &manifest.migration_source_digest,
                retired_authorities: &manifest.retired_authorities,
                signature: SnapshotManifestSignatureMetadata {
                    algorithm: manifest.signature.algorithm,
                    runtime_authority_id: &manifest.signature.runtime_authority_id,
                    runtime_key_id: &manifest.signature.runtime_key_id,
                    trust_bundle_id: &manifest.signature.trust_bundle_id,
                },
            })?);
    manifest.signature.value = format!("base64:{}", BASE64_STANDARD.encode(signature.to_bytes()));
    write_json_file(&snapshot_manifest_path(root), &manifest)
}

fn remove_obsolete_tracked_change_snapshot_artifacts(root: &Path) -> Result<()> {
    let changes_dir = snapshot_changes_dir(root);
    if changes_dir.exists() {
        fs::remove_dir_all(&changes_dir).with_context(|| {
            format!(
                "failed to remove obsolete tracked change snapshots {}",
                changes_dir.display()
            )
        })?;
    }
    remove_file_if_exists(&snapshot_indexes_dir(root).join("changes.json"))
}

fn retained_tracked_authority_digests(
    root: &Path,
    previous_manifest: Option<&SnapshotManifest>,
) -> Result<Vec<SnapshotRetiredAuthority>> {
    let mut retired = previous_manifest
        .map(|manifest| manifest.retired_authorities.clone())
        .unwrap_or_default();
    if retired
        .iter()
        .any(|entry| entry.authority == "tracked_changes_snapshot")
    {
        return Ok(retired);
    }
    if let Some(digest) = tracked_changes_authority_digest(root)? {
        retired.push(SnapshotRetiredAuthority {
            authority: "tracked_changes_snapshot".to_string(),
            digest,
        });
    }
    if !retired
        .iter()
        .any(|entry| entry.authority == "legacy_tracked_repo_state")
    {
        if let Some(digest) = legacy_authoritative_migration_source_digest(root)? {
            retired.push(SnapshotRetiredAuthority {
                authority: "legacy_tracked_repo_state".to_string(),
                digest,
            });
        }
    }
    Ok(retired)
}

fn retired_authority_digest(manifest: &SnapshotManifest, authority: &str) -> Option<String> {
    manifest
        .retired_authorities
        .iter()
        .find(|entry| entry.authority == authority)
        .map(|entry| entry.digest.clone())
}

fn tracked_changes_authority_digest(root: &Path) -> Result<Option<String>> {
    let mut digests = BTreeMap::<String, String>::new();
    let changes_dir = snapshot_changes_dir(root);
    if changes_dir.exists() {
        let mut paths = fs::read_dir(&changes_dir)
            .with_context(|| format!("failed to read {}", changes_dir.display()))?
            .filter_map(|entry| entry.ok().map(|entry| entry.path()))
            .filter(|path| path.extension().and_then(|ext| ext.to_str()) == Some("json"))
            .collect::<Vec<_>>();
        paths.sort();
        for path in paths {
            let bytes =
                fs::read(&path).with_context(|| format!("failed to read {}", path.display()))?;
            digests.insert(
                path.strip_prefix(root)
                    .unwrap_or(path.as_path())
                    .to_string_lossy()
                    .replace('\\', "/"),
                sha256_prefixed(&bytes),
            );
        }
    }
    let changes_index = snapshot_indexes_dir(root).join("changes.json");
    if changes_index.exists() {
        let bytes = fs::read(&changes_index)
            .with_context(|| format!("failed to read {}", changes_index.display()))?;
        digests.insert(
            changes_index
                .strip_prefix(root)
                .unwrap_or(changes_index.as_path())
                .to_string_lossy()
                .replace('\\', "/"),
            sha256_prefixed(&bytes),
        );
    }
    if digests.is_empty() {
        return Ok(None);
    }
    Ok(Some(sha256_prefixed(&canonical_json_bytes(&digests)?)))
}

fn canonical_manifest_digest(manifest: &SnapshotManifest) -> Result<String> {
    Ok(sha256_prefixed(&canonical_json_bytes(manifest)?))
}

fn publish_context_from_manifest(manifest: &SnapshotManifest) -> TrackedSnapshotPublishContext {
    TrackedSnapshotPublishContext {
        published_at: manifest.published_at,
        principal: ProtectedPrincipalIdentity {
            principal_authority_id: manifest.publisher.principal_authority_id.clone(),
            principal_id: manifest.publisher.principal_id.clone(),
            credential_id: manifest.publisher.credential_id.clone(),
        },
        work_context: Some(manifest.work_context.clone()),
        publish_summary: manifest
            .publish_summary
            .clone()
            .or_else(|| Some(publish_summary_from_work_context(&manifest.work_context))),
    }
}

fn implicit_work_context() -> WorkContextSnapshot {
    WorkContextSnapshot {
        work_id: "work:legacy_implicit_publication".to_string(),
        kind: WorkContextKind::Undeclared,
        title: "Legacy implicit repo publication".to_string(),
        summary: Some(
            "Fallback publish context for snapshot authority when no explicit declared work is available."
                .to_string(),
        ),
        parent_work_id: None,
        coordination_task_id: None,
        plan_id: None,
        plan_title: None,
    }
}

fn publish_summary_from_work_context(
    work_context: &WorkContextSnapshot,
) -> SnapshotManifestPublishSummary {
    SnapshotManifestPublishSummary {
        title: work_context.title.clone(),
        summary: work_context.summary.clone(),
    }
}

fn collect_snapshot_file_map(root: &Path) -> Result<BTreeMap<String, SnapshotManifestFile>> {
    let state_root = snapshot_root(root);
    if !state_root.exists() {
        return Ok(BTreeMap::new());
    }
    let mut files = BTreeMap::new();
    collect_snapshot_files_recursive(root, &state_root, &mut files)?;
    Ok(files)
}

fn legacy_authoritative_migration_source_digest(root: &Path) -> Result<Option<String>> {
    let mut digests = BTreeMap::<String, String>::new();
    for path in legacy_authoritative_stream_paths(root)? {
        let relative = path
            .strip_prefix(root)
            .unwrap_or(path.as_path())
            .to_string_lossy()
            .replace('\\', "/");
        let bytes =
            fs::read(&path).with_context(|| format!("failed to read {}", path.display()))?;
        digests.insert(relative, sha256_prefixed(&bytes));
    }
    if digests.is_empty() {
        return Ok(None);
    }
    Ok(Some(sha256_prefixed(&canonical_json_bytes(&digests)?)))
}

fn legacy_authoritative_stream_paths(root: &Path) -> Result<Vec<PathBuf>> {
    let mut paths = Vec::new();
    for relative in [
        PathBuf::from(".prism/concepts/events.jsonl"),
        PathBuf::from(".prism/concepts/relations.jsonl"),
        PathBuf::from(".prism/contracts/events.jsonl"),
        PathBuf::from(".prism/changes/events.jsonl"),
        PathBuf::from(".prism/memory/events.jsonl"),
    ] {
        let path = root.join(&relative);
        if path.exists() {
            paths.push(path);
        }
    }
    let plan_streams_dir = root.join(".prism").join("plans").join("streams");
    if plan_streams_dir.exists() {
        let mut plan_paths = fs::read_dir(&plan_streams_dir)
            .with_context(|| format!("failed to read {}", plan_streams_dir.display()))?
            .filter_map(|entry| entry.ok().map(|entry| entry.path()))
            .filter(|path| path.extension().and_then(|ext| ext.to_str()) == Some("jsonl"))
            .collect::<Vec<_>>();
        plan_paths.sort();
        paths.extend(plan_paths);
    }
    paths.sort();
    Ok(paths)
}

fn collect_snapshot_files_recursive(
    root: &Path,
    current: &Path,
    files: &mut BTreeMap<String, SnapshotManifestFile>,
) -> Result<()> {
    for entry in
        fs::read_dir(current).with_context(|| format!("failed to read {}", current.display()))?
    {
        let entry = entry?;
        let path = entry.path();
        if path.is_dir() {
            collect_snapshot_files_recursive(root, &path, files)?;
            continue;
        }
        if path == snapshot_manifest_path(root)
            || path.extension().and_then(|ext| ext.to_str()) != Some("json")
        {
            continue;
        }
        let bytes =
            fs::read(&path).with_context(|| format!("failed to read {}", path.display()))?;
        let relative = path
            .strip_prefix(root)
            .unwrap_or(path.as_path())
            .to_string_lossy()
            .replace('\\', "/");
        files.insert(
            relative.clone(),
            SnapshotManifestFile {
                path: relative,
                sha256: sha256_prefixed(&bytes),
            },
        );
    }
    Ok(())
}

fn cleanup_directory_json_files(dir: &Path, expected: &BTreeSet<PathBuf>) -> Result<()> {
    if !dir.exists() {
        if expected.is_empty() {
            return Ok(());
        }
        fs::create_dir_all(dir)?;
        return Ok(());
    }
    for entry in fs::read_dir(dir).with_context(|| format!("failed to read {}", dir.display()))? {
        let entry = entry?;
        let path = entry.path();
        if path.extension().and_then(|ext| ext.to_str()) == Some("json")
            && !expected.contains(&path)
        {
            fs::remove_file(&path)
                .with_context(|| format!("failed to remove stale snapshot {}", path.display()))?;
        }
    }
    Ok(())
}

fn load_json_records<T>(dir: &Path) -> Result<Vec<(String, T)>>
where
    T: for<'de> Deserialize<'de>,
{
    if !dir.exists() {
        return Ok(Vec::new());
    }
    let mut records = Vec::new();
    for entry in fs::read_dir(dir).with_context(|| format!("failed to read {}", dir.display()))? {
        let entry = entry?;
        let path = entry.path();
        if path.extension().and_then(|ext| ext.to_str()) != Some("json") {
            continue;
        }
        let value = read_json_file::<T>(&path)?;
        records.push((
            path.strip_prefix(dir.parent().unwrap_or(dir))
                .unwrap_or(path.as_path())
                .to_string_lossy()
                .replace('\\', "/"),
            value,
        ));
    }
    records.sort_by(|left, right| left.0.cmp(&right.0));
    Ok(records)
}

fn read_json_file<T>(path: &Path) -> Result<T>
where
    T: for<'de> Deserialize<'de>,
{
    let bytes = fs::read(path).with_context(|| format!("failed to read {}", path.display()))?;
    serde_json::from_slice(&bytes).with_context(|| format!("failed to parse {}", path.display()))
}

fn remove_file_if_exists(path: &Path) -> Result<()> {
    if path.exists() {
        fs::remove_file(path).with_context(|| format!("failed to remove {}", path.display()))?;
    }
    Ok(())
}

fn remove_dir_if_empty(path: &Path) -> Result<()> {
    if !path.exists() {
        return Ok(());
    }
    if fs::read_dir(path)
        .with_context(|| format!("failed to read {}", path.display()))?
        .next()
        .is_none()
    {
        fs::remove_dir(path).with_context(|| format!("failed to remove {}", path.display()))?;
    }
    Ok(())
}

fn remove_dir_all_if_exists(path: &Path) -> Result<()> {
    if path.exists() {
        match fs::remove_dir_all(path) {
            Ok(()) => {}
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
            Err(error) => {
                return Err(error).with_context(|| format!("failed to remove {}", path.display()));
            }
        }
    }
    Ok(())
}

fn write_json_file<T>(path: &Path, value: &T) -> Result<()>
where
    T: Serialize,
{
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)?;
    }
    let mut bytes = serde_json::to_vec_pretty(value)
        .with_context(|| format!("failed to encode {}", path.display()))?;
    bytes.push(b'\n');
    let should_write = match fs::read(path) {
        Ok(existing) => existing != bytes,
        Err(_) => true,
    };
    if should_write {
        let tmp_path = path.with_extension(format!("tmp-{}", prism_ir::new_sortable_token()));
        fs::write(&tmp_path, &bytes)
            .with_context(|| format!("failed to write {}", tmp_path.display()))?;
        fs::rename(&tmp_path, path)
            .with_context(|| format!("failed to replace {}", path.display()))?;
    }
    Ok(())
}
