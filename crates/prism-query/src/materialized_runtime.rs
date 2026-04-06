use std::collections::BTreeMap;

use anyhow::Result;
use prism_coordination::{CoordinationRuntimeState, CoordinationSnapshot};
use prism_ir::{PlanExecutionOverlay, PlanGraph};

use crate::plan_runtime::NativePlanRuntimeState;

pub(crate) struct MaterializedCoordinationRuntime {
    continuity_runtime: CoordinationRuntimeState,
    plan_runtime: NativePlanRuntimeState,
}

impl MaterializedCoordinationRuntime {
    pub(crate) fn from_snapshot(snapshot: CoordinationSnapshot) -> Self {
        let plan_runtime = NativePlanRuntimeState::from_coordination_snapshot(&snapshot);
        let continuity_runtime = CoordinationRuntimeState::from_snapshot(snapshot);
        Self {
            continuity_runtime,
            plan_runtime,
        }
    }

    pub(crate) fn from_snapshot_with_graphs_and_overlays(
        snapshot: CoordinationSnapshot,
        plan_graphs: Vec<PlanGraph>,
        execution_overlays: BTreeMap<String, Vec<PlanExecutionOverlay>>,
    ) -> Self {
        let plan_runtime = NativePlanRuntimeState::from_snapshot_with_graphs_and_overlays(
            &snapshot,
            plan_graphs,
            execution_overlays,
        );
        let continuity_runtime = CoordinationRuntimeState::from_snapshot(
            plan_runtime.apply_task_execution_authored_fields_to_coordination_snapshot(snapshot),
        );
        Self {
            continuity_runtime,
            plan_runtime,
        }
    }

    pub(crate) fn snapshot(&self) -> CoordinationSnapshot {
        self.continuity_runtime.snapshot()
    }

    pub(crate) fn plan_runtime(&self) -> &NativePlanRuntimeState {
        &self.plan_runtime
    }

    pub(crate) fn plan_runtime_mut(&mut self) -> &mut NativePlanRuntimeState {
        &mut self.plan_runtime
    }

    pub(crate) fn continuity_runtime(&self) -> &CoordinationRuntimeState {
        &self.continuity_runtime
    }

    pub(crate) fn continuity_runtime_mut(&mut self) -> &mut CoordinationRuntimeState {
        &mut self.continuity_runtime
    }

    pub(crate) fn replace_from_snapshot(&mut self, snapshot: CoordinationSnapshot) {
        *self = Self::from_snapshot(snapshot);
    }

    pub(crate) fn replace_from_snapshot_with_graphs_and_overlays(
        &mut self,
        snapshot: CoordinationSnapshot,
        plan_graphs: Vec<PlanGraph>,
        execution_overlays: BTreeMap<String, Vec<PlanExecutionOverlay>>,
    ) {
        *self =
            Self::from_snapshot_with_graphs_and_overlays(snapshot, plan_graphs, execution_overlays);
    }

    pub(crate) fn replace_continuity_snapshot(&mut self, snapshot: CoordinationSnapshot) {
        self.continuity_runtime.replace_from_snapshot(snapshot);
    }

    pub(crate) fn refresh_plan_runtime_from_coordination(&mut self) {
        *self = Self::from_snapshot(self.snapshot());
    }

    pub(crate) fn apply_plan_runtime_to_current_snapshot(&mut self) {
        let snapshot = self
            .plan_runtime
            .apply_to_coordination_snapshot(self.snapshot());
        self.continuity_runtime.replace_from_snapshot(snapshot);
    }

    pub(crate) fn persist_coordination_snapshot(
        &mut self,
        snapshot: CoordinationSnapshot,
    ) -> Result<()> {
        self.plan_runtime
            .sync_task_execution_plan_statuses_from_coordination_snapshot(&snapshot)?;
        let snapshot = self.plan_runtime.apply_to_coordination_snapshot(snapshot);
        self.continuity_runtime.replace_from_snapshot(snapshot);
        Ok(())
    }
}
