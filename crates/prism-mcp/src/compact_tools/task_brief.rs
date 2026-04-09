use std::collections::HashSet;
use std::sync::Arc;
use std::time::Instant;

use anyhow::{anyhow, Result};
use prism_coordination::{
    CanonicalTaskRecord, LeaseState, TaskBlocker, TaskGitExecution,
};
use prism_ir::{
    AnchorRef, CoordinationTaskId, EdgeKind, GitExecutionStatus, GitIntegrationStatus, NodeId,
    NodeRef, NodeRefKind, TaskId, WorkspaceRevision,
};
use prism_js::{
    AgentOutcomeSummaryView, AgentTaskBlockerView, AgentTaskBriefResultView,
    CoordinationTaskLifecycleView, TaskLeaseHolderView,
};
use prism_memory::OutcomeRecallQuery;
use prism_query::Prism;
use serde_json::json;

use super::suggested_actions::{dedupe_suggested_actions, suggested_open_action};
use super::*;
use crate::{
    symbol_view_without_excerpt, task_heartbeat_advice, task_heartbeat_next_action,
    PrismTaskBriefArgs, TaskHeartbeatAdvice,
};

impl QueryHost {
    pub(crate) fn compact_task_brief(
        &self,
        session: Arc<SessionState>,
        args: PrismTaskBriefArgs,
    ) -> Result<AgentTaskBriefResultView> {
        let task_id = args.task_id.trim().to_string();
        let query_text = format!("prism_task_brief({task_id})");
        self.execute_compact_tool(
            Arc::clone(&session),
            "prism_task_brief",
            query_text,
            move |host, query_run| {
                let prism = host.current_prism();
                let now = crate::current_timestamp();
                let subject_started = Instant::now();
                let subject = resolve_task_brief_subject(prism.as_ref(), &task_id, now)?;
                query_run.record_phase(
                    "compact.taskBrief.subject",
                    &json!({
                        "coordinationTask": subject.coordination_task_id.is_some(),
                        "anchorCount": subject.anchors.len(),
                        "taskId": &subject.task_id,
                    }),
                    subject_started.elapsed(),
                    true,
                    None,
                );

                let plan_started = Instant::now();
                query_run.record_phase(
                    "compact.taskBrief.planContext",
                    &json!({
                        "dependencyCount": subject.dependencies.len(),
                        "dependentCount": subject.dependents.len(),
                    }),
                    plan_started.elapsed(),
                    true,
                    None,
                );

                let coordination_started = Instant::now();
                let claims = prism.claims(&subject.anchors, now);
                let conflicts = prism.conflicts(&subject.anchors, now);
                query_run.record_phase(
                    "compact.taskBrief.coordinationContext",
                    &json!({
                        "claimCount": claims.len(),
                        "conflictCount": conflicts.len(),
                    }),
                    coordination_started.elapsed(),
                    true,
                    None,
                );

                let task_id = TaskId::new(subject.task_id.clone());
                let replay_started = Instant::now();
                let recent_outcomes = load_task_brief_recent_outcomes(
                    host.workspace_session_ref(),
                    prism.as_ref(),
                    &task_id,
                    TASK_BRIEF_OUTCOME_LIMIT,
                )?;
                query_run.record_phase(
                    "compact.taskBrief.replay",
                    &json!({
                        "eventCount": recent_outcomes.len(),
                    }),
                    replay_started.elapsed(),
                    true,
                    None,
                );

                let next_reads_started = Instant::now();
                let next_reads = compact_task_next_reads(
                    session.as_ref(),
                    prism.as_ref(),
                    &subject.task_id,
                    subject.status,
                    subject.blockers.as_slice(),
                    &subject.anchors,
                    &subject.dependencies,
                    &subject.dependents,
                )?;
                query_run.record_phase(
                    "compact.taskBrief.nextReads",
                    &json!({
                        "nextReadCount": next_reads.len(),
                    }),
                    next_reads_started.elapsed(),
                    true,
                    None,
                );
                let heartbeat_advice = subject
                    .coordination_task_id
                    .as_ref()
                    .and_then(|task_id| task_heartbeat_advice(prism.as_ref(), task_id, now));
                let next_action = compact_task_brief_next_action(
                    subject.status,
                    heartbeat_advice.as_ref(),
                    subject.blockers.as_slice(),
                    next_reads.as_slice(),
                );
                let likely_validations = task_brief_likely_validations(
                    subject.likely_validations.as_slice(),
                    subject.fallback_validation_refs.as_slice(),
                );
                let risk_hint = subject.risk_hint;

                let mut result = AgentTaskBriefResultView {
                    task_id: subject.task_id.clone(),
                    title: clamp_string(&subject.title, TASK_BRIEF_TEXT_MAX_CHARS),
                    status: subject.status,
                    lifecycle: task_brief_lifecycle_view(subject.status, &subject.git_execution),
                    assignee: subject.assignee.clone(),
                    pending_handoff_to: subject.pending_handoff_to.clone(),
                    lease_state: subject.lease_state.clone(),
                    lease_holder: subject.lease_holder.clone(),
                    blockers: subject.blockers,
                    claim_holders: compact_claim_holders(claims.as_slice()),
                    conflict_summaries: conflicts
                        .iter()
                        .take(TASK_BRIEF_CONFLICT_LIMIT)
                        .map(|conflict| clamp_string(&conflict.summary, TASK_BRIEF_TEXT_MAX_CHARS))
                        .collect(),
                    recent_outcomes: recent_outcomes
                        .iter()
                        .map(|event| compact_outcome_summary_view(event, TASK_BRIEF_TEXT_MAX_CHARS))
                        .collect(),
                    likely_validations,
                    next_reads,
                    risk_hint,
                    truncated: false,
                    next_action: Some(next_action),
                    suggested_actions: Vec::new(),
                };
                if !task_brief_status_is_terminal(result.status) {
                    if let Some(next_read) = result.next_reads.first() {
                        result.suggested_actions =
                            dedupe_suggested_actions([suggested_open_action(
                                next_read.handle.clone(),
                                prism_js::AgentOpenMode::Focus,
                            )]);
                    }
                }
                Ok((budgeted_task_brief_result(result)?, Vec::new()))
            },
        )
    }
}

