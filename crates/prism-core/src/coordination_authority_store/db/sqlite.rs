use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use anyhow::{Context, Result};
use prism_coordination::{
    coordination_snapshot_from_events, CoordinationSnapshot, CoordinationSnapshotV2,
    EventExecutionRecord, RuntimeDescriptor,
};
use prism_ir::EventExecutionStatus;
use prism_store::{
    CoordinationCheckpointStore, CoordinationEventExecutionStore, CoordinationJournal,
    CoordinationPersistBatch, CoordinationStartupCheckpoint,
    CoordinationStartupCheckpointAuthority, SqliteStore,
};
use rusqlite::Connection;

use super::store::DbCoordinationAuthorityStore;
use super::traits::CoordinationAuthorityDb;
use crate::coordination_authority_store::{
    CoordinationAuthorityBackendDetails, CoordinationAuthorityBackendKind,
    CoordinationAuthorityCapabilities, CoordinationAuthorityDiagnostics,
    CoordinationAuthorityProvenance, CoordinationAuthorityStamp, CoordinationAuthorityStore,
    CoordinationConflictInfo, CoordinationCurrentState, CoordinationDiagnosticsRequest,
    CoordinationHistoryEntry, CoordinationHistoryEnvelope, CoordinationHistoryRequest,
    CoordinationReadEnvelope, CoordinationReadRequest, CoordinationTransactionBase,
    CoordinationTransactionRequest, CoordinationTransactionResult, CoordinationTransactionStatus,
    EventExecutionOwnerExpectation, EventExecutionRecordAuthorityQuery,
    EventExecutionRecordWriteResult, EventExecutionTransitionKind,
    EventExecutionTransitionPreconditions, EventExecutionTransitionRequest,
    EventExecutionTransitionResult, EventExecutionTransitionStatus, RuntimeDescriptorClearRequest,
    RuntimeDescriptorPublishRequest, RuntimeDescriptorQuery,
    SqliteCoordinationAuthorityBackendDetails,
};
use crate::coordination_reads::CoordinationReadConsistency;
use crate::coordination_persistence::repo_semantic_coordination_snapshot;
use crate::util::current_timestamp;
use crate::workspace_identity::{
    coordination_persist_context_for_root, workspace_identity_for_root,
};
use crate::PrismPaths;

const SQLITE_AUTHORITY_REF_NAME: &str = "sqlite-authority";

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct SqliteCoordinationAuthorityDb {
    root: PathBuf,
    db_path: PathBuf,
}

#[derive(Debug, Clone)]
struct LoadedSqliteAuthorityView {
    authority: CoordinationAuthorityStamp,
    current_state: Option<CoordinationCurrentState>,
    revision: u64,
    checkpoint: Option<CoordinationStartupCheckpoint>,
}

impl SqliteCoordinationAuthorityDb {
    pub(crate) fn open(root: &Path, db_path: &Path) -> Result<Self> {
        let resolved = Self::resolve_db_path(root, db_path)?;
        let _ = SqliteStore::open(&resolved)?;
        Ok(Self {
            root: root.to_path_buf(),
            db_path: resolved,
        })
    }

    fn resolve_db_path(root: &Path, db_path: &Path) -> Result<PathBuf> {
        if db_path.as_os_str().is_empty() {
            return PrismPaths::for_workspace_root(root)?.coordination_authority_db_path();
        }
        if db_path.is_absolute() {
            return Ok(db_path.to_path_buf());
        }
        Ok(root.join(db_path))
    }

    fn open_store(&self) -> Result<SqliteStore> {
        SqliteStore::open(&self.db_path)
    }

    fn current_revision(store: &SqliteStore) -> Result<u64> {
        store.coordination_revision()
    }

    fn effective_revision(
        revision: u64,
        checkpoint: Option<&CoordinationStartupCheckpoint>,
    ) -> u64 {
        checkpoint
            .map(|value| value.coordination_revision.max(revision))
            .unwrap_or(revision)
    }

