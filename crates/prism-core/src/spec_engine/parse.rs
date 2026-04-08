use std::collections::BTreeMap;
use std::fs;
use std::process::Command;

use serde_yaml::{Mapping, Value};

use super::types::{
    DiscoveredSpecSource, ParsedSpecDocument, SpecChecklistIdentitySource, SpecChecklistItem,
    SpecChecklistRequirementLevel, SpecDeclaredStatus, SpecDependency, SpecParseDiagnostic,
    SpecParseDiagnosticKind, SpecSourceMetadata, ParsedSpecSet,
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
    let dependencies = parse_dependencies(source, &frontmatter, &spec_id)?;
    let checklist_items = extract_checklist_items(body, &spec_id);
    let source_metadata = SpecSourceMetadata {
        repo_relative_path: source.repo_relative_path.clone(),
        content_digest: blake3::hash(contents.as_bytes()).to_hex().to_string(),
        git_revision: detect_git_revision(source),
    };

    Ok(ParsedSpecDocument {
        source: source.clone(),
        source_metadata,
        frontmatter,
        body: body.to_owned(),
        spec_id,
        title,
        status,
        created,
        checklist_items,
        dependencies,
    })
}

pub fn parse_spec_sources(sources: &[DiscoveredSpecSource]) -> ParsedSpecSet {
    let mut parsed = Vec::new();
    let mut diagnostics = Vec::new();

    for source in sources {
        match parse_spec_source(source) {
            Ok(document) => parsed.push(document),
            Err(diagnostic) => diagnostics.push(diagnostic),
        }
    }

    let mut first_seen_by_id = BTreeMap::<String, std::path::PathBuf>::new();
    for document in &parsed {
        if let Some(first_path) = first_seen_by_id.get(&document.spec_id) {
            diagnostics.push(SpecParseDiagnostic {
                source_path: document.source.repo_relative_path.clone(),
                kind: SpecParseDiagnosticKind::DuplicateSpecId,
                field: Some("id".to_owned()),
                message: format!(
                    "spec id `{}` is already declared by {}",
                    document.spec_id,
                    first_path.display()
                ),
            });
        } else {
            first_seen_by_id.insert(
                document.spec_id.clone(),
                document.source.repo_relative_path.clone(),
            );
        }
    }

    ParsedSpecSet { parsed, diagnostics }
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

fn parse_dependencies(
    source: &DiscoveredSpecSource,
    frontmatter: &Mapping,
    spec_id: &str,
) -> Result<Vec<SpecDependency>, SpecParseDiagnostic> {
    let key = Value::String("depends_on".to_owned());
    let Some(value) = frontmatter.get(&key) else {
        return Ok(Vec::new());
    };
    let sequence = value.as_sequence().ok_or_else(|| {
        build_diagnostic(
            source,
            SpecParseDiagnosticKind::InvalidFieldType,
            Some("depends_on"),
            "spec frontmatter field `depends_on` must be a YAML sequence of strings".to_owned(),
        )
    })?;

    let mut dependencies = Vec::with_capacity(sequence.len());
    for entry in sequence {
        let Some(dependency_id) = entry.as_str() else {
            return Err(build_diagnostic(
                source,
                SpecParseDiagnosticKind::InvalidFieldType,
                Some("depends_on"),
                "spec dependency entries must be strings".to_owned(),
            ));
        };
        let dependency_id = dependency_id.trim();
        if dependency_id.is_empty() {
            return Err(build_diagnostic(
                source,
                SpecParseDiagnosticKind::InvalidDependency,
                Some("depends_on"),
                "spec dependency entries must not be empty".to_owned(),
            ));
        }
        if dependency_id == spec_id {
            return Err(build_diagnostic(
                source,
                SpecParseDiagnosticKind::InvalidDependency,
                Some("depends_on"),
                format!("spec `{spec_id}` must not depend on itself"),
            ));
        }
        dependencies.push(SpecDependency {
            spec_id: dependency_id.to_owned(),
        });
    }

    Ok(dependencies)
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

fn detect_git_revision(source: &DiscoveredSpecSource) -> Option<String> {
    let parent = source.absolute_path.parent()?;
    let output = Command::new("git")
        .current_dir(parent)
        .args(["rev-parse", "HEAD"])
        .output()
        .ok()?;
    if !output.status.success() {
        return None;
    }
    let revision = String::from_utf8_lossy(&output.stdout).trim().to_owned();
    if revision.is_empty() {
        None
    } else {
        Some(revision)
    }
}

fn extract_checklist_items(body: &str, spec_id: &str) -> Vec<SpecChecklistItem> {
    #[derive(Clone)]
    struct SectionContext {
        level: usize,
        title: String,
        default_requirement_level: SpecChecklistRequirementLevel,
    }

    let mut sections = Vec::<SectionContext>::new();
    let mut generated_counts = BTreeMap::<String, usize>::new();
    let mut items = Vec::new();

    for (line_index, line) in body.lines().enumerate() {
        let trimmed = line.trim_start();
        if let Some((level, title, default_requirement_level)) = parse_heading(trimmed) {
            while sections.last().is_some_and(|section| section.level >= level) {
                sections.pop();
            }
            sections.push(SectionContext {
                level,
                title,
                default_requirement_level,
            });
            continue;
        }

        let Some((checked, raw_label)) = parse_checklist_line(trimmed) else {
            continue;
        };

        let section_path = sections
            .iter()
            .map(|section| section.title.clone())
            .collect::<Vec<_>>();
        let mut requirement_level = sections
            .last()
            .map(|section| section.default_requirement_level.clone())
            .unwrap_or(SpecChecklistRequirementLevel::Required);
        let (label_without_comment, explicit_id) = strip_explicit_id_annotation(raw_label);
        let label = strip_requirement_marker(label_without_comment, &mut requirement_level)
            .trim()
            .to_owned();
        if label.is_empty() {
            continue;
        }

        let (item_id, identity_source, explicit_annotation_id) = if let Some(explicit_id) =
            explicit_id
        {
            (
                format!("{spec_id}::checklist::{}", normalize_identity_fragment(&explicit_id)),
                SpecChecklistIdentitySource::Explicit,
                Some(explicit_id),
            )
        } else {
            let mut fragments = vec![
                normalize_identity_fragment(spec_id),
                "checklist".to_owned(),
            ];
            fragments.extend(
                section_path
                    .iter()
                    .map(|segment| normalize_identity_fragment(segment)),
            );
            fragments.push(normalize_identity_fragment(&label));
            let base = fragments.join("::");
            let occurrence = generated_counts.entry(base.clone()).or_insert(0);
            *occurrence += 1;
            let item_id = if *occurrence == 1 {
                base
            } else {
                format!("{base}::{}", occurrence)
            };
            (item_id, SpecChecklistIdentitySource::Generated, None)
        };

        items.push(SpecChecklistItem {
            item_id,
            identity_source,
            explicit_id: explicit_annotation_id,
            label,
            checked,
            requirement_level,
            section_path,
            line_number: line_index + 1,
        });
    }

    items
}

fn parse_heading(line: &str) -> Option<(usize, String, SpecChecklistRequirementLevel)> {
    let hashes = line.chars().take_while(|ch| *ch == '#').count();
    if hashes == 0 || hashes > 6 {
        return None;
    }
    let title = line[hashes..].trim();
    if title.is_empty() {
        return None;
    }
    let informational_suffix = "(informational)";
    if title
        .to_ascii_lowercase()
        .ends_with(informational_suffix)
    {
        let suffix_start = title.len() - informational_suffix.len();
        return Some((
            hashes,
            title[..suffix_start].trim_end().to_owned(),
            SpecChecklistRequirementLevel::Informational,
        ));
    }
    Some((
        hashes,
        title.to_owned(),
        SpecChecklistRequirementLevel::Required,
    ))
}

fn parse_checklist_line(line: &str) -> Option<(bool, &str)> {
    let marker = ["- [ ] ", "- [x] ", "- [X] ", "* [ ] ", "* [x] ", "* [X] ", "+ [ ] ", "+ [x] ", "+ [X] "]
        .into_iter()
        .find(|candidate| line.starts_with(candidate))?;
    let checked = matches!(marker.as_bytes()[3], b'x' | b'X');
    Some((checked, &line[marker.len()..]))
}

fn strip_explicit_id_annotation(label: &str) -> (&str, Option<String>) {
    let Some(comment_start) = label.rfind("<!--") else {
        return (label, None);
    };
    let Some(comment_end_relative) = label[comment_start..].find("-->") else {
        return (label, None);
    };
    let comment_end = comment_start + comment_end_relative + 3;
    let comment_body = label[comment_start + 4..comment_end - 3].trim();
    let Some(explicit_id) = comment_body.strip_prefix("id:") else {
        return (label, None);
    };
    let explicit_id = explicit_id.trim();
    if explicit_id.is_empty() {
        return (label, None);
    }
    (&label[..comment_start], Some(explicit_id.to_owned()))
}

fn strip_requirement_marker<'a>(
    label: &'a str,
    requirement_level: &mut SpecChecklistRequirementLevel,
) -> &'a str {
    let trimmed = label.trim_start();
    let lower = trimmed.to_ascii_lowercase();
    for marker in ["[info]", "[informational]"] {
        if lower.starts_with(marker) {
            *requirement_level = SpecChecklistRequirementLevel::Informational;
            return trimmed[marker.len()..].trim_start();
        }
    }
    trimmed
}

