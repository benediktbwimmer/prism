use std::collections::{HashMap, HashSet};

use prism_history::HistorySnapshot;
use prism_ir::{AnchorRef, LineageEvent, LineageId, NodeId};
use prism_memory::{OutcomeEvent, OutcomeMemorySnapshot};

use crate::common::{event_weight, validation_labels};
use crate::concept_relations::{
    concept_relation_query_bonus, concept_relations_for_handle, merge_concept_relations,
};
use crate::concepts::{
    concept_by_handle, curated_concepts_from_events, hydrate_curated_concepts,
    merge_concept_packets, resolve_concepts, resolve_curated_concept_members,
    resolve_curated_concepts,
};
use crate::contracts::{
    contract_by_handle, curated_contracts_from_events, merge_contract_packets, resolve_contracts,
};
use crate::types::{
    CoChangeDelta, CoChangeRecord, ConceptEvent, ConceptHealth, ConceptHealthSignals,
    ConceptHealthStatus, ConceptPacket, ConceptRelation, ConceptResolution, ConceptScope,
    ContractEvent, ContractHealth, ContractHealthSignals, ContractHealthStatus, ContractPacket,
    ContractResolution, ContractStatus, ProjectionSnapshot, ValidationCheck, ValidationDelta,
};

pub const MAX_CO_CHANGE_NEIGHBORS_PER_LINEAGE: usize = 32;
// Guard the hot path against quadratic co-change explosions on bulk edits.
pub const MAX_CO_CHANGE_LINEAGES_PER_CHANGESET: usize = 128;
pub const MAX_CO_CHANGE_DELTAS_PER_CHANGESET: usize = 2048;
pub const MAX_CO_CHANGE_SAMPLED_LINEAGES_PER_CHANGESET: usize = sampled_co_change_lineage_limit(
    MAX_CO_CHANGE_LINEAGES_PER_CHANGESET,
    MAX_CO_CHANGE_DELTAS_PER_CHANGESET,
);

const fn sampled_co_change_lineage_limit(max_lineages: usize, max_deltas: usize) -> usize {
    let mut lineages = 1usize;
    while lineages < max_lineages {
        let next = lineages + 1;
        if next.saturating_mul(next.saturating_sub(1)) > max_deltas {
            break;
        }
        lineages = next;
    }
    lineages
}

#[derive(Debug, Clone)]
pub struct CoChangeDeltaBatch {
    pub deltas: Vec<CoChangeDelta>,
    pub distinct_lineage_count: usize,
    pub sampled_lineage_count: usize,
    pub truncated: bool,
}

#[derive(Debug, Clone, Default)]
pub struct ProjectionIndex {
    co_change_by_lineage: HashMap<LineageId, Vec<CoChangeRecord>>,
    validation_by_lineage: HashMap<LineageId, Vec<ValidationCheck>>,
    node_to_lineage: HashMap<NodeId, LineageId>,
    curated_concepts: Vec<ConceptPacket>,
    concept_relations: Vec<ConceptRelation>,
    concept_packets: Vec<ConceptPacket>,
    curated_contracts: Vec<ContractPacket>,
    contract_packets: Vec<ContractPacket>,
    history_hydrated: bool,
}

impl ProjectionIndex {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn from_snapshot(snapshot: ProjectionSnapshot) -> Self {
        Self::from_snapshot_with_history(snapshot, None)
    }

    pub fn from_snapshot_with_history(
        snapshot: ProjectionSnapshot,
        history: Option<&HistorySnapshot>,
    ) -> Self {
        let ProjectionSnapshot {
            co_change_by_lineage,
            validation_by_lineage,
            curated_concepts,
            concept_relations,
        } = snapshot;
        let mut co_change_by_lineage = co_change_by_lineage.into_iter().collect::<HashMap<_, _>>();
        normalize_co_change_by_lineage(&mut co_change_by_lineage);
        let node_to_lineage = history
            .map(|history| {
                history
                    .node_to_lineage
                    .iter()
                    .cloned()
                    .collect::<HashMap<NodeId, LineageId>>()
            })
            .unwrap_or_default();
        let validation_by_lineage = validation_by_lineage.into_iter().collect();
        let curated_concepts = if let Some(history) = history {
            hydrate_curated_concepts(curated_concepts, &node_to_lineage, &history.events)
        } else {
            curated_concepts
        };
        let concept_relations = merge_concept_relations(&concept_relations);
        let concept_packets = merge_concept_packets(&resolve_curated_concepts(
            &curated_concepts,
            &node_to_lineage,
        ));
        Self {
            co_change_by_lineage,
            validation_by_lineage,
            node_to_lineage,
            curated_concepts,
            concept_relations,
            concept_packets,
            curated_contracts: Vec::new(),
            contract_packets: Vec::new(),
            history_hydrated: history.is_some(),
        }
    }

