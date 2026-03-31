use std::collections::{hash_map::DefaultHasher, BTreeMap, BTreeSet, HashMap};
use std::fs;
use std::hash::{Hash, Hasher};
use std::path::{Path, PathBuf};

use anyhow::Result;
use prism_store::{
    WorkspaceTreeDirectoryFingerprint, WorkspaceTreeFileFingerprint, WorkspaceTreeSnapshot,
};

use crate::layout::WorkspaceLayout;
use crate::util::{
    is_relevant_workspace_file, metadata_changed_ns, stable_hash_bytes, workspace_walk,
};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum WorkspaceRefreshMode {
    Incremental,
    Rescan,
    Full,
}

impl WorkspaceRefreshMode {
    pub(crate) fn as_str(self) -> &'static str {
        match self {
            Self::Incremental => "incremental",
            Self::Rescan => "rescan",
            Self::Full => "full",
        }
    }
}

fn fallback_refresh_mode(cached: &WorkspaceTreeSnapshot) -> WorkspaceRefreshMode {
    if cached.files.is_empty() && cached.directories.is_empty() {
        WorkspaceRefreshMode::Full
    } else {
        WorkspaceRefreshMode::Rescan
    }
}

#[derive(Debug, Clone, Default)]
pub(crate) struct WorkspaceRefreshDelta {
    pub(crate) changed_files: BTreeSet<PathBuf>,
    pub(crate) removed_files: BTreeSet<PathBuf>,
    pub(crate) changed_directories: BTreeSet<PathBuf>,
    pub(crate) changed_packages: BTreeSet<PathBuf>,
    pub(crate) unaffected_directories: BTreeSet<PathBuf>,
    pub(crate) unaffected_packages: BTreeSet<PathBuf>,
}

impl WorkspaceRefreshDelta {
    pub(crate) fn is_empty(&self) -> bool {
        self.changed_files.is_empty()
            && self.removed_files.is_empty()
            && self.changed_directories.is_empty()
    }

    pub(crate) fn scope_paths(&self) -> BTreeSet<PathBuf> {
        self.changed_files
            .iter()
            .chain(self.removed_files.iter())
            .cloned()
            .collect()
    }
}

#[derive(Debug, Clone)]
pub(crate) struct WorkspaceRefreshPlan {
    pub(crate) mode: WorkspaceRefreshMode,
    pub(crate) delta: WorkspaceRefreshDelta,
    pub(crate) next_snapshot: WorkspaceTreeSnapshot,
}

pub(crate) fn build_workspace_tree_snapshot(
    root: &Path,
    cached: Option<&WorkspaceTreeSnapshot>,
) -> Result<WorkspaceTreeSnapshot> {
    if let Some(cached) = cached {
        return build_workspace_tree_snapshot_from_cached(root, cached);
    }

    let mut files = BTreeMap::new();
    for entry in workspace_walk(root).filter_map(Result::ok) {
        let path = entry.path();
        let is_file = entry
            .file_type()
            .map(|file_type| file_type.is_file())
            .unwrap_or(false);
        if !is_file || !is_relevant_workspace_file(path) {
            continue;
        }
        let fingerprint = file_fingerprint(
            path,
            cached.and_then(|snapshot| snapshot.files.get(path)),
            false,
        )?;
        files.insert(path.to_path_buf(), fingerprint);
    }
    Ok(snapshot_from_files(root, files))
}

fn build_workspace_tree_snapshot_from_cached(
    root: &Path,
    cached: &WorkspaceTreeSnapshot,
) -> Result<WorkspaceTreeSnapshot> {
    let mut next_snapshot = cached.clone();

    for path in cached.files.keys().cloned().collect::<Vec<_>>() {
        refresh_file_entry(&path, &mut next_snapshot, cached, false)?;
    }

    for directory in changed_directory_scan_roots(root, cached)? {
        let seen =
            collect_subtree_files_with_options(&directory, cached, &mut next_snapshot, false)?;
        remove_missing_subtree_files(&directory, &seen, &mut next_snapshot);
    }

    rebuild_directory_fingerprints(root, &mut next_snapshot);
    Ok(next_snapshot)
}

