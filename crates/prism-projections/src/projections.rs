use std::collections::HashMap;

use prism_history::HistorySnapshot;
use prism_ir::{AnchorRef, LineageEvent, LineageId, NodeId};
use prism_memory::{OutcomeEvent, OutcomeMemorySnapshot};

use crate::common::{event_weight, validation_labels};
use crate::concepts::{
    concept_by_handle, curated_concepts_from_events, hydrate_curated_concepts,
    merge_concept_packets, rank_concepts, resolve_concepts, resolve_curated_concepts,
};
use crate::types::{
    CoChangeDelta, CoChangeRecord, ConceptEvent, ConceptHealth, ConceptHealthSignals,
    ConceptHealthStatus, ConceptPacket, ConceptResolution, ConceptScope, ProjectionSnapshot,
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
        let concept_packets = merge_concept_packets(&resolve_curated_concepts(
            &curated_concepts,
            &node_to_lineage,
        ));
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
        let concept_packets = merge_concept_packets(&curated_concepts);

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

    pub fn resolve_concepts(&self, query: &str, limit: usize) -> Vec<ConceptResolution> {
        resolve_concepts(&self.concept_packets, query, limit)
    }

    pub fn concept_by_handle(&self, handle: &str) -> Option<ConceptPacket> {
        concept_by_handle(&self.concept_packets, handle)
    }

    pub fn concept_health(&self, handle: &str) -> Option<ConceptHealth> {
        let resolved = concept_by_handle(&self.concept_packets, handle)?;
        let original =
            concept_by_handle(&self.curated_concepts, handle).unwrap_or_else(|| resolved.clone());
        Some(self.compute_concept_health(&original, &resolved))
    }

    pub fn concept_packets(&self) -> &[ConceptPacket] {
        &self.concept_packets
    }

    fn rebuild_concepts(&mut self) {
        self.concept_packets = merge_concept_packets(&resolve_curated_concepts(
            &self.curated_concepts,
            &self.node_to_lineage,
        ));
    }

    fn compute_concept_health(
        &self,
        original: &ConceptPacket,
        resolved: &ConceptPacket,
    ) -> ConceptHealth {
        let original_core_count = original.core_members.len().max(1);
        let original_slots = collect_member_slots(original);
        let slot_count = original_slots.len().max(1);
        let live_core_member_ratio = (resolved.core_members.len() as f32 / original_core_count as f32)
            .clamp(0.0, 1.0);

        let lineage_coverage_count = original_slots
            .iter()
            .filter(|slot| slot.lineage.is_some())
            .count();
        let lineage_coverage_ratio =
            (lineage_coverage_count as f32 / slot_count as f32).clamp(0.0, 1.0);

        let mut changed_slots = 0usize;
        let mut rebound_slots = 0usize;
        for slot in &original_slots {
            let current = resolve_member_for_health(&slot.member, slot.lineage.as_ref(), &self.node_to_lineage);
            if current.as_ref() != Some(&slot.member) {
                changed_slots += 1;
                if current.is_some() {
                    rebound_slots += 1;
                }
            }
        }
        let member_churn_ratio = (changed_slots as f32 / slot_count as f32).clamp(0.0, 1.0);
        let rebind_success_ratio = if changed_slots == 0 {
            1.0
        } else {
            (rebound_slots as f32 / changed_slots as f32).clamp(0.0, 1.0)
        };

        let concept_lineages = current_concept_lineages(resolved);
        let validated_lineages = concept_lineages
            .iter()
            .filter(|lineage| {
                self.validation_by_lineage
                    .get(*lineage)
                    .is_some_and(|checks| !checks.is_empty())
            })
            .count();
        let validation_coverage_ratio = if concept_lineages.is_empty() {
            0.0
        } else {
            (validated_lineages as f32 / concept_lineages.len() as f32).clamp(0.0, 1.0)
        };

        let ambiguity_ratio = self.concept_ambiguity_ratio(resolved);
        let stale_validation_links =
            !resolved.likely_tests.is_empty() && validation_coverage_ratio == 0.0;
        let stale_risk_hint = resolved.risk_hint.is_some()
            && (member_churn_ratio > 0.0 || live_core_member_ratio < 1.0 || ambiguity_ratio >= 0.7);
        let superseded_by = self.active_superseders(&resolved.handle);

        let mut reasons = Vec::new();
        if live_core_member_ratio < 1.0 {
            reasons.push(format!(
                "Only {} of {} core members still resolve cleanly.",
                resolved.core_members.len(),
                original.core_members.len()
            ));
        }
        if changed_slots > 0 {
            reasons.push(format!(
                "{} member binding(s) moved; {} rebounded through lineage.",
                changed_slots, rebound_slots
            ));
        }
        if ambiguity_ratio >= 0.6 {
            reasons.push("Concept retrieval is ambiguous against nearby concepts.".to_string());
        }
        if stale_validation_links {
            reasons.push(
                "Concept has likely tests but no validation checks on current member lineages."
                    .to_string(),
            );
        }
        if stale_risk_hint {
            reasons.push("Risk hint may be stale relative to the current concept bindings.".to_string());
        }
        if !superseded_by.is_empty() {
            reasons.push(format!(
                "Superseded by active concept(s): {}.",
                superseded_by.join(", ")
            ));
        }
        if reasons.is_empty() {
            reasons.push("Concept members, retrieval, and validations look stable.".to_string());
        }

        let signals = ConceptHealthSignals {
            live_core_member_ratio,
            lineage_coverage_ratio,
            rebind_success_ratio,
            member_churn_ratio,
            validation_coverage_ratio,
            ambiguity_ratio,
            stale_validation_links,
            stale_risk_hint,
        };
        let score = (
            0.35 * live_core_member_ratio
                + 0.15 * lineage_coverage_ratio
                + 0.15 * rebind_success_ratio
                + 0.15 * (1.0 - member_churn_ratio)
                + 0.10 * validation_coverage_ratio
                + 0.10 * (1.0 - ambiguity_ratio)
                - if stale_validation_links { 0.1 } else { 0.0 }
                - if stale_risk_hint { 0.05 } else { 0.0 }
        )
        .clamp(0.0, 1.0);

        let status = if !superseded_by.is_empty() {
            ConceptHealthStatus::SupersededCandidate
        } else if live_core_member_ratio < 0.5 || rebind_success_ratio < 0.5 {
            ConceptHealthStatus::NeedsRepair
        } else if ambiguity_ratio >= 0.9 && member_churn_ratio >= 0.25 {
            ConceptHealthStatus::SplitCandidate
        } else if live_core_member_ratio < 1.0
            || member_churn_ratio > 0.0
            || stale_validation_links
            || stale_risk_hint
            || ambiguity_ratio >= 0.6
        {
            ConceptHealthStatus::Drifted
        } else {
            ConceptHealthStatus::Healthy
        };

        ConceptHealth {
            handle: resolved.handle.clone(),
            status,
            score,
            reasons,
            signals,
            superseded_by,
        }
    }

    fn concept_ambiguity_ratio(&self, concept: &ConceptPacket) -> f32 {
        let mut queries = vec![concept.canonical_name.clone()];
        queries.extend(concept.aliases.iter().cloned());
        queries.sort();
        queries.dedup();

        let mut max_ratio = 0.0;
        for query in queries {
            let resolutions = resolve_concepts(&self.concept_packets, &query, 2);
            let Some(primary) = resolutions.first() else {
                continue;
            };
            if !primary.packet.handle.eq_ignore_ascii_case(&concept.handle) {
                return 1.0;
            }
            if let Some(second) = resolutions.get(1) {
                let top_score = primary.score.max(1) as f32;
                max_ratio = max_ratio.max((second.score as f32 / top_score).clamp(0.0, 1.0));
            }
        }
        max_ratio
    }

    fn active_superseders(&self, handle: &str) -> Vec<String> {
        let mut handles = self
            .concept_packets
            .iter()
            .filter(|concept| {
                concept.handle != handle
                    && concept.publication.as_ref().is_some_and(|publication| {
                        publication.status == crate::ConceptPublicationStatus::Active
                            && publication
                                .supersedes
                                .iter()
                                .any(|candidate| candidate.eq_ignore_ascii_case(handle))
                    })
            })
            .map(|concept| concept.handle.clone())
            .collect::<Vec<_>>();
        handles.sort();
        handles
    }
}

