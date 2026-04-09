use prism_coordination::LeaseHeartbeatDueState;
use prism_ir::{CoordinationTaskId, LeaseRenewalMode};
use prism_query::Prism;

#[derive(Debug, Clone)]
pub(crate) struct TaskHeartbeatAdvice {
    pub(crate) task_id: CoordinationTaskId,
    pub(crate) due_state: LeaseHeartbeatDueState,
    pub(crate) renewal_mode: LeaseRenewalMode,
}

pub(crate) fn task_heartbeat_advice(
    prism: &Prism,
    task_id: &CoordinationTaskId,
    now: u64,
) -> Option<TaskHeartbeatAdvice> {
    let task = prism.coordination_task_v2_by_coordination_id(task_id)?;
    let plan = prism.coordination_plan_v2(&task.task.parent_plan_id)?;
    let due_state = prism.effective_canonical_task_heartbeat_due_state(&task.task, &plan.plan.policy, now);
    if !matches!(due_state, LeaseHeartbeatDueState::NotDue)
        && prism.canonical_task_has_active_local_assisted_lease(&task.task, now)
    {
        return None;
    }
    (!matches!(due_state, LeaseHeartbeatDueState::NotDue)).then_some(TaskHeartbeatAdvice {
        task_id: task_id.clone(),
        due_state,
        renewal_mode: plan.plan.policy.lease_renewal_mode,
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
        advice.task_id.0
    )
}
