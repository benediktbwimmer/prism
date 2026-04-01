use std::collections::{HashMap, HashSet};

use prism_ir::{NodeId, NodeKind};
use prism_js::NodeIdView;
use prism_query::{ConceptPacket, Prism};

use crate::next_reads;

const FOLLOWTHROUGH_SEARCH_LIMIT: usize = 6;
const FOLLOWTHROUGH_SUPPORTING_LIMIT: usize = 3;
const FOLLOWTHROUGH_TEST_LIMIT: usize = 2;

#[derive(Debug, Clone, Default)]
pub(crate) struct ConceptFollowthroughTargets {
    pub(crate) inspect_first: Option<NodeId>,
    pub(crate) supporting_reads: Vec<NodeId>,
    pub(crate) likely_tests: Vec<NodeId>,
}

#[derive(Debug, Clone)]
struct ScoredNode {
    node: NodeId,
    score: i32,
}

pub(crate) fn concept_followthrough_targets(
    prism: &Prism,
    packet: &ConceptPacket,
) -> ConceptFollowthroughTargets {
    let general = search_followthrough_candidates(prism, packet, false);
    let tests = search_followthrough_candidates(prism, packet, true);
    let governing_doc = preferred_doc_candidate(&general).map(|candidate| candidate.node.clone());
    let strongest_code = preferred_code_candidate(&general).map(|candidate| candidate.node.clone());

    let inspect_first = governing_doc
        .clone()
        .or_else(|| strongest_code.clone())
        .or_else(|| {
            general
                .iter()
                .find(|candidate| !is_test_like_node(&candidate.node))
                .or_else(|| general.first())
                .map(|candidate| candidate.node.clone())
        });
    let mut seen = HashSet::<String>::new();
    if let Some(primary) = inspect_first.as_ref() {
        seen.insert(primary.path.to_string());
    }

    let mut supporting_reads = Vec::<NodeId>::new();
    if inspect_first.as_ref().is_some_and(is_docs_like_node) {
        if let Some(code) = strongest_code.as_ref() {
            if Some(code) != inspect_first.as_ref() && seen.insert(code.path.to_string()) {
                supporting_reads.push(code.clone());
            }
        }
        for candidate in general
            .iter()
            .filter(|candidate| is_docs_like_node(&candidate.node))
        {
            if Some(&candidate.node) == inspect_first.as_ref()
                || !seen.insert(candidate.node.path.to_string())
            {
                continue;
            }
            supporting_reads.push(candidate.node.clone());
            if supporting_reads.len() >= FOLLOWTHROUGH_SUPPORTING_LIMIT {
                break;
            }
        }
    }
    for candidate in general {
        if supporting_reads.len() >= FOLLOWTHROUGH_SUPPORTING_LIMIT {
            break;
        }
        if Some(&candidate.node) == inspect_first.as_ref()
            || is_test_like_node(&candidate.node)
            || !seen.insert(candidate.node.path.to_string())
        {
            continue;
        }
        supporting_reads.push(candidate.node);
    }
    if supporting_reads.is_empty() {
        if let Some(primary) = inspect_first.as_ref() {
            if let Ok(neighbors) = next_reads(prism, primary, FOLLOWTHROUGH_SEARCH_LIMIT) {
                for neighbor in neighbors {
                    let node = node_id_from_view(&neighbor.symbol.id);
                    if is_test_like_node(&node) || !seen.insert(node.path.to_string()) {
                        continue;
                    }
                    supporting_reads.push(node);
                    if supporting_reads.len() >= FOLLOWTHROUGH_SUPPORTING_LIMIT {
                        break;
                    }
                }
            }
        }
    }

    let likely_tests = tests
        .iter()
        .filter(|candidate| seen.insert(candidate.node.path.to_string()))
        .take(FOLLOWTHROUGH_TEST_LIMIT)
        .map(|candidate| candidate.node.clone())
        .collect();

    ConceptFollowthroughTargets {
        inspect_first,
        supporting_reads,
        likely_tests,
    }
}

fn preferred_doc_candidate(candidates: &[ScoredNode]) -> Option<&ScoredNode> {
    candidates
        .iter()
        .filter(|candidate| is_docs_like_node(&candidate.node))
        .max_by_key(|candidate| {
            (
                doc_continuity_priority(&candidate.node),
                candidate.score,
                std::cmp::Reverse(candidate.node.path.as_str()),
            )
        })
}

