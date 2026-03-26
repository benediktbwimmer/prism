use std::collections::HashMap;

use prism_ir::{
    EventId, EventMeta, LineageEvent, LineageId, NodeId, ObservedChangeSet, ObservedNode,
};

use crate::resolver::resolve_change_set;
use crate::snapshot::{CoChangeNeighbor, HistorySnapshot, LineageTombstone};

#[derive(Debug, Clone, Default)]
pub struct HistoryStore {
    pub(crate) node_to_lineage: HashMap<NodeId, LineageId>,
    pub(crate) events: Vec<LineageEvent>,
    pub(crate) co_change_counts: HashMap<(LineageId, LineageId), u32>,
    pub(crate) tombstones: HashMap<LineageId, LineageTombstone>,
    pub(crate) next_lineage: u64,
    pub(crate) next_event: u64,
}

impl HistoryStore {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn apply(&mut self, change_set: &ObservedChangeSet) -> Vec<LineageEvent> {
        resolve_change_set(self, change_set)
    }

    pub fn apply_all(&mut self, change_sets: &[ObservedChangeSet]) -> Vec<LineageEvent> {
        let mut events = Vec::new();
        for change_set in change_sets {
            events.extend(self.apply(change_set));
        }
        events
    }

    pub fn seed_nodes<I>(&mut self, nodes: I)
    where
        I: IntoIterator<Item = NodeId>,
    {
        for node in nodes {
            if !self.node_to_lineage.contains_key(&node) {
                let lineage = self.alloc_lineage();
                self.node_to_lineage.insert(node, lineage);
            }
        }
    }

    pub fn lineage_of(&self, node: &NodeId) -> Option<LineageId> {
        self.node_to_lineage.get(node).cloned()
    }

    pub fn lineage_history(&self, lineage: &LineageId) -> Vec<LineageEvent> {
        self.events
            .iter()
            .filter(|event| &event.lineage == lineage)
            .cloned()
            .collect()
    }

    pub fn co_change_neighbors(&self, lineage: &LineageId, limit: usize) -> Vec<CoChangeNeighbor> {
        let mut neighbors = self
            .co_change_counts
            .iter()
            .filter_map(|((left, right), count)| {
                if left == lineage {
                    Some(CoChangeNeighbor {
                        lineage: right.clone(),
                        count: *count,
                    })
                } else if right == lineage {
                    Some(CoChangeNeighbor {
                        lineage: left.clone(),
                        count: *count,
                    })
                } else {
                    None
                }
            })
            .collect::<Vec<_>>();
        neighbors.sort_by(|left, right| {
            right
                .count
                .cmp(&left.count)
                .then_with(|| left.lineage.0.cmp(&right.lineage.0))
        });
        if limit > 0 {
            neighbors.truncate(limit);
        }
        neighbors
    }

    pub fn current_nodes_for_lineage(&self, lineage: &LineageId) -> Vec<NodeId> {
        let mut nodes = self
            .node_to_lineage
            .iter()
            .filter(|(_, mapped)| *mapped == lineage)
            .map(|(node, _)| node.clone())
            .collect::<Vec<_>>();
        nodes.sort_by(|left, right| {
            left.crate_name
                .cmp(&right.crate_name)
                .then_with(|| left.path.cmp(&right.path))
                .then_with(|| left.kind.to_string().cmp(&right.kind.to_string()))
        });
        nodes
    }

    pub fn snapshot(&self) -> HistorySnapshot {
        HistorySnapshot {
            node_to_lineage: self
                .node_to_lineage
                .iter()
                .map(|(node, lineage)| (node.clone(), lineage.clone()))
                .collect(),
            events: self.events.clone(),
            co_change_counts: self
                .co_change_counts
                .iter()
                .map(|((left, right), count)| (left.clone(), right.clone(), *count))
                .collect(),
            tombstones: self.tombstones.values().cloned().collect(),
            next_lineage: self.next_lineage,
            next_event: self.next_event,
        }
    }

    pub fn from_snapshot(snapshot: HistorySnapshot) -> Self {
        Self {
            node_to_lineage: snapshot.node_to_lineage.into_iter().collect(),
            events: snapshot.events,
            co_change_counts: snapshot
                .co_change_counts
                .into_iter()
                .map(|(left, right, count)| (normalize_lineage_pair(left, right), count))
                .collect(),
            tombstones: snapshot
                .tombstones
                .into_iter()
                .map(|tombstone| (tombstone.lineage.clone(), tombstone))
                .collect(),
            next_lineage: snapshot.next_lineage,
            next_event: snapshot.next_event,
        }
    }

    pub(crate) fn make_event(
        &mut self,
        change_set: &ObservedChangeSet,
        lineage: LineageId,
        kind: prism_ir::LineageEventKind,
        before: Vec<NodeId>,
        after: Vec<NodeId>,
        confidence: f32,
        evidence: Vec<prism_ir::LineageEvidence>,
    ) -> LineageEvent {
        self.next_event += 1;
        LineageEvent {
            meta: EventMeta {
                id: EventId::new(format!(
                    "{}:lineage:{}",
                    change_set.meta.id.0, self.next_event
                )),
                ts: change_set.meta.ts,
                actor: change_set.meta.actor.clone(),
                correlation: change_set.meta.correlation.clone(),
                causation: Some(change_set.meta.id.clone()),
            },
            lineage,
            kind,
            before,
            after,
            confidence,
            evidence,
        }
    }

    pub(crate) fn alloc_lineage(&mut self) -> LineageId {
        self.next_lineage += 1;
        LineageId::new(format!("lineage:{}", self.next_lineage))
    }

    pub(crate) fn record_co_changes(&mut self, events: &[LineageEvent]) {
        let mut lineages = events
            .iter()
            .map(|event| event.lineage.clone())
            .collect::<Vec<_>>();
        lineages.sort_by(|left, right| left.0.cmp(&right.0));
        lineages.dedup();

        for (index, left) in lineages.iter().enumerate() {
            for right in lineages.iter().skip(index + 1) {
                *self
                    .co_change_counts
                    .entry(normalize_lineage_pair(left.clone(), right.clone()))
                    .or_insert(0) += 1;
            }
        }
    }

    pub(crate) fn record_tombstone(&mut self, lineage: &LineageId, removed: &ObservedNode) {
        self.tombstones.insert(
            lineage.clone(),
            LineageTombstone {
                lineage: lineage.clone(),
                nodes: vec![removed.node.id.clone()],
                fingerprint: removed.fingerprint.clone(),
            },
        );
    }
}

pub(crate) fn normalize_lineage_pair(left: LineageId, right: LineageId) -> (LineageId, LineageId) {
    if left.0 <= right.0 {
        (left, right)
    } else {
        (right, left)
    }
}
