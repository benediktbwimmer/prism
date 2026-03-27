use prism_js::{QueryLogEntryView, QueryTraceView, RuntimeLogEventView, RuntimeStatusView};
use serde::Serialize;

use crate::SessionView;

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct DashboardSummaryView {
    pub(crate) session: SessionView,
    pub(crate) runtime: RuntimeStatusView,
    pub(crate) active_query_count: usize,
    pub(crate) active_mutation_count: usize,
    pub(crate) recent_query_error_count: usize,
    pub(crate) last_runtime_event: Option<RuntimeLogEventView>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct DashboardBootstrapView {
    pub(crate) summary: DashboardSummaryView,
    pub(crate) operations: DashboardOperationsView,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct DashboardOperationsView {
    pub(crate) active: Vec<ActiveOperationView>,
    pub(crate) recent_queries: Vec<QueryLogEntryView>,
    pub(crate) recent_mutations: Vec<MutationLogEntryView>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct ActiveOperationView {
    pub(crate) id: String,
    pub(crate) kind: String,
    pub(crate) label: String,
    pub(crate) started_at: u64,
    pub(crate) session_id: String,
    pub(crate) task_id: Option<String>,
    pub(crate) status: String,
    pub(crate) phase: Option<String>,
    pub(crate) touched: Vec<String>,
    pub(crate) error: Option<String>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct MutationLogEntryView {
    pub(crate) id: String,
    pub(crate) action: String,
    pub(crate) started_at: u64,
    pub(crate) duration_ms: u64,
    pub(crate) session_id: String,
    pub(crate) task_id: Option<String>,
    pub(crate) success: bool,
    pub(crate) error: Option<String>,
    pub(crate) result_ids: Vec<String>,
    pub(crate) violation_count: usize,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct MutationTraceView {
    pub(crate) entry: MutationLogEntryView,
}

#[derive(Debug, Clone, Serialize)]
#[serde(tag = "kind", rename_all = "camelCase")]
pub(crate) enum DashboardOperationDetailView {
    Active { operation: ActiveOperationView },
    Query { trace: QueryTraceView },
    Mutation { trace: MutationTraceView },
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct DashboardEventEnvelope {
    pub(crate) id: u64,
    pub(crate) event: String,
    pub(crate) data: serde_json::Value,
}
