use prism_js::ValidationCheckView;

pub(crate) fn same_workspace_file(expected: &str, actual: &str) -> bool {
    let expected = normalize_path(expected);
    let actual = normalize_path(actual);
    actual == expected || (is_absolute_like(&actual) && actual.ends_with(&format!("/{expected}")))
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
        assert!(same_workspace_file("Cargo.toml", "Cargo.toml"));
        assert!(same_workspace_file(
            "Cargo.toml",
            "/Users/bene/code/prism/Cargo.toml"
        ));
        assert!(!same_workspace_file(
            "Cargo.toml",
            "crates/prism-cli/Cargo.toml"
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
