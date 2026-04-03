use std::collections::{BTreeMap, BTreeSet};
use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;

use anyhow::{anyhow, Context, Result};
use base64::engine::general_purpose::STANDARD as BASE64_STANDARD;
use base64::Engine;
use ed25519_dalek::{Signer, Verifier};
use prism_coordination::{
    Artifact, ArtifactReview, CoordinationSnapshot, CoordinationTask, Plan, WorkClaim,
};
use prism_ir::{PlanExecutionOverlay, PlanGraph, WorkContextKind, WorkContextSnapshot};
use serde::{Deserialize, Serialize};

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
    let expected_remote_head =
        refresh_local_shared_coordination_ref(root, shared_coordination_remote_name(), &ref_name)?;
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
    );
    let _ = fs::remove_dir_all(&stage_dir);
    result
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
) -> Result<()> {
    sync_plan_objects(stage_dir, snapshot, plan_graphs, execution_overlays)?;
    sync_task_objects(stage_dir, &snapshot.tasks)?;
    sync_artifact_objects(stage_dir, &snapshot.artifacts)?;
    sync_claim_objects(stage_dir, &snapshot.claims)?;
    sync_review_objects(stage_dir, &snapshot.reviews)?;
    rebuild_plan_index(stage_dir)?;
    rebuild_task_index(stage_dir)?;
    rebuild_artifact_index(stage_dir)?;
    rebuild_claim_index(stage_dir)?;
    rebuild_review_index(stage_dir)?;
    let previous_manifest = load_shared_coordination_manifest_from_ref(root, &ref_name)?;
    write_manifest(stage_dir, paths, publish, previous_manifest.as_ref())?;
    publish_stage_to_ref(root, stage_dir, ref_name)?;
    push_shared_coordination_ref(
        root,
        shared_coordination_remote_name(),
        ref_name,
        expected_remote_head,
    )
}

pub(crate) fn load_shared_coordination_ref_state(
    root: &Path,
) -> Result<Option<SharedCoordinationRefState>> {
    if !git_repo_available(root) {
        return Ok(None);
    }
    let ref_name = shared_coordination_ref_name(root);
    let _ =
        refresh_local_shared_coordination_ref(root, shared_coordination_remote_name(), &ref_name)?;
    let Some(manifest) = load_shared_coordination_manifest_from_ref(root, &ref_name)? else {
        return Ok(None);
    };
    verify_shared_coordination_manifest(root, &manifest)?;
    let plan_records =
        load_records_from_ref::<SharedCoordinationPlanRecord, _>(root, &ref_name, |path| {
            path.starts_with("plans/")
        })?
        .into_iter()
        .map(|(_, record)| record)
        .collect::<Vec<_>>();
    let tasks = load_records_from_ref::<CoordinationTask, _>(root, &ref_name, |path| {
        path.starts_with("coordination/tasks/")
    })?
    .into_iter()
    .map(|(_, task)| task)
    .collect::<Vec<_>>();
    let artifacts = load_records_from_ref::<Artifact, _>(root, &ref_name, |path| {
        path.starts_with("coordination/artifacts/")
    })?
    .into_iter()
    .map(|(_, artifact)| artifact)
    .collect::<Vec<_>>();
    let claims = load_records_from_ref::<WorkClaim, _>(root, &ref_name, |path| {
        path.starts_with("coordination/claims/")
    })?
    .into_iter()
    .map(|(_, claim)| claim)
    .collect::<Vec<_>>();
    let reviews = load_records_from_ref::<ArtifactReview, _>(root, &ref_name, |path| {
        path.starts_with("coordination/reviews/")
    })?
    .into_iter()
    .map(|(_, review)| review)
    .collect::<Vec<_>>();

    if plan_records.is_empty()
        && tasks.is_empty()
        && artifacts.is_empty()
        && claims.is_empty()
        && reviews.is_empty()
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
    for task in &snapshot.tasks {
        execution_overlays
            .entry(task.plan.0.to_string())
            .or_default();
    }
    Ok(Some(SharedCoordinationRefState {
        snapshot,
        plan_graphs,
        execution_overlays,
    }))
}

