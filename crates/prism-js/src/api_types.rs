use prism_coordination::{ArtifactRequirement, BlockerKind, ReviewRequirement, ReviewerClass};
use prism_ir::{
    AnchorRef, ArtifactStatus, BlockerCauseSource, Capability, ClaimMode, ClaimStatus,
    ConflictOverlapKind, ConflictSeverity, CoordinationTaskStatus, DerivedPlanStatus, EdgeKind,
    EdgeOrigin, EffectiveTaskStatus, ExecutorClass, GitExecutionStatus, GitIntegrationEvidence,
    GitIntegrationMode, GitIntegrationStatus, Language, NodeKind, NodeRefKind, PlanKind,
    PlanOperatorState, PlanScope, PlanStatus, ReviewVerdict, Span, TaskLifecycleStatus,
};
use prism_memory::OutcomeEvent;
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use serde_json::Value;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct NodeIdView {
    pub crate_name: String,
    pub path: String,
    pub kind: NodeKind,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct SourceLocationView {
    pub start_line: usize,
    pub start_column: usize,
    pub end_line: usize,
    pub end_column: usize,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct SourceExcerptView {
    pub text: String,
    pub start_line: usize,
    pub end_line: usize,
    pub truncated: bool,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct SourceSliceView {
    pub text: String,
    pub start_line: usize,
    pub end_line: usize,
    pub focus: SourceLocationView,
    pub relative_focus: SourceLocationView,
    pub truncated: bool,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum AgentHandleCategoryView {
    Symbol,
    TextFragment,
    Concept,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct AgentTargetHandleView {
    pub handle: String,
    pub handle_category: AgentHandleCategoryView,
    pub kind: NodeKind,
    pub path: String,
    pub name: String,
    pub why_short: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub why_not_top: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub confidence_label: Option<ConfidenceLabel>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub file_path: Option<String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum AgentLocateStatus {
    Ok,
    Empty,
    Ambiguous,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct AgentLocateResultView {
    pub candidates: Vec<AgentTargetHandleView>,
    pub status: AgentLocateStatus,
    pub truncated: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub selection_reason: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub narrowing_hint: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub top_preview: Option<AgentTextPreviewView>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct AgentTextPreviewView {
    pub handle: String,
    pub file_path: String,
    pub start_line: usize,
    pub end_line: usize,
    pub text: String,
    pub truncated: bool,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct AgentSuggestedActionView {
    pub tool: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub handle: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub open_mode: Option<AgentOpenMode>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub expand_kind: Option<AgentExpandKind>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct AgentGatherResultView {
    pub matches: Vec<AgentOpenResultView>,
    pub truncated: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub narrowing_hint: Option<String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "lowercase")]
pub enum AgentOpenMode {
    Focus,
    Edit,
    Raw,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum AgentResultFreshnessView {
    Current,
    Remapped,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct AgentOpenResultView {
    pub handle: String,
    pub handle_category: AgentHandleCategoryView,
    pub file_path: String,
    pub start_line: usize,
    pub end_line: usize,
    pub text: String,
    pub truncated: bool,
    pub remapped: bool,
    pub freshness: AgentResultFreshnessView,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub next_action: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub promoted_handle: Option<AgentTargetHandleView>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub related_handles: Option<Vec<AgentTargetHandleView>>,
    #[serde(skip_serializing_if = "Vec::is_empty", default)]
    pub suggested_actions: Vec<AgentSuggestedActionView>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct AgentWorksetResultView {
    pub primary: AgentTargetHandleView,
    pub supporting_reads: Vec<AgentTargetHandleView>,
    pub likely_tests: Vec<AgentTargetHandleView>,
    pub why: String,
    pub truncated: bool,
    pub remapped: bool,
    pub freshness: AgentResultFreshnessView,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub next_action: Option<String>,
    #[serde(skip_serializing_if = "Vec::is_empty", default)]
    pub suggested_actions: Vec<AgentSuggestedActionView>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum AgentExpandKind {
    Diagnostics,
    Lineage,
    Neighbors,
    Diff,
    Health,
    Validation,
    Impact,
    Timeline,
    Memory,
    Drift,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct AgentExpandResultView {
    pub handle: String,
    pub handle_category: AgentHandleCategoryView,
    pub kind: AgentExpandKind,
    pub result: Value,
    pub remapped: bool,
    pub freshness: AgentResultFreshnessView,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub top_preview: Option<AgentTextPreviewView>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub next_action: Option<String>,
    #[serde(skip_serializing_if = "Vec::is_empty", default)]
    pub suggested_actions: Vec<AgentSuggestedActionView>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct AgentTaskBlockerView {
    pub kind: BlockerKind,
    pub summary: String,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct AgentOutcomeSummaryView {
    pub ts: u64,
    pub kind: String,
    pub result: String,
    pub summary: String,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct CoordinationTaskLifecycleView {
    pub completed: bool,
    pub published_to_branch: bool,
    pub coordination_published: bool,
    pub integrated_to_target: bool,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct TaskLeaseHolderView {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub principal: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub session_id: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub worktree_id: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub agent_id: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct AgentTaskBriefResultView {
    pub task_id: String,
    pub title: String,
    pub status: CoordinationTaskStatus,
    pub lifecycle: CoordinationTaskLifecycleView,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub assignee: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub pending_handoff_to: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub lease_state: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub lease_holder: Option<TaskLeaseHolderView>,
    pub blockers: Vec<AgentTaskBlockerView>,
    pub claim_holders: Vec<String>,
    pub conflict_summaries: Vec<String>,
    pub recent_outcomes: Vec<AgentOutcomeSummaryView>,
    pub likely_validations: Vec<String>,
    pub next_reads: Vec<AgentTargetHandleView>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub risk_hint: Option<String>,
    pub truncated: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub next_action: Option<String>,
    #[serde(skip_serializing_if = "Vec::is_empty", default)]
    pub suggested_actions: Vec<AgentSuggestedActionView>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct AgentConceptPacketView {
    pub handle: String,
    pub canonical_name: String,
    pub summary: String,
    pub aliases: Vec<String>,
    pub confidence: f32,
    pub core_members: Vec<AgentTargetHandleView>,
    pub supporting_members: Vec<AgentTargetHandleView>,
    pub likely_tests: Vec<AgentTargetHandleView>,
    pub evidence: Vec<String>,
    pub risk_hint: Option<String>,
    pub decode_lenses: Vec<ConceptDecodeLensView>,
    pub verbosity_applied: ConceptPacketVerbosityView,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub truncation: Option<ConceptPacketTruncationView>,
    pub scope: ConceptScopeView,
    pub provenance: ConceptProvenanceView,
    pub publication: Option<ConceptPublicationView>,
    #[serde(skip_serializing_if = "Vec::is_empty", default)]
    pub relations: Vec<ConceptRelationView>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub resolution: Option<ConceptResolutionView>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub binding_metadata: Option<ConceptBindingMetadataView>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub next_action: Option<String>,
    #[serde(skip_serializing_if = "Vec::is_empty", default)]
    pub suggested_actions: Vec<AgentSuggestedActionView>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub enum ConceptScopeView {
    Local,
    Session,
    Repo,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum ConceptPacketVerbosityView {
    Summary,
    Standard,
    Full,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct ConceptPacketTruncationView {
    pub core_members_omitted: usize,
    pub supporting_members_omitted: usize,
    pub likely_tests_omitted: usize,
    pub evidence_omitted: usize,
    pub relations_omitted: usize,
    pub relation_evidence_omitted: usize,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct ConceptCurationHintsView {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub inspect_first: Option<NodeIdView>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub supporting_read: Option<NodeIdView>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub likely_test: Option<NodeIdView>,
    pub next_action: String,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct AgentConceptResultView {
    pub packet: AgentConceptPacketView,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub decode: Option<ConceptDecodeView>,
    #[serde(skip_serializing_if = "Vec::is_empty", default)]
    pub alternates: Vec<AgentConceptPacketView>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct FocusedBlockView {
    pub symbol: SymbolView,
    pub slice: Option<SourceSliceView>,
    pub excerpt: Option<SourceExcerptView>,
    pub strategy: String,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct ToolCatalogEntryView {
    pub tool_name: String,
    pub schema_uri: String,
    pub description: String,
    pub example_input: Value,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub example_uri: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub shape_uri: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct ToolFieldSchemaView {
    pub name: String,
    pub required: bool,
    pub description: Option<String>,
    pub types: Vec<String>,
    pub enum_values: Vec<String>,
    #[serde(default)]
    pub nested_fields: Vec<ToolFieldSchemaView>,
    pub schema: Value,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct ToolPayloadVariantSchemaView {
    pub tag: String,
    pub schema_uri: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub example_uri: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub shape_uri: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub recipe_uri: Option<String>,
    pub required_fields: Vec<String>,
    pub fields: Vec<ToolFieldSchemaView>,
    pub schema: Value,
    pub example_input: Option<Value>,
    #[serde(default)]
    pub example_inputs: Vec<Value>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct ToolActionSchemaView {
    pub action: String,
    pub schema_uri: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub example_uri: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub shape_uri: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub recipe_uri: Option<String>,
    pub description: Option<String>,
    pub required_fields: Vec<String>,
    pub fields: Vec<ToolFieldSchemaView>,
    pub input_schema: Value,
    pub example_input: Option<Value>,
    #[serde(default)]
    pub example_inputs: Vec<Value>,
    pub payload_discriminator: Option<String>,
    #[serde(default)]
    pub payload_variants: Vec<ToolPayloadVariantSchemaView>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct ToolSchemaView {
    pub tool_name: String,
    pub schema_uri: String,
    pub description: String,
    pub example_input: Value,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub example_uri: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub shape_uri: Option<String>,
    #[serde(default)]
    pub example_inputs: Vec<Value>,
    pub input_schema: Value,
    pub actions: Vec<ToolActionSchemaView>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct ToolValidationIssueView {
    pub code: String,
    pub path: Option<String>,
    pub summary: String,
    #[serde(default)]
    pub allowed_values: Vec<String>,
    #[serde(default)]
    pub required_fields: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct ToolInputValidationView {
    pub tool_name: String,
    pub schema_uri: String,
    pub valid: bool,
    pub normalized_input: Value,
    pub action: Option<String>,
    pub action_schema_uri: Option<String>,
    pub summary: String,
    #[serde(default)]
    pub issues: Vec<ToolValidationIssueView>,
    #[serde(default)]
    pub example_inputs: Vec<Value>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct TextSearchMatchView {
    pub path: String,
    pub location: SourceLocationView,
    pub excerpt: SourceExcerptView,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct ChangedSymbolView {
    pub status: String,
    pub id: Option<NodeIdView>,
    pub name: String,
    pub kind: NodeKind,
    pub file_path: String,
    pub location: Option<SourceLocationView>,
    pub excerpt: Option<SourceExcerptView>,
    pub lineage_id: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct ChangedFileView {
    pub path: String,
    pub event_id: String,
    pub ts: u64,
    pub task_id: Option<String>,
    pub trigger: Option<String>,
    pub actor: Option<String>,
    pub reason: Option<String>,
    pub work_id: Option<String>,
    pub work_title: Option<String>,
    pub summary: String,
    pub changed_symbol_count: usize,
    pub added_count: usize,
    pub removed_count: usize,
    pub updated_count: usize,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct PatchEventView {
    pub event_id: String,
    pub ts: u64,
    pub task_id: Option<String>,
    pub trigger: Option<String>,
    pub actor: Option<String>,
    pub reason: Option<String>,
    pub work_id: Option<String>,
    pub work_title: Option<String>,
    pub summary: String,
    pub files: Vec<String>,
    pub changed_symbol_count: usize,
    pub changed_symbols_truncated: bool,
    pub changed_symbols: Vec<ChangedSymbolView>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct DiffHunkView {
    pub event_id: String,
    pub ts: u64,
    pub task_id: Option<String>,
    pub trigger: Option<String>,
    pub summary: String,
    pub symbol: ChangedSymbolView,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct RuntimeHealthView {
    pub ok: bool,
    pub detail: String,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct ConnectionInfoView {
    pub root: String,
    pub mode: String,
    pub transport: String,
    pub uri: Option<String>,
    pub uri_file: String,
    pub health_uri: Option<String>,
    pub health: RuntimeHealthView,
    pub bridge_role: String,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct RuntimeProcessView {
    pub pid: u32,
    pub parent_pid: u32,
    pub rss_kb: u64,
    pub rss_mb: f64,
    pub elapsed: String,
    pub kind: String,
    pub command: String,
    pub health_path: Option<String>,
    pub bridge_state: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct RuntimeMaterializationCoverageView {
    pub known_files: usize,
    pub known_directories: usize,
    pub materialized_files: usize,
    pub materialized_nodes: usize,
    pub materialized_edges: usize,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct RuntimeBoundaryRegionView {
    pub id: String,
    pub path: String,
    pub provenance: String,
    pub materialization_state: String,
    pub scope_state: String,
    pub known_file_count: usize,
    pub materialized_file_count: usize,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct RuntimeMaterializationItemView {
    pub status: String,
    pub depth: String,
    pub loaded_revision: u64,
    pub current_revision: Option<u64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub coverage: Option<RuntimeMaterializationCoverageView>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub boundaries: Vec<RuntimeBoundaryRegionView>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct RuntimeMaterializationView {
    pub workspace: RuntimeMaterializationItemView,
    pub episodic: RuntimeMaterializationItemView,
    pub inference: RuntimeMaterializationItemView,
    pub coordination: RuntimeMaterializationItemView,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct RuntimeCoordinationSurfaceLagItemView {
    pub name: String,
    pub status: String,
    pub revision: Option<u64>,
    pub authoritative_revision: u64,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct RuntimeCoordinationLagView {
    pub authoritative_revision: u64,
    pub tracked_snapshot: RuntimeCoordinationSurfaceLagItemView,
    pub startup_checkpoint: RuntimeCoordinationSurfaceLagItemView,
    pub read_model: RuntimeCoordinationSurfaceLagItemView,
    pub queue_read_model: RuntimeCoordinationSurfaceLagItemView,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum ProjectionClassView {
    Published,
    Serving,
    AdHoc,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum ProjectionAuthorityPlaneView {
    PublishedRepo,
    SharedRuntime,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum ProjectionFreshnessStateView {
    Current,
    Pending,
    Stale,
    Recovery,
    Deferred,
    Unknown,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum ProjectionMaterializationStateView {
    Materialized,
    Partial,
    Deferred,
    KnownUnmaterialized,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct ProjectionReadModelView {
    pub name: String,
    pub projection_class: ProjectionClassView,
    pub authority_planes: Vec<ProjectionAuthorityPlaneView>,
    pub freshness: ProjectionFreshnessStateView,
    pub materialization: ProjectionMaterializationStateView,
    pub entry_count: usize,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct RuntimeProjectionScopeView {
    pub scope: String,
    pub projection_class: ProjectionClassView,
    pub authority_planes: Vec<ProjectionAuthorityPlaneView>,
    pub freshness: ProjectionFreshnessStateView,
    pub materialization: ProjectionMaterializationStateView,
    pub concept_count: usize,
    pub relation_count: usize,
    pub contract_count: usize,
    pub co_change_lineage_count: usize,
    pub validation_lineage_count: usize,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub read_models: Vec<ProjectionReadModelView>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct RuntimeOverlayScopeView {
    pub scope: String,
    pub plan_count: usize,
    pub plan_node_count: usize,
    pub overlay_count: usize,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct RuntimeScopesView {
    pub projections: Vec<RuntimeProjectionScopeView>,
    pub overlays: Vec<RuntimeOverlayScopeView>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct RuntimeFreshnessView {
    pub fs_observed_revision: u64,
    pub fs_applied_revision: u64,
    pub fs_dirty: bool,
    pub generation_id: Option<u64>,
    pub parent_generation_id: Option<u64>,
    pub committed_delta_sequence: Option<u64>,
    pub last_refresh_path: Option<String>,
    pub last_refresh_timestamp: Option<String>,
    pub last_refresh_duration_ms: Option<u64>,
    pub last_refresh_loaded_bytes: Option<u64>,
    pub last_refresh_replay_volume: Option<u64>,
    pub last_refresh_full_rebuild_count: Option<u64>,
    pub last_refresh_workspace_reloaded: Option<bool>,
    pub last_workspace_build_ms: Option<u64>,
    pub last_runtime_ready_ms: Option<u64>,
    pub materialization: RuntimeMaterializationView,
    pub coordination_lag: Option<RuntimeCoordinationLagView>,
    pub domains: Vec<RuntimeDomainFreshnessView>,
    pub active_command: Option<String>,
    pub active_queue_class: Option<String>,
    pub queue_depth: usize,
    pub queued_by_class: Vec<RuntimeQueueDepthView>,
    pub status: String,
    pub error: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct RuntimeCoordinationAuthorityView {
    pub ref_name: String,
    pub head_commit: Option<String>,
    pub history_depth: u64,
    pub max_history_commits: u64,
    pub snapshot_file_count: usize,
    pub verification_status: String,
    pub authoritative_hydration_allowed: bool,
    pub degraded: bool,
    pub verification_error: Option<String>,
    pub repair_hint: Option<String>,
    pub current_manifest_digest: Option<String>,
    pub last_verified_manifest_digest: Option<String>,
    pub previous_manifest_digest: Option<String>,
    pub last_successful_publish_at: Option<u64>,
    pub last_successful_publish_retry_count: u32,
    pub publish_retry_budget: u32,
    pub compacted_head: bool,
    pub needs_compaction: bool,
    pub compaction_status: String,
    pub compaction_mode: Option<String>,
    pub last_compacted_at: Option<u64>,
    pub compaction_previous_head_commit: Option<String>,
    pub compaction_previous_history_depth: Option<u64>,
    pub archive_boundary_manifest_digest: Option<String>,
    pub summary_published_at: Option<u64>,
    pub summary_freshness_status: String,
    pub authoritative_fallback_required: bool,
    pub freshness_reason: Option<String>,
    pub lagging_task_shard_refs: usize,
    pub lagging_claim_shard_refs: usize,
    pub lagging_runtime_refs: usize,
    pub newest_authoritative_ref_at: Option<u64>,
    pub runtime_descriptor_count: usize,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub runtime_descriptors: Vec<RuntimeSharedCoordinationRuntimeDescriptorView>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum RuntimeDiscoveryModeView {
    None,
    LanDirect,
    PublicUrl,
    Full,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum RuntimeDescriptorCapabilityView {
    CoordinationRefPublisher,
    BoundedPeerReads,
    BundleExports,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct RuntimeSharedCoordinationRuntimeDescriptorView {
    pub runtime_id: String,
    pub repo_id: String,
    pub worktree_id: String,
    pub principal_id: String,
    pub instance_started_at: u64,
    pub last_seen_at: u64,
    pub branch_ref: Option<String>,
    pub checked_out_commit: Option<String>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub capabilities: Vec<RuntimeDescriptorCapabilityView>,
    pub discovery_mode: RuntimeDiscoveryModeView,
    pub peer_endpoint: Option<String>,
    pub public_endpoint: Option<String>,
    pub peer_transport_identity: Option<String>,
    pub blob_snapshot_head: Option<String>,
    pub export_policy: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct RuntimeAssistedLeaseRenewalView {
    pub enabled: bool,
    pub env_var: String,
    pub default_enabled: bool,
    pub authoritative: bool,
    pub scope: String,
    pub requires_authenticated_mutation: bool,
    pub bounded_by: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct RuntimeDomainFreshnessView {
    pub domain: String,
    pub freshness: String,
    pub materialization_depth: String,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct RuntimeQueueDepthView {
    pub queue_class: String,
    pub depth: usize,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct RuntimeStatusView {
    pub root: String,
    pub connection: ConnectionInfoView,
    pub uri: Option<String>,
    pub uri_file: String,
    pub log_path: String,
    pub log_bytes: Option<u64>,
    pub mcp_call_log_path: Option<String>,
    pub mcp_call_log_bytes: Option<u64>,
    pub cache_path: String,
    pub cache_bytes: Option<u64>,
    pub coordination_materialization_path: Option<String>,
    pub coordination_materialization_bytes: Option<u64>,
    pub health_path: String,
    pub health: RuntimeHealthView,
    pub runtime_count: usize,
    pub bridge_count: usize,
    pub connected_bridge_count: usize,
    pub idle_bridge_count: usize,
    pub orphan_bridge_count: usize,
    pub processes: Vec<RuntimeProcessView>,
    pub process_error: Option<String>,
    pub assisted_lease_renewal: RuntimeAssistedLeaseRenewalView,
    pub coordination_authority: Option<RuntimeCoordinationAuthorityView>,
    pub scopes: RuntimeScopesView,
    pub freshness: RuntimeFreshnessView,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct RuntimeLogEventView {
    pub timestamp: Option<String>,
    pub level: Option<String>,
    pub message: String,
    pub target: Option<String>,
    pub file: Option<String>,
    pub line_number: Option<u64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub repo_id: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub worktree_id: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub workspace_root: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub log_path: Option<String>,
    pub fields: Option<Value>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct SymbolView {
    pub id: NodeIdView,
    pub name: String,
    pub kind: NodeKind,
    pub signature: String,
    pub file_path: Option<String>,
    pub span: Span,
    pub location: Option<SourceLocationView>,
    pub language: Language,
    pub lineage_id: Option<String>,
    pub source_excerpt: Option<SourceExcerptView>,
    pub owner_hint: Option<OwnerHintView>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct OwnerHintView {
    pub kind: String,
    pub score: usize,
    pub matched_terms: Vec<String>,
    pub why: String,
    pub trust_signals: TrustSignalsView,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "lowercase")]
pub enum ConfidenceLabel {
    Low,
    Medium,
    High,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum EvidenceSourceKind {
    DirectGraph,
    Inferred,
    Memory,
    Outcome,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct TrustSignalsView {
    pub confidence_label: ConfidenceLabel,
    pub evidence_sources: Vec<EvidenceSourceKind>,
    pub why: Vec<String>,
}

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct RelationsView {
    pub contains: Vec<SymbolView>,
    pub callers: Vec<SymbolView>,
    pub callees: Vec<SymbolView>,
    pub references: Vec<SymbolView>,
    pub imports: Vec<SymbolView>,
    pub implements: Vec<SymbolView>,
    pub specifies: Vec<SymbolView>,
    pub specified_by: Vec<SymbolView>,
    pub validates: Vec<SymbolView>,
    pub validated_by: Vec<SymbolView>,
    pub related: Vec<SymbolView>,
    pub related_by: Vec<SymbolView>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct LineageView {
    pub lineage_id: String,
    pub current: SymbolView,
    pub status: LineageStatus,
    pub summary: String,
    pub uncertainty: Vec<String>,
    pub history: Vec<LineageEventView>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "lowercase")]
pub enum LineageStatus {
    Active,
    Dead,
    Ambiguous,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct LineageEventView {
    pub event_id: String,
    pub ts: u64,
    pub kind: String,
    pub confidence: f32,
    pub before: Vec<NodeIdView>,
    pub after: Vec<NodeIdView>,
    pub evidence: Vec<String>,
    pub evidence_details: Vec<LineageEvidenceView>,
    pub summary: String,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct LineageEvidenceView {
    pub code: String,
    pub label: String,
    pub detail: String,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct EdgeView {
    pub kind: EdgeKind,
    pub source: NodeIdView,
    pub target: NodeIdView,
    pub origin: EdgeOrigin,
    pub confidence: f32,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct SubgraphView {
    pub nodes: Vec<SymbolView>,
    pub edges: Vec<EdgeView>,
    pub truncated: bool,
    pub max_depth_reached: Option<usize>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct ChangeImpactView {
    pub direct_nodes: Vec<NodeIdView>,
    pub lineages: Vec<String>,
    pub likely_validations: Vec<String>,
    pub validation_checks: Vec<ValidationCheckView>,
    pub co_change_neighbors: Vec<CoChangeView>,
    pub risk_events: Vec<OutcomeEvent>,
    pub promoted_summaries: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct ValidationCheckView {
    pub label: String,
    pub score: f32,
    pub last_seen: u64,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct CoChangeView {
    pub lineage: String,
    pub count: u32,
    pub nodes: Vec<NodeIdView>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct ValidationRecipeView {
    pub target: NodeIdView,
    pub checks: Vec<String>,
    pub scored_checks: Vec<ValidationCheckView>,
    pub related_nodes: Vec<NodeIdView>,
    pub co_change_neighbors: Vec<CoChangeView>,
    pub recent_failures: Vec<OutcomeEvent>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct QueryEvidenceView {
    pub kind: String,
    pub detail: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub path: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub line: Option<usize>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub target: Option<NodeIdView>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct RepoPlaybookSectionView {
    pub status: String,
    pub summary: String,
    pub commands: Vec<String>,
    pub why: String,
    pub provenance: Vec<QueryEvidenceView>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct RepoPlaybookGotchaView {
    pub summary: String,
    pub why: String,
    pub provenance: Vec<QueryEvidenceView>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct RepoPlaybookView {
    pub root: String,
    pub build: RepoPlaybookSectionView,
    pub test: RepoPlaybookSectionView,
    pub lint: RepoPlaybookSectionView,
    pub format: RepoPlaybookSectionView,
    pub workflow: RepoPlaybookSectionView,
    pub gotchas: Vec<RepoPlaybookGotchaView>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct ValidationPlanSubjectView {
    pub kind: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub task_id: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub target: Option<NodeIdView>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub paths: Vec<String>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub unresolved_paths: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct ValidationPlanCheckView {
    pub label: String,
    pub why: String,
    pub provenance: Vec<QueryEvidenceView>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub score: Option<f32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub last_seen: Option<u64>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct ValidationPlanView {
    pub subject: ValidationPlanSubjectView,
    pub fast: Vec<ValidationPlanCheckView>,
    pub broader: Vec<ValidationPlanCheckView>,
    pub related_targets: Vec<NodeIdView>,
    pub notes: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct QueryViewSubjectView {
    pub kind: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub task_id: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub target: Option<NodeIdView>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub paths: Vec<String>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub unresolved_paths: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct QueryRecommendationView {
    pub kind: String,
    pub label: String,
    pub why: String,
    pub provenance: Vec<QueryEvidenceView>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub target: Option<NodeIdView>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub path: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub score: Option<f32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub last_seen: Option<u64>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct QueryRiskHintView {
    pub summary: String,
    pub why: String,
    pub provenance: Vec<QueryEvidenceView>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct ImpactView {
    pub subject: QueryViewSubjectView,
    pub downstream: Vec<QueryRecommendationView>,
    pub risks: Vec<QueryRiskHintView>,
    pub recommended_checks: Vec<QueryRecommendationView>,
    pub contracts: Vec<ContractPacketView>,
    pub notes: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct AfterEditView {
    pub subject: QueryViewSubjectView,
    pub next_reads: Vec<QueryRecommendationView>,
    pub tests: Vec<QueryRecommendationView>,
    pub docs: Vec<QueryRecommendationView>,
    pub risk_checks: Vec<QueryRecommendationView>,
    pub contracts: Vec<ContractPacketView>,
    pub notes: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct CommandMemoryCommandView {
    pub command: String,
    pub confidence: f32,
    pub why: String,
    pub provenance: Vec<QueryEvidenceView>,
    pub caveats: Vec<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub last_seen: Option<u64>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct CommandMemoryView {
    pub subject: QueryViewSubjectView,
    pub commands: Vec<CommandMemoryCommandView>,
    pub notes: Vec<String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum ConceptDecodeLensView {
    Open,
    Workset,
    Validation,
    Timeline,
    Memory,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum ConceptPublicationStatusView {
    Active,
    Retired,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct ConceptProvenanceView {
    pub origin: String,
    pub kind: String,
    pub task_id: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct ConceptPublicationView {
    pub published_at: u64,
    pub last_reviewed_at: Option<u64>,
    pub status: ConceptPublicationStatusView,
    pub supersedes: Vec<String>,
    pub retired_at: Option<u64>,
    pub retirement_reason: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct ConceptBindingMetadataView {
    pub core_member_lineages: Vec<Option<String>>,
    pub supporting_member_lineages: Vec<Option<String>>,
    pub likely_test_lineages: Vec<Option<String>>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct ConceptResolutionView {
    pub score: i32,
    pub reasons: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum ConceptRelationKindView {
    DependsOn,
    Specializes,
    PartOf,
    ValidatedBy,
    OftenUsedWith,
    Supersedes,
    ConfusedWith,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum ConceptRelationDirectionView {
    Outgoing,
    Incoming,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct ConceptRelationView {
    pub kind: ConceptRelationKindView,
    pub direction: ConceptRelationDirectionView,
    pub related_handle: String,
    pub related_canonical_name: Option<String>,
    pub related_summary: Option<String>,
    pub confidence: f32,
    pub evidence: Vec<String>,
    pub scope: ConceptScopeView,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct ConceptPacketView {
    pub handle: String,
    pub canonical_name: String,
    pub summary: String,
    pub aliases: Vec<String>,
    pub confidence: f32,
    pub core_members: Vec<NodeIdView>,
    pub supporting_members: Vec<NodeIdView>,
    pub likely_tests: Vec<NodeIdView>,
    pub evidence: Vec<String>,
    pub risk_hint: Option<String>,
    pub decode_lenses: Vec<ConceptDecodeLensView>,
    pub verbosity_applied: ConceptPacketVerbosityView,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub truncation: Option<ConceptPacketTruncationView>,
    pub curation_hints: ConceptCurationHintsView,
    pub scope: ConceptScopeView,
    pub provenance: ConceptProvenanceView,
    pub publication: Option<ConceptPublicationView>,
    #[serde(skip_serializing_if = "Vec::is_empty", default)]
    pub relations: Vec<ConceptRelationView>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub resolution: Option<ConceptResolutionView>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub binding_metadata: Option<ConceptBindingMetadataView>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct ConceptDecodeView {
    pub concept: ConceptPacketView,
    pub lens: ConceptDecodeLensView,
    pub primary: Option<SymbolView>,
    pub members: Vec<SymbolView>,
    pub supporting_reads: Vec<SymbolView>,
    pub likely_tests: Vec<SymbolView>,
    pub recent_failures: Vec<OutcomeEvent>,
    pub related_memory: Vec<ScoredMemoryView>,
    pub recent_patches: Vec<PatchEventView>,
    pub validation_recipe: Option<ValidationRecipeView>,
    pub evidence: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum AnchorRefView {
    Node {
        #[serde(rename = "crateName")]
        crate_name: String,
        path: String,
        kind: String,
    },
    Lineage {
        #[serde(rename = "lineageId")]
        lineage_id: String,
    },
    File {
        #[serde(rename = "fileId", default, skip_serializing_if = "Option::is_none")]
        file_id: Option<u32>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        path: Option<String>,
    },
    Kind {
        kind: String,
    },
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum ContractKindView {
    Interface,
    Behavioral,
    DataShape,
    DependencyBoundary,
    Lifecycle,
    Protocol,
    Operational,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum ContractStatusView {
    Candidate,
    Active,
    Deprecated,
    Retired,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum ContractStabilityView {
    Experimental,
    Internal,
    Public,
    Deprecated,
    Migrating,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum ContractGuaranteeStrengthView {
    Hard,
    Soft,
    Conditional,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum ContractHealthStatusView {
    Healthy,
    Watch,
    Degraded,
    Stale,
    Superseded,
    Retired,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct ContractTargetView {
    pub anchors: Vec<AnchorRefView>,
    pub concept_handles: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct ContractGuaranteeView {
    pub id: String,
    pub statement: String,
    pub scope: Option<String>,
    pub strength: Option<ContractGuaranteeStrengthView>,
    pub evidence_refs: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct ContractHealthSignalsView {
    pub guarantee_count: usize,
    pub validation_count: usize,
    pub consumer_count: usize,
    pub validation_coverage_ratio: f32,
    pub guarantee_evidence_ratio: f32,
    pub stale_validation_links: bool,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct ContractHealthView {
    pub status: ContractHealthStatusView,
    pub score: f32,
    pub reasons: Vec<String>,
    pub signals: ContractHealthSignalsView,
    pub superseded_by: Vec<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub next_action: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct ContractValidationView {
    pub id: String,
    pub summary: Option<String>,
    pub anchors: Vec<AnchorRefView>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct ContractCompatibilityView {
    pub compatible: Vec<String>,
    pub additive: Vec<String>,
    pub risky: Vec<String>,
    pub breaking: Vec<String>,
    pub migrating: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct ContractResolutionView {
    pub score: i32,
    pub reasons: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct ContractPacketView {
    pub handle: String,
    pub name: String,
    pub summary: String,
    pub aliases: Vec<String>,
    pub kind: ContractKindView,
    pub subject: ContractTargetView,
    pub guarantees: Vec<ContractGuaranteeView>,
    pub assumptions: Vec<String>,
    pub consumers: Vec<ContractTargetView>,
    pub validations: Vec<ContractValidationView>,
    pub stability: ContractStabilityView,
    pub compatibility: ContractCompatibilityView,
    pub evidence: Vec<String>,
    pub status: ContractStatusView,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub health: Option<ContractHealthView>,
    pub scope: ConceptScopeView,
    pub provenance: ConceptProvenanceView,
    pub publication: Option<ConceptPublicationView>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub resolution: Option<ContractResolutionView>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct TaskValidationRecipeView {
    pub task_id: String,
    pub checks: Vec<String>,
    pub scored_checks: Vec<ValidationCheckView>,
    pub related_nodes: Vec<NodeIdView>,
    pub co_change_neighbors: Vec<CoChangeView>,
    pub recent_failures: Vec<OutcomeEvent>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct TaskRiskView {
    pub task_id: String,
    pub risk_score: f32,
    pub review_required: bool,
    pub stale_task: bool,
    pub has_approved_artifact: bool,
    pub likely_validations: Vec<String>,
    pub missing_validations: Vec<String>,
    pub validation_checks: Vec<ValidationCheckView>,
    pub co_change_neighbors: Vec<CoChangeView>,
    pub risk_events: Vec<OutcomeEvent>,
    pub contracts: Vec<ContractPacketView>,
    pub contract_review_notes: Vec<String>,
    pub promoted_summaries: Vec<String>,
    pub approved_artifact_ids: Vec<String>,
    pub stale_artifact_ids: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct ArtifactRiskView {
    pub artifact_id: String,
    pub task_id: String,
    pub risk_score: f32,
    pub review_required: bool,
    pub stale: bool,
    pub required_validations: Vec<String>,
    pub validated_checks: Vec<String>,
    pub missing_validations: Vec<String>,
    pub co_change_neighbors: Vec<CoChangeView>,
    pub risk_events: Vec<OutcomeEvent>,
    pub contracts: Vec<ContractPacketView>,
    pub contract_review_notes: Vec<String>,
    pub promoted_summaries: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct ArtifactReviewView {
    pub id: String,
    pub artifact_id: String,
    pub review_requirement_id: String,
    pub reviewer_class: Option<ReviewerClass>,
    pub verdict: ReviewVerdict,
    pub summary: String,
    pub ts: u64,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct TaskEvidenceArtifactStatusView {
    pub artifact: ArtifactView,
    pub reviews: Vec<ArtifactReviewView>,
    pub latest_review: Option<ArtifactReviewView>,
    pub latest_review_verdict: Option<ReviewVerdict>,
    pub pending_review: bool,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct TaskEvidenceStatusView {
    pub task_id: String,
    pub artifacts: Vec<TaskEvidenceArtifactStatusView>,
    pub blockers: Vec<BlockerView>,
    pub pending_review_count: usize,
    pub approved_artifact_count: usize,
    pub rejected_artifact_count: usize,
    pub missing_validations: Vec<String>,
    pub stale_artifact_ids: Vec<String>,
    pub review_required: bool,
    pub has_approved_artifact: bool,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct TaskReviewStatusView {
    pub task_id: String,
    pub artifacts: Vec<TaskEvidenceArtifactStatusView>,
    pub pending_review_count: usize,
    pub approved_artifact_count: usize,
    pub rejected_artifact_count: usize,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct DriftCandidateView {
    pub spec: NodeIdView,
    pub implementations: Vec<NodeIdView>,
    pub validations: Vec<NodeIdView>,
    pub related: Vec<NodeIdView>,
    pub reasons: Vec<String>,
    pub recent_failures: Vec<OutcomeEvent>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct OwnerCandidateView {
    pub symbol: SymbolView,
    pub kind: String,
    pub score: usize,
    pub matched_terms: Vec<String>,
    pub why: String,
    pub trust_signals: TrustSignalsView,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct TaskIntentView {
    pub task_id: String,
    pub specs: Vec<NodeIdView>,
    pub implementations: Vec<NodeIdView>,
    pub validations: Vec<NodeIdView>,
    pub related: Vec<NodeIdView>,
    pub drift_candidates: Vec<DriftCandidateView>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct WorkspaceRevisionView {
    pub graph_version: u64,
    pub git_commit: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct LinkedSpecSummaryView {
    pub spec_id: String,
    pub source_path: String,
    pub linked_source_revision: Option<String>,
    pub current_source_revision: Option<String>,
    pub drift_status: String,
    pub title: Option<String>,
    pub declared_status: Option<String>,
    pub overall_status: Option<String>,
    pub sync_kind: Option<String>,
    pub covered_checklist_items: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct NodeRefView {
    pub kind: NodeRefKind,
    pub id: String,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct CoordinationPlanV2View {
    pub id: String,
    pub parent_plan_id: Option<String>,
    pub title: String,
    pub goal: String,
    pub scope: PlanScope,
    pub kind: PlanKind,
    pub operator_state: PlanOperatorState,
    pub status: DerivedPlanStatus,
    pub scheduling: PlanSchedulingView,
    pub git_execution_policy: GitExecutionPolicyView,
    pub tags: Vec<String>,
    pub created_from: Option<String>,
    pub metadata: Value,
    pub children: Vec<NodeRefView>,
    pub dependencies: Vec<NodeRefView>,
    pub dependents: Vec<NodeRefView>,
    pub estimated_minutes_total: u32,
    pub remaining_estimated_minutes: u32,
    pub activity: Option<PlanActivityView>,
    pub linked_specs: Vec<LinkedSpecSummaryView>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct TaskExecutorPolicyView {
    pub executor_class: ExecutorClass,
    pub target_label: Option<String>,
    pub allowed_principals: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct CoordinationTaskV2View {
    pub id: String,
    pub parent_plan_id: String,
    pub title: String,
    pub summary: Option<String>,
    pub lifecycle_status: TaskLifecycleStatus,
    pub status: EffectiveTaskStatus,
    pub graph_actionable: bool,
    pub estimated_minutes: u32,
    pub executor: TaskExecutorPolicyView,
    pub assignee: Option<String>,
    pub pending_handoff_to: Option<String>,
    pub session: Option<String>,
    pub worktree_id: Option<String>,
    pub branch_ref: Option<String>,
    pub anchors: Vec<AnchorRef>,
    pub bindings: PlanBindingView,
    pub artifact_requirements: Vec<ArtifactRequirement>,
    pub review_requirements: Vec<ReviewRequirement>,
    pub validation_refs: Vec<ValidationRefView>,
    pub base_revision: WorkspaceRevisionView,
    pub priority: Option<u8>,
    pub tags: Vec<String>,
    pub metadata: Value,
    pub git_execution: TaskGitExecutionView,
    pub blocker_causes: Vec<BlockerCauseView>,
    pub dependencies: Vec<NodeRefView>,
    pub dependents: Vec<NodeRefView>,
    pub linked_specs: Vec<LinkedSpecSummaryView>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct PlanChildrenV2View {
    pub plan_id: String,
    pub children: Vec<NodeRefView>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct PlanListEntryView {
    pub plan_id: String,
    pub title: String,
    pub goal: String,
    pub status: PlanStatus,
    pub scope: PlanScope,
    pub kind: PlanKind,
    pub scheduling: PlanSchedulingView,
    pub git_execution_policy: GitExecutionPolicyView,
    pub created_at: Option<u64>,
    pub last_updated_at: Option<u64>,
    pub node_status_counts: PlanNodeStatusCountsView,
    pub summary: String,
    pub plan_summary: PlanSummaryView,
    pub activity: Option<PlanActivityView>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct PlanNodeStatusCountsView {
    pub proposed: usize,
    pub ready: usize,
    pub in_progress: usize,
    pub blocked: usize,
    pub waiting: usize,
    pub in_review: usize,
    pub validating: usize,
    pub completed: usize,
    pub abandoned: usize,
    pub abstract_nodes: usize,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct PlanActivityView {
    pub created_at: Option<u64>,
    pub last_updated_at: Option<u64>,
    pub last_event_kind: Option<String>,
    pub last_event_summary: Option<String>,
    pub last_event_task_id: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct PlanSchedulingView {
    pub importance: u8,
    pub urgency: u8,
    pub manual_boost: i16,
    pub due_at: Option<u64>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct GitExecutionPolicyView {
    pub start_mode: String,
    pub completion_mode: String,
    pub integration_mode: String,
    pub target_ref: Option<String>,
    pub target_branch: String,
    pub require_task_branch: bool,
    pub max_commits_behind_target: u32,
    pub max_fetch_age_seconds: Option<u64>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct ValidationRefView {
    pub id: String,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct PlanBindingView {
    pub anchors: Vec<AnchorRef>,
    pub concept_handles: Vec<String>,
    pub artifact_refs: Vec<String>,
    pub memory_refs: Vec<String>,
    pub outcome_refs: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct GitPreflightReportView {
    pub source_ref: Option<String>,
    pub target_ref: Option<String>,
    pub publish_ref: Option<String>,
    pub checked_at: u64,
    pub target_branch: String,
    pub max_commits_behind_target: u32,
    pub fetch_age_seconds: Option<u64>,
    pub current_branch: Option<String>,
    pub head_commit: Option<String>,
    pub target_commit: Option<String>,
    pub merge_base_commit: Option<String>,
    pub behind_target_commits: u32,
    pub worktree_dirty: bool,
    pub dirty_paths: Vec<String>,
    pub protected_dirty_paths: Vec<String>,
    pub failure: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct GitPublishReportView {
    pub attempted_at: u64,
    pub publish_ref: Option<String>,
    pub code_commit: Option<String>,
    pub coordination_commit: Option<String>,
    pub pushed_ref: Option<String>,
    pub staged_paths: Vec<String>,
    pub protected_paths: Vec<String>,
    pub failure: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct TaskGitExecutionView {
    pub status: GitExecutionStatus,
    pub pending_task_status: Option<CoordinationTaskStatus>,
    pub source_ref: Option<String>,
    pub target_ref: Option<String>,
    pub publish_ref: Option<String>,
    pub target_branch: Option<String>,
    pub source_commit: Option<String>,
    pub publish_commit: Option<String>,
    pub target_commit_at_publish: Option<String>,
    pub review_artifact_ref: Option<String>,
    pub integration_commit: Option<String>,
    pub integration_evidence: Option<GitIntegrationEvidence>,
    pub integration_mode: GitIntegrationMode,
    pub integration_status: GitIntegrationStatus,
    pub last_preflight: Option<GitPreflightReportView>,
    pub last_publish: Option<GitPublishReportView>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct BlockerCauseView {
    pub source: BlockerCauseSource,
    pub code: Option<String>,
    pub acceptance_label: Option<String>,
    pub threshold_metric: Option<String>,
    pub threshold_value: Option<f32>,
    pub observed_value: Option<f32>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct PlanSummaryView {
    pub plan_id: String,
    pub status: PlanStatus,
    pub total_nodes: usize,
    pub completed_nodes: usize,
    pub abandoned_nodes: usize,
    pub in_progress_nodes: usize,
    pub actionable_nodes: usize,
    pub execution_blocked_nodes: usize,
    pub completion_gated_nodes: usize,
    pub review_gated_nodes: usize,
    pub validation_gated_nodes: usize,
    pub stale_nodes: usize,
    pub claim_conflicted_nodes: usize,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct ClaimView {
    pub id: String,
    pub holder: String,
    pub task_id: Option<String>,
    pub agent: Option<String>,
    pub worktree_id: Option<String>,
    pub capability: Capability,
    pub mode: ClaimMode,
    pub status: ClaimStatus,
    pub anchors: Vec<AnchorRef>,
    pub expires_at: u64,
    pub base_revision: WorkspaceRevisionView,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct ConflictView {
    pub severity: ConflictSeverity,
    pub summary: String,
    pub anchors: Vec<AnchorRef>,
    pub overlap_kinds: Vec<ConflictOverlapKind>,
    pub blocking_claim_ids: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct CoordinationInboxView {
    pub plan: Option<CoordinationPlanV2View>,
    pub children: Option<PlanChildrenV2View>,
    pub graph_actionable_tasks: Vec<CoordinationTaskV2View>,
    pub actionable_tasks: Vec<CoordinationTaskV2View>,
    pub ready_tasks: Vec<CoordinationTaskV2View>,
    pub pending_reviews: Vec<ArtifactView>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct TaskContextView {
    pub task: Option<CoordinationTaskV2View>,
    pub dependencies: Vec<NodeRefView>,
    pub dependents: Vec<NodeRefView>,
    pub blockers: Vec<BlockerView>,
    pub artifacts: Vec<ArtifactView>,
    pub claims: Vec<ClaimView>,
    pub conflicts: Vec<ConflictView>,
    pub blast_radius: Option<ChangeImpactView>,
    pub validation_recipe: Option<TaskValidationRecipeView>,
    pub risk: Option<TaskRiskView>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct BlockerView {
    pub kind: BlockerKind,
    pub summary: String,
    pub related_task_id: Option<String>,
    pub related_artifact_id: Option<String>,
    pub risk_score: Option<f32>,
    pub validation_checks: Vec<String>,
    pub causes: Vec<BlockerCauseView>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct ArtifactView {
    pub id: String,
    pub task_id: String,
    pub artifact_requirement_id: String,
    pub status: ArtifactStatus,
    pub anchors: Vec<AnchorRef>,
    pub base_revision: WorkspaceRevisionView,
    pub diff_ref: Option<String>,
    pub required_validations: Vec<String>,
    pub validated_checks: Vec<String>,
    pub risk_score: Option<f32>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct PolicyViolationView {
    pub code: String,
    pub summary: String,
    pub plan_id: Option<String>,
    pub task_id: Option<String>,
    pub claim_id: Option<String>,
    pub artifact_id: Option<String>,
    pub details: Value,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct PolicyViolationRecordView {
    pub event_id: String,
    pub ts: u64,
    pub summary: String,
    pub plan_id: Option<String>,
    pub task_id: Option<String>,
    pub claim_id: Option<String>,
    pub artifact_id: Option<String>,
    pub violations: Vec<PolicyViolationView>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct MemoryEntryView {
    pub id: String,
    pub anchors: Vec<AnchorRef>,
    pub kind: String,
    pub scope: String,
    pub content: String,
    pub metadata: Value,
    pub created_at: u64,
    pub source: String,
    pub trust: f32,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct ScoredMemoryView {
    pub id: String,
    pub entry: MemoryEntryView,
    pub score: f32,
    pub source_module: String,
    pub explanation: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct MemoryEventView {
    pub id: String,
    pub action: String,
    pub memory_id: String,
    pub scope: String,
    pub entry: Option<MemoryEntryView>,
    pub recorded_at: u64,
    pub task_id: Option<String>,
    pub promoted_from: Vec<String>,
    pub supersedes: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct TaskLifecycleSummaryView {
    pub plan_count: usize,
    pub patch_count: usize,
    pub build_count: usize,
    pub test_count: usize,
    pub failure_count: usize,
    pub validation_count: usize,
    pub note_count: usize,
    pub started_at: Option<u64>,
    pub last_updated_at: Option<u64>,
    pub final_summary: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct TaskJournalView {
    pub task_id: String,
    pub description: Option<String>,
    pub tags: Vec<String>,
    pub disposition: String,
    pub active: bool,
    pub anchors: Vec<AnchorRef>,
    pub summary: TaskLifecycleSummaryView,
    pub diagnostics: Vec<QueryDiagnostic>,
    pub related_memory: Vec<ScoredMemoryView>,
    pub recent_events: Vec<OutcomeEvent>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct CuratorProposalView {
    pub index: usize,
    pub kind: String,
    pub disposition: String,
    pub payload: Value,
    pub decided_at: Option<u64>,
    pub task_id: Option<String>,
    pub note: Option<String>,
    pub output: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct CuratorProposalRecordView {
    pub job_id: String,
    pub job_trigger: String,
    pub job_status: String,
    pub job_task_id: Option<String>,
    pub focus: Vec<AnchorRef>,
    pub job_created_at: u64,
    pub job_started_at: Option<u64>,
    pub job_finished_at: Option<u64>,
    pub index: usize,
    pub kind: String,
    pub disposition: String,
    pub payload: Value,
    pub decided_at: Option<u64>,
    pub proposal_task_id: Option<String>,
    pub note: Option<String>,
    pub output: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct CuratorJobView {
    pub id: String,
    pub trigger: String,
    pub status: String,
    pub task_id: Option<String>,
    pub focus: Vec<AnchorRef>,
    pub created_at: u64,
    pub started_at: Option<u64>,
    pub finished_at: Option<u64>,
    pub proposals: Vec<CuratorProposalView>,
    pub diagnostics: Vec<QueryDiagnostic>,
    pub error: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct SuggestedQueryView {
    pub label: String,
    pub query: String,
    pub why: String,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct ReadContextView {
    pub target: SymbolView,
    pub target_block: FocusedBlockView,
    pub direct_links: Vec<SymbolView>,
    pub direct_link_blocks: Vec<FocusedBlockView>,
    pub suggested_reads: Vec<OwnerCandidateView>,
    pub tests: Vec<OwnerCandidateView>,
    pub test_blocks: Vec<FocusedBlockView>,
    pub related_memory: Vec<ScoredMemoryView>,
    pub recent_failures: Vec<OutcomeEvent>,
    pub validation_recipe: ValidationRecipeView,
    pub contracts: Vec<ContractPacketView>,
    pub why: Vec<String>,
    pub suggested_queries: Vec<SuggestedQueryView>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct EditContextView {
    pub target: SymbolView,
    pub target_block: FocusedBlockView,
    pub direct_links: Vec<SymbolView>,
    pub direct_link_blocks: Vec<FocusedBlockView>,
    pub suggested_reads: Vec<OwnerCandidateView>,
    pub write_paths: Vec<OwnerCandidateView>,
    pub write_path_blocks: Vec<FocusedBlockView>,
    pub tests: Vec<OwnerCandidateView>,
    pub test_blocks: Vec<FocusedBlockView>,
    pub related_memory: Vec<ScoredMemoryView>,
    pub recent_failures: Vec<OutcomeEvent>,
    pub blast_radius: ChangeImpactView,
    pub validation_recipe: ValidationRecipeView,
    pub checklist: Vec<String>,
    pub suggested_queries: Vec<SuggestedQueryView>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct ValidationContextView {
    pub target: SymbolView,
    pub target_block: FocusedBlockView,
    pub tests: Vec<OwnerCandidateView>,
    pub test_blocks: Vec<FocusedBlockView>,
    pub related_memory: Vec<ScoredMemoryView>,
    pub recent_failures: Vec<OutcomeEvent>,
    pub blast_radius: ChangeImpactView,
    pub validation_recipe: ValidationRecipeView,
    pub why: Vec<String>,
    pub suggested_queries: Vec<SuggestedQueryView>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct RecentChangeContextView {
    pub target: SymbolView,
    pub recent_events: Vec<OutcomeEvent>,
    pub recent_failures: Vec<OutcomeEvent>,
    pub co_change_neighbors: Vec<CoChangeView>,
    pub related_memory: Vec<ScoredMemoryView>,
    pub promoted_summaries: Vec<String>,
    pub lineage: Option<LineageView>,
    pub why: Vec<String>,
    pub suggested_queries: Vec<SuggestedQueryView>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct SpecImplementationClusterView {
    pub spec: SymbolView,
    pub notes: Vec<String>,
    pub implementations: Vec<SymbolView>,
    pub validations: Vec<SymbolView>,
    pub related: Vec<SymbolView>,
    pub read_path: Vec<OwnerCandidateView>,
    pub write_path: Vec<OwnerCandidateView>,
    pub persistence_path: Vec<OwnerCandidateView>,
    pub tests: Vec<OwnerCandidateView>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct SpecDriftExplanationView {
    pub spec: SymbolView,
    pub notes: Vec<String>,
    pub drift_reasons: Vec<String>,
    pub expectations: Vec<String>,
    pub observations: Vec<String>,
    pub gaps: Vec<String>,
    pub next_reads: Vec<OwnerCandidateView>,
    pub trust_signals: TrustSignalsView,
    pub cluster: SpecImplementationClusterView,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct DiscoveryBundleView {
    pub target: SymbolView,
    pub suggested_reads: Vec<OwnerCandidateView>,
    pub read_context: ReadContextView,
    pub edit_context: EditContextView,
    pub validation_context: ValidationContextView,
    pub recent_change_context: RecentChangeContextView,
    pub entrypoints: Vec<SymbolView>,
    pub where_used_direct: Vec<SymbolView>,
    pub where_used_behavioral: Vec<SymbolView>,
    pub suggested_queries: Vec<SuggestedQueryView>,
    pub relations: RelationsView,
    pub spec_cluster: Option<SpecImplementationClusterView>,
    pub spec_drift: Option<SpecDriftExplanationView>,
    pub lineage: Option<LineageView>,
    pub co_change_neighbors: Vec<CoChangeView>,
    pub related_failures: Vec<OutcomeEvent>,
    pub blast_radius: ChangeImpactView,
    pub validation_recipe: ValidationRecipeView,
    pub trust_signals: TrustSignalsView,
    pub why: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct SearchBundleView {
    pub query: String,
    pub results: Vec<SymbolView>,
    pub top_result: Option<SymbolView>,
    pub discovery: Option<DiscoveryBundleView>,
    pub focused_block: Option<FocusedBlockView>,
    pub read_context: Option<ReadContextView>,
    pub suggested_reads: Vec<OwnerCandidateView>,
    pub validation_context: Option<ValidationContextView>,
    pub recent_change_context: Option<RecentChangeContextView>,
    pub summary: BundleSummaryView,
    pub diagnostics: Vec<QueryDiagnostic>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct SymbolBundleView {
    pub query: String,
    pub result: Option<SymbolView>,
    pub candidates: Vec<SymbolView>,
    pub discovery: Option<DiscoveryBundleView>,
    pub focused_block: Option<FocusedBlockView>,
    pub read_context: Option<ReadContextView>,
    pub suggested_reads: Vec<OwnerCandidateView>,
    pub summary: BundleSummaryView,
    pub diagnostics: Vec<QueryDiagnostic>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct TextSearchBundleView {
    pub query: String,
    pub matches: Vec<TextSearchMatchView>,
    pub top_match: Option<TextSearchMatchView>,
    pub raw_context: Option<SourceSliceView>,
    pub semantic_query: Option<String>,
    pub semantic_results: Vec<SymbolView>,
    pub top_symbol: Option<SymbolView>,
    pub discovery: Option<DiscoveryBundleView>,
    pub focused_block: Option<FocusedBlockView>,
    pub read_context: Option<ReadContextView>,
    pub suggested_reads: Vec<OwnerCandidateView>,
    pub summary: BundleSummaryView,
    pub diagnostics: Vec<QueryDiagnostic>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct TargetBundleView {
    pub target: SymbolView,
    pub discovery: Option<DiscoveryBundleView>,
    pub focused_block: Option<FocusedBlockView>,
    pub diff: Vec<DiffHunkView>,
    pub edit_context: EditContextView,
    pub read_context: ReadContextView,
    pub suggested_reads: Vec<OwnerCandidateView>,
    pub likely_tests: Vec<FocusedBlockView>,
    pub summary: BundleSummaryView,
    pub diagnostics: Vec<QueryDiagnostic>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct BundleSummaryView {
    pub kind: String,
    pub result_count: usize,
    pub empty: bool,
    pub truncated: bool,
    pub ambiguous: bool,
    pub diagnostic_codes: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct QueryResultSummaryView {
    pub kind: String,
    pub json_bytes: usize,
    pub item_count: Option<usize>,
    pub truncated: bool,
    pub output_cap_hit: bool,
    pub result_cap_hit: bool,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct QueryPhaseView {
    pub operation: String,
    pub started_at: u64,
    pub duration_ms: u64,
    pub args_summary: Option<Value>,
    pub touched: Vec<String>,
    pub success: bool,
    pub error: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct McpCallPayloadSummaryView {
    pub kind: String,
    pub json_bytes: usize,
    pub item_count: Option<usize>,
    pub truncated: bool,
    pub excerpt: Option<Value>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct McpCallLogEntryView {
    pub id: String,
    pub call_type: String,
    pub name: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub view_name: Option<String>,
    pub summary: String,
    pub started_at: u64,
    pub duration_ms: u64,
    pub session_id: Option<String>,
    pub task_id: Option<String>,
    pub success: bool,
    pub error: Option<String>,
    pub operations: Vec<String>,
    pub touched: Vec<String>,
    pub diagnostics: Vec<QueryDiagnostic>,
    pub request: McpCallPayloadSummaryView,
    pub response: McpCallPayloadSummaryView,
    pub server_instance_id: String,
    pub process_id: u32,
    pub workspace_root: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub repo_id: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub worktree_id: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub log_path: Option<String>,
    pub trace_available: bool,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct McpCallTraceView {
    pub entry: McpCallLogEntryView,
    pub phases: Vec<QueryPhaseView>,
    pub request_payload: Option<Value>,
    pub request_preview: Option<Value>,
    pub response_preview: Option<Value>,
    pub metadata: Value,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct McpCallStatsBucketView {
    pub key: String,
    pub count: usize,
    pub error_count: usize,
    pub unique_task_count: usize,
    pub average_duration_ms: u64,
    pub max_duration_ms: u64,
    pub average_result_json_bytes: u64,
    pub max_result_json_bytes: u64,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct McpCallStatsView {
    pub total_calls: usize,
    pub success_count: usize,
    pub error_count: usize,
    pub average_duration_ms: u64,
    pub max_duration_ms: u64,
    pub by_call_type: Vec<McpCallStatsBucketView>,
    pub by_name: Vec<McpCallStatsBucketView>,
    pub by_view_name: Vec<McpCallStatsBucketView>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct QueryLogEntryView {
    pub id: String,
    pub kind: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub view_name: Option<String>,
    pub query_summary: String,
    pub query_text: String,
    pub started_at: u64,
    pub duration_ms: u64,
    pub session_id: String,
    pub task_id: Option<String>,
    pub success: bool,
    pub error: Option<String>,
    pub operations: Vec<String>,
    pub touched: Vec<String>,
    pub diagnostics: Vec<QueryDiagnostic>,
    pub result: QueryResultSummaryView,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct QueryTraceView {
    pub entry: QueryLogEntryView,
    pub phases: Vec<QueryPhaseView>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct SpecListEntryView {
    pub spec_id: String,
    pub title: String,
    pub source_path: String,
    pub declared_status: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub overall_status: Option<String>,
    pub created: String,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct SpecChecklistItemView {
    pub item_id: String,
    pub label: String,
    pub checked: bool,
    pub requirement_level: String,
    pub section_path: Vec<String>,
    pub line_number: usize,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct SpecStatusView {
    pub declared_status: String,
    pub checklist_posture: String,
    pub dependency_posture: String,
    pub overall_status: String,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct SpecDocumentView {
    pub spec_id: String,
    pub source_path: String,
    pub title: String,
    pub declared_status: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub overall_status: Option<String>,
    pub created: String,
    pub content_digest: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub git_revision: Option<String>,
    pub body: String,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct SpecCoverageRecordView {
    pub checklist_item_id: String,
    pub coverage_kind: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub coordination_ref: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct SpecSyncProvenanceRecordView {
    pub target_coordination_ref: String,
    pub sync_kind: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub source_revision: Option<String>,
    pub covered_checklist_items: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct SpecSyncBriefView {
    pub spec: SpecDocumentView,
    pub required_checklist_items: Vec<SpecChecklistItemView>,
    pub coverage: Vec<SpecCoverageRecordView>,
    pub linked_coordination_refs: Vec<SpecSyncProvenanceRecordView>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct ValidationFeedbackOptions {
    pub limit: Option<usize>,
    pub since: Option<u64>,
    pub task_id: Option<String>,
    pub verdict: Option<String>,
    pub category: Option<String>,
    pub contains: Option<String>,
    pub corrected_manually: Option<bool>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct ValidationFeedbackView {
    pub id: String,
    pub recorded_at: u64,
    pub task_id: Option<String>,
    pub context: String,
    pub anchors: Vec<AnchorRef>,
    pub prism_said: String,
    pub actually_true: String,
    pub category: String,
    pub verdict: String,
    pub corrected_manually: bool,
    pub correction: Option<String>,
    pub metadata: Value,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct QueryEnvelope {
    pub result: Value,
    pub diagnostics: Vec<QueryDiagnostic>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct QueryDiagnostic {
    pub code: String,
    pub message: String,
    pub data: Option<Value>,
}
