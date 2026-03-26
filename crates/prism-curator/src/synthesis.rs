use std::collections::HashSet;

use prism_ir::AnchorRef;
use prism_memory::{MemoryKind, OutcomeEvent, OutcomeEvidence, OutcomeKind, OutcomeResult};

use crate::types::{
    CandidateMemory, CandidateMemoryEvidence, CuratorContext, CuratorDiagnostic, CuratorJob,
    CuratorProposal, CuratorRun,
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
    if let Some(proposal) = semantic_outcome_summary(job, ctx) {
        proposals.push(proposal);
    }

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
            validation_checks: failure_check_labels(&recent),
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

fn has_matching_memory(ctx: &CuratorContext, kind: MemoryKind, content: &str) -> bool {
    ctx.memories
        .iter()
        .any(|memory| memory.kind == kind && memory.content.eq_ignore_ascii_case(content))
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
