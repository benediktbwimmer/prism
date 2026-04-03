use std::path::Path;

use anyhow::{bail, Result};
use prism_memory::{OutcomeEvent, OutcomeKind, OutcomeMemory};

use crate::protected_state::repo_streams::{
    append_protected_stream_event, implicit_principal_identity, inspect_protected_stream,
};
use crate::protected_state::streams::{ProtectedRepoStream, ProtectedVerificationStatus};
use crate::tracked_snapshot::{
    append_patch_snapshot, legacy_tracked_stream_bridge_active, load_patch_snapshots,
    publish_context_from_event, tracked_snapshot_authority_active,
};
use crate::util::repo_patch_events_path;

pub(crate) fn append_repo_patch_event(root: &Path, event: &OutcomeEvent) -> Result<()> {
    if legacy_tracked_stream_bridge_active(root)? {
        append_protected_stream_event(
            root,
            &ProtectedRepoStream::patch_events(),
            event.meta.id.0.as_str(),
            event,
            &implicit_principal_identity(
                Some(&event.meta.actor),
                event.meta.execution_context.as_ref(),
            ),
        )?;
    }
    append_patch_snapshot(
        root,
        event,
        &publish_context_from_event(
            Some(&event.meta.actor),
            event.meta.execution_context.as_ref(),
            event.meta.ts,
        ),
    )
}

pub(crate) fn load_repo_patch_events(root: &Path) -> Result<Vec<OutcomeEvent>> {
    let path = repo_patch_events_path(root);
    if tracked_snapshot_authority_active(root)? || !path.exists() {
        let snapshots = load_patch_snapshots(root)?;
        for event in &snapshots {
            if event.kind != OutcomeKind::PatchApplied {
                bail!(
                    "tracked patch snapshot contained non-patch outcome `{}`",
                    event.meta.id.0
                );
            }
        }
        return Ok(snapshots);
    }

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
