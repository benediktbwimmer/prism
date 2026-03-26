use std::collections::{HashSet, VecDeque};
use std::fs;
use std::path::Path;

use prism_ir::{Edge, EdgeKind, Node, NodeId, NodeKind, Skeleton, Subgraph};

use crate::source::{SourceDocument, SourceExcerpt, SourceExcerptOptions, SourceLocation};
use crate::Prism;

struct Match<'a> {
    score: u8,
    is_test: bool,
    path_len: usize,
    path: String,
    node: &'a Node,
}

pub struct Symbol<'a> {
    pub(crate) prism: &'a Prism,
    pub(crate) id: NodeId,
}

#[derive(Debug, Clone, Default)]
pub struct Relations {
    pub outgoing_calls: Vec<NodeId>,
    pub incoming_calls: Vec<NodeId>,
    pub outgoing_imports: Vec<NodeId>,
    pub incoming_imports: Vec<NodeId>,
    pub outgoing_implements: Vec<NodeId>,
    pub incoming_implements: Vec<NodeId>,
    pub outgoing_specifies: Vec<NodeId>,
    pub incoming_specifies: Vec<NodeId>,
    pub outgoing_validates: Vec<NodeId>,
    pub incoming_validates: Vec<NodeId>,
    pub outgoing_related: Vec<NodeId>,
    pub incoming_related: Vec<NodeId>,
}

impl Prism {
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
            outgoing_specifies: self.targets(EdgeKind::Specifies),
            incoming_specifies: self.sources(EdgeKind::Specifies),
            outgoing_validates: self.targets(EdgeKind::Validates),
            incoming_validates: self.sources(EdgeKind::Validates),
            outgoing_related: self.targets(EdgeKind::RelatedTo),
            incoming_related: self.sources(EdgeKind::RelatedTo),
        }
    }

    pub fn full(&self) -> String {
        let Some(source) = self.read_source() else {
            return String::new();
        };
        SourceDocument::new(&source)
            .span_text(self.node().span)
            .to_owned()
    }

    pub fn excerpt(&self, options: SourceExcerptOptions) -> Option<SourceExcerpt> {
        let source = self.read_source()?;
        Some(SourceDocument::new(&source).excerpt(self.node().span, options))
    }

    pub fn location(&self) -> Option<SourceLocation> {
        let source = self.read_source()?;
        Some(SourceDocument::new(&source).location(self.node().span))
    }

    fn read_source(&self) -> Option<String> {
        let node = self.node();
        let path = self.prism.graph.file_path(node.file)?;
        fs::read_to_string(path).ok()
    }

    pub fn call_graph(&self, depth: usize) -> Subgraph {
        let mut visited = HashSet::new();
        let mut nodes = Vec::new();
        let mut edges = Vec::<Edge>::new();
        let mut queue = VecDeque::from([(self.id.clone(), 0usize)]);
        let mut max_depth_reached: Option<usize> = None;

        while let Some((current, current_depth)) = queue.pop_front() {
            if !visited.insert(current.clone()) {
                continue;
            }
            nodes.push(current.clone());
            max_depth_reached =
                Some(max_depth_reached.map_or(current_depth, |max| max.max(current_depth)));

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