    fn authority_stamp(
        &self,
        revision: u64,
        checkpoint: Option<&CoordinationStartupCheckpoint>,
    ) -> CoordinationAuthorityStamp {
        let revision = Self::effective_revision(revision, checkpoint);
        let logical_repo_id = workspace_identity_for_root(&self.root).repo_id;
        let committed_at = checkpoint.map(|value| value.materialized_at);
        let checkpoint_token = committed_at
            .map(|value| format!("sqlite-revision:{revision}:materialized:{value}"))
            .unwrap_or_else(|| format!("sqlite-revision:{revision}"));
        CoordinationAuthorityStamp {
            backend_kind: CoordinationAuthorityBackendKind::Sqlite,
            logical_repo_id,
            snapshot_id: checkpoint_token.clone(),
            transaction_id: Some(checkpoint_token),
            committed_at,
            provenance: CoordinationAuthorityProvenance {
                ref_name: Some(SQLITE_AUTHORITY_REF_NAME.to_string()),
                head_commit: None,
                manifest_digest: None,
            },
        }
    }

    fn load_authoritative_current_state(
        &self,
        store: &mut SqliteStore,
        revision: u64,
        checkpoint: Option<&CoordinationStartupCheckpoint>,
    ) -> Result<Option<CoordinationCurrentState>> {
        let stream = store.load_coordination_event_stream()?;
        if let Some(snapshot) =
            coordination_snapshot_from_events(&stream.suffix_events, stream.fallback_snapshot)
        {
            let snapshot = if let Some(checkpoint) =
                checkpoint.filter(|value| value.coordination_revision == revision)
            {
                merge_checkpoint_counters(snapshot, &checkpoint.snapshot)
            } else {
                snapshot
            };
            let runtime_descriptors = checkpoint
                .filter(|value| value.coordination_revision == revision)
                .map(|value| value.runtime_descriptors.clone())
                .unwrap_or_default();
            let canonical_snapshot_v2 = checkpoint
                .filter(|value| value.coordination_revision == revision)
                .and_then(|value| value.canonical_snapshot_v2.clone())
                .unwrap_or_else(|| snapshot.to_canonical_snapshot_v2());
            return Ok(Some(CoordinationCurrentState {
                snapshot,
                canonical_snapshot_v2,
                runtime_descriptors,
            }));
        }

        Ok(checkpoint
            .cloned()
            .map(|checkpoint| CoordinationCurrentState {
                snapshot: checkpoint.snapshot.clone(),
                canonical_snapshot_v2: checkpoint
                    .canonical_snapshot_v2
                    .unwrap_or_else(|| checkpoint.snapshot.to_canonical_snapshot_v2()),
                runtime_descriptors: checkpoint.runtime_descriptors,
            }))
    }

    fn load_authority_view(&self) -> Result<LoadedSqliteAuthorityView> {
        let mut store = self.open_store()?;
        let revision = Self::current_revision(&store)?;
        let checkpoint = store.load_coordination_startup_checkpoint()?;
        let effective_revision = Self::effective_revision(revision, checkpoint.as_ref());
        let authority = self.authority_stamp(revision, checkpoint.as_ref());
        let current_state = self.load_authoritative_current_state(
            &mut store,
            effective_revision,
            checkpoint.as_ref(),
        )?;
        Ok(LoadedSqliteAuthorityView {
            authority,
            current_state,
            revision: effective_revision,
            checkpoint,
        })
    }

    fn conflict_transaction_result(
        &self,
        authority: CoordinationAuthorityStamp,
        snapshot: Option<CoordinationCurrentState>,
        reason: impl Into<String>,
    ) -> CoordinationTransactionResult {
        CoordinationTransactionResult {
            status: CoordinationTransactionStatus::Conflict,
            committed: false,
            authority: Some(authority),
            snapshot,
            persisted: None,
            conflict: Some(CoordinationConflictInfo {
                reason: reason.into(),
            }),
            diagnostics: Vec::new(),
        }
    }

