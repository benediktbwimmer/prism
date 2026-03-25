use std::collections::hash_map::DefaultHasher;
use std::fs;
use std::hash::{Hash, Hasher};
use std::path::Path;

use anyhow::Result;
use prism_ir::{Edge, EdgeKind, EdgeOrigin, Language, Node, NodeId, NodeKind, Span};
use prism_lang_json::JsonAdapter;
use prism_lang_markdown::MarkdownAdapter;
use prism_lang_rust::RustAdapter;
use prism_lang_yaml::YamlAdapter;
use prism_parser::{LanguageAdapter, ParseInput, UnresolvedCall};
use prism_query::Prism;
use prism_store::Graph;
use smol_str::SmolStr;
use walkdir::WalkDir;

pub fn index_workspace(root: impl AsRef<Path>) -> Result<Prism> {
    let root = root.as_ref().canonicalize()?;
    let crate_name = workspace_name(&root);
    let mut graph = Graph::new();
    let adapters: Vec<Box<dyn LanguageAdapter>> = vec![
        Box::new(RustAdapter),
        Box::new(MarkdownAdapter),
        Box::new(JsonAdapter),
        Box::new(YamlAdapter),
    ];
    let mut unresolved = Vec::<UnresolvedCall>::new();
    let manifest_file = graph.ensure_file(&root.join("Cargo.toml"));

    let workspace_id = NodeId::new(
        crate_name.clone(),
        format!("{crate_name}::workspace"),
        NodeKind::Workspace,
    );
    graph.add_node(Node {
        id: workspace_id.clone(),
        name: SmolStr::new(crate_name.clone()),
        kind: NodeKind::Workspace,
        file: manifest_file,
        span: Span::line(1),
        language: Language::Unknown,
    });

    let package_id = NodeId::new(crate_name.clone(), crate_name.clone(), NodeKind::Package);
    graph.add_node(Node {
        id: package_id.clone(),
        name: SmolStr::new(crate_name.clone()),
        kind: NodeKind::Package,
        file: manifest_file,
        span: Span::line(1),
        language: Language::Unknown,
    });
    graph.add_edge(Edge {
        kind: EdgeKind::Contains,
        source: workspace_id.clone(),
        target: package_id.clone(),
        origin: EdgeOrigin::Static,
        confidence: 1.0,
    });

    for entry in WalkDir::new(&root).into_iter().filter_map(Result::ok) {
        if !entry.file_type().is_file() {
            continue;
        }

        let path = entry.path();
        let Some(adapter) = adapters.iter().find(|adapter| adapter.supports_path(path)) else {
            continue;
        };
        let source = fs::read_to_string(path)?;
        let file_id = graph.ensure_file(path);
        let input = ParseInput {
            crate_name: &crate_name,
            workspace_root: &root,
            path,
            file_id,
            source: &source,
        };
        let parsed = adapter.parse(&input)?;
        unresolved.extend(parsed.unresolved_calls.clone());
        graph.upsert_file(
            path,
            stable_hash(&source),
            parsed.nodes.clone(),
            parsed.edges.clone(),
        );

        for node in parsed
            .nodes
            .iter()
            .filter(|node| node.kind == NodeKind::Module)
        {
            graph.add_edge(Edge {
                kind: EdgeKind::Contains,
                source: package_id.clone(),
                target: node.id.clone(),
                origin: EdgeOrigin::Static,
                confidence: 1.0,
            });
        }
    }

    resolve_calls(&mut graph, unresolved);

    Ok(Prism::new(graph))
}

fn resolve_calls(graph: &mut Graph, unresolved: Vec<UnresolvedCall>) {
    for call in unresolved {
        let Some(target) = resolve_call_target(graph, &call) else {
            continue;
        };
        graph.add_edge(Edge {
            kind: EdgeKind::Calls,
            source: call.source.clone(),
            target,
            origin: EdgeOrigin::Static,
            confidence: 0.6,
        });
    }
}

fn resolve_call_target(graph: &Graph, call: &UnresolvedCall) -> Option<NodeId> {
    let exact_path = format!("{}::{}", call.module_path, call.name);
    if let Some(node) = graph.all_nodes().find(|node| {
        matches!(node.kind, NodeKind::Function | NodeKind::Method) && node.id.path == exact_path
    }) {
        return Some(node.id.clone());
    }

    let mut matches = graph
        .all_nodes()
        .filter(|node| matches!(node.kind, NodeKind::Function | NodeKind::Method))
        .filter(|node| node.name == call.name)
        .map(|node| node.id.clone())
        .collect::<Vec<_>>();

    if matches.len() == 1 {
        return matches.pop();
    }

    None
}

fn stable_hash(source: &str) -> u64 {
    let mut hasher = DefaultHasher::new();
    source.hash(&mut hasher);
    hasher.finish()
}

fn workspace_name(root: &Path) -> String {
    root.file_name()
        .and_then(|value| value.to_str())
        .unwrap_or("workspace")
        .to_owned()
}
