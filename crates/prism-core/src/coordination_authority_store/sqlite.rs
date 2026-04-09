use std::path::{Path, PathBuf};

use anyhow::Result;
use prism_coordination::{
    coordination_snapshot_from_events, CoordinationSnapshot, CoordinationSnapshotV2,
    EventExecutionRecord, RuntimeDescriptor,
};
use prism_ir::EventExecutionStatus;
use prism_store::{
    CoordinationCheckpointStore, CoordinationEventExecutionStore, CoordinationJournal,
    CoordinationMutationLogEntry, CoordinationPersistBatch, CoordinationStartupCheckpoint,
    CoordinationStartupCheckpointAuthority, SqliteStore,
};

use super::traits::{
    CoordinationAuthorityCurrentStateStore, CoordinationAuthorityDiagnosticsStore,
    CoordinationAuthorityEventExecutionStore, CoordinationAuthorityHistoryStore,
    CoordinationAuthorityMutationStore, CoordinationAuthorityRuntimeStore,
    CoordinationAuthoritySnapshotStore,
};
use super::types::{
    CoordinationAppendRequest, CoordinationAuthorityBackendDetails,
    CoordinationAuthorityBackendKind, CoordinationAuthorityCapabilities,
    CoordinationAuthorityDiagnostics, CoordinationAuthorityProvenance, CoordinationAuthorityStamp,
    CoordinationAuthoritySummary, CoordinationCommitReceipt, CoordinationConflictInfo,
    CoordinationCurrentState, CoordinationDiagnosticsRequest, CoordinationHistoryEnvelope,
    CoordinationHistoryRequest, CoordinationReadEnvelope, CoordinationReplaceCurrentStateRequest,
    CoordinationTransactionBase, CoordinationTransactionResult, CoordinationTransactionStatus,
    EventExecutionOwnerExpectation, EventExecutionRecordAuthorityQuery,
    EventExecutionRecordWriteResult, EventExecutionTransitionKind,
    EventExecutionTransitionPreconditions, EventExecutionTransitionRequest,
    EventExecutionTransitionResult, EventExecutionTransitionStatus, RuntimeDescriptorClearRequest,
    RuntimeDescriptorPublishRequest, RuntimeDescriptorQuery,
    SqliteCoordinationAuthorityBackendDetails,
};
use crate::coordination_persistence::repo_semantic_coordination_snapshot;
use crate::coordination_reads::CoordinationReadConsistency;
use crate::util::current_timestamp;
use crate::workspace_identity::{
    coordination_persist_context_for_root, workspace_identity_for_root,
};

#[derive(Debug, Clone)]
pub struct SqliteCoordinationAuthorityStore {
    root: PathBuf,
    db_path: PathBuf,
}

impl SqliteCoordinationAuthorityStore {
    pub fn new(root: impl AsRef<Path>, db_path: impl AsRef<Path>) -> Self {
        Self {
            root: root.as_ref().to_path_buf(),
            db_path: db_path.as_ref().to_path_buf(),
        }
    }

    fn open_store(&self) -> Result<SqliteStore> {
        SqliteStore::open(&self.db_path)
    }

    fn load_checkpoint(
        &self,
        store: &mut SqliteStore,
    ) -> Result<Option<CoordinationStartupCheckpoint>> {
        store.load_coordination_startup_checkpoint()
    }

    fn checkpoint_state(
        &self,
        checkpoint: CoordinationStartupCheckpoint,
    ) -> CoordinationCurrentState {
        let snapshot = checkpoint.snapshot;
        let canonical_snapshot_v2 = checkpoint.canonical_snapshot_v2;
        CoordinationCurrentState {
            snapshot,
            canonical_snapshot_v2,
            runtime_descriptors: checkpoint.runtime_descriptors,
        }
    }

    fn effective_revision(
        revision: u64,
        checkpoint: Option<&CoordinationStartupCheckpoint>,
    ) -> u64 {
        checkpoint
            .map(|value| value.coordination_revision.max(revision))
            .unwrap_or(revision)
    }

    fn load_current_state_from_store(
        &self,
        store: &mut SqliteStore,
    ) -> Result<Option<CoordinationCurrentState>> {
        let revision = store.coordination_revision()?;
        let checkpoint = self.load_checkpoint(store)?;
        let effective_revision = Self::effective_revision(revision, checkpoint.as_ref());
        let stream = store.load_coordination_event_stream()?;
        if let Some(snapshot) =
            coordination_snapshot_from_events(&stream.suffix_events, stream.fallback_snapshot)
        {
            let snapshot = if let Some(checkpoint) = checkpoint
                .as_ref()
                .filter(|value| value.coordination_revision == effective_revision)
            {
                merge_checkpoint_counters(snapshot, &checkpoint.snapshot)
            } else {
                snapshot
            };
            let runtime_descriptors = checkpoint
                .as_ref()
                .map(|value| value.runtime_descriptors.clone())
                .unwrap_or_default();
            let canonical_snapshot_v2 = checkpoint
                .as_ref()
                .filter(|value| value.coordination_revision == effective_revision)
                .map(|value| value.canonical_snapshot_v2.clone())
                .unwrap_or_else(|| snapshot.to_canonical_snapshot_v2());
            return Ok(Some(CoordinationCurrentState {
                canonical_snapshot_v2,
                snapshot,
                runtime_descriptors,
            }));
        }

        match checkpoint {
            Some(checkpoint) => Ok(Some(self.checkpoint_state(checkpoint))),
            None => Ok(None),
        }
    }

