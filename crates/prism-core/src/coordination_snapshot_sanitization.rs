use prism_coordination::{CoordinationSnapshot, Plan};

pub(crate) fn sanitize_persisted_coordination_snapshot(
    mut snapshot: CoordinationSnapshot,
) -> CoordinationSnapshot {
    snapshot.plans = snapshot.plans.into_iter().map(sanitize_plan).collect();
    snapshot
}

pub(crate) fn sanitize_plan(plan: Plan) -> Plan {
    plan
}
