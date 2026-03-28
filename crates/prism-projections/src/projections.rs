use std::collections::HashMap;

use prism_history::HistorySnapshot;
use prism_ir::{AnchorRef, LineageEvent, LineageId, NodeId};
use prism_memory::{OutcomeEvent, OutcomeMemorySnapshot};

use crate::common::{event_weight, validation_labels};
use crate::concepts::{
    concept_by_handle, curated_concepts_from_events, derive_concept_packets,
    hydrate_curated_concepts, merge_concept_packets, rank_concepts, resolve_curated_concepts,
};
use crate::types::{
    CoChangeDelta, CoChangeRecord, ConceptEvent, ConceptPacket, ConceptScope, ProjectionSnapshot,
    ValidationCheck, ValidationDelta,
};

pub const MAX_CO_CHANGE_NEIGHBORS_PER_LINEAGE: usize = 32;

#[derive(Debug, Clone, Default)]
pub struct ProjectionIndex {
    co_change_by_lineage: HashMap<LineageId, Vec<CoChangeRecord>>,
    validation_by_lineage: HashMap<LineageId, Vec<ValidationCheck>>,
    node_to_lineage: HashMap<NodeId, LineageId>,
    curated_concepts: Vec<ConceptPacket>,
    concept_packets: Vec<ConceptPacket>,
}

impl ProjectionIndex {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn from_snapshot(snapshot: ProjectionSnapshot) -> Self {
        let mut co_change_by_lineage = snapshot
            .co_change_by_lineage
            .into_iter()
            .collect::<HashMap<_, _>>();
        normalize_co_change_by_lineage(&mut co_change_by_lineage);
        let node_to_lineage = HashMap::new();
        let validation_by_lineage = snapshot.validation_by_lineage.into_iter().collect();
        let curated_concepts = snapshot.curated_concepts;
        let concept_packets = merge_concept_packets(
            derive_concept_packets(
                &node_to_lineage,
                &validation_by_lineage,
                &co_change_by_lineage,
            ),
            &resolve_curated_concepts(&curated_concepts, &node_to_lineage),
        );
        Self {
            co_change_by_lineage,
            validation_by_lineage,
            node_to_lineage,
            curated_concepts,
            concept_packets,
        }
    }

    pub fn derive(history: &HistorySnapshot, outcomes: &OutcomeMemorySnapshot) -> Self {
        Self::derive_with_curated(history, outcomes, Vec::new())
    }

    pub fn derive_with_curated(
        history: &HistorySnapshot,
        outcomes: &OutcomeMemorySnapshot,
        curated_concepts: Vec<ConceptPacket>,
    ) -> Self {
        let node_to_lineage = history
            .node_to_lineage
            .iter()
            .cloned()
            .collect::<HashMap<NodeId, LineageId>>();

        let mut co_change_by_lineage = HashMap::<LineageId, Vec<CoChangeRecord>>::new();
        for (left, right, count) in &history.co_change_counts {
            co_change_by_lineage
                .entry(left.clone())
                .or_default()
                .push(CoChangeRecord {
                    lineage: right.clone(),
                    count: *count,
                });
            co_change_by_lineage
                .entry(right.clone())
                .or_default()
                .push(CoChangeRecord {
                    lineage: left.clone(),
                    count: *count,
                });
        }
        normalize_co_change_by_lineage(&mut co_change_by_lineage);

        let mut validation_scores = HashMap::<LineageId, HashMap<String, (f32, u64)>>::new();
        for event in &outcomes.events {
            let lineages = event_lineages(event, &node_to_lineage);
            if lineages.is_empty() {
                continue;
            }

            let weight = event_weight(event);
            if weight <= 0.0 {
                continue;
            }

            let labels = validation_labels(&event.evidence);
            if labels.is_empty() {
                continue;
            }

            for lineage in lineages {
                let by_label = validation_scores.entry(lineage).or_default();
                for label in &labels {
                    let entry = by_label.entry(label.clone()).or_insert((0.0, 0));
                    entry.0 += weight;
                    entry.1 = entry.1.max(event.meta.ts);
                }
            }
        }

        let validation_by_lineage = validation_scores
            .into_iter()
            .map(|(lineage, by_label)| {
                let mut checks = by_label
                    .into_iter()
                    .map(|(label, (score, last_seen))| ValidationCheck {
                        label,
                        score,
                        last_seen,
                    })
                    .collect::<Vec<_>>();
                checks.sort_by(|left, right| {
                    right
                        .score
                        .total_cmp(&left.score)
                        .then_with(|| right.last_seen.cmp(&left.last_seen))
                        .then_with(|| left.label.cmp(&right.label))
                });
                (lineage, checks)
            })
            .collect();

        let curated_concepts =
            hydrate_curated_concepts(curated_concepts, &node_to_lineage, &history.events);
        let concept_packets = merge_concept_packets(
            derive_concept_packets(
                &node_to_lineage,
                &validation_by_lineage,
                &co_change_by_lineage,
            ),
            &curated_concepts,
        );

        Self {
            co_change_by_lineage,
            validation_by_lineage,
            node_to_lineage,
            curated_concepts,
            concept_packets,
        }
    }