    fn authority_stamp_from_store(
        &self,
        store: &mut SqliteStore,
    ) -> Result<Option<CoordinationAuthorityStamp>> {
        let revision = store.coordination_revision()?;
        let checkpoint = self.load_checkpoint(store)?;
        let effective_revision = Self::effective_revision(revision, checkpoint.as_ref());
        if effective_revision == 0 && checkpoint.is_none() {
            return Ok(None);
        }
        let logical_repo_id = workspace_identity_for_root(&self.root).repo_id;
        let snapshot_id = format!("sqlite-revision:{effective_revision}");
        let committed_at = checkpoint.as_ref().map(|value| value.materialized_at);
        Ok(Some(CoordinationAuthorityStamp {
            backend_kind: CoordinationAuthorityBackendKind::Sqlite,
            logical_repo_id,
            snapshot_id: snapshot_id.clone(),
            transaction_id: Some(snapshot_id),
            committed_at,
            provenance: CoordinationAuthorityProvenance {
                ref_name: Some("sqlite-authority".to_string()),
                head_commit: None,
                manifest_digest: None,
            },
        }))
    }

    fn conflict_transaction_result(
        &self,
        authority: Option<CoordinationAuthorityStamp>,
        reason: impl Into<String>,
    ) -> CoordinationTransactionResult {
        CoordinationTransactionResult {
            status: CoordinationTransactionStatus::Conflict,
            committed: false,
            authority,
            commit: None,
            conflict: Some(CoordinationConflictInfo {
                reason: reason.into(),
            }),
            diagnostics: Vec::new(),
        }
    }

    fn validate_transaction_base(
        &self,
        base: &CoordinationTransactionBase,
        current_revision: u64,
        current_authority: &Option<CoordinationAuthorityStamp>,
        _current_state: &Option<CoordinationCurrentState>,
    ) -> Option<CoordinationTransactionResult> {
        match base {
            CoordinationTransactionBase::LatestStrong => None,
            CoordinationTransactionBase::ExpectedRevision(expected_revision) => {
                if current_revision == *expected_revision {
                    return None;
                }
                Some(self.conflict_transaction_result(
                    current_authority.clone(),
                    format!(
                        "authority revision no longer matches the current sqlite state: expected `{expected_revision}`, found `{current_revision}`"
                    ),
                ))
            }
            CoordinationTransactionBase::ExpectedAuthorityStamp(expected) => {
                if current_authority.as_ref() == Some(expected) {
                    return None;
                }
                Some(self.conflict_transaction_result(
                    current_authority.clone(),
                    "authority stamp no longer matches the current sqlite authority state",
                ))
            }
        }
    }

    fn commit_expected_revision(
        &self,
        base: &CoordinationTransactionBase,
        current_revision: u64,
    ) -> Option<u64> {
        match base {
            CoordinationTransactionBase::LatestStrong => None,
            CoordinationTransactionBase::ExpectedRevision(expected_revision) => {
                Some(*expected_revision)
            }
            CoordinationTransactionBase::ExpectedAuthorityStamp(_) => Some(current_revision),
        }
    }

    fn persist_current_state(
        &self,
        store: &mut SqliteStore,
        revision: u64,
        snapshot: &CoordinationSnapshot,
        canonical_snapshot_v2: &CoordinationSnapshotV2,
        runtime_descriptors: &[RuntimeDescriptor],
    ) -> Result<()> {
        let mut sanitized_snapshot = repo_semantic_coordination_snapshot(snapshot.clone());
        sanitized_snapshot.events.clear();
        store.save_coordination_startup_checkpoint(&CoordinationStartupCheckpoint {
            version: CoordinationStartupCheckpoint::VERSION,
            materialized_at: current_timestamp(),
            coordination_revision: revision,
            authority: CoordinationStartupCheckpointAuthority {
                ref_name: "sqlite-authority".to_string(),
                head_commit: None,
                manifest_digest: None,
            },
            snapshot: sanitized_snapshot,
            canonical_snapshot_v2: canonical_snapshot_v2.clone(),
            runtime_descriptors: runtime_descriptors.to_vec(),
        })
    }

    fn transaction_result_from_store(
        &self,
        store: &mut SqliteStore,
        persisted: Option<prism_store::CoordinationPersistResult>,
    ) -> Result<CoordinationTransactionResult> {
        Ok(CoordinationTransactionResult {
            status: CoordinationTransactionStatus::Committed,
            committed: true,
            authority: self.authority_stamp_from_store(store)?,
            commit: persisted.map(|persisted| CoordinationCommitReceipt {
                revision: persisted.revision,
                inserted_events: persisted.inserted_events,
                applied: persisted.applied,
            }),
            conflict: None,
            diagnostics: Vec::new(),
        })
    }

    fn conflict_from_revision_mismatch(
        &self,
        error: &anyhow::Error,
        authority: Option<CoordinationAuthorityStamp>,
    ) -> Option<CoordinationTransactionResult> {
        let message = error.to_string();
        if message.contains("coordination revision mismatch") {
            return Some(self.conflict_transaction_result(authority, message));
        }
        None
    }

