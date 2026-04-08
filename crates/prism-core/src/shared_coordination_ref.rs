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
    Artifact, ArtifactReview, CoordinationSnapshot, CoordinationSnapshotV2, CoordinationTask, Plan,
    RuntimeDescriptor, RuntimeDescriptorCapability, WorkClaim, COORDINATION_SCHEMA_V2,
};
use prism_ir::{WorkContextKind, WorkContextSnapshot};
use prism_store::CoordinationStartupCheckpointAuthority;
use serde::{Deserialize, Serialize};

use crate::coordination_snapshot_sanitization::sanitize_plan;
use crate::peer_runtime::{
    configured_public_runtime_endpoint, local_peer_runtime_discovery_mode,
    local_peer_runtime_endpoint,
};
use crate::protected_state::canonical::{canonical_json_bytes, sha256_prefixed};
use crate::protected_state::envelope::ProtectedSignatureAlgorithm;
use crate::protected_state::repo_streams::{
    implicit_principal_identity, ProtectedPrincipalIdentity,
};
use crate::protected_state::trust::{load_active_runtime_signing_key, resolve_trusted_runtime_key};
use crate::shared_coordination_schema::{
    parse_authoritative_payload, parse_top_level_authoritative_payload, wrap_authoritative_payload,
    SHARED_COORDINATION_KIND_ARTIFACT, SHARED_COORDINATION_KIND_CLAIM,
    SHARED_COORDINATION_KIND_MANIFEST, SHARED_COORDINATION_KIND_PLAN_RECORD,
    SHARED_COORDINATION_KIND_REVIEW, SHARED_COORDINATION_KIND_RUNTIME_DESCRIPTOR,
    SHARED_COORDINATION_KIND_TASK, SHARED_COORDINATION_SCHEMA_VERSION,
};
use crate::tracked_snapshot::{SnapshotManifestPublishSummary, TrackedSnapshotPublishContext};
use crate::util::{current_timestamp, current_timestamp_millis, stable_hash_bytes};
use crate::workspace_identity::workspace_identity_for_root;
use crate::PrismPaths;

#[cfg(not(test))]
const SHARED_COORDINATION_PUSH_MAX_RETRIES: usize = 3;
#[cfg(test)]
const SHARED_COORDINATION_PUSH_MAX_RETRIES: usize = 1;
#[cfg(not(test))]
const SHARED_COORDINATION_HISTORY_MAX_COMMITS: u64 = 32;
#[cfg(test)]
const SHARED_COORDINATION_HISTORY_MAX_COMMITS: u64 = 8;
const SHARED_COORDINATION_RUNTIME_REF_PREFIX: &str = "runtimes";
const SHARED_COORDINATION_TASK_SHARD_PREFIX: &str = "tasks";
const SHARED_COORDINATION_CLAIM_SHARD_PREFIX: &str = "claims";
const GIT_REPO_AVAILABLE_CACHE_TTL_MS: u64 = 5_000;
static SHARED_COORDINATION_LIVE_SYNC_STATE: OnceLock<
    Mutex<HashMap<PathBuf, SharedCoordinationLiveSyncState>>,
> = OnceLock::new();
static SHARED_COORDINATION_STATE_CACHE: OnceLock<
    Mutex<HashMap<PathBuf, SharedCoordinationStateCacheEntry>>,
> = OnceLock::new();
static GIT_REPO_AVAILABLE_CACHE: OnceLock<Mutex<HashMap<PathBuf, GitRepoAvailableCacheEntry>>> =
    OnceLock::new();

#[derive(Debug, Clone, Copy)]
struct GitRepoAvailableCacheEntry {
    available: bool,
    checked_at_ms: u64,
}

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
    #[serde(rename = "schema_version", alias = "schemaVersion", alias = "version")]
    schema_version: u32,
    #[serde(default = "shared_coordination_manifest_kind")]
    kind: String,
    published_at: u64,
    publisher: SharedCoordinationManifestPublisher,
    work_context: WorkContextSnapshot,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    publish_summary: Option<SnapshotManifestPublishSummary>,
    files: BTreeMap<String, SharedCoordinationManifestFile>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    previous_manifest_digest: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    summary_sources: Option<SharedCoordinationSummarySourceHeads>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    publish_diagnostics: Option<SharedCoordinationManifestPublishDiagnostics>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    compaction: Option<SharedCoordinationManifestCompaction>,
    signature: SharedCoordinationManifestSignature,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct SharedCoordinationManifestSigningView<'a> {
    #[serde(rename = "schema_version")]
    schema_version: u32,
    kind: &'a str,
    published_at: u64,
    publisher: &'a SharedCoordinationManifestPublisher,
    work_context: &'a WorkContextSnapshot,
    publish_summary: &'a Option<SnapshotManifestPublishSummary>,
    files: &'a BTreeMap<String, SharedCoordinationManifestFile>,
    previous_manifest_digest: &'a Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    summary_sources: &'a Option<SharedCoordinationSummarySourceHeads>,
    #[serde(skip_serializing_if = "Option::is_none")]
    publish_diagnostics: &'a Option<SharedCoordinationManifestPublishDiagnostics>,
    #[serde(skip_serializing_if = "Option::is_none")]
    compaction: &'a Option<SharedCoordinationManifestCompaction>,
    signature: SharedCoordinationManifestSignatureMetadata<'a>,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct LegacySharedCoordinationManifestSigningView<'a> {
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

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "camelCase")]
struct SharedCoordinationSummarySourceHeads {
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    task_shard_heads: BTreeMap<String, String>,
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    claim_shard_heads: BTreeMap<String, String>,
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    runtime_ref_heads: BTreeMap<String, String>,
}

impl SharedCoordinationSummarySourceHeads {
    fn is_empty(&self) -> bool {
        self.task_shard_heads.is_empty()
            && self.claim_shard_heads.is_empty()
            && self.runtime_ref_heads.is_empty()
    }
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
}