#[derive(Clone)]
struct MemberSlot {
    member: NodeId,
    lineage: Option<LineageId>,
}

fn collect_member_slots(packet: &ConceptPacket) -> Vec<MemberSlot> {
    let mut slots = member_slots(&packet.core_members, &packet.core_member_lineages);
    slots.extend(member_slots(
        &packet.supporting_members,
        &packet.supporting_member_lineages,
    ));
    slots.extend(member_slots(&packet.likely_tests, &packet.likely_test_lineages));
    slots
}

fn member_slots(members: &[NodeId], lineages: &[Option<LineageId>]) -> Vec<MemberSlot> {
    members
        .iter()
        .enumerate()
        .map(|(index, member)| MemberSlot {
            member: member.clone(),
            lineage: lineages.get(index).cloned().flatten(),
        })
        .collect()
}

fn current_concept_lineages(packet: &ConceptPacket) -> Vec<LineageId> {
    let mut lineages = packet
        .core_member_lineages
        .iter()
        .chain(packet.supporting_member_lineages.iter())
        .chain(packet.likely_test_lineages.iter())
        .filter_map(|lineage| lineage.clone())
        .collect::<Vec<_>>();
    lineages.sort_by(|left, right| left.0.cmp(&right.0));
    lineages.dedup();
    lineages
}

fn resolve_member_for_health(
    original: &NodeId,
    lineage: Option<&LineageId>,
    node_to_lineage: &HashMap<NodeId, LineageId>,
) -> Option<NodeId> {
    match lineage {
        Some(lineage) => {
            if node_to_lineage.get(original) == Some(lineage) {
                return Some(original.clone());
            }
            node_to_lineage
                .iter()
                .filter_map(|(candidate, current)| (current == lineage).then_some(candidate.clone()))
                .min_by(|left, right| {
                    candidate_rank_for_health(left, original).cmp(&candidate_rank_for_health(right, original))
                })
        }
        None if node_to_lineage.contains_key(original) => Some(original.clone()),
        None => None,
    }
}

fn candidate_rank_for_health(candidate: &NodeId, original: &NodeId) -> (u8, u8, String, String) {
    (
        u8::from(candidate.kind != original.kind),
        u8::from(candidate.crate_name != original.crate_name),
        candidate.path.to_string(),
        candidate.crate_name.to_string(),
    )
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
