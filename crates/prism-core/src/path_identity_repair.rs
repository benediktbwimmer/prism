use std::collections::BTreeSet;
use std::path::{Path, PathBuf};

use anyhow::{bail, Result};
use prism_memory::{OutcomeEvent, OutcomeKind};
use prism_store::{PatchPathIdentityRepairReport, SqliteStore, Store};
use serde::Deserialize;
use serde_json::Value;

use crate::path_identity::normalize_repo_relative_path;
use crate::protected_state::repo_streams::{
    implicit_principal_identity, inspect_protected_stream, rewrite_protected_stream_events,
};
use crate::protected_state::streams::{ProtectedRepoStream, ProtectedVerificationStatus};
use crate::PrismPaths;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LegacyPathIdentityRepairTargetReport {
    pub label: String,
    pub location: String,
    pub entry_label: String,
    pub scanned_entry_count: usize,
    pub entries_needing_repair: usize,
    pub repaired: bool,
}

#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct LegacyPathIdentityRepairReport {
    pub scanned_target_count: usize,
    pub targets_needing_repair: usize,
    pub repaired_target_count: usize,
    pub total_scanned_entry_count: usize,
    pub total_entries_needing_repair: usize,
    pub targets: Vec<LegacyPathIdentityRepairTargetReport>,
}

pub fn inspect_legacy_path_identity_state(root: &Path) -> Result<LegacyPathIdentityRepairReport> {
    inspect_or_repair_legacy_path_identity_state(root, false)
}

pub fn repair_legacy_path_identity_state(root: &Path) -> Result<LegacyPathIdentityRepairReport> {
    inspect_or_repair_legacy_path_identity_state(root, true)
}

fn inspect_or_repair_legacy_path_identity_state(
    root: &Path,
    apply_repairs: bool,
) -> Result<LegacyPathIdentityRepairReport> {
    let mut targets = Vec::new();
    let repo_roots = candidate_repo_roots(root)?;

    if let Some(report) = inspect_or_repair_repo_patch_stream(root, &repo_roots, apply_repairs)? {
        targets.push(report);
    }

    let paths = PrismPaths::for_workspace_root(root)?;
    let shared_runtime_path = paths.shared_runtime_db_path()?;
    if let Some(report) = inspect_or_repair_sqlite_patch_log(
        root,
        &repo_roots,
        &shared_runtime_path,
        "shared runtime patch log",
        apply_repairs,
    )? {
        targets.push(report);
    }
    let worktree_cache_path = paths.worktree_cache_dir().join("state.db");
    if let Some(report) = inspect_or_repair_sqlite_patch_log(
        root,
        &repo_roots,
        &worktree_cache_path,
        "worktree cache patch log",
        apply_repairs,
    )? {
        targets.push(report);
    }

    if let Some(report) = inspect_or_repair_graph_snapshot(
        root,
        &shared_runtime_path,
        "shared runtime graph snapshot",
        apply_repairs,
    )? {
        targets.push(report);
    }
    if let Some(report) = inspect_or_repair_graph_snapshot(
        root,
        &worktree_cache_path,
        "worktree cache graph snapshot",
        apply_repairs,
    )? {
        targets.push(report);
    }

    Ok(summarize_reports(targets))
}

fn inspect_or_repair_repo_patch_stream(
    root: &Path,
    repo_roots: &[PathBuf],
    apply_repairs: bool,
) -> Result<Option<LegacyPathIdentityRepairTargetReport>> {
    let stream = ProtectedRepoStream::patch_events();
    let path = root.join(stream.relative_path());
    if !path.exists() {
        return Ok(None);
    }

    let inspection = inspect_protected_stream::<OutcomeEvent>(root, &stream)?;
    if inspection.verification.verification_status != ProtectedVerificationStatus::Verified {
        bail!(
            "refused to repair repo patch stream {} because verification status is {:?}",
            path.display(),
            inspection.verification.verification_status
        );
    }

    let mut scanned_entry_count = 0usize;
    let mut entries_needing_repair = 0usize;
    let mut rewritten = Vec::with_capacity(inspection.payloads.len());
    for mut event in inspection.payloads {
        if event.kind == OutcomeKind::PatchApplied {
            scanned_entry_count += 1;
            if normalize_patch_event_paths(repo_roots, &mut event) {
                entries_needing_repair += 1;
            }
        }
        rewritten.push((event.meta.id.0.to_string(), event));
    }

    if apply_repairs && entries_needing_repair > 0 {
        rewrite_protected_stream_events(
            root,
            &stream,
            rewritten,
            &implicit_principal_identity(None, None),
        )?;
    }

    Ok(Some(LegacyPathIdentityRepairTargetReport {
        label: "repo patch stream".to_string(),
        location: stream.relative_path().display().to_string(),
        entry_label: "patch event(s)".to_string(),
        scanned_entry_count,
        entries_needing_repair,
        repaired: apply_repairs && entries_needing_repair > 0,
    }))
}

fn inspect_or_repair_sqlite_patch_log(
    _root: &Path,
    repo_roots: &[PathBuf],
    sqlite_path: &Path,
    label: &str,
    apply_repairs: bool,
) -> Result<Option<LegacyPathIdentityRepairTargetReport>> {
    if !sqlite_path.exists() {
        return Ok(None);
    }
    let mut store = SqliteStore::open(sqlite_path)?;
    let repair = if apply_repairs {
        store.repair_patch_path_identity_for_roots(repo_roots)?
    } else {
        store.inspect_patch_path_identity_for_roots(repo_roots)?
    };
    if repair.scanned_patch_event_count == 0 {
        return Ok(None);
    }
    Ok(Some(render_patch_log_report(
        label,
        sqlite_path,
        repair,
        apply_repairs,
    )))
}

