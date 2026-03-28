use std::collections::{HashMap, HashSet};

use prism_ir::{
    EventId, EventMeta, LineageEvent, LineageId, NodeId, ObservedChangeSet, ObservedNode,
};

use crate::resolver::resolve_change_set;
use crate::snapshot::{
    CoChangeNeighbor, HistoryCoChangeDelta, HistoryPersistDelta, HistorySnapshot, LineageTombstone,
};

#[derive(Debug, Clone, Default)]
pub struct HistoryStore {
    pub(crate) node_to_lineage: HashMap<NodeId, LineageId>,
    pub(crate) lineage_to_nodes: HashMap<LineageId, Vec<NodeId>>,
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

    pub fn seed_nodes<I>(&mut self, nodes: I) -> Vec<(NodeId, LineageId)>
    where
        I: IntoIterator<Item = NodeId>,
    {
        let mut seeded = Vec::new();
        for node in nodes {
            if !self.node_to_lineage.contains_key(&node) {
                let lineage = self.alloc_lineage();
                self.assign_node_lineage(node.clone(), lineage.clone());
                seeded.push((node, lineage));
            }
        }
        seeded
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
        self.lineage_to_nodes
            .get(lineage)
            .cloned()
            .unwrap_or_default()
    }

    pub fn snapshot(&self) -> HistorySnapshot {
        self.snapshot_with_co_change_counts(true)
    }

    pub fn snapshot_without_co_change_counts(&self) -> HistorySnapshot {
        self.snapshot_with_co_change_counts(false)
    }

    fn snapshot_with_co_change_counts(&self, include_co_change_counts: bool) -> HistorySnapshot {
        HistorySnapshot {
            node_to_lineage: self
                .node_to_lineage
                .iter()
                .map(|(node, lineage)| (node.clone(), lineage.clone()))
                .collect(),
            events: self.events.clone(),
            co_change_counts: include_co_change_counts
                .then(|| {
                    self.co_change_counts
                        .iter()
                        .map(|((left, right), count)| (left.clone(), right.clone(), *count))
                        .collect()
                })
                .unwrap_or_default(),
            tombstones: self.tombstones.values().cloned().collect(),
            next_lineage: self.next_lineage,
            next_event: self.next_event,
        }
    }

    pub fn persistence_delta(
        &self,
        events: &[LineageEvent],
        seeded_node_lineages: &[(NodeId, LineageId)],
        co_change_deltas: &[HistoryCoChangeDelta],
    ) -> HistoryPersistDelta {
        let mut removed_nodes = HashSet::<NodeId>::new();
        let mut upserted_node_lineages = HashMap::<NodeId, LineageId>::new();
        let mut touched_lineages = HashSet::<LineageId>::new();

        for (node, lineage) in seeded_node_lineages {
            upserted_node_lineages.insert(node.clone(), lineage.clone());
        }

        for event in events {
            touched_lineages.insert(event.lineage.clone());
            for node in &event.before {
                removed_nodes.insert(node.clone());
            }
            for node in &event.after {
                if let Some(lineage) = self.node_to_lineage.get(node) {
                    upserted_node_lineages.insert(node.clone(), lineage.clone());
                }
            }
        }

        let mut removed_nodes = removed_nodes.into_iter().collect::<Vec<_>>();
        sort_node_ids(&mut removed_nodes);

        let mut upserted_node_lineages = upserted_node_lineages.into_iter().collect::<Vec<_>>();
        upserted_node_lineages.sort_by(|left, right| compare_node_ids(&left.0, &right.0));

        let mut upserted_tombstones = Vec::new();
        let mut removed_tombstone_lineages = Vec::new();
        let mut touched_lineages = touched_lineages.into_iter().collect::<Vec<_>>();
        touched_lineages.sort_by(|left, right| left.0.cmp(&right.0));
        for lineage in touched_lineages {
            if let Some(tombstone) = self.tombstones.get(&lineage) {
                upserted_tombstones.push(tombstone.clone());
            } else {
                removed_tombstone_lineages.push(lineage);
            }
        }

        HistoryPersistDelta {
            removed_nodes,
            upserted_node_lineages,
            appended_events: events.to_vec(),
            co_change_deltas: co_change_deltas.to_vec(),
            upserted_tombstones,
            removed_tombstone_lineages,
            next_lineage: self.next_lineage,
            next_event: self.next_event,
        }
    }

