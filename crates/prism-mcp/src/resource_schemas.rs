pub(crate) use prism_js::{
    AnchorRefView, ChangeImpactView, CoChangeView, ContractKindView, ContractPacketView,
    ContractStatusView, DiscoveryBundleView, EdgeView, EditContextView, LineageEventView,
    LineageStatus, LineageView, MemoryEntryView, MemoryEventView, OwnerCandidateView,
    PlanListEntryView, PlanSummaryView, PlanView, QueryDiagnostic, ReadContextView, RelationsView,
    SourceExcerptView, SuggestedQueryView, SymbolView, TaskJournalView, ValidationRecipeView,
    WorkspaceRevisionView,
};
use rmcp::schemars::JsonSchema;
use serde_json::Value;

use crate::OutcomeEvent;
use prism_ir::WorkContextKind;

#[derive(Debug, Clone, serde::Serialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub(crate) struct SessionLimitsView {
    pub(crate) max_result_nodes: usize,
    pub(crate) max_call_graph_depth: usize,
    pub(crate) max_output_json_bytes: usize,
}

#[derive(Debug, Clone, serde::Serialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub(crate) struct SessionRepairActionView {
    pub(crate) tool: String,
    pub(crate) input: Value,
    pub(crate) label: String,
}

#[derive(Debug, Clone, serde::Serialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub(crate) struct SessionTaskView {
    pub(crate) task_id: String,
    pub(crate) description: Option<String>,
    pub(crate) tags: Vec<String>,
    pub(crate) coordination_task_id: Option<String>,
    pub(crate) context_status: String,
    pub(crate) context_summary: String,
    pub(crate) next_action: String,
    pub(crate) repair_action: Option<SessionRepairActionView>,
}

#[derive(Debug, Clone, serde::Serialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub(crate) struct SessionWorkView {
    pub(crate) work_id: String,
    pub(crate) kind: WorkContextKind,
    pub(crate) title: String,
    pub(crate) summary: Option<String>,
    pub(crate) parent_work_id: Option<String>,
    pub(crate) coordination_task_id: Option<String>,
    pub(crate) plan_id: Option<String>,
    pub(crate) plan_title: Option<String>,
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
    pub(crate) internal_developer: bool,
}

#[derive(Debug, Clone, serde::Serialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub(crate) struct SessionView {
    pub(crate) workspace_root: Option<String>,
    pub(crate) current_task: Option<SessionTaskView>,
    pub(crate) current_work: Option<SessionWorkView>,
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
    pub(crate) current_work: Option<SessionWorkView>,
    pub(crate) current_agent: Option<String>,
    pub(crate) limits: SessionLimitsView,
    pub(crate) features: FeatureFlagsView,
    pub(crate) related_resources: Vec<ResourceLinkView>,
}

#[derive(Debug, Clone, serde::Serialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub(crate) struct CapabilitiesBuildInfoView {
    pub(crate) server_name: String,
    pub(crate) server_version: String,
    pub(crate) protocol_version: String,
    pub(crate) workspace_revision: WorkspaceRevisionView,
    pub(crate) api_reference_uri: String,
}

#[derive(Debug, Clone, serde::Serialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub(crate) struct QueryMethodCapabilityView {
    pub(crate) name: String,
    pub(crate) enabled: bool,
    pub(crate) group: String,
    pub(crate) feature_gate: Option<String>,
    pub(crate) description: String,
}

#[derive(Debug, Clone, serde::Serialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub(crate) struct QueryViewCapabilityView {
    pub(crate) name: String,
    pub(crate) enabled: bool,
    pub(crate) feature_flag: String,
    pub(crate) stability: String,
    pub(crate) owner: String,
    pub(crate) description: String,
}

#[derive(Debug, Clone, serde::Serialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub(crate) struct ResourceCapabilityView {
    pub(crate) name: String,
    pub(crate) uri: String,
    pub(crate) mime_type: String,
    pub(crate) description: String,
    pub(crate) schema_uri: Option<String>,
    pub(crate) example_uri: Option<String>,
}

#[derive(Debug, Clone, serde::Serialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub(crate) struct ResourceTemplateCapabilityView {
    pub(crate) name: String,
    pub(crate) uri_template: String,
    pub(crate) mime_type: String,
    pub(crate) description: String,
    pub(crate) example_uri: Option<String>,
}

#[derive(Debug, Clone, serde::Serialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub(crate) struct ToolCapabilityView {
    pub(crate) name: String,
    pub(crate) description: String,
    pub(crate) schema_uri: String,
    pub(crate) example_input: Value,
}

