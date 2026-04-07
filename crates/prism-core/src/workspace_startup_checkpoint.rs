use std::collections::BTreeMap;
use std::fs::{self, File};
use std::io::{BufReader, BufWriter, ErrorKind, Read, Write};
use std::path::Path;
use std::time::{Instant, SystemTime, UNIX_EPOCH};

use anyhow::{Context, Result};
use bincode::Options;
use prism_history::{HistorySnapshot, HistoryStore, LineageTombstone};
use prism_ir::{
    CredentialId, EventActor, EventExecutionContext, EventId, LineageEvent, LineageEventKind,
    LineageEvidence, LineageId, NodeId, PrincipalActor, TaskId, WorkContextKind,
    WorkContextSnapshot,
};
use prism_memory::{OutcomeMemory, OutcomeMemorySnapshot};
use prism_projections::{IntentIndex, ProjectionIndex, ProjectionSnapshot};
use prism_store::{
    CoordinationStartupCheckpointAuthority, FileRecord, Graph, GraphSnapshot, SnapshotRevisions,
    SqliteStore, Store, WorkspaceTreeDirectoryFingerprint, WorkspaceTreeFileFingerprint,
    WorkspaceTreeSnapshot,
};
use serde::{de::DeserializeOwned, Serialize};
use smol_str::SmolStr;
use tracing::info;

use crate::coordination_startup_checkpoint::coordination_startup_authority;
use crate::indexer::workspace_recovery_work;
use crate::indexer::WorkspaceIndexer;
use crate::layout::WorkspaceLayout;
use crate::projection_hydration::persisted_projection_load_plan;
use crate::protected_state::runtime_sync::load_repo_protected_knowledge;
use crate::repo_patch_events::merge_repo_patch_events_into_memory;
use crate::session::{WorkspaceRefreshSeed, HOT_OUTCOME_HYDRATION_LIMIT};
use crate::shared_runtime::{
    merged_projection_index, overlay_persisted_projection_knowledge,
    projection_snapshot_without_knowledge,
};
use crate::shared_runtime_store::SharedRuntimeStore;
use crate::util::cache_path;
use crate::workspace_runtime_state::WorkspaceRuntimeState;
use crate::{WorkspaceSession, WorkspaceSessionOptions};

