use std::collections::BTreeMap;

use prism_coordination::coordination_snapshot_from_events;
use prism_ir::{PlanEdgeId, PlanId, PlanNodeId, Timestamp};
use prism_projections::{ProjectionAuthorityPlane, ProjectionClass};

use crate::plan_runtime::NativePlanRuntimeState;
use crate::types::ad_hoc_plan_projection_summary;
use crate::{AdHocPlanProjection, AdHocPlanProjectionDiff, Prism};

impl Prism {
    pub fn plan_projection_at(
        &self,
        plan_id: &PlanId,
        at: Timestamp,
    ) -> Option<AdHocPlanProjection> {
        let events = self
            .coordination_events()
            .into_iter()
            .filter(|event| event.meta.ts <= at)
            .collect::<Vec<_>>();
        let snapshot = coordination_snapshot_from_events(&events, None)?;
        let runtime = NativePlanRuntimeState::from_coordination_snapshot(&snapshot);
        let graph = runtime.plan_graph(plan_id)?;
        let execution_overlays = runtime.plan_execution(plan_id);
        Some(AdHocPlanProjection {
            projection_class: ProjectionClass::AdHoc,
            authority_planes: vec![ProjectionAuthorityPlane::SharedRuntime],
            history_source: "coordination_events".to_string(),
            plan_id: plan_id.clone(),
            as_of: at,
            replayed_event_count: events.len(),
            summary: ad_hoc_plan_projection_summary(&graph),
            graph,
            execution_overlays,
        })
    }

    pub fn plan_projection_diff(
        &self,
        plan_id: &PlanId,
        from: Timestamp,
        to: Timestamp,
    ) -> AdHocPlanProjectionDiff {
        let (from, to) = if from <= to { (from, to) } else { (to, from) };
        let before = self.plan_projection_at(plan_id, from);
        let after = self.plan_projection_at(plan_id, to);
        let (added_nodes, removed_nodes, changed_nodes) =
            diff_nodes(before.as_ref(), after.as_ref());
        let (added_edges, removed_edges, changed_edges) =
            diff_edges(before.as_ref(), after.as_ref());
        let changed_execution_nodes = diff_execution_nodes(before.as_ref(), after.as_ref());

        let plan_metadata_changed = match (&before, &after) {
            (Some(before), Some(after)) => {
                before.graph.title != after.graph.title
                    || before.graph.goal != after.graph.goal
                    || before.graph.status != after.graph.status
                    || before.graph.scope != after.graph.scope
                    || before.graph.kind != after.graph.kind
                    || before.graph.revision != after.graph.revision
                    || before.graph.root_nodes != after.graph.root_nodes
                    || before.graph.tags != after.graph.tags
                    || before.graph.created_from != after.graph.created_from
                    || before.graph.metadata != after.graph.metadata
            }
            (None, None) => false,
            _ => true,
        };

        AdHocPlanProjectionDiff {
            projection_class: ProjectionClass::AdHoc,
            authority_planes: vec![ProjectionAuthorityPlane::SharedRuntime],
            history_source: "coordination_events".to_string(),
            plan_id: plan_id.clone(),
            from,
            to,
            before,
            after,
            plan_metadata_changed,
            added_nodes,
            removed_nodes,
            changed_nodes,
            added_edges,
            removed_edges,
            changed_edges,
            changed_execution_nodes,
        }
    }
}

fn diff_nodes(
    before: Option<&AdHocPlanProjection>,
    after: Option<&AdHocPlanProjection>,
) -> (Vec<PlanNodeId>, Vec<PlanNodeId>, Vec<PlanNodeId>) {
    let before_nodes = before
        .map(|projection| {
            projection
                .graph
                .nodes
                .iter()
                .map(|node| (node.id.clone(), node))
                .collect::<BTreeMap<_, _>>()
        })
        .unwrap_or_default();
    let after_nodes = after
        .map(|projection| {
            projection
                .graph
                .nodes
                .iter()
                .map(|node| (node.id.clone(), node))
                .collect::<BTreeMap<_, _>>()
        })
        .unwrap_or_default();

    let added = after_nodes
        .keys()
        .filter(|node_id| !before_nodes.contains_key(*node_id))
        .cloned()
        .collect::<Vec<_>>();
    let removed = before_nodes
        .keys()
        .filter(|node_id| !after_nodes.contains_key(*node_id))
        .cloned()
        .collect::<Vec<_>>();
    let changed = after_nodes
        .iter()
        .filter_map(|(node_id, after_node)| {
            before_nodes
                .get(node_id)
                .filter(|before_node| *before_node != after_node)
                .map(|_| node_id.clone())
        })
        .collect::<Vec<_>>();

    (added, removed, changed)
}

fn diff_edges(
    before: Option<&AdHocPlanProjection>,
    after: Option<&AdHocPlanProjection>,
) -> (Vec<PlanEdgeId>, Vec<PlanEdgeId>, Vec<PlanEdgeId>) {
    let before_edges = before
        .map(|projection| {
            projection
                .graph
                .edges
                .iter()
                .map(|edge| (edge.id.clone(), edge))
                .collect::<BTreeMap<_, _>>()
        })
        .unwrap_or_default();
    let after_edges = after
        .map(|projection| {
            projection
                .graph
                .edges
                .iter()
                .map(|edge| (edge.id.clone(), edge))
                .collect::<BTreeMap<_, _>>()
        })
        .unwrap_or_default();

    let added = after_edges
        .keys()
        .filter(|edge_id| !before_edges.contains_key(*edge_id))
        .cloned()
        .collect::<Vec<_>>();
    let removed = before_edges
        .keys()
        .filter(|edge_id| !after_edges.contains_key(*edge_id))
        .cloned()
        .collect::<Vec<_>>();
    let changed = after_edges
        .iter()
        .filter_map(|(edge_id, after_edge)| {
            before_edges
                .get(edge_id)
                .filter(|before_edge| *before_edge != after_edge)
                .map(|_| edge_id.clone())
        })
        .collect::<Vec<_>>();

    (added, removed, changed)
}

fn diff_execution_nodes(
    before: Option<&AdHocPlanProjection>,
    after: Option<&AdHocPlanProjection>,
) -> Vec<PlanNodeId> {
    let before_overlays = before
        .map(|projection| {
            projection
                .execution_overlays
                .iter()
                .map(|overlay| (overlay.node_id.clone(), overlay))
                .collect::<BTreeMap<_, _>>()
        })
        .unwrap_or_default();
    let after_overlays = after
        .map(|projection| {
            projection
                .execution_overlays
                .iter()
                .map(|overlay| (overlay.node_id.clone(), overlay))
                .collect::<BTreeMap<_, _>>()
        })
        .unwrap_or_default();

    let mut changed = after_overlays
        .keys()
        .filter(|node_id| !before_overlays.contains_key(*node_id))
        .cloned()
        .collect::<Vec<_>>();
    changed.extend(
        before_overlays
            .keys()
            .filter(|node_id| !after_overlays.contains_key(*node_id))
            .cloned(),
    );
    changed.extend(
        after_overlays
            .iter()
            .filter_map(|(node_id, after_overlay)| {
                before_overlays
                    .get(node_id)
                    .filter(|before_overlay| *before_overlay != after_overlay)
                    .map(|_| node_id.clone())
            }),
    );
    changed.sort_by(|left, right| left.0.cmp(&right.0));
    changed.dedup();
    changed
}
