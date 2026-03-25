use std::collections::{HashMap, HashSet, VecDeque};
use std::fs;
use std::path::Path;
use std::sync::Arc;

use prism_history::{HistorySnapshot, HistoryStore};
use prism_ir::{
    AnchorRef, Edge, EdgeKind, LineageEvent, LineageId, Node, NodeId, NodeKind, Skeleton, Subgraph,
    TaskId,
};
use prism_memory::{
    OutcomeEvent, OutcomeEvidence, OutcomeMemory, OutcomeMemorySnapshot, TaskReplay,
};
use prism_store::Graph;
use serde::{Deserialize, Serialize};

pub struct Prism {
    graph: Arc<Graph>,
    history: Arc<HistoryStore>,
    outcomes: Arc<OutcomeMemory>,
}

#[derive(Debug, Clone, Default)]
pub struct ChangeImpact {
    pub direct_nodes: Vec<NodeId>,
    pub lineages: Vec<LineageId>,
    pub likely_validations: Vec<String>,
    pub validation_checks: Vec<ValidationCheck>,
    pub co_change_neighbors: Vec<CoChange>,
    pub risk_events: Vec<OutcomeEvent>,
}

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct ValidationCheck {
    pub label: String,
    pub score: f32,
    pub last_seen: u64,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct CoChange {
    pub lineage: LineageId,
    pub count: u32,
    pub nodes: Vec<NodeId>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ValidationRecipe {
    pub target: NodeId,
    pub checks: Vec<String>,
    pub scored_checks: Vec<ValidationCheck>,
    pub related_nodes: Vec<NodeId>,
    pub co_change_neighbors: Vec<CoChange>,
    pub recent_failures: Vec<OutcomeEvent>,
}

impl Prism {
    pub fn new(graph: Graph) -> Self {
        let mut history = HistoryStore::new();
        history.seed_nodes(graph.all_nodes().map(|node| node.id.clone()));
        Self::with_history(graph, history)
    }

    pub fn with_history(graph: Graph, history: HistoryStore) -> Self {
        Self::with_history_and_outcomes(graph, history, OutcomeMemory::new())
    }

    pub fn with_history_and_outcomes(
        graph: Graph,
        history: HistoryStore,
        outcomes: OutcomeMemory,
    ) -> Self {
        Self {
            graph: Arc::new(graph),
            history: Arc::new(history),
            outcomes: Arc::new(outcomes),
        }
    }

    pub fn graph(&self) -> &Graph {
        self.graph.as_ref()
    }

    pub fn lineage_of(&self, node: &NodeId) -> Option<LineageId> {
        self.history.lineage_of(node)
    }

    pub fn lineage_history(&self, lineage: &LineageId) -> Vec<LineageEvent> {
        self.history.lineage_history(lineage)
    }

    pub fn outcome_memory(&self) -> Arc<OutcomeMemory> {
        Arc::clone(&self.outcomes)
    }

    pub fn anchors_for(&self, anchors: &[AnchorRef]) -> Vec<AnchorRef> {
        self.expand_anchors(anchors)
    }

    pub fn history_snapshot(&self) -> HistorySnapshot {
        self.history.snapshot()
    }

    pub fn outcome_snapshot(&self) -> OutcomeMemorySnapshot {
        self.outcomes.snapshot()
    }

    pub fn outcomes_for(&self, anchors: &[AnchorRef], limit: usize) -> Vec<OutcomeEvent> {
        self.outcomes
            .outcomes_for(&self.expand_anchors(anchors), limit)
    }

    pub fn related_failures(&self, node: &NodeId) -> Vec<OutcomeEvent> {
        self.outcomes
            .related_failures(&self.expand_anchors(&[AnchorRef::Node(node.clone())]), 20)
    }

    pub fn blast_radius(&self, node: &NodeId) -> ChangeImpact {
        let mut direct_nodes = self.graph_neighbors(node);
        let mut lineages = direct_nodes
            .iter()
            .filter_map(|neighbor| self.lineage_of(neighbor))
            .collect::<Vec<_>>();
        let co_change_neighbors = self.co_change_neighbors(node, 8);
        if let Some(lineage) = self.lineage_of(node) {
            lineages.push(lineage);
        }
        lineages.extend(
            co_change_neighbors
                .iter()
                .map(|neighbor| neighbor.lineage.clone()),
        );
        lineages.sort_by(|left, right| left.0.cmp(&right.0));
        lineages.dedup();

        direct_nodes.extend(
            lineages
                .iter()
                .flat_map(|lineage| self.history.current_nodes_for_lineage(lineage)),
        );
        direct_nodes.retain(|candidate| candidate != node);
        sort_node_ids(&mut direct_nodes);

        let mut impact_anchors = vec![AnchorRef::Node(node.clone())];
        impact_anchors.extend(direct_nodes.iter().cloned().map(AnchorRef::Node));
        impact_anchors.extend(lineages.iter().cloned().map(AnchorRef::Lineage));
        let impact_anchors = self.expand_anchors(&impact_anchors);

        let risk_events = self.outcomes.related_failures(&impact_anchors, 20);
        let validation_checks = infer_validation_checks(
            self.outcomes.outcomes_for(&impact_anchors, 100),
            &impact_anchors,
            8,
        );
        let likely_validations = validation_checks
            .iter()
            .map(|check| check.label.clone())
            .collect();

        ChangeImpact {
            direct_nodes,
            lineages,
            likely_validations,
            validation_checks,
            co_change_neighbors,
            risk_events,
        }
    }

    pub fn resume_task(&self, task: &TaskId) -> TaskReplay {
        self.outcomes.resume_task(task)
    }

    pub fn co_change_neighbors(&self, node: &NodeId, limit: usize) -> Vec<CoChange> {
        let Some(lineage) = self.lineage_of(node) else {
            return Vec::new();
        };

        self.history
            .co_change_neighbors(&lineage, limit)
            .into_iter()
            .map(|neighbor| {
                let mut nodes = self.history.current_nodes_for_lineage(&neighbor.lineage);
                sort_node_ids(&mut nodes);
                CoChange {
                    lineage: neighbor.lineage,
                    count: neighbor.count,
                    nodes,
                }
            })
            .collect()
    }

    pub fn validation_recipe(&self, node: &NodeId) -> ValidationRecipe {
        let impact = self.blast_radius(node);
        ValidationRecipe {
            target: node.clone(),
            checks: impact.likely_validations,
            scored_checks: impact.validation_checks,
            related_nodes: impact.direct_nodes,
            co_change_neighbors: impact.co_change_neighbors,
            recent_failures: impact.risk_events,
        }
    }

    pub fn symbol(&self, query: &str) -> Vec<Symbol<'_>> {
        let matches = self.sorted_matches(query);
        let Some(best_score) = matches.first().map(|entry| entry.score) else {
            return Vec::new();
        };

        matches
            .into_iter()
            .take_while(|entry| entry.score == best_score)
            .map(|entry| Symbol {
                prism: self,
                id: entry.node.id.clone(),
            })
            .collect()
    }

    pub fn search(
        &self,
        query: &str,
        limit: usize,
        kind: Option<NodeKind>,
        path_filter: Option<&str>,
    ) -> Vec<Symbol<'_>> {
        let path_filter = path_filter.map(|value| value.trim().to_ascii_lowercase());
        self.sorted_matches(query)
            .into_iter()
            .filter(|entry| kind.map_or(true, |kind| entry.node.kind == kind))
            .filter(|entry| {
                path_filter
                    .as_deref()
                    .map_or(true, |filter| self.matches_path_filter(entry.node, filter))
            })
            .take(limit)
            .map(|entry| Symbol {
                prism: self,
                id: entry.node.id.clone(),
            })
            .collect()
    }

    pub fn entrypoints(&self) -> Vec<Symbol<'_>> {
        let mains: Vec<_> = self
            .graph
            .all_nodes()
            .filter(|node| matches!(node.kind, NodeKind::Function | NodeKind::Method))
            .filter(|node| node.name == "main")
            .map(|node| Symbol {
                prism: self,
                id: node.id.clone(),
            })
            .collect();
        if !mains.is_empty() {
            return mains;
        }

        self.graph
            .all_nodes()
            .filter(|node| matches!(node.kind, NodeKind::Function | NodeKind::Method))
            .filter(|node| {
                self.graph
                    .edges_to(&node.id, Some(EdgeKind::Calls))
                    .is_empty()
            })
            .map(|node| Symbol {
                prism: self,
                id: node.id.clone(),
            })
            .collect()
    }

    fn sorted_matches(&self, query: &str) -> Vec<Match<'_>> {
        let query = query.trim();
        if query.is_empty() {
            return Vec::new();
        }

        let query_lower = query.to_ascii_lowercase();
        let mut matches = self
            .graph
            .all_nodes()
            .filter_map(|node| {
                match_score(node, query, &query_lower).map(|score| Match {
                    score,
                    is_test: is_test_node(node),
                    path_len: node.id.path.len(),
                    path: node.id.path.as_str().to_owned(),
                    node,
                })
            })
            .collect::<Vec<_>>();

        matches.sort_by(|left, right| {
            left.score
                .cmp(&right.score)
                .then_with(|| left.is_test.cmp(&right.is_test))
                .then_with(|| left.path_len.cmp(&right.path_len))
                .then_with(|| left.path.cmp(&right.path))
        });

        matches
    }

    fn matches_path_filter(&self, node: &Node, path_filter: &str) -> bool {
        self.graph
            .file_path(node.file)
            .map(|path| {
                path.to_string_lossy()
                    .to_ascii_lowercase()
                    .contains(path_filter)
            })
            .unwrap_or(false)
            || node
                .id
                .path
                .as_str()
                .to_ascii_lowercase()
                .contains(path_filter)
            || node
                .name
                .as_str()
                .to_ascii_lowercase()
                .contains(path_filter)
    }

    fn expand_anchors(&self, anchors: &[AnchorRef]) -> Vec<AnchorRef> {
        let mut expanded = anchors.to_vec();
        for anchor in anchors {
            if let AnchorRef::Node(node) = anchor {
                if let Some(lineage) = self.lineage_of(node) {
                    expanded.push(AnchorRef::Lineage(lineage));
                }
            }
        }
        expanded.sort_by(anchor_sort_key);
        expanded.dedup();
        expanded
    }

    fn graph_neighbors(&self, node: &NodeId) -> Vec<NodeId> {
        let mut neighbors = self
            .graph
            .edges_from(node, None)
            .into_iter()
            .map(|edge| edge.target.clone())
            .chain(
                self.graph
                    .edges_to(node, None)
                    .into_iter()
                    .map(|edge| edge.source.clone()),
            )
            .collect::<Vec<_>>();
        sort_node_ids(&mut neighbors);
        neighbors
    }
}

