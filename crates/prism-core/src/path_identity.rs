use std::path::{Component, Path, PathBuf};

pub(crate) fn normalize_repo_relative_path(root: &Path, path: &Path) -> PathBuf {
    if path.is_relative() {
        return normalize_path_components(path);
    }
    let canonical = path.canonicalize().unwrap_or_else(|_| path.to_path_buf());
    canonical
        .strip_prefix(root)
        .or_else(|_| path.strip_prefix(root))
        .map(normalize_path_components)
        .unwrap_or_else(|_| path.to_path_buf())
}

pub(crate) fn repo_relative_string(root: &Path, path: &Path) -> String {
    normalize_repo_relative_path(root, path)
        .to_string_lossy()
        .into_owned()
}

pub(crate) fn is_repo_relative_path_string(path: &str) -> bool {
    let trimmed = path.trim();
    if trimmed.is_empty() {
        return false;
    }
    if trimmed.starts_with('/') || trimmed.starts_with("\\\\") {
        return false;
    }
    let bytes = trimmed.as_bytes();
    if bytes.len() >= 2 && bytes[0].is_ascii_alphabetic() && bytes[1] == b':' {
        return false;
    }
    true
}

fn normalize_path_components(path: &Path) -> PathBuf {
    let mut normalized = PathBuf::new();
    for component in path.components() {
        match component {
            Component::CurDir => {}
            Component::ParentDir => normalized.push(component.as_os_str()),
            Component::Normal(part) => normalized.push(part),
            other => normalized.push(other.as_os_str()),
        }
    }
    normalized
}
