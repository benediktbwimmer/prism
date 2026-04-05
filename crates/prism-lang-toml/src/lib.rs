use anyhow::{Context, Result};
use prism_ir::{
    Edge, EdgeKind, EdgeOrigin, Language, Node, NodeId, NodeKind, Span, UnresolvedIntent,
};
use prism_parser::{
    document_name, document_path, extract_intent_targets, fingerprint_from_parts,
    intent_kind_for_context, normalized_shape_hash, whole_file_span, LanguageAdapter, ParseInput,
    ParseResult,
};
use smol_str::SmolStr;
use std::collections::HashMap;
use toml::Value;

pub struct TomlAdapter;

impl LanguageAdapter for TomlAdapter {
    fn language(&self) -> Language {
        Language::Toml
    }

    fn supports_path(&self, path: &std::path::Path) -> bool {
        matches!(path.extension().and_then(|ext| ext.to_str()), Some("toml"))
    }

    fn parse(&self, input: &ParseInput<'_>) -> Result<ParseResult> {
        let value: Value = input
            .source
            .parse()
            .with_context(|| format!("failed to parse TOML file {}", input.path.display()))?;
        let mut result = ParseResult::default();
        let document_path = document_path(input);
        let document_id = NodeId::new(input.crate_name, document_path.clone(), NodeKind::Document);
        let document_shape = normalized_shape_hash(input.source);
        let document_node = Node {
            id: document_id.clone(),
            name: SmolStr::new(document_name(input)),
            kind: NodeKind::Document,
            file: input.file_id,
            span: Span::whole_file(input.source.len()),
            language: Language::Toml,
        };
        result.record_fingerprint(
            &document_id,
            fingerprint_from_parts(["toml", "document", document_shape.as_str()]),
        );
        result.nodes.push(document_node);
        let spans = TomlSpanIndex::new(input.source);
        walk_value(
            input,
            &mut result,
            &value,
            Some(document_id),
            &document_path,
            input.crate_name,
            &spans,
            Vec::new(),
        );
        Ok(result)
    }
}

fn walk_value(
    input: &ParseInput<'_>,
    result: &mut ParseResult,
    value: &Value,
    parent: Option<NodeId>,
    prefix: &str,
    crate_name: &str,
    spans: &TomlSpanIndex,
    key_path: Vec<String>,
) {
    match value {
        Value::Table(table) => {
            for (key, child) in table {
                let path = format!("{prefix}::{key}");
                let id = NodeId::new(crate_name, path.clone(), NodeKind::TomlKey);
                let child_shape = value_shape(child);
                let mut current_key_path = key_path.clone();
                current_key_path.push(key.clone());
                let span = spans
                    .span_for_path(&current_key_path)
                    .unwrap_or_else(|| whole_file_span(input.source));
                let node = Node {
                    id: id.clone(),
                    name: SmolStr::new(key),
                    kind: NodeKind::TomlKey,
                    file: input.file_id,
                    span,
                    language: Language::Toml,
                };
                result.record_fingerprint(
                    &id,
                    fingerprint_from_parts(["toml", "key", child_shape.as_str()]),
                );
                result.nodes.push(node);
                if let Some(parent) = &parent {
                    result.edges.push(Edge {
                        kind: EdgeKind::Contains,
                        source: parent.clone(),
                        target: id.clone(),
                        origin: EdgeOrigin::Static,
                        confidence: 1.0,
                    });
                }
                let intent_kind = intent_kind_for_context(&path, EdgeKind::RelatedTo);
                for target in intent_targets_for_value(key, child) {
                    result.unresolved_intents.push(UnresolvedIntent {
                        source: id.clone(),
                        kind: intent_kind,
                        target: target.into(),
                        span,
                    });
                }
                walk_value(
                    input,
                    result,
                    child,
                    Some(id),
                    &path,
                    crate_name,
                    spans,
                    current_key_path,
                );
            }
        }
        Value::Array(values) => {
            for (index, child) in values.iter().enumerate() {
                let path = format!("{prefix}::[{index}]");
                walk_value(
                    input,
                    result,
                    child,
                    parent.clone(),
                    &path,
                    crate_name,
                    spans,
                    key_path.clone(),
                );
            }
        }
        Value::String(_)
        | Value::Integer(_)
        | Value::Float(_)
        | Value::Boolean(_)
        | Value::Datetime(_) => {}
    }
}

fn value_shape(value: &Value) -> String {
    match value {
        Value::String(_) => "string".to_owned(),
        Value::Integer(_) => "integer".to_owned(),
        Value::Float(_) => "float".to_owned(),
        Value::Boolean(_) => "boolean".to_owned(),
        Value::Datetime(_) => "datetime".to_owned(),
        Value::Array(values) => format!("array:{}", values.len()),
        Value::Table(table) => format!("table:{}", table.len()),
    }
}

fn intent_targets_for_value(key: &str, value: &Value) -> Vec<String> {
    let mut targets = extract_intent_targets(key);
    collect_value_targets(value, &mut targets);
    targets.sort();
    targets.dedup();
    targets
}

