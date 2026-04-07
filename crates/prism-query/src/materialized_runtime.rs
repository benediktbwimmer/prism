use anyhow::Result;
use prism_coordination::{
    CoordinationRuntimeState, CoordinationSnapshot, CoordinationSnapshotV2, RuntimeDescriptor,
};

use crate::plan_runtime::NativePlanRuntimeState;

pub(crate) struct MaterializedCoordinationRuntime {
    continuity_runtime: CoordinationRuntimeState,
    canonical_snapshot_v2: CoordinationSnapshotV2,
    plan_runtime: NativePlanRuntimeState,
    runtime_descriptors: Vec<RuntimeDescriptor>,
}

impl MaterializedCoordinationRuntime {
    pub(crate) fn from_snapshot(snapshot: CoordinationSnapshot) -> Self {
        Self::from_snapshot_with_runtime_descriptors(snapshot, Vec::new())
    }

    pub(crate) fn from_snapshot_with_runtime_descriptors(
        snapshot: CoordinationSnapshot,
        runtime_descriptors: Vec<RuntimeDescriptor>,
    ) -> Self {
        let plan_runtime = NativePlanRuntimeState::from_coordination_snapshot(&snapshot);
        let canonical_snapshot_v2 = snapshot.to_canonical_snapshot_v2();
        let continuity_runtime = CoordinationRuntimeState::from_snapshot_with_runtime_descriptors(
            snapshot,
            runtime_descriptors.clone(),
        );
        Self {
            continuity_runtime,
            canonical_snapshot_v2,
            plan_runtime,
            runtime_descriptors,
        }
    }

    pub(crate) fn from_snapshot_with_canonical_and_runtime_descriptors(
        snapshot: CoordinationSnapshot,
        canonical_snapshot_v2: CoordinationSnapshotV2,
        runtime_descriptors: Vec<RuntimeDescriptor>,
    ) -> Self {
        let plan_revisions = snapshot
            .plans
            .iter()
            .map(|plan| (plan.id.0.to_string(), plan.revision))
            .collect();
        let plan_runtime =
            NativePlanRuntimeState::from_canonical_snapshot(canonical_snapshot_v2.clone(), plan_revisions);
        let continuity_runtime = CoordinationRuntimeState::from_snapshot_with_runtime_descriptors(
            snapshot,
            runtime_descriptors.clone(),
        );
        Self {
            continuity_runtime,
            canonical_snapshot_v2,
            plan_runtime,
            runtime_descriptors,
        }
    }

    pub(crate) fn snapshot(&self) -> CoordinationSnapshot {
        self.continuity_runtime.snapshot()
    }

    pub(crate) fn snapshot_v2(&self) -> CoordinationSnapshotV2 {
        self.canonical_snapshot_v2.clone()
    }

    pub(crate) fn plan_runtime(&self) -> &NativePlanRuntimeState {
        &self.plan_runtime
    }

    pub(crate) fn continuity_runtime(&self) -> &CoordinationRuntimeState {
        &self.continuity_runtime
    }

    pub(crate) fn continuity_runtime_mut(&mut self) -> &mut CoordinationRuntimeState {
        &mut self.continuity_runtime
    }

    pub(crate) fn runtime_descriptors(&self) -> &[RuntimeDescriptor] {
        &self.runtime_descriptors
    }

    pub(crate) fn replace_runtime_descriptors(
        &mut self,
        runtime_descriptors: Vec<RuntimeDescriptor>,
    ) {
        self.runtime_descriptors = runtime_descriptors.clone();
        self.continuity_runtime
            .replace_runtime_descriptors(runtime_descriptors);
    }

    pub(crate) fn replace_from_snapshot(&mut self, snapshot: CoordinationSnapshot) {
        *self = Self::from_snapshot_with_runtime_descriptors(
            snapshot,
            self.runtime_descriptors.clone(),
        );
    }

    pub(crate) fn replace_from_snapshot_with_canonical(
        &mut self,
        snapshot: CoordinationSnapshot,
        canonical_snapshot_v2: CoordinationSnapshotV2,
    ) {
        *self = Self::from_snapshot_with_canonical_and_runtime_descriptors(
            snapshot,
            canonical_snapshot_v2,
            self.runtime_descriptors.clone(),
        );
    }

    pub(crate) fn replace_continuity_snapshot(&mut self, snapshot: CoordinationSnapshot) {
        self.replace_from_snapshot(snapshot);
    }

    pub(crate) fn persist_coordination_snapshot(
        &mut self,
        snapshot: CoordinationSnapshot,
    ) -> Result<()> {
        self.replace_from_snapshot(snapshot);
        Ok(())
    }
}
