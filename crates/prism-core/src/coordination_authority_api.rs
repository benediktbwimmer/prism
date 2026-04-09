use std::path::Path;

use anyhow::{anyhow, Result};
use prism_store::CoordinationStartupCheckpointAuthority;

use crate::coordination_authority_store::{
    default_coordination_authority_store_provider, CoordinationAuthorityBackendDetails,
    CoordinationDiagnosticsRequest, CoordinationReadRequest, CoordinationStateView,
    CoordinationTransactionBase, RuntimeDescriptorPublishRequest,
};
use crate::coordination_reads::CoordinationReadConsistency;
use crate::shared_coordination_ref::{
    build_local_runtime_descriptor_for_current_state, initialize_shared_coordination_ref_live_sync,
    poll_shared_coordination_ref_live_sync, SharedCoordinationRefDiagnostics,
    SharedCoordinationRefLiveSync,
};

#[derive(Debug, Clone, PartialEq)]
pub(crate) enum CoordinationAuthorityLiveSync {
    Unchanged,
    Changed(crate::CoordinationCurrentState),
}

pub fn shared_coordination_ref_diagnostics(
    root: &Path,
) -> Result<Option<SharedCoordinationRefDiagnostics>> {
    let store = default_coordination_authority_store_provider().open(root)?;
    let diagnostics = store.diagnostics(CoordinationDiagnosticsRequest {
        include_backend_details: true,
    })?;
    match diagnostics.backend_details {
        CoordinationAuthorityBackendDetails::GitSharedRefs(value) => Ok(Some(value)),
        CoordinationAuthorityBackendDetails::Sqlite { .. } => Ok(None),
        CoordinationAuthorityBackendDetails::Unavailable => Ok(None),
    }
}

pub fn sync_live_runtime_descriptor(root: &Path) -> Result<()> {
    let store = default_coordination_authority_store_provider().open(root)?;
    let descriptor = build_local_runtime_descriptor_for_current_state(root)?;
    let result = store.publish_runtime_descriptor(RuntimeDescriptorPublishRequest {
        base: CoordinationTransactionBase::LatestStrong,
        descriptor,
    })?;
    if result.committed {
        return Ok(());
    }
    Err(anyhow!(
        "runtime descriptor publication did not commit successfully: {:?}",
        result.status
    ))
}

pub(crate) fn shared_coordination_startup_authority(
    root: &Path,
) -> Result<Option<CoordinationStartupCheckpointAuthority>> {
    let store = default_coordination_authority_store_provider().open(root)?;
    let authority = store
        .read_current(CoordinationReadRequest {
            consistency: CoordinationReadConsistency::Strong,
            view: CoordinationStateView::Summary,
        })?
        .authority;
    Ok(
        authority.map(|authority| CoordinationStartupCheckpointAuthority {
            ref_name: authority.provenance.ref_name.unwrap_or_else(|| {
                match authority.backend_kind {
                    crate::CoordinationAuthorityBackendKind::GitSharedRefs => {
                        "shared-coordination".to_string()
                    }
                    crate::CoordinationAuthorityBackendKind::Sqlite => {
                        "sqlite-authority".to_string()
                    }
                    crate::CoordinationAuthorityBackendKind::Postgres => {
                        "postgres-authority".to_string()
                    }
                }
            }),
            head_commit: authority.provenance.head_commit,
            manifest_digest: authority.provenance.manifest_digest,
        }),
    )
}

pub(crate) fn initialize_coordination_authority_live_sync(root: &Path) -> Result<()> {
    initialize_shared_coordination_ref_live_sync(root)
}

pub(crate) fn poll_coordination_authority_live_sync(
    root: &Path,
) -> Result<CoordinationAuthorityLiveSync> {
    Ok(match poll_shared_coordination_ref_live_sync(root)? {
        SharedCoordinationRefLiveSync::Unchanged => CoordinationAuthorityLiveSync::Unchanged,
        SharedCoordinationRefLiveSync::Changed(shared) => {
            CoordinationAuthorityLiveSync::Changed(crate::CoordinationCurrentState {
                snapshot: shared.snapshot,
                canonical_snapshot_v2: shared.canonical_snapshot_v2,
                runtime_descriptors: shared.runtime_descriptors,
            })
        }
    })
}
