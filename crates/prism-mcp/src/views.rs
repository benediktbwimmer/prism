use anyhow::{anyhow, Result};
use prism_curator::{
    CuratorJobRecord, CuratorProposal, CuratorProposalDisposition, CuratorTrigger,
};
use prism_ir::{AnchorRef, Edge, NodeId, WorkspaceRevision};
use prism_js::{
    ArtifactRiskView, ArtifactView, BlockerView, ChangeImpactView, ClaimView, CoChangeView,
    ConceptBindingMetadataView, ConceptDecodeLensView, ConceptPacketView, ConceptProvenanceView,
    ConceptPublicationStatusView, ConceptPublicationView, ConceptResolutionView, ConceptScopeView,
    ConflictView, CoordinationTaskView, CuratorJobView, CuratorProposalRecordView,
    CuratorProposalView, DriftCandidateView, EdgeView, MemoryEntryView, MemoryEventView,
    NodeIdView, PlanAcceptanceCriterionView, PlanBindingView, PlanEdgeView,
    PlanExecutionOverlayView, PlanGraphView, PlanNodeView, PlanView, PolicyViolationRecordView,
    PolicyViolationView, QueryDiagnostic, ScoredMemoryView, TaskIntentView, TaskRiskView,
    TaskValidationRecipeView, ValidationCheckView, ValidationRecipeView, ValidationRefView,
    WorkspaceRevisionView,
};
use prism_memory::{MemoryEntry, MemoryEvent, MemorySource, ScoredMemory};
use prism_query::{
    ArtifactRisk, ChangeImpact, CoChange, ConceptDecodeLens, ConceptPacket, ConceptProvenance,
    ConceptPublication, ConceptPublicationStatus, ConceptResolution, ConceptScope, DriftCandidate,
    Prism, TaskIntent, TaskRisk, TaskValidationRecipe, ValidationCheck, ValidationRecipe,
};
use serde_json::Value;

use crate::{normalize_query_diagnostic, InferredEdgeRecordView, SessionState};

pub(crate) fn curator_job_view(record: CuratorJobRecord) -> Result<CuratorJobView> {
    let id = record.id.0.clone();
    let trigger = curator_trigger_label(&record.job.trigger).to_owned();
    let status = curator_job_status_label(&record).to_owned();
    let task_id = record.job.task.as_ref().map(|task| task.0.to_string());
    let run = record.run.clone().unwrap_or_default();
    let mut proposals = Vec::with_capacity(run.proposals.len());
    for (index, proposal) in run.proposals.into_iter().enumerate() {
        let state = record
            .proposal_states
            .get(index)
            .cloned()
            .unwrap_or_default();
        proposals.push(curator_proposal_view(index, proposal, state)?);
    }
    Ok(CuratorJobView {
        id,
        trigger,
        status,
        task_id,
        focus: record.job.focus,
        created_at: record.created_at,
        started_at: record.started_at,
        finished_at: record.finished_at,
        proposals,
        diagnostics: run
            .diagnostics
            .into_iter()
            .map(|diagnostic| {
                normalize_query_diagnostic(QueryDiagnostic {
                    code: diagnostic.code,
                    message: diagnostic.message,
                    data: diagnostic.data,
                })
            })
            .collect(),
        error: record.error,
    })
}

pub(crate) fn curator_proposal_view(
    index: usize,
    proposal: CuratorProposal,
    state: prism_curator::CuratorProposalState,
) -> Result<CuratorProposalView> {
    let (kind, payload) = match proposal {
        CuratorProposal::InferredEdge(candidate) => {
            ("inferred_edge", serde_json::to_value(candidate)?)
        }
        CuratorProposal::StructuralMemory(candidate) => {
            ("structural_memory", serde_json::to_value(candidate)?)
        }
        CuratorProposal::SemanticMemory(candidate) => {
            ("semantic_memory", serde_json::to_value(candidate)?)
        }
        CuratorProposal::RiskSummary(candidate) => {
            ("risk_summary", serde_json::to_value(candidate)?)
        }
        CuratorProposal::ValidationRecipe(candidate) => {
            ("validation_recipe", serde_json::to_value(candidate)?)
        }
    };
    Ok(CuratorProposalView {
        index,
        kind: kind.to_owned(),
        disposition: curator_disposition_label(state.disposition).to_owned(),
        payload,
        decided_at: state.decided_at,
        task_id: state.task.map(|task| task.0.to_string()),
        note: state.note,
        output: state.output,
    })
}

