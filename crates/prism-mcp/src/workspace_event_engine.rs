use std::sync::Arc;

use anyhow::Result;
use prism_core::{
    CoordinationAuthorityStoreProvider, EventExecutionRecordAuthorityQuery,
    EventExecutionTransitionRequest, EventExecutionTransitionResult,
};

use crate::host_mutations::WorkspaceMutationBroker;
use crate::read_broker::WorkspaceReadBroker;

#[derive(Clone)]
pub(crate) struct WorkspaceEventEngine {
    workspace_root: std::path::PathBuf,
    authority_store_provider: CoordinationAuthorityStoreProvider,
    _read_broker: Arc<WorkspaceReadBroker>,
    _mutation_broker: Arc<WorkspaceMutationBroker>,
}

impl WorkspaceEventEngine {
    pub(crate) fn new(
        workspace_root: std::path::PathBuf,
        authority_store_provider: CoordinationAuthorityStoreProvider,
        read_broker: Arc<WorkspaceReadBroker>,
        mutation_broker: Arc<WorkspaceMutationBroker>,
    ) -> Self {
        Self {
            workspace_root,
            authority_store_provider,
            _read_broker: read_broker,
            _mutation_broker: mutation_broker,
        }
    }

    pub(crate) fn read_event_execution_records(
        &self,
        request: EventExecutionRecordAuthorityQuery,
    ) -> Result<Vec<prism_coordination::EventExecutionRecord>> {
        Ok(self
            .authority_store_provider
            .open(&self.workspace_root)?
            .read_event_execution_records(request)?
            .value
            .unwrap_or_default())
    }

    pub(crate) fn workspace_root(&self) -> &std::path::Path {
        &self.workspace_root
    }

    pub(crate) fn read_broker(&self) -> &WorkspaceReadBroker {
        self._read_broker.as_ref()
    }

    pub(crate) fn apply_event_execution_transition(
        &self,
        request: EventExecutionTransitionRequest,
    ) -> Result<EventExecutionTransitionResult> {
        self.authority_store_provider
            .open(&self.workspace_root)?
            .apply_event_execution_transition(request)
    }
}

#[cfg(test)]
mod tests {
    use prism_coordination::{CoordinationSnapshot, EventExecutionOwner, EventExecutionRecord, Plan};
    use prism_core::{
        EventExecutionOwnerExpectation, EventExecutionTransitionKind,
        EventExecutionTransitionPreconditions, EventExecutionTransitionRequest,
        EventExecutionTransitionStatus,
    };
    use prism_ir::{
        EventExecutionId, EventExecutionStatus, EventTriggerKind, PlanId, PlanStatus,
        PrincipalActor,
        PrincipalAuthorityId, PrincipalId, PrincipalKind, SessionId,
    };
    use serde_json::json;

    use crate::workspace_event_engine_claim_loop::{
        EventTriggerClaimLoopRequest, EventTriggerClaimOutcome, EventTriggerClaimSkipReason,
    };
    use crate::tests_support::{
        host_with_session, index_workspace_session_with_shared_runtime, temp_workspace,
    };

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

