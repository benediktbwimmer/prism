use std::collections::{HashMap, HashSet};
use std::sync::RwLock;

use prism_ir::{Edge, EdgeKind, EdgeOrigin, NodeId, TaskId};
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct EdgeId(pub String);

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum InferredEdgeScope {
    SessionOnly,
    Persisted,
    Rejected,
    Expired,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct InferredEdgeRecord {
    pub id: EdgeId,
    pub edge: Edge,
    pub scope: InferredEdgeScope,
    pub task: Option<TaskId>,
    pub evidence: Vec<String>,
}

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct InferenceSnapshot {
    pub records: Vec<InferredEdgeRecord>,
}

pub trait Agent {
    fn infer_edges(&self, context: AgentContext) -> Vec<Edge>;
}

#[derive(Debug, Clone, Default)]
pub struct AgentContext {
    pub symbol: NodeId,
    pub known_edges: Vec<Edge>,
    pub unresolved_calls: Vec<String>,
    pub task: Option<TaskId>,
}

#[derive(Default)]
pub struct InferenceStore {
    state: RwLock<InferenceState>,
}

#[derive(Default)]
struct InferenceState {
    next_edge: u64,
    records: HashMap<EdgeId, InferredEdgeRecord>,
    outgoing: HashMap<NodeId, HashSet<EdgeId>>,
    incoming: HashMap<NodeId, HashSet<EdgeId>>,
}

impl InferenceStore {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn from_snapshot(snapshot: InferenceSnapshot) -> Self {
        let store = Self::new();
        let mut state = store.state.write().expect("inference store lock poisoned");
        for record in snapshot.records {
            insert_record(&mut state, record);
        }
        drop(state);
        store
    }

    pub fn store_edge(
        &self,
        mut edge: Edge,
        scope: InferredEdgeScope,
        task: Option<TaskId>,
        evidence: Vec<String>,
    ) -> EdgeId {
        edge.origin = EdgeOrigin::Inferred;
        let mut state = self.state.write().expect("inference store lock poisoned");
        state.next_edge += 1;
        let id = EdgeId(format!("edge:{}", state.next_edge));
        let record = InferredEdgeRecord {
            id: id.clone(),
            edge: edge.clone(),
            scope,
            task,
            evidence,
        };
        state
            .outgoing
            .entry(edge.source.clone())
            .or_default()
            .insert(id.clone());
        state
            .incoming
            .entry(edge.target.clone())
            .or_default()
            .insert(id.clone());
        state.records.insert(id.clone(), record);
        id
    }

    pub fn edges_from(&self, source: &NodeId, kind: Option<EdgeKind>) -> Vec<InferredEdgeRecord> {
        let state = self.state.read().expect("inference store lock poisoned");
        state
            .outgoing
            .get(source)
            .into_iter()
            .flat_map(|ids| ids.iter())
            .filter_map(|id| state.records.get(id).cloned())
            .filter(|record| {
                !matches!(
                    record.scope,
                    InferredEdgeScope::Rejected | InferredEdgeScope::Expired
                ) && kind.map_or(true, |kind| record.edge.kind == kind)
            })
            .collect()
    }

    pub fn edges_to(&self, target: &NodeId, kind: Option<EdgeKind>) -> Vec<InferredEdgeRecord> {
        let state = self.state.read().expect("inference store lock poisoned");
        state
            .incoming
            .get(target)
            .into_iter()
            .flat_map(|ids| ids.iter())
            .filter_map(|id| state.records.get(id).cloned())
            .filter(|record| {
                !matches!(
                    record.scope,
                    InferredEdgeScope::Rejected | InferredEdgeScope::Expired
                ) && kind.map_or(true, |kind| record.edge.kind == kind)
            })
            .collect()
    }

    pub fn all_edges(&self) -> Vec<InferredEdgeRecord> {
        let state = self.state.read().expect("inference store lock poisoned");
        state
            .records
            .values()
            .filter(|record| {
                !matches!(
                    record.scope,
                    InferredEdgeScope::Rejected | InferredEdgeScope::Expired
                )
            })
            .cloned()
            .collect()
    }

    pub fn snapshot_persisted(&self) -> InferenceSnapshot {
        let state = self.state.read().expect("inference store lock poisoned");
        let mut records = state
            .records
            .values()
            .filter(|record| record.scope != InferredEdgeScope::SessionOnly)
            .cloned()
            .collect::<Vec<_>>();
        records.sort_by(|left, right| left.id.0.cmp(&right.id.0));
        InferenceSnapshot { records }
    }
}

fn insert_record(state: &mut InferenceState, mut record: InferredEdgeRecord) {
    record.edge.origin = EdgeOrigin::Inferred;
    state.next_edge = state
        .next_edge
        .max(edge_sequence(&record.id).unwrap_or(state.next_edge));
    state
        .outgoing
        .entry(record.edge.source.clone())
        .or_default()
        .insert(record.id.clone());
    state
        .incoming
        .entry(record.edge.target.clone())
        .or_default()
        .insert(record.id.clone());
    state.records.insert(record.id.clone(), record);
}

fn edge_sequence(id: &EdgeId) -> Option<u64> {
    id.0.strip_prefix("edge:")?.parse().ok()
}

#[cfg(test)]
mod tests {
    use super::*;
    use prism_ir::{EdgeKind, EdgeOrigin, NodeKind};

    fn node(name: &str) -> NodeId {
        NodeId::new("demo", format!("demo::{name}"), NodeKind::Function)
    }

    #[test]
    fn stores_and_indexes_session_inferred_edges() {
        let store = InferenceStore::new();
        let alpha = node("alpha");
        let beta = node("beta");

        let id = store.store_edge(
            Edge {
                kind: EdgeKind::Calls,
                source: alpha.clone(),
                target: beta.clone(),
                origin: EdgeOrigin::Static,
                confidence: 0.7,
            },
            InferredEdgeScope::SessionOnly,
            Some(TaskId::new("task:demo")),
            vec!["resolved from task context".to_string()],
        );

        let outgoing = store.edges_from(&alpha, Some(EdgeKind::Calls));
        let incoming = store.edges_to(&beta, Some(EdgeKind::Calls));

        assert_eq!(outgoing.len(), 1);
        assert_eq!(incoming.len(), 1);
        assert_eq!(outgoing[0].id, id);
        assert_eq!(outgoing[0].edge.origin, EdgeOrigin::Inferred);
        assert_eq!(outgoing[0].task, Some(TaskId::new("task:demo")));
    }

    #[test]
    fn snapshot_round_trip_keeps_only_persisted_records() {
        let store = InferenceStore::new();
        let alpha = node("alpha");
        let beta = node("beta");
        let gamma = node("gamma");

        store.store_edge(
            Edge {
                kind: EdgeKind::Calls,
                source: alpha.clone(),
                target: beta.clone(),
                origin: EdgeOrigin::Static,
                confidence: 0.7,
            },
            InferredEdgeScope::SessionOnly,
            None,
            Vec::new(),
        );
        store.store_edge(
            Edge {
                kind: EdgeKind::Calls,
                source: alpha.clone(),
                target: gamma.clone(),
                origin: EdgeOrigin::Static,
                confidence: 0.9,
            },
            InferredEdgeScope::Persisted,
            Some(TaskId::new("task:persist")),
            vec!["confirmed".to_string()],
        );

        let restored = InferenceStore::from_snapshot(store.snapshot_persisted());
        let restored_edges = restored.edges_from(&alpha, Some(EdgeKind::Calls));

        assert_eq!(restored_edges.len(), 1);
        assert_eq!(restored_edges[0].edge.target, gamma);
        assert_eq!(restored_edges[0].scope, InferredEdgeScope::Persisted);
    }
}
