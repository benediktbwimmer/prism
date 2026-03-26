use std::collections::hash_map::DefaultHasher;
use std::fs;
use std::hash::{Hash, Hasher};
use std::path::{Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};

use anyhow::Result;
use prism_lang_json::JsonAdapter;
use prism_lang_markdown::MarkdownAdapter;
use prism_lang_rust::RustAdapter;
use prism_lang_yaml::YamlAdapter;
use prism_parser::LanguageAdapter;

pub(crate) fn current_timestamp() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .expect("system clock should be after unix epoch")
        .as_secs()
}

pub(crate) fn stable_hash(source: &str) -> u64 {
    let mut hasher = DefaultHasher::new();
    source.hash(&mut hasher);
    hasher.finish()
}

pub(crate) fn default_adapters() -> Vec<Box<dyn LanguageAdapter>> {
    vec![
        Box::new(RustAdapter),
        Box::new(MarkdownAdapter),
        Box::new(JsonAdapter),
        Box::new(YamlAdapter),
    ]
}

pub(crate) fn cache_path(root: &Path) -> PathBuf {
    root.join(".prism").join("cache.db")
}

pub(crate) fn cleanup_legacy_cache(root: &Path) -> Result<()> {
    let legacy = root.join(".prism").join("cache.bin");
    if legacy.exists() {
        fs::remove_file(legacy)?;
    }
    Ok(())
}

pub(crate) fn should_walk(path: &Path, root: &Path) -> bool {
    let Ok(relative) = path.strip_prefix(root) else {
        return true;
    };
    let Some(first) = relative.components().next() else {
        return true;
    };
    let first = first.as_os_str().to_string_lossy();
    !matches!(first.as_ref(), ".git" | ".prism" | "target")
}