#[derive(Debug, Clone, serde::Serialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub(crate) struct CapabilitiesResourcePayload {
    pub(crate) uri: String,
    pub(crate) schema_uri: String,
    pub(crate) build: CapabilitiesBuildInfoView,
    pub(crate) features: FeatureFlagsView,
    pub(crate) query_methods: Vec<QueryMethodCapabilityView>,
    pub(crate) query_views: Vec<QueryViewCapabilityView>,
    pub(crate) resources: Vec<ResourceCapabilityView>,
    pub(crate) resource_templates: Vec<ResourceTemplateCapabilityView>,
    pub(crate) tools: Vec<ToolCapabilityView>,
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
pub(crate) struct FileResourcePayload {
    pub(crate) uri: String,
    pub(crate) schema_uri: String,
    pub(crate) workspace_revision: WorkspaceRevisionView,
    pub(crate) path: String,
    pub(crate) excerpt: SourceExcerptView,
    pub(crate) related_resources: Vec<ResourceLinkView>,
}

#[derive(Debug, Clone, serde::Serialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub(crate) struct PlansResourcePayload {
    pub(crate) uri: String,
    pub(crate) schema_uri: String,
    pub(crate) workspace_revision: WorkspaceRevisionView,
    pub(crate) status: Option<String>,
    pub(crate) scope: Option<String>,
    pub(crate) contains: Option<String>,
    pub(crate) plans: Vec<PlanListEntryView>,
    pub(crate) page: ResourcePageView,
    pub(crate) truncated: bool,
    pub(crate) diagnostics: Vec<QueryDiagnostic>,
    pub(crate) related_resources: Vec<ResourceLinkView>,
}

#[derive(Debug, Clone, serde::Serialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub(crate) struct PlanResourcePayload {
    pub(crate) uri: String,
    pub(crate) schema_uri: String,
    pub(crate) workspace_revision: WorkspaceRevisionView,
    pub(crate) plan: PlanView,
    pub(crate) summary: Option<PlanSummaryView>,
    pub(crate) related_resources: Vec<ResourceLinkView>,
}

#[derive(Debug, Clone, serde::Serialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub(crate) struct VocabularyValueView {
    pub(crate) value: String,
    pub(crate) aliases: Vec<String>,
    pub(crate) description: String,
}

#[derive(Debug, Clone, serde::Serialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub(crate) struct VocabularyCategoryView {
    pub(crate) key: String,
    pub(crate) title: String,
    pub(crate) description: String,
    pub(crate) values: Vec<VocabularyValueView>,
}

#[derive(Debug, Clone, serde::Serialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub(crate) struct VocabularyResourcePayload {
    pub(crate) uri: String,
    pub(crate) schema_uri: String,
    pub(crate) vocabularies: Vec<VocabularyCategoryView>,
    pub(crate) related_resources: Vec<ResourceLinkView>,
}

#[derive(Debug, Clone, serde::Serialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub(crate) struct ContractsResourcePayload {
    pub(crate) uri: String,
    pub(crate) schema_uri: String,
    pub(crate) workspace_revision: WorkspaceRevisionView,
    pub(crate) contains: Option<String>,
    pub(crate) status: Option<String>,
    pub(crate) scope: Option<String>,
    pub(crate) kind: Option<String>,
    pub(crate) contracts: Vec<ContractPacketView>,
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
    pub(crate) workspace_revision: WorkspaceRevisionView,
    pub(crate) query: String,
    pub(crate) strategy: String,
    pub(crate) owner_kind: Option<String>,
    pub(crate) kind: Option<String>,
    pub(crate) path: Option<String>,
    pub(crate) module: Option<String>,
    pub(crate) task_id: Option<String>,
    pub(crate) path_mode: Option<String>,
    pub(crate) structured_path: Option<String>,
    pub(crate) top_level_only: Option<bool>,
    pub(crate) prefer_callable_code: Option<bool>,
    pub(crate) prefer_editable_targets: Option<bool>,
    pub(crate) prefer_behavioral_owners: Option<bool>,
    pub(crate) include_inferred: bool,
    pub(crate) suggested_reads: Vec<OwnerCandidateView>,
    pub(crate) results: Vec<SymbolView>,
    pub(crate) discovery: Option<DiscoveryBundleView>,
    pub(crate) top_read_context: Option<ReadContextView>,
    pub(crate) ambiguity: Option<crate::SearchAmbiguityView>,
    pub(crate) suggested_queries: Vec<SuggestedQueryView>,
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
    pub(crate) workspace_revision: WorkspaceRevisionView,
    pub(crate) symbol: SymbolView,
    pub(crate) discovery: DiscoveryBundleView,
    pub(crate) suggested_reads: Vec<OwnerCandidateView>,
    pub(crate) read_context: ReadContextView,
    pub(crate) edit_context: EditContextView,
    pub(crate) suggested_queries: Vec<SuggestedQueryView>,
    pub(crate) relations: RelationsView,
    pub(crate) spec_cluster: Option<prism_js::SpecImplementationClusterView>,
    pub(crate) spec_drift: Option<prism_js::SpecDriftExplanationView>,
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
    pub(crate) history: Vec<MemoryEventView>,
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
    pub(crate) example_uri: Option<String>,
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
