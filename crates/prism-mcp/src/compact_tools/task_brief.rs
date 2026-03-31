use std::collections::HashSet;
use std::sync::Arc;

use anyhow::{anyhow, Result};
use prism_ir::{
    AnchorRef, CoordinationTaskId, NodeId, PlanGraph, PlanNode, PlanNodeBlocker,
    PlanNodeBlockerKind, PlanNodeId, PlanNodeStatus, TaskId,
};
use prism_js::{AgentOutcomeSummaryView, AgentTaskBlockerView, AgentTaskBriefResultView};
use prism_query::{PlanNodeRecommendation, PlanSummary, Prism};

use super::suggested_actions::{dedupe_suggested_actions, suggested_open_action};
use super::*;
use crate::PrismTaskBriefArgs;

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
            move |host, _query_run| {
                let prism = host.current_prism();
                let now = crate::current_timestamp();
                let subject = resolve_task_brief_subject(prism.as_ref(), &task_id, now)?;
                let plan_graph = prism.plan_graph(&subject.plan_id);
                let plan_summary = prism.plan_summary(&subject.plan_id);
                let plan_next = prism.plan_next(
                    &subject.plan_id,
                    TASK_BRIEF_NEXT_READ_LIMIT.saturating_mul(3),
                );
                let task_execution = prism
                    .plan_execution(&subject.plan_id)
                    .into_iter()
                    .find(|overlay| overlay.node_id == subject.node_id);
                let claims = prism.claims(&subject.anchors, now);
                let conflicts = prism.conflicts(&subject.anchors, now);
                let task_id = TaskId::new(subject.task_id.clone());
                let replay = crate::load_task_replay(
                    host.workspace_session_ref(),
                    prism.as_ref(),
                    &task_id,
                )?;
                let journal = crate::task_journal_view_from_replay(
                    session.as_ref(),
                    prism.as_ref(),
                    replay,
                    Some((Some(subject.title.clone()), Vec::new())),
                    TASK_BRIEF_OUTCOME_LIMIT,
                    0,
                )?;
                let next_reads = compact_task_next_reads(
                    session.as_ref(),
                    prism.as_ref(),
                    &subject.node_id,
                    &subject.anchors,
                    plan_graph.as_ref(),
                    plan_next.as_slice(),
                )?;
                let next_action = compact_task_brief_next_action(
                    &subject.node_id,
                    subject.blockers.as_slice(),
                    plan_summary.as_ref(),
                    plan_next.as_slice(),
                );

                let mut result = AgentTaskBriefResultView {
                    task_id: subject.task_id.clone(),
                    title: clamp_string(&subject.title, TASK_BRIEF_TEXT_MAX_CHARS),
                    status: subject.status,
                    assignee: subject.assignee.clone(),
                    pending_handoff_to: task_execution
                        .and_then(|overlay| overlay.pending_handoff_to)
                        .or(subject.pending_handoff_to.clone())
                        .map(|agent| agent.0.to_string()),
                    blockers: subject.blockers,
                    claim_holders: compact_claim_holders(claims.as_slice()),
                    conflict_summaries: conflicts
                        .iter()
                        .take(TASK_BRIEF_CONFLICT_LIMIT)
                        .map(|conflict| clamp_string(&conflict.summary, TASK_BRIEF_TEXT_MAX_CHARS))
                        .collect(),
                    recent_outcomes: journal
                        .recent_events
                        .iter()
                        .map(|event| compact_outcome_summary_view(event, TASK_BRIEF_TEXT_MAX_CHARS))
                        .collect(),
                    likely_validations: subject.likely_validations,
                    next_reads,
                    risk_hint: subject.risk_hint,
                    truncated: false,
                    next_action: Some(next_action),
                    suggested_actions: Vec::new(),
                };
                if let Some(next_read) = result.next_reads.first() {
                    result.suggested_actions = dedupe_suggested_actions([suggested_open_action(
                        next_read.handle.clone(),
                        prism_js::AgentOpenMode::Focus,
                    )]);
                }
                Ok((budgeted_task_brief_result(result)?, Vec::new()))
            },
        )
    }
}

struct TaskBriefSubject {
    task_id: String,
    node_id: PlanNodeId,
    plan_id: prism_ir::PlanId,
    title: String,
    status: prism_ir::CoordinationTaskStatus,
    assignee: Option<String>,
    pending_handoff_to: Option<prism_ir::AgentId>,
    anchors: Vec<AnchorRef>,
    blockers: Vec<AgentTaskBlockerView>,
    likely_validations: Vec<String>,
    risk_hint: Option<String>,
}

