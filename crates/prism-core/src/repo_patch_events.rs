use std::path::Path;

use anyhow::{bail, Result};
use prism_memory::{OutcomeEvent, OutcomeMemory, OutcomeKind};

use crate::protected_state::repo_streams::{
    append_protected_stream_event, implicit_principal_identity, inspect_protected_stream,
};
use crate::protected_state::streams::{ProtectedRepoStream, ProtectedVerificationStatus};
use crate::util::repo_patch_events_path;

pub(crate) fn append_repo_patch_event(root: &Path, event: &OutcomeEvent) -> Result<()> {
    append_protected_stream_event(
        root,
        &ProtectedRepoStream::patch_events(),
        event.meta.id.as_str(),
        event,
        &implicit_principal_identity(Some(&event.meta.actor), event.meta.execution_context.as_ref()),
    )
}

pub(crate) fn load_repo_patch_events(root: &Path) -> Result<Vec<OutcomeEvent>> {
    let path = repo_patch_events_path(root);
    let inspection =
        inspect_protected_stream::<OutcomeEvent>(root, &ProtectedRepoStream::patch_events())?;
    if inspection.verification.verification_status != ProtectedVerificationStatus::Verified {
        bail!(
            "refused to hydrate repo patch events from {} because verification status is {:?}: {}",
            path.display(),
            inspection.verification.verification_status,
            inspection
                .verification
                .diagnostic_summary
                .as_deref()
                .unwrap_or("verification failed")
        );
    }
    for event in &inspection.payloads {
        if event.kind != OutcomeKind::PatchApplied {
            bail!(
                "repo patch log {} contained non-patch outcome `{}`",
                path.display(),
                event.meta.id.0
            );
        }
    }
    Ok(inspection.payloads)
}

pub(crate) fn merge_repo_patch_events_into_memory(
    root: &Path,
    outcomes: &OutcomeMemory,
) -> Result<usize> {
    let events = load_repo_patch_events(root)?;
    let count = events.len();
    for event in events {
        let _ = outcomes.store_event(event)?;
    }
    Ok(count)
}

pub(crate) fn sync_repo_patch_events<S: prism_store::EventJournalStore>(
    root: &Path,
    store: &mut S,
) -> Result<bool> {
    let events = load_repo_patch_events(root)?;
    if events.is_empty() {
        return Ok(false);
    }
    Ok(prism_store::EventJournalStore::append_outcome_events(store, &events, &[])? > 0)
}
