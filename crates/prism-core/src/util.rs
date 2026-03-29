use std::collections::hash_map::DefaultHasher;
use std::collections::HashMap;
use std::fs;
use std::hash::{Hash, Hasher};
use std::path::{Path, PathBuf};
use std::time::{Instant, SystemTime, UNIX_EPOCH};

use anyhow::Result;
use ignore::{DirEntry, Walk, WalkBuilder};
use prism_lang_json::JsonAdapter;
use prism_lang_markdown::MarkdownAdapter;
use prism_lang_python::PythonAdapter;
use prism_lang_rust::RustAdapter;
use prism_lang_toml::TomlAdapter;
use prism_lang_yaml::YamlAdapter;
use prism_parser::LanguageAdapter;
use tracing::info;

const INDEX_FORMAT_VERSION: u64 = 1;
const FINGERPRINT_SLOW_LOG_MS: u128 = 100;
const FINGERPRINT_LOG_TOP_PREFIXES: usize = 8;

pub(crate) fn current_timestamp() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .expect("system clock should be after unix epoch")
        .as_secs()
}

pub(crate) fn current_timestamp_millis() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .expect("system clock should be after unix epoch")
        .as_millis() as u64
}

pub(crate) fn persisted_file_hash(source: &str) -> u64 {
    stable_hash_with_version(source, INDEX_FORMAT_VERSION)
}

pub(crate) fn stable_hash_bytes(bytes: &[u8]) -> u64 {
    let mut hasher = DefaultHasher::new();
    bytes.hash(&mut hasher);
    hasher.finish()
}

fn stable_hash_with_version(source: &str, version: u64) -> u64 {
    let mut hasher = DefaultHasher::new();
    version.hash(&mut hasher);
    source.hash(&mut hasher);
    hasher.finish()
}

#[derive(Clone, Debug, Default)]
pub(crate) struct WorkspaceFingerprint {
    pub(crate) value: u64,
    pub(crate) files: HashMap<PathBuf, CachedWorkspaceFile>,
}

#[derive(Clone, Debug)]
pub(crate) struct CachedWorkspaceFile {
    pub(crate) len: u64,
    pub(crate) modified_ns: Option<u128>,
    pub(crate) changed_ns: Option<u128>,
    pub(crate) content_hash: u64,
}

pub(crate) fn workspace_fingerprint(
    root: &Path,
    cached: Option<&WorkspaceFingerprint>,
) -> Result<WorkspaceFingerprint> {
    let started = Instant::now();
    let mut hasher = DefaultHasher::new();
    let mut files = HashMap::new();
    let mut walk_entry_count = 0usize;
    let mut walk_file_count = 0usize;
    let mut relevant_file_count = 0usize;
    let mut cache_hits = 0usize;
    let mut cache_misses = 0usize;
    let mut bytes_read = 0u64;
    let mut top_level_file_counts = HashMap::new();
    let mut top_level_relevant_counts = HashMap::new();
    for entry in workspace_walk(root).filter_map(Result::ok) {
        walk_entry_count += 1;
        let path = entry.path();
        let is_file = entry
            .file_type()
            .map(|file_type| file_type.is_file())
            .unwrap_or(false);
        if !is_file {
            continue;
        }
        walk_file_count += 1;
        if let Some(prefix) = top_level_prefix(root, path) {
            *top_level_file_counts.entry(prefix).or_insert(0) += 1;
        }
        if !is_relevant_workspace_file(path) {
            continue;
        }
        relevant_file_count += 1;
        let Ok(relative) = path.strip_prefix(root) else {
            continue;
        };
        if let Some(prefix) = top_level_prefix(root, path) {
            *top_level_relevant_counts.entry(prefix).or_insert(0) += 1;
        }
        let metadata = match fs::metadata(path) {
            Ok(metadata) => metadata,
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => continue,
            Err(error) => return Err(error.into()),
        };
        let modified_ns = metadata
            .modified()
            .ok()
            .and_then(|value| value.duration_since(UNIX_EPOCH).ok())
            .map(|value| value.as_nanos());
        let changed_ns = metadata_changed_ns(&metadata);
        let cached_file = cached.and_then(|snapshot| snapshot.files.get(path));
        let content_hash = if cached_file.is_some_and(|file| {
            file.len == metadata.len()
                && file.modified_ns == modified_ns
                && file.changed_ns == changed_ns
        }) {
            cache_hits += 1;
            cached_file.expect("cached file should exist").content_hash
        } else {
            let bytes = match fs::read(path) {
                Ok(bytes) => bytes,
                Err(error) if error.kind() == std::io::ErrorKind::NotFound => continue,
                Err(error) => return Err(error.into()),
            };
            cache_misses += 1;
            bytes_read += bytes.len() as u64;
            stable_hash_bytes(&bytes)
        };
        relative.hash(&mut hasher);
        content_hash.hash(&mut hasher);
        files.insert(
            path.to_path_buf(),
            CachedWorkspaceFile {
                len: metadata.len(),
                modified_ns,
                changed_ns,
                content_hash,
            },
        );
    }
    let fingerprint = WorkspaceFingerprint {
        value: hasher.finish(),
        files,
    };
    let duration_ms = started.elapsed().as_millis();
    if duration_ms >= FINGERPRINT_SLOW_LOG_MS {
        info!(
            root = %root.display(),
            duration_ms,
            walk_entry_count,
            walk_file_count,
            relevant_file_count,
            cached_snapshot_file_count = cached.map(|snapshot| snapshot.files.len()).unwrap_or(0),
            cache_hits,
            cache_misses,
            bytes_read,
            fingerprint_file_count = fingerprint.files.len(),
            top_level_file_counts = ?summarize_prefix_counts(&top_level_file_counts),
            top_level_relevant_counts = ?summarize_prefix_counts(&top_level_relevant_counts),
            "computed workspace fingerprint"
        );
    }
    Ok(fingerprint)
}

