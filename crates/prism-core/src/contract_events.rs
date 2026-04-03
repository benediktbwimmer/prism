use std::path::Path;

use anyhow::{bail, Result};
use prism_projections::{ContractEvent, ContractPacket};

use crate::protected_state::repo_streams::inspect_protected_stream;
use crate::protected_state::streams::{ProtectedRepoStream, ProtectedVerificationStatus};
use crate::tracked_snapshot::{
    load_contract_snapshots, publish_context_from_event, sync_contract_snapshot,
    tracked_snapshot_authority_active,
};
use crate::util::repo_contract_events_path;

pub(crate) fn append_repo_contract_event(root: &Path, event: &ContractEvent) -> Result<()> {
    sync_contract_snapshot(
        root,
        &event.contract,
        &publish_context_from_event(
            event.actor.as_ref(),
            event.execution_context.as_ref(),
            event.recorded_at,
        ),
    )
}

pub(crate) fn load_repo_contract_events(root: &Path) -> Result<Vec<ContractEvent>> {
    let path = repo_contract_events_path(root);
    let inspection =
        inspect_protected_stream::<ContractEvent>(root, &ProtectedRepoStream::contract_events())?;
    if inspection.verification.verification_status != ProtectedVerificationStatus::Verified {
        bail!(
            "refused to hydrate repo contracts from {} because verification status is {:?}: {}",
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

pub(crate) fn load_repo_curated_contracts(root: &Path) -> Result<Vec<ContractPacket>> {
    let path = repo_contract_events_path(root);
    if tracked_snapshot_authority_active(root)? || !path.exists() {
        return load_contract_snapshots(root);
    }
    Ok(prism_projections::curated_contracts_from_events(
        &load_repo_contract_events(root)?,
    ))
}
