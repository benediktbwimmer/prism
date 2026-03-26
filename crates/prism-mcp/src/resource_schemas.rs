use prism_js::{
    ChangeImpactView, CoChangeView, EdgeView, LineageEventView, LineageStatus, LineageView,
    MemoryEntryView, QueryDiagnostic, RelationsView, SymbolView, TaskJournalView,
    ValidationRecipeView,
};
use rmcp::schemars::JsonSchema;

use crate::OutcomeEvent;

#[derive(Debug, Clone, serde::Serialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub(crate) struct SessionLimitsView {
    pub(crate) max_result_nodes: usize,
    pub(crate) max_call_graph_depth: usize,
    pub(crate) max_output_json_bytes: usize,
}

#[derive(Debug, Clone, serde::Serialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub(crate) struct SessionTaskView {
    pub(crate) task_id: String,
    pub(crate) description: Option<String>,
    pub(crate) tags: Vec<String>,
}

#[derive(Debug, Clone, serde::Serialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub(crate) struct CoordinationFeaturesView {
    pub(crate) workflow: bool,
    pub(crate) claims: bool,
    pub(crate) artifacts: bool,
}

#[derive(Debug, Clone, serde::Serialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub(crate) struct FeatureFlagsView {
    pub(crate) mode: String,
    pub(crate) coordination: CoordinationFeaturesView,
}

#[derive(Debug, Clone, serde::Serialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub(crate) struct SessionView {
    pub(crate) workspace_root: Option<String>,
    pub(crate) current_task: Option<SessionTaskView>,
    pub(crate) current_agent: Option<String>,
    pub(crate) limits: SessionLimitsView,
    pub(crate) features: FeatureFlagsView,
}

#[derive(Debug, Clone, serde::Serialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub(crate) struct ResourcePageView {
    pub(crate) cursor: Option<String>,
    pub(crate) next_cursor: Option<String>,
    pub(crate) limit: usize,
    pub(crate) returned: usize,
    pub(crate) total: usize,
    pub(crate) has_more: bool,
    pub(crate) limit_capped: bool,
}

#[derive(Debug, Clone, serde::Serialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub(crate) struct InferredEdgeRecordView {
    pub(crate) id: String,
    pub(crate) edge: EdgeView,
    pub(crate) scope: String,
    pub(crate) task_id: Option<String>,
    pub(crate) evidence: Vec<String>,
}

#[derive(Debug, Clone, serde::Serialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub(crate) struct ResourceLinkView {
    pub(crate) uri: String,
    pub(crate) name: String,
    pub(crate) description: Option<String>,
}

#[derive(Debug, Clone, serde::Serialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub(crate) struct SessionResourcePayload {
    pub(crate) uri: String,
    pub(crate) schema_uri: String,
    pub(crate) workspace_root: Option<String>,
    pub(crate) current_task: Option<SessionTaskView>,
    pub(crate) current_agent: Option<String>,
    pub(crate) limits: SessionLimitsView,
    pub(crate) features: FeatureFlagsView,
    pub(crate) related_resources: Vec<ResourceLinkView>,
}

#[derive(Debug, Clone, serde::Serialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub(crate) struct EntrypointsResourcePayload {
    pub(crate) uri: String,
    pub(crate) schema_uri: String,
    pub(crate) entrypoints: Vec<SymbolView>,
    pub(crate) page: ResourcePageView,
    pub(crate) truncated: bool,
    pub(crate) diagnostics: Vec<QueryDiagnostic>,
    pub(crate) related_resources: Vec<ResourceLinkView>,
}

#[derive(Debug, Clone, serde::Serialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub(crate) struct SearchResourcePayload {
    pub(crate) uri: String,
    pub(crate) schema_uri: String,
    pub(crate) query: String,
    pub(crate) results: Vec<SymbolView>,
    pub(crate) page: ResourcePageView,
    pub(crate) truncated: bool,
    pub(crate) diagnostics: Vec<QueryDiagnostic>,
    pub(crate) related_resources: Vec<ResourceLinkView>,
}

#[derive(Debug, Clone, serde::Serialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub(crate) struct SymbolResourcePayload {
    pub(crate) uri: String,
    pub(crate) schema_uri: String,
    pub(crate) symbol: SymbolView,
    pub(crate) relations: RelationsView,
    pub(crate) lineage: Option<LineageView>,
    pub(crate) co_change_neighbors: Vec<CoChangeView>,
    pub(crate) related_failures: Vec<OutcomeEvent>,
    pub(crate) blast_radius: ChangeImpactView,
    pub(crate) validation_recipe: ValidationRecipeView,
    pub(crate) diagnostics: Vec<QueryDiagnostic>,
    pub(crate) related_resources: Vec<ResourceLinkView>,
}

#[derive(Debug, Clone, serde::Serialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub(crate) struct LineageResourcePayload {
    pub(crate) uri: String,
    pub(crate) schema_uri: String,
    pub(crate) lineage_id: String,
    pub(crate) status: LineageStatus,
    pub(crate) current_nodes: Vec<SymbolView>,
    pub(crate) current_nodes_truncated: bool,
    pub(crate) history: Vec<LineageEventView>,
    pub(crate) history_page: ResourcePageView,
    pub(crate) truncated: bool,
    pub(crate) co_change_neighbors: Vec<CoChangeView>,
    pub(crate) diagnostics: Vec<QueryDiagnostic>,
    pub(crate) related_resources: Vec<ResourceLinkView>,
}

#[derive(Debug, Clone, serde::Serialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub(crate) struct TaskResourcePayload {
    pub(crate) uri: String,
    pub(crate) schema_uri: String,
    pub(crate) task_id: String,
    pub(crate) journal: TaskJournalView,
    pub(crate) events: Vec<OutcomeEvent>,
    pub(crate) page: ResourcePageView,
    pub(crate) truncated: bool,
    pub(crate) related_resources: Vec<ResourceLinkView>,
}

#[derive(Debug, Clone, serde::Serialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub(crate) struct EventResourcePayload {
    pub(crate) uri: String,
    pub(crate) schema_uri: String,
    pub(crate) event: OutcomeEvent,
    pub(crate) related_resources: Vec<ResourceLinkView>,
}

#[derive(Debug, Clone, serde::Serialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub(crate) struct MemoryResourcePayload {
    pub(crate) uri: String,
    pub(crate) schema_uri: String,
    pub(crate) memory: MemoryEntryView,
    pub(crate) task_id: Option<String>,
    pub(crate) related_resources: Vec<ResourceLinkView>,
}

#[derive(Debug, Clone, serde::Serialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub(crate) struct EdgeResourcePayload {
    pub(crate) uri: String,
    pub(crate) schema_uri: String,
    pub(crate) edge: InferredEdgeRecordView,
    pub(crate) related_resources: Vec<ResourceLinkView>,
}

#[derive(Debug, Clone, serde::Serialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub(crate) struct ResourceSchemaCatalogEntry {
    pub(crate) resource_kind: String,
    pub(crate) schema_uri: String,
    pub(crate) resource_uri: Option<String>,
    pub(crate) description: String,
}

#[derive(Debug, Clone, serde::Serialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub(crate) struct ResourceSchemaCatalogPayload {
    pub(crate) uri: String,
    pub(crate) schema_uri: String,
    pub(crate) schemas: Vec<ResourceSchemaCatalogEntry>,
    pub(crate) related_resources: Vec<ResourceLinkView>,
}