fn resolve_task_brief_subject(prism: &Prism, task_id: &str, now: u64) -> Result<TaskBriefSubject> {
    let coordination_task_id = CoordinationTaskId::new(task_id.to_string());
    if let Some(task) = prism.coordination_task(&coordination_task_id) {
        let blockers = prism
            .blockers(&coordination_task_id, now)
            .into_iter()
            .take(TASK_BRIEF_BLOCKER_LIMIT)
            .map(|blocker| AgentTaskBlockerView {
                kind: blocker.kind,
                summary: clamp_string(&blocker.summary, TASK_BRIEF_TEXT_MAX_CHARS),
            })
            .collect::<Vec<_>>();
        let likely_validations = prism
            .task_validation_recipe(&coordination_task_id)
            .as_ref()
            .map(|recipe| {
                let scored_checks = recipe
                    .scored_checks
                    .iter()
                    .cloned()
                    .map(crate::validation_check_view)
                    .collect::<Vec<_>>();
                compact_validation_checks(
                    &recipe.checks,
                    &scored_checks,
                    TASK_BRIEF_VALIDATION_LIMIT,
                    COMPACT_VALIDATION_CHECK_MAX_CHARS,
                )
            })
            .unwrap_or_default();
        let risk_hint = compact_task_risk_hint(
            prism.task_risk(&coordination_task_id, now).as_ref(),
            prism.plan_summary(&task.plan).as_ref(),
        );
        return Ok(TaskBriefSubject {
            task_id: task.id.0.to_string(),
            node_id: PlanNodeId::new(task.id.0.to_string()),
            plan_id: task.plan.clone(),
            title: task.title,
            status: task.status,
            assignee: task.assignee.map(|agent| agent.0.to_string()),
            pending_handoff_to: task.pending_handoff_to,
            anchors: task.anchors,
            blockers,
            likely_validations,
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
    let risk_hint =
        compact_native_plan_risk_hint(&plan_blockers, prism.plan_summary(&plan_id).as_ref());
    let effective_assignee = prism
        .plan_execution(&plan_id)
        .into_iter()
        .find(|overlay| overlay.node_id == node.id)
        .and_then(|overlay| overlay.effective_assignee)
        .or(node.assignee.clone())
        .map(|agent| agent.0.to_string());
    Ok(TaskBriefSubject {
        task_id: node.id.0.to_string(),
        node_id: node.id.clone(),
        plan_id,
        title: node.title,
        status: coordination_task_status_for_plan_node(node.status),
        assignee: effective_assignee,
        pending_handoff_to: None,
        anchors: node.bindings.anchors,
        blockers,
        likely_validations,
        risk_hint,
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

fn compact_task_next_reads(
    session: &SessionState,
    prism: &Prism,
    current_node_id: &PlanNodeId,
    anchors: &[AnchorRef],
    plan_graph: Option<&PlanGraph>,
    plan_next: &[PlanNodeRecommendation],
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

    for recommendation in plan_next {
        if recommendation.node.id == *current_node_id {
            continue;
        }
        let why = clamp_string(
            &recommendation.reasons.first().cloned().unwrap_or_else(|| {
                format!("Plan recommends `{}` next.", recommendation.node.title)
            }),
            TASK_BRIEF_TEXT_MAX_CHARS,
        );
        for anchor in &recommendation.node.bindings.anchors {
            let AnchorRef::Node(node_id) = anchor else {
                continue;
            };
            if seed_nodes.iter().any(|seed| seed == node_id) || !seen.insert(node_id.clone()) {
                continue;
            }
            candidates.push((node_id.clone(), why.clone()));
            if candidates.len() >= TASK_BRIEF_NEXT_READ_LIMIT.saturating_mul(4) {
                break;
            }
        }
        if candidates.len() >= TASK_BRIEF_NEXT_READ_LIMIT.saturating_mul(4) {
            break;
        }
    }

    if let Some(plan_graph) = plan_graph {
        for edge in plan_graph
            .edges
            .iter()
            .filter(|edge| edge.from == *current_node_id || edge.to == *current_node_id)
        {
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
        for owner in next_reads(prism, node, TASK_BRIEF_NEXT_READ_LIMIT.saturating_mul(3))? {
            let owner_node = node_id_from_view(&owner.symbol.id);
            if seed_nodes.iter().any(|seed| seed == &owner_node) || !seen.insert(owner_node.clone())
            {
                continue;
            }
            candidates.push((owner_node, owner.why));
        }
    }

    for node in seed_nodes.iter().take(4) {
        for related in prism.blast_radius(node).direct_nodes {
            if seed_nodes.iter().any(|seed| seed == &related) || !seen.insert(related.clone()) {
                continue;
            }
            candidates.push((related, "Task blast-radius follow-up.".to_string()));
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
        let symbol = symbol_view(prism, &symbol)?;
        next_reads.push(compact_target_view(session, &symbol, None, Some(why)));
        if next_reads.len() >= TASK_BRIEF_NEXT_READ_LIMIT {
            break;
        }
    }
    Ok(next_reads)
}

fn node_id_from_view(id: &prism_js::NodeIdView) -> NodeId {
    NodeId::new(id.crate_name.clone(), id.path.clone(), id.kind)
}

fn compact_task_risk_hint(
    risk: Option<&prism_query::TaskRisk>,
    plan_summary: Option<&PlanSummary>,
) -> Option<String> {
    let hint = if let Some(risk) = risk {
        if risk.review_required && !risk.has_approved_artifact {
            format!(
                "Risk {:.2} requires review before completion.",
                risk.risk_score
            )
        } else if !risk.contract_review_notes.is_empty() {
            risk.contract_review_notes[0].clone()
        } else if !risk.missing_validations.is_empty() {
            format!(
                "Missing validations: {}.",
                risk.missing_validations
                    .iter()
                    .take(2)
                    .cloned()
                    .collect::<Vec<_>>()
                    .join(", ")
            )
        } else if risk.stale_task {
            "Task base revision is stale against the current graph.".to_string()
        } else if !risk.risk_events.is_empty() {
            "Recent failures still contribute to task risk.".to_string()
        } else if !risk.likely_validations.is_empty() {
            format!(
                "Likely validations: {}.",
                risk.likely_validations
                    .iter()
                    .take(2)
                    .cloned()
                    .collect::<Vec<_>>()
                    .join(", ")
            )
        } else {
            format!(
                "Risk score {:.2} with no specific validations inferred yet.",
                risk.risk_score
            )
        }
    } else if let Some(summary) = plan_summary {
        if summary.completion_gated_nodes > 0 {
            format!(
                "{} plan node(s) are execution-ready but still gated on completion evidence.",
                summary.completion_gated_nodes
            )
        } else if summary.execution_blocked_nodes > 0 && summary.actionable_nodes == 0 {
            format!(
                "No actionable plan nodes right now; {} node(s) remain execution-blocked.",
                summary.execution_blocked_nodes
            )
        } else {
            return None;
        }
    } else {
        return None;
    };
    Some(clamp_string(&hint, TASK_BRIEF_TEXT_MAX_CHARS))
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
    current_node_id: &PlanNodeId,
    blockers: &[AgentTaskBlockerView],
    plan_summary: Option<&PlanSummary>,
    plan_next: &[PlanNodeRecommendation],
) -> String {
    if blockers
        .iter()
        .any(|blocker| blocker.kind == prism_coordination::BlockerKind::StaleRevision)
    {
        return "Refresh this task against the current workspace revision, then rerun prism_task_brief or prism.blockers(taskId).".to_string();
    }
    if !blockers.is_empty() {
        return "Inspect the current task blockers before switching nodes; use prism.blockers(taskId) or prism_query for full coordination detail.".to_string();
    }
    if let Some(recommendation) = plan_next.iter().find(|recommendation| {
        recommendation.actionable && recommendation.node.id != *current_node_id
    }) {
        return clamp_string(
            &format!(
                "Use prism_open on the recommended plan node `{}` to advance blocking work, or prism_query for full coordination detail.",
                recommendation.node.title
            ),
            TASK_BRIEF_TEXT_MAX_CHARS,
        );
    }
    if plan_next.iter().any(|recommendation| {
        recommendation.actionable && recommendation.node.id == *current_node_id
    }) {
        return "Use prism_open on a nextRead to work this plan node, or prism_query for full coordination detail.".to_string();
    }
    if let Some(recommendation) = plan_next
        .iter()
        .find(|recommendation| recommendation.node.id != *current_node_id)
    {
        return clamp_string(
            &format!(
                "Use prism_open on the recommended plan node `{}` to inspect blocking work, or prism_query for full coordination detail.",
                recommendation.node.title
            ),
            TASK_BRIEF_TEXT_MAX_CHARS,
        );
    }
    if let Some(summary) = plan_summary {
        if summary.actionable_nodes == 0 && summary.execution_blocked_nodes > 0 {
            return "Use prism_query to inspect blockers across the plan before continuing."
                .to_string();
        }
    }
    "Use prism_open on a nextRead, or prism_query for full coordination detail.".to_string()
}

fn compact_native_plan_risk_hint(
    blockers: &[PlanNodeBlocker],
    plan_summary: Option<&PlanSummary>,
) -> Option<String> {
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
    } else if let Some(summary) = plan_summary {
        if summary.completion_gated_nodes > 0 {
            format!(
                "{} plan node(s) are execution-ready but still gated on completion evidence.",
                summary.completion_gated_nodes
            )
        } else if summary.execution_blocked_nodes > 0 && summary.actionable_nodes == 0 {
            format!(
                "No actionable plan nodes right now; {} node(s) remain execution-blocked.",
                summary.execution_blocked_nodes
            )
        } else {
            return None;
        }
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
