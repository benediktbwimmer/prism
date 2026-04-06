use anyhow::Result;
use prism_coordination::{
    migrate_legacy_hybrid_snapshot_to_canonical_v2, CoordinationRuntimeState,
};
use prism_ir::CoordinationEventKind;

use crate::plan_runtime::NativePlanRuntimeState;
use crate::Prism;

impl Prism {
    pub(crate) fn coordination_transaction<T, F>(&self, mutate: F) -> Result<T>
    where
        F: FnOnce(&mut CoordinationRuntimeState, &mut NativePlanRuntimeState) -> Result<T>,
    {
        let mut runtime = self
            .materialized_runtime
            .write()
            .expect("materialized runtime lock poisoned");
        let before_snapshot = runtime.snapshot();
        let before_plan_runtime = runtime.plan_runtime().clone();

        let result = {
            let (coordination_runtime, plan_runtime) = runtime.runtimes_mut();
            match mutate(coordination_runtime, plan_runtime) {
                Ok(value) => match finalize_coordination_transaction(coordination_runtime, plan_runtime)
                {
                    Ok(()) => Ok(value),
                    Err(error) => Err(error),
                },
                Err(error) => Err(error),
            }
        };

        match result {
            Ok(value) => {
                drop(runtime);
                self.invalidate_plan_discovery_cache();
                Ok(value)
            }
            Err(error) => {
                let failed_snapshot = runtime.snapshot();
                runtime.replace_continuity_snapshot(rollback_snapshot_with_rejections(
                    before_snapshot,
                    &failed_snapshot,
                ));
                *runtime.plan_runtime_mut() = before_plan_runtime;
                Err(error)
            }
        }
    }
}

fn finalize_coordination_transaction(
    coordination_runtime: &mut CoordinationRuntimeState,
    plan_runtime: &mut NativePlanRuntimeState,
) -> Result<()> {
    let snapshot = coordination_runtime.snapshot();
    plan_runtime.sync_task_execution_plan_statuses_from_coordination_snapshot(&snapshot)?;
    let snapshot = plan_runtime.apply_to_coordination_snapshot(snapshot);
    snapshot.validate_canonical_projection()?;
    migrate_legacy_hybrid_snapshot_to_canonical_v2(
        &snapshot,
        &plan_runtime.plan_graphs(),
        &plan_runtime.execution_overlays_by_plan(),
    )?
    .validate_graph()?;
    coordination_runtime.replace_from_snapshot(snapshot);
    Ok(())
}

fn rollback_snapshot_with_rejections(
    mut before_snapshot: prism_coordination::CoordinationSnapshot,
    failed_snapshot: &prism_coordination::CoordinationSnapshot,
) -> prism_coordination::CoordinationSnapshot {
    let rejection_events = failed_snapshot
        .events
        .iter()
        .skip(before_snapshot.events.len())
        .filter(|event| event.kind == CoordinationEventKind::MutationRejected)
        .cloned();
    before_snapshot.events.extend(rejection_events);
    before_snapshot
}
