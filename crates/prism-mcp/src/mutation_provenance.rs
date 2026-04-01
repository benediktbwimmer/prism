use prism_core::ValidationFeedbackRecord;
use prism_core::{AuthenticatedPrincipal, WorkspaceSession};
use prism_ir::{
    CredentialId, EventActor, EventExecutionContext, EventId, EventMeta, PrincipalActor, TaskId,
};
use prism_memory::MemoryEvent;
use prism_query::{ConceptEvent, ConceptRelationEvent, ContractEvent};

use crate::session_state::SessionState;

#[derive(Clone)]
pub(crate) struct MutationProvenance {
    actor: EventActor,
    execution_context: Option<EventExecutionContext>,
}

impl MutationProvenance {
    pub(crate) fn fallback(workspace: Option<&WorkspaceSession>, session: &SessionState) -> Self {
        Self {
            actor: EventActor::Agent,
            execution_context: execution_context(workspace, session, None),
        }
    }

    pub(crate) fn authenticated(
        workspace: Option<&WorkspaceSession>,
        session: &SessionState,
        authenticated: &AuthenticatedPrincipal,
    ) -> Self {
        Self {
            actor: EventActor::Principal(PrincipalActor {
                authority_id: authenticated.principal.authority_id.clone(),
                principal_id: authenticated.principal.principal_id.clone(),
                kind: Some(authenticated.principal.kind),
                name: Some(authenticated.principal.name.clone()),
            }),
            execution_context: execution_context(
                workspace,
                session,
                Some(&authenticated.credential.credential_id),
            ),
        }
    }

    pub(crate) fn event_meta(
        &self,
        id: EventId,
        task_id: Option<TaskId>,
        causation: Option<EventId>,
        ts: u64,
    ) -> EventMeta {
        EventMeta {
            id,
            ts,
            actor: self.actor.clone(),
            correlation: task_id,
            causation,
            execution_context: self.execution_context.clone(),
        }
    }

    pub(crate) fn stamp_memory_event(&self, event: &mut MemoryEvent) {
        event.actor = Some(self.actor.clone());
        event.execution_context = self.execution_context.clone();
    }

    pub(crate) fn stamp_validation_feedback_record(&self, record: &mut ValidationFeedbackRecord) {
        record.actor = Some(self.actor.clone());
        record.execution_context = self.execution_context.clone();
    }

    pub(crate) fn stamp_concept_event(&self, event: &mut ConceptEvent) {
        event.actor = Some(self.actor.clone());
        event.execution_context = self.execution_context.clone();
    }

    pub(crate) fn stamp_contract_event(&self, event: &mut ContractEvent) {
        event.actor = Some(self.actor.clone());
        event.execution_context = self.execution_context.clone();
    }

    pub(crate) fn stamp_concept_relation_event(&self, event: &mut ConceptRelationEvent) {
        event.actor = Some(self.actor.clone());
        event.execution_context = self.execution_context.clone();
    }
}

fn execution_context(
    workspace: Option<&WorkspaceSession>,
    session: &SessionState,
    credential_id: Option<&CredentialId>,
) -> Option<EventExecutionContext> {
    workspace
        .map(|workspace| {
            workspace.event_execution_context(Some(&session.session_id()), credential_id)
        })
        .or_else(|| {
            if credential_id.is_none() {
                Some(EventExecutionContext {
                    session_id: Some(session.session_id().0.to_string()),
                    ..EventExecutionContext::default()
                })
            } else {
                Some(EventExecutionContext {
                    session_id: Some(session.session_id().0.to_string()),
                    credential_id: credential_id.cloned(),
                    ..EventExecutionContext::default()
                })
            }
        })
}
