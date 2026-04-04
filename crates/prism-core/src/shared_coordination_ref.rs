use std::collections::{BTreeMap, BTreeSet, HashMap};
use std::fs;
use std::io::Write;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::sync::{Mutex, OnceLock};

use anyhow::{anyhow, Context, Result};
use base64::engine::general_purpose::STANDARD as BASE64_STANDARD;
use base64::Engine;
use ed25519_dalek::{Signer, Verifier};
use prism_coordination::{
    execution_overlays_from_tasks, snapshot_plan_graphs, Artifact, ArtifactReview,
    CoordinationSnapshot, CoordinationTask, Plan, RuntimeDescriptor, RuntimeDescriptorCapability,
    WorkClaim,
};
use prism_ir::{PlanExecutionOverlay, PlanGraph, WorkContextKind, WorkContextSnapshot};
use prism_store::CoordinationStartupCheckpointAuthority;
use serde::{Deserialize, Serialize};
use tracing::warn;

use crate::peer_runtime::{
    configured_public_runtime_endpoint, local_peer_runtime_discovery_mode,
    local_peer_runtime_endpoint,
};
use crate::published_plans::load_hydrated_coordination_plan_state;
use crate::protected_state::canonical::{canonical_json_bytes, sha256_prefixed};
use crate::protected_state::envelope::ProtectedSignatureAlgorithm;
use crate::protected_state::repo_streams::{
    implicit_principal_identity, ProtectedPrincipalIdentity,
};
use crate::protected_state::trust::{load_active_runtime_signing_key, resolve_trusted_runtime_key};
use crate::tracked_snapshot::{SnapshotManifestPublishSummary, TrackedSnapshotPublishContext};
use crate::util::{current_timestamp, stable_hash_bytes};
use crate::workspace_identity::workspace_identity_for_root;
use crate::PrismPaths;

const SHARED_COORDINATION_MANIFEST_VERSION: u32 = 1;
const SHARED_COORDINATION_PUSH_MAX_RETRIES: usize = 3;
const SHARED_COORDINATION_HISTORY_MAX_COMMITS: u64 = 32;
static SHARED_COORDINATION_LIVE_SYNC_STATE: OnceLock<
    Mutex<HashMap<PathBuf, SharedCoordinationLiveSyncState>>,
> = OnceLock::new();
static SHARED_COORDINATION_STATE_CACHE: OnceLock<
    Mutex<HashMap<PathBuf, SharedCoordinationStateCacheEntry>>,
> = OnceLock::new();

#[derive(Debug, Clone, Default)]
struct SharedCoordinationLiveSyncState {
    observed_head: Option<String>,
}