    pub fn derive(history: &HistorySnapshot, outcomes: &OutcomeMemorySnapshot) -> Self {
        Self::derive_with_knowledge(history, outcomes, Vec::new(), Vec::new())
    }

    pub fn derive_with_curated(
        history: &HistorySnapshot,
        outcomes: &OutcomeMemorySnapshot,
        curated_concepts: Vec<ConceptPacket>,
    ) -> Self {
        Self::derive_with_knowledge(history, outcomes, curated_concepts, Vec::new())
    }

    pub fn derive_with_knowledge(
        history: &HistorySnapshot,
        outcomes: &OutcomeMemorySnapshot,
        curated_concepts: Vec<ConceptPacket>,
        concept_relations: Vec<ConceptRelation>,
    ) -> Self {
        let node_to_lineage = history
            .node_to_lineage
            .iter()
            .cloned()
            .collect::<HashMap<NodeId, LineageId>>();

        let mut co_change_by_lineage = co_change_by_lineage_from_history_events(&history.events);
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
        let concept_relations = merge_concept_relations(&concept_relations);
        let concept_packets = merge_concept_packets(&curated_concepts);

        Self {
            co_change_by_lineage,
            validation_by_lineage,
            node_to_lineage,
            curated_concepts,
            concept_relations,
            concept_packets,
            curated_contracts: Vec::new(),
            contract_packets: Vec::new(),
            history_hydrated: true,
        }
    }

    pub fn replace_curated_concepts(&mut self, curated_concepts: Vec<ConceptPacket>) {
        self.curated_concepts = curated_concepts;
        self.rebuild_concepts();
    }