const WORKSPACE_RUNTIME_STARTUP_CHECKPOINT_MAGIC: &[u8; 8] = b"PRWSCP01";
const WORKSPACE_RUNTIME_STARTUP_CHECKPOINT_VERSION: u32 = 9;
const MAX_STARTUP_CHECKPOINT_SEGMENT_BYTES: u64 = 1 << 30;

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
struct HistorySnapshotCheckpoint {
    node_to_lineage: Vec<(NodeId, LineageId)>,
    events: Vec<LineageEventCheckpoint>,
    tombstones: Vec<LineageTombstone>,
    next_lineage: u64,
    next_event: u64,
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
struct LineageEventCheckpoint {
    meta: EventMetaCheckpoint,
    lineage: LineageId,
    kind: LineageEventKind,
    before: Vec<NodeId>,
    after: Vec<NodeId>,
    confidence: f32,
    evidence: Vec<LineageEvidence>,
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
struct EventMetaCheckpoint {
    id: EventId,
    ts: u64,
    actor: EventActorCheckpoint,
    correlation: Option<TaskId>,
    causation: Option<EventId>,
    execution_context: Option<EventExecutionContextCheckpoint>,
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
enum EventActorCheckpoint {
    User,
    Agent,
    System,
    Principal(PrincipalActor),
    GitAuthor { name: String, email: Option<String> },
    CI,
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
struct EventExecutionContextCheckpoint {
    repo_id: Option<String>,
    worktree_id: Option<String>,
    branch_ref: Option<String>,
    session_id: Option<String>,
    instance_id: Option<String>,
    request_id: Option<String>,
    credential_id: Option<CredentialId>,
    work_context: Option<WorkContextSnapshotCheckpoint>,
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
struct WorkContextSnapshotCheckpoint {
    work_id: String,
    kind: WorkContextKind,
    title: String,
    summary: Option<String>,
    parent_work_id: Option<String>,
    coordination_task_id: Option<String>,
    plan_id: Option<String>,
    plan_title: Option<String>,
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
struct CoordinationAuthorityCheckpoint {
    ref_name: String,
    head_commit: Option<String>,
    manifest_digest: Option<String>,
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
struct WorkspaceTreeSnapshotCheckpoint {
    root_hash: u64,
    files: BTreeMap<String, WorkspaceTreeFileFingerprint>,
    directories: BTreeMap<String, WorkspaceTreeDirectoryFingerprint>,
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
struct WorkspaceLayoutCheckpoint {
    workspace_name: String,
    workspace_display_name: String,
    workspace_manifest: String,
    packages: Vec<PackageInfoCheckpoint>,
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
struct PackageInfoCheckpoint {
    package_name: String,
    crate_name: String,
    root: String,
    manifest_path: String,
    node_id: NodeId,
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
struct GraphSnapshotCheckpoint {
    nodes: std::collections::HashMap<NodeId, prism_ir::Node>,
    edges: Vec<prism_ir::Edge>,
    file_records: BTreeMap<String, FileRecord>,
    next_file_id: u32,
}

#[derive(Debug, Clone)]
struct WorkspaceRuntimeStartupCheckpointHeader {
    version: u32,
    materialized_at: u64,
    revisions: SnapshotRevisions,
    outcome_revision: u64,
    coordination_authority: CoordinationAuthorityCheckpoint,
}

impl From<HistorySnapshot> for HistorySnapshotCheckpoint {
    fn from(snapshot: HistorySnapshot) -> Self {
        Self {
            node_to_lineage: snapshot.node_to_lineage,
            events: snapshot.events.into_iter().map(Into::into).collect(),
            tombstones: snapshot.tombstones,
            next_lineage: snapshot.next_lineage,
            next_event: snapshot.next_event,
        }
    }
}

impl From<HistorySnapshotCheckpoint> for HistorySnapshot {
    fn from(snapshot: HistorySnapshotCheckpoint) -> Self {
        Self {
            node_to_lineage: snapshot.node_to_lineage,
            events: snapshot.events.into_iter().map(Into::into).collect(),
            tombstones: snapshot.tombstones,
            next_lineage: snapshot.next_lineage,
            next_event: snapshot.next_event,
        }
    }
}

impl From<LineageEvent> for LineageEventCheckpoint {
    fn from(event: LineageEvent) -> Self {
        Self {
            meta: event.meta.into(),
            lineage: event.lineage,
            kind: event.kind,
            before: event.before,
            after: event.after,
            confidence: event.confidence,
            evidence: event.evidence,
        }
    }
}

impl From<LineageEventCheckpoint> for LineageEvent {
    fn from(event: LineageEventCheckpoint) -> Self {
        Self {
            meta: event.meta.into(),
            lineage: event.lineage,
            kind: event.kind,
            before: event.before,
            after: event.after,
            confidence: event.confidence,
            evidence: event.evidence,
        }
    }
}

impl From<prism_ir::EventMeta> for EventMetaCheckpoint {
    fn from(meta: prism_ir::EventMeta) -> Self {
        Self {
            id: meta.id,
            ts: meta.ts,
            actor: meta.actor.into(),
            correlation: meta.correlation,
            causation: meta.causation,
            execution_context: meta.execution_context.map(Into::into),
        }
    }
}

impl From<EventMetaCheckpoint> for prism_ir::EventMeta {
    fn from(meta: EventMetaCheckpoint) -> Self {
        Self {
            id: meta.id,
            ts: meta.ts,
            actor: meta.actor.into(),
            correlation: meta.correlation,
            causation: meta.causation,
            execution_context: meta.execution_context.map(Into::into),
        }
    }
}

impl From<EventActor> for EventActorCheckpoint {
    fn from(actor: EventActor) -> Self {
        match actor {
            EventActor::User => Self::User,
            EventActor::Agent => Self::Agent,
            EventActor::System => Self::System,
            EventActor::Principal(actor) => Self::Principal(actor),
            EventActor::GitAuthor { name, email } => Self::GitAuthor {
                name: name.to_string(),
                email: email.map(|value| value.to_string()),
            },
            EventActor::CI => Self::CI,
        }
    }
}

impl From<EventActorCheckpoint> for EventActor {
    fn from(actor: EventActorCheckpoint) -> Self {
        match actor {
            EventActorCheckpoint::User => Self::User,
            EventActorCheckpoint::Agent => Self::Agent,
            EventActorCheckpoint::System => Self::System,
            EventActorCheckpoint::Principal(actor) => Self::Principal(actor),
            EventActorCheckpoint::GitAuthor { name, email } => Self::GitAuthor {
                name: SmolStr::new(name),
                email: email.map(SmolStr::new),
            },
            EventActorCheckpoint::CI => Self::CI,
        }
    }
}

impl From<EventExecutionContext> for EventExecutionContextCheckpoint {
    fn from(context: EventExecutionContext) -> Self {
        Self {
            repo_id: context.repo_id,
            worktree_id: context.worktree_id,
            branch_ref: context.branch_ref,
            session_id: context.session_id,
            instance_id: context.instance_id,
            request_id: context.request_id,
            credential_id: context.credential_id,
            work_context: context.work_context.map(Into::into),
        }
    }
}

impl From<EventExecutionContextCheckpoint> for EventExecutionContext {
    fn from(context: EventExecutionContextCheckpoint) -> Self {
        Self {
            repo_id: context.repo_id,
            worktree_id: context.worktree_id,
            branch_ref: context.branch_ref,
            session_id: context.session_id,
            instance_id: context.instance_id,
            request_id: context.request_id,
            credential_id: context.credential_id,
            work_context: context.work_context.map(Into::into),
        }
    }
}

impl From<WorkContextSnapshot> for WorkContextSnapshotCheckpoint {
    fn from(context: WorkContextSnapshot) -> Self {
        Self {
            work_id: context.work_id,
            kind: context.kind,
            title: context.title,
            summary: context.summary,
            parent_work_id: context.parent_work_id,
            coordination_task_id: context.coordination_task_id,
            plan_id: context.plan_id,
            plan_title: context.plan_title,
        }
    }
}

impl From<CoordinationStartupCheckpointAuthority> for CoordinationAuthorityCheckpoint {
    fn from(authority: CoordinationStartupCheckpointAuthority) -> Self {
        Self {
            ref_name: authority.ref_name,
            head_commit: authority.head_commit,
            manifest_digest: authority.manifest_digest,
        }
    }
}

impl From<CoordinationAuthorityCheckpoint> for CoordinationStartupCheckpointAuthority {
    fn from(authority: CoordinationAuthorityCheckpoint) -> Self {
        Self {
            ref_name: authority.ref_name,
            head_commit: authority.head_commit,
            manifest_digest: authority.manifest_digest,
        }
    }
}

impl From<WorkContextSnapshotCheckpoint> for WorkContextSnapshot {
    fn from(context: WorkContextSnapshotCheckpoint) -> Self {
        Self {
            work_id: context.work_id,
            kind: context.kind,
            title: context.title,
            summary: context.summary,
            parent_work_id: context.parent_work_id,
            coordination_task_id: context.coordination_task_id,
            plan_id: context.plan_id,
            plan_title: context.plan_title,
        }
    }
}

impl From<WorkspaceTreeSnapshot> for WorkspaceTreeSnapshotCheckpoint {
    fn from(snapshot: WorkspaceTreeSnapshot) -> Self {
        Self {
            root_hash: snapshot.root_hash,
            files: snapshot
                .files
                .into_iter()
                .map(|(path, fingerprint)| (portable_path_string(path), fingerprint))
                .collect(),
            directories: snapshot
                .directories
                .into_iter()
                .map(|(path, fingerprint)| (portable_path_string(path), fingerprint))
                .collect(),
        }
    }
}

impl From<WorkspaceTreeSnapshotCheckpoint> for WorkspaceTreeSnapshot {
    fn from(snapshot: WorkspaceTreeSnapshotCheckpoint) -> Self {
        Self {
            root_hash: snapshot.root_hash,
            files: snapshot
                .files
                .into_iter()
                .map(|(path, fingerprint)| (std::path::PathBuf::from(path), fingerprint))
                .collect(),
            directories: snapshot
                .directories
                .into_iter()
                .map(|(path, fingerprint)| (std::path::PathBuf::from(path), fingerprint))
                .collect(),
        }
    }
}

impl From<WorkspaceLayout> for WorkspaceLayoutCheckpoint {
    fn from(layout: WorkspaceLayout) -> Self {
        Self {
            workspace_name: layout.workspace_name,
            workspace_display_name: layout.workspace_display_name,
            workspace_manifest: portable_path_string(layout.workspace_manifest),
            packages: layout.packages.into_iter().map(Into::into).collect(),
        }
    }
}

impl From<WorkspaceLayoutCheckpoint> for WorkspaceLayout {
    fn from(layout: WorkspaceLayoutCheckpoint) -> Self {
        Self {
            workspace_name: layout.workspace_name,
            workspace_display_name: layout.workspace_display_name,
            workspace_manifest: std::path::PathBuf::from(layout.workspace_manifest),
            packages: layout.packages.into_iter().map(Into::into).collect(),
        }
    }
}

impl From<crate::layout::PackageInfo> for PackageInfoCheckpoint {
    fn from(package: crate::layout::PackageInfo) -> Self {
        Self {
            package_name: package.package_name,
            crate_name: package.crate_name,
            root: portable_path_string(package.root),
            manifest_path: portable_path_string(package.manifest_path),
            node_id: package.node_id,
        }
    }
}

impl From<PackageInfoCheckpoint> for crate::layout::PackageInfo {
    fn from(package: PackageInfoCheckpoint) -> Self {
        Self {
            package_name: package.package_name,
            crate_name: package.crate_name,
            root: std::path::PathBuf::from(package.root),
            manifest_path: std::path::PathBuf::from(package.manifest_path),
            node_id: package.node_id,
        }
    }
}

impl From<GraphSnapshot> for GraphSnapshotCheckpoint {
    fn from(snapshot: GraphSnapshot) -> Self {
        Self {
            nodes: snapshot.nodes,
            edges: snapshot.edges,
            file_records: snapshot
                .file_records
                .into_iter()
                .map(|(path, record)| (portable_path_string(path), record))
                .collect(),
            next_file_id: snapshot.next_file_id,
        }
    }
}

impl From<GraphSnapshotCheckpoint> for GraphSnapshot {
    fn from(snapshot: GraphSnapshotCheckpoint) -> Self {
        Self {
            nodes: snapshot.nodes,
            edges: snapshot.edges,
            file_records: snapshot
                .file_records
                .into_iter()
                .map(|(path, record)| (std::path::PathBuf::from(path), record))
                .collect(),
            next_file_id: snapshot.next_file_id,
        }
    }
}

pub(crate) fn build_workspace_indexer_with_startup_checkpoint(
    root: &Path,
    options: WorkspaceSessionOptions,
) -> Result<WorkspaceIndexer<SqliteStore>> {
    let root = root.canonicalize()?;
    let store = SqliteStore::open(cache_path(&root)?)?;
    let shared_runtime_aliases_workspace_store = options
        .shared_runtime
        .aliases_sqlite_path(&cache_path(&root)?);
    let shared_runtime_store = SharedRuntimeStore::open(&options.shared_runtime)?;
    if let Some(restored) = load_workspace_runtime_startup_checkpoint(
        &root,
        &store,
        shared_runtime_store.as_ref(),
        shared_runtime_aliases_workspace_store,
    )? {
        let started = Instant::now();
        let mut indexer = WorkspaceIndexer::with_runtime_state_stores_and_options(
            &root,
            store,
            shared_runtime_store,
            restored.runtime_state,
            restored.layout,
            false,
            Some(restored.workspace_tree_snapshot),
            None,
            options.clone(),
        )?;
        let reload_metrics = refresh_restored_runtime_domains(
            &root,
            &mut indexer,
            &options,
            shared_runtime_aliases_workspace_store,
            restored.local_stale,
            restored.outcome_stale,
            restored.coordination_stale,
        )?;
        if restored.local_stale {
            indexer.startup_intent = None;
        } else {
            indexer.startup_intent = Some(restored.intent_index);
        }
        indexer.trust_cached_query_state = !restored.outcome_stale && !restored.local_stale;
        let mut recovery_work = workspace_recovery_work(
            &indexer.graph,
            &indexer.history,
            &indexer.outcomes,
            crate::session::WorkspaceRefreshWork::default(),
            &indexer.coordination_snapshot,
        )?;
        recovery_work.workspace_reloaded = true;
        indexer.startup_refresh = Some(WorkspaceRefreshSeed {
            path: "recovery",
            duration_ms: u64::try_from(started.elapsed().as_millis()).unwrap_or(u64::MAX),
            work: recovery_work,
        });
        info!(
            root = %root.display(),
            workspace_revision = restored.revisions.workspace,
            episodic_revision = restored.revisions.episodic,
            inference_revision = restored.revisions.inference,
            coordination_revision = restored.revisions.coordination,
            local_reloaded = restored.local_stale,
            outcome_stale = restored.outcome_stale,
            coordination_reloaded = restored.coordination_stale,
            load_outcomes_ms = reload_metrics.load_outcomes_ms,
            reload_projections_ms = reload_metrics.reload_projections_ms,
            coordination_reload_ms = reload_metrics.reload_coordination_ms,
            total_ms = started.elapsed().as_millis(),
            "restored prism workspace indexer from startup checkpoint"
        );
        return Ok(indexer);
    }
    WorkspaceIndexer::new_with_options(&root, options)
}

pub(crate) fn persist_workspace_runtime_startup_checkpoint(
    session: &WorkspaceSession,
) -> Result<()> {
    let revisions = session.snapshot_revisions()?;
    let outcome_revision = merged_outcome_revision_for_session(session)?;
    let coordination_authority = coordination_startup_authority(session.root())?;
    let (
        layout,
        graph_snapshot,
        history_snapshot,
        intent_index,
        outcome_snapshot,
        coordination_snapshot,
        projection_snapshot,
    ) = {
        let runtime_state = session
            .runtime_state
            .lock()
            .expect("workspace runtime state lock poisoned");
        (
            runtime_state.layout(),
            runtime_state.graph.snapshot(),
            runtime_state.history.snapshot(),
            IntentIndex::derive(
                runtime_state.graph.all_nodes().collect::<Vec<_>>(),
                runtime_state.graph.edges.iter().collect::<Vec<_>>(),
            ),
            runtime_state.outcomes.snapshot(),
            runtime_state.coordination_snapshot.clone(),
            runtime_state.projections.snapshot(),
        )
    };
    let path = crate::PrismPaths::for_workspace_root(session.root())?
        .workspace_runtime_startup_checkpoint_path()?;
    let unique_suffix = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_nanos();
    let tmp_path = path.with_extension(format!("bin.tmp.{}.{unique_suffix}", std::process::id()));
    let writer = File::create(&tmp_path)
        .with_context(|| format!("failed to create startup checkpoint {}", tmp_path.display()))?;
    let mut writer = BufWriter::new(writer);
    let header = WorkspaceRuntimeStartupCheckpointHeader {
        version: WORKSPACE_RUNTIME_STARTUP_CHECKPOINT_VERSION,
        materialized_at: crate::util::current_timestamp(),
        revisions,
        outcome_revision,
        coordination_authority: coordination_authority.into(),
    };
    write_checkpoint_header(&mut writer, &header)?;
    write_bincode_segment(
        &mut writer,
        &WorkspaceTreeSnapshotCheckpoint::from(session.workspace_tree_snapshot()),
        "workspace tree snapshot",
    )?;
    write_bincode_segment(
        &mut writer,
        &WorkspaceLayoutCheckpoint::from(layout),
        "workspace layout",
    )?;
    write_bincode_segment(
        &mut writer,
        &GraphSnapshotCheckpoint::from(graph_snapshot),
        "graph snapshot",
    )?;
    write_bincode_segment(
        &mut writer,
        &HistorySnapshotCheckpoint::from(history_snapshot),
        "history snapshot",
    )?;
    write_bincode_segment(&mut writer, &intent_index, "intent index")?;
    write_bytes_segment(
        &mut writer,
        &encode_json(&outcome_snapshot)?,
        "outcome snapshot json",
    )?;
    write_bytes_segment(
        &mut writer,
        &encode_json(&coordination_snapshot)?,
        "coordination snapshot json",
    )?;
    write_bytes_segment(
        &mut writer,
        &encode_json(&projection_snapshot)?,
        "projection snapshot json",
    )?;
    writer.flush().with_context(|| {
        format!(
            "failed to flush startup checkpoint writer {}",
            tmp_path.display()
        )
    })?;
    writer
        .get_ref()
        .sync_all()
        .with_context(|| format!("failed to sync startup checkpoint {}", tmp_path.display()))?;
    drop(writer);
    fs::rename(&tmp_path, &path).with_context(|| {
        format!(
            "failed to move startup checkpoint {} into place at {}",
            tmp_path.display(),
            path.display()
        )
    })?;
    Ok(())
}

struct RestoredWorkspaceRuntimeCheckpoint {
    revisions: SnapshotRevisions,
    layout: WorkspaceLayout,
    workspace_tree_snapshot: WorkspaceTreeSnapshot,
    intent_index: IntentIndex,
    runtime_state: WorkspaceRuntimeState,
    local_stale: bool,
    outcome_stale: bool,
    coordination_stale: bool,
}

#[derive(Debug, Clone, Copy, Default)]
struct RestoredRuntimeReloadMetrics {
    load_outcomes_ms: u128,
    reload_projections_ms: u128,
    reload_coordination_ms: u128,
}

fn load_workspace_runtime_startup_checkpoint(
    root: &Path,
    store: &SqliteStore,
    shared_runtime_store: Option<&SharedRuntimeStore>,
    shared_runtime_aliases_workspace_store: bool,
) -> Result<Option<RestoredWorkspaceRuntimeCheckpoint>> {
    let path =
        crate::PrismPaths::for_workspace_root(root)?.workspace_runtime_startup_checkpoint_path()?;
    let (file, checkpoint_bytes) = match File::open(&path) {
        Ok(file) => {
            let checkpoint_bytes = file
                .metadata()
                .with_context(|| {
                    format!(
                        "failed to read startup checkpoint metadata {}",
                        path.display()
                    )
                })?
                .len();
            (file, checkpoint_bytes)
        }
        Err(error) if error.kind() == ErrorKind::NotFound => return Ok(None),
        Err(error) => {
            return Err(error)
                .with_context(|| format!("failed to read startup checkpoint {}", path.display()));
        }
    };
    let mut reader = BufReader::new(file);
    let header = match read_checkpoint_header(&mut reader) {
        Ok(header) => header,
        Err(error) => {
            info!(
                root = %root.display(),
                path = %path.display(),
                checkpoint_bytes,
                error = %error,
                "ignoring unreadable startup checkpoint"
            );
            let _ = fs::remove_file(&path);
            return Ok(None);
        }
    };
    if header.version != WORKSPACE_RUNTIME_STARTUP_CHECKPOINT_VERSION {
        info!(
            root = %root.display(),
            checkpoint_version = header.version,
            expected_version = WORKSPACE_RUNTIME_STARTUP_CHECKPOINT_VERSION,
            "ignoring stale startup checkpoint version"
        );
        return Ok(None);
    }
    let current_revisions = merged_snapshot_revisions(
        store,
        shared_runtime_store,
        shared_runtime_aliases_workspace_store,
    )?;
    let current_outcome_revision = merged_outcome_revision(
        store,
        shared_runtime_store,
        shared_runtime_aliases_workspace_store,
    )?;
    let local_stale = !local_restore_revisions_match(header.revisions, current_revisions);
    if local_stale && !local_restore_revisions_recoverable(header.revisions, current_revisions) {
        info!(
            root = %root.display(),
            checkpoint_workspace_revision = header.revisions.workspace,
            checkpoint_episodic_revision = header.revisions.episodic,
            checkpoint_inference_revision = header.revisions.inference,
            checkpoint_coordination_revision = header.revisions.coordination,
            current_workspace_revision = current_revisions.workspace,
            current_episodic_revision = current_revisions.episodic,
            current_inference_revision = current_revisions.inference,
            current_coordination_revision = current_revisions.coordination,
            "ignoring startup checkpoint with stale local revisions"
        );
        return Ok(None);
    }
    if local_stale {
        info!(
            root = %root.display(),
            checkpoint_workspace_revision = header.revisions.workspace,
            checkpoint_episodic_revision = header.revisions.episodic,
            checkpoint_inference_revision = header.revisions.inference,
            current_workspace_revision = current_revisions.workspace,
            current_episodic_revision = current_revisions.episodic,
            current_inference_revision = current_revisions.inference,
            "restoring startup checkpoint with stale local revisions; workspace will resync asynchronously"
        );
    }
    let outcome_stale = header.outcome_revision != current_outcome_revision;
    if outcome_stale && header.outcome_revision > current_outcome_revision {
        info!(
            root = %root.display(),
            checkpoint_outcome_revision = header.outcome_revision,
            current_outcome_revision,
            "ignoring startup checkpoint with stale outcome revision"
        );
        return Ok(None);
    }
    if outcome_stale {
        info!(
            root = %root.display(),
            checkpoint_outcome_revision = header.outcome_revision,
            current_outcome_revision,
            "restoring startup checkpoint with stale outcome revision; outcomes will reload from persisted state"
        );
    }
    let current_authority = coordination_startup_authority(root)?;
    let checkpoint_authority: CoordinationStartupCheckpointAuthority =
        header.coordination_authority.into();
    let coordination_stale = coordination_restore_stale(
        header.revisions.coordination,
        current_revisions.coordination,
        &checkpoint_authority,
        &current_authority,
    );
    if coordination_stale {
        info!(
            root = %root.display(),
            checkpoint_coordination_revision = header.revisions.coordination,
            current_coordination_revision = current_revisions.coordination,
            checkpoint_ref_name = checkpoint_authority.ref_name,
            checkpoint_head = checkpoint_authority
                .head_commit
                .as_deref()
                .unwrap_or(""),
            checkpoint_manifest_digest = checkpoint_authority
                .manifest_digest
                .as_deref()
                .unwrap_or(""),
            current_ref_name = current_authority.ref_name,
            current_head = current_authority.head_commit.as_deref().unwrap_or(""),
            current_manifest_digest = current_authority.manifest_digest.as_deref().unwrap_or(""),
            "restoring startup checkpoint with stale coordination state; coordination will resync asynchronously"
        );
    }
    let restored = (|| -> Result<RestoredWorkspaceRuntimeCheckpoint> {
        let workspace_tree_snapshot: WorkspaceTreeSnapshot =
            read_bincode_segment::<_, WorkspaceTreeSnapshotCheckpoint>(
                &mut reader,
                "workspace tree snapshot",
            )?
            .into();
        let layout: WorkspaceLayout =
            read_bincode_segment::<_, WorkspaceLayoutCheckpoint>(&mut reader, "workspace layout")?
                .into();
        let graph_snapshot: GraphSnapshot =
            read_bincode_segment::<_, GraphSnapshotCheckpoint>(&mut reader, "graph snapshot")?
                .into();
        let history_snapshot: HistorySnapshot =
            read_bincode_segment::<_, HistorySnapshotCheckpoint>(&mut reader, "history snapshot")?
                .into();
        let intent_index: IntentIndex = read_bincode_segment(&mut reader, "intent index")?;
        let outcome_snapshot: OutcomeMemorySnapshot = decode_json(
            &read_bytes_segment(&mut reader, "outcome snapshot json")?,
            "outcome snapshot",
        )?;
        let coordination_snapshot: prism_coordination::CoordinationSnapshot = decode_json(
            &read_bytes_segment(&mut reader, "coordination snapshot json")?,
            "coordination snapshot",
        )?;
        let projection_snapshot: ProjectionSnapshot = decode_json(
            &read_bytes_segment(&mut reader, "projection snapshot json")?,
            "projection snapshot",
        )?;
        let runtime_state = WorkspaceRuntimeState::new(
            layout.clone(),
            Graph::from_snapshot(graph_snapshot),
            HistoryStore::from_snapshot(history_snapshot.clone()),
            OutcomeMemory::from_snapshot(outcome_snapshot),
            coordination_snapshot,
            Vec::new(),
            ProjectionIndex::from_snapshot_with_history(
                projection_snapshot,
                Some(&history_snapshot),
            ),
        );
        Ok(RestoredWorkspaceRuntimeCheckpoint {
            revisions: header.revisions,
            layout,
            workspace_tree_snapshot,
            intent_index,
            runtime_state,
            local_stale,
            outcome_stale,
            coordination_stale,
        })
    })();
    match restored {
        Ok(restored) => Ok(Some(restored)),
        Err(error) => {
            info!(
                root = %root.display(),
                path = %path.display(),
                checkpoint_bytes,
                error = %error,
                "ignoring unreadable startup checkpoint"
            );
            let _ = fs::remove_file(&path);
            Ok(None)
        }
    }
}

fn merged_snapshot_revisions(
    store: &SqliteStore,
    shared_runtime_store: Option<&SharedRuntimeStore>,
    shared_runtime_aliases_workspace_store: bool,
) -> Result<SnapshotRevisions> {
    let local = store.snapshot_revisions()?;
    if shared_runtime_aliases_workspace_store {
        return Ok(local);
    }
    let Some(shared) = shared_runtime_store else {
        return Ok(local);
    };
    let shared = shared.snapshot_revisions()?;
    Ok(SnapshotRevisions {
        workspace: local.workspace.max(shared.workspace),
        episodic: local.episodic.max(shared.episodic),
        inference: local.inference.max(shared.inference),
        coordination: local.coordination.max(shared.coordination),
    })
}

fn merged_outcome_revision(
    store: &SqliteStore,
    shared_runtime_store: Option<&SharedRuntimeStore>,
    shared_runtime_aliases_workspace_store: bool,
) -> Result<u64> {
    let local = store.outcome_revision()?;
    if shared_runtime_aliases_workspace_store {
        return Ok(local);
    }
    let Some(shared) = shared_runtime_store else {
        return Ok(local);
    };
    Ok(local.max(shared.outcome_revision()?))
}

fn merged_outcome_revision_for_session(session: &WorkspaceSession) -> Result<u64> {
    let local = session
        .store
        .lock()
        .expect("workspace store lock poisoned")
        .outcome_revision()?;
    let Some(shared_runtime_store) = session.shared_runtime_store.as_ref() else {
        return Ok(local);
    };
    let shared_runtime_aliases_workspace_store = session
        .shared_runtime
        .aliases_sqlite_path(&cache_path(session.root())?);
    if shared_runtime_aliases_workspace_store {
        return Ok(local);
    }
    let shared = shared_runtime_store
        .lock()
        .expect("shared runtime store lock poisoned")
        .outcome_revision()?;
    Ok(local.max(shared))
}

fn refresh_restored_runtime_domains(
    root: &Path,
    indexer: &mut WorkspaceIndexer<SqliteStore>,
    options: &WorkspaceSessionOptions,
    shared_runtime_aliases_workspace_store: bool,
    local_stale: bool,
    outcome_stale: bool,
    coordination_stale: bool,
) -> Result<RestoredRuntimeReloadMetrics> {
    let mut metrics = RestoredRuntimeReloadMetrics::default();
    if !local_stale && !outcome_stale && !coordination_stale {
        return Ok(metrics);
    }
    let projection_metadata = indexer.store.load_projection_materialization_metadata()?;
    let local_projection_snapshot = if options.hydrate_persisted_projections {
        indexer.store.load_projection_snapshot()?
    } else if options.hydrate_persisted_co_change {
        indexer.store.load_projection_snapshot()?
    } else {
        indexer.store.load_projection_snapshot_without_co_change()?
    };
    let load_plan = persisted_projection_load_plan(
        projection_metadata,
        options.hydrate_persisted_projections,
        options.hydrate_persisted_co_change,
    );
    let outcomes_started = Instant::now();
    let mut shared_projection_snapshot = None;
    indexer.outcomes = if shared_runtime_aliases_workspace_store {
        if load_plan.load_full_outcomes {
            Store::load_outcome_snapshot(&mut indexer.store)?
        } else {
            Store::load_recent_outcome_snapshot(&mut indexer.store, HOT_OUTCOME_HYDRATION_LIMIT)?
        }
    } else if let Some(shared_store) = indexer.shared_runtime_store.as_mut() {
        shared_projection_snapshot = shared_store.load_projection_knowledge_snapshot()?;
        if load_plan.load_full_outcomes {
            prism_store::ColdQueryStore::load_outcome_snapshot(shared_store)?
        } else {
            prism_store::ColdQueryStore::load_recent_outcome_snapshot(
                shared_store,
                HOT_OUTCOME_HYDRATION_LIMIT,
            )?
        }
    } else if load_plan.load_full_outcomes {
        Store::load_outcome_snapshot(&mut indexer.store)?
    } else {
        Store::load_recent_outcome_snapshot(&mut indexer.store, HOT_OUTCOME_HYDRATION_LIMIT)?
    }
    .map(OutcomeMemory::from_snapshot)
    .unwrap_or_else(OutcomeMemory::new);
    merge_repo_patch_events_into_memory(root, &indexer.outcomes)?;
    metrics.load_outcomes_ms = outcomes_started.elapsed().as_millis();

    let projections_started = Instant::now();
    let repo_knowledge = load_repo_protected_knowledge(root)?;
    let base_local_projection_snapshot = local_projection_snapshot.clone().map(|snapshot| {
        if options.hydrate_persisted_projections {
            snapshot
        } else {
            projection_snapshot_without_knowledge(snapshot)
        }
    });
    let base_shared_projection_snapshot = if options.hydrate_persisted_projections {
        shared_projection_snapshot.clone()
    } else {
        None
    };
    indexer.projections = merged_projection_index(
        base_local_projection_snapshot,
        base_shared_projection_snapshot,
        repo_knowledge.curated_concepts,
        repo_knowledge.curated_contracts,
        repo_knowledge.concept_relations,
        &indexer.history.snapshot(),
        &indexer.outcomes.snapshot(),
    );
    if !options.hydrate_persisted_projections {
        overlay_persisted_projection_knowledge(
            &mut indexer.projections,
            local_projection_snapshot
                .into_iter()
                .chain(shared_projection_snapshot),
        );
    }
    metrics.reload_projections_ms = projections_started.elapsed().as_millis();

    let _ = (root, coordination_stale, options.coordination);

    Ok(metrics)
}

fn local_restore_revisions_match(
    checkpoint: SnapshotRevisions,
    current: SnapshotRevisions,
) -> bool {
    checkpoint.workspace == current.workspace
        && checkpoint.episodic == current.episodic
        && checkpoint.inference == current.inference
}

fn local_restore_revisions_recoverable(
    checkpoint: SnapshotRevisions,
    current: SnapshotRevisions,
) -> bool {
    checkpoint.workspace <= current.workspace
        && checkpoint.episodic <= current.episodic
        && checkpoint.inference <= current.inference
}

fn coordination_restore_stale(
    checkpoint_revision: u64,
    current_revision: u64,
    checkpoint_authority: &CoordinationStartupCheckpointAuthority,
    current_authority: &CoordinationStartupCheckpointAuthority,
) -> bool {
    checkpoint_revision != current_revision || checkpoint_authority != current_authority
}

fn encode_json<T: serde::Serialize>(value: &T) -> Result<Vec<u8>> {
    serde_json::to_vec(value).context("failed to encode startup checkpoint json segment")
}

fn decode_json<T: DeserializeOwned>(bytes: &[u8], label: &str) -> Result<T> {
    serde_json::from_slice(bytes)
        .with_context(|| format!("failed to decode startup checkpoint {}", label))
}

fn portable_path_string(path: impl AsRef<Path>) -> String {
    path.as_ref().to_string_lossy().into_owned()
}

fn write_checkpoint_header<W: Write>(
    writer: &mut W,
    header: &WorkspaceRuntimeStartupCheckpointHeader,
) -> Result<()> {
    writer
        .write_all(WORKSPACE_RUNTIME_STARTUP_CHECKPOINT_MAGIC)
        .context("failed to write startup checkpoint magic")?;
    writer
        .write_all(&header.version.to_le_bytes())
        .context("failed to write startup checkpoint version")?;
    write_u64(writer, header.materialized_at, "materialized_at")?;
    write_u64(writer, header.revisions.workspace, "workspace revision")?;
    write_u64(writer, header.revisions.episodic, "episodic revision")?;
    write_u64(writer, header.revisions.inference, "inference revision")?;
    write_u64(
        writer,
        header.revisions.coordination,
        "coordination revision",
    )?;
    write_u64(writer, header.outcome_revision, "outcome revision")?;
    write_string(
        writer,
        &header.coordination_authority.ref_name,
        "coordination authority ref_name",
    )?;
    write_option_string(
        writer,
        header.coordination_authority.head_commit.as_deref(),
        "coordination authority head_commit",
    )?;
    write_option_string(
        writer,
        header.coordination_authority.manifest_digest.as_deref(),
        "coordination authority manifest_digest",
    )?;
    Ok(())
}

fn read_checkpoint_header<R: Read>(
    reader: &mut R,
) -> Result<WorkspaceRuntimeStartupCheckpointHeader> {
    let mut magic = [0_u8; WORKSPACE_RUNTIME_STARTUP_CHECKPOINT_MAGIC.len()];
    reader
        .read_exact(&mut magic)
        .context("failed to read startup checkpoint magic")?;
    if magic != *WORKSPACE_RUNTIME_STARTUP_CHECKPOINT_MAGIC {
        anyhow::bail!("invalid startup checkpoint magic");
    }
    let version = read_u32(reader, "version")?;
    let materialized_at = read_u64(reader, "materialized_at")?;
    let revisions = SnapshotRevisions {
        workspace: read_u64(reader, "workspace revision")?,
        episodic: read_u64(reader, "episodic revision")?,
        inference: read_u64(reader, "inference revision")?,
        coordination: read_u64(reader, "coordination revision")?,
    };
    let outcome_revision = read_u64(reader, "outcome revision")?;
    let coordination_authority = CoordinationAuthorityCheckpoint {
        ref_name: read_string(reader, "coordination authority ref_name")?,
        head_commit: read_option_string(reader, "coordination authority head_commit")?,
        manifest_digest: read_option_string(reader, "coordination authority manifest_digest")?,
    };
    Ok(WorkspaceRuntimeStartupCheckpointHeader {
        version,
        materialized_at,
        revisions,
        outcome_revision,
        coordination_authority,
    })
}

fn write_bincode_segment<W: Write, T: Serialize>(
    writer: &mut W,
    value: &T,
    label: &str,
) -> Result<()> {
    let byte_len = checkpoint_bincode_options()
        .serialized_size(value)
        .with_context(|| format!("failed to size startup checkpoint {}", label))?;
    write_u64(writer, byte_len, label)?;
    checkpoint_bincode_options()
        .serialize_into(writer, value)
        .with_context(|| format!("failed to encode startup checkpoint {}", label))
}

fn read_bincode_segment<R: Read, T: DeserializeOwned>(reader: &mut R, label: &str) -> Result<T> {
    let byte_len = read_u64(reader, label)?;
    ensure_checkpoint_segment_size(byte_len, label)?;
    let mut limited = reader.take(byte_len);
    let value = checkpoint_bincode_options()
        .with_limit(byte_len)
        .deserialize_from(&mut limited)
        .with_context(|| format!("failed to decode startup checkpoint {}", label))?;
    let mut sink = std::io::sink();
    std::io::copy(&mut limited, &mut sink)
        .with_context(|| format!("failed to drain startup checkpoint {}", label))?;
    Ok(value)
}

fn checkpoint_bincode_options() -> impl Options {
    bincode::DefaultOptions::new().with_fixint_encoding()
}

fn write_bytes_segment<W: Write>(writer: &mut W, bytes: &[u8], label: &str) -> Result<()> {
    write_u64(writer, bytes.len() as u64, label)?;
    writer
        .write_all(bytes)
        .with_context(|| format!("failed to write startup checkpoint {}", label))
}

fn read_bytes_segment<R: Read>(reader: &mut R, label: &str) -> Result<Vec<u8>> {
    let byte_len = read_u64(reader, label)?;
    ensure_checkpoint_segment_size(byte_len, label)?;
    let byte_len = usize::try_from(byte_len)
        .with_context(|| format!("startup checkpoint {} exceeds addressable memory", label))?;
    let mut bytes = vec![0_u8; byte_len];
    reader
        .read_exact(&mut bytes)
        .with_context(|| format!("failed to read startup checkpoint {}", label))?;
    Ok(bytes)
}

fn ensure_checkpoint_segment_size(byte_len: u64, label: &str) -> Result<()> {
    if byte_len > MAX_STARTUP_CHECKPOINT_SEGMENT_BYTES {
        anyhow::bail!(
            "startup checkpoint {} segment is too large ({} bytes)",
            label,
            byte_len
        );
    }
    Ok(())
}

fn write_u64<W: Write>(writer: &mut W, value: u64, label: &str) -> Result<()> {
    writer
        .write_all(&value.to_le_bytes())
        .with_context(|| format!("failed to write startup checkpoint {}", label))
}

fn read_u64<R: Read>(reader: &mut R, label: &str) -> Result<u64> {
    let mut bytes = [0_u8; 8];
    reader
        .read_exact(&mut bytes)
        .with_context(|| format!("failed to read startup checkpoint {}", label))?;
    Ok(u64::from_le_bytes(bytes))
}

fn read_u32<R: Read>(reader: &mut R, label: &str) -> Result<u32> {
    let mut bytes = [0_u8; 4];
    reader
        .read_exact(&mut bytes)
        .with_context(|| format!("failed to read startup checkpoint {}", label))?;
    Ok(u32::from_le_bytes(bytes))
}

fn write_string<W: Write>(writer: &mut W, value: &str, label: &str) -> Result<()> {
    write_bytes_segment(writer, value.as_bytes(), label)
}

fn read_string<R: Read>(reader: &mut R, label: &str) -> Result<String> {
    String::from_utf8(read_bytes_segment(reader, label)?)
        .with_context(|| format!("startup checkpoint {} is not valid utf-8", label))
}

fn write_option_string<W: Write>(writer: &mut W, value: Option<&str>, label: &str) -> Result<()> {
    match value {
        Some(value) => {
            writer
                .write_all(&[1])
                .with_context(|| format!("failed to write startup checkpoint {}", label))?;
            write_string(writer, value, label)?;
        }
        None => writer
            .write_all(&[0])
            .with_context(|| format!("failed to write startup checkpoint {}", label))?,
    }
    Ok(())
}

fn read_option_string<R: Read>(reader: &mut R, label: &str) -> Result<Option<String>> {
    let mut tag = [0_u8; 1];
    reader
        .read_exact(&mut tag)
        .with_context(|| format!("failed to read startup checkpoint {}", label))?;
    match tag[0] {
        0 => Ok(None),
        1 => read_string(reader, label).map(Some),
        other => anyhow::bail!("invalid startup checkpoint {} tag {}", label, other),
    }
}

#[cfg(test)]
mod tests {
    use std::fs;
    use std::path::PathBuf;
    use std::sync::atomic::{AtomicU64, Ordering};
    use std::sync::OnceLock;
    use std::time::{SystemTime, UNIX_EPOCH};