#[derive(Debug, Clone)]
struct SharedCoordinationStateCacheEntry {
    head: String,
    state: SharedCoordinationRefState,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
struct SharedCoordinationManifestPublisher {
    principal_authority_id: String,
    principal_id: String,
    credential_id: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
struct SharedCoordinationManifestFile {
    path: String,
    sha256: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
struct SharedCoordinationManifestSignature {
    algorithm: ProtectedSignatureAlgorithm,
    runtime_authority_id: String,
    runtime_key_id: String,
    trust_bundle_id: String,
    value: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
struct SharedCoordinationManifestPublishDiagnostics {
    retry_count: u32,
    retry_budget: u32,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
enum SharedCoordinationManifestCompactionMode {
    ContinuityPreserved,
    ArchiveBoundary,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
struct SharedCoordinationManifestCompaction {
    mode: SharedCoordinationManifestCompactionMode,
    compacted_at: u64,
    previous_head_commit: String,
    previous_history_depth: u64,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    archive_boundary_manifest_digest: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
struct SharedCoordinationManifest {
    version: u32,
    published_at: u64,
    publisher: SharedCoordinationManifestPublisher,
    work_context: WorkContextSnapshot,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    publish_summary: Option<SnapshotManifestPublishSummary>,
    files: BTreeMap<String, SharedCoordinationManifestFile>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    previous_manifest_digest: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    publish_diagnostics: Option<SharedCoordinationManifestPublishDiagnostics>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    compaction: Option<SharedCoordinationManifestCompaction>,
    signature: SharedCoordinationManifestSignature,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct SharedCoordinationManifestSigningView<'a> {
    version: u32,
    published_at: u64,
    publisher: &'a SharedCoordinationManifestPublisher,
    work_context: &'a WorkContextSnapshot,
    publish_summary: &'a Option<SnapshotManifestPublishSummary>,
    files: &'a BTreeMap<String, SharedCoordinationManifestFile>,
    previous_manifest_digest: &'a Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    publish_diagnostics: &'a Option<SharedCoordinationManifestPublishDiagnostics>,
    #[serde(skip_serializing_if = "Option::is_none")]
    compaction: &'a Option<SharedCoordinationManifestCompaction>,
    signature: SharedCoordinationManifestSignatureMetadata<'a>,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct SharedCoordinationManifestSignatureMetadata<'a> {
    algorithm: ProtectedSignatureAlgorithm,
    runtime_authority_id: &'a str,
    runtime_key_id: &'a str,
    trust_bundle_id: &'a str,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
struct SharedCoordinationIndexEntry {
    id: String,
    title: String,
    status: String,
    path: String,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
struct SharedCoordinationPlanRecord {
    plan: Plan,
    graph: PlanGraph,
    execution_overlays: Vec<PlanExecutionOverlay>,
}

#[derive(Debug, Clone, PartialEq)]
pub(crate) struct SharedCoordinationRefState {
    pub(crate) snapshot: CoordinationSnapshot,
    pub(crate) plan_graphs: Vec<PlanGraph>,
    pub(crate) execution_overlays: BTreeMap<String, Vec<PlanExecutionOverlay>>,
    pub(crate) runtime_descriptors: Vec<RuntimeDescriptor>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SharedCoordinationRefDiagnostics {
    pub ref_name: String,
    pub head_commit: Option<String>,
    pub history_depth: u64,
    pub max_history_commits: u64,
    pub snapshot_file_count: usize,
    pub current_manifest_digest: Option<String>,
    pub last_verified_manifest_digest: Option<String>,
    pub previous_manifest_digest: Option<String>,
    pub last_successful_publish_at: Option<u64>,
    pub last_successful_publish_retry_count: u32,
    pub publish_retry_budget: u32,
    pub compacted_head: bool,
    pub needs_compaction: bool,
    pub compaction_status: String,
    pub compaction_mode: Option<String>,
    pub last_compacted_at: Option<u64>,
    pub compaction_previous_head_commit: Option<String>,
    pub compaction_previous_history_depth: Option<u64>,
    pub archive_boundary_manifest_digest: Option<String>,
    pub runtime_descriptor_count: usize,
    pub runtime_descriptors: Vec<RuntimeDescriptor>,
}

pub(crate) enum SharedCoordinationRefLiveSync {
    Unchanged,
    Changed(SharedCoordinationRefState),
}

fn shared_coordination_ref_name(root: &Path) -> String {
    let identity = workspace_identity_for_root(root);
    let logical_repo_id = identity
        .repo_id
        .chars()
        .map(|ch| {
            if ch.is_ascii_alphanumeric() || ch == '-' || ch == '_' {
                ch.to_ascii_lowercase()
            } else {
                '-'
            }
        })
        .collect::<String>();
    format!("refs/prism/coordination/{logical_repo_id}/live")
}

fn shared_coordination_remote_name() -> &'static str {
    "origin"
}

fn shared_coordination_live_sync_states(
) -> &'static Mutex<HashMap<PathBuf, SharedCoordinationLiveSyncState>> {
    SHARED_COORDINATION_LIVE_SYNC_STATE.get_or_init(|| Mutex::new(HashMap::new()))
}

fn shared_coordination_state_cache(
) -> &'static Mutex<HashMap<PathBuf, SharedCoordinationStateCacheEntry>> {
    SHARED_COORDINATION_STATE_CACHE.get_or_init(|| Mutex::new(HashMap::new()))
}

fn record_observed_shared_coordination_head(root: &Path, head: Option<String>) {
    shared_coordination_live_sync_states()
        .lock()
        .expect("shared coordination live sync state lock poisoned")
        .insert(
            root.to_path_buf(),
            SharedCoordinationLiveSyncState {
                observed_head: head,
            },
        );
}

fn observed_shared_coordination_head(root: &Path) -> Option<String> {
    shared_coordination_live_sync_states()
        .lock()
        .expect("shared coordination live sync state lock poisoned")
        .get(root)
        .and_then(|state| state.observed_head.clone())
}

fn cache_shared_coordination_state(root: &Path, head: String, state: &SharedCoordinationRefState) {
    shared_coordination_state_cache()
        .lock()
        .expect("shared coordination state cache lock poisoned")
        .insert(
            root.to_path_buf(),
            SharedCoordinationStateCacheEntry {
                head,
                state: state.clone(),
            },
        );
}

pub(crate) fn initialize_shared_coordination_ref_live_sync(root: &Path) -> Result<()> {
    if !git_repo_available(root) {
        return Ok(());
    }
    let ref_name = shared_coordination_ref_name(root);
    let head = resolve_ref_commit(root, &ref_name)?;
    record_observed_shared_coordination_head(root, head);
    Ok(())
}

pub(crate) fn poll_shared_coordination_ref_live_sync(
    root: &Path,
) -> Result<SharedCoordinationRefLiveSync> {
    if !git_repo_available(root) {
        return Ok(SharedCoordinationRefLiveSync::Unchanged);
    }
    let ref_name = shared_coordination_ref_name(root);
    let local_head_before = resolve_ref_commit(root, &ref_name)?;
    let remote_head =
        refresh_local_shared_coordination_ref(root, shared_coordination_remote_name(), &ref_name)?;
    let current_head = remote_head.or_else(|| resolve_ref_commit(root, &ref_name).ok().flatten());
    {
        let mut states = shared_coordination_live_sync_states()
            .lock()
            .expect("shared coordination live sync state lock poisoned");
        let state = states.entry(root.to_path_buf()).or_default();
        if state.observed_head == current_head && local_head_before == current_head {
            return Ok(SharedCoordinationRefLiveSync::Unchanged);
        }
        state.observed_head = current_head.clone();
    }
    if current_head.is_none() {
        return Ok(SharedCoordinationRefLiveSync::Unchanged);
    }
    Ok(
        load_shared_coordination_ref_state_from_current_ref(root, &ref_name)?
            .map(SharedCoordinationRefLiveSync::Changed)
            .unwrap_or(SharedCoordinationRefLiveSync::Unchanged),
    )
}

fn stage_root(paths: &PrismPaths) -> PathBuf {
    paths
        .repo_home_dir()
        .join("shared")
        .join("coordination-ref")
        .join("stage")
}

fn stage_snapshot_root(stage_root: &Path) -> PathBuf {
    stage_root.join("coordination")
}

fn stage_manifest_path(stage_root: &Path) -> PathBuf {
    stage_snapshot_root(stage_root).join("manifest.json")
}

fn stage_plans_dir(stage_root: &Path) -> PathBuf {
    stage_snapshot_root(stage_root).join("plans")
}

fn stage_tasks_dir(stage_root: &Path) -> PathBuf {
    stage_snapshot_root(stage_root)
        .join("coordination")
        .join("tasks")
}

fn stage_artifacts_dir(stage_root: &Path) -> PathBuf {
    stage_snapshot_root(stage_root)
        .join("coordination")
        .join("artifacts")
}

fn stage_claims_dir(stage_root: &Path) -> PathBuf {
    stage_snapshot_root(stage_root)
        .join("coordination")
        .join("claims")
}

fn stage_reviews_dir(stage_root: &Path) -> PathBuf {
    stage_snapshot_root(stage_root)
        .join("coordination")
        .join("reviews")
}

fn stage_indexes_dir(stage_root: &Path) -> PathBuf {
    stage_snapshot_root(stage_root).join("indexes")
}

fn stage_runtimes_dir(stage_root: &Path) -> PathBuf {
    stage_snapshot_root(stage_root)
        .join("coordination")
        .join("runtimes")
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
    let digest = stable_hash_bytes(identity.as_bytes());
    format!("{stem}-{digest:016x}.json")
}

#[cfg(test)]
fn plan_snapshot_relative_path(plan_id: &str) -> String {
    format!("plans/{}", snapshot_file_name(plan_id))
}

#[cfg(test)]
fn task_snapshot_relative_path(task_id: &str) -> String {
    format!("coordination/tasks/{}", snapshot_file_name(task_id))
}

fn plan_snapshot_path(stage_root: &Path, plan_id: &str) -> PathBuf {
    stage_plans_dir(stage_root).join(snapshot_file_name(plan_id))
}

fn task_snapshot_path(stage_root: &Path, task_id: &str) -> PathBuf {
    stage_tasks_dir(stage_root).join(snapshot_file_name(task_id))
}

fn artifact_snapshot_path(stage_root: &Path, artifact_id: &str) -> PathBuf {
    stage_artifacts_dir(stage_root).join(snapshot_file_name(artifact_id))
}

fn claim_snapshot_path(stage_root: &Path, claim_id: &str) -> PathBuf {
    stage_claims_dir(stage_root).join(snapshot_file_name(claim_id))
}

fn review_snapshot_path(stage_root: &Path, review_id: &str) -> PathBuf {
    stage_reviews_dir(stage_root).join(snapshot_file_name(review_id))
}

fn runtime_descriptor_snapshot_path(stage_root: &Path, worktree_id: &str) -> PathBuf {
    stage_runtimes_dir(stage_root).join(snapshot_file_name(worktree_id))
}

pub(crate) fn sync_shared_coordination_ref_state(
    root: &Path,
    snapshot: &CoordinationSnapshot,
    plan_graphs: &[PlanGraph],
    execution_overlays: &BTreeMap<String, Vec<PlanExecutionOverlay>>,
    publish: Option<&TrackedSnapshotPublishContext>,
) -> Result<()> {
    if !git_repo_available(root) {
        return Ok(());
    }
    let ref_name = shared_coordination_ref_name(root);
    let expected_remote_head = observed_shared_coordination_head(root)
        .or_else(|| resolve_ref_commit(root, &ref_name).ok().flatten());
    let baseline_state =
        load_shared_coordination_ref_state_from_current_ref_lenient(root, &ref_name)?;
    let paths = PrismPaths::for_workspace_root(root)?;
    let stage_parent = stage_root(&paths);
    fs::create_dir_all(&stage_parent)?;
    let stage_dir = stage_parent.join(format!(
        "stage-{}-{}",
        std::process::id(),
        current_timestamp()
    ));
    fs::create_dir_all(&stage_dir)?;
    let result = sync_shared_coordination_ref_state_inner(
        root,
        &paths,
        &stage_dir,
        snapshot,
        plan_graphs,
        execution_overlays,
        publish,
        &ref_name,
        expected_remote_head.as_deref(),
        baseline_state.as_ref(),
    );
    let _ = fs::remove_dir_all(&stage_dir);
    result
}

pub fn sync_live_runtime_descriptor(root: &Path) -> Result<()> {
    if !git_repo_available(root) {
        return Ok(());
    }
    let ref_name = shared_coordination_ref_name(root);
    let ref_exists = resolve_ref_commit(root, &ref_name)?.is_some();
    let current_shared_state = load_shared_coordination_ref_state(root)?;
    let runtime_descriptors = current_shared_state
        .as_ref()
        .map(|state| state.runtime_descriptors.clone())
        .unwrap_or_default();
    let hydrated = load_hydrated_coordination_plan_state(root, None)?;
    if hydrated.is_none() && ref_exists && current_shared_state.is_none() {
        warn!(
            root = %root.display(),
            ref_name,
            "skipping shared coordination runtime descriptor publish because the current ref is invalid and no local coordination baseline is available"
        );
        return Ok(());
    }
    let state = hydrated
        .map(|state| SharedCoordinationRefState {
            snapshot: state.snapshot,
            plan_graphs: state.plan_graphs,
            execution_overlays: state.execution_overlays,
            runtime_descriptors: runtime_descriptors.clone(),
        })
        .unwrap_or_else(|| SharedCoordinationRefState {
            runtime_descriptors,
            ..empty_shared_coordination_ref_state()
        });
    sync_shared_coordination_ref_state(
        root,
        &state.snapshot,
        &state.plan_graphs,
        &state.execution_overlays,
        None,
    )
}

fn sync_shared_coordination_ref_state_inner(
    root: &Path,
    paths: &PrismPaths,
    stage_dir: &Path,
    snapshot: &CoordinationSnapshot,
    plan_graphs: &[PlanGraph],
    execution_overlays: &BTreeMap<String, Vec<PlanExecutionOverlay>>,
    publish: Option<&TrackedSnapshotPublishContext>,
    ref_name: &str,
    expected_remote_head: Option<&str>,
    baseline_state: Option<&SharedCoordinationRefState>,
) -> Result<()> {
    let desired_snapshot = snapshot.clone();
    let desired_plan_graphs = plan_graphs.to_vec();
    let desired_execution_overlays = execution_overlays.clone();
    let mut current_snapshot = desired_snapshot.clone();
    let mut current_plan_graphs = desired_plan_graphs.clone();
    let mut current_execution_overlays = desired_execution_overlays.clone();
    let mut current_runtime_descriptors = desired_runtime_descriptors(
        root,
        publish,
        baseline_state.map(|state| state.runtime_descriptors.as_slice()),
    )?;
    let mut current_expected_head = expected_remote_head.map(str::to_string);
    let mut current_previous_manifest =
        load_shared_coordination_manifest_from_ref_lenient(root, ref_name)?;

    for attempt in 0..=SHARED_COORDINATION_PUSH_MAX_RETRIES {
        sync_plan_objects(
            stage_dir,
            &current_snapshot,
            &current_plan_graphs,
            &current_execution_overlays,
        )?;
        sync_task_objects(stage_dir, &current_snapshot.tasks)?;
        sync_artifact_objects(stage_dir, &current_snapshot.artifacts)?;
        sync_claim_objects(stage_dir, &current_snapshot.claims)?;
        sync_review_objects(stage_dir, &current_snapshot.reviews)?;
        sync_runtime_descriptor_objects(stage_dir, &current_runtime_descriptors)?;
        rebuild_plan_index(stage_dir, &current_snapshot.plans)?;
        rebuild_task_index(stage_dir, &current_snapshot.tasks)?;
        rebuild_artifact_index(stage_dir, &current_snapshot.artifacts)?;
        rebuild_claim_index(stage_dir, &current_snapshot.claims)?;
        rebuild_review_index(stage_dir, &current_snapshot.reviews)?;
        rebuild_runtime_descriptor_index(stage_dir, &current_runtime_descriptors)?;
        write_manifest(
            stage_dir,
            paths,
            publish,
            current_previous_manifest.as_ref(),
            Some(attempt as u32),
            None,
        )?;
        publish_stage_to_ref(root, stage_dir, ref_name)?;
        match push_shared_coordination_ref(
            root,
            shared_coordination_remote_name(),
            ref_name,
            current_expected_head.as_deref(),
        ) {
            Ok(()) => {
                let published_head = resolve_ref_commit(root, ref_name)?;
                let final_head = maybe_compact_shared_coordination_ref(
                    root,
                    paths,
                    shared_coordination_remote_name(),
                    ref_name,
                    published_head.as_deref(),
                )?;
                if let Some(final_head) = final_head.clone() {
                    cache_shared_coordination_state(
                        root,
                        final_head,
                        &SharedCoordinationRefState {
                            snapshot: current_snapshot.clone(),
                            plan_graphs: current_plan_graphs.clone(),
                            execution_overlays: current_execution_overlays.clone(),
                            runtime_descriptors: current_runtime_descriptors.clone(),
                        },
                    );
                }
                record_observed_shared_coordination_head(root, final_head);
                return Ok(());
            }
            Err(error)
                if attempt < SHARED_COORDINATION_PUSH_MAX_RETRIES
                    && is_shared_coordination_push_conflict(&error) =>
            {
                current_expected_head = refresh_local_shared_coordination_ref(
                    root,
                    shared_coordination_remote_name(),
                    ref_name,
                )?;
                current_previous_manifest =
                    load_shared_coordination_manifest_from_ref_lenient(root, ref_name)?;
                let latest_state =
                    load_shared_coordination_ref_state_from_current_ref_lenient(root, ref_name)?;
                let reconciled = reconcile_shared_coordination_ref_state(
                    baseline_state,
                    &desired_snapshot,
                    latest_state.as_ref(),
                )?;
                current_snapshot = reconciled.snapshot;
                current_plan_graphs = reconciled.plan_graphs;
                current_execution_overlays = reconciled.execution_overlays;
                current_runtime_descriptors = desired_runtime_descriptors(
                    root,
                    publish,
                    latest_state
                        .as_ref()
                        .map(|state| state.runtime_descriptors.as_slice()),
                )?;
            }
            Err(error) => return Err(error),
        }
    }

    Err(anyhow!(
        "shared coordination ref publish exceeded retry budget after repeated compare-and-swap conflicts"
    ))
}

pub(crate) fn load_shared_coordination_ref_state(
    root: &Path,
) -> Result<Option<SharedCoordinationRefState>> {
    if !git_repo_available(root) {
        return Ok(None);
    }
    let ref_name = shared_coordination_ref_name(root);
    let Some(current_head) = resolve_ref_commit(root, &ref_name)? else {
        return Ok(None);
    };
    if let Some(cached) = shared_coordination_state_cache()
        .lock()
        .expect("shared coordination state cache lock poisoned")
        .get(root)
        .filter(|entry| entry.head == current_head)
        .cloned()
    {
        return Ok(Some(cached.state));
    }
    load_shared_coordination_ref_state_from_current_ref_lenient(root, &ref_name)
}

pub(crate) fn shared_coordination_startup_authority(
    root: &Path,
) -> Result<Option<CoordinationStartupCheckpointAuthority>> {
    if !git_repo_available(root) {
        return Ok(None);
    }
    let ref_name = shared_coordination_ref_name(root);
    let Some(head_commit) = resolve_ref_commit(root, &ref_name)? else {
        return Ok(None);
    };
    let manifest_digest = load_shared_coordination_manifest_from_ref_lenient(root, &ref_name)?
        .as_ref()
        .map(canonical_manifest_digest)
        .transpose()?;
    Ok(Some(CoordinationStartupCheckpointAuthority {
        ref_name,
        head_commit: Some(head_commit),
        manifest_digest,
    }))
}

fn load_shared_coordination_ref_state_from_current_ref(
    root: &Path,
    ref_name: &str,
) -> Result<Option<SharedCoordinationRefState>> {
    let Some(current_head) = resolve_ref_commit(root, ref_name)? else {
        return Ok(None);
    };
    if let Some(cached) = shared_coordination_state_cache()
        .lock()
        .expect("shared coordination state cache lock poisoned")
        .get(root)
        .filter(|entry| entry.head == current_head)
        .cloned()
    {
        return Ok(Some(cached.state));
    }
    let Some(contents) = load_shared_coordination_ref_contents(root, ref_name)? else {
        return Ok(None);
    };
    let manifest = contents.parse_manifest()?;
    verify_shared_coordination_manifest(root, &manifest, &contents)?;
    let plan_records = contents
        .parse_records::<SharedCoordinationPlanRecord, _>(|path| path.starts_with("plans/"))?
        .into_iter()
        .map(|(_, record)| record)
        .collect::<Vec<_>>();
    let tasks = contents
        .parse_records::<CoordinationTask, _>(|path| path.starts_with("coordination/tasks/"))?
        .into_iter()
        .map(|(_, task)| task)
        .collect::<Vec<_>>();
    let artifacts = contents
        .parse_records::<Artifact, _>(|path| path.starts_with("coordination/artifacts/"))?
        .into_iter()
        .map(|(_, artifact)| artifact)
        .collect::<Vec<_>>();
    let claims = contents
        .parse_records::<WorkClaim, _>(|path| path.starts_with("coordination/claims/"))?
        .into_iter()
        .map(|(_, claim)| claim)
        .collect::<Vec<_>>();
    let reviews = contents
        .parse_records::<ArtifactReview, _>(|path| path.starts_with("coordination/reviews/"))?
        .into_iter()
        .map(|(_, review)| review)
        .collect::<Vec<_>>();
    let mut runtime_descriptors = contents
        .parse_records::<RuntimeDescriptor, _>(|path| path.starts_with("coordination/runtimes/"))?
        .into_iter()
        .map(|(_, descriptor)| descriptor)
        .collect::<Vec<_>>();

    if plan_records.is_empty()
        && tasks.is_empty()
        && artifacts.is_empty()
        && claims.is_empty()
        && reviews.is_empty()
        && runtime_descriptors.is_empty()
    {
        return Ok(None);
    }

    let mut plans = plan_records
        .iter()
        .map(|record| record.plan.clone())
        .collect::<Vec<_>>();
    let mut plan_graphs = plan_records
        .iter()
        .map(|record| record.graph.clone())
        .collect::<Vec<_>>();
    let mut execution_overlays = plan_records
        .iter()
        .map(|record| {
            (
                record.plan.id.0.to_string(),
                record.execution_overlays.clone(),
            )
        })
        .collect::<BTreeMap<_, _>>();
    plans.sort_by(|left, right| left.id.0.cmp(&right.id.0));
    plan_graphs.sort_by(|left, right| left.id.0.cmp(&right.id.0));

    let mut snapshot = CoordinationSnapshot {
        plans,
        tasks,
        claims,
        artifacts,
        reviews,
        events: Vec::new(),
        next_plan: 0,
        next_task: 0,
        next_claim: 0,
        next_artifact: 0,
        next_review: 0,
    };
    snapshot
        .tasks
        .sort_by(|left, right| left.id.0.cmp(&right.id.0));
    snapshot
        .claims
        .sort_by(|left, right| left.id.0.cmp(&right.id.0));
    snapshot
        .artifacts
        .sort_by(|left, right| left.id.0.cmp(&right.id.0));
    snapshot
        .reviews
        .sort_by(|left, right| left.id.0.cmp(&right.id.0));
    runtime_descriptors.sort_by(|left, right| {
        left.worktree_id
            .cmp(&right.worktree_id)
            .then_with(|| left.runtime_id.cmp(&right.runtime_id))
    });
    for task in &snapshot.tasks {
        execution_overlays
            .entry(task.plan.0.to_string())
            .or_default();
    }
    let state = SharedCoordinationRefState {
        snapshot,
        plan_graphs,
        execution_overlays,
        runtime_descriptors,
    };
    shared_coordination_state_cache()
        .lock()
        .expect("shared coordination state cache lock poisoned")
        .insert(
            root.to_path_buf(),
            SharedCoordinationStateCacheEntry {
                head: current_head,
                state: state.clone(),
            },
        );
    Ok(Some(state))
}

fn load_shared_coordination_ref_state_from_current_ref_lenient(
    root: &Path,
    ref_name: &str,
) -> Result<Option<SharedCoordinationRefState>> {
    match load_shared_coordination_ref_state_from_current_ref(root, ref_name) {
        Ok(state) => Ok(state),
        Err(error) if is_shared_coordination_ref_integrity_error(&error) => {
            warn!(
                root = %root.display(),
                ref_name,
                error = %error,
                "ignoring invalid shared coordination ref state and falling back to local materialized state"
            );
            Ok(None)
        }
        Err(error) => Err(error),
    }
}

fn empty_shared_coordination_ref_state() -> SharedCoordinationRefState {
    SharedCoordinationRefState {
        snapshot: CoordinationSnapshot {
            plans: Vec::new(),
            tasks: Vec::new(),
            claims: Vec::new(),
            artifacts: Vec::new(),
            reviews: Vec::new(),
            events: Vec::new(),
            next_plan: 0,
            next_task: 0,
            next_claim: 0,
            next_artifact: 0,
            next_review: 0,
        },
        plan_graphs: Vec::new(),
        execution_overlays: BTreeMap::new(),
        runtime_descriptors: Vec::new(),
    }
}

fn reconcile_shared_coordination_ref_state(
    baseline: Option<&SharedCoordinationRefState>,
    desired_snapshot: &CoordinationSnapshot,
    latest: Option<&SharedCoordinationRefState>,
) -> Result<SharedCoordinationRefState> {
    let baseline = baseline
        .cloned()
        .unwrap_or_else(empty_shared_coordination_ref_state);
    let latest = latest
        .cloned()
        .unwrap_or_else(empty_shared_coordination_ref_state);

    let plans = reconcile_collection(
        &baseline.snapshot.plans,
        &desired_snapshot.plans,
        &latest.snapshot.plans,
        |plan| plan.id.0.as_str(),
        "plan",
    )?;
    let tasks = reconcile_collection(
        &baseline.snapshot.tasks,
        &desired_snapshot.tasks,
        &latest.snapshot.tasks,
        |task| task.id.0.as_str(),
        "task",
    )?;
    let claims = reconcile_collection(
        &baseline.snapshot.claims,
        &desired_snapshot.claims,
        &latest.snapshot.claims,
        |claim| claim.id.0.as_str(),
        "claim",
    )?;
    let artifacts = reconcile_collection(
        &baseline.snapshot.artifacts,
        &desired_snapshot.artifacts,
        &latest.snapshot.artifacts,
        |artifact| artifact.id.0.as_str(),
        "artifact",
    )?;
    let reviews = reconcile_collection(
        &baseline.snapshot.reviews,
        &desired_snapshot.reviews,
        &latest.snapshot.reviews,
        |review| review.id.0.as_str(),
        "review",
    )?;

    let snapshot = CoordinationSnapshot {
        next_plan: desired_snapshot
            .next_plan
            .max(latest.snapshot.next_plan)
            .max(baseline.snapshot.next_plan),
        next_task: desired_snapshot
            .next_task
            .max(latest.snapshot.next_task)
            .max(baseline.snapshot.next_task),
        next_claim: desired_snapshot
            .next_claim
            .max(latest.snapshot.next_claim)
            .max(baseline.snapshot.next_claim),
        next_artifact: desired_snapshot
            .next_artifact
            .max(latest.snapshot.next_artifact)
            .max(baseline.snapshot.next_artifact),
        next_review: desired_snapshot
            .next_review
            .max(latest.snapshot.next_review)
            .max(baseline.snapshot.next_review),
        plans,
        tasks,
        claims,
        artifacts,
        reviews,
        events: Vec::new(),
    };
    let plan_graphs = snapshot_plan_graphs(&snapshot);
    let execution_overlays = execution_overlays_by_plan(&snapshot);
    Ok(SharedCoordinationRefState {
        snapshot,
        plan_graphs,
        execution_overlays,
        runtime_descriptors: latest.runtime_descriptors,
    })
}

fn execution_overlays_by_plan(
    snapshot: &CoordinationSnapshot,
) -> BTreeMap<String, Vec<PlanExecutionOverlay>> {
    let task_plan_ids = snapshot
        .tasks
        .iter()
        .map(|task| (task.id.0.to_string(), task.plan.0.to_string()))
        .collect::<BTreeMap<_, _>>();
    let mut by_plan = BTreeMap::<String, Vec<PlanExecutionOverlay>>::new();
    for overlay in execution_overlays_from_tasks(&snapshot.tasks) {
        if let Some(plan_id) = task_plan_ids.get(overlay.node_id.0.as_str()) {
            by_plan.entry(plan_id.clone()).or_default().push(overlay);
        }
    }
    for plan in &snapshot.plans {
        by_plan.entry(plan.id.0.to_string()).or_default();
    }
    by_plan
}

fn reconcile_collection<T, F>(
    baseline: &[T],
    desired: &[T],
    latest: &[T],
    key_for: F,
    kind: &str,
) -> Result<Vec<T>>
where
    T: Clone + PartialEq,
    F: Fn(&T) -> &str,
{
    let baseline_map = baseline
        .iter()
        .cloned()
        .map(|value| (key_for(&value).to_string(), value))
        .collect::<BTreeMap<_, _>>();
    let desired_map = desired
        .iter()
        .cloned()
        .map(|value| (key_for(&value).to_string(), value))
        .collect::<BTreeMap<_, _>>();
    let latest_map = latest
        .iter()
        .cloned()
        .map(|value| (key_for(&value).to_string(), value))
        .collect::<BTreeMap<_, _>>();

    let mut result = latest_map.clone();
    let touched_ids = baseline_map
        .keys()
        .chain(desired_map.keys())
        .cloned()
        .collect::<BTreeSet<_>>();

    for id in touched_ids {
        let baseline_value = baseline_map.get(&id);
        let desired_value = desired_map.get(&id);
        if baseline_value == desired_value {
            continue;
        }
        let latest_value = latest_map.get(&id);
        match (baseline_value, desired_value, latest_value) {
            (Some(base), Some(desired), Some(latest)) if latest == base || latest == desired => {
                result.insert(id, desired.clone());
            }
            (Some(base), Some(desired), None) if desired == base => {}
            (None, Some(desired), None) => {
                result.insert(id, desired.clone());
            }
            (None, Some(desired), Some(latest)) if latest == desired => {
                result.insert(id, desired.clone());
            }
            (Some(base), None, Some(latest)) if latest == base => {
                result.remove(&id);
            }
            (Some(_base), None, None) => {
                result.remove(&id);
            }
            (None, None, _) => {}
            _ => {
                return Err(anyhow!(
                    "shared coordination ref {kind} `{id}` changed concurrently and cannot be retried safely"
                ));
            }
        }
    }

    Ok(result.into_values().collect())
}

pub(crate) fn shared_coordination_ref_exists(root: &Path) -> Result<bool> {
    if !git_repo_available(root) {
        return Ok(false);
    }
    Ok(resolve_ref_commit(root, &shared_coordination_ref_name(root))?.is_some())
}

pub fn shared_coordination_ref_diagnostics(
    root: &Path,
) -> Result<Option<SharedCoordinationRefDiagnostics>> {
    if !git_repo_available(root) {
        return Ok(None);
    }
    let ref_name = shared_coordination_ref_name(root);
    let Some(head_commit) = resolve_ref_commit(root, &ref_name)? else {
        return Ok(None);
    };
    let history_depth = ref_history_depth(root, &ref_name)?;
    let compacted_head = ref_head_has_no_parent(root, &ref_name)?;
    let manifest = load_shared_coordination_manifest_from_ref(root, &ref_name)?;
    let current_manifest_digest = manifest
        .as_ref()
        .map(canonical_manifest_digest)
        .transpose()?;
    let verified_state = load_shared_coordination_ref_state_from_current_ref(root, &ref_name)?;
    let last_verified_manifest_digest =
        verified_state.as_ref().and(current_manifest_digest.clone());
    let previous_manifest_digest = manifest
        .as_ref()
        .and_then(|manifest| manifest.previous_manifest_digest.clone());
    let last_successful_publish_at = manifest.as_ref().map(|manifest| manifest.published_at);
    let last_successful_publish_retry_count = manifest
        .as_ref()
        .and_then(|manifest| manifest.publish_diagnostics.as_ref())
        .map(|diagnostics| diagnostics.retry_count)
        .unwrap_or_default();
    let publish_retry_budget = manifest
        .as_ref()
        .and_then(|manifest| manifest.publish_diagnostics.as_ref())
        .map(|diagnostics| diagnostics.retry_budget)
        .unwrap_or_default();
    let compaction_mode = manifest.as_ref().and_then(|manifest| {
        manifest.compaction.as_ref().map(|compaction| {
            match compaction.mode {
                SharedCoordinationManifestCompactionMode::ContinuityPreserved => {
                    "continuity_preserved"
                }
                SharedCoordinationManifestCompactionMode::ArchiveBoundary => "archive_boundary",
            }
            .to_string()
        })
    });
    let last_compacted_at = manifest
        .as_ref()
        .and_then(|manifest| manifest.compaction.as_ref())
        .map(|compaction| compaction.compacted_at);
    let compaction_previous_head_commit = manifest
        .as_ref()
        .and_then(|manifest| manifest.compaction.as_ref())
        .map(|compaction| compaction.previous_head_commit.clone());
    let compaction_previous_history_depth = manifest
        .as_ref()
        .and_then(|manifest| manifest.compaction.as_ref())
        .map(|compaction| compaction.previous_history_depth);
    let archive_boundary_manifest_digest = manifest
        .as_ref()
        .and_then(|manifest| manifest.compaction.as_ref())
        .and_then(|compaction| compaction.archive_boundary_manifest_digest.clone());
    let snapshot_file_count = list_ref_json_paths(root, &ref_name)?.len();
    let runtime_descriptors = verified_state
        .map(|state| state.runtime_descriptors)
        .unwrap_or_default();
    let needs_compaction = history_depth > SHARED_COORDINATION_HISTORY_MAX_COMMITS;
    let compaction_status = if compacted_head {
        "compacted"
    } else if needs_compaction {
        "compaction_recommended"
    } else {
        "healthy"
    };
    Ok(Some(SharedCoordinationRefDiagnostics {
        ref_name,
        head_commit: Some(head_commit),
        history_depth,
        max_history_commits: SHARED_COORDINATION_HISTORY_MAX_COMMITS,
        snapshot_file_count,
        current_manifest_digest,
        last_verified_manifest_digest,
        previous_manifest_digest,
        last_successful_publish_at,
        last_successful_publish_retry_count,
        publish_retry_budget,
        compacted_head,
        needs_compaction,
        compaction_status: compaction_status.to_string(),
        compaction_mode,
        last_compacted_at,
        compaction_previous_head_commit,
        compaction_previous_history_depth,
        archive_boundary_manifest_digest,
        runtime_descriptor_count: runtime_descriptors.len(),
        runtime_descriptors,
    }))
}

fn sync_plan_objects(
    stage_dir: &Path,
    snapshot: &CoordinationSnapshot,
    plan_graphs: &[PlanGraph],
    execution_overlays: &BTreeMap<String, Vec<PlanExecutionOverlay>>,
) -> Result<()> {
    let mut expected = BTreeSet::new();
    for plan in &snapshot.plans {
        let Some(graph) = plan_graphs.iter().find(|graph| graph.id == plan.id) else {
            continue;
        };
        let path = plan_snapshot_path(stage_dir, &plan.id.0);
        expected.insert(path.clone());
        write_json_file(
            &path,
            &SharedCoordinationPlanRecord {
                plan: plan.clone(),
                graph: graph.clone(),
                execution_overlays: execution_overlays
                    .get(plan.id.0.as_str())
                    .cloned()
                    .unwrap_or_default(),
            },
        )?;
    }
    cleanup_directory_json_files(&stage_plans_dir(stage_dir), &expected)
}

fn sync_task_objects(stage_dir: &Path, tasks: &[CoordinationTask]) -> Result<()> {
    let mut expected = BTreeSet::new();
    for task in tasks {
        let path = task_snapshot_path(stage_dir, &task.id.0);
        expected.insert(path.clone());
        write_json_file(&path, task)?;
    }
    cleanup_directory_json_files(&stage_tasks_dir(stage_dir), &expected)
}

fn sync_artifact_objects(stage_dir: &Path, artifacts: &[Artifact]) -> Result<()> {
    let mut expected = BTreeSet::new();
    for artifact in artifacts {
        let path = artifact_snapshot_path(stage_dir, &artifact.id.0);
        expected.insert(path.clone());
        write_json_file(&path, artifact)?;
    }
    cleanup_directory_json_files(&stage_artifacts_dir(stage_dir), &expected)
}

fn sync_claim_objects(stage_dir: &Path, claims: &[WorkClaim]) -> Result<()> {
    let mut expected = BTreeSet::new();
    for claim in claims {
        let path = claim_snapshot_path(stage_dir, &claim.id.0);
        expected.insert(path.clone());
        write_json_file(&path, claim)?;
    }
    cleanup_directory_json_files(&stage_claims_dir(stage_dir), &expected)
}

fn sync_review_objects(stage_dir: &Path, reviews: &[ArtifactReview]) -> Result<()> {
    let mut expected = BTreeSet::new();
    for review in reviews {
        let path = review_snapshot_path(stage_dir, &review.id.0);
        expected.insert(path.clone());
        write_json_file(&path, review)?;
    }
    cleanup_directory_json_files(&stage_reviews_dir(stage_dir), &expected)
}

fn sync_runtime_descriptor_objects(
    stage_dir: &Path,
    descriptors: &[RuntimeDescriptor],
) -> Result<()> {
    let mut expected = BTreeSet::new();
    for descriptor in descriptors {
        let path = runtime_descriptor_snapshot_path(stage_dir, &descriptor.worktree_id);
        expected.insert(path.clone());
        write_json_file(&path, descriptor)?;
    }
    cleanup_directory_json_files(&stage_runtimes_dir(stage_dir), &expected)
}

fn relative_index_path(dir: &Path, path: &Path) -> String {
    path.strip_prefix(dir.parent().unwrap_or(dir))
        .unwrap_or(path)
        .to_string_lossy()
        .replace('\\', "/")
}

fn rebuild_plan_index(stage_dir: &Path, plans: &[Plan]) -> Result<()> {
    let mut entries = plans
        .iter()
        .map(|plan| {
            let path = plan_snapshot_path(stage_dir, &plan.id.0);
            SharedCoordinationIndexEntry {
                id: plan.id.0.to_string(),
                title: if plan.title.trim().is_empty() {
                    plan.goal.clone()
                } else {
                    plan.title.clone()
                },
                status: format!("{:?}", plan.status),
                path: relative_index_path(&stage_plans_dir(stage_dir), &path),
            }
        })
        .collect::<Vec<_>>();
    entries.sort_by(|left, right| left.path.cmp(&right.path));
    write_json_file(&stage_indexes_dir(stage_dir).join("plans.json"), &entries)
}

fn rebuild_task_index(stage_dir: &Path, tasks: &[CoordinationTask]) -> Result<()> {
    let mut entries = tasks
        .iter()
        .map(|task| {
            let path = task_snapshot_path(stage_dir, &task.id.0);
            SharedCoordinationIndexEntry {
                id: task.id.0.to_string(),
                title: task.title.clone(),
                status: format!("{:?}", task.status),
                path: relative_index_path(&stage_tasks_dir(stage_dir), &path),
            }
        })
        .collect::<Vec<_>>();
    entries.sort_by(|left, right| left.path.cmp(&right.path));
    write_json_file(&stage_indexes_dir(stage_dir).join("tasks.json"), &entries)
}

fn rebuild_artifact_index(stage_dir: &Path, artifacts: &[Artifact]) -> Result<()> {
    let mut entries = artifacts
        .iter()
        .map(|artifact| {
            let path = artifact_snapshot_path(stage_dir, &artifact.id.0);
            SharedCoordinationIndexEntry {
                id: artifact.id.0.to_string(),
                title: artifact.task.0.to_string(),
                status: format!("{:?}", artifact.status),
                path: relative_index_path(&stage_artifacts_dir(stage_dir), &path),
            }
        })
        .collect::<Vec<_>>();
    entries.sort_by(|left, right| left.path.cmp(&right.path));
    write_json_file(
        &stage_indexes_dir(stage_dir).join("artifacts.json"),
        &entries,
    )
}

fn rebuild_claim_index(stage_dir: &Path, claims: &[WorkClaim]) -> Result<()> {
    let mut entries = claims
        .iter()
        .map(|claim| {
            let path = claim_snapshot_path(stage_dir, &claim.id.0);
            SharedCoordinationIndexEntry {
                id: claim.id.0.to_string(),
                title: claim
                    .task
                    .as_ref()
                    .map(|task| task.0.to_string())
                    .unwrap_or_else(|| claim.id.0.to_string()),
                status: format!("{:?}", claim.status),
                path: relative_index_path(&stage_claims_dir(stage_dir), &path),
            }
        })
        .collect::<Vec<_>>();
    entries.sort_by(|left, right| left.path.cmp(&right.path));
    write_json_file(&stage_indexes_dir(stage_dir).join("claims.json"), &entries)
}

fn rebuild_review_index(stage_dir: &Path, reviews: &[ArtifactReview]) -> Result<()> {
    let mut entries = reviews
        .iter()
        .map(|review| {
            let path = review_snapshot_path(stage_dir, &review.id.0);
            SharedCoordinationIndexEntry {
                id: review.id.0.to_string(),
                title: review.summary.clone(),
                status: format!("{:?}", review.verdict),
                path: relative_index_path(&stage_reviews_dir(stage_dir), &path),
            }
        })
        .collect::<Vec<_>>();
    entries.sort_by(|left, right| left.path.cmp(&right.path));
    write_json_file(&stage_indexes_dir(stage_dir).join("reviews.json"), &entries)
}

fn rebuild_runtime_descriptor_index(
    stage_dir: &Path,
    descriptors: &[RuntimeDescriptor],
) -> Result<()> {
    let mut entries = descriptors
        .iter()
        .map(|descriptor| {
            let path = runtime_descriptor_snapshot_path(stage_dir, &descriptor.worktree_id);
            SharedCoordinationIndexEntry {
                id: descriptor.runtime_id.clone(),
                title: descriptor.worktree_id.clone(),
                status: format!("{:?}", descriptor.discovery_mode),
                path: relative_index_path(&stage_runtimes_dir(stage_dir), &path),
            }
        })
        .collect::<Vec<_>>();
    entries.sort_by(|left, right| left.path.cmp(&right.path));
    write_json_file(
        &stage_indexes_dir(stage_dir).join("runtimes.json"),
        &entries,
    )
}

fn desired_runtime_descriptors(
    root: &Path,
    publish: Option<&TrackedSnapshotPublishContext>,
    existing: Option<&[RuntimeDescriptor]>,
) -> Result<Vec<RuntimeDescriptor>> {
    let local = local_runtime_descriptor(root, publish, existing)?;
    let mut descriptors = existing.unwrap_or(&[]).to_vec();
    descriptors.retain(|descriptor| descriptor.worktree_id != local.worktree_id);
    descriptors.push(local);
    descriptors.sort_by(|left, right| {
        left.worktree_id
            .cmp(&right.worktree_id)
            .then_with(|| left.runtime_id.cmp(&right.runtime_id))
    });
    Ok(descriptors)
}

fn local_runtime_descriptor(
    root: &Path,
    publish: Option<&TrackedSnapshotPublishContext>,
    existing: Option<&[RuntimeDescriptor]>,
) -> Result<RuntimeDescriptor> {
    let identity = workspace_identity_for_root(root);
    let now = current_timestamp();
    let publish = publish
        .cloned()
        .unwrap_or_else(|| TrackedSnapshotPublishContext {
            published_at: now,
            principal: implicit_principal_identity(None, None),
            work_context: Some(implicit_work_context()),
            publish_summary: None,
        });
    let previous = existing
        .unwrap_or(&[])
        .iter()
        .find(|descriptor| descriptor.worktree_id == identity.worktree_id);
    let continuing_instance =
        previous.filter(|descriptor| descriptor.runtime_id == identity.instance_id);
    let peer_endpoint = local_peer_runtime_endpoint(root)?
        .or_else(|| previous.and_then(|descriptor| descriptor.peer_endpoint.clone()));
    let public_endpoint = configured_public_runtime_endpoint(root)?;
    let discovery_mode =
        local_peer_runtime_discovery_mode(peer_endpoint.as_deref(), public_endpoint.as_deref());
    let mut capabilities = vec![RuntimeDescriptorCapability::CoordinationRefPublisher];
    if peer_endpoint.is_some() || public_endpoint.is_some() {
        capabilities.push(RuntimeDescriptorCapability::BoundedPeerReads);
    }
    Ok(RuntimeDescriptor {
        runtime_id: identity.instance_id,
        repo_id: identity.repo_id,
        worktree_id: identity.worktree_id,
        principal_id: publish.principal.principal_id,
        instance_started_at: continuing_instance
            .map(|descriptor| descriptor.instance_started_at)
            .unwrap_or(now),
        last_seen_at: now,
        branch_ref: identity.branch_ref,
        checked_out_commit: resolve_checked_out_commit(root)?,
        capabilities,
        discovery_mode,
        peer_endpoint,
        public_endpoint,
        peer_transport_identity: previous
            .and_then(|descriptor| descriptor.peer_transport_identity.clone()),
        blob_snapshot_head: previous.and_then(|descriptor| descriptor.blob_snapshot_head.clone()),
        export_policy: previous.and_then(|descriptor| descriptor.export_policy.clone()),
    })
}

fn resolve_checked_out_commit(root: &Path) -> Result<Option<String>> {
    resolve_ref_commit(root, "HEAD")
}

fn write_manifest(
    stage_dir: &Path,
    paths: &PrismPaths,
    publish: Option<&TrackedSnapshotPublishContext>,
    previous_manifest: Option<&SharedCoordinationManifest>,
    publish_retry_count: Option<u32>,
    compaction: Option<SharedCoordinationManifestCompaction>,
) -> Result<()> {
    let previous_manifest_digest = previous_manifest
        .map(canonical_manifest_digest)
        .transpose()?;
    let files = collect_snapshot_file_map(stage_dir)?;
    let publish = publish
        .cloned()
        .or_else(|| previous_manifest.map(publish_context_from_manifest))
        .unwrap_or_else(|| TrackedSnapshotPublishContext {
            published_at: current_timestamp(),
            principal: implicit_principal_identity(None, None),
            work_context: Some(implicit_work_context()),
            publish_summary: None,
        });
    let work_context = publish.work_context.unwrap_or_else(implicit_work_context);
    let publish_diagnostics = publish_retry_count
        .map(|retry_count| SharedCoordinationManifestPublishDiagnostics {
            retry_count,
            retry_budget: SHARED_COORDINATION_PUSH_MAX_RETRIES as u32,
        })
        .or_else(|| previous_manifest.and_then(|manifest| manifest.publish_diagnostics.clone()));
    let active_key = load_active_runtime_signing_key(paths)?;
    let mut manifest = SharedCoordinationManifest {
        version: SHARED_COORDINATION_MANIFEST_VERSION,
        published_at: publish.published_at,
        publisher: SharedCoordinationManifestPublisher {
            principal_authority_id: publish.principal.principal_authority_id,
            principal_id: publish.principal.principal_id,
            credential_id: publish.principal.credential_id,
        },
        work_context,
        publish_summary: publish.publish_summary,
        files,
        previous_manifest_digest,
        publish_diagnostics,
        compaction,
        signature: SharedCoordinationManifestSignature {
            algorithm: ProtectedSignatureAlgorithm::Ed25519,
            runtime_authority_id: active_key.state.runtime_authority_id.clone(),
            runtime_key_id: active_key.runtime_key.runtime_key_id.clone(),
            trust_bundle_id: active_key.bundle.bundle_id.clone(),
            value: String::new(),
        },
    };
    let signature = active_key.signing_key.sign(&canonical_json_bytes(
        &SharedCoordinationManifestSigningView {
            version: manifest.version,
            published_at: manifest.published_at,
            publisher: &manifest.publisher,
            work_context: &manifest.work_context,
            publish_summary: &manifest.publish_summary,
            files: &manifest.files,
            previous_manifest_digest: &manifest.previous_manifest_digest,
            publish_diagnostics: &manifest.publish_diagnostics,
            compaction: &manifest.compaction,
            signature: SharedCoordinationManifestSignatureMetadata {
                algorithm: manifest.signature.algorithm,
                runtime_authority_id: &manifest.signature.runtime_authority_id,
                runtime_key_id: &manifest.signature.runtime_key_id,
                trust_bundle_id: &manifest.signature.trust_bundle_id,
            },
        },
    )?);
    manifest.signature.value = format!("base64:{}", BASE64_STANDARD.encode(signature.to_bytes()));
    write_json_file(&stage_manifest_path(stage_dir), &manifest)
}

fn load_shared_coordination_manifest_from_ref(
    root: &Path,
    ref_name: &str,
) -> Result<Option<SharedCoordinationManifest>> {
    if resolve_ref_commit(root, ref_name)?.is_none() {
        return Ok(None);
    }
    let blob = run_git(
        root,
        &["show", &format!("{ref_name}:coordination/manifest.json")],
    );
    match blob {
        Ok(contents) => Ok(Some(
            serde_json::from_str(contents.trim())
                .context("failed to parse shared coordination manifest from git ref")?,
        )),
        Err(error) => {
            let message = error.to_string();
            if message.contains("does not exist")
                || message.contains("exists on disk, but not in")
                || message.contains("path 'coordination/manifest.json' does not exist")
            {
                Ok(None)
            } else {
                Err(error)
            }
        }
    }
}

fn load_shared_coordination_manifest_from_ref_lenient(
    root: &Path,
    ref_name: &str,
) -> Result<Option<SharedCoordinationManifest>> {
    match load_shared_coordination_manifest_from_ref(root, ref_name) {
        Ok(manifest) => Ok(manifest),
        Err(error) if is_shared_coordination_ref_integrity_error(&error) => {
            warn!(
                root = %root.display(),
                ref_name,
                error = %error,
                "ignoring invalid shared coordination manifest and falling back to local materialized state"
            );
            Ok(None)
        }
        Err(error) => Err(error),
    }
}

fn verify_shared_coordination_manifest(
    root: &Path,
    manifest: &SharedCoordinationManifest,
    contents: &SharedCoordinationRefContents,
) -> Result<()> {
    let paths = PrismPaths::for_workspace_root(root)?;
    let trusted = resolve_trusted_runtime_key(
        &paths,
        &manifest.signature.trust_bundle_id,
        &manifest.signature.runtime_authority_id,
        &manifest.signature.runtime_key_id,
    )?;
    let signature = decode_signature(&manifest.signature.value)?;
    trusted
        .verifying_key
        .verify(
            &canonical_json_bytes(&SharedCoordinationManifestSigningView {
                version: manifest.version,
                published_at: manifest.published_at,
                publisher: &manifest.publisher,
                work_context: &manifest.work_context,
                publish_summary: &manifest.publish_summary,
                files: &manifest.files,
                previous_manifest_digest: &manifest.previous_manifest_digest,
                publish_diagnostics: &manifest.publish_diagnostics,
                compaction: &manifest.compaction,
                signature: SharedCoordinationManifestSignatureMetadata {
                    algorithm: manifest.signature.algorithm,
                    runtime_authority_id: &manifest.signature.runtime_authority_id,
                    runtime_key_id: &manifest.signature.runtime_key_id,
                    trust_bundle_id: &manifest.signature.trust_bundle_id,
                },
            })?,
            &signature,
        )
        .map_err(|error| {
            anyhow!("shared coordination manifest signature verification failed: {error}")
        })?;
    for file in manifest.files.values() {
        let bytes = contents
            .coordination_bytes(&file.path)
            .ok_or_else(|| anyhow!("shared coordination manifest is missing `{}`", file.path))?;
        let digest = sha256_prefixed(&bytes);
        if digest != file.sha256 {
            return Err(anyhow!(
                "shared coordination manifest digest mismatch for `{}`",
                file.path
            ));
        }
    }
    Ok(())
}

fn publish_stage_to_ref(root: &Path, stage_dir: &Path, ref_name: &str) -> Result<()> {
    let tree = write_stage_tree(root, stage_dir)?;
    let parent = resolve_ref_commit(root, ref_name)?;
    let message = if parent.is_some() {
        "prism: update shared coordination ref"
    } else {
        "prism: initialize shared coordination ref"
    };
    let commit = create_tree_commit(root, tree.trim(), parent.as_deref(), message)?;
    update_ref_to_commit(root, ref_name, commit.trim(), parent.as_deref(), message)?;
    Ok(())
}

fn write_stage_tree(root: &Path, stage_dir: &Path) -> Result<String> {
    let index_path = stage_dir.join(".shared-coordination.index");
    let index_path_str = index_path.to_string_lossy().to_string();
    let envs = [("GIT_INDEX_FILE", index_path_str.as_str())];
    let _ = run_git_with_env(root, &envs, &["read-tree", "--empty"])?;
    let _ = run_git_with_env(
        root,
        &envs,
        &[
            "--work-tree",
            stage_dir.to_string_lossy().as_ref(),
            "add",
            "-A",
            "--",
            "coordination",
        ],
    )?;
    run_git_with_env(root, &envs, &["write-tree"])
}

fn create_tree_commit(
    root: &Path,
    tree: &str,
    parent: Option<&str>,
    message: &str,
) -> Result<String> {
    match parent {
        Some(parent) => run_git(root, &["commit-tree", tree, "-p", parent, "-m", message]),
        None => run_git(root, &["commit-tree", tree, "-m", message]),
    }
}

fn git_remote_available(root: &Path, remote: &str) -> bool {
    run_git(root, &["remote", "get-url", remote]).is_ok()
}

fn refresh_local_shared_coordination_ref(
    root: &Path,
    remote: &str,
    ref_name: &str,
) -> Result<Option<String>> {
    if !git_remote_available(root, remote) {
        return Ok(resolve_ref_commit(root, ref_name)?);
    }
    let output = run_git(root, &["ls-remote", remote, ref_name])?;
    let remote_head = output
        .lines()
        .find_map(|line| line.split_whitespace().next().map(str::to_string));
    if remote_head.is_some() {
        let refspec = format!("+{ref_name}:{ref_name}");
        let _ = run_git(root, &["fetch", remote, &refspec])?;
    }
    Ok(remote_head)
}

fn push_shared_coordination_ref(
    root: &Path,
    remote: &str,
    ref_name: &str,
    expected_remote_head: Option<&str>,
) -> Result<()> {
    push_commit_to_shared_coordination_ref(root, remote, ref_name, ref_name, expected_remote_head)
}

fn push_commit_to_shared_coordination_ref(
    root: &Path,
    remote: &str,
    ref_name: &str,
    source: &str,
    expected_remote_head: Option<&str>,
) -> Result<()> {
    if !git_remote_available(root, remote) {
        return Ok(());
    }
    let lease = format!(
        "--force-with-lease={ref_name}:{}",
        expected_remote_head.unwrap_or("")
    );
    let refspec = format!("{source}:{ref_name}");
    let _ = run_git(root, &["push", "--porcelain", &lease, remote, &refspec])?;
    Ok(())
}

fn maybe_compact_shared_coordination_ref(
    root: &Path,
    paths: &PrismPaths,
    remote: &str,
    ref_name: &str,
    current_head: Option<&str>,
) -> Result<Option<String>> {
    let Some(current_head) = current_head else {
        return Ok(None);
    };
    let history_depth = ref_history_depth(root, ref_name)?;
    if history_depth <= SHARED_COORDINATION_HISTORY_MAX_COMMITS {
        return Ok(Some(current_head.to_string()));
    }
    let compact_commit = create_compacted_shared_coordination_commit(
        root,
        paths,
        ref_name,
        current_head,
        history_depth,
    )?;
    if git_remote_available(root, remote) {
        push_commit_to_shared_coordination_ref(
            root,
            remote,
            ref_name,
            compact_commit.trim(),
            Some(current_head),
        )?;
    }
    update_ref_to_commit(
        root,
        ref_name,
        compact_commit.trim(),
        Some(current_head),
        "prism: compact shared coordination ref",
    )?;
    Ok(Some(compact_commit.trim().to_string()))
}

fn create_compacted_shared_coordination_commit(
    root: &Path,
    paths: &PrismPaths,
    ref_name: &str,
    current_head: &str,
    previous_history_depth: u64,
) -> Result<String> {
    let previous_manifest = load_shared_coordination_manifest_from_ref(root, ref_name)?
        .ok_or_else(|| anyhow!("shared coordination manifest missing before compaction"))?;
    let stage_parent = stage_root(paths);
    fs::create_dir_all(&stage_parent)?;
    let suffix = stable_hash_bytes(current_head.as_bytes());
    let stage_dir = stage_parent.join(format!("compaction-{}-{suffix:016x}", current_timestamp()));
    materialize_shared_coordination_ref_to_stage_dir(root, ref_name, &stage_dir)?;
    let result = (|| {
        write_manifest(
            &stage_dir,
            paths,
            None,
            Some(&previous_manifest),
            None,
            Some(SharedCoordinationManifestCompaction {
                mode: SharedCoordinationManifestCompactionMode::ContinuityPreserved,
                compacted_at: current_timestamp(),
                previous_head_commit: current_head.to_string(),
                previous_history_depth,
                archive_boundary_manifest_digest: None,
            }),
        )?;
        let tree = write_stage_tree(root, &stage_dir)?;
        create_tree_commit(
            root,
            tree.trim(),
            None,
            "prism: compact shared coordination ref",
        )
    })();
    let _ = fs::remove_dir_all(&stage_dir);
    result
}

fn is_shared_coordination_push_conflict(error: &anyhow::Error) -> bool {
    let message = error.to_string().to_ascii_lowercase();
    message.contains("stale info")
        || message.contains("fetch first")
        || message.contains("non-fast-forward")
        || message.contains("[rejected]")
        || message.contains("failed to push some refs")
}

fn resolve_ref_commit(root: &Path, ref_name: &str) -> Result<Option<String>> {
    match run_git(root, &["rev-parse", "--verify", ref_name]) {
        Ok(value) => Ok(Some(value)),
        Err(error)
            if error.to_string().contains("unknown revision")
                || error.to_string().contains("Needed a single revision")
                || error.to_string().contains("ambiguous argument") =>
        {
            Ok(None)
        }
        Err(error) => Err(error),
    }
}

fn ref_history_depth(root: &Path, ref_name: &str) -> Result<u64> {
    let output = run_git(root, &["rev-list", "--count", ref_name])?;
    output.trim().parse::<u64>().with_context(|| {
        format!("failed to parse shared coordination ref history depth for `{ref_name}`")
    })
}

fn ref_head_has_no_parent(root: &Path, ref_name: &str) -> Result<bool> {
    let output = run_git(root, &["rev-list", "--parents", "-n", "1", ref_name])?;
    Ok(output.split_whitespace().count() <= 1)
}

fn update_ref_to_commit(
    root: &Path,
    ref_name: &str,
    new_commit: &str,
    old_commit: Option<&str>,
    message: &str,
) -> Result<()> {
    match old_commit {
        Some(old_commit) => {
            let _ = run_git(
                root,
                &[
                    "update-ref",
                    "-m",
                    message,
                    ref_name,
                    new_commit,
                    old_commit,
                ],
            )?;
        }
        None => {
            let _ = run_git(root, &["update-ref", "-m", message, ref_name, new_commit])?;
        }
    }
    Ok(())
}

#[cfg(test)]
#[derive(Debug, Clone, PartialEq, Eq)]
struct SharedCoordinationPublishPatch {
    upserts: BTreeSet<String>,
    deletes: BTreeSet<String>,
}

#[cfg(test)]
fn build_shared_coordination_publish_patch(
    stage_dir: &Path,
    previous_manifest: Option<&SharedCoordinationManifest>,
    _baseline_state: Option<&SharedCoordinationRefState>,
    desired_state: &SharedCoordinationRefState,
) -> Result<SharedCoordinationPublishPatch> {
    let _ = fs::remove_dir_all(stage_dir);
    fs::create_dir_all(stage_dir)?;
    sync_plan_objects(
        stage_dir,
        &desired_state.snapshot,
        &desired_state.plan_graphs,
        &desired_state.execution_overlays,
    )?;
    sync_task_objects(stage_dir, &desired_state.snapshot.tasks)?;
    sync_artifact_objects(stage_dir, &desired_state.snapshot.artifacts)?;
    sync_claim_objects(stage_dir, &desired_state.snapshot.claims)?;
    sync_review_objects(stage_dir, &desired_state.snapshot.reviews)?;
    sync_runtime_descriptor_objects(stage_dir, &desired_state.runtime_descriptors)?;
    rebuild_plan_index(stage_dir, &desired_state.snapshot.plans)?;
    rebuild_task_index(stage_dir, &desired_state.snapshot.tasks)?;
    rebuild_artifact_index(stage_dir, &desired_state.snapshot.artifacts)?;
    rebuild_claim_index(stage_dir, &desired_state.snapshot.claims)?;
    rebuild_review_index(stage_dir, &desired_state.snapshot.reviews)?;
    rebuild_runtime_descriptor_index(stage_dir, &desired_state.runtime_descriptors)?;

    let previous_files = previous_manifest
        .map(|manifest| manifest.files.clone())
        .unwrap_or_default();
    let staged_files = collect_snapshot_file_map(stage_dir)?;
    let tracked_paths = previous_files
        .keys()
        .chain(staged_files.keys())
        .cloned()
        .collect::<BTreeSet<_>>();
    let mut upserts = BTreeSet::new();
    let mut deletes = BTreeSet::new();
    for path in tracked_paths {
        match (previous_files.get(&path), staged_files.get(&path)) {
            (Some(previous), Some(current)) if previous == current => {}
            (_, Some(_)) => {
                upserts.insert(format!("coordination/{path}"));
            }
            (Some(_), None) => {
                deletes.insert(format!("coordination/{path}"));
            }
            (None, None) => {}
        }
    }
    Ok(SharedCoordinationPublishPatch { upserts, deletes })
}

fn materialize_shared_coordination_ref_to_stage_dir(
    root: &Path,
    ref_name: &str,
    stage_dir: &Path,
) -> Result<()> {
    let Some(contents) = load_shared_coordination_ref_contents(root, ref_name)? else {
        return Err(anyhow!(
            "shared coordination ref `{ref_name}` cannot be compacted because its contents are missing"
        ));
    };
    let _ = fs::remove_dir_all(stage_dir);
    fs::create_dir_all(stage_snapshot_root(stage_dir))?;
    for (relative_path, bytes) in contents.files {
        let path = stage_snapshot_root(stage_dir).join(relative_path);
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent)?;
        }
        fs::write(path, bytes)?;
    }
    Ok(())
}

fn list_ref_json_paths(root: &Path, ref_name: &str) -> Result<Vec<String>> {
    Ok(list_ref_blob_entries(root, ref_name)?
        .into_iter()
        .map(|entry| entry.relative_path)
        .filter(|relative| relative.ends_with(".json") && relative != "manifest.json")
        .collect())
}

#[derive(Debug, Clone)]
struct RefBlobEntry {
    relative_path: String,
    object_id: String,
}

#[derive(Debug, Clone)]
struct SharedCoordinationRefContents {
    files: BTreeMap<String, Vec<u8>>,
}

impl SharedCoordinationRefContents {
    fn parse_manifest(&self) -> Result<SharedCoordinationManifest> {
        let bytes = self
            .files
            .get("manifest.json")
            .ok_or_else(|| anyhow!("shared coordination manifest is missing"))?;
        serde_json::from_slice(bytes).context("failed to parse shared coordination manifest")
    }

