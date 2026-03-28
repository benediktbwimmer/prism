use std::collections::HashMap;

use prism_ir::{LineageEvent, LineageId, NodeId};

use crate::types::{
    ConceptEvent, ConceptEventAction, ConceptPacket, ConceptProvenance, ConceptPublication,
    ConceptPublicationStatus, ConceptScope,
};

pub(crate) fn rank_concepts(
    concepts: &[ConceptPacket],
    query: &str,
    limit: usize,
) -> Vec<ConceptPacket> {
    let normalized = normalize_text(query);
    if normalized.is_empty() {
        return Vec::new();
    }
    let mut ranked = concepts
        .iter()
        .filter_map(|concept| {
            let score = concept_query_score(concept, &normalized);
            (score > 0).then(|| (score, concept.clone()))
        })
        .collect::<Vec<_>>();
    ranked.sort_by(|left, right| {
        right
            .0
            .cmp(&left.0)
            .then_with(|| right.1.confidence.total_cmp(&left.1.confidence))
            .then_with(|| left.1.handle.cmp(&right.1.handle))
    });
    ranked
        .into_iter()
        .take(limit)
        .map(|(_, concept)| concept)
        .collect()
}

pub(crate) fn concept_by_handle(concepts: &[ConceptPacket], handle: &str) -> Option<ConceptPacket> {
    let normalized = normalize_handle(handle);
    concepts
        .iter()
        .find(|concept| normalize_handle(&concept.handle) == normalized)
        .cloned()
}

pub(crate) fn merge_concept_packets(curated: &[ConceptPacket]) -> Vec<ConceptPacket> {
    let mut merged = HashMap::<String, ConceptPacket>::new();
    for concept in curated {
        merged.insert(normalize_handle(&concept.handle), concept.clone());
    }
    let mut packets = merged.into_values().collect::<Vec<_>>();
    packets.sort_by(|left, right| left.handle.cmp(&right.handle));
    packets
}

pub(crate) fn hydrate_curated_concepts(
    concepts: Vec<ConceptPacket>,
    node_to_lineage: &HashMap<NodeId, LineageId>,
    history_events: &[LineageEvent],
) -> Vec<ConceptPacket> {
    concepts
        .into_iter()
        .map(|concept| hydrate_curated_concept(concept, node_to_lineage, history_events))
        .collect()
}

pub(crate) fn resolve_curated_concepts(
    concepts: &[ConceptPacket],
    node_to_lineage: &HashMap<NodeId, LineageId>,
) -> Vec<ConceptPacket> {
    concepts
        .iter()
        .cloned()
        .map(|concept| resolve_curated_concept_members(concept, node_to_lineage))
        .collect()
}

pub fn curated_concepts_from_events(events: &[ConceptEvent]) -> Vec<ConceptPacket> {
    let mut concepts = HashMap::<String, ConceptPacket>::new();
    for event in events {
        let key = normalize_handle(&event.concept.handle);
        let previous = concepts.get(&key).cloned();
        let concept = normalize_curated_concept(event, previous.as_ref());
        if concept
            .publication
            .as_ref()
            .is_some_and(|publication| publication.status == ConceptPublicationStatus::Retired)
        {
            concepts.remove(&key);
        } else {
            concepts.insert(key, concept);
        }
    }
    let mut concepts = concepts.into_values().collect::<Vec<_>>();
    concepts.sort_by(|left, right| left.handle.cmp(&right.handle));
    concepts
}

pub fn canonical_concept_handle(name: &str) -> String {
    let slug = normalize_slug(name);
    if slug.is_empty() {
        "concept://concept".to_string()
    } else {
        format!("concept://{slug}")
    }
}

