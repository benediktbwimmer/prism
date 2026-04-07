use anyhow::{anyhow, Result};
use prism_curator::{
    CuratorJobRecord, CuratorProposal, CuratorProposalDisposition, CuratorTrigger,
};
use prism_ir::{AnchorRef, Edge, NodeId, WorkspaceRevision};
use prism_js::{
    AdHocPlanProjectionDiffView, AdHocPlanProjectionSummaryView, AdHocPlanProjectionView,
    AnchorRefView, ArtifactRiskView, ArtifactView, BlockerCauseView, BlockerView, ChangeImpactView,
    ClaimView, CoChangeView, ConceptBindingMetadataView, ConceptCurationHintsView,
    ConceptDecodeLensView, ConceptPacketTruncationView, ConceptPacketVerbosityView,
    ConceptPacketView, ConceptProvenanceView, ConceptPublicationStatusView, ConceptPublicationView,
    ConceptRelationDirectionView, ConceptRelationKindView, ConceptRelationView,
    ConceptResolutionView, ConceptScopeView, ConflictView, ContractCompatibilityView,
    ContractGuaranteeStrengthView, ContractGuaranteeView, ContractHealthSignalsView,
    ContractHealthStatusView, ContractHealthView, ContractKindView, ContractPacketView,
    ContractResolutionView, ContractStabilityView, ContractStatusView, ContractTargetView,
    ContractValidationView, CoordinationTaskLifecycleView, CoordinationTaskView, CuratorJobView,
    CuratorProposalRecordView, CuratorProposalView, DriftCandidateView, EdgeView,
    GitExecutionOverlayView, GitExecutionPolicyView, GitPreflightReportView, GitPublishReportView,
    MemoryEntryView, MemoryEventView, NodeIdView, PlanAcceptanceCriterionView, PlanActivityView,
    PlanBindingView, PlanEdgeView, PlanExecutionOverlayView, PlanGraphView, PlanListEntryView,
    PlanNodeBlockerView, PlanNodeRecommendationView, PlanNodeStatusCountsView, PlanNodeView,
    PlanSchedulingView, PlanSummaryView, PlanView, PolicyViolationRecordView, PolicyViolationView,
    ProjectionAuthorityPlaneView, ProjectionClassView, QueryDiagnostic, ScoredMemoryView,
    TaskGitExecutionView, TaskIntentView, TaskRiskView, TaskValidationRecipeView,
    ValidationCheckView, ValidationRecipeView, ValidationRefView, WorkspaceRevisionView,
};
use prism_memory::{MemoryEntry, MemoryEvent, MemorySource, ScoredMemory};
use prism_projections::{ProjectionAuthorityPlane, ProjectionClass};
use prism_query::{
    AdHocPlanProjection, AdHocPlanProjectionDiff, AdHocPlanProjectionSummary, ArtifactRisk,
    ChangeImpact, CoChange, ConceptDecodeLens, ConceptPacket, ConceptProvenance,
    ConceptPublication, ConceptPublicationStatus, ConceptRelation, ConceptRelationKind,
    ConceptResolution, ConceptScope, ContractCompatibility, ContractGuarantee,
    ContractGuaranteeStrength, ContractHealth, ContractHealthSignals, ContractHealthStatus,
    ContractKind, ContractPacket, ContractResolution, ContractStability, ContractStatus,
    ContractTarget, ContractValidation, DriftCandidate, PlanActivity, PlanListEntry,
    PlanNodeRecommendation, PlanNodeStatusCounts, PlanSummary, Prism, TaskIntent, TaskRisk,
    TaskValidationRecipe, ValidationCheck, ValidationRecipe,
};
use serde_json::Value;
use std::path::Path;

use crate::{
    compact_followups::workspace_display_path, normalize_query_diagnostic, InferredEdgeRecordView,
    SessionState,
};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum ConceptVerbosity {
    Summary,
    Standard,
    Full,
}

impl ConceptVerbosity {
    pub(crate) fn max_member_count(self) -> Option<usize> {
        match self {
            Self::Summary => Some(3),
            Self::Standard => Some(8),
            Self::Full => None,
        }
    }

    pub(crate) fn max_evidence_count(self) -> Option<usize> {
        match self {
            Self::Summary => Some(2),
            Self::Standard => Some(4),
            Self::Full => None,
        }
    }

    pub(crate) fn max_relation_count(self) -> Option<usize> {
        match self {
            Self::Summary => Some(6),
            Self::Standard => Some(12),
            Self::Full => None,
        }
    }

    pub(crate) fn max_relation_evidence_count(self) -> Option<usize> {
        match self {
            Self::Summary => Some(0),
            Self::Standard => Some(1),
            Self::Full => None,
        }
    }
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub(crate) struct ConceptPacketTruncationStats {
    pub(crate) core_members_omitted: usize,
    pub(crate) supporting_members_omitted: usize,
    pub(crate) likely_tests_omitted: usize,
    pub(crate) evidence_omitted: usize,
    pub(crate) relations_omitted: usize,
    pub(crate) relation_evidence_omitted: usize,
}

impl ConceptPacketTruncationStats {
    pub(crate) fn is_empty(self) -> bool {
        self.core_members_omitted == 0
            && self.supporting_members_omitted == 0
            && self.likely_tests_omitted == 0
            && self.evidence_omitted == 0
            && self.relations_omitted == 0
            && self.relation_evidence_omitted == 0
    }