struct Match<'a> {
    score: u8,
    is_test: bool,
    path_len: usize,
    path: String,
    node: &'a Node,
}

pub struct Symbol<'a> {
    prism: &'a Prism,
    id: NodeId,
}

#[derive(Debug, Clone, Default)]
pub struct Relations {
    pub outgoing_calls: Vec<NodeId>,
    pub incoming_calls: Vec<NodeId>,
    pub outgoing_imports: Vec<NodeId>,
    pub incoming_imports: Vec<NodeId>,
    pub outgoing_implements: Vec<NodeId>,
    pub incoming_implements: Vec<NodeId>,
}

impl<'a> Symbol<'a> {
    pub fn id(&self) -> &NodeId {
        &self.id
    }

    pub fn node(&self) -> &Node {
        self.prism
            .graph
            .node(&self.id)
            .expect("symbol node must exist in graph")
    }

    pub fn name(&self) -> &str {
        self.node().name.as_str()
    }

    pub fn signature(&self) -> String {
        format!("{} {}", self.node().kind, self.id.path)
    }

    pub fn skeleton(&self) -> Skeleton {
        let calls = self.targets(EdgeKind::Calls);
        Skeleton { calls }
    }

    pub fn imports(&self) -> Vec<NodeId> {
        self.targets(EdgeKind::Imports)
    }