    fn history_entry_summary(entry: &CoordinationMutationLogEntry) -> String {
        let scope = entry
            .context
            .branch_ref
            .clone()
            .unwrap_or_else(|| entry.context.worktree_id.clone());
        if entry.applied {
            format!(
                "applied {} coordination event(s) for {scope}",
                entry.inserted_events
            )
        } else {
            format!("replayed coordination mutation for {scope}")
        }
    }

    fn conflict_event_transition_result(
        &self,
        authority: Option<CoordinationAuthorityStamp>,
        record: Option<EventExecutionRecord>,
        reason: impl Into<String>,
    ) -> EventExecutionTransitionResult {
        EventExecutionTransitionResult {
            status: EventExecutionTransitionStatus::Conflict,
            authority,
            record,
            conflict: Some(CoordinationConflictInfo {
                reason: reason.into(),
            }),
            diagnostics: Vec::new(),
        }
    }

    fn validate_event_transition_preconditions(
        &self,
        preconditions: &EventExecutionTransitionPreconditions,
        current: Option<&EventExecutionRecord>,
        authority: &Option<CoordinationAuthorityStamp>,
    ) -> Option<EventExecutionTransitionResult> {
        if preconditions.require_missing {
            if let Some(record) = current.cloned() {
                return Some(self.conflict_event_transition_result(
                    authority.clone(),
                    Some(record),
                    "event execution record already exists",
                ));
            }
            return None;
        }

        let current = match current {
            Some(current) => current,
            None => {
                return Some(self.conflict_event_transition_result(
                    authority.clone(),
                    None,
                    "event execution record does not exist",
                ))
            }
        };

        if let Some(expected_status) = preconditions.expected_status {
            if current.status != expected_status {
                return Some(self.conflict_event_transition_result(
                    authority.clone(),
                    Some(current.clone()),
                    format!(
                        "event execution status no longer matches: expected `{:?}`, found `{:?}`",
                        expected_status, current.status
                    ),
                ));
            }
        }

        match &preconditions.expected_owner {
            EventExecutionOwnerExpectation::Any => None,
            EventExecutionOwnerExpectation::Missing => current.owner.as_ref().map(|_| {
                self.conflict_event_transition_result(
                    authority.clone(),
                    Some(current.clone()),
                    "event execution owner is already set",
                )
            }),
            EventExecutionOwnerExpectation::Exact(expected_owner) => {
                if current.owner.as_ref() == Some(expected_owner) {
                    None
                } else {
                    Some(self.conflict_event_transition_result(
                        authority.clone(),
                        Some(current.clone()),
                        "event execution owner no longer matches the expected owner",
                    ))
                }
            }
        }
    }

    fn apply_event_execution_transition_to_record(
        &self,
        current: Option<&EventExecutionRecord>,
        transition: EventExecutionTransitionKind,
    ) -> EventExecutionRecord {
        match transition {
            EventExecutionTransitionKind::Claim { record } => record,
            EventExecutionTransitionKind::Start {
                started_at,
                summary,
            } => {
                let mut record = current.expect("validated current event record").clone();
                record.status = EventExecutionStatus::Running;
                record.started_at = Some(started_at);
                if let Some(summary) = summary {
                    record.summary = Some(summary);
                }
                record
            }
            EventExecutionTransitionKind::Succeed {
                finished_at,
                summary,
            } => {
                let mut record = current.expect("validated current event record").clone();
                record.status = EventExecutionStatus::Succeeded;
                record.finished_at = Some(finished_at);
                if let Some(summary) = summary {
                    record.summary = Some(summary);
                }
                record
            }
            EventExecutionTransitionKind::Fail {
                finished_at,
                summary,
            } => {
                let mut record = current.expect("validated current event record").clone();
                record.status = EventExecutionStatus::Failed;
                record.finished_at = Some(finished_at);
                if let Some(summary) = summary {
                    record.summary = Some(summary);
                }
                record
            }
            EventExecutionTransitionKind::Expire {
                finished_at,
                summary,
            } => {
                let mut record = current.expect("validated current event record").clone();
                record.status = EventExecutionStatus::Expired;
                record.finished_at = Some(finished_at);
                if let Some(summary) = summary {
                    record.summary = Some(summary);
                }
                record
            }
            EventExecutionTransitionKind::Abandon {
                finished_at,
                summary,
            } => {
                let mut record = current.expect("validated current event record").clone();
                record.status = EventExecutionStatus::Abandoned;
                record.finished_at = Some(finished_at);
                if let Some(summary) = summary {
                    record.summary = Some(summary);
                }
                record
            }
        }
    }
}

fn merge_checkpoint_counters(
    mut snapshot: CoordinationSnapshot,
    checkpoint_snapshot: &CoordinationSnapshot,
) -> CoordinationSnapshot {
    snapshot.next_plan = snapshot.next_plan.max(checkpoint_snapshot.next_plan);
    snapshot.next_task = snapshot.next_task.max(checkpoint_snapshot.next_task);
    snapshot.next_claim = snapshot.next_claim.max(checkpoint_snapshot.next_claim);
    snapshot.next_artifact = snapshot
        .next_artifact
        .max(checkpoint_snapshot.next_artifact);
    snapshot.next_review = snapshot.next_review.max(checkpoint_snapshot.next_review);
    snapshot
}

impl CoordinationAuthorityCurrentStateStore for SqliteCoordinationAuthorityStore {
    fn capabilities(&self) -> CoordinationAuthorityCapabilities {
        CoordinationAuthorityCapabilities {
            supports_eventual_reads: false,
            supports_transactions: true,
            supports_runtime_descriptors: true,
            supports_event_execution_records: true,
            supports_retained_history: true,
            supports_diagnostics: true,
        }
    }

