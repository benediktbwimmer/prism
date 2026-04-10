use anyhow::Result;
use prism_core::{
    CoordinationReadConsistency, SharedExecutionFamily, SharedExecutionRecord,
    SharedExecutionRecordQuery, SharedExecutionStatus,
};
use prism_ir::{EventExecutionId, Timestamp};

use crate::workspace_event_engine::{service_event_execution_owner, WorkspaceEventEngine};

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct EventTriggerExecutionPassRequest {
    pub(crate) now: Timestamp,
    pub(crate) limit: Option<usize>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum EventTriggerExecutionAction {
    Start,
    Expire,
}

#[derive(Debug, Clone, PartialEq)]
pub(crate) struct EventTriggerExecutionPassCandidate {
    pub(crate) record: SharedExecutionRecord,
    pub(crate) action: EventTriggerExecutionAction,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum EventTriggerExecutionPassSkipReason {
    MissingOwner,
    OwnerMismatch,
    NonClaimedStatus { status: SharedExecutionStatus },
}

#[derive(Debug, Clone, PartialEq)]
pub(crate) enum EventTriggerExecutionPassOutcome {
    Candidate(EventTriggerExecutionPassCandidate),
    Skipped {
        event_execution_id: EventExecutionId,
        reason: EventTriggerExecutionPassSkipReason,
    },
}

#[derive(Debug, Clone, PartialEq)]
pub(crate) struct EventTriggerExecutionPassPlan {
    pub(crate) evaluated_at: Timestamp,
    pub(crate) outcomes: Vec<EventTriggerExecutionPassOutcome>,
}

impl WorkspaceEventEngine {
    pub(crate) fn plan_trigger_execution_pass(
        &self,
        request: EventTriggerExecutionPassRequest,
    ) -> Result<EventTriggerExecutionPassPlan> {
        let owner = service_event_execution_owner(self.workspace_root());
        let mut records = self.read_execution_records(SharedExecutionRecordQuery {
            consistency: CoordinationReadConsistency::Strong,
            family: Some(SharedExecutionFamily::EventJob),
            execution_id: None,
            limit: None,
        })?;
        records.sort_by(|left, right| {
            left.claimed_at
                .cmp(&right.claimed_at)
                .then_with(|| left.execution_id.cmp(&right.execution_id))
        });

        let mut outcomes = Vec::new();
        let mut candidate_count = 0usize;
        for record in records {
            let Some(record_owner) = record.owner.as_ref() else {
                outcomes.push(EventTriggerExecutionPassOutcome::Skipped {
                    event_execution_id: EventExecutionId::new(record.execution_id.clone()),
                    reason: EventTriggerExecutionPassSkipReason::MissingOwner,
                });
                continue;
            };
            if *record_owner != owner {
                outcomes.push(EventTriggerExecutionPassOutcome::Skipped {
                    event_execution_id: EventExecutionId::new(record.execution_id.clone()),
                    reason: EventTriggerExecutionPassSkipReason::OwnerMismatch,
                });
                continue;
            }
            if record.status != SharedExecutionStatus::Claimed {
                outcomes.push(EventTriggerExecutionPassOutcome::Skipped {
                    event_execution_id: EventExecutionId::new(record.execution_id.clone()),
                    reason: EventTriggerExecutionPassSkipReason::NonClaimedStatus {
                        status: record.status,
                    },
                });
                continue;
            }
            if request.limit.is_some_and(|limit| candidate_count >= limit) {
                break;
            }
            let action = match record.expires_at {
                Some(expires_at) if expires_at <= request.now => {
                    EventTriggerExecutionAction::Expire
                }
                _ => EventTriggerExecutionAction::Start,
            };
            outcomes.push(EventTriggerExecutionPassOutcome::Candidate(
                EventTriggerExecutionPassCandidate { record, action },
            ));
            candidate_count += 1;
        }

        Ok(EventTriggerExecutionPassPlan {
            evaluated_at: request.now,
            outcomes,
        })
    }
}
