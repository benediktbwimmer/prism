use std::path::Path;

use prism_js::ValidationCheckView;

pub(crate) fn same_workspace_file(
    workspace_root: Option<&Path>,
    expected: &str,
    actual: &str,
) -> bool {
    let expected = normalize_path(expected);
    let actual = normalize_path(actual);
    if actual == expected {
        return true;
    }
    let expected_has_dirs = expected.contains('/');
    if let Some(root) = workspace_root {
        let resolved = normalize_path(root.join(expected.as_str()).to_string_lossy().as_ref());
        if actual == resolved {
            return true;
        }
    }
    expected_has_dirs && is_absolute_like(&actual) && actual.ends_with(&format!("/{expected}"))
}

pub(crate) fn workspace_scoped_path(workspace_root: Option<&Path>, path: &str) -> String {
    let path = normalize_path(path);
    if path.starts_with("prism://") {
        return path;
    }
    if is_absolute_like(&path) {
        return path;
    }
    workspace_root
        .map(|root| normalize_path(root.join(path.as_str()).to_string_lossy().as_ref()))
        .unwrap_or(path)
}

pub(crate) fn workspace_display_path(workspace_root: Option<&Path>, path: &Path) -> String {
    if let Some(root) = workspace_root {
        if let Ok(relative) = path.strip_prefix(root) {
            return normalize_path(relative.to_string_lossy().as_ref());
        }
    }
    normalize_path(path.to_string_lossy().as_ref())
}

pub(crate) fn spec_body_identifier_terms(text: &str, limit: usize) -> Vec<String> {
    let mut terms = Vec::<String>::new();
    for raw in text.split(|ch: char| {
        !(ch.is_ascii_alphanumeric() || matches!(ch, '_' | ':' | '.' | '/' | '-'))
    }) {
        let candidate = raw
            .trim_matches(|ch: char| matches!(ch, '`' | '"' | '\''))
            .trim();
        if candidate.len() < 3 || candidate.chars().all(|ch| ch.is_ascii_digit()) {
            continue;
        }
        if !is_identifier_like(candidate) {
            continue;
        }
        if terms.iter().any(|existing| existing == candidate) {
            continue;
        }
        terms.push(candidate.to_string());
        if terms.len() >= limit {
            break;
        }
    }
    terms
}

pub(crate) fn compact_validation_checks(
    checks: &[String],
    scored_checks: &[ValidationCheckView],
    limit: usize,
    max_chars: usize,
) -> Vec<String> {
    let mut compact = Vec::<String>::new();

    for label in scored_checks.iter().map(|check| check.label.as_str()) {
        push_compact_check(&mut compact, label, max_chars);
        if compact.len() >= limit {
            return compact;
        }
    }
    for label in checks {
        push_compact_check(&mut compact, label, max_chars);
        if compact.len() >= limit {
            return compact;
        }
    }

    compact
}

fn push_compact_check(compact: &mut Vec<String>, label: &str, max_chars: usize) {
    let label = compact_check_label(label, max_chars);
    if label.is_empty() || compact.iter().any(|existing| existing == &label) {
        return;
    }
    compact.push(label);
}

fn compact_check_label(label: &str, max_chars: usize) -> String {
    let mut first_step = label
        .split(" && ")
        .next()
        .unwrap_or(label)
        .split(" || ")
        .next()
        .unwrap_or(label)
        .split(';')
        .next()
        .unwrap_or(label)
        .trim();
    if let Some(stripped) = first_step.strip_prefix("test:") {
        first_step = stripped.trim();
    } else if let Some(stripped) = first_step.strip_prefix("build:") {
        first_step = stripped.trim();
    }
    clamp_string(&collapse_whitespace(first_step), max_chars)
}

fn collapse_whitespace(value: &str) -> String {
    value.split_whitespace().collect::<Vec<_>>().join(" ")
}

fn normalize_path(path: &str) -> String {
    let path = path.replace('\\', "/");
    if path.is_empty() {
        return path;
    }
    if is_uri_like(&path) {
        return path;
    }
    let (prefix, absolute, remainder) = split_path_prefix(&path);
    let mut segments = Vec::<&str>::new();
    for segment in remainder.split('/') {
        if segment.is_empty() || segment == "." {
            continue;
        }
        if segment == ".." {
            if segments.last().is_some_and(|last| *last != "..") {
                segments.pop();
                continue;
            }
            if !absolute {
                segments.push(segment);
            }
            continue;
        }
        segments.push(segment);
    }
    if prefix.is_empty() {
        return segments.join("/");
    }
    if segments.is_empty() {
        return prefix;
    }
    if prefix.ends_with('/') {
        format!("{prefix}{}", segments.join("/"))
    } else {
        format!("{prefix}/{}", segments.join("/"))
    }
}

fn split_path_prefix(path: &str) -> (String, bool, &str) {
    if let Some(remainder) = path.strip_prefix("//") {
        return ("//".to_string(), true, remainder);
    }
    if let Some(remainder) = path.strip_prefix('/') {
        return ("/".to_string(), true, remainder);
    }
    let bytes = path.as_bytes();
    if bytes.len() >= 3 && bytes[1] == b':' && bytes[2] == b'/' {
        return (path[..3].to_string(), true, &path[3..]);
    }
    if bytes.len() >= 2 && bytes[1] == b':' {
        return (path[..2].to_string(), false, &path[2..]);
    }
    (String::new(), false, path)
}