pub(crate) fn default_adapters() -> Vec<Box<dyn LanguageAdapter + Send + Sync>> {
    vec![
        Box::new(RustAdapter),
        Box::new(PythonAdapter),
        Box::new(MarkdownAdapter),
        Box::new(JsonAdapter),
        Box::new(TomlAdapter),
        Box::new(YamlAdapter),
    ]
}

#[cfg(test)]
mod tests {
    use std::fs;
    use std::path::PathBuf;
    use std::sync::atomic::{AtomicU64, Ordering};
    use std::time::{SystemTime, UNIX_EPOCH};

    use super::{persisted_file_hash, stable_hash_with_version, workspace_walk};

    static NEXT_TEMP_UTIL_WORKSPACE: AtomicU64 = AtomicU64::new(0);

    #[test]
    fn persisted_file_hash_changes_when_index_format_version_changes() {
        let source = "pub fn alpha() {}\n";
        assert_eq!(
            persisted_file_hash(source),
            stable_hash_with_version(source, 1)
        );
        assert_ne!(
            stable_hash_with_version(source, 1),
            stable_hash_with_version(source, 2)
        );
    }

    #[test]
    fn workspace_walk_skips_hidden_junk_roots() {
        let root = temp_workspace();
        fs::create_dir_all(root.join("src")).unwrap();
        fs::create_dir_all(root.join(".git")).unwrap();
        fs::create_dir_all(root.join(".prism")).unwrap();
        fs::create_dir_all(root.join("target")).unwrap();
        fs::create_dir_all(root.join("node_modules")).unwrap();
        fs::create_dir_all(root.join(".codex-target-trash-123")).unwrap();
        fs::write(root.join("src/lib.rs"), "pub fn live() {}\n").unwrap();
        fs::write(root.join(".git/ignored.rs"), "pub fn ignored() {}\n").unwrap();
        fs::write(root.join(".prism/ignored.rs"), "pub fn ignored() {}\n").unwrap();
        fs::write(root.join("target/ignored.rs"), "pub fn ignored() {}\n").unwrap();
        fs::write(
            root.join("node_modules/ignored.rs"),
            "pub fn ignored() {}\n",
        )
        .unwrap();
        fs::write(
            root.join(".codex-target-trash-123/ignored.rs"),
            "pub fn ignored() {}\n",
        )
        .unwrap();

        let walked = workspace_walk(&root)
            .filter_map(Result::ok)
            .map(|entry| entry.path().strip_prefix(&root).unwrap().to_path_buf())
            .collect::<Vec<_>>();

        assert!(walked
            .iter()
            .any(|path| path == &PathBuf::from("src/lib.rs")));
        assert!(!walked.iter().any(|path| path.starts_with(".git")));
        assert!(!walked.iter().any(|path| path.starts_with(".prism")));
        assert!(!walked.iter().any(|path| path.starts_with("target")));
        assert!(!walked.iter().any(|path| path.starts_with("node_modules")));
        assert!(!walked
            .iter()
            .any(|path| path.starts_with(".codex-target-trash-123")));

        let _ = fs::remove_dir_all(root);
    }