    pub fn imported_by(&self) -> Vec<NodeId> {
        self.sources(EdgeKind::Imports)
    }

    pub fn implements(&self) -> Vec<NodeId> {
        self.targets(EdgeKind::Implements)
    }

    pub fn implemented_by(&self) -> Vec<NodeId> {
        self.sources(EdgeKind::Implements)
    }

    pub fn callers(&self) -> Vec<NodeId> {
        self.sources(EdgeKind::Calls)
    }

    pub fn relations(&self) -> Relations {
        Relations {
            outgoing_calls: self.targets(EdgeKind::Calls),
            incoming_calls: self.sources(EdgeKind::Calls),
            outgoing_imports: self.targets(EdgeKind::Imports),
            incoming_imports: self.sources(EdgeKind::Imports),
            outgoing_implements: self.targets(EdgeKind::Implements),
            incoming_implements: self.sources(EdgeKind::Implements),
        }
    }

    pub fn full(&self) -> String {
        let node = self.node();
        let Some(path) = self.prism.graph.file_path(node.file) else {
            return String::new();
        };
        let Ok(source) = fs::read_to_string(path) else {
            return String::new();
        };

        let start = usize::min(node.span.start as usize, source.len());
        let end = usize::min(node.span.end as usize, source.len());
        source.get(start..end).unwrap_or_default().to_owned()
    }

    pub fn call_graph(&self, depth: usize) -> Subgraph {
        let mut visited = HashSet::new();
        let mut nodes = Vec::new();
        let mut edges = Vec::<Edge>::new();
        let mut queue = VecDeque::from([(self.id.clone(), 0usize)]);
        let mut max_depth_reached = None;

        while let Some((current, current_depth)) = queue.pop_front() {
            if !visited.insert(current.clone()) {
                continue;
            }
            nodes.push(current.clone());
            max_depth_reached = Some(max_depth_reached.map_or(current_depth, |max| max.max(current_depth)));

            if current_depth >= depth {
                continue;
            }

            for edge in self.prism.graph.edges_from(&current, Some(EdgeKind::Calls)) {
                edges.push(edge.clone());
                queue.push_back((edge.target.clone(), current_depth + 1));
            }
        }

        Subgraph {
            root: self.id.clone(),
            nodes,
            edges,
            truncated: false,
            max_depth_reached,
        }
    }

    fn targets(&self, kind: EdgeKind) -> Vec<NodeId> {
        self.prism
            .graph
            .edges_from(&self.id, Some(kind))
            .into_iter()
            .map(|edge| edge.target.clone())
            .collect()
    }

