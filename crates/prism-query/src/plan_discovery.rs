use prism_ir::{DerivedPlanStatus, EffectiveTaskStatus, NodeRefKind, PlanScope, PlanStatus};
use std::collections::{BTreeMap, BTreeSet};

use crate::common::current_timestamp;
use crate::{PlanListEntry, PlanNodeStatusCounts, Prism};

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
        let contains = contains
            .map(str::trim)
            .filter(|value| !value.is_empty())
            .map(|value| value.to_ascii_lowercase());
        if let Some(entries) = self.cached_plan_entries(status, scope, contains.as_deref()) {
            return entries;
        }

        let snapshot = self.coordination_snapshot_v2();
        let Some(derivations) = snapshot.derive_statuses().ok() else {
            return Vec::new();
        };
        let Ok(graph) = snapshot.graph() else {
            return Vec::new();
        };
        let activity_by_plan = self.plan_activity_index();
        let task_ids_by_plan = descendant_task_ids_by_plan(&graph);

        let mut plans: Vec<PlanListEntry> = snapshot
            .plans
            .iter()
            .into_iter()
            .filter_map(|plan| {
                let summary = self.plan_summary(&plan.id)?;
                let node_status_counts = canonical_plan_node_status_counts(
                    task_ids_by_plan
                        .get(plan.id.0.as_str())
                        .map(Vec::as_slice)
                        .unwrap_or(&[]),
                    &derivations,
                );
                let derived_status = derivations.plan_state(&plan.id)?;
                let status = compatibility_plan_status(derived_status.derived_status);
                Some(PlanListEntry {
                    plan_id: plan.id.clone(),
                    title: plan.title.clone(),
                    goal: plan.goal.clone(),
                    status,
                    scope: plan.scope,
                    kind: plan.kind,
                    policy: plan.policy.clone(),
                    scheduling: plan.scheduling.clone(),
                    summary: plan_discovery_summary(&summary),
                    plan_summary: summary,
                    node_status_counts,
                    activity: activity_by_plan
                        .get(plan.id.0.as_str())
                        .cloned()
                        .unwrap_or_default(),
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
                .then_with(|| {
                    right
                        .plan_summary
                        .in_progress_nodes
                        .cmp(&left.plan_summary.in_progress_nodes)
                })
                .then_with(|| left.title.cmp(&right.title))
                .then_with(|| left.plan_id.0.cmp(&right.plan_id.0))
        });
        let entries = plans;
        self.store_plan_entries_cache(&entries);
        filter_plan_entries(&entries, status, scope, contains.as_deref())
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

fn descendant_task_ids_by_plan(
    graph: &prism_coordination::CanonicalCoordinationGraph<'_>,
) -> BTreeMap<String, Vec<prism_ir::TaskId>> {
    let mut descendant_tasks = BTreeMap::<String, Vec<prism_ir::TaskId>>::new();
    for node in graph.topological_order().iter().rev() {
        let Some(prism_coordination::CanonicalNodeRecord::Plan(plan)) = graph.node(node) else {
            continue;
        };
        let mut tasks = Vec::new();
        for child in graph.children_of_plan(&plan.id) {
            match child.kind {
                NodeRefKind::Task => tasks.push(prism_ir::TaskId::new(child.id)),
                NodeRefKind::Plan => {
                    if let Some(child_tasks) = descendant_tasks.get(child.id.as_str()) {
                        tasks.extend(child_tasks.iter().cloned());
                    }
                }
            }
        }
        tasks.sort_by(|left, right| left.0.cmp(&right.0));
        tasks.dedup_by(|left, right| left == right);
        descendant_tasks.insert(plan.id.0.to_string(), tasks);
    }
    descendant_tasks
}

fn canonical_plan_node_status_counts(
    task_ids: &[prism_ir::TaskId],
    derivations: &prism_coordination::CoordinationDerivations,
) -> PlanNodeStatusCounts {
    let mut counts = PlanNodeStatusCounts::default();
    for task_id in task_ids {
        let Some(task_state) = derivations.task_state(task_id) else {
            continue;
        };
        match task_state.effective_status {
            EffectiveTaskStatus::Pending => counts.proposed += 1,
            EffectiveTaskStatus::Active => counts.in_progress += 1,
            EffectiveTaskStatus::Blocked => counts.blocked += 1,
            EffectiveTaskStatus::BrokenDependency => counts.waiting += 1,
            EffectiveTaskStatus::Completed => counts.completed += 1,
            EffectiveTaskStatus::Abandoned => counts.abandoned += 1,
            EffectiveTaskStatus::Failed => counts.blocked += 1,
        }
        if task_state.graph_actionable {
            counts.ready += 1;
        }
    }
    counts
}

fn compatibility_plan_status(status: DerivedPlanStatus) -> PlanStatus {
    match status {
        DerivedPlanStatus::Pending => PlanStatus::Draft,
        DerivedPlanStatus::Active => PlanStatus::Active,
        DerivedPlanStatus::Blocked => PlanStatus::Blocked,
        DerivedPlanStatus::BrokenDependency => PlanStatus::Blocked,
        DerivedPlanStatus::Completed => PlanStatus::Completed,
        DerivedPlanStatus::Failed => PlanStatus::Blocked,
        DerivedPlanStatus::Abandoned => PlanStatus::Abandoned,
        DerivedPlanStatus::Archived => PlanStatus::Archived,
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
