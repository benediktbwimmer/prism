use std::collections::HashMap;

use anyhow::Result;
use prism_ir::{
    Edge, EdgeKind, EdgeOrigin, Language, Node, NodeId, NodeKind, Span, UnresolvedIntent,
};
use prism_parser::{
    document_name, document_path, extract_intent_targets, fingerprint_from_parts,
    intent_kind_for_context, normalized_shape_hash, LanguageAdapter, NodeFingerprint, ParseInput,
    ParseResult,
};
use smol_str::SmolStr;

pub struct MarkdownAdapter;

#[derive(Debug, Clone, Copy)]
struct HeadingSection {
    node_index: usize,
    level: usize,
    line_index: usize,
}

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
        let mut headings = Vec::<HeadingSection>::new();
        let line_ranges = source_line_ranges(input.source);
        let prefix = document_path(input);
        let document_id = NodeId::new(input.crate_name, prefix.clone(), NodeKind::Document);
        let document_shape = normalized_shape_hash(input.source);
        let default_intent_kind = intent_kind_for_context(&prefix, EdgeKind::RelatedTo);
        push_fingerprinted_node(
            &mut result,
            Node {
                id: document_id.clone(),
                name: SmolStr::new(document_name(input)),
                kind: NodeKind::Document,
                file: input.file_id,
                span: Span::whole_file(input.source.len()),
                language: Language::Markdown,
            },
            fingerprint_from_parts(["markdown", "document", document_shape.as_str()]),
        );

        for (index, (line_start, line_end)) in line_ranges.iter().copied().enumerate() {
            let line = input.source.get(line_start..line_end).unwrap_or_default();
            let trimmed = line.trim();
            let mut anchor = stack
                .last()
                .map(|(_, id)| id.clone())
                .unwrap_or_else(|| document_id.clone());
            let mut context = prefix.clone();

            if trimmed.starts_with('#') {
                let level = trimmed.chars().take_while(|ch| *ch == '#').count();
                let title = trimmed[level..].trim();
                if !title.is_empty() {
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
                        span: Span::new(line_start, line_end),
                        language: Language::Markdown,
                    };
                    let title_shape = normalized_shape_hash(title);
                    result.record_fingerprint(
                        &id,
                        fingerprint_from_parts([
                            "markdown",
                            "heading",
                            &level.to_string(),
                            title_shape.as_str(),
                        ]),
                    );
                    let node_index = result.nodes.len();
                    result.nodes.push(node);
                    headings.push(HeadingSection {
                        node_index,
                        level,
                        line_index: index,
                    });

                    while stack
                        .last()
                        .is_some_and(|(stack_level, _)| *stack_level >= level)
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

                    stack.push((level, id.clone()));
                    anchor = id;
                    context = title.to_owned();
                }
            } else if let Some((_, heading)) = stack.last() {
                anchor = heading.clone();
                if let Some(node) = result.nodes.iter().find(|node| node.id == anchor) {
                    context = node.name.to_string();
                }
            }

            let intent_kind =
                intent_kind_for_context(&format!("{context} {trimmed}"), default_intent_kind);
            for target in extract_intent_targets(trimmed) {
                result.unresolved_intents.push(UnresolvedIntent {
                    source: anchor.clone(),
                    kind: intent_kind,
                    target: target.into(),
                    span: Span::new(line_start, line_end),
                });
            }
        }

        apply_heading_section_spans(
            &mut result.nodes,
            &headings,
            &line_ranges,
            input.source.len(),
        );

        Ok(result)
    }
}

fn source_line_ranges(source: &str) -> Vec<(usize, usize)> {
    let mut ranges = Vec::new();
    let mut start = 0usize;
    for line in source.split_inclusive('\n') {
        let end = start + line.len();
        ranges.push((start, end));
        start = end;
    }
    if ranges.is_empty() {
        ranges.push((0, 0));
    } else if start < source.len() {
        ranges.push((start, source.len()));
    }
    ranges
}

fn apply_heading_section_spans(
    nodes: &mut [Node],
    headings: &[HeadingSection],
    line_ranges: &[(usize, usize)],
    source_len: usize,
) {
    for (index, heading) in headings.iter().enumerate() {
        let start = line_ranges
            .get(heading.line_index)
            .map(|(start, _)| *start)
            .unwrap_or(source_len);
        let end = headings
            .iter()
            .skip(index + 1)
            .find(|next| next.level <= heading.level)
            .and_then(|next| line_ranges.get(next.line_index).map(|(start, _)| *start))
            .unwrap_or(source_len);
        if let Some(node) = nodes.get_mut(heading.node_index) {
            node.span = Span::new(start, end);
        }
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

    use prism_ir::{FileId, NodeKind, Span};
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
        assert_eq!(result.nodes[1].span, Span::new(0, input.source.len()));
        assert_eq!(result.nodes[2].span, Span::new(6, input.source.len()));
    }

    #[test]
    fn heading_span_stops_at_next_peer_heading() {
        let adapter = MarkdownAdapter;
        let input = ParseInput {
            package_name: "prism",
            crate_name: "prism",
            package_root: Path::new("/workspace"),
            path: Path::new("/workspace/docs/spec.md"),
            file_id: FileId(1),
            source: "# One\nalpha\n## Child\nbeta\n# Two\ngamma\n",
        };

        let result = adapter.parse(&input).unwrap();
        assert_eq!(result.nodes[1].span, Span::new(0, 26));
        assert_eq!(result.nodes[2].span, Span::new(12, 26));
        assert_eq!(result.nodes[3].span, Span::new(26, input.source.len()));
    }
}