    use prism_ir::{EventMeta, LineageEvent, LineageEventKind, LineageEvidence, NodeId, NodeKind};
    use prism_store::{
        CoordinationStartupCheckpointAuthority, SnapshotRevisions, WorkspaceTreeFileFingerprint,
        WorkspaceTreeSnapshot,
    };

    use super::{
        build_workspace_indexer_with_startup_checkpoint, coordination_restore_stale,
        local_restore_revisions_match, local_restore_revisions_recoverable, read_bincode_segment,
        write_bincode_segment, HistorySnapshotCheckpoint, WorkspaceTreeSnapshotCheckpoint,
    };
    use crate::{index_workspace_session_with_options, WorkspaceSessionOptions};

    static NEXT_TEMP_WORKSPACE: AtomicU64 = AtomicU64::new(1);

    #[test]
    fn history_checkpoint_round_trips_event_meta_without_execution_context() {
        let snapshot = prism_history::HistorySnapshot {
            node_to_lineage: vec![(
                NodeId::new("prism_core", "prism_core::workspace", NodeKind::Workspace),
                prism_ir::LineageId::new("lineage:1"),
            )],
            events: vec![LineageEvent {
                meta: EventMeta {
                    id: prism_ir::EventId::new("event:1"),
                    ts: 42,
                    actor: prism_ir::EventActor::User,
                    correlation: None,
                    causation: None,
                    execution_context: None,
                },
                lineage: prism_ir::LineageId::new("lineage:1"),
                kind: LineageEventKind::Born,
                before: Vec::new(),
                after: vec![NodeId::new(
                    "prism_core",
                    "prism_core::workspace",
                    NodeKind::Workspace,
                )],
                confidence: 1.0,
                evidence: vec![LineageEvidence::ExactNodeId],
            }],
            tombstones: Vec::new(),
            next_lineage: 2,
            next_event: 2,
        };

        let encoded = bincode::serialize(&HistorySnapshotCheckpoint::from(snapshot.clone()))
            .expect("checkpoint should encode");
        let decoded: HistorySnapshotCheckpoint =
            bincode::deserialize(&encoded).expect("checkpoint should decode");
        let restored: prism_history::HistorySnapshot = decoded.into();
        assert_eq!(restored, snapshot);
    }

