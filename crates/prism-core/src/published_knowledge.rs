use anyhow::{anyhow, Result};
use prism_ir::EventActor;
use prism_memory::{MemoryEvent, MemoryScope, OutcomeEvent, OutcomeKind, OutcomeResult};
use prism_projections::{
    ConceptEvent, ConceptPacket, ConceptProvenance, ConceptPublicationStatus, ConceptRelation,
    ConceptRelationEvent, ContractEvent, ContractPacket, ContractStatus, ContractTarget,
};
use serde_json::Value;

pub(crate) fn validate_repo_memory_event(event: &MemoryEvent) -> Result<()> {
    if event.scope != MemoryScope::Repo {
        return Ok(());
    }
    let Some(entry) = event.entry.as_ref() else {
        return Err(anyhow!(
            "repo-published memory event `{}` must include an entry payload",
            event.id
        ));
    };
    if entry.scope != MemoryScope::Repo {
        return Err(anyhow!(
            "repo-published memory event `{}` must contain a repo-scoped entry",
            event.id
        ));
    }
    if entry.anchors.is_empty() {
        return Err(anyhow!(
            "repo-published memory `{}` must include at least one anchor",
            entry.id.0
        ));
    }
    if entry.content.trim().chars().count() < 24 {
        return Err(anyhow!(
            "repo-published memory `{}` must contain at least 24 characters of durable content",
            entry.id.0
        ));
    }
    if entry.trust < 0.7 {
        return Err(anyhow!(
            "repo-published memory `{}` must have trust >= 0.7",
            entry.id.0
        ));
    }
    validate_repo_memory_metadata(&entry.metadata)
}

pub(crate) fn validate_repo_concept_event(event: &ConceptEvent) -> Result<()> {
    if event.concept.scope != prism_projections::ConceptScope::Repo {
        return Ok(());
    }
    validate_repo_concept_packet(&event.concept)
}

pub(crate) fn validate_repo_concept_relation_event(event: &ConceptRelationEvent) -> Result<()> {
    if event.relation.scope != prism_projections::ConceptScope::Repo {
        return Ok(());
    }
    validate_repo_concept_relation(&event.relation)
}

pub(crate) fn validate_repo_contract_event(event: &ContractEvent) -> Result<()> {
    if event.contract.scope != prism_projections::ContractScope::Repo {
        return Ok(());
    }
    validate_repo_contract_packet(&event.contract)
}

pub(crate) fn validate_repo_patch_event(event: &OutcomeEvent) -> Result<()> {
    if event.kind != OutcomeKind::PatchApplied {
        return Err(anyhow!(
            "repo-published patch event `{}` must use PatchApplied kind",
            event.meta.id.0
        ));
    }
    if event.result != OutcomeResult::Success {
        return Err(anyhow!(
            "repo-published patch event `{}` must have a successful result",
            event.meta.id.0
        ));
    }
    if matches!(event.meta.actor, EventActor::System) {
        return Err(anyhow!(
            "repo-published patch event `{}` must record a non-system actor",
            event.meta.id.0
        ));
    }
    let Some(context) = event.meta.execution_context.as_ref() else {
        return Err(anyhow!(
            "repo-published patch event `{}` must include execution context",
            event.meta.id.0
        ));
    };
    let Some(work) = context.work_context.as_ref() else {
        return Err(anyhow!(
            "repo-published patch event `{}` must include work context",
            event.meta.id.0
        ));
    };
    if work.work_id.trim().is_empty() || work.title.trim().is_empty() {
        return Err(anyhow!(
            "repo-published patch event `{}` must include non-empty work id and title",
            event.meta.id.0
        ));
    }
    let Some(metadata) = event.metadata.as_object() else {
        return Err(anyhow!(
            "repo-published patch event `{}` must include metadata",
            event.meta.id.0
        ));
    };
    let reason = metadata.get("reason").and_then(Value::as_str).unwrap_or("");
    if reason.trim().is_empty() {
        return Err(anyhow!(
            "repo-published patch event `{}` must include a non-empty provenance reason",
            event.meta.id.0
        ));
    }
    let file_paths = metadata
        .get("filePaths")
        .and_then(Value::as_array)
        .map(|paths| paths.iter().filter_map(Value::as_str).collect::<Vec<_>>())
        .unwrap_or_default();
    if file_paths.is_empty() {
        return Err(anyhow!(
            "repo-published patch event `{}` must include filePaths",
            event.meta.id.0
        ));
    }
    Ok(())
}

