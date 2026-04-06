use std::collections::{HashMap, HashSet};

use prism_ir::{Edge, EdgeKind, Node, NodeId};

use crate::common::{is_intent_source_kind, push_unique};
use crate::types::{IntentDriftRecord, IntentSpecProjection};
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct IntentIndex {
    by_spec: HashMap<NodeId, IntentSpecProjection>,
    specs_by_target: HashMap<NodeId, Vec<NodeId>>,
}

impl IntentIndex {
    pub fn derive<'a, N, E>(nodes: N, edges: E) -> Self
    where
        N: IntoIterator<Item = &'a Node>,
        E: IntoIterator<Item = &'a Edge>,
    {
        let mut doc_like = nodes
            .into_iter()
            .filter(|node| is_intent_source_kind(node.kind))
            .map(|node| node.id.clone())
            .collect::<HashSet<_>>();
        let mut by_spec = HashMap::<NodeId, IntentSpecProjection>::new();
        let mut specs_by_target = HashMap::<NodeId, Vec<NodeId>>::new();

        for edge in edges {
            if !doc_like.contains(&edge.source) {
                continue;
            }
            let entry =
                by_spec
                    .entry(edge.source.clone())
                    .or_insert_with(|| IntentSpecProjection {
                        spec: edge.source.clone(),
                        ..IntentSpecProjection::default()
                    });
            match edge.kind {
                EdgeKind::Specifies => {
                    push_unique(&mut entry.implementations, edge.target.clone());
                    push_unique(
                        specs_by_target.entry(edge.target.clone()).or_default(),
                        edge.source.clone(),
                    );
                }
                EdgeKind::Validates => push_unique(&mut entry.validations, edge.target.clone()),
                EdgeKind::RelatedTo => push_unique(&mut entry.related, edge.target.clone()),
                _ => {}
            }
        }

        doc_like.retain(|node| by_spec.contains_key(node));
        let _ = doc_like;
        Self {
            by_spec,
            specs_by_target,
        }
    }

    pub fn specs_for(&self, node: &NodeId) -> Vec<NodeId> {
        self.specs_by_target.get(node).cloned().unwrap_or_default()
    }

    pub fn implementations_for(&self, spec: &NodeId) -> Vec<NodeId> {
        self.by_spec
            .get(spec)
            .map(|projection| projection.implementations.clone())
            .unwrap_or_default()
    }

    pub fn validations_for(&self, spec: &NodeId) -> Vec<NodeId> {
        self.by_spec
            .get(spec)
            .map(|projection| projection.validations.clone())
            .unwrap_or_default()
    }

    pub fn related_for(&self, spec: &NodeId) -> Vec<NodeId> {
        self.by_spec
            .get(spec)
            .map(|projection| projection.related.clone())
            .unwrap_or_default()
    }

    pub fn drift_candidates(&self, specs: &[NodeId], limit: usize) -> Vec<IntentDriftRecord> {
        let mut candidates = specs
            .iter()
            .filter_map(|spec| {
                let projection = self.by_spec.get(spec)?;
                let mut reasons = Vec::new();
                if projection.implementations.is_empty() {
                    reasons.push("no implementation links".to_string());
                }
                if !projection.implementations.is_empty() && projection.validations.is_empty() {
                    reasons.push("no validation links".to_string());
                }
                if projection.implementations.is_empty() && !projection.related.is_empty() {
                    reasons.push("only related references were resolved".to_string());
                }
                (!reasons.is_empty()).then(|| IntentDriftRecord {
                    spec: projection.spec.clone(),
                    implementations: projection.implementations.clone(),
                    validations: projection.validations.clone(),
                    related: projection.related.clone(),
                    reasons,
                })
            })
            .collect::<Vec<_>>();
        candidates.sort_by(|left, right| {
            right
                .reasons
                .len()
                .cmp(&left.reasons.len())
                .then_with(|| left.spec.path.cmp(&right.spec.path))
        });
        if limit > 0 {
            candidates.truncate(limit);
        }
        candidates
    }

    pub fn known_specs(&self) -> Vec<NodeId> {
        let mut specs = self.by_spec.keys().cloned().collect::<Vec<_>>();
        specs.sort_by(|left, right| left.path.cmp(&right.path));
        specs
    }

    pub fn remove_spec_projection(&mut self, spec: &NodeId) {
        let Some(previous) = self.by_spec.remove(spec) else {
            return;
        };
        for target in previous.implementations {
            let remove_target = if let Some(specs) = self.specs_by_target.get_mut(&target) {
                specs.retain(|candidate| candidate != spec);
                specs.is_empty()
            } else {
                false
            };
            if remove_target {
                self.specs_by_target.remove(&target);
            }
        }
    }

    pub fn replace_spec_projection(
        &mut self,
        spec: NodeId,
        implementations: Vec<NodeId>,
        validations: Vec<NodeId>,
        related: Vec<NodeId>,
    ) {
        self.remove_spec_projection(&spec);
        if implementations.is_empty() && validations.is_empty() && related.is_empty() {
            return;
        }
        let projection = IntentSpecProjection {
            spec: spec.clone(),
            implementations: implementations.clone(),
            validations,
            related,
        };
        for target in implementations {
            push_unique(
                self.specs_by_target.entry(target).or_default(),
                spec.clone(),
            );
        }
        self.by_spec.insert(spec, projection);
    }
}
