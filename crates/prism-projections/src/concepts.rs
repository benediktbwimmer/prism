use std::collections::HashMap;

use prism_ir::{LineageId, NodeId, NodeKind};

use crate::types::{
    CoChangeRecord, ConceptDecodeLens, ConceptEvent, ConceptPacket, ValidationCheck,
};

const CORE_MEMBER_LIMIT: usize = 4;
const SUPPORTING_MEMBER_LIMIT: usize = 4;
const LIKELY_TEST_LIMIT: usize = 2;

struct ConceptDefinition {
    handle_slug: &'static str,
    canonical_name: &'static str,
    summary: &'static str,
    aliases: &'static [&'static str],
    member_terms: &'static [&'static str],
    preferred_terms: &'static [&'static str],
    risk_hint: Option<&'static str>,
    decode_lenses: &'static [ConceptDecodeLens],
}

#[derive(Debug, Clone)]
struct RankedNode {
    id: NodeId,
    score: i32,
}

const DEFAULT_LENSES: &[ConceptDecodeLens] = &[
    ConceptDecodeLens::Open,
    ConceptDecodeLens::Workset,
    ConceptDecodeLens::Validation,
    ConceptDecodeLens::Timeline,
    ConceptDecodeLens::Memory,
];

const CONCEPT_DEFINITIONS: &[ConceptDefinition] = &[
    ConceptDefinition {
        handle_slug: "validation_pipeline",
        canonical_name: "validation_pipeline",
        summary: "Validation checks, likely tests, risk signals, and recent failures behind a change.",
        aliases: &["validation", "checks", "likely tests", "what to run"],
        member_terms: &["validation", "check", "test", "risk", "impact", "failure"],
        preferred_terms: &["validation_recipe", "validation_context", "task_validation_recipe"],
        risk_hint: Some("Validation-related drift tends to show up as missing checks or stale expectations."),
        decode_lenses: DEFAULT_LENSES,
    },
    ConceptDefinition {
        handle_slug: "session_lifecycle",
        canonical_name: "session_lifecycle",
        summary: "Session state, current task context, resumption paths, and task-local continuity.",
        aliases: &["session", "current task", "resume", "task continuity"],
        member_terms: &["session", "resume", "task", "current", "state"],
        preferred_terms: &["session_state", "prism_session", "task_journal", "resume_task"],
        risk_hint: Some("Session continuity depends on keeping task metadata and anchors aligned."),
        decode_lenses: DEFAULT_LENSES,
    },
    ConceptDefinition {
        handle_slug: "runtime_surface",
        canonical_name: "runtime_surface",
        summary: "Daemon status, runtime logs, health checks, and connection state for live Prism use.",
        aliases: &["runtime", "status", "daemon", "health"],
        member_terms: &["runtime", "status", "daemon", "health", "connection"],
        preferred_terms: &["runtime_status", "runtime_logs", "runtime_timeline", "connection_info"],
        risk_hint: Some("Runtime issues usually propagate through daemon health, bridge connection, or stale refresh state."),
        decode_lenses: DEFAULT_LENSES,
    },
    ConceptDefinition {
        handle_slug: "memory_system",
        canonical_name: "memory_system",
        summary: "Episodic, structural, and semantic memory plus outcomes and recall paths.",
        aliases: &["memory", "outcomes", "lessons", "recall"],
        member_terms: &["memory", "outcome", "recall", "episodic", "structural", "semantic"],
        preferred_terms: &["SessionMemory", "OutcomeMemory", "memory.recall", "semantic_memory"],
        risk_hint: Some("Memory quality depends on durable anchors and clean separation from authority."),
        decode_lenses: DEFAULT_LENSES,
    },
    ConceptDefinition {
        handle_slug: "compact_tools",
        canonical_name: "compact_tools",
        summary: "Locate, open, workset, expand, and other compact staged agent entrypoints.",
        aliases: &["compact tools", "locate", "open", "workset", "expand"],
        member_terms: &["compact", "locate", "open", "workset", "expand", "task_brief"],
        preferred_terms: &["prism_locate", "prism_open", "prism_workset", "prism_expand"],
        risk_hint: Some("Compact-tool quality depends on first-hop ranking and bounded decode ergonomics."),
        decode_lenses: DEFAULT_LENSES,
    },
    ConceptDefinition {
        handle_slug: "task_continuity",
        canonical_name: "task_continuity",
        summary: "Coordination tasks, claims, artifacts, handoffs, and resumable task history.",
        aliases: &["task continuity", "coordination", "claims", "handoff", "artifact"],
        member_terms: &["task", "coordination", "claim", "artifact", "handoff", "plan"],
        preferred_terms: &["task_context", "coordination", "claim", "artifact", "task_journal"],
        risk_hint: Some("Task continuity breaks when anchors, revisions, or validation state drift apart."),
        decode_lenses: DEFAULT_LENSES,
    },
];

