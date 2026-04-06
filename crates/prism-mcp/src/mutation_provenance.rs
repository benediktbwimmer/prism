use prism_core::ValidationFeedbackRecord;
use prism_core::{AuthenticatedPrincipal, WorkspaceSession, WorktreeMutatorSlotRecord};
use prism_ir::{
    CoordinationTaskId, CredentialId, EventActor, EventExecutionContext, EventId, EventMeta,
    PrincipalActor, PrincipalAuthorityId, PrincipalId, TaskId, WorkContextKind,
    WorkContextSnapshot,
};
use prism_memory::MemoryEvent;
use prism_query::{ConceptEvent, ConceptRelationEvent, ContractEvent, Prism};
use std::sync::Arc;

use crate::request_envelope::current_request_id;
use crate::session_state::{SessionState, SessionTaskState, SessionWorkState};

#[derive(Clone)]
pub(crate) struct MutationProvenance {
    actor: EventActor,
    execution_context: Option<EventExecutionContext>,
    prism: Arc<Prism>,
    current_task: Option<SessionTaskState>,
    current_work: Option<SessionWorkState>,
}

impl MutationProvenance {
    pub(crate) fn fallback(
        workspace: Option<&WorkspaceSession>,
        session: &SessionState,
        prism: Arc<Prism>,
    ) -> Self {
        Self {
            actor: EventActor::Agent.canonical_identity_actor(),
            execution_context: execution_context(workspace, session, None),
            prism,
            current_task: session.effective_current_task_state(),
            current_work: session.current_work_state(),
        }
    }

