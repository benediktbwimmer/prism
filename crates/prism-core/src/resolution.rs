use prism_ir::{Edge, EdgeKind, EdgeOrigin, NodeId, NodeKind};
use prism_parser::{
    SymbolTarget, UnresolvedCall, UnresolvedImpl, UnresolvedImport, UnresolvedIntent,
};
use prism_store::Graph;
use std::collections::HashSet;

pub(crate) fn resolve_calls(graph: &mut Graph, unresolved: Vec<UnresolvedCall>) {
    for call in unresolved {
        let Some(target) = resolve_target(
            graph,
            SymbolTarget {
                kind: EdgeKind::Calls,
                source: &call.caller,
                module_path: &call.module_path,
                name: &call.name,
                target_path: "",
            },
        ) else {
            continue;
        };
        graph.add_edge(Edge {
            kind: EdgeKind::Calls,
            source: call.caller.clone(),
            target,
            origin: EdgeOrigin::Static,
            confidence: 0.6,
        });
    }
}

pub(crate) fn resolve_imports(graph: &mut Graph, unresolved: Vec<UnresolvedImport>) {
    for import in unresolved {
        let name = import
            .path
            .rsplit("::")
            .next()
            .unwrap_or(import.path.as_str())
            .to_owned();
        let Some(target) = resolve_target(
            graph,
            SymbolTarget {
                kind: EdgeKind::Imports,
                source: &import.importer,
                module_path: &import.module_path,
                name: &name,
                target_path: &import.path,
            },
        ) else {
            continue;
        };
        graph.add_edge(Edge {
            kind: EdgeKind::Imports,
            source: import.importer.clone(),
            target,
            origin: EdgeOrigin::Static,
            confidence: 0.8,
        });
    }
}

pub(crate) fn resolve_impls(graph: &mut Graph, unresolved: Vec<UnresolvedImpl>) {
    for implementation in unresolved {
        let name = implementation
            .target
            .rsplit("::")
            .next()
            .unwrap_or(implementation.target.as_str())
            .to_owned();
        let Some(target) = resolve_target(
            graph,
            SymbolTarget {
                kind: EdgeKind::Implements,
                source: &implementation.impl_node,
                module_path: &implementation.module_path,
                name: &name,
                target_path: &implementation.target,
            },
        ) else {
            continue;
        };
        graph.add_edge(Edge {
            kind: EdgeKind::Implements,
            source: implementation.impl_node.clone(),
            target,
            origin: EdgeOrigin::Static,
            confidence: 0.8,
        });
    }
}

pub(crate) fn resolve_intents(graph: &mut Graph, unresolved: Vec<UnresolvedIntent>) {
    let mut seen = HashSet::<(EdgeKind, NodeId, NodeId)>::new();
    for intent in unresolved {
        let Some(target) = resolve_intent_target(graph, &intent) else {
            continue;
        };
        if intent.source == target
            || !seen.insert((intent.kind, intent.source.clone(), target.clone()))
        {
            continue;
        }
        graph.add_edge(Edge {
            kind: intent.kind,
            source: intent.source.clone(),
            target,
            origin: EdgeOrigin::Static,
            confidence: 0.7,
        });
    }
}

fn resolve_target(graph: &Graph, target: SymbolTarget<'_>) -> Option<NodeId> {
    let allowed = |kind: NodeKind| match target.kind {
        EdgeKind::Calls => matches!(kind, NodeKind::Function | NodeKind::Method),
        EdgeKind::Implements => kind == NodeKind::Trait,
        EdgeKind::Imports => !matches!(kind, NodeKind::Workspace | NodeKind::Package),
        _ => false,
    };

    if !target.target_path.is_empty() {
        if let Some(node) = graph
            .all_nodes()
            .find(|node| allowed(node.kind) && node.id.path == target.target_path)
        {
            return Some(node.id.clone());
        }
    }

    let exact_path = format!("{}::{}", target.module_path, target.name);
    if let Some(node) = graph
        .all_nodes()
        .find(|node| allowed(node.kind) && node.id.path == exact_path)
    {
        return Some(node.id.clone());
    }

    let mut matches = graph
        .all_nodes()
        .filter(|node| allowed(node.kind))
        .filter(|node| node.name == target.name)
        .map(|node| node.id.clone())
        .collect::<Vec<_>>();

    if matches.len() == 1 {
        return matches.pop();
    }

    None
}

fn resolve_intent_target(graph: &Graph, intent: &UnresolvedIntent) -> Option<NodeId> {
    let exact = intent.target.as_str();
    let mut matches = graph
        .all_nodes()
        .filter(|node| is_intent_target_kind(node.kind))
        .filter(|node| node.id.path == exact || node.name == exact)
        .map(|node| node.id.clone())
        .collect::<Vec<_>>();

    if matches.is_empty() && exact.contains("::") {
        matches = graph
            .all_nodes()
            .filter(|node| is_intent_target_kind(node.kind))
            .filter(|node| node.id.path.ends_with(exact))
            .map(|node| node.id.clone())
            .collect::<Vec<_>>();
    }

    if matches.len() == 1 {
        return matches.pop();
    }

    matches.sort_by(|left, right| {
        score_intent_target(left, intent)
            .cmp(&score_intent_target(right, intent))
            .reverse()
            .then_with(|| left.path.cmp(&right.path))
    });
    let best = matches.first()?.clone();
    let best_score = score_intent_target(&best, intent);
    let next_score = matches
        .get(1)
        .map(|candidate| score_intent_target(candidate, intent))
        .unwrap_or(i32::MIN);
    (best_score > next_score).then_some(best)
}

fn is_intent_target_kind(kind: NodeKind) -> bool {
    !matches!(
        kind,
        NodeKind::Workspace | NodeKind::Package | NodeKind::Document | NodeKind::MarkdownHeading
    )
}

fn score_intent_target(candidate: &NodeId, intent: &UnresolvedIntent) -> i32 {
    let mut score = 0;
    if candidate.path == intent.target {
        score += 4;
    }
    if candidate.path.ends_with(intent.target.as_str()) {
        score += 2;
    }
    if candidate.path.rsplit("::").next() == Some(intent.target.as_str()) {
        score += 3;
    }
    if matches!(candidate.kind, NodeKind::Function | NodeKind::Method) {
        score += 1;
    }
    score
}
