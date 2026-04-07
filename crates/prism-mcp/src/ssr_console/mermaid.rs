use std::collections::BTreeMap;

use super::html::escape_html;

pub(crate) fn concept_graph_mermaid(
    focus_handle: &str,
    nodes: &[(String, String)],
    edges: &[(String, String, String)],
) -> String {
    let mut lines = vec![
        "flowchart LR".to_string(),
        "classDef focus fill:#d7efe5,stroke:#1f5f4a,color:#10211a;".to_string(),
        "classDef concept fill:#fff7eb,stroke:#8b7d5b,color:#10211a;".to_string(),
    ];

    let ids = nodes
        .iter()
        .enumerate()
        .map(|(index, (handle, _))| (handle.clone(), format!("c{index}")))
        .collect::<BTreeMap<_, _>>();

    for (handle, label) in nodes {
        let node_ref = ids.get(handle).expect("concept node id should exist");
        lines.push(format!("{}[\"{}\"]", node_ref, escape_html(label)));
        let class_name = if handle == focus_handle {
            "focus"
        } else {
            "concept"
        };
        lines.push(format!("class {} {};", node_ref, class_name));
    }

    for (from, to, label) in edges {
        let Some(from_ref) = ids.get(from) else {
            continue;
        };
        let Some(to_ref) = ids.get(to) else {
            continue;
        };
        lines.push(format!(
            "{} -->|{}| {}",
            from_ref,
            escape_html(label),
            to_ref
        ));
    }

    lines.join("\n")
}