struct TaskBriefSubject {
    task_id: String,
    coordination_task_id: Option<CoordinationTaskId>,
    title: String,
    status: prism_ir::CoordinationTaskStatus,
    assignee: Option<String>,
    pending_handoff_to: Option<String>,
    git_execution: TaskGitExecution,
    lease_state: Option<String>,
    lease_holder: Option<TaskLeaseHolderView>,
    anchors: Vec<AnchorRef>,
    blockers: Vec<AgentTaskBlockerView>,
    dependencies: Vec<NodeRef>,
    dependents: Vec<NodeRef>,
    likely_validations: Vec<String>,
    fallback_validation_refs: Vec<String>,
    risk_hint: Option<String>,
}

fn resolve_task_brief_subject(prism: &Prism, task_id: &str, now: u64) -> Result<TaskBriefSubject> {
    let coordination_task_id = CoordinationTaskId::new(task_id.to_string());
    let canonical_task_id = TaskId::new(task_id.to_string());
    let evidence_status = prism.task_evidence_status(&coordination_task_id, now);
    let Some(task_v2) = prism.coordination_task_v2(&canonical_task_id) else {
        return Err(anyhow!("unknown coordination task `{task_id}`"));
    };

    debug_assert!(evidence_status.is_some());
    let raw_blockers = evidence_status
        .as_ref()
        .map(|status| status.blockers.clone())
        .unwrap_or_default();
    let blockers = raw_blockers
        .iter()
        .take(TASK_BRIEF_BLOCKER_LIMIT)
        .map(|blocker| AgentTaskBlockerView {
            kind: blocker.kind,
            summary: clamp_string(&blocker.summary, TASK_BRIEF_TEXT_MAX_CHARS),
        })
        .collect::<Vec<_>>();
    let likely_validations = compact_validation_labels_from_task_blockers(&raw_blockers);
    let risk_hint =
        compact_canonical_task_risk_hint(&task_v2.task, &raw_blockers, &prism.workspace_revision());
    let fallback_validation_refs = task_v2
        .task
        .validation_refs
        .iter()
        .map(|validation| validation.id.clone())
        .collect::<Vec<_>>();
    let lease_state = task_brief_lease_state_for_canonical(prism, &task_v2.task, now);
    let lease_holder = task_brief_authoritative_lease_holder_for_canonical(&task_v2.task);
    return Ok(TaskBriefSubject {
        task_id: task_v2.task.id.0.to_string(),
        coordination_task_id: Some(coordination_task_id),
        title: task_v2.task.title,
        status: task_brief_status_from_canonical(task_v2.status),
        assignee: task_v2.task.assignee.map(|agent| agent.0.to_string()),
        pending_handoff_to: task_v2
            .task
            .pending_handoff_to
            .map(|agent| agent.0.to_string()),
        git_execution: task_v2.task.git_execution.clone(),
        lease_state,
        lease_holder,
        anchors: task_v2.task.anchors,
        blockers,
        dependencies: task_v2.dependencies,
        dependents: task_v2.dependents,
        likely_validations,
        fallback_validation_refs,
        risk_hint,
    });
}

