use std::collections::{HashMap, HashSet};

use prism_ir::{AnchorRef, Node, NodeId, NodeKind};
use prism_memory::{
    MemoryEntry, MemoryKind, OutcomeEvent, OutcomeEvidence, OutcomeKind, OutcomeResult,
};
use serde_json::Value;

use crate::types::{
    CandidateConcept, CandidateConceptOperation, CandidateMemory, CandidateMemoryEvidence,
    CuratorContext, CuratorDiagnostic, CuratorJob, CuratorProposal, CuratorRun,
};

pub fn synthesize_curator_run(job: &CuratorJob, ctx: &CuratorContext) -> CuratorRun {
    let mut proposals = Vec::new();

    if let Some(proposal) = repeated_failure_validation_rule(job, ctx) {
        proposals.push(proposal);
    }
    if let Some(proposal) = migration_rule(job, ctx) {
        proposals.push(proposal);
    }
    if let Some(proposal) = co_change_rule(job, ctx) {
        proposals.push(proposal);
    }
    if let Some(proposal) = hotspot_concept_rule(job, ctx) {
        proposals.push(proposal);
    }
    if let Some(proposal) = semantic_outcome_summary(job, ctx) {
        proposals.push(proposal);
    }
    proposals.extend(episodic_promotion_rules(job, ctx));

    proposals = dedupe_proposals(proposals);
    if proposals.len() > job.budget.max_proposals {
        proposals.truncate(job.budget.max_proposals);
    }

    CuratorRun {
        proposals,
        diagnostics: Vec::new(),
    }
}

pub fn merge_curator_runs(
    synthesized: CuratorRun,
    backend: Option<CuratorRun>,
    max_proposals: usize,
    backend_error: Option<String>,
) -> CuratorRun {
    let mut proposals = backend
        .as_ref()
        .map(|backend_run| backend_run.proposals.clone())
        .unwrap_or_default();
    proposals.extend(synthesized.proposals);
    proposals = dedupe_proposals(proposals);
    if proposals.len() > max_proposals {
        proposals.truncate(max_proposals);
    }

    let mut diagnostics = synthesized.diagnostics;
    if let Some(backend_run) = backend {
        diagnostics.extend(backend_run.diagnostics);
    }
    if let Some(error) = backend_error {
        diagnostics.push(CuratorDiagnostic {
            code: "backend_error".to_string(),
            message: error,
            data: None,
        });
    }

    CuratorRun {
        proposals,
        diagnostics,
    }
}

fn repeated_failure_validation_rule(
    job: &CuratorJob,
    ctx: &CuratorContext,
) -> Option<CuratorProposal> {
    let failures = failure_outcomes(ctx);
    if failures.len() < 2 {
        return None;
    }

    let mut checks = failure_check_labels(&failures);
    checks.extend(
        ctx.projections
            .validation_checks
            .iter()
            .filter(|check| check.score >= 0.45)
            .map(|check| check.label.clone()),
    );
    sort_dedup_strings(&mut checks);
    if checks.is_empty() {
        return None;
    }

    let content = format!(
        "Changes in this area should run validation: {}",
        checks.join(", ")
    );
    if has_matching_memory(ctx, MemoryKind::Structural, &content) {
        return None;
    }

    let evidence = CandidateMemoryEvidence {
        event_ids: failures.iter().map(|event| event.meta.id.clone()).collect(),
        memory_ids: Vec::new(),
        validation_checks: checks.clone(),
        co_change_lineages: Vec::new(),
    };

    Some(CuratorProposal::StructuralMemory(CandidateMemory {
        anchors: job.focus.clone(),
        kind: MemoryKind::Structural,
        content,
        trust: trust_from_count(failures.len(), 0.72, 0.92),
        rationale: "Repeated failures and validation signals indicate this area needs explicit regression coverage.".to_string(),
        category: Some("validation_rule".to_string()),
        evidence,
    }))
}