    fn sources(&self, kind: EdgeKind) -> Vec<NodeId> {
        self.prism
            .graph
            .edges_to(&self.id, Some(kind))
            .into_iter()
            .map(|edge| edge.source.clone())
            .collect()
    }
}

fn match_score(node: &Node, query: &str, query_lower: &str) -> Option<u8> {
    let name = node.name.as_str();
    let path = node.id.path.as_str();
    let name_lower = name.to_ascii_lowercase();
    let path_lower = path.to_ascii_lowercase();

    if path == query {
        Some(0)
    } else if name == query {
        Some(1)
    } else if last_path_segment(path) == Some(query) {
        Some(2)
    } else if node.kind == NodeKind::Document && document_stem(name).as_deref() == Some(query_lower)
    {
        Some(3)
    } else if name_lower == query_lower {
        Some(4)
    } else if path_lower == query_lower {
        Some(5)
    } else if path.ends_with(&format!("::{query}")) {
        Some(6)
    } else if path_lower.ends_with(&format!("::{}", query_lower)) {
        Some(7)
    } else if node.kind == NodeKind::Document && has_token(&name_lower, query_lower) {
        Some(8)
    } else if has_token(&name_lower, query_lower) {
        Some(9)
    } else if has_token(&path_lower, query_lower) {
        Some(10)
    } else if node.kind == NodeKind::Document && has_token_prefix(&name_lower, query_lower) {
        Some(11)
    } else if has_token_prefix(&name_lower, query_lower) {
        Some(12)
    } else if has_token_prefix(&path_lower, query_lower) {
        Some(13)
    } else {
        None
    }
}

fn last_path_segment(path: &str) -> Option<&str> {
    path.rsplit("::").next()
}

fn document_stem(name: &str) -> Option<String> {
    Path::new(name)
        .file_stem()
        .and_then(|stem| stem.to_str())
        .map(|stem| stem.to_ascii_lowercase())
}

fn has_token(value: &str, query: &str) -> bool {
    tokens(value).any(|token| token == query)
}

fn has_token_prefix(value: &str, query: &str) -> bool {
    tokens(value).any(|token| token.starts_with(query))
}

fn tokens(value: &str) -> impl Iterator<Item = &str> {
    value
        .split(|ch: char| !ch.is_ascii_alphanumeric())
        .filter(|token| !token.is_empty())
}

fn is_test_node(node: &Node) -> bool {
    let path = node.id.path.as_str();
    path.contains("::tests::") || path.ends_with("::tests")
}

fn sort_node_ids(nodes: &mut Vec<NodeId>) {
    nodes.sort_by(|left, right| {
        left.crate_name
            .cmp(&right.crate_name)
            .then_with(|| left.path.cmp(&right.path))
            .then_with(|| left.kind.to_string().cmp(&right.kind.to_string()))
    });
    nodes.dedup();
}

fn infer_validation_checks(
    events: Vec<OutcomeEvent>,
    focus: &[AnchorRef],
    limit: usize,
) -> Vec<ValidationCheck> {
    let focus = focus.iter().collect::<HashSet<_>>();
    let mut scores = HashMap::<String, (f32, u64)>::new();

    for event in events {
        let overlap = event
            .anchors
            .iter()
            .filter(|anchor| focus.contains(anchor))
            .count()
            .max(1) as f32;
        let event_weight = match event.kind {
            prism_memory::OutcomeKind::FailureObserved
            | prism_memory::OutcomeKind::RegressionObserved => 2.5,
            prism_memory::OutcomeKind::FixValidated => 2.0,
            prism_memory::OutcomeKind::BuildRan | prism_memory::OutcomeKind::TestRan => 1.25,
            _ => 1.0,
        };
        let result_weight = match event.result {
            prism_memory::OutcomeResult::Failure => 2.0,
            prism_memory::OutcomeResult::Success => 1.0,
            prism_memory::OutcomeResult::Partial => 0.75,
            prism_memory::OutcomeResult::Unknown => 0.5,
        };
        let score = overlap * event_weight * result_weight;

        for label in validation_labels(&event.evidence) {
            let entry = scores.entry(label).or_insert((0.0, 0));
            entry.0 += score;
            entry.1 = entry.1.max(event.meta.ts);
        }
    }

    let mut checks = scores.into_iter().collect::<Vec<_>>();
    checks.sort_by(|left, right| {
        right
            .1
            .0
            .total_cmp(&left.1 .0)
            .then_with(|| right.1 .1.cmp(&left.1 .1))
            .then_with(|| left.0.cmp(&right.0))
    });
    if limit > 0 {
        checks.truncate(limit);
    }
    checks
        .into_iter()
        .map(|(label, (score, last_seen))| ValidationCheck {
            label,
            score,
            last_seen,
        })
        .collect()
}

