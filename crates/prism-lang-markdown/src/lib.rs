use std::collections::HashMap;

use anyhow::Result;
use prism_ir::{Edge, EdgeKind, EdgeOrigin, Language, Node, NodeId, NodeKind, Span};
use prism_parser::{relative_file, LanguageAdapter, ParseInput, ParseResult};
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
        let mut nodes = Vec::new();
        let mut edges = Vec::new();
        let mut slug_counts = HashMap::<String, usize>::new();
        let mut stack: Vec<(usize, NodeId)> = Vec::new();
        let prefix = file_prefix(input);

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
            nodes.push(node);

            while stack
                .last()
                .map_or(false, |(stack_level, _)| *stack_level >= level)
            {
                stack.pop();
            }

            if let Some((_, parent)) = stack.last() {
                edges.push(Edge {
                    kind: EdgeKind::Contains,
                    source: parent.clone(),
                    target: id.clone(),
                    origin: EdgeOrigin::Static,
                    confidence: 1.0,
                });
            }

            stack.push((level, id));
        }

        Ok(ParseResult {
            nodes,
            edges,
            unresolved_calls: Vec::new(),
            unresolved_imports: Vec::new(),
            unresolved_impls: Vec::new(),
        })
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
            crate_name: "prism",
            workspace_root: Path::new("/workspace"),
            path: Path::new("/workspace/docs/spec.md"),
            file_id: FileId(1),
            source: "# Top\n## Child\n",
        };

        let result = adapter.parse(&input).unwrap();
        assert_eq!(result.nodes.len(), 2);
        assert_eq!(result.nodes[0].kind, NodeKind::MarkdownHeading);
        assert_eq!(result.edges.len(), 1);
    }
}
