use std::path::Path;

use anyhow::{bail, Result};
use prism_projections::{concept_relations_from_events, ConceptRelation, ConceptRelationEvent};

use crate::protected_state::repo_streams::{
    append_protected_stream_event, implicit_principal_identity, inspect_protected_stream,
};
use crate::protected_state::streams::{ProtectedRepoStream, ProtectedVerificationStatus};
use crate::util::repo_concept_relations_path;

pub(crate) fn append_repo_concept_relation_event(
    root: &Path,
    event: &ConceptRelationEvent,
) -> Result<()> {
    append_protected_stream_event(
        root,
        &ProtectedRepoStream::concept_relations(),
        &event.id,
        event,
        &implicit_principal_identity(event.actor.as_ref(), event.execution_context.as_ref()),
    )
}

pub(crate) fn load_repo_concept_relation_events(root: &Path) -> Result<Vec<ConceptRelationEvent>> {
    let path = repo_concept_relations_path(root);
    let inspection = inspect_protected_stream::<ConceptRelationEvent>(
        root,
        &ProtectedRepoStream::concept_relations(),
    )?;
    if inspection.verification.verification_status != ProtectedVerificationStatus::Verified {
        bail!(
            "refused to hydrate repo concept relations from {} because verification status is {:?}: {}",
            path.display(),
            inspection.verification.verification_status,
            inspection
                .verification
                .diagnostic_summary
                .as_deref()
                .unwrap_or("verification failed")
        );
    }
    Ok(inspection.payloads)
}

pub(crate) fn load_repo_concept_relations(root: &Path) -> Result<Vec<ConceptRelation>> {
    Ok(concept_relations_from_events(
        &load_repo_concept_relation_events(root)?,
    ))
}
