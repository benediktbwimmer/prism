use std::path::Path;

use prism_coordination::TaskExecutorCaller;
use prism_core::PrismPaths;
use prism_ir::{ExecutorClass, PrincipalId};

use crate::SessionState;

pub(crate) fn current_executor_caller(
    workspace_root: Option<&Path>,
    session: Option<&SessionState>,
) -> Option<TaskExecutorCaller> {
    if let Some(workspace_root) = workspace_root {
        if let Ok(paths) = PrismPaths::for_workspace_root(workspace_root) {
            if let Ok(Some(registration)) = paths.worktree_registration() {
                return Some(TaskExecutorCaller::new(
                    ExecutorClass::WorktreeExecutor,
                    Some(registration.agent_label),
                    Some(PrincipalId::new(registration.worktree_id)),
                ));
            }
        }
    }

    session
        .and_then(|session| session.current_agent())
        .map(|agent| {
            TaskExecutorCaller::new(
                ExecutorClass::WorktreeExecutor,
                Some(agent.0.to_string()),
                None,
            )
        })
}
