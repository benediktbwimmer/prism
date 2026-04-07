use anyhow::Result;
use prism_coordination::{CoordinationRuntimeState, CoordinationSnapshot};
use prism_ir::CoordinationEventKind;

use crate::Prism;

impl Prism {
    pub(crate) fn coordination_transaction<T, F>(&self, mutate: F) -> Result<T>
    where
        F: FnOnce(&mut CoordinationRuntimeState) -> Result<T>,
    {
        let mut runtime = self
            .materialized_runtime
            .write()
            .expect("materialized runtime lock poisoned");
        let before_snapshot = runtime.snapshot();

        let result = {
            let coordination_runtime = runtime.continuity_runtime_mut();
            match mutate(coordination_runtime) {
                Ok(value) => match finalize_coordination_transaction(coordination_runtime) {
                    Ok(snapshot) => {
                        runtime.replace_from_snapshot(snapshot);
                        Ok(value)
                    }
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
                Err(error)
            }
        }
    }
}

fn finalize_coordination_transaction(
    coordination_runtime: &mut CoordinationRuntimeState,
) -> Result<CoordinationSnapshot> {
    let snapshot = coordination_runtime.snapshot();
    snapshot.validate_canonical_projection()?;
    snapshot.to_canonical_snapshot_v2().validate_graph()?;
    Ok(snapshot)
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