pub(crate) fn curator_proposal_record_view(
    record: &CuratorJobRecord,
    index: usize,
    proposal: CuratorProposal,
    state: prism_curator::CuratorProposalState,
) -> Result<CuratorProposalRecordView> {
    let proposal = curator_proposal_view(index, proposal, state)?;
    Ok(CuratorProposalRecordView {
        job_id: record.id.0.clone(),
        job_trigger: curator_trigger_label(&record.job.trigger).to_owned(),
        job_status: curator_job_status_label(record).to_owned(),
        job_task_id: record.job.task.as_ref().map(|task| task.0.to_string()),
        focus: record.job.focus.clone(),
        job_created_at: record.created_at,
        job_started_at: record.started_at,
        job_finished_at: record.finished_at,
        index: proposal.index,
        kind: proposal.kind,
        disposition: proposal.disposition,
        payload: proposal.payload,
        decided_at: proposal.decided_at,
        proposal_task_id: proposal.task_id,
        note: proposal.note,
        output: proposal.output,
    })
}

pub(crate) fn curator_job_status_label(record: &CuratorJobRecord) -> &'static str {
    match record.status {
        prism_curator::CuratorJobStatus::Queued => "queued",
        prism_curator::CuratorJobStatus::Running => "running",
        prism_curator::CuratorJobStatus::Completed => "completed",
        prism_curator::CuratorJobStatus::Failed => "failed",
        prism_curator::CuratorJobStatus::Skipped => "skipped",
    }
}

pub(crate) fn curator_trigger_label(trigger: &CuratorTrigger) -> &'static str {
    match trigger {
        CuratorTrigger::Manual => "manual",
        CuratorTrigger::PostChange => "post_change",
        CuratorTrigger::TaskCompleted => "task_completed",
        CuratorTrigger::RepeatedFailure => "repeated_failure",
        CuratorTrigger::AmbiguousLineage => "ambiguous_lineage",
        CuratorTrigger::HotspotChanged => "hotspot_changed",
    }
}

pub(crate) fn curator_disposition_label(disposition: CuratorProposalDisposition) -> &'static str {
    match disposition {
        CuratorProposalDisposition::Pending => "pending",
        CuratorProposalDisposition::Applied => "applied",
        CuratorProposalDisposition::Rejected => "rejected",
    }
}

pub(crate) fn curator_proposal_state(
    record: &CuratorJobRecord,
    proposal_index: usize,
) -> Result<prism_curator::CuratorProposalState> {
    if record
        .run
        .as_ref()
        .and_then(|run| run.proposals.get(proposal_index))
        .is_none()
    {
        return Err(anyhow!("unknown curator proposal index {proposal_index}"));
    }
    Ok(record
        .proposal_states
        .get(proposal_index)
        .cloned()
        .unwrap_or_default())
}

pub(crate) fn curator_proposal(
    record: &CuratorJobRecord,
    proposal_index: usize,
) -> Result<&CuratorProposal> {
    record
        .run
        .as_ref()
        .and_then(|run| run.proposals.get(proposal_index))
        .ok_or_else(|| anyhow!("unknown curator proposal index {proposal_index}"))
}

pub(crate) fn change_impact_view(impact: ChangeImpact) -> ChangeImpactView {
    ChangeImpactView {
        direct_nodes: impact.direct_nodes.into_iter().map(node_id_view).collect(),
        lineages: impact
            .lineages
            .into_iter()
            .map(|lineage| lineage.0.to_string())
            .collect(),
        likely_validations: impact.likely_validations,
        validation_checks: impact
            .validation_checks
            .into_iter()
            .map(validation_check_view)
            .collect(),
        co_change_neighbors: impact
            .co_change_neighbors
            .into_iter()
            .map(co_change_view)
            .collect(),
        risk_events: impact.risk_events,
        promoted_summaries: Vec::new(),
    }
}