fn migration_rule(job: &CuratorJob, ctx: &CuratorContext) -> Option<CuratorProposal> {
    let migration_events = ctx
        .outcomes
        .iter()
        .filter(|event| {
            event.kind == OutcomeKind::MigrationRequired
                || event.summary.to_ascii_lowercase().contains("migration")
        })
        .collect::<Vec<_>>();
    if migration_events.is_empty() {
        return None;
    }

    let content = "Changes in this area require migration planning or rollout review".to_string();
    if has_matching_memory(ctx, MemoryKind::Structural, &content) {
        return None;
    }

    Some(CuratorProposal::StructuralMemory(CandidateMemory {
        anchors: job.focus.clone(),
        kind: MemoryKind::Structural,
        content,
        trust: trust_from_count(migration_events.len(), 0.76, 0.9),
        rationale: "Outcome history includes explicit migration signals, so this should be treated as a standing structural rule.".to_string(),
        category: Some("migration_rule".to_string()),
        evidence: CandidateMemoryEvidence {
            event_ids: migration_events
                .iter()
                .map(|event| event.meta.id.clone())
                .collect(),
            memory_ids: Vec::new(),
            validation_checks: Vec::new(),
            co_change_lineages: Vec::new(),
        },
    }))
}

fn co_change_rule(job: &CuratorJob, ctx: &CuratorContext) -> Option<CuratorProposal> {
    let related = ctx
        .projections
        .co_change
        .iter()
        .filter(|record| record.count >= 2)
        .take(3)
        .cloned()
        .collect::<Vec<_>>();
    if related.is_empty() {
        return None;
    }

    let content =
        "This area frequently co-changes with neighboring lineages and should be reviewed together"
            .to_string();
    if has_matching_memory(ctx, MemoryKind::Structural, &content) {
        return None;
    }

    Some(CuratorProposal::StructuralMemory(CandidateMemory {
        anchors: job.focus.clone(),
        kind: MemoryKind::Structural,
        content,
        trust: trust_from_count(related.len(), 0.68, 0.85),
        rationale: "Projection history shows repeat co-change neighbors, which is better captured as a durable coordination rule than a transient note.".to_string(),
        category: Some("co_change_rule".to_string()),
        evidence: CandidateMemoryEvidence {
            event_ids: Vec::new(),
            memory_ids: Vec::new(),
            validation_checks: Vec::new(),
            co_change_lineages: related.into_iter().map(|record| record.lineage).collect(),
        },
    }))
}

fn semantic_outcome_summary(job: &CuratorJob, ctx: &CuratorContext) -> Option<CuratorProposal> {
    let recent = interesting_outcomes(ctx);
    if recent.len() < 2 {
        return None;
    }

    let mut summaries = recent
        .iter()
        .map(|event| event.summary.trim())
        .filter(|summary| !summary.is_empty())
        .take(3)
        .map(str::to_string)
        .collect::<Vec<_>>();
    sort_dedup_strings(&mut summaries);
    if summaries.is_empty() {
        return None;
    }

    let content = format!("Recent outcome context: {}", summaries.join("; "));
    if has_matching_memory(ctx, MemoryKind::Semantic, &content) {
        return None;
    }

    Some(CuratorProposal::SemanticMemory(CandidateMemory {
        anchors: job.focus.clone(),
        kind: MemoryKind::Semantic,
        content,
        trust: trust_from_count(recent.len(), 0.58, 0.82),
        rationale: "Recent failures and validations around the same focus form reusable fuzzy context worth preserving as semantic memory.".to_string(),
        category: Some("risk_summary".to_string()),
        evidence: CandidateMemoryEvidence {
            event_ids: recent.iter().map(|event| event.meta.id.clone()).collect(),
            memory_ids: Vec::new(),
            validation_checks: failure_check_labels(&recent),
            co_change_lineages: Vec::new(),
        },
    }))
}

