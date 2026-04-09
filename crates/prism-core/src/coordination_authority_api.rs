use std::path::Path;

use anyhow::{anyhow, Result};
use prism_store::CoordinationStartupCheckpointAuthority;

use crate::coordination_authority_store::{
    configured_coordination_authority_store_provider, CoordinationAuthorityBackendDetails,
    CoordinationAuthorityBackendKind, CoordinationAuthorityDiagnostics,
    CoordinationAuthorityStoreProvider, CoordinationDiagnosticsRequest, CoordinationReadRequest,
    CoordinationStateView, CoordinationTransactionBase, RuntimeDescriptorPublishRequest,
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

pub fn coordination_authority_diagnostics(root: &Path) -> Result<CoordinationAuthorityDiagnostics> {
    coordination_authority_diagnostics_with_provider(
        root,
        &configured_coordination_authority_store_provider(root)?,
    )
}

pub fn coordination_authority_diagnostics_with_provider(
    root: &Path,
    provider: &CoordinationAuthorityStoreProvider,
) -> Result<CoordinationAuthorityDiagnostics> {
    let store = provider.open(root)?;
    store.diagnostics(CoordinationDiagnosticsRequest {
        include_backend_details: true,
    })
}

pub fn git_shared_coordination_ref_diagnostics(
    root: &Path,
) -> Result<Option<SharedCoordinationRefDiagnostics>> {
    git_shared_coordination_ref_diagnostics_with_provider(
        root,
        &configured_coordination_authority_store_provider(root)?,
    )
}

pub fn git_shared_coordination_ref_diagnostics_with_provider(
    root: &Path,
    provider: &CoordinationAuthorityStoreProvider,
) -> Result<Option<SharedCoordinationRefDiagnostics>> {
    let diagnostics = coordination_authority_diagnostics_with_provider(root, provider)?;
    match diagnostics.backend_details {
        CoordinationAuthorityBackendDetails::GitSharedRefs(value) => Ok(Some(value)),
        CoordinationAuthorityBackendDetails::Sqlite(_)
        | CoordinationAuthorityBackendDetails::Postgres(_)
        | CoordinationAuthorityBackendDetails::Unavailable => Ok(None),
    }
}

pub fn publish_local_runtime_descriptor(root: &Path) -> Result<()> {
    publish_local_runtime_descriptor_with_provider(
        root,
        &configured_coordination_authority_store_provider(root)?,
    )
}

pub fn publish_local_runtime_descriptor_with_provider(
    root: &Path,
    provider: &CoordinationAuthorityStoreProvider,
) -> Result<()> {
    let store = provider.open(root)?;
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

pub fn sync_live_runtime_descriptor(root: &Path) -> Result<()> {
    publish_local_runtime_descriptor(root)
}

pub fn sync_live_runtime_descriptor_with_provider(
    root: &Path,
    provider: &CoordinationAuthorityStoreProvider,
) -> Result<()> {
    publish_local_runtime_descriptor_with_provider(root, provider)
}

pub(crate) fn coordination_startup_checkpoint_authority(
    root: &Path,
) -> Result<Option<CoordinationStartupCheckpointAuthority>> {
    coordination_startup_checkpoint_authority_with_provider(
        root,
        &configured_coordination_authority_store_provider(root)?,
    )
}

pub(crate) fn coordination_startup_checkpoint_authority_with_provider(
    root: &Path,
    provider: &CoordinationAuthorityStoreProvider,
) -> Result<Option<CoordinationStartupCheckpointAuthority>> {
    let store = provider.open(root)?;
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

pub(crate) fn coordination_authority_live_sync_enabled(root: &Path) -> Result<bool> {
    match coordination_authority_diagnostics(root)?.backend_kind {
        CoordinationAuthorityBackendKind::GitSharedRefs => {
            initialize_shared_coordination_ref_live_sync(root)?;
            Ok(true)
        }
        CoordinationAuthorityBackendKind::Sqlite | CoordinationAuthorityBackendKind::Postgres => {
            Ok(false)
        }
    }
}

pub(crate) fn poll_coordination_authority_live_sync(
    root: &Path,
) -> Result<CoordinationAuthorityLiveSync> {
    match coordination_authority_diagnostics(root)?.backend_kind {
        CoordinationAuthorityBackendKind::GitSharedRefs => {
            Ok(match poll_shared_coordination_ref_live_sync(root)? {
                SharedCoordinationRefLiveSync::Unchanged => {
                    CoordinationAuthorityLiveSync::Unchanged
                }
                SharedCoordinationRefLiveSync::Changed(shared) => {
                    CoordinationAuthorityLiveSync::Changed(crate::CoordinationCurrentState {
                        snapshot: shared.snapshot,
                        canonical_snapshot_v2: shared.canonical_snapshot_v2,
                        runtime_descriptors: shared.runtime_descriptors,
                    })
                }
            })
        }
        CoordinationAuthorityBackendKind::Sqlite | CoordinationAuthorityBackendKind::Postgres => {
            Ok(CoordinationAuthorityLiveSync::Unchanged)
        }
    }
}

#[cfg(test)]
mod tests {
    use std::fs;
    use std::sync::atomic::{AtomicU64, Ordering};
    use std::time::{SystemTime, UNIX_EPOCH};

    use super::coordination_authority_live_sync_enabled;

    static NEXT_TEMP_ROOT: AtomicU64 = AtomicU64::new(0);

    fn temp_workspace_root() -> std::path::PathBuf {
        let nonce = NEXT_TEMP_ROOT.fetch_add(1, Ordering::Relaxed);
        let unique = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        let root = std::env::temp_dir().join(format!("prism-authority-sync-{unique}-{nonce}"));
        let _ = fs::remove_dir_all(&root);
        fs::create_dir_all(&root).unwrap();
        root
    }

    #[test]
    fn sqlite_default_backend_disables_authority_live_sync() {
        let root = temp_workspace_root();
        assert!(!coordination_authority_live_sync_enabled(&root)
            .expect("sqlite default backend should resolve live-sync support"));
    }
}