    pub(crate) fn authenticated(
        workspace: Option<&WorkspaceSession>,
        session: &SessionState,
        prism: Arc<Prism>,
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
            prism,
            current_task: session.effective_current_task_state(),
            current_work: session.current_work_state(),
        }
    }

    pub(crate) fn worktree_executor(
        workspace: &WorkspaceSession,
        session: &SessionState,
        prism: Arc<Prism>,
        slot: &WorktreeMutatorSlotRecord,
    ) -> Self {
        Self {
            actor: EventActor::Principal(PrincipalActor {
                authority_id: PrincipalAuthorityId::new(slot.authority_id.clone()),
                principal_id: PrincipalId::new(slot.principal_id.clone()),
                kind: Some(slot.principal_kind),
                name: Some(slot.principal_name.clone()),
            }),
            execution_context: execution_context(Some(workspace), session, None),
            prism,
            current_task: session.effective_current_task_state(),
            current_work: session.current_work_state(),
        }
    }

    pub(crate) fn event_meta(
        &self,
        id: EventId,
        task_id: Option<TaskId>,
        causation: Option<EventId>,
        ts: u64,
    ) -> EventMeta {
        let mut execution_context = self.execution_context.clone();
        stamp_work_context(
            &mut execution_context,
            self.work_context_snapshot(task_id.as_ref().map(|task| task.0.as_str())),
        );
        EventMeta {
            id,
            ts,
            actor: self.actor.clone(),
            correlation: task_id,
            causation,
            execution_context,
        }
    }

    pub(crate) fn stamp_memory_event(&self, event: &mut MemoryEvent) {
        event.actor = Some(self.actor.clone());
        let mut execution_context = self.execution_context.clone();
        stamp_work_context(
            &mut execution_context,
            self.work_context_snapshot(event.task_id.as_deref()),
        );
        event.execution_context = execution_context;
    }

    pub(crate) fn stamp_validation_feedback_record(&self, record: &mut ValidationFeedbackRecord) {
        record.actor = Some(self.actor.clone());
        let mut execution_context = self.execution_context.clone();
        stamp_work_context(
            &mut execution_context,
            self.work_context_snapshot(record.task_id.as_deref()),
        );
        record.execution_context = execution_context;
    }

    pub(crate) fn stamp_concept_event(&self, event: &mut ConceptEvent) {
        event.actor = Some(self.actor.clone());
        let mut execution_context = self.execution_context.clone();
        stamp_work_context(
            &mut execution_context,
            self.work_context_snapshot(event.task_id.as_deref()),
        );
        event.execution_context = execution_context;
    }

    pub(crate) fn stamp_contract_event(&self, event: &mut ContractEvent) {
        event.actor = Some(self.actor.clone());
        let mut execution_context = self.execution_context.clone();
        stamp_work_context(
            &mut execution_context,
            self.work_context_snapshot(event.task_id.as_deref()),
        );
        event.execution_context = execution_context;
    }

    pub(crate) fn stamp_concept_relation_event(&self, event: &mut ConceptRelationEvent) {
        event.actor = Some(self.actor.clone());
        let mut execution_context = self.execution_context.clone();
        stamp_work_context(
            &mut execution_context,
            self.work_context_snapshot(event.task_id.as_deref()),
        );
        event.execution_context = execution_context;
    }

    fn work_context_snapshot(&self, task_id: Option<&str>) -> Option<WorkContextSnapshot> {
        let task_id = task_id?.trim();
        if task_id.is_empty() {
            return None;
        }

        let current_work = self
            .current_work
            .as_ref()
            .filter(|state| state.id.0.as_str() == task_id);
        if let Some(current_work) = current_work {
            let plan_title = current_work.plan_title.clone().or_else(|| {
                current_work.plan_id.as_ref().and_then(|plan_id| {
                    self.prism
                        .coordination_plan(&prism_ir::PlanId::new(plan_id.clone()))
                        .map(|plan| plan.title)
                })
            });
            return Some(WorkContextSnapshot {
                work_id: task_id.to_string(),
                kind: current_work.kind,
                title: current_work.title.clone(),
                summary: current_work.summary.clone(),
                parent_work_id: current_work
                    .parent_work_id
                    .as_ref()
                    .map(|id| id.0.to_string()),
                coordination_task_id: current_work.coordination_task_id.clone(),
                plan_id: current_work.plan_id.clone(),
                plan_title,
            });
        }

        let current_task = self
            .current_task
            .as_ref()
            .filter(|state| state.id.0.as_str() == task_id);
        let coordination_task_id = current_task
            .and_then(|state| state.coordination_task_id.clone())
            .or_else(|| {
                task_id
                    .starts_with("coord-task:")
                    .then(|| task_id.to_string())
            });
        let coordination_task = coordination_task_id
            .as_ref()
            .and_then(|coordination_task_id| {
                self.prism
                    .coordination_task(&CoordinationTaskId::new(coordination_task_id.clone()))
            });
        let plan = coordination_task
            .as_ref()
            .and_then(|task| self.prism.coordination_plan(&task.plan));
        let title = coordination_task
            .as_ref()
            .map(|task| task.title.clone())
            .or_else(|| {
                current_task
                    .and_then(|state| state.description.clone())
                    .filter(|description| {
                        let trimmed = description.trim();
                        !trimmed.is_empty() && trimmed != "session"
                    })
            })
            .unwrap_or_else(|| task_id.to_string());
        let kind = if coordination_task.is_some() {
            WorkContextKind::Coordination
        } else if current_task
            .and_then(|state| state.description.as_deref())
            .is_some_and(|description| {
                let trimmed = description.trim();
                !trimmed.is_empty() && trimmed != "session"
            })
        {
            WorkContextKind::AdHoc
        } else {
            WorkContextKind::Undeclared
        };

        Some(WorkContextSnapshot {
            work_id: task_id.to_string(),
            kind,
            title,
            summary: coordination_task.and_then(|task| task.summary.clone()),
            parent_work_id: None,
            coordination_task_id,
            plan_id: plan.as_ref().map(|plan| plan.id.0.to_string()),
            plan_title: plan.map(|plan| plan.title),
        })
    }
}

fn stamp_work_context(
    execution_context: &mut Option<EventExecutionContext>,
    work_context: Option<WorkContextSnapshot>,
) {
    let Some(work_context) = work_context else {
        return;
    };
    match execution_context.as_mut() {
        Some(context) => context.work_context = Some(work_context),
        None => {
            *execution_context = Some(EventExecutionContext {
                work_context: Some(work_context),
                ..EventExecutionContext::default()
            });
        }
    }
}

fn execution_context(
    workspace: Option<&WorkspaceSession>,
    session: &SessionState,
    credential_id: Option<&CredentialId>,
) -> Option<EventExecutionContext> {
    let request_id = current_request_id();
    workspace
        .map(|workspace| {
            workspace.event_execution_context(
                Some(&session.session_id()),
                request_id.clone(),
                credential_id,
            )
        })
        .or_else(|| {
            if credential_id.is_none() {
                Some(EventExecutionContext {
                    session_id: Some(session.session_id().0.to_string()),
                    request_id: request_id.clone(),
                    work_context: None,
                    ..EventExecutionContext::default()
                })
            } else {
                Some(EventExecutionContext {
                    session_id: Some(session.session_id().0.to_string()),
                    request_id,
                    credential_id: credential_id.cloned(),
                    work_context: None,
                    ..EventExecutionContext::default()
                })
            }
        })
}
