use anyhow::{anyhow, Result};
use prism_curator::{
    CuratorJobRecord, CuratorProposal, CuratorProposalDisposition, CuratorTrigger,
};
use prism_ir::{AnchorRef, Edge, NodeId, WorkspaceRevision};
use prism_js::{
    AnchorRefView, ArtifactRiskView, ArtifactView, BlockerView, ChangeImpactView, ClaimView,
    CoChangeView, ConceptBindingMetadataView, ConceptCurationHintsView, ConceptDecodeLensView,
    ConceptPacketTruncationView, ConceptPacketVerbosityView, ConceptPacketView,
    ConceptProvenanceView, ConceptPublicationStatusView, ConceptPublicationView,
    ConceptRelationDirectionView, ConceptRelationKindView, ConceptRelationView,
    ConceptResolutionView, ConceptScopeView, ConflictView, ContractCompatibilityView,
    ContractGuaranteeStrengthView, ContractGuaranteeView, ContractHealthSignalsView,
    ContractHealthStatusView, ContractHealthView, ContractKindView, ContractPacketView,
    ContractResolutionView, ContractStabilityView, ContractStatusView, ContractTargetView,
    ContractValidationView, CoordinationTaskView, CuratorJobView, CuratorProposalRecordView,
    CuratorProposalView, DriftCandidateView, EdgeView, MemoryEntryView, MemoryEventView,
    NodeIdView, PlanAcceptanceCriterionView, PlanBindingView, PlanEdgeView,
    PlanExecutionOverlayView, PlanGraphView, PlanListEntryView, PlanNodeBlockerView,
    PlanNodeRecommendationView, PlanNodeView, PlanSummaryView, PlanView, PolicyViolationRecordView,
    PolicyViolationView, QueryDiagnostic, ScoredMemoryView, TaskIntentView, TaskRiskView,
    TaskValidationRecipeView, ValidationCheckView, ValidationRecipeView, ValidationRefView,
    WorkspaceRevisionView,
};
use prism_memory::{MemoryEntry, MemoryEvent, MemorySource, ScoredMemory};
use prism_query::{
    ArtifactRisk, ChangeImpact, CoChange, ConceptDecodeLens, ConceptPacket, ConceptProvenance,
    ConceptPublication, ConceptPublicationStatus, ConceptRelation, ConceptRelationKind,
    ConceptResolution, ConceptScope, ContractCompatibility, ContractGuarantee,
    ContractGuaranteeStrength, ContractHealth, ContractHealthSignals, ContractHealthStatus,
    ContractKind, ContractPacket, ContractResolution, ContractStability, ContractStatus,
    ContractTarget, ContractValidation, DriftCandidate, PlanListEntry, PlanNodeRecommendation,
    PlanSummary, Prism, TaskIntent, TaskRisk, TaskValidationRecipe, ValidationCheck,
    ValidationRecipe,
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
    let curation_hints = concept_curation_hints_view_from_packet(&packet, verbosity);
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
    packet: &ConceptPacket,
    verbosity: ConceptVerbosity,
) -> ConceptCurationHintsView {
    let inspect_first = packet
        .core_members
        .first()
        .cloned()
        .map(node_id_view)
        .or_else(|| packet.supporting_members.first().cloned().map(node_id_view))
        .or_else(|| packet.likely_tests.first().cloned().map(node_id_view));
    let supporting_read = packet.supporting_members.first().cloned().map(node_id_view);
    let likely_test = packet.likely_tests.first().cloned().map(node_id_view);
    let next_action = concept_packet_next_action(
        packet,
        inspect_first.as_ref(),
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
    likely_test: Option<&NodeIdView>,
    verbosity: ConceptVerbosity,
) -> String {
    let inspect_target = inspect_first.map(|node| format!("`{}`", node.path));
    let likely_test = likely_test.map(|node| format!("`{}`", node.path));
    let validation_lens = packet
        .decode_lenses
        .iter()
        .any(|lens| matches!(lens, ConceptDecodeLens::Validation));

    match (inspect_target, likely_test, validation_lens, verbosity) {
        (Some(target), Some(test), _, ConceptVerbosity::Summary) => format!(
            "Inspect {target} first, verify it with likely test {test}, then retry with `verbosity: \"full\"` if you need the remaining members."
        ),
        (Some(target), Some(test), _, _) => {
            format!("Inspect {target} first, then verify it with likely test {test}.")
        }
        (Some(target), None, true, ConceptVerbosity::Summary) => format!(
            "Inspect {target} first, then use `prism.decodeConcept({{ handle: \"{}\", lens: \"validation\" }})` for broader validation context or retry with `verbosity: \"full\"`.",
            packet.handle
        ),
        (Some(target), None, true, _) => format!(
            "Inspect {target} first, then use `prism.decodeConcept({{ handle: \"{}\", lens: \"validation\" }})` for broader validation context.",
            packet.handle
        ),
        (Some(target), None, false, ConceptVerbosity::Summary) => format!(
            "Inspect {target} first, then retry with `verbosity: \"full\"` if you need the remaining members or tests."
        ),
        (Some(target), None, false, _) => {
            format!("Inspect {target} first, then expand outward from the remaining concept members.")
        }
        (None, Some(test), _, _) => format!(
            "Start with likely test {test}, then add or refresh stronger concept member bindings before relying on this packet."
        ),
        (None, None, _, _) => {
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
) -> PlanView {
    PlanView {
        id: value.id.0.to_string(),
        goal: value.goal,
        status: value.status,
        root_node_ids: root_node_ids
            .into_iter()
            .map(|node_id| node_id.0.to_string())
            .collect(),
    }
}

pub(crate) fn plan_list_entry_view(value: PlanListEntry) -> PlanListEntryView {
    PlanListEntryView {
        plan_id: value.plan_id.0.to_string(),
        title: value.title,
        goal: value.goal,
        status: value.status,
        scope: value.scope,
        kind: value.kind,
        root_node_ids: value
            .root_node_ids
            .into_iter()
            .map(|node_id| node_id.0.to_string())
            .collect(),
        summary: plan_summary_view(value.summary),
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
    CoordinationTaskView {
        id: value.id.0.to_string(),
        plan_id: value.plan.0.to_string(),
        kind: value.kind,
        title: value.title,
        summary: value.summary,
        status: value.status,
        assignee: value.assignee.map(|agent| agent.0.to_string()),
        pending_handoff_to: value.pending_handoff_to.map(|agent| agent.0.to_string()),
        anchors: value.anchors,
        bindings: plan_binding_view(value.bindings),
        depends_on: value
            .depends_on
            .into_iter()
            .map(|task_id| task_id.0.to_string())
            .collect(),
        validation_refs: value
            .validation_refs
            .into_iter()
            .map(|check| ValidationRefView { id: check.id })
            .collect(),
        is_abstract: value.is_abstract,
        base_revision: workspace_revision_view(value.base_revision),
        priority: value.priority,
        tags: value.tags,
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