    fn validate_transaction_base(
        &self,
        base: &CoordinationTransactionBase,
        current: &LoadedSqliteAuthorityView,
    ) -> Option<CoordinationTransactionResult> {
        match base {
            CoordinationTransactionBase::LatestStrong => None,
            CoordinationTransactionBase::ExpectedRevision(expected_revision) => {
                if current.revision == *expected_revision {
                    return None;
                }
                Some(self.conflict_transaction_result(
                    current.authority.clone(),
                    current.current_state.clone(),
                    format!(
                        "authority revision no longer matches the current sqlite coordination state: expected `{expected_revision}`, found `{}`",
                        current.revision
                    ),
                ))
            }
            CoordinationTransactionBase::ExpectedAuthorityStamp(expected) => {
                if &current.authority == expected {
                    return None;
                }
                Some(self.conflict_transaction_result(
                    current.authority.clone(),
                    current.current_state.clone(),
                    "authority stamp no longer matches the current sqlite coordination state",
                ))
            }
        }
    }

    fn revision_mismatch(error: &anyhow::Error) -> bool {
        error.to_string().contains("coordination revision mismatch")
    }

    fn build_runtime_descriptor_index(
        descriptors: &[RuntimeDescriptor],
    ) -> BTreeMap<String, RuntimeDescriptor> {
        descriptors
            .iter()
            .cloned()
            .map(|descriptor| (descriptor.runtime_id.clone(), descriptor))
            .collect()
    }

    fn descriptor_state_or_default(
        current: Option<CoordinationCurrentState>,
    ) -> CoordinationCurrentState {
        current.unwrap_or_else(|| CoordinationCurrentState {
            snapshot: CoordinationSnapshot::default(),
            canonical_snapshot_v2: CoordinationSnapshot::default().to_canonical_snapshot_v2(),
            runtime_descriptors: Vec::new(),
        })
    }

    fn persist_runtime_descriptor_state(
        &self,
        revision: u64,
        snapshot: CoordinationSnapshot,
        canonical_snapshot_v2: CoordinationSnapshotV2,
        runtime_descriptors: Vec<RuntimeDescriptor>,
    ) -> Result<()> {
        let mut authority_store = self.open_store()?;
        self.persist_authoritative_startup_checkpoint(
            &mut authority_store,
            revision,
            &snapshot,
            &canonical_snapshot_v2,
            &runtime_descriptors,
        )?;
        Ok(())
    }

    fn persist_authoritative_startup_checkpoint(
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
                ref_name: SQLITE_AUTHORITY_REF_NAME.to_string(),
                head_commit: None,
                manifest_digest: None,
            },
            snapshot: sanitized_snapshot,
            canonical_snapshot_v2: Some(canonical_snapshot_v2.clone()),
            runtime_descriptors: runtime_descriptors.to_vec(),
        })
    }

    fn conflict_event_transition_result(
        &self,
        authority: CoordinationAuthorityStamp,
        record: Option<EventExecutionRecord>,
        reason: impl Into<String>,
    ) -> EventExecutionTransitionResult {
        EventExecutionTransitionResult {
            status: EventExecutionTransitionStatus::Conflict,
            authority: Some(authority),
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
        authority: &CoordinationAuthorityStamp,
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
                ));
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
    snapshot.next_artifact = snapshot.next_artifact.max(checkpoint_snapshot.next_artifact);
    snapshot.next_review = snapshot.next_review.max(checkpoint_snapshot.next_review);
    snapshot
}

impl CoordinationAuthorityDb for SqliteCoordinationAuthorityDb {
    fn capabilities(&self) -> CoordinationAuthorityCapabilities {
        CoordinationAuthorityCapabilities {
            supports_eventual_reads: true,
            supports_transactions: true,
            supports_runtime_descriptors: true,
            supports_event_execution_records: true,
            supports_retained_history: true,
            supports_diagnostics: true,
        }
    }

