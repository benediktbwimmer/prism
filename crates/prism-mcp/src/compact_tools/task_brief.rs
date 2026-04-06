use std::collections::HashSet;
use std::sync::Arc;
use std::time::Instant;

use anyhow::{anyhow, Result};
use prism_coordination::{CoordinationTask, TaskBlocker};
use prism_ir::{
    AnchorRef, CoordinationTaskId, EdgeKind, GitExecutionStatus, GitIntegrationStatus, NodeId,
    PlanExecutionOverlay, PlanGraph, PlanNode, PlanNodeBlocker, PlanNodeBlockerKind, PlanNodeId,
    PlanNodeStatus, TaskId, WorkspaceRevision,
};
use prism_js::{
    AgentOutcomeSummaryView, AgentTaskBlockerView, AgentTaskBriefResultView,
    CoordinationTaskLifecycleView,
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
                let plan_graph = prism.plan_graph(&subject.plan_id);
                let plan_execution = prism.plan_execution(&subject.plan_id);
                let current_plan_node = plan_graph
                    .as_ref()
                    .and_then(|graph| graph.nodes.iter().find(|node| node.id == subject.node_id));
                query_run.record_phase(
                    "compact.taskBrief.planContext",
                    &json!({
                        "executionCount": plan_execution.len(),
                        "hasPlanGraph": plan_graph.is_some(),
                        "hasCurrentNode": current_plan_node.is_some(),
                    }),
                    plan_started.elapsed(),
                    true,
                    None,
                );

                let coordination_started = Instant::now();
                let task_execution = plan_execution
                    .iter()
                    .find(|overlay| overlay.node_id == subject.node_id);
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
                    host.features.cognition_layer_enabled(),
                    session.as_ref(),
                    prism.as_ref(),
                    &subject.node_id,
                    subject.status,
                    subject.blockers.as_slice(),
                    &subject.anchors,
                    plan_graph.as_ref(),
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
                    host.features.cognition_layer_enabled(),
                    subject.status,
                    heartbeat_advice.as_ref(),
                    subject.blockers.as_slice(),
                    next_reads.as_slice(),
                );
                let likely_validations = task_brief_likely_validations(
                    current_plan_node,
                    subject.likely_validations.as_slice(),
                    subject.fallback_validation_refs.as_slice(),
                );
                let risk_hint = subject
                    .risk_hint
                    .or_else(|| compact_native_plan_risk_hint(&subject.plan_blockers));

                let mut result = AgentTaskBriefResultView {
                    task_id: subject.task_id.clone(),
                    title: clamp_string(&subject.title, TASK_BRIEF_TEXT_MAX_CHARS),
                    status: subject.status,
                    lifecycle: task_brief_lifecycle_view(subject.status, task_execution),
                    assignee: subject.assignee.clone(),
                    pending_handoff_to: task_execution
                        .and_then(|overlay| overlay.pending_handoff_to.clone())
                        .or(subject.pending_handoff_to.clone())
                        .map(|agent| agent.0.to_string()),
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
    node_id: PlanNodeId,
    plan_id: prism_ir::PlanId,
    title: String,
    status: prism_ir::CoordinationTaskStatus,
    assignee: Option<String>,
    pending_handoff_to: Option<prism_ir::AgentId>,
    anchors: Vec<AnchorRef>,
    blockers: Vec<AgentTaskBlockerView>,
    plan_blockers: Vec<PlanNodeBlocker>,
    likely_validations: Vec<String>,
    fallback_validation_refs: Vec<String>,
    risk_hint: Option<String>,
}