pub(crate) fn validation_recipe_view(recipe: ValidationRecipe) -> ValidationRecipeView {
    ValidationRecipeView {
        target: node_id_view(recipe.target),
        checks: recipe.checks,
        scored_checks: recipe
            .scored_checks
            .into_iter()
            .map(validation_check_view)
            .collect(),
        related_nodes: recipe.related_nodes.into_iter().map(node_id_view).collect(),
        co_change_neighbors: recipe
            .co_change_neighbors
            .into_iter()
            .map(co_change_view)
            .collect(),
        recent_failures: recipe.recent_failures,
    }
}

pub(crate) fn concept_decode_lens_view(lens: ConceptDecodeLens) -> ConceptDecodeLensView {
    match lens {
        ConceptDecodeLens::Open => ConceptDecodeLensView::Open,
        ConceptDecodeLens::Workset => ConceptDecodeLensView::Workset,
        ConceptDecodeLens::Validation => ConceptDecodeLensView::Validation,
        ConceptDecodeLens::Timeline => ConceptDecodeLensView::Timeline,
        ConceptDecodeLens::Memory => ConceptDecodeLensView::Memory,
    }
}

pub(crate) fn concept_packet_view(
    packet: ConceptPacket,
    include_binding_metadata: bool,
    resolution: Option<ConceptResolution>,
) -> ConceptPacketView {
    ConceptPacketView {
        handle: packet.handle,
        canonical_name: packet.canonical_name,
        summary: packet.summary,
        aliases: packet.aliases,
        confidence: packet.confidence,
        core_members: packet.core_members.into_iter().map(node_id_view).collect(),
        supporting_members: packet
            .supporting_members
            .into_iter()
            .map(node_id_view)
            .collect(),
        likely_tests: packet.likely_tests.into_iter().map(node_id_view).collect(),
        evidence: packet.evidence,
        risk_hint: packet.risk_hint,
        decode_lenses: packet
            .decode_lenses
            .into_iter()
            .map(concept_decode_lens_view)
            .collect(),
        scope: concept_scope_view(packet.scope),
        provenance: concept_provenance_view(packet.provenance),
        publication: packet.publication.map(concept_publication_view),
        resolution: resolution.map(concept_resolution_view),
        binding_metadata: include_binding_metadata.then(|| ConceptBindingMetadataView {
            core_member_lineages: packet
                .core_member_lineages
                .into_iter()
                .map(|lineage| lineage.map(|lineage| lineage.0.to_string()))
                .collect(),
            supporting_member_lineages: packet
                .supporting_member_lineages
                .into_iter()
                .map(|lineage| lineage.map(|lineage| lineage.0.to_string()))
                .collect(),
            likely_test_lineages: packet
                .likely_test_lineages
                .into_iter()
                .map(|lineage| lineage.map(|lineage| lineage.0.to_string()))
                .collect(),
        }),
    }
}

pub(crate) fn concept_resolution_view(resolution: ConceptResolution) -> ConceptResolutionView {
    ConceptResolutionView {
        score: resolution.score,
        reasons: resolution.reasons,
    }
}

fn concept_scope_view(scope: ConceptScope) -> ConceptScopeView {
    match scope {
        ConceptScope::Local => ConceptScopeView::Local,
        ConceptScope::Session => ConceptScopeView::Session,
        ConceptScope::Repo => ConceptScopeView::Repo,
    }
}

fn concept_provenance_view(provenance: ConceptProvenance) -> ConceptProvenanceView {
    ConceptProvenanceView {
        origin: provenance.origin,
        kind: provenance.kind,
        task_id: provenance.task_id,
    }
}

fn concept_publication_status_view(
    status: ConceptPublicationStatus,
) -> ConceptPublicationStatusView {
    match status {
        ConceptPublicationStatus::Active => ConceptPublicationStatusView::Active,
        ConceptPublicationStatus::Retired => ConceptPublicationStatusView::Retired,
    }
}

fn concept_publication_view(publication: ConceptPublication) -> ConceptPublicationView {
    ConceptPublicationView {
        published_at: publication.published_at,
        last_reviewed_at: publication.last_reviewed_at,
        status: concept_publication_status_view(publication.status),
        supersedes: publication.supersedes,
        retired_at: publication.retired_at,
        retirement_reason: publication.retirement_reason,
    }
}