#[derive(Debug, Clone, PartialEq)]
pub(crate) struct SharedCoordinationRefState {
    pub(crate) snapshot: CoordinationSnapshot,
    pub(crate) canonical_snapshot_v2: CoordinationSnapshotV2,
    pub(crate) runtime_descriptors: Vec<RuntimeDescriptor>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SharedCoordinationRefDiagnostics {
    pub ref_name: String,
    pub head_commit: Option<String>,
    pub history_depth: u64,
    pub max_history_commits: u64,
    pub snapshot_file_count: usize,
    pub verification_status: String,
    pub authoritative_hydration_allowed: bool,
    pub degraded: bool,
    pub verification_error: Option<String>,
    pub repair_hint: Option<String>,
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
    pub summary_published_at: Option<u64>,
    pub summary_freshness_status: String,
    pub authoritative_fallback_required: bool,
    pub freshness_reason: Option<String>,
    pub lagging_task_shard_refs: usize,
    pub lagging_claim_shard_refs: usize,
    pub lagging_runtime_refs: usize,
    pub newest_authoritative_ref_at: Option<u64>,
    pub runtime_descriptor_count: usize,
    pub runtime_descriptors: Vec<RuntimeDescriptor>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SharedCoordinationRefStatusSummary {
    pub ref_name: String,
    pub head_commit: Option<String>,
    pub history_depth: u64,
    pub snapshot_file_count: usize,
    pub needs_compaction: bool,
    pub compaction_status: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct SharedCoordinationRetainedHistoryEntry {
    pub(crate) head_commit: String,
    pub(crate) manifest_digest: Option<String>,
    pub(crate) published_at: Option<u64>,
    pub(crate) previous_manifest_digest: Option<String>,
    pub(crate) summary: String,
}

pub(crate) enum SharedCoordinationRefLiveSync {
    Unchanged,
    Changed(SharedCoordinationRefState),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum SharedCoordinationSummaryFreshnessStatus {
    Current,
    Stale,
    Ambiguous,
}

impl SharedCoordinationSummaryFreshnessStatus {
    fn as_str(self) -> &'static str {
        match self {
            Self::Current => "current",
            Self::Stale => "stale",
            Self::Ambiguous => "ambiguous",
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct SharedCoordinationSummaryFreshness {
    summary_published_at: Option<u64>,
    status: SharedCoordinationSummaryFreshnessStatus,
    authoritative_fallback_required: bool,
    reason: Option<String>,
    lagging_task_shard_refs: usize,
    lagging_claim_shard_refs: usize,
    lagging_runtime_refs: usize,
    newest_authoritative_ref_at: Option<u64>,
    task_fallback_required: bool,
    claim_fallback_required: bool,
    runtime_fallback_required: bool,
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct SharedCoordinationRefHead {
    head: String,
    published_at: Option<u64>,
}

#[derive(Debug, Clone, PartialEq, Eq, Default)]
struct SharedCoordinationAuthoritativeHeads {
    task_shard_heads: BTreeMap<String, SharedCoordinationRefHead>,
    claim_shard_heads: BTreeMap<String, SharedCoordinationRefHead>,
    runtime_ref_heads: BTreeMap<String, SharedCoordinationRefHead>,
}

impl SharedCoordinationAuthoritativeHeads {
    fn is_empty(&self) -> bool {
        self.task_shard_heads.is_empty()
            && self.claim_shard_heads.is_empty()
            && self.runtime_ref_heads.is_empty()
    }

    fn newest_published_at(&self) -> Option<u64> {
        self.task_shard_heads
            .values()
            .chain(self.claim_shard_heads.values())
            .chain(self.runtime_ref_heads.values())
            .filter_map(|head| head.published_at)
            .max()
    }
}

fn shared_coordination_manifest_kind() -> String {
    SHARED_COORDINATION_KIND_MANIFEST.to_string()
}

fn shared_coordination_ref_base(root: &Path) -> String {
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
    format!("refs/prism/coordination/{logical_repo_id}")
}

fn shared_coordination_ref_name(root: &Path) -> String {
    format!("{}/live", shared_coordination_ref_base(root))
}

fn shared_coordination_runtime_ref_prefix(root: &Path) -> String {
    format!(
        "{}/{}",
        shared_coordination_ref_base(root),
        SHARED_COORDINATION_RUNTIME_REF_PREFIX
    )
}

fn shared_coordination_task_ref_prefix(root: &Path) -> String {
    format!(
        "{}/{}",
        shared_coordination_ref_base(root),
        SHARED_COORDINATION_TASK_SHARD_PREFIX
    )
}

fn shared_coordination_claim_ref_prefix(root: &Path) -> String {
    format!(
        "{}/{}",
        shared_coordination_ref_base(root),
        SHARED_COORDINATION_CLAIM_SHARD_PREFIX
    )
}

fn shared_coordination_ref_component(identity: &str) -> String {
    snapshot_file_name(identity)
        .trim_end_matches(".json")
        .to_string()
}

fn shared_coordination_runtime_ref_name(root: &Path, runtime_id: &str) -> String {
    format!(
        "{}/{}",
        shared_coordination_runtime_ref_prefix(root),
        shared_coordination_ref_component(runtime_id)
    )
}

fn shared_coordination_task_shard_ref_name(root: &Path, shard: &str) -> String {
    format!("{}/{}", shared_coordination_task_ref_prefix(root), shard)
}

fn shared_coordination_claim_shard_ref_name(root: &Path, shard: &str) -> String {
    format!("{}/{}", shared_coordination_claim_ref_prefix(root), shard)
}

fn shared_coordination_shard_key(stable_id: &str) -> String {
    let digest = blake3::hash(stable_id.as_bytes());
    format!("{:02x}", digest.as_bytes()[0])
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

fn list_local_coordination_ref_heads(
    root: &Path,
    prefix: &str,
) -> Result<BTreeMap<String, String>> {
    let output = run_git(
        root,
        &["for-each-ref", "--format=%(refname) %(objectname)", prefix],
    )?;
    let mut refs = BTreeMap::new();
    for line in output.lines().filter(|line| !line.trim().is_empty()) {
        let mut parts = line.split_whitespace();
        let Some(ref_name) = parts.next() else {
            continue;
        };
        let Some(head) = parts.next() else {
            continue;
        };
        refs.insert(ref_name.to_string(), head.to_string());
    }
    Ok(refs)
}

fn list_local_coordination_ref_head_metadata(
    root: &Path,
    prefix: &str,
) -> Result<BTreeMap<String, SharedCoordinationRefHead>> {
    let output = run_git(
        root,
        &[
            "for-each-ref",
            "--format=%(refname) %(objectname) %(committerdate:unix)",
            prefix,
        ],
    )?;
    let mut refs = BTreeMap::new();
    for line in output.lines().filter(|line| !line.trim().is_empty()) {
        let mut parts = line.split_whitespace();
        let Some(ref_name) = parts.next() else {
            continue;
        };
        let Some(head) = parts.next() else {
            continue;
        };
        let published_at = parts.next().and_then(|value| value.parse::<u64>().ok());
        refs.insert(
            ref_name.to_string(),
            SharedCoordinationRefHead {
                head: head.to_string(),
                published_at,
            },
        );
    }
    Ok(refs)
}

fn list_remote_coordination_ref_heads(
    root: &Path,
    remote: &str,
    prefix: &str,
) -> Result<BTreeMap<String, String>> {
    let output = run_git(root, &["ls-remote", remote, &format!("{prefix}/*")])?;
    let mut refs = BTreeMap::new();
    for line in output.lines().filter(|line| !line.trim().is_empty()) {
        let mut parts = line.split_whitespace();
        let Some(head) = parts.next() else {
            continue;
        };
        let Some(ref_name) = parts.next() else {
            continue;
        };
        refs.insert(ref_name.to_string(), head.to_string());
    }
    Ok(refs)
}

fn refresh_local_shared_coordination_ref_family(
    root: &Path,
    remote: &str,
    prefix: &str,
) -> Result<BTreeMap<String, String>> {
    if !git_remote_available(root, remote) {
        return list_local_coordination_ref_heads(root, prefix);
    }
    let remote_refs = list_remote_coordination_ref_heads(root, remote, prefix)?;
    if !remote_refs.is_empty() {
        let refspec = format!("+{prefix}/*:{prefix}/*");
        let _ = run_git(root, &["fetch", remote, &refspec])?;
    }
    let local_refs = list_local_coordination_ref_heads(root, prefix)?;
    for local_ref in local_refs.keys() {
        if !remote_refs.contains_key(local_ref) {
            let _ = run_git(root, &["update-ref", "-d", local_ref])?;
        }
    }
    Ok(remote_refs)
}

fn authoritative_shared_coordination_ref_heads(root: &Path) -> Result<BTreeMap<String, String>> {
    let mut refs = BTreeMap::new();
    let summary_ref = shared_coordination_ref_name(root);
    if let Some(head) = resolve_ref_commit(root, &summary_ref)? {
        refs.insert(summary_ref, head);
    }
    refs.extend(list_local_coordination_ref_heads(
        root,
        &shared_coordination_runtime_ref_prefix(root),
    )?);
    refs.extend(list_local_coordination_ref_heads(
        root,
        &shared_coordination_task_ref_prefix(root),
    )?);
    refs.extend(list_local_coordination_ref_heads(
        root,
        &shared_coordination_claim_ref_prefix(root),
    )?);
    Ok(refs)
}

fn authoritative_shared_coordination_state_key(root: &Path) -> Result<Option<String>> {
    resolve_ref_commit(root, &shared_coordination_ref_name(root))
}

fn current_shared_coordination_authoritative_heads(
    root: &Path,
) -> Result<SharedCoordinationAuthoritativeHeads> {
    Ok(SharedCoordinationAuthoritativeHeads {
        task_shard_heads: list_local_coordination_ref_head_metadata(
            root,
            &shared_coordination_task_ref_prefix(root),
        )?,
        claim_shard_heads: list_local_coordination_ref_head_metadata(
            root,
            &shared_coordination_claim_ref_prefix(root),
        )?,
        runtime_ref_heads: list_local_coordination_ref_head_metadata(
            root,
            &shared_coordination_runtime_ref_prefix(root),
        )?,
    })
}

fn current_shared_coordination_summary_source_heads(
    root: &Path,
) -> Result<SharedCoordinationSummarySourceHeads> {
    let authoritative_heads = current_shared_coordination_authoritative_heads(root)?;
    Ok(SharedCoordinationSummarySourceHeads {
        task_shard_heads: authoritative_heads
            .task_shard_heads
            .into_iter()
            .map(|(ref_name, head)| (ref_name, head.head))
            .collect(),
        claim_shard_heads: authoritative_heads
            .claim_shard_heads
            .into_iter()
            .map(|(ref_name, head)| (ref_name, head.head))
            .collect(),
        runtime_ref_heads: authoritative_heads
            .runtime_ref_heads
            .into_iter()
            .map(|(ref_name, head)| (ref_name, head.head))
            .collect(),
    })
}

fn differing_ref_head_count(
    summary_heads: &BTreeMap<String, String>,
    authoritative_heads: &BTreeMap<String, SharedCoordinationRefHead>,
) -> usize {
    summary_heads
        .keys()
        .chain(authoritative_heads.keys())
        .collect::<BTreeSet<_>>()
        .into_iter()
        .filter(|ref_name| {
            summary_heads.get(*ref_name).map(String::as_str)
                != authoritative_heads
                    .get(*ref_name)
                    .map(|head| head.head.as_str())
        })
        .count()
}

fn inspect_shared_coordination_summary_freshness(
    summary_manifest: Option<&SharedCoordinationManifest>,
    summary_state: Option<&SharedCoordinationRefState>,
    authoritative_heads: &SharedCoordinationAuthoritativeHeads,
) -> SharedCoordinationSummaryFreshness {
    let summary_published_at = summary_manifest.map(|manifest| manifest.published_at);
    if summary_state.is_none() && !authoritative_heads.is_empty() {
        return SharedCoordinationSummaryFreshness {
            summary_published_at,
            status: SharedCoordinationSummaryFreshnessStatus::Ambiguous,
            authoritative_fallback_required: true,
            reason: Some(
                "summary ref is missing or unreadable while authoritative shard/runtime state exists"
                    .to_string(),
            ),
            lagging_task_shard_refs: authoritative_heads.task_shard_heads.len(),
            lagging_claim_shard_refs: authoritative_heads.claim_shard_heads.len(),
            lagging_runtime_refs: authoritative_heads.runtime_ref_heads.len(),
            newest_authoritative_ref_at: authoritative_heads.newest_published_at(),
            task_fallback_required: !authoritative_heads.task_shard_heads.is_empty(),
            claim_fallback_required: !authoritative_heads.claim_shard_heads.is_empty(),
            runtime_fallback_required: !authoritative_heads.runtime_ref_heads.is_empty(),
        };
    }
    if authoritative_heads.is_empty() {
        return SharedCoordinationSummaryFreshness {
            summary_published_at,
            status: SharedCoordinationSummaryFreshnessStatus::Current,
            authoritative_fallback_required: false,
            reason: None,
            lagging_task_shard_refs: 0,
            lagging_claim_shard_refs: 0,
            lagging_runtime_refs: 0,
            newest_authoritative_ref_at: None,
            task_fallback_required: false,
            claim_fallback_required: false,
            runtime_fallback_required: false,
        };
    }
    let Some(summary_sources) =
        summary_manifest.and_then(|manifest| manifest.summary_sources.as_ref())
    else {
        return SharedCoordinationSummaryFreshness {
            summary_published_at,
            status: SharedCoordinationSummaryFreshnessStatus::Ambiguous,
            authoritative_fallback_required: true,
            reason: Some(
                "summary manifest is missing authoritative source-head metadata".to_string(),
            ),
            lagging_task_shard_refs: authoritative_heads.task_shard_heads.len(),
            lagging_claim_shard_refs: authoritative_heads.claim_shard_heads.len(),
            lagging_runtime_refs: authoritative_heads.runtime_ref_heads.len(),
            newest_authoritative_ref_at: authoritative_heads.newest_published_at(),
            task_fallback_required: !authoritative_heads.task_shard_heads.is_empty(),
            claim_fallback_required: !authoritative_heads.claim_shard_heads.is_empty(),
            runtime_fallback_required: !authoritative_heads.runtime_ref_heads.is_empty(),
        };
    };
    let lagging_task_shard_refs = differing_ref_head_count(
        &summary_sources.task_shard_heads,
        &authoritative_heads.task_shard_heads,
    );
    let lagging_claim_shard_refs = differing_ref_head_count(
        &summary_sources.claim_shard_heads,
        &authoritative_heads.claim_shard_heads,
    );
    let lagging_runtime_refs = differing_ref_head_count(
        &summary_sources.runtime_ref_heads,
        &authoritative_heads.runtime_ref_heads,
    );
    let lagging_total = lagging_task_shard_refs + lagging_claim_shard_refs + lagging_runtime_refs;
    SharedCoordinationSummaryFreshness {
        summary_published_at,
        status: if lagging_total > 0 {
            SharedCoordinationSummaryFreshnessStatus::Stale
        } else {
            SharedCoordinationSummaryFreshnessStatus::Current
        },
        authoritative_fallback_required: lagging_total > 0,
        reason: if lagging_total > 0 {
            Some(format!(
                "summary source heads lag {lagging_task_shard_refs} task shard ref(s), {lagging_claim_shard_refs} claim shard ref(s), and {lagging_runtime_refs} runtime ref(s)"
            ))
        } else {
            None
        },
        lagging_task_shard_refs,
        lagging_claim_shard_refs,
        lagging_runtime_refs,
        newest_authoritative_ref_at: authoritative_heads.newest_published_at(),
        task_fallback_required: lagging_task_shard_refs > 0,
        claim_fallback_required: lagging_claim_shard_refs > 0,
        runtime_fallback_required: lagging_runtime_refs > 0,
    }
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
    record_observed_shared_coordination_head(
        root,
        authoritative_shared_coordination_state_key(root)?,
    );
    Ok(())
}

pub(crate) fn poll_shared_coordination_ref_live_sync(
    root: &Path,
) -> Result<SharedCoordinationRefLiveSync> {
    if !git_repo_available(root) {
        return Ok(SharedCoordinationRefLiveSync::Unchanged);
    }
    let ref_name = shared_coordination_ref_name(root);
    let local_head_before = authoritative_shared_coordination_state_key(root)?;
    let _ =
        refresh_local_shared_coordination_ref(root, shared_coordination_remote_name(), &ref_name)?;
    let _ = refresh_local_shared_coordination_ref_family(
        root,
        shared_coordination_remote_name(),
        &shared_coordination_task_ref_prefix(root),
    )?;
    let _ = refresh_local_shared_coordination_ref_family(
        root,
        shared_coordination_remote_name(),
        &shared_coordination_claim_ref_prefix(root),
    )?;
    let current_head = authoritative_shared_coordination_state_key(root)?;
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
    Ok(load_shared_coordination_ref_state(root)?
        .map(SharedCoordinationRefLiveSync::Changed)
        .unwrap_or(SharedCoordinationRefLiveSync::Unchanged))
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

fn stage_v2_dir(stage_root: &Path) -> PathBuf {
    stage_snapshot_root(stage_root).join("v2")
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
fn task_snapshot_relative_path(task_id: &str) -> String {
    format!("coordination/tasks/{}", snapshot_file_name(task_id))
}

fn plan_snapshot_path(stage_root: &Path, plan_id: &str) -> PathBuf {
    stage_plans_dir(stage_root).join(snapshot_file_name(plan_id))
}

fn task_snapshot_path(stage_root: &Path, task_id: &str) -> PathBuf {
    stage_tasks_dir(stage_root).join(snapshot_file_name(task_id))
}

fn v2_snapshot_path(stage_root: &Path) -> PathBuf {
    stage_v2_dir(stage_root).join("snapshot.json")
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

fn runtime_descriptor_snapshot_path(stage_root: &Path, runtime_id: &str) -> PathBuf {
    stage_runtimes_dir(stage_root).join(snapshot_file_name(runtime_id))
}

pub(crate) fn sync_shared_coordination_ref_state(
    root: &Path,
    snapshot: &CoordinationSnapshot,
    canonical_snapshot_v2: &CoordinationSnapshotV2,
    publish: Option<&TrackedSnapshotPublishContext>,
) -> Result<()> {
    if !git_repo_available(root) {
        return Ok(());
    }
    let desired_state = SharedCoordinationRefState {
        snapshot: summary_snapshot(snapshot),
        canonical_snapshot_v2: canonical_snapshot_v2.clone(),
        runtime_descriptors: load_shared_coordination_runtime_refs(root)?,
    };
    sync_shared_coordination_ref_family_state(root, &desired_state, publish)?;
    record_observed_shared_coordination_head(
        root,
        authoritative_shared_coordination_state_key(root)?,
    );
    Ok(())
}

pub(crate) fn publish_runtime_descriptor_record(
    root: &Path,
    descriptor: &RuntimeDescriptor,
) -> Result<()> {
    if !git_repo_available(root) {
        return Ok(());
    }
    let mut desired_state = load_shared_coordination_ref_state_authoritative(root)?
        .unwrap_or_else(empty_shared_coordination_ref_state);
    let existing = desired_state.runtime_descriptors.clone();
    desired_state.runtime_descriptors =
        overlay_records(&existing, std::slice::from_ref(descriptor), |descriptor| {
            descriptor.runtime_id.as_str()
        });
    sync_shared_coordination_ref_family_state(root, &desired_state, None)?;
    record_observed_shared_coordination_head(
        root,
        authoritative_shared_coordination_state_key(root)?,
    );
    Ok(())
}

pub(crate) fn clear_runtime_descriptor_record(root: &Path, runtime_id: &str) -> Result<()> {
    if !git_repo_available(root) {
        return Ok(());
    }
    let mut desired_state = load_shared_coordination_ref_state_authoritative(root)?
        .unwrap_or_else(empty_shared_coordination_ref_state);
    desired_state
        .runtime_descriptors
        .retain(|descriptor| descriptor.runtime_id != runtime_id);
    sync_shared_coordination_ref_family_state(root, &desired_state, None)?;
    record_observed_shared_coordination_head(
        root,
        authoritative_shared_coordination_state_key(root)?,
    );
    Ok(())
}

pub(crate) fn build_local_runtime_descriptor_for_current_state(
    root: &Path,
) -> Result<RuntimeDescriptor> {
    let existing = load_shared_coordination_ref_state_authoritative(root)?
        .map(|state| state.runtime_descriptors)
        .unwrap_or_default();
    local_runtime_descriptor(root, None, Some(&existing))
}

pub fn sync_live_runtime_descriptor(root: &Path) -> Result<()> {
    let local = build_local_runtime_descriptor_for_current_state(root)?;
    publish_runtime_descriptor_record(root, &local)
}

fn sync_shared_coordination_ref_family_state(
    root: &Path,
    desired_state: &SharedCoordinationRefState,
    publish: Option<&TrackedSnapshotPublishContext>,
) -> Result<()> {
    let ref_name = shared_coordination_ref_name(root);
    let current_head = resolve_ref_commit(root, &ref_name)?;
    let cached_state_before = shared_coordination_state_cache()
        .lock()
        .expect("shared coordination state cache lock poisoned")
        .get(root)
        .cloned();
    let latest_state_at_entry =
        load_shared_coordination_ref_state_from_current_ref(root, &ref_name)?;
    let baseline_state = cached_state_before
        .as_ref()
        .filter(|entry| Some(entry.head.clone()) != current_head)
        .map(|entry| entry.state.clone())
        .or_else(|| latest_state_at_entry.clone());
    let mut current_state = if let Some(previously_observed_state) = cached_state_before
        .filter(|entry| Some(entry.head.clone()) != current_head)
        .map(|entry| entry.state)
    {
        reconcile_shared_coordination_ref_state(
            Some(&previously_observed_state),
            desired_state,
            latest_state_at_entry.as_ref(),
        )?
    } else {
        desired_state.clone()
    };
    let task_prefix = shared_coordination_task_ref_prefix(root);
    let claim_prefix = shared_coordination_claim_ref_prefix(root);
    let runtime_prefix = shared_coordination_runtime_ref_prefix(root);
    let paths = PrismPaths::for_workspace_root(root)?;

    for attempt in 0..=SHARED_COORDINATION_PUSH_MAX_RETRIES {
        let mut updates = prepare_shared_coordination_shard_updates(
            root,
            &task_prefix,
            current_state.snapshot.tasks.iter().cloned().fold(
                BTreeMap::<String, Vec<CoordinationTask>>::new(),
                |mut shards, task| {
                    shards
                        .entry(shared_coordination_shard_key(task.id.0.as_str()))
                        .or_default()
                        .push(task);
                    shards
                },
            ),
            publish,
            |shard| shared_coordination_task_shard_ref_name(root, shard),
            "task-shard",
            sync_task_objects,
            rebuild_task_index,
            attempt as u32,
        )?;
        updates.extend(prepare_shared_coordination_shard_updates(
            root,
            &claim_prefix,
            current_state.snapshot.claims.iter().cloned().fold(
                BTreeMap::<String, Vec<WorkClaim>>::new(),
                |mut shards, claim| {
                    shards
                        .entry(shared_coordination_shard_key(claim.id.0.as_str()))
                        .or_default()
                        .push(claim);
                    shards
                },
            ),
            publish,
            |shard| shared_coordination_claim_shard_ref_name(root, shard),
            "claim-shard",
            sync_claim_objects,
            rebuild_claim_index,
            attempt as u32,
        )?);
        updates.extend(prepare_shared_coordination_shard_updates(
            root,
            &runtime_prefix,
            current_state.runtime_descriptors.iter().cloned().fold(
                BTreeMap::<String, Vec<RuntimeDescriptor>>::new(),
                |mut refs, descriptor| {
                    refs.entry(descriptor.runtime_id.clone())
                        .or_default()
                        .push(descriptor);
                    refs
                },
            ),
            publish,
            |runtime_id| shared_coordination_runtime_ref_name(root, runtime_id),
            "runtime",
            sync_runtime_descriptor_objects,
            rebuild_runtime_descriptor_index,
            attempt as u32,
        )?);

        let summary_sources =
            planned_shared_coordination_summary_source_heads(root, updates.as_slice())?;
        if let Some(summary_update) = prepare_shared_coordination_summary_update(
            root,
            &paths,
            &current_state,
            &summary_sources,
            publish,
            attempt as u32,
        )? {
            updates.push(summary_update);
        }

        if updates.is_empty() {
            return Ok(());
        }

        match push_shared_coordination_ref_updates_atomic(
            root,
            shared_coordination_remote_name(),
            updates.as_slice(),
        ) {
            Ok(()) => {
                refresh_local_shared_coordination_authority(root)?;
                let published_head = resolve_ref_commit(root, &ref_name)?;
                let final_head = maybe_compact_shared_coordination_ref(
                    root,
                    &paths,
                    shared_coordination_remote_name(),
                    &ref_name,
                    published_head.as_deref(),
                )?;
                if let Some(final_head) = final_head.clone() {
                    cache_shared_coordination_state(root, final_head, &current_state);
                }
                return Ok(());
            }
            Err(error)
                if attempt < SHARED_COORDINATION_PUSH_MAX_RETRIES
                    && is_shared_coordination_push_conflict(&error) =>
            {
                refresh_local_shared_coordination_authority(root)?;
                let latest_state =
                    load_shared_coordination_ref_state_from_current_ref(root, &ref_name)?;
                let reconciled = reconcile_shared_coordination_ref_state(
                    baseline_state.as_ref(),
                    desired_state,
                    latest_state.as_ref(),
                )?;
                current_state = reconciled;
            }
            Err(error) => return Err(error),
        }
    }

    Err(anyhow!(
        "shared coordination ref publish exceeded retry budget after repeated compare-and-swap conflicts"
    ))
}

fn refresh_local_shared_coordination_authority(root: &Path) -> Result<()> {
    let remote = shared_coordination_remote_name();
    let _ =
        refresh_local_shared_coordination_ref(root, remote, &shared_coordination_ref_name(root))?;
    let _ = refresh_local_shared_coordination_ref_family(
        root,
        remote,
        &shared_coordination_task_ref_prefix(root),
    )?;
    let _ = refresh_local_shared_coordination_ref_family(
        root,
        remote,
        &shared_coordination_claim_ref_prefix(root),
    )?;
    let _ = refresh_local_shared_coordination_ref_family(
        root,
        remote,
        &shared_coordination_runtime_ref_prefix(root),
    )?;
    Ok(())
}

fn planned_shared_coordination_summary_source_heads(
    root: &Path,
    updates: &[PreparedSharedCoordinationRefUpdate],
) -> Result<SharedCoordinationSummarySourceHeads> {
    let mut heads = current_shared_coordination_summary_source_heads(root)?;
    let task_prefix = shared_coordination_task_ref_prefix(root);
    let claim_prefix = shared_coordination_claim_ref_prefix(root);
    let runtime_prefix = shared_coordination_runtime_ref_prefix(root);
    for update in updates {
        if update.ref_name.starts_with(&task_prefix) {
            heads
                .task_shard_heads
                .insert(update.ref_name.clone(), update.new_commit.clone());
        } else if update.ref_name.starts_with(&claim_prefix) {
            heads
                .claim_shard_heads
                .insert(update.ref_name.clone(), update.new_commit.clone());
        } else if update.ref_name.starts_with(&runtime_prefix) {
            heads
                .runtime_ref_heads
                .insert(update.ref_name.clone(), update.new_commit.clone());
        }
    }
    Ok(heads)
}

fn prepare_shared_coordination_summary_update(
    root: &Path,
    paths: &PrismPaths,
    desired_state: &SharedCoordinationRefState,
    summary_sources: &SharedCoordinationSummarySourceHeads,
    publish: Option<&TrackedSnapshotPublishContext>,
    attempt: u32,
) -> Result<Option<PreparedSharedCoordinationRefUpdate>> {
    let ref_name = shared_coordination_ref_name(root);
    let previous_manifest = load_shared_coordination_manifest_from_ref(root, &ref_name)?;
    let stage_parent = stage_root(paths);
    fs::create_dir_all(&stage_parent)?;
    let stage_dir = stage_parent.join(format!(
        "summary-{}-{}",
        std::process::id(),
        current_timestamp()
    ));
    fs::create_dir_all(&stage_dir)?;
    let result = (|| {
        sync_plan_objects(&stage_dir, &desired_state.snapshot)?;
        sync_v2_snapshot(&stage_dir, &desired_state.canonical_snapshot_v2)?;
        sync_task_objects(&stage_dir, &desired_state.snapshot.tasks)?;
        sync_artifact_objects(&stage_dir, &desired_state.snapshot.artifacts)?;
        sync_claim_objects(&stage_dir, &desired_state.snapshot.claims)?;
        sync_review_objects(&stage_dir, &desired_state.snapshot.reviews)?;
        sync_runtime_descriptor_objects(&stage_dir, &desired_state.runtime_descriptors)?;
        rebuild_plan_index(&stage_dir, &desired_state.snapshot.plans)?;
        rebuild_task_index(&stage_dir, &desired_state.snapshot.tasks)?;
        rebuild_artifact_index(&stage_dir, &desired_state.snapshot.artifacts)?;
        rebuild_claim_index(&stage_dir, &desired_state.snapshot.claims)?;
        rebuild_review_index(&stage_dir, &desired_state.snapshot.reviews)?;
        rebuild_runtime_descriptor_index(&stage_dir, &desired_state.runtime_descriptors)?;
        write_manifest(
            &stage_dir,
            paths,
            publish,
            previous_manifest.as_ref(),
            Some(attempt),
            None,
            Some(summary_sources),
        )?;
        let mut publish_patch = build_shared_coordination_publish_patch_from_stage(
            &stage_dir,
            previous_manifest.as_ref(),
            false,
        )?;
        if publish_patch.upserts.is_empty() && publish_patch.deletes.is_empty() {
            return Ok(None);
        }
        publish_patch
            .upserts
            .insert("coordination/manifest.json".to_string());
        let new_commit = create_shared_coordination_commit_from_stage_patch(
            root,
            &stage_dir,
            &ref_name,
            &publish_patch,
        )?;
        Ok(Some(PreparedSharedCoordinationRefUpdate {
            ref_name,
            new_commit,
        }))
    })();
    let _ = fs::remove_dir_all(&stage_dir);
    result
}

fn prepare_shared_coordination_shard_updates<T, RefNameFn, SyncFn, RebuildIndexFn>(
    root: &Path,
    prefix: &str,
    desired_by_ref: BTreeMap<String, Vec<T>>,
    publish: Option<&TrackedSnapshotPublishContext>,
    ref_name_for: RefNameFn,
    stage_label: &str,
    sync_records: SyncFn,
    rebuild_index: RebuildIndexFn,
    attempt: u32,
) -> Result<Vec<PreparedSharedCoordinationRefUpdate>>
where
    T: Clone + PartialEq,
    RefNameFn: Fn(&str) -> String + Copy,
    SyncFn: Fn(&Path, &[T]) -> Result<()> + Copy,
    RebuildIndexFn: Fn(&Path, &[T]) -> Result<()> + Copy,
{
    if !git_repo_available(root) {
        return Ok(Vec::new());
    }
    let existing = list_local_coordination_ref_heads(root, prefix)?
        .into_keys()
        .filter_map(|ref_name| ref_name.rsplit('/').next().map(str::to_string))
        .collect::<BTreeSet<_>>();
    let refs = existing
        .into_iter()
        .chain(desired_by_ref.keys().cloned())
        .collect::<BTreeSet<_>>();
    let paths = PrismPaths::for_workspace_root(root)?;
    let stage_parent = stage_root(&paths);
    fs::create_dir_all(&stage_parent)?;
    let mut updates = Vec::new();
    for stable_id in refs {
        let ref_name = ref_name_for(&stable_id);
        let previous_manifest = load_shared_coordination_manifest_from_ref(root, &ref_name)?;
        let stage_dir = stage_parent.join(format!(
            "{stage_label}-{stable_id}-{}-{}",
            std::process::id(),
            current_timestamp()
        ));
        fs::create_dir_all(&stage_dir)?;
        let result: Result<Option<PreparedSharedCoordinationRefUpdate>> = (|| {
            let desired_records = desired_by_ref.get(&stable_id).cloned().unwrap_or_default();
            sync_records(&stage_dir, &desired_records)?;
            rebuild_index(&stage_dir, &desired_records)?;
            write_manifest(
                &stage_dir,
                &paths,
                publish,
                previous_manifest.as_ref(),
                Some(attempt),
                None,
                None,
            )?;
            let mut publish_patch = build_shared_coordination_publish_patch_from_stage(
                &stage_dir,
                previous_manifest.as_ref(),
                false,
            )?;
            if publish_patch.upserts.is_empty() && publish_patch.deletes.is_empty() {
                return Ok(None);
            }
            publish_patch
                .upserts
                .insert("coordination/manifest.json".to_string());
            let new_commit = create_shared_coordination_commit_from_stage_patch(
                root,
                &stage_dir,
                &ref_name,
                &publish_patch,
            )?;
            Ok(Some(PreparedSharedCoordinationRefUpdate {
                ref_name,
                new_commit,
            }))
        })();
        let _ = fs::remove_dir_all(&stage_dir);
        if let Some(update) = result? {
            updates.push(update);
        }
    }
    Ok(updates)
}

fn sync_task_shard_refs(
    root: &Path,
    tasks: &[CoordinationTask],
    publish: Option<&TrackedSnapshotPublishContext>,
) -> Result<()> {
    let desired = tasks.iter().cloned().fold(
        BTreeMap::<String, Vec<CoordinationTask>>::new(),
        |mut shards, task| {
            shards
                .entry(shared_coordination_shard_key(task.id.0.as_str()))
                .or_default()
                .push(task);
            shards
        },
    );
    sync_sharded_coordination_records(
        root,
        &shared_coordination_task_ref_prefix(root),
        desired,
        publish,
        |shard| shared_coordination_task_shard_ref_name(root, shard),
        |task| task.id.0.as_str(),
        "task-shard",
        sync_task_objects,
        rebuild_task_index,
        load_task_records_from_ref,
        |baseline, desired, latest| {
            reconcile_collection(baseline, desired, latest, |task| task.id.0.as_str(), "task")
        },
    )
}

fn sync_claim_shard_refs(
    root: &Path,
    claims: &[WorkClaim],
    publish: Option<&TrackedSnapshotPublishContext>,
) -> Result<()> {
    let desired = claims.iter().cloned().fold(
        BTreeMap::<String, Vec<WorkClaim>>::new(),
        |mut shards, claim| {
            shards
                .entry(shared_coordination_shard_key(claim.id.0.as_str()))
                .or_default()
                .push(claim);
            shards
        },
    );
    sync_sharded_coordination_records(
        root,
        &shared_coordination_claim_ref_prefix(root),
        desired,
        publish,
        |shard| shared_coordination_claim_shard_ref_name(root, shard),
        |claim| claim.id.0.as_str(),
        "claim-shard",
        sync_claim_objects,
        rebuild_claim_index,
        load_claim_records_from_ref,
        |baseline, desired, latest| {
            reconcile_collection(
                baseline,
                desired,
                latest,
                |claim| claim.id.0.as_str(),
                "claim",
            )
        },
    )
}

fn sync_runtime_descriptor_ref(
    root: &Path,
    descriptor: &RuntimeDescriptor,
    publish: Option<&TrackedSnapshotPublishContext>,
) -> Result<()> {
    if !git_repo_available(root) {
        return Ok(());
    }
    let ref_name = shared_coordination_runtime_ref_name(root, &descriptor.runtime_id);
    let expected_remote_head = resolve_ref_commit(root, &ref_name).ok().flatten();
    let baseline_records = load_runtime_descriptor_records_from_ref(root, &ref_name)?;
    let paths = PrismPaths::for_workspace_root(root)?;
    let stage_parent = stage_root(&paths);
    fs::create_dir_all(&stage_parent)?;
    let stage_dir = stage_parent.join(format!(
        "runtime-{}-{}-{}",
        shared_coordination_ref_component(&descriptor.runtime_id),
        std::process::id(),
        current_timestamp()
    ));
    fs::create_dir_all(&stage_dir)?;
    let result = sync_sharded_coordination_records_inner(
        root,
        &stage_dir,
        vec![descriptor.clone()],
        publish,
        &ref_name,
        expected_remote_head.as_deref(),
        &baseline_records,
        |descriptor| descriptor.runtime_id.as_str(),
        sync_runtime_descriptor_objects,
        rebuild_runtime_descriptor_index,
        load_runtime_descriptor_records_from_ref,
        |baseline, desired, latest| {
            reconcile_collection(
                baseline,
                desired,
                latest,
                |descriptor| descriptor.runtime_id.as_str(),
                "runtime descriptor",
            )
        },
    );
    let _ = fs::remove_dir_all(&stage_dir);
    result
}

fn sync_sharded_coordination_records<
    T,
    RefNameFn,
    KeyFn,
    SyncFn,
    RebuildIndexFn,
    LoadFn,
    ReconcileFn,
>(
    root: &Path,
    prefix: &str,
    desired_by_shard: BTreeMap<String, Vec<T>>,
    publish: Option<&TrackedSnapshotPublishContext>,
    ref_name_for: RefNameFn,
    key_for: KeyFn,
    stage_label: &str,
    sync_records: SyncFn,
    rebuild_index: RebuildIndexFn,
    load_records_from_ref: LoadFn,
    reconcile_records: ReconcileFn,
) -> Result<()>
where
    T: Clone + PartialEq,
    RefNameFn: Fn(&str) -> String + Copy,
    KeyFn: Fn(&T) -> &str + Copy,
    SyncFn: Fn(&Path, &[T]) -> Result<()> + Copy,
    RebuildIndexFn: Fn(&Path, &[T]) -> Result<()> + Copy,
    LoadFn: Fn(&Path, &str) -> Result<Vec<T>> + Copy,
    ReconcileFn: Fn(&[T], &[T], &[T]) -> Result<Vec<T>> + Copy,
{
    if !git_repo_available(root) {
        return Ok(());
    }
    let existing_shards = list_local_coordination_ref_heads(root, prefix)?
        .into_keys()
        .filter_map(|ref_name| ref_name.rsplit('/').next().map(str::to_string))
        .collect::<BTreeSet<_>>();
    let shards = existing_shards
        .into_iter()
        .chain(desired_by_shard.keys().cloned())
        .collect::<BTreeSet<_>>();
    let paths = PrismPaths::for_workspace_root(root)?;
    let stage_parent = stage_root(&paths);
    fs::create_dir_all(&stage_parent)?;
    for shard in shards {
        let ref_name = ref_name_for(&shard);
        let expected_remote_head = resolve_ref_commit(root, &ref_name).ok().flatten();
        let baseline_records = load_records_from_ref(root, &ref_name)?;
        let stage_dir = stage_parent.join(format!(
            "{stage_label}-{shard}-{}-{}",
            std::process::id(),
            current_timestamp()
        ));
        fs::create_dir_all(&stage_dir)?;
        let result = sync_sharded_coordination_records_inner(
            root,
            &stage_dir,
            desired_by_shard.get(&shard).cloned().unwrap_or_default(),
            publish,
            &ref_name,
            expected_remote_head.as_deref(),
            &baseline_records,
            key_for,
            sync_records,
            rebuild_index,
            load_records_from_ref,
            reconcile_records,
        );
        let _ = fs::remove_dir_all(&stage_dir);
        result?;
    }
    Ok(())
}

fn sync_sharded_coordination_records_inner<T, KeyFn, SyncFn, RebuildIndexFn, LoadFn, ReconcileFn>(
    root: &Path,
    stage_dir: &Path,
    desired_records: Vec<T>,
    publish: Option<&TrackedSnapshotPublishContext>,
    ref_name: &str,
    expected_remote_head: Option<&str>,
    baseline_records: &[T],
    key_for: KeyFn,
    sync_records: SyncFn,
    rebuild_index: RebuildIndexFn,
    load_records_from_ref: LoadFn,
    reconcile_records: ReconcileFn,
) -> Result<()>
where
    T: Clone + PartialEq,
    KeyFn: Fn(&T) -> &str + Copy,
    SyncFn: Fn(&Path, &[T]) -> Result<()> + Copy,
    RebuildIndexFn: Fn(&Path, &[T]) -> Result<()> + Copy,
    LoadFn: Fn(&Path, &str) -> Result<Vec<T>> + Copy,
    ReconcileFn: Fn(&[T], &[T], &[T]) -> Result<Vec<T>> + Copy,
{
    let paths = PrismPaths::for_workspace_root(root)?;
    let mut current_records = overlay_records(baseline_records, &desired_records, key_for);
    let mut current_expected_head = expected_remote_head.map(str::to_string);
    let mut current_previous_manifest = load_shared_coordination_manifest_from_ref(root, ref_name)?;
    for attempt in 0..=SHARED_COORDINATION_PUSH_MAX_RETRIES {
        sync_records(stage_dir, &current_records)?;
        rebuild_index(stage_dir, &current_records)?;
        write_manifest(
            stage_dir,
            &paths,
            publish,
            current_previous_manifest.as_ref(),
            Some(attempt as u32),
            None,
            None,
        )?;
        let mut publish_patch = build_shared_coordination_publish_patch_from_stage(
            stage_dir,
            current_previous_manifest.as_ref(),
            false,
        )?;
        if publish_patch.upserts.is_empty()
            && publish_patch.deletes.is_empty()
            && current_previous_manifest.is_none()
        {
            return Ok(());
        }
        if publish_patch.upserts.is_empty()
            && publish_patch.deletes.is_empty()
            && current_previous_manifest.is_some()
        {
            return Ok(());
        }
        publish_patch
            .upserts
            .insert("coordination/manifest.json".to_string());
        publish_stage_patch_to_ref(root, stage_dir, ref_name, &publish_patch)?;
        match push_shared_coordination_ref(
            root,
            shared_coordination_remote_name(),
            ref_name,
            current_expected_head.as_deref(),
        ) {
            Ok(()) => return Ok(()),
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
                    load_shared_coordination_manifest_from_ref(root, ref_name)?;
                let latest_records = load_records_from_ref(root, ref_name)?;
                current_records =
                    reconcile_records(baseline_records, &current_records, &latest_records)?;
            }
            Err(error) => return Err(error),
        }
    }

    Err(anyhow!(
        "shared coordination ref publish exceeded retry budget after repeated compare-and-swap conflicts"
    ))
}

fn load_task_records_from_ref(root: &Path, ref_name: &str) -> Result<Vec<CoordinationTask>> {
    load_records_from_ref(root, ref_name, |contents| {
        contents.parse_authoritative_records::<CoordinationTask, _>(
            |path| path.starts_with("coordination/tasks/"),
            SHARED_COORDINATION_KIND_TASK,
        )
    })
}

fn load_claim_records_from_ref(root: &Path, ref_name: &str) -> Result<Vec<WorkClaim>> {
    load_records_from_ref(root, ref_name, |contents| {
        contents.parse_authoritative_records::<WorkClaim, _>(
            |path| path.starts_with("coordination/claims/"),
            SHARED_COORDINATION_KIND_CLAIM,
        )
    })
}

fn load_runtime_descriptor_records_from_ref(
    root: &Path,
    ref_name: &str,
) -> Result<Vec<RuntimeDescriptor>> {
    load_records_from_ref(root, ref_name, |contents| {
        contents.parse_authoritative_records::<RuntimeDescriptor, _>(
            |path| path.starts_with("coordination/runtimes/"),
            SHARED_COORDINATION_KIND_RUNTIME_DESCRIPTOR,
        )
    })
}

fn load_records_from_ref<T, F>(root: &Path, ref_name: &str, parse: F) -> Result<Vec<T>>
where
    T: Clone,
    F: Fn(&SharedCoordinationRefContents) -> Result<Vec<(String, T)>>,
{
    let Some(contents) = load_shared_coordination_ref_contents(root, ref_name)? else {
        return Ok(Vec::new());
    };
    let manifest = contents.parse_manifest()?;
    verify_shared_coordination_manifest(root, &manifest, &contents)?;
    Ok(parse(&contents)?
        .into_iter()
        .map(|(_, value)| value)
        .collect())
}

pub(crate) fn load_shared_coordination_ref_state(
    root: &Path,
) -> Result<Option<SharedCoordinationRefState>> {
    load_shared_coordination_ref_state_authoritative(root)
}

pub(crate) fn load_shared_coordination_ref_state_authoritative(
    root: &Path,
) -> Result<Option<SharedCoordinationRefState>> {
    if !git_repo_available(root) {
        return Ok(None);
    }
    let Some(current_head) = authoritative_shared_coordination_state_key(root)? else {
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
    let Some(state) = load_authoritative_shared_coordination_ref_state(root)? else {
        return Ok(None);
    };
    cache_shared_coordination_state(root, current_head, &state);
    Ok(Some(state))
}

fn load_authoritative_shared_coordination_ref_state(
    root: &Path,
) -> Result<Option<SharedCoordinationRefState>> {
    let ref_name = shared_coordination_ref_name(root);
    let summary_state = load_shared_coordination_ref_state_from_current_ref(root, &ref_name)?;
    if summary_state.is_none()
        && load_shared_coordination_runtime_refs(root)?.is_empty()
        && load_shared_coordination_task_shards(root)?.is_empty()
        && load_shared_coordination_claim_shards(root)?.is_empty()
    {
        return Ok(None);
    }
    summary_state
        .ok_or_else(|| {
            anyhow!(
            "shared coordination summary ref is missing while shard or runtime refs still exist"
        )
        })
        .map(Some)
}

pub(crate) fn load_shared_coordination_runtime_refs(root: &Path) -> Result<Vec<RuntimeDescriptor>> {
    let mut runtime_descriptors = load_shared_coordination_sharded_records(
        root,
        &shared_coordination_runtime_ref_prefix(root),
        |contents| {
            contents.parse_authoritative_records::<RuntimeDescriptor, _>(
                |path| path.starts_with("coordination/runtimes/"),
                SHARED_COORDINATION_KIND_RUNTIME_DESCRIPTOR,
            )
        },
    )?;
    runtime_descriptors.sort_by(|left, right| {
        left.worktree_id
            .cmp(&right.worktree_id)
            .then_with(|| left.runtime_id.cmp(&right.runtime_id))
    });
    Ok(runtime_descriptors)
}

fn load_shared_coordination_task_shards(root: &Path) -> Result<Vec<CoordinationTask>> {
    load_shared_coordination_sharded_records(
        root,
        &shared_coordination_task_ref_prefix(root),
        |contents| {
            contents.parse_authoritative_records::<CoordinationTask, _>(
                |path| path.starts_with("coordination/tasks/"),
                SHARED_COORDINATION_KIND_TASK,
            )
        },
    )
}

fn load_shared_coordination_claim_shards(root: &Path) -> Result<Vec<WorkClaim>> {
    load_shared_coordination_sharded_records(
        root,
        &shared_coordination_claim_ref_prefix(root),
        |contents| {
            contents.parse_authoritative_records::<WorkClaim, _>(
                |path| path.starts_with("coordination/claims/"),
                SHARED_COORDINATION_KIND_CLAIM,
            )
        },
    )
}

fn load_shared_coordination_sharded_records<T, F>(
    root: &Path,
    prefix: &str,
    parse: F,
) -> Result<Vec<T>>
where
    T: Clone,
    F: Fn(&SharedCoordinationRefContents) -> Result<Vec<(String, T)>>,
{
    let shard_refs = list_local_coordination_ref_heads(root, prefix)?;
    if shard_refs.is_empty() {
        return Ok(Vec::new());
    }
    let mut records = Vec::new();
    for ref_name in shard_refs.keys() {
        let Some(contents) = load_shared_coordination_ref_contents(root, ref_name)? else {
            continue;
        };
        let manifest = contents.parse_manifest()?;
        verify_shared_coordination_manifest(root, &manifest, &contents)?;
        records.extend(parse(&contents)?.into_iter().map(|(_, value)| value));
    }
    Ok(records)
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
    let manifest_digest = load_shared_coordination_manifest_from_ref(root, &ref_name)?
        .as_ref()
        .map(canonical_manifest_digest)
        .transpose()?;
    Ok(Some(CoordinationStartupCheckpointAuthority {
        ref_name,
        head_commit: Some(head_commit),
        manifest_digest,
    }))
}

pub(crate) fn load_shared_coordination_retained_history(
    root: &Path,
    limit: Option<u64>,
) -> Result<Vec<SharedCoordinationRetainedHistoryEntry>> {
    if !git_repo_available(root) {
        return Ok(Vec::new());
    }
    let ref_name = shared_coordination_ref_name(root);
    if resolve_ref_commit(root, &ref_name)?.is_none() {
        return Ok(Vec::new());
    }
    let mut args = vec!["rev-list".to_string()];
    if let Some(limit) = limit {
        args.push(format!("--max-count={limit}"));
    }
    args.push(ref_name);
    let arg_refs = args.iter().map(String::as_str).collect::<Vec<_>>();
    let output = run_git(root, &arg_refs)?;
    let mut entries = Vec::new();
    for commit in output
        .lines()
        .map(str::trim)
        .filter(|line| !line.is_empty())
    {
        let manifest = load_shared_coordination_manifest_from_ref(root, commit)?;
        let manifest_digest = manifest
            .as_ref()
            .map(canonical_manifest_digest)
            .transpose()?;
        let published_at = manifest.as_ref().map(|value| value.published_at);
        let previous_manifest_digest = manifest
            .as_ref()
            .and_then(|value| value.previous_manifest_digest.clone());
        let summary = manifest.as_ref().map_or_else(
            || "shared coordination commit".to_string(),
            |value| {
                format!(
                    "shared coordination publish at {} with {} files",
                    value.published_at,
                    value.files.len()
                )
            },
        );
        entries.push(SharedCoordinationRetainedHistoryEntry {
            head_commit: commit.to_string(),
            manifest_digest,
            published_at,
            previous_manifest_digest,
            summary,
        });
    }
    Ok(entries)
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
        .parse_plan_records()?
        .into_iter()
        .map(|(_, record)| record)
        .collect::<Vec<_>>();
    let tasks = contents
        .parse_authoritative_records::<CoordinationTask, _>(
            |path| path.starts_with("coordination/tasks/"),
            SHARED_COORDINATION_KIND_TASK,
        )?
        .into_iter()
        .map(|(_, task)| task)
        .collect::<Vec<_>>();
    let artifacts = contents
        .parse_authoritative_records::<Artifact, _>(
            |path| path.starts_with("coordination/artifacts/"),
            SHARED_COORDINATION_KIND_ARTIFACT,
        )?
        .into_iter()
        .map(|(_, artifact)| artifact)
        .collect::<Vec<_>>();
    let claims = contents
        .parse_authoritative_records::<WorkClaim, _>(
            |path| path.starts_with("coordination/claims/"),
            SHARED_COORDINATION_KIND_CLAIM,
        )?
        .into_iter()
        .map(|(_, claim)| claim)
        .collect::<Vec<_>>();
    let reviews = contents
        .parse_authoritative_records::<ArtifactReview, _>(
            |path| path.starts_with("coordination/reviews/"),
            SHARED_COORDINATION_KIND_REVIEW,
        )?
        .into_iter()
        .map(|(_, review)| review)
        .collect::<Vec<_>>();
    let mut runtime_descriptors = contents
        .parse_authoritative_records::<RuntimeDescriptor, _>(
            |path| path.starts_with("coordination/runtimes/"),
            SHARED_COORDINATION_KIND_RUNTIME_DESCRIPTOR,
        )?
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
    plans.sort_by(|left, right| left.id.0.cmp(&right.id.0));

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
    let canonical_snapshot_v2 = match contents.coordination_bytes("v2/snapshot.json") {
        Some(bytes) => {
            let snapshot_v2 = serde_json::from_slice::<CoordinationSnapshotV2>(&bytes)
                .context("failed to parse shared coordination ref file `v2/snapshot.json`")?;
            if snapshot_v2.schema_version > COORDINATION_SCHEMA_V2 {
                return Err(anyhow!(
                    "shared coordination ref v2 snapshot schema_version {} exceeds supported {}",
                    snapshot_v2.schema_version,
                    COORDINATION_SCHEMA_V2
                ));
            }
            snapshot_v2
        }
        None => {
            return Err(anyhow!(
                "shared coordination ref `{}` is missing canonical coordination snapshot `v2/snapshot.json`",
                ref_name
            ))
        }
    };
    let state = SharedCoordinationRefState {
        snapshot,
        canonical_snapshot_v2,
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

fn empty_shared_coordination_ref_state() -> SharedCoordinationRefState {
    let snapshot = CoordinationSnapshot {
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
    };
    SharedCoordinationRefState {
        canonical_snapshot_v2: snapshot.to_canonical_snapshot_v2(),
        snapshot,
        runtime_descriptors: Vec::new(),
    }
}

fn reconcile_shared_coordination_ref_state(
    baseline: Option<&SharedCoordinationRefState>,
    desired: &SharedCoordinationRefState,
    latest: Option<&SharedCoordinationRefState>,
) -> Result<SharedCoordinationRefState> {
    let baseline = baseline
        .cloned()
        .unwrap_or_else(empty_shared_coordination_ref_state);
    let latest = latest
        .cloned()
        .unwrap_or_else(empty_shared_coordination_ref_state);

    let baseline_plans = baseline
        .snapshot
        .plans
        .iter()
        .map(summary_plan_record)
        .collect::<Vec<_>>();
    let desired_plans = desired
        .snapshot
        .plans
        .iter()
        .map(summary_plan_record)
        .collect::<Vec<_>>();
    let latest_plans = latest
        .snapshot
        .plans
        .iter()
        .map(summary_plan_record)
        .collect::<Vec<_>>();
    let plans = reconcile_collection(
        &baseline_plans,
        &desired_plans,
        &latest_plans,
        |plan| plan.id.0.as_str(),
        "plan",
    )?;
    let tasks = reconcile_collection(
        &baseline.snapshot.tasks,
        &desired.snapshot.tasks,
        &latest.snapshot.tasks,
        |task| task.id.0.as_str(),
        "task",
    )?;
    let claims = reconcile_collection(
        &baseline.snapshot.claims,
        &desired.snapshot.claims,
        &latest.snapshot.claims,
        |claim| claim.id.0.as_str(),
        "claim",
    )?;
    let artifacts = reconcile_collection(
        &baseline.snapshot.artifacts,
        &desired.snapshot.artifacts,
        &latest.snapshot.artifacts,
        |artifact| artifact.id.0.as_str(),
        "artifact",
    )?;
    let reviews = reconcile_collection(
        &baseline.snapshot.reviews,
        &desired.snapshot.reviews,
        &latest.snapshot.reviews,
        |review| review.id.0.as_str(),
        "review",
    )?;
    let runtime_descriptors = reconcile_collection(
        &baseline.runtime_descriptors,
        &desired.runtime_descriptors,
        &latest.runtime_descriptors,
        |descriptor| descriptor.runtime_id.as_str(),
        "runtime descriptor",
    )?;

    let snapshot = CoordinationSnapshot {
        next_plan: desired
            .snapshot
            .next_plan
            .max(latest.snapshot.next_plan)
            .max(baseline.snapshot.next_plan),
        next_task: desired
            .snapshot
            .next_task
            .max(latest.snapshot.next_task)
            .max(baseline.snapshot.next_task),
        next_claim: desired
            .snapshot
            .next_claim
            .max(latest.snapshot.next_claim)
            .max(baseline.snapshot.next_claim),
        next_artifact: desired
            .snapshot
            .next_artifact
            .max(latest.snapshot.next_artifact)
            .max(baseline.snapshot.next_artifact),
        next_review: desired
            .snapshot
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
    let canonical_snapshot_v2 = snapshot.to_canonical_snapshot_v2();
    Ok(SharedCoordinationRefState {
        snapshot,
        canonical_snapshot_v2,
        runtime_descriptors,
    })
}

fn summary_plan_record(plan: &Plan) -> Plan {
    sanitize_plan(plan.clone())
}

fn summary_snapshot(snapshot: &CoordinationSnapshot) -> CoordinationSnapshot {
    let mut snapshot = snapshot.clone();
    snapshot.plans = snapshot.plans.iter().map(summary_plan_record).collect();
    snapshot
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

fn overlay_records<T, F>(baseline: &[T], desired: &[T], key_for: F) -> Vec<T>
where
    T: Clone,
    F: Fn(&T) -> &str,
{
    let mut by_id = baseline
        .iter()
        .cloned()
        .map(|value| (key_for(&value).to_string(), value))
        .collect::<BTreeMap<_, _>>();
    for value in desired {
        by_id.insert(key_for(value).to_string(), value.clone());
    }
    by_id.into_values().collect()
}

#[cfg_attr(not(test), allow(dead_code))]
pub(crate) fn shared_coordination_ref_exists(root: &Path) -> Result<bool> {
    if !git_repo_available(root) {
        return Ok(false);
    }
    Ok(authoritative_shared_coordination_state_key(root)?.is_some())
}

pub fn shared_coordination_ref_diagnostics(
    root: &Path,
) -> Result<Option<SharedCoordinationRefDiagnostics>> {
    if !git_repo_available(root) {
        return Ok(None);
    }
    let ref_name = shared_coordination_ref_name(root);
    let (verified_state, verification_status, authoritative_hydration_allowed, degraded, verification_error, repair_hint) =
        match load_shared_coordination_ref_state_authoritative(root) {
            Ok(state) => (
                state,
                "verified".to_string(),
                true,
                false,
                None,
                None,
            ),
            Err(error) => (
                None,
                "degraded".to_string(),
                false,
                true,
                Some(error.to_string()),
                Some(
                    "Repair or republish the shared coordination ref before relying on authoritative shared-ref hydration."
                        .to_string(),
                ),
            ),
        };
    let head_commit = resolve_ref_commit(root, &ref_name)?;
    if head_commit.is_none() && verified_state.is_none() {
        return Ok(None);
    }
    let history_depth = if head_commit.is_some() {
        ref_history_depth(root, &ref_name)?
    } else {
        0
    };
    let compacted_head = if head_commit.is_some() {
        ref_head_has_no_parent(root, &ref_name)?
    } else {
        false
    };
    let manifest = if head_commit.is_some() {
        load_shared_coordination_manifest_from_ref(root, &ref_name)?
    } else {
        None
    };
    let summary_read_state = verified_state.clone();
    let authoritative_heads = current_shared_coordination_authoritative_heads(root)?;
    let freshness = inspect_shared_coordination_summary_freshness(
        manifest.as_ref(),
        summary_read_state.as_ref(),
        &authoritative_heads,
    );
    let current_manifest_digest = manifest
        .as_ref()
        .map(canonical_manifest_digest)
        .transpose()?;
    let last_verified_manifest_digest = if authoritative_hydration_allowed {
        current_manifest_digest.clone()
    } else {
        None
    };
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
    let snapshot_file_count = if head_commit.is_some() {
        list_ref_json_paths(root, &ref_name)?.len()
    } else {
        0
    };
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
        head_commit,
        history_depth,
        max_history_commits: SHARED_COORDINATION_HISTORY_MAX_COMMITS,
        snapshot_file_count,
        verification_status,
        authoritative_hydration_allowed,
        degraded,
        verification_error,
        repair_hint,
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
        summary_published_at: freshness.summary_published_at,
        summary_freshness_status: freshness.status.as_str().to_string(),
        authoritative_fallback_required: freshness.authoritative_fallback_required,
        freshness_reason: freshness.reason,
        lagging_task_shard_refs: freshness.lagging_task_shard_refs,
        lagging_claim_shard_refs: freshness.lagging_claim_shard_refs,
        lagging_runtime_refs: freshness.lagging_runtime_refs,
        newest_authoritative_ref_at: freshness.newest_authoritative_ref_at,
        runtime_descriptor_count: runtime_descriptors.len(),
        runtime_descriptors,
    }))
}

pub fn shared_coordination_ref_status_summary(
    root: &Path,
) -> Result<Option<SharedCoordinationRefStatusSummary>> {
    if !git_repo_available(root) {
        return Ok(None);
    }
    let ref_name = shared_coordination_ref_name(root);
    let head_commit = resolve_ref_commit(root, &ref_name)?;
    if head_commit.is_none() {
        return Ok(None);
    }
    let history_depth = ref_history_depth(root, &ref_name)?;
    let compacted_head = ref_head_has_no_parent(root, &ref_name)?;
    let snapshot_file_count = list_ref_json_paths(root, &ref_name)?.len();
    let needs_compaction = history_depth > SHARED_COORDINATION_HISTORY_MAX_COMMITS;
    let compaction_status = if compacted_head {
        "compacted"
    } else if needs_compaction {
        "compaction_recommended"
    } else {
        "healthy"
    };
    Ok(Some(SharedCoordinationRefStatusSummary {
        ref_name,
        head_commit,
        history_depth,
        snapshot_file_count,
        needs_compaction,
        compaction_status: compaction_status.to_string(),
    }))
}

fn sync_plan_objects(stage_dir: &Path, snapshot: &CoordinationSnapshot) -> Result<()> {
    let mut expected = BTreeSet::new();
    for plan in &snapshot.plans {
        let path = plan_snapshot_path(stage_dir, &plan.id.0);
        expected.insert(path.clone());
        let record = SharedCoordinationPlanRecord {
            plan: summary_plan_record(plan),
        };
        write_json_file(
            &path,
            &wrap_authoritative_payload(&record, SHARED_COORDINATION_KIND_PLAN_RECORD)?,
        )?;
    }
    cleanup_directory_json_files(&stage_plans_dir(stage_dir), &expected)
}

fn sync_task_objects(stage_dir: &Path, tasks: &[CoordinationTask]) -> Result<()> {
    let mut expected = BTreeSet::new();
    for task in tasks {
        let path = task_snapshot_path(stage_dir, &task.id.0);
        expected.insert(path.clone());
        write_json_file(
            &path,
            &wrap_authoritative_payload(task, SHARED_COORDINATION_KIND_TASK)?,
        )?;
    }
    cleanup_directory_json_files(&stage_tasks_dir(stage_dir), &expected)
}

fn sync_v2_snapshot(stage_dir: &Path, snapshot: &CoordinationSnapshotV2) -> Result<()> {
    let path = v2_snapshot_path(stage_dir);
    let expected = BTreeSet::from([path.clone()]);
    write_json_file(&path, snapshot)?;
    cleanup_directory_json_files(&stage_v2_dir(stage_dir), &expected)
}

fn sync_artifact_objects(stage_dir: &Path, artifacts: &[Artifact]) -> Result<()> {
    let mut expected = BTreeSet::new();
    for artifact in artifacts {
        let path = artifact_snapshot_path(stage_dir, &artifact.id.0);
        expected.insert(path.clone());
        write_json_file(
            &path,
            &wrap_authoritative_payload(artifact, SHARED_COORDINATION_KIND_ARTIFACT)?,
        )?;
    }
    cleanup_directory_json_files(&stage_artifacts_dir(stage_dir), &expected)
}

fn sync_claim_objects(stage_dir: &Path, claims: &[WorkClaim]) -> Result<()> {
    let mut expected = BTreeSet::new();
    for claim in claims {
        let path = claim_snapshot_path(stage_dir, &claim.id.0);
        expected.insert(path.clone());
        write_json_file(
            &path,
            &wrap_authoritative_payload(claim, SHARED_COORDINATION_KIND_CLAIM)?,
        )?;
    }
    cleanup_directory_json_files(&stage_claims_dir(stage_dir), &expected)
}

fn sync_review_objects(stage_dir: &Path, reviews: &[ArtifactReview]) -> Result<()> {
    let mut expected = BTreeSet::new();
    for review in reviews {
        let path = review_snapshot_path(stage_dir, &review.id.0);
        expected.insert(path.clone());
        write_json_file(
            &path,
            &wrap_authoritative_payload(review, SHARED_COORDINATION_KIND_REVIEW)?,
        )?;
    }
    cleanup_directory_json_files(&stage_reviews_dir(stage_dir), &expected)
}

fn sync_runtime_descriptor_objects(
    stage_dir: &Path,
    descriptors: &[RuntimeDescriptor],
) -> Result<()> {
    let mut expected = BTreeSet::new();
    for descriptor in descriptors {
        let path = runtime_descriptor_snapshot_path(stage_dir, &descriptor.runtime_id);
        expected.insert(path.clone());
        write_json_file(
            &path,
            &wrap_authoritative_payload(descriptor, SHARED_COORDINATION_KIND_RUNTIME_DESCRIPTOR)?,
        )?;
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
            let path = runtime_descriptor_snapshot_path(stage_dir, &descriptor.runtime_id);
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

fn local_runtime_descriptor(
    root: &Path,
    publish: Option<&TrackedSnapshotPublishContext>,
    existing: Option<&[RuntimeDescriptor]>,
) -> Result<RuntimeDescriptor> {
    let identity = PrismPaths::for_workspace_root(root)?.identity().clone();
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
        .find(|descriptor| descriptor.runtime_id == identity.instance_id)
        .or_else(|| {
            existing
                .unwrap_or(&[])
                .iter()
                .filter(|descriptor| descriptor.worktree_id == identity.worktree_id)
                .max_by_key(|descriptor| descriptor.last_seen_at)
        });
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
    summary_sources: Option<&SharedCoordinationSummarySourceHeads>,
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
        schema_version: SHARED_COORDINATION_SCHEMA_VERSION,
        kind: shared_coordination_manifest_kind(),
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
        summary_sources: summary_sources
            .cloned()
            .filter(|sources| !sources.is_empty()),
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
    let signature =
        active_key
            .signing_key
            .sign(&canonical_shared_coordination_manifest_signing_bytes(
                &manifest,
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
        Ok(contents) => Ok(Some(parse_top_level_authoritative_payload(
            contents.trim().as_bytes(),
            "coordination/manifest.json",
            SHARED_COORDINATION_KIND_MANIFEST,
        )?)),
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

fn verify_shared_coordination_manifest(
    root: &Path,
    manifest: &SharedCoordinationManifest,
    contents: &SharedCoordinationRefContents,
) -> Result<()> {
    if manifest.schema_version > SHARED_COORDINATION_SCHEMA_VERSION {
        return Err(anyhow!(
            "shared coordination payload `coordination/manifest.json` requires schema_version {} for kind `{}`, but this PRISM supports up to {}. Upgrade PRISM and retry.",
            manifest.schema_version,
            manifest.kind,
            SHARED_COORDINATION_SCHEMA_VERSION,
        ));
    }
    if manifest.kind != SHARED_COORDINATION_KIND_MANIFEST {
        return Err(anyhow!(
            "shared coordination payload `coordination/manifest.json` declared kind `{}`, expected `{}`",
            manifest.kind,
            SHARED_COORDINATION_KIND_MANIFEST,
        ));
    }
    let paths = PrismPaths::for_workspace_root(root)?;
    let trusted = resolve_trusted_runtime_key(
        &paths,
        &manifest.signature.trust_bundle_id,
        &manifest.signature.runtime_authority_id,
        &manifest.signature.runtime_key_id,
    )?;
    let signature = decode_signature(&manifest.signature.value)?;
    match trusted.verifying_key.verify(
        &canonical_shared_coordination_manifest_signing_bytes(manifest)?,
        &signature,
    ) {
        Ok(()) => {}
        Err(current_error) if manifest_uses_legacy_signature_shape(contents) => {
            trusted
                .verifying_key
                .verify(
                    &canonical_legacy_shared_coordination_manifest_signing_bytes(manifest)?,
                    &signature,
                )
                .map_err(|legacy_error| {
                    anyhow!(
                        "shared coordination manifest signature verification failed for both current and legacy compatibility payloads: current: {current_error}; legacy: {legacy_error}"
                    )
                })?;
        }
        Err(error) => {
            return Err(anyhow!(
                "shared coordination manifest signature verification failed: {error}"
            ));
        }
    }
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

fn canonical_shared_coordination_manifest_signing_bytes(
    manifest: &SharedCoordinationManifest,
) -> Result<Vec<u8>> {
    canonical_json_bytes(&SharedCoordinationManifestSigningView {
        schema_version: manifest.schema_version,
        kind: manifest.kind.as_str(),
        published_at: manifest.published_at,
        publisher: &manifest.publisher,
        work_context: &manifest.work_context,
        publish_summary: &manifest.publish_summary,
        files: &manifest.files,
        previous_manifest_digest: &manifest.previous_manifest_digest,
        summary_sources: &manifest.summary_sources,
        publish_diagnostics: &manifest.publish_diagnostics,
        compaction: &manifest.compaction,
        signature: SharedCoordinationManifestSignatureMetadata {
            algorithm: manifest.signature.algorithm,
            runtime_authority_id: &manifest.signature.runtime_authority_id,
            runtime_key_id: &manifest.signature.runtime_key_id,
            trust_bundle_id: &manifest.signature.trust_bundle_id,
        },
    })
}

fn canonical_legacy_shared_coordination_manifest_signing_bytes(
    manifest: &SharedCoordinationManifest,
) -> Result<Vec<u8>> {
    canonical_json_bytes(&LegacySharedCoordinationManifestSigningView {
        version: manifest.schema_version,
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
    })
}

fn manifest_uses_legacy_signature_shape(contents: &SharedCoordinationRefContents) -> bool {
    let Some(bytes) = contents.coordination_bytes("manifest.json") else {
        return false;
    };
    let Ok(value) = serde_json::from_slice::<serde_json::Value>(&bytes) else {
        return false;
    };
    let Some(object) = value.as_object() else {
        return false;
    };
    object.contains_key("version")
        && !object.contains_key("kind")
        && !object.contains_key("schema_version")
        && !object.contains_key("schemaVersion")
}

#[cfg(test)]
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

fn publish_stage_patch_to_ref(
    root: &Path,
    stage_dir: &Path,
    ref_name: &str,
    patch: &SharedCoordinationPublishPatch,
) -> Result<()> {
    let tree = write_stage_tree_patch(root, stage_dir, ref_name, patch)?;
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

fn create_shared_coordination_commit_from_stage_patch(
    root: &Path,
    stage_dir: &Path,
    ref_name: &str,
    patch: &SharedCoordinationPublishPatch,
) -> Result<String> {
    let tree = write_stage_tree_patch(root, stage_dir, ref_name, patch)?;
    let parent = resolve_ref_commit(root, ref_name)?;
    let message = if parent.is_some() {
        "prism: update shared coordination ref"
    } else {
        "prism: initialize shared coordination ref"
    };
    create_tree_commit(root, tree.trim(), parent.as_deref(), message)
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

fn write_stage_tree_patch(
    root: &Path,
    stage_dir: &Path,
    ref_name: &str,
    patch: &SharedCoordinationPublishPatch,
) -> Result<String> {
    let index_path = stage_dir.join(".shared-coordination.index");
    let index_path_str = index_path.to_string_lossy().to_string();
    let envs = [("GIT_INDEX_FILE", index_path_str.as_str())];
    if let Some(parent) = resolve_ref_commit(root, ref_name)? {
        let _ = run_git_with_env(root, &envs, &["read-tree", parent.as_str()])?;
    } else {
        let _ = run_git_with_env(root, &envs, &["read-tree", "--empty"])?;
    }
    for delete in &patch.deletes {
        let _ = run_git_with_env(
            root,
            &envs,
            &["rm", "--cached", "--ignore-unmatch", "--", delete.as_str()],
        )?;
    }
    if !patch.upserts.is_empty() {
        let mut args = vec![
            "--work-tree".to_string(),
            stage_dir.to_string_lossy().to_string(),
            "add".to_string(),
            "--".to_string(),
        ];
        args.extend(patch.upserts.iter().cloned());
        let arg_refs = args.iter().map(String::as_str).collect::<Vec<_>>();
        let _ = run_git_with_env(root, &envs, &arg_refs)?;
    }
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

fn push_shared_coordination_ref_updates_atomic(
    root: &Path,
    remote: &str,
    updates: &[PreparedSharedCoordinationRefUpdate],
) -> Result<()> {
    if updates.is_empty() {
        return Ok(());
    }
    if !git_remote_available(root, remote) {
        return apply_local_shared_coordination_ref_updates_atomic(root, updates);
    }
    let mut args = vec![
        "push".to_string(),
        "--porcelain".to_string(),
        "--atomic".to_string(),
    ];
    args.push(remote.to_string());
    for update in updates {
        args.push(format!("{}:{}", update.new_commit, update.ref_name));
    }
    let arg_refs = args.iter().map(String::as_str).collect::<Vec<_>>();
    let _ = run_git(root, &arg_refs)?;
    Ok(())
}

fn apply_local_shared_coordination_ref_updates_atomic(
    root: &Path,
    updates: &[PreparedSharedCoordinationRefUpdate],
) -> Result<()> {
    let mut command = Command::new("git");
    command
        .current_dir(root)
        .env_remove("GIT_DIR")
        .env_remove("GIT_WORK_TREE")
        .env_remove("GIT_COMMON_DIR")
        .env_remove("GIT_INDEX_FILE")
        .env_remove("GIT_OBJECT_DIRECTORY")
        .env_remove("GIT_ALTERNATE_OBJECT_DIRECTORIES")
        .env("GIT_AUTHOR_NAME", "PRISM")
        .env("GIT_AUTHOR_EMAIL", "prism@local")
        .env("GIT_COMMITTER_NAME", "PRISM")
        .env("GIT_COMMITTER_EMAIL", "prism@local")
        .args(["update-ref", "--stdin"])
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());
    let mut child = command.spawn().context(
        "failed to spawn `git update-ref --stdin` for local shared coordination publish",
    )?;
    {
        let mut stdin = child
            .stdin
            .take()
            .context("failed to open stdin for `git update-ref --stdin`")?;
        writeln!(stdin, "start")?;
        for update in updates {
            if let Some(old_commit) = resolve_ref_commit(root, &update.ref_name)? {
                writeln!(
                    stdin,
                    "update {} {} {}",
                    update.ref_name, update.new_commit, old_commit
                )?;
            } else {
                writeln!(stdin, "create {} {}", update.ref_name, update.new_commit)?;
            }
        }
        writeln!(stdin, "prepare")?;
        writeln!(stdin, "commit")?;
    }
    let output = child
        .wait_with_output()
        .context("failed to wait for local shared coordination ref update transaction")?;
    if output.status.success() {
        return Ok(());
    }
    Err(anyhow!(
        "git update-ref --stdin failed: {}",
        String::from_utf8_lossy(&output.stderr)
    ))
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
            previous_manifest.summary_sources.as_ref(),
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

#[derive(Debug, Clone, PartialEq, Eq)]
struct SharedCoordinationPublishPatch {
    upserts: BTreeSet<String>,
    deletes: BTreeSet<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct PreparedSharedCoordinationRefUpdate {
    ref_name: String,
    new_commit: String,
}

fn is_authoritative_summary_only_path(path: &str) -> bool {
    matches!(
        path,
        "coordination/manifest.json"
            | "coordination/indexes/tasks.json"
            | "coordination/indexes/claims.json"
            | "coordination/indexes/runtimes.json"
    ) || path.starts_with("coordination/coordination/tasks/")
        || path.starts_with("coordination/coordination/claims/")
        || path.starts_with("coordination/coordination/runtimes/")
}

#[cfg(test)]
fn build_shared_coordination_publish_patch(
    _root: &Path,
    stage_dir: &Path,
    previous_manifest: Option<&SharedCoordinationManifest>,
    _baseline_state: Option<&SharedCoordinationRefState>,
    desired_state: &SharedCoordinationRefState,
) -> Result<SharedCoordinationPublishPatch> {
    let _ = fs::remove_dir_all(stage_dir);
    fs::create_dir_all(stage_dir)?;
    sync_plan_objects(stage_dir, &desired_state.snapshot)?;
    sync_v2_snapshot(stage_dir, &desired_state.canonical_snapshot_v2)?;
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
    build_shared_coordination_publish_patch_from_stage(stage_dir, previous_manifest, true)
}

fn build_shared_coordination_publish_patch_from_stage(
    stage_dir: &Path,
    previous_manifest: Option<&SharedCoordinationManifest>,
    suppress_authoritative_only: bool,
) -> Result<SharedCoordinationPublishPatch> {
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
    if suppress_authoritative_only
        && upserts
            .iter()
            .chain(deletes.iter())
            .all(|path| is_authoritative_summary_only_path(path))
    {
        return Ok(SharedCoordinationPublishPatch {
            upserts: BTreeSet::new(),
            deletes: BTreeSet::new(),
        });
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
        parse_top_level_authoritative_payload(
            bytes,
            "coordination/manifest.json",
            SHARED_COORDINATION_KIND_MANIFEST,
        )
    }

    fn coordination_bytes(&self, relative_path: &str) -> Option<Vec<u8>> {
        self.files.get(relative_path).cloned()
    }

    fn parse_authoritative_records<T, F>(&self, filter: F, kind: &str) -> Result<Vec<(String, T)>>
    where
        T: for<'de> Deserialize<'de>,
        F: Fn(&str) -> bool,
    {
        let mut records = self
            .files
            .iter()
            .filter(|(path, _)| filter(path.as_str()))
            .map(|(path, bytes)| {
                let value = parse_authoritative_payload(bytes, path, kind)?;
                Ok((path.clone(), value))
            })
            .collect::<Result<Vec<_>>>()?;
        records.sort_by(|left, right| left.0.cmp(&right.0));
        Ok(records)
    }

    fn parse_plan_records(&self) -> Result<Vec<(String, SharedCoordinationPlanRecord)>> {
        let mut records = self
            .files
            .iter()
            .filter(|(path, _)| path.starts_with("plans/"))
            .map(|(path, bytes)| {
                let value = parse_shared_coordination_plan_record(bytes, path)?;
                Ok((path.clone(), value))
            })
            .collect::<Result<Vec<_>>>()?;
        records.sort_by(|left, right| left.0.cmp(&right.0));
        Ok(records)
    }
}

fn parse_shared_coordination_plan_record(
    bytes: &[u8],
    path: &str,
) -> Result<SharedCoordinationPlanRecord> {
    parse_authoritative_payload(bytes, path, SHARED_COORDINATION_KIND_PLAN_RECORD).or_else(
        |primary_error| {
            let value: serde_json::Value = serde_json::from_slice(bytes).with_context(|| {
                format!("failed to parse shared coordination ref file `{path}`")
            })?;
            let payload = value.get("payload").cloned().unwrap_or(value);
            let plan_value = payload.get("plan").cloned().unwrap_or(payload);
            let plan = serde_json::from_value(plan_value).with_context(|| {
                format!("failed to decode shared coordination payload `{path}`")
            })?;
            let _ = primary_error;
            Ok(SharedCoordinationPlanRecord { plan })
        },
    )
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
    let canonical_root = root.canonicalize().unwrap_or_else(|_| root.to_path_buf());
    let now_ms = current_timestamp_millis();
    let cache = GIT_REPO_AVAILABLE_CACHE.get_or_init(|| Mutex::new(HashMap::new()));
    if let Some(entry) = cache
        .lock()
        .expect("git repo availability cache lock poisoned")
        .get(&canonical_root)
        .copied()
    {
        if now_ms.saturating_sub(entry.checked_at_ms) < GIT_REPO_AVAILABLE_CACHE_TTL_MS {
            return entry.available;
        }
    }
    let available = run_git(&canonical_root, &["rev-parse", "--git-dir"]).is_ok();
    cache
        .lock()
        .expect("git repo availability cache lock poisoned")
        .insert(
            canonical_root,
            GitRepoAvailableCacheEntry {
                available,
                checked_at_ms: now_ms,
            },
        );
    available
}

fn run_git_with_env(root: &Path, envs: &[(&str, &str)], args: &[&str]) -> Result<String> {
    let mut command = Command::new("git");
    command
        .current_dir(root)
        .env_remove("GIT_DIR")
        .env_remove("GIT_WORK_TREE")
        .env_remove("GIT_COMMON_DIR")
        .env_remove("GIT_INDEX_FILE")
        .env_remove("GIT_OBJECT_DIRECTORY")
        .env_remove("GIT_ALTERNATE_OBJECT_DIRECTORIES")
        .env("GIT_AUTHOR_NAME", "PRISM")
        .env("GIT_AUTHOR_EMAIL", "prism@local")
        .env("GIT_COMMITTER_NAME", "PRISM")
        .env("GIT_COMMITTER_EMAIL", "prism@local");
    for key in [
        "GIT_DIR",
        "GIT_WORK_TREE",
        "GIT_COMMON_DIR",
        "GIT_INDEX_FILE",
        "GIT_OBJECT_DIRECTORY",
        "GIT_ALTERNATE_OBJECT_DIRECTORIES",
        "GIT_CEILING_DIRECTORIES",
    ] {
        command.env_remove(key);
    }
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

#[cfg(test)]
mod tests {
    use std::cell::RefCell;
    use std::collections::BTreeSet;
    use std::fs;
    use std::path::{Path, PathBuf};
    use std::sync::atomic::{AtomicU64, Ordering};
    use std::sync::OnceLock;
    use std::time::{SystemTime, UNIX_EPOCH};

    use base64::Engine;
    use prism_coordination::{
        CoordinationPolicy, CoordinationSnapshot, CoordinationTask, Plan, PlanScheduling,
        RuntimeDescriptor, RuntimeDescriptorCapability, RuntimeDiscoveryMode, TaskGitExecution,
        WorkClaim,
    };
    use prism_ir::{
        ClaimId, ClaimMode, ClaimStatus, CoordinationTaskId, CoordinationTaskStatus, EventActor,
        EventId, EventMeta, PlanId, PlanKind, PlanScope, PlanStatus, SessionId, TaskId,
        WorkspaceRevision,
    };
    use prism_store::{CoordinationCheckpointStore, CoordinationStartupCheckpoint, MemoryStore};

    use super::{
        implicit_principal_identity, initialize_shared_coordination_ref_live_sync,
        load_shared_coordination_ref_state, poll_shared_coordination_ref_live_sync,
        shared_coordination_ref_diagnostics, shared_coordination_ref_exists,
        sync_live_runtime_descriptor, SharedCoordinationRefLiveSync,
    };
    use crate::coordination_startup_checkpoint::load_persisted_coordination_plan_state;
    use crate::index_workspace_session;
    use crate::published_plans::load_authoritative_coordination_plan_state;
    use crate::tracked_snapshot::TrackedSnapshotPublishContext;
    use crate::util::current_timestamp;
    use crate::workspace_identity::workspace_identity_for_root;
    use crate::{CoordinationReadConsistency, CoordinationReadFreshness};

    static NEXT_TEMP_REPO: AtomicU64 = AtomicU64::new(0);
    static SHARED_COORDINATION_GIT_TEMPLATE: OnceLock<PathBuf> = OnceLock::new();

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
        track_temp_dir(&root);
        super::run_git(
            std::env::temp_dir().as_path(),
            &[
                "clone",
                "--local",
                "--quiet",
                shared_coordination_git_template()
                    .to_string_lossy()
                    .as_ref(),
                root.to_string_lossy().as_ref(),
            ],
        )
        .unwrap();
        let _ = fs::remove_dir_all(
            root.join(".git")
                .join("refs")
                .join("remotes")
                .join("origin"),
        );
        root
    }

    fn temp_git_repo_with_origin() -> (PathBuf, PathBuf) {
        let remote = temp_dir_path("prism-shared-coord-remote", "git");
        let _ = fs::remove_dir_all(&remote);
        track_temp_dir(&remote);
        super::run_git(
            std::env::temp_dir().as_path(),
            &[
                "clone",
                "--bare",
                "--quiet",
                shared_coordination_git_template()
                    .to_string_lossy()
                    .as_ref(),
                remote.to_string_lossy().as_ref(),
            ],
        )
        .unwrap();

        let root = temp_git_repo();
        super::run_git(
            &root,
            &[
                "remote",
                "set-url",
                super::shared_coordination_remote_name(),
                remote.to_string_lossy().as_ref(),
            ],
        )
        .or_else(|_| {
            super::run_git(
                &root,
                &[
                    "remote",
                    "add",
                    super::shared_coordination_remote_name(),
                    remote.to_string_lossy().as_ref(),
                ],
            )
        })
        .unwrap();
        (root, remote)
    }

    fn temp_stage_dir(label: &str) -> PathBuf {
        let root = temp_dir_path(&format!("prism-shared-coord-stage-{label}"), "");
        let _ = fs::remove_dir_all(&root);
        fs::create_dir_all(&root).unwrap();
        track_temp_dir(&root);
        root
    }

    fn tamper_shared_coordination_manifest<F>(root: &Path, mutate: F)
    where
        F: FnOnce(&mut super::SharedCoordinationManifest),
    {
        let ref_name = super::shared_coordination_ref_name(root);
        let contents = super::load_shared_coordination_ref_contents(root, &ref_name)
            .unwrap()
            .expect("shared coordination contents should exist");
        let stage_dir = temp_stage_dir("tamper");
        for (relative_path, bytes) in &contents.files {
            let path = stage_dir.join("coordination").join(relative_path);
            fs::create_dir_all(path.parent().unwrap()).unwrap();
            fs::write(path, bytes).unwrap();
        }
        let mut manifest = serde_json::from_slice::<super::SharedCoordinationManifest>(
            contents.files.get("manifest.json").unwrap(),
        )
        .unwrap();
        mutate(&mut manifest);
        super::write_json_file(&super::stage_manifest_path(&stage_dir), &manifest).unwrap();
        super::publish_stage_to_ref(root, &stage_dir, &ref_name).unwrap();
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
        let root = temp_dir_path("prism-shared-coord-worktree", "");
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

    fn shared_coordination_git_template() -> &'static PathBuf {
        SHARED_COORDINATION_GIT_TEMPLATE.get_or_init(|| {
            let root = temp_dir_path("prism-shared-coord-template", "");
            let _ = fs::remove_dir_all(&root);
            fs::create_dir_all(&root).unwrap();
            track_temp_dir(&root);
            fs::create_dir_all(root.join(".prism")).unwrap();
            super::run_git(&root, &["init", "-b", "main"]).unwrap();
            fs::write(root.join("README.md"), "# test\n").unwrap();
            super::run_git(&root, &["add", "README.md"]).unwrap();
            super::run_git(&root, &["commit", "-m", "init"]).unwrap();
            root
        })
    }

    fn temp_dir_path(label: &str, extension: &str) -> PathBuf {
        let nonce = NEXT_TEMP_REPO.fetch_add(1, Ordering::Relaxed);
        let unique = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        let path = std::env::temp_dir().join(format!("{label}-{unique}-{nonce}"));
        if extension.is_empty() {
            path
        } else {
            path.with_extension(extension)
        }
    }

    fn sample_publish_context() -> TrackedSnapshotPublishContext {
        TrackedSnapshotPublishContext {
            published_at: current_timestamp(),
            principal: implicit_principal_identity(None, None),
            work_context: Some(super::implicit_work_context()),
            publish_summary: None,
        }
    }

    fn sync_shared_coordination_ref_state(
        root: &Path,
        snapshot: &CoordinationSnapshot,
        publish: Option<&TrackedSnapshotPublishContext>,
    ) -> anyhow::Result<()> {
        super::sync_shared_coordination_ref_state(
            root,
            snapshot,
            &snapshot.to_canonical_snapshot_v2(),
            publish,
        )
    }

    fn save_shared_coordination_startup_checkpoint<S>(
        root: &Path,
        store: &mut S,
        snapshot: &CoordinationSnapshot,
        runtime_descriptors: Option<&[RuntimeDescriptor]>,
    ) -> anyhow::Result<()>
    where
        S: CoordinationCheckpointStore + prism_store::CoordinationJournal + ?Sized,
    {
        crate::coordination_startup_checkpoint::save_shared_coordination_startup_checkpoint(
            root,
            store,
            snapshot,
            &snapshot.to_canonical_snapshot_v2(),
            runtime_descriptors,
        )
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

    fn rewrite_shared_coordination_manifest_as_legacy_signed_payload(root: &Path) {
        let ref_name = super::shared_coordination_ref_name(root);
        let contents = super::load_shared_coordination_ref_contents(root, &ref_name)
            .unwrap()
            .expect("shared coordination contents should exist");
        let stage_dir = temp_stage_dir("legacy-manifest");
        for (relative_path, bytes) in &contents.files {
            let path = stage_dir.join("coordination").join(relative_path);
            fs::create_dir_all(path.parent().unwrap()).unwrap();
            fs::write(path, bytes).unwrap();
        }
        let manifest: super::SharedCoordinationManifest =
            serde_json::from_slice(contents.files.get("manifest.json").unwrap()).unwrap();
        let paths = crate::PrismPaths::for_workspace_root(root).unwrap();
        let active_key = super::load_active_runtime_signing_key(&paths).unwrap();
        let signature = ed25519_dalek::Signer::sign(
            &active_key.signing_key,
            &super::canonical_legacy_shared_coordination_manifest_signing_bytes(&manifest).unwrap(),
        );
        let mut manifest_value = serde_json::to_value(&manifest).unwrap();
        let object = manifest_value
            .as_object_mut()
            .expect("shared coordination manifest object");
        object.remove("schema_version");
        object.remove("kind");
        object.insert(
            "version".to_string(),
            serde_json::json!(manifest.schema_version),
        );
        object.get_mut("signature").expect("manifest signature")["value"] =
            serde_json::Value::String(format!(
                "base64:{}",
                super::BASE64_STANDARD.encode(signature.to_bytes())
            ));
        super::write_json_file(
            &stage_dir.join("coordination").join("manifest.json"),
            &manifest_value,
        )
        .unwrap();
        super::publish_stage_to_ref(root, &stage_dir, &ref_name).unwrap();
    }

    fn sample_snapshot_for(plan_id: &str, task_id: &str) -> CoordinationSnapshot {
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
        CoordinationSnapshot {
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
        }
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
        sync_shared_coordination_ref_state(&root, &snapshot, Some(&sample_publish_context()))
            .unwrap();
        assert!(shared_coordination_ref_exists(&root).unwrap());
        let ref_name = super::shared_coordination_ref_name(&root);
        let contents = super::load_shared_coordination_ref_contents(&root, &ref_name)
            .unwrap()
            .expect("shared ref contents should load");
        let plan_record_path = format!("plans/{}", super::snapshot_file_name(&plan_id.0));
        let plan_record: serde_json::Value = serde_json::from_slice(
            contents
                .files
                .get(plan_record_path.as_str())
                .expect("shared plan record"),
        )
        .unwrap();
        let payload = plan_record
            .get("payload")
            .and_then(serde_json::Value::as_object)
            .expect("wrapped plan payload");
        assert!(!payload.contains_key("graph"));
        assert!(!payload.contains_key("executionOverlays"));
        assert!(payload.get("root_tasks").is_none());
        let loaded = load_shared_coordination_ref_state(&root)
            .unwrap()
            .expect("shared ref state should load");
        assert_eq!(loaded.snapshot.tasks, vec![task]);
        assert_eq!(loaded.snapshot.claims, vec![claim]);
        assert_eq!(
            loaded.snapshot.plans,
            vec![super::summary_plan_record(&plan)]
        );
        assert_eq!(
            loaded.canonical_snapshot_v2,
            snapshot.to_canonical_snapshot_v2()
        );
    }

    #[test]
    fn shared_coordination_plan_record_parser_accepts_direct_plan_payloads() {
        let bytes = serde_json::to_vec(&serde_json::json!({
            "schema_version": 1,
            "kind": "coordination_plan_record",
            "payload": {
                "id": "plan:compat",
                "goal": "Compatibility fallback",
                "title": "Compatibility fallback",
                "status": "Active",
                "policy": {
                    "default_claim_mode": "Advisory",
                    "max_parallel_editors_per_anchor": 2,
                    "require_review_for_completion": false,
                    "require_validation_for_completion": false,
                    "stale_after_graph_change": true,
                    "review_required_above_risk_score": null,
                    "lease_stale_after_seconds": 1800,
                    "lease_expires_after_seconds": 7200,
                    "lease_renewal_mode": "strict",
                    "git_execution": {
                        "startMode": "off",
                        "completionMode": "off",
                        "targetRef": null,
                        "targetBranch": "",
                        "requireTaskBranch": false,
                        "maxCommitsBehindTarget": 0,
                        "maxFetchAgeSeconds": null,
                        "integrationMode": "external"
                    }
                },
                "scope": "Repo",
                "kind": "TaskExecution",
                "revision": 0,
                "scheduling": {
                    "importance": 0,
                    "urgency": 0,
                    "manualBoost": 0,
                    "dueAt": null
                },
                "tags": [],
                "created_from": null,
                "metadata": null
            }
        }))
        .unwrap();

        let record = super::parse_shared_coordination_plan_record(&bytes, "plans/plan-compat.json")
            .expect("plan record fallback should succeed");

        assert_eq!(record.plan.id.0, "plan:compat");
        assert_eq!(record.plan.title, "Compatibility fallback");
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
        sync_shared_coordination_ref_state(
            &root,
            &shared_snapshot,
            Some(&sample_publish_context()),
        )
        .unwrap();

        task.git_execution = TaskGitExecution::default();
        task.session = None;
        task.worktree_id = None;
        task.branch_ref = None;
        let loaded = load_authoritative_coordination_plan_state(&root)
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
        assert_eq!(
            loaded.canonical_snapshot_v2,
            shared_snapshot.to_canonical_snapshot_v2()
        );
    }

    #[test]
    fn startup_loader_prefers_materialized_checkpoint_over_inline_shared_ref_hydration() {
        let root = temp_git_repo();
        let ref_name = super::shared_coordination_ref_name(&root);
        let head = super::run_git(&root, &["rev-parse", "HEAD"]).unwrap();
        super::run_git(&root, &["update-ref", &ref_name, &head]).unwrap();

        let snapshot = sample_snapshot_for("plan:checkpoint", "coord-task:checkpoint");
        let mut canonical_snapshot_v2 = snapshot.to_canonical_snapshot_v2();
        canonical_snapshot_v2.next_plan = canonical_snapshot_v2.next_plan + 7;
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
                canonical_snapshot_v2: Some(canonical_snapshot_v2.clone()),
                runtime_descriptors: Vec::new(),
            })
            .unwrap();

        let loaded = load_persisted_coordination_plan_state(&mut store)
            .unwrap()
            .expect("startup checkpoint should hydrate cached plan state");

        assert_eq!(loaded.snapshot, snapshot);
        assert_eq!(loaded.canonical_snapshot_v2, canonical_snapshot_v2);
        let checkpoint = store
            .load_coordination_startup_checkpoint()
            .unwrap()
            .expect("coordination startup checkpoint");
        assert_eq!(
            checkpoint.canonical_snapshot_v2,
            Some(canonical_snapshot_v2)
        );
    }

    #[test]
    fn startup_checkpoint_omits_durable_legacy_plan_graph_state() {
        let root = temp_git_repo();
        let snapshot = sample_snapshot_for("plan:checkpoint-save", "coord-task:checkpoint-save");
        let mut store = MemoryStore::default();

        save_shared_coordination_startup_checkpoint(&root, &mut store, &snapshot, None).unwrap();

        let checkpoint = store
            .load_coordination_startup_checkpoint()
            .unwrap()
            .expect("coordination startup checkpoint");
        let checkpoint_plans = checkpoint.snapshot.plans.clone();
        assert_eq!(
            checkpoint_plans
                .clone()
                .into_iter()
                .map(|plan| super::summary_plan_record(&plan))
                .collect::<Vec<_>>(),
            checkpoint_plans
        );
        assert_eq!(
            checkpoint.canonical_snapshot_v2,
            Some(snapshot.to_canonical_snapshot_v2())
        );
    }

    #[test]
    fn startup_checkpoint_load_preserves_persisted_runtime_descriptors() {
        let root = temp_git_repo();
        let snapshot = sample_snapshot_for(
            "plan:checkpoint-runtime-descriptors",
            "coord-task:checkpoint-runtime-descriptors",
        );
        let canonical_snapshot_v2 = snapshot.to_canonical_snapshot_v2();
        sync_shared_coordination_ref_state(&root, &snapshot, Some(&sample_publish_context()))
            .unwrap();
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
                snapshot,
                canonical_snapshot_v2: Some(canonical_snapshot_v2),
                runtime_descriptors: vec![RuntimeDescriptor {
                    runtime_id: "runtime:stale-checkpoint".to_string(),
                    repo_id: "repo:stale-checkpoint".to_string(),
                    worktree_id: "worktree:stale-checkpoint".to_string(),
                    principal_id: "codex-stale-checkpoint".to_string(),
                    instance_started_at: 1,
                    last_seen_at: 2,
                    branch_ref: Some("refs/heads/stale-checkpoint".to_string()),
                    checked_out_commit: None,
                    capabilities: vec![RuntimeDescriptorCapability::CoordinationRefPublisher],
                    discovery_mode: RuntimeDiscoveryMode::LanDirect,
                    peer_endpoint: Some("http://127.0.0.1:48137/peer/query".to_string()),
                    public_endpoint: None,
                    peer_transport_identity: None,
                    blob_snapshot_head: None,
                    export_policy: None,
                }],
            })
            .unwrap();

        let loaded = load_persisted_coordination_plan_state(&mut store)
            .unwrap()
            .expect("startup checkpoint should hydrate plan state");

        assert_eq!(loaded.runtime_descriptors.len(), 1);
        assert_eq!(
            loaded.runtime_descriptors[0].runtime_id,
            "runtime:stale-checkpoint"
        );
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
        sync_shared_coordination_ref_state(&root, &snapshot, Some(&sample_publish_context()))
            .unwrap();

        let ref_name = super::shared_coordination_ref_name(&root);
        let remote_head =
            super::run_git(&remote, &["rev-parse", "--verify", ref_name.as_str()]).unwrap();
        assert!(!remote_head.trim().is_empty());

        super::run_git(&root, &["update-ref", "-d", ref_name.as_str()]).unwrap();
        let task_shard_ref = super::shared_coordination_task_shard_ref_name(
            &root,
            &super::shared_coordination_shard_key(&task.id.0),
        );
        super::run_git(&root, &["update-ref", "-d", &task_shard_ref]).unwrap();
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
        assert_eq!(
            loaded.snapshot.plans,
            vec![super::summary_plan_record(&plan)]
        );
    }

    #[test]
    fn invalid_shared_coordination_manifest_blocks_next_publish_until_repaired() {
        let (root, _remote) = temp_git_repo_with_origin();
        let snapshot = sample_snapshot_for(
            "plan:shared-invalid-manifest",
            "coord-task:shared-invalid-manifest",
        );
        sync_shared_coordination_ref_state(&root, &snapshot, Some(&sample_publish_context()))
            .unwrap();

        tamper_shared_coordination_manifest_signature(&root);
        let error =
            sync_shared_coordination_ref_state(&root, &snapshot, Some(&sample_publish_context()))
                .expect_err(
                    "authoritative publish should fail while the shared manifest is invalid",
                );
        assert!(error.to_string().contains("base64"));
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
        sync_shared_coordination_ref_state(
            &root_a,
            &base_snapshot,
            Some(&sample_publish_context()),
        )
        .unwrap();

        let ref_name = super::shared_coordination_ref_name(&root_b);
        super::refresh_local_shared_coordination_ref(
            &root_b,
            super::shared_coordination_remote_name(),
            &ref_name,
        )
        .unwrap();
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
        let artifact_a = prism_coordination::Artifact {
            id: prism_ir::ArtifactId::new("artifact:a".to_string()),
            task: task_id.clone(),
            worktree_id: Some("worktree:a".to_string()),
            branch_ref: Some("refs/heads/task/a".to_string()),
            anchors: Vec::new(),
            base_revision: WorkspaceRevision::default(),
            diff_ref: None,
            status: prism_ir::ArtifactStatus::Proposed,
            evidence: Vec::new(),
            reviews: Vec::new(),
            required_validations: Vec::new(),
            validated_checks: Vec::new(),
            risk_score: None,
        };
        let snapshot_a = CoordinationSnapshot {
            claims: vec![claim_a.clone()],
            artifacts: vec![artifact_a.clone()],
            next_claim: 1,
            next_artifact: 1,
            ..base_snapshot.clone()
        };
        sync_shared_coordination_ref_state(&root_a, &snapshot_a, Some(&sample_publish_context()))
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
        let artifact_b = prism_coordination::Artifact {
            id: prism_ir::ArtifactId::new("artifact:b".to_string()),
            task: task_id.clone(),
            worktree_id: Some("worktree:b".to_string()),
            branch_ref: Some("refs/heads/task/b".to_string()),
            anchors: Vec::new(),
            base_revision: WorkspaceRevision::default(),
            diff_ref: None,
            status: prism_ir::ArtifactStatus::Proposed,
            evidence: Vec::new(),
            reviews: Vec::new(),
            required_validations: Vec::new(),
            validated_checks: Vec::new(),
            risk_score: None,
        };
        let snapshot_b = CoordinationSnapshot {
            claims: vec![claim_b.clone()],
            artifacts: vec![artifact_b.clone()],
            next_claim: 1,
            next_artifact: 1,
            ..base_snapshot.clone()
        };
        super::sync_shared_coordination_ref_state(
            &root_b,
            &snapshot_b,
            &snapshot_b.to_canonical_snapshot_v2(),
            Some(&sample_publish_context()),
        )
        .unwrap();

        super::refresh_local_shared_coordination_authority(&root_a).unwrap();
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
        let artifact_ids = loaded
            .snapshot
            .artifacts
            .iter()
            .map(|artifact| artifact.id.0.as_str())
            .collect::<BTreeSet<_>>();
        assert!(
            artifact_ids.contains("artifact:a"),
            "artifact ids: {artifact_ids:?}"
        );
        assert!(
            artifact_ids.contains("artifact:b"),
            "artifact ids: {artifact_ids:?}"
        );

        let diagnostics = shared_coordination_ref_diagnostics(&root_a)
            .unwrap()
            .expect("shared coordination diagnostics should exist");
        assert_eq!(diagnostics.last_successful_publish_retry_count, 0);
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
        sync_shared_coordination_ref_state(&root, &snapshot, Some(&sample_publish_context()))
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
        let changed_state = super::SharedCoordinationRefState {
            snapshot: changed_snapshot.clone(),
            canonical_snapshot_v2: changed_snapshot.to_canonical_snapshot_v2(),
            runtime_descriptors: baseline_state.runtime_descriptors.clone(),
        };
        let stage_dir = root.join(".prism").join("publish-patch-test");
        let _ = fs::remove_dir_all(&stage_dir);
        fs::create_dir_all(&stage_dir).unwrap();
        let patch = super::build_shared_coordination_publish_patch(
            &root,
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
        assert_eq!(
            patch.upserts,
            BTreeSet::from([
                "coordination/v2/snapshot.json".to_string(),
                expected_task_path.clone(),
            ]),
            "summary-only task edits should touch the changed task payload and canonical v2 snapshot, but not unrelated summary records"
        );
        assert!(patch.deletes.is_empty());

        let head_before = super::run_git(&root, &["rev-parse", &ref_name]).unwrap();
        let task_shard_ref = super::shared_coordination_task_shard_ref_name(
            &root,
            &super::shared_coordination_shard_key(&changed_task_b.id.0),
        );
        let task_shard_head_before =
            super::run_git(&root, &["rev-parse", &task_shard_ref]).unwrap();
        sync_shared_coordination_ref_state(
            &root,
            &changed_snapshot,
            Some(&sample_publish_context()),
        )
        .unwrap();
        let head_after = super::run_git(&root, &["rev-parse", &ref_name]).unwrap();
        let task_shard_head_after = super::run_git(&root, &["rev-parse", &task_shard_ref]).unwrap();
        assert_ne!(
            head_before.trim(),
            head_after.trim(),
            "task-only edits should republish the canonical v2 summary ref"
        );
        assert_ne!(
            task_shard_head_before.trim(),
            task_shard_head_after.trim(),
            "task-only edits should advance the owning task shard ref"
        );
        let changed_paths = super::run_git(
            &root,
            &[
                "diff",
                "--name-only",
                task_shard_head_before.trim(),
                task_shard_head_after.trim(),
            ],
        )
        .unwrap()
        .lines()
        .map(str::to_string)
        .collect::<BTreeSet<_>>();
        assert_eq!(
            changed_paths,
            BTreeSet::from([
                "coordination/manifest.json".to_string(),
                expected_task_path,
            ]),
            "task shard publish should only touch the changed task payload and shard manifest when shard membership is unchanged; saw {changed_paths:?}"
        );

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
        assert_eq!(
            loaded.canonical_snapshot_v2,
            changed_snapshot.to_canonical_snapshot_v2()
        );
        let diagnostics = shared_coordination_ref_diagnostics(&root)
            .unwrap()
            .expect("shared coordination diagnostics should exist");
        assert_eq!(diagnostics.summary_freshness_status, "current");
        assert!(!diagnostics.authoritative_fallback_required);
        assert_eq!(diagnostics.lagging_task_shard_refs, 0);
        assert_eq!(diagnostics.lagging_claim_shard_refs, 0);
        assert_eq!(diagnostics.lagging_runtime_refs, 0);
        assert!(diagnostics.freshness_reason.is_none());
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
        sync_shared_coordination_ref_state(&root_a, &snapshot, Some(&sample_publish_context()))
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
        sync_shared_coordination_ref_state(
            &root_b,
            &changed_snapshot,
            Some(&sample_publish_context()),
        )
        .unwrap();

        crate::watch::sync_shared_coordination_ref_watch_update(
            &root_a,
            &session.published_generation,
            &session.runtime_state,
            &session.store,
            &session.cold_query_store,
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
        let eventual = session
            .read_coordination_snapshot_with_consistency(
                crate::coordination_reads::CoordinationReadConsistency::Eventual,
            )
            .unwrap()
            .into_value()
            .expect("eventual coordination snapshot should exist after live sync");
        assert_eq!(eventual.tasks[0].status, CoordinationTaskStatus::Completed);
        assert!(matches!(
            poll_shared_coordination_ref_live_sync(&root_a).unwrap(),
            SharedCoordinationRefLiveSync::Unchanged
        ));
    }

    #[test]
    fn session_coordination_reads_split_eventual_and_strong_consistency() {
        let (root_a, _remote) = temp_git_repo_with_origin();
        seed_workspace_project(&root_a);

        let snapshot = sample_snapshot_for("plan:coord-read-modes", "coord-task:coord-read-modes");
        sync_shared_coordination_ref_state(&root_a, &snapshot, Some(&sample_publish_context()))
            .unwrap();

        let session = index_workspace_session(&root_a).unwrap();
        let eventual_before = session
            .read_coordination_plan_state_with_consistency(CoordinationReadConsistency::Eventual)
            .unwrap();
        assert_eq!(
            eventual_before.freshness,
            CoordinationReadFreshness::Unavailable
        );
        assert!(eventual_before.value.is_none());

        let strong_initial = session
            .read_coordination_plan_state_with_consistency(CoordinationReadConsistency::Strong)
            .unwrap();
        assert_eq!(
            strong_initial.freshness,
            CoordinationReadFreshness::VerifiedCurrent
        );
        let initial_title = strong_initial
            .value
            .as_ref()
            .and_then(|state| state.snapshot.tasks.first())
            .map(|task| task.title.as_str())
            .unwrap();
        assert_eq!(initial_title, "ship it");

        let eventual_after_initial = session
            .read_coordination_plan_state_with_consistency(CoordinationReadConsistency::Eventual)
            .unwrap();
        assert_eq!(
            eventual_after_initial.freshness,
            CoordinationReadFreshness::VerifiedCurrent
        );
        assert_eq!(
            eventual_after_initial
                .value
                .as_ref()
                .and_then(|state| state.snapshot.tasks.first())
                .map(|task| task.title.as_str())
                .unwrap(),
            "ship it"
        );
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
        let task_shard_ref = super::shared_coordination_task_shard_ref_name(
            &root,
            &super::shared_coordination_shard_key(&task_id.0),
        );
        let task_shard_head_before =
            super::run_git(&root, &["rev-parse", &task_shard_ref]).unwrap();

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
        let task_shard_head_after = super::run_git(&root, &["rev-parse", &task_shard_ref]).unwrap();
        assert_ne!(task_shard_head_after, task_shard_head_before);
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
        assert!(
            head_after != head_before || loaded_task.lease_refreshed_at == Some(1700),
            "due heartbeat should either advance the summary ref head or publish refreshed lease state"
        );
    }

    #[test]
    fn shared_coordination_ref_compacts_history_after_threshold() {
        let (root, _remote) = temp_git_repo_with_origin();
        let base_snapshot =
            sample_snapshot_for("plan:shared-compaction", "coord-task:shared-compaction");
        let publish = sample_publish_context();
        let task_id = base_snapshot.tasks[0].id.clone();

        for iteration in 0..(super::SHARED_COORDINATION_HISTORY_MAX_COMMITS + 1) {
            let artifact = prism_coordination::Artifact {
                id: prism_ir::ArtifactId::new(format!("artifact:shared-compaction-{iteration}")),
                task: task_id.clone(),
                worktree_id: Some(format!("worktree:shared-compaction-{iteration}")),
                branch_ref: Some(format!("refs/heads/task/shared-compaction-{iteration}")),
                anchors: Vec::new(),
                base_revision: WorkspaceRevision::default(),
                diff_ref: None,
                status: prism_ir::ArtifactStatus::Proposed,
                evidence: Vec::new(),
                reviews: Vec::new(),
                required_validations: Vec::new(),
                validated_checks: Vec::new(),
                risk_score: None,
            };
            let snapshot = CoordinationSnapshot {
                artifacts: vec![artifact],
                next_artifact: iteration + 1,
                ..base_snapshot.clone()
            };
            sync_shared_coordination_ref_state(&root, &snapshot, Some(&publish)).unwrap();
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
        let snapshot =
            sample_snapshot_for("plan:shared-diagnostics", "coord-task:shared-diagnostics");
        let publish = sample_publish_context();
        sync_shared_coordination_ref_state(&root, &snapshot, Some(&publish)).unwrap();
        sync_live_runtime_descriptor(&root).unwrap();

        let diagnostics = shared_coordination_ref_diagnostics(&root)
            .unwrap()
            .expect("shared coordination diagnostics should exist");
        let ref_name = super::shared_coordination_ref_name(&root);
        let raw_manifest: serde_json::Value = serde_json::from_str(
            &super::run_git(
                &root,
                &["show", &format!("{ref_name}:coordination/manifest.json")],
            )
            .unwrap(),
        )
        .unwrap();
        let task_shard_ref = super::shared_coordination_task_shard_ref_name(
            &root,
            &super::shared_coordination_shard_key("coord-task:shared-diagnostics"),
        );
        let task_relative_path =
            super::run_git(&root, &["ls-tree", "-r", "--name-only", &task_shard_ref])
                .unwrap()
                .lines()
                .find(|path| {
                    path.ends_with(&super::snapshot_file_name("coord-task:shared-diagnostics"))
                })
                .map(str::to_string)
                .expect("task shard should contain the task payload");
        let raw_task: serde_json::Value = serde_json::from_str(
            &super::run_git(
                &root,
                &["show", &format!("{task_shard_ref}:{task_relative_path}")],
            )
            .unwrap(),
        )
        .unwrap();
        let runtime_ref = super::shared_coordination_runtime_ref_name(
            &root,
            &workspace_identity_for_root(&root).instance_id,
        );
        let runtime_relative_path =
            super::run_git(&root, &["ls-tree", "-r", "--name-only", &runtime_ref])
                .unwrap()
                .lines()
                .find(|path| {
                    path.ends_with(&super::snapshot_file_name(
                        &workspace_identity_for_root(&root).instance_id,
                    ))
                })
                .map(str::to_string)
                .expect("runtime ref should contain the runtime descriptor");
        let raw_runtime_descriptor: serde_json::Value = serde_json::from_str(
            &super::run_git(
                &root,
                &["show", &format!("{runtime_ref}:{runtime_relative_path}")],
            )
            .unwrap(),
        )
        .unwrap();
        assert!(diagnostics.ref_name.starts_with("refs/prism/coordination/"));
        assert!(diagnostics.head_commit.is_some());
        assert!(diagnostics.history_depth >= 1);
        assert!(diagnostics.snapshot_file_count >= 3);
        assert!(diagnostics.current_manifest_digest.is_some());
        assert_eq!(
            diagnostics.current_manifest_digest,
            diagnostics.last_verified_manifest_digest
        );
        assert_eq!(diagnostics.summary_freshness_status, "current");
        assert!(!diagnostics.authoritative_fallback_required);
        assert_eq!(diagnostics.lagging_task_shard_refs, 0);
        assert_eq!(diagnostics.lagging_claim_shard_refs, 0);
        assert_eq!(diagnostics.lagging_runtime_refs, 0);
        assert_eq!(diagnostics.summary_published_at, Some(publish.published_at));
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
        assert_eq!(raw_manifest["schema_version"], serde_json::json!(1));
        assert_eq!(
            raw_manifest["kind"],
            serde_json::json!(super::SHARED_COORDINATION_KIND_MANIFEST)
        );
        assert_eq!(
            raw_manifest["summarySources"]["taskShardHeads"][task_shard_ref.as_str()],
            serde_json::json!(super::run_git(&root, &["rev-parse", &task_shard_ref])
                .unwrap()
                .trim())
        );
        assert!(raw_manifest.get("version").is_none());
        assert_eq!(raw_task["schema_version"], serde_json::json!(1));
        assert_eq!(
            raw_task["kind"],
            serde_json::json!(super::SHARED_COORDINATION_KIND_TASK)
        );
        assert_eq!(
            raw_task["payload"]["id"],
            serde_json::json!("coord-task:shared-diagnostics")
        );
        assert!(raw_task.get("version").is_none());
        assert_eq!(
            raw_runtime_descriptor["schema_version"],
            serde_json::json!(1)
        );
        assert_eq!(
            raw_runtime_descriptor["kind"],
            serde_json::json!(super::SHARED_COORDINATION_KIND_RUNTIME_DESCRIPTOR)
        );
        assert_eq!(
            raw_runtime_descriptor["payload"]["worktreeId"],
            serde_json::json!(workspace_identity_for_root(&root).worktree_id)
        );
        assert!(raw_runtime_descriptor.get("version").is_none());
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
    fn shared_coordination_ref_status_summary_matches_displayed_diagnostics_fields() {
        let (root, _remote) = temp_git_repo_with_origin();
        let snapshot = sample_snapshot_for("plan:shared-status", "coord-task:shared-status");
        let publish = sample_publish_context();
        sync_shared_coordination_ref_state(&root, &snapshot, Some(&publish)).unwrap();

        let diagnostics = shared_coordination_ref_diagnostics(&root)
            .unwrap()
            .expect("shared coordination diagnostics should exist");
        let summary = super::shared_coordination_ref_status_summary(&root)
            .unwrap()
            .expect("shared coordination status summary should exist");

        assert_eq!(summary.ref_name, diagnostics.ref_name);
        assert_eq!(summary.head_commit, diagnostics.head_commit);
        assert_eq!(summary.history_depth, diagnostics.history_depth);
        assert_eq!(summary.snapshot_file_count, diagnostics.snapshot_file_count);
        assert_eq!(summary.needs_compaction, diagnostics.needs_compaction);
        assert_eq!(summary.compaction_status, diagnostics.compaction_status);
    }

    #[test]
    fn shared_coordination_summary_materializes_current_task_snapshots() {
        let (root, _remote) = temp_git_repo_with_origin();
        let snapshot = sample_snapshot_for(
            "plan:summary-materialized",
            "coord-task:summary-materialized",
        );
        sync_shared_coordination_ref_state(&root, &snapshot, Some(&sample_publish_context()))
            .unwrap();

        let diagnostics = shared_coordination_ref_diagnostics(&root)
            .unwrap()
            .expect("shared coordination diagnostics should exist");
        assert_eq!(diagnostics.summary_freshness_status, "current");
        assert!(!diagnostics.authoritative_fallback_required);
        assert_eq!(diagnostics.lagging_task_shard_refs, 0);
        assert_eq!(diagnostics.lagging_claim_shard_refs, 0);
        assert_eq!(diagnostics.lagging_runtime_refs, 0);

        let ref_name = super::shared_coordination_ref_name(&root);
        let raw_task: serde_json::Value = serde_json::from_str(
            &super::run_git(
                &root,
                &[
                    "show",
                    &format!(
                        "{ref_name}:coordination/{}",
                        super::task_snapshot_relative_path("coord-task:summary-materialized")
                    ),
                ],
            )
            .unwrap(),
        )
        .unwrap();
        assert_eq!(
            raw_task["payload"]["id"],
            serde_json::json!("coord-task:summary-materialized")
        );

        let loaded = load_shared_coordination_ref_state(&root)
            .unwrap()
            .expect("shared coordination state should load");
        assert_eq!(loaded.snapshot.tasks.len(), snapshot.tasks.len());
        assert!(loaded
            .snapshot
            .tasks
            .iter()
            .any(|task| task.id.0 == "coord-task:summary-materialized"));
    }

    #[test]
    fn shared_coordination_ref_diagnostics_accept_legacy_signed_manifest_shape() {
        let (root, _remote) = temp_git_repo_with_origin();
        let task_id = "coord-task:shared-legacy-manifest";
        let snapshot = sample_snapshot_for("plan:shared-legacy-manifest", task_id);
        let publish = sample_publish_context();
        sync_shared_coordination_ref_state(&root, &snapshot, Some(&publish)).unwrap();
        rewrite_shared_coordination_manifest_as_legacy_signed_payload(&root);

        let diagnostics = shared_coordination_ref_diagnostics(&root)
            .unwrap()
            .expect("shared coordination diagnostics should exist");
        let ref_name = super::shared_coordination_ref_name(&root);
        let raw_manifest: serde_json::Value = serde_json::from_str(
            &super::run_git(
                &root,
                &["show", &format!("{ref_name}:coordination/manifest.json")],
            )
            .unwrap(),
        )
        .unwrap();
        assert_eq!(raw_manifest["version"], serde_json::json!(1));
        assert!(raw_manifest.get("schema_version").is_none());
        assert!(raw_manifest.get("kind").is_none());
        assert_eq!(diagnostics.verification_status, "verified");
        assert!(!diagnostics.degraded);
        assert!(diagnostics.authoritative_hydration_allowed);
        let hydrated = load_authoritative_coordination_plan_state(&root)
            .unwrap()
            .expect("hydrated state");
        assert!(hydrated
            .snapshot
            .tasks
            .iter()
            .any(|task| task.id.0 == task_id));
    }

    #[test]
    fn shared_coordination_ref_diagnostics_surface_degraded_verification_state() {
        let (root, _remote) = temp_git_repo_with_origin();
        let snapshot = sample_snapshot_for("plan:shared-degraded", "coord-task:shared-degraded");
        let publish = sample_publish_context();
        sync_shared_coordination_ref_state(&root, &snapshot, Some(&publish)).unwrap();

        tamper_shared_coordination_manifest(&root, |manifest| {
            manifest.published_at += 1;
        });

        let diagnostics = shared_coordination_ref_diagnostics(&root)
            .unwrap()
            .expect("shared coordination diagnostics should exist");
        assert_eq!(diagnostics.verification_status, "degraded");
        assert!(diagnostics.degraded);
        assert!(!diagnostics.authoritative_hydration_allowed);
        assert!(diagnostics.last_verified_manifest_digest.is_none());
        assert!(diagnostics.current_manifest_digest.is_some());
        assert!(diagnostics
            .verification_error
            .as_deref()
            .is_some_and(|error| error.contains("verification failed")));
        assert!(diagnostics
            .repair_hint
            .as_deref()
            .is_some_and(|hint| hint.contains("Repair or republish")));
    }

    #[test]
    fn invalid_shared_coordination_ref_blocks_authoritative_hydration_without_fallback() {
        let (root, _remote) = temp_git_repo_with_origin();
        let shared_snapshot =
            sample_snapshot_for("plan:shared-blocked", "coord-task:shared-blocked");
        sync_shared_coordination_ref_state(
            &root,
            &shared_snapshot,
            Some(&sample_publish_context()),
        )
        .unwrap();
        tamper_shared_coordination_manifest(&root, |manifest| {
            manifest.published_at += 1;
        });

        let error = load_authoritative_coordination_plan_state(&root)
            .expect_err("invalid shared coordination ref should block authoritative hydration");
        assert!(error.to_string().contains("verification failed"));
    }

    #[test]
    fn sync_live_runtime_descriptor_publishes_descriptor_without_coordination_objects() {
        let (root, _remote) = temp_git_repo_with_origin();
        let snapshot = sample_snapshot_for("plan:runtime-publish", "coord-task:runtime-publish");
        sync_shared_coordination_ref_state(&root, &snapshot, Some(&sample_publish_context()))
            .unwrap();
        let summary_ref = super::shared_coordination_ref_name(&root);
        let summary_head_before = super::run_git(&root, &["rev-parse", &summary_ref]).unwrap();
        sync_live_runtime_descriptor(&root).unwrap();
        let summary_head_after = super::run_git(&root, &["rev-parse", &summary_ref]).unwrap();
        let runtime_ref = super::shared_coordination_runtime_ref_name(
            &root,
            &workspace_identity_for_root(&root).instance_id,
        );

        let diagnostics = shared_coordination_ref_diagnostics(&root)
            .unwrap()
            .expect("shared coordination diagnostics should exist");
        assert_ne!(summary_head_after, summary_head_before);
        assert!(
            super::run_git(&root, &["ls-tree", "-r", "--name-only", &summary_ref])
                .unwrap()
                .lines()
                .any(|path| path.starts_with("coordination/coordination/runtimes/"))
        );
        assert!(super::resolve_ref_commit(&root, &runtime_ref)
            .unwrap()
            .is_some());
        assert_eq!(diagnostics.summary_freshness_status, "current");
        assert!(!diagnostics.authoritative_fallback_required);
        assert_eq!(diagnostics.runtime_descriptor_count, 1);
        assert_eq!(diagnostics.runtime_descriptors.len(), 1);
        assert!(diagnostics.runtime_descriptors[0]
            .capabilities
            .contains(&RuntimeDescriptorCapability::CoordinationRefPublisher));
    }

    #[test]
    fn shared_coordination_ref_diagnostics_load_runtime_only_authority_without_summary_ref() {
        let (root, _remote) = temp_git_repo_with_origin();
        seed_workspace_project(&root);

        sync_live_runtime_descriptor(&root).unwrap();

        let diagnostics = shared_coordination_ref_diagnostics(&root)
            .unwrap()
            .expect("runtime sync should publish a summary authority root");
        assert!(diagnostics.head_commit.is_some());
        assert!(diagnostics.history_depth >= 1);
        assert!(diagnostics.snapshot_file_count >= 1);
        assert!(diagnostics.authoritative_hydration_allowed);
        assert_eq!(diagnostics.summary_freshness_status, "current");
        assert!(!diagnostics.authoritative_fallback_required);
        assert_eq!(diagnostics.runtime_descriptor_count, 1);
        assert_eq!(diagnostics.runtime_descriptors.len(), 1);
    }

    #[test]
    fn sync_live_runtime_descriptor_publishes_configured_public_url() {
        let (root, _remote) = temp_git_repo_with_origin();
        let snapshot =
            sample_snapshot_for("plan:runtime-public-url", "coord-task:runtime-public-url");
        sync_shared_coordination_ref_state(&root, &snapshot, Some(&sample_publish_context()))
            .unwrap();
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
        let snapshot = sample_snapshot_for(
            "plan:runtime-clear-public-url",
            "coord-task:runtime-clear-public-url",
        );
        sync_shared_coordination_ref_state(&root, &snapshot, Some(&sample_publish_context()))
            .unwrap();
        let summary_ref = super::shared_coordination_ref_name(&root);
        let summary_head_before = super::run_git(&root, &["rev-parse", &summary_ref]).unwrap();
        let runtime_ref = super::shared_coordination_runtime_ref_name(
            &root,
            &workspace_identity_for_root(&root).instance_id,
        );
        let public_url_path = crate::PrismPaths::for_workspace_root(&root)
            .unwrap()
            .mcp_public_url_path()
            .unwrap();
        fs::create_dir_all(public_url_path.parent().unwrap()).unwrap();
        fs::write(&public_url_path, "https://runtime.example/peer/query\n").unwrap();

        sync_live_runtime_descriptor(&root).unwrap();
        let runtime_head_before = super::run_git(&root, &["rev-parse", &runtime_ref]).unwrap();
        fs::remove_file(&public_url_path).unwrap();
        sync_live_runtime_descriptor(&root).unwrap();
        let summary_head_after = super::run_git(&root, &["rev-parse", &summary_ref]).unwrap();
        let runtime_head_after = super::run_git(&root, &["rev-parse", &runtime_ref]).unwrap();

        let diagnostics = shared_coordination_ref_diagnostics(&root)
            .unwrap()
            .expect("shared coordination diagnostics should exist");
        assert_ne!(summary_head_after, summary_head_before);
        assert_ne!(runtime_head_after, runtime_head_before);
        assert_eq!(diagnostics.runtime_descriptor_count, 1);
        assert_eq!(diagnostics.runtime_descriptors[0].public_endpoint, None);
        assert_eq!(
            diagnostics.runtime_descriptors[0].discovery_mode,
            prism_coordination::RuntimeDiscoveryMode::None
        );
    }
}