    fn read_current(
        &self,
        request: CoordinationReadRequest,
    ) -> Result<CoordinationReadEnvelope<CoordinationCurrentState>> {
        let authority_view = self.load_authority_view()?;
        match request.consistency {
            CoordinationReadConsistency::Strong | CoordinationReadConsistency::Eventual => {
                match authority_view.current_state {
                    Some(state) => Ok(CoordinationReadEnvelope::verified_current(
                        request.consistency,
                        Some(authority_view.authority),
                        state,
                    )),
                    None => Ok(CoordinationReadEnvelope::unavailable(
                        request.consistency,
                        Some(authority_view.authority),
                        None,
                    )),
                }
            }
        }
    }

    fn apply_transaction(
        &self,
        request: CoordinationTransactionRequest,
    ) -> Result<CoordinationTransactionResult> {
        let current = self.load_authority_view()?;
        if let Some(result) = self.validate_transaction_base(&request.base, &current) {
            return Ok(result);
        }

        let preserved_runtime_descriptors = current
            .current_state
            .as_ref()
            .map(|state| state.runtime_descriptors.clone())
            .unwrap_or_default();
        let mut store = self.open_store()?;
        let persisted = match store.commit_coordination_persist_batch(&CoordinationPersistBatch {
            context: coordination_persist_context_for_root(&self.root, request.session_id.as_ref()),
            expected_revision: Some(current.revision),
            appended_events: request.appended_events.clone(),
        }) {
            Ok(result) => result,
            Err(error) if Self::revision_mismatch(&error) => {
                let refreshed = self.load_authority_view()?;
                return Ok(self.conflict_transaction_result(
                    refreshed.authority,
                    refreshed.current_state,
                    format!(
                        "authority revision no longer matches the current sqlite coordination state: expected `{}`, found `{}`",
                        current.revision, refreshed.revision
                    ),
                ));
            }
            Err(error) => return Err(error),
        };

        self.persist_authoritative_startup_checkpoint(
            &mut store,
            persisted.revision,
            &request.snapshot,
            &request.canonical_snapshot_v2,
            &preserved_runtime_descriptors,
        )?;

        let refreshed = self.load_authority_view()?;
        Ok(CoordinationTransactionResult {
            status: CoordinationTransactionStatus::Committed,
            committed: true,
            authority: Some(refreshed.authority),
            snapshot: refreshed.current_state,
            persisted: Some(persisted),
            conflict: None,
            diagnostics: Vec::new(),
        })
    }

    fn publish_runtime_descriptor(
        &self,
        request: RuntimeDescriptorPublishRequest,
    ) -> Result<CoordinationTransactionResult> {
        let current = self.load_authority_view()?;
        if let Some(result) = self.validate_transaction_base(&request.base, &current) {
            return Ok(result);
        }

        let state = Self::descriptor_state_or_default(current.current_state);
        let mut descriptors = Self::build_runtime_descriptor_index(&state.runtime_descriptors);
        descriptors.insert(request.descriptor.runtime_id.clone(), request.descriptor);
        self.persist_runtime_descriptor_state(
            current.revision,
            state.snapshot,
            state.canonical_snapshot_v2,
            descriptors.into_values().collect(),
        )?;

        let refreshed = self.load_authority_view()?;
        Ok(CoordinationTransactionResult {
            status: CoordinationTransactionStatus::Committed,
            committed: true,
            authority: Some(refreshed.authority),
            snapshot: refreshed.current_state,
            persisted: None,
            conflict: None,
            diagnostics: Vec::new(),
        })
    }

