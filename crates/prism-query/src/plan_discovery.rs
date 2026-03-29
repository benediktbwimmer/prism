use prism_ir::{PlanScope, PlanStatus};
use std::collections::BTreeSet;

use crate::{NativePlanRuntimeState, PlanListEntry, Prism};

impl Prism {
    pub fn plans(
        &self,
        status: Option<PlanStatus>,
        scope: Option<PlanScope>,
        contains: Option<&str>,
    ) -> Vec<PlanListEntry> {
        let runtime = self
            .plan_runtime
            .read()
            .expect("plan runtime lock poisoned")
            .clone();
        self.plans_for_runtime(&runtime, status, scope, contains)
    }

    fn plans_for_runtime(
        &self,
        runtime: &NativePlanRuntimeState,
        status: Option<PlanStatus>,
        scope: Option<PlanScope>,
        contains: Option<&str>,
    ) -> Vec<PlanListEntry> {
        let contains = contains
            .map(str::trim)
            .filter(|value| !value.is_empty())
            .map(|value| value.to_ascii_lowercase());

        let mut plans = self
            .hydrated_plan_graphs_for_runtime(runtime)
            .into_iter()
            .filter(|graph| status.is_none_or(|expected| graph.status == expected))
            .filter(|graph| scope.is_none_or(|expected| graph.scope == expected))
            .filter(|graph| {
                contains
                    .as_ref()
                    .is_none_or(|needle| plan_matches_contains_filter(graph, needle))
            })
            .filter_map(|graph| {
                let summary = self.plan_summary_for_runtime(runtime, &graph.id)?;
                Some(PlanListEntry {
                    plan_id: graph.id.clone(),
                    title: graph.title,
                    goal: graph.goal,
                    status: graph.status,
                    scope: graph.scope,
                    kind: graph.kind,
                    root_node_ids: graph.root_nodes,
                    summary,
                })
            })
            .collect::<Vec<_>>();

        plans.sort_by(|left, right| {
            plan_status_rank(left.status)
                .cmp(&plan_status_rank(right.status))
                .then_with(|| {
                    right
                        .summary
                        .actionable_nodes
                        .cmp(&left.summary.actionable_nodes)
                })
                .then_with(|| left.title.cmp(&right.title))
                .then_with(|| left.plan_id.0.cmp(&right.plan_id.0))
        });
        plans
    }
}

fn plan_matches_contains_filter(graph: &prism_ir::PlanGraph, needle: &str) -> bool {
    let id = graph.id.0.to_ascii_lowercase();
    let title = graph.title.to_ascii_lowercase();
    let goal = graph.goal.to_ascii_lowercase();
    if id.contains(needle) || title.contains(needle) || goal.contains(needle) {
        return true;
    }

    let plan_terms = normalized_plan_terms(&format!("{id} {title} {goal}"));
    let query_terms = normalized_plan_terms(needle);
    !query_terms.is_empty()
        && query_terms
            .iter()
            .all(|term| plan_terms.contains(term.as_str()))
}

fn normalized_plan_terms(value: &str) -> BTreeSet<String> {
    value
        .split(|ch: char| !ch.is_ascii_alphanumeric())
        .filter(|token| !token.is_empty())
        .map(|token| normalize_plan_term(&token.to_ascii_lowercase()))
        .filter(|token| !token.is_empty())
        .collect()
}

fn normalize_plan_term(token: &str) -> String {
    if token.len() > 3 && token.ends_with("ies") {
        let mut stem = token[..token.len() - 3].to_string();
        stem.push('y');
        return stem;
    }
    if token.len() > 3
        && token.ends_with('s')
        && !token.ends_with("ss")
        && !token.ends_with("us")
        && !token.ends_with("is")
    {
        return token[..token.len() - 1].to_string();
    }
    token.to_string()
}

fn plan_status_rank(status: PlanStatus) -> u8 {
    match status {
        PlanStatus::Active => 0,
        PlanStatus::Blocked => 1,
        PlanStatus::Draft => 2,
        PlanStatus::Completed => 3,
        PlanStatus::Abandoned => 4,
    }
}
