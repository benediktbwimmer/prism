use anyhow::{anyhow, Result};
use prism_memory::{MemoryEvent, MemoryScope};
use prism_projections::{ConceptEvent, ConceptPacket, ConceptProvenance, ConceptPublicationStatus};
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
    validate_repo_concept_packet(&event.concept)
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
