use std::collections::HashMap;

use anyhow::Result;
use prism_ir::{Edge, EdgeKind, EdgeOrigin, Language, Node, NodeId, NodeKind, Span};
use prism_parser::{
    document_name, document_path, fingerprint_from_parts, normalized_shape_hash, LanguageAdapter,
    NodeFingerprint, ParseInput, ParseResult,
};
use smol_str::SmolStr;

pub struct MarkdownAdapter;

impl LanguageAdapter for MarkdownAdapter {
    fn language(&self) -> Language {
        Language::Markdown
    }

    fn supports_path(&self, path: &std::path::Path) -> bool {
        matches!(path.extension().and_then(|ext| ext.to_str()), Some("md"))
    }

    fn parse(&self, input: &ParseInput<'_>) -> Result<ParseResult> {
        let mut result = ParseResult::default();
        let mut slug_counts = HashMap::<String, usize>::new();
        let mut stack: Vec<(usize, NodeId)> = Vec::new();
        let prefix = document_path(input);
        let document_id = NodeId::new(input.crate_name, prefix.clone(), NodeKind::Document);
        let document_shape = normalized_shape_hash(input.source);
        push_fingerprinted_node(
            &mut result,
            Node {
                id: document_id.clone(),
                name: SmolStr::new(document_name(input)),
                kind: NodeKind::Document,
                file: input.file_id,
                span: Span::whole_file(input.source.lines().count()),
                language: Language::Markdown,
            },
            fingerprint_from_parts(["markdown", "document", document_shape.as_str()]),
        );

        for (index, line) in input.source.lines().enumerate() {
            let trimmed = line.trim();
            if !trimmed.starts_with('#') {
                continue;
            }

            let level = trimmed.chars().take_while(|ch| *ch == '#').count();
            let title = trimmed[level..].trim();
            if title.is_empty() {
                continue;
            }

            let slug = slugify(title);
            let count = slug_counts.entry(slug.clone()).or_insert(0);
            *count += 1;
            let path = if *count == 1 {
                format!("{prefix}::{slug}")
            } else {
                format!("{prefix}::{slug}:{}", count)
            };
            let id = NodeId::new(input.crate_name, path, NodeKind::MarkdownHeading);

            let node = Node {
                id: id.clone(),
                name: SmolStr::new(title),
                kind: NodeKind::MarkdownHeading,
                file: input.file_id,
                span: Span::line(index + 1),
                language: Language::Markdown,
            };
            let title_shape = normalized_shape_hash(title);
            result.record_fingerprint(
                &id,
                fingerprint_from_parts(["markdown", "heading", &level.to_string(), title_shape.as_str()]),
            );
            result.nodes.push(node);

            while stack
                .last()
                .map_or(false, |(stack_level, _)| *stack_level >= level)
            {
                stack.pop();
            }

            if let Some((_, parent)) = stack.last() {
                result.edges.push(Edge {
                    kind: EdgeKind::Contains,
                    source: parent.clone(),
                    target: id.clone(),
                    origin: EdgeOrigin::Static,
                    confidence: 1.0,
                });
            } else {
                result.edges.push(Edge {
                    kind: EdgeKind::Contains,
                    source: document_id.clone(),
                    target: id.clone(),
                    origin: EdgeOrigin::Static,
                    confidence: 1.0,
                });
            }

            stack.push((level, id));
        }

        Ok(result)
    }
}

fn push_fingerprinted_node(result: &mut ParseResult, node: Node, fingerprint: NodeFingerprint) {
    result.record_fingerprint(&node.id, fingerprint);
    result.nodes.push(node);
}

fn slugify(value: &str) -> String {
    let mut slug = String::new();
    let mut previous_dash = false;
    for ch in value.chars() {
        if ch.is_ascii_alphanumeric() {
            slug.push(ch.to_ascii_lowercase());
            previous_dash = false;
        } else if !previous_dash {
            slug.push('_');
            previous_dash = true;
        }
    }
    slug.trim_matches('_').to_owned()
}

#[cfg(test)]
mod tests {
    use std::path::Path;

    use prism_ir::{FileId, NodeKind};
    use prism_parser::{LanguageAdapter, ParseInput};

    use super::MarkdownAdapter;

    #[test]
    fn parses_heading_hierarchy() {
        let adapter = MarkdownAdapter;
        let input = ParseInput {
            package_name: "prism",
            crate_name: "prism",
            package_root: Path::new("/workspace"),
            path: Path::new("/workspace/docs/spec.md"),
            file_id: FileId(1),
            source: "# Top\n## Child\n",
        };

        let result = adapter.parse(&input).unwrap();
        assert_eq!(result.nodes.len(), 3);
        assert_eq!(result.nodes[0].kind, NodeKind::Document);
        assert_eq!(result.nodes[1].kind, NodeKind::MarkdownHeading);
        assert_eq!(result.edges.len(), 2);
    }
}
