use std::collections::hash_map::DefaultHasher;
use std::collections::HashSet;
use std::fs;
use std::hash::{Hash, Hasher};
use std::path::{Path, PathBuf};

use anyhow::Result;
use prism_ir::{Edge, EdgeKind, EdgeOrigin, Language, Node, NodeId, NodeKind, Span};
use prism_lang_json::JsonAdapter;
use prism_lang_markdown::MarkdownAdapter;
use prism_lang_rust::RustAdapter;
use prism_lang_yaml::YamlAdapter;
use prism_parser::{
    LanguageAdapter, ParseInput, ParseResult, SymbolTarget, UnresolvedCall, UnresolvedImpl,
    UnresolvedImport,
};
use prism_query::Prism;
use prism_store::Graph;
use smol_str::SmolStr;
use walkdir::WalkDir;

pub fn index_workspace(root: impl AsRef<Path>) -> Result<Prism> {
    let mut indexer = WorkspaceIndexer::new(root)?;
    indexer.index()?;
    Ok(indexer.into_prism())
}

pub struct WorkspaceIndexer {
    root: PathBuf,
    crate_name: String,
    graph: Graph,
    adapters: Vec<Box<dyn LanguageAdapter>>,
    package_id: NodeId,
}

impl WorkspaceIndexer {
    pub fn new(root: impl AsRef<Path>) -> Result<Self> {
        let root = root.as_ref().canonicalize()?;
        let crate_name = workspace_name(&root);
        let mut graph = load_cache(&root).unwrap_or_default();
        let package_id = ensure_root_nodes(&mut graph, &root, &crate_name);

        Ok(Self {
            root,
            crate_name,
            graph,
            adapters: default_adapters(),
            package_id,
        })
    }

    pub fn index(&mut self) -> Result<()> {
        let mut seen_files = HashSet::<PathBuf>::new();

        for entry in WalkDir::new(&self.root).into_iter().filter_map(Result::ok) {
            if !entry.file_type().is_file() {
                continue;
            }

            let path = entry.path();
            let Some(adapter) = self
                .adapters
                .iter()
                .find(|adapter| adapter.supports_path(path))
            else {
                continue;
            };

            let canonical_path = path.to_path_buf();
            seen_files.insert(canonical_path.clone());
            let source = fs::read_to_string(path)?;
            let hash = stable_hash(&source);

            if self
                .graph
                .file_record(path)
                .map(|record| record.hash == hash)
                .unwrap_or(false)
            {
                continue;
            }

            let file_id = self.graph.ensure_file(path);
            let input = ParseInput {
                crate_name: &self.crate_name,
                workspace_root: &self.root,
                path,
                file_id,
                source: &source,
            };
            let parsed = adapter.parse(&input)?;
            self.upsert_parsed_file(path, hash, parsed);
        }

        for tracked in self.graph.tracked_files() {
            if !seen_files.contains(&tracked) {
                self.graph.remove_file(&tracked);
            }
        }

        self.resolve_all_edges();
        save_cache(&self.root, &self.graph)?;
        Ok(())
    }

    pub fn graph(&self) -> &Graph {
        &self.graph
    }

    pub fn into_prism(self) -> Prism {
        Prism::new(self.graph)
    }

    fn upsert_parsed_file(&mut self, path: &Path, hash: u64, parsed: ParseResult) {
        let package_id = self.package_id.clone();
        let module_edges = parsed
            .nodes
            .iter()
            .filter(|node| node.kind == NodeKind::Module)
            .map(|node| Edge {
                kind: EdgeKind::Contains,
                source: package_id.clone(),
                target: node.id.clone(),
                origin: EdgeOrigin::Static,
                confidence: 1.0,
            })
            .collect::<Vec<_>>();

        let mut edges = parsed.edges;
        edges.extend(module_edges);
        self.graph.upsert_file(
            path,
            hash,
            parsed.nodes,
            edges,
            parsed.unresolved_calls,
            parsed.unresolved_imports,
            parsed.unresolved_impls,
        );
    }

    fn resolve_all_edges(&mut self) {
        self.graph
            .clear_edges_by_kind(&[EdgeKind::Calls, EdgeKind::Imports, EdgeKind::Implements]);
        let unresolved_calls = self.graph.unresolved_calls();
        let unresolved_imports = self.graph.unresolved_imports();
        let unresolved_impls = self.graph.unresolved_impls();
        resolve_calls(&mut self.graph, unresolved_calls);
        resolve_imports(&mut self.graph, unresolved_imports);
        resolve_impls(&mut self.graph, unresolved_impls);
    }
}

fn default_adapters() -> Vec<Box<dyn LanguageAdapter>> {
    vec![
        Box::new(RustAdapter),
        Box::new(MarkdownAdapter),
        Box::new(JsonAdapter),
        Box::new(YamlAdapter),
    ]
}

fn ensure_root_nodes(graph: &mut Graph, root: &Path, crate_name: &str) -> NodeId {
    let manifest_file = graph.ensure_file(&root.join("Cargo.toml"));
    let workspace_id = NodeId::new(
        crate_name.to_owned(),
        format!("{crate_name}::workspace"),
        NodeKind::Workspace,
    );
    if graph.node(&workspace_id).is_none() {
        graph.add_node(Node {
            id: workspace_id.clone(),
            name: SmolStr::new(crate_name),
            kind: NodeKind::Workspace,
            file: manifest_file,
            span: Span::line(1),
            language: Language::Unknown,
        });
    }

    let package_id = NodeId::new(
        crate_name.to_owned(),
        crate_name.to_owned(),
        NodeKind::Package,
    );
    if graph.node(&package_id).is_none() {
        graph.add_node(Node {
            id: package_id.clone(),
            name: SmolStr::new(crate_name),
            kind: NodeKind::Package,
            file: manifest_file,
            span: Span::line(1),
            language: Language::Unknown,
        });
    }

    if !graph.edges.iter().any(|edge| {
        edge.kind == EdgeKind::Contains
            && edge.source == workspace_id
            && edge.target == package_id
            && edge.origin == EdgeOrigin::Static
    }) {
        graph.add_edge(Edge {
            kind: EdgeKind::Contains,
            source: workspace_id,
            target: package_id.clone(),
            origin: EdgeOrigin::Static,
            confidence: 1.0,
        });
    }

    package_id
}