fn validate_repo_memory_metadata(metadata: &Value) -> Result<()> {
    let Some(metadata) = metadata.as_object() else {
        return Err(anyhow!(
            "repo-published memory metadata must be an object with provenance and publication"
        ));
    };
    let Some(provenance) = metadata.get("provenance").and_then(Value::as_object) else {
        return Err(anyhow!(
            "repo-published memory metadata must include provenance"
        ));
    };
    if provenance
        .get("origin")
        .and_then(Value::as_str)
        .is_none_or(|value| value.trim().is_empty())
    {
        return Err(anyhow!(
            "repo-published memory provenance must include a non-empty origin"
        ));
    }
    if provenance
        .get("kind")
        .and_then(Value::as_str)
        .is_none_or(|value| value.trim().is_empty())
    {
        return Err(anyhow!(
            "repo-published memory provenance must include a non-empty kind"
        ));
    }
    let Some(publication) = metadata.get("publication").and_then(Value::as_object) else {
        return Err(anyhow!(
            "repo-published memory metadata must include publication"
        ));
    };
    if publication
        .get("publishedAt")
        .and_then(Value::as_u64)
        .is_none_or(|value| value == 0)
    {
        return Err(anyhow!(
            "repo-published memory publication must include publishedAt"
        ));
    }
    if publication
        .get("status")
        .and_then(Value::as_str)
        .is_none_or(|value| value.trim().is_empty())
    {
        return Err(anyhow!(
            "repo-published memory publication must include status"
        ));
    }
    if publication
        .get("status")
        .and_then(Value::as_str)
        .is_some_and(|value| value.eq_ignore_ascii_case("retired"))
    {
        if publication
            .get("retiredAt")
            .and_then(Value::as_u64)
            .is_none_or(|value| value == 0)
        {
            return Err(anyhow!(
                "retired repo-published memory publication must include retiredAt"
            ));
        }
        if publication
            .get("retirementReason")
            .and_then(Value::as_str)
            .is_none_or(|value| value.trim().is_empty())
        {
            return Err(anyhow!(
                "retired repo-published memory publication must include retirementReason"
            ));
        }
    }
    Ok(())
}

fn validate_repo_concept_packet(packet: &ConceptPacket) -> Result<()> {
    if packet.handle.trim().is_empty() {
        return Err(anyhow!("repo-published concept handle cannot be empty"));
    }
    if packet.canonical_name.trim().is_empty() {
        return Err(anyhow!(
            "repo-published concept canonical name cannot be empty"
        ));
    }
    if packet.summary.trim().chars().count() < 24 {
        return Err(anyhow!(
            "repo-published concept summary must contain at least 24 characters"
        ));
    }
    if packet.core_members.len() < 2 || packet.core_members.len() > 5 {
        return Err(anyhow!(
            "repo-published concept coreMembers must contain 2 to 5 central members"
        ));
    }
    if packet.evidence.is_empty() {
        return Err(anyhow!("repo-published concept evidence cannot be empty"));
    }
    if packet.confidence < 0.7 {
        return Err(anyhow!(
            "repo-published concept confidence must be at least 0.7"
        ));
    }
    if packet.decode_lenses.is_empty() {
        return Err(anyhow!(
            "repo-published concept decodeLenses cannot be empty"
        ));
    }
    let Some(publication) = packet.publication.as_ref() else {
        return Err(anyhow!(
            "repo-published concept must include publication metadata"
        ));
    };
    if publication.published_at == 0 {
        return Err(anyhow!(
            "repo-published concept publication must include publishedAt"
        ));
    }
    if publication.status == ConceptPublicationStatus::Retired
        && publication
            .retirement_reason
            .as_deref()
            .unwrap_or("")
            .trim()
            .is_empty()
    {
        return Err(anyhow!(
            "retired repo-published concept must include retirementReason"
        ));
    }
    if packet.provenance == ConceptProvenance::default() {
        return Err(anyhow!(
            "repo-published concept must include provenance metadata"
        ));
    }
    Ok(())
}

