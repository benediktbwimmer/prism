use std::collections::hash_map::DefaultHasher;
use std::fs;
use std::hash::{Hash, Hasher};
use std::path::{Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};

use anyhow::Result;
use ignore::{DirEntry, Walk, WalkBuilder};
use prism_lang_json::JsonAdapter;
use prism_lang_markdown::MarkdownAdapter;
use prism_lang_python::PythonAdapter;
use prism_lang_rust::RustAdapter;
use prism_lang_toml::TomlAdapter;
use prism_lang_yaml::YamlAdapter;
use prism_parser::LanguageAdapter;

use crate::PrismPaths;

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
    use std::cell::RefCell;
    use std::fs;
    use std::path::PathBuf;
    use std::sync::atomic::{AtomicU64, Ordering};
    use std::time::{SystemTime, UNIX_EPOCH};

    use super::{
        is_generated_projection_relative_path, persisted_file_hash, stable_hash_with_version,
        workspace_walk,
    };

    static NEXT_TEMP_UTIL_WORKSPACE: AtomicU64 = AtomicU64::new(0);

    thread_local! {
        static TEMP_TEST_DIRS: RefCell<TempTestDirState> = RefCell::new(TempTestDirState {
            paths: Vec::new(),
        });
    }

    struct TempTestDirState {
        paths: Vec<PathBuf>,
    }

    impl Drop for TempTestDirState {
        fn drop(&mut self) {
            for path in self.paths.drain(..).rev() {
                let _ = fs::remove_dir_all(path);
            }
        }
    }

    fn track_temp_dir(path: &std::path::Path) {
        TEMP_TEST_DIRS.with(|state| state.borrow_mut().paths.push(path.to_path_buf()));
    }

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
        fs::create_dir_all(root.join("docs/prism")).unwrap();
        fs::create_dir_all(root.join(".git")).unwrap();
        fs::create_dir_all(root.join(".prism")).unwrap();
        fs::create_dir_all(root.join("target")).unwrap();
        fs::create_dir_all(root.join("node_modules")).unwrap();
        fs::create_dir_all(root.join(".codex-target-trash-123")).unwrap();
        fs::write(root.join("src/lib.rs"), "pub fn live() {}\n").unwrap();
        fs::write(root.join("PRISM.md"), "# Derived\n").unwrap();
        fs::write(root.join("docs/prism/plans.md"), "# Derived Plans\n").unwrap();
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
        assert!(!walked.iter().any(|path| path == &PathBuf::from("PRISM.md")));
        assert!(!walked.iter().any(|path| path.starts_with("docs/prism")));
        assert!(!walked
            .iter()
            .any(|path| path.starts_with(".codex-target-trash-123")));

        let _ = fs::remove_dir_all(root);
    }

    #[test]
    fn generated_projection_paths_are_detected() {
        assert!(is_generated_projection_relative_path(
            PathBuf::from("PRISM.md").as_path()
        ));
        assert!(is_generated_projection_relative_path(
            PathBuf::from("docs/prism/plans/index.md").as_path()
        ));
        assert!(!is_generated_projection_relative_path(
            PathBuf::from("docs/notes.md").as_path()
        ));
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
        track_temp_dir(&root);
        root
    }
}

pub(crate) fn cache_path(root: &Path) -> Result<PathBuf> {
    PrismPaths::for_workspace_root(root)?.worktree_cache_db_path()
}

pub(crate) fn validation_feedback_path(root: &Path) -> Result<PathBuf> {
    PrismPaths::for_workspace_root(root)?.validation_feedback_path()
}

pub(crate) fn repo_memory_events_path(root: &Path) -> PathBuf {
    root.join(".prism").join("memory").join("events.jsonl")
}

pub(crate) fn repo_patch_events_path(root: &Path) -> PathBuf {
    root.join(".prism").join("changes").join("events.jsonl")
}

pub(crate) fn prism_doc_path(root: &Path) -> PathBuf {
    root.join("PRISM.md")
}

pub(crate) fn repo_concept_events_path(root: &Path) -> PathBuf {
    root.join(".prism").join("concepts").join("events.jsonl")
}

pub(crate) fn repo_contract_events_path(root: &Path) -> PathBuf {
    root.join(".prism").join("contracts").join("events.jsonl")
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

fn should_skip_workspace_walk_entry(root: &Path, entry: &DirEntry) -> bool {
    let Ok(relative) = entry.path().strip_prefix(root) else {
        return false;
    };
    is_ignored_workspace_walk_relative_path(relative)
}

pub(crate) fn is_generated_projection_relative_path(relative: &Path) -> bool {
    let components = relative
        .components()
        .map(|component| component.as_os_str().to_string_lossy().into_owned())
        .collect::<Vec<_>>();
    components.as_slice() == ["PRISM.md"]
        || (components.len() >= 2 && components[0] == "docs" && components[1] == "prism")
}

pub(crate) fn is_generated_projection_path(root: &Path, path: &Path) -> bool {
    path.strip_prefix(root)
        .ok()
        .is_some_and(is_generated_projection_relative_path)
}

fn is_ignored_workspace_walk_relative_path(relative: &Path) -> bool {
    if is_generated_projection_relative_path(relative) {
        return true;
    }
    relative.components().any(|component| {
        let name = component.as_os_str().to_string_lossy();
        matches!(name.as_ref(), ".git" | ".prism" | "target" | "node_modules")
            || name.starts_with(".codex-target-trash-")
    })
}

pub(crate) fn is_relevant_workspace_file(path: &Path) -> bool {
    matches!(
        path.extension().and_then(|ext| ext.to_str()),
        Some("rs" | "py" | "md" | "json" | "toml" | "yaml" | "yml")
    )
}

#[cfg(unix)]
pub(crate) fn metadata_changed_ns(metadata: &fs::Metadata) -> Option<u128> {
    use std::os::unix::fs::MetadataExt;

    Some((metadata.ctime() as u128) * 1_000_000_000 + (metadata.ctime_nsec() as u128))
}

#[cfg(not(unix))]
pub(crate) fn metadata_changed_ns(_metadata: &fs::Metadata) -> Option<u128> {
    None
}
