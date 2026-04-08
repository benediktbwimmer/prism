use std::path::{Path, PathBuf};

use anyhow::{anyhow, Result};
use prism_coordination::RuntimeDescriptor;

use super::traits::CoordinationAuthorityStore;
use super::types::{
    CoordinationAuthorityBackendDetails, CoordinationAuthorityBackendKind,
    CoordinationAuthorityCapabilities, CoordinationAuthorityDiagnostics,
    CoordinationAuthorityProvenance, CoordinationAuthorityStamp, CoordinationCurrentState,
    CoordinationConflictInfo, CoordinationDiagnosticsRequest, CoordinationHistoryEnvelope,
    CoordinationHistoryRequest, CoordinationReadEnvelope, CoordinationReadRequest,
    CoordinationTransactionBase, CoordinationTransactionRequest, CoordinationTransactionResult,
    CoordinationTransactionStatus, RuntimeDescriptorClearRequest,
    RuntimeDescriptorPublishRequest, RuntimeDescriptorQuery,
};
use crate::coordination_reads::CoordinationReadConsistency;
use crate::coordination_startup_checkpoint::coordination_startup_authority;
use crate::shared_coordination_ref::{
    clear_runtime_descriptor_record, load_shared_coordination_ref_state_authoritative,
    publish_runtime_descriptor_record, shared_coordination_ref_diagnostics,
    sync_shared_coordination_ref_state,
};
use crate::tracked_snapshot::publish_context_from_coordination_events;
use crate::workspace_identity::workspace_identity_for_root;

#[derive(Debug, Clone)]
pub struct GitSharedRefsCoordinationAuthorityStore {
    root: PathBuf,
}

impl GitSharedRefsCoordinationAuthorityStore {
    pub fn new(root: impl AsRef<Path>) -> Self {
        Self {
            root: root.as_ref().to_path_buf(),
        }
    }

    pub fn root(&self) -> &Path {
        &self.root
    }

    fn authority_stamp(&self) -> Result<Option<CoordinationAuthorityStamp>> {
        let logical_repo_id = workspace_identity_for_root(&self.root).repo_id;
        let diagnostics = shared_coordination_ref_diagnostics(&self.root)?;
        let Some(diagnostics) = diagnostics else {
            return Ok(None);
        };
        let authority = coordination_startup_authority(&self.root)?;
        let snapshot_id = authority
            .manifest_digest
            .clone()
            .or_else(|| authority.head_commit.clone())
            .unwrap_or_else(|| authority.ref_name.clone());
        Ok(Some(CoordinationAuthorityStamp {
            backend_kind: CoordinationAuthorityBackendKind::GitSharedRefs,
            logical_repo_id,
            snapshot_id,
            transaction_id: authority.head_commit.clone(),
            committed_at: diagnostics.last_successful_publish_at,
            provenance: CoordinationAuthorityProvenance {
                ref_name: Some(authority.ref_name),
                head_commit: authority.head_commit,
                manifest_digest: authority.manifest_digest,
            },
        }))
    }

    fn load_current_state(&self) -> Result<Option<CoordinationCurrentState>> {
        Ok(load_shared_coordination_ref_state_authoritative(&self.root)?.map(|shared| {
            CoordinationCurrentState {
                snapshot: shared.snapshot,
                canonical_snapshot_v2: shared.canonical_snapshot_v2,
                runtime_descriptors: shared.runtime_descriptors,
            }
        }))
    }
}

impl CoordinationAuthorityStore for GitSharedRefsCoordinationAuthorityStore {
    fn capabilities(&self) -> CoordinationAuthorityCapabilities {
        CoordinationAuthorityCapabilities {
            supports_eventual_reads: true,
            supports_transactions: true,
            supports_runtime_descriptors: true,
            supports_retained_history: true,
            supports_diagnostics: true,
        }
    }

    fn read_current(
        &self,
        request: CoordinationReadRequest,
    ) -> Result<CoordinationReadEnvelope<CoordinationCurrentState>> {
        let authority = self.authority_stamp()?;
        match self.load_current_state()? {
            Some(state) => Ok(CoordinationReadEnvelope::verified_current(
                request.consistency,
                authority,
                state,
            )),
            None => Ok(CoordinationReadEnvelope::unavailable(
                request.consistency,
                authority,
                None,
            )),
        }
    }