fn task_brief_status_from_canonical(
    status: prism_ir::EffectiveTaskStatus,
) -> prism_ir::CoordinationTaskStatus {
    match status {
        prism_ir::EffectiveTaskStatus::Pending => prism_ir::CoordinationTaskStatus::Ready,
        prism_ir::EffectiveTaskStatus::Active => prism_ir::CoordinationTaskStatus::InProgress,
        prism_ir::EffectiveTaskStatus::Blocked
        | prism_ir::EffectiveTaskStatus::BrokenDependency
        | prism_ir::EffectiveTaskStatus::Failed => prism_ir::CoordinationTaskStatus::Blocked,
        prism_ir::EffectiveTaskStatus::Completed => prism_ir::CoordinationTaskStatus::Completed,
        prism_ir::EffectiveTaskStatus::Abandoned => prism_ir::CoordinationTaskStatus::Abandoned,
    }
}

fn task_brief_lease_state_for_canonical(
    prism: &Prism,
    task: &prism_coordination::CanonicalTaskRecord,
    now: u64,
) -> Option<String> {
    match prism.effective_canonical_task_lease_state(task, now) {
        LeaseState::Active => Some("active".to_string()),
        LeaseState::Stale => Some("stale".to_string()),
        LeaseState::Expired => Some("expired".to_string()),
        LeaseState::Unleased => None,
    }
}

fn task_brief_authoritative_lease_holder_for_canonical(
    task: &prism_coordination::CanonicalTaskRecord,
) -> Option<TaskLeaseHolderView> {
    let mut holder = task
        .lease_holder
        .clone()
        .unwrap_or(prism_coordination::LeaseHolder {
            principal: None,
            session_id: None,
            worktree_id: None,
            agent_id: None,
        });
    if holder.session_id.is_none() {
        holder.session_id = task.session.clone();
    }
    if holder.worktree_id.is_none() {
        holder.worktree_id = task.worktree_id.clone();
    }
    if holder.agent_id.is_none() {
        holder.agent_id = task.assignee.clone();
    }
    let view = TaskLeaseHolderView {
        principal: holder.principal.map(|principal| {
            principal
                .name
                .clone()
                .unwrap_or_else(|| principal.scoped_id())
        }),
        session_id: holder.session_id.map(|session| session.0.to_string()),
        worktree_id: holder.worktree_id,
        agent_id: holder.agent_id.map(|agent| agent.0.to_string()),
    };
    (view.principal.is_some()
        || view.session_id.is_some()
        || view.worktree_id.is_some()
        || view.agent_id.is_some())
    .then_some(view)
}

fn push_unique_string(values: &mut Vec<String>, value: String) {
    if !values.iter().any(|existing| existing == &value) {
        values.push(value);
    }
}

fn compact_claim_holders(claims: &[prism_coordination::WorkClaim]) -> Vec<String> {
    let mut holders = claims
        .iter()
        .map(|claim| claim.holder.0.to_string())
        .collect::<Vec<_>>();
    holders.sort();
    holders.dedup();
    holders.truncate(TASK_BRIEF_CLAIM_HOLDER_LIMIT);
    holders
}