    #[test]
    fn workspace_tree_checkpoint_segment_round_trips() {
        let snapshot = WorkspaceTreeSnapshotCheckpoint::from(WorkspaceTreeSnapshot {
            root_hash: 42,
            files: [
                (
                    PathBuf::from("src/lib.rs"),
                    WorkspaceTreeFileFingerprint {
                        len: 128,
                        modified_ns: Some(12),
                        changed_ns: Some(34),
                        content_hash: 56,
                    },
                ),
                (
                    PathBuf::from("Cargo.toml"),
                    WorkspaceTreeFileFingerprint {
                        len: 64,
                        modified_ns: Some(78),
                        changed_ns: None,
                        content_hash: 90,
                    },
                ),
            ]
            .into_iter()
            .collect(),
            directories: Default::default(),
        });
        let mut bytes = Vec::new();
        write_bincode_segment(&mut bytes, &snapshot, "workspace tree snapshot")
            .expect("checkpoint segment should encode");
        let decoded: WorkspaceTreeSnapshotCheckpoint =
            read_bincode_segment(&mut std::io::Cursor::new(bytes), "workspace tree snapshot")
                .expect("checkpoint segment should decode");
        assert_eq!(decoded.root_hash, snapshot.root_hash);
        assert_eq!(decoded.files, snapshot.files);
        assert_eq!(decoded.directories, snapshot.directories);
    }