pub(crate) fn task_validation_recipe_view(
    recipe: TaskValidationRecipe,
) -> TaskValidationRecipeView {
    TaskValidationRecipeView {
        task_id: recipe.task_id.0.to_string(),
        checks: recipe.checks,
        scored_checks: recipe
            .scored_checks
            .into_iter()
            .map(validation_check_view)
            .collect(),
        related_nodes: recipe.related_nodes.into_iter().map(node_id_view).collect(),
        co_change_neighbors: recipe
            .co_change_neighbors
            .into_iter()
            .map(co_change_view)
            .collect(),
        recent_failures: recipe.recent_failures,
    }
}

pub(crate) fn task_risk_view(value: TaskRisk, promoted_summaries: Vec<String>) -> TaskRiskView {
    TaskRiskView {
        task_id: value.task_id.0.to_string(),
        risk_score: value.risk_score,
        review_required: value.review_required,
        stale_task: value.stale_task,
        has_approved_artifact: value.has_approved_artifact,
        likely_validations: value.likely_validations,
        missing_validations: value.missing_validations,
        validation_checks: value
            .validation_checks
            .into_iter()
            .map(validation_check_view)
            .collect(),
        co_change_neighbors: value
            .co_change_neighbors
            .into_iter()
            .map(co_change_view)
            .collect(),
        risk_events: value.risk_events,
        promoted_summaries,
        approved_artifact_ids: value
            .approved_artifact_ids
            .into_iter()
            .map(|artifact_id| artifact_id.0.to_string())
            .collect(),
        stale_artifact_ids: value
            .stale_artifact_ids
            .into_iter()
            .map(|artifact_id| artifact_id.0.to_string())
            .collect(),
    }
}

pub(crate) fn artifact_risk_view(
    value: ArtifactRisk,
    promoted_summaries: Vec<String>,
) -> ArtifactRiskView {
    ArtifactRiskView {
        artifact_id: value.artifact_id.0.to_string(),
        task_id: value.task_id.0.to_string(),
        risk_score: value.risk_score,
        review_required: value.review_required,
        stale: value.stale,
        required_validations: value.required_validations,
        validated_checks: value.validated_checks,
        missing_validations: value.missing_validations,
        co_change_neighbors: value
            .co_change_neighbors
            .into_iter()
            .map(co_change_view)
            .collect(),
        risk_events: value.risk_events,
        promoted_summaries,
    }
}

pub(crate) fn drift_candidate_view(value: DriftCandidate) -> DriftCandidateView {
    DriftCandidateView {
        spec: node_id_view(value.spec),
        implementations: value
            .implementations
            .into_iter()
            .map(node_id_view)
            .collect(),
        validations: value.validations.into_iter().map(node_id_view).collect(),
        related: value.related.into_iter().map(node_id_view).collect(),
        reasons: value.reasons,
        recent_failures: value.recent_failures,
    }
}

pub(crate) fn task_intent_view(value: TaskIntent) -> TaskIntentView {
    TaskIntentView {
        task_id: value.task_id.0.to_string(),
        specs: value.specs.into_iter().map(node_id_view).collect(),
        implementations: value
            .implementations
            .into_iter()
            .map(node_id_view)
            .collect(),
        validations: value.validations.into_iter().map(node_id_view).collect(),
        related: value.related.into_iter().map(node_id_view).collect(),
        drift_candidates: value
            .drift_candidates
            .into_iter()
            .map(drift_candidate_view)
            .collect(),
    }
}

fn entry_curator_kind(entry: &MemoryEntry) -> Option<&str> {
    entry
        .metadata
        .get("curator")
        .and_then(|value| value.get("kind"))
        .and_then(Value::as_str)
}

fn entry_curator_category(entry: &MemoryEntry) -> Option<&str> {
    entry
        .metadata
        .get("curator")
        .and_then(|value| value.get("category"))
        .and_then(Value::as_str)
        .or_else(|| entry.metadata.get("category").and_then(Value::as_str))
}

fn entry_overlaps_anchors(entry: &MemoryEntry, anchors: &[AnchorRef]) -> bool {
    entry.anchors.iter().any(|anchor| anchors.contains(anchor))
}