fn normalize_identity_fragment(text: &str) -> String {
    let mut normalized = String::new();
    let mut previous_was_separator = false;
    for ch in text.chars().flat_map(char::to_lowercase) {
        if ch.is_ascii_alphanumeric() {
            normalized.push(ch);
            previous_was_separator = false;
        } else if !previous_was_separator {
            normalized.push('-');
            previous_was_separator = true;
        }
    }
    normalized.trim_matches('-').to_owned()
}

#[cfg(test)]
mod tests {
    use std::collections::BTreeMap;
    use std::fs;
    use std::path::PathBuf;
    use std::sync::atomic::{AtomicU64, Ordering};
    use std::time::{SystemTime, UNIX_EPOCH};

    use super::{parse_spec_source, parse_spec_sources};
    use crate::spec_engine::{
        discover_spec_sources, SpecChecklistIdentitySource, SpecChecklistRequirementLevel,
        SpecDeclaredStatus, SpecParseDiagnosticKind,
    };

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
        assert!(parsed.checklist_items.is_empty());
        assert!(parsed.dependencies.is_empty());
        assert_eq!(
            parsed.source_metadata.repo_relative_path,
            PathBuf::from(".prism/specs/2026-04-09-sample.md")
        );
        assert_eq!(parsed.source_metadata.content_digest.len(), 64);
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

