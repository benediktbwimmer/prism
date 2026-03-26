use prism_ir::{CoordinationTaskId, NodeId, NodeKind};

use crate::common::dedupe_node_ids;
use crate::types::{DriftCandidate, TaskIntent};
use crate::Prism;

impl Prism {
    pub fn spec_for(&self, node: &NodeId) -> Vec<NodeId> {
        self.intent
            .read()
            .expect("intent lock poisoned")
            .specs_for(node)
    }

    pub fn implementation_for(&self, spec: &NodeId) -> Vec<NodeId> {
        self.intent
            .read()
            .expect("intent lock poisoned")
            .implementations_for(spec)
    }

    pub fn drift_candidates(&self, limit: usize) -> Vec<DriftCandidate> {
        let specs = self
            .intent
            .read()
            .expect("intent lock poisoned")
            .known_specs();
        self.drift_candidates_for_specs(&specs, limit)
    }

    pub fn task_intent(&self, task_id: &CoordinationTaskId) -> Option<TaskIntent> {
        let task = self.coordination.task(task_id)?;
        let intent = self.intent.read().expect("intent lock poisoned");
        let task_nodes = self.resolve_anchor_nodes(&task.anchors);
        let mut specs = task_nodes
            .iter()
            .flat_map(|node| intent.specs_for(node))
            .collect::<Vec<_>>();
        specs.extend(
            task_nodes
                .iter()
                .filter(|node| is_intent_source(node))
                .cloned(),
        );
        let specs = dedupe_node_ids(specs);

        let mut implementations = Vec::new();
        let mut validations = Vec::new();
        let mut related = Vec::new();
        for spec in &specs {
            implementations.extend(intent.implementations_for(spec));
            validations.extend(intent.validations_for(spec));
            related.extend(intent.related_for(spec));
        }
        Some(TaskIntent {
            task_id: task_id.clone(),
            specs: specs.clone(),
            implementations: dedupe_node_ids(implementations),
            validations: dedupe_node_ids(validations),
            related: dedupe_node_ids(related),
            drift_candidates: self.drift_candidates_for_specs(&specs, 10),
        })
    }

    fn drift_candidates_for_specs(&self, specs: &[NodeId], limit: usize) -> Vec<DriftCandidate> {
        self.intent
            .read()
            .expect("intent lock poisoned")
            .drift_candidates(specs, limit)
            .into_iter()
            .map(|candidate| DriftCandidate {
                recent_failures: candidate
                    .implementations
                    .iter()
                    .flat_map(|node| self.related_failures(node))
                    .take(10)
                    .collect(),
                spec: candidate.spec,
                implementations: candidate.implementations,
                validations: candidate.validations,
                related: candidate.related,
                reasons: candidate.reasons,
            })
            .collect()
    }
}

fn is_intent_source(node: &NodeId) -> bool {
    matches!(
        node.kind,
        NodeKind::Document | NodeKind::MarkdownHeading | NodeKind::JsonKey | NodeKind::YamlKey
    )
}
