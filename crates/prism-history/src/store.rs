use std::collections::{HashMap, HashSet};

use prism_ir::{
    new_prefixed_id, new_sortable_token, EventId, EventMeta, LineageEvent, LineageId, NodeId,
    ObservedChangeSet, ObservedNode,
};

use crate::resolver::resolve_change_set;
use crate::snapshot::{HistoryPersistDelta, HistorySnapshot, LineageTombstone};

#[derive(Debug, Clone, Default)]
pub struct HistoryStore {
    pub(crate) node_to_lineage: HashMap<NodeId, LineageId>,
    pub(crate) lineage_to_nodes: HashMap<LineageId, Vec<NodeId>>,
    pub(crate) events: Vec<LineageEvent>,
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

    pub fn current_nodes_for_lineage(&self, lineage: &LineageId) -> Vec<NodeId> {
        self.lineage_to_nodes
            .get(lineage)
            .cloned()
            .unwrap_or_default()
    }

    pub fn snapshot(&self) -> HistorySnapshot {
        HistorySnapshot {
            node_to_lineage: self
                .node_to_lineage
                .iter()
                .map(|(node, lineage)| (node.clone(), lineage.clone()))
                .collect(),
            events: self.events.clone(),
            tombstones: self.tombstones.values().cloned().collect(),
            next_lineage: self.next_lineage,
            next_event: self.next_event,
        }
    }

    pub fn persistence_delta(
        &self,
        events: &[LineageEvent],
        seeded_node_lineages: &[(NodeId, LineageId)],
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
            upserted_tombstones,
            removed_tombstone_lineages,
            next_lineage: self.next_lineage,
            next_event: self.next_event,
        }
    }

    pub fn apply_persistence_delta(&mut self, delta: &HistoryPersistDelta) {
        for node in &delta.removed_nodes {
            self.remove_node_lineage(node);
        }
        for (node, lineage) in &delta.upserted_node_lineages {
            self.assign_node_lineage(node.clone(), lineage.clone());
        }
        self.events.extend(delta.appended_events.clone());
        for lineage in &delta.removed_tombstone_lineages {
            self.tombstones.remove(lineage);
        }
        for tombstone in &delta.upserted_tombstones {
            self.tombstones
                .insert(tombstone.lineage.clone(), tombstone.clone());
        }
        self.next_lineage = delta.next_lineage;
        self.next_event = delta.next_event;
    }

    pub fn from_snapshot(snapshot: HistorySnapshot) -> Self {
        let HistorySnapshot {
            node_to_lineage,
            events,
            tombstones,
            next_lineage,
            next_event,
        } = snapshot;
        let node_to_lineage = node_to_lineage.into_iter().collect::<HashMap<_, _>>();
        Self {
            lineage_to_nodes: lineage_to_nodes_index(&node_to_lineage),
            node_to_lineage,
            events,
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
        let evidence = sanitize_lineage_evidence(kind.clone(), evidence);
        LineageEvent {
            meta: EventMeta {
                id: EventId::new(format!(
                    "{}:lineage:{}",
                    change_set.meta.id.0,
                    new_sortable_token()
                )),
                ts: change_set.meta.ts,
                actor: change_set.meta.actor.clone(),
                correlation: change_set.meta.correlation.clone(),
                causation: Some(change_set.meta.id.clone()),
                execution_context: change_set.meta.execution_context.clone(),
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
        LineageId::new(new_prefixed_id("lineage"))
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

fn sanitize_lineage_evidence(
    kind: prism_ir::LineageEventKind,
    evidence: Vec<prism_ir::LineageEvidence>,
) -> Vec<prism_ir::LineageEvidence> {
    match kind {
        prism_ir::LineageEventKind::Born | prism_ir::LineageEventKind::Died => Vec::new(),
        _ => evidence,
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