fn collect_value_targets(value: &Value, targets: &mut Vec<String>) {
    match value {
        Value::String(text) => targets.extend(extract_intent_targets(text)),
        Value::Array(values) => {
            for value in values {
                collect_value_targets(value, targets);
            }
        }
        Value::Table(table) => {
            for (key, value) in table {
                targets.extend(extract_intent_targets(key));
                collect_value_targets(value, targets);
            }
        }
        Value::Integer(_) | Value::Float(_) | Value::Boolean(_) | Value::Datetime(_) => {}
    }
}

struct TomlSpanIndex {
    spans: HashMap<String, Span>,
}

impl TomlSpanIndex {
    fn new(source: &str) -> Self {
        let mut spans = HashMap::new();
        let mut current_table = Vec::<String>::new();
        let mut line_start = 0usize;

        while line_start < source.len() {
            let line_end = source[line_start..]
                .find('\n')
                .map(|offset| line_start + offset)
                .unwrap_or(source.len());
            let line = &source[line_start..line_end];
            let indent = line
                .char_indices()
                .find(|(_, ch)| !matches!(ch, ' ' | '\t'))
                .map(|(offset, _)| offset)
                .unwrap_or(line.len());
            let trimmed = &line[indent..];
            if !trimmed.is_empty() && !trimmed.starts_with('#') {
                if let Some(segments) = parse_toml_header_segments(trimmed, line_start + indent) {
                    current_table.clear();
                    for (segment, span) in segments {
                        current_table.push(segment);
                        spans.entry(current_table.join("::")).or_insert(span);
                    }
                } else if let Some(segments) =
                    parse_toml_assignment_segments(trimmed, line_start + indent)
                {
                    let mut path = current_table.clone();
                    for (segment, span) in segments {
                        path.push(segment);
                        spans.entry(path.join("::")).or_insert(span);
                    }
                }
            }
            if line_end == source.len() {
                break;
            }
            line_start = line_end + 1;
        }

        Self { spans }
    }

    fn span_for_path(&self, path: &[String]) -> Option<Span> {
        self.spans.get(&path.join("::")).copied()
    }
}

fn parse_toml_header_segments(line: &str, absolute_start: usize) -> Option<Vec<(String, Span)>> {
    let (content_start, content_end) = if let Some(rest) = line.strip_prefix("[[") {
        let end = rest.find("]]")?;
        (2, 2 + end)
    } else if let Some(rest) = line.strip_prefix('[') {
        let end = rest.find(']')?;
        (1, 1 + end)
    } else {
        return None;
    };
    parse_dotted_toml_segments(
        &line[content_start..content_end],
        absolute_start + content_start,
    )
}

fn parse_toml_assignment_segments(
    line: &str,
    absolute_start: usize,
) -> Option<Vec<(String, Span)>> {
    let equals = line.find('=')?;
    parse_dotted_toml_segments(line[..equals].trim_end(), absolute_start)
}

fn parse_dotted_toml_segments(text: &str, absolute_start: usize) -> Option<Vec<(String, Span)>> {
    let mut segments = Vec::new();
    let mut segment_start = 0usize;
    for segment in text.split('.') {
        let trimmed = segment.trim();
        if trimmed.is_empty() {
            return None;
        }
        let leading_ws = segment.len() - segment.trim_start().len();
        let quoted = trimmed.starts_with('"') || trimmed.starts_with('\'');
        let normalized = trimmed.trim_matches('"').trim_matches('\'').to_string();
        let raw_start = segment_start + leading_ws + usize::from(quoted);
        let span = Span::new(
            absolute_start + raw_start,
            absolute_start + raw_start + normalized.len(),
        );
        segments.push((normalized, span));
        segment_start += segment.len() + 1;
    }
    Some(segments)
}

#[cfg(test)]
mod tests {
    use std::path::Path;

    use prism_ir::{FileId, NodeKind};
    use prism_parser::{LanguageAdapter, ParseDepth, ParseInput};

    use super::TomlAdapter;

    #[test]
    fn parses_document_anchor_and_keys() {
        let adapter = TomlAdapter;
        let input = ParseInput {
            package_name: "demo",
            crate_name: "demo",
            package_root: Path::new("workspace"),
            path: Path::new("workspace/Cargo.toml"),
            file_id: FileId(1),
            parse_depth: ParseDepth::Deep,
            source: "[workspace]\nmembers = [\"crates/alpha\"]\n[dependencies]\nserde = \"1.0\"\n",
        };

        let result = adapter.parse(&input).unwrap();
        assert!(result
            .nodes
            .iter()
            .any(|node| node.kind == NodeKind::Document));
        let workspace = result
            .nodes
            .iter()
            .find(|node| node.kind == NodeKind::TomlKey && node.name == "workspace")
            .unwrap();
        assert_eq!(
            &input.source[workspace.span.start as usize..workspace.span.end as usize],
            "workspace"
        );
        assert!(result
            .nodes
            .iter()
            .any(|node| node.kind == NodeKind::TomlKey && node.name == "members"));
        assert!(result
            .nodes
            .iter()
            .any(|node| node.kind == NodeKind::TomlKey && node.name == "dependencies"));
        assert!(result
            .nodes
            .iter()
            .any(|node| node.kind == NodeKind::TomlKey && node.name == "serde"));
        assert_eq!(result.edges.len(), 4);
    }
}
