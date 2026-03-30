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
    path.replace('\\', "/")
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
    use prism_js::ValidationCheckView;

    use super::*;

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
        let root = Path::new("/Users/bene/code/prism");
        assert!(same_workspace_file(Some(root), "Cargo.toml", "Cargo.toml"));
        assert!(same_workspace_file(
            Some(root),
            "Cargo.toml",
            "/Users/bene/code/prism/Cargo.toml"
        ));
        assert!(!same_workspace_file(
            Some(root),
            "Cargo.toml",
            "crates/prism-cli/Cargo.toml"
        ));
        assert!(!same_workspace_file(
            Some(root),
            "Cargo.toml",
            "/Users/bene/code/prism/crates/prism-cli/Cargo.toml"
        ));
    }

    #[test]
    fn workspace_scoped_path_resolves_relative_paths_against_workspace_root() {
        let root = Path::new("/Users/bene/code/prism");
        assert_eq!(
            workspace_scoped_path(Some(root), "Cargo.toml"),
            "/Users/bene/code/prism/Cargo.toml"
        );
        assert_eq!(
            workspace_scoped_path(Some(root), "crates/prism-cli/Cargo.toml"),
            "/Users/bene/code/prism/crates/prism-cli/Cargo.toml"
        );
        assert_eq!(
            workspace_scoped_path(Some(root), "/Users/bene/code/prism/Cargo.toml"),
            "/Users/bene/code/prism/Cargo.toml"
        );
    }

    #[test]
    fn workspace_display_path_prefers_repo_relative_paths() {
        let root = Path::new("/Users/bene/code/prism");
        assert_eq!(
            workspace_display_path(Some(root), &root.join("src/lib.rs")),
            "src/lib.rs"
        );
    }

    #[test]
    fn workspace_display_path_keeps_external_absolute_paths() {
        let root = Path::new("/Users/bene/code/prism");
        assert_eq!(
            workspace_display_path(Some(root), Path::new("/tmp/elsewhere.rs")),
            "/tmp/elsewhere.rs"
        );
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