fn preferred_code_candidate(candidates: &[ScoredNode]) -> Option<&ScoredNode> {
    candidates.iter().find(|candidate| {
        !is_test_like_node(&candidate.node) && is_code_like_kind(candidate.node.kind)
    })
}

fn search_followthrough_candidates(
    prism: &Prism,
    packet: &ConceptPacket,
    prefer_tests: bool,
) -> Vec<ScoredNode> {
    let concept_tokens = concept_tokens(packet);
    let mut scored = HashMap::<String, ScoredNode>::new();

    for (priority, term) in concept_search_terms(packet).into_iter().enumerate() {
        let path_filter = prefer_tests.then_some("tests/");
        for symbol in prism.search(&term, FOLLOWTHROUGH_SEARCH_LIMIT, None, path_filter) {
            let node = symbol.id().clone();
            let score = followthrough_score(&node, &concept_tokens, &term, priority, prefer_tests);
            if score <= 0 {
                continue;
            }
            upsert_scored(&mut scored, node, score);
        }
        if prefer_tests {
            continue;
        }
        for symbol in prism.search(&term, FOLLOWTHROUGH_SEARCH_LIMIT, None, Some("docs/")) {
            let node = symbol.id().clone();
            let score = followthrough_score(&node, &concept_tokens, &term, priority, false);
            if score <= 0 {
                continue;
            }
            upsert_scored(&mut scored, node, score);
        }
    }
    if !prefer_tests
        && !scored
            .values()
            .any(|candidate| is_docs_like_node(&candidate.node))
    {
        supplement_doc_candidates(prism, packet, &concept_tokens, &mut scored);
    }

    let mut results = scored.into_values().collect::<Vec<_>>();
    results.sort_by(|left, right| {
        right
            .score
            .cmp(&left.score)
            .then_with(|| left.node.path.cmp(&right.node.path))
    });
    results
}

fn supplement_doc_candidates(
    prism: &Prism,
    packet: &ConceptPacket,
    concept_tokens: &HashSet<String>,
    scored: &mut HashMap<String, ScoredNode>,
) {
    for (priority, token) in concept_doc_tokens(packet).into_iter().enumerate() {
        for kind in [NodeKind::MarkdownHeading, NodeKind::Document] {
            for symbol in prism.search(
                &token,
                FOLLOWTHROUGH_SEARCH_LIMIT,
                Some(kind),
                Some("docs/"),
            ) {
                let node = symbol.id().clone();
                let mut score = followthrough_score(&node, concept_tokens, &token, priority, false);
                score += 24;
                if score <= 0 {
                    continue;
                }
                upsert_scored(scored, node, score);
            }
        }
    }
}

fn upsert_scored(scored: &mut HashMap<String, ScoredNode>, node: NodeId, score: i32) {
    scored
        .entry(node.path.to_string())
        .and_modify(|existing| {
            if score > existing.score {
                existing.score = score;
                existing.node = node.clone();
            }
        })
        .or_insert(ScoredNode { node, score });
}

fn concept_search_terms(packet: &ConceptPacket) -> Vec<String> {
    let mut terms = vec![
        packet.canonical_name.clone(),
        packet.handle.trim_start_matches("concept://").to_string(),
    ];
    terms.extend(packet.aliases.iter().cloned());

    let mut expanded = Vec::<String>::new();
    let mut seen = HashSet::<String>::new();
    for term in terms {
        for variant in term_variants(&term) {
            if !variant.trim().is_empty() && seen.insert(variant.clone()) {
                expanded.push(variant);
            }
        }
    }
    expanded
}

fn term_variants(term: &str) -> Vec<String> {
    let trimmed = term.trim();
    if trimmed.is_empty() {
        return Vec::new();
    }
    let spaced = trimmed.replace(['_', '-'], " ");
    let collapsed = normalized_tokens(trimmed).join(" ");
    let mut variants = vec![trimmed.to_string()];
    if spaced != trimmed {
        variants.push(spaced);
    }
    if !collapsed.is_empty() && !variants.iter().any(|existing| existing == &collapsed) {
        variants.push(collapsed);
    }
    variants
}

fn concept_tokens(packet: &ConceptPacket) -> HashSet<String> {
    normalized_tokens(&format!(
        "{} {} {}",
        packet.canonical_name,
        packet.aliases.join(" "),
        packet.summary
    ))
    .into_iter()
    .collect()
}