fn normalize_curated_concept(
    event: &ConceptEvent,
    previous: Option<&ConceptPacket>,
) -> ConceptPacket {
    let mut concept = event.concept.clone();
    if concept.provenance == ConceptProvenance::default() {
        concept.provenance = ConceptProvenance {
            origin: "repo_mutation".to_string(),
            kind: match event.action {
                ConceptEventAction::Promote => "manual_concept_promote".to_string(),
                ConceptEventAction::Update => "manual_concept_update".to_string(),
                ConceptEventAction::Retire => "manual_concept_retire".to_string(),
            },
            task_id: event.task_id.clone(),
        };
    } else if concept.provenance.task_id.is_none() {
        concept.provenance.task_id = event.task_id.clone();
    }

    if concept.scope == ConceptScope::Repo || matches!(event.action, ConceptEventAction::Retire) {
        let mut publication = concept
            .publication
            .clone()
            .unwrap_or_else(|| previous_publication(previous, event.recorded_at));
        if publication.published_at == 0 && concept.scope == ConceptScope::Repo {
            publication.published_at = previous
                .and_then(|packet| packet.publication.as_ref())
                .map(|value| value.published_at)
                .filter(|value| *value > 0)
                .unwrap_or(event.recorded_at);
        }
        publication.last_reviewed_at = Some(event.recorded_at);
        if publication.supersedes.is_empty() {
            publication.supersedes = previous
                .and_then(|packet| packet.publication.as_ref())
                .map(|value| value.supersedes.clone())
                .unwrap_or_default();
        }
        match event.action {
            ConceptEventAction::Promote | ConceptEventAction::Update => {
                publication.status = ConceptPublicationStatus::Active;
                publication.retired_at = None;
                publication.retirement_reason = None;
            }
            ConceptEventAction::Retire => {
                publication.status = ConceptPublicationStatus::Retired;
                publication.retired_at = Some(event.recorded_at);
            }
        }
        concept.publication = Some(publication);
    } else {
        concept.publication = None;
    }
    concept
}

fn previous_publication(previous: Option<&ConceptPacket>, recorded_at: u64) -> ConceptPublication {
    let mut publication = previous
        .and_then(|packet| packet.publication.clone())
        .unwrap_or_default();
    if publication.published_at == 0 {
        publication.published_at = recorded_at;
    }
    publication
}

fn hydrate_curated_concept(
    mut concept: ConceptPacket,
    node_to_lineage: &HashMap<NodeId, LineageId>,
    history_events: &[LineageEvent],
) -> ConceptPacket {
    concept.core_member_lineages = normalize_member_lineages(
        &concept.core_members,
        &concept.core_member_lineages,
        node_to_lineage,
        history_events,
    );
    concept.supporting_member_lineages = normalize_member_lineages(
        &concept.supporting_members,
        &concept.supporting_member_lineages,
        node_to_lineage,
        history_events,
    );
    concept.likely_test_lineages = normalize_member_lineages(
        &concept.likely_tests,
        &concept.likely_test_lineages,
        node_to_lineage,
        history_events,
    );
    resolve_curated_concept_members(concept, node_to_lineage)
}

fn resolve_curated_concept_members(
    mut concept: ConceptPacket,
    node_to_lineage: &HashMap<NodeId, LineageId>,
) -> ConceptPacket {
    let (core_members, core_member_lineages) = resolve_member_bindings(
        &concept.core_members,
        &concept.core_member_lineages,
        node_to_lineage,
    );
    let (supporting_members, supporting_member_lineages) = resolve_member_bindings(
        &concept.supporting_members,
        &concept.supporting_member_lineages,
        node_to_lineage,
    );
    let (likely_tests, likely_test_lineages) = resolve_member_bindings(
        &concept.likely_tests,
        &concept.likely_test_lineages,
        node_to_lineage,
    );
    concept.core_members = core_members;
    concept.core_member_lineages = core_member_lineages;
    concept.supporting_members = supporting_members;
    concept.supporting_member_lineages = supporting_member_lineages;
    concept.likely_tests = likely_tests;
    concept.likely_test_lineages = likely_test_lineages;
    concept
}

fn normalize_member_lineages(
    members: &[NodeId],
    lineages: &[Option<LineageId>],
    node_to_lineage: &HashMap<NodeId, LineageId>,
    history_events: &[LineageEvent],
) -> Vec<Option<LineageId>> {
    members
        .iter()
        .enumerate()
        .map(|(index, member)| {
            lineages
                .get(index)
                .cloned()
                .flatten()
                .or_else(|| node_to_lineage.get(member).cloned())
                .or_else(|| lineage_hint_from_history(member, history_events))
        })
        .collect()
}

