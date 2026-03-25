use std::collections::{HashMap, HashSet};

use prism_ir::{
    EventId, EventMeta, LineageEvent, LineageEventKind, LineageEvidence, LineageId, NodeId,
    ObservedChangeSet, ObservedNode, SymbolFingerprint,
};
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct HistorySnapshot {
    pub node_to_lineage: Vec<(NodeId, LineageId)>,
    pub events: Vec<LineageEvent>,
    pub next_lineage: u64,
    pub next_event: u64,
}

#[derive(Debug, Clone, Default)]
pub struct HistoryStore {
    node_to_lineage: HashMap<NodeId, LineageId>,
    events: Vec<LineageEvent>,
    next_lineage: u64,
    next_event: u64,
}

impl HistoryStore {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn apply(&mut self, change_set: &ObservedChangeSet) -> Vec<LineageEvent> {
        let mut emitted = Vec::new();

        for (before, after) in &change_set.updated {
            let lineage = self
                .node_to_lineage
                .get(&before.node.id)
                .cloned()
                .unwrap_or_else(|| self.alloc_lineage());
            self.node_to_lineage
                .insert(after.node.id.clone(), lineage.clone());
            emitted.push(self.make_event(
                change_set,
                lineage,
                LineageEventKind::Updated,
                vec![before.node.id.clone()],
                vec![after.node.id.clone()],
                vec![LineageEvidence::ExactNodeId],
            ));
        }

        let matches = self.match_lineage_candidates(&change_set.removed, &change_set.added);
        let matched_removed = matches
            .iter()
            .map(|(removed_index, _, _, _, _)| *removed_index)
            .collect::<HashSet<_>>();
        let matched_added = matches
            .iter()
            .map(|(_, added_index, _, _, _)| *added_index)
            .collect::<HashSet<_>>();

        for (removed_index, added_index, kind, _confidence, evidence) in matches {
            let before = &change_set.removed[removed_index];
            let after = &change_set.added[added_index];
            let lineage = self
                .node_to_lineage
                .remove(&before.node.id)
                .unwrap_or_else(|| self.alloc_lineage());
            self.node_to_lineage
                .insert(after.node.id.clone(), lineage.clone());
            emitted.push(self.make_event(
                change_set,
                lineage,
                kind,
                vec![before.node.id.clone()],
                vec![after.node.id.clone()],
                evidence,
            ));
        }

        for (index, removed) in change_set.removed.iter().enumerate() {
            if matched_removed.contains(&index) {
                continue;
            }
            let lineage = self
                .node_to_lineage
                .remove(&removed.node.id)
                .unwrap_or_else(|| self.alloc_lineage());
            emitted.push(self.make_event(
                change_set,
                lineage,
                LineageEventKind::Died,
                vec![removed.node.id.clone()],
                Vec::new(),
                vec![LineageEvidence::FingerprintMatch],
            ));
        }

        for (index, added) in change_set.added.iter().enumerate() {
            if matched_added.contains(&index) {
                continue;
            }
            let lineage = self.alloc_lineage();
            self.node_to_lineage
                .insert(added.node.id.clone(), lineage.clone());
            emitted.push(self.make_event(
                change_set,
                lineage,
                LineageEventKind::Born,
                Vec::new(),
                vec![added.node.id.clone()],
                vec![LineageEvidence::FingerprintMatch],
            ));
        }

        self.events.extend(emitted.iter().cloned());
        emitted
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

    pub fn snapshot(&self) -> HistorySnapshot {
        HistorySnapshot {
            node_to_lineage: self
                .node_to_lineage
                .iter()
                .map(|(node, lineage)| (node.clone(), lineage.clone()))
                .collect(),
            events: self.events.clone(),
            next_lineage: self.next_lineage,
            next_event: self.next_event,
        }
    }

    pub fn from_snapshot(snapshot: HistorySnapshot) -> Self {
        Self {
            node_to_lineage: snapshot.node_to_lineage.into_iter().collect(),
            events: snapshot.events,
            next_lineage: snapshot.next_lineage,
            next_event: snapshot.next_event,
        }
    }

    fn match_lineage_candidates(
        &self,
        removed: &[ObservedNode],
        added: &[ObservedNode],
    ) -> Vec<(usize, usize, LineageEventKind, f32, Vec<LineageEvidence>)> {
        let mut candidates = Vec::new();

        for (removed_index, before) in removed.iter().enumerate() {
            for (added_index, after) in added.iter().enumerate() {
                let Some((score, evidence)) =
                    fingerprint_match(&before.fingerprint, &after.fingerprint)
                else {
                    continue;
                };
                if before.node.kind != after.node.kind {
                    continue;
                }
                candidates.push((
                    removed_index,
                    added_index,
                    classify_change(before, after),
                    score,
                    evidence,
                ));
            }
        }

        candidates.sort_by(|left, right| {
            right
                .3
                .total_cmp(&left.3)
                .then_with(|| left.0.cmp(&right.0))
                .then_with(|| left.1.cmp(&right.1))
        });

        let mut used_removed = HashSet::new();
        let mut used_added = HashSet::new();
        let mut matches = Vec::new();

        for candidate in candidates {
            if used_removed.contains(&candidate.0) || used_added.contains(&candidate.1) {
                continue;
            }
            used_removed.insert(candidate.0);
            used_added.insert(candidate.1);
            matches.push(candidate);
        }

        matches
    }

