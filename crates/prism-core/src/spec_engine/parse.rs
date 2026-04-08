use std::fs;

use serde_yaml::{Mapping, Value};

use super::types::{
    DiscoveredSpecSource, ParsedSpecDocument, SpecDeclaredStatus, SpecParseDiagnostic,
    SpecParseDiagnosticKind,
};

pub fn parse_spec_source(
    source: &DiscoveredSpecSource,
) -> Result<ParsedSpecDocument, SpecParseDiagnostic> {
    let contents = fs::read_to_string(&source.absolute_path).map_err(|error| SpecParseDiagnostic {
        source_path: source.repo_relative_path.clone(),
        kind: SpecParseDiagnosticKind::MissingFrontmatter,
        field: None,
        message: format!("failed to read spec source: {error}"),
    })?;
    let (frontmatter_text, body) = split_frontmatter(&contents).map_err(|kind| {
        let message = match kind {
            SpecParseDiagnosticKind::MissingFrontmatter => {
                "spec file must begin with YAML frontmatter delimited by `---`".to_owned()
            }
            SpecParseDiagnosticKind::MissingClosingFrontmatter => {
                "spec frontmatter must end with a closing `---` delimiter".to_owned()
            }
            _ => "unexpected frontmatter parse error".to_owned(),
        };
        build_diagnostic(
            source,
            kind,
            None,
            message,
        )
    })?;

    let frontmatter_value: Value = serde_yaml::from_str(frontmatter_text).map_err(|error| {
        build_diagnostic(
            source,
            SpecParseDiagnosticKind::InvalidFrontmatterYaml,
            None,
            format!("failed to parse YAML frontmatter: {error}"),
        )
    })?;
    let frontmatter = frontmatter_value.as_mapping().cloned().ok_or_else(|| {
        build_diagnostic(
            source,
            SpecParseDiagnosticKind::InvalidFrontmatterYaml,
            None,
            "spec frontmatter must be a YAML mapping".to_owned(),
        )
    })?;

    let spec_id = required_string_field(source, &frontmatter, "id")?;
    let title = required_string_field(source, &frontmatter, "title")?;
    let status_text = required_string_field(source, &frontmatter, "status")?;
    let created = required_string_field(source, &frontmatter, "created")?;
    let status = SpecDeclaredStatus::parse(&status_text).ok_or_else(|| {
        build_diagnostic(
            source,
            SpecParseDiagnosticKind::InvalidStatus,
            Some("status"),
            format!(
                "spec status must be one of draft, in_progress, blocked, completed, superseded, or abandoned; found `{status_text}`"
            ),
        )
    })?;

    Ok(ParsedSpecDocument {
        source: source.clone(),
        frontmatter,
        body: body.to_owned(),
        spec_id,
        title,
        status,
        created,
    })
}

fn split_frontmatter(source: &str) -> Result<(&str, &str), SpecParseDiagnosticKind> {
    let mut offset = 0usize;
    let Some((first_line, first_len)) = next_line(source, offset) else {
        return Err(SpecParseDiagnosticKind::MissingFrontmatter);
    };
    if trim_line_ending(first_line) != "---" {
        return Err(SpecParseDiagnosticKind::MissingFrontmatter);
    }
    offset += first_len;
    let frontmatter_start = offset;

    while let Some((line, line_len)) = next_line(source, offset) {
        if trim_line_ending(line) == "---" {
            let frontmatter = &source[frontmatter_start..offset];
            let body = &source[offset + line_len..];
            return Ok((frontmatter, body));
        }
        offset += line_len;
    }

    Err(SpecParseDiagnosticKind::MissingClosingFrontmatter)
}

fn next_line(source: &str, start: usize) -> Option<(&str, usize)> {
    if start >= source.len() {
        return None;
    }
    let rest = &source[start..];
    if let Some(newline) = rest.find('\n') {
        let end = start + newline + 1;
        return Some((&source[start..end], newline + 1));
    }
    Some((&source[start..], rest.len()))
}

fn trim_line_ending(line: &str) -> &str {
    line.trim_end_matches('\n').trim_end_matches('\r')
}

fn required_string_field(
    source: &DiscoveredSpecSource,
    frontmatter: &Mapping,
    field: &'static str,
) -> Result<String, SpecParseDiagnostic> {
    let key = Value::String(field.to_owned());
    let Some(value) = frontmatter.get(&key) else {
        return Err(build_diagnostic(
            source,
            SpecParseDiagnosticKind::MissingRequiredField,
            Some(field),
            format!("spec frontmatter must include required field `{field}`"),
        ));
    };
    let Some(text) = value.as_str() else {
        return Err(build_diagnostic(
            source,
            SpecParseDiagnosticKind::InvalidFieldType,
            Some(field),
            format!("spec frontmatter field `{field}` must be a string"),
        ));
    };
    let trimmed = text.trim();
    if trimmed.is_empty() {
        return Err(build_diagnostic(
            source,
            SpecParseDiagnosticKind::InvalidFieldType,
            Some(field),
            format!("spec frontmatter field `{field}` must not be empty"),
        ));
    }
    Ok(trimmed.to_owned())
}