    #[test]
    fn parse_spec_source_extracts_checklist_items_with_explicit_ids() {
        let root = temp_repo("explicit-checklist-id");
        write_spec(
            &root,
            ".prism/specs/2026-04-09-sample.md",
            "---\nid: spec:sample\ntitle: Sample Spec\nstatus: draft\ncreated: 2026-04-09\n---\n\n## Build\n\n- [ ] implement parser <!-- id: parser -->\n",
        );

        let discovered = discover_spec_sources(&root).unwrap();
        let parsed = parse_spec_source(&discovered[0]).unwrap();
        assert_eq!(parsed.checklist_items.len(), 1);
        let item = &parsed.checklist_items[0];
        assert_eq!(item.item_id, "spec:sample::checklist::parser");
        assert_eq!(item.identity_source, SpecChecklistIdentitySource::Explicit);
        assert_eq!(item.explicit_id.as_deref(), Some("parser"));
        assert_eq!(item.label, "implement parser");
        assert_eq!(item.section_path, vec!["Build".to_owned()]);
        assert_eq!(
            item.requirement_level,
            SpecChecklistRequirementLevel::Required
        );
    }

    #[test]
    fn parse_spec_source_applies_informational_markers_and_section_defaults() {
        let root = temp_repo("info-markers");
        write_spec(
            &root,
            ".prism/specs/2026-04-09-sample.md",
            "---\nid: spec:sample\ntitle: Sample Spec\nstatus: draft\ncreated: 2026-04-09\n---\n\n## Context (informational)\n\n- [ ] note background\n\n## Build\n\n- [ ] [info] mention implementation detail\n- [ ] implement parser\n",
        );

        let discovered = discover_spec_sources(&root).unwrap();
        let parsed = parse_spec_source(&discovered[0]).unwrap();
        assert_eq!(parsed.checklist_items.len(), 3);
        assert_eq!(
            parsed.checklist_items[0].requirement_level,
            SpecChecklistRequirementLevel::Informational
        );
        assert_eq!(
            parsed.checklist_items[1].requirement_level,
            SpecChecklistRequirementLevel::Informational
        );
        assert_eq!(
            parsed.checklist_items[2].requirement_level,
            SpecChecklistRequirementLevel::Required
        );
    }