    pub fn replace_curated_concepts(&mut self, curated_concepts: Vec<ConceptPacket>) {
        self.curated_concepts = curated_concepts;
        self.rebuild_concepts();
    }

    pub fn replace_curated_concepts_from_events(&mut self, events: &[ConceptEvent]) {
        self.replace_curated_concepts(curated_concepts_from_events(events));
    }

    pub fn upsert_curated_concept(&mut self, concept: ConceptPacket) {
        let normalized = concept.handle.to_ascii_lowercase();
        self.curated_concepts
            .retain(|candidate| candidate.handle.to_ascii_lowercase() != normalized);
        let retired = concept.publication.as_ref().is_some_and(|publication| {
            publication.status == crate::ConceptPublicationStatus::Retired
        });
        if !retired {
            self.curated_concepts.push(concept);
        }
        self.curated_concepts
            .sort_by(|left, right| left.handle.cmp(&right.handle));
        self.rebuild_concepts();
    }

    pub fn curated_concepts(&self) -> &[ConceptPacket] {
        &self.curated_concepts
    }

    pub fn reseed_from_history(&mut self, history: &HistorySnapshot) {
        self.node_to_lineage = history
            .node_to_lineage
            .iter()
            .cloned()
            .collect::<HashMap<_, _>>();
        self.curated_concepts = hydrate_curated_concepts(
            std::mem::take(&mut self.curated_concepts),
            &self.node_to_lineage,
            &history.events,
        );
        self.rebuild_concepts();
    }

    pub fn apply_lineage_events(&mut self, events: &[LineageEvent]) {
        self.apply_co_change_deltas(&co_change_deltas_for_events(events));
        for event in events {
            for before in &event.before {
                self.node_to_lineage.remove(before);
            }
            for after in &event.after {
                self.node_to_lineage
                    .insert(after.clone(), event.lineage.clone());
            }
        }
        self.rebuild_concepts();
    }

    pub fn apply_outcome_event<F>(&mut self, event: &OutcomeEvent, mut lineage_of: F)
    where
        F: FnMut(&NodeId) -> Option<LineageId>,
    {
        self.apply_validation_deltas(&validation_deltas_for_event(event, &mut lineage_of));
        self.rebuild_concepts();
    }

    pub fn apply_co_change_deltas(&mut self, deltas: &[CoChangeDelta]) {
        for delta in deltas {
            increment_co_change_neighbor(
                &mut self.co_change_by_lineage,
                &delta.source_lineage,
                &delta.target_lineage,
                delta.count_delta,
            );
        }
    }

    pub fn apply_validation_deltas(&mut self, deltas: &[ValidationDelta]) {
        for delta in deltas {
            increment_validation_check(
                &mut self.validation_by_lineage,
                &delta.lineage,
                &delta.label,
                delta.score_delta,
                delta.last_seen,
            );
        }
    }