pub(crate) fn shared_coordination_ref_exists(root: &Path) -> Result<bool> {
    if !git_repo_available(root) {
        return Ok(false);
    }
    Ok(resolve_ref_commit(root, &shared_coordination_ref_name(root))?.is_some())
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

fn rebuild_plan_index(stage_dir: &Path) -> Result<()> {
    let entries = load_json_records::<SharedCoordinationPlanRecord>(&stage_plans_dir(stage_dir))?
        .into_iter()
        .map(|(path, record)| SharedCoordinationIndexEntry {
            id: record.plan.id.0.to_string(),
            title: if record.plan.title.trim().is_empty() {
                record.plan.goal.clone()
            } else {
                record.plan.title.clone()
            },
            status: format!("{:?}", record.plan.status),
            path,
        })
        .collect::<Vec<_>>();
    write_json_file(&stage_indexes_dir(stage_dir).join("plans.json"), &entries)
}

fn rebuild_task_index(stage_dir: &Path) -> Result<()> {
    let entries = load_json_records::<CoordinationTask>(&stage_tasks_dir(stage_dir))?
        .into_iter()
        .map(|(path, task)| SharedCoordinationIndexEntry {
            id: task.id.0.to_string(),
            title: task.title,
            status: format!("{:?}", task.status),
            path,
        })
        .collect::<Vec<_>>();
    write_json_file(&stage_indexes_dir(stage_dir).join("tasks.json"), &entries)
}

fn rebuild_artifact_index(stage_dir: &Path) -> Result<()> {
    let entries = load_json_records::<Artifact>(&stage_artifacts_dir(stage_dir))?
        .into_iter()
        .map(|(path, artifact)| SharedCoordinationIndexEntry {
            id: artifact.id.0.to_string(),
            title: artifact.task.0.to_string(),
            status: format!("{:?}", artifact.status),
            path,
        })
        .collect::<Vec<_>>();
    write_json_file(
        &stage_indexes_dir(stage_dir).join("artifacts.json"),
        &entries,
    )
}

fn rebuild_claim_index(stage_dir: &Path) -> Result<()> {
    let entries = load_json_records::<WorkClaim>(&stage_claims_dir(stage_dir))?
        .into_iter()
        .map(|(path, claim)| SharedCoordinationIndexEntry {
            id: claim.id.0.to_string(),
            title: claim
                .task
                .as_ref()
                .map(|task| task.0.to_string())
                .unwrap_or_else(|| claim.id.0.to_string()),
            status: format!("{:?}", claim.status),
            path,
        })
        .collect::<Vec<_>>();
    write_json_file(&stage_indexes_dir(stage_dir).join("claims.json"), &entries)
}

fn rebuild_review_index(stage_dir: &Path) -> Result<()> {
    let entries = load_json_records::<ArtifactReview>(&stage_reviews_dir(stage_dir))?
        .into_iter()
        .map(|(path, review)| SharedCoordinationIndexEntry {
            id: review.id.0.to_string(),
            title: review.summary,
            status: format!("{:?}", review.verdict),
            path,
        })
        .collect::<Vec<_>>();
    write_json_file(&stage_indexes_dir(stage_dir).join("reviews.json"), &entries)
}

fn write_manifest(
    stage_dir: &Path,
    paths: &PrismPaths,
    publish: Option<&TrackedSnapshotPublishContext>,
    previous_manifest: Option<&SharedCoordinationManifest>,
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
    let Some(_) = resolve_ref_commit(root, ref_name)? else {
        return Ok(None);
    };
    let bytes = match git_show_file(root, ref_name, "coordination/manifest.json") {
        Ok(bytes) => bytes,
        Err(error) if error.to_string().contains("does not exist") => return Ok(None),
        Err(error) => return Err(error),
    };
    Ok(Some(
        serde_json::from_slice(&bytes).context("failed to parse shared coordination manifest")?,
    ))
}

fn verify_shared_coordination_manifest(
    root: &Path,
    manifest: &SharedCoordinationManifest,
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
    let ref_name = shared_coordination_ref_name(root);
    for file in manifest.files.values() {
        let bytes = git_show_file(root, &ref_name, &format!("coordination/{}", file.path))?;
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
    let tree = run_git_with_env(root, &envs, &["write-tree"])?;
    let parent = resolve_ref_commit(root, ref_name)?;
    let commit = if let Some(parent) = parent.as_deref() {
        run_git(
            root,
            &[
                "commit-tree",
                tree.trim(),
                "-p",
                parent,
                "-m",
                "prism: update shared coordination ref",
            ],
        )?
    } else {
        run_git(
            root,
            &[
                "commit-tree",
                tree.trim(),
                "-m",
                "prism: initialize shared coordination ref",
            ],
        )?
    };
    if let Some(parent) = parent.as_deref() {
        let _ = run_git(
            root,
            &[
                "update-ref",
                "-m",
                "prism: update shared coordination ref",
                ref_name,
                commit.trim(),
                parent,
            ],
        )?;
    } else {
        let _ = run_git(
            root,
            &[
                "update-ref",
                "-m",
                "prism: initialize shared coordination ref",
                ref_name,
                commit.trim(),
            ],
        )?;
    }
    Ok(())
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
    if !git_remote_available(root, remote) {
        return Ok(());
    }
    let lease = format!(
        "--force-with-lease={ref_name}:{}",
        expected_remote_head.unwrap_or("")
    );
    let refspec = format!("{ref_name}:{ref_name}");
    let _ = run_git(root, &["push", "--porcelain", &lease, remote, &refspec])?;
    Ok(())
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

fn load_records_from_ref<T, F>(root: &Path, ref_name: &str, filter: F) -> Result<Vec<(String, T)>>
where
    T: for<'de> Deserialize<'de>,
    F: Fn(&str) -> bool,
{
    let paths = list_ref_json_paths(root, ref_name)?
        .into_iter()
        .filter(|path| filter(path.as_str()))
        .collect::<Vec<_>>();
    let mut records = Vec::new();
    for path in paths {
        let bytes = git_show_file(root, ref_name, &format!("coordination/{path}"))?;
        let value = serde_json::from_slice::<T>(&bytes)
            .with_context(|| format!("failed to parse shared coordination ref file `{path}`"))?;
        records.push((path, value));
    }
    records.sort_by(|left, right| left.0.cmp(&right.0));
    Ok(records)
}

fn list_ref_json_paths(root: &Path, ref_name: &str) -> Result<Vec<String>> {
    let output = run_git(
        root,
        &["ls-tree", "-r", "--name-only", ref_name, "coordination"],
    )?;
    Ok(output
        .lines()
        .map(str::trim)
        .filter(|line| !line.is_empty())
        .filter_map(|line| {
            let relative = line.strip_prefix("coordination/")?;
            (relative.ends_with(".json") && relative != "manifest.json")
                .then(|| relative.to_string())
        })
        .collect())
}

fn git_show_file(root: &Path, ref_name: &str, path: &str) -> Result<Vec<u8>> {
    let output = Command::new("git")
        .current_dir(root)
        .env("GIT_AUTHOR_NAME", "PRISM")
        .env("GIT_AUTHOR_EMAIL", "prism@local")
        .env("GIT_COMMITTER_NAME", "PRISM")
        .env("GIT_COMMITTER_EMAIL", "prism@local")
        .args(["show", &format!("{ref_name}:{path}")])
        .output()
        .with_context(|| format!("failed to run git show for `{ref_name}:{path}`"))?;
    if !output.status.success() {
        return Err(anyhow!(
            "git show {}:{} failed: {}",
            ref_name,
            path,
            String::from_utf8_lossy(&output.stderr).trim()
        ));
    }
    Ok(output.stdout)
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
    use std::collections::BTreeMap;
    use std::fs;
    use std::path::{Path, PathBuf};
    use std::sync::atomic::{AtomicU64, Ordering};
    use std::time::{SystemTime, UNIX_EPOCH};

    use prism_coordination::{
        CoordinationPolicy, CoordinationSnapshot, CoordinationTask, Plan, PlanScheduling,
        TaskGitExecution, WorkClaim,
    };
    use prism_ir::{
        ClaimId, ClaimMode, ClaimStatus, CoordinationTaskId, CoordinationTaskStatus, PlanId,
        PlanKind, PlanScope, PlanStatus, SessionId, WorkspaceRevision,
    };

    use super::{
        implicit_principal_identity, load_shared_coordination_ref_state,
        shared_coordination_ref_exists, sync_shared_coordination_ref_state,
    };
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

    fn sample_publish_context() -> TrackedSnapshotPublishContext {
        TrackedSnapshotPublishContext {
            published_at: current_timestamp(),
            principal: implicit_principal_identity(None, None),
            work_context: Some(super::implicit_work_context()),
            publish_summary: None,
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
            acceptance: Vec::new(),
            validation_refs: Vec::new(),
            is_abstract: false,
            base_revision: WorkspaceRevision::default(),
            priority: Some(1),
            tags: Vec::new(),
            metadata: serde_json::Value::Null,
            git_execution: TaskGitExecution {
                status: prism_ir::GitExecutionStatus::Published,
                pending_task_status: Some(CoordinationTaskStatus::Completed),
                source_ref: Some("refs/heads/task/shared".to_string()),
                target_ref: Some("origin/main".to_string()),
                publish_ref: Some("refs/heads/task/shared".to_string()),
                target_branch: Some("main".to_string()),
                last_preflight: None,
                last_publish: None,
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
            acceptance: Vec::new(),
            validation_refs: Vec::new(),
            is_abstract: false,
            base_revision: WorkspaceRevision::default(),
            priority: Some(1),
            tags: Vec::new(),
            metadata: serde_json::Value::Null,
            git_execution: TaskGitExecution {
                status: prism_ir::GitExecutionStatus::Published,
                pending_task_status: Some(CoordinationTaskStatus::Completed),
                source_ref: Some("refs/heads/task/shared".to_string()),
                target_ref: Some("origin/main".to_string()),
                publish_ref: Some("refs/heads/task/shared".to_string()),
                target_branch: Some("main".to_string()),
                last_preflight: None,
                last_publish: None,
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
            prism_ir::GitExecutionStatus::Published
        );
        assert_eq!(
            loaded_task.branch_ref.as_deref(),
            Some("refs/heads/task/shared")
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
            acceptance: Vec::new(),
            validation_refs: Vec::new(),
            is_abstract: false,
            base_revision: WorkspaceRevision::default(),
            priority: Some(1),
            tags: Vec::new(),
            metadata: serde_json::Value::Null,
            git_execution: TaskGitExecution {
                status: prism_ir::GitExecutionStatus::Published,
                pending_task_status: Some(CoordinationTaskStatus::Completed),
                source_ref: Some("refs/heads/task/shared".to_string()),
                target_ref: Some("origin/main".to_string()),
                publish_ref: Some("refs/heads/task/shared".to_string()),
                target_branch: Some("main".to_string()),
                last_preflight: None,
                last_publish: None,
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
        let loaded = load_shared_coordination_ref_state(&root)
            .unwrap()
            .expect("shared ref state should reload from remote");
        assert_eq!(loaded.snapshot.tasks, vec![task]);
        assert_eq!(loaded.snapshot.plans, vec![plan]);
    }
}
