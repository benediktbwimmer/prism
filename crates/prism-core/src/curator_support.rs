use std::collections::HashSet;

use anyhow::Result;
use prism_curator::{
    CuratorBudget, CuratorContext, CuratorGraphSlice, CuratorLineageSlice, CuratorProjectionSlice,
    CuratorSnapshot, CuratorTrigger,
};
use prism_ir::{AnchorRef, Node, NodeId};
use prism_memory::{EpisodicMemorySnapshot, OutcomeEvent, OutcomeKind, OutcomeResult};
use prism_query::Prism;
use prism_store::{SqliteStore, Store};

use crate::patch_outcomes::{dedupe_anchors, observed_is_empty};

pub(crate) fn next_curator_sequence(snapshot: &CuratorSnapshot) -> u64 {
    snapshot
        .records
        .iter()
        .filter_map(|record| record.id.0.rsplit(':').next()?.parse::<u64>().ok())
        .max()
        .unwrap_or(0)
}

pub(crate) fn curator_job_for_observed(
    observed: &prism_ir::ObservedChangeSet,
    prism: &Prism,
) -> Option<(CuratorTrigger, Vec<AnchorRef>)> {
    if observed_is_empty(observed) {
        return None;
    }

    let changed_nodes = observed.added.len() + observed.removed.len() + observed.updated.len() * 2;
    let mut focus = Vec::new();
    focus.extend(
        observed
            .updated
            .iter()
            .map(|(_, after)| AnchorRef::Node(after.node.id.clone())),
    );
    focus.extend(
        observed
            .added
            .iter()
            .map(|node| AnchorRef::Node(node.node.id.clone())),
    );
    focus.extend(
        observed
            .removed
            .iter()
            .map(|node| AnchorRef::Node(node.node.id.clone())),
    );
    let focus = dedupe_anchors(prism.anchors_for(&focus));
    if focus.is_empty() {
        return None;
    }

    let has_related_failures = focus.iter().any(|anchor| match anchor {
        AnchorRef::Node(id) => !prism.related_failures(id).is_empty(),
        _ => false,
    });
    if changed_nodes < 3 && observed.files.len() < 2 && !has_related_failures {
        return None;
    }

    let trigger = if changed_nodes >= 6 || observed.files.len() >= 2 {
        CuratorTrigger::HotspotChanged
    } else {
        CuratorTrigger::PostChange
    };
    Some((trigger, focus))
}

pub(crate) fn curator_trigger_for_outcome(
    prism: &Prism,
    event: &OutcomeEvent,
) -> Option<CuratorTrigger> {
    match event.kind {
        OutcomeKind::FailureObserved | OutcomeKind::RegressionObserved => {
            let failures = prism.outcomes_for(&event.anchors, 8);
            if failures
                .iter()
                .filter(|candidate| {
                    matches!(
                        candidate.kind,
                        OutcomeKind::FailureObserved | OutcomeKind::RegressionObserved
                    )
                })
                .count()
                >= 2
            {
                Some(CuratorTrigger::RepeatedFailure)
            } else {
                None
            }
        }
        OutcomeKind::FixValidated => Some(CuratorTrigger::TaskCompleted),
        OutcomeKind::BuildRan | OutcomeKind::TestRan
            if matches!(event.result, OutcomeResult::Failure) =>
        {
            Some(CuratorTrigger::RepeatedFailure)
        }
        _ => None,
    }
}

pub(crate) fn build_curator_context(
    prism: &Prism,
    store: &mut SqliteStore,
    focus: &[AnchorRef],
    budget: &CuratorBudget,
) -> Result<CuratorContext> {
    let focus = prism.anchors_for(focus);
    let lineages = focus_lineages(prism, &focus);
    let nodes = focus_nodes(prism, &focus, budget.max_context_nodes);
    let node_set = nodes
        .iter()
        .map(|node| node.id.clone())
        .collect::<HashSet<_>>();
    let max_edges = budget.max_context_nodes.saturating_mul(4).max(1);
    let mut edges = prism
        .graph()
        .edges
        .iter()
        .filter(|edge| node_set.contains(&edge.source) || node_set.contains(&edge.target))
        .cloned()
        .collect::<Vec<_>>();
    if edges.len() > max_edges {
        edges.truncate(max_edges);
    }

    let mut lineage_events = lineages
        .iter()
        .flat_map(|lineage| prism.lineage_history(lineage))
        .collect::<Vec<_>>();
    lineage_events.sort_by(|left, right| {
        left.meta
            .ts
            .cmp(&right.meta.ts)
            .then_with(|| left.meta.id.0.cmp(&right.meta.id.0))
    });

    let mut co_change = Vec::new();
    let mut validation_checks = Vec::new();
    let projection_snapshot = prism.projection_snapshot();
    for (lineage, records) in projection_snapshot.co_change_by_lineage {
        if lineages.contains(&lineage) {
            co_change.extend(records);
        }
    }
    for (lineage, checks) in projection_snapshot.validation_by_lineage {
        if lineages.contains(&lineage) {
            validation_checks.extend(checks);
        }
    }

    let outcomes = prism.outcomes_for(&focus, budget.max_outcomes);
    let memories = store
        .load_episodic_snapshot()?
        .unwrap_or(EpisodicMemorySnapshot {
            entries: Vec::new(),
        })
        .entries
        .into_iter()
        .filter(|entry| entry.anchors.iter().any(|anchor| focus.contains(anchor)))
        .take(budget.max_memories)
        .collect();

    Ok(CuratorContext {
        graph: CuratorGraphSlice { nodes, edges },
        lineage: CuratorLineageSlice {
            events: lineage_events,
        },
        outcomes,
        memories,
        projections: CuratorProjectionSlice {
            co_change,
            validation_checks,
        },
    })
}

fn focus_nodes(prism: &Prism, focus: &[AnchorRef], limit: usize) -> Vec<Node> {
    let mut node_ids = HashSet::<NodeId>::new();
    for anchor in focus {
        match anchor {
            AnchorRef::Node(id) => {
                node_ids.insert(id.clone());
            }
            AnchorRef::Lineage(lineage) => {
                for node in
                    prism
                        .history_snapshot()
                        .node_to_lineage
                        .iter()
                        .filter_map(|(id, candidate)| {
                            if candidate == lineage {
                                Some(id.clone())
                            } else {
                                None
                            }
                        })
                {
                    node_ids.insert(node);
                }
            }
            _ => {}
        }
    }

    let mut nodes = node_ids
        .into_iter()
        .filter_map(|id| prism.graph().node(&id).cloned())
        .collect::<Vec<_>>();
    nodes.sort_by(|left, right| left.id.path.cmp(&right.id.path));
    if nodes.len() > limit {
        nodes.truncate(limit);
    }
    nodes
}

fn focus_lineages(prism: &Prism, focus: &[AnchorRef]) -> HashSet<prism_ir::LineageId> {
    let mut lineages = HashSet::new();
    for anchor in focus {
        match anchor {
            AnchorRef::Lineage(lineage) => {
                lineages.insert(lineage.clone());
            }
            AnchorRef::Node(node) => {
                if let Some(lineage) = prism.lineage_of(node) {
                    lineages.insert(lineage);
                }
            }
            _ => {}
        }
    }
    lineages
}
