use std::collections::HashSet;
use std::sync::Arc;

use anyhow::{anyhow, Result};
use prism_ir::{AnchorRef, CoordinationTaskId, NodeId, PlanGraph, TaskId};
use prism_js::{AgentOutcomeSummaryView, AgentTaskBlockerView, AgentTaskBriefResultView};
use prism_query::{PlanNodeRecommendation, PlanSummary, Prism};

use super::suggested_actions::{dedupe_suggested_actions, suggested_open_action};
use super::*;
use crate::task_journal_view;
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
                let coordination_task_id = CoordinationTaskId::new(task_id.clone());
                let task = prism
                    .coordination_task(&coordination_task_id)
                    .ok_or_else(|| anyhow!("unknown coordination task `{task_id}`"))?;
                let plan_graph = prism.plan_graph(&task.plan);
                let plan_summary = prism.plan_summary(&task.plan);
                let plan_next =
                    prism.plan_next(&task.plan, TASK_BRIEF_NEXT_READ_LIMIT.saturating_mul(3));
                let task_execution = prism
                    .plan_execution(&task.plan)
                    .into_iter()
                    .find(|overlay| overlay.node_id.0 == task.id.0);
                let now = crate::current_timestamp();
                let blockers = prism
                    .blockers(&coordination_task_id, now)
                    .into_iter()
                    .take(TASK_BRIEF_BLOCKER_LIMIT)
                    .map(|blocker| AgentTaskBlockerView {
                        kind: blocker.kind,
                        summary: clamp_string(&blocker.summary, TASK_BRIEF_TEXT_MAX_CHARS),
                    })
                    .collect::<Vec<_>>();
                let claims = prism.claims(&task.anchors, now);
                let conflicts = prism.conflicts(&task.anchors, now);
                let journal = task_journal_view(
                    session.as_ref(),
                    prism.as_ref(),
                    &TaskId::new(task.id.0.clone()),
                    Some((Some(task.title.clone()), Vec::new())),
                    TASK_BRIEF_OUTCOME_LIMIT,
                    0,
                )?;
                let validation_recipe = prism.task_validation_recipe(&coordination_task_id);
                let likely_validations = validation_recipe
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
                let next_reads = compact_task_next_reads(
                    session.as_ref(),
                    prism.as_ref(),
                    &task,
                    plan_graph.as_ref(),
                    plan_next.as_slice(),
                )?;
                let risk_hint = compact_task_risk_hint(
                    prism.task_risk(&coordination_task_id, now).as_ref(),
                    plan_summary.as_ref(),
                );
                let next_action = compact_task_brief_next_action(
                    &task,
                    blockers.as_slice(),
                    plan_summary.as_ref(),
                    plan_next.as_slice(),
                );

                let mut result = AgentTaskBriefResultView {
                    task_id: task.id.0.to_string(),
                    title: clamp_string(&task.title, TASK_BRIEF_TEXT_MAX_CHARS),
                    status: task.status,
                    assignee: task.assignee.clone().map(|agent| agent.0.to_string()),
                    pending_handoff_to: task_execution
                        .and_then(|overlay| overlay.pending_handoff_to)
                        .or(task.pending_handoff_to.clone())
                        .map(|agent| agent.0.to_string()),
                    blockers,
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
                    likely_validations,
                    next_reads,
                    risk_hint,
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
    task: &prism_coordination::CoordinationTask,
    plan_graph: Option<&PlanGraph>,
    plan_next: &[PlanNodeRecommendation],
) -> Result<Vec<AgentTargetHandleView>> {
    let seed_nodes = task
        .anchors
        .iter()
        .filter_map(|anchor| match anchor {
            AnchorRef::Node(node) => Some(node.clone()),
            _ => None,
        })
        .collect::<Vec<_>>();
    let mut seen = HashSet::<NodeId>::new();
    let mut candidates = Vec::<(NodeId, String)>::new();

    for recommendation in plan_next {
        if recommendation.node.id.0 == task.id.0 {
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
            .filter(|edge| edge.from.0 == task.id.0 || edge.to.0 == task.id.0)
        {
            let adjacent_id = if edge.from.0 == task.id.0 {
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
    task: &prism_coordination::CoordinationTask,
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
    if let Some(recommendation) = plan_next
        .iter()
        .find(|recommendation| recommendation.actionable && recommendation.node.id.0 != task.id.0)
    {
        return clamp_string(
            &format!(
                "Use prism_open on the recommended plan node `{}` to advance blocking work, or prism_query for full coordination detail.",
                recommendation.node.title
            ),
            TASK_BRIEF_TEXT_MAX_CHARS,
        );
    }
    if plan_next
        .iter()
        .any(|recommendation| recommendation.actionable && recommendation.node.id.0 == task.id.0)
    {
        return "Use prism_open on a nextRead to work this plan node, or prism_query for full coordination detail.".to_string();
    }
    if let Some(recommendation) = plan_next
        .iter()
        .find(|recommendation| recommendation.node.id.0 != task.id.0)
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
