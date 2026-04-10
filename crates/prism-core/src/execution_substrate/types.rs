use anyhow::{anyhow, Result};
use prism_coordination::{EventExecutionOwner, EventExecutionRecord};
use prism_ir::{EventExecutionId, EventExecutionStatus, EventTriggerKind, NodeRef, Timestamp};
use serde::{Deserialize, Serialize};
use serde_json::{Map, Value};

use crate::coordination_authority_store::{
    CoordinationAuthorityStamp, CoordinationConflictInfo, CoordinationReadEnvelope,
    CoordinationTransactionDiagnostic, EventExecutionOwnerExpectation,
    EventExecutionRecordWriteResult, EventExecutionTransitionKind,
    EventExecutionTransitionPreconditions, EventExecutionTransitionRequest,
    EventExecutionTransitionResult, EventExecutionTransitionStatus,
};
use crate::coordination_reads::CoordinationReadConsistency;

const EXECUTION_SUBSTRATE_METADATA_KEY: &str = "executionSubstrate";

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SharedExecutionFamily {
    EventJob,
    Validation,
    Action,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SharedExecutionRunnerCategory {
    EventRunner,
    ValidationRunner,
    ActionRunner,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SharedExecutionRunnerRef {
    pub category: SharedExecutionRunnerCategory,
    pub kind: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SharedExecutionCapabilityClassRef {
    pub name: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum SharedExecutionTargetRef {
    Service {
        #[serde(default)]
        worktree_id: Option<String>,
        #[serde(default)]
        service_instance_id: Option<String>,
    },
    Runtime {
        runtime_id: String,
    },
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum SharedExecutionSourceRef {
    EventTrigger {
        trigger_kind: EventTriggerKind,
        #[serde(default)]
        trigger_target: Option<NodeRef>,
        #[serde(default)]
        hook_id: Option<String>,
        #[serde(default)]
        hook_version_digest: Option<String>,
        #[serde(default)]
        authoritative_revision: Option<u64>,
    },
    Validation {
        task_id: String,
    },
    Action {
        action_id: String,
    },
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SharedExecutionStatus {
    Claimed,
    Running,
    Succeeded,
    Failed,
    Expired,
    Abandoned,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SharedExecutionResultEnvelope {
    pub status: SharedExecutionStatus,
    #[serde(default)]
    pub summary: Option<String>,
    #[serde(default)]
    pub duration_ms: Option<u64>,
    #[serde(default)]
    pub details: Value,
    #[serde(default)]
    pub evidence_refs: Vec<String>,
    #[serde(default)]
    pub diagnostics_refs: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SharedExecutionRecord {
    pub execution_id: String,
    pub family: SharedExecutionFamily,
    pub source: SharedExecutionSourceRef,
    pub runner: SharedExecutionRunnerRef,
    #[serde(default)]
    pub capability_class: Option<SharedExecutionCapabilityClassRef>,
    #[serde(default)]
    pub target: Option<SharedExecutionTargetRef>,
    pub status: SharedExecutionStatus,
    #[serde(default)]
    pub owner: Option<EventExecutionOwner>,
    pub claimed_at: Timestamp,
    #[serde(default)]
    pub started_at: Option<Timestamp>,
    #[serde(default)]
    pub finished_at: Option<Timestamp>,
    #[serde(default)]
    pub expires_at: Option<Timestamp>,
    #[serde(default)]
    pub summary: Option<String>,
    #[serde(default)]
    pub result: Option<SharedExecutionResultEnvelope>,
    #[serde(default)]
    pub details: Value,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SharedExecutionRecordQuery {
    pub consistency: CoordinationReadConsistency,
    pub family: Option<SharedExecutionFamily>,
    pub execution_id: Option<EventExecutionId>,
    pub limit: Option<usize>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SharedExecutionOwnerExpectation {
    Any,
    Missing,
    Exact(EventExecutionOwner),
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SharedExecutionTransitionPreconditions {
    pub require_missing: bool,
    pub expected_status: Option<SharedExecutionStatus>,
    pub expected_owner: SharedExecutionOwnerExpectation,
}

impl Default for SharedExecutionTransitionPreconditions {
    fn default() -> Self {
        Self {
            require_missing: false,
            expected_status: None,
            expected_owner: SharedExecutionOwnerExpectation::Any,
        }
    }
}

#[derive(Debug, Clone, PartialEq)]
pub enum SharedExecutionTransitionKind {
    Claim {
        record: SharedExecutionRecord,
    },
    Start {
        started_at: Timestamp,
        summary: Option<String>,
    },
    Succeed {
        finished_at: Timestamp,
        summary: Option<String>,
    },
    Fail {
        finished_at: Timestamp,
        summary: Option<String>,
    },
    Expire {
        finished_at: Timestamp,
        summary: Option<String>,
    },
    Abandon {
        finished_at: Timestamp,
        summary: Option<String>,
    },
}

#[derive(Debug, Clone, PartialEq)]
pub struct SharedExecutionTransitionRequest {
    pub execution_id: EventExecutionId,
    pub preconditions: SharedExecutionTransitionPreconditions,
    pub transition: SharedExecutionTransitionKind,
}

#[derive(Debug, Clone, PartialEq)]
pub struct SharedExecutionTransitionResult {
    pub status: EventExecutionTransitionStatus,
    pub authority: Option<CoordinationAuthorityStamp>,
    pub record: Option<SharedExecutionRecord>,
    pub conflict: Option<CoordinationConflictInfo>,
    pub diagnostics: Vec<CoordinationTransactionDiagnostic>,
}

#[derive(Debug, Clone, PartialEq)]
pub struct SharedExecutionRecordWriteResult {
    pub authority: Option<CoordinationAuthorityStamp>,
    pub record: SharedExecutionRecord,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
struct SharedExecutionMetadata {
    family: SharedExecutionFamily,
    runner: SharedExecutionRunnerRef,
    #[serde(default)]
    capability_class: Option<SharedExecutionCapabilityClassRef>,
    #[serde(default)]
    target: Option<SharedExecutionTargetRef>,
    #[serde(default)]
    result: Option<SharedExecutionResultEnvelope>,
}

impl SharedExecutionRecord {
    pub fn from_event_execution_record(record: EventExecutionRecord) -> Self {
        let metadata = shared_execution_metadata_from_value(&record.metadata);
        let family = metadata
            .as_ref()
            .map(|value| value.family)
            .unwrap_or(SharedExecutionFamily::EventJob);
        let runner = metadata
            .as_ref()
            .map(|value| value.runner.clone())
            .unwrap_or_else(|| default_event_runner_ref(&record));
        let capability_class = metadata
            .as_ref()
            .and_then(|value| value.capability_class.clone());
        let target = metadata
            .as_ref()
            .and_then(|value| value.target.clone())
            .or_else(|| infer_target_from_owner(record.owner.as_ref()));
        let result = metadata.as_ref().and_then(|value| value.result.clone());

        Self {
            execution_id: record.id.0.to_string(),
            family,
            source: SharedExecutionSourceRef::EventTrigger {
                trigger_kind: record.trigger_kind,
                trigger_target: record.trigger_target.clone(),
                hook_id: record.hook_id.clone(),
                hook_version_digest: record.hook_version_digest.clone(),
                authoritative_revision: record.authoritative_revision,
            },
            runner,
            capability_class,
            target,
            status: SharedExecutionStatus::from(record.status),
            owner: record.owner,
            claimed_at: record.claimed_at,
            started_at: record.started_at,
            finished_at: record.finished_at,
            expires_at: record.expires_at,
            summary: record.summary,
            result,
            details: strip_execution_substrate_metadata(record.metadata),
        }
    }

    pub fn into_event_execution_record(self) -> Result<EventExecutionRecord> {
        let SharedExecutionSourceRef::EventTrigger {
            trigger_kind,
            trigger_target,
            hook_id,
            hook_version_digest,
            authoritative_revision,
        } = self.source
        else {
            return Err(anyhow!(
                "shared execution source cannot yet persist through the current authority-backed \
                 event execution store"
            ));
        };

        Ok(EventExecutionRecord {
            id: EventExecutionId::new(self.execution_id),
            trigger_kind,
            trigger_target,
            hook_id,
            hook_version_digest,
            authoritative_revision,
            status: self.status.into(),
            owner: self.owner,
            claimed_at: self.claimed_at,
            started_at: self.started_at,
            finished_at: self.finished_at,
            expires_at: self.expires_at,
            summary: self.summary,
            metadata: attach_execution_substrate_metadata(
                self.details,
                SharedExecutionMetadata {
                    family: self.family,
                    runner: self.runner,
                    capability_class: self.capability_class,
                    target: self.target,
                    result: self.result,
                },
            ),
        })
    }
}

impl From<SharedExecutionStatus> for EventExecutionStatus {
    fn from(value: SharedExecutionStatus) -> Self {
        match value {
            SharedExecutionStatus::Claimed => Self::Claimed,
            SharedExecutionStatus::Running => Self::Running,
            SharedExecutionStatus::Succeeded => Self::Succeeded,
            SharedExecutionStatus::Failed => Self::Failed,
            SharedExecutionStatus::Expired => Self::Expired,
            SharedExecutionStatus::Abandoned => Self::Abandoned,
        }
    }
}

impl From<EventExecutionStatus> for SharedExecutionStatus {
    fn from(value: EventExecutionStatus) -> Self {
        match value {
            EventExecutionStatus::Claimed => Self::Claimed,
            EventExecutionStatus::Running => Self::Running,
            EventExecutionStatus::Succeeded => Self::Succeeded,
            EventExecutionStatus::Failed => Self::Failed,
            EventExecutionStatus::Expired => Self::Expired,
            EventExecutionStatus::Abandoned => Self::Abandoned,
        }
    }
}

impl From<SharedExecutionOwnerExpectation> for EventExecutionOwnerExpectation {
    fn from(value: SharedExecutionOwnerExpectation) -> Self {
        match value {
            SharedExecutionOwnerExpectation::Any => Self::Any,
            SharedExecutionOwnerExpectation::Missing => Self::Missing,
            SharedExecutionOwnerExpectation::Exact(owner) => Self::Exact(owner),
        }
    }
}

impl From<SharedExecutionTransitionPreconditions> for EventExecutionTransitionPreconditions {
    fn from(value: SharedExecutionTransitionPreconditions) -> Self {
        Self {
            require_missing: value.require_missing,
            expected_status: value.expected_status.map(Into::into),
            expected_owner: value.expected_owner.into(),
        }
    }
}

impl SharedExecutionTransitionRequest {
    pub fn into_event_execution_transition_request(
        self,
    ) -> Result<EventExecutionTransitionRequest> {
        let transition = match self.transition {
            SharedExecutionTransitionKind::Claim { record } => {
                EventExecutionTransitionKind::Claim {
                    record: record.into_event_execution_record()?,
                }
            }
            SharedExecutionTransitionKind::Start {
                started_at,
                summary,
            } => EventExecutionTransitionKind::Start {
                started_at,
                summary,
            },
            SharedExecutionTransitionKind::Succeed {
                finished_at,
                summary,
            } => EventExecutionTransitionKind::Succeed {
                finished_at,
                summary,
            },
            SharedExecutionTransitionKind::Fail {
                finished_at,
                summary,
            } => EventExecutionTransitionKind::Fail {
                finished_at,
                summary,
            },
            SharedExecutionTransitionKind::Expire {
                finished_at,
                summary,
            } => EventExecutionTransitionKind::Expire {
                finished_at,
                summary,
            },
            SharedExecutionTransitionKind::Abandon {
                finished_at,
                summary,
            } => EventExecutionTransitionKind::Abandon {
                finished_at,
                summary,
            },
        };

        Ok(EventExecutionTransitionRequest {
            event_execution_id: self.execution_id,
            preconditions: self.preconditions.into(),
            transition,
        })
    }
}

impl SharedExecutionTransitionResult {
    pub fn from_event_execution_transition_result(value: EventExecutionTransitionResult) -> Self {
        Self {
            status: value.status,
            authority: value.authority,
            record: value
                .record
                .map(SharedExecutionRecord::from_event_execution_record),
            conflict: value.conflict,
            diagnostics: value.diagnostics,
        }
    }
}

impl SharedExecutionRecordWriteResult {
    pub fn from_event_execution_record_write_result(
        value: EventExecutionRecordWriteResult,
    ) -> Self {
        Self {
            authority: value.authority,
            record: SharedExecutionRecord::from_event_execution_record(value.record),
        }
    }
}

fn default_event_runner_ref(record: &EventExecutionRecord) -> SharedExecutionRunnerRef {
    SharedExecutionRunnerRef {
        category: SharedExecutionRunnerCategory::EventRunner,
        kind: record
            .hook_id
            .clone()
            .unwrap_or_else(|| default_event_runner_kind(record.trigger_kind)),
    }
}

fn default_event_runner_kind(kind: EventTriggerKind) -> String {
    match kind {
        EventTriggerKind::TaskBecameActionable => "task_became_actionable",
        EventTriggerKind::ClaimExpired => "claim_expired",
        EventTriggerKind::RecurringPlanTick => "recurring_plan_tick",
        EventTriggerKind::RuntimeBecameStale => "runtime_became_stale",
        EventTriggerKind::HookRequested => "hook_requested",
    }
    .to_string()
}

fn infer_target_from_owner(
    owner: Option<&EventExecutionOwner>,
) -> Option<SharedExecutionTargetRef> {
    let owner = owner?;
    if owner.worktree_id.is_some() || owner.service_instance_id.is_some() {
        Some(SharedExecutionTargetRef::Service {
            worktree_id: owner.worktree_id.clone(),
            service_instance_id: owner.service_instance_id.clone(),
        })
    } else {
        None
    }
}

fn shared_execution_metadata_from_value(value: &Value) -> Option<SharedExecutionMetadata> {
    let object = value.as_object()?;
    let substrate = object.get(EXECUTION_SUBSTRATE_METADATA_KEY)?;
    serde_json::from_value(substrate.clone()).ok()
}

fn strip_execution_substrate_metadata(value: Value) -> Value {
    match value {
        Value::Object(mut object) => {
            object.remove(EXECUTION_SUBSTRATE_METADATA_KEY);
            Value::Object(object)
        }
        other => other,
    }
}

fn attach_execution_substrate_metadata(value: Value, metadata: SharedExecutionMetadata) -> Value {
    let mut object = match value {
        Value::Object(object) => object,
        Value::Null => Map::new(),
        other => {
            let mut object = Map::new();
            object.insert("details".to_string(), other);
            object
        }
    };
    object.insert(
        EXECUTION_SUBSTRATE_METADATA_KEY.to_string(),
        serde_json::to_value(metadata).unwrap_or(Value::Null),
    );
    Value::Object(object)
}

pub fn shared_execution_records_from_event_envelope(
    envelope: CoordinationReadEnvelope<Vec<EventExecutionRecord>>,
    family: Option<SharedExecutionFamily>,
) -> CoordinationReadEnvelope<Vec<SharedExecutionRecord>> {
    CoordinationReadEnvelope {
        consistency: envelope.consistency,
        freshness: envelope.freshness,
        authority: envelope.authority,
        value: envelope.value.map(|records| {
            records
                .into_iter()
                .map(SharedExecutionRecord::from_event_execution_record)
                .filter(|record| family.is_none_or(|expected| record.family == expected))
                .collect()
        }),
        refresh_error: envelope.refresh_error,
    }
}

#[cfg(test)]
mod tests {
    use prism_coordination::EventExecutionOwner;
    use prism_ir::{EventExecutionId, EventExecutionStatus, EventTriggerKind, NodeRef, PlanId};
    use serde_json::{json, Value};

    use super::{
        SharedExecutionFamily, SharedExecutionRecord, SharedExecutionRunnerCategory,
        SharedExecutionRunnerRef, SharedExecutionSourceRef, SharedExecutionStatus,
        SharedExecutionTargetRef,
    };

    #[test]
    fn shared_execution_record_round_trips_through_event_execution_storage_shape() {
        let shared = SharedExecutionRecord {
            execution_id: "event-exec:test".to_string(),
            family: SharedExecutionFamily::EventJob,
            source: SharedExecutionSourceRef::EventTrigger {
                trigger_kind: EventTriggerKind::RecurringPlanTick,
                trigger_target: Some(NodeRef::plan(PlanId::new("plan:test"))),
                hook_id: Some("hooks/recurring-plan".to_string()),
                hook_version_digest: Some("sha256:hook".to_string()),
                authoritative_revision: Some(7),
            },
            runner: SharedExecutionRunnerRef {
                category: SharedExecutionRunnerCategory::EventRunner,
                kind: "recurring_plan_tick".to_string(),
            },
            capability_class: None,
            target: Some(SharedExecutionTargetRef::Service {
                worktree_id: Some("worktree:test".to_string()),
                service_instance_id: Some("service:test".to_string()),
            }),
            status: SharedExecutionStatus::Claimed,
            owner: Some(EventExecutionOwner {
                principal: None,
                session_id: None,
                worktree_id: Some("worktree:test".to_string()),
                service_instance_id: Some("service:test".to_string()),
            }),
            claimed_at: 100,
            started_at: None,
            finished_at: None,
            expires_at: Some(160),
            summary: Some("claimed".to_string()),
            result: None,
            details: json!({
                "eventTrigger": {
                    "kind": "recurring_plan_tick"
                }
            }),
        };

        let stored = shared
            .clone()
            .into_event_execution_record()
            .expect("shared execution record should persist through event execution storage");
        assert!(stored.metadata.get("executionSubstrate").is_some());

        let round_trip = SharedExecutionRecord::from_event_execution_record(stored);
        assert_eq!(round_trip, shared);
    }

    #[test]
    fn shared_execution_record_infers_default_runner_and_target_from_legacy_event_record() {
        let legacy = prism_coordination::EventExecutionRecord {
            id: EventExecutionId::new("event-exec:legacy"),
            trigger_kind: EventTriggerKind::RecurringPlanTick,
            trigger_target: None,
            hook_id: None,
            hook_version_digest: None,
            authoritative_revision: None,
            status: EventExecutionStatus::Claimed,
            owner: Some(EventExecutionOwner {
                principal: None,
                session_id: None,
                worktree_id: Some("worktree:test".to_string()),
                service_instance_id: Some("service:test".to_string()),
            }),
            claimed_at: 5,
            started_at: None,
            finished_at: None,
            expires_at: None,
            summary: None,
            metadata: Value::Null,
        };

        let shared = SharedExecutionRecord::from_event_execution_record(legacy);
        assert_eq!(shared.family, SharedExecutionFamily::EventJob);
        assert_eq!(
            shared.runner,
            SharedExecutionRunnerRef {
                category: SharedExecutionRunnerCategory::EventRunner,
                kind: "recurring_plan_tick".to_string(),
            }
        );
        assert_eq!(
            shared.target,
            Some(SharedExecutionTargetRef::Service {
                worktree_id: Some("worktree:test".to_string()),
                service_instance_id: Some("service:test".to_string()),
            })
        );
    }
}
