use std::path::Path;

use anyhow::{bail, Result};
use prism_projections::{ConceptRelation, ConceptRelationEvent};

use crate::protected_state::repo_streams::inspect_protected_stream;
use crate::protected_state::streams::{ProtectedRepoStream, ProtectedVerificationStatus};
use crate::tracked_snapshot::{
    apply_concept_relation_snapshot, load_relation_snapshots, publish_context_from_event,
    tracked_snapshot_authority_active,
};
use crate::util::repo_concept_relations_path;

pub(crate) fn append_repo_concept_relation_event(
    root: &Path,
    event: &ConceptRelationEvent,
) -> Result<()> {
    apply_concept_relation_snapshot(
        root,
        event,
        &publish_context_from_event(
            event.actor.as_ref(),
            event.execution_context.as_ref(),
            event.recorded_at,
        ),
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
    let path = repo_concept_relations_path(root);
    if tracked_snapshot_authority_active(root)? || !path.exists() {
        return load_relation_snapshots(root);
    }
    Ok(prism_projections::concept_relations_from_events(
        &load_repo_concept_relation_events(root)?,
    ))
}