    fn clear_runtime_descriptor(
        &self,
        request: RuntimeDescriptorClearRequest,
    ) -> Result<CoordinationTransactionResult> {
        let current = self.load_authority_view()?;
        if let Some(result) = self.validate_transaction_base(&request.base, &current) {
            return Ok(result);
        }

        let state = Self::descriptor_state_or_default(current.current_state);
        let mut descriptors = Self::build_runtime_descriptor_index(&state.runtime_descriptors);
        descriptors.remove(&request.runtime_id);
        self.persist_runtime_descriptor_state(
            current.revision,
            state.snapshot,
            state.canonical_snapshot_v2,
            descriptors.into_values().collect(),
        )?;

        let refreshed = self.load_authority_view()?;
        Ok(CoordinationTransactionResult {
            status: CoordinationTransactionStatus::Committed,
            committed: true,
            authority: Some(refreshed.authority),
            snapshot: refreshed.current_state,
            persisted: None,
            conflict: None,
            diagnostics: Vec::new(),
        })
    }

    fn list_runtime_descriptors(
        &self,
        request: RuntimeDescriptorQuery,
    ) -> Result<CoordinationReadEnvelope<Vec<RuntimeDescriptor>>> {
        let authority_view = self.load_authority_view()?;
        match request.consistency {
            CoordinationReadConsistency::Strong | CoordinationReadConsistency::Eventual => {
                Ok(CoordinationReadEnvelope::verified_current(
                    request.consistency,
                    Some(authority_view.authority),
                    authority_view
                        .current_state
                        .map(|state| state.runtime_descriptors)
                        .unwrap_or_default(),
                ))
            }
        }
    }

    fn read_event_execution_records(
        &self,
        request: EventExecutionRecordAuthorityQuery,
    ) -> Result<CoordinationReadEnvelope<Vec<EventExecutionRecord>>> {
        let authority = self.load_authority_view()?.authority;
        let mut store = self.open_store()?;
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
            Some(authority),
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
            authority: Some(self.load_authority_view()?.authority),
            record,
        })
    }

    fn apply_event_execution_transition(
        &self,
        request: EventExecutionTransitionRequest,
    ) -> Result<EventExecutionTransitionResult> {
        let authority = self.load_authority_view()?.authority;
        let mut store = self.open_store()?;
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
            authority: Some(authority),
            record: Some(record),
            conflict: None,
            diagnostics: Vec::new(),
        })
    }

    fn read_history(
        &self,
        request: CoordinationHistoryRequest,
    ) -> Result<CoordinationHistoryEnvelope> {
        let conn = Connection::open(&self.db_path).with_context(|| {
            format!(
                "failed to open sqlite coordination authority history at {}",
                self.db_path.display()
            )
        })?;

        let (query, params): (&str, Vec<i64>) = match request.limit {
            Some(limit) => (
                "SELECT sequence, revision, inserted_events, applied
                 FROM coordination_mutation_log
                 ORDER BY sequence DESC
                 LIMIT ?1",
                vec![i64::try_from(limit.saturating_add(1)).unwrap_or(i64::MAX)],
            ),
            None => (
                "SELECT sequence, revision, inserted_events, applied
                 FROM coordination_mutation_log
                 ORDER BY sequence DESC",
                Vec::new(),
            ),
        };

        let mut stmt = conn.prepare(query)?;
        let rows = if params.is_empty() {
            stmt.query_map([], |row| {
                Ok((
                    row.get::<_, i64>(0)?,
                    row.get::<_, i64>(1)?,
                    row.get::<_, i64>(2)?,
                    row.get::<_, i64>(3)?,
                ))
            })?
            .collect::<std::result::Result<Vec<_>, _>>()?
        } else {
            stmt.query_map([params[0]], |row| {
                Ok((
                    row.get::<_, i64>(0)?,
                    row.get::<_, i64>(1)?,
                    row.get::<_, i64>(2)?,
                    row.get::<_, i64>(3)?,
                ))
            })?
            .collect::<std::result::Result<Vec<_>, _>>()?
        };

        let mut entries = rows
            .into_iter()
            .map(|(sequence, revision, inserted_events, applied)| {
                let applied = applied != 0;
                CoordinationHistoryEntry {
                    transaction_id: Some(format!("sqlite-sequence:{sequence}")),
                    snapshot_id: Some(format!("sqlite-revision:{revision}")),
                    committed_at: None,
                    summary: if applied {
                        format!(
                            "sqlite authority revision {revision} committed {inserted_events} coordination event(s)"
                        )
                    } else {
                        format!(
                            "sqlite authority revision {revision} observed no new coordination events"
                        )
                    },
                }
            })
            .collect::<Vec<_>>();
        let truncated = request
            .limit
            .map(|limit| entries.len() as u64 > limit)
            .unwrap_or(false);
        if let Some(limit) = request.limit {
            entries.truncate(usize::try_from(limit).unwrap_or(usize::MAX));
        }
        Ok(CoordinationHistoryEnvelope {
            backend_kind: CoordinationAuthorityBackendKind::Sqlite,
            entries,
            truncated,
        })
    }

    fn diagnostics(
        &self,
        _request: CoordinationDiagnosticsRequest,
    ) -> Result<CoordinationAuthorityDiagnostics> {
        let current = self.load_authority_view()?;
        let runtime_descriptor_count = current
            .current_state
            .as_ref()
            .map(|state| state.runtime_descriptors.len())
            .unwrap_or_else(|| {
                current
                    .checkpoint
                    .as_ref()
                    .map(|checkpoint| checkpoint.runtime_descriptors.len())
                    .unwrap_or(0)
            });
        Ok(CoordinationAuthorityDiagnostics {
            backend_kind: CoordinationAuthorityBackendKind::Sqlite,
            latest_authority: Some(current.authority),
            runtime_descriptor_count,
            backend_details: CoordinationAuthorityBackendDetails::Sqlite(
                SqliteCoordinationAuthorityBackendDetails {
                    db_path: self.db_path.clone(),
                    coordination_revision: Some(current.revision),
                },
            ),
        })
    }
}