    #[test]
    fn parse_spec_source_generates_stable_checklist_ids_across_reordering() {
        let first_root = temp_repo("reorder-a");
        write_spec(
            &first_root,
            ".prism/specs/2026-04-09-sample.md",
            "---\nid: spec:sample\ntitle: Sample Spec\nstatus: draft\ncreated: 2026-04-09\n---\n\n## Build\n\n- [ ] implement parser\n- [ ] write tests\n",
        );
        let second_root = temp_repo("reorder-b");
        write_spec(
            &second_root,
            ".prism/specs/2026-04-09-sample.md",
            "---\nid: spec:sample\ntitle: Sample Spec\nstatus: draft\ncreated: 2026-04-09\n---\n\n## Build\n\n- [ ] write tests\n- [ ] implement parser\n",
        );

        let first = parse_spec_source(&discover_spec_sources(&first_root).unwrap()[0]).unwrap();
        let second = parse_spec_source(&discover_spec_sources(&second_root).unwrap()[0]).unwrap();

        let first_ids = first
            .checklist_items
            .iter()
            .map(|item| (item.label.clone(), item.item_id.clone()))
            .collect::<BTreeMap<_, _>>();
        let second_ids = second
            .checklist_items
            .iter()
            .map(|item| (item.label.clone(), item.item_id.clone()))
            .collect::<BTreeMap<_, _>>();
        assert_eq!(first_ids, second_ids);
    }

    #[test]
    fn parse_spec_source_preserves_dependency_order_and_rejects_self_dependency() {
        let root = temp_repo("dependencies");
        write_spec(
            &root,
            ".prism/specs/2026-04-09-sample.md",
            "---\nid: spec:sample\ntitle: Sample Spec\nstatus: draft\ncreated: 2026-04-09\ndepends_on:\n  - spec:alpha\n  - spec:beta\n---\n",
        );

        let discovered = discover_spec_sources(&root).unwrap();
        let parsed = parse_spec_source(&discovered[0]).unwrap();
        assert_eq!(
            parsed
                .dependencies
                .iter()
                .map(|dependency| dependency.spec_id.clone())
                .collect::<Vec<_>>(),
            vec!["spec:alpha".to_owned(), "spec:beta".to_owned()]
        );

        write_spec(
            &root,
            ".prism/specs/2026-04-09-self.md",
            "---\nid: spec:self\ntitle: Self Spec\nstatus: draft\ncreated: 2026-04-09\ndepends_on:\n  - spec:self\n---\n",
        );
        let discovered = discover_spec_sources(&root).unwrap();
        let self_source = discovered
            .iter()
            .find(|source| {
                source.repo_relative_path == PathBuf::from(".prism/specs/2026-04-09-self.md")
            })
            .unwrap();
        let diagnostic = parse_spec_source(self_source).unwrap_err();
        assert_eq!(diagnostic.kind, SpecParseDiagnosticKind::InvalidDependency);
    }

    #[test]
    fn parse_spec_sources_reports_duplicate_spec_ids() {
        let root = temp_repo("duplicate-spec-ids");
        write_spec(
            &root,
            ".prism/specs/2026-04-09-a.md",
            "---\nid: spec:duplicate\ntitle: First\nstatus: draft\ncreated: 2026-04-09\n---\n",
        );
        write_spec(
            &root,
            ".prism/specs/2026-04-09-b.md",
            "---\nid: spec:duplicate\ntitle: Second\nstatus: draft\ncreated: 2026-04-09\n---\n",
        );

        let discovered = discover_spec_sources(&root).unwrap();
        let parsed = parse_spec_sources(&discovered);
        assert_eq!(parsed.parsed.len(), 2);
        assert!(parsed
            .diagnostics
            .iter()
            .any(|diagnostic| diagnostic.kind == SpecParseDiagnosticKind::DuplicateSpecId));
    }
}