    pub fn replace_concept_relations(&mut self, concept_relations: Vec<ConceptRelation>) {
        self.concept_relations = merge_concept_relations(&concept_relations);
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

    pub fn upsert_concept_relation(&mut self, relation: ConceptRelation) {
        self.concept_relations.push(relation);
        self.concept_relations = merge_concept_relations(&self.concept_relations);
        self.rebuild_concepts();
    }

    pub fn replace_curated_contracts(&mut self, curated_contracts: Vec<ContractPacket>) {
        self.curated_contracts = curated_contracts;
        self.rebuild_contracts();
    }

    pub fn replace_curated_contracts_from_events(&mut self, events: &[ContractEvent]) {
        self.replace_curated_contracts(curated_contracts_from_events(events));
    }

    pub fn upsert_curated_contract(&mut self, contract: ContractPacket) {
        let normalized = contract.handle.to_ascii_lowercase();
        self.curated_contracts
            .retain(|candidate| candidate.handle.to_ascii_lowercase() != normalized);
        if contract.status != ContractStatus::Retired {
            self.curated_contracts.push(contract);
        }
        self.curated_contracts
            .sort_by(|left, right| left.handle.cmp(&right.handle));
        self.rebuild_contracts();
    }

    pub fn remove_concept_relation(
        &mut self,
        source_handle: &str,
        target_handle: &str,
        kind: crate::ConceptRelationKind,
    ) {
        let source = crate::concept_relations::normalize_handle(source_handle);
        let target = crate::concept_relations::normalize_handle(target_handle);
        self.concept_relations.retain(|relation| {
            !(crate::concept_relations::normalize_handle(&relation.source_handle) == source
                && crate::concept_relations::normalize_handle(&relation.target_handle) == target
                && relation.kind == kind)
        });
        self.rebuild_concepts();
    }

    pub fn curated_concepts(&self) -> &[ConceptPacket] {
        &self.curated_concepts
    }

    pub fn concept_relations(&self) -> &[ConceptRelation] {
        &self.concept_relations
    }

    pub fn curated_contracts(&self) -> &[ContractPacket] {
        &self.curated_contracts
    }

    pub fn co_change_lineage_count(&self) -> usize {
        self.co_change_by_lineage.len()
    }

    pub fn validation_lineage_count(&self) -> usize {
        self.validation_by_lineage.len()
    }

    pub fn reseed_from_history(&mut self, history: &HistorySnapshot) {
        if self.history_hydrated {
            return;
        }
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
        self.history_hydrated = true;
    }

    pub fn apply_lineage_events(&mut self, events: &[LineageEvent]) {
        let deltas = co_change_deltas_for_events(events);
        self.apply_lineage_events_with_co_change_deltas(events, &deltas);
    }

    pub fn apply_lineage_events_with_co_change_deltas(
        &mut self,
        events: &[LineageEvent],
        deltas: &[CoChangeDelta],
    ) {
        self.apply_co_change_deltas(deltas);
        let dirty_lineages = events
            .iter()
            .map(|event| event.lineage.clone())
            .collect::<HashSet<_>>();
        let dirty_nodes = events
            .iter()
            .flat_map(|event| event.before.iter().chain(event.after.iter()))
            .cloned()
            .collect::<HashSet<_>>();
        for event in events {
            for before in &event.before {
                self.node_to_lineage.remove(before);
            }
            for after in &event.after {
                self.node_to_lineage
                    .insert(after.clone(), event.lineage.clone());
            }
        }
        self.refresh_concepts_for_invalidation(&dirty_lineages, &dirty_nodes);
        self.history_hydrated = true;
    }

    pub fn apply_outcome_event<F>(&mut self, event: &OutcomeEvent, mut lineage_of: F)
    where
        F: FnMut(&NodeId) -> Option<LineageId>,
    {
        let validation_deltas = validation_deltas_for_event(event, &mut lineage_of);
        let dirty_lineages = validation_deltas
            .iter()
            .map(|delta| delta.lineage.clone())
            .collect::<HashSet<_>>();
        self.apply_validation_deltas(&validation_deltas);
        self.refresh_concepts_for_invalidation(&dirty_lineages, &HashSet::new());
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
            concept_relations: self
                .concept_relations
                .iter()
                .filter(|relation| relation.scope == ConceptScope::Session)
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
        self.resolve_concepts(query, limit)
            .into_iter()
            .map(|resolution| resolution.packet)
            .collect()
    }

    pub fn resolve_concepts(&self, query: &str, limit: usize) -> Vec<ConceptResolution> {
        let mut resolutions = resolve_concepts(&self.concept_packets, query, 0);
        for resolution in &mut resolutions {
            let (bonus, reasons) = concept_relation_query_bonus(
                &resolution.packet.handle,
                query,
                &self.concept_relations,
                &self.concept_packets,
            );
            resolution.score += bonus;
            resolution.reasons.extend(reasons);
        }
        resolutions.sort_by(|left, right| {
            right
                .score
                .cmp(&left.score)
                .then_with(|| right.packet.confidence.total_cmp(&left.packet.confidence))
                .then_with(|| left.packet.handle.cmp(&right.packet.handle))
        });
        if limit > 0 {
            resolutions.truncate(limit);
        }
        resolutions
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

    pub fn concept_relations_for_handle(&self, handle: &str) -> Vec<ConceptRelation> {
        concept_relations_for_handle(&self.concept_relations, handle)
    }

    pub fn contracts(&self, query: &str, limit: usize) -> Vec<ContractPacket> {
        self.resolve_contracts(query, limit)
            .into_iter()
            .map(|resolution| resolution.packet)
            .collect()
    }

    pub fn resolve_contracts(&self, query: &str, limit: usize) -> Vec<ContractResolution> {
        resolve_contracts(&self.contract_packets, query, limit)
    }

    pub fn contract_by_handle(&self, handle: &str) -> Option<ContractPacket> {
        contract_by_handle(&self.contract_packets, handle)
    }

    pub fn contract_health(&self, handle: &str) -> Option<ContractHealth> {
        let packet = contract_by_handle(&self.contract_packets, handle)?;
        Some(self.compute_contract_health(&packet))
    }

    pub fn contract_packets(&self) -> &[ContractPacket] {
        &self.contract_packets
    }

    fn rebuild_concepts(&mut self) {
        self.concept_packets = merge_concept_packets(&resolve_curated_concepts(
            &self.curated_concepts,
            &self.node_to_lineage,
        ));
        self.prune_concept_relations();
    }

    fn refresh_concepts_for_invalidation(
        &mut self,
        dirty_lineages: &HashSet<LineageId>,
        dirty_nodes: &HashSet<NodeId>,
    ) {
        let dirty_handles = self
            .curated_concepts
            .iter()
            .filter(|concept| concept_touches_invalidation(concept, dirty_lineages, dirty_nodes))
            .map(|concept| crate::concept_relations::normalize_handle(&concept.handle))
            .collect::<HashSet<_>>();

        if dirty_handles.is_empty() {
            return;
        }

        let mut packets = self
            .concept_packets
            .iter()
            .cloned()
            .map(|packet| {
                (
                    crate::concept_relations::normalize_handle(&packet.handle),
                    packet,
                )
            })
            .collect::<HashMap<_, _>>();

        for concept in &self.curated_concepts {
            let normalized = crate::concept_relations::normalize_handle(&concept.handle);
            if dirty_handles.contains(&normalized) {
                packets.insert(
                    normalized,
                    resolve_curated_concept_members(concept.clone(), &self.node_to_lineage),
                );
            }
        }

        let mut packets = packets.into_values().collect::<Vec<_>>();
        packets.sort_by(|left, right| left.handle.cmp(&right.handle));
        self.concept_packets = merge_concept_packets(&packets);
        self.prune_concept_relations();
    }

    fn prune_concept_relations(&mut self) {
        let known_handles = self
            .concept_packets
            .iter()
            .map(|packet| crate::concept_relations::normalize_handle(&packet.handle))
            .collect::<std::collections::HashSet<_>>();
        self.concept_relations.retain(|relation| {
            known_handles.contains(&crate::concept_relations::normalize_handle(
                &relation.source_handle,
            )) && known_handles.contains(&crate::concept_relations::normalize_handle(
                &relation.target_handle,
            ))
        });
        self.concept_relations = merge_concept_relations(&self.concept_relations);
    }

    fn rebuild_contracts(&mut self) {
        self.contract_packets = merge_contract_packets(&self.curated_contracts);
    }

    fn compute_concept_health(
        &self,
        original: &ConceptPacket,
        resolved: &ConceptPacket,
    ) -> ConceptHealth {
        let original_core_count = original.core_members.len().max(1);
        let original_slots = collect_member_slots(original);
        let slot_count = original_slots.len().max(1);
        let live_core_member_ratio =
            (resolved.core_members.len() as f32 / original_core_count as f32).clamp(0.0, 1.0);

        let lineage_coverage_count = original_slots
            .iter()
            .filter(|slot| slot.lineage.is_some())
            .count();
        let lineage_coverage_ratio =
            (lineage_coverage_count as f32 / slot_count as f32).clamp(0.0, 1.0);

        let mut changed_slots = 0usize;
        let mut rebound_slots = 0usize;
        for slot in &original_slots {
            let current = resolve_member_for_health(
                &slot.member,
                slot.lineage.as_ref(),
                &self.node_to_lineage,
            );
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
            reasons.push(
                "Risk hint may be stale relative to the current concept bindings.".to_string(),
            );
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
        let score = (0.35 * live_core_member_ratio
            + 0.15 * lineage_coverage_ratio
            + 0.15 * rebind_success_ratio
            + 0.15 * (1.0 - member_churn_ratio)
            + 0.10 * validation_coverage_ratio
            + 0.10 * (1.0 - ambiguity_ratio)
            - if stale_validation_links { 0.1 } else { 0.0 }
            - if stale_risk_hint { 0.05 } else { 0.0 })
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

    fn compute_contract_health(&self, packet: &ContractPacket) -> ContractHealth {
        let guarantee_count = packet.guarantees.len();
        let validation_count = packet.validations.len();
        let consumer_count = packet.consumers.len();
        let validation_coverage_ratio = if guarantee_count == 0 {
            0.0
        } else {
            (validation_count as f32 / guarantee_count as f32).clamp(0.0, 1.0)
        };
        let guarantees_with_evidence = packet
            .guarantees
            .iter()
            .filter(|guarantee| !guarantee.evidence_refs.is_empty())
            .count();
        let guarantee_evidence_ratio = if guarantee_count == 0 {
            0.0
        } else {
            (guarantees_with_evidence as f32 / guarantee_count as f32).clamp(0.0, 1.0)
        };
        let stale_validation_links = packet.validations.iter().any(|validation| {
            !validation.anchors.is_empty()
                && validation
                    .anchors
                    .iter()
                    .all(|anchor| !self.contract_anchor_is_live(anchor))
        });
        let superseded_by = self.active_contract_superseders(&packet.handle);

        let mut reasons = Vec::new();
        if !superseded_by.is_empty() {
            reasons.push(format!(
                "Superseded by active contract(s): {}.",
                superseded_by.join(", ")
            ));
        }
        if packet.status == ContractStatus::Retired {
            reasons.push(
                "Contract is retired and should no longer be treated as a live promise."
                    .to_string(),
            );
        }
        if validation_count == 0 {
            reasons.push("Contract has no explicit validation links yet.".to_string());
        } else if validation_coverage_ratio < 1.0 {
            reasons.push(format!(
                "Only {} validation link(s) cover {} guarantee clause(s).",
                validation_count, guarantee_count
            ));
        }
        if guarantee_evidence_ratio < 1.0 {
            reasons.push(format!(
                "{} of {} guarantee clause(s) have clause-level evidence refs.",
                guarantees_with_evidence, guarantee_count
            ));
        }
        if stale_validation_links {
            reasons.push(
                "At least one contract validation anchor no longer resolves cleanly.".to_string(),
            );
        }
        if reasons.is_empty() {
            reasons.push(
                "Contract guarantees, validations, and publication state look healthy.".to_string(),
            );
        }

        let signals = ContractHealthSignals {
            guarantee_count,
            validation_count,
            consumer_count,
            validation_coverage_ratio,
            guarantee_evidence_ratio,
            stale_validation_links,
        };
        let score = (0.45 * validation_coverage_ratio
            + 0.30 * guarantee_evidence_ratio
            + if stale_validation_links { 0.0 } else { 0.15 }
            + if superseded_by.is_empty() { 0.10 } else { 0.0 })
        .clamp(0.0, 1.0);

        let status = if packet.status == ContractStatus::Retired {
            ContractHealthStatus::Retired
        } else if !superseded_by.is_empty() {
            ContractHealthStatus::Superseded
        } else if stale_validation_links || validation_count == 0 {
            ContractHealthStatus::Stale
        } else if validation_coverage_ratio < 0.5 || guarantee_evidence_ratio < 0.5 {
            ContractHealthStatus::Degraded
        } else if validation_coverage_ratio < 1.0 || guarantee_evidence_ratio < 1.0 {
            ContractHealthStatus::Watch
        } else {
            ContractHealthStatus::Healthy
        };

        ContractHealth {
            handle: packet.handle.clone(),
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

        let mut max_ratio: f32 = 0.0;
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

    fn active_contract_superseders(&self, handle: &str) -> Vec<String> {
        let mut handles = self
            .contract_packets
            .iter()
            .filter(|contract| {
                contract.handle != handle
                    && contract.status != ContractStatus::Retired
                    && contract.publication.as_ref().is_some_and(|publication| {
                        publication.status == crate::ContractPublicationStatus::Active
                            && publication
                                .supersedes
                                .iter()
                                .any(|candidate| candidate.eq_ignore_ascii_case(handle))
                    })
            })
            .map(|contract| contract.handle.clone())
            .collect::<Vec<_>>();
        handles.sort();
        handles
    }

    fn contract_anchor_is_live(&self, anchor: &AnchorRef) -> bool {
        match anchor {
            AnchorRef::Node(node) => self.node_to_lineage.contains_key(node),
            AnchorRef::Lineage(lineage) => {
                self.node_to_lineage.values().any(|value| value == lineage)
            }
            AnchorRef::File(_) | AnchorRef::Kind(_) => true,
        }
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
    slots.extend(member_slots(
        &packet.likely_tests,
        &packet.likely_test_lineages,
    ));
    slots
}

fn concept_touches_invalidation(
    concept: &ConceptPacket,
    dirty_lineages: &HashSet<LineageId>,
    dirty_nodes: &HashSet<NodeId>,
) -> bool {
    concept
        .core_members
        .iter()
        .chain(concept.supporting_members.iter())
        .chain(concept.likely_tests.iter())
        .any(|member| dirty_nodes.contains(member))
        || concept
            .core_member_lineages
            .iter()
            .chain(concept.supporting_member_lineages.iter())
            .chain(concept.likely_test_lineages.iter())
            .filter_map(|lineage| lineage.as_ref())
            .any(|lineage| dirty_lineages.contains(lineage))
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
                .filter_map(|(candidate, current)| {
                    (current == lineage).then_some(candidate.clone())
                })
                .min_by(|left, right| {
                    candidate_rank_for_health(left, original)
                        .cmp(&candidate_rank_for_health(right, original))
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

pub fn co_change_delta_batch_for_events(events: &[LineageEvent]) -> CoChangeDeltaBatch {
    let lineage_selection = distinct_change_set_lineages(events);
    let lineages = lineage_selection.lineages;
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
    CoChangeDeltaBatch {
        deltas,
        distinct_lineage_count: lineage_selection.total_distinct_lineages,
        sampled_lineage_count: lineages.len(),
        truncated: lineage_selection.truncated,
    }
}

pub fn co_change_deltas_for_events(events: &[LineageEvent]) -> Vec<CoChangeDelta> {
    co_change_delta_batch_for_events(events).deltas
}

fn co_change_by_lineage_from_history_events(
    events: &[LineageEvent],
) -> HashMap<LineageId, Vec<CoChangeRecord>> {
    let mut change_sets = HashMap::<String, Vec<LineageId>>::new();
    for event in events {
        let change_set_id = event
            .meta
            .causation
            .as_ref()
            .map(|id| id.0.to_string())
            .unwrap_or_else(|| event.meta.id.0.to_string());
        change_sets
            .entry(change_set_id)
            .or_default()
            .push(event.lineage.clone());
    }

    let mut counts = HashMap::<LineageId, HashMap<LineageId, u32>>::new();
    for lineages in change_sets.into_values() {
        let lineages = distinct_sorted_lineages(lineages).lineages;
        if lineages.len() < 2 {
            continue;
        }
        for (index, left) in lineages.iter().enumerate() {
            for right in lineages.iter().skip(index + 1) {
                *counts
                    .entry(left.clone())
                    .or_default()
                    .entry(right.clone())
                    .or_default() += 1;
                *counts
                    .entry(right.clone())
                    .or_default()
                    .entry(left.clone())
                    .or_default() += 1;
            }
        }
    }

    counts
        .into_iter()
        .map(|(source, neighbors)| {
            let neighbors = neighbors
                .into_iter()
                .map(|(lineage, count)| CoChangeRecord { lineage, count })
                .collect();
            (source, neighbors)
        })
        .collect()
}

fn distinct_change_set_lineages(events: &[LineageEvent]) -> DistinctLineageSelection {
    distinct_sorted_lineages(events.iter().map(|event| event.lineage.clone()).collect())
}

#[derive(Debug, Clone)]
struct DistinctLineageSelection {
    lineages: Vec<LineageId>,
    total_distinct_lineages: usize,
    truncated: bool,
}

fn distinct_sorted_lineages(mut lineages: Vec<LineageId>) -> DistinctLineageSelection {
    lineages.sort_by(|left, right| left.0.cmp(&right.0));
    lineages.dedup();
    let total_distinct_lineages = lineages.len();
    if total_distinct_lineages <= MAX_CO_CHANGE_SAMPLED_LINEAGES_PER_CHANGESET {
        return DistinctLineageSelection {
            lineages,
            total_distinct_lineages,
            truncated: false,
        };
    }

    let mut sampled = Vec::with_capacity(MAX_CO_CHANGE_SAMPLED_LINEAGES_PER_CHANGESET);
    let last_index = total_distinct_lineages - 1;
    let last_slot = MAX_CO_CHANGE_SAMPLED_LINEAGES_PER_CHANGESET - 1;
    for slot in 0..MAX_CO_CHANGE_SAMPLED_LINEAGES_PER_CHANGESET {
        let index = if last_slot == 0 {
            0
        } else {
            slot * last_index / last_slot
        };
        sampled.push(lineages[index].clone());
    }

    DistinctLineageSelection {
        lineages: sampled,
        total_distinct_lineages,
        truncated: true,
    }
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