fn load_task_brief_recent_outcomes(
    workspace: Option<&prism_core::WorkspaceSession>,
    prism: &Prism,
    task_id: &TaskId,
    limit: usize,
) -> Result<Vec<prism_memory::OutcomeEvent>> {
    let query = OutcomeRecallQuery {
        anchors: Vec::new(),
        task: Some(task_id.clone()),
        kinds: None,
        result: None,
        actor: None,
        since: None,
        limit,
    };
    if let Some(workspace) = workspace {
        return workspace
            .load_outcomes(&query)
            .or_else(|_| Ok(prism.query_outcomes(&query)));
    }
    Ok(prism.query_outcomes(&query))
}

fn compact_task_next_reads(
    session: &SessionState,
    prism: &Prism,
    current_task_id: &str,
    current_status: prism_ir::CoordinationTaskStatus,
    blockers: &[AgentTaskBlockerView],
    anchors: &[AnchorRef],
    dependencies: &[NodeRef],
    dependents: &[NodeRef],
) -> Result<Vec<AgentTargetHandleView>> {
    let seed_nodes = anchors
        .iter()
        .filter_map(|anchor| match anchor {
            AnchorRef::Node(node) => Some(node.clone()),
            _ => None,
        })
        .collect::<Vec<_>>();
    let mut seen = HashSet::<NodeId>::new();
    let mut candidates = Vec::<(NodeId, String)>::new();

    if task_brief_should_follow_task_neighbors(current_status, blockers) {
        for (node_refs, relation_label) in [(dependencies, "Dependency"), (dependents, "Dependent")]
        {
            for node_ref in node_refs {
                let Some(related_task) =
                    compact_task_related_task(prism, current_task_id, node_ref)
                else {
                    continue;
                };
                for node_id in task_anchor_nodes(&related_task) {
                    if seed_nodes.iter().any(|seed| seed == &node_id)
                        || !seen.insert(node_id.clone())
                    {
                        continue;
                    }
                    candidates.push((
                        node_id,
                        format!("{relation_label} task `{}`.", related_task.title),
                    ));
                    if candidates.len() >= TASK_BRIEF_NEXT_READ_LIMIT.saturating_mul(4) {
                        break;
                    }
                }
            }
            if candidates.len() >= TASK_BRIEF_NEXT_READ_LIMIT.saturating_mul(4) {
                break;
            }
        }
    }

    for node in seed_nodes.iter().take(4) {
        for related in compact_task_direct_neighbors(prism, node)? {
            if seed_nodes.iter().any(|seed| seed == &related) || !seen.insert(related.clone()) {
                continue;
            }
            candidates.push((
                related,
                "Direct semantic neighbor of a task anchor.".to_string(),
            ));
            if candidates.len() >= TASK_BRIEF_NEXT_READ_LIMIT.saturating_mul(4) {
                break;
            }
        }
    }

    let mut next_reads = Vec::new();
    for (node_id, why) in candidates {
        let symbol = match symbol_for(prism, &node_id) {
            Ok(symbol) => symbol,
            Err(_) => continue,
        };
        let symbol = symbol_view_without_excerpt(prism, &symbol)?;
        next_reads.push(compact_target_view(session, &symbol, None, Some(why)));
        if next_reads.len() >= TASK_BRIEF_NEXT_READ_LIMIT {
            break;
        }
    }
    Ok(next_reads)
}