fn resolve_calls(graph: &mut Graph, unresolved: Vec<UnresolvedCall>) {
    for call in unresolved {
        let Some(target) = resolve_target(
            graph,
            SymbolTarget {
                kind: EdgeKind::Calls,
                source: &call.source,
                module_path: &call.module_path,
                name: &call.name,
                target_path: "",
            },
        ) else {
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

fn resolve_imports(graph: &mut Graph, unresolved: Vec<UnresolvedImport>) {
    for import in unresolved {
        let Some(target) = resolve_target(
            graph,
            SymbolTarget {
                kind: EdgeKind::Imports,
                source: &import.source,
                module_path: &import.module_path,
                name: &import.name,
                target_path: &import.target_path,
            },
        ) else {
            continue;
        };
        graph.add_edge(Edge {
            kind: EdgeKind::Imports,
            source: import.source.clone(),
            target,
            origin: EdgeOrigin::Static,
            confidence: 0.8,
        });
    }
}

fn resolve_impls(graph: &mut Graph, unresolved: Vec<UnresolvedImpl>) {
    for implementation in unresolved {
        let Some(target) = resolve_target(
            graph,
            SymbolTarget {
                kind: EdgeKind::Implements,
                source: &implementation.source,
                module_path: &implementation.module_path,
                name: &implementation.name,
                target_path: &implementation.trait_path,
            },
        ) else {
            continue;
        };
        graph.add_edge(Edge {
            kind: EdgeKind::Implements,
            source: implementation.source.clone(),
            target,
            origin: EdgeOrigin::Static,
            confidence: 0.8,
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

fn cache_path(root: &Path) -> PathBuf {
    root.join(".prism").join("cache.bin")
}

fn load_cache(root: &Path) -> Option<Graph> {
    let cache_path = cache_path(root);
    let bytes = fs::read(cache_path).ok()?;
    bincode::deserialize(&bytes).ok()
}

fn save_cache(root: &Path, graph: &Graph) -> Result<()> {
    let cache_path = cache_path(root);
    if let Some(parent) = cache_path.parent() {
        fs::create_dir_all(parent)?;
    }
    let bytes = bincode::serialize(graph)?;
    fs::write(cache_path, bytes)?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use std::fs;
    use std::path::PathBuf;
    use std::time::{SystemTime, UNIX_EPOCH};

    use prism_ir::EdgeKind;

    use super::WorkspaceIndexer;

    #[test]
    fn reindexes_incrementally_across_file_changes() {
        let root = temp_workspace();
        fs::create_dir_all(root.join("src")).unwrap();
        fs::write(
            root.join("Cargo.toml"),
            "[package]\nname = \"demo\"\nversion = \"0.1.0\"\n",
        )
        .unwrap();
        fs::write(
            root.join("src/lib.rs"),
            "fn alpha() { beta(); }\nfn beta() {}\n",
        )
        .unwrap();

        let mut indexer = WorkspaceIndexer::new(&root).unwrap();
        indexer.index().unwrap();

        let initial_calls = indexer
            .graph()
            .edges
            .iter()
            .filter(|edge| edge.kind == EdgeKind::Calls)
            .count();
        assert_eq!(initial_calls, 1);

        fs::write(
            root.join("src/lib.rs"),
            "fn alpha() { gamma(); }\nfn gamma() {}\n",
        )
        .unwrap();
        indexer.index().unwrap();

        assert!(indexer
            .graph()
            .nodes_by_name("gamma")
            .into_iter()
            .any(|node| node.id.path == "prism::gamma" || node.id.path.ends_with("::gamma")));
        assert_eq!(
            indexer
                .graph()
                .edges
                .iter()
                .filter(|edge| edge.kind == EdgeKind::Calls)
                .count(),
            1
        );

        fs::remove_file(root.join("src/lib.rs")).unwrap();
        indexer.index().unwrap();

        assert!(indexer.graph().nodes_by_name("alpha").is_empty());
        assert!(indexer
            .graph()
            .edges
            .iter()
            .all(|edge| edge.kind != EdgeKind::Calls));

        let _ = fs::remove_dir_all(root);
    }

    #[test]
    fn reloads_graph_from_disk_cache() {
        let root = temp_workspace();
        fs::create_dir_all(root.join("src")).unwrap();
        fs::write(
            root.join("Cargo.toml"),
            "[package]\nname = \"demo\"\nversion = \"0.1.0\"\n",
        )
        .unwrap();
        fs::write(root.join("src/lib.rs"), "fn alpha() {}\n").unwrap();

        let mut first = WorkspaceIndexer::new(&root).unwrap();
        first.index().unwrap();
        drop(first);

        assert!(root.join(".prism/cache.bin").exists());

        let second = WorkspaceIndexer::new(&root).unwrap();
        assert!(second
            .graph()
            .nodes_by_name("alpha")
            .into_iter()
            .any(|node| node.id.path.ends_with("::alpha")));

        let _ = fs::remove_dir_all(root);
    }

    fn temp_workspace() -> PathBuf {
        let stamp = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        std::env::temp_dir().join(format!("prism-test-{}-{stamp}", std::process::id()))
    }
}
