use std::collections::hash_map::DefaultHasher;
use std::collections::HashMap;
use std::fs;
use std::hash::{Hash, Hasher};
use std::path::{Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};

use anyhow::Result;
use ignore::{Walk, WalkBuilder};
use prism_lang_json::JsonAdapter;
use prism_lang_markdown::MarkdownAdapter;
use prism_lang_python::PythonAdapter;
use prism_lang_rust::RustAdapter;
use prism_lang_toml::TomlAdapter;
use prism_lang_yaml::YamlAdapter;
use prism_parser::LanguageAdapter;

const INDEX_FORMAT_VERSION: u64 = 1;

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
    let mut hasher = DefaultHasher::new();
    let mut files = HashMap::new();
    for entry in workspace_walk(root).filter_map(Result::ok) {
        let path = entry.path();
        if !entry
            .file_type()
            .map(|file_type| file_type.is_file())
            .unwrap_or(false)
            || !is_relevant_workspace_file(path)
        {
            continue;
        }
        let Ok(relative) = path.strip_prefix(root) else {
            continue;
        };
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
            cached_file.expect("cached file should exist").content_hash
        } else {
            let bytes = match fs::read(path) {
                Ok(bytes) => bytes,
                Err(error) if error.kind() == std::io::ErrorKind::NotFound => continue,
                Err(error) => return Err(error.into()),
            };
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
    Ok(WorkspaceFingerprint {
        value: hasher.finish(),
        files,
    })
}

pub(crate) fn default_adapters() -> Vec<Box<dyn LanguageAdapter>> {
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
    use super::{persisted_file_hash, stable_hash_with_version};

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
}

pub(crate) fn cache_path(root: &Path) -> PathBuf {
    root.join(".prism").join("cache.db")
}

pub(crate) fn validation_feedback_path(root: &Path) -> PathBuf {
    root.join(".prism").join("validation_feedback.jsonl")
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
    builder.build()
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
