use std::collections::{HashMap, HashSet};

use crate::types::{
    ConceptPacket, ConceptRelation, ConceptRelationEvent, ConceptRelationEventAction,
    ConceptRelationKind,
};

pub(crate) fn merge_concept_relations(relations: &[ConceptRelation]) -> Vec<ConceptRelation> {
    let mut merged = HashMap::<String, ConceptRelation>::new();
    for relation in relations {
        merged.insert(relation_key(relation), normalize_relation(relation.clone()));
    }
    let mut relations = merged.into_values().collect::<Vec<_>>();
    sort_relations(&mut relations);
    relations
}

pub fn concept_relations_from_events(events: &[ConceptRelationEvent]) -> Vec<ConceptRelation> {
    let mut relations = HashMap::<String, ConceptRelation>::new();
    for event in events {
        let relation = normalize_relation(event.relation.clone());
        let key = relation_key(&relation);
        match event.action {
            ConceptRelationEventAction::Upsert => {
                relations.insert(key, relation);
            }
            ConceptRelationEventAction::Retire => {
                relations.remove(&key);
            }
        }
    }
    let mut relations = relations.into_values().collect::<Vec<_>>();
    sort_relations(&mut relations);
    relations
}

pub(crate) fn concept_relations_for_handle(
    relations: &[ConceptRelation],
    handle: &str,
) -> Vec<ConceptRelation> {
    let normalized = normalize_handle(handle);
    let mut related = relations
        .iter()
        .filter(|relation| {
            normalize_handle(&relation.source_handle) == normalized
                || normalize_handle(&relation.target_handle) == normalized
        })
        .cloned()
        .collect::<Vec<_>>();
    sort_relations(&mut related);
    related
}

pub(crate) fn concept_relation_query_bonus(
    handle: &str,
    query: &str,
    relations: &[ConceptRelation],
    concepts: &[ConceptPacket],
) -> (i32, Vec<String>) {
    let query_tokens = normalized_tokens(query);
    if query_tokens.is_empty() {
        return (0, Vec::new());
    }

    let concepts_by_handle = concepts
        .iter()
        .map(|packet| (normalize_handle(&packet.handle), packet))
        .collect::<HashMap<_, _>>();
    let normalized = normalize_handle(handle);
    let mut bonus = 0;
    let mut reasons = HashSet::new();

    for relation in relations.iter().filter(|relation| {
        normalize_handle(&relation.source_handle) == normalized
            || normalize_handle(&relation.target_handle) == normalized
    }) {
        let related_handle = if normalize_handle(&relation.source_handle) == normalized {
            &relation.target_handle
        } else {
            &relation.source_handle
        };
        let Some(related) = concepts_by_handle.get(&normalize_handle(related_handle)) else {
            continue;
        };
        let related_tokens = related_concept_tokens(related);
        let overlap = query_tokens.intersection(&related_tokens).count();
        if overlap == 0 {
            continue;
        }
        bonus += ((overlap as i32) * 12).min(28);
        reasons.insert(format!(
            "related concept term match ({})",
            relation_kind_label(relation.kind)
        ));
    }

    let mut reasons = reasons.into_iter().collect::<Vec<_>>();
    reasons.sort();
    (bonus, reasons)
}

pub(crate) fn normalize_handle(handle: &str) -> String {
    handle.trim().to_ascii_lowercase()
}

fn relation_key(relation: &ConceptRelation) -> String {
    format!(
        "{}|{}|{}",
        normalize_handle(&relation.source_handle),
        normalize_handle(&relation.target_handle),
        relation_kind_label(relation.kind)
    )
}

fn normalize_relation(mut relation: ConceptRelation) -> ConceptRelation {
    relation.source_handle = relation.source_handle.trim().to_string();
    relation.target_handle = relation.target_handle.trim().to_string();
    relation.confidence = relation.confidence.clamp(0.0, 1.0);
    relation.evidence.retain(|value| !value.trim().is_empty());
    relation
}

fn sort_relations(relations: &mut [ConceptRelation]) {
    relations.sort_by(|left, right| {
        left.source_handle
            .cmp(&right.source_handle)
            .then_with(|| left.target_handle.cmp(&right.target_handle))
            .then_with(|| relation_kind_label(left.kind).cmp(relation_kind_label(right.kind)))
    });
}

fn relation_kind_label(kind: ConceptRelationKind) -> &'static str {
    match kind {
        ConceptRelationKind::DependsOn => "depends_on",
        ConceptRelationKind::Specializes => "specializes",
        ConceptRelationKind::PartOf => "part_of",
        ConceptRelationKind::ValidatedBy => "validated_by",
        ConceptRelationKind::OftenUsedWith => "often_used_with",
        ConceptRelationKind::Supersedes => "supersedes",
        ConceptRelationKind::ConfusedWith => "confused_with",
    }
}

fn related_concept_tokens(packet: &ConceptPacket) -> HashSet<String> {
    let mut tokens = normalized_tokens(&packet.canonical_name);
    for alias in &packet.aliases {
        tokens.extend(normalized_tokens(alias));
    }
    tokens
}

fn normalized_tokens(value: &str) -> HashSet<String> {
    value
        .split(|ch: char| !ch.is_ascii_alphanumeric())
        .filter(|part| part.len() >= 3)
        .map(|part| part.to_ascii_lowercase())
        .collect()
}