    pub(crate) fn into_view(self) -> Option<ConceptPacketTruncationView> {
        (!self.is_empty()).then_some(ConceptPacketTruncationView {
            core_members_omitted: self.core_members_omitted,
            supporting_members_omitted: self.supporting_members_omitted,
            likely_tests_omitted: self.likely_tests_omitted,
            evidence_omitted: self.evidence_omitted,
            relations_omitted: self.relations_omitted,
            relation_evidence_omitted: self.relation_evidence_omitted,
        })
    }
}

pub(crate) fn concept_verbosity_view(verbosity: ConceptVerbosity) -> ConceptPacketVerbosityView {
    match verbosity {
        ConceptVerbosity::Summary => ConceptPacketVerbosityView::Summary,
        ConceptVerbosity::Standard => ConceptPacketVerbosityView::Standard,
        ConceptVerbosity::Full => ConceptPacketVerbosityView::Full,
    }
}

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
        CuratorProposal::ConceptCandidate(candidate) => {
            ("concept_candidate", serde_json::to_value(candidate)?)
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
    prism: &Prism,
    packet: ConceptPacket,
    verbosity: ConceptVerbosity,
    include_binding_metadata: bool,
    resolution: Option<ConceptResolution>,
) -> ConceptPacketView {
    let handle = packet.handle.clone();
    let curation_hints = concept_curation_hints_view_from_packet(prism, &packet, verbosity);
    let (core_members, core_members_omitted) = truncate_vec_with_omitted(
        packet.core_members.into_iter().map(node_id_view).collect(),
        verbosity.max_member_count(),
    );
    let (supporting_members, supporting_members_omitted) = truncate_vec_with_omitted(
        packet
            .supporting_members
            .into_iter()
            .map(node_id_view)
            .collect(),
        verbosity.max_member_count(),
    );
    let (likely_tests, likely_tests_omitted) = truncate_vec_with_omitted(
        packet.likely_tests.into_iter().map(node_id_view).collect(),
        verbosity.max_member_count(),
    );
    let (evidence, evidence_omitted) =
        truncate_vec_with_omitted(packet.evidence, verbosity.max_evidence_count());
    let (relations, relation_truncation) = truncate_concept_relations(
        prism
            .concept_relations_for_handle(&handle)
            .into_iter()
            .map(|relation| concept_relation_view(prism, &handle, relation))
            .collect(),
        verbosity,
    );
    let truncation = ConceptPacketTruncationStats {
        core_members_omitted,
        supporting_members_omitted,
        likely_tests_omitted,
        evidence_omitted,
        relations_omitted: relation_truncation.relations_omitted,
        relation_evidence_omitted: relation_truncation.relation_evidence_omitted,
    }
    .into_view();
    ConceptPacketView {
        handle: handle.clone(),
        canonical_name: packet.canonical_name,
        summary: packet.summary,
        aliases: packet.aliases,
        confidence: packet.confidence,
        core_members,
        supporting_members,
        likely_tests,
        evidence,
        risk_hint: packet.risk_hint,
        decode_lenses: packet
            .decode_lenses
            .into_iter()
            .map(concept_decode_lens_view)
            .collect(),
        verbosity_applied: concept_verbosity_view(verbosity),
        truncation,
        curation_hints,
        scope: concept_scope_view(packet.scope),
        provenance: concept_provenance_view(packet.provenance),
        publication: packet.publication.map(concept_publication_view),
        relations,
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

fn concept_curation_hints_view_from_packet(
    prism: &Prism,
    packet: &ConceptPacket,
    verbosity: ConceptVerbosity,
) -> ConceptCurationHintsView {
    let has_explicit_members = !packet.core_members.is_empty()
        || !packet.supporting_members.is_empty()
        || !packet.likely_tests.is_empty();
    let fallback = if !has_explicit_members {
        crate::concept_followthrough_targets(prism, packet)
    } else {
        crate::concept_followthrough::ConceptFollowthroughTargets::default()
    };
    let inspect_first = packet
        .core_members
        .first()
        .cloned()
        .map(node_id_view)
        .or_else(|| packet.supporting_members.first().cloned().map(node_id_view))
        .or_else(|| packet.likely_tests.first().cloned().map(node_id_view))
        .or_else(|| fallback.inspect_first.clone().map(node_id_view));
    let supporting_read = packet
        .supporting_members
        .first()
        .cloned()
        .map(node_id_view)
        .or_else(|| fallback.supporting_reads.first().cloned().map(node_id_view));
    let likely_test = packet
        .likely_tests
        .first()
        .cloned()
        .map(node_id_view)
        .or_else(|| fallback.likely_tests.first().cloned().map(node_id_view));
    let next_action = concept_packet_next_action(
        packet,
        inspect_first.as_ref(),
        supporting_read.as_ref(),
        likely_test.as_ref(),
        verbosity,
    );
    ConceptCurationHintsView {
        inspect_first,
        supporting_read,
        likely_test,
        next_action,
    }
}

fn concept_packet_next_action(
    packet: &ConceptPacket,
    inspect_first: Option<&NodeIdView>,
    supporting_read: Option<&NodeIdView>,
    likely_test: Option<&NodeIdView>,
    verbosity: ConceptVerbosity,
) -> String {
    let inspect_target = inspect_first.map(|node| format!("`{}`", node.path));
    let supporting_target = supporting_read.map(|node| format!("`{}`", node.path));
    let likely_test = likely_test.map(|node| format!("`{}`", node.path));
    let validation_lens = packet
        .decode_lenses
        .iter()
        .any(|lens| matches!(lens, ConceptDecodeLens::Validation));
    let inspect_is_doc = inspect_first.is_some_and(|node| {
        matches!(
            node.kind,
            prism_ir::NodeKind::MarkdownHeading | prism_ir::NodeKind::Document
        )
    });

    match (
        inspect_target,
        supporting_target,
        likely_test,
        validation_lens,
        verbosity,
        inspect_is_doc,
    ) {
        (Some(target), Some(support), Some(test), _, ConceptVerbosity::Summary, true) => format!(
            "Inspect doc or spec section {target} first, follow it into supporting owner {support}, verify with likely test {test}, then retry with `verbosity: \"full\"` if you need the remaining members."
        ),
        (Some(target), Some(support), Some(test), _, _, true) => format!(
            "Inspect doc or spec section {target} first, follow it into supporting owner {support}, then verify with likely test {test}."
        ),
        (Some(target), Some(support), None, true, ConceptVerbosity::Summary, true) => format!(
            "Inspect doc or spec section {target} first, follow it into supporting owner {support}, then use `prism.decodeConcept({{ handle: \"{}\", lens: \"validation\" }})` or retry with `verbosity: \"full\"`.",
            packet.handle
        ),
        (Some(target), Some(support), None, true, _, true) => format!(
            "Inspect doc or spec section {target} first, then follow it into supporting owner {support} and use `prism.decodeConcept({{ handle: \"{}\", lens: \"validation\" }})` for broader validation context.",
            packet.handle
        ),
        (Some(target), Some(support), _, false, ConceptVerbosity::Summary, true) => format!(
            "Inspect doc or spec section {target} first, follow it into supporting owner {support}, then retry with `verbosity: \"full\"` if you need the remaining members or tests."
        ),
        (Some(target), Some(support), _, false, _, true) => format!(
            "Inspect doc or spec section {target} first, then follow it into supporting owner {support}."
        ),
        (Some(target), Some(support), None, true, ConceptVerbosity::Summary, false) => format!(
            "Inspect {target} first, then follow it into supporting member {support} and use `prism.decodeConcept({{ handle: \"{}\", lens: \"validation\" }})` or retry with `verbosity: \"full\"`.",
            packet.handle
        ),
        (Some(target), Some(support), None, true, _, false) => format!(
            "Inspect {target} first, then follow it into supporting member {support} and use `prism.decodeConcept({{ handle: \"{}\", lens: \"validation\" }})` for broader validation context.",
            packet.handle
        ),
        (Some(target), Some(support), None, false, ConceptVerbosity::Summary, false) => format!(
            "Inspect {target} first, then follow it into supporting member {support}, and retry with `verbosity: \"full\"` if you need the remaining members or tests."
        ),
        (Some(target), Some(support), None, false, _, false) => format!(
            "Inspect {target} first, then follow it into supporting member {support}."
        ),
        (Some(target), _, Some(test), _, ConceptVerbosity::Summary, _) => format!(
            "Inspect {target} first, verify it with likely test {test}, then retry with `verbosity: \"full\"` if you need the remaining members."
        ),
        (Some(target), _, Some(test), _, _, _) => {
            format!("Inspect {target} first, then verify it with likely test {test}.")
        }
        (Some(target), None, None, true, ConceptVerbosity::Summary, _) => format!(
            "Inspect {target} first, then use `prism.decodeConcept({{ handle: \"{}\", lens: \"validation\" }})` for broader validation context or retry with `verbosity: \"full\"`.",
            packet.handle
        ),
        (Some(target), None, None, true, _, _) => format!(
            "Inspect {target} first, then use `prism.decodeConcept({{ handle: \"{}\", lens: \"validation\" }})` for broader validation context.",
            packet.handle
        ),
        (Some(target), None, None, false, ConceptVerbosity::Summary, _) => format!(
            "Inspect {target} first, then retry with `verbosity: \"full\"` if you need the remaining members or tests."
        ),
        (Some(target), None, None, false, _, _) => {
            format!("Inspect {target} first, then expand outward from the remaining concept members.")
        }
        (None, _, Some(test), _, _, _) => format!(
            "Start with likely test {test}, then add or refresh stronger concept member bindings before relying on this packet."
        ),
        (None, _, None, _, _, _) => {
            "Retry with `verbosity: \"full\"` or refresh the concept bindings so the packet exposes a concrete member to inspect.".to_string()
        }
    }
}

fn contract_health_view(packet: &ContractPacket, health: ContractHealth) -> ContractHealthView {
    let next_action = contract_health_next_action(packet, &health);
    ContractHealthView {
        status: contract_health_status_view(health.status),
        score: health.score,
        reasons: health.reasons,
        signals: contract_health_signals_view(health.signals),
        superseded_by: health.superseded_by,
        next_action,
    }
}

pub(crate) fn truncate_concept_relations(
    relations: Vec<ConceptRelationView>,
    verbosity: ConceptVerbosity,
) -> (Vec<ConceptRelationView>, ConceptPacketTruncationStats) {
    let (relations, relations_omitted) =
        truncate_vec_with_omitted(relations, verbosity.max_relation_count());
    let mut relation_evidence_omitted = 0;
    let relations = relations
        .into_iter()
        .map(|mut relation| {
            let (evidence, omitted) = truncate_vec_with_omitted(
                relation.evidence,
                verbosity.max_relation_evidence_count(),
            );
            relation.evidence = evidence;
            relation_evidence_omitted += omitted;
            relation
        })
        .collect();
    (
        relations,
        ConceptPacketTruncationStats {
            relations_omitted,
            relation_evidence_omitted,
            ..ConceptPacketTruncationStats::default()
        },
    )
}

pub(crate) fn truncate_vec_with_omitted<T>(
    mut items: Vec<T>,
    limit: Option<usize>,
) -> (Vec<T>, usize) {
    let omitted = limit
        .map(|limit| items.len().saturating_sub(limit))
        .unwrap_or(0);
    if let Some(limit) = limit {
        items.truncate(limit);
    }
    (items, omitted)
}

pub(crate) fn concept_relation_view(
    prism: &Prism,
    focus_handle: &str,
    relation: ConceptRelation,
) -> ConceptRelationView {
    let focus = focus_handle.trim().to_ascii_lowercase();
    let outgoing = relation.source_handle.trim().to_ascii_lowercase() == focus;
    let related_handle = if outgoing {
        relation.target_handle.clone()
    } else {
        relation.source_handle.clone()
    };
    let related = prism.concept_by_handle(&related_handle);
    ConceptRelationView {
        kind: concept_relation_kind_view(relation.kind),
        direction: if outgoing {
            ConceptRelationDirectionView::Outgoing
        } else {
            ConceptRelationDirectionView::Incoming
        },
        related_handle,
        related_canonical_name: related.as_ref().map(|packet| packet.canonical_name.clone()),
        related_summary: related.as_ref().map(|packet| packet.summary.clone()),
        confidence: relation.confidence,
        evidence: relation.evidence,
        scope: concept_scope_view(relation.scope),
    }
}

pub(crate) fn concept_resolution_view(resolution: ConceptResolution) -> ConceptResolutionView {
    ConceptResolutionView {
        score: resolution.score,
        reasons: resolution.reasons,
    }
}

pub(crate) fn anchor_ref_view(
    prism: &Prism,
    workspace_root: Option<&Path>,
    anchor: AnchorRef,
) -> AnchorRefView {
    match anchor {
        AnchorRef::Node(node) => AnchorRefView::Node {
            crate_name: node.crate_name.to_string(),
            path: node.path.to_string(),
            kind: node.kind.to_string(),
        },
        AnchorRef::Lineage(lineage) => AnchorRefView::Lineage {
            lineage_id: lineage.0.to_string(),
        },
        AnchorRef::File(file) => AnchorRefView::File {
            file_id: Some(file.0),
            path: prism
                .graph()
                .file_path(file)
                .map(|path| workspace_display_path(workspace_root, path)),
        },
        AnchorRef::Kind(kind) => AnchorRefView::Kind {
            kind: kind.to_string(),
        },
    }
}

pub(crate) fn contract_packet_view(
    prism: &Prism,
    workspace_root: Option<&Path>,
    packet: ContractPacket,
    resolution: Option<ContractResolution>,
) -> ContractPacketView {
    let health = prism.contract_health_by_handle(&packet.handle);
    let health = health.map(|health| contract_health_view(&packet, health));
    ContractPacketView {
        handle: packet.handle,
        name: packet.name,
        summary: packet.summary,
        aliases: packet.aliases,
        kind: contract_kind_view(packet.kind),
        subject: contract_target_view(prism, workspace_root, packet.subject),
        guarantees: packet
            .guarantees
            .into_iter()
            .map(contract_guarantee_view)
            .collect(),
        assumptions: packet.assumptions,
        consumers: packet
            .consumers
            .into_iter()
            .map(|target| contract_target_view(prism, workspace_root, target))
            .collect(),
        validations: packet
            .validations
            .into_iter()
            .map(|validation| contract_validation_view(prism, workspace_root, validation))
            .collect(),
        stability: contract_stability_view(packet.stability),
        compatibility: contract_compatibility_view(packet.compatibility),
        evidence: packet.evidence,
        status: contract_status_view(packet.status),
        health,
        scope: concept_scope_view(packet.scope),
        provenance: concept_provenance_view(packet.provenance),
        publication: packet.publication.map(concept_publication_view),
        resolution: resolution.map(contract_resolution_view),
    }
}

fn contract_kind_view(kind: ContractKind) -> ContractKindView {
    match kind {
        ContractKind::Interface => ContractKindView::Interface,
        ContractKind::Behavioral => ContractKindView::Behavioral,
        ContractKind::DataShape => ContractKindView::DataShape,
        ContractKind::DependencyBoundary => ContractKindView::DependencyBoundary,
        ContractKind::Lifecycle => ContractKindView::Lifecycle,
        ContractKind::Protocol => ContractKindView::Protocol,
        ContractKind::Operational => ContractKindView::Operational,
    }
}

fn contract_status_view(status: ContractStatus) -> ContractStatusView {
    match status {
        ContractStatus::Candidate => ContractStatusView::Candidate,
        ContractStatus::Active => ContractStatusView::Active,
        ContractStatus::Deprecated => ContractStatusView::Deprecated,
        ContractStatus::Retired => ContractStatusView::Retired,
    }
}

fn contract_stability_view(stability: ContractStability) -> ContractStabilityView {
    match stability {
        ContractStability::Experimental => ContractStabilityView::Experimental,
        ContractStability::Internal => ContractStabilityView::Internal,
        ContractStability::Public => ContractStabilityView::Public,
        ContractStability::Deprecated => ContractStabilityView::Deprecated,
        ContractStability::Migrating => ContractStabilityView::Migrating,
    }
}

fn contract_target_view(
    prism: &Prism,
    workspace_root: Option<&Path>,
    target: ContractTarget,
) -> ContractTargetView {
    ContractTargetView {
        anchors: target
            .anchors
            .into_iter()
            .map(|anchor| anchor_ref_view(prism, workspace_root, anchor))
            .collect(),
        concept_handles: target.concept_handles,
    }
}

fn contract_guarantee_view(guarantee: ContractGuarantee) -> ContractGuaranteeView {
    ContractGuaranteeView {
        id: guarantee.id,
        statement: guarantee.statement,
        scope: guarantee.scope,
        strength: guarantee.strength.map(contract_guarantee_strength_view),
        evidence_refs: guarantee.evidence_refs,
    }
}

fn contract_guarantee_strength_view(
    strength: ContractGuaranteeStrength,
) -> ContractGuaranteeStrengthView {
    match strength {
        ContractGuaranteeStrength::Hard => ContractGuaranteeStrengthView::Hard,
        ContractGuaranteeStrength::Soft => ContractGuaranteeStrengthView::Soft,
        ContractGuaranteeStrength::Conditional => ContractGuaranteeStrengthView::Conditional,
    }
}

fn contract_health_next_action(packet: &ContractPacket, health: &ContractHealth) -> Option<String> {
    let missing_evidence_ids = packet
        .guarantees
        .iter()
        .filter(|guarantee| guarantee.evidence_refs.is_empty())
        .map(|guarantee| format!("`{}`", guarantee.id))
        .collect::<Vec<_>>();

    match health.status {
        ContractHealthStatus::Healthy => None,
        ContractHealthStatus::Retired => Some(
            "Do not rely on this retired contract as a live promise; inspect an active replacement or publish a new active contract if the guarantee still matters."
                .to_string(),
        ),
        ContractHealthStatus::Superseded => health.superseded_by.first().map_or_else(
            || {
                Some(
                    "Inspect the active superseding contract and migrate callers, validations, and evidence to it before editing this retired promise surface."
                        .to_string(),
                )
            },
            |handle| {
                Some(format!(
                    "Inspect superseding contract `{handle}` and migrate callers, validations, and evidence to it before relying on this older promise."
                ))
            },
        ),
        ContractHealthStatus::Stale => {
            if health.signals.stale_validation_links {
                Some(
                    "Repair or replace the stale validation anchors with `prism_mutate` action `contract`, operation `attach_validation`, so the contract points at live checks again."
                        .to_string(),
                )
            } else if health.signals.validation_count == 0 {
                Some(
                    "Attach at least one explicit validation with `prism_mutate` action `contract`, operation `attach_validation`, before treating this contract as a dependable review signal."
                        .to_string(),
                )
            } else if !missing_evidence_ids.is_empty() {
                Some(format!(
                    "Add clause-level `evidenceRefs` for guarantee ids {} with `prism_mutate` action `contract`, operation `update`.",
                    missing_evidence_ids.join(", ")
                ))
            } else {
                Some(
                    "Refresh this contract's validations or evidence before treating it as a dependable review signal."
                        .to_string(),
                )
            }
        }
        ContractHealthStatus::Degraded | ContractHealthStatus::Watch => {
            let mut steps = Vec::new();
            if health.signals.validation_coverage_ratio < 1.0 {
                steps.push(format!(
                    "attach more explicit validations with `prism_mutate` action `contract`, operation `attach_validation` ({} validation link(s) for {} guarantee clause(s))",
                    health.signals.validation_count, health.signals.guarantee_count
                ));
            }
            if !missing_evidence_ids.is_empty() {
                steps.push(format!(
                    "add clause-level `evidenceRefs` for guarantee ids {} with `prism_mutate` action `contract`, operation `update`",
                    missing_evidence_ids.join(", ")
                ));
            }
            if steps.is_empty() {
                Some(
                    "Review this contract's validations and evidence before relying on it in impact or review flows."
                        .to_string(),
                )
            } else {
                Some(format!(
                    "Next repair step: {}.",
                    steps.join(", then ")
                ))
            }
        }
    }
}

fn contract_health_signals_view(signals: ContractHealthSignals) -> ContractHealthSignalsView {
    ContractHealthSignalsView {
        guarantee_count: signals.guarantee_count,
        validation_count: signals.validation_count,
        consumer_count: signals.consumer_count,
        validation_coverage_ratio: signals.validation_coverage_ratio,
        guarantee_evidence_ratio: signals.guarantee_evidence_ratio,
        stale_validation_links: signals.stale_validation_links,
    }
}

fn contract_health_status_view(status: ContractHealthStatus) -> ContractHealthStatusView {
    match status {
        ContractHealthStatus::Healthy => ContractHealthStatusView::Healthy,
        ContractHealthStatus::Watch => ContractHealthStatusView::Watch,
        ContractHealthStatus::Degraded => ContractHealthStatusView::Degraded,
        ContractHealthStatus::Stale => ContractHealthStatusView::Stale,
        ContractHealthStatus::Superseded => ContractHealthStatusView::Superseded,
        ContractHealthStatus::Retired => ContractHealthStatusView::Retired,
    }
}

fn contract_validation_view(
    prism: &Prism,
    workspace_root: Option<&Path>,
    validation: ContractValidation,
) -> ContractValidationView {
    ContractValidationView {
        id: validation.id,
        summary: validation.summary,
        anchors: validation
            .anchors
            .into_iter()
            .map(|anchor| anchor_ref_view(prism, workspace_root, anchor))
            .collect(),
    }
}

fn contract_compatibility_view(compatibility: ContractCompatibility) -> ContractCompatibilityView {
    ContractCompatibilityView {
        compatible: compatibility.compatible,
        additive: compatibility.additive,
        risky: compatibility.risky,
        breaking: compatibility.breaking,
        migrating: compatibility.migrating,
    }
}

fn contract_resolution_view(resolution: ContractResolution) -> ContractResolutionView {
    ContractResolutionView {
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

fn concept_relation_kind_view(kind: ConceptRelationKind) -> ConceptRelationKindView {
    match kind {
        ConceptRelationKind::DependsOn => ConceptRelationKindView::DependsOn,
        ConceptRelationKind::Specializes => ConceptRelationKindView::Specializes,
        ConceptRelationKind::PartOf => ConceptRelationKindView::PartOf,
        ConceptRelationKind::ValidatedBy => ConceptRelationKindView::ValidatedBy,
        ConceptRelationKind::OftenUsedWith => ConceptRelationKindView::OftenUsedWith,
        ConceptRelationKind::Supersedes => ConceptRelationKindView::Supersedes,
        ConceptRelationKind::ConfusedWith => ConceptRelationKindView::ConfusedWith,
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

pub(crate) fn task_risk_view(
    prism: &Prism,
    value: TaskRisk,
    promoted_summaries: Vec<String>,
) -> TaskRiskView {
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
        contracts: value
            .contracts
            .into_iter()
            .map(|packet| contract_packet_view(prism, None, packet, None))
            .collect(),
        contract_review_notes: value.contract_review_notes,
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
    prism: &Prism,
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
        contracts: value
            .contracts
            .into_iter()
            .map(|packet| contract_packet_view(prism, None, packet, None))
            .collect(),
        contract_review_notes: value.contract_review_notes,
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

pub(crate) fn plan_view(
    value: prism_coordination::Plan,
    root_node_ids: Vec<prism_ir::PlanNodeId>,
    activity: Option<PlanActivity>,
) -> PlanView {
    PlanView {
        id: value.id.0.to_string(),
        title: value.title,
        goal: value.goal,
        status: value.status,
        scope: value.scope,
        kind: value.kind,
        revision: value.revision,
        scheduling: plan_scheduling_view(value.scheduling),
        git_execution_policy: git_execution_policy_view(value.policy.git_execution),
        tags: value.tags,
        created_from: value.created_from,
        root_node_ids: root_node_ids
            .into_iter()
            .map(|node_id| node_id.0.to_string())
            .collect(),
        activity: activity.map(plan_activity_view),
    }
}

pub(crate) fn plan_list_entry_view(value: PlanListEntry) -> PlanListEntryView {
    let activity = plan_activity_view(value.activity);
    PlanListEntryView {
        plan_id: value.plan_id.0.to_string(),
        title: value.title,
        goal: value.goal,
        status: value.status,
        scope: value.scope,
        kind: value.kind,
        scheduling: plan_scheduling_view(value.scheduling),
        git_execution_policy: git_execution_policy_view(value.policy.git_execution),
        root_node_ids: value
            .root_node_ids
            .into_iter()
            .map(|node_id| node_id.0.to_string())
            .collect(),
        created_at: activity.created_at,
        last_updated_at: activity.last_updated_at,
        node_status_counts: plan_node_status_counts_view(value.node_status_counts),
        summary: value.summary,
        plan_summary: plan_summary_view(value.plan_summary),
        activity: Some(activity),
    }
}

pub(crate) fn plan_activity_view(value: PlanActivity) -> PlanActivityView {
    PlanActivityView {
        created_at: value.created_at,
        last_updated_at: value.last_updated_at,
        last_event_kind: value.last_event_kind.map(|kind| format!("{kind:?}")),
        last_event_summary: value.last_event_summary,
        last_event_task_id: value
            .last_event_task_id
            .map(|task_id| task_id.0.to_string()),
    }
}

pub(crate) fn plan_node_status_counts_view(
    value: PlanNodeStatusCounts,
) -> PlanNodeStatusCountsView {
    PlanNodeStatusCountsView {
        proposed: value.proposed,
        ready: value.ready,
        in_progress: value.in_progress,
        blocked: value.blocked,
        waiting: value.waiting,
        in_review: value.in_review,
        validating: value.validating,
        completed: value.completed,
        abandoned: value.abandoned,
        abstract_nodes: value.abstract_nodes,
    }
}

pub(crate) fn plan_scheduling_view(
    value: prism_coordination::PlanScheduling,
) -> PlanSchedulingView {
    PlanSchedulingView {
        importance: value.importance,
        urgency: value.urgency,
        manual_boost: value.manual_boost,
        due_at: value.due_at,
    }
}

pub(crate) fn git_execution_policy_view(
    value: prism_coordination::GitExecutionPolicy,
) -> GitExecutionPolicyView {
    GitExecutionPolicyView {
        start_mode: format!("{:?}", value.start_mode).to_ascii_lowercase(),
        completion_mode: format!("{:?}", value.completion_mode).to_ascii_lowercase(),
        integration_mode: format!("{:?}", value.integration_mode).to_ascii_lowercase(),
        target_ref: value.target_ref,
        target_branch: value.target_branch,
        require_task_branch: value.require_task_branch,
        max_commits_behind_target: value.max_commits_behind_target,
        max_fetch_age_seconds: value.max_fetch_age_seconds,
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
        effective_assignee: value.effective_assignee.map(|agent| agent.0.to_string()),
        awaiting_handoff_from: value.awaiting_handoff_from.map(|node| node.0.to_string()),
        git_execution: value.git_execution.map(git_execution_overlay_view),
    }
}

fn git_execution_overlay_view(value: prism_ir::GitExecutionOverlay) -> GitExecutionOverlayView {
    GitExecutionOverlayView {
        status: value.status,
        pending_task_status: value.pending_task_status,
        source_ref: value.source_ref,
        target_ref: value.target_ref,
        publish_ref: value.publish_ref,
        target_branch: value.target_branch,
        source_commit: value.source_commit,
        publish_commit: value.publish_commit,
        target_commit_at_publish: value.target_commit_at_publish,
        review_artifact_ref: value.review_artifact_ref,
        integration_commit: value.integration_commit,
        integration_evidence: value.integration_evidence,
        integration_mode: value.integration_mode,
        integration_status: value.integration_status,
    }
}

fn git_preflight_report_view(
    value: prism_coordination::GitPreflightReport,
) -> GitPreflightReportView {
    GitPreflightReportView {
        source_ref: value.source_ref,
        target_ref: value.target_ref,
        publish_ref: value.publish_ref,
        checked_at: value.checked_at,
        target_branch: value.target_branch,
        max_commits_behind_target: value.max_commits_behind_target,
        fetch_age_seconds: value.fetch_age_seconds,
        current_branch: value.current_branch,
        head_commit: value.head_commit,
        target_commit: value.target_commit,
        merge_base_commit: value.merge_base_commit,
        behind_target_commits: value.behind_target_commits,
        worktree_dirty: value.worktree_dirty,
        dirty_paths: value.dirty_paths,
        protected_dirty_paths: value.protected_dirty_paths,
        failure: value.failure,
    }
}

fn git_publish_report_view(value: prism_coordination::GitPublishReport) -> GitPublishReportView {
    GitPublishReportView {
        attempted_at: value.attempted_at,
        publish_ref: value.publish_ref,
        code_commit: value.code_commit,
        coordination_commit: value.coordination_commit,
        pushed_ref: value.pushed_ref,
        staged_paths: value.staged_paths,
        protected_paths: value.protected_paths,
        failure: value.failure,
    }
}

pub(crate) fn ad_hoc_plan_projection_summary_view(
    value: AdHocPlanProjectionSummary,
) -> AdHocPlanProjectionSummaryView {
    AdHocPlanProjectionSummaryView {
        total_nodes: value.total_nodes,
        abstract_nodes: value.abstract_nodes,
        proposed_nodes: value.proposed_nodes,
        ready_nodes: value.ready_nodes,
        waiting_nodes: value.waiting_nodes,
        in_progress_nodes: value.in_progress_nodes,
        in_review_nodes: value.in_review_nodes,
        validating_nodes: value.validating_nodes,
        blocked_nodes: value.blocked_nodes,
        completed_nodes: value.completed_nodes,
        abandoned_nodes: value.abandoned_nodes,
        total_edges: value.total_edges,
    }
}

pub(crate) fn ad_hoc_plan_projection_view(value: AdHocPlanProjection) -> AdHocPlanProjectionView {
    AdHocPlanProjectionView {
        projection_class: match value.projection_class {
            ProjectionClass::Published => ProjectionClassView::Published,
            ProjectionClass::Serving => ProjectionClassView::Serving,
            ProjectionClass::AdHoc => ProjectionClassView::AdHoc,
        },
        authority_planes: value
            .authority_planes
            .into_iter()
            .map(|plane| match plane {
                ProjectionAuthorityPlane::PublishedRepo => {
                    ProjectionAuthorityPlaneView::PublishedRepo
                }
                ProjectionAuthorityPlane::SharedRuntime => {
                    ProjectionAuthorityPlaneView::SharedRuntime
                }
            })
            .collect(),
        history_source: value.history_source,
        plan_id: value.plan_id.0.to_string(),
        as_of: value.as_of,
        replayed_event_count: value.replayed_event_count,
        graph: plan_graph_view(value.graph),
        execution_overlays: value
            .execution_overlays
            .into_iter()
            .map(plan_execution_overlay_view)
            .collect(),
        summary: ad_hoc_plan_projection_summary_view(value.summary),
    }
}

pub(crate) fn ad_hoc_plan_projection_diff_view(
    value: AdHocPlanProjectionDiff,
) -> AdHocPlanProjectionDiffView {
    AdHocPlanProjectionDiffView {
        projection_class: match value.projection_class {
            ProjectionClass::Published => ProjectionClassView::Published,
            ProjectionClass::Serving => ProjectionClassView::Serving,
            ProjectionClass::AdHoc => ProjectionClassView::AdHoc,
        },
        authority_planes: value
            .authority_planes
            .into_iter()
            .map(|plane| match plane {
                ProjectionAuthorityPlane::PublishedRepo => {
                    ProjectionAuthorityPlaneView::PublishedRepo
                }
                ProjectionAuthorityPlane::SharedRuntime => {
                    ProjectionAuthorityPlaneView::SharedRuntime
                }
            })
            .collect(),
        history_source: value.history_source,
        plan_id: value.plan_id.0.to_string(),
        from: value.from,
        to: value.to,
        before: value.before.map(ad_hoc_plan_projection_view),
        after: value.after.map(ad_hoc_plan_projection_view),
        plan_metadata_changed: value.plan_metadata_changed,
        added_nodes: value
            .added_nodes
            .into_iter()
            .map(|id| id.0.to_string())
            .collect(),
        removed_nodes: value
            .removed_nodes
            .into_iter()
            .map(|id| id.0.to_string())
            .collect(),
        changed_nodes: value
            .changed_nodes
            .into_iter()
            .map(|id| id.0.to_string())
            .collect(),
        added_edges: value
            .added_edges
            .into_iter()
            .map(|id| id.0.to_string())
            .collect(),
        removed_edges: value
            .removed_edges
            .into_iter()
            .map(|id| id.0.to_string())
            .collect(),
        changed_edges: value
            .changed_edges
            .into_iter()
            .map(|id| id.0.to_string())
            .collect(),
        changed_execution_nodes: value
            .changed_execution_nodes
            .into_iter()
            .map(|id| id.0.to_string())
            .collect(),
    }
}

pub(crate) fn plan_node_blocker_view(value: prism_ir::PlanNodeBlocker) -> PlanNodeBlockerView {
    PlanNodeBlockerView {
        kind: value.kind,
        summary: value.summary,
        related_node_id: value.related_node_id.map(|node_id| node_id.0.to_string()),
        related_artifact_id: value
            .related_artifact_id
            .map(|artifact_id| artifact_id.0.to_string()),
        risk_score: value.risk_score,
        validation_checks: value.validation_checks,
        causes: value.causes.into_iter().map(blocker_cause_view).collect(),
    }
}

pub(crate) fn plan_summary_view(value: PlanSummary) -> PlanSummaryView {
    PlanSummaryView {
        plan_id: value.plan_id.0.to_string(),
        status: value.status,
        total_nodes: value.total_nodes,
        completed_nodes: value.completed_nodes,
        abandoned_nodes: value.abandoned_nodes,
        in_progress_nodes: value.in_progress_nodes,
        actionable_nodes: value.actionable_nodes,
        execution_blocked_nodes: value.execution_blocked_nodes,
        completion_gated_nodes: value.completion_gated_nodes,
        review_gated_nodes: value.review_gated_nodes,
        validation_gated_nodes: value.validation_gated_nodes,
        stale_nodes: value.stale_nodes,
        claim_conflicted_nodes: value.claim_conflicted_nodes,
    }
}

pub(crate) fn plan_node_recommendation_view(
    value: PlanNodeRecommendation,
) -> PlanNodeRecommendationView {
    PlanNodeRecommendationView {
        node: plan_node_view(value.node),
        actionable: value.actionable,
        effective_assignee: value.effective_assignee.map(|agent| agent.0.to_string()),
        score: value.score,
        reasons: value.reasons,
        blockers: value
            .blockers
            .into_iter()
            .map(plan_node_blocker_view)
            .collect(),
        unblocks: value
            .unblocks
            .into_iter()
            .map(|id| id.0.to_string())
            .collect(),
    }
}

pub(crate) fn plan_node_view(value: prism_ir::PlanNode) -> PlanNodeView {
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
        validation_refs: value
            .validation_refs
            .into_iter()
            .map(|check| ValidationRefView { id: check.id })
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

pub(crate) fn plan_edge_view(value: prism_ir::PlanEdge) -> PlanEdgeView {
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
    let effective_status = effective_coordination_task_status(&value);
    let lifecycle = coordination_task_lifecycle_view(effective_status, &value.git_execution);
    CoordinationTaskView {
        id: value.id.0.to_string(),
        plan_id: value.plan.0.to_string(),
        kind: value.kind,
        title: value.title,
        summary: value.summary,
        status: effective_status,
        published_task_status: value.published_task_status,
        assignee: value.assignee.map(|agent| agent.0.to_string()),
        pending_handoff_to: value.pending_handoff_to.map(|agent| agent.0.to_string()),
        anchors: value.anchors,
        bindings: plan_binding_view(value.bindings),
        depends_on: value
            .depends_on
            .into_iter()
            .map(|task_id| task_id.0.to_string())
            .collect(),
        coordination_depends_on: value
            .coordination_depends_on
            .into_iter()
            .map(|task_id| task_id.0.to_string())
            .collect(),
        integrated_depends_on: value
            .integrated_depends_on
            .into_iter()
            .map(|task_id| task_id.0.to_string())
            .collect(),
        lifecycle,
        validation_refs: value
            .validation_refs
            .into_iter()
            .map(|check| ValidationRefView { id: check.id })
            .collect(),
        is_abstract: value.is_abstract,
        base_revision: workspace_revision_view(value.base_revision),
        priority: value.priority,
        tags: value.tags,
        git_execution: TaskGitExecutionView {
            status: value.git_execution.status,
            pending_task_status: value.git_execution.pending_task_status,
            source_ref: value.git_execution.source_ref,
            target_ref: value.git_execution.target_ref,
            publish_ref: value.git_execution.publish_ref,
            target_branch: value.git_execution.target_branch,
            source_commit: value.git_execution.source_commit,
            publish_commit: value.git_execution.publish_commit,
            target_commit_at_publish: value.git_execution.target_commit_at_publish,
            review_artifact_ref: value.git_execution.review_artifact_ref,
            integration_commit: value.git_execution.integration_commit,
            integration_evidence: value.git_execution.integration_evidence,
            integration_mode: value.git_execution.integration_mode,
            integration_status: value.git_execution.integration_status,
            last_preflight: value
                .git_execution
                .last_preflight
                .map(git_preflight_report_view),
            last_publish: value
                .git_execution
                .last_publish
                .map(git_publish_report_view),
        },
    }
}

pub(crate) fn task_shaped_view_for_native_plan_node(
    value: prism_ir::PlanNode,
) -> CoordinationTaskView {
    coordination_task_view(prism_coordination::CoordinationTask {
        id: prism_ir::CoordinationTaskId::new(value.id.0.clone()),
        plan: value.plan_id,
        kind: value.kind,
        title: value.title,
        summary: value.summary,
        status: native_plan_node_status_as_coordination_status(value.status),
        published_task_status: None,
        assignee: value.assignee,
        pending_handoff_to: None,
        session: None,
        lease_holder: None,
        lease_started_at: None,
        lease_refreshed_at: None,
        lease_stale_at: None,
        lease_expires_at: None,
        worktree_id: None,
        branch_ref: None,
        anchors: Vec::new(),
        bindings: value.bindings,
        depends_on: Vec::new(),
        coordination_depends_on: Vec::new(),
        integrated_depends_on: Vec::new(),
        acceptance: Vec::new(),
        validation_refs: value.validation_refs,
        is_abstract: value.is_abstract,
        base_revision: value.base_revision,
        priority: value.priority,
        tags: value.tags,
        metadata: value.metadata,
        git_execution: prism_coordination::TaskGitExecution::default(),
    })
}

fn coordination_task_lifecycle_view(
    status: prism_ir::CoordinationTaskStatus,
    git_execution: &prism_coordination::TaskGitExecution,
) -> CoordinationTaskLifecycleView {
    let published_to_branch = matches!(
        git_execution.integration_status,
        prism_ir::GitIntegrationStatus::PublishedToBranch
            | prism_ir::GitIntegrationStatus::IntegrationPending
            | prism_ir::GitIntegrationStatus::IntegrationInProgress
            | prism_ir::GitIntegrationStatus::IntegratedToTarget
    );
    CoordinationTaskLifecycleView {
        completed: status == prism_ir::CoordinationTaskStatus::Completed,
        published_to_branch,
        coordination_published: git_execution.status
            == prism_ir::GitExecutionStatus::CoordinationPublished,
        integrated_to_target: git_execution.integration_status
            == prism_ir::GitIntegrationStatus::IntegratedToTarget,
    }
}

fn native_plan_node_status_as_coordination_status(
    status: prism_ir::PlanNodeStatus,
) -> prism_ir::CoordinationTaskStatus {
    match status {
        prism_ir::PlanNodeStatus::Proposed => prism_ir::CoordinationTaskStatus::Proposed,
        prism_ir::PlanNodeStatus::Ready => prism_ir::CoordinationTaskStatus::Ready,
        prism_ir::PlanNodeStatus::InProgress => prism_ir::CoordinationTaskStatus::InProgress,
        prism_ir::PlanNodeStatus::Blocked | prism_ir::PlanNodeStatus::Waiting => {
            prism_ir::CoordinationTaskStatus::Blocked
        }
        prism_ir::PlanNodeStatus::InReview => prism_ir::CoordinationTaskStatus::InReview,
        prism_ir::PlanNodeStatus::Validating => prism_ir::CoordinationTaskStatus::Validating,
        prism_ir::PlanNodeStatus::Completed => prism_ir::CoordinationTaskStatus::Completed,
        prism_ir::PlanNodeStatus::Abandoned => prism_ir::CoordinationTaskStatus::Abandoned,
    }
}

fn effective_coordination_task_status(
    task: &prism_coordination::CoordinationTask,
) -> prism_ir::CoordinationTaskStatus {
    if task.pending_handoff_to.is_some() {
        prism_ir::CoordinationTaskStatus::Blocked
    } else {
        task.published_task_status.unwrap_or(task.status)
    }
}

#[cfg(test)]
mod tests {
    use super::coordination_task_view;

    #[test]
    fn coordination_task_view_exposes_integration_aware_dependencies_and_lifecycle() {
        let view = coordination_task_view(prism_coordination::CoordinationTask {
            id: prism_ir::CoordinationTaskId::new("coord-task:view".to_string()),
            plan: prism_ir::PlanId::new("plan:view".to_string()),
            kind: prism_ir::PlanNodeKind::Edit,
            title: "Ship integration-aware dependency".to_string(),
            summary: None,
            status: prism_ir::CoordinationTaskStatus::Completed,
            published_task_status: None,
            assignee: None,
            pending_handoff_to: None,
            session: None,
            lease_holder: None,
            lease_started_at: None,
            lease_refreshed_at: None,
            lease_stale_at: None,
            lease_expires_at: None,
            worktree_id: None,
            branch_ref: None,
            anchors: Vec::new(),
            bindings: prism_ir::PlanBinding::default(),
            depends_on: vec![prism_ir::CoordinationTaskId::new(
                "coord-task:completed".to_string(),
            )],
            coordination_depends_on: vec![prism_ir::CoordinationTaskId::new(
                "coord-task:published".to_string(),
            )],
            integrated_depends_on: vec![prism_ir::CoordinationTaskId::new(
                "coord-task:integrated".to_string(),
            )],
            acceptance: Vec::new(),
            validation_refs: Vec::new(),
            is_abstract: false,
            base_revision: prism_ir::WorkspaceRevision::default(),
            priority: None,
            tags: Vec::new(),
            metadata: serde_json::Value::Null,
            git_execution: prism_coordination::TaskGitExecution {
                status: prism_ir::GitExecutionStatus::CoordinationPublished,
                integration_status: prism_ir::GitIntegrationStatus::IntegratedToTarget,
                ..prism_coordination::TaskGitExecution::default()
            },
        });

        assert_eq!(view.depends_on, vec!["coord-task:completed".to_string()]);
        assert_eq!(
            view.coordination_depends_on,
            vec!["coord-task:published".to_string()]
        );
        assert_eq!(
            view.integrated_depends_on,
            vec!["coord-task:integrated".to_string()]
        );
        assert!(view.lifecycle.completed);
        assert!(view.lifecycle.published_to_branch);
        assert!(view.lifecycle.coordination_published);
        assert!(view.lifecycle.integrated_to_target);
    }
}

pub(crate) fn claim_view(value: prism_coordination::WorkClaim) -> ClaimView {
    ClaimView {
        id: value.id.0.to_string(),
        holder: value.holder.0.to_string(),
        task_id: value.task.map(|task| task.0.to_string()),
        agent: value.agent.map(|agent| agent.0.to_string()),
        worktree_id: value.worktree_id,
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
        causes: value.causes.into_iter().map(blocker_cause_view).collect(),
    }
}

fn blocker_cause_view(value: prism_ir::BlockerCause) -> BlockerCauseView {
    BlockerCauseView {
        source: value.source,
        code: value.code,
        acceptance_label: value.acceptance_label,
        threshold_metric: value.threshold_metric,
        threshold_value: value.threshold_value,
        observed_value: value.observed_value,
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
