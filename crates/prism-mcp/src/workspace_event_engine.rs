use std::sync::Arc;

use anyhow::Result;
use prism_coordination::EventExecutionRecord;
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
    ) -> Result<Vec<EventExecutionRecord>> {
        Ok(self
            .authority_store_provider
            .open(&self.workspace_root)?
            .read_event_execution_records(request)?
            .value
            .unwrap_or_default())
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
    use prism_coordination::{EventExecutionOwner, EventExecutionRecord};
    use prism_core::{
        EventExecutionOwnerExpectation, EventExecutionTransitionKind,
        EventExecutionTransitionPreconditions, EventExecutionTransitionRequest,
        EventExecutionTransitionStatus,
    };
    use prism_ir::{
        EventExecutionId, EventExecutionStatus, EventTriggerKind, PlanId, PrincipalActor,
        PrincipalAuthorityId, PrincipalId, PrincipalKind, SessionId,
    };

    use crate::tests_support::{host_with_session, temp_workspace};

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
}
