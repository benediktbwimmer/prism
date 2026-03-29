use prism_coordination::BlockerKind;
use prism_ir::{
    AnchorRef, ArtifactStatus, Capability, ClaimMode, ClaimStatus, ConflictOverlapKind,
    ConflictSeverity, CoordinationTaskStatus, EdgeKind, EdgeOrigin, Language, NodeKind,
    PlanEdgeKind, PlanKind, PlanNodeBlockerKind, PlanNodeKind, PlanNodeStatus, PlanScope,
    PlanStatus, Span,
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
pub struct AgentTaskBriefResultView {
    pub task_id: String,
    pub title: String,
    pub status: CoordinationTaskStatus,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub assignee: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub pending_handoff_to: Option<String>,
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
pub struct ToolActionSchemaView {
    pub action: String,
    pub required_fields: Vec<String>,
    pub fields: Vec<ToolFieldSchemaView>,
    pub input_schema: Value,
    pub example_input: Option<Value>,
    #[serde(default)]
    pub example_inputs: Vec<Value>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct ToolSchemaView {
    pub tool_name: String,
    pub schema_uri: String,
    pub description: String,
    pub example_input: Value,
    #[serde(default)]
    pub example_inputs: Vec<Value>,
    pub input_schema: Value,
    pub actions: Vec<ToolActionSchemaView>,
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
    pub summary: String,
    pub files: Vec<String>,
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
pub struct RuntimeMaterializationItemView {
    pub status: String,
    pub loaded_revision: u64,
    pub current_revision: Option<u64>,
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
pub struct RuntimeFreshnessView {
    pub fs_observed_revision: u64,
    pub fs_applied_revision: u64,
    pub fs_dirty: bool,
    pub last_refresh_path: Option<String>,
    pub last_refresh_timestamp: Option<String>,
    pub last_refresh_duration_ms: Option<u64>,
    pub last_workspace_build_ms: Option<u64>,
    pub last_daemon_ready_ms: Option<u64>,
    pub materialization: RuntimeMaterializationView,
    pub status: String,
    pub error: Option<String>,
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
    pub cache_path: String,
    pub cache_bytes: Option<u64>,
    pub health_path: String,
    pub health: RuntimeHealthView,
    pub daemon_count: usize,
    pub bridge_count: usize,
    pub connected_bridge_count: usize,
    pub orphan_bridge_count: usize,
    pub processes: Vec<RuntimeProcessView>,
    pub process_error: Option<String>,
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
    pub promoted_summaries: Vec<String>,
    pub approved_artifact_ids: Vec<String>,
    pub stale_artifact_ids: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
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
    pub promoted_summaries: Vec<String>,
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
pub struct PlanView {
    pub id: String,
    pub goal: String,
    pub status: PlanStatus,
    pub root_node_ids: Vec<String>,
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
    pub root_node_ids: Vec<String>,
    pub summary: PlanSummaryView,
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
pub struct PlanAcceptanceCriterionView {
    pub label: String,
    pub anchors: Vec<AnchorRef>,
    pub required_checks: Vec<ValidationRefView>,
    pub evidence_policy: String,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct PlanNodeView {
    pub id: String,
    pub plan_id: String,
    pub kind: PlanNodeKind,
    pub title: String,
    pub summary: Option<String>,
    pub status: PlanNodeStatus,
    pub bindings: PlanBindingView,
    pub acceptance: Vec<PlanAcceptanceCriterionView>,
    pub validation_refs: Vec<ValidationRefView>,
    pub is_abstract: bool,
    pub assignee: Option<String>,
    pub base_revision: WorkspaceRevisionView,
    pub priority: Option<u8>,
    pub tags: Vec<String>,
    pub metadata: Value,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct PlanEdgeView {
    pub id: String,
    pub plan_id: String,
    pub from: String,
    pub to: String,
    pub kind: PlanEdgeKind,
    pub summary: Option<String>,
    pub metadata: Value,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct PlanGraphView {
    pub id: String,
    pub scope: PlanScope,
    pub kind: PlanKind,
    pub title: String,
    pub goal: String,
    pub status: PlanStatus,
    pub revision: u64,
    pub root_node_ids: Vec<String>,
    pub tags: Vec<String>,
    pub created_from: Option<String>,
    pub metadata: Value,
    pub nodes: Vec<PlanNodeView>,
    pub edges: Vec<PlanEdgeView>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct PlanExecutionOverlayView {
    pub node_id: String,
    pub pending_handoff_to: Option<String>,
    pub session: Option<String>,
    pub effective_assignee: Option<String>,
    pub awaiting_handoff_from: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct PlanNodeBlockerView {
    pub kind: PlanNodeBlockerKind,
    pub summary: String,
    pub related_node_id: Option<String>,
    pub related_artifact_id: Option<String>,
    pub risk_score: Option<f32>,
    pub validation_checks: Vec<String>,
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
pub struct PlanNodeRecommendationView {
    pub node: PlanNodeView,
    pub actionable: bool,
    pub effective_assignee: Option<String>,
    pub score: f32,
    pub reasons: Vec<String>,
    pub blockers: Vec<PlanNodeBlockerView>,
    pub unblocks: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct CoordinationTaskView {
    pub id: String,
    pub plan_id: String,
    pub title: String,
    pub status: CoordinationTaskStatus,
    pub assignee: Option<String>,
    pub pending_handoff_to: Option<String>,
    pub anchors: Vec<AnchorRef>,
    pub depends_on: Vec<String>,
    pub base_revision: WorkspaceRevisionView,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct ClaimView {
    pub id: String,
    pub holder: String,
    pub task_id: Option<String>,
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
    pub plan: Option<PlanView>,
    pub plan_graph: Option<PlanGraphView>,
    pub plan_execution: Vec<PlanExecutionOverlayView>,
    pub plan_summary: Option<PlanSummaryView>,
    pub plan_next: Vec<PlanNodeRecommendationView>,
    pub ready_tasks: Vec<CoordinationTaskView>,
    pub pending_reviews: Vec<ArtifactView>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct TaskContextView {
    pub task: Option<CoordinationTaskView>,
    pub task_node: Option<PlanNodeView>,
    pub task_execution: Option<PlanExecutionOverlayView>,
    pub plan_graph: Option<PlanGraphView>,
    pub plan_summary: Option<PlanSummaryView>,
    pub plan_next: Vec<PlanNodeRecommendationView>,
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
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct ArtifactView {
    pub id: String,
    pub task_id: String,
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
pub struct QueryLogEntryView {
    pub id: String,
    pub kind: String,
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