fn hotspot_concept_rule(job: &CuratorJob, ctx: &CuratorContext) -> Option<CuratorProposal> {
    if !matches!(job.trigger, crate::CuratorTrigger::HotspotChanged) {
        return None;
    }

    let focus_nodes = focus_nodes(job, ctx);
    if focus_nodes.len() < 2 {
        return None;
    }

    let likely_tests = focus_nodes
        .iter()
        .filter(|node| is_test_like_node(node))
        .map(|node| node.id.clone())
        .collect::<Vec<_>>();
    let core_members = preferred_concept_members(&focus_nodes, 4);
    if core_members.len() < 2 {
        return None;
    }

    let supporting_members = focus_nodes
        .iter()
        .map(|node| node.id.clone())
        .filter(|id| !core_members.contains(id) && !likely_tests.contains(id))
        .take(2)
        .collect::<Vec<_>>();
    let validation_count = ctx
        .projections
        .validation_checks
        .iter()
        .filter(|check| check.score >= 0.45)
        .count();
    let co_change_count = ctx
        .projections
        .co_change
        .iter()
        .filter(|record| record.count >= 2)
        .count();
    let canonical_name = infer_concept_name(&focus_nodes, &ctx.projections.validation_checks);
    let summary = if validation_count > 0 {
        format!(
            "Hotspot-sized cluster around `{canonical_name}` with recurring validation and co-change signals."
        )
    } else {
        format!(
            "Hotspot-sized cluster around `{canonical_name}` observed across multiple focus members."
        )
    };

    let mut evidence = vec![format!(
        "Hotspot change touched {} focus members in this cluster.",
        focus_nodes.len()
    )];
    if co_change_count > 0 {
        evidence.push(format!(
            "Projection history shows {} repeated co-change neighbor(s) for the affected lineages.",
            co_change_count
        ));
    }
    if validation_count > 0 {
        let labels = ctx
            .projections
            .validation_checks
            .iter()
            .filter(|check| check.score >= 0.45)
            .take(3)
            .map(|check| check.label.clone())
            .collect::<Vec<_>>();
        evidence.push(format!(
            "Validation projections repeatedly point at this area: {}.",
            labels.join(", ")
        ));
    }

    let confidence = (0.62
        + (core_members.len().saturating_sub(2) as f32 * 0.05)
        + (validation_count.min(2) as f32 * 0.06)
        + (co_change_count.min(2) as f32 * 0.04))
        .clamp(0.0, 0.9);

    Some(CuratorProposal::ConceptCandidate(CandidateConcept {
        recommended_operation: CandidateConceptOperation::Promote,
        canonical_name: canonical_name.clone(),
        summary,
        aliases: concept_aliases(&canonical_name, &focus_nodes),
        core_members,
        supporting_members,
        likely_tests,
        evidence,
        confidence,
        rationale: "A multi-node hotspot repeatedly changed together and accumulated enough projection evidence to justify a reusable concept proposal.".to_string(),
    }))
}

fn episodic_promotion_rules(job: &CuratorJob, ctx: &CuratorContext) -> Vec<CuratorProposal> {
    let mut promotable = ctx
        .memories
        .iter()
        .filter(|memory| is_promotable_episodic(memory))
        .cloned()
        .collect::<Vec<_>>();
    promotable.sort_by(|left, right| {
        right
            .trust
            .total_cmp(&left.trust)
            .then_with(|| right.created_at.cmp(&left.created_at))
            .then_with(|| left.id.0.cmp(&right.id.0))
    });

    promotable
        .into_iter()
        .filter_map(|memory| episodic_promotion_rule(job, ctx, memory))
        .take(2)
        .collect()
}

fn episodic_promotion_rule(
    job: &CuratorJob,
    ctx: &CuratorContext,
    memory: MemoryEntry,
) -> Option<CuratorProposal> {
    let content = memory.content.trim().to_string();
    if content.is_empty() || has_matching_memory(ctx, MemoryKind::Structural, &content) {
        return None;
    }

    Some(CuratorProposal::StructuralMemory(CandidateMemory {
        anchors: if memory.anchors.is_empty() {
            job.focus.clone()
        } else {
            memory.anchors.clone()
        },
        kind: MemoryKind::Structural,
        content,
        trust: (memory.trust + 0.08).clamp(0.74, 0.96),
        rationale:
            "A high-trust episodic memory in this focus looks durable enough to review as structural knowledge."
                .to_string(),
        category: Some("episodic_promotion".to_string()),
        evidence: CandidateMemoryEvidence {
            event_ids: Vec::new(),
            memory_ids: vec![memory.id],
            validation_checks: Vec::new(),
            co_change_lineages: Vec::new(),
        },
    }))
}

fn failure_outcomes(ctx: &CuratorContext) -> Vec<OutcomeEvent> {
    ctx.outcomes
        .iter()
        .filter(|event| {
            matches!(
                event.kind,
                OutcomeKind::FailureObserved | OutcomeKind::RegressionObserved
            ) || matches!(event.result, OutcomeResult::Failure)
        })
        .cloned()
        .collect()
}