    fn read_current_state(
        &self,
        consistency: CoordinationReadConsistency,
    ) -> Result<CoordinationReadEnvelope<CoordinationCurrentState>> {
        let mut store = self.open_store()?;
        let authority = self.authority_stamp_from_store(&mut store)?;
        match self.load_current_state_from_store(&mut store)? {
            Some(state) => Ok(CoordinationReadEnvelope::verified_current(
                consistency,
                authority,
                state,
            )),
            None => Ok(CoordinationReadEnvelope::unavailable(
                consistency,
                authority,
                None,
            )),
        }
    }

    fn read_summary(
        &self,
        consistency: CoordinationReadConsistency,
    ) -> Result<CoordinationReadEnvelope<CoordinationAuthoritySummary>> {
        let mut store = self.open_store()?;
        let authority = self.authority_stamp_from_store(&mut store)?;
        let current_state = self.load_current_state_from_store(&mut store)?;
        Ok(CoordinationReadEnvelope::verified_current(
            consistency,
            authority,
            CoordinationAuthoritySummary {
                has_current_state: current_state.is_some(),
                runtime_descriptor_count: current_state
                    .as_ref()
                    .map(|state| state.runtime_descriptors.len())
                    .unwrap_or(0),
            },
        ))
    }
}

impl CoordinationAuthorityMutationStore for SqliteCoordinationAuthorityStore {
    fn append_events(
        &self,
        request: CoordinationAppendRequest,
    ) -> Result<CoordinationTransactionResult> {
        let mut store = self.open_store()?;
        let current_revision = store.coordination_revision()?;
        let current_authority = self.authority_stamp_from_store(&mut store)?;
        let current_state = self.load_current_state_from_store(&mut store)?;
        if let Some(result) = self.validate_transaction_base(
            &request.base,
            current_revision,
            &current_authority,
            &current_state,
        ) {
            return Ok(result);
        }
        let persisted = match store.commit_coordination_persist_batch(&CoordinationPersistBatch {
            context: coordination_persist_context_for_root(&self.root, request.session_id.as_ref()),
            expected_revision: self.commit_expected_revision(&request.base, current_revision),
            appended_events: request.appended_events.clone(),
        }) {
            Ok(persisted) => persisted,
            Err(error) => {
                if let Some(conflict) =
                    self.conflict_from_revision_mismatch(&error, current_authority)
                {
                    return Ok(conflict);
                }
                return Err(error);
            }
        };
        let next_snapshot = coordination_snapshot_from_events(
            &request.appended_events,
            current_state.as_ref().map(|state| state.snapshot.clone()),
        )
        .unwrap_or_default();
        let next_canonical_snapshot_v2 = next_snapshot.to_canonical_snapshot_v2();
        let runtime_descriptors = current_state
            .as_ref()
            .map(|state| state.runtime_descriptors.clone())
            .unwrap_or_default();
        self.persist_current_state(
            &mut store,
            persisted.revision,
            &next_snapshot,
            &next_canonical_snapshot_v2,
            &runtime_descriptors,
        )?;
        self.transaction_result_from_store(&mut store, Some(persisted))
    }
}

impl CoordinationAuthorityRuntimeStore for SqliteCoordinationAuthorityStore {
    fn publish_runtime_descriptor(
        &self,
        request: RuntimeDescriptorPublishRequest,
    ) -> Result<CoordinationTransactionResult> {
        let mut store = self.open_store()?;
        let current_revision = store.coordination_revision()?;
        let current_authority = self.authority_stamp_from_store(&mut store)?;
        let current_state = self.load_current_state_from_store(&mut store)?;
        if let Some(result) = self.validate_transaction_base(
            &request.base,
            current_revision,
            &current_authority,
            &current_state,
        ) {
            return Ok(result);
        }
        let mut runtime_descriptors = current_state
            .as_ref()
            .map(|state| state.runtime_descriptors.clone())
            .unwrap_or_default();
        runtime_descriptors.retain(|value| value.runtime_id != request.descriptor.runtime_id);
        runtime_descriptors.push(request.descriptor);
        runtime_descriptors.sort_by(|left, right| left.runtime_id.cmp(&right.runtime_id));
        let snapshot = current_state
            .as_ref()
            .map(|state| state.snapshot.clone())
            .unwrap_or_default();
        let canonical_snapshot_v2 = current_state
            .as_ref()
            .map(|state| state.canonical_snapshot_v2.clone())
            .unwrap_or_else(|| snapshot.to_canonical_snapshot_v2());
        self.persist_current_state(
            &mut store,
            current_revision,
            &snapshot,
            &canonical_snapshot_v2,
            &runtime_descriptors,
        )?;
        self.transaction_result_from_store(&mut store, None)
    }

