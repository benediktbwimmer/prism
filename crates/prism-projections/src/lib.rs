use std::collections::{HashMap, HashSet};

use prism_history::HistorySnapshot;
use prism_ir::{AnchorRef, LineageEvent, LineageId, NodeId};
use prism_memory::{
    OutcomeEvent, OutcomeEvidence, OutcomeKind, OutcomeMemorySnapshot, OutcomeResult,
};
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct ValidationCheck {
    pub label: String,
    pub score: f32,
    pub last_seen: u64,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct CoChangeRecord {
    pub lineage: LineageId,
    pub count: u32,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct CoChangeDelta {
    pub source_lineage: LineageId,
    pub target_lineage: LineageId,
    pub count_delta: u32,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ValidationDelta {
    pub lineage: LineageId,
    pub label: String,
    pub score_delta: f32,
    pub last_seen: u64,
}

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct ProjectionSnapshot {
    pub co_change_by_lineage: Vec<(LineageId, Vec<CoChangeRecord>)>,
    pub validation_by_lineage: Vec<(LineageId, Vec<ValidationCheck>)>,
}

#[derive(Debug, Clone, Default)]
pub struct ProjectionIndex {
    co_change_by_lineage: HashMap<LineageId, Vec<CoChangeRecord>>,
    validation_by_lineage: HashMap<LineageId, Vec<ValidationCheck>>,
}

impl ProjectionIndex {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn from_snapshot(snapshot: ProjectionSnapshot) -> Self {
        Self {
            co_change_by_lineage: snapshot.co_change_by_lineage.into_iter().collect(),
            validation_by_lineage: snapshot.validation_by_lineage.into_iter().collect(),
        }
    }

    pub fn derive(history: &HistorySnapshot, outcomes: &OutcomeMemorySnapshot) -> Self {
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
        for neighbors in co_change_by_lineage.values_mut() {
            neighbors.sort_by(|left, right| {
                right
                    .count
                    .cmp(&left.count)
                    .then_with(|| left.lineage.0.cmp(&right.lineage.0))
            });
            neighbors.dedup_by(|left, right| left.lineage == right.lineage);
        }

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

        Self {
            co_change_by_lineage,
            validation_by_lineage,
        }
    }

    pub fn apply_lineage_events(&mut self, events: &[LineageEvent]) {
        self.apply_co_change_deltas(&co_change_deltas_for_events(events));
    }

    pub fn apply_outcome_event<F>(&mut self, event: &OutcomeEvent, mut lineage_of: F)
    where
        F: FnMut(&NodeId) -> Option<LineageId>,
    {
        self.apply_validation_deltas(&validation_deltas_for_event(event, &mut lineage_of));
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
                neighbors.sort_by(|left, right| {
                    right
                        .count
                        .cmp(&left.count)
                        .then_with(|| left.lineage.0.cmp(&right.lineage.0))
                });
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
    neighbors.sort_by(|left, right| {
        right
            .count
            .cmp(&left.count)
            .then_with(|| left.lineage.0.cmp(&right.lineage.0))
    });
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

fn event_weight(event: &OutcomeEvent) -> f32 {
    let kind_weight = match event.kind {
        OutcomeKind::FailureObserved | OutcomeKind::RegressionObserved => 2.5,
        OutcomeKind::FixValidated => 2.0,
        OutcomeKind::BuildRan | OutcomeKind::TestRan => 1.25,
        _ => 1.0,
    };
    let result_weight = match event.result {
        OutcomeResult::Failure => 2.0,
        OutcomeResult::Success => 1.0,
        OutcomeResult::Partial => 0.75,
        OutcomeResult::Unknown => 0.5,
    };
    kind_weight * result_weight
}

fn validation_labels(evidence: &[OutcomeEvidence]) -> Vec<String> {
    let mut labels = evidence
        .iter()
        .filter_map(|evidence| match evidence {
            OutcomeEvidence::Test { name, .. } => Some(format!("test:{name}")),
            OutcomeEvidence::Build { target, .. } => Some(format!("build:{target}")),
            _ => None,
        })
        .collect::<Vec<_>>();
    let mut deduped = HashSet::new();
    labels.retain(|label| deduped.insert(label.clone()));
    labels
}

#[cfg(test)]
mod tests {
    use prism_history::HistorySnapshot;
    use prism_ir::{
        AnchorRef, EventActor, EventId, EventMeta, LineageEvent, LineageEventKind, LineageId,
        NodeId, NodeKind, TaskId,
    };
    use prism_memory::{
        OutcomeEvent, OutcomeEvidence, OutcomeKind, OutcomeMemorySnapshot, OutcomeResult,
    };

    use super::*;

    #[test]
    fn derives_validation_and_co_change_indexes() {
        let alpha = NodeId::new("demo", "demo::alpha", NodeKind::Function);
        let beta = NodeId::new("demo", "demo::beta", NodeKind::Function);
        let alpha_lineage = LineageId::new("lineage:1");
        let beta_lineage = LineageId::new("lineage:2");
        let history = HistorySnapshot {
            node_to_lineage: vec![
                (alpha.clone(), alpha_lineage.clone()),
                (beta.clone(), beta_lineage.clone()),
            ],
            events: Vec::new(),
            co_change_counts: vec![(alpha_lineage.clone(), beta_lineage.clone(), 3)],
            next_lineage: 2,
            next_event: 0,
        };
        let outcomes = OutcomeMemorySnapshot {
            events: vec![OutcomeEvent {
                meta: EventMeta {
                    id: EventId::new("outcome:1"),
                    ts: 10,
                    actor: EventActor::Agent,
                    correlation: Some(TaskId::new("task:1")),
                    causation: None,
                },
                anchors: vec![AnchorRef::Node(alpha)],
                kind: OutcomeKind::FailureObserved,
                result: OutcomeResult::Failure,
                summary: "alpha failed".into(),
                evidence: vec![OutcomeEvidence::Test {
                    name: "alpha_integration".into(),
                    passed: false,
                }],
                metadata: serde_json::Value::Null,
            }],
        };

        let index = ProjectionIndex::derive(&history, &outcomes);
        let checks = index.validation_checks_for_lineages(&[alpha_lineage.clone()], 10);
        assert_eq!(checks[0].label, "test:alpha_integration");
        assert_eq!(checks[0].last_seen, 10);

        let neighbors = index.co_change_neighbors(&alpha_lineage, 10);
        assert_eq!(neighbors[0].lineage, beta_lineage);
        assert_eq!(neighbors[0].count, 3);
    }

    #[test]
    fn incremental_updates_match_derived_index() {
        let alpha = NodeId::new("demo", "demo::alpha", NodeKind::Function);
        let beta = NodeId::new("demo", "demo::beta", NodeKind::Function);
        let alpha_lineage = LineageId::new("lineage:1");
        let beta_lineage = LineageId::new("lineage:2");
        let history = HistorySnapshot {
            node_to_lineage: vec![
                (alpha.clone(), alpha_lineage.clone()),
                (beta.clone(), beta_lineage.clone()),
            ],
            events: Vec::new(),
            co_change_counts: vec![(alpha_lineage.clone(), beta_lineage.clone(), 1)],
            next_lineage: 2,
            next_event: 0,
        };
        let event = OutcomeEvent {
            meta: EventMeta {
                id: EventId::new("outcome:2"),
                ts: 11,
                actor: EventActor::Agent,
                correlation: Some(TaskId::new("task:2")),
                causation: None,
            },
            anchors: vec![AnchorRef::Node(alpha.clone())],
            kind: OutcomeKind::FailureObserved,
            result: OutcomeResult::Failure,
            summary: "alpha failed".into(),
            evidence: vec![OutcomeEvidence::Test {
                name: "alpha_unit".into(),
                passed: false,
            }],
            metadata: serde_json::Value::Null,
        };
        let derived = ProjectionIndex::derive(
            &history,
            &OutcomeMemorySnapshot {
                events: vec![event.clone()],
            },
        );

        let mut incremental = ProjectionIndex::new();
        incremental.apply_lineage_events(&[
            LineageEvent {
                meta: EventMeta {
                    id: EventId::new("lineage:1"),
                    ts: 10,
                    actor: EventActor::System,
                    correlation: None,
                    causation: None,
                },
                lineage: alpha_lineage.clone(),
                kind: LineageEventKind::Updated,
                before: vec![alpha.clone()],
                after: vec![alpha.clone()],
                confidence: 1.0,
                evidence: Vec::new(),
            },
            LineageEvent {
                meta: EventMeta {
                    id: EventId::new("lineage:2"),
                    ts: 10,
                    actor: EventActor::System,
                    correlation: None,
                    causation: None,
                },
                lineage: beta_lineage.clone(),
                kind: LineageEventKind::Updated,
                before: vec![beta.clone()],
                after: vec![beta.clone()],
                confidence: 1.0,
                evidence: Vec::new(),
            },
        ]);
        incremental.apply_outcome_event(&event, |node| {
            if node == &alpha {
                Some(alpha_lineage.clone())
            } else if node == &beta {
                Some(beta_lineage.clone())
            } else {
                None
            }
        });

        assert_eq!(incremental.snapshot(), derived.snapshot());
    }
}