pub(crate) fn promoted_memory_entries(
    session: &SessionState,
    prism: &Prism,
    anchors: &[AnchorRef],
    curator_kind: &str,
) -> Vec<MemoryEntry> {
    let expanded = prism.anchors_for(anchors);
    let mut entries = session
        .notes
        .snapshot()
        .entries
        .into_iter()
        .filter(|entry| entry.source == MemorySource::System)
        .filter(|entry| {
            entry_curator_kind(entry) == Some(curator_kind)
                || entry_curator_category(entry) == Some(curator_kind)
        })
        .filter(|entry| entry_overlaps_anchors(entry, &expanded))
        .collect::<Vec<_>>();
    entries.sort_by(|left, right| {
        right
            .created_at
            .cmp(&left.created_at)
            .then_with(|| left.id.0.cmp(&right.id.0))
    });
    entries
}

pub(crate) fn promoted_validation_checks(
    session: &SessionState,
    prism: &Prism,
    anchors: &[AnchorRef],
) -> Vec<ValidationCheck> {
    let mut checks = promoted_memory_entries(session, prism, anchors, "validation_recipe")
        .into_iter()
        .flat_map(|entry| {
            entry
                .metadata
                .get("checks")
                .and_then(Value::as_array)
                .into_iter()
                .flatten()
                .filter_map(Value::as_str)
                .map(|label| ValidationCheck {
                    label: label.to_string(),
                    score: entry.trust,
                    last_seen: entry.created_at,
                })
                .collect::<Vec<_>>()
        })
        .collect::<Vec<_>>();
    checks.sort_by(|left, right| {
        right
            .score
            .partial_cmp(&left.score)
            .unwrap_or(std::cmp::Ordering::Equal)
            .then_with(|| right.last_seen.cmp(&left.last_seen))
            .then_with(|| left.label.cmp(&right.label))
    });
    checks.dedup_by(|left, right| left.label == right.label);
    checks
}

pub(crate) fn promoted_summary_texts(
    session: &SessionState,
    prism: &Prism,
    anchors: &[AnchorRef],
) -> Vec<String> {
    let mut summaries = promoted_memory_entries(session, prism, anchors, "risk_summary")
        .into_iter()
        .map(|entry| entry.content)
        .collect::<Vec<_>>();
    summaries.sort();
    summaries.dedup();
    summaries
}

pub(crate) fn merge_promoted_checks(
    existing: &mut Vec<ValidationCheck>,
    promoted: Vec<ValidationCheck>,
) {
    existing.extend(promoted);
    existing.sort_by(|left, right| {
        right
            .score
            .partial_cmp(&left.score)
            .unwrap_or(std::cmp::Ordering::Equal)
            .then_with(|| right.last_seen.cmp(&left.last_seen))
            .then_with(|| left.label.cmp(&right.label))
    });
    existing.dedup_by(|left, right| left.label == right.label);
}

pub(crate) fn scored_memory_view(memory: ScoredMemory) -> ScoredMemoryView {
    ScoredMemoryView {
        id: memory.id.0,
        entry: memory_entry_view(memory.entry),
        score: memory.score,
        source_module: memory.source_module,
        explanation: memory.explanation,
    }
}

pub(crate) fn memory_entry_view(entry: MemoryEntry) -> MemoryEntryView {
    MemoryEntryView {
        id: entry.id.0,
        anchors: entry.anchors,
        kind: format!("{:?}", entry.kind),
        scope: format!("{:?}", entry.scope),
        content: entry.content,
        metadata: entry.metadata,
        created_at: entry.created_at,
        source: format!("{:?}", entry.source),
        trust: entry.trust,
    }
}

pub(crate) fn memory_event_view(event: MemoryEvent) -> MemoryEventView {
    MemoryEventView {
        id: event.id,
        action: format!("{:?}", event.action),
        memory_id: event.memory_id.0,
        scope: format!("{:?}", event.scope),
        entry: event.entry.map(memory_entry_view),
        recorded_at: event.recorded_at,
        task_id: event.task_id,
        promoted_from: event.promoted_from.into_iter().map(|id| id.0).collect(),
        supersedes: event.supersedes.into_iter().map(|id| id.0).collect(),
    }
}

pub(crate) fn validation_check_view(check: ValidationCheck) -> ValidationCheckView {
    ValidationCheckView {
        label: check.label,
        score: check.score,
        last_seen: check.last_seen,
    }
}