fn resolve_task_brief_subject(prism: &Prism, task_id: &str, now: u64) -> Result<TaskBriefSubject> {
    let coordination_task_id = CoordinationTaskId::new(task_id.to_string());
    if let Some(task) = prism.coordination_task(&coordination_task_id) {
        let raw_blockers = prism.base_blockers(&coordination_task_id, now);
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
            compact_coordination_task_risk_hint(&task, &raw_blockers, &prism.workspace_revision());
        let fallback_validation_refs = coordination_task_validation_refs(&task);
        return Ok(TaskBriefSubject {
            task_id: task.id.0.to_string(),
            coordination_task_id: Some(task.id.clone()),
            node_id: PlanNodeId::new(task.id.0.to_string()),
            plan_id: task.plan.clone(),
            title: task.title,
            status: task.status,
            assignee: task.assignee.map(|agent| agent.0.to_string()),
            pending_handoff_to: task.pending_handoff_to,
            anchors: task.anchors,
            blockers,
            plan_blockers: Vec::new(),
            likely_validations,
            fallback_validation_refs,
            risk_hint,
        });
    }

    let Some((plan_id, node)) = resolve_native_plan_node(prism, task_id) else {
        return Err(anyhow!("unknown coordination task `{task_id}`"));
    };
    let plan_blockers = prism.plan_node_blockers(&plan_id, &node.id);
    let blockers = plan_blockers
        .iter()
        .take(TASK_BRIEF_BLOCKER_LIMIT)
        .map(plan_node_blocker_view)
        .collect::<Vec<_>>();
    let likely_validations = native_plan_likely_validations(&node, &plan_blockers);
    Ok(TaskBriefSubject {
        task_id: node.id.0.to_string(),
        coordination_task_id: None,
        node_id: node.id.clone(),
        plan_id,
        title: node.title,
        status: coordination_task_status_for_plan_node(node.status),
        assignee: node.assignee.as_ref().map(|agent| agent.0.to_string()),
        pending_handoff_to: None,
        anchors: node.bindings.anchors,
        blockers,
        plan_blockers,
        likely_validations,
        fallback_validation_refs: Vec::new(),
        risk_hint: None,
    })
}

fn resolve_native_plan_node(prism: &Prism, task_id: &str) -> Option<(prism_ir::PlanId, PlanNode)> {
    prism.plan_graphs().into_iter().find_map(|graph| {
        graph
            .nodes
            .into_iter()
            .find_map(|node| (node.id.0 == task_id).then(|| (graph.id.clone(), node)))
    })
}

fn plan_node_blocker_view(blocker: &PlanNodeBlocker) -> AgentTaskBlockerView {
    AgentTaskBlockerView {
        kind: blocker_kind_for_plan_node(blocker.kind),
        summary: clamp_string(&blocker.summary, TASK_BRIEF_TEXT_MAX_CHARS),
    }
}

fn blocker_kind_for_plan_node(kind: PlanNodeBlockerKind) -> prism_coordination::BlockerKind {
    match kind {
        PlanNodeBlockerKind::Dependency
        | PlanNodeBlockerKind::BlockingNode
        | PlanNodeBlockerKind::ChildIncomplete
        | PlanNodeBlockerKind::Handoff => prism_coordination::BlockerKind::Dependency,
        PlanNodeBlockerKind::ClaimConflict => prism_coordination::BlockerKind::ClaimConflict,
        PlanNodeBlockerKind::ReviewRequired => prism_coordination::BlockerKind::ReviewRequired,
        PlanNodeBlockerKind::RiskReviewRequired => {
            prism_coordination::BlockerKind::RiskReviewRequired
        }
        PlanNodeBlockerKind::ValidationGate | PlanNodeBlockerKind::ValidationRequired => {
            prism_coordination::BlockerKind::ValidationRequired
        }
        PlanNodeBlockerKind::StaleRevision => prism_coordination::BlockerKind::StaleRevision,
        PlanNodeBlockerKind::ArtifactStale => prism_coordination::BlockerKind::ArtifactStale,
    }
}

fn coordination_task_status_for_plan_node(
    status: PlanNodeStatus,
) -> prism_ir::CoordinationTaskStatus {
    match status {
        PlanNodeStatus::Proposed => prism_ir::CoordinationTaskStatus::Proposed,
        PlanNodeStatus::Ready => prism_ir::CoordinationTaskStatus::Ready,
        PlanNodeStatus::InProgress => prism_ir::CoordinationTaskStatus::InProgress,
        PlanNodeStatus::Blocked | PlanNodeStatus::Waiting => {
            prism_ir::CoordinationTaskStatus::Blocked
        }
        PlanNodeStatus::InReview => prism_ir::CoordinationTaskStatus::InReview,
        PlanNodeStatus::Validating => prism_ir::CoordinationTaskStatus::Validating,
        PlanNodeStatus::Completed => prism_ir::CoordinationTaskStatus::Completed,
        PlanNodeStatus::Abandoned => prism_ir::CoordinationTaskStatus::Abandoned,
    }
}