pub(crate) fn derive_concept_packets(
    node_to_lineage: &HashMap<NodeId, LineageId>,
    validation_by_lineage: &HashMap<LineageId, Vec<ValidationCheck>>,
    co_change_by_lineage: &HashMap<LineageId, Vec<CoChangeRecord>>,
) -> Vec<ConceptPacket> {
    let mut packets = CONCEPT_DEFINITIONS
        .iter()
        .filter_map(|definition| {
            derive_packet(
                definition,
                node_to_lineage,
                validation_by_lineage,
                co_change_by_lineage,
            )
        })
        .collect::<Vec<_>>();
    packets.sort_by(|left, right| left.handle.cmp(&right.handle));
    packets
}

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

pub(crate) fn merge_concept_packets(
    derived: Vec<ConceptPacket>,
    curated: &[ConceptPacket],
) -> Vec<ConceptPacket> {
    let mut merged = derived
        .into_iter()
        .map(|concept| (normalize_handle(&concept.handle), concept))
        .collect::<HashMap<_, _>>();
    for concept in curated {
        merged.insert(normalize_handle(&concept.handle), concept.clone());
    }
    let mut packets = merged.into_values().collect::<Vec<_>>();
    packets.sort_by(|left, right| left.handle.cmp(&right.handle));
    packets
}

