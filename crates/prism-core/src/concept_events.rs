use std::path::Path;

use anyhow::{bail, Result};
use prism_projections::{ConceptEvent, ConceptPacket};

use crate::protected_state::repo_streams::{
    append_protected_stream_event, implicit_principal_identity, inspect_protected_stream,
};
use crate::protected_state::streams::{ProtectedRepoStream, ProtectedVerificationStatus};
use crate::tracked_snapshot::{
    legacy_tracked_stream_bridge_active, load_concept_snapshots, publish_context_from_event,
    sync_concept_snapshot, tracked_snapshot_authority_active,
};
use crate::util::repo_concept_events_path;

pub(crate) fn append_repo_concept_event(root: &Path, event: &ConceptEvent) -> Result<()> {
    if legacy_tracked_stream_bridge_active(root)? {
        append_protected_stream_event(
            root,
            &ProtectedRepoStream::concept_events(),
            &event.id,
            event,
            &implicit_principal_identity(event.actor.as_ref(), event.execution_context.as_ref()),
        )?;
    }
    sync_concept_snapshot(
        root,
        &event.concept,
        &publish_context_from_event(
            event.actor.as_ref(),
            event.execution_context.as_ref(),
            event.recorded_at,
        ),
    )
}

pub(crate) fn load_repo_concept_events(root: &Path) -> Result<Vec<ConceptEvent>> {
    let path = repo_concept_events_path(root);
    let inspection =
        inspect_protected_stream::<ConceptEvent>(root, &ProtectedRepoStream::concept_events())?;
    if inspection.verification.verification_status != ProtectedVerificationStatus::Verified {
        bail!(
            "refused to hydrate repo concepts from {} because verification status is {:?}: {}",
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

pub(crate) fn load_repo_curated_concepts(root: &Path) -> Result<Vec<ConceptPacket>> {
    let path = repo_concept_events_path(root);
    if tracked_snapshot_authority_active(root)? || !path.exists() {
        return load_concept_snapshots(root);
    }
    Ok(prism_projections::curated_concepts_from_events(
        &load_repo_concept_events(root)?,
    ))
}
