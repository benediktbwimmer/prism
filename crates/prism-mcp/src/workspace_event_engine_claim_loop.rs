use std::collections::hash_map::DefaultHasher;
use std::hash::{Hash, Hasher};

use anyhow::Result;
use prism_coordination::{EventExecutionOwner, EventExecutionRecord, Plan};
use prism_core::{
    CoordinationReadConsistency, EventExecutionOwnerExpectation,
    EventExecutionRecordAuthorityQuery, EventExecutionTransitionKind,
    EventExecutionTransitionPreconditions, EventExecutionTransitionRequest,
    EventExecutionTransitionStatus,
};
use prism_ir::{EventExecutionId, EventExecutionStatus, EventTriggerKind, NodeRef, Timestamp};
use serde_json::Value;

use crate::workspace_event_engine::WorkspaceEventEngine;

const DEFAULT_CLAIM_TTL_SECONDS: u64 = 5 * 60;

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct EventTriggerClaimLoopRequest {
    pub(crate) now: Timestamp,
    pub(crate) limit: Option<usize>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct EventTriggerClaimLoopCandidate {
    pub(crate) plan_id: String,
    pub(crate) due_at: Timestamp,
    pub(crate) event_execution_id: EventExecutionId,
    pub(crate) trigger_kind: EventTriggerKind,
    pub(crate) trigger_target: NodeRef,
    pub(crate) hook_id: Option<String>,
    pub(crate) hook_version_digest: Option<String>,
    pub(crate) recurrence_policy: Option<String>,
    pub(crate) claim_ttl_seconds: u64,
    pub(crate) authoritative_revision: Option<u64>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum EventTriggerClaimSkipReason {
    ExistingExecution {
        status: EventExecutionStatus,
    },
    ActiveTargetExecution {
        event_execution_id: EventExecutionId,
        status: EventExecutionStatus,
    },
}

#[derive(Debug, Clone, PartialEq)]
pub(crate) enum EventTriggerClaimOutcome {
    Claimed {
        candidate: EventTriggerClaimLoopCandidate,
        record: EventExecutionRecord,
    },
    Conflict {
        candidate: EventTriggerClaimLoopCandidate,
        reason: String,
    },
    Skipped {
        candidate: EventTriggerClaimLoopCandidate,
        reason: EventTriggerClaimSkipReason,
    },
}

#[derive(Debug, Clone, PartialEq)]
pub(crate) struct EventTriggerClaimLoopResult {
    pub(crate) evaluated_at: Timestamp,
    pub(crate) outcomes: Vec<EventTriggerClaimOutcome>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct RecurringPlanTickPolicy {
    hook_id: Option<String>,
    hook_version_digest: Option<String>,
    recurrence_policy: Option<String>,
    claim_ttl_seconds: u64,
}

impl WorkspaceEventEngine {
    pub(crate) fn claim_due_triggers(
        &self,
        request: EventTriggerClaimLoopRequest,
    ) -> Result<EventTriggerClaimLoopResult> {
        let candidates = self.due_trigger_candidates(&request)?;
        let existing_records =
            self.read_event_execution_records(EventExecutionRecordAuthorityQuery {
                consistency: CoordinationReadConsistency::Strong,
                event_execution_id: None,
                limit: None,
            })?;
        let owner = claim_owner(self.workspace_root())?;
        let mut outcomes = Vec::with_capacity(candidates.len());

        for candidate in candidates {
            if let Some(existing) = existing_records
                .iter()
                .find(|record| record.id == candidate.event_execution_id)
            {
                outcomes.push(EventTriggerClaimOutcome::Skipped {
                    candidate,
                    reason: EventTriggerClaimSkipReason::ExistingExecution {
                        status: existing.status,
                    },
                });
                continue;
            }
            if let Some(existing) = existing_records.iter().find(|record| {
                event_execution_is_active(record)
                    && record.trigger_kind == candidate.trigger_kind
                    && record.trigger_target.as_ref() == Some(&candidate.trigger_target)
            }) {
                outcomes.push(EventTriggerClaimOutcome::Skipped {
                    candidate,
                    reason: EventTriggerClaimSkipReason::ActiveTargetExecution {
                        event_execution_id: existing.id.clone(),
                        status: existing.status,
                    },
                });
                continue;
            }

            let record = candidate.claim_record(request.now, owner.clone());
            let result =
                self.apply_event_execution_transition(EventExecutionTransitionRequest {
                    event_execution_id: candidate.event_execution_id.clone(),
                    preconditions: EventExecutionTransitionPreconditions {
                        require_missing: true,
                        expected_status: None,
                        expected_owner: EventExecutionOwnerExpectation::Any,
                    },
                    transition: EventExecutionTransitionKind::Claim {
                        record: record.clone(),
                    },
                })?;

            match result.status {
                EventExecutionTransitionStatus::Applied => {
                    outcomes.push(EventTriggerClaimOutcome::Claimed {
                        candidate,
                        record: result.record.unwrap_or(record),
                    });
                }
                EventExecutionTransitionStatus::Conflict => {
                    let reason = result
                        .conflict
                        .map(|conflict| conflict.reason)
                        .unwrap_or_else(|| "authoritative conflict".to_string());
                    outcomes.push(EventTriggerClaimOutcome::Conflict { candidate, reason });
                }
            }
        }

        Ok(EventTriggerClaimLoopResult {
            evaluated_at: request.now,
            outcomes,
        })
    }

    fn due_trigger_candidates(
        &self,
        request: &EventTriggerClaimLoopRequest,
    ) -> Result<Vec<EventTriggerClaimLoopCandidate>> {
        let snapshot = self.read_broker().current_coordination_surface()?.snapshot;
        let mut candidates: Vec<_> = snapshot
            .plans
            .iter()
            .filter_map(|plan| recurring_plan_tick_candidate(plan, request.now))
            .collect();
        candidates.sort_by(|left, right| {
            left.due_at
                .cmp(&right.due_at)
                .then_with(|| left.plan_id.cmp(&right.plan_id))
        });
        if let Some(limit) = request.limit {
            candidates.truncate(limit);
        }
        Ok(candidates)
    }
}

impl EventTriggerClaimLoopCandidate {
    fn claim_record(
        &self,
        claimed_at: Timestamp,
        owner: EventExecutionOwner,
    ) -> EventExecutionRecord {
        EventExecutionRecord {
            id: self.event_execution_id.clone(),
            trigger_kind: self.trigger_kind,
            trigger_target: Some(self.trigger_target.clone()),
            hook_id: self.hook_id.clone(),
            hook_version_digest: self.hook_version_digest.clone(),
            authoritative_revision: self.authoritative_revision,
            status: EventExecutionStatus::Claimed,
            owner: Some(owner),
            claimed_at,
            started_at: None,
            finished_at: None,
            expires_at: Some(claimed_at + self.claim_ttl_seconds),
            summary: Some("Recurring plan tick claimed".to_string()),
            metadata: recurring_claim_metadata(self),
        }
    }
}

fn recurring_plan_tick_candidate(
    plan: &Plan,
    now: Timestamp,
) -> Option<EventTriggerClaimLoopCandidate> {
    if plan.status != prism_ir::PlanStatus::Active {
        return None;
    }
    let due_at = plan.scheduling.due_at?;
    if due_at > now {
        return None;
    }
    let policy = recurring_plan_tick_policy(&plan.metadata)?;
    let trigger_target = NodeRef::plan(plan.id.clone());
    Some(EventTriggerClaimLoopCandidate {
        plan_id: plan.id.0.to_string(),
        due_at,
        event_execution_id: EventExecutionId::new(format!(
            "event-exec:recurring-plan-tick:{}:{}:{}",
            plan.id.0, plan.revision, due_at
        )),
        trigger_kind: EventTriggerKind::RecurringPlanTick,
        trigger_target,
        hook_id: policy.hook_id,
        hook_version_digest: policy.hook_version_digest,
        recurrence_policy: policy.recurrence_policy,
        claim_ttl_seconds: policy.claim_ttl_seconds,
        authoritative_revision: Some(plan.revision),
    })
}

fn recurring_plan_tick_policy(metadata: &Value) -> Option<RecurringPlanTickPolicy> {
    let trigger = metadata.get("eventTrigger")?;
    if trigger.get("kind")?.as_str()? != "recurring_plan_tick" {
        return None;
    }
    Some(RecurringPlanTickPolicy {
        hook_id: trigger
            .get("hookId")
            .and_then(Value::as_str)
            .map(str::to_string),
        hook_version_digest: trigger
            .get("hookVersionDigest")
            .and_then(Value::as_str)
            .map(str::to_string),
        recurrence_policy: trigger
            .get("recurrencePolicy")
            .and_then(Value::as_str)
            .map(str::to_string),
        claim_ttl_seconds: trigger
            .get("claimTtlSeconds")
            .and_then(Value::as_u64)
            .unwrap_or(DEFAULT_CLAIM_TTL_SECONDS),
    })
}

fn recurring_claim_metadata(candidate: &EventTriggerClaimLoopCandidate) -> Value {
    serde_json::json!({
        "eventTrigger": {
            "kind": "recurring_plan_tick",
            "planId": candidate.plan_id,
            "dueAt": candidate.due_at,
            "recurrencePolicy": candidate.recurrence_policy,
            "claimTtlSeconds": candidate.claim_ttl_seconds,
        }
    })
}

fn claim_owner(workspace_root: &std::path::Path) -> Result<EventExecutionOwner> {
    let canonical_root = workspace_root
        .canonicalize()
        .unwrap_or_else(|_| workspace_root.to_path_buf());
    let mut hasher = DefaultHasher::new();
    canonical_root.hash(&mut hasher);
    let worktree_fingerprint = format!("worktree:{:016x}", hasher.finish());
    Ok(EventExecutionOwner {
        principal: None,
        session_id: None,
        worktree_id: Some(worktree_fingerprint.clone()),
        service_instance_id: Some(format!(
            "service:{}:{worktree_fingerprint}",
            std::process::id()
        )),
    })
}

fn event_execution_is_active(record: &EventExecutionRecord) -> bool {
    matches!(
        record.status,
        EventExecutionStatus::Claimed | EventExecutionStatus::Running
    )
}
