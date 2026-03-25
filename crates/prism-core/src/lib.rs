use std::collections::hash_map::DefaultHasher;
use std::collections::{HashMap, HashSet};
use std::fs;
use std::hash::{Hash, Hasher};
use std::path::{Path, PathBuf};

use anyhow::Result;
use prism_history::HistoryStore;
use prism_ir::{
    Edge, EdgeKind, EdgeOrigin, GraphChange, Language, Node, NodeId, NodeKind, ObservedChangeSet,
    Span,
};
use prism_lang_json::JsonAdapter;
use prism_lang_markdown::MarkdownAdapter;
use prism_lang_rust::RustAdapter;
use prism_lang_yaml::YamlAdapter;
use prism_parser::{
    LanguageAdapter, ParseInput, ParseResult, SymbolTarget, UnresolvedCall, UnresolvedImpl,
    UnresolvedImport,
};
use prism_query::Prism;
use prism_store::{FileState, Graph, SqliteStore, Store};
use smol_str::SmolStr;
use toml::Value;
use walkdir::WalkDir;

pub fn index_workspace(root: impl AsRef<Path>) -> Result<Prism> {
    let mut indexer = WorkspaceIndexer::new(root)?;
    indexer.index()?;
    Ok(indexer.into_prism())
}

pub struct WorkspaceIndexer<S: Store> {
    root: PathBuf,
    layout: WorkspaceLayout,
    graph: Graph,
    history: HistoryStore,
    adapters: Vec<Box<dyn LanguageAdapter>>,
    store: S,
}

#[derive(Debug, Clone)]
struct WorkspaceLayout {
    workspace_name: String,
    workspace_display_name: String,
    workspace_manifest: PathBuf,
    packages: Vec<PackageInfo>,
}

#[derive(Debug, Clone)]
struct PackageInfo {
    package_name: String,
    crate_name: String,
    root: PathBuf,
    manifest_path: PathBuf,
    node_id: NodeId,
}

#[derive(Debug, Clone)]
struct PendingFileParse {
    path: PathBuf,
    source: String,
    hash: u64,
    previous_path: Option<PathBuf>,
}

impl WorkspaceIndexer<SqliteStore> {
    pub fn new(root: impl AsRef<Path>) -> Result<Self> {
        let root = root.as_ref().canonicalize()?;
        cleanup_legacy_cache(&root)?;
        let store = SqliteStore::open(cache_path(&root))?;
        Self::with_store(root, store)
    }
}

impl<S: Store> WorkspaceIndexer<S> {
    pub fn with_store(root: impl AsRef<Path>, mut store: S) -> Result<Self> {
        let root = root.as_ref().canonicalize()?;
        let layout = discover_layout(&root)?;
        let mut graph = store.load_graph()?.unwrap_or_default();
        sync_root_nodes(&mut graph, &layout);
        let mut history = HistoryStore::new();
        history.seed_nodes(graph.all_nodes().map(|node| node.id.clone()));

        Ok(Self {
            root,
            layout,
            graph,
            history,
            adapters: default_adapters(),
            store,
        })
    }

    pub fn index(&mut self) -> Result<()> {
        let _ = self.index_with_observed_changes()?;
        Ok(())
    }

    pub fn index_with_changes(&mut self) -> Result<Vec<GraphChange>> {
        let (_, changes) = self.index_impl()?;
        Ok(changes)
    }

    pub fn index_with_observed_changes(&mut self) -> Result<Vec<ObservedChangeSet>> {
        let (observed, _) = self.index_impl()?;
        Ok(observed)
    }