fn compact_task_direct_neighbors(prism: &Prism, target: &NodeId) -> Result<Vec<NodeId>> {
    let symbol = symbol_for(prism, target)?;
    let relations = symbol.relations();
    let mut neighbors = Vec::<NodeId>::new();

    for node_id in prism.spec_for(target) {
        push_unique_node(&mut neighbors, node_id);
    }
    for node_id in prism.implementation_for(target) {
        push_unique_node(&mut neighbors, node_id);
    }
    for node_id in relations.outgoing_related {
        push_unique_node(&mut neighbors, node_id);
    }
    for node_id in relations.incoming_related {
        push_unique_node(&mut neighbors, node_id);
    }
    for node_id in relations.outgoing_validates {
        push_unique_node(&mut neighbors, node_id);
    }
    for node_id in relations.incoming_validates {
        push_unique_node(&mut neighbors, node_id);
    }
    for node_id in relations.outgoing_specifies {
        push_unique_node(&mut neighbors, node_id);
    }
    for node_id in relations.incoming_specifies {
        push_unique_node(&mut neighbors, node_id);
    }
    for kind in [
        EdgeKind::Calls,
        EdgeKind::References,
        EdgeKind::Imports,
        EdgeKind::Implements,
    ] {
        for edge in prism.graph().edges_from(target, Some(kind)) {
            push_unique_node(&mut neighbors, edge.target.clone());
        }
        for edge in prism.graph().edges_to(target, Some(kind)) {
            push_unique_node(&mut neighbors, edge.source.clone());
        }
    }

    Ok(neighbors)
}

fn push_unique_node(values: &mut Vec<NodeId>, value: NodeId) {
    if !values.iter().any(|existing| existing == &value) {
        values.push(value);
    }
}

fn task_brief_likely_validations(
    subject_validations: &[String],
    fallback_validation_refs: &[String],
) -> Vec<String> {
    let mut validations = subject_validations.to_vec();
    if validations.is_empty() {
        validations = fallback_validation_refs.to_vec();
    }
    validations.truncate(TASK_BRIEF_VALIDATION_LIMIT);
    validations
}

fn compact_validation_labels_from_task_blockers(blockers: &[TaskBlocker]) -> Vec<String> {
    let mut labels = Vec::new();
    for blocker in blockers {
        for label in &blocker.validation_checks {
            push_unique_string(&mut labels, label.clone());
            if labels.len() >= TASK_BRIEF_VALIDATION_LIMIT {
                return labels;
            }
        }
    }
    labels
}

fn compact_canonical_task_risk_hint(
    task: &CanonicalTaskRecord,
    blockers: &[TaskBlocker],
    workspace_revision: &WorkspaceRevision,
) -> Option<String> {
    if let Some(blocker) = blockers.iter().find(|blocker| {
        matches!(
            blocker.kind,
            prism_coordination::BlockerKind::ReviewRequired
                | prism_coordination::BlockerKind::RiskReviewRequired
                | prism_coordination::BlockerKind::ValidationRequired
                | prism_coordination::BlockerKind::ArtifactStale
        )
    }) {
        return Some(clamp_string(&blocker.summary, TASK_BRIEF_TEXT_MAX_CHARS));
    }
    if canonical_task_is_workspace_bound(task)
        && task.base_revision.graph_version < workspace_revision.graph_version
    {
        return Some("Task base revision is stale against the current graph.".to_string());
    }
    None
}

fn canonical_task_is_workspace_bound(task: &CanonicalTaskRecord) -> bool {
    !task.anchors.is_empty()
        || task
            .acceptance
            .iter()
            .any(|criterion| !criterion.anchors.is_empty())
}

fn compact_outcome_summary_view(
    event: &prism_memory::OutcomeEvent,
    max_chars: usize,
) -> AgentOutcomeSummaryView {
    AgentOutcomeSummaryView {
        ts: event.meta.ts,
        kind: super::expand::enum_label(&event.kind),
        result: super::expand::enum_label(&event.result),
        summary: clamp_string(&event.summary, max_chars),
    }
}

