use prism_ir::{Edge, NodeId};

pub trait Agent {
    fn infer_edges(&self, context: AgentContext) -> Vec<Edge>;
}

#[derive(Debug, Clone, Default)]
pub struct AgentContext {
    pub symbol: NodeId,
    pub known_edges: Vec<Edge>,
    pub unresolved_calls: Vec<String>,
}