    pub fn snapshot(&self) -> ProjectionSnapshot {
        let mut co_change_by_lineage = self
            .co_change_by_lineage
            .iter()
            .map(|(lineage, neighbors)| {
                let mut neighbors = neighbors.clone();
                normalize_co_change_neighbors(&mut neighbors);
                (lineage.clone(), neighbors)
            })
            .collect::<Vec<_>>();
        co_change_by_lineage.sort_by(|left, right| left.0 .0.cmp(&right.0 .0));

        let mut validation_by_lineage = self
            .validation_by_lineage
            .iter()
            .map(|(lineage, checks)| {
                let mut checks = checks.clone();
                checks.sort_by(|left, right| {
                    right
                        .score
                        .total_cmp(&left.score)
                        .then_with(|| right.last_seen.cmp(&left.last_seen))
                        .then_with(|| left.label.cmp(&right.label))
                });
                (lineage.clone(), checks)
            })
            .collect::<Vec<_>>();
        validation_by_lineage.sort_by(|left, right| left.0 .0.cmp(&right.0 .0));

        ProjectionSnapshot {
            co_change_by_lineage,
            validation_by_lineage,
            curated_concepts: self
                .curated_concepts
                .iter()
                .filter(|concept| concept.scope == ConceptScope::Session)
                .cloned()
                .collect(),
        }
    }

    pub fn co_change_neighbors(&self, lineage: &LineageId, limit: usize) -> Vec<CoChangeRecord> {
        let mut neighbors = self
            .co_change_by_lineage
            .get(lineage)
            .cloned()
            .unwrap_or_default();
        if limit > 0 {
            neighbors.truncate(limit);
        }
        neighbors
    }

    pub fn validation_checks_for_lineages(
        &self,
        lineages: &[LineageId],
        limit: usize,
    ) -> Vec<ValidationCheck> {
        let mut merged = HashMap::<String, (f32, u64)>::new();
        for lineage in lineages {
            let Some(checks) = self.validation_by_lineage.get(lineage) else {
                continue;
            };
            for check in checks {
                let entry = merged.entry(check.label.clone()).or_insert((0.0, 0));
                entry.0 += check.score;
                entry.1 = entry.1.max(check.last_seen);
            }
        }

        let mut checks = merged
            .into_iter()
            .map(|(label, (score, last_seen))| ValidationCheck {
                label,
                score,
                last_seen,
            })
            .collect::<Vec<_>>();
        checks.sort_by(|left, right| {
            right
                .score
                .total_cmp(&left.score)
                .then_with(|| right.last_seen.cmp(&left.last_seen))
                .then_with(|| left.label.cmp(&right.label))
        });
        if limit > 0 {
            checks.truncate(limit);
        }
        checks
    }

    pub fn concepts(&self, query: &str, limit: usize) -> Vec<ConceptPacket> {
        rank_concepts(&self.concept_packets, query, limit)
    }

    pub fn concept_by_handle(&self, handle: &str) -> Option<ConceptPacket> {
        concept_by_handle(&self.concept_packets, handle)
    }

    pub fn concept_packets(&self) -> &[ConceptPacket] {
        &self.concept_packets
    }

    fn rebuild_concepts(&mut self) {
        self.concept_packets = merge_concept_packets(
            derive_concept_packets(
                &self.node_to_lineage,
                &self.validation_by_lineage,
                &self.co_change_by_lineage,
            ),
            &resolve_curated_concepts(&self.curated_concepts, &self.node_to_lineage),
        );
    }
}

pub fn co_change_deltas_for_events(events: &[LineageEvent]) -> Vec<CoChangeDelta> {
    let mut lineages = events
        .iter()
        .map(|event| event.lineage.clone())
        .collect::<Vec<_>>();
    lineages.sort_by(|left, right| left.0.cmp(&right.0));
    lineages.dedup();

    let mut deltas = Vec::new();
    for (index, left) in lineages.iter().enumerate() {
        for right in lineages.iter().skip(index + 1) {
            deltas.push(CoChangeDelta {
                source_lineage: left.clone(),
                target_lineage: right.clone(),
                count_delta: 1,
            });
            deltas.push(CoChangeDelta {
                source_lineage: right.clone(),
                target_lineage: left.clone(),
                count_delta: 1,
            });
        }
    }
    deltas
}