fn validation_labels(evidence: &[OutcomeEvidence]) -> Vec<String> {
    evidence
        .iter()
        .filter_map(|evidence| match evidence {
            OutcomeEvidence::Test { name, .. } => Some(format!("test:{name}")),
            OutcomeEvidence::Build { target, .. } => Some(format!("build:{target}")),
            _ => None,
        })
        .collect()
}

fn anchor_sort_key(left: &AnchorRef, right: &AnchorRef) -> std::cmp::Ordering {
    anchor_label(left).cmp(&anchor_label(right))
}

fn anchor_label(anchor: &AnchorRef) -> String {
    match anchor {
        AnchorRef::Node(node) => format!("node:{}:{}:{}", node.crate_name, node.path, node.kind),
        AnchorRef::Lineage(lineage) => format!("lineage:{}", lineage.0),
        AnchorRef::File(file) => format!("file:{}", file.0),
        AnchorRef::Kind(kind) => format!("kind:{kind}"),
    }
}

#[cfg(test)]
mod tests {
    use prism_history::HistoryStore;
    use prism_ir::{
        AnchorRef, ChangeTrigger, Edge, EdgeKind, EventActor, EventId, EventMeta, FileId,
        Language, Node, NodeId, NodeKind, ObservedChangeSet, ObservedNode, Span, TaskId,
    };
    use prism_memory::{OutcomeEvent, OutcomeEvidence, OutcomeKind, OutcomeMemory, OutcomeResult};
    use prism_store::Graph;

    use super::Prism;

    #[test]
    fn finds_documents_by_file_stem_and_path_fragment() {
        let mut graph = Graph::new();
        graph.add_node(Node {
            id: NodeId::new("demo", "demo::document::docs::SPEC_md", NodeKind::Document),
            name: "docs/SPEC.md".into(),
            kind: NodeKind::Document,
            file: FileId(1),
            span: Span::whole_file(1),
            language: Language::Markdown,
        });
        graph.add_node(Node {
            id: NodeId::new(
                "demo",
                "demo::document::docs::SPEC_md::overview",
                NodeKind::MarkdownHeading,
            ),
            name: "Overview".into(),
            kind: NodeKind::MarkdownHeading,
            file: FileId(1),
            span: Span::line(1),
            language: Language::Markdown,
        });
        graph.add_node(Node {
            id: NodeId::new(
                "demo",
                "demo::document::docs::SPEC_md::spec_details",
                NodeKind::MarkdownHeading,
            ),
            name: "Spec Details".into(),
            kind: NodeKind::MarkdownHeading,
            file: FileId(1),
            span: Span::line(2),
            language: Language::Markdown,
        });
        graph.add_node(Node {
            id: NodeId::new(
                "demo",
                "demo::tests::search_respects_limit",
                NodeKind::Function,
            ),
            name: "search_respects_limit".into(),
            kind: NodeKind::Function,
            file: FileId(2),
            span: Span::line(1),
            language: Language::Rust,
        });

        let prism = Prism::new(graph);
        let symbol_matches = prism.symbol("SPEC");
        assert_eq!(symbol_matches.len(), 1);
        assert_eq!(symbol_matches[0].node().kind, NodeKind::Document);
        assert!(prism
            .symbol("docs/SPEC.md")
            .into_iter()
            .any(|symbol| symbol.node().kind == NodeKind::Document));
        assert!(prism
            .search("SPEC", 10, None, None)
            .into_iter()
            .any(|symbol| symbol.node().kind == NodeKind::MarkdownHeading));
        assert!(!prism
            .search("SPEC", 10, None, None)
            .into_iter()
            .any(|symbol| symbol.id().path == "demo::tests::search_respects_limit"));
    }

    #[test]
    fn prefers_exact_name_matches_before_fuzzy_matches() {
        let mut graph = Graph::new();
        graph.add_node(Node {
            id: NodeId::new("demo", "demo::alpha", NodeKind::Function),
            name: "alpha".into(),
            kind: NodeKind::Function,
            file: FileId(1),
            span: Span::line(1),
            language: Language::Rust,
        });
        graph.add_node(Node {
            id: NodeId::new(
                "demo",
                "demo::document::notes::alpha_md",
                NodeKind::Document,
            ),
            name: "notes/alpha.md".into(),
            kind: NodeKind::Document,
            file: FileId(2),
            span: Span::whole_file(1),
            language: Language::Markdown,
        });

        let prism = Prism::new(graph);
        let symbols = prism.symbol("alpha");

        assert_eq!(symbols[0].node().kind, NodeKind::Function);
    }

    #[test]
    fn search_respects_limit() {
        let mut graph = Graph::new();
        for index in 0..3 {
            graph.add_node(Node {
                id: NodeId::new(
                    "demo",
                    format!("demo::document::notes::alpha_{index}"),
                    NodeKind::Document,
                ),
                name: format!("notes/alpha-{index}.md").into(),
                kind: NodeKind::Document,
                file: FileId(index + 1),
                span: Span::whole_file(1),
                language: Language::Markdown,
            });
        }

        let prism = Prism::new(graph);
        assert_eq!(prism.search("alpha", 2, None, None).len(), 2);
    }