fn validate_repo_concept_relation(relation: &ConceptRelation) -> Result<()> {
    if relation.source_handle.trim().is_empty() || relation.target_handle.trim().is_empty() {
        return Err(anyhow!(
            "repo-published concept relations must include non-empty source and target handles"
        ));
    }
    if relation
        .source_handle
        .eq_ignore_ascii_case(&relation.target_handle)
    {
        return Err(anyhow!(
            "repo-published concept relations cannot point to the same concept on both sides"
        ));
    }
    if relation.confidence < 0.7 {
        return Err(anyhow!(
            "repo-published concept relation confidence must be at least 0.7"
        ));
    }
    if relation.evidence.is_empty() {
        return Err(anyhow!(
            "repo-published concept relations must include evidence"
        ));
    }
    if relation.provenance == ConceptProvenance::default() {
        return Err(anyhow!(
            "repo-published concept relations must include provenance metadata"
        ));
    }
    Ok(())
}

fn validate_repo_contract_packet(packet: &ContractPacket) -> Result<()> {
    if packet.handle.trim().is_empty() {
        return Err(anyhow!("repo-published contract handle cannot be empty"));
    }
    if packet.name.trim().is_empty() {
        return Err(anyhow!("repo-published contract name cannot be empty"));
    }
    if packet.summary.trim().chars().count() < 24 {
        return Err(anyhow!(
            "repo-published contract summary must contain at least 24 characters"
        ));
    }
    if !contract_target_has_refs(&packet.subject) {
        return Err(anyhow!(
            "repo-published contract subject must include at least one anchor or concept handle"
        ));
    }
    if packet.guarantees.is_empty() {
        return Err(anyhow!(
            "repo-published contract must include at least one guarantee"
        ));
    }
    if packet
        .guarantees
        .iter()
        .any(|guarantee| guarantee.statement.trim().is_empty() || guarantee.id.trim().is_empty())
    {
        return Err(anyhow!(
            "repo-published contract guarantees must have non-empty ids and statements"
        ));
    }
    let unique_guarantee_ids = packet
        .guarantees
        .iter()
        .map(|guarantee| guarantee.id.to_ascii_lowercase())
        .collect::<std::collections::HashSet<_>>();
    if unique_guarantee_ids.len() != packet.guarantees.len() {
        return Err(anyhow!(
            "repo-published contract guarantee ids must be unique within a packet"
        ));
    }
    if packet.evidence.is_empty() {
        return Err(anyhow!("repo-published contract evidence cannot be empty"));
    }
    if packet.status == ContractStatus::Candidate {
        return Err(anyhow!(
            "repo-published contract status cannot remain candidate"
        ));
    }
    let Some(publication) = packet.publication.as_ref() else {
        return Err(anyhow!(
            "repo-published contract must include publication metadata"
        ));
    };
    if publication.published_at == 0 {
        return Err(anyhow!(
            "repo-published contract publication must include publishedAt"
        ));
    }
    if publication.status == ConceptPublicationStatus::Retired
        && publication
            .retirement_reason
            .as_deref()
            .unwrap_or("")
            .trim()
            .is_empty()
    {
        return Err(anyhow!(
            "retired repo-published contract must include retirementReason"
        ));
    }
    if packet.provenance == ConceptProvenance::default() {
        return Err(anyhow!(
            "repo-published contract must include provenance metadata"
        ));
    }
    Ok(())
}

fn contract_target_has_refs(target: &ContractTarget) -> bool {
    !target.anchors.is_empty()
        || target
            .concept_handles
            .iter()
            .any(|handle| !handle.trim().is_empty())
}