pub(crate) fn open_sqlite_coordination_authority_store(
    root: &Path,
    db_path: &Path,
) -> Result<Box<dyn CoordinationAuthorityStore>> {
    let db = SqliteCoordinationAuthorityDb::open(root, db_path)?;
    Ok(Box::new(DbCoordinationAuthorityStore::new(db)))
}

#[cfg(test)]
mod tests {
    use std::fs;
    use std::path::Path;
    use std::sync::atomic::{AtomicU64, Ordering};
    use std::time::{SystemTime, UNIX_EPOCH};

    use prism_coordination::{
        CoordinationEvent, CoordinationSnapshot, RuntimeDescriptor, RuntimeDiscoveryMode,
    };
    use prism_ir::{CoordinationEventKind, EventActor, EventId, EventMeta};

    use super::SqliteCoordinationAuthorityDb;
    use crate::coordination_authority_store::db::traits::CoordinationAuthorityDb;
    use crate::coordination_authority_store::{
        CoordinationAuthorityBackendKind, CoordinationTransactionBase,
        CoordinationTransactionRequest, RuntimeDescriptorPublishRequest,
    };
    use crate::coordination_reads::CoordinationReadConsistency;

    static NEXT_TEMP_ROOT: AtomicU64 = AtomicU64::new(0);