fn compact_task_brief_next_action(
    status: prism_ir::CoordinationTaskStatus,
    heartbeat_advice: Option<&TaskHeartbeatAdvice>,
    blockers: &[AgentTaskBlockerView],
    next_reads: &[AgentTargetHandleView],
) -> String {
    if let Some(advice) = heartbeat_advice {
        return task_heartbeat_next_action(advice);
    }
    if task_brief_status_is_terminal(status) {
        return match status {
            prism_ir::CoordinationTaskStatus::Completed => {
                if next_reads.is_empty() {
                    "Task is completed. Inspect recent outcomes or prism_query if you need follow-up context.".to_string()
                } else {
                    "Task is completed. Inspect recent outcomes or open a nextRead only if you need follow-up context.".to_string()
                }
            }
            prism_ir::CoordinationTaskStatus::Abandoned => {
                "Task is abandoned. Inspect recent outcomes or prism_query if you need historical context.".to_string()
            }
            _ => unreachable!("terminal task status must be completed or abandoned"),
        };
    }
    if blockers
        .iter()
        .any(|blocker| blocker.kind == prism_coordination::BlockerKind::StaleRevision)
    {
        return "Refresh this task against the current workspace revision, then rerun prism_task_brief or prism.blockers(taskId).".to_string();
    }
    if !blockers.is_empty() {
        return "Inspect the current task blockers before switching nodes; use prism.blockers(taskId) or prism_query for full coordination detail.".to_string();
    }
    if !next_reads.is_empty() {
        return "Use prism_open on a nextRead to work this task, or prism_query for full coordination detail.".to_string();
    }
    "Inspect recent outcomes, validations, or prism_query for full coordination detail.".to_string()
}

fn task_brief_status_is_terminal(status: prism_ir::CoordinationTaskStatus) -> bool {
    matches!(
        status,
        prism_ir::CoordinationTaskStatus::Completed | prism_ir::CoordinationTaskStatus::Abandoned
    )
}

fn task_brief_lifecycle_view(
    status: prism_ir::CoordinationTaskStatus,
    git_execution: &TaskGitExecution,
) -> CoordinationTaskLifecycleView {
    let integration_status = git_execution.integration_status;
    let execution_status = git_execution.status;
    CoordinationTaskLifecycleView {
        completed: status == prism_ir::CoordinationTaskStatus::Completed,
        published_to_branch: matches!(
            integration_status,
            GitIntegrationStatus::PublishedToBranch
                | GitIntegrationStatus::IntegrationPending
                | GitIntegrationStatus::IntegrationInProgress
                | GitIntegrationStatus::IntegratedToTarget
        ),
        coordination_published: execution_status == GitExecutionStatus::CoordinationPublished,
        integrated_to_target: integration_status == GitIntegrationStatus::IntegratedToTarget,
    }
}

fn task_brief_should_follow_task_neighbors(
    status: prism_ir::CoordinationTaskStatus,
    blockers: &[AgentTaskBlockerView],
) -> bool {
    !task_brief_status_is_terminal(status)
        && (!blockers.is_empty()
            || matches!(
                status,
                prism_ir::CoordinationTaskStatus::Blocked
                    | prism_ir::CoordinationTaskStatus::InReview
                    | prism_ir::CoordinationTaskStatus::Validating
            ))
}

fn compact_task_related_task(
    prism: &Prism,
    current_task_id: &str,
    node_ref: &NodeRef,
) -> Option<CanonicalTaskRecord> {
    if node_ref.kind != NodeRefKind::Task || node_ref.id == current_task_id {
        return None;
    }
    prism
        .coordination_task_v2(&TaskId::new(node_ref.id.clone()))
        .map(|task| task.task)
}

fn task_anchor_nodes(task: &CanonicalTaskRecord) -> Vec<NodeId> {
    let anchors = if task.bindings.anchors.is_empty() {
        &task.anchors
    } else {
        &task.bindings.anchors
    };
    anchors
        .iter()
        .filter_map(|anchor| match anchor {
            AnchorRef::Node(node_id) => Some(node_id.clone()),
            _ => None,
        })
        .collect()
}

fn budgeted_task_brief_result(
    mut result: AgentTaskBriefResultView,
) -> Result<AgentTaskBriefResultView> {
    while task_brief_json_bytes(&result)? > TASK_BRIEF_MAX_JSON_BYTES {
        result.truncated = true;
        if strip_file_paths(&mut result.next_reads) {
            continue;
        }
        if !result.suggested_actions.is_empty() {
            result.suggested_actions.clear();
            continue;
        }
        if result.next_action.take().is_some() {
            continue;
        }
        if result.next_reads.pop().is_some() {
            continue;
        }
        if result.conflict_summaries.pop().is_some() {
            continue;
        }
        if result.claim_holders.pop().is_some() {
            continue;
        }
        if result.recent_outcomes.pop().is_some() {
            continue;
        }
        if result.blockers.pop().is_some() {
            continue;
        }
        if result.likely_validations.pop().is_some() {
            continue;
        }
        if result.risk_hint.take().is_some() {
            continue;
        }
        if result.next_action.take().is_some() {
            continue;
        }
        break;
    }
    Ok(result)
}

