use prism_coordination::{task_heartbeat_due_state, CoordinationTask, LeaseHeartbeatDueState};
use prism_ir::{CoordinationTaskId, LeaseRenewalMode};
use prism_query::Prism;

#[derive(Debug, Clone)]
pub(crate) struct TaskHeartbeatAdvice {
    pub(crate) task: CoordinationTask,
    pub(crate) due_state: LeaseHeartbeatDueState,
    pub(crate) renewal_mode: LeaseRenewalMode,
}

pub(crate) fn task_heartbeat_advice(
    prism: &Prism,
    task_id: &CoordinationTaskId,
    now: u64,
) -> Option<TaskHeartbeatAdvice> {
    let task = prism.coordination_task(task_id)?;
    let plan = prism.coordination_plan(&task.plan)?;
    let due_state = task_heartbeat_due_state(&task, &plan.policy, now);
    if !matches!(due_state, LeaseHeartbeatDueState::NotDue)
        && prism.task_has_active_local_assisted_lease(&task, now)
    {
        return None;
    }
    (!matches!(due_state, LeaseHeartbeatDueState::NotDue)).then_some(TaskHeartbeatAdvice {
        task,
        due_state,
        renewal_mode: plan.policy.lease_renewal_mode,
    })
}

pub(crate) fn task_heartbeat_next_action(advice: &TaskHeartbeatAdvice) -> String {
    let urgency = match advice.due_state {
        LeaseHeartbeatDueState::DueNow => "is due now",
        LeaseHeartbeatDueState::DueSoon => "is due soon",
        LeaseHeartbeatDueState::NotDue => return String::new(),
    };
    let mode_suffix = match advice.renewal_mode {
        LeaseRenewalMode::Strict => String::new(),
        LeaseRenewalMode::Assisted => {
            " This plan allows local assisted lease renewal, but that path is off by default, non-authoritative, and does not replace this authenticated heartbeat mutation.".to_string()
        }
    };
    format!(
        "Before any other task work, call prism_mutate with action `heartbeat_lease` and input `{{\"taskId\":\"{}\"}}`; this task lease {urgency}.{mode_suffix}",
        advice.task.id.0
    )
}
