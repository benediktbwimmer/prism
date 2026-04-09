use prism_ir::{AnchorRef, NodeId};
use std::time::{SystemTime, UNIX_EPOCH};

pub(crate) fn dedupe_strings(mut values: Vec<String>) -> Vec<String> {
    values.sort();
    values.dedup();
    values
}

pub(crate) fn dedupe_node_ids(mut nodes: Vec<NodeId>) -> Vec<NodeId> {
    sort_node_ids(&mut nodes);
    nodes.dedup();
    nodes
}

pub(crate) fn sort_node_ids(nodes: &mut Vec<NodeId>) {
    nodes.sort_by(|left, right| {
        left.crate_name
            .cmp(&right.crate_name)
            .then_with(|| left.path.cmp(&right.path))
            .then_with(|| left.kind.to_string().cmp(&right.kind.to_string()))
    });
    nodes.dedup();
}

pub(crate) fn anchor_sort_key(left: &AnchorRef, right: &AnchorRef) -> std::cmp::Ordering {
    anchor_label(left).cmp(&anchor_label(right))
}

pub(crate) fn current_timestamp() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|duration| duration.as_secs())
        .unwrap_or(0)
}

fn anchor_label(anchor: &AnchorRef) -> String {
    match anchor {
        AnchorRef::Node(node) => format!("node:{}:{}:{}", node.crate_name, node.path, node.kind),
        AnchorRef::Lineage(lineage) => format!("lineage:{}", lineage.0),
        AnchorRef::File(file) => format!("file:{}", file.0),
        AnchorRef::WorkspacePath(path) => format!("file_path:{path}"),
        AnchorRef::Kind(kind) => format!("kind:{kind}"),
    }
}