fn resolve_member_bindings(
    members: &[NodeId],
    lineages: &[Option<LineageId>],
    node_to_lineage: &HashMap<NodeId, LineageId>,
) -> (Vec<NodeId>, Vec<Option<LineageId>>) {
    let mut resolved_members = Vec::new();
    let mut resolved_lineages = Vec::new();

    for (index, member) in members.iter().enumerate() {
        let lineage = lineages
            .get(index)
            .cloned()
            .flatten()
            .or_else(|| node_to_lineage.get(member).cloned());
        let resolved = match lineage.as_ref() {
            Some(lineage) => resolve_current_member(member, lineage, node_to_lineage),
            None if node_to_lineage.contains_key(member) => Some(member.clone()),
            None => None,
        };
        let Some(resolved) = resolved else {
            continue;
        };
        if resolved_members
            .iter()
            .any(|candidate| candidate == &resolved)
        {
            continue;
        }
        resolved_lineages.push(lineage);
        resolved_members.push(resolved);
    }

    (resolved_members, resolved_lineages)
}

fn resolve_current_member(
    original: &NodeId,
    lineage: &LineageId,
    node_to_lineage: &HashMap<NodeId, LineageId>,
) -> Option<NodeId> {
    if node_to_lineage.get(original) == Some(lineage) {
        return Some(original.clone());
    }

    current_nodes_for_lineage(node_to_lineage, lineage)
        .into_iter()
        .min_by(|left, right| candidate_rank(left, original).cmp(&candidate_rank(right, original)))
}

fn current_nodes_for_lineage(
    node_to_lineage: &HashMap<NodeId, LineageId>,
    lineage: &LineageId,
) -> Vec<NodeId> {
    let mut nodes = node_to_lineage
        .iter()
        .filter_map(|(node, candidate)| (candidate == lineage).then_some(node.clone()))
        .collect::<Vec<_>>();
    nodes.sort_by(|left, right| {
        left.path
            .cmp(&right.path)
            .then_with(|| left.crate_name.cmp(&right.crate_name))
    });
    nodes
}

fn candidate_rank(candidate: &NodeId, original: &NodeId) -> (u8, u8, String, String) {
    (
        u8::from(candidate.kind != original.kind),
        u8::from(candidate.crate_name != original.crate_name),
        candidate.path.to_string(),
        candidate.crate_name.to_string(),
    )
}

fn lineage_hint_from_history(node: &NodeId, history_events: &[LineageEvent]) -> Option<LineageId> {
    history_events.iter().rev().find_map(|event| {
        (event.before.iter().any(|candidate| candidate == node)
            || event.after.iter().any(|candidate| candidate == node))
        .then(|| event.lineage.clone())
    })
}

fn concept_query_score(concept: &ConceptPacket, query: &str) -> i32 {
    let normalized_handle = normalize_handle(&concept.handle);
    if query == normalized_handle {
        return 320;
    }

    let mut score = phrase_score(query, &normalize_text(&concept.canonical_name), 220, 96);
    for alias in &concept.aliases {
        score += phrase_score(query, &normalize_text(alias), 180, 72);
    }
    score += phrase_score(query, &normalized_handle, 260, 120);
    score += phrase_score(query, &normalize_text(&concept.summary), 24, 12);
    score
}

fn phrase_score(haystack: &str, needle: &str, exact_score: i32, contains_score: i32) -> i32 {
    if needle.is_empty() {
        return 0;
    }
    if haystack == needle {
        return exact_score;
    }
    if haystack.contains(needle) || needle.contains(haystack) {
        return contains_score;
    }
    0
}

fn normalize_text(value: &str) -> String {
    value
        .chars()
        .map(|ch| {
            if ch.is_ascii_alphanumeric() {
                ch.to_ascii_lowercase()
            } else {
                ' '
            }
        })
        .collect::<String>()
        .split_whitespace()
        .collect::<Vec<_>>()
        .join(" ")
}

fn normalize_handle(handle: &str) -> String {
    normalize_text(handle.trim_start_matches("concept://"))
}

fn normalize_slug(value: &str) -> String {
    let mut slug = String::new();
    let mut previous_was_separator = false;
    for ch in value.chars() {
        if ch.is_ascii_alphanumeric() {
            slug.push(ch.to_ascii_lowercase());
            previous_was_separator = false;
        } else if !previous_was_separator && !slug.is_empty() {
            slug.push('_');
            previous_was_separator = true;
        }
    }
    slug.trim_matches('_').to_string()
}
