use std::collections::BTreeMap;

use prism_js::{PlanGraphView, PlanNodeView};

use super::html::{escape_html, status_slug};

pub(crate) fn plan_graph_mermaid(graph: &PlanGraphView) -> String {
    let mut lines = vec![
        "flowchart TD".to_string(),
        "classDef status-ready fill:#dfe4e2,stroke:#5f6d64,color:#10211a;".to_string(),
        "classDef status-in-progress fill:#d7edf7,stroke:#0b5a78,color:#10211a;".to_string(),
        "classDef status-completed fill:#d8efdf,stroke:#25684d,color:#10211a;".to_string(),
        "classDef status-blocked fill:#f3ddd4,stroke:#b4542c,color:#10211a;".to_string(),
        "classDef status-other fill:#efe6d2,stroke:#8b7d5b,color:#10211a;".to_string(),
    ];

    let ids = graph
        .nodes
        .iter()
        .enumerate()
        .map(|(index, node)| (node.id.clone(), format!("n{index}")))
        .collect::<BTreeMap<_, _>>();

    for node in &graph.nodes {
        let node_ref = ids
            .get(&node.id)
            .expect("node id should have a generated ref");
        lines.push(format!("{}[\"{}\"]", node_ref, plan_node_label(node)));
        let class_name = match status_slug(&format!("{:?}", node.status)).as_str() {
            "ready" | "proposed" => "status-ready",
            "in-progress" | "in_progress" | "validating" | "in-review" | "in_review" => {
                "status-in-progress"
            }
            "completed" => "status-completed",
            "blocked" | "abandoned" => "status-blocked",
            _ => "status-other",
        };
        lines.push(format!("class {} {};", node_ref, class_name));
        if node.id.starts_with("coord-task:") {
            lines.push(format!(
                "click {} href \"/console/tasks/{}\" \"Open task detail\";",
                node_ref,
                escape_html(&node.id)
            ));
        }
    }

    for edge in &graph.edges {
        let Some(from) = ids.get(&edge.from) else {
            continue;
        };
        let Some(to) = ids.get(&edge.to) else {
            continue;
        };
        lines.push(format!(
            "{} -->|{}| {}",
            from,
            format!("{:?}", edge.kind),
            to
        ));
    }

    lines.join("\n")
}

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

fn plan_node_label(node: &PlanNodeView) -> String {
    let status = format!("{:?}", node.status);
    let mut label = escape_html(&node.title);
    label.push_str("<br/><small>");
    label.push_str(&escape_html(&status));
    if let Some(assignee) = &node.assignee {
        label.push_str(" · ");
        label.push_str(&escape_html(assignee));
    }
    label.push_str("</small>");
    label
}
