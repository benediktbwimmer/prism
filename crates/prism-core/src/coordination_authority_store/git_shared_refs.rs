use std::path::{Path, PathBuf};

use anyhow::Result;
use prism_coordination::RuntimeDescriptor;

use super::traits::CoordinationAuthorityStore;
use super::types::{
    CoordinationAuthorityBackendDetails, CoordinationAuthorityBackendKind,
    CoordinationAuthorityCapabilities, CoordinationAuthorityDiagnostics,
    CoordinationAuthorityProvenance, CoordinationAuthorityStamp, CoordinationConflictInfo,
    CoordinationCurrentState, CoordinationDiagnosticsRequest, CoordinationHistoryEnvelope,
    CoordinationHistoryRequest, CoordinationReadEnvelope, CoordinationReadRequest,
    CoordinationTransactionBase, CoordinationTransactionRequest, CoordinationTransactionResult,
    CoordinationTransactionStatus, RuntimeDescriptorClearRequest, RuntimeDescriptorPublishRequest,
    RuntimeDescriptorQuery,
};
use crate::coordination_reads::CoordinationReadConsistency;
use crate::shared_coordination_ref::{
    clear_runtime_descriptor_record, load_shared_coordination_ref_state_authoritative,
    load_shared_coordination_retained_history, load_shared_coordination_runtime_refs,
    publish_runtime_descriptor_record, shared_coordination_ref_diagnostics,
    shared_coordination_startup_authority, sync_shared_coordination_ref_state,
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
        let authority = shared_coordination_startup_authority(&self.root)?.unwrap_or_else(|| {
            prism_store::CoordinationStartupCheckpointAuthority {
                ref_name: "shared-coordination".to_string(),
                head_commit: None,
                manifest_digest: None,
            }
        });
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
        Ok(
            load_shared_coordination_ref_state_authoritative(&self.root)?.map(|shared| {
                CoordinationCurrentState {
                    snapshot: shared.snapshot,
                    canonical_snapshot_v2: shared.canonical_snapshot_v2,
                    runtime_descriptors: shared.runtime_descriptors,
                }
            }),
        )
    }

    fn indeterminate_transaction_result(
        &self,
        error: &anyhow::Error,
    ) -> Result<CoordinationTransactionResult> {
        Ok(CoordinationTransactionResult {
            status: CoordinationTransactionStatus::Indeterminate,
            committed: false,
            authority: self.authority_stamp()?,
            snapshot: self.load_current_state()?,
            persisted: None,
            conflict: None,
            diagnostics: vec![super::types::CoordinationTransactionDiagnostic {
                code: "transport_uncertain".to_string(),
                message: error.to_string(),
            }],
        })
    }
}

fn transport_outcome_uncertain(error: &anyhow::Error) -> bool {
    let message = error.to_string().to_ascii_lowercase();
    [
        "broken pipe",
        "connection reset",
        "connection aborted",
        "unexpected disconnect",
        "remote end hung up",
        "timed out",
        "timeout",
        "eof",
        "early eof",
        "failed to send request",
    ]
    .iter()
    .any(|needle| message.contains(needle))
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
        if let Err(error) = sync_shared_coordination_ref_state(
            &self.root,
            &request.snapshot,
            &request.canonical_snapshot_v2,
            publish_context.as_ref(),
        ) {
            if transport_outcome_uncertain(&error) {
                return self.indeterminate_transaction_result(&error);
            }
            return Err(error);
        }
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
        if let Err(error) = publish_runtime_descriptor_record(&self.root, &request.descriptor) {
            if transport_outcome_uncertain(&error) {
                return self.indeterminate_transaction_result(&error);
            }
            return Err(error);
        }
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
        if let Err(error) = clear_runtime_descriptor_record(&self.root, &request.runtime_id) {
            if transport_outcome_uncertain(&error) {
                return self.indeterminate_transaction_result(&error);
            }
            return Err(error);
        }
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
        let mut value = self
            .load_current_state()?
            .map(|state| state.runtime_descriptors)
            .unwrap_or_default();
        if value.is_empty() {
            value = load_shared_coordination_runtime_refs(&self.root)?;
        }
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
        request: CoordinationHistoryRequest,
    ) -> Result<CoordinationHistoryEnvelope> {
        let entries = load_shared_coordination_retained_history(&self.root, request.limit)?
            .into_iter()
            .map(|entry| super::types::CoordinationHistoryEntry {
                transaction_id: Some(entry.head_commit.clone()),
                snapshot_id: entry.manifest_digest.or(Some(entry.head_commit)),
                committed_at: entry.published_at,
                summary: entry.summary,
            })
            .collect::<Vec<_>>();
        let truncated = request
            .limit
            .map(|limit| entries.len() as u64 >= limit)
            .unwrap_or(false);
        Ok(CoordinationHistoryEnvelope {
            backend_kind: CoordinationAuthorityBackendKind::GitSharedRefs,
            entries,
            truncated,
        })
    }

    fn diagnostics(
        &self,
        _request: CoordinationDiagnosticsRequest,
    ) -> Result<CoordinationAuthorityDiagnostics> {
        let details = shared_coordination_ref_diagnostics(&self.root)?
            .map(CoordinationAuthorityBackendDetails::GitSharedRefs)
            .unwrap_or(CoordinationAuthorityBackendDetails::Unavailable);
        let runtime_descriptor_count = match &details {
            CoordinationAuthorityBackendDetails::GitSharedRefs(value) => {
                value.runtime_descriptors.len()
            }
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