    fn make_event(
        &mut self,
        change_set: &ObservedChangeSet,
        lineage: LineageId,
        kind: LineageEventKind,
        before: Vec<NodeId>,
        after: Vec<NodeId>,
        evidence: Vec<LineageEvidence>,
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
            confidence: 1.0,
            evidence,
        }
    }

    fn alloc_lineage(&mut self) -> LineageId {
        self.next_lineage += 1;
        LineageId::new(format!("lineage:{}", self.next_lineage))
    }
}

fn classify_change(before: &ObservedNode, after: &ObservedNode) -> LineageEventKind {
    if before.node.file != after.node.file {
        LineageEventKind::Moved
    } else if last_path_segment(&before.node.id.path) != last_path_segment(&after.node.id.path) {
        LineageEventKind::Renamed
    } else if before.node.id.path != after.node.id.path {
        LineageEventKind::Reparented
    } else {
        LineageEventKind::Updated
    }
}

fn last_path_segment(path: &str) -> &str {
    path.rsplit("::").next().unwrap_or(path)
}

fn fingerprint_match(
    before: &SymbolFingerprint,
    after: &SymbolFingerprint,
) -> Option<(f32, Vec<LineageEvidence>)> {
    if before.signature_hash != after.signature_hash {
        return None;
    }

    let mut score = 0.4;
    let mut evidence = vec![LineageEvidence::SignatureMatch];

    if before.body_hash.is_some() && before.body_hash == after.body_hash {
        score += 0.3;
        evidence.push(LineageEvidence::BodyHashMatch);
    }
    if before.skeleton_hash.is_some() && before.skeleton_hash == after.skeleton_hash {
        score += 0.2;
        evidence.push(LineageEvidence::SkeletonMatch);
    }
    if before.child_shape_hash.is_some() && before.child_shape_hash == after.child_shape_hash {
        score += 0.1;
    }
    if before == after {
        evidence.insert(0, LineageEvidence::FingerprintMatch);
    }

    Some((score, evidence))
}

#[cfg(test)]
mod tests {
    use prism_ir::{
        ChangeTrigger, Edge, EdgeKind, EdgeOrigin, EventActor, FileId, Language, Node, NodeKind,
        Span,
    };

    use super::*;

    fn node(path: &str, file_id: u32) -> Node {
        Node {
            id: NodeId::new("demo", path, NodeKind::Function),
            name: last_path_segment(path).into(),
            kind: NodeKind::Function,
            file: FileId(file_id),
            span: Span::line(1),
            language: Language::Rust,
        }
    }

    fn observed(node: Node, signature: u64, body: u64) -> ObservedNode {
        ObservedNode {
            node,
            fingerprint: SymbolFingerprint::with_parts(signature, Some(body), Some(body), None),
        }
    }

    fn change_set(added: Vec<ObservedNode>, removed: Vec<ObservedNode>) -> ObservedChangeSet {
        ObservedChangeSet {
            meta: EventMeta {
                id: EventId::new("change:1"),
                ts: 1,
                actor: EventActor::System,
                correlation: None,
                causation: None,
            },
            trigger: ChangeTrigger::ManualReindex,
            files: vec![FileId(1)],
            added,
            removed,
            updated: Vec::new(),
            edge_added: vec![Edge {
                kind: EdgeKind::Contains,
                source: NodeId::new("demo", "demo", NodeKind::Module),
                target: NodeId::new("demo", "demo::new_name", NodeKind::Function),
                origin: EdgeOrigin::Static,
                confidence: 1.0,
            }],
            edge_removed: Vec::new(),
        }
    }

    #[test]
    fn matches_rename_by_fingerprint() {
        let mut history = HistoryStore::new();
        history.seed_nodes([NodeId::new("demo", "demo::old_name", NodeKind::Function)]);

        let events = history.apply(&change_set(
            vec![observed(node("demo::new_name", 1), 10, 20)],
            vec![observed(node("demo::old_name", 1), 10, 20)],
        ));

        assert_eq!(events.len(), 1);
        assert_eq!(events[0].kind, LineageEventKind::Renamed);
        let lineage = history
            .lineage_of(&NodeId::new("demo", "demo::new_name", NodeKind::Function))
            .unwrap();
        assert_eq!(history.lineage_history(&lineage).len(), 1);
    }

    #[test]
    fn allocates_born_events_for_new_symbols() {
        let mut history = HistoryStore::new();
        let events = history.apply(&change_set(
            vec![observed(node("demo::new_name", 1), 10, 20)],
            Vec::new(),
        ));

        assert_eq!(events.len(), 1);
        assert_eq!(events[0].kind, LineageEventKind::Born);
        assert!(history
            .lineage_of(&NodeId::new("demo", "demo::new_name", NodeKind::Function))
            .is_some());
    }
}