pub(crate) fn co_change_view(value: CoChange) -> CoChangeView {
    CoChangeView {
        lineage: value.lineage.0.to_string(),
        count: value.count,
        nodes: value.nodes.into_iter().map(node_id_view).collect(),
    }
}

pub(crate) fn workspace_revision_view(value: WorkspaceRevision) -> WorkspaceRevisionView {
    WorkspaceRevisionView {
        graph_version: value.graph_version,
        git_commit: value.git_commit.map(|commit| commit.to_string()),
    }
}

pub(crate) fn plan_view(value: prism_coordination::Plan) -> PlanView {
    PlanView {
        id: value.id.0.to_string(),
        goal: value.goal,
        status: value.status,
        root_task_ids: value
            .root_tasks
            .into_iter()
            .map(|task_id| task_id.0.to_string())
            .collect(),
    }
}

pub(crate) fn plan_graph_view(value: prism_ir::PlanGraph) -> PlanGraphView {
    PlanGraphView {
        id: value.id.0.to_string(),
        scope: value.scope,
        kind: value.kind,
        title: value.title,
        goal: value.goal,
        status: value.status,
        revision: value.revision,
        root_node_ids: value
            .root_nodes
            .into_iter()
            .map(|node_id| node_id.0.to_string())
            .collect(),
        tags: value.tags,
        created_from: value.created_from,
        metadata: value.metadata,
        nodes: value.nodes.into_iter().map(plan_node_view).collect(),
        edges: value.edges.into_iter().map(plan_edge_view).collect(),
    }
}

pub(crate) fn plan_execution_overlay_view(
    value: prism_ir::PlanExecutionOverlay,
) -> PlanExecutionOverlayView {
    PlanExecutionOverlayView {
        node_id: value.node_id.0.to_string(),
        pending_handoff_to: value.pending_handoff_to.map(|agent| agent.0.to_string()),
        session: value.session.map(|session| session.0.to_string()),
    }
}

fn plan_node_view(value: prism_ir::PlanNode) -> PlanNodeView {
    PlanNodeView {
        id: value.id.0.to_string(),
        plan_id: value.plan_id.0.to_string(),
        kind: value.kind,
        title: value.title,
        summary: value.summary,
        status: value.status,
        bindings: plan_binding_view(value.bindings),
        acceptance: value
            .acceptance
            .into_iter()
            .map(plan_acceptance_criterion_view)
            .collect(),
        is_abstract: value.is_abstract,
        assignee: value.assignee.map(|agent| agent.0.to_string()),
        base_revision: workspace_revision_view(value.base_revision),
        priority: value.priority,
        tags: value.tags,
        metadata: value.metadata,
    }
}

fn plan_binding_view(value: prism_ir::PlanBinding) -> PlanBindingView {
    PlanBindingView {
        anchors: value.anchors,
        concept_handles: value.concept_handles,
        artifact_refs: value.artifact_refs,
        memory_refs: value.memory_refs,
        outcome_refs: value.outcome_refs,
    }
}

fn plan_acceptance_criterion_view(
    value: prism_ir::PlanAcceptanceCriterion,
) -> PlanAcceptanceCriterionView {
    PlanAcceptanceCriterionView {
        label: value.label,
        anchors: value.anchors,
        required_checks: value
            .required_checks
            .into_iter()
            .map(|check| ValidationRefView { id: check.id })
            .collect(),
        evidence_policy: format!("{:?}", value.evidence_policy),
    }
}

fn plan_edge_view(value: prism_ir::PlanEdge) -> PlanEdgeView {
    PlanEdgeView {
        id: value.id.0.to_string(),
        plan_id: value.plan_id.0.to_string(),
        from: value.from.0.to_string(),
        to: value.to.0.to_string(),
        kind: value.kind,
        summary: value.summary,
        metadata: value.metadata,
    }
}

pub(crate) fn coordination_task_view(
    value: prism_coordination::CoordinationTask,
) -> CoordinationTaskView {
    CoordinationTaskView {
        id: value.id.0.to_string(),
        plan_id: value.plan.0.to_string(),
        title: value.title,
        status: value.status,
        assignee: value.assignee.map(|agent| agent.0.to_string()),
        pending_handoff_to: value.pending_handoff_to.map(|agent| agent.0.to_string()),
        anchors: value.anchors,
        depends_on: value
            .depends_on
            .into_iter()
            .map(|task_id| task_id.0.to_string())
            .collect(),
        base_revision: workspace_revision_view(value.base_revision),
    }
}