    #[test]
    fn workspace_event_engine_routes_event_execution_transitions_through_authority_store() {
        let root = temp_workspace();
        let session = prism_core::index_workspace_session(&root).expect("workspace session");
        let host = host_with_session(session);
        let event_engine = host
            .workspace_event_engine()
            .cloned()
            .expect("workspace event engine");
        let record = event_execution_record("event-exec:mcp:1", 100);

        let claim = event_engine
            .apply_event_execution_transition(EventExecutionTransitionRequest {
                event_execution_id: record.id.clone(),
                preconditions: EventExecutionTransitionPreconditions {
                    require_missing: true,
                    ..EventExecutionTransitionPreconditions::default()
                },
                transition: EventExecutionTransitionKind::Claim {
                    record: record.clone(),
                },
            })
            .expect("claim transition should succeed");
        assert_eq!(claim.status, EventExecutionTransitionStatus::Applied);

        let started = event_engine
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
                    started_at: 110,
                    summary: Some("running".to_string()),
                },
            })
            .expect("start transition should succeed");
        assert_eq!(started.status, EventExecutionTransitionStatus::Applied);

        let stored = event_engine
            .read_event_execution_records(prism_core::EventExecutionRecordAuthorityQuery {
                consistency: prism_core::CoordinationReadConsistency::Strong,
                event_execution_id: Some(record.id.clone()),
                limit: None,
            })
            .expect("stored event execution record");
        assert_eq!(stored.len(), 1);
        assert_eq!(stored[0].status, EventExecutionStatus::Running);
        assert_eq!(stored[0].started_at, Some(110));
    }

    fn recurring_plan(id: &str, due_at: u64, revision: u64) -> Plan {
        Plan {
            id: PlanId::new(id),
            goal: "Recurring plan".to_string(),
            title: "Recurring plan".to_string(),
            status: PlanStatus::Active,
            policy: prism_coordination::CoordinationPolicy::default(),
            scope: prism_ir::PlanScope::Repo,
            kind: prism_ir::PlanKind::TaskExecution,
            revision,
            scheduling: prism_coordination::PlanScheduling {
                importance: 0,
                urgency: 0,
                manual_boost: 0,
                due_at: Some(due_at),
            },
            tags: Vec::new(),
            created_from: None,
            spec_refs: Vec::new(),
            metadata: json!({
                "eventTrigger": {
                    "kind": "recurring_plan_tick",
                    "recurrencePolicy": "daily",
                    "hookId": "hooks/recurring-plan",
                    "hookVersionDigest": "sha256:recurring",
                    "claimTtlSeconds": 45
                }
            }),
        }
    }

    #[test]
    fn workspace_event_engine_claims_due_recurring_plan_ticks_from_service_backed_state() {
        let root = temp_workspace();
        let session = index_workspace_session_with_shared_runtime(&root);
        session
            .mutate_coordination(|prism| {
                let mut snapshot = CoordinationSnapshot::default();
                snapshot.plans.push(recurring_plan("plan:recurring", 100, 7));
                prism.replace_coordination_snapshot(snapshot);
                Ok(())
            })
            .expect("coordination snapshot should persist");
        session
            .flush_materializations()
            .expect("coordination materialization should flush");
        let host = host_with_session(session);
        let event_engine = host
            .workspace_event_engine()
            .cloned()
            .expect("workspace event engine");

        let result = event_engine
            .claim_due_triggers(EventTriggerClaimLoopRequest {
                now: 100,
                limit: None,
            })
            .expect("claim loop should succeed");

        assert_eq!(result.outcomes.len(), 1);
        let EventTriggerClaimOutcome::Claimed { candidate, record } = &result.outcomes[0] else {
            panic!("expected claimed outcome");
        };
        assert_eq!(candidate.plan_id, "plan:recurring");
        assert_eq!(record.status, EventExecutionStatus::Claimed);
        assert_eq!(record.authoritative_revision, Some(7));
        assert_eq!(record.trigger_kind, EventTriggerKind::RecurringPlanTick);
        assert_eq!(record.trigger_target.as_ref().map(|target| target.id.as_str()), Some("plan:recurring"));
        assert_eq!(record.expires_at, Some(145));
        assert_eq!(record.metadata["eventTrigger"]["recurrencePolicy"], "daily");
    }

    #[test]
    fn workspace_event_engine_skips_due_recurring_plan_ticks_with_existing_execution() {
        let root = temp_workspace();
        let session = index_workspace_session_with_shared_runtime(&root);
        session
            .mutate_coordination(|prism| {
                let mut snapshot = CoordinationSnapshot::default();
                snapshot.plans.push(recurring_plan("plan:recurring", 100, 7));
                prism.replace_coordination_snapshot(snapshot);
                Ok(())
            })
            .expect("coordination snapshot should persist");
        session
            .flush_materializations()
            .expect("coordination materialization should flush");
        let host = host_with_session(session);
        let event_engine = host
            .workspace_event_engine()
            .cloned()
            .expect("workspace event engine");

        let first = event_engine
            .claim_due_triggers(EventTriggerClaimLoopRequest {
                now: 100,
                limit: None,
            })
            .expect("first claim loop should succeed");
        assert!(matches!(
            first.outcomes.first(),
            Some(EventTriggerClaimOutcome::Claimed { .. })
        ));

        let second = event_engine
            .claim_due_triggers(EventTriggerClaimLoopRequest {
                now: 100,
                limit: None,
            })
            .expect("second claim loop should succeed");

        assert_eq!(second.outcomes.len(), 1);
        let EventTriggerClaimOutcome::Skipped { candidate, reason } = &second.outcomes[0] else {
            panic!("expected skipped outcome");
        };
        assert_eq!(candidate.plan_id, "plan:recurring");
        assert_eq!(
            reason,
            &EventTriggerClaimSkipReason::ExistingExecution {
                status: EventExecutionStatus::Claimed,
            }
        );
    }
}