pub fn curated_concepts_from_events(events: &[ConceptEvent]) -> Vec<ConceptPacket> {
    let mut concepts = HashMap::<String, ConceptPacket>::new();
    for event in events {
        concepts.insert(normalize_handle(&event.concept.handle), event.concept.clone());
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

fn derive_packet(
    definition: &ConceptDefinition,
    node_to_lineage: &HashMap<NodeId, LineageId>,
    validation_by_lineage: &HashMap<LineageId, Vec<ValidationCheck>>,
    co_change_by_lineage: &HashMap<LineageId, Vec<CoChangeRecord>>,
) -> Option<ConceptPacket> {
    let mut ranked = node_to_lineage
        .keys()
        .filter_map(|node| {
            let score = node_score(
                definition,
                node,
                node_to_lineage,
                validation_by_lineage,
                co_change_by_lineage,
            );
            (score > 0).then(|| RankedNode {
                id: node.clone(),
                score,
            })
        })
        .collect::<Vec<_>>();
    ranked.sort_by(|left, right| {
        right
            .score
            .cmp(&left.score)
            .then_with(|| left.id.path.cmp(&right.id.path))
    });

    let mut core_members = Vec::new();
    let mut supporting_members = Vec::new();
    let mut likely_tests = Vec::new();
    for candidate in ranked {
        if is_test_like(&candidate.id) {
            if likely_tests.len() < LIKELY_TEST_LIMIT {
                likely_tests.push(candidate.id);
            }
            continue;
        }
        if core_members.len() < CORE_MEMBER_LIMIT {
            core_members.push(candidate.id);
            continue;
        }
        if supporting_members.len() < SUPPORTING_MEMBER_LIMIT {
            supporting_members.push(candidate.id);
        }
        if core_members.len() >= CORE_MEMBER_LIMIT
            && supporting_members.len() >= SUPPORTING_MEMBER_LIMIT
        {
            break;
        }
    }

    if core_members.is_empty() {
        return None;
    }

    let matched_lineages = core_members
        .iter()
        .filter_map(|node| node_to_lineage.get(node))
        .collect::<Vec<_>>();
    let validation_hits = matched_lineages
        .iter()
        .filter_map(|lineage| validation_by_lineage.get(*lineage))
        .map(Vec::len)
        .sum::<usize>();
    let neighbor_hits = matched_lineages
        .iter()
        .filter_map(|lineage| co_change_by_lineage.get(*lineage))
        .map(Vec::len)
        .sum::<usize>();
    let confidence = (0.3
        + (core_members.len().min(CORE_MEMBER_LIMIT) as f32 * 0.12)
        + (supporting_members.len().min(SUPPORTING_MEMBER_LIMIT) as f32 * 0.05)
        + (validation_hits.min(4) as f32 * 0.04)
        + (neighbor_hits.min(4) as f32 * 0.03))
        .min(0.98);

    let mut evidence = vec![format!(
        "{} core members matched repo terms for `{}`.",
        core_members.len(),
        definition.canonical_name
    )];
    if !supporting_members.is_empty() {
        evidence.push(format!(
            "{} supporting members reinforced the same concept cluster.",
            supporting_members.len()
        ));
    }
    if validation_hits > 0 {
        evidence.push(format!(
            "{} validation signals attach to the concept's current lineages.",
            validation_hits
        ));
    }
    if neighbor_hits > 0 {
        evidence.push(format!(
            "{} co-change neighbors reinforce the concept boundary.",
            neighbor_hits
        ));
    }

    Some(ConceptPacket {
        handle: format!("concept://{}", definition.handle_slug),
        canonical_name: definition.canonical_name.to_string(),
        summary: definition.summary.to_string(),
        aliases: definition
            .aliases
            .iter()
            .map(|alias| (*alias).to_string())
            .collect(),
        confidence,
        core_members,
        supporting_members,
        likely_tests,
        evidence,
        risk_hint: definition.risk_hint.map(str::to_string),
        decode_lenses: definition.decode_lenses.to_vec(),
    })
}

fn node_score(
    definition: &ConceptDefinition,
    node: &NodeId,
    node_to_lineage: &HashMap<NodeId, LineageId>,
    validation_by_lineage: &HashMap<LineageId, Vec<ValidationCheck>>,
    co_change_by_lineage: &HashMap<LineageId, Vec<CoChangeRecord>>,
) -> i32 {
    let haystack = normalize_text(node.path.as_str());
    let mut score = 0;

    for alias in definition.aliases {
        score += phrase_score(&haystack, &normalize_text(alias), 60, 28);
    }
    score += phrase_score(
        &haystack,
        &normalize_text(definition.canonical_name),
        80,
        36,
    );
    for term in definition.member_terms {
        score += phrase_score(&haystack, &normalize_text(term), 26, 10);
    }
    for term in definition.preferred_terms {
        score += phrase_score(&haystack, &normalize_text(term), 52, 18);
    }

    score += match node.kind {
        NodeKind::Function | NodeKind::Method => 14,
        NodeKind::Struct | NodeKind::Enum | NodeKind::Trait | NodeKind::Module => 10,
        NodeKind::MarkdownHeading | NodeKind::Document => 8,
        _ => 0,
    };

    if is_test_like(node) {
        score -= 12;
    }

    if let Some(lineage) = node_to_lineage.get(node) {
        if validation_by_lineage
            .get(lineage)
            .is_some_and(|checks| !checks.is_empty())
        {
            score += 8;
        }
        if co_change_by_lineage
            .get(lineage)
            .is_some_and(|neighbors| !neighbors.is_empty())
        {
            score += 6;
        }
    }

    score
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

fn is_test_like(node: &NodeId) -> bool {
    let path = node.path.as_str().to_ascii_lowercase();
    path.contains("::tests::")
        || path.ends_with("_test")
        || path.ends_with("_tests")
        || path.contains("integration_test")
        || path.contains("validation_test")
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