    fn temp_workspace() -> PathBuf {
        let nonce = NEXT_TEMP_UTIL_WORKSPACE.fetch_add(1, Ordering::Relaxed);
        let unique = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        let root = std::env::temp_dir().join(format!("prism-util-{unique}-{nonce}"));
        let _ = fs::remove_dir_all(&root);
        fs::create_dir_all(&root).unwrap();
        root
    }
}

pub(crate) fn cache_path(root: &Path) -> PathBuf {
    root.join(".prism").join("cache.db")
}

pub(crate) fn validation_feedback_path(root: &Path) -> PathBuf {
    root.join(".prism").join("validation_feedback.jsonl")
}

pub(crate) fn repo_memory_events_path(root: &Path) -> PathBuf {
    root.join(".prism").join("memory").join("events.jsonl")
}

pub(crate) fn repo_concept_events_path(root: &Path) -> PathBuf {
    root.join(".prism").join("concepts").join("events.jsonl")
}

pub(crate) fn repo_concept_relations_path(root: &Path) -> PathBuf {
    root.join(".prism").join("concepts").join("relations.jsonl")
}

pub(crate) fn repo_plans_dir(root: &Path) -> PathBuf {
    root.join(".prism").join("plans")
}

pub(crate) fn repo_plan_index_path(root: &Path) -> PathBuf {
    repo_plans_dir(root).join("index.jsonl")
}

pub(crate) fn repo_active_plans_dir(root: &Path) -> PathBuf {
    repo_plans_dir(root).join("active")
}

pub(crate) fn repo_archived_plans_dir(root: &Path) -> PathBuf {
    repo_plans_dir(root).join("archived")
}

pub(crate) fn cleanup_legacy_cache(root: &Path) -> Result<()> {
    let legacy = root.join(".prism").join("cache.bin");
    if legacy.exists() {
        fs::remove_file(legacy)?;
    }
    Ok(())
}

pub(crate) fn workspace_walk(root: &Path) -> Walk {
    let mut builder = WalkBuilder::new(root);
    builder.hidden(false);
    builder.ignore(false);
    builder.git_ignore(true);
    builder.git_global(true);
    builder.git_exclude(true);
    builder.parents(true);
    builder.require_git(false);
    builder.sort_by_file_path(|left, right| left.cmp(right));
    let walk_root = root.to_path_buf();
    builder.filter_entry(move |entry| !should_skip_workspace_walk_entry(&walk_root, entry));
    builder.build()
}

fn top_level_prefix(root: &Path, path: &Path) -> Option<String> {
    path.strip_prefix(root)
        .ok()?
        .components()
        .next()
        .map(|component| component.as_os_str().to_string_lossy().into_owned())
}

fn should_skip_workspace_walk_entry(root: &Path, entry: &DirEntry) -> bool {
    let Ok(relative) = entry.path().strip_prefix(root) else {
        return false;
    };
    is_ignored_workspace_walk_relative_path(relative)
}

fn is_ignored_workspace_walk_relative_path(relative: &Path) -> bool {
    relative.components().any(|component| {
        let name = component.as_os_str().to_string_lossy();
        matches!(name.as_ref(), ".git" | ".prism" | "target" | "node_modules")
            || name.starts_with(".codex-target-trash-")
    })
}

fn summarize_prefix_counts(counts: &HashMap<String, usize>) -> Vec<String> {
    let mut entries = counts
        .iter()
        .map(|(prefix, count)| (prefix.clone(), *count))
        .collect::<Vec<_>>();
    entries.sort_by(|left, right| right.1.cmp(&left.1).then_with(|| left.0.cmp(&right.0)));
    entries
        .into_iter()
        .take(FINGERPRINT_LOG_TOP_PREFIXES)
        .map(|(prefix, count)| format!("{prefix}:{count}"))
        .collect()
}

fn is_relevant_workspace_file(path: &Path) -> bool {
    matches!(
        path.extension().and_then(|ext| ext.to_str()),
        Some("rs" | "py" | "md" | "json" | "toml" | "yaml" | "yml")
    )
}

#[cfg(unix)]
fn metadata_changed_ns(metadata: &fs::Metadata) -> Option<u128> {
    use std::os::unix::fs::MetadataExt;

    Some((metadata.ctime() as u128) * 1_000_000_000 + (metadata.ctime_nsec() as u128))
}

#[cfg(not(unix))]
fn metadata_changed_ns(_metadata: &fs::Metadata) -> Option<u128> {
    None
}