fn interesting_outcomes(ctx: &CuratorContext) -> Vec<OutcomeEvent> {
    let mut outcomes = ctx
        .outcomes
        .iter()
        .filter(|event| {
            matches!(
                event.kind,
                OutcomeKind::FailureObserved
                    | OutcomeKind::RegressionObserved
                    | OutcomeKind::FixValidated
                    | OutcomeKind::MigrationRequired
            ) || matches!(event.result, OutcomeResult::Failure)
        })
        .cloned()
        .collect::<Vec<_>>();
    outcomes.sort_by(|left, right| {
        right
            .meta
            .ts
            .cmp(&left.meta.ts)
            .then_with(|| left.meta.id.0.cmp(&right.meta.id.0))
    });
    outcomes
}

fn failure_check_labels(events: &[OutcomeEvent]) -> Vec<String> {
    let mut checks = Vec::new();
    for event in events {
        for evidence in &event.evidence {
            match evidence {
                OutcomeEvidence::Test { name, passed } if !passed => {
                    checks.push(format!("test:{name}"));
                }
                OutcomeEvidence::Build { target, passed } if !passed => {
                    checks.push(format!("build:{target}"));
                }
                _ => {}
            }
        }
    }
    sort_dedup_strings(&mut checks);
    checks
}

fn is_promotable_episodic(memory: &MemoryEntry) -> bool {
    memory.kind == MemoryKind::Episodic
        && memory.trust >= 0.72
        && !memory.content.trim().is_empty()
        && !memory.anchors.is_empty()
        && !is_task_summary_memory(memory)
}

fn is_task_summary_memory(memory: &MemoryEntry) -> bool {
    let provenance = memory.metadata.get("provenance");
    metadata_string(provenance.and_then(|value| value.get("origin"))) == Some("task_journal")
        || metadata_string(provenance.and_then(|value| value.get("kind"))) == Some("task_summary")
        || memory
            .metadata
            .get("taskLifecycle")
            .and_then(|value| value.get("closed"))
            .and_then(Value::as_bool)
            .unwrap_or(false)
}

fn metadata_string(value: Option<&Value>) -> Option<&str> {
    value.and_then(Value::as_str)
}

fn has_matching_memory(ctx: &CuratorContext, kind: MemoryKind, content: &str) -> bool {
    ctx.memories
        .iter()
        .any(|memory| memory.kind == kind && memory.content.eq_ignore_ascii_case(content))
}

fn focus_nodes<'a>(job: &CuratorJob, ctx: &'a CuratorContext) -> Vec<&'a Node> {
    let by_id = ctx
        .graph
        .nodes
        .iter()
        .map(|node| (node.id.clone(), node))
        .collect::<HashMap<_, _>>();
    job.focus
        .iter()
        .filter_map(|anchor| match anchor {
            AnchorRef::Node(id) => by_id.get(id).copied(),
            _ => None,
        })
        .collect()
}

fn preferred_concept_members(nodes: &[&Node], limit: usize) -> Vec<NodeId> {
    let mut preferred = nodes
        .iter()
        .filter(|node| is_preferred_concept_kind(node.kind))
        .map(|node| node.id.clone())
        .collect::<Vec<_>>();
    if preferred.len() < 2 {
        preferred = nodes
            .iter()
            .map(|node| node.id.clone())
            .filter(|id| !matches!(id.kind, NodeKind::Workspace | NodeKind::Package))
            .collect();
    }
    preferred.truncate(limit);
    preferred
}

fn is_preferred_concept_kind(kind: NodeKind) -> bool {
    matches!(
        kind,
        NodeKind::Function
            | NodeKind::Struct
            | NodeKind::Enum
            | NodeKind::Trait
            | NodeKind::Impl
            | NodeKind::Method
            | NodeKind::TypeAlias
            | NodeKind::Module
    )
}

fn is_test_like_node(node: &Node) -> bool {
    let name = node.name.to_ascii_lowercase();
    let path = node.id.path.to_string().to_ascii_lowercase();
    name.contains("test")
        || path.contains("test")
        || path.contains("spec")
        || path.contains("bench")
}