    #[test]
    fn search_can_filter_by_kind_and_path() {
        use std::path::Path;

        let mut graph = Graph::new();
        let spec_file = graph.ensure_file(Path::new("/workspace/docs/SPEC.md"));
        let source_file = graph.ensure_file(Path::new("/workspace/src/spec.rs"));

        graph.add_node(Node {
            id: NodeId::new("demo", "demo::document::docs::SPEC_md", NodeKind::Document),
            name: "docs/SPEC.md".into(),
            kind: NodeKind::Document,
            file: spec_file,
            span: Span::whole_file(1),
            language: Language::Markdown,
        });
        graph.add_node(Node {
            id: NodeId::new("demo", "demo::inspect_spec", NodeKind::Function),
            name: "inspect_spec".into(),
            kind: NodeKind::Function,
            file: source_file,
            span: Span::line(1),
            language: Language::Rust,
        });

        let prism = Prism::new(graph);

        let documents = prism.search("spec", 10, Some(NodeKind::Document), Some("docs/"));
        assert_eq!(documents.len(), 1);
        assert_eq!(documents[0].node().kind, NodeKind::Document);

        let functions = prism.search("spec", 10, Some(NodeKind::Function), Some("src/"));
        assert_eq!(functions.len(), 1);
        assert_eq!(functions[0].node().kind, NodeKind::Function);
    }

    #[test]
    fn exposes_lineage_queries_when_history_is_present() {
        let mut graph = Graph::new();
        let node_id = NodeId::new("demo", "demo::alpha", NodeKind::Function);
        graph.add_node(Node {
            id: node_id.clone(),
            name: "alpha".into(),
            kind: NodeKind::Function,
            file: FileId(1),
            span: Span::line(1),
            language: Language::Rust,
        });

        let mut history = HistoryStore::new();
        history.seed_nodes([node_id.clone()]);
        let prism = Prism::with_history(graph, history);

        let lineage = prism.lineage_of(&node_id).unwrap();
        assert!(prism.lineage_history(&lineage).is_empty());
    }

    #[test]
    fn outcome_queries_expand_node_to_lineage() {
        let mut graph = Graph::new();
        let old_id = NodeId::new("demo", "demo::alpha", NodeKind::Function);
        let new_id = NodeId::new("demo", "demo::renamed_alpha", NodeKind::Function);
        graph.add_node(Node {
            id: new_id.clone(),
            name: "renamed_alpha".into(),
            kind: NodeKind::Function,
            file: FileId(1),
            span: Span::line(1),
            language: Language::Rust,
        });

        let mut history = HistoryStore::new();
        history.seed_nodes([old_id.clone()]);
        let lineage = history.apply(&prism_ir::ObservedChangeSet {
            meta: EventMeta {
                id: EventId::new("observed:1"),
                ts: 1,
                actor: EventActor::System,
                correlation: None,
                causation: None,
            },
            trigger: prism_ir::ChangeTrigger::ManualReindex,
            files: vec![FileId(1)],
            added: vec![prism_ir::ObservedNode {
                node: Node {
                    id: new_id.clone(),
                    name: "renamed_alpha".into(),
                    kind: NodeKind::Function,
                    file: FileId(1),
                    span: Span::line(1),
                    language: Language::Rust,
                },
                fingerprint: prism_ir::SymbolFingerprint::with_parts(1, Some(2), Some(2), None),
            }],
            removed: vec![prism_ir::ObservedNode {
                node: Node {
                    id: old_id.clone(),
                    name: "alpha".into(),
                    kind: NodeKind::Function,
                    file: FileId(1),
                    span: Span::line(1),
                    language: Language::Rust,
                },
                fingerprint: prism_ir::SymbolFingerprint::with_parts(1, Some(2), Some(2), None),
            }],
            updated: Vec::new(),
            edge_added: Vec::new(),
            edge_removed: Vec::new(),
        })[0]
            .lineage
            .clone();

        let outcomes = OutcomeMemory::new();
        outcomes
            .store_event(OutcomeEvent {
                meta: EventMeta {
                    id: EventId::new("outcome:1"),
                    ts: 2,
                    actor: EventActor::Agent,
                    correlation: Some(TaskId::new("task:rename")),
                    causation: None,
                },
                anchors: vec![AnchorRef::Lineage(lineage)],
                kind: OutcomeKind::FailureObserved,
                result: OutcomeResult::Failure,
                summary: "rename caused a failure".into(),
                evidence: vec![OutcomeEvidence::Test {
                    name: "rename_flow".into(),
                    passed: false,
                }],
                metadata: serde_json::Value::Null,
            })
            .unwrap();

        let prism = Prism::with_history_and_outcomes(graph, history, outcomes);
        let failures = prism.related_failures(&new_id);
        assert_eq!(failures.len(), 1);
        assert!(failures[0].summary.contains("failure"));
    }