pub(crate) fn plan_full_refresh(
    root: &Path,
    cached: &WorkspaceTreeSnapshot,
) -> Result<WorkspaceRefreshPlan> {
    let next_snapshot = build_workspace_tree_snapshot(root, Some(cached))?;
    let delta = diff_workspace_tree_snapshot(root, cached, &next_snapshot);
    Ok(WorkspaceRefreshPlan {
        mode: fallback_refresh_mode(cached),
        delta,
        next_snapshot,
    })
}

pub(crate) fn plan_incremental_refresh(
    root: &Path,
    cached: &WorkspaceTreeSnapshot,
    dirty_paths: &[PathBuf],
) -> Result<WorkspaceRefreshPlan> {
    let mut next_snapshot = cached.clone();
    let mut touched_subtree_files = BTreeSet::new();

    for dirty_path in dirty_paths
        .iter()
        .map(|path| normalize_workspace_path(root, path))
        .collect::<BTreeSet<_>>()
    {
        if !dirty_path.starts_with(root) {
            continue;
        }

        if dirty_path.exists() {
            if dirty_path.is_file() {
                refresh_file_entry(&dirty_path, &mut next_snapshot, cached, true)?;
                touched_subtree_files.insert(dirty_path);
                continue;
            }

            if dirty_path.is_dir() {
                let subtree_files = collect_subtree_files(&dirty_path, cached, &mut next_snapshot)?;
                remove_missing_subtree_files(&dirty_path, &subtree_files, &mut next_snapshot);
                touched_subtree_files.extend(subtree_files);
                continue;
            }
        }

        remove_missing_subtree_files(&dirty_path, &BTreeSet::new(), &mut next_snapshot);
    }

    rebuild_directory_fingerprints(root, &mut next_snapshot);
    let delta = diff_workspace_tree_snapshot(root, cached, &next_snapshot);
    Ok(WorkspaceRefreshPlan {
        mode: WorkspaceRefreshMode::Incremental,
        delta,
        next_snapshot,
    })
}

pub(crate) fn populate_package_regions(
    delta: &mut WorkspaceRefreshDelta,
    layout: &WorkspaceLayout,
) {
    let changed_regions = delta.scope_paths();
    let changed_packages = layout
        .packages
        .iter()
        .filter(|package| {
            changed_regions
                .iter()
                .any(|path| path.starts_with(&package.root))
                || delta
                    .changed_directories
                    .iter()
                    .any(|path| path.starts_with(&package.root))
        })
        .map(|package| package.root.clone())
        .collect::<BTreeSet<_>>();
    let unaffected_packages = layout
        .packages
        .iter()
        .map(|package| package.root.clone())
        .filter(|path| !changed_packages.contains(path))
        .collect::<BTreeSet<_>>();
    delta.changed_packages = changed_packages;
    delta.unaffected_packages = unaffected_packages;
}

fn collect_subtree_files(
    root: &Path,
    cached_snapshot: &WorkspaceTreeSnapshot,
    next_snapshot: &mut WorkspaceTreeSnapshot,
) -> Result<BTreeSet<PathBuf>> {
    collect_subtree_files_with_options(root, cached_snapshot, next_snapshot, true)
}

fn collect_subtree_files_with_options(
    root: &Path,
    cached_snapshot: &WorkspaceTreeSnapshot,
    next_snapshot: &mut WorkspaceTreeSnapshot,
    force_rehash: bool,
) -> Result<BTreeSet<PathBuf>> {
    let mut seen = BTreeSet::new();
    for entry in workspace_walk(root).filter_map(Result::ok) {
        let path = entry.path();
        let is_file = entry
            .file_type()
            .map(|file_type| file_type.is_file())
            .unwrap_or(false);
        if !is_file || !is_relevant_workspace_file(path) {
            continue;
        }
        refresh_file_entry(path, next_snapshot, cached_snapshot, force_rehash)?;
        seen.insert(path.to_path_buf());
    }
    Ok(seen)
}

fn refresh_file_entry(
    path: &Path,
    next_snapshot: &mut WorkspaceTreeSnapshot,
    cached_snapshot: &WorkspaceTreeSnapshot,
    force_rehash: bool,
) -> Result<()> {
    if !is_relevant_workspace_file(path) {
        next_snapshot.files.remove(path);
        return Ok(());
    }

    let fingerprint = file_fingerprint(path, cached_snapshot.files.get(path), force_rehash)?;
    next_snapshot.files.insert(path.to_path_buf(), fingerprint);
    Ok(())
}