pub(crate) fn claim_view(value: prism_coordination::WorkClaim) -> ClaimView {
    ClaimView {
        id: value.id.0.to_string(),
        holder: value.holder.0.to_string(),
        task_id: value.task.map(|task| task.0.to_string()),
        capability: value.capability,
        mode: value.mode,
        status: value.status,
        anchors: value.anchors,
        expires_at: value.expires_at,
        base_revision: workspace_revision_view(value.base_revision),
    }
}

pub(crate) fn conflict_view(value: prism_coordination::CoordinationConflict) -> ConflictView {
    ConflictView {
        severity: value.severity,
        summary: value.summary,
        anchors: value.anchors,
        overlap_kinds: value.overlap_kinds,
        blocking_claim_ids: value
            .blocking_claims
            .into_iter()
            .map(|claim_id| claim_id.0.to_string())
            .collect(),
    }
}

pub(crate) fn blocker_view(value: prism_coordination::TaskBlocker) -> BlockerView {
    BlockerView {
        kind: value.kind,
        summary: value.summary,
        related_task_id: value.related_task_id.map(|task_id| task_id.0.to_string()),
        related_artifact_id: value
            .related_artifact_id
            .map(|artifact_id| artifact_id.0.to_string()),
        risk_score: value.risk_score,
        validation_checks: value.validation_checks,
    }
}

pub(crate) fn artifact_view(value: prism_coordination::Artifact) -> ArtifactView {
    ArtifactView {
        id: value.id.0.to_string(),
        task_id: value.task.0.to_string(),
        status: value.status,
        anchors: value.anchors,
        base_revision: workspace_revision_view(value.base_revision),
        diff_ref: value.diff_ref,
        required_validations: value.required_validations,
        validated_checks: value.validated_checks,
        risk_score: value.risk_score,
    }
}

pub(crate) fn policy_violation_view(
    value: prism_coordination::PolicyViolation,
) -> PolicyViolationView {
    PolicyViolationView {
        code: serde_json::to_string(&value.code)
            .unwrap_or_else(|_| "\"unknown\"".to_string())
            .trim_matches('"')
            .to_string(),
        summary: value.summary,
        plan_id: value.plan_id.map(|id| id.0.to_string()),
        task_id: value.task_id.map(|id| id.0.to_string()),
        claim_id: value.claim_id.map(|id| id.0.to_string()),
        artifact_id: value.artifact_id.map(|id| id.0.to_string()),
        details: value.details,
    }
}

pub(crate) fn policy_violation_record_view(
    value: prism_coordination::PolicyViolationRecord,
) -> PolicyViolationRecordView {
    PolicyViolationRecordView {
        event_id: value.event_id.0.to_string(),
        ts: value.ts,
        summary: value.summary,
        plan_id: value.plan_id.map(|id| id.0.to_string()),
        task_id: value.task_id.map(|id| id.0.to_string()),
        claim_id: value.claim_id.map(|id| id.0.to_string()),
        artifact_id: value.artifact_id.map(|id| id.0.to_string()),
        violations: value
            .violations
            .into_iter()
            .map(policy_violation_view)
            .collect(),
    }
}

pub(crate) fn node_id_view(node: NodeId) -> NodeIdView {
    NodeIdView {
        crate_name: node.crate_name.to_string(),
        path: node.path.to_string(),
        kind: node.kind,
    }
}

pub(crate) fn edge_view(edge: Edge) -> EdgeView {
    EdgeView {
        kind: edge.kind,
        source: node_id_view(edge.source),
        target: node_id_view(edge.target),
        origin: edge.origin,
        confidence: edge.confidence,
    }
}

pub(crate) fn inferred_edge_record_view(
    record: prism_agent::InferredEdgeRecord,
) -> InferredEdgeRecordView {
    InferredEdgeRecordView {
        id: record.id.0,
        edge: edge_view(record.edge),
        scope: format!("{:?}", record.scope),
        task_id: record.task.map(|task| task.0.to_string()),
        evidence: record.evidence,
    }
}
