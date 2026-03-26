use anyhow::{Context, Result};
use prism_ir::{
    Edge, EdgeKind, EdgeOrigin, Language, Node, NodeId, NodeKind, Span, UnresolvedIntent,
};
use prism_parser::{
    document_name, document_path, extract_intent_targets, fingerprint_from_parts,
    intent_kind_for_context, normalized_shape_hash, whole_file_span, LanguageAdapter, ParseInput,
    ParseResult,
};
use serde_yaml::Value;
use smol_str::SmolStr;
use std::collections::HashMap;

pub struct YamlAdapter;

impl LanguageAdapter for YamlAdapter {
    fn language(&self) -> Language {
        Language::Yaml
    }

    fn supports_path(&self, path: &std::path::Path) -> bool {
        matches!(
            path.extension().and_then(|ext| ext.to_str()),
            Some("yaml" | "yml")
        )
    }

    fn parse(&self, input: &ParseInput<'_>) -> Result<ParseResult> {
        let value: Value = serde_yaml::from_str(input.source)
            .with_context(|| format!("failed to parse YAML file {}", input.path.display()))?;
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
            language: Language::Yaml,
        };
        result.record_fingerprint(
            &document_id,
            fingerprint_from_parts(["yaml", "document", document_shape.as_str()]),
        );
        result.nodes.push(document_node);
        let spans = YamlSpanIndex::new(input.source);
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
    spans: &YamlSpanIndex,
    key_path: Vec<String>,
) {
    match value {
        Value::Mapping(map) => {
            for (key, child) in map {
                let Value::String(key) = key else {
                    continue;
                };
                let path = format!("{prefix}::{key}");
                let id = NodeId::new(crate_name, path.clone(), NodeKind::YamlKey);
                let child_shape = value_shape(child);
                let mut current_key_path = key_path.clone();
                current_key_path.push(key.clone());
                let span = spans
                    .span_for_path(&current_key_path)
                    .unwrap_or_else(|| whole_file_span(input.source));
                let node = Node {
                    id: id.clone(),
                    name: SmolStr::new(key),
                    kind: NodeKind::YamlKey,
                    file: input.file_id,
                    span,
                    language: Language::Yaml,
                };
                result.record_fingerprint(
                    &id,
                    fingerprint_from_parts(["yaml", "key", child_shape.as_str()]),
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
        Value::Sequence(values) => {
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
        _ => {}
    }
}

fn value_shape(value: &Value) -> String {
    match value {
        Value::Null => "null".to_owned(),
        Value::Bool(_) => "bool".to_owned(),
        Value::Number(_) => "number".to_owned(),
        Value::String(_) => "string".to_owned(),
        Value::Sequence(values) => format!("sequence:{}", values.len()),
        Value::Mapping(map) => format!("mapping:{}", map.len()),
        Value::Tagged(tagged) => format!("tagged:{}", value_shape(&tagged.value)),
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
        Value::Sequence(values) => {
            for value in values {
                collect_value_targets(value, targets);
            }
        }
        Value::Mapping(map) => {
            for (key, value) in map {
                if let Value::String(key) = key {
                    targets.extend(extract_intent_targets(key));
                }
                collect_value_targets(value, targets);
            }
        }
        Value::Tagged(tagged) => collect_value_targets(&tagged.value, targets),
        _ => {}
    }
}

struct YamlSpanIndex {
    spans: HashMap<String, Span>,
}

impl YamlSpanIndex {
    fn new(source: &str) -> Self {
        let mut spans = HashMap::new();
        let mut stack = Vec::<(usize, Vec<String>)>::new();
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
                let (content, indent_offset) = if let Some(rest) = trimmed.strip_prefix("- ") {
                    (rest, indent + 2)
                } else {
                    (trimmed, indent)
                };
                if let Some((key, span, nests)) =
                    parse_yaml_key_line(content, line_start + indent_offset)
                {
                    while stack
                        .last()
                        .map(|(stack_indent, _)| *stack_indent >= indent_offset)
                        .unwrap_or(false)
                    {
                        stack.pop();
                    }
                    let mut path = stack
                        .last()
                        .map(|(_, path)| path.clone())
                        .unwrap_or_default();
                    path.push(key);
                    spans.entry(path.join("::")).or_insert(span);
                    if nests {
                        stack.push((indent_offset, path));
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

fn parse_yaml_key_line(line: &str, absolute_start: usize) -> Option<(String, Span, bool)> {
    for candidate in ['"', '\''] {
        if line.starts_with(candidate) {
            let closing = line[1..].find(candidate)? + 1;
            let remainder = line[closing + 1..].trim_start();
            if let Some(rest) = remainder.strip_prefix(':') {
                let key = line[1..closing].to_string();
                let span = Span::new(absolute_start + 1, absolute_start + closing);
                return Some((key, span, rest.trim().is_empty()));
            }
        }
    }
    let colon = line.find(':')?;
    let key = line[..colon].trim_end();
    if key.is_empty() || key.contains(' ') {
        return None;
    }
    let span = Span::new(absolute_start, absolute_start + key.len());
    Some((key.to_string(), span, line[colon + 1..].trim().is_empty()))
}

#[cfg(test)]
mod tests {
    use std::path::Path;

    use prism_ir::{FileId, NodeKind};
    use prism_parser::{LanguageAdapter, ParseInput};

    use super::YamlAdapter;

    #[test]
    fn parses_document_anchor_and_keys() {
        let adapter = YamlAdapter;
        let input = ParseInput {
            package_name: "demo",
            crate_name: "demo",
            package_root: Path::new("/workspace"),
            path: Path::new("/workspace/config/app.yaml"),
            file_id: FileId(1),
            source: "service:\n  port: 8080\n",
        };

        let result = adapter.parse(&input).unwrap();
        assert!(result
            .nodes
            .iter()
            .any(|node| node.kind == NodeKind::Document));
        let service = result
            .nodes
            .iter()
            .find(|node| node.kind == NodeKind::YamlKey && node.name == "service")
            .unwrap();
        assert_eq!(
            &input.source[service.span.start as usize..service.span.end as usize],
            "service"
        );
        assert_eq!(result.edges.len(), 2);
    }
}
