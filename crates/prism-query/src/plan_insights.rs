use std::collections::BTreeSet;

use prism_ir::{
    PlanEdgeKind, PlanId, PlanNode, PlanNodeBlockerKind, PlanNodeId, PlanNodeStatus, PlanStatus,
    Timestamp,
};

use crate::plan_completion::current_timestamp;
use crate::plan_runtime::NativePlanRuntimeState;
use crate::{PlanNodeRecommendation, PlanSummary, Prism};

impl Prism {
    pub(crate) fn actionable_plan_nodes_for_runtime(
        &self,
        runtime: &NativePlanRuntimeState,
        plan_id: &PlanId,
        now: Timestamp,
    ) -> Vec<PlanNode> {
        let Some(graph) = runtime.plan_graph(plan_id) else {
            return Vec::new();
        };
        if graph.status != PlanStatus::Active {
            return Vec::new();
        }

        let mut nodes = graph
            .nodes
            .iter()
            .filter(|node| is_actionable_candidate(node))
            .filter(|node| {
                self.plan_node_blockers_for_runtime(runtime, &graph.id, &node.id, now)
                    .is_empty()
            })
            .cloned()
            .collect::<Vec<_>>();
        nodes.sort_by(|left, right| left.id.0.cmp(&right.id.0));
        nodes
    }

    pub fn plan_summary(&self, plan_id: &PlanId) -> Option<PlanSummary> {
        let runtime = self
            .plan_runtime
            .read()
            .expect("plan runtime lock poisoned")
            .clone();
        self.plan_summary_for_runtime(&runtime, plan_id)
    }

    pub fn plan_next(&self, plan_id: &PlanId, limit: usize) -> Vec<PlanNodeRecommendation> {
        let runtime = self
            .plan_runtime
            .read()
            .expect("plan runtime lock poisoned")
            .clone();
        self.plan_next_for_runtime(&runtime, plan_id, limit)
    }

    pub(crate) fn plan_summary_for_runtime(
        &self,
        runtime: &NativePlanRuntimeState,
        plan_id: &PlanId,
    ) -> Option<PlanSummary> {
        let graph = runtime.plan_graph(plan_id)?;
        let now = current_timestamp();
        let actionable_ids = self
            .actionable_plan_nodes_for_runtime(runtime, plan_id, now)
            .into_iter()
            .map(|node| node.id)
            .collect::<BTreeSet<_>>();

        let mut summary = PlanSummary {
            plan_id: graph.id.clone(),
            status: graph.status,
            total_nodes: graph.nodes.len(),
            completed_nodes: 0,
            abandoned_nodes: 0,
            in_progress_nodes: 0,
            actionable_nodes: 0,
            execution_blocked_nodes: 0,
            completion_gated_nodes: 0,
            review_gated_nodes: 0,
            validation_gated_nodes: 0,
            stale_nodes: 0,
            claim_conflicted_nodes: 0,
        };

        for node in graph.nodes.iter().filter(|node| !node.is_abstract) {
            match node.status {
                PlanNodeStatus::Completed => {
                    summary.completed_nodes += 1;
                    continue;
                }
                PlanNodeStatus::Abandoned => {
                    summary.abandoned_nodes += 1;
                    continue;
                }
                PlanNodeStatus::InProgress => summary.in_progress_nodes += 1,
                _ => {}
            }

            let blockers = self.plan_node_blockers_for_runtime(runtime, &graph.id, &node.id, now);
            let has_completion_gates = blockers
                .iter()
                .any(|blocker| is_completion_gate(blocker.kind));

            if actionable_ids.contains(&node.id) {
                summary.actionable_nodes += 1;
            } else {
                summary.execution_blocked_nodes += 1;
            }

            if has_completion_gates {
                summary.completion_gated_nodes += 1;
            }
            if blockers.iter().any(|blocker| is_review_gate(blocker.kind)) {
                summary.review_gated_nodes += 1;
            }
            if blockers
                .iter()
                .any(|blocker| is_validation_gate(blocker.kind))
            {
                summary.validation_gated_nodes += 1;
            }
            if blockers.iter().any(|blocker| is_stale_gate(blocker.kind)) {
                summary.stale_nodes += 1;
            }
            if blockers
                .iter()
                .any(|blocker| blocker.kind == PlanNodeBlockerKind::ClaimConflict)
            {
                summary.claim_conflicted_nodes += 1;
            }
        }

        Some(summary)
    }

