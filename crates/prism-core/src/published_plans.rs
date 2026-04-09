use std::collections::BTreeSet;
use std::fs::{self, File};
use std::io::Write;
use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

use anyhow::Result;
use prism_coordination::{
    CanonicalPlanRecord, CanonicalTaskRecord, CoordinationDependencyRecord,
    CoordinationDerivations, CoordinationSnapshot, CoordinationSnapshotV2, RuntimeDescriptor,
};
use prism_ir::{DerivedPlanStatus, NodeRef, NodeRefKind, PlanId, PlanStatus};
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};

use crate::coordination_authority_store::{
    configured_coordination_authority_store_provider, CoordinationCurrentState,
    CoordinationReplaceCurrentStateRequest, CoordinationTransactionBase,
    CoordinationTransactionStatus,
};
use crate::coordination_reads::CoordinationReadConsistency;
use crate::tracked_snapshot::{
    remove_obsolete_legacy_tracked_authority_artifacts, tracked_snapshot_authority_active,
    TrackedSnapshotPublishContext,
};
use crate::util::{repo_active_plans_dir, repo_archived_plans_dir, repo_plan_index_path};

fn observe_published_plan_step<T, E, O, F, A>(
    observe_phase: &mut O,
    operation: &str,
    success_args: A,
    step: F,
) -> std::result::Result<T, E>
where
    E: ToString,
    O: FnMut(&str, Duration, Value, bool, Option<String>),
    F: FnOnce() -> std::result::Result<T, E>,
    A: FnOnce(&T) -> Value,
{
    let started = Instant::now();
    match step() {
        Ok(value) => {
            observe_phase(
                operation,
                started.elapsed(),
                success_args(&value),
                true,
                None,
            );
            Ok(value)
        }
        Err(error) => {
            observe_phase(
                operation,
                started.elapsed(),
                json!({}),
                false,
                Some(error.to_string()),
            );
            Err(error)
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub(crate) struct PublishedPlanIndexEntry {
    pub(crate) plan_id: PlanId,
    pub(crate) title: String,
    pub(crate) status: PlanStatus,
    pub(crate) scope: String,
    pub(crate) kind: String,
    pub(crate) log_path: String,
}

#[derive(Debug, Clone, PartialEq)]
pub struct HydratedCoordinationPlanState {
    pub(crate) snapshot: CoordinationSnapshot,
    pub(crate) canonical_snapshot_v2: CoordinationSnapshotV2,
    pub(crate) runtime_descriptors: Vec<RuntimeDescriptor>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
struct PublishedPlanArtifact {
    schema_version: u64,
    status: PlanStatus,
    plan: CanonicalPlanRecord,
    #[serde(default)]
    direct_child_plans: Vec<CanonicalPlanRecord>,
    #[serde(default)]
    direct_tasks: Vec<CanonicalTaskRecord>,
    #[serde(default)]
    dependencies: Vec<CoordinationDependencyRecord>,
}

pub(crate) fn sync_repo_published_plans(
    root: &Path,
    snapshot: &CoordinationSnapshot,
    canonical_snapshot_v2: &CoordinationSnapshotV2,
    publish: Option<&TrackedSnapshotPublishContext>,
) -> Result<()> {
    sync_repo_published_plan_state_observed(
        root,
        snapshot,
        canonical_snapshot_v2,
        publish,
        |_operation, _duration, _args, _success, _error| {},
    )
}

pub fn regenerate_repo_published_plan_artifacts(root: &Path) -> Result<()> {
    if tracked_snapshot_authority_active(root)? {
        remove_obsolete_legacy_tracked_authority_artifacts(root)?;
        return Ok(());
    }

    let Some(snapshot_v2) = load_authoritative_coordination_snapshot_v2(root)? else {
        remove_published_plan_artifacts(root)?;
        return Ok(());
    };

    write_derived_published_plan_artifacts(root, &snapshot_v2)
}

pub(crate) fn sync_repo_published_plan_state_observed<O>(
    root: &Path,
    snapshot: &CoordinationSnapshot,
    canonical_snapshot_v2: &CoordinationSnapshotV2,
    _publish: Option<&TrackedSnapshotPublishContext>,
    mut observe_phase: O,
) -> Result<()>
where
    O: FnMut(&str, Duration, Value, bool, Option<String>),
{
    let authority_store =
        configured_coordination_authority_store_provider(root)?.open_snapshot(root)?;
    observe_phase(
        "mutation.coordination.publishedPlans.writeLogs",
        Duration::ZERO,
        json!({
            "eventCount": 0,
            "logCount": 0,
            "skipped": true,
            "reason": "handled_by_authority_pipeline",
        }),
        true,
        None,
    );
    observe_phase(
        "mutation.coordination.publishedPlans.writeIndex",
        Duration::ZERO,
        json!({
            "entryCount": 0,
            "skipped": true,
            "reason": "handled_by_authority_pipeline",
        }),
        true,
        None,
    );
    observe_phase(
        "mutation.coordination.publishedPlans.cleanupLogs",
        Duration::ZERO,
        json!({
            "expectedLogCount": 0,
            "skipped": true,
            "reason": "handled_by_authority_pipeline",
        }),
        true,
        None,
    );
    observe_published_plan_step(
        &mut observe_phase,
        "mutation.coordination.authority.applyTransaction",
        |result: &crate::coordination_authority_store::CoordinationTransactionResult| {
            json!({
                "committed": result.committed,
                "status": format!("{:?}", result.status),
            })
        },
        || {
            let runtime_store =
                configured_coordination_authority_store_provider(root)?.open(root)?;
            match authority_store.replace_current_state(CoordinationReplaceCurrentStateRequest {
                base: CoordinationTransactionBase::LatestStrong,
                state: CoordinationCurrentState {
                    snapshot: snapshot.clone(),
                    canonical_snapshot_v2: canonical_snapshot_v2.clone(),
                    runtime_descriptors: runtime_store
                        .list_runtime_descriptors(crate::RuntimeDescriptorQuery {
                            consistency: CoordinationReadConsistency::Strong,
                        })?
                        .value
                        .unwrap_or_default(),
                },
            })? {
                result if matches!(result.status, CoordinationTransactionStatus::Committed) => {
                    Ok(result)
                }
                result => Err(anyhow::anyhow!(
                    "coordination authority transaction did not commit successfully: {:?}",
                    result.status
                )),
            }
        },
    )?;
    observe_published_plan_step(
        &mut observe_phase,
        "mutation.coordination.publishedPlans.syncTrackedSnapshot",
        |_| {
            json!({
                "skipped": true,
                "reason": "handled_by_authority_pipeline",
            })
        },
        || Ok::<(), anyhow::Error>(()),
    )
}

pub(crate) fn load_authoritative_coordination_snapshot(
    root: &Path,
) -> Result<Option<CoordinationSnapshot>> {
    let store = configured_coordination_authority_store_provider(root)?.open_snapshot(root)?;
    Ok(store
        .read_snapshot(CoordinationReadConsistency::Strong)?
        .value)
}

pub(crate) fn load_authoritative_coordination_snapshot_v2(
    root: &Path,
) -> Result<Option<CoordinationSnapshotV2>> {
    let store = configured_coordination_authority_store_provider(root)?.open_snapshot(root)?;
    Ok(store
        .read_snapshot_v2(CoordinationReadConsistency::Strong)?
        .value)
}

pub(crate) fn load_authoritative_coordination_plan_state(
    root: &Path,
) -> Result<Option<HydratedCoordinationPlanState>> {
    let store = configured_coordination_authority_store_provider(root)?.open(root)?;
    Ok(store
        .read_current_state(CoordinationReadConsistency::Strong)?
        .value
        .map(Into::into))
}

pub(crate) fn merge_shared_coordination_into_snapshot(
    mut snapshot: CoordinationSnapshot,
    shared_snapshot: CoordinationSnapshot,
) -> CoordinationSnapshot {
    let shared_plan_ids = shared_snapshot
        .plans
        .iter()
        .map(|plan| plan.id.0.to_string())
        .collect::<BTreeSet<_>>();
    let shared_task_ids = shared_snapshot
        .tasks
        .iter()
        .map(|task| task.id.0.to_string())
        .collect::<BTreeSet<_>>();
    let shared_artifact_ids = shared_snapshot
        .artifacts
        .iter()
        .map(|artifact| artifact.id.0.to_string())
        .collect::<BTreeSet<_>>();
    let shared_claim_ids = shared_snapshot
        .claims
        .iter()
        .map(|claim| claim.id.0.to_string())
        .collect::<BTreeSet<_>>();
    let shared_review_ids = shared_snapshot
        .reviews
        .iter()
        .map(|review| review.id.0.to_string())
        .collect::<BTreeSet<_>>();
    snapshot
        .plans
        .retain(|plan| !shared_plan_ids.contains(plan.id.0.as_str()));
    snapshot
        .tasks
        .retain(|task| !shared_task_ids.contains(task.id.0.as_str()));
    snapshot
        .artifacts
        .retain(|artifact| !shared_artifact_ids.contains(artifact.id.0.as_str()));
    snapshot
        .claims
        .retain(|claim| !shared_claim_ids.contains(claim.id.0.as_str()));
    snapshot
        .reviews
        .retain(|review| !shared_review_ids.contains(review.id.0.as_str()));
    snapshot.plans.extend(shared_snapshot.plans);
    snapshot.tasks.extend(shared_snapshot.tasks);
    snapshot.artifacts.extend(shared_snapshot.artifacts);
    snapshot.claims.extend(shared_snapshot.claims);
    snapshot.reviews.extend(shared_snapshot.reviews);
    snapshot
        .plans
        .sort_by(|left, right| left.id.0.cmp(&right.id.0));
    snapshot
        .tasks
        .sort_by(|left, right| left.id.0.cmp(&right.id.0));
    snapshot
        .artifacts
        .sort_by(|left, right| left.id.0.cmp(&right.id.0));
    snapshot
        .claims
        .sort_by(|left, right| left.id.0.cmp(&right.id.0));
    snapshot
        .reviews
        .sort_by(|left, right| left.id.0.cmp(&right.id.0));
    snapshot.next_plan = snapshot.next_plan.max(shared_snapshot.next_plan);
    snapshot.next_task = snapshot.next_task.max(shared_snapshot.next_task);
    snapshot.next_claim = snapshot.next_claim.max(shared_snapshot.next_claim);
    snapshot.next_artifact = snapshot.next_artifact.max(shared_snapshot.next_artifact);
    snapshot.next_review = snapshot.next_review.max(shared_snapshot.next_review);
    snapshot
}

fn write_derived_published_plan_artifacts(
    root: &Path,
    snapshot: &CoordinationSnapshotV2,
) -> Result<()> {
    let active_dir = repo_active_plans_dir(root);
    let archived_dir = repo_archived_plans_dir(root);
    fs::create_dir_all(&active_dir)?;
    fs::create_dir_all(&archived_dir)?;

    remove_dir_all_if_exists(&root.join(".prism").join("plans").join("streams"))?;

    let derivations = CoordinationDerivations::derive(snapshot)?;
    let mut expected_derived_logs = BTreeSet::new();
    let mut index_entries = Vec::new();

    for plan in &snapshot.plans {
        let derived_status = derivations
            .plan_state(&plan.id)
            .map(|state| state.derived_status)
            .unwrap_or(DerivedPlanStatus::Pending);
        let status = compatibility_plan_status(derived_status);
        let artifact = published_plan_artifact(snapshot, plan, status);
        let relative_log_path = derived_plan_log_path(status, &plan.id);
        let derived_log_path = root.join(&relative_log_path);
        write_jsonl_file(&derived_log_path, &[artifact])?;
        expected_derived_logs.insert(normalize_relative_path(&derived_log_path));
        index_entries.push(PublishedPlanIndexEntry {
            plan_id: plan.id.clone(),
            title: plan.title.clone(),
            status,
            scope: format!("{:?}", plan.scope),
            kind: format!("{:?}", plan.kind),
            log_path: normalize_relative_path(&relative_log_path),
        });
    }

    index_entries.sort_by(|left, right| left.plan_id.0.cmp(&right.plan_id.0));
    write_jsonl_file(&repo_plan_index_path(root), &index_entries)?;
    cleanup_stale_derived_plan_logs(root, &expected_derived_logs)?;
    Ok(())
}

fn published_plan_artifact(
    snapshot: &CoordinationSnapshotV2,
    plan: &CanonicalPlanRecord,
    status: PlanStatus,
) -> PublishedPlanArtifact {
    let mut direct_child_plans = snapshot
        .plans
        .iter()
        .filter(|candidate| candidate.parent_plan_id.as_ref() == Some(&plan.id))
        .cloned()
        .collect::<Vec<_>>();
    direct_child_plans.sort_by(|left, right| left.id.0.cmp(&right.id.0));
    let direct_child_plan_ids = direct_child_plans
        .iter()
        .map(|candidate| candidate.id.0.to_string())
        .collect::<BTreeSet<_>>();

    let mut direct_tasks = snapshot
        .tasks
        .iter()
        .filter(|task| task.parent_plan_id == plan.id)
        .cloned()
        .collect::<Vec<_>>();
    direct_tasks.sort_by(|left, right| left.id.0.cmp(&right.id.0));
    let direct_task_ids = direct_tasks
        .iter()
        .map(|task| task.id.0.to_string())
        .collect::<BTreeSet<_>>();

    let mut dependencies = snapshot
        .dependencies
        .iter()
        .filter(|dependency| {
            artifact_contains_node(&dependency.source, &direct_child_plan_ids, &direct_task_ids)
                && artifact_contains_node(
                    &dependency.target,
                    &direct_child_plan_ids,
                    &direct_task_ids,
                )
        })
        .cloned()
        .collect::<Vec<_>>();
    dependencies.sort_by(|left, right| {
        left.source
            .kind
            .cmp(&right.source.kind)
            .then_with(|| left.source.id.cmp(&right.source.id))
            .then_with(|| left.target.kind.cmp(&right.target.kind))
            .then_with(|| left.target.id.cmp(&right.target.id))
    });

    PublishedPlanArtifact {
        schema_version: snapshot.schema_version,
        status,
        plan: plan.clone(),
        direct_child_plans,
        direct_tasks,
        dependencies,
    }
}

fn artifact_contains_node(
    node: &NodeRef,
    direct_child_plan_ids: &BTreeSet<String>,
    direct_task_ids: &BTreeSet<String>,
) -> bool {
    match node.kind {
        NodeRefKind::Plan => direct_child_plan_ids.contains(node.id.as_str()),
        NodeRefKind::Task => direct_task_ids.contains(node.id.as_str()),
    }
}

fn compatibility_plan_status(status: DerivedPlanStatus) -> PlanStatus {
    match status {
        DerivedPlanStatus::Pending => PlanStatus::Draft,
        DerivedPlanStatus::Active => PlanStatus::Active,
        DerivedPlanStatus::Blocked => PlanStatus::Blocked,
        DerivedPlanStatus::BrokenDependency => PlanStatus::Blocked,
        DerivedPlanStatus::Completed => PlanStatus::Completed,
        DerivedPlanStatus::Failed => PlanStatus::Blocked,
        DerivedPlanStatus::Abandoned => PlanStatus::Abandoned,
        DerivedPlanStatus::Archived => PlanStatus::Archived,
    }
}

fn derived_plan_log_path(status: PlanStatus, plan_id: &PlanId) -> PathBuf {
    let base = if matches!(status, PlanStatus::Archived) {
        PathBuf::from(".prism").join("plans").join("archived")
    } else {
        PathBuf::from(".prism").join("plans").join("active")
    };
    base.join(format!("{}.jsonl", plan_id.0))
}

fn cleanup_stale_derived_plan_logs(
    root: &Path,
    expected_derived_logs: &BTreeSet<String>,
) -> Result<()> {
    for dir in [repo_active_plans_dir(root), repo_archived_plans_dir(root)] {
        if !dir.exists() {
            continue;
        }
        for entry in fs::read_dir(&dir)? {
            let entry = entry?;
            let path = entry.path();
            if path.extension().and_then(|ext| ext.to_str()) != Some("jsonl") {
                continue;
            }
            if !expected_derived_logs.contains(&normalize_relative_path(&path)) {
                fs::remove_file(path)?;
            }
        }
    }
    Ok(())
}

fn remove_published_plan_artifacts(root: &Path) -> Result<()> {
    remove_file_if_exists(&repo_plan_index_path(root))?;
    remove_dir_all_if_exists(&root.join(".prism").join("plans").join("streams"))?;
    remove_dir_all_if_exists(&repo_active_plans_dir(root))?;
    remove_dir_all_if_exists(&repo_archived_plans_dir(root))?;
    remove_dir_if_empty(&root.join(".prism").join("plans"))?;
    Ok(())
}

fn remove_file_if_exists(path: &Path) -> Result<()> {
    if path.exists() {
        fs::remove_file(path)?;
    }
    Ok(())
}

fn remove_dir_all_if_exists(path: &Path) -> Result<()> {
    if path.exists() {
        fs::remove_dir_all(path)?;
    }
    Ok(())
}

fn remove_dir_if_empty(path: &Path) -> Result<()> {
    if path.exists() && fs::read_dir(path)?.next().is_none() {
        fs::remove_dir(path)?;
    }
    Ok(())
}

fn write_jsonl_file<T>(path: &Path, values: &[T]) -> Result<()>
where
    T: Serialize,
{
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)?;
    }
    let mut next = Vec::new();
    for value in values {
        serde_json::to_writer(&mut next, value)?;
        next.push(b'\n');
    }
    if fs::read(path).ok().as_deref() == Some(next.as_slice()) {
        return Ok(());
    }
    let temp_path = path.with_extension("jsonl.tmp");
    let mut file = File::create(&temp_path)?;
    file.write_all(&next)?;
    file.sync_all()?;
    fs::rename(temp_path, path)?;
    Ok(())
}

fn normalize_relative_path(path: impl AsRef<Path>) -> String {
    path.as_ref().to_string_lossy().replace('\\', "/")
}
