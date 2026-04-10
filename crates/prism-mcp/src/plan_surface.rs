use anyhow::Result;
use prism_ir::PlanId;
use prism_js::{CoordinationPlanV2View, PlanListEntryView, PlanSummaryView};

use crate::spec_surface::linked_plan_view;
use crate::ui_read_models::{
    filtered_plan_entries_from_snapshot, plan_entries_from_snapshot, PlansResourceSort,
};
use crate::{plan_summary_view, QueryHost};

pub(crate) fn all_plan_entries(host: &QueryHost) -> Result<Vec<PlanListEntryView>> {
    let snapshot = host.current_coordination_snapshot_v2()?;
    Ok(plan_entries_from_snapshot(&snapshot))
}

pub(crate) fn filtered_plan_resource_entries(
    host: &QueryHost,
    status: Option<prism_ir::PlanStatus>,
    scope: Option<prism_ir::PlanScope>,
    contains: Option<&str>,
    sort: PlansResourceSort,
) -> Result<Vec<PlanListEntryView>> {
    let snapshot = host.current_coordination_snapshot_v2()?;
    Ok(filtered_plan_entries_from_snapshot(
        &snapshot, status, scope, contains, sort,
    ))
}

pub(crate) struct PlanResourceSurface {
    pub(crate) plan: CoordinationPlanV2View,
    pub(crate) summary: Option<PlanSummaryView>,
}

pub(crate) fn linked_plan_resource(
    host: &QueryHost,
    plan_id: &PlanId,
) -> Result<Option<PlanResourceSurface>> {
    let prism = host.current_prism();
    let Some(plan) = linked_plan_view(host, plan_id)? else {
        return Ok(None);
    };
    let summary = prism.plan_summary(plan_id).map(plan_summary_view);
    Ok(Some(PlanResourceSurface { plan, summary }))
}