    pub fn from_snapshot(snapshot: HistorySnapshot) -> Self {
        let HistorySnapshot {
            node_to_lineage,
            events,
            co_change_counts,
            tombstones,
            next_lineage,
            next_event,
        } = snapshot;
        let node_to_lineage = node_to_lineage.into_iter().collect::<HashMap<_, _>>();
        Self {
            lineage_to_nodes: lineage_to_nodes_index(&node_to_lineage),
            node_to_lineage,
            events,
            co_change_counts: co_change_counts
                .into_iter()
                .map(|(left, right, count)| (normalize_lineage_pair(left, right), count))
                .collect(),
            tombstones: tombstones
                .into_iter()
                .map(|tombstone| (tombstone.lineage.clone(), tombstone))
                .collect(),
            next_lineage,
            next_event,
        }
    }

    pub(crate) fn assign_node_lineage(&mut self, node: NodeId, lineage: LineageId) {
        let previous = self.node_to_lineage.insert(node.clone(), lineage.clone());
        if previous.as_ref() == Some(&lineage) {
            ensure_lineage_node(&mut self.lineage_to_nodes, lineage, node);
            return;
        }

        if let Some(previous) = previous {
            remove_lineage_node(&mut self.lineage_to_nodes, &previous, &node);
        }
        ensure_lineage_node(&mut self.lineage_to_nodes, lineage, node);
    }

    pub(crate) fn remove_node_lineage(&mut self, node: &NodeId) -> Option<LineageId> {
        let lineage = self.node_to_lineage.remove(node)?;
        remove_lineage_node(&mut self.lineage_to_nodes, &lineage, node);
        Some(lineage)
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

fn lineage_to_nodes_index(
    node_to_lineage: &HashMap<NodeId, LineageId>,
) -> HashMap<LineageId, Vec<NodeId>> {
    let mut lineage_to_nodes = HashMap::<LineageId, Vec<NodeId>>::new();
    for (node, lineage) in node_to_lineage {
        ensure_lineage_node(&mut lineage_to_nodes, lineage.clone(), node.clone());
    }
    lineage_to_nodes
}

fn ensure_lineage_node(
    lineage_to_nodes: &mut HashMap<LineageId, Vec<NodeId>>,
    lineage: LineageId,
    node: NodeId,
) {
    let nodes = lineage_to_nodes.entry(lineage).or_default();
    if nodes.iter().any(|existing| existing == &node) {
        return;
    }
    nodes.push(node);
    sort_node_ids(nodes);
}

fn remove_lineage_node(
    lineage_to_nodes: &mut HashMap<LineageId, Vec<NodeId>>,
    lineage: &LineageId,
    node: &NodeId,
) {
    let remove_entry = match lineage_to_nodes.get_mut(lineage) {
        Some(nodes) => {
            nodes.retain(|existing| existing != node);
            nodes.is_empty()
        }
        None => false,
    };
    if remove_entry {
        lineage_to_nodes.remove(lineage);
    }
}

fn sort_node_ids(nodes: &mut [NodeId]) {
    nodes.sort_by(compare_node_ids);
}

fn compare_node_ids(left: &NodeId, right: &NodeId) -> std::cmp::Ordering {
    left.crate_name
        .cmp(&right.crate_name)
        .then_with(|| left.path.cmp(&right.path))
        .then_with(|| left.kind.to_string().cmp(&right.kind.to_string()))
}