    fn index_impl(&mut self) -> Result<(Vec<ObservedChangeSet>, Vec<GraphChange>)> {
        let mut pending = Vec::<PendingFileParse>::new();
        let mut seen_files = HashSet::<PathBuf>::new();
        let mut observed_changes = Vec::<ObservedChangeSet>::new();
        let mut changes = Vec::<GraphChange>::new();
        let walk_root = self.root.clone();

        for entry in WalkDir::new(&walk_root)
            .into_iter()
            .filter_entry(|entry| should_walk(entry.path(), &walk_root))
            .filter_map(Result::ok)
        {
            if !entry.file_type().is_file() {
                continue;
            }

            let path = entry.path();
            let Some(_adapter) = self
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
            pending.push(PendingFileParse {
                path: canonical_path,
                source,
                hash,
                previous_path: None,
            });
        }

        let moved_paths = detect_moved_files(&self.graph, &seen_files, &mut pending);

        for pending_file in pending {
            if pending_file.previous_path.is_none()
                && self
                    .graph
                    .file_record(&pending_file.path)
                    .map(|record| record.hash == pending_file.hash)
                    .unwrap_or(false)
            {
                continue;
            }

            let Some(adapter) = self
                .adapters
                .iter()
                .find(|adapter| adapter.supports_path(&pending_file.path))
            else {
                continue;
            };

            let previous_path = pending_file.previous_path.as_deref();
            let file_id = previous_path
                .and_then(|path| self.graph.file_record(path).map(|record| record.file_id))
                .unwrap_or_else(|| self.graph.ensure_file(&pending_file.path));
            let package = self.layout.package_for(&pending_file.path).clone();
            let input = ParseInput {
                package_name: &package.package_name,
                crate_name: &package.crate_name,
                package_root: &package.root,
                path: &pending_file.path,
                file_id,
                source: &pending_file.source,
            };
            let parsed = adapter.parse(&input)?;
            let update = self.upsert_parsed_file(
                previous_path,
                &pending_file.path,
                pending_file.hash,
                &package,
                parsed,
            );
            self.history.apply(&update.observed);
            observed_changes.push(update.observed.clone());
            changes.extend(update.changes);
            self.store
                .save_file_state(&pending_file.path, &self.graph)?;
        }

        for tracked in self.graph.tracked_files() {
            if !seen_files.contains(&tracked) && !moved_paths.contains(&tracked) {
                let update = self.graph.remove_file_with_update(&tracked);
                self.history.apply(&update.observed);
                observed_changes.push(update.observed.clone());
                changes.extend(update.changes);
                self.store.remove_file_state(&tracked)?;
            }
        }

        self.resolve_all_edges();
        self.store.replace_derived_edges(&self.graph)?;
        self.store.finalize(&self.graph)?;
        self.history
            .seed_nodes(self.graph.all_nodes().map(|node| node.id.clone()));
        Ok((observed_changes, changes))
    }

    pub fn graph(&self) -> &Graph {
        &self.graph
    }

    pub fn into_prism(self) -> Prism {
        Prism::with_history(self.graph, self.history)
    }

