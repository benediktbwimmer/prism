use std::collections::BTreeMap;

use prism_ir::NodeRefKind;
use prism_js::{PlanGraphView, PlanNodeView};

use super::html::{escape_html, status_slug};
use crate::ui_types::PrismSsrPlanDetailView;

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

pub(crate) fn plan_detail_mermaid(view: &PrismSsrPlanDetailView) -> String {
    let mut lines = vec![
        "flowchart TD".to_string(),
        "classDef status-ready fill:#dfe4e2,stroke:#5f6d64,color:#10211a;".to_string(),
        "classDef status-in-progress fill:#d7edf7,stroke:#0b5a78,color:#10211a;".to_string(),
        "classDef status-completed fill:#d8efdf,stroke:#25684d,color:#10211a;".to_string(),
        "classDef status-blocked fill:#f3ddd4,stroke:#b4542c,color:#10211a;".to_string(),
        "classDef status-other fill:#efe6d2,stroke:#8b7d5b,color:#10211a;".to_string(),
        "classDef external fill:#fbf5e8,stroke:#8b7d5b,stroke-dasharray: 5 3,color:#10211a;"
            .to_string(),
    ];
    let mut ids = BTreeMap::new();
    let mut next_index = 0usize;

    for plan in &view.child_plans {
        let key = format!("plan:{}", plan.plan.id);
        ids.insert(key, format!("n{next_index}"));
        next_index += 1;
    }
    for task in &view.child_tasks {
        let key = format!("task:{}", task.task.id);
        ids.insert(key, format!("n{next_index}"));
        next_index += 1;
    }
    for stub in &view.external_stubs {
        let key = format!("{:?}:{}", stub.node.kind, stub.node.id);
        ids.insert(key, format!("n{next_index}"));
        next_index += 1;
    }

    for plan in &view.child_plans {
        let key = format!("plan:{}", plan.plan.id);
        let node_ref = ids
            .get(&key)
            .expect("child plan node should have a generated ref");
        lines.push(format!(
            "{}([\"{}\"])",
            node_ref,
            escape_html(&format!(
                "{}<br/><small>{:?} · {} direct children</small>",
                plan.plan.title,
                plan.plan.status,
                plan.direct_child_plan_count + plan.direct_child_task_count
            ))
        ));
        lines.push(format!(
            "class {} {};",
            node_ref,
            status_class(&format!("{:?}", plan.plan.status))
        ));
        lines.push(format!(
            "click {} href \"/console/plans/{}\" \"Open child plan\";",
            node_ref,
            escape_html(&plan.plan.id)
        ));
    }

    for task in &view.child_tasks {
        let key = format!("task:{}", task.task.id);
        let node_ref = ids
            .get(&key)
            .expect("child task node should have a generated ref");
        let target = task
            .task
            .executor
            .target_label
            .as_deref()
            .map(|label| format!(" · {label}"))
            .unwrap_or_default();
        lines.push(format!(
            "{}[\"{}\"]",
            node_ref,
            escape_html(&format!(
                "{}<br/><small>{:?} · {:?}{} · {} deps</small>",
                task.task.title,
                task.task.status,
                task.task.executor.executor_class,
                target,
                task.dependency_count
            ))
        ));
        lines.push(format!(
            "class {} {};",
            node_ref,
            status_class(&format!("{:?}", task.task.status))
        ));
        lines.push(format!(
            "click {} href \"/console/tasks/{}\" \"Open task detail\";",
            node_ref,
            escape_html(&task.task.id)
        ));
    }

    for stub in &view.external_stubs {
        let key = format!("{:?}:{}", stub.node.kind, stub.node.id);
        let node_ref = ids
            .get(&key)
            .expect("external stub node should have a generated ref");
        let kind_label = match stub.node.kind {
            NodeRefKind::Plan => "external plan",
            NodeRefKind::Task => "external task",
        };
        lines.push(format!(
            "{}[[\"{}\"]]",
            node_ref,
            escape_html(&format!(
                "{}<br/><small>{} · {} · {}</small>",
                stub.title,
                kind_label,
                stub.status,
                stub.relation_labels.join(" / ")
            ))
        ));
        lines.push(format!("class {} external;", node_ref));
        lines.push(format!(
            "click {} href \"{}\" \"Open external dependency\";",
            node_ref,
            escape_html(&stub.href)
        ));
    }

    for plan in &view.child_plans {
        let from_key = format!("plan:{}", plan.plan.id);
        let Some(from) = ids.get(&from_key) else {
            continue;
        };
        for dependency in &plan.plan.dependencies {
            let target_key = format!("{:?}:{}", dependency.kind, dependency.id);
            let Some(to) = ids.get(&target_key) else {
                continue;
            };
            lines.push(format!("{from} -->|depends_on| {to}"));
        }
    }

    for task in &view.child_tasks {
        let from_key = format!("task:{}", task.task.id);
        let Some(from) = ids.get(&from_key) else {
            continue;
        };
        for dependency in &task.task.dependencies {
            let target_key = format!("{:?}:{}", dependency.kind, dependency.id);
            let Some(to) = ids.get(&target_key) else {
                continue;
            };
            lines.push(format!("{from} -->|depends_on| {to}"));
        }
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

fn status_class(status: &str) -> &'static str {
    match status_slug(status).as_str() {
        "ready" | "proposed" => "status-ready",
        "in-progress" | "in_progress" | "validating" | "in-review" | "in_review" => {
            "status-in-progress"
        }
        "completed" => "status-completed",
        "blocked" | "abandoned" | "broken-dependency" | "broken_dependency" => "status-blocked",
        _ => "status-other",
    }
}