fn infer_concept_name(nodes: &[&Node], checks: &[prism_projections::ValidationCheck]) -> String {
    let mut counts = HashMap::<String, usize>::new();
    for node in nodes {
        for token in path_tokens(node.id.path.as_str()) {
            *counts.entry(token).or_insert(0) += 1;
        }
        for token in path_tokens(&node.name) {
            *counts.entry(token).or_insert(0) += 1;
        }
    }
    for check in checks {
        for token in path_tokens(&check.label) {
            *counts.entry(token).or_insert(0) += 1;
        }
    }
    counts
        .into_iter()
        .filter(|(token, _)| !is_generic_concept_token(token))
        .max_by(|left, right| left.1.cmp(&right.1).then_with(|| right.0.cmp(&left.0)))
        .map(|(token, _)| token)
        .unwrap_or_else(|| fallback_concept_name(nodes))
}

fn fallback_concept_name(nodes: &[&Node]) -> String {
    nodes
        .first()
        .map(|node| sanitize_concept_token(&node.name))
        .filter(|value| !value.is_empty())
        .unwrap_or_else(|| "hotspot_cluster".to_string())
}

fn concept_aliases(canonical_name: &str, nodes: &[&Node]) -> Vec<String> {
    let mut aliases = vec![canonical_name.replace('_', " ")];
    aliases.extend(
        nodes
            .iter()
            .map(|node| sanitize_concept_token(&node.name))
            .filter(|value| !value.is_empty() && value != canonical_name)
            .take(2),
    );
    sort_dedup_strings(&mut aliases);
    aliases.retain(|alias| !alias.is_empty());
    aliases
}

fn path_tokens(value: &str) -> Vec<String> {
    value
        .split(|ch: char| !ch.is_ascii_alphanumeric())
        .map(sanitize_concept_token)
        .filter(|token| token.len() >= 3)
        .collect()
}

fn sanitize_concept_token(value: &str) -> String {
    value
        .chars()
        .map(|ch| {
            if ch.is_ascii_alphanumeric() {
                ch.to_ascii_lowercase()
            } else {
                '_'
            }
        })
        .collect::<String>()
        .trim_matches('_')
        .split('_')
        .filter(|segment| !segment.is_empty())
        .collect::<Vec<_>>()
        .join("_")
}

fn is_generic_concept_token(token: &str) -> bool {
    matches!(
        token,
        "demo"
            | "src"
            | "lib"
            | "main"
            | "mod"
            | "tests"
            | "test"
            | "spec"
            | "function"
            | "method"
            | "module"
    )
}

fn dedupe_proposals(proposals: Vec<CuratorProposal>) -> Vec<CuratorProposal> {
    let mut seen = HashSet::new();
    let mut deduped = Vec::new();
    for proposal in proposals {
        let key = proposal_key(&proposal);
        if seen.insert(key) {
            deduped.push(proposal);
        }
    }
    deduped
}

fn proposal_key(proposal: &CuratorProposal) -> String {
    match proposal {
        CuratorProposal::StructuralMemory(candidate) => format!(
            "structural:{}:{}",
            anchor_key(&candidate.anchors),
            candidate.content
        ),
        CuratorProposal::SemanticMemory(candidate) => format!(
            "semantic:{}:{}",
            anchor_key(&candidate.anchors),
            candidate.content
        ),
        CuratorProposal::RiskSummary(candidate) => format!(
            "risk:{}:{}",
            anchor_key(&candidate.anchors),
            candidate.summary
        ),
        CuratorProposal::ValidationRecipe(candidate) => {
            format!(
                "validation:{}:{}",
                candidate.target.path,
                candidate.checks.join("|")
            )
        }
        CuratorProposal::InferredEdge(candidate) => format!(
            "edge:{:?}:{}:{}",
            candidate.edge.kind, candidate.edge.source.path, candidate.edge.target.path
        ),
        CuratorProposal::ConceptCandidate(candidate) => format!(
            "concept:{}:{}",
            candidate.canonical_name,
            candidate
                .core_members
                .iter()
                .map(|member| member.path.as_str())
                .collect::<Vec<_>>()
                .join("|")
        ),
    }
}

fn anchor_key(anchors: &[AnchorRef]) -> String {
    let mut keys = anchors
        .iter()
        .map(|anchor| format!("{anchor:?}"))
        .collect::<Vec<_>>();
    keys.sort();
    keys.join("|")
}

fn sort_dedup_strings(values: &mut Vec<String>) {
    values.sort();
    values.dedup();
}

fn trust_from_count(count: usize, floor: f32, ceiling: f32) -> f32 {
    let boost = (count.saturating_sub(1) as f32 * 0.08).min(ceiling - floor);
    (floor + boost).clamp(0.0, 1.0)
}