pub fn validation_deltas_for_event<F>(
    event: &OutcomeEvent,
    mut lineage_of: F,
) -> Vec<ValidationDelta>
where
    F: FnMut(&NodeId) -> Option<LineageId>,
{
    let lineages = outcome_lineages(event, &mut lineage_of);
    if lineages.is_empty() {
        return Vec::new();
    }

    let weight = event_weight(event);
    if weight <= 0.0 {
        return Vec::new();
    }

    let labels = validation_labels(&event.evidence);
    if labels.is_empty() {
        return Vec::new();
    }

    let mut deltas = Vec::new();
    for lineage in lineages {
        for label in &labels {
            deltas.push(ValidationDelta {
                lineage: lineage.clone(),
                label: label.clone(),
                score_delta: weight,
                last_seen: event.meta.ts,
            });
        }
    }
    deltas
}

fn event_lineages(
    event: &OutcomeEvent,
    node_to_lineage: &HashMap<NodeId, LineageId>,
) -> Vec<LineageId> {
    let mut lineages = event
        .anchors
        .iter()
        .filter_map(|anchor| match anchor {
            AnchorRef::Lineage(lineage) => Some(lineage.clone()),
            AnchorRef::Node(node) => node_to_lineage.get(node).cloned(),
            _ => None,
        })
        .collect::<Vec<_>>();
    lineages.sort_by(|left, right| left.0.cmp(&right.0));
    lineages.dedup();
    lineages
}

fn outcome_lineages<F>(event: &OutcomeEvent, lineage_of: &mut F) -> Vec<LineageId>
where
    F: FnMut(&NodeId) -> Option<LineageId>,
{
    let mut lineages = event
        .anchors
        .iter()
        .filter_map(|anchor| match anchor {
            AnchorRef::Lineage(lineage) => Some(lineage.clone()),
            AnchorRef::Node(node) => lineage_of(node),
            _ => None,
        })
        .collect::<Vec<_>>();
    lineages.sort_by(|left, right| left.0.cmp(&right.0));
    lineages.dedup();
    lineages
}

fn increment_co_change_neighbor(
    by_lineage: &mut HashMap<LineageId, Vec<CoChangeRecord>>,
    source: &LineageId,
    target: &LineageId,
    count_delta: u32,
) {
    let neighbors = by_lineage.entry(source.clone()).or_default();
    if let Some(existing) = neighbors
        .iter_mut()
        .find(|record| record.lineage == *target)
    {
        existing.count += count_delta;
    } else {
        neighbors.push(CoChangeRecord {
            lineage: target.clone(),
            count: count_delta,
        });
    }
    normalize_co_change_neighbors(neighbors);
}

fn normalize_co_change_by_lineage(by_lineage: &mut HashMap<LineageId, Vec<CoChangeRecord>>) {
    for neighbors in by_lineage.values_mut() {
        normalize_co_change_neighbors(neighbors);
    }
}

fn normalize_co_change_neighbors(neighbors: &mut Vec<CoChangeRecord>) {
    neighbors.sort_by(|left, right| {
        right
            .count
            .cmp(&left.count)
            .then_with(|| left.lineage.0.cmp(&right.lineage.0))
    });
    neighbors.dedup_by(|left, right| left.lineage == right.lineage);
    neighbors.truncate(MAX_CO_CHANGE_NEIGHBORS_PER_LINEAGE);
}

fn increment_validation_check(
    by_lineage: &mut HashMap<LineageId, Vec<ValidationCheck>>,
    lineage: &LineageId,
    label: &str,
    score: f32,
    last_seen: u64,
) {
    let checks = by_lineage.entry(lineage.clone()).or_default();
    if let Some(existing) = checks.iter_mut().find(|check| check.label == label) {
        existing.score += score;
        existing.last_seen = existing.last_seen.max(last_seen);
    } else {
        checks.push(ValidationCheck {
            label: label.to_string(),
            score,
            last_seen,
        });
    }
    checks.sort_by(|left, right| {
        right
            .score
            .total_cmp(&left.score)
            .then_with(|| right.last_seen.cmp(&left.last_seen))
            .then_with(|| left.label.cmp(&right.label))
    });
}