    fn clear_runtime_descriptor(
        &self,
        request: RuntimeDescriptorClearRequest,
    ) -> Result<CoordinationTransactionResult> {
        let mut store = self.open_store()?;
        let current_revision = store.coordination_revision()?;
        let current_authority = self.authority_stamp_from_store(&mut store)?;
        let current_state = self.load_current_state_from_store(&mut store)?;
        if let Some(result) = self.validate_transaction_base(
            &request.base,
            current_revision,
            &current_authority,
            &current_state,
        ) {
            return Ok(result);
        }
        let mut runtime_descriptors = current_state
            .as_ref()
            .map(|state| state.runtime_descriptors.clone())
            .unwrap_or_default();
        runtime_descriptors.retain(|value| value.runtime_id != request.runtime_id);
        let snapshot = current_state
            .as_ref()
            .map(|state| state.snapshot.clone())
            .unwrap_or_default();
        let canonical_snapshot_v2 = current_state
            .as_ref()
            .map(|state| state.canonical_snapshot_v2.clone())
            .unwrap_or_else(|| snapshot.to_canonical_snapshot_v2());
        self.persist_current_state(
            &mut store,
            current_revision,
            &snapshot,
            &canonical_snapshot_v2,
            &runtime_descriptors,
        )?;
        self.transaction_result_from_store(&mut store, None)
    }

    fn list_runtime_descriptors(
        &self,
        request: RuntimeDescriptorQuery,
    ) -> Result<CoordinationReadEnvelope<Vec<RuntimeDescriptor>>> {
        let mut store = self.open_store()?;
        let authority = self.authority_stamp_from_store(&mut store)?;
        match self.load_current_state_from_store(&mut store)? {
            Some(state) => Ok(CoordinationReadEnvelope::verified_current(
                request.consistency,
                authority,
                state.runtime_descriptors,
            )),
            None if matches!(request.consistency, CoordinationReadConsistency::Strong) => Ok(
                CoordinationReadEnvelope::unavailable(request.consistency, authority, None),
            ),
            None => Ok(CoordinationReadEnvelope::verified_current(
                request.consistency,
                authority,
                Vec::new(),
            )),
        }
    }
}

impl CoordinationAuthorityEventExecutionStore for SqliteCoordinationAuthorityStore {
    fn read_event_execution_records(
        &self,
        request: EventExecutionRecordAuthorityQuery,
    ) -> Result<CoordinationReadEnvelope<Vec<EventExecutionRecord>>> {
        let mut store = self.open_store()?;
        let authority = self.authority_stamp_from_store(&mut store)?;
        let records = match request.event_execution_id.as_ref() {
            Some(event_execution_id) => store
                .load_event_execution_record(event_execution_id)?
                .into_iter()
                .collect(),
            None => {
                store.load_event_execution_records(&prism_store::EventExecutionRecordQuery {
                    limit: request.limit,
                })?
            }
        };
        Ok(CoordinationReadEnvelope::verified_current(
            request.consistency,
            authority,
            records,
        ))
    }

    fn upsert_event_execution_record(
        &self,
        record: EventExecutionRecord,
    ) -> Result<EventExecutionRecordWriteResult> {
        let mut store = self.open_store()?;
        store.save_event_execution_record(&record)?;
        Ok(EventExecutionRecordWriteResult {
            authority: self.authority_stamp_from_store(&mut store)?,
            record,
        })
    }

    fn apply_event_execution_transition(
        &self,
        request: EventExecutionTransitionRequest,
    ) -> Result<EventExecutionTransitionResult> {
        let mut store = self.open_store()?;
        let authority = self.authority_stamp_from_store(&mut store)?;
        let current = store.load_event_execution_record(&request.event_execution_id)?;
        if let Some(conflict) = self.validate_event_transition_preconditions(
            &request.preconditions,
            current.as_ref(),
            &authority,
        ) {
            return Ok(conflict);
        }
        let record =
            self.apply_event_execution_transition_to_record(current.as_ref(), request.transition);
        store.save_event_execution_record(&record)?;
        Ok(EventExecutionTransitionResult {
            status: EventExecutionTransitionStatus::Applied,
            authority: self.authority_stamp_from_store(&mut store)?,
            record: Some(record),
            conflict: None,
            diagnostics: Vec::new(),
        })
    }
}

impl CoordinationAuthorityHistoryStore for SqliteCoordinationAuthorityStore {
    fn read_history(
        &self,
        request: CoordinationHistoryRequest,
    ) -> Result<CoordinationHistoryEnvelope> {
        let store = self.open_store()?;
        let entries = store
            .load_coordination_mutation_log(request.limit.map(|value| value as usize))?
            .into_iter()
            .map(|entry| super::types::CoordinationHistoryEntry {
                transaction_id: Some(format!("sqlite-mutation:{}", entry.sequence)),
                snapshot_id: Some(format!("sqlite-revision:{}", entry.revision)),
                committed_at: None,
                summary: Self::history_entry_summary(&entry),
            })
            .collect::<Vec<_>>();
        let truncated = request
            .limit
            .map(|limit| entries.len() as u64 >= limit)
            .unwrap_or(false);
        Ok(CoordinationHistoryEnvelope {
            backend_kind: CoordinationAuthorityBackendKind::Sqlite,
            entries,
            truncated,
        })
    }
}

impl CoordinationAuthorityDiagnosticsStore for SqliteCoordinationAuthorityStore {
    fn diagnostics(
        &self,
        _request: CoordinationDiagnosticsRequest,
    ) -> Result<CoordinationAuthorityDiagnostics> {
        let mut store = self.open_store()?;
        let current_state = self.load_current_state_from_store(&mut store)?;
        let coordination_revision = Some(store.coordination_revision()?);
        Ok(CoordinationAuthorityDiagnostics {
            backend_kind: CoordinationAuthorityBackendKind::Sqlite,
            latest_authority: self.authority_stamp_from_store(&mut store)?,
            runtime_descriptor_count: current_state
                .as_ref()
                .map(|state| state.runtime_descriptors.len())
                .unwrap_or_default(),
            backend_details: CoordinationAuthorityBackendDetails::Sqlite(
                SqliteCoordinationAuthorityBackendDetails {
                    db_path: self.db_path.clone(),
                    coordination_revision,
                },
            ),
        })
    }
}