    fn coordination_bytes(&self, relative_path: &str) -> Option<Vec<u8>> {
        self.files.get(relative_path).cloned()
    }

    fn parse_records<T, F>(&self, filter: F) -> Result<Vec<(String, T)>>
    where
        T: for<'de> Deserialize<'de>,
        F: Fn(&str) -> bool,
    {
        let mut records = self
            .files
            .iter()
            .filter(|(path, _)| filter(path.as_str()))
            .map(|(path, bytes)| {
                let value = serde_json::from_slice::<T>(bytes).with_context(|| {
                    format!("failed to parse shared coordination ref file `{path}`")
                })?;
                Ok((path.clone(), value))
            })
            .collect::<Result<Vec<_>>>()?;
        records.sort_by(|left, right| left.0.cmp(&right.0));
        Ok(records)
    }
}

fn load_shared_coordination_ref_contents(
    root: &Path,
    ref_name: &str,
) -> Result<Option<SharedCoordinationRefContents>> {
    let Some(_) = resolve_ref_commit(root, ref_name)? else {
        return Ok(None);
    };
    let entries = list_ref_blob_entries(root, ref_name)?;
    if entries.is_empty() {
        return Ok(None);
    }
    let blob_ids = entries
        .iter()
        .map(|entry| entry.object_id.as_str())
        .collect::<Vec<_>>();
    let blobs = git_cat_file_batch(root, &blob_ids)?;
    let mut files = BTreeMap::new();
    for entry in entries {
        if let Some(bytes) = blobs.get(&entry.object_id) {
            files.insert(entry.relative_path, bytes.clone());
        }
    }
    Ok(Some(SharedCoordinationRefContents { files }))
}

fn list_ref_blob_entries(root: &Path, ref_name: &str) -> Result<Vec<RefBlobEntry>> {
    let output = Command::new("git")
        .current_dir(root)
        .args(["ls-tree", "-r", "-z", ref_name, "coordination"])
        .output()
        .with_context(|| format!("failed to list shared coordination ref tree for `{ref_name}`"))?;
    if !output.status.success() {
        return Err(anyhow!(
            "git ls-tree -r -z {} coordination failed: {}",
            ref_name,
            String::from_utf8_lossy(&output.stderr)
        ));
    }
    let mut entries = Vec::new();
    for record in output
        .stdout
        .split(|byte| *byte == 0)
        .filter(|record| !record.is_empty())
    {
        let Some(tab_index) = record.iter().position(|byte| *byte == b'\t') else {
            continue;
        };
        let header = std::str::from_utf8(&record[..tab_index])
            .context("shared coordination ls-tree header is not utf-8")?;
        let path = std::str::from_utf8(&record[tab_index + 1..])
            .context("shared coordination ls-tree path is not utf-8")?;
        let mut parts = header.split_whitespace();
        let _mode = parts.next();
        let kind = parts.next();
        let object_id = parts.next();
        if kind != Some("blob") {
            continue;
        }
        let Some(relative) = path.strip_prefix("coordination/") else {
            continue;
        };
        if !relative.ends_with(".json") {
            continue;
        }
        let Some(object_id) = object_id else {
            continue;
        };
        entries.push(RefBlobEntry {
            relative_path: relative.to_string(),
            object_id: object_id.to_string(),
        });
    }
    Ok(entries)
}

fn git_cat_file_batch(root: &Path, object_ids: &[&str]) -> Result<HashMap<String, Vec<u8>>> {
    if object_ids.is_empty() {
        return Ok(HashMap::new());
    }
    let mut child = Command::new("git")
        .current_dir(root)
        .args(["cat-file", "--batch"])
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .context("failed to spawn git cat-file --batch")?;
    {
        let mut stdin = child
            .stdin
            .take()
            .context("git cat-file stdin unavailable")?;
        for object_id in object_ids {
            writeln!(stdin, "{object_id}")?;
        }
    }
    let output = child
        .wait_with_output()
        .context("failed to wait for git cat-file --batch")?;
    if !output.status.success() {
        return Err(anyhow!(
            "git cat-file --batch failed: {}",
            String::from_utf8_lossy(&output.stderr)
        ));
    }
    parse_git_cat_file_batch_output(&output.stdout)
}

fn parse_git_cat_file_batch_output(stdout: &[u8]) -> Result<HashMap<String, Vec<u8>>> {
    let mut blobs = HashMap::new();
    let mut cursor = 0usize;
    while cursor < stdout.len() {
        let Some(header_end_rel) = stdout[cursor..].iter().position(|byte| *byte == b'\n') else {
            break;
        };
        let header_end = cursor + header_end_rel;
        let header = std::str::from_utf8(&stdout[cursor..header_end])
            .context("git cat-file batch header is not utf-8")?;
        cursor = header_end + 1;
        if header.ends_with(" missing") {
            continue;
        }
        let mut parts = header.split_whitespace();
        let object_id = parts
            .next()
            .ok_or_else(|| anyhow!("git cat-file batch header missing object id"))?;
        let object_type = parts
            .next()
            .ok_or_else(|| anyhow!("git cat-file batch header missing object type"))?;
        let size = parts
            .next()
            .ok_or_else(|| anyhow!("git cat-file batch header missing size"))?
            .parse::<usize>()
            .context("failed to parse git cat-file batch object size")?;
        if object_type != "blob" {
            return Err(anyhow!(
                "git cat-file batch returned non-blob object `{object_type}` for `{object_id}`"
            ));
        }
        let end = cursor.saturating_add(size);
        if end > stdout.len() {
            return Err(anyhow!(
                "git cat-file batch output truncated while reading `{object_id}`"
            ));
        }
        blobs.insert(object_id.to_string(), stdout[cursor..end].to_vec());
        cursor = end;
        if cursor < stdout.len() && stdout[cursor] == b'\n' {
            cursor += 1;
        }
    }
    Ok(blobs)
}

fn run_git(root: &Path, args: &[&str]) -> Result<String> {
    run_git_with_env(root, &[], args)
}

fn git_repo_available(root: &Path) -> bool {
    run_git(root, &["rev-parse", "--git-dir"]).is_ok()
}

fn run_git_with_env(root: &Path, envs: &[(&str, &str)], args: &[&str]) -> Result<String> {
    let mut command = Command::new("git");
    command
        .current_dir(root)
        .env("GIT_AUTHOR_NAME", "PRISM")
        .env("GIT_AUTHOR_EMAIL", "prism@local")
        .env("GIT_COMMITTER_NAME", "PRISM")
        .env("GIT_COMMITTER_EMAIL", "prism@local");
    for (key, value) in envs {
        command.env(key, value);
    }
    let output = command
        .args(args)
        .output()
        .with_context(|| format!("failed to run git {}", args.join(" ")))?;
    if !output.status.success() {
        return Err(anyhow!(
            "git {} failed: {}",
            args.join(" "),
            String::from_utf8_lossy(&output.stderr).trim()
        ));
    }
    Ok(String::from_utf8_lossy(&output.stdout)
        .trim_end()
        .to_string())
}

fn canonical_manifest_digest(manifest: &SharedCoordinationManifest) -> Result<String> {
    Ok(sha256_prefixed(&canonical_json_bytes(manifest)?))
}

fn publish_context_from_manifest(
    manifest: &SharedCoordinationManifest,
) -> TrackedSnapshotPublishContext {
    TrackedSnapshotPublishContext {
        published_at: manifest.published_at,
        principal: ProtectedPrincipalIdentity {
            principal_authority_id: manifest.publisher.principal_authority_id.clone(),
            principal_id: manifest.publisher.principal_id.clone(),
            credential_id: manifest.publisher.credential_id.clone(),
        },
        work_context: Some(manifest.work_context.clone()),
        publish_summary: manifest.publish_summary.clone(),
    }
}

fn implicit_work_context() -> WorkContextSnapshot {
    WorkContextSnapshot {
        work_id: "work:shared_coordination_ref_publication".to_string(),
        kind: WorkContextKind::Undeclared,
        title: "Shared coordination ref publication".to_string(),
        summary: Some(
            "Fallback publish context for shared coordination ref authority when no explicit declared work is available."
                .to_string(),
        ),
        parent_work_id: None,
        coordination_task_id: None,
        plan_id: None,
        plan_title: None,
    }
}

fn collect_snapshot_file_map(
    stage_dir: &Path,
) -> Result<BTreeMap<String, SharedCoordinationManifestFile>> {
    let root = stage_snapshot_root(stage_dir);
    let mut files = BTreeMap::new();
    collect_snapshot_files_recursive(&root, &root, &mut files)?;
    Ok(files)
}

fn collect_snapshot_files_recursive(
    snapshot_root: &Path,
    current: &Path,
    files: &mut BTreeMap<String, SharedCoordinationManifestFile>,
) -> Result<()> {
    for entry in
        fs::read_dir(current).with_context(|| format!("failed to read {}", current.display()))?
    {
        let entry = entry?;
        let path = entry.path();
        if path.is_dir() {
            collect_snapshot_files_recursive(snapshot_root, &path, files)?;
            continue;
        }
        if path == snapshot_root.join("manifest.json")
            || path.extension().and_then(|ext| ext.to_str()) != Some("json")
        {
            continue;
        }
        let bytes =
            fs::read(&path).with_context(|| format!("failed to read {}", path.display()))?;
        let relative = path
            .strip_prefix(snapshot_root)
            .unwrap_or(path.as_path())
            .to_string_lossy()
            .replace('\\', "/");
        files.insert(
            relative.clone(),
            SharedCoordinationManifestFile {
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
        fs::write(path, bytes).with_context(|| format!("failed to write {}", path.display()))?;
    }
    Ok(())
}

fn decode_signature(value: &str) -> Result<ed25519_dalek::Signature> {
    let encoded = value.strip_prefix("base64:").ok_or_else(|| {
        anyhow!("shared coordination manifest signature must use `base64:` prefix")
    })?;
    let decoded = BASE64_STANDARD.decode(encoded).map_err(|error| {
        anyhow!("shared coordination manifest signature is not valid base64: {error}")
    })?;
    ed25519_dalek::Signature::try_from(decoded.as_slice()).map_err(|error| {
        anyhow!("shared coordination manifest signature has invalid Ed25519 bytes: {error}")
    })
}

fn is_shared_coordination_ref_integrity_error(error: &anyhow::Error) -> bool {
    let message = error.to_string();
    message.contains("failed to parse shared coordination manifest from git ref")
        || message.contains("shared coordination manifest signature")
        || message.contains("shared coordination manifest digest mismatch")
        || message.contains("shared coordination manifest is missing `")
        || message.contains("failed to parse shared coordination manifest")
}

#[cfg(test)]
mod tests {
    use std::cell::RefCell;
    use std::collections::{BTreeMap, BTreeSet};
    use std::fs;
    use std::path::{Path, PathBuf};
    use std::sync::atomic::{AtomicU64, Ordering};
    use std::time::{SystemTime, UNIX_EPOCH};

    use prism_coordination::{
        CoordinationPolicy, CoordinationSnapshot, CoordinationTask, Plan, PlanScheduling,
        RuntimeDescriptorCapability, TaskGitExecution, WorkClaim,
    };
    use prism_ir::{
        ClaimId, ClaimMode, ClaimStatus, CoordinationTaskId, CoordinationTaskStatus, EventActor,
        EventId, EventMeta, PlanExecutionOverlay, PlanGraph, PlanId, PlanKind, PlanScope,
        PlanStatus, SessionId, TaskId, WorkspaceRevision,
    };
    use prism_store::{CoordinationCheckpointStore, CoordinationStartupCheckpoint, MemoryStore};

    use super::{
        implicit_principal_identity, initialize_shared_coordination_ref_live_sync,
        load_shared_coordination_ref_state, poll_shared_coordination_ref_live_sync,
        shared_coordination_ref_diagnostics, shared_coordination_ref_exists,
        sync_live_runtime_descriptor, sync_shared_coordination_ref_state,
        SharedCoordinationRefLiveSync,
    };
    use crate::index_workspace_session;
    use crate::published_plans::load_hydrated_coordination_plan_state;
    use crate::tracked_snapshot::TrackedSnapshotPublishContext;
    use crate::util::current_timestamp;

    static NEXT_TEMP_REPO: AtomicU64 = AtomicU64::new(0);

    thread_local! {
        static TEMP_TEST_DIRS: RefCell<Vec<PathBuf>> = RefCell::new(Vec::new());
    }

    fn track_temp_dir(path: &Path) {
        TEMP_TEST_DIRS.with(|state| state.borrow_mut().push(path.to_path_buf()));
    }

    fn temp_git_repo() -> PathBuf {
        let nonce = NEXT_TEMP_REPO.fetch_add(1, Ordering::Relaxed);
        let unique = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        let root = std::env::temp_dir().join(format!("prism-shared-coord-{unique}-{nonce}"));
        let _ = fs::remove_dir_all(&root);
        fs::create_dir_all(&root).unwrap();
        track_temp_dir(&root);
        fs::create_dir_all(root.join(".prism")).unwrap();
        super::run_git(&root, &["init", "-b", "main"]).unwrap();
        fs::write(root.join("README.md"), "# test\n").unwrap();
        super::run_git(&root, &["add", "README.md"]).unwrap();
        super::run_git(&root, &["commit", "-m", "init"]).unwrap();
        root
    }

    fn temp_git_repo_with_origin() -> (PathBuf, PathBuf) {
        let remote = temp_git_repo().with_extension("remote.git");
        let _ = fs::remove_dir_all(&remote);
        fs::create_dir_all(&remote).unwrap();
        track_temp_dir(&remote);
        super::run_git(&remote, &["init", "--bare"]).unwrap();

        let root = temp_git_repo();
        super::run_git(
            &root,
            &[
                "remote",
                "add",
                super::shared_coordination_remote_name(),
                remote.to_string_lossy().as_ref(),
            ],
        )
        .unwrap();
        super::run_git(
            &root,
            &[
                "push",
                "-u",
                super::shared_coordination_remote_name(),
                "main",
            ],
        )
        .unwrap();
        (root, remote)
    }

    fn seed_workspace_project(root: &Path) {
        fs::create_dir_all(root.join("src")).unwrap();
        fs::write(
            root.join("Cargo.toml"),
            "[package]\nname = \"demo\"\nversion = \"0.1.0\"\n",
        )
        .unwrap();
        fs::write(root.join("src/lib.rs"), "pub fn alpha() {}\n").unwrap();
    }

    fn temp_git_worktree(repo_root: &Path) -> PathBuf {
        let nonce = NEXT_TEMP_REPO.fetch_add(1, Ordering::Relaxed);
        let unique = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        let root =
            std::env::temp_dir().join(format!("prism-shared-coord-worktree-{unique}-{nonce}"));
        let _ = fs::remove_dir_all(&root);
        track_temp_dir(&root);
        super::run_git(
            repo_root,
            &[
                "worktree",
                "add",
                "-b",
                &format!("task/shared-coordination-test-{nonce}"),
                root.to_string_lossy().as_ref(),
            ],
        )
        .unwrap();
        root
    }

    fn sample_publish_context() -> TrackedSnapshotPublishContext {
        TrackedSnapshotPublishContext {
            published_at: current_timestamp(),
            principal: implicit_principal_identity(None, None),
            work_context: Some(super::implicit_work_context()),
            publish_summary: None,
        }
    }

    fn tamper_shared_coordination_manifest_signature(root: &Path) {
        let ref_name = super::shared_coordination_ref_name(root);
        let contents = super::load_shared_coordination_ref_contents(root, &ref_name)
            .unwrap()
            .expect("shared coordination ref contents");
        let stage_dir = root.join(".prism").join("shared-coordination-tamper-stage");
        let _ = fs::remove_dir_all(&stage_dir);
        fs::create_dir_all(&stage_dir).unwrap();
        for (relative_path, bytes) in &contents.files {
            let path = stage_dir.join("coordination").join(relative_path);
            if let Some(parent) = path.parent() {
                fs::create_dir_all(parent).unwrap();
            }
            fs::write(&path, bytes).unwrap();
        }
        let manifest_path = stage_dir.join("coordination").join("manifest.json");
        let mut manifest: serde_json::Value =
            serde_json::from_slice(&fs::read(&manifest_path).unwrap()).unwrap();
        let signature = manifest["signature"]["value"]
            .as_str()
            .expect("manifest signature value")
            .to_string();
        manifest["signature"]["value"] = serde_json::Value::String(format!("{signature}tampered"));
        fs::write(
            &manifest_path,
            serde_json::to_vec_pretty(&manifest).unwrap(),
        )
        .unwrap();
        super::publish_stage_to_ref(root, &stage_dir, &ref_name).unwrap();
        let _ = fs::remove_dir_all(&stage_dir);
    }

    fn sample_snapshot_for(
        plan_id: &str,
        task_id: &str,
    ) -> (
        CoordinationSnapshot,
        PlanGraph,
        BTreeMap<String, Vec<PlanExecutionOverlay>>,
    ) {
        let plan_id = PlanId::new(plan_id.to_string());
        let task_id = CoordinationTaskId::new(task_id.to_string());
        let plan = Plan {
            id: plan_id.clone(),
            goal: "ship".to_string(),
            title: "ship".to_string(),
            status: PlanStatus::Active,
            policy: CoordinationPolicy::default(),
            scope: PlanScope::Repo,
            kind: PlanKind::TaskExecution,
            revision: 1,
            scheduling: PlanScheduling::default(),
            tags: Vec::new(),
            created_from: None,
            metadata: serde_json::Value::Null,
            authored_edges: Vec::new(),
            root_tasks: vec![task_id.clone()],
        };
        let task = CoordinationTask {
            id: task_id,
            plan: plan_id.clone(),
            kind: prism_ir::PlanNodeKind::Edit,
            title: "ship it".to_string(),
            summary: None,
            status: CoordinationTaskStatus::InProgress,
            published_task_status: None,
            assignee: None,
            pending_handoff_to: None,
            session: Some(SessionId::new("session:test".to_string())),
            lease_holder: None,
            lease_started_at: Some(10),
            lease_refreshed_at: Some(11),
            lease_stale_at: Some(12),
            lease_expires_at: Some(13),
            worktree_id: Some("worktree:test".to_string()),
            branch_ref: Some("refs/heads/task/shared".to_string()),
            anchors: Vec::new(),
            bindings: prism_ir::PlanBinding::default(),
            depends_on: Vec::new(),
            coordination_depends_on: Vec::new(),
            integrated_depends_on: Vec::new(),
            acceptance: Vec::new(),
            validation_refs: Vec::new(),
            is_abstract: false,
            base_revision: WorkspaceRevision::default(),
            priority: Some(1),
            tags: Vec::new(),
            metadata: serde_json::Value::Null,
            git_execution: TaskGitExecution::default(),
        };
        let snapshot = CoordinationSnapshot {
            plans: vec![plan.clone()],
            tasks: vec![task.clone()],
            claims: Vec::new(),
            artifacts: Vec::new(),
            reviews: Vec::new(),
            events: Vec::new(),
            next_plan: 1,
            next_task: 1,
            next_claim: 0,
            next_artifact: 0,
            next_review: 0,
        };
        let graph = prism_coordination::snapshot_plan_graphs(&snapshot)
            .into_iter()
            .next()
            .unwrap();
        let execution_map = BTreeMap::from([(
            plan.id.0.to_string(),
            prism_coordination::execution_overlays_from_tasks(&snapshot.tasks),
        )]);
        (snapshot, graph, execution_map)
    }

    #[test]
    fn shared_coordination_ref_round_trips_claims_and_git_execution_state() {
        let root = temp_git_repo();
        let plan_id = PlanId::new("plan:shared".to_string());
        let task_id = CoordinationTaskId::new("coord-task:shared".to_string());
        let claim_id = ClaimId::new("claim:shared".to_string());
        let plan = Plan {
            id: plan_id.clone(),
            goal: "ship".to_string(),
            title: "ship".to_string(),
            status: PlanStatus::Active,
            policy: CoordinationPolicy::default(),
            scope: PlanScope::Repo,
            kind: PlanKind::TaskExecution,
            revision: 1,
            scheduling: PlanScheduling::default(),
            tags: Vec::new(),
            created_from: None,
            metadata: serde_json::Value::Null,
            authored_edges: Vec::new(),
            root_tasks: vec![task_id.clone()],
        };
        let task = CoordinationTask {
            id: task_id.clone(),
            plan: plan_id.clone(),
            kind: prism_ir::PlanNodeKind::Edit,
            title: "ship it".to_string(),
            summary: None,
            status: CoordinationTaskStatus::InProgress,
            published_task_status: Some(CoordinationTaskStatus::Completed),
            assignee: None,
            pending_handoff_to: None,
            session: Some(SessionId::new("session:test".to_string())),
            lease_holder: None,
            lease_started_at: Some(10),
            lease_refreshed_at: Some(11),
            lease_stale_at: Some(12),
            lease_expires_at: Some(13),
            worktree_id: Some("worktree:test".to_string()),
            branch_ref: Some("refs/heads/task/shared".to_string()),
            anchors: Vec::new(),
            bindings: prism_ir::PlanBinding::default(),
            depends_on: Vec::new(),
            coordination_depends_on: Vec::new(),
            integrated_depends_on: Vec::new(),
            acceptance: Vec::new(),
            validation_refs: Vec::new(),
            is_abstract: false,
            base_revision: WorkspaceRevision::default(),
            priority: Some(1),
            tags: Vec::new(),
            metadata: serde_json::Value::Null,
            git_execution: TaskGitExecution {
                status: prism_ir::GitExecutionStatus::CoordinationPublished,
                pending_task_status: Some(CoordinationTaskStatus::Completed),
                source_ref: Some("refs/heads/task/shared".to_string()),
                target_ref: Some("origin/main".to_string()),
                publish_ref: Some("refs/heads/task/shared".to_string()),
                target_branch: Some("main".to_string()),
                integration_evidence: Some(prism_ir::GitIntegrationEvidence {
                    kind: prism_ir::GitIntegrationEvidenceKind::TrustedRecord,
                    target_commit: "deadbeef".to_string(),
                    review_artifact_ref: Some("artifact:review".to_string()),
                    record_ref: Some("coordination:landing".to_string()),
                }),
                last_preflight: None,
                last_publish: None,
                ..TaskGitExecution::default()
            },
        };
        let claim = WorkClaim {
            id: claim_id.clone(),
            holder: SessionId::new("session:test".to_string()),
            agent: None,
            lease_holder: None,
            worktree_id: Some("worktree:test".to_string()),
            branch_ref: Some("refs/heads/task/shared".to_string()),
            task: Some(task_id.clone()),
            anchors: Vec::new(),
            capability: prism_ir::Capability::Edit,
            mode: ClaimMode::SoftExclusive,
            since: 10,
            refreshed_at: Some(11),
            stale_at: Some(12),
            expires_at: 13,
            status: ClaimStatus::Active,
            base_revision: WorkspaceRevision::default(),
        };
        let snapshot = CoordinationSnapshot {
            plans: vec![plan.clone()],
            tasks: vec![task.clone()],
            claims: vec![claim.clone()],
            artifacts: Vec::new(),
            reviews: Vec::new(),
            events: Vec::new(),
            next_plan: 1,
            next_task: 1,
            next_claim: 1,
            next_artifact: 0,
            next_review: 0,
        };
        let graph = prism_coordination::snapshot_plan_graphs(&snapshot)
            .into_iter()
            .next()
            .unwrap();
        let execution_overlays = prism_coordination::execution_overlays_from_tasks(&snapshot.tasks);
        let execution_map = BTreeMap::from([(plan.id.0.to_string(), execution_overlays.clone())]);
        sync_shared_coordination_ref_state(
            &root,
            &snapshot,
            &[graph.clone()],
            &execution_map,
            Some(&sample_publish_context()),
        )
        .unwrap();
        assert!(shared_coordination_ref_exists(&root).unwrap());
        let loaded = load_shared_coordination_ref_state(&root)
            .unwrap()
            .expect("shared ref state should load");
        assert_eq!(loaded.snapshot.tasks, vec![task]);
        assert_eq!(loaded.snapshot.claims, vec![claim]);
        assert_eq!(loaded.snapshot.plans, vec![plan]);
        assert_eq!(loaded.plan_graphs, vec![graph]);
        assert_eq!(
            loaded
                .execution_overlays
                .get(plan_id.0.as_str())
                .cloned()
                .unwrap_or_default(),
            execution_overlays
        );
    }

    #[test]
    fn hydrated_plan_state_prefers_shared_coordination_ref_over_branch_snapshot() {
        let root = temp_git_repo();
        let plan_id = PlanId::new("plan:shared".to_string());
        let task_id = CoordinationTaskId::new("coord-task:shared".to_string());
        let plan = Plan {
            id: plan_id.clone(),
            goal: "ship".to_string(),
            title: "ship".to_string(),
            status: PlanStatus::Active,
            policy: CoordinationPolicy::default(),
            scope: PlanScope::Repo,
            kind: PlanKind::TaskExecution,
            revision: 1,
            scheduling: PlanScheduling::default(),
            tags: Vec::new(),
            created_from: None,
            metadata: serde_json::Value::Null,
            authored_edges: Vec::new(),
            root_tasks: vec![task_id.clone()],
        };
        let mut task = CoordinationTask {
            id: task_id.clone(),
            plan: plan_id.clone(),
            kind: prism_ir::PlanNodeKind::Edit,
            title: "ship it".to_string(),
            summary: None,
            status: CoordinationTaskStatus::InProgress,
            published_task_status: Some(CoordinationTaskStatus::Completed),
            assignee: None,
            pending_handoff_to: None,
            session: Some(SessionId::new("session:test".to_string())),
            lease_holder: None,
            lease_started_at: Some(10),
            lease_refreshed_at: Some(11),
            lease_stale_at: Some(12),
            lease_expires_at: Some(13),
            worktree_id: Some("worktree:test".to_string()),
            branch_ref: Some("refs/heads/task/shared".to_string()),
            anchors: Vec::new(),
            bindings: prism_ir::PlanBinding::default(),
            depends_on: Vec::new(),
            coordination_depends_on: Vec::new(),
            integrated_depends_on: Vec::new(),
            acceptance: Vec::new(),
            validation_refs: Vec::new(),
            is_abstract: false,
            base_revision: WorkspaceRevision::default(),
            priority: Some(1),
            tags: Vec::new(),
            metadata: serde_json::Value::Null,
            git_execution: TaskGitExecution {
                status: prism_ir::GitExecutionStatus::CoordinationPublished,
                pending_task_status: Some(CoordinationTaskStatus::Completed),
                source_ref: Some("refs/heads/task/shared".to_string()),
                target_ref: Some("origin/main".to_string()),
                publish_ref: Some("refs/heads/task/shared".to_string()),
                target_branch: Some("main".to_string()),
                last_preflight: None,
                last_publish: None,
                ..TaskGitExecution::default()
            },
        };
        let shared_snapshot = CoordinationSnapshot {
            plans: vec![plan.clone()],
            tasks: vec![task.clone()],
            claims: Vec::new(),
            artifacts: Vec::new(),
            reviews: Vec::new(),
            events: Vec::new(),
            next_plan: 1,
            next_task: 1,
            next_claim: 0,
            next_artifact: 0,
            next_review: 0,
        };
        let graph = prism_coordination::snapshot_plan_graphs(&shared_snapshot)
            .into_iter()
            .next()
            .unwrap();
        let execution_overlays =
            prism_coordination::execution_overlays_from_tasks(&shared_snapshot.tasks);
        let execution_map = BTreeMap::from([(plan.id.0.to_string(), execution_overlays.clone())]);
        sync_shared_coordination_ref_state(
            &root,
            &shared_snapshot,
            &[graph],
            &execution_map,
            Some(&sample_publish_context()),
        )
        .unwrap();

        task.git_execution = TaskGitExecution::default();
        task.session = None;
        task.worktree_id = None;
        task.branch_ref = None;
        let branch_snapshot = CoordinationSnapshot {
            tasks: vec![task.clone()],
            ..shared_snapshot.clone()
        };
        let loaded = load_hydrated_coordination_plan_state(&root, Some(branch_snapshot))
            .unwrap()
            .expect("hydrated state");
        let loaded_task = loaded
            .snapshot
            .tasks
            .into_iter()
            .find(|candidate| candidate.id == task_id)
            .expect("shared task should be present");
        assert_eq!(
            loaded_task.git_execution.status,
            prism_ir::GitExecutionStatus::CoordinationPublished
        );
        assert_eq!(
            loaded_task.branch_ref.as_deref(),
            Some("refs/heads/task/shared")
        );
    }

    #[test]
    fn startup_loader_prefers_materialized_checkpoint_over_inline_shared_ref_hydration() {
        let root = temp_git_repo();
        let ref_name = super::shared_coordination_ref_name(&root);
        let head = super::run_git(&root, &["rev-parse", "HEAD"]).unwrap();
        super::run_git(&root, &["update-ref", &ref_name, &head]).unwrap();

        let (snapshot, graph, execution_overlays) =
            sample_snapshot_for("plan:checkpoint", "coord-task:checkpoint");
        let authority = super::shared_coordination_startup_authority(&root)
            .unwrap()
            .expect("shared coordination authority");
        let mut store = MemoryStore::default();
        store
            .save_coordination_startup_checkpoint(&CoordinationStartupCheckpoint {
                version: CoordinationStartupCheckpoint::VERSION,
                materialized_at: current_timestamp(),
                coordination_revision: 0,
                authority,
                snapshot: snapshot.clone(),
                plan_graphs: vec![graph.clone()],
                execution_overlays: execution_overlays.clone(),
            })
            .unwrap();

        let loaded =
            crate::protected_state::runtime_sync::load_repo_protected_plan_state(&root, &mut store)
                .unwrap()
                .expect("startup checkpoint should hydrate plan state");

        assert_eq!(loaded.snapshot, snapshot);
        assert_eq!(loaded.plan_graphs, vec![graph]);
        assert_eq!(loaded.execution_overlays, execution_overlays);
    }

    #[test]
    fn shared_coordination_ref_pushes_to_origin_and_reloads_from_remote() {
        let (root, remote) = temp_git_repo_with_origin();
        let plan_id = PlanId::new("plan:shared".to_string());
        let task_id = CoordinationTaskId::new("coord-task:shared".to_string());
        let plan = Plan {
            id: plan_id.clone(),
            goal: "ship".to_string(),
            title: "ship".to_string(),
            status: PlanStatus::Active,
            policy: CoordinationPolicy::default(),
            scope: PlanScope::Repo,
            kind: PlanKind::TaskExecution,
            revision: 1,
            scheduling: PlanScheduling::default(),
            tags: Vec::new(),
            created_from: None,
            metadata: serde_json::Value::Null,
            authored_edges: Vec::new(),
            root_tasks: vec![task_id.clone()],
        };
        let task = CoordinationTask {
            id: task_id.clone(),
            plan: plan_id.clone(),
            kind: prism_ir::PlanNodeKind::Edit,
            title: "ship it".to_string(),
            summary: None,
            status: CoordinationTaskStatus::InProgress,
            published_task_status: Some(CoordinationTaskStatus::Completed),
            assignee: None,
            pending_handoff_to: None,
            session: Some(SessionId::new("session:test".to_string())),
            lease_holder: None,
            lease_started_at: Some(10),
            lease_refreshed_at: Some(11),
            lease_stale_at: Some(12),
            lease_expires_at: Some(13),
            worktree_id: Some("worktree:test".to_string()),
            branch_ref: Some("refs/heads/task/shared".to_string()),
            anchors: Vec::new(),
            bindings: prism_ir::PlanBinding::default(),
            depends_on: Vec::new(),
            coordination_depends_on: Vec::new(),
            integrated_depends_on: Vec::new(),
            acceptance: Vec::new(),
            validation_refs: Vec::new(),
            is_abstract: false,
            base_revision: WorkspaceRevision::default(),
            priority: Some(1),
            tags: Vec::new(),
            metadata: serde_json::Value::Null,
            git_execution: TaskGitExecution {
                status: prism_ir::GitExecutionStatus::CoordinationPublished,
                pending_task_status: Some(CoordinationTaskStatus::Completed),
                source_ref: Some("refs/heads/task/shared".to_string()),
                target_ref: Some("origin/main".to_string()),
                publish_ref: Some("refs/heads/task/shared".to_string()),
                target_branch: Some("main".to_string()),
                last_preflight: None,
                last_publish: None,
                ..TaskGitExecution::default()
            },
        };
        let snapshot = CoordinationSnapshot {
            plans: vec![plan.clone()],
            tasks: vec![task.clone()],
            claims: Vec::new(),
            artifacts: Vec::new(),
            reviews: Vec::new(),
            events: Vec::new(),
            next_plan: 1,
            next_task: 1,
            next_claim: 0,
            next_artifact: 0,
            next_review: 0,
        };
        let graph = prism_coordination::snapshot_plan_graphs(&snapshot)
            .into_iter()
            .next()
            .unwrap();
        let execution_overlays = prism_coordination::execution_overlays_from_tasks(&snapshot.tasks);
        let execution_map = BTreeMap::from([(plan.id.0.to_string(), execution_overlays.clone())]);
        sync_shared_coordination_ref_state(
            &root,
            &snapshot,
            &[graph],
            &execution_map,
            Some(&sample_publish_context()),
        )
        .unwrap();

        let ref_name = super::shared_coordination_ref_name(&root);
        let remote_head =
            super::run_git(&remote, &["rev-parse", "--verify", ref_name.as_str()]).unwrap();
        assert!(!remote_head.trim().is_empty());

        super::run_git(&root, &["update-ref", "-d", ref_name.as_str()]).unwrap();
        assert!(
            load_shared_coordination_ref_state(&root).unwrap().is_none(),
            "request-path shared ref loads should stay local-only until live sync refreshes the ref"
        );
        let SharedCoordinationRefLiveSync::Changed(loaded) =
            poll_shared_coordination_ref_live_sync(&root).unwrap()
        else {
            panic!("shared ref live sync should reload state from remote");
        };
        assert_eq!(loaded.snapshot.tasks, vec![task]);
        assert_eq!(loaded.snapshot.plans, vec![plan]);
    }

    #[test]
    fn invalid_shared_coordination_manifest_is_ignored_and_repaired_on_next_publish() {
        let (root, _remote) = temp_git_repo_with_origin();
        let (snapshot, graph, execution_map) = sample_snapshot_for(
            "plan:shared-invalid-manifest",
            "coord-task:shared-invalid-manifest",
        );
        sync_shared_coordination_ref_state(
            &root,
            &snapshot,
            std::slice::from_ref(&graph),
            &execution_map,
            Some(&sample_publish_context()),
        )
        .unwrap();

        tamper_shared_coordination_manifest_signature(&root);
        assert!(
            load_shared_coordination_ref_state(&root).unwrap().is_none(),
            "invalid manifests should be ignored instead of aborting shared-ref hydration"
        );

        sync_shared_coordination_ref_state(
            &root,
            &snapshot,
            std::slice::from_ref(&graph),
            &execution_map,
            Some(&sample_publish_context()),
        )
        .unwrap();

        let repaired = load_shared_coordination_ref_state(&root)
            .unwrap()
            .expect("shared ref state should be repaired by the next publish");
        assert_eq!(repaired.snapshot.plans, snapshot.plans);
        assert_eq!(repaired.snapshot.tasks, snapshot.tasks);
    }

    #[test]
    fn shared_coordination_ref_retries_stale_head_and_merges_disjoint_changes() {
        let (root_a, _remote) = temp_git_repo_with_origin();
        let root_b = temp_git_worktree(&root_a);
        let plan_id = PlanId::new("plan:shared".to_string());
        let task_id = CoordinationTaskId::new("coord-task:shared".to_string());
        let plan = Plan {
            id: plan_id.clone(),
            goal: "ship".to_string(),
            title: "ship".to_string(),
            status: PlanStatus::Active,
            policy: CoordinationPolicy::default(),
            scope: PlanScope::Repo,
            kind: PlanKind::TaskExecution,
            revision: 1,
            scheduling: PlanScheduling::default(),
            tags: Vec::new(),
            created_from: None,
            metadata: serde_json::Value::Null,
            authored_edges: Vec::new(),
            root_tasks: vec![task_id.clone()],
        };
        let task = CoordinationTask {
            id: task_id.clone(),
            plan: plan_id.clone(),
            kind: prism_ir::PlanNodeKind::Edit,
            title: "ship it".to_string(),
            summary: None,
            status: CoordinationTaskStatus::InProgress,
            published_task_status: None,
            assignee: None,
            pending_handoff_to: None,
            session: Some(SessionId::new("session:test".to_string())),
            lease_holder: None,
            lease_started_at: Some(10),
            lease_refreshed_at: Some(11),
            lease_stale_at: Some(12),
            lease_expires_at: Some(13),
            worktree_id: Some("worktree:test".to_string()),
            branch_ref: Some("refs/heads/task/shared".to_string()),
            anchors: Vec::new(),
            bindings: prism_ir::PlanBinding::default(),
            depends_on: Vec::new(),
            coordination_depends_on: Vec::new(),
            integrated_depends_on: Vec::new(),
            acceptance: Vec::new(),
            validation_refs: Vec::new(),
            is_abstract: false,
            base_revision: WorkspaceRevision::default(),
            priority: Some(1),
            tags: Vec::new(),
            metadata: serde_json::Value::Null,
            git_execution: TaskGitExecution::default(),
        };
        let base_snapshot = CoordinationSnapshot {
            plans: vec![plan.clone()],
            tasks: vec![task.clone()],
            claims: Vec::new(),
            artifacts: Vec::new(),
            reviews: Vec::new(),
            events: Vec::new(),
            next_plan: 1,
            next_task: 1,
            next_claim: 0,
            next_artifact: 0,
            next_review: 0,
        };
        let base_graph = prism_coordination::snapshot_plan_graphs(&base_snapshot)
            .into_iter()
            .next()
            .unwrap();
        let base_execution_map = BTreeMap::from([(
            plan.id.0.to_string(),
            prism_coordination::execution_overlays_from_tasks(&base_snapshot.tasks),
        )]);
        sync_shared_coordination_ref_state(
            &root_a,
            &base_snapshot,
            &[base_graph],
            &base_execution_map,
            Some(&sample_publish_context()),
        )
        .unwrap();

        let ref_name = super::shared_coordination_ref_name(&root_b);
        let expected_head = super::refresh_local_shared_coordination_ref(
            &root_b,
            super::shared_coordination_remote_name(),
            &ref_name,
        )
        .unwrap();
        let baseline_state =
            super::load_shared_coordination_ref_state_from_current_ref(&root_b, &ref_name).unwrap();

        let claim_a = WorkClaim {
            id: ClaimId::new("claim:a".to_string()),
            holder: SessionId::new("session:a".to_string()),
            agent: None,
            lease_holder: None,
            worktree_id: Some("worktree:a".to_string()),
            branch_ref: Some("refs/heads/task/a".to_string()),
            task: Some(task_id.clone()),
            anchors: Vec::new(),
            capability: prism_ir::Capability::Edit,
            mode: ClaimMode::SoftExclusive,
            since: 20,
            refreshed_at: Some(21),
            stale_at: Some(22),
            expires_at: 23,
            status: ClaimStatus::Active,
            base_revision: WorkspaceRevision::default(),
        };
        let snapshot_a = CoordinationSnapshot {
            claims: vec![claim_a.clone()],
            next_claim: 1,
            ..base_snapshot.clone()
        };
        let graph_a = prism_coordination::snapshot_plan_graphs(&snapshot_a)
            .into_iter()
            .next()
            .unwrap();
        let execution_map_a = BTreeMap::from([(
            plan.id.0.to_string(),
            prism_coordination::execution_overlays_from_tasks(&snapshot_a.tasks),
        )]);
        sync_shared_coordination_ref_state(
            &root_a,
            &snapshot_a,
            &[graph_a],
            &execution_map_a,
            Some(&sample_publish_context()),
        )
        .unwrap();

        let claim_b = WorkClaim {
            id: ClaimId::new("claim:b".to_string()),
            holder: SessionId::new("session:b".to_string()),
            agent: None,
            lease_holder: None,
            worktree_id: Some("worktree:b".to_string()),
            branch_ref: Some("refs/heads/task/b".to_string()),
            task: Some(task_id.clone()),
            anchors: Vec::new(),
            capability: prism_ir::Capability::Edit,
            mode: ClaimMode::SoftExclusive,
            since: 30,
            refreshed_at: Some(31),
            stale_at: Some(32),
            expires_at: 33,
            status: ClaimStatus::Active,
            base_revision: WorkspaceRevision::default(),
        };
        let snapshot_b = CoordinationSnapshot {
            claims: vec![claim_b.clone()],
            next_claim: 1,
            ..base_snapshot.clone()
        };
        let graph_b = prism_coordination::snapshot_plan_graphs(&snapshot_b)
            .into_iter()
            .next()
            .unwrap();
        let execution_map_b = BTreeMap::from([(
            plan.id.0.to_string(),
            prism_coordination::execution_overlays_from_tasks(&snapshot_b.tasks),
        )]);
        let paths_b = crate::PrismPaths::for_workspace_root(&root_b).unwrap();
        let stage_parent = super::stage_root(&paths_b);
        fs::create_dir_all(&stage_parent).unwrap();
        let stage_dir = stage_parent.join("retry-stage");
        let _ = fs::remove_dir_all(&stage_dir);
        fs::create_dir_all(&stage_dir).unwrap();
        super::sync_shared_coordination_ref_state_inner(
            &root_b,
            &paths_b,
            &stage_dir,
            &snapshot_b,
            &[graph_b],
            &execution_map_b,
            Some(&sample_publish_context()),
            &ref_name,
            expected_head.as_deref(),
            baseline_state.as_ref(),
        )
        .unwrap();

        let loaded = load_shared_coordination_ref_state(&root_a)
            .unwrap()
            .expect("shared ref state should load");
        let claim_ids = loaded
            .snapshot
            .claims
            .iter()
            .map(|claim| claim.id.0.as_str())
            .collect::<BTreeSet<_>>();
        assert!(claim_ids.contains("claim:a"), "claim ids: {claim_ids:?}");
        assert!(claim_ids.contains("claim:b"), "claim ids: {claim_ids:?}");

        let diagnostics = shared_coordination_ref_diagnostics(&root_a)
            .unwrap()
            .expect("shared coordination diagnostics should exist");
        assert_eq!(diagnostics.last_successful_publish_retry_count, 1);
        assert_eq!(
            diagnostics.publish_retry_budget,
            super::SHARED_COORDINATION_PUSH_MAX_RETRIES as u32
        );
    }

    #[test]
    fn shared_coordination_publish_patch_only_stages_changed_task_payload() {
        let root = temp_git_repo();
        let plan_id = PlanId::new("plan:patch".to_string());
        let task_a_id = CoordinationTaskId::new("coord-task:alpha".to_string());
        let task_b_id = CoordinationTaskId::new("coord-task:beta".to_string());
        let plan = Plan {
            id: plan_id.clone(),
            goal: "ship".to_string(),
            title: "ship".to_string(),
            status: PlanStatus::Active,
            policy: CoordinationPolicy::default(),
            scope: PlanScope::Repo,
            kind: PlanKind::TaskExecution,
            revision: 1,
            scheduling: PlanScheduling::default(),
            tags: Vec::new(),
            created_from: None,
            metadata: serde_json::Value::Null,
            authored_edges: Vec::new(),
            root_tasks: vec![task_a_id.clone(), task_b_id.clone()],
        };
        let task_a = CoordinationTask {
            id: task_a_id.clone(),
            plan: plan_id.clone(),
            kind: prism_ir::PlanNodeKind::Edit,
            title: "alpha".to_string(),
            summary: None,
            status: CoordinationTaskStatus::Ready,
            published_task_status: None,
            assignee: None,
            pending_handoff_to: None,
            session: None,
            lease_holder: None,
            lease_started_at: None,
            lease_refreshed_at: None,
            lease_stale_at: None,
            lease_expires_at: None,
            worktree_id: None,
            branch_ref: None,
            anchors: Vec::new(),
            bindings: prism_ir::PlanBinding::default(),
            depends_on: Vec::new(),
            coordination_depends_on: Vec::new(),
            integrated_depends_on: Vec::new(),
            acceptance: Vec::new(),
            validation_refs: Vec::new(),
            is_abstract: false,
            base_revision: WorkspaceRevision::default(),
            priority: Some(1),
            tags: Vec::new(),
            metadata: serde_json::Value::Null,
            git_execution: TaskGitExecution::default(),
        };
        let task_b = CoordinationTask {
            id: task_b_id.clone(),
            title: "beta".to_string(),
            ..task_a.clone()
        };
        let snapshot = CoordinationSnapshot {
            plans: vec![plan.clone()],
            tasks: vec![task_a.clone(), task_b.clone()],
            claims: Vec::new(),
            artifacts: Vec::new(),
            reviews: Vec::new(),
            events: Vec::new(),
            next_plan: 1,
            next_task: 2,
            next_claim: 0,
            next_artifact: 0,
            next_review: 0,
        };
        let graph = prism_coordination::snapshot_plan_graphs(&snapshot)
            .into_iter()
            .next()
            .unwrap();
        let execution_map = BTreeMap::from([(
            plan.id.0.to_string(),
            prism_coordination::execution_overlays_from_tasks(&snapshot.tasks),
        )]);
        sync_shared_coordination_ref_state(
            &root,
            &snapshot,
            &[graph],
            &execution_map,
            Some(&sample_publish_context()),
        )
        .unwrap();

        let ref_name = super::shared_coordination_ref_name(&root);
        let baseline_state =
            super::load_shared_coordination_ref_state_from_current_ref(&root, &ref_name)
                .unwrap()
                .unwrap();
        let previous_manifest =
            super::load_shared_coordination_manifest_from_ref(&root, &ref_name).unwrap();

        let mut changed_task_b = task_b.clone();
        changed_task_b.summary = Some("narrow diff".to_string());
        let changed_snapshot = CoordinationSnapshot {
            tasks: vec![task_a.clone(), changed_task_b.clone()],
            ..snapshot.clone()
        };
        let changed_graph = prism_coordination::snapshot_plan_graphs(&changed_snapshot)
            .into_iter()
            .next()
            .unwrap();
        let changed_execution_map = BTreeMap::from([(
            plan.id.0.to_string(),
            prism_coordination::execution_overlays_from_tasks(&changed_snapshot.tasks),
        )]);
        let changed_state = super::SharedCoordinationRefState {
            snapshot: changed_snapshot.clone(),
            plan_graphs: vec![changed_graph.clone()],
            execution_overlays: changed_execution_map.clone(),
            runtime_descriptors: baseline_state.runtime_descriptors.clone(),
        };
        let stage_dir = root.join(".prism").join("publish-patch-test");
        let _ = fs::remove_dir_all(&stage_dir);
        fs::create_dir_all(&stage_dir).unwrap();
        let patch = super::build_shared_coordination_publish_patch(
            &stage_dir,
            previous_manifest.as_ref(),
            Some(&baseline_state),
            &changed_state,
        )
        .unwrap();

        let expected_task_path = format!(
            "coordination/{}",
            super::task_snapshot_relative_path(&changed_task_b.id.0)
        );
        let expected_plan_path = format!(
            "coordination/{}",
            super::plan_snapshot_relative_path(&plan.id.0)
        );
        assert_eq!(
            patch.upserts,
            BTreeSet::from([expected_plan_path, expected_task_path]),
            "summary-only task edits should touch the task payload and its containing plan record, but not unrelated indexes"
        );
        assert!(patch.deletes.is_empty());

        sync_shared_coordination_ref_state(
            &root,
            &changed_snapshot,
            &[changed_graph],
            &changed_execution_map,
            Some(&sample_publish_context()),
        )
        .unwrap();

        let loaded = load_shared_coordination_ref_state(&root)
            .unwrap()
            .expect("shared ref state should load");
        assert_eq!(loaded.snapshot.tasks.len(), 2);
        assert_eq!(
            loaded
                .snapshot
                .tasks
                .iter()
                .find(|task| task.id == changed_task_b.id)
                .and_then(|task| task.summary.as_deref()),
            Some("narrow diff")
        );
    }

    #[test]
    fn shared_coordination_ref_live_sync_suppresses_self_write_and_imports_remote_change() {
        let (root_a, _remote) = temp_git_repo_with_origin();
        seed_workspace_project(&root_a);
        let root_b = temp_git_worktree(&root_a);

        let session = index_workspace_session(&root_a).unwrap();
        initialize_shared_coordination_ref_live_sync(&root_a).unwrap();

        let plan_id = PlanId::new("plan:shared-live-sync".to_string());
        let task_id = CoordinationTaskId::new("coord-task:shared-live-sync".to_string());
        let plan = Plan {
            id: plan_id.clone(),
            goal: "ship".to_string(),
            title: "ship".to_string(),
            status: PlanStatus::Active,
            policy: CoordinationPolicy::default(),
            scope: PlanScope::Repo,
            kind: PlanKind::TaskExecution,
            revision: 1,
            scheduling: PlanScheduling::default(),
            tags: Vec::new(),
            created_from: None,
            metadata: serde_json::Value::Null,
            authored_edges: Vec::new(),
            root_tasks: vec![task_id.clone()],
        };
        let task = CoordinationTask {
            id: task_id.clone(),
            plan: plan_id.clone(),
            kind: prism_ir::PlanNodeKind::Edit,
            title: "ship it".to_string(),
            summary: None,
            status: CoordinationTaskStatus::InProgress,
            published_task_status: None,
            assignee: None,
            pending_handoff_to: None,
            session: Some(SessionId::new("session:test".to_string())),
            lease_holder: None,
            lease_started_at: Some(10),
            lease_refreshed_at: Some(11),
            lease_stale_at: Some(12),
            lease_expires_at: Some(13),
            worktree_id: Some("worktree:test".to_string()),
            branch_ref: Some("refs/heads/task/shared".to_string()),
            anchors: Vec::new(),
            bindings: prism_ir::PlanBinding::default(),
            depends_on: Vec::new(),
            coordination_depends_on: Vec::new(),
            integrated_depends_on: Vec::new(),
            acceptance: Vec::new(),
            validation_refs: Vec::new(),
            is_abstract: false,
            base_revision: WorkspaceRevision::default(),
            priority: Some(1),
            tags: Vec::new(),
            metadata: serde_json::Value::Null,
            git_execution: TaskGitExecution::default(),
        };
        let snapshot = CoordinationSnapshot {
            plans: vec![plan.clone()],
            tasks: vec![task.clone()],
            claims: Vec::new(),
            artifacts: Vec::new(),
            reviews: Vec::new(),
            events: Vec::new(),
            next_plan: 1,
            next_task: 1,
            next_claim: 0,
            next_artifact: 0,
            next_review: 0,
        };
        let graph = prism_coordination::snapshot_plan_graphs(&snapshot)
            .into_iter()
            .next()
            .unwrap();
        let execution_map = BTreeMap::from([(
            plan.id.0.to_string(),
            prism_coordination::execution_overlays_from_tasks(&snapshot.tasks),
        )]);

        sync_shared_coordination_ref_state(
            &root_a,
            &snapshot,
            &[graph],
            &execution_map,
            Some(&sample_publish_context()),
        )
        .unwrap();
        assert!(matches!(
            poll_shared_coordination_ref_live_sync(&root_a).unwrap(),
            SharedCoordinationRefLiveSync::Unchanged
        ));

        let before_revision = session
            .coordination_runtime_revision
            .load(Ordering::Relaxed);
        crate::watch::sync_shared_coordination_ref_watch_update(
            &root_a,
            &session.published_generation,
            &session.runtime_state,
            &session.store,
            &session.cold_query_store,
            session.shared_runtime_store.as_ref(),
            &session.refresh_lock,
            &session.loaded_workspace_revision,
            &session.coordination_runtime_revision,
            session.coordination_enabled,
        )
        .unwrap();
        assert_eq!(
            session
                .coordination_runtime_revision
                .load(Ordering::Relaxed),
            before_revision
        );

        let mut changed_task = task.clone();
        changed_task.status = CoordinationTaskStatus::Completed;
        let changed_snapshot = CoordinationSnapshot {
            plans: vec![plan.clone()],
            tasks: vec![changed_task.clone()],
            claims: vec![WorkClaim {
                id: ClaimId::new("claim:shared-live-sync".to_string()),
                holder: SessionId::new("session:remote".to_string()),
                agent: None,
                lease_holder: None,
                worktree_id: Some("worktree:remote".to_string()),
                branch_ref: Some("refs/heads/task/remote".to_string()),
                task: Some(task_id.clone()),
                anchors: Vec::new(),
                capability: prism_ir::Capability::Edit,
                mode: ClaimMode::SoftExclusive,
                since: 20,
                refreshed_at: Some(21),
                stale_at: Some(22),
                expires_at: 23,
                status: ClaimStatus::Active,
                base_revision: WorkspaceRevision::default(),
            }],
            artifacts: Vec::new(),
            reviews: Vec::new(),
            events: Vec::new(),
            next_plan: 1,
            next_task: 1,
            next_claim: 1,
            next_artifact: 0,
            next_review: 0,
        };
        let changed_graph = prism_coordination::snapshot_plan_graphs(&changed_snapshot)
            .into_iter()
            .next()
            .unwrap();
        let changed_execution_map = BTreeMap::from([(
            plan.id.0.to_string(),
            prism_coordination::execution_overlays_from_tasks(&changed_snapshot.tasks),
        )]);
        sync_shared_coordination_ref_state(
            &root_b,
            &changed_snapshot,
            &[changed_graph],
            &changed_execution_map,
            Some(&sample_publish_context()),
        )
        .unwrap();

        crate::watch::sync_shared_coordination_ref_watch_update(
            &root_a,
            &session.published_generation,
            &session.runtime_state,
            &session.store,
            &session.cold_query_store,
            session.shared_runtime_store.as_ref(),
            &session.refresh_lock,
            &session.loaded_workspace_revision,
            &session.coordination_runtime_revision,
            session.coordination_enabled,
        )
        .unwrap();
        assert!(session
            .prism()
            .coordination_snapshot()
            .claims
            .iter()
            .any(|claim| claim.id.0 == "claim:shared-live-sync"));
        assert_eq!(
            session.prism().coordination_snapshot().tasks[0].status,
            CoordinationTaskStatus::Completed
        );
        assert!(matches!(
            poll_shared_coordination_ref_live_sync(&root_a).unwrap(),
            SharedCoordinationRefLiveSync::Unchanged
        ));
    }

    #[test]
    fn early_task_heartbeat_does_not_advance_shared_coordination_ref_head() {
        let (root, _remote) = temp_git_repo_with_origin();
        seed_workspace_project(&root);
        let session = index_workspace_session(&root).unwrap();
        let (plan_id, task_id) = session
            .mutate_coordination(|prism| {
                let plan_id = prism.create_native_plan(
                    EventMeta {
                        id: EventId::new("coordination:lease-noop-plan"),
                        ts: 1,
                        actor: EventActor::Agent,
                        correlation: Some(TaskId::new("task:lease-noop")),
                        causation: None,
                        execution_context: None,
                    },
                    "Exercise early heartbeat suppression".into(),
                    "Exercise early heartbeat suppression".into(),
                    None,
                    Some(Default::default()),
                )?;
                let task = prism.create_native_task(
                    EventMeta {
                        id: EventId::new("coordination:lease-noop-task"),
                        ts: 2,
                        actor: EventActor::Agent,
                        correlation: Some(TaskId::new("task:lease-noop")),
                        causation: None,
                        execution_context: None,
                    },
                    prism_coordination::TaskCreateInput {
                        plan_id: plan_id.clone(),
                        title: "Keep early heartbeats local".into(),
                        status: Some(CoordinationTaskStatus::Ready),
                        assignee: None,
                        session: Some(SessionId::new("session:lease-noop-owner")),
                        worktree_id: None,
                        branch_ref: None,
                        anchors: Vec::new(),
                        depends_on: Vec::new(),
                        coordination_depends_on: Vec::new(),
                        integrated_depends_on: Vec::new(),
                        acceptance: Vec::new(),
                        base_revision: prism.workspace_revision(),
                    },
                )?;
                Ok::<_, anyhow::Error>((plan_id, task.id))
            })
            .unwrap();
        let ref_name = super::shared_coordination_ref_name(&root);
        let head_before = super::run_git(&root, &["rev-parse", &ref_name]).unwrap();

        let task = session
            .mutate_coordination(|prism| {
                prism.heartbeat_native_task(
                    EventMeta {
                        id: EventId::new("coordination:lease-noop-heartbeat"),
                        ts: 30,
                        actor: EventActor::Agent,
                        correlation: Some(TaskId::new("task:lease-noop")),
                        causation: None,
                        execution_context: None,
                    },
                    &task_id,
                    "explicit",
                )
            })
            .unwrap();

        let head_after = super::run_git(&root, &["rev-parse", &ref_name]).unwrap();
        assert_eq!(head_after, head_before);
        assert_eq!(task.plan, plan_id);
        assert_eq!(task.lease_started_at, Some(2));
        assert_eq!(task.lease_refreshed_at, Some(2));
        let loaded = load_shared_coordination_ref_state(&root)
            .unwrap()
            .expect("shared ref state should load");
        let loaded_task = loaded
            .snapshot
            .tasks
            .into_iter()
            .find(|candidate| candidate.id == task_id)
            .expect("shared ref should keep the task");
        assert_eq!(loaded_task.lease_started_at, Some(2));
        assert_eq!(loaded_task.lease_refreshed_at, Some(2));
    }

    #[test]
    fn due_task_heartbeat_refreshes_shared_coordination_ref_lease_state() {
        let (root, _remote) = temp_git_repo_with_origin();
        seed_workspace_project(&root);
        let session = index_workspace_session(&root).unwrap();
        let task_id = session
            .mutate_coordination(|prism| {
                let plan_id = prism.create_native_plan(
                    EventMeta {
                        id: EventId::new("coordination:lease-refresh-plan"),
                        ts: 1,
                        actor: EventActor::Agent,
                        correlation: Some(TaskId::new("task:lease-refresh")),
                        causation: None,
                        execution_context: None,
                    },
                    "Exercise due heartbeat publication".into(),
                    "Exercise due heartbeat publication".into(),
                    None,
                    Some(Default::default()),
                )?;
                let task = prism.create_native_task(
                    EventMeta {
                        id: EventId::new("coordination:lease-refresh-task"),
                        ts: 2,
                        actor: EventActor::Agent,
                        correlation: Some(TaskId::new("task:lease-refresh")),
                        causation: None,
                        execution_context: None,
                    },
                    prism_coordination::TaskCreateInput {
                        plan_id,
                        title: "Refresh the authoritative lease when due".into(),
                        status: Some(CoordinationTaskStatus::Ready),
                        assignee: None,
                        session: Some(SessionId::new("session:lease-refresh-owner")),
                        worktree_id: None,
                        branch_ref: None,
                        anchors: Vec::new(),
                        depends_on: Vec::new(),
                        coordination_depends_on: Vec::new(),
                        integrated_depends_on: Vec::new(),
                        acceptance: Vec::new(),
                        base_revision: prism.workspace_revision(),
                    },
                )?;
                Ok::<_, anyhow::Error>(task.id)
            })
            .unwrap();
        let ref_name = super::shared_coordination_ref_name(&root);
        let head_before = super::run_git(&root, &["rev-parse", &ref_name]).unwrap();

        let task = session
            .mutate_coordination(|prism| {
                prism.heartbeat_native_task(
                    EventMeta {
                        id: EventId::new("coordination:lease-refresh-heartbeat"),
                        ts: 1700,
                        actor: EventActor::Agent,
                        correlation: Some(TaskId::new("task:lease-refresh")),
                        causation: None,
                        execution_context: None,
                    },
                    &task_id,
                    "explicit",
                )
            })
            .unwrap();

        let head_after = super::run_git(&root, &["rev-parse", &ref_name]).unwrap();
        assert_ne!(head_after, head_before);
        assert_eq!(task.lease_started_at, Some(2));
        assert_eq!(task.lease_refreshed_at, Some(1700));
        let loaded = load_shared_coordination_ref_state(&root)
            .unwrap()
            .expect("shared ref state should load");
        let loaded_task = loaded
            .snapshot
            .tasks
            .into_iter()
            .find(|candidate| candidate.id == task_id)
            .expect("shared ref should keep the task");
        assert_eq!(loaded_task.lease_started_at, Some(2));
        assert_eq!(loaded_task.lease_refreshed_at, Some(1700));
        assert!(loaded_task.lease_stale_at.is_some_and(|value| value > 1700));
        assert!(loaded_task
            .lease_expires_at
            .is_some_and(|value| value > 1700));
    }

    #[test]
    fn shared_coordination_ref_compacts_history_after_threshold() {
        let (root, _remote) = temp_git_repo_with_origin();
        let (snapshot, graph, execution_map) =
            sample_snapshot_for("plan:shared-compaction", "coord-task:shared-compaction");
        let publish = sample_publish_context();

        for _ in 0..(super::SHARED_COORDINATION_HISTORY_MAX_COMMITS + 1) {
            sync_shared_coordination_ref_state(
                &root,
                &snapshot,
                std::slice::from_ref(&graph),
                &execution_map,
                Some(&publish),
            )
            .unwrap();
        }

        let diagnostics = shared_coordination_ref_diagnostics(&root)
            .unwrap()
            .expect("shared coordination diagnostics should exist");
        let ref_name = super::shared_coordination_ref_name(&root);
        let manifest = super::load_shared_coordination_manifest_from_ref(&root, &ref_name)
            .unwrap()
            .expect("compacted manifest should exist");
        assert_eq!(diagnostics.history_depth, 1);
        assert!(diagnostics.compacted_head);
        assert_eq!(diagnostics.compaction_status, "compacted");
        assert!(!diagnostics.needs_compaction);
        assert_eq!(
            diagnostics.compaction_mode.as_deref(),
            Some("continuity_preserved")
        );
        assert!(diagnostics.last_verified_manifest_digest.is_some());
        assert_eq!(
            diagnostics.last_successful_publish_at,
            Some(publish.published_at)
        );
        assert_eq!(diagnostics.last_successful_publish_retry_count, 0);
        assert_eq!(
            diagnostics.publish_retry_budget,
            super::SHARED_COORDINATION_PUSH_MAX_RETRIES as u32
        );
        assert_eq!(
            manifest
                .compaction
                .as_ref()
                .map(|compaction| &compaction.mode),
            Some(&super::SharedCoordinationManifestCompactionMode::ContinuityPreserved)
        );
        assert_eq!(manifest.published_at, publish.published_at);
        assert!(manifest.previous_manifest_digest.is_some());
    }

    #[test]
    fn shared_coordination_ref_diagnostics_report_manifest_and_history() {
        let (root, _remote) = temp_git_repo_with_origin();
        let (snapshot, graph, execution_map) =
            sample_snapshot_for("plan:shared-diagnostics", "coord-task:shared-diagnostics");
        let publish = sample_publish_context();
        sync_shared_coordination_ref_state(
            &root,
            &snapshot,
            &[graph],
            &execution_map,
            Some(&publish),
        )
        .unwrap();

        let diagnostics = shared_coordination_ref_diagnostics(&root)
            .unwrap()
            .expect("shared coordination diagnostics should exist");
        assert!(diagnostics.ref_name.starts_with("refs/prism/coordination/"));
        assert!(diagnostics.head_commit.is_some());
        assert!(diagnostics.history_depth >= 1);
        assert!(diagnostics.snapshot_file_count >= 3);
        assert!(diagnostics.current_manifest_digest.is_some());
        assert_eq!(
            diagnostics.current_manifest_digest,
            diagnostics.last_verified_manifest_digest
        );
        assert_eq!(
            diagnostics.last_successful_publish_at,
            Some(publish.published_at)
        );
        assert_eq!(diagnostics.last_successful_publish_retry_count, 0);
        assert_eq!(
            diagnostics.publish_retry_budget,
            super::SHARED_COORDINATION_PUSH_MAX_RETRIES as u32
        );
        assert_eq!(diagnostics.runtime_descriptor_count, 1);
        assert_eq!(diagnostics.runtime_descriptors.len(), 1);
        assert!(diagnostics.runtime_descriptors[0]
            .capabilities
            .contains(&RuntimeDescriptorCapability::CoordinationRefPublisher));
        assert!(diagnostics.runtime_descriptors[0]
            .checked_out_commit
            .is_some());
        assert!(matches!(
            diagnostics.compaction_status.as_str(),
            "healthy" | "compacted"
        ));
    }

    #[test]
    fn sync_live_runtime_descriptor_publishes_descriptor_without_coordination_objects() {
        let (root, _remote) = temp_git_repo_with_origin();
        sync_live_runtime_descriptor(&root).unwrap();

        let diagnostics = shared_coordination_ref_diagnostics(&root)
            .unwrap()
            .expect("shared coordination diagnostics should exist");
        assert_eq!(diagnostics.runtime_descriptor_count, 1);
        assert_eq!(diagnostics.runtime_descriptors.len(), 1);
        assert!(diagnostics.runtime_descriptors[0]
            .capabilities
            .contains(&RuntimeDescriptorCapability::CoordinationRefPublisher));
    }

    #[test]
    fn sync_live_runtime_descriptor_publishes_configured_public_url() {
        let (root, _remote) = temp_git_repo_with_origin();
        let public_url_path = crate::PrismPaths::for_workspace_root(&root)
            .unwrap()
            .mcp_public_url_path()
            .unwrap();
        fs::create_dir_all(public_url_path.parent().unwrap()).unwrap();
        fs::write(&public_url_path, "https://runtime.example/peer/query\n").unwrap();

        sync_live_runtime_descriptor(&root).unwrap();

        let diagnostics = shared_coordination_ref_diagnostics(&root)
            .unwrap()
            .expect("shared coordination diagnostics should exist");
        assert_eq!(
            diagnostics.runtime_descriptors[0]
                .public_endpoint
                .as_deref(),
            Some("https://runtime.example/peer/query")
        );
        assert_eq!(
            diagnostics.runtime_descriptors[0].discovery_mode,
            prism_coordination::RuntimeDiscoveryMode::PublicUrl
        );
    }

    #[test]
    fn sync_live_runtime_descriptor_clears_public_url_when_configuration_is_removed() {
        let (root, _remote) = temp_git_repo_with_origin();
        let public_url_path = crate::PrismPaths::for_workspace_root(&root)
            .unwrap()
            .mcp_public_url_path()
            .unwrap();
        fs::create_dir_all(public_url_path.parent().unwrap()).unwrap();
        fs::write(&public_url_path, "https://runtime.example/peer/query\n").unwrap();

        sync_live_runtime_descriptor(&root).unwrap();
        fs::remove_file(&public_url_path).unwrap();
        sync_live_runtime_descriptor(&root).unwrap();

        let diagnostics = shared_coordination_ref_diagnostics(&root)
            .unwrap()
            .expect("shared coordination diagnostics should exist");
        assert_eq!(diagnostics.runtime_descriptor_count, 1);
        assert_eq!(diagnostics.runtime_descriptors[0].public_endpoint, None);
        assert_eq!(
            diagnostics.runtime_descriptors[0].discovery_mode,
            prism_coordination::RuntimeDiscoveryMode::None
        );
    }
}