fn concept_doc_tokens(packet: &ConceptPacket) -> Vec<String> {
    let mut tokens = Vec::new();
    let mut seen = HashSet::<String>::new();
    for term in std::iter::once(packet.canonical_name.as_str())
        .chain(packet.aliases.iter().map(String::as_str))
        .chain(std::iter::once(packet.summary.as_str()))
    {
        for token in normalized_tokens(term) {
            if token.len() < 4 || !seen.insert(token.clone()) {
                continue;
            }
            tokens.push(token);
        }
    }
    tokens
}

fn followthrough_score(
    node: &NodeId,
    concept_tokens: &HashSet<String>,
    query: &str,
    priority: usize,
    prefer_tests: bool,
) -> i32 {
    let path_tokens = normalized_tokens(node.path.as_str());
    let query_tokens = normalized_tokens(query);
    let overlap = path_tokens
        .iter()
        .filter(|token| concept_tokens.contains(*token))
        .count() as i32;
    let query_overlap = query_tokens
        .iter()
        .filter(|token| path_tokens.contains(*token))
        .count() as i32;
    let exact_phrase = path_contains_phrase(node.path.as_str(), query);
    let test_like = is_test_like_node(node);
    let code_like = is_code_like_kind(node.kind);

    let mut score = kind_weight(node.kind);
    score += overlap * 14;
    score += query_overlap * 18;
    if exact_phrase {
        score += 36;
    }
    if priority == 0 {
        score += 18;
    } else if priority == 1 {
        score += 12;
    }
    if prefer_tests {
        score += if test_like { 54 } else { -28 };
    } else if test_like {
        score -= 60;
    }
    if code_like {
        score += 18;
    } else if matches!(node.kind, NodeKind::MarkdownHeading | NodeKind::Document) {
        score += 4;
    } else {
        score -= 12;
    }
    score
}

fn kind_weight(kind: NodeKind) -> i32 {
    match kind {
        NodeKind::Module => 80,
        NodeKind::Method => 72,
        NodeKind::Function => 70,
        NodeKind::Struct | NodeKind::Enum | NodeKind::Trait | NodeKind::TypeAlias => 64,
        NodeKind::Field => 52,
        NodeKind::MarkdownHeading => 36,
        NodeKind::Document => 28,
        NodeKind::Impl => 18,
        NodeKind::Workspace | NodeKind::Package => 8,
        NodeKind::JsonKey | NodeKind::TomlKey | NodeKind::YamlKey => 20,
    }
}

fn is_code_like_kind(kind: NodeKind) -> bool {
    matches!(
        kind,
        NodeKind::Module
            | NodeKind::Function
            | NodeKind::Method
            | NodeKind::Struct
            | NodeKind::Enum
            | NodeKind::Trait
            | NodeKind::Field
            | NodeKind::TypeAlias
    )
}

fn is_docs_like_node(node: &NodeId) -> bool {
    matches!(node.kind, NodeKind::MarkdownHeading | NodeKind::Document)
}

fn doc_continuity_priority(node: &NodeId) -> u8 {
    let path = node.path.to_ascii_lowercase();
    if path.contains("governance") {
        3
    } else if matches!(node.kind, NodeKind::MarkdownHeading) {
        2
    } else if path.contains("spec") {
        1
    } else {
        0
    }
}

fn is_test_like_node(node: &NodeId) -> bool {
    let path = node.path.to_ascii_lowercase();
    path.contains("::tests::")
        || path.contains("_test")
        || path.contains("test_")
        || path.contains("tests::")
}

fn node_id_from_view(view: &NodeIdView) -> NodeId {
    NodeId::new(view.crate_name.clone(), view.path.clone(), view.kind)
}

fn path_contains_phrase(path: &str, query: &str) -> bool {
    let normalized_path = normalized_tokens(path).join(" ");
    let normalized_query = normalized_tokens(query).join(" ");
    !normalized_query.is_empty() && normalized_path.contains(&normalized_query)
}

fn normalized_tokens(value: &str) -> Vec<String> {
    value
        .split(|ch: char| !ch.is_ascii_alphanumeric())
        .filter(|token| !token.is_empty())
        .map(|token| token.to_ascii_lowercase())
        .collect()
}