fn build_diagnostic(
    source: &DiscoveredSpecSource,
    kind: SpecParseDiagnosticKind,
    field: Option<&str>,
    message: String,
) -> SpecParseDiagnostic {
    SpecParseDiagnostic {
        source_path: source.repo_relative_path.clone(),
        kind,
        field: field.map(str::to_owned),
        message,
    }
}

#[cfg(test)]
mod tests {
    use std::fs;
    use std::path::PathBuf;
    use std::sync::atomic::{AtomicU64, Ordering};
    use std::time::{SystemTime, UNIX_EPOCH};

    use super::parse_spec_source;
    use crate::spec_engine::{discover_spec_sources, SpecDeclaredStatus, SpecParseDiagnosticKind};

    static NEXT_TEMP_REPO: AtomicU64 = AtomicU64::new(0);

    fn temp_repo(label: &str) -> PathBuf {
        let nonce = NEXT_TEMP_REPO.fetch_add(1, Ordering::Relaxed);
        let unique = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        let root = std::env::temp_dir().join(format!("prism-spec-parse-{label}-{unique}-{nonce}"));
        let _ = fs::remove_dir_all(&root);
        fs::create_dir_all(root.join(".git")).unwrap();
        fs::write(root.join("Cargo.toml"), "[workspace]\nmembers = []\n").unwrap();
        fs::write(root.join(".git/HEAD"), "ref: refs/heads/main\n").unwrap();
        root
    }

    fn write_spec(root: &PathBuf, relative_path: &str, contents: &str) {
        let path = root.join(relative_path);
        fs::create_dir_all(path.parent().unwrap()).unwrap();
        fs::write(path, contents).unwrap();
    }

    #[test]
    fn parse_spec_source_extracts_required_frontmatter_fields() {
        let root = temp_repo("valid");
        write_spec(
            &root,
            ".prism/specs/2026-04-09-sample.md",
            "---\nid: spec:sample\ntitle: Sample Spec\nstatus: in_progress\ncreated: 2026-04-09\n---\n\n# Summary\n",
        );

        let discovered = discover_spec_sources(&root).unwrap();
        let parsed = parse_spec_source(&discovered[0]).unwrap();
        assert_eq!(parsed.spec_id, "spec:sample");
        assert_eq!(parsed.title, "Sample Spec");
        assert_eq!(parsed.status, SpecDeclaredStatus::InProgress);
        assert_eq!(parsed.created, "2026-04-09");
        assert_eq!(parsed.body, "\n# Summary\n");
    }

    #[test]
    fn parse_spec_source_reports_missing_frontmatter() {
        let root = temp_repo("missing-frontmatter");
        write_spec(&root, ".prism/specs/2026-04-09-sample.md", "# Summary\n");

        let discovered = discover_spec_sources(&root).unwrap();
        let diagnostic = parse_spec_source(&discovered[0]).unwrap_err();
        assert_eq!(
            diagnostic.kind,
            SpecParseDiagnosticKind::MissingFrontmatter
        );
        assert_eq!(
            diagnostic.source_path,
            PathBuf::from(".prism/specs/2026-04-09-sample.md")
        );
    }

    #[test]
    fn parse_spec_source_reports_invalid_yaml() {
        let root = temp_repo("invalid-yaml");
        write_spec(
            &root,
            ".prism/specs/2026-04-09-sample.md",
            "---\nid: spec:sample\ntitle: [unterminated\nstatus: draft\ncreated: 2026-04-09\n---\n",
        );

        let discovered = discover_spec_sources(&root).unwrap();
        let diagnostic = parse_spec_source(&discovered[0]).unwrap_err();
        assert_eq!(
            diagnostic.kind,
            SpecParseDiagnosticKind::InvalidFrontmatterYaml
        );
    }

    #[test]
    fn parse_spec_source_reports_missing_required_field() {
        let root = temp_repo("missing-field");
        write_spec(
            &root,
            ".prism/specs/2026-04-09-sample.md",
            "---\nid: spec:sample\ntitle: Sample Spec\ncreated: 2026-04-09\n---\n",
        );

        let discovered = discover_spec_sources(&root).unwrap();
        let diagnostic = parse_spec_source(&discovered[0]).unwrap_err();
        assert_eq!(
            diagnostic.kind,
            SpecParseDiagnosticKind::MissingRequiredField
        );
        assert_eq!(diagnostic.field.as_deref(), Some("status"));
    }

    #[test]
    fn parse_spec_source_reports_invalid_status_values() {
        let root = temp_repo("invalid-status");
        write_spec(
            &root,
            ".prism/specs/2026-04-09-sample.md",
            "---\nid: spec:sample\ntitle: Sample Spec\nstatus: active\ncreated: 2026-04-09\n---\n",
        );

        let discovered = discover_spec_sources(&root).unwrap();
        let diagnostic = parse_spec_source(&discovered[0]).unwrap_err();
        assert_eq!(diagnostic.kind, SpecParseDiagnosticKind::InvalidStatus);
        assert_eq!(diagnostic.field.as_deref(), Some("status"));
    }
}