    #[test]
    fn startup_checkpoint_restores_workspace_indexer_from_disk() {
        ensure_test_live_watches_disabled();
        let root = temp_workspace();
        fs::create_dir_all(root.join("src")).expect("workspace src dir should exist");
        fs::write(
            root.join("Cargo.toml"),
            "[package]\nname = \"demo\"\nversion = \"0.1.0\"\nedition = \"2021\"\n",
        )
        .expect("workspace manifest should write");
        fs::write(root.join("src/lib.rs"), "pub fn alpha() -> u32 { 7 }\n")
            .expect("workspace source should write");

        let session =
            index_workspace_session_with_options(&root, WorkspaceSessionOptions::default())
                .expect("initial workspace index should succeed");
        session
            .persist_runtime_startup_checkpoint()
            .expect("startup checkpoint should persist");
        drop(session);

        let restored = build_workspace_indexer_with_startup_checkpoint(
            &root,
            WorkspaceSessionOptions::default(),
        )
        .expect("startup checkpoint restore should succeed");
        let startup_refresh = restored
            .startup_refresh
            .expect("restored indexer should record startup recovery");
        assert_eq!(startup_refresh.path, "recovery");
        assert!(startup_refresh.work.workspace_reloaded);
        assert!(
            restored.startup_intent.is_some(),
            "clean startup checkpoint restore should reuse cached intent"
        );
        assert!(
            restored.trust_cached_query_state,
            "clean startup checkpoint restore should trust cached query state"
        );
    }