    #[test]
    fn blast_radius_includes_validations_and_neighbors() {
        let mut graph = Graph::new();
        let alpha = NodeId::new("demo", "demo::alpha", NodeKind::Function);
        let beta = NodeId::new("demo", "demo::beta", NodeKind::Function);
        graph.add_node(Node {
            id: alpha.clone(),
            name: "alpha".into(),
            kind: NodeKind::Function,
            file: FileId(1),
            span: Span::line(1),
            language: Language::Rust,
        });
        graph.add_node(Node {
            id: beta.clone(),
            name: "beta".into(),
            kind: NodeKind::Function,
            file: FileId(1),
            span: Span::line(2),
            language: Language::Rust,
        });
        graph.add_edge(Edge {
            kind: EdgeKind::Calls,
            source: alpha.clone(),
            target: beta.clone(),
            origin: prism_ir::EdgeOrigin::Static,
            confidence: 1.0,
        });

        let mut history = HistoryStore::new();
        history.seed_nodes([alpha.clone(), beta.clone()]);

        let outcomes = OutcomeMemory::new();
        outcomes
            .store_event(OutcomeEvent {
                meta: EventMeta {
                    id: EventId::new("outcome:2"),
                    ts: 2,
                    actor: EventActor::Agent,
                    correlation: Some(TaskId::new("task:beta")),
                    causation: None,
                },
                anchors: vec![AnchorRef::Node(alpha.clone())],
                kind: OutcomeKind::TestRan,
                result: OutcomeResult::Success,
                summary: "alpha requires unit test".into(),
                evidence: vec![OutcomeEvidence::Test {
                    name: "alpha_unit".into(),
                    passed: true,
                }],
                metadata: serde_json::Value::Null,
            })
            .unwrap();

        let prism = Prism::with_history_and_outcomes(graph, history, outcomes);
        let impact = prism.blast_radius(&alpha);
        assert!(impact.direct_nodes.contains(&beta));
        assert!(impact
            .likely_validations
            .iter()
            .any(|validation| validation == "test:alpha_unit"));
        assert!(impact
            .validation_checks
            .iter()
            .any(|check| check.label == "test:alpha_unit" && check.score > 0.0));
    }

    #[test]
    fn blast_radius_uses_co_change_history_and_neighbor_validations() {
        let mut graph = Graph::new();
        let alpha = NodeId::new("demo", "demo::alpha", NodeKind::Function);
        let beta = NodeId::new("demo", "demo::beta", NodeKind::Function);
        graph.add_node(Node {
            id: alpha.clone(),
            name: "alpha".into(),
            kind: NodeKind::Function,
            file: FileId(1),
            span: Span::line(1),
            language: Language::Rust,
        });
        graph.add_node(Node {
            id: beta.clone(),
            name: "beta".into(),
            kind: NodeKind::Function,
            file: FileId(1),
            span: Span::line(2),
            language: Language::Rust,
        });

        let mut history = HistoryStore::new();
        history.seed_nodes([alpha.clone(), beta.clone()]);
        history.apply(&ObservedChangeSet {
            meta: EventMeta {
                id: EventId::new("observed:cochange"),
                ts: 10,
                actor: EventActor::System,
                correlation: None,
                causation: None,
            },
            trigger: ChangeTrigger::ManualReindex,
            files: vec![FileId(1)],
            added: Vec::new(),
            removed: Vec::new(),
            updated: vec![
                (
                    ObservedNode {
                        node: Node {
                            id: alpha.clone(),
                            name: "alpha".into(),
                            kind: NodeKind::Function,
                            file: FileId(1),
                            span: Span::line(1),
                            language: Language::Rust,
                        },
                        fingerprint: prism_ir::SymbolFingerprint::with_parts(10, Some(20), None, None),
                    },
                    ObservedNode {
                        node: Node {
                            id: alpha.clone(),
                            name: "alpha".into(),
                            kind: NodeKind::Function,
                            file: FileId(1),
                            span: Span::line(1),
                            language: Language::Rust,
                        },
                        fingerprint: prism_ir::SymbolFingerprint::with_parts(10, Some(21), None, None),
                    },
                ),
                (
                    ObservedNode {
                        node: Node {
                            id: beta.clone(),
                            name: "beta".into(),
                            kind: NodeKind::Function,
                            file: FileId(1),
                            span: Span::line(2),
                            language: Language::Rust,
                        },
                        fingerprint: prism_ir::SymbolFingerprint::with_parts(11, Some(30), None, None),
                    },
                    ObservedNode {
                        node: Node {
                            id: beta.clone(),
                            name: "beta".into(),
                            kind: NodeKind::Function,
                            file: FileId(1),
                            span: Span::line(2),
                            language: Language::Rust,
                        },
                        fingerprint: prism_ir::SymbolFingerprint::with_parts(11, Some(31), None, None),
                    },
                ),
            ],
            edge_added: Vec::new(),
            edge_removed: Vec::new(),
        });

        let beta_lineage = history.lineage_of(&beta).unwrap();
        let outcomes = OutcomeMemory::new();
        outcomes
            .store_event(OutcomeEvent {
                meta: EventMeta {
                    id: EventId::new("outcome:cochange"),
                    ts: 11,
                    actor: EventActor::Agent,
                    correlation: Some(TaskId::new("task:beta")),
                    causation: None,
                },
                anchors: vec![AnchorRef::Lineage(beta_lineage)],
                kind: OutcomeKind::FailureObserved,
                result: OutcomeResult::Failure,
                summary: "beta changes usually need the integration test".into(),
                evidence: vec![OutcomeEvidence::Test {
                    name: "beta_integration".into(),
                    passed: false,
                }],
                metadata: serde_json::Value::Null,
            })
            .unwrap();

        let prism = Prism::with_history_and_outcomes(graph, history, outcomes);
        let impact = prism.blast_radius(&alpha);

        assert!(impact.direct_nodes.contains(&beta));
        assert!(impact
            .co_change_neighbors
            .iter()
            .any(|neighbor| neighbor.count == 1 && neighbor.nodes.contains(&beta)));
        assert!(impact
            .likely_validations
            .iter()
            .any(|validation| validation == "test:beta_integration"));
        assert!(impact
            .validation_checks
            .iter()
            .any(|check| check.label == "test:beta_integration" && check.score > 0.0));
        assert!(impact
            .risk_events
            .iter()
            .any(|event| event.summary.contains("integration test")));
    }