    fn apply_transaction(
        &self,
        request: CoordinationTransactionRequest,
    ) -> Result<CoordinationTransactionResult> {
        let current_authority = self.authority_stamp()?;
        if let CoordinationTransactionBase::ExpectedAuthorityStamp(expected) = &request.base {
            if current_authority.as_ref() != Some(expected) {
                return Ok(CoordinationTransactionResult {
                    status: CoordinationTransactionStatus::Conflict,
                    committed: false,
                    authority: current_authority,
                    snapshot: self.load_current_state()?,
                    persisted: None,
                    conflict: Some(CoordinationConflictInfo {
                        reason: "authority stamp no longer matches the current shared-ref head"
                            .to_string(),
                    }),
                    diagnostics: Vec::new(),
                });
            }
        }
        let publish_context = publish_context_from_coordination_events(&request.appended_events);
        sync_shared_coordination_ref_state(
            &self.root,
            &request.snapshot,
            &request.canonical_snapshot_v2,
            publish_context.as_ref(),
        )?;
        Ok(CoordinationTransactionResult {
            status: CoordinationTransactionStatus::Committed,
            committed: true,
            authority: self.authority_stamp()?,
            snapshot: self.load_current_state()?,
            persisted: None,
            conflict: None,
            diagnostics: Vec::new(),
        })
    }

    fn publish_runtime_descriptor(
        &self,
        request: RuntimeDescriptorPublishRequest,
    ) -> Result<CoordinationTransactionResult> {
        let current_authority = self.authority_stamp()?;
        if let CoordinationTransactionBase::ExpectedAuthorityStamp(expected) = &request.base {
            if current_authority.as_ref() != Some(expected) {
                return Ok(CoordinationTransactionResult {
                    status: CoordinationTransactionStatus::Conflict,
                    committed: false,
                    authority: current_authority,
                    snapshot: self.load_current_state()?,
                    persisted: None,
                    conflict: Some(CoordinationConflictInfo {
                        reason: "authority stamp no longer matches the current shared-ref head"
                            .to_string(),
                    }),
                    diagnostics: Vec::new(),
                });
            }
        }
        publish_runtime_descriptor_record(&self.root, &request.descriptor)?;
        Ok(CoordinationTransactionResult {
            status: CoordinationTransactionStatus::Committed,
            committed: true,
            authority: self.authority_stamp()?,
            snapshot: self.load_current_state()?,
            persisted: None,
            conflict: None,
            diagnostics: Vec::new(),
        })
    }

    fn clear_runtime_descriptor(
        &self,
        request: RuntimeDescriptorClearRequest,
    ) -> Result<CoordinationTransactionResult> {
        let current_authority = self.authority_stamp()?;
        if let CoordinationTransactionBase::ExpectedAuthorityStamp(expected) = &request.base {
            if current_authority.as_ref() != Some(expected) {
                return Ok(CoordinationTransactionResult {
                    status: CoordinationTransactionStatus::Conflict,
                    committed: false,
                    authority: current_authority,
                    snapshot: self.load_current_state()?,
                    persisted: None,
                    conflict: Some(CoordinationConflictInfo {
                        reason: "authority stamp no longer matches the current shared-ref head"
                            .to_string(),
                    }),
                    diagnostics: Vec::new(),
                });
            }
        }
        clear_runtime_descriptor_record(&self.root, &request.runtime_id)?;
        Ok(CoordinationTransactionResult {
            status: CoordinationTransactionStatus::Committed,
            committed: true,
            authority: self.authority_stamp()?,
            snapshot: self.load_current_state()?,
            persisted: None,
            conflict: None,
            diagnostics: Vec::new(),
        })
    }

    fn list_runtime_descriptors(
        &self,
        request: RuntimeDescriptorQuery,
    ) -> Result<CoordinationReadEnvelope<Vec<RuntimeDescriptor>>> {
        let authority = self.authority_stamp()?;
        let value = self
            .load_current_state()?
            .map(|state| state.runtime_descriptors)
            .unwrap_or_default();
        if value.is_empty() && matches!(request.consistency, CoordinationReadConsistency::Strong) {
            return Ok(CoordinationReadEnvelope::unavailable(
                request.consistency,
                authority,
                None,
            ));
        }
        Ok(CoordinationReadEnvelope::verified_current(
            request.consistency,
            authority,
            value,
        ))
    }

    fn read_history(
        &self,
        _request: CoordinationHistoryRequest,
    ) -> Result<CoordinationHistoryEnvelope> {
        Err(anyhow!(
            "retained history is not wired through CoordinationAuthorityStore yet"
        ))
    }

    fn diagnostics(
        &self,
        _request: CoordinationDiagnosticsRequest,
    ) -> Result<CoordinationAuthorityDiagnostics> {
        let details = shared_coordination_ref_diagnostics(&self.root)?
            .map(CoordinationAuthorityBackendDetails::GitSharedRefs)
            .unwrap_or(CoordinationAuthorityBackendDetails::Unavailable);
        let runtime_descriptor_count = match &details {
            CoordinationAuthorityBackendDetails::GitSharedRefs(value) => value.runtime_descriptors.len(),
            CoordinationAuthorityBackendDetails::Unavailable => 0,
        };
        Ok(CoordinationAuthorityDiagnostics {
            backend_kind: CoordinationAuthorityBackendKind::GitSharedRefs,
            latest_authority: self.authority_stamp()?,
            runtime_descriptor_count,
            backend_details: details,
        })
    }
}