fn inspect_or_repair_graph_snapshot(
    root: &Path,
    sqlite_path: &Path,
    label: &str,
    apply_repairs: bool,
) -> Result<Option<LegacyPathIdentityRepairTargetReport>> {
    if !sqlite_path.exists() {
        return Ok(None);
    }
    let mut store = SqliteStore::open(sqlite_path)?;
    let Some(mut graph) = store.load_graph()? else {
        return Ok(None);
    };

    let before_paths = graph
        .snapshot()
        .file_records
        .into_keys()
        .collect::<Vec<_>>();
    let scanned_entry_count = before_paths.len();
    let entries_needing_repair = before_paths
        .iter()
        .filter(|path| normalize_repo_relative_path(root, path.as_path()) != **path)
        .count();
    graph.bind_workspace_root(root);

    if apply_repairs && entries_needing_repair > 0 {
        store.save_graph_snapshot(&graph)?;
    }

    if scanned_entry_count == 0 {
        return Ok(None);
    }
    Ok(Some(LegacyPathIdentityRepairTargetReport {
        label: label.to_string(),
        location: sqlite_path.display().to_string(),
        entry_label: "file path(s)".to_string(),
        scanned_entry_count,
        entries_needing_repair,
        repaired: apply_repairs && entries_needing_repair > 0,
    }))
}

fn render_patch_log_report(
    label: &str,
    sqlite_path: &Path,
    repair: PatchPathIdentityRepairReport,
    apply_repairs: bool,
) -> LegacyPathIdentityRepairTargetReport {
    LegacyPathIdentityRepairTargetReport {
        label: label.to_string(),
        location: sqlite_path.display().to_string(),
        entry_label: "patch event(s)".to_string(),
        scanned_entry_count: repair.scanned_patch_event_count,
        entries_needing_repair: repair.patch_events_needing_repair,
        repaired: apply_repairs && repair.repaired,
    }
}

fn summarize_reports(
    targets: Vec<LegacyPathIdentityRepairTargetReport>,
) -> LegacyPathIdentityRepairReport {
    let scanned_target_count = targets.len();
    let targets_needing_repair = targets
        .iter()
        .filter(|target| target.entries_needing_repair > 0)
        .count();
    let repaired_target_count = targets.iter().filter(|target| target.repaired).count();
    let total_scanned_entry_count = targets
        .iter()
        .map(|target| target.scanned_entry_count)
        .sum();
    let total_entries_needing_repair = targets
        .iter()
        .map(|target| target.entries_needing_repair)
        .sum();
    LegacyPathIdentityRepairReport {
        scanned_target_count,
        targets_needing_repair,
        repaired_target_count,
        total_scanned_entry_count,
        total_entries_needing_repair,
        targets,
    }
}

fn normalize_patch_event_paths(repo_roots: &[PathBuf], event: &mut OutcomeEvent) -> bool {
    if event.kind != OutcomeKind::PatchApplied {
        return false;
    }
    let Some(metadata) = event.metadata.as_object_mut() else {
        return false;
    };

    let mut changed = false;
    if let Some(file_paths) = metadata.get_mut("filePaths").and_then(Value::as_array_mut) {
        for path in file_paths {
            changed |= normalize_json_path_value(repo_roots, path);
        }
    }
    if let Some(summaries) = metadata
        .get_mut("changedFilesSummary")
        .and_then(Value::as_array_mut)
    {
        for summary in summaries {
            let Some(file_path) = summary.get_mut("filePath") else {
                continue;
            };
            changed |= normalize_json_path_value(repo_roots, file_path);
        }
    }
    if let Some(symbols) = metadata
        .get_mut("changedSymbols")
        .and_then(Value::as_array_mut)
    {
        for symbol in symbols {
            let Some(file_path) = symbol.get_mut("filePath") else {
                continue;
            };
            changed |= normalize_json_path_value(repo_roots, file_path);
        }
    }
    changed
}

fn normalize_json_path_value(repo_roots: &[PathBuf], value: &mut Value) -> bool {
    let Some(path) = value.as_str() else {
        return false;
    };
    let normalized = normalize_repo_relative_path_candidates(repo_roots, Path::new(path))
        .to_string_lossy()
        .into_owned();
    if normalized == path {
        return false;
    }
    *value = Value::String(normalized);
    true
}

fn normalize_repo_relative_path_candidates(repo_roots: &[PathBuf], path: &Path) -> PathBuf {
    if path.is_relative() {
        return path.to_path_buf();
    }
    for root in repo_roots {
        let normalized = normalize_repo_relative_path(root, path);
        if normalized != path {
            return normalized;
        }
    }
    path.to_path_buf()
}

fn candidate_repo_roots(root: &Path) -> Result<Vec<PathBuf>> {
    let paths = PrismPaths::for_workspace_root(root)?;
    let mut roots = BTreeSet::new();
    roots.insert(root.canonicalize().unwrap_or_else(|_| root.to_path_buf()));

    let worktrees_dir = paths.repo_home_dir().join("worktrees");
    if worktrees_dir.exists() {
        for entry in std::fs::read_dir(worktrees_dir)? {
            let entry = entry?;
            let metadata_path = entry.path().join("worktree.json");
            if !metadata_path.exists() {
                continue;
            }
            let Ok(raw) = std::fs::read_to_string(&metadata_path) else {
                continue;
            };
            let Ok(metadata) = serde_json::from_str::<RepairWorktreeMetadata>(&raw) else {
                continue;
            };
            if !metadata.canonical_root.is_empty() {
                roots.insert(PathBuf::from(metadata.canonical_root));
            }
        }
    }

    Ok(roots.into_iter().collect())
}

#[derive(Debug, Deserialize)]
struct RepairWorktreeMetadata {
    canonical_root: String,
}