impl CoordinationAuthoritySnapshotStore for SqliteCoordinationAuthorityStore {
    fn read_snapshot(
        &self,
        consistency: CoordinationReadConsistency,
    ) -> Result<CoordinationReadEnvelope<CoordinationSnapshot>> {
        let mut store = self.open_store()?;
        let authority = self.authority_stamp_from_store(&mut store)?;
        match self.load_current_state_from_store(&mut store)? {
            Some(state) => Ok(CoordinationReadEnvelope::verified_current(
                consistency,
                authority,
                state.snapshot,
            )),
            None => Ok(CoordinationReadEnvelope::unavailable(
                consistency,
                authority,
                None,
            )),
        }
    }

    fn read_snapshot_v2(
        &self,
        consistency: CoordinationReadConsistency,
    ) -> Result<CoordinationReadEnvelope<CoordinationSnapshotV2>> {
        let mut store = self.open_store()?;
        let authority = self.authority_stamp_from_store(&mut store)?;
        match self.load_current_state_from_store(&mut store)? {
            Some(state) => Ok(CoordinationReadEnvelope::verified_current(
                consistency,
                authority,
                state.canonical_snapshot_v2,
            )),
            None => Ok(CoordinationReadEnvelope::unavailable(
                consistency,
                authority,
                None,
            )),
        }
    }

    fn replace_current_state(
        &self,
        request: CoordinationReplaceCurrentStateRequest,
    ) -> Result<CoordinationTransactionResult> {
        let mut store = self.open_store()?;
        let current_revision = store.coordination_revision()?;
        let current_authority = self.authority_stamp_from_store(&mut store)?;
        let current_state = self.load_current_state_from_store(&mut store)?;
        if let Some(result) = self.validate_transaction_base(
            &request.base,
            current_revision,
            &current_authority,
            &current_state,
        ) {
            return Ok(result);
        }
        self.persist_current_state(
            &mut store,
            current_revision,
            &request.state.snapshot,
            &request.state.canonical_snapshot_v2,
            &request.state.runtime_descriptors,
        )?;
        self.transaction_result_from_store(&mut store, None)
    }
}

#[cfg(test)]
mod tests {
    use std::fs;
    use std::path::PathBuf;
    use std::sync::atomic::{AtomicU64, Ordering};
    use std::time::{SystemTime, UNIX_EPOCH};

    use prism_coordination::{
        CoordinationEvent, EventExecutionOwner, EventExecutionRecord, RuntimeDescriptor,
        RuntimeDiscoveryMode,
    };
    use prism_ir::{
        CoordinationEventKind, EventActor, EventExecutionId, EventExecutionStatus, EventId,
        EventMeta, EventTriggerKind, PlanId, PrincipalActor, PrincipalAuthorityId, PrincipalId,
        PrincipalKind, SessionId,
    };

    use super::*;
    use crate::coordination_authority_store::CoordinationAppendRequest;
    use crate::CoordinationReadFreshness;

    static NEXT_TEMP_ROOT: AtomicU64 = AtomicU64::new(0);

    fn temp_root() -> PathBuf {
        let nonce = NEXT_TEMP_ROOT.fetch_add(1, Ordering::Relaxed);
        let unique = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        let root = std::env::temp_dir().join(format!("prism-sqlite-authority-{unique}-{nonce}"));
        let _ = fs::remove_dir_all(&root);
        fs::create_dir_all(&root).unwrap();
        root
    }

    fn temp_db_path(root: &Path) -> PathBuf {
        root.join("coordination-authority.db")
    }

    fn default_transaction_request(base: CoordinationTransactionBase) -> CoordinationAppendRequest {
        CoordinationAppendRequest {
            base,
            session_id: None,
            appended_events: Vec::new(),
        }
    }

    fn runtime_descriptor(runtime_id: &str) -> RuntimeDescriptor {
        RuntimeDescriptor {
            runtime_id: runtime_id.to_string(),
            repo_id: "repo:test".to_string(),
            worktree_id: "worktree:test".to_string(),
            principal_id: "principal:test".to_string(),
            instance_started_at: 1,
            last_seen_at: 2,
            branch_ref: Some("refs/heads/main".to_string()),
            checked_out_commit: None,
            capabilities: Vec::new(),
            discovery_mode: RuntimeDiscoveryMode::None,
            peer_endpoint: None,
            public_endpoint: None,
            peer_transport_identity: None,
            blob_snapshot_head: None,
            export_policy: None,
        }
    }

    fn coordination_event(event_id: &str, ts: u64) -> CoordinationEvent {
        CoordinationEvent {
            meta: EventMeta {
                id: EventId::new(event_id),
                ts,
                actor: EventActor::Agent,
                correlation: None,
                causation: None,
                execution_context: None,
            },
            kind: CoordinationEventKind::PlanCreated,
            summary: "create plan".to_string(),
            plan: None,
            task: None,
            claim: None,
            artifact: None,
            review: None,
            metadata: serde_json::Value::Null,
        }
    }

