use prism_coordination::{CoordinationSnapshot, Plan};
use prism_ir::PlanEdgeKind;

pub(crate) fn sanitize_persisted_coordination_snapshot(
    mut snapshot: CoordinationSnapshot,
) -> CoordinationSnapshot {
    snapshot.plans = snapshot.plans.into_iter().map(sanitize_plan).collect();
    snapshot
}

pub(crate) fn sanitize_plan(mut plan: Plan) -> Plan {
    plan.root_tasks.clear();
    plan.authored_edges
        .retain(|edge| edge.kind == PlanEdgeKind::DependsOn);
    plan
}