    fn plan_next_for_runtime(
        &self,
        runtime: &NativePlanRuntimeState,
        plan_id: &PlanId,
        limit: usize,
    ) -> Vec<PlanNodeRecommendation> {
        let Some(graph) = runtime.plan_graph(plan_id) else {
            return Vec::new();
        };
        let execution = runtime.plan_execution(plan_id);
        let now = current_timestamp();

        let mut recommendations = graph
            .nodes
            .iter()
            .filter(|node| !node.is_abstract && !is_terminal(node))
            .map(|node| {
                let effective_assignee = execution
                    .iter()
                    .find(|overlay| overlay.node_id == node.id)
                    .and_then(|overlay| overlay.effective_assignee.clone())
                    .or_else(|| node.assignee.clone());
                let blockers =
                    self.plan_node_blockers_for_runtime(runtime, &graph.id, &node.id, now);
                let actionable = is_actionable_candidate(node) && blockers.is_empty();
                let unblocks = unlocked_neighbors(&graph, &node.id);
                let reasons = recommendation_reasons(
                    node,
                    actionable,
                    effective_assignee.as_ref().map(|agent| agent.0.as_str()),
                    &blockers,
                    &unblocks,
                );
                let score = recommendation_score(node, actionable, &blockers, unblocks.len());
                PlanNodeRecommendation {
                    node: node.clone(),
                    actionable,
                    effective_assignee,
                    score,
                    reasons,
                    blockers,
                    unblocks,
                }
            })
            .collect::<Vec<_>>();

        recommendations.sort_by(|left, right| {
            right
                .actionable
                .cmp(&left.actionable)
                .then_with(|| {
                    right
                        .score
                        .partial_cmp(&left.score)
                        .unwrap_or(std::cmp::Ordering::Equal)
                })
                .then_with(|| right.unblocks.len().cmp(&left.unblocks.len()))
                .then_with(|| left.node.id.0.cmp(&right.node.id.0))
        });
        recommendations.truncate(limit.max(1));
        recommendations
    }
}

fn is_terminal(node: &PlanNode) -> bool {
    matches!(
        node.status,
        PlanNodeStatus::Completed | PlanNodeStatus::Abandoned
    )
}

fn is_actionable_candidate(node: &PlanNode) -> bool {
    !node.is_abstract
        && matches!(
            node.status,
            PlanNodeStatus::Ready | PlanNodeStatus::InProgress
        )
}

fn is_execution_blocker(kind: PlanNodeBlockerKind) -> bool {
    matches!(
        kind,
        PlanNodeBlockerKind::Dependency
            | PlanNodeBlockerKind::BlockingNode
            | PlanNodeBlockerKind::Handoff
            | PlanNodeBlockerKind::ClaimConflict
    )
}

fn is_completion_gate(kind: PlanNodeBlockerKind) -> bool {
    !is_execution_blocker(kind)
}

fn is_review_gate(kind: PlanNodeBlockerKind) -> bool {
    matches!(
        kind,
        PlanNodeBlockerKind::ReviewRequired | PlanNodeBlockerKind::RiskReviewRequired
    )
}

fn is_validation_gate(kind: PlanNodeBlockerKind) -> bool {
    matches!(
        kind,
        PlanNodeBlockerKind::ValidationGate | PlanNodeBlockerKind::ValidationRequired
    )
}

fn is_stale_gate(kind: PlanNodeBlockerKind) -> bool {
    matches!(
        kind,
        PlanNodeBlockerKind::StaleRevision | PlanNodeBlockerKind::ArtifactStale
    )
}

fn unlocked_neighbors(graph: &prism_ir::PlanGraph, node_id: &PlanNodeId) -> Vec<PlanNodeId> {
    let mut unblocks = BTreeSet::new();
    for edge in &graph.edges {
        match edge.kind {
            PlanEdgeKind::DependsOn | PlanEdgeKind::Blocks | PlanEdgeKind::Validates
                if edge.to == *node_id =>
            {
                unblocks.insert(edge.from.clone());
            }
            PlanEdgeKind::ChildOf if edge.from == *node_id => {
                unblocks.insert(edge.to.clone());
            }
            PlanEdgeKind::HandoffTo if edge.from == *node_id => {
                unblocks.insert(edge.to.clone());
            }
            _ => {}
        }
    }
    unblocks.into_iter().collect()
}

fn recommendation_reasons(
    node: &PlanNode,
    actionable: bool,
    effective_assignee: Option<&str>,
    blockers: &[prism_ir::PlanNodeBlocker],
    unblocks: &[PlanNodeId],
) -> Vec<String> {
    let mut reasons = Vec::new();
    if node.status == PlanNodeStatus::InProgress {
        reasons.push("Already in progress.".to_string());
    } else if actionable {
        reasons.push("Actionable now.".to_string());
    } else if !blockers.is_empty() {
        reasons.push(format!(
            "Blocked by {} execution issue(s).",
            blockers
                .iter()
                .filter(|blocker| is_execution_blocker(blocker.kind))
                .count()
        ));
    }
    if !unblocks.is_empty() {
        reasons.push(format!(
            "Completion would unblock {} node(s).",
            unblocks.len()
        ));
    }
    if let Some(assignee) = effective_assignee {
        reasons.push(format!("Suggested owner: `{assignee}`."));
    }
    let completion_gates = blockers
        .iter()
        .filter(|blocker| is_completion_gate(blocker.kind))
        .map(|blocker| blocker.summary.clone())
        .collect::<Vec<_>>();
    if !completion_gates.is_empty() {
        reasons.push(format!(
            "Closure still needs: {}",
            completion_gates.join("; ")
        ));
    }
    reasons
}

fn recommendation_score(
    node: &PlanNode,
    actionable: bool,
    blockers: &[prism_ir::PlanNodeBlocker],
    unblock_count: usize,
) -> f32 {
    let mut score = 0.0;
    if actionable {
        score += 1000.0;
    }
    if node.status == PlanNodeStatus::InProgress {
        score += 200.0;
    }
    score += unblock_count as f32 * 25.0;
    score += node.priority.unwrap_or(0) as f32;
    score -= blockers
        .iter()
        .filter(|blocker| is_execution_blocker(blocker.kind))
        .count() as f32
        * 50.0;
    score -= blockers
        .iter()
        .filter(|blocker| is_completion_gate(blocker.kind))
        .count() as f32
        * 5.0;
    score
}