    fn event_execution_record(id: &str, claimed_at: u64) -> EventExecutionRecord {
        EventExecutionRecord {
            id: EventExecutionId::new(id),
            trigger_kind: EventTriggerKind::RecurringPlanTick,
            trigger_target: Some(prism_ir::NodeRef::plan(PlanId::new("plan:test"))),
            hook_id: Some("hook:test".to_string()),
            hook_version_digest: Some("sha256:test".to_string()),
            authoritative_revision: Some(1),
            status: EventExecutionStatus::Claimed,
            owner: Some(EventExecutionOwner {
                principal: Some(PrincipalActor {
                    authority_id: PrincipalAuthorityId::new("authority:test"),
                    principal_id: PrincipalId::new("principal:test"),
                    kind: Some(PrincipalKind::Agent),
                    name: Some("principal:test".to_string()),
                }),
                session_id: Some(SessionId::new("session:test")),
                worktree_id: Some("worktree:test".to_string()),
                service_instance_id: Some("service:test".to_string()),
            }),
            claimed_at,
            started_at: None,
            finished_at: None,
            expires_at: Some(claimed_at + 30),
            summary: Some("tick".to_string()),
            metadata: serde_json::json!({ "attempt": 1 }),
        }
    }

    fn claim_transition(record: EventExecutionRecord) -> EventExecutionTransitionRequest {
        EventExecutionTransitionRequest {
            event_execution_id: record.id.clone(),
            preconditions: EventExecutionTransitionPreconditions {
                require_missing: true,
                ..EventExecutionTransitionPreconditions::default()
            },
            transition: EventExecutionTransitionKind::Claim { record },
        }
    }

    #[test]
    fn sqlite_authority_commits_and_reads_current_state() {
        let root = temp_root();
        let store = SqliteCoordinationAuthorityStore::new(&root, temp_db_path(&root));

        let result = store
            .append_events(default_transaction_request(
                CoordinationTransactionBase::ExpectedRevision(0),
            ))
            .expect("authority transaction should succeed");

        assert_eq!(result.status, CoordinationTransactionStatus::Committed);
        assert!(result.committed);
        assert_eq!(
            result.authority.as_ref().map(|value| value.backend_kind),
            Some(CoordinationAuthorityBackendKind::Sqlite)
        );
        let current = store
            .read_summary(CoordinationReadConsistency::Strong)
            .expect("current authority read should succeed");
        assert_eq!(
            current.freshness,
            CoordinationReadFreshness::VerifiedCurrent
        );
        assert!(current.value.is_some());

        let _ = fs::remove_dir_all(root);
    }

    #[test]
    fn sqlite_authority_publishes_and_clears_runtime_descriptors() {
        let root = temp_root();
        let store = SqliteCoordinationAuthorityStore::new(&root, temp_db_path(&root));

        let publish = store
            .publish_runtime_descriptor(RuntimeDescriptorPublishRequest {
                base: CoordinationTransactionBase::LatestStrong,
                descriptor: runtime_descriptor("runtime:test"),
            })
            .expect("runtime descriptor publish should succeed");
        assert!(publish.committed);
        let listed = store
            .list_runtime_descriptors(RuntimeDescriptorQuery {
                consistency: CoordinationReadConsistency::Strong,
            })
            .expect("descriptor listing should succeed");
        assert_eq!(listed.value.unwrap_or_default().len(), 1);

        let cleared = store
            .clear_runtime_descriptor(RuntimeDescriptorClearRequest {
                base: CoordinationTransactionBase::LatestStrong,
                runtime_id: "runtime:test".to_string(),
            })
            .expect("runtime descriptor clear should succeed");
        assert!(cleared.committed);
        let listed = store
            .list_runtime_descriptors(RuntimeDescriptorQuery {
                consistency: CoordinationReadConsistency::Strong,
            })
            .expect("descriptor listing should succeed after clear");
        assert!(listed.value.unwrap_or_default().is_empty());

        let _ = fs::remove_dir_all(root);
    }

    #[test]
    fn sqlite_authority_records_retained_history() {
        let root = temp_root();
        let store = SqliteCoordinationAuthorityStore::new(&root, temp_db_path(&root));

        store
            .append_events(default_transaction_request(
                CoordinationTransactionBase::ExpectedRevision(0),
            ))
            .expect("authority transaction should succeed");

        let history = store
            .read_history(CoordinationHistoryRequest { limit: Some(10) })
            .expect("history read should succeed");
        assert_eq!(
            history.backend_kind,
            CoordinationAuthorityBackendKind::Sqlite
        );
        assert_eq!(history.entries.len(), 1);
        assert!(history.entries[0].summary.contains("coordination"));

        let _ = fs::remove_dir_all(root);
    }

    #[test]
    fn sqlite_authority_conflicts_when_expected_revision_is_stale() {
        let root = temp_root();
        let store = SqliteCoordinationAuthorityStore::new(&root, temp_db_path(&root));
        let event = coordination_event("coordination:event:sqlite:1", 1);

        store
            .append_events(CoordinationAppendRequest {
                base: CoordinationTransactionBase::ExpectedRevision(0),
                session_id: None,
                appended_events: vec![event],
            })
            .expect("initial authority transaction should succeed");

        let result = store
            .append_events(default_transaction_request(
                CoordinationTransactionBase::ExpectedRevision(0),
            ))
            .expect("stale authority transaction should return a conflict result");

        assert_eq!(result.status, CoordinationTransactionStatus::Conflict);
        assert!(!result.committed);
        assert_eq!(
            result.conflict.as_ref().map(|value| value.reason.as_str()),
            Some(
                "authority revision no longer matches the current sqlite state: expected `0`, found `1`"
            )
        );

        let _ = fs::remove_dir_all(root);
    }

