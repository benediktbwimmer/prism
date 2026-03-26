use anyhow::{Context, Result};
use prism_ir::{
    Edge, EdgeKind, EdgeOrigin, Language, Node, NodeId, NodeKind, Span, UnresolvedIntent,
};
use prism_parser::{
    document_name, document_path, extract_intent_targets, fingerprint_from_parts,
    intent_kind_for_context, normalized_shape_hash, LanguageAdapter, ParseInput, ParseResult,
};
use serde_json::Value;
use smol_str::SmolStr;

pub struct JsonAdapter;

impl LanguageAdapter for JsonAdapter {
    fn language(&self) -> Language {
        Language::Json
    }

    fn supports_path(&self, path: &std::path::Path) -> bool {
        matches!(path.extension().and_then(|ext| ext.to_str()), Some("json"))
    }

    fn parse(&self, input: &ParseInput<'_>) -> Result<ParseResult> {
        let value = parse_json_document(input.source)
            .with_context(|| format!("failed to parse JSON file {}", input.path.display()))?;
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
            language: Language::Json,
        };
        result.record_fingerprint(
            &document_id,
            fingerprint_from_parts(["json", "document", document_shape.as_str()]),
        );
        result.nodes.push(document_node);
        walk_value(
            input,
            &mut result,
            &value,
            Some(document_id),
            &document_path,
            input.crate_name,
        );
        Ok(result)
    }
}

fn parse_json_document(source: &str) -> Result<Value> {
    match serde_json::from_str(source) {
        Ok(value) => Ok(value),
        Err(_) => {
            let without_comments = strip_json_comments(source);
            let normalized = strip_trailing_commas(&without_comments);
            serde_json::from_str(&normalized).context("input is not valid JSON or JSONC")
        }
    }
}

fn strip_json_comments(source: &str) -> String {
    let chars = source.chars().collect::<Vec<_>>();
    let mut output = String::with_capacity(source.len());
    let mut index = 0;
    let mut in_string = false;
    let mut escape = false;

    while index < chars.len() {
        let ch = chars[index];
        if in_string {
            output.push(ch);
            if escape {
                escape = false;
            } else if ch == '\\' {
                escape = true;
            } else if ch == '"' {
                in_string = false;
            }
            index += 1;
            continue;
        }

        if ch == '"' {
            in_string = true;
            output.push(ch);
            index += 1;
            continue;
        }

        if ch == '/' && index + 1 < chars.len() {
            match chars[index + 1] {
                '/' => {
                    output.push(' ');
                    output.push(' ');
                    index += 2;
                    while index < chars.len() {
                        let comment_ch = chars[index];
                        if comment_ch == '\n' {
                            output.push('\n');
                            index += 1;
                            break;
                        }
                        output.push(if comment_ch == '\r' { '\r' } else { ' ' });
                        index += 1;
                    }
                    continue;
                }
                '*' => {
                    output.push(' ');
                    output.push(' ');
                    index += 2;
                    while index < chars.len() {
                        let comment_ch = chars[index];
                        if comment_ch == '*' && index + 1 < chars.len() && chars[index + 1] == '/' {
                            output.push(' ');
                            output.push(' ');
                            index += 2;
                            break;
                        }
                        output.push(if matches!(comment_ch, '\n' | '\r') {
                            comment_ch
                        } else {
                            ' '
                        });
                        index += 1;
                    }
                    continue;
                }
                _ => {}
            }
        }

        output.push(ch);
        index += 1;
    }

    output
}

fn strip_trailing_commas(source: &str) -> String {
    let chars = source.chars().collect::<Vec<_>>();
    let mut output = String::with_capacity(source.len());
    let mut in_string = false;
    let mut escape = false;

    for (index, ch) in chars.iter().copied().enumerate() {
        if in_string {
            output.push(ch);
            if escape {
                escape = false;
            } else if ch == '\\' {
                escape = true;
            } else if ch == '"' {
                in_string = false;
            }
            continue;
        }

        if ch == '"' {
            in_string = true;
            output.push(ch);
            continue;
        }

        if ch == ',' {
            let mut lookahead = index + 1;
            while lookahead < chars.len() && chars[lookahead].is_whitespace() {
                lookahead += 1;
            }
            if lookahead < chars.len() && matches!(chars[lookahead], ']' | '}') {
                output.push(' ');
                continue;
            }
        }

        output.push(ch);
    }

    output
}

fn walk_value(
    input: &ParseInput<'_>,
    result: &mut ParseResult,
    value: &Value,
    parent: Option<NodeId>,
    prefix: &str,
    crate_name: &str,
) {
    match value {
        Value::Object(map) => {
            for (key, child) in map {
                let path = format!("{prefix}::{key}");
                let id = NodeId::new(crate_name, path.clone(), NodeKind::JsonKey);
                let value_shape = value_shape(child);
                let node = Node {
                    id: id.clone(),
                    name: SmolStr::new(key),
                    kind: NodeKind::JsonKey,
                    file: input.file_id,
                    span: Span::whole_file(input.source.len()),
                    language: Language::Json,
                };
                result.record_fingerprint(
                    &id,
                    fingerprint_from_parts(["json", "key", value_shape.as_str()]),
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
                        span: Span::whole_file(input.source.len()),
                    });
                }
                walk_value(input, result, child, Some(id), &path, crate_name);
            }
        }
        Value::Array(values) => {
            for (index, child) in values.iter().enumerate() {
                let path = format!("{prefix}::[{index}]");
                walk_value(input, result, child, parent.clone(), &path, crate_name);
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
        Value::Array(values) => format!("array:{}", values.len()),
        Value::Object(map) => format!("object:{}", map.len()),
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
        Value::Object(map) => {
            for (key, value) in map {
                targets.extend(extract_intent_targets(key));
                collect_value_targets(value, targets);
            }
        }
        _ => {}
    }
}

#[cfg(test)]
mod tests {
    use std::path::Path;

    use prism_ir::{FileId, NodeKind};
    use prism_parser::{LanguageAdapter, ParseInput};

    use super::JsonAdapter;

    #[test]
    fn parses_document_anchor_and_keys() {
        let adapter = JsonAdapter;
        let input = ParseInput {
            package_name: "demo",
            crate_name: "demo",
            package_root: Path::new("/workspace"),
            path: Path::new("/workspace/config/app.json"),
            file_id: FileId(1),
            source: "{\"service\":{\"port\":8080}}",
        };

        let result = adapter.parse(&input).unwrap();
        assert!(result
            .nodes
            .iter()
            .any(|node| node.kind == NodeKind::Document));
        assert!(result
            .nodes
            .iter()
            .any(|node| node.kind == NodeKind::JsonKey && node.name == "service"));
        assert_eq!(result.edges.len(), 2);
    }

    #[test]
    fn parses_jsonc_with_comments_and_trailing_commas() {
        let adapter = JsonAdapter;
        let input = ParseInput {
            package_name: "demo",
            crate_name: "demo",
            package_root: Path::new("/workspace"),
            path: Path::new("/workspace/config/tsconfig.json"),
            file_id: FileId(1),
            source: r#"{
  "compilerOptions": {
    /* Bundler mode */
    "moduleResolution": "bundler",
    "types": [
      "vite/client",
    ],
  },
}"#,
        };

        let result = adapter.parse(&input).unwrap();
        assert!(result
            .nodes
            .iter()
            .any(|node| node.kind == NodeKind::JsonKey && node.name == "compilerOptions"));
        assert!(result
            .nodes
            .iter()
            .any(|node| node.kind == NodeKind::JsonKey && node.name == "moduleResolution"));
    }
}
