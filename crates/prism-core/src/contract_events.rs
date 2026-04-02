use std::path::Path;

use anyhow::{bail, Result};
use prism_projections::{curated_contracts_from_events, ContractEvent, ContractPacket};

use crate::protected_state::repo_streams::{
    append_protected_stream_event, implicit_principal_identity, inspect_protected_stream,
};
use crate::protected_state::streams::{ProtectedRepoStream, ProtectedVerificationStatus};
use crate::util::repo_contract_events_path;

pub(crate) fn append_repo_contract_event(root: &Path, event: &ContractEvent) -> Result<()> {
    append_protected_stream_event(
        root,
        &ProtectedRepoStream::contract_events(),
        &event.id,
        event,
        &implicit_principal_identity(event.actor.as_ref(), event.execution_context.as_ref()),
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
    Ok(curated_contracts_from_events(&load_repo_contract_events(
        root,
    )?))
}