    fn upsert_parsed_file(
        &mut self,
        previous_path: Option<&Path>,
        path: &Path,
        hash: u64,
        package: &PackageInfo,
        parsed: ParseResult,
    ) -> prism_store::FileUpdate {
        let previous_state = previous_path
            .or(Some(path))
            .and_then(|candidate| self.graph.file_state(candidate));
        let reanchors = previous_state
            .as_ref()
            .map(|state| infer_reanchors(state, &parsed))
            .unwrap_or_default();
        let package_id = package.node_id.clone();
        let contained_nodes = parsed
            .edges
            .iter()
            .filter(|edge| edge.kind == EdgeKind::Contains)
            .map(|edge| edge.target.clone())
            .collect::<HashSet<_>>();
        let package_edges = parsed
            .nodes
            .iter()
            .filter(|node| !contained_nodes.contains(&node.id))
            .map(|node| Edge {
                kind: EdgeKind::Contains,
                source: package_id.clone(),
                target: node.id.clone(),
                origin: EdgeOrigin::Static,
                confidence: 1.0,
            })
            .collect::<Vec<_>>();

        let mut edges = parsed.edges;
        edges.extend(package_edges);
        self.graph.upsert_file_from(
            previous_path,
            path,
            hash,
            parsed.nodes,
            edges,
            parsed.fingerprints,
            parsed.unresolved_calls,
            parsed.unresolved_imports,
            parsed.unresolved_impls,
            &reanchors,
        )
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

fn detect_moved_files(
    graph: &Graph,
    seen_files: &HashSet<PathBuf>,
    pending: &mut [PendingFileParse],
) -> HashSet<PathBuf> {
    let mut old_by_hash = HashMap::<u64, Vec<PathBuf>>::new();
    for tracked in graph.tracked_files() {
        if seen_files.contains(&tracked) {
            continue;
        }
        if let Some(record) = graph.file_record(&tracked) {
            old_by_hash.entry(record.hash).or_default().push(tracked);
        }
    }

    let mut moved_paths = HashSet::new();
    for pending_file in pending
        .iter_mut()
        .filter(|pending_file| graph.file_record(&pending_file.path).is_none())
    {
        let Some(candidates) = old_by_hash.get(&pending_file.hash) else {
            continue;
        };
        let available = candidates
            .iter()
            .filter(|candidate| !moved_paths.contains(*candidate))
            .collect::<Vec<_>>();
        if available.len() == 1 {
            let previous = (*available[0]).clone();
            pending_file.previous_path = Some(previous.clone());
            moved_paths.insert(previous);
        }
    }

    moved_paths
}

fn infer_reanchors(previous: &FileState, parsed: &ParseResult) -> Vec<(NodeId, NodeId)> {
    let previous_nodes = previous
        .nodes
        .iter()
        .map(|node| (node.id.clone(), node))
        .collect::<HashMap<_, _>>();
    let parsed_nodes = parsed
        .nodes
        .iter()
        .map(|node| (node.id.clone(), node))
        .collect::<HashMap<_, _>>();

    let mut matched_old = HashSet::<NodeId>::new();
    let mut matched_new = HashSet::<NodeId>::new();
    let mut reanchors = Vec::<(NodeId, NodeId)>::new();
    let mut old_by_fingerprint = HashMap::<prism_parser::NodeFingerprint, Vec<NodeId>>::new();
    let mut new_by_fingerprint = HashMap::<prism_parser::NodeFingerprint, Vec<NodeId>>::new();

    for node in previous
        .nodes
        .iter()
        .filter(|node| parsed_nodes.contains_key(&node.id))
    {
        matched_old.insert(node.id.clone());
        matched_new.insert(node.id.clone());
    }

    for (id, fingerprint) in &previous.record.fingerprints {
        if previous_nodes.contains_key(id) {
            old_by_fingerprint
                .entry(fingerprint.clone())
                .or_default()
                .push(id.clone());
        }
    }

    for (id, fingerprint) in &parsed.fingerprints {
        if parsed_nodes.contains_key(id) {
            new_by_fingerprint
                .entry(fingerprint.clone())
                .or_default()
                .push(id.clone());
        }
    }

    for (fingerprint, old_ids) in &old_by_fingerprint {
        let Some(new_ids) = new_by_fingerprint.get(fingerprint) else {
            continue;
        };
        let available_old = old_ids
            .iter()
            .filter(|id| !matched_old.contains(*id))
            .cloned()
            .collect::<Vec<_>>();
        let available_new = new_ids
            .iter()
            .filter(|id| !matched_new.contains(*id))
            .cloned()
            .collect::<Vec<_>>();

        if available_old.len() == 1 && available_new.len() == 1 {
            let old = available_old[0].clone();
            let new = available_new[0].clone();
            matched_old.insert(old.clone());
            matched_new.insert(new.clone());
            if old != new {
                reanchors.push((old, new));
            }
        }
    }

    for (fingerprint, old_ids) in old_by_fingerprint {
        let Some(new_ids) = new_by_fingerprint.get(&fingerprint) else {
            continue;
        };

        for old_id in old_ids {
            if matched_old.contains(&old_id) {
                continue;
            }

            let Some(old_node) = previous_nodes.get(&old_id) else {
                continue;
            };
            let best = new_ids
                .iter()
                .filter(|new_id| !matched_new.contains(*new_id))
                .filter_map(|new_id| {
                    let new_node = parsed_nodes.get(new_id)?;
                    Some((score_reanchor_candidate(old_node, new_node), new_id.clone()))
                })
                .filter(|(score, _)| *score >= 40)
                .max_by_key(|(score, _)| *score);

            if let Some((_, new_id)) = best {
                matched_old.insert(old_id.clone());
                matched_new.insert(new_id.clone());
                if old_id != new_id {
                    reanchors.push((old_id, new_id));
                }
            }
        }
    }

    reanchors
}

fn score_reanchor_candidate(old: &Node, new: &Node) -> i32 {
    if old.kind != new.kind || old.language != new.language {
        return 0;
    }

    let mut score = 0;
    if old.name == new.name {
        score += 20;
    }
    if old.id.crate_name == new.id.crate_name {
        score += 10;
    }
    if parent_path(old.id.path.as_str()) == parent_path(new.id.path.as_str()) {
        score += 10;
    }

    let start_delta = old.span.start_line.abs_diff(new.span.start_line);
    score += (20 - start_delta.min(20)) as i32;

    let end_delta = old.span.end_line.abs_diff(new.span.end_line);
    score += (20 - end_delta.min(20)) as i32;

    score
}

fn parent_path(path: &str) -> &str {
    path.rsplit_once("::")
        .map(|(parent, _)| parent)
        .unwrap_or(path)
}

fn default_adapters() -> Vec<Box<dyn LanguageAdapter>> {
    vec![
        Box::new(RustAdapter),
        Box::new(MarkdownAdapter),
        Box::new(JsonAdapter),
        Box::new(YamlAdapter),
    ]
}

impl WorkspaceLayout {
    fn package_for(&self, path: &Path) -> &PackageInfo {
        self.packages
            .iter()
            .filter(|package| path.starts_with(&package.root))
            .max_by_key(|package| package.root.components().count())
            .unwrap_or(&self.packages[0])
    }
}

impl PackageInfo {
    fn new(package_name: String, root: PathBuf, manifest_path: PathBuf) -> Self {
        let crate_name = normalize_identifier(&package_name);
        let node_id = NodeId::new(crate_name.clone(), crate_name.clone(), NodeKind::Package);
        Self {
            package_name,
            crate_name,
            root,
            manifest_path,
            node_id,
        }
    }
}

fn sync_root_nodes(graph: &mut Graph, layout: &WorkspaceLayout) -> NodeId {
    let manifest_file = graph.ensure_file(&layout.workspace_manifest);
    let workspace_id = NodeId::new(
        layout.workspace_name.clone(),
        format!("{}::workspace", layout.workspace_name),
        NodeKind::Workspace,
    );
    let allowed_root_ids = std::iter::once(workspace_id.clone())
        .chain(
            layout
                .packages
                .iter()
                .map(|package| package.node_id.clone()),
        )
        .collect::<HashSet<_>>();
    graph.retain_root_nodes(&allowed_root_ids);

    graph.add_node(Node {
        id: workspace_id.clone(),
        name: SmolStr::new(layout.workspace_display_name.clone()),
        kind: NodeKind::Workspace,
        file: manifest_file,
        span: Span::line(1),
        language: Language::Unknown,
    });

    for package in &layout.packages {
        let manifest_file = graph.ensure_file(&package.manifest_path);
        graph.add_node(Node {
            id: package.node_id.clone(),
            name: SmolStr::new(package.package_name.clone()),
            kind: NodeKind::Package,
            file: manifest_file,
            span: Span::line(1),
            language: Language::Unknown,
        });
    }

    graph.clear_root_contains_edges();
    for package in &layout.packages {
        graph.add_edge(Edge {
            kind: EdgeKind::Contains,
            source: workspace_id.clone(),
            target: package.node_id.clone(),
            origin: EdgeOrigin::Static,
            confidence: 1.0,
        });
    }

    workspace_id
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

fn discover_layout(root: &Path) -> Result<WorkspaceLayout> {
    let workspace_display_name = root
        .file_name()
        .and_then(|value| value.to_str())
        .unwrap_or("workspace")
        .to_owned();
    let workspace_name = normalize_identifier(&workspace_display_name);
    let workspace_manifest = root.join("Cargo.toml");
    let root_package_name = manifest_package_name(&workspace_manifest)?
        .unwrap_or_else(|| workspace_display_name.clone());
    let mut packages = vec![PackageInfo::new(
        root_package_name,
        root.to_path_buf(),
        workspace_manifest.clone(),
    )];

    for entry in WalkDir::new(root)
        .into_iter()
        .filter_entry(|entry| should_walk(entry.path(), root))
        .filter_map(Result::ok)
    {
        if !entry.file_type().is_file() || entry.file_name() != "Cargo.toml" {
            continue;
        }

        let manifest_path = entry.path();
        if manifest_path == workspace_manifest {
            continue;
        }

        let Some(package_name) = manifest_package_name(manifest_path)? else {
            continue;
        };
        let package_root = manifest_path
            .parent()
            .unwrap_or(root)
            .canonicalize()
            .unwrap_or_else(|_| manifest_path.parent().unwrap_or(root).to_path_buf());
        packages.push(PackageInfo::new(
            package_name,
            package_root,
            manifest_path.to_path_buf(),
        ));
    }

    Ok(WorkspaceLayout {
        workspace_name,
        workspace_display_name,
        workspace_manifest,
        packages,
    })
}

fn manifest_package_name(path: &Path) -> Result<Option<String>> {
    if !path.exists() {
        return Ok(None);
    }

    let manifest = fs::read_to_string(path)?;
    let value: Value = toml::from_str(&manifest)?;
    Ok(value
        .get("package")
        .and_then(|package| package.get("name"))
        .and_then(Value::as_str)
        .map(ToOwned::to_owned))
}

fn normalize_identifier(value: &str) -> String {
    let mut normalized = String::new();
    let mut previous_underscore = false;

    for ch in value.chars() {
        let ch = ch.to_ascii_lowercase();
        if ch.is_ascii_alphanumeric() {
            normalized.push(ch);
            previous_underscore = false;
        } else if !previous_underscore {
            normalized.push('_');
            previous_underscore = true;
        }
    }

    let normalized = normalized.trim_matches('_').to_owned();
    if normalized.is_empty() {
        "workspace".to_owned()
    } else {
        normalized
    }
}

fn cache_path(root: &Path) -> PathBuf {
    root.join(".prism").join("cache.db")
}

fn cleanup_legacy_cache(root: &Path) -> Result<()> {
    let legacy = root.join(".prism").join("cache.bin");
    if legacy.exists() {
        fs::remove_file(legacy)?;
    }
    Ok(())
}

fn should_walk(path: &Path, root: &Path) -> bool {
    let Ok(relative) = path.strip_prefix(root) else {
        return true;
    };
    let Some(first) = relative.components().next() else {
        return true;
    };
    let first = first.as_os_str().to_string_lossy();
    !matches!(first.as_ref(), ".git" | ".prism" | "target")
}

#[cfg(test)]
mod tests {
    use std::fs;
    use std::path::PathBuf;
    use std::time::{SystemTime, UNIX_EPOCH};

    use prism_ir::{EdgeKind, GraphChange, NodeId, NodeKind};
    use prism_store::MemoryStore;

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

        let mut indexer = WorkspaceIndexer::with_store(&root, MemoryStore::default()).unwrap();
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

        assert!(root.join(".prism/cache.db").exists());

        let second = WorkspaceIndexer::new(&root).unwrap();
        assert!(second
            .graph()
            .nodes_by_name("alpha")
            .into_iter()
            .any(|node| node.id.path.ends_with("::alpha")));

        let _ = fs::remove_dir_all(root);
    }

    #[test]
    fn uses_member_package_identity_and_attaches_workspace_docs() {
        let root = temp_workspace();
        fs::create_dir_all(root.join("crates/alpha/src")).unwrap();
        fs::create_dir_all(root.join("crates/beta/src")).unwrap();
        fs::create_dir_all(root.join("docs")).unwrap();
        fs::write(
            root.join("Cargo.toml"),
            "[workspace]\nmembers = [\"crates/alpha\", \"crates/beta\"]\n",
        )
        .unwrap();
        fs::write(
            root.join("crates/alpha/Cargo.toml"),
            "[package]\nname = \"alpha-pkg\"\nversion = \"0.1.0\"\n",
        )
        .unwrap();
        fs::write(
            root.join("crates/beta/Cargo.toml"),
            "[package]\nname = \"beta-pkg\"\nversion = \"0.1.0\"\n",
        )
        .unwrap();
        fs::write(root.join("crates/alpha/src/lib.rs"), "fn alpha() {}\n").unwrap();
        fs::write(
            root.join("crates/beta/src/lib.rs"),
            "mod outer { mod inner {} }\n",
        )
        .unwrap();
        fs::write(root.join("docs/SPEC.md"), "# Spec\n").unwrap();

        let mut indexer = WorkspaceIndexer::with_store(&root, MemoryStore::default()).unwrap();
        indexer.index().unwrap();

        assert!(indexer
            .graph()
            .nodes_by_name("alpha")
            .into_iter()
            .any(|node| node.id.crate_name == "alpha_pkg" && node.id.path == "alpha_pkg::alpha"));
        assert!(indexer
            .graph()
            .nodes_by_name("inner")
            .into_iter()
            .any(
                |node| node.id.crate_name == "beta_pkg" && node.id.path == "beta_pkg::outer::inner"
            ));

        let inner_module = indexer
            .graph()
            .nodes_by_name("inner")
            .into_iter()
            .find(|node| node.kind == NodeKind::Module)
            .unwrap();
        assert!(!indexer
            .graph()
            .edges_to(&inner_module.id, Some(EdgeKind::Contains))
            .iter()
            .any(|edge| edge.source.kind == NodeKind::Package));

        let spec = indexer
            .graph()
            .nodes_by_name("Spec")
            .into_iter()
            .find(|node| node.kind == NodeKind::MarkdownHeading)
            .unwrap();
        let spec_document = indexer
            .graph()
            .nodes_by_name("docs/SPEC.md")
            .into_iter()
            .find(|node| node.kind == NodeKind::Document)
            .unwrap();
        assert!(indexer
            .graph()
            .edges_to(&spec_document.id, Some(EdgeKind::Contains))
            .iter()
            .any(|edge| edge.source.kind == NodeKind::Package));
        assert!(indexer
            .graph()
            .edges_to(&spec.id, Some(EdgeKind::Contains))
            .iter()
            .any(|edge| edge.source == spec_document.id));

        let _ = fs::remove_dir_all(root);
    }

    #[test]
    fn emits_reanchored_change_for_symbol_rename() {
        let root = temp_workspace();
        fs::create_dir_all(root.join("src")).unwrap();
        fs::write(
            root.join("Cargo.toml"),
            "[package]\nname = \"demo\"\nversion = \"0.1.0\"\n",
        )
        .unwrap();
        fs::write(
            root.join("src/lib.rs"),
            "fn alpha() { helper(); }\nfn helper() {}\n",
        )
        .unwrap();

        let mut indexer = WorkspaceIndexer::with_store(&root, MemoryStore::default()).unwrap();
        indexer.index().unwrap();

        fs::write(
            root.join("src/lib.rs"),
            "fn renamed_alpha() { helper(); }\nfn helper() {}\n",
        )
        .unwrap();

        let changes = indexer.index_with_changes().unwrap();

        assert!(changes.contains(&GraphChange::Reanchored {
            old: NodeId::new("demo", "demo::alpha", NodeKind::Function),
            new: NodeId::new("demo", "demo::renamed_alpha", NodeKind::Function),
        }));

        let _ = fs::remove_dir_all(root);
    }

    #[test]
    fn emits_reanchored_changes_for_file_move_with_same_content() {
        let root = temp_workspace();
        fs::create_dir_all(root.join("src")).unwrap();
        fs::write(
            root.join("Cargo.toml"),
            "[package]\nname = \"demo\"\nversion = \"0.1.0\"\n",
        )
        .unwrap();
        fs::write(
            root.join("src/feature.rs"),
            "pub fn alpha() { helper(); }\nfn helper() {}\n",
        )
        .unwrap();

        let mut indexer = WorkspaceIndexer::with_store(&root, MemoryStore::default()).unwrap();
        indexer.index().unwrap();

        fs::rename(root.join("src/feature.rs"), root.join("src/renamed.rs")).unwrap();

        let changes = indexer.index_with_changes().unwrap();

        assert!(changes.contains(&GraphChange::Reanchored {
            old: NodeId::new("demo", "demo::feature", NodeKind::Module),
            new: NodeId::new("demo", "demo::renamed", NodeKind::Module),
        }));
        assert!(changes.contains(&GraphChange::Reanchored {
            old: NodeId::new("demo", "demo::feature::alpha", NodeKind::Function),
            new: NodeId::new("demo", "demo::renamed::alpha", NodeKind::Function),
        }));

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