    fn temp_workspace_root() -> std::path::PathBuf {
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

    fn coordination_event(id: &str, ts: u64, summary: &str) -> CoordinationEvent {
        CoordinationEvent {
            meta: EventMeta {
                id: EventId::new(id),
                ts,
                actor: EventActor::Agent,
                correlation: None,
                causation: None,
                execution_context: None,
            },
            kind: CoordinationEventKind::PlanCreated,
            summary: summary.to_string(),
            plan: None,
            task: None,
            claim: None,
            artifact: None,
            review: None,
            metadata: serde_json::Value::Null,
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
            checked_out_commit: Some("abc123".to_string()),
            capabilities: Vec::new(),
            discovery_mode: RuntimeDiscoveryMode::None,
            peer_endpoint: None,
            public_endpoint: None,
            peer_transport_identity: None,
            blob_snapshot_head: None,
            export_policy: None,
        }
    }

    #[test]
    fn sqlite_authority_applies_transactions_and_reads_current_state() {
        let root = temp_workspace_root();
        let authority = SqliteCoordinationAuthorityDb::open(&root, Path::new("")).unwrap();
        let event = coordination_event("coordination:event:sqlite:1", 1, "create plan");
        let snapshot = CoordinationSnapshot {
            events: vec![event.clone()],
            ..CoordinationSnapshot::default()
        };

        let result = authority
            .apply_transaction(CoordinationTransactionRequest {
                base: CoordinationTransactionBase::LatestStrong,
                session_id: None,
                snapshot: snapshot.clone(),
                canonical_snapshot_v2: snapshot.to_canonical_snapshot_v2(),
                appended_events: vec![event],
                derived_state_mode: crate::CoordinationDerivedStateMode::Inline,
            })
            .unwrap();
        assert!(result.committed);
        assert_eq!(
            result.status,
            crate::CoordinationTransactionStatus::Committed
        );
        assert_eq!(
            result.authority.as_ref().unwrap().backend_kind,
            CoordinationAuthorityBackendKind::Sqlite
        );
        assert_eq!(result.persisted.as_ref().unwrap().revision, 1);

        let current = authority
            .read_current(crate::CoordinationReadRequest {
                consistency: CoordinationReadConsistency::Strong,
                view: crate::CoordinationStateView::PlanState,
            })
            .unwrap();
        assert_eq!(current.value.unwrap().snapshot.events.len(), 1);
    }

    #[test]
    fn sqlite_authority_conflicts_on_stale_expected_revision() {
        let root = temp_workspace_root();
        let authority = SqliteCoordinationAuthorityDb::open(&root, Path::new("")).unwrap();
        let first = coordination_event("coordination:event:sqlite:2", 1, "first");
        let first_snapshot = CoordinationSnapshot {
            events: vec![first.clone()],
            ..CoordinationSnapshot::default()
        };
        authority
            .apply_transaction(CoordinationTransactionRequest {
                base: CoordinationTransactionBase::LatestStrong,
                session_id: None,
                snapshot: first_snapshot.clone(),
                canonical_snapshot_v2: first_snapshot.to_canonical_snapshot_v2(),
                appended_events: vec![first],
                derived_state_mode: crate::CoordinationDerivedStateMode::Inline,
            })
            .unwrap();

        let second = coordination_event("coordination:event:sqlite:3", 2, "second");
        let second_snapshot = CoordinationSnapshot {
            events: vec![second.clone()],
            ..CoordinationSnapshot::default()
        };
        let result = authority
            .apply_transaction(CoordinationTransactionRequest {
                base: CoordinationTransactionBase::ExpectedRevision(0),
                session_id: None,
                snapshot: second_snapshot.clone(),
                canonical_snapshot_v2: second_snapshot.to_canonical_snapshot_v2(),
                appended_events: vec![second],
                derived_state_mode: crate::CoordinationDerivedStateMode::Inline,
            })
            .unwrap();
        assert_eq!(
            result.status,
            crate::CoordinationTransactionStatus::Conflict
        );
        assert!(!result.committed);
    }

    #[test]
    fn sqlite_authority_publishes_runtime_descriptors_without_events() {
        let root = temp_workspace_root();
        let authority = SqliteCoordinationAuthorityDb::open(&root, Path::new("")).unwrap();
        authority
            .publish_runtime_descriptor(RuntimeDescriptorPublishRequest {
                base: CoordinationTransactionBase::LatestStrong,
                descriptor: runtime_descriptor("runtime:test"),
            })
            .unwrap();

        let descriptors = authority
            .list_runtime_descriptors(crate::RuntimeDescriptorQuery {
                consistency: CoordinationReadConsistency::Strong,
            })
            .unwrap()
            .value
            .unwrap();
        assert_eq!(descriptors.len(), 1);
        assert_eq!(descriptors[0].runtime_id, "runtime:test");
    }
}