fn native_plan_likely_validations(node: &PlanNode, blockers: &[PlanNodeBlocker]) -> Vec<String> {
    let mut checks = Vec::new();
    for validation in &node.validation_refs {
        push_unique_string(&mut checks, validation.id.clone());
    }
    for criterion in &node.acceptance {
        for validation in &criterion.required_checks {
            push_unique_string(&mut checks, validation.id.clone());
        }
    }
    for blocker in blockers {
        if matches!(
            blocker.kind,
            PlanNodeBlockerKind::ValidationGate | PlanNodeBlockerKind::ValidationRequired
        ) {
            for validation in &blocker.validation_checks {
                push_unique_string(&mut checks, validation.clone());
            }
        }
    }
    checks.truncate(TASK_BRIEF_VALIDATION_LIMIT);
    checks
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
    cognition_enabled: bool,
    session: &SessionState,
    prism: &Prism,
    current_node_id: &PlanNodeId,
    current_status: prism_ir::CoordinationTaskStatus,
    blockers: &[AgentTaskBlockerView],
    anchors: &[AnchorRef],
    plan_graph: Option<&PlanGraph>,
) -> Result<Vec<AgentTargetHandleView>> {
    if !cognition_enabled {
        return Ok(Vec::new());
    }
    let seed_nodes = anchors
        .iter()
        .filter_map(|anchor| match anchor {
            AnchorRef::Node(node) => Some(node.clone()),
            _ => None,
        })
        .collect::<Vec<_>>();
    let mut seen = HashSet::<NodeId>::new();
    let mut candidates = Vec::<(NodeId, String)>::new();

    if let Some(plan_graph) =
        plan_graph.filter(|_| task_brief_should_follow_plan_neighbors(current_status, blockers))
    {
        for edge in plan_graph.edges.iter().filter(|edge| {
            (edge.from == *current_node_id || edge.to == *current_node_id)
                && task_brief_actionable_plan_edge(edge.kind)
        }) {
            let adjacent_id = if edge.from == *current_node_id {
                &edge.to
            } else {
                &edge.from
            };
            let Some(adjacent_node) = plan_graph.nodes.iter().find(|node| &node.id == adjacent_id)
            else {
                continue;
            };
            for anchor in &adjacent_node.bindings.anchors {
                let AnchorRef::Node(node_id) = anchor else {
                    continue;
                };
                if seed_nodes.iter().any(|seed| seed == node_id) || !seen.insert(node_id.clone()) {
                    continue;
                }
                candidates.push((
                    node_id.clone(),
                    format!(
                        "Adjacent plan node `{}` via {:?}.",
                        adjacent_node.title, edge.kind
                    ),
                ));
                if candidates.len() >= TASK_BRIEF_NEXT_READ_LIMIT.saturating_mul(4) {
                    break;
                }
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
    current_plan_node: Option<&PlanNode>,
    subject_validations: &[String],
    fallback_validation_refs: &[String],
) -> Vec<String> {
    let mut validations = if let Some(node) = current_plan_node {
        native_plan_likely_validations(node, &[])
    } else {
        subject_validations.to_vec()
    };
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

fn coordination_task_validation_refs(task: &CoordinationTask) -> Vec<String> {
    let mut labels = Vec::new();
    for validation in &task.validation_refs {
        push_unique_string(&mut labels, validation.id.clone());
        if labels.len() >= TASK_BRIEF_VALIDATION_LIMIT {
            break;
        }
    }
    labels
}

fn compact_coordination_task_risk_hint(
    task: &CoordinationTask,
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
    if coordination_task_is_workspace_bound(task)
        && task.base_revision.graph_version < workspace_revision.graph_version
    {
        return Some("Task base revision is stale against the current graph.".to_string());
    }
    None
}

fn coordination_task_is_workspace_bound(task: &CoordinationTask) -> bool {
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
    cognition_enabled: bool,
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
                if !cognition_enabled {
                    "Task is completed. Review the recent outcomes in this brief, then rerun prism_task_brief only if coordination changes.".to_string()
                } else if next_reads.is_empty() {
                    "Task is completed. Inspect recent outcomes or prism_query if you need follow-up context.".to_string()
                } else {
                    "Task is completed. Inspect recent outcomes or open a nextRead only if you need follow-up context.".to_string()
                }
            }
            prism_ir::CoordinationTaskStatus::Abandoned => {
                if cognition_enabled {
                    "Task is abandoned. Inspect recent outcomes or prism_query if you need historical context.".to_string()
                } else {
                    "Task is abandoned. Review the recent outcomes in this brief, then use prism_mutate if coordination state needs cleanup.".to_string()
                }
            }
            _ => unreachable!("terminal task status must be completed or abandoned"),
        };
    }
    if blockers
        .iter()
        .any(|blocker| blocker.kind == prism_coordination::BlockerKind::StaleRevision)
    {
        return if cognition_enabled {
            "Refresh this task against the current workspace revision, then rerun prism_task_brief or prism.blockers(taskId).".to_string()
        } else {
            "Refresh this task against the current workspace revision, then rerun prism_task_brief."
                .to_string()
        };
    }
    if !blockers.is_empty() {
        return if cognition_enabled {
            "Inspect the current task blockers before switching nodes; use prism.blockers(taskId) or prism_query for full coordination detail.".to_string()
        } else {
            "Inspect the blockers in this brief, use prism_mutate for the needed coordination change, then rerun prism_task_brief.".to_string()
        };
    }
    if !next_reads.is_empty() {
        return if cognition_enabled {
            "Use prism_open on a nextRead to work this plan node, or prism_query for full coordination detail.".to_string()
        } else {
            "Use the nextRead targets from this brief in your local workflow, then rerun prism_task_brief after coordination updates.".to_string()
        };
    }
    if cognition_enabled {
        "Inspect recent outcomes, validations, or prism_query for full coordination detail."
            .to_string()
    } else {
        "Inspect the recent outcomes and validations in this brief, then use prism_mutate for coordination changes before rerunning prism_task_brief.".to_string()
    }
}

fn task_brief_status_is_terminal(status: prism_ir::CoordinationTaskStatus) -> bool {
    matches!(
        status,
        prism_ir::CoordinationTaskStatus::Completed | prism_ir::CoordinationTaskStatus::Abandoned
    )
}

fn task_brief_lifecycle_view(
    status: prism_ir::CoordinationTaskStatus,
    task_execution: Option<&PlanExecutionOverlay>,
) -> CoordinationTaskLifecycleView {
    let integration_status = task_execution
        .map(|overlay| {
            overlay
                .git_execution
                .as_ref()
                .map(|git| git.integration_status)
        })
        .flatten()
        .unwrap_or(GitIntegrationStatus::NotStarted);
    let execution_status = task_execution
        .map(|overlay| overlay.git_execution.as_ref().map(|git| git.status))
        .flatten()
        .unwrap_or(GitExecutionStatus::NotStarted);
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

fn task_brief_should_follow_plan_neighbors(
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

fn task_brief_actionable_plan_edge(kind: prism_ir::PlanEdgeKind) -> bool {
    matches!(
        kind,
        prism_ir::PlanEdgeKind::DependsOn
            | prism_ir::PlanEdgeKind::Blocks
            | prism_ir::PlanEdgeKind::Validates
            | prism_ir::PlanEdgeKind::HandoffTo
            | prism_ir::PlanEdgeKind::ChildOf
    )
}

fn compact_native_plan_risk_hint(blockers: &[PlanNodeBlocker]) -> Option<String> {
    let hint = if let Some(blocker) = blockers.iter().find(|blocker| {
        matches!(
            blocker.kind,
            PlanNodeBlockerKind::RiskReviewRequired
                | PlanNodeBlockerKind::ReviewRequired
                | PlanNodeBlockerKind::ValidationRequired
                | PlanNodeBlockerKind::ValidationGate
                | PlanNodeBlockerKind::StaleRevision
                | PlanNodeBlockerKind::ArtifactStale
        )
    }) {
        blocker.summary.clone()
    } else {
        return None;
    };
    Some(clamp_string(&hint, TASK_BRIEF_TEXT_MAX_CHARS))
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
