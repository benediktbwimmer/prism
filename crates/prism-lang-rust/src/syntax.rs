use std::collections::BTreeMap;

use prism_ir::{Edge, EdgeKind, EdgeOrigin, Node, NodeId, NodeKind, Span};
use prism_parser::{NodeFingerprint, ParseResult};
use tree_sitter::Node as TsNode;

use crate::paths::simplify_symbol;

pub(crate) fn extract_calls(node: TsNode<'_>, source: &[u8]) -> Vec<(String, Span)> {
    let mut calls = BTreeMap::new();
    collect_calls(node, source, &mut calls);
    calls.into_iter().collect()
}

fn collect_calls(node: TsNode<'_>, source: &[u8], calls: &mut BTreeMap<String, Span>) {
    if node.kind() == "call_expression" {
        if let Some(name) = extract_call_name(node, source) {
            calls.entry(name).or_insert_with(|| node_span(node));
        }
    }

    let mut cursor = node.walk();
    for child in node.named_children(&mut cursor) {
        collect_calls(child, source, calls);
    }
}

fn extract_call_name(node: TsNode<'_>, source: &[u8]) -> Option<String> {
    let function = node.child_by_field_name("function")?;
    extract_called_symbol(function, source)
}

fn extract_called_symbol(node: TsNode<'_>, source: &[u8]) -> Option<String> {
    match node.kind() {
        "identifier" | "field_identifier" | "self" => {
            Some(simplify_symbol(&node_text(node, source)))
        }
        "scoped_identifier" => Some(simplify_symbol(&node_text(node, source))),
        "field_expression" => node
            .child_by_field_name("field")
            .map(|field| simplify_symbol(&node_text(field, source))),
        "generic_function" => node
            .child_by_field_name("function")
            .and_then(|function| extract_called_symbol(function, source)),
        _ => None,
    }
}

pub(crate) fn node_name(node: TsNode<'_>, source: &[u8]) -> Option<String> {
    Some(node_text(node.child_by_field_name("name")?, source))
}

pub(crate) fn node_text(node: TsNode<'_>, source: &[u8]) -> String {
    node.utf8_text(source).unwrap_or_default().to_owned()
}

pub(crate) fn node_span(node: TsNode<'_>) -> Span {
    Span::new(node.start_byte(), node.end_byte())
}

pub(crate) fn push_contains_edge(result: &mut ParseResult, source: NodeId, target: NodeId) {
    result.edges.push(Edge {
        kind: EdgeKind::Contains,
        source,
        target,
        origin: EdgeOrigin::Static,
        confidence: 1.0,
    });
}

pub(crate) fn push_fingerprinted_node(
    result: &mut ParseResult,
    node: Node,
    fingerprint: NodeFingerprint,
) {
    result.record_fingerprint(&node.id, fingerprint);
    result.nodes.push(node);
}

pub(crate) fn kind_label(kind: NodeKind) -> &'static str {
    match kind {
        NodeKind::Module => "module",
        NodeKind::Function => "function",
        NodeKind::Struct => "struct",
        NodeKind::Enum => "enum",
        NodeKind::Trait => "trait",
        NodeKind::Impl => "impl",
        NodeKind::Method => "method",
        NodeKind::Field => "field",
        NodeKind::TypeAlias => "type_alias",
        _ => "node",
    }
}
