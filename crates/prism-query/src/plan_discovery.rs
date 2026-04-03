use prism_ir::{PlanScope, PlanStatus};
use std::collections::BTreeSet;

use crate::plan_completion::current_timestamp;
use crate::{NativePlanRuntimeState, PlanListEntry, Prism};

const PLAN_DISCOVERY_CACHE_TTL_SECS: u64 = 5;

#[derive(Debug, Clone)]
pub(crate) struct PlanDiscoveryCache {
    built_at: u64,
    entries: Vec<PlanListEntry>,
}

impl PlanDiscoveryCache {
    fn is_fresh(&self, now: u64) -> bool {
        now.saturating_sub(self.built_at) <= PLAN_DISCOVERY_CACHE_TTL_SECS
    }
}

impl Prism {
    pub(crate) fn invalidate_plan_discovery_cache(&self) {
        *self
            .plan_discovery_cache
            .write()
            .expect("plan discovery cache lock poisoned") = None;
    }

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
        if let Some(entries) = self.cached_plan_entries(status, scope, contains.as_deref()) {
            return entries;
        }

        let mut plans = self
            .hydrated_plan_graphs_for_runtime(runtime)
            .into_iter()
            .filter_map(|graph| {
                let summary = self.plan_summary_for_hydrated_graph(runtime, &graph);
                Some(PlanListEntry {
                    plan_id: graph.id.clone(),
                    title: graph.title,
                    goal: graph.goal,
                    status: graph.status,
                    scope: graph.scope,
                    kind: graph.kind,
                    root_node_ids: graph.root_nodes,
                    summary: plan_discovery_summary(&summary),
                    plan_summary: summary,
                })
            })
            .collect::<Vec<_>>();

        plans.sort_by(|left, right| {
            plan_status_rank(left.status)
                .cmp(&plan_status_rank(right.status))
                .then_with(|| {
                    right
                        .plan_summary
                        .actionable_nodes
                        .cmp(&left.plan_summary.actionable_nodes)
                })
                .then_with(|| left.title.cmp(&right.title))
                .then_with(|| left.plan_id.0.cmp(&right.plan_id.0))
        });
        self.store_plan_entries_cache(&plans);
        filter_plan_entries(&plans, status, scope, contains.as_deref())
    }

    fn cached_plan_entries(
        &self,
        status: Option<PlanStatus>,
        scope: Option<PlanScope>,
        contains: Option<&str>,
    ) -> Option<Vec<PlanListEntry>> {
        let now = current_timestamp();
        let cache = self
            .plan_discovery_cache
            .read()
            .expect("plan discovery cache lock poisoned");
        let cache = cache.as_ref()?;
        if !cache.is_fresh(now) {
            return None;
        }
        Some(filter_plan_entries(&cache.entries, status, scope, contains))
    }

    fn store_plan_entries_cache(&self, entries: &[PlanListEntry]) {
        *self
            .plan_discovery_cache
            .write()
            .expect("plan discovery cache lock poisoned") = Some(PlanDiscoveryCache {
            built_at: current_timestamp(),
            entries: entries.to_vec(),
        });
    }
}

fn plan_list_entry_matches_contains_filter(entry: &PlanListEntry, needle: &str) -> bool {
    let id = entry.plan_id.0.to_ascii_lowercase();
    let title = entry.title.to_ascii_lowercase();
    let goal = entry.goal.to_ascii_lowercase();
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

fn filter_plan_entries(
    entries: &[PlanListEntry],
    status: Option<PlanStatus>,
    scope: Option<PlanScope>,
    contains: Option<&str>,
) -> Vec<PlanListEntry> {
    entries
        .iter()
        .filter(|entry| status.is_none_or(|expected| entry.status == expected))
        .filter(|entry| scope.is_none_or(|expected| entry.scope == expected))
        .filter(|entry| {
            contains.is_none_or(|needle| plan_list_entry_matches_contains_filter(entry, needle))
        })
        .cloned()
        .collect()
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
        PlanStatus::Archived => 5,
    }
}

fn plan_discovery_summary(summary: &crate::PlanSummary) -> String {
    let mut parts = Vec::new();
    if summary.actionable_nodes > 0 {
        parts.push(format!("{} actionable", summary.actionable_nodes));
    }
    if summary.in_progress_nodes > 0 {
        parts.push(format!("{} in progress", summary.in_progress_nodes));
    }
    if summary.execution_blocked_nodes > 0 {
        parts.push(format!("{} blocked", summary.execution_blocked_nodes));
    }
    if summary.completed_nodes > 0 {
        parts.push(format!("{} completed", summary.completed_nodes));
    }
    if summary.abandoned_nodes > 0 {
        parts.push(format!("{} abandoned", summary.abandoned_nodes));
    }
    if parts.is_empty() {
        parts.push(format!("{} nodes", summary.total_nodes));
    }
    format!("{} of {} nodes", parts.join(", "), summary.total_nodes)
}
