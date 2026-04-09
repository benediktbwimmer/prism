use anyhow::Result;
use prism_coordination::{
    CoordinationRuntimeState, CoordinationSnapshot, CoordinationSnapshotV2, RuntimeDescriptor,
};

pub(crate) struct MaterializedCoordinationRuntime {
    continuity_runtime: CoordinationRuntimeState,
    canonical_snapshot_v2: CoordinationSnapshotV2,
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
        let canonical_snapshot_v2 = snapshot.to_canonical_snapshot_v2();
        Self::from_snapshots_with_runtime_descriptors(
            snapshot,
            canonical_snapshot_v2,
            runtime_descriptors,
        )
    }

    pub(crate) fn from_snapshots_with_runtime_descriptors(
        snapshot: CoordinationSnapshot,
        canonical_snapshot_v2: CoordinationSnapshotV2,
        runtime_descriptors: Vec<RuntimeDescriptor>,
    ) -> Self {
        let continuity_runtime = CoordinationRuntimeState::from_snapshot_with_runtime_descriptors(
            snapshot,
            runtime_descriptors.clone(),
        );
        Self {
            continuity_runtime,
            canonical_snapshot_v2,
            runtime_descriptors,
        }
    }

    pub(crate) fn snapshot(&self) -> CoordinationSnapshot {
        self.continuity_runtime.snapshot()
    }

    pub(crate) fn snapshot_v2(&self) -> CoordinationSnapshotV2 {
        self.canonical_snapshot_v2.clone()
    }

    pub(crate) fn refresh_canonical_snapshot_v2(&mut self) -> CoordinationSnapshot {
        let snapshot = self.continuity_runtime.snapshot();
        self.canonical_snapshot_v2 = snapshot.to_canonical_snapshot_v2();
        snapshot
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

    pub(crate) fn replace_from_snapshots(
        &mut self,
        snapshot: CoordinationSnapshot,
        canonical_snapshot_v2: CoordinationSnapshotV2,
    ) {
        *self = Self::from_snapshots_with_runtime_descriptors(
            snapshot,
            canonical_snapshot_v2,
            self.runtime_descriptors.clone(),
        );
    }

    pub(crate) fn persist_coordination_snapshot(
        &mut self,
        snapshot: CoordinationSnapshot,
    ) -> Result<()> {
        self.continuity_runtime
            .replace_from_snapshot_with_runtime_descriptors(
                snapshot,
                self.runtime_descriptors.clone(),
            );
        self.refresh_canonical_snapshot_v2();
        Ok(())
    }

}