fn is_uri_like(path: &str) -> bool {
    let Some((scheme, _rest)) = path.split_once("://") else {
        return false;
    };
    let mut chars = scheme.chars();
    matches!(chars.next(), Some(ch) if ch.is_ascii_alphabetic())
        && chars.all(|ch| ch.is_ascii_alphanumeric() || matches!(ch, '+' | '-' | '.'))
}

fn is_absolute_like(path: &str) -> bool {
    path.starts_with('/') || path.chars().nth(1) == Some(':')
}

fn clamp_string(value: &str, max_chars: usize) -> String {
    let mut truncated = value.chars().take(max_chars).collect::<String>();
    if value.chars().count() > max_chars && max_chars > 1 {
        truncated.truncate(max_chars.saturating_sub(1));
        truncated.push('…');
    }
    truncated
}

fn is_identifier_like(value: &str) -> bool {
    value.contains('_')
        || value.contains("::")
        || value.contains('/')
        || value.contains('.')
        || value.contains('-')
}

#[cfg(test)]
mod tests {
    use std::path::PathBuf;
    use std::time::{SystemTime, UNIX_EPOCH};

    use prism_js::ValidationCheckView;

    use super::*;

    fn temp_root(label: &str) -> PathBuf {
        let stamp = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        std::env::temp_dir().join(format!(
            "prism-compact-followups-tests-{label}-{}-{stamp}",
            std::process::id()
        ))
    }

    #[test]
    fn compact_validation_checks_trim_shell_chains() {
        let checks = compact_validation_checks(
            &["test:cargo test -p prism-mcp compact_locate && cargo test -p prism-mcp compact_open"
                .to_string()],
            &[ValidationCheckView {
                label: "cargo test -p prism-mcp compact_locate && cargo test -p prism-mcp compact_open"
                    .to_string(),
                score: 0.8,
                last_seen: 1,
            }],
            2,
            96,
        );

        assert_eq!(checks.len(), 1);
        assert_eq!(checks[0], "cargo test -p prism-mcp compact_locate");
    }

    #[test]
    fn same_workspace_file_accepts_relative_and_absolute_matches() {
        let root = temp_root("same-workspace-file");
        assert!(same_workspace_file(
            Some(root.as_path()),
            "Cargo.toml",
            "Cargo.toml"
        ));
        assert!(same_workspace_file(
            Some(root.as_path()),
            "Cargo.toml",
            root.join("Cargo.toml").to_string_lossy().as_ref()
        ));
        assert!(!same_workspace_file(
            Some(root.as_path()),
            "Cargo.toml",
            "crates/prism-cli/Cargo.toml"
        ));
        assert!(!same_workspace_file(
            Some(root.as_path()),
            "Cargo.toml",
            root.join("crates/prism-cli/Cargo.toml")
                .to_string_lossy()
                .as_ref()
        ));
    }

    #[test]
    fn workspace_scoped_path_resolves_relative_paths_against_workspace_root() {
        let root = temp_root("workspace-scoped-path");
        assert_eq!(
            workspace_scoped_path(Some(root.as_path()), "Cargo.toml"),
            root.join("Cargo.toml").to_string_lossy()
        );
        assert_eq!(
            workspace_scoped_path(Some(root.as_path()), "crates/prism-cli/Cargo.toml"),
            root.join("crates/prism-cli/Cargo.toml").to_string_lossy()
        );
        assert_eq!(
            workspace_scoped_path(
                Some(root.as_path()),
                root.join("Cargo.toml").to_string_lossy().as_ref()
            ),
            root.join("Cargo.toml").to_string_lossy()
        );
        assert_eq!(
            workspace_scoped_path(Some(root.as_path()), "./crates/../Cargo.toml"),
            root.join("Cargo.toml").to_string_lossy()
        );
    }

    #[test]
    fn workspace_display_path_prefers_repo_relative_paths() {
        let root = temp_root("workspace-display-path");
        assert_eq!(
            workspace_display_path(Some(root.as_path()), &root.join("src/lib.rs")),
            "src/lib.rs"
        );
        assert_eq!(
            workspace_display_path(Some(root.as_path()), &root.join("src/./nested/../lib.rs")),
            "src/lib.rs"
        );
    }

    #[test]
    fn workspace_display_path_keeps_external_absolute_paths() {
        let root = temp_root("workspace-display-external");
        let external = std::env::temp_dir().join("elsewhere.rs");
        assert_eq!(
            workspace_display_path(Some(root.as_path()), &external),
            external.to_string_lossy()
        );
    }

    #[test]
    fn same_workspace_file_accepts_dot_segment_variants() {
        let root = temp_root("same-workspace-dot-segments");
        assert!(same_workspace_file(
            Some(root.as_path()),
            "src/lib.rs",
            root.join("src/./nested/../lib.rs")
                .to_string_lossy()
                .as_ref()
        ));
    }

    #[test]
    fn spec_body_identifier_terms_keep_identifier_like_tokens() {
        let terms = spec_body_identifier_terms(
            "Use `prism_locate`, `prism_open`, and prism_mcp::server_surface::PrismMcpServer.",
            8,
        );
        assert!(terms.iter().any(|term| term == "prism_locate"));
        assert!(terms.iter().any(|term| term == "prism_open"));
        assert!(terms
            .iter()
            .any(|term| term == "prism_mcp::server_surface::PrismMcpServer."));
    }
}