    #[test]
    fn local_restore_revisions_ignore_coordination_drift() {
        let checkpoint = SnapshotRevisions {
            workspace: 10,
            episodic: 11,
            inference: 12,
            coordination: 13,
        };
        let current = SnapshotRevisions {
            workspace: 10,
            episodic: 11,
            inference: 12,
            coordination: 99,
        };

        assert!(local_restore_revisions_match(checkpoint, current));
    }

    #[test]
    fn local_restore_revisions_allow_recoverable_forward_drift() {
        let checkpoint = SnapshotRevisions {
            workspace: 10,
            episodic: 11,
            inference: 12,
            coordination: 13,
        };
        let current = SnapshotRevisions {
            workspace: 14,
            episodic: 11,
            inference: 15,
            coordination: 99,
        };

        assert!(local_restore_revisions_recoverable(checkpoint, current));
    }

    #[test]
    fn local_restore_revisions_reject_checkpoint_newer_than_persisted_state() {
        let checkpoint = SnapshotRevisions {
            workspace: 10,
            episodic: 11,
            inference: 12,
            coordination: 13,
        };
        let current = SnapshotRevisions {
            workspace: 9,
            episodic: 11,
            inference: 12,
            coordination: 13,
        };

        assert!(!local_restore_revisions_recoverable(checkpoint, current));
    }

