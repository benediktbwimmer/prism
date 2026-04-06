use prism_ir::{
    EventActor, EventMeta, ExecutorClass, PrincipalActor, PrincipalId, PrincipalKind,
    TaskExecutorPolicy,
};
use serde::{Deserialize, Serialize};
use serde_json::Value;

use crate::types::CoordinationTask;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ExecutorMismatchReason {
    ExecutorClassMismatch,
    TargetLabelMismatch,
    PrincipalNotAllowed,
}

impl ExecutorMismatchReason {
    pub const fn as_code(self) -> &'static str {
        match self {
            Self::ExecutorClassMismatch => "executor_class_mismatch",
            Self::TargetLabelMismatch => "target_label_mismatch",
            Self::PrincipalNotAllowed => "principal_not_allowed",
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct TaskExecutorCaller {
    pub executor_class: ExecutorClass,
    #[serde(default)]
    pub target_label: Option<String>,
    #[serde(default)]
    pub principal_id: Option<PrincipalId>,
}

impl TaskExecutorCaller {
    pub fn new(
        executor_class: ExecutorClass,
        target_label: Option<String>,
        principal_id: Option<PrincipalId>,
    ) -> Self {
        Self {
            executor_class,
            target_label,
            principal_id,
        }
    }

    pub fn from_event_meta(meta: &EventMeta) -> Self {
        let mut caller = Self::from_actor(&meta.actor);
        if meta
            .execution_context
            .as_ref()
            .and_then(|context| context.worktree_id.as_ref())
            .is_some()
            && caller.executor_class == ExecutorClass::Human
        {
            caller.executor_class = ExecutorClass::WorktreeExecutor;
        }
        caller
    }

    pub fn from_actor(actor: &EventActor) -> Self {
        match actor.clone().canonical_identity_actor() {
            EventActor::Principal(principal) => Self::from_principal(&principal),
            EventActor::User => Self::new(ExecutorClass::Human, None, None),
            EventActor::Agent => Self::new(ExecutorClass::WorktreeExecutor, None, None),
            EventActor::System | EventActor::CI | EventActor::GitAuthor { .. } => {
                Self::new(ExecutorClass::Service, None, None)
            }
        }
    }

    pub fn from_principal(principal: &PrincipalActor) -> Self {
        Self::new(
            executor_class_for_principal_kind(principal.kind),
            principal.name.clone(),
            Some(principal.principal_id.clone()),
        )
    }
}

pub fn task_executor_policy(task: &CoordinationTask) -> TaskExecutorPolicy {
    let mut policy = TaskExecutorPolicy::default();
    if task
        .metadata
        .get("executor")
        .and_then(Value::as_object)
        .is_some()
    {
        if let Ok(parsed) = serde_json::from_value::<TaskExecutorPolicy>(
            task.metadata
                .get("executor")
                .cloned()
                .unwrap_or(Value::Null),
        ) {
            policy = parsed;
        }
    }
    policy
}

pub fn executor_mismatch_reasons(
    task: &CoordinationTask,
    caller: &TaskExecutorCaller,
) -> Vec<ExecutorMismatchReason> {
    let policy = task_executor_policy(task);
    let mut reasons = Vec::new();
    if caller.executor_class != policy.executor_class {
        reasons.push(ExecutorMismatchReason::ExecutorClassMismatch);
    }
    if policy
        .target_label
        .as_ref()
        .is_some_and(|label| caller.target_label.as_ref() != Some(label))
    {
        reasons.push(ExecutorMismatchReason::TargetLabelMismatch);
    }
    if !policy.allowed_principals.is_empty()
        && caller
            .principal_id
            .as_ref()
            .is_none_or(|principal_id| !policy.allowed_principals.contains(principal_id))
    {
        reasons.push(ExecutorMismatchReason::PrincipalNotAllowed);
    }
    reasons
}

pub fn caller_matches_task_executor_policy(
    task: &CoordinationTask,
    caller: &TaskExecutorCaller,
) -> bool {
    executor_mismatch_reasons(task, caller).is_empty()
}

fn executor_class_for_principal_kind(kind: Option<PrincipalKind>) -> ExecutorClass {
    match kind.unwrap_or(PrincipalKind::Agent) {
        PrincipalKind::Human => ExecutorClass::Human,
        PrincipalKind::Service | PrincipalKind::System | PrincipalKind::Ci => {
            ExecutorClass::Service
        }
        PrincipalKind::Agent | PrincipalKind::External => ExecutorClass::WorktreeExecutor,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use prism_ir::{EventExecutionContext, EventId, PrincipalAuthorityId, SessionId, TaskId};

    #[test]
    fn worktree_bound_human_meta_routes_as_worktree_executor() {
        let meta = EventMeta {
            id: EventId::new("coordination:executor-routing"),
            ts: 7,
            actor: EventActor::Principal(PrincipalActor {
                authority_id: PrincipalAuthorityId::new("authority:test-human"),
                principal_id: PrincipalId::new("principal:test-human"),
                kind: Some(PrincipalKind::Human),
                name: Some("Test Human".to_string()),
            }),
            correlation: Some(TaskId::new("task:executor-routing")),
            causation: None,
            execution_context: Some(EventExecutionContext {
                worktree_id: Some("worktree:test".to_string()),
                session_id: Some(SessionId::new("session:test").0.to_string()),
                ..EventExecutionContext::default()
            }),
        };

        let caller = TaskExecutorCaller::from_event_meta(&meta);
        assert_eq!(caller.executor_class, ExecutorClass::WorktreeExecutor);
        assert_eq!(caller.principal_id, Some(PrincipalId::new("principal:test-human")));
        assert_eq!(caller.target_label.as_deref(), Some("Test Human"));
    }
}
