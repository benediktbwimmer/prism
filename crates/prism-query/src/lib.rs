use std::collections::{HashSet, VecDeque};
use std::fs;
use std::sync::Arc;

use prism_ir::{Edge, EdgeKind, Node, NodeId, NodeKind, Skeleton, Subgraph};
use prism_store::Graph;

pub struct Prism {
    graph: Arc<Graph>,
}

impl Prism {
    pub fn new(graph: Graph) -> Self {
        Self {
            graph: Arc::new(graph),
        }
    }

    pub fn graph(&self) -> &Graph {
        self.graph.as_ref()
    }

    pub fn symbol(&self, name: &str) -> Vec<Symbol<'_>> {
        self.graph
            .nodes_by_name(name)
            .into_iter()
            .map(|node| Symbol {
                prism: self,
                id: node.id.clone(),
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

        let start = node.span.start_line.saturating_sub(1) as usize;
        let end = node.span.end_line.max(node.span.start_line) as usize;
        source
            .lines()
            .skip(start)
            .take(end.saturating_sub(start))
            .collect::<Vec<_>>()
            .join("\n")
    }

    pub fn call_graph(&self, depth: usize) -> Subgraph {
        let mut visited = HashSet::new();
        let mut nodes = Vec::new();
        let mut edges = Vec::<Edge>::new();
        let mut queue = VecDeque::from([(self.id.clone(), 0usize)]);

        while let Some((current, current_depth)) = queue.pop_front() {
            if !visited.insert(current.clone()) {
                continue;
            }
            nodes.push(current.clone());

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