fn remove_missing_subtree_files(
    dirty_path: &Path,
    seen_files: &BTreeSet<PathBuf>,
    next_snapshot: &mut WorkspaceTreeSnapshot,
) {
    let removed = next_snapshot
        .files
        .keys()
        .filter(|path| path.starts_with(dirty_path) && !seen_files.contains(*path))
        .cloned()
        .collect::<Vec<_>>();
    for path in removed {
        next_snapshot.files.remove(&path);
    }
}

fn file_fingerprint(
    path: &Path,
    cached: Option<&WorkspaceTreeFileFingerprint>,
    force_rehash: bool,
) -> Result<WorkspaceTreeFileFingerprint> {
    let metadata = match fs::metadata(path) {
        Ok(metadata) => metadata,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
            return Ok(WorkspaceTreeFileFingerprint {
                len: 0,
                modified_ns: None,
                changed_ns: None,
                content_hash: 0,
            });
        }
        Err(error) => return Err(error.into()),
    };
    let modified_ns = metadata
        .modified()
        .ok()
        .and_then(|value| value.duration_since(std::time::UNIX_EPOCH).ok())
        .map(|value| value.as_nanos());
    let changed_ns = metadata_changed_ns(&metadata);
    let content_hash = if !force_rehash
        && cached.is_some_and(|file| {
            file.len == metadata.len()
                && file.modified_ns == modified_ns
                && file.changed_ns == changed_ns
        }) {
        cached
            .expect("cached fingerprint should exist")
            .content_hash
    } else {
        stable_hash_bytes(&fs::read(path)?)
    };
    Ok(WorkspaceTreeFileFingerprint {
        len: metadata.len(),
        modified_ns,
        changed_ns,
        content_hash,
    })
}

fn normalize_workspace_path(root: &Path, path: &Path) -> PathBuf {
    let candidate = if path.is_relative() {
        root.join(path)
    } else {
        path.to_path_buf()
    };
    if let Ok(canonical) = candidate.canonicalize() {
        return canonical;
    }
    if let (Some(parent), Some(name)) = (candidate.parent(), candidate.file_name()) {
        if let Ok(canonical_parent) = parent.canonicalize() {
            return canonical_parent.join(name);
        }
    }
    candidate
}

fn snapshot_from_files(
    root: &Path,
    files: BTreeMap<PathBuf, WorkspaceTreeFileFingerprint>,
) -> WorkspaceTreeSnapshot {
    let directories = directory_fingerprints(root, &files);
    let root_hash = directories
        .get(root)
        .map(|fingerprint| fingerprint.aggregate_hash)
        .unwrap_or_default();
    WorkspaceTreeSnapshot {
        root_hash,
        files,
        directories,
    }
}

fn rebuild_directory_fingerprints(root: &Path, snapshot: &mut WorkspaceTreeSnapshot) {
    snapshot.directories = directory_fingerprints(root, &snapshot.files);
    snapshot.root_hash = snapshot
        .directories
        .get(root)
        .map(|fingerprint| fingerprint.aggregate_hash)
        .unwrap_or_default();
}

fn directory_fingerprints(
    root: &Path,
    files: &BTreeMap<PathBuf, WorkspaceTreeFileFingerprint>,
) -> BTreeMap<PathBuf, WorkspaceTreeDirectoryFingerprint> {
    let mut inputs = HashMap::<PathBuf, Vec<(PathBuf, u64)>>::new();
    inputs.entry(root.to_path_buf()).or_default();
    for (path, fingerprint) in files {
        let mut current = path.parent();
        while let Some(directory) = current {
            if !directory.starts_with(root) {
                break;
            }
            inputs
                .entry(directory.to_path_buf())
                .or_default()
                .push((path.clone(), fingerprint.content_hash));
            if directory == root {
                break;
            }
            current = directory.parent();
        }
    }

    inputs
        .into_iter()
        .map(|(directory, mut entries)| {
            entries.sort_by(|left, right| left.0.cmp(&right.0));
            let mut hasher = DefaultHasher::new();
            for (path, content_hash) in &entries {
                path.hash(&mut hasher);
                content_hash.hash(&mut hasher);
            }
            let modified_ns = directory_modified_ns(&directory);
            let changed_ns = directory_changed_ns(&directory);
            (
                directory,
                WorkspaceTreeDirectoryFingerprint {
                    aggregate_hash: hasher.finish(),
                    file_count: entries.len(),
                    modified_ns,
                    changed_ns,
                },
            )
        })
        .collect()
}

