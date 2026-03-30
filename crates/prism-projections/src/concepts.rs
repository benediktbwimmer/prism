use std::collections::{HashMap, HashSet};

use prism_ir::{LineageEvent, LineageId, NodeId};

use crate::types::{
    ConceptEvent, ConceptEventAction, ConceptPacket, ConceptProvenance, ConceptPublication,
    ConceptPublicationStatus, ConceptResolution, ConceptScope,
};

pub(crate) fn resolve_concepts(
    concepts: &[ConceptPacket],
    query: &str,
    limit: usize,
) -> Vec<ConceptResolution> {
    let normalized_query = normalize_text(query);
    let query_tokens = normalized_tokens(query);
    if normalized_query.is_empty() || query_tokens.is_empty() {
        return Vec::new();
    }
    let query_trigrams = trigrams(&normalized_query);
    let mut ranked = concepts
        .iter()
        .filter_map(|concept| {
            concept_query_resolution(concept, &normalized_query, &query_tokens, &query_trigrams)
        })
        .collect::<Vec<_>>();
    ranked.sort_by(|left, right| {
        right
            .score
            .cmp(&left.score)
            .then_with(|| right.packet.confidence.total_cmp(&left.packet.confidence))
            .then_with(|| left.packet.handle.cmp(&right.packet.handle))
    });
    if limit > 0 {
        ranked.truncate(limit);
    }
    ranked
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
        let concept = concept_from_event(previous.as_ref(), event);
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

pub fn concept_from_event(previous: Option<&ConceptPacket>, event: &ConceptEvent) -> ConceptPacket {
    let concept = concept_event_post_image(previous, event);
    normalize_curated_concept(event, previous, concept)
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
    mut concept: ConceptPacket,
) -> ConceptPacket {
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

fn concept_event_post_image(
    previous: Option<&ConceptPacket>,
    event: &ConceptEvent,
) -> ConceptPacket {
    let Some(previous) = previous else {
        return event.concept.clone();
    };
    let Some(patch) = event.patch.as_ref() else {
        return event.concept.clone();
    };
    let mut concept = previous.clone();
    if has_patch_field(&patch.set_fields, "canonicalName") {
        concept.canonical_name = patch
            .canonical_name
            .clone()
            .unwrap_or_else(|| event.concept.canonical_name.clone());
    }
    if has_patch_field(&patch.set_fields, "summary") {
        concept.summary = patch
            .summary
            .clone()
            .unwrap_or_else(|| event.concept.summary.clone());
    }
    if has_patch_field(&patch.set_fields, "aliases") {
        concept.aliases = patch
            .aliases
            .clone()
            .unwrap_or_else(|| event.concept.aliases.clone());
    }
    if has_patch_field(&patch.set_fields, "coreMembers") {
        concept.core_members = patch
            .core_members
            .clone()
            .unwrap_or_else(|| event.concept.core_members.clone());
        concept.core_member_lineages = patch
            .core_member_lineages
            .clone()
            .unwrap_or_else(|| event.concept.core_member_lineages.clone());
    }
    if has_patch_field(&patch.set_fields, "supportingMembers") {
        concept.supporting_members = patch
            .supporting_members
            .clone()
            .unwrap_or_else(|| event.concept.supporting_members.clone());
        concept.supporting_member_lineages = patch
            .supporting_member_lineages
            .clone()
            .unwrap_or_else(|| event.concept.supporting_member_lineages.clone());
    }
    if has_patch_field(&patch.set_fields, "likelyTests") {
        concept.likely_tests = patch
            .likely_tests
            .clone()
            .unwrap_or_else(|| event.concept.likely_tests.clone());
        concept.likely_test_lineages = patch
            .likely_test_lineages
            .clone()
            .unwrap_or_else(|| event.concept.likely_test_lineages.clone());
    }
    if has_patch_field(&patch.set_fields, "evidence") {
        concept.evidence = patch
            .evidence
            .clone()
            .unwrap_or_else(|| event.concept.evidence.clone());
    }
    if has_patch_field(&patch.cleared_fields, "riskHint") {
        concept.risk_hint = None;
    } else if has_patch_field(&patch.set_fields, "riskHint") {
        concept.risk_hint = Some(
            patch
                .risk_hint
                .clone()
                .or_else(|| event.concept.risk_hint.clone())
                .unwrap_or_default(),
        );
    }
    if has_patch_field(&patch.set_fields, "confidence") {
        concept.confidence = patch.confidence.unwrap_or(event.concept.confidence);
    }
    if has_patch_field(&patch.set_fields, "decodeLenses") {
        concept.decode_lenses = patch
            .decode_lenses
            .clone()
            .unwrap_or_else(|| event.concept.decode_lenses.clone());
    }
    if has_patch_field(&patch.set_fields, "scope") {
        concept.scope = patch.scope.unwrap_or(event.concept.scope);
    }
    if has_patch_field(&patch.set_fields, "supersedes") {
        let mut publication = concept
            .publication
            .clone()
            .or_else(|| event.concept.publication.clone())
            .unwrap_or_default();
        publication.supersedes = patch.supersedes.clone().unwrap_or_else(|| {
            event
                .concept
                .publication
                .as_ref()
                .map(|publication| publication.supersedes.clone())
                .unwrap_or_default()
        });
        concept.publication = Some(publication);
    }
    if has_patch_field(&patch.set_fields, "retirementReason") {
        let mut publication = concept
            .publication
            .clone()
            .or_else(|| event.concept.publication.clone())
            .unwrap_or_default();
        publication.retirement_reason = patch.retirement_reason.clone().or_else(|| {
            event
                .concept
                .publication
                .as_ref()
                .and_then(|publication| publication.retirement_reason.clone())
        });
        concept.publication = Some(publication);
    }
    concept
}

fn has_patch_field(fields: &[String], target: &str) -> bool {
    fields.iter().any(|field| field == target)
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

pub(crate) fn resolve_curated_concept_members(
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

fn normalize_text(value: &str) -> String {
    normalized_tokens(value).join(" ")
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

fn concept_query_resolution(
    concept: &ConceptPacket,
    query: &str,
    query_tokens: &[String],
    query_trigrams: &HashSet<String>,
) -> Option<ConceptResolution> {
    let canonical = normalize_text(&concept.canonical_name);
    let handle = normalize_handle(&concept.handle);
    let summary = normalize_text(&concept.summary);
    let aliases = concept
        .aliases
        .iter()
        .map(|alias| normalize_text(alias))
        .collect::<Vec<_>>();
    let member_labels = concept_member_labels(concept);

    let mut score = 0;
    let mut reasons = Vec::new();

    score += exact_or_contains_score(
        query,
        &canonical,
        320,
        170,
        &mut reasons,
        "canonical exact match",
        "canonical lexical match",
    );
    score += exact_or_contains_score(
        query,
        &handle,
        340,
        180,
        &mut reasons,
        "handle exact match",
        "handle lexical match",
    );
    for alias in &aliases {
        score += exact_or_contains_score(
            query,
            alias,
            300,
            160,
            &mut reasons,
            "alias exact match",
            "alias lexical match",
        );
    }
    score += exact_or_contains_score(
        query,
        &summary,
        70,
        40,
        &mut reasons,
        "summary exact match",
        "summary term match",
    );

    let query_token_set = query_tokens.iter().cloned().collect::<HashSet<_>>();
    score += token_overlap_score(
        &query_token_set,
        &normalized_tokens(&concept.canonical_name),
        26,
        100,
        &mut reasons,
        "canonical term match",
    );
    for alias in &concept.aliases {
        score += token_overlap_score(
            &query_token_set,
            &normalized_tokens(alias),
            22,
            88,
            &mut reasons,
            "alias term match",
        );
    }
    score += token_overlap_score(
        &query_token_set,
        &normalized_tokens(&concept.summary),
        10,
        36,
        &mut reasons,
        "summary term match",
    );
    for label in &member_labels {
        score += token_overlap_score(
            &query_token_set,
            &normalized_tokens(label),
            12,
            48,
            &mut reasons,
            "member name match",
        );
    }

    score += fuzzy_text_score(
        query,
        query_tokens,
        query_trigrams,
        &canonical,
        &mut reasons,
        "fuzzy canonical match",
    );
    for alias in &aliases {
        score += fuzzy_text_score(
            query,
            query_tokens,
            query_trigrams,
            alias,
            &mut reasons,
            "fuzzy alias match",
        );
    }
    for label in &member_labels {
        score += fuzzy_text_score(
            query,
            query_tokens,
            query_trigrams,
            &normalize_text(label),
            &mut reasons,
            "fuzzy member match",
        );
    }

    if score <= 0 {
        return None;
    }

    score += (concept.confidence * 20.0).round() as i32;

    Some(ConceptResolution {
        packet: concept.clone(),
        score,
        reasons,
    })
}

fn concept_member_labels(concept: &ConceptPacket) -> Vec<String> {
    concept
        .core_members
        .iter()
        .chain(concept.supporting_members.iter())
        .chain(concept.likely_tests.iter())
        .map(node_label)
        .collect()
}

fn node_label(node: &NodeId) -> String {
    node.path
        .rsplit("::")
        .next()
        .unwrap_or(node.path.as_str())
        .replace("_md", " md")
}

fn exact_or_contains_score(
    query: &str,
    candidate: &str,
    exact_score: i32,
    contains_score: i32,
    reasons: &mut Vec<String>,
    exact_reason: &str,
    contains_reason: &str,
) -> i32 {
    if candidate.is_empty() {
        return 0;
    }
    if query == candidate {
        push_reason(reasons, exact_reason);
        return exact_score;
    }
    if candidate.contains(query) || query.contains(candidate) {
        push_reason(reasons, contains_reason);
        return contains_score;
    }
    0
}

fn token_overlap_score(
    query_tokens: &HashSet<String>,
    candidate_tokens: &[String],
    per_token: i32,
    max_score: i32,
    reasons: &mut Vec<String>,
    reason: &str,
) -> i32 {
    if query_tokens.is_empty() || candidate_tokens.is_empty() {
        return 0;
    }
    let overlap = candidate_tokens
        .iter()
        .filter(|token| query_tokens.contains(*token))
        .count();
    if overlap == 0 {
        return 0;
    }
    push_reason(reasons, reason);
    (overlap as i32 * per_token).min(max_score)
}

fn fuzzy_text_score(
    query: &str,
    query_tokens: &[String],
    query_trigrams: &HashSet<String>,
    candidate: &str,
    reasons: &mut Vec<String>,
    reason: &str,
) -> i32 {
    if candidate.is_empty() {
        return 0;
    }
    if within_edit_distance(query, candidate, 2) {
        push_reason(reasons, reason);
        return 90;
    }

    let candidate_tokens = normalized_tokens(candidate);
    let fuzzy_token_overlap = query_tokens
        .iter()
        .filter(|query_token| {
            candidate_tokens
                .iter()
                .any(|candidate_token| within_edit_distance(query_token, candidate_token, 1))
        })
        .count();
    if fuzzy_token_overlap > 0 {
        push_reason(reasons, reason);
        return (fuzzy_token_overlap as i32 * 36).min(84);
    }

    let similarity = trigram_similarity(query_trigrams, &trigrams(candidate));
    if similarity >= 0.6 {
        push_reason(reasons, reason);
        return (similarity * 70.0).round() as i32;
    }

    0
}

fn push_reason(reasons: &mut Vec<String>, reason: &str) {
    if !reasons.iter().any(|existing| existing == reason) {
        reasons.push(reason.to_string());
    }
}

fn normalized_tokens(value: &str) -> Vec<String> {
    let mut normalized = String::new();
    let mut previous: Option<char> = None;
    for ch in value.chars() {
        if ch.is_ascii_alphanumeric() {
            if let Some(previous) = previous {
                let boundary = (previous.is_ascii_lowercase() && ch.is_ascii_uppercase())
                    || (previous.is_ascii_digit() && ch.is_ascii_alphabetic())
                    || (previous.is_ascii_alphabetic() && ch.is_ascii_digit());
                if boundary {
                    normalized.push(' ');
                }
            }
            normalized.push(ch.to_ascii_lowercase());
        } else {
            normalized.push(' ');
        }
        previous = Some(ch);
    }

    normalized
        .split_whitespace()
        .map(ToString::to_string)
        .collect()
}

fn trigrams(value: &str) -> HashSet<String> {
    let compact = value.replace(' ', "");
    let chars = compact.chars().collect::<Vec<_>>();
    if chars.len() < 3 {
        return chars
            .into_iter()
            .map(|ch| ch.to_string())
            .collect::<HashSet<_>>();
    }

    let mut grams = HashSet::new();
    for index in 0..=(chars.len() - 3) {
        grams.insert(chars[index..index + 3].iter().collect::<String>());
    }
    grams
}

fn trigram_similarity(left: &HashSet<String>, right: &HashSet<String>) -> f32 {
    if left.is_empty() || right.is_empty() {
        return 0.0;
    }
    let intersection = left.intersection(right).count() as f32;
    let union = left.union(right).count() as f32;
    if union == 0.0 {
        0.0
    } else {
        intersection / union
    }
}

fn within_edit_distance(left: &str, right: &str, max_distance: usize) -> bool {
    let left = left.chars().collect::<Vec<_>>();
    let right = right.chars().collect::<Vec<_>>();
    let left_len = left.len();
    let right_len = right.len();
    if left_len.abs_diff(right_len) > max_distance {
        return false;
    }

    let mut previous = (0..=right_len).collect::<Vec<_>>();
    let mut current = vec![0; right_len + 1];
    for (left_index, left_char) in left.iter().enumerate() {
        current[0] = left_index + 1;
        let mut row_min = current[0];
        for (right_index, right_char) in right.iter().enumerate() {
            let cost = usize::from(left_char != right_char);
            current[right_index + 1] = (current[right_index] + 1)
                .min(previous[right_index + 1] + 1)
                .min(previous[right_index] + cost);
            row_min = row_min.min(current[right_index + 1]);
        }
        if row_min > max_distance {
            return false;
        }
        std::mem::swap(&mut previous, &mut current);
    }
    previous[right_len] <= max_distance
}