    #[test]
    fn sqlite_authority_round_trips_event_execution_records() {
        let root = temp_root();
        let store = SqliteCoordinationAuthorityStore::new(&root, temp_db_path(&root));
        let record = event_execution_record("event-exec:sqlite:1", 100);

        let persisted = store
            .upsert_event_execution_record(record.clone())
            .expect("event execution record write should succeed");
        assert_eq!(persisted.record, record);
        assert!(
            persisted.authority.is_none()
                || persisted.authority.as_ref().map(|value| value.backend_kind)
                    == Some(CoordinationAuthorityBackendKind::Sqlite)
        );

        let records = store
            .read_event_execution_records(EventExecutionRecordAuthorityQuery {
                consistency: CoordinationReadConsistency::Strong,
                event_execution_id: Some(record.id.clone()),
                limit: None,
            })
            .expect("event execution record read should succeed");
        assert_eq!(records.value.unwrap_or_default(), vec![record]);

        let _ = fs::remove_dir_all(root);
    }

    #[test]
    fn sqlite_authority_claims_missing_event_execution_record() {
        let root = temp_root();
        let store = SqliteCoordinationAuthorityStore::new(&root, temp_db_path(&root));
        let record = event_execution_record("event-exec:sqlite:claim", 100);

        let result = store
            .apply_event_execution_transition(claim_transition(record.clone()))
            .expect("claim transition should succeed");

        assert_eq!(result.status, EventExecutionTransitionStatus::Applied);
        assert_eq!(result.record, Some(record.clone()));

        let stored = store
            .read_event_execution_records(EventExecutionRecordAuthorityQuery {
                consistency: CoordinationReadConsistency::Strong,
                event_execution_id: Some(record.id.clone()),
                limit: None,
            })
            .expect("claimed event execution record should be readable")
            .value
            .unwrap_or_default();
        assert_eq!(stored, vec![record]);

        let _ = fs::remove_dir_all(root);
    }

    #[test]
    fn sqlite_authority_conflicts_when_event_execution_owner_is_stale() {
        let root = temp_root();
        let store = SqliteCoordinationAuthorityStore::new(&root, temp_db_path(&root));
        let record = event_execution_record("event-exec:sqlite:owner-conflict", 100);
        let stale_owner = EventExecutionOwner {
            principal: Some(PrincipalActor {
                authority_id: PrincipalAuthorityId::new("authority:other"),
                principal_id: PrincipalId::new("principal:other"),
                kind: Some(PrincipalKind::Agent),
                name: Some("principal:other".to_string()),
            }),
            session_id: Some(SessionId::new("session:other")),
            worktree_id: Some("worktree:other".to_string()),
            service_instance_id: Some("service:other".to_string()),
        };

        store
            .apply_event_execution_transition(claim_transition(record.clone()))
            .expect("claim transition should succeed");

        let result = store
            .apply_event_execution_transition(EventExecutionTransitionRequest {
                event_execution_id: record.id.clone(),
                preconditions: EventExecutionTransitionPreconditions {
                    require_missing: false,
                    expected_status: Some(EventExecutionStatus::Claimed),
                    expected_owner: EventExecutionOwnerExpectation::Exact(stale_owner),
                },
                transition: EventExecutionTransitionKind::Start {
                    started_at: 110,
                    summary: Some("started".to_string()),
                },
            })
            .expect("start transition should return a conflict result");

        assert_eq!(result.status, EventExecutionTransitionStatus::Conflict);
        assert_eq!(
            result.conflict.as_ref().map(|value| value.reason.as_str()),
            Some("event execution owner no longer matches the expected owner")
        );

        let _ = fs::remove_dir_all(root);
    }

    #[test]
    fn sqlite_authority_starts_claimed_event_execution_when_preconditions_match() {
        let root = temp_root();
        let store = SqliteCoordinationAuthorityStore::new(&root, temp_db_path(&root));
        let record = event_execution_record("event-exec:sqlite:start", 100);

        store
            .apply_event_execution_transition(claim_transition(record.clone()))
            .expect("claim transition should succeed");

        let result = store
            .apply_event_execution_transition(EventExecutionTransitionRequest {
                event_execution_id: record.id.clone(),
                preconditions: EventExecutionTransitionPreconditions {
                    require_missing: false,
                    expected_status: Some(EventExecutionStatus::Claimed),
                    expected_owner: EventExecutionOwnerExpectation::Exact(
                        record.owner.clone().expect("event execution owner"),
                    ),
                },
                transition: EventExecutionTransitionKind::Start {
                    started_at: 120,
                    summary: Some("running".to_string()),
                },
            })
            .expect("start transition should succeed");

        let started = result.record.expect("transition result record");
        assert_eq!(started.status, EventExecutionStatus::Running);
        assert_eq!(started.started_at, Some(120));
        assert_eq!(started.summary.as_deref(), Some("running"));

        let _ = fs::remove_dir_all(root);
    }
}