fn changed_directory_scan_roots(
    root: &Path,
    cached: &WorkspaceTreeSnapshot,
) -> Result<Vec<PathBuf>> {
    let mut changed = Vec::new();
    for (path, fingerprint) in &cached.directories {
        if directory_scan_changed(path, fingerprint)? {
            changed.push(path.clone());
        }
    }
    changed.sort_by_key(|path| path.components().count());

    let mut roots = Vec::new();
    for path in changed {
        if path == root
            || !roots
                .iter()
                .any(|parent: &PathBuf| path.starts_with(parent))
        {
            roots.push(path);
        }
    }
    Ok(roots)
}

fn directory_scan_changed(path: &Path, cached: &WorkspaceTreeDirectoryFingerprint) -> Result<bool> {
    let metadata = match fs::metadata(path) {
        Ok(metadata) => metadata,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(true),
        Err(error) => return Err(error.into()),
    };
    let modified_ns = metadata
        .modified()
        .ok()
        .and_then(|value| value.duration_since(std::time::UNIX_EPOCH).ok())
        .map(|value| value.as_nanos());
    let changed_ns = metadata_changed_ns(&metadata);
    Ok(cached.modified_ns != modified_ns || cached.changed_ns != changed_ns)
}

fn directory_modified_ns(path: &Path) -> Option<u128> {
    fs::metadata(path)
        .ok()
        .and_then(|metadata| metadata.modified().ok())
        .and_then(|value| value.duration_since(std::time::UNIX_EPOCH).ok())
        .map(|value| value.as_nanos())
}

fn directory_changed_ns(path: &Path) -> Option<u128> {
    fs::metadata(path)
        .ok()
        .and_then(|metadata| metadata_changed_ns(&metadata))
}

pub(crate) fn diff_workspace_tree_snapshot(
    root: &Path,
    previous: &WorkspaceTreeSnapshot,
    next: &WorkspaceTreeSnapshot,
) -> WorkspaceRefreshDelta {
    let changed_files = next
        .files
        .iter()
        .filter(|(path, fingerprint)| previous.files.get(*path) != Some(*fingerprint))
        .map(|(path, _)| path.clone())
        .collect::<BTreeSet<_>>();
    let removed_files = previous
        .files
        .keys()
        .filter(|path| !next.files.contains_key(*path))
        .cloned()
        .collect::<BTreeSet<_>>();
    let changed_directories = next
        .directories
        .iter()
        .filter(|(path, fingerprint)| {
            path.as_path() != root && previous.directories.get(*path) != Some(*fingerprint)
        })
        .map(|(path, _)| path.clone())
        .chain(
            previous
                .directories
                .keys()
                .filter(|path| path.as_path() != root && !next.directories.contains_key(*path))
                .cloned(),
        )
        .collect::<BTreeSet<_>>();
    let top_level_directories = next
        .directories
        .keys()
        .chain(previous.directories.keys())
        .filter(|path| path.as_path() != root && path.parent() == Some(root))
        .cloned()
        .collect::<BTreeSet<_>>();
    let unaffected_directories = top_level_directories
        .into_iter()
        .filter(|path| !changed_directories.contains(path))
        .collect::<BTreeSet<_>>();

    WorkspaceRefreshDelta {
        changed_files,
        removed_files,
        changed_directories,
        changed_packages: BTreeSet::new(),
        unaffected_directories,
        unaffected_packages: BTreeSet::new(),
    }
}

#[cfg(test)]
mod tests {
    use std::fs;
    use std::path::PathBuf;
    use std::sync::atomic::{AtomicU64, Ordering};
    use std::time::{SystemTime, UNIX_EPOCH};

    use super::{build_workspace_tree_snapshot, plan_full_refresh, plan_incremental_refresh};

    static NEXT_TEMP_WORKSPACE: AtomicU64 = AtomicU64::new(0);

