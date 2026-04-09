use prism_ir::PlanId;

use crate::coordination_query_engine::CoordinationQueryEngine;
use crate::{PlanSummary, Prism};

impl Prism {
    pub fn plan_summary(&self, plan_id: &PlanId) -> Option<PlanSummary> {
        CoordinationQueryEngine::new(self).plan_summary(plan_id)
    }
}