    #[test]
    fn coordination_restore_detects_authority_drift() {
        let checkpoint_authority = CoordinationStartupCheckpointAuthority {
            ref_name: "refs/prism/coordination/demo/live".to_string(),
            head_commit: Some("commit-a".to_string()),
            manifest_digest: Some("manifest-a".to_string()),
        };
        let current_authority = CoordinationStartupCheckpointAuthority {
            ref_name: "refs/prism/coordination/demo/live".to_string(),
            head_commit: Some("commit-b".to_string()),
            manifest_digest: Some("manifest-b".to_string()),
        };

        assert!(coordination_restore_stale(
            1,
            1,
            &checkpoint_authority,
            &current_authority,
        ));
    }

    fn temp_workspace() -> PathBuf {
        let stamp = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        let sequence = NEXT_TEMP_WORKSPACE.fetch_add(1, Ordering::Relaxed);
        std::env::temp_dir().join(format!(
            "prism-startup-checkpoint-test-{}-{stamp}-{sequence}",
            std::process::id()
        ))
    }

    fn ensure_test_live_watches_disabled() {
        static TEST_WATCH_FLAG: OnceLock<()> = OnceLock::new();
        TEST_WATCH_FLAG.get_or_init(|| {
            // SAFETY: tests set this process-wide flag once and never mutate it again.
            unsafe {
                std::env::set_var("PRISM_TEST_DISABLE_LIVE_WATCHERS", "1");
            }
        });
    }
}