    #[test]
    fn validation_recipe_reuses_blast_radius_signal() {
        let mut graph = Graph::new();
        let alpha = NodeId::new("demo", "demo::alpha", NodeKind::Function);
        graph.add_node(Node {
            id: alpha.clone(),
            name: "alpha".into(),
            kind: NodeKind::Function,
            file: FileId(1),
            span: Span::line(1),
            language: Language::Rust,
        });

        let mut history = HistoryStore::new();
        history.seed_nodes([alpha.clone()]);

        let outcomes = OutcomeMemory::new();
        outcomes
            .store_event(OutcomeEvent {
                meta: EventMeta {
                    id: EventId::new("outcome:5"),
                    ts: 5,
                    actor: EventActor::Agent,
                    correlation: Some(TaskId::new("task:validate")),
                    causation: None,
                },
                anchors: vec![AnchorRef::Node(alpha.clone())],
                kind: OutcomeKind::FailureObserved,
                result: OutcomeResult::Failure,
                summary: "alpha broke an integration test".into(),
                evidence: vec![OutcomeEvidence::Test {
                    name: "alpha_integration".into(),
                    passed: false,
                }],
                metadata: serde_json::Value::Null,
            })
            .unwrap();

        let prism = Prism::with_history_and_outcomes(graph, history, outcomes);
        let recipe = prism.validation_recipe(&alpha);
        assert_eq!(recipe.target, alpha);
        assert_eq!(recipe.checks, vec!["test:alpha_integration"]);
        assert_eq!(recipe.scored_checks.len(), 1);
        assert_eq!(recipe.scored_checks[0].label, "test:alpha_integration");
        assert_eq!(recipe.recent_failures.len(), 1);
        assert_eq!(recipe.recent_failures[0].summary, "alpha broke an integration test");
    }

    #[test]
    fn resume_task_returns_correlated_events() {
        let graph = Graph::new();
        let history = HistoryStore::new();
        let outcomes = OutcomeMemory::new();
        let task = TaskId::new("task:fix");
        outcomes
            .store_event(OutcomeEvent {
                meta: EventMeta {
                    id: EventId::new("outcome:3"),
                    ts: 3,
                    actor: EventActor::Agent,
                    correlation: Some(task.clone()),
                    causation: None,
                },
                anchors: Vec::new(),
                kind: OutcomeKind::PatchApplied,
                result: OutcomeResult::Success,
                summary: "applied patch".into(),
                evidence: Vec::new(),
                metadata: serde_json::Value::Null,
            })
            .unwrap();
        outcomes
            .store_event(OutcomeEvent {
                meta: EventMeta {
                    id: EventId::new("outcome:4"),
                    ts: 4,
                    actor: EventActor::Agent,
                    correlation: Some(task.clone()),
                    causation: Some(EventId::new("outcome:3")),
                },
                anchors: Vec::new(),
                kind: OutcomeKind::FixValidated,
                result: OutcomeResult::Success,
                summary: "validated patch".into(),
                evidence: Vec::new(),
                metadata: serde_json::Value::Null,
            })
            .unwrap();

        let prism = Prism::with_history_and_outcomes(graph, history, outcomes);
        let replay = prism.resume_task(&task);
        assert_eq!(replay.events.len(), 2);
        assert_eq!(replay.events[0].summary, "validated patch");
    }
}