    #[test]
    fn incremental_refresh_scopes_changed_file_and_directory() {
        let root = temp_workspace();
        fs::create_dir_all(root.join("src")).unwrap();
        fs::create_dir_all(root.join("docs")).unwrap();
        fs::write(root.join("src/lib.rs"), "pub fn alpha() {}\n").unwrap();
        fs::write(root.join("docs/guide.md"), "# Guide\n").unwrap();

        let snapshot = build_workspace_tree_snapshot(&root, None).unwrap();
        fs::write(
            root.join("src/lib.rs"),
            "pub fn alpha() { println!(\"hi\"); }\n",
        )
        .unwrap();

        let plan = plan_incremental_refresh(&root, &snapshot, &[root.join("src/lib.rs")]).unwrap();

        assert_eq!(
            plan.delta.changed_files,
            [root.join("src/lib.rs")].into_iter().collect()
        );
        assert!(plan.delta.removed_files.is_empty());
        assert!(plan.delta.changed_directories.contains(&root.join("src")));
        assert!(plan
            .delta
            .unaffected_directories
            .contains(&root.join("docs")));

        let _ = fs::remove_dir_all(root);
    }

    #[test]
    fn incremental_refresh_detects_removed_file() {
        let root = temp_workspace();
        fs::create_dir_all(root.join("src")).unwrap();
        fs::write(root.join("src/lib.rs"), "pub fn alpha() {}\n").unwrap();
        fs::write(root.join("src/bin.rs"), "pub fn beta() {}\n").unwrap();

        let snapshot = build_workspace_tree_snapshot(&root, None).unwrap();
        fs::remove_file(root.join("src/bin.rs")).unwrap();

        let plan = plan_incremental_refresh(&root, &snapshot, &[root.join("src/bin.rs")]).unwrap();

        assert!(plan.delta.changed_files.is_empty());
        assert_eq!(
            plan.delta.removed_files,
            [root.join("src/bin.rs")].into_iter().collect()
        );
        assert!(plan.delta.changed_directories.contains(&root.join("src")));

        let _ = fs::remove_dir_all(root);
    }

    #[test]
    fn full_refresh_detects_in_place_file_edits_from_cached_snapshot() {
        let root = temp_workspace();
        fs::create_dir_all(root.join("src")).unwrap();
        fs::write(root.join("src/lib.rs"), "pub fn alpha() {}\n").unwrap();

        let snapshot = build_workspace_tree_snapshot(&root, None).unwrap();
        fs::write(
            root.join("src/lib.rs"),
            "pub fn alpha() { println!(\"updated\"); }\n",
        )
        .unwrap();

        let plan = plan_full_refresh(&root, &snapshot).unwrap();

        assert_eq!(plan.mode, super::WorkspaceRefreshMode::Rescan);
        assert_eq!(
            plan.delta.changed_files,
            [root.join("src/lib.rs")].into_iter().collect()
        );
        assert!(plan.delta.changed_directories.contains(&root.join("src")));

        let _ = fs::remove_dir_all(root);
    }

    #[test]
    fn full_refresh_detects_new_files_from_changed_directories() {
        let root = temp_workspace();
        fs::create_dir_all(root.join("src")).unwrap();
        fs::write(root.join("src/lib.rs"), "pub fn alpha() {}\n").unwrap();

        let snapshot = build_workspace_tree_snapshot(&root, None).unwrap();
        fs::write(root.join("src/new.rs"), "pub fn beta() {}\n").unwrap();

        let plan = plan_full_refresh(&root, &snapshot).unwrap();

        assert_eq!(plan.mode, super::WorkspaceRefreshMode::Rescan);
        assert!(plan.delta.changed_files.contains(&root.join("src/new.rs")));
        assert!(plan.delta.changed_directories.contains(&root.join("src")));

        let _ = fs::remove_dir_all(root);
    }

    fn temp_workspace() -> PathBuf {
        let nonce = NEXT_TEMP_WORKSPACE.fetch_add(1, Ordering::Relaxed);
        let unique = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        let root = std::env::temp_dir().join(format!("prism-tree-{unique}-{nonce}"));
        let _ = fs::remove_dir_all(&root);
        fs::create_dir_all(&root).unwrap();
        root.canonicalize().unwrap()
    }
}