fn task_brief_json_bytes(result: &AgentTaskBriefResultView) -> Result<usize> {
    Ok(serde_json::to_vec(result)?.len())
}

#[cfg(test)]
mod tests {
    use prism_ir::{CoordinationTaskStatus, NodeKind};

    use super::*;

    #[test]
    fn task_brief_budget_trims_optional_context_to_fit() {
        let result = AgentTaskBriefResultView {
            task_id: "coord-task:1".to_string(),
            title: "compact task brief".to_string(),
            status: CoordinationTaskStatus::Ready,
            lifecycle: CoordinationTaskLifecycleView {
                completed: false,
                published_to_branch: false,
                coordination_published: false,
                integrated_to_target: false,
            },
            assignee: Some("agent-a".to_string()),
            pending_handoff_to: Some("agent-b".to_string()),
            lease_state: Some("active".to_string()),
            lease_holder: Some(TaskLeaseHolderView {
                principal: Some("principal:local:agent-a".to_string()),
                session_id: Some("session:a".to_string()),
                worktree_id: Some("worktree:a".to_string()),
                agent_id: Some("agent-a".to_string()),
            }),
            blockers: (0..4)
                .map(|index| AgentTaskBlockerView {
                    kind: prism_coordination::BlockerKind::ValidationRequired,
                    summary: format!("blocker summary {index} with extra compact task text"),
                })
                .collect(),
            claim_holders: vec![
                "agent-a".to_string(),
                "agent-b".to_string(),
                "agent-c".to_string(),
            ],
            conflict_summaries: vec![
                "conflict summary one with extra compact task text".to_string(),
                "conflict summary two with extra compact task text".to_string(),
            ],
            recent_outcomes: (0..4)
                .map(|index| AgentOutcomeSummaryView {
                    ts: index,
                    kind: "failure_observed".to_string(),
                    result: "failure".to_string(),
                    summary: format!("outcome summary {index} with extra compact task text"),
                })
                .collect(),
            likely_validations: vec![
                "cargo test --lib".to_string(),
                "cargo test --test memory".to_string(),
                "cargo check".to_string(),
                "cargo fmt --check".to_string(),
            ],
            next_reads: vec![
                AgentTargetHandleView {
                    handle: "handle:1".to_string(),
                    handle_category: prism_js::AgentHandleCategoryView::Symbol,
                    kind: NodeKind::Function,
                    path: "demo::one".to_string(),
                    name: "one".to_string(),
                    why_short: "next read one".to_string(),
                    why_not_top: None,
                    confidence_label: None,
                    file_path: Some(
                        "src/really/deeply/nested/module/with/a/very/long/path.rs".to_string(),
                    ),
                },
                AgentTargetHandleView {
                    handle: "handle:2".to_string(),
                    handle_category: prism_js::AgentHandleCategoryView::Symbol,
                    kind: NodeKind::Function,
                    path: "demo::two".to_string(),
                    name: "two".to_string(),
                    why_short: "next read two".to_string(),
                    why_not_top: None,
                    confidence_label: None,
                    file_path: Some(
                        "src/really/deeply/nested/module/with/a/very/long/other_path.rs"
                            .to_string(),
                    ),
                },
            ],
            risk_hint: Some("This risk hint is intentionally verbose for budget trimming.".into()),
            truncated: false,
            next_action: Some(
                "Use prism_open on a nextRead, or prism_query for full coordination detail."
                    .to_string(),
            ),
            suggested_actions: vec![suggested_open_action(
                "handle:1",
                prism_js::AgentOpenMode::Focus,
            )],
        };

        assert!(task_brief_json_bytes(&result).expect("json bytes") > TASK_BRIEF_MAX_JSON_BYTES);
        let result = budgeted_task_brief_result(result).expect("budgeted task brief");
        assert!(task_brief_json_bytes(&result).expect("json bytes") <= TASK_BRIEF_MAX_JSON_BYTES);
        assert!(result.truncated);
    }
}
