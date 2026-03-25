use anyhow::Result;
use prism_ir::{Edge, EdgeKind, EdgeOrigin, Language, Node, NodeId, NodeKind, Span};
use prism_parser::{relative_file, LanguageAdapter, ParseInput, ParseResult};
use serde_yaml::Value;
use smol_str::SmolStr;

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
        let value: Value = serde_yaml::from_str(input.source)?;
        let mut result = ParseResult::default();
        walk_value(
            input,
            &mut result,
            &value,
            None,
            &file_prefix(input),
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
        Value::Mapping(map) => {
            for (key, child) in map {
                let Value::String(key) = key else {
                    continue;
                };
                let path = format!("{prefix}::{key}");
                let id = NodeId::new(crate_name, path.clone(), NodeKind::YamlKey);
                result.nodes.push(Node {
                    id: id.clone(),
                    name: SmolStr::new(key),
                    kind: NodeKind::YamlKey,
                    file: input.file_id,
                    span: Span::line(1),
                    language: Language::Yaml,
                });
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
        Value::Sequence(values) => {
            for (index, child) in values.iter().enumerate() {
                let path = format!("{prefix}::[{index}]");
                walk_value(input, result, child, parent.clone(), &path, crate_name);
            }
        }
        _ => {}
    }
}

fn file_prefix(input: &ParseInput<'_>) -> String {
    let relative = relative_file(input);
    let mut parts = vec![input.crate_name.to_owned()];
    for component in relative.components() {
        let value = component.as_os_str().to_string_lossy();
        parts.push(value.replace(['.', '-', ' '], "_"));
    }
    parts.join("::")
}
