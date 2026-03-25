use anyhow::Result;
use prism_ir::{Edge, EdgeKind, EdgeOrigin, Language, Node, NodeId, NodeKind, Span};
use prism_parser::{
    document_name, document_path, fingerprint_from_parts, normalized_shape_hash, LanguageAdapter,
    ParseInput, ParseResult,
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
        let value: Value = serde_json::from_str(input.source)?;
        let mut result = ParseResult::default();
        let document_path = document_path(input);
        let document_id = NodeId::new(input.crate_name, document_path.clone(), NodeKind::Document);
        let document_shape = normalized_shape_hash(input.source);
        let document_node = Node {
            id: document_id.clone(),
            name: SmolStr::new(document_name(input)),
            kind: NodeKind::Document,
            file: input.file_id,
            span: Span::whole_file(input.source.lines().count()),
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
                    span: Span::line(1),
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
}
