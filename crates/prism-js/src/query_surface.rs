use std::collections::{BTreeMap, BTreeSet};
use std::sync::OnceLock;

use prism_memory::TaskReplay;
use schemars::schema_for;
use serde_json::{Map, Value};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PrismSurfaceTypeRef {
    Unknown,
    Primitive(&'static str),
    NullablePrimitive(&'static str),
    Named(&'static str),
    NullableNamed(&'static str),
    ArrayOfNamed(&'static str),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct PrismRecordArgBundle {
    pub bundle_name: &'static str,
    pub arg_name: &'static str,
    pub arg_index: usize,
    pub allowed_keys: &'static [&'static str],
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct PrismApiMethodSpec {
    pub path: &'static str,
    pub declaration: Option<&'static str>,
    pub return_type: PrismSurfaceTypeRef,
    pub record_arg: Option<PrismRecordArgBundle>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum PrismSurfaceType {
    Unknown,
    Primitive(&'static str),
    Nullable(Box<PrismSurfaceType>),
    Array(Box<PrismSurfaceType>),
    Object(BTreeMap<String, PrismSurfaceType>),
}

macro_rules! method {
    ($path:literal, $decl:literal, $return:expr) => {
        PrismApiMethodSpec {
            path: $path,
            declaration: Some($decl),
            return_type: $return,
            record_arg: None,
        }
    };
    ($path:literal, $decl:literal, $return:expr, $record:expr) => {
        PrismApiMethodSpec {
            path: $path,
            declaration: Some($decl),
            return_type: $return,
            record_arg: Some($record),
        }
    };
}

macro_rules! helper {
    ($path:literal, $return:expr) => {
        PrismApiMethodSpec {
            path: $path,
            declaration: None,
            return_type: $return,
            record_arg: None,
        }
    };
    ($path:literal, $return:expr, $record:expr) => {
        PrismApiMethodSpec {
            path: $path,
            declaration: None,
            return_type: $return,
            record_arg: Some($record),
        }
    };
}

const CLAIM_PREVIEW_KEYS: &[&str] = &[
    "anchors",
    "anchor",
    "capability",
    "mode",
    "taskId",
    "task_id",
];
const CHANGED_FILES_KEYS: &[&str] = &["since", "limit", "taskId", "task_id", "path"];
const CHANGED_SYMBOLS_KEYS: &[&str] = &["since", "limit", "taskId", "task_id"];
const COMMAND_MEMORY_KEYS: &[&str] = &["taskId", "task_id"];
const CONCEPT_KEYS: &[&str] = &[
    "limit",
    "verbosity",
    "includeBindingMetadata",
    "include_binding_metadata",
];
const CONTRACTS_KEYS: &[&str] = &["status", "scope", "contains", "kind", "limit"];
const CURATOR_JOB_KEYS: &[&str] = &["status", "trigger", "limit"];
const CURATOR_PROPOSALS_KEYS: &[&str] = &[
    "status",
    "trigger",
    "kind",
    "disposition",
    "taskId",
    "task_id",
    "limit",
];
const DECODE_CONCEPT_KEYS: &[&str] = &[
    "handle",
    "query",
    "lens",
    "verbosity",
    "includeBindingMetadata",
    "include_binding_metadata",
];
const DIFF_FOR_KEYS: &[&str] = &["since", "limit", "taskId", "task_id"];
const EDIT_SLICE_KEYS: &[&str] = &["beforeLines", "afterLines", "maxLines", "maxChars"];
const EXCERPT_KEYS: &[&str] = &["contextLines", "maxLines", "maxChars"];
const FILE_AROUND_KEYS: &[&str] = &[
    "line",
    "before",
    "after",
    "beforeLines",
    "afterLines",
    "maxChars",
];
const FILE_READ_KEYS: &[&str] = &["startLine", "endLine", "maxChars"];
const IMPLEMENTATION_FOR_KEYS: &[&str] = &["mode", "ownerKind", "owner_kind"];
const IMPACT_KEYS: &[&str] = &["taskId", "task_id", "target", "paths"];
const MCP_LOG_KEYS: &[&str] = &[
    "limit",
    "since",
    "scope",
    "callType",
    "call_type",
    "name",
    "taskId",
    "task_id",
    "worktreeId",
    "worktree_id",
    "repoId",
    "repo_id",
    "workspaceRoot",
    "workspace_root",
    "sessionId",
    "session_id",
    "serverInstanceId",
    "server_instance_id",
    "processId",
    "process_id",
    "success",
    "minDurationMs",
    "min_duration_ms",
    "contains",
];
const MEMORY_EVENTS_KEYS: &[&str] = &[
    "memoryId", "focus", "text", "limit", "kinds", "actions", "scope", "taskId", "task_id", "since",
];
const MEMORY_OUTCOMES_KEYS: &[&str] = &[
    "focus", "taskId", "task_id", "kinds", "result", "actor", "since", "limit",
];
const MEMORY_RECALL_KEYS: &[&str] = &["focus", "text", "limit", "kinds", "since"];
const NEXT_READS_KEYS: &[&str] = &["limit"];
const OWNERS_KEYS: &[&str] = &["kind", "limit"];
const PLANS_KEYS: &[&str] = &["status", "scope", "contains", "limit"];
const POLICY_VIOLATIONS_KEYS: &[&str] = &["planId", "plan_id", "taskId", "task_id", "limit"];
const QUERY_LOG_KEYS: &[&str] = &[
    "limit",
    "since",
    "target",
    "operation",
    "taskId",
    "task_id",
    "minDurationMs",
    "min_duration_ms",
];
const RECENT_PATCHES_KEYS: &[&str] = &["target", "since", "limit", "taskId", "task_id", "path"];
const RUNTIME_LOGS_KEYS: &[&str] = &[
    "limit",
    "scope",
    "worktreeId",
    "worktree_id",
    "level",
    "target",
    "contains",
];
const RUNTIME_TIMELINE_KEYS: &[&str] = &["limit", "scope", "worktreeId", "worktree_id", "contains"];
const SEARCH_KEYS: &[&str] = &[
    "limit",
    "kind",
    "path",
    "module",
    "taskId",
    "task_id",
    "pathMode",
    "path_mode",
    "strategy",
    "structuredPath",
    "structured_path",
    "topLevelOnly",
    "top_level_only",
    "preferCallableCode",
    "prefer_callable_code",
    "preferEditableTargets",
    "prefer_editable_targets",
    "preferBehavioralOwners",
    "prefer_behavioral_owners",
    "ownerKind",
    "owner_kind",
    "includeInferred",
    "include_inferred",
];
const SEARCH_BUNDLE_KEYS: &[&str] = &[
    "limit",
    "kind",
    "path",
    "module",
    "taskId",
    "task_id",
    "pathMode",
    "path_mode",
    "strategy",
    "structuredPath",
    "structured_path",
    "topLevelOnly",
    "top_level_only",
    "preferCallableCode",
    "prefer_callable_code",
    "preferEditableTargets",
    "prefer_editable_targets",
    "preferBehavioralOwners",
    "prefer_behavioral_owners",
    "ownerKind",
    "owner_kind",
    "includeInferred",
    "include_inferred",
    "includeDiscovery",
    "suggestedReadLimit",
    "suggested_read_limit",
];
const SEARCH_TEXT_KEYS: &[&str] = &[
    "regex",
    "caseSensitive",
    "case_sensitive",
    "path",
    "glob",
    "limit",
    "contextLines",
    "context_lines",
];
const TARGET_BUNDLE_KEYS: &[&str] = &[
    "since",
    "limit",
    "taskId",
    "task_id",
    "path",
    "includeDiscovery",
    "suggestedReadLimit",
    "suggested_read_limit",
];
const TASK_CHANGES_KEYS: &[&str] = &["since", "limit", "path"];
const TASK_JOURNAL_KEYS: &[&str] = &["eventLimit", "event_limit", "memoryLimit", "memory_limit"];
const TEXT_SEARCH_BUNDLE_KEYS: &[&str] = &[
    "regex",
    "caseSensitive",
    "case_sensitive",
    "path",
    "glob",
    "limit",
    "contextLines",
    "context_lines",
    "semanticQuery",
    "semanticLimit",
    "semanticKind",
    "ownerKind",
    "owner_kind",
    "strategy",
    "includeDiscovery",
    "includeInferred",
    "include_inferred",
    "aroundBefore",
    "aroundAfter",
    "aroundMaxChars",
    "preferCallableCode",
    "prefer_callable_code",
    "preferEditableTargets",
    "prefer_editable_targets",
    "preferBehavioralOwners",
    "prefer_behavioral_owners",
    "suggestedReadLimit",
    "suggested_read_limit",
];
const VALIDATION_FEEDBACK_KEYS: &[&str] = &[
    "limit",
    "since",
    "taskId",
    "task_id",
    "verdict",
    "category",
    "contains",
    "correctedManually",
    "corrected_manually",
];
const VALIDATION_PLAN_KEYS: &[&str] = &["taskId", "task_id", "target", "paths"];
const WHERE_USED_KEYS: &[&str] = &["mode", "limit"];

pub fn prism_api_method_specs() -> &'static [PrismApiMethodSpec] {
    static SPECS: &[PrismApiMethodSpec] = &[
        method!("prism.from", "from(runtimeId: string): PrismApi;", PrismSurfaceTypeRef::Unknown),
        method!("prism.symbol", "symbol(query: string): SymbolView | null;", PrismSurfaceTypeRef::NullableNamed("SymbolView")),
        method!("prism.symbolBundle", "symbolBundle(query: string, options?: SymbolBundleOptions): SymbolBundleView;", PrismSurfaceTypeRef::Named("SymbolBundleView")),
        method!("prism.symbols", "symbols(query: string): SymbolView[];", PrismSurfaceTypeRef::ArrayOfNamed("SymbolView")),
        method!("prism.search", "search(query: string, options?: SearchOptions): SymbolView[];", PrismSurfaceTypeRef::ArrayOfNamed("SymbolView"), PrismRecordArgBundle { bundle_name: "search", arg_name: "options", arg_index: 1, allowed_keys: SEARCH_KEYS }),
        method!("prism.concepts", "concepts(query: string, options?: ConceptQueryOptions): ConceptPacketView[];", PrismSurfaceTypeRef::ArrayOfNamed("ConceptPacketView"), PrismRecordArgBundle { bundle_name: "concept", arg_name: "options", arg_index: 1, allowed_keys: CONCEPT_KEYS }),
        method!("prism.concept", "concept(query: string, options?: ConceptQueryOptions): ConceptPacketView | null;", PrismSurfaceTypeRef::NullableNamed("ConceptPacketView"), PrismRecordArgBundle { bundle_name: "concept", arg_name: "options", arg_index: 1, allowed_keys: CONCEPT_KEYS }),
        method!("prism.conceptByHandle", "conceptByHandle(handle: string, options?: { verbosity?: \"summary\" | \"standard\" | \"full\"; includeBindingMetadata?: boolean }): ConceptPacketView | null;", PrismSurfaceTypeRef::NullableNamed("ConceptPacketView"), PrismRecordArgBundle { bundle_name: "concept", arg_name: "options", arg_index: 1, allowed_keys: CONCEPT_KEYS }),
        method!("prism.contract", "contract(query: string): ContractPacketView | null;", PrismSurfaceTypeRef::NullableNamed("ContractPacketView")),
        method!("prism.contracts", "contracts(options?: ContractListOptions): ContractPacketView[];", PrismSurfaceTypeRef::ArrayOfNamed("ContractPacketView"), PrismRecordArgBundle { bundle_name: "contracts", arg_name: "options", arg_index: 0, allowed_keys: CONTRACTS_KEYS }),
        method!("prism.contractsFor", "contractsFor(target: QueryTarget): ContractPacketView[];", PrismSurfaceTypeRef::ArrayOfNamed("ContractPacketView")),
        method!("prism.conceptRelations", "conceptRelations(handle: string): ConceptRelationView[];", PrismSurfaceTypeRef::ArrayOfNamed("ConceptRelationView")),
        method!("prism.decodeConcept", "decodeConcept(input: { handle?: string; query?: string; lens?: \"open\" | \"workset\" | \"validation\" | \"timeline\" | \"memory\"; verbosity?: \"summary\" | \"standard\" | \"full\"; includeBindingMetadata?: boolean }): ConceptDecodeView | null;", PrismSurfaceTypeRef::NullableNamed("ConceptDecodeView"), PrismRecordArgBundle { bundle_name: "decodeConcept", arg_name: "input", arg_index: 0, allowed_keys: DECODE_CONCEPT_KEYS }),
        method!("prism.searchText", "searchText(query: string, options?: SearchTextOptions): TextSearchMatchView[];", PrismSurfaceTypeRef::ArrayOfNamed("TextSearchMatchView"), PrismRecordArgBundle { bundle_name: "searchText", arg_name: "options", arg_index: 1, allowed_keys: SEARCH_TEXT_KEYS }),
        method!("prism.textSearchBundle", "textSearchBundle(query: string, options?: TextSearchBundleOptions): TextSearchBundleView;", PrismSurfaceTypeRef::Named("TextSearchBundleView"), PrismRecordArgBundle { bundle_name: "textSearchBundle", arg_name: "options", arg_index: 1, allowed_keys: TEXT_SEARCH_BUNDLE_KEYS }),
        method!("prism.tools", "tools(): ToolCatalogEntryView[];", PrismSurfaceTypeRef::ArrayOfNamed("ToolCatalogEntryView")),
        method!("prism.tool", "tool(name: string): ToolSchemaView | null;", PrismSurfaceTypeRef::NullableNamed("ToolSchemaView")),
        method!("prism.validateToolInput", "validateToolInput(name: string, input: unknown): ToolInputValidationView;", PrismSurfaceTypeRef::Named("ToolInputValidationView")),
        method!("prism.entrypoints", "entrypoints(): SymbolView[];", PrismSurfaceTypeRef::ArrayOfNamed("SymbolView")),
        method!("prism.file", "file(path: string): FileView;", PrismSurfaceTypeRef::Unknown),
        helper!("prism.file(path).read", PrismSurfaceTypeRef::NullableNamed("SourceExcerptView"), PrismRecordArgBundle { bundle_name: "fileRead", arg_name: "options", arg_index: 0, allowed_keys: FILE_READ_KEYS }),
        helper!("prism.file(path).around", PrismSurfaceTypeRef::NullableNamed("SourceSliceView"), PrismRecordArgBundle { bundle_name: "fileAround", arg_name: "options", arg_index: 0, allowed_keys: FILE_AROUND_KEYS }),
        method!("prism.plans", "plans(options?: PlanListOptions): PlanListEntryView[];", PrismSurfaceTypeRef::ArrayOfNamed("PlanListEntryView"), PrismRecordArgBundle { bundle_name: "plans", arg_name: "options", arg_index: 0, allowed_keys: PLANS_KEYS }),
        method!("prism.plan", "plan(planId: string): PlanView | null;", PrismSurfaceTypeRef::NullableNamed("PlanView")),
        method!("prism.planGraph", "planGraph(planId: string): PlanGraphView | null;", PrismSurfaceTypeRef::Unknown),
        method!("prism.planProjectionAt", "planProjectionAt(planId: string, at: number): AdHocPlanProjectionView | null;", PrismSurfaceTypeRef::NullableNamed("AdHocPlanProjectionView")),
        method!("prism.planProjectionDiff", "planProjectionDiff(planId: string, from: number, to: number): AdHocPlanProjectionDiffView;", PrismSurfaceTypeRef::Named("AdHocPlanProjectionDiffView")),
        method!("prism.planExecution", "planExecution(planId: string): PlanExecutionOverlayView[];", PrismSurfaceTypeRef::ArrayOfNamed("PlanExecutionOverlayView")),
        method!("prism.planReadyNodes", "planReadyNodes(planId: string): PlanNodeView[];", PrismSurfaceTypeRef::ArrayOfNamed("PlanNodeView")),
        method!("prism.planNodeBlockers", "planNodeBlockers(planId: string, nodeId: string): PlanNodeBlockerView[];", PrismSurfaceTypeRef::ArrayOfNamed("PlanNodeBlockerView")),
        method!("prism.planSummary", "planSummary(planId: string): PlanSummaryView | null;", PrismSurfaceTypeRef::NullableNamed("PlanSummaryView")),
        method!("prism.planNext", "planNext(planId: string, limit?: number): PlanNodeRecommendationView[];", PrismSurfaceTypeRef::ArrayOfNamed("PlanNodeRecommendationView")),
        method!("prism.portfolioNext", "portfolioNext(limit?: number): PlanNodeRecommendationView[];", PrismSurfaceTypeRef::ArrayOfNamed("PlanNodeRecommendationView")),
        method!("prism.task", "task(taskId: string): CoordinationTaskView | null;", PrismSurfaceTypeRef::NullableNamed("CoordinationTaskView")),
        method!("prism.readyTasks", "readyTasks(planId: string): CoordinationTaskView[];", PrismSurfaceTypeRef::ArrayOfNamed("CoordinationTaskView")),
        method!("prism.claims", "claims(target: SymbolView | NodeId | AnchorRef | Array<SymbolView | NodeId | AnchorRef>): ClaimView[];", PrismSurfaceTypeRef::ArrayOfNamed("ClaimView")),
        method!("prism.conflicts", "conflicts(target: SymbolView | NodeId | AnchorRef | Array<SymbolView | NodeId | AnchorRef>): ConflictView[];", PrismSurfaceTypeRef::ArrayOfNamed("ConflictView")),
        method!("prism.blockers", "blockers(taskId: string): BlockerView[];", PrismSurfaceTypeRef::ArrayOfNamed("BlockerView")),
        method!("prism.pendingReviews", "pendingReviews(planId?: string): ArtifactView[];", PrismSurfaceTypeRef::ArrayOfNamed("ArtifactView")),
        method!("prism.artifacts", "artifacts(taskId: string): ArtifactView[];", PrismSurfaceTypeRef::ArrayOfNamed("ArtifactView")),
        method!("prism.policyViolations", "policyViolations(input?: { planId?: string; taskId?: string; limit?: number }): PolicyViolationRecordView[];", PrismSurfaceTypeRef::ArrayOfNamed("PolicyViolationRecordView"), PrismRecordArgBundle { bundle_name: "policyViolations", arg_name: "input", arg_index: 0, allowed_keys: POLICY_VIOLATIONS_KEYS }),
        method!("prism.taskBlastRadius", "taskBlastRadius(taskId: string): ChangeImpactView | null;", PrismSurfaceTypeRef::NullableNamed("ChangeImpactView")),
        method!("prism.taskValidationRecipe", "taskValidationRecipe(taskId: string): TaskValidationRecipeView | null;", PrismSurfaceTypeRef::NullableNamed("TaskValidationRecipeView")),
        method!("prism.taskRisk", "taskRisk(taskId: string): TaskRiskView | null;", PrismSurfaceTypeRef::NullableNamed("TaskRiskView")),
        method!("prism.repoPlaybook", "repoPlaybook(): RepoPlaybookView;", PrismSurfaceTypeRef::Named("RepoPlaybookView")),
        method!("prism.validationPlan", "validationPlan(input: { taskId?: string; target?: QueryTarget; paths?: string[] }): ValidationPlanView;", PrismSurfaceTypeRef::Named("ValidationPlanView"), PrismRecordArgBundle { bundle_name: "validationPlan", arg_name: "input", arg_index: 0, allowed_keys: VALIDATION_PLAN_KEYS }),
        method!("prism.impact", "impact(input: { taskId?: string; target?: QueryTarget; paths?: string[] }): ImpactView;", PrismSurfaceTypeRef::Named("ImpactView"), PrismRecordArgBundle { bundle_name: "impact", arg_name: "input", arg_index: 0, allowed_keys: IMPACT_KEYS }),
        method!("prism.afterEdit", "afterEdit(input?: { taskId?: string; target?: QueryTarget; paths?: string[] }): AfterEditView;", PrismSurfaceTypeRef::Named("AfterEditView"), PrismRecordArgBundle { bundle_name: "afterEdit", arg_name: "input", arg_index: 0, allowed_keys: IMPACT_KEYS }),
        method!("prism.commandMemory", "commandMemory(input?: { taskId?: string }): CommandMemoryView;", PrismSurfaceTypeRef::Named("CommandMemoryView"), PrismRecordArgBundle { bundle_name: "commandMemory", arg_name: "input", arg_index: 0, allowed_keys: COMMAND_MEMORY_KEYS }),
        method!("prism.artifactRisk", "artifactRisk(artifactId: string): ArtifactRiskView | null;", PrismSurfaceTypeRef::NullableNamed("ArtifactRiskView")),
        method!("prism.taskIntent", "taskIntent(taskId: string): TaskIntentView | null;", PrismSurfaceTypeRef::NullableNamed("TaskIntentView")),
        method!("prism.coordinationInbox", "coordinationInbox(planId: string): CoordinationInboxView;", PrismSurfaceTypeRef::Named("CoordinationInboxView")),
        method!("prism.taskContext", "taskContext(taskId: string): TaskContextView;", PrismSurfaceTypeRef::Named("TaskContextView")),
        method!("prism.claimPreview", "claimPreview(input: {\n    anchors: Array<SymbolView | NodeId | AnchorRef>;\n    capability: string;\n    mode?: string;\n    taskId?: string;\n  }): ClaimPreviewView;", PrismSurfaceTypeRef::Named("ClaimPreviewView"), PrismRecordArgBundle { bundle_name: "claimPreview", arg_name: "input", arg_index: 0, allowed_keys: CLAIM_PREVIEW_KEYS }),
        method!("prism.simulateClaim", "simulateClaim(input: {\n    anchors: Array<SymbolView | NodeId | AnchorRef>;\n    capability: string;\n    mode?: string;\n    taskId?: string;\n  }): ConflictView[];", PrismSurfaceTypeRef::ArrayOfNamed("ConflictView"), PrismRecordArgBundle { bundle_name: "claimPreview", arg_name: "input", arg_index: 0, allowed_keys: CLAIM_PREVIEW_KEYS }),
        method!("prism.full", "full(target: QueryTarget): string | null;", PrismSurfaceTypeRef::NullablePrimitive("string")),
        method!("prism.excerpt", "excerpt(target: QueryTarget, options?: SourceExcerptOptions): SourceExcerptView | null;", PrismSurfaceTypeRef::NullableNamed("SourceExcerptView"), PrismRecordArgBundle { bundle_name: "excerpt", arg_name: "options", arg_index: 1, allowed_keys: EXCERPT_KEYS }),
        method!("prism.editSlice", "editSlice(target: QueryTarget, options?: EditSliceOptions): SourceSliceView | null;", PrismSurfaceTypeRef::NullableNamed("SourceSliceView"), PrismRecordArgBundle { bundle_name: "editSlice", arg_name: "options", arg_index: 1, allowed_keys: EDIT_SLICE_KEYS }),
        method!("prism.focusedBlock", "focusedBlock(target: QueryTarget, options?: EditSliceOptions): FocusedBlockView | null;", PrismSurfaceTypeRef::NullableNamed("FocusedBlockView"), PrismRecordArgBundle { bundle_name: "editSlice", arg_name: "options", arg_index: 1, allowed_keys: EDIT_SLICE_KEYS }),
        method!("prism.lineage", "lineage(target: QueryTarget): LineageView | null;", PrismSurfaceTypeRef::NullableNamed("LineageView")),
        method!("prism.coChangeNeighbors", "coChangeNeighbors(target: QueryTarget): CoChangeView[];", PrismSurfaceTypeRef::ArrayOfNamed("CoChangeView")),
        method!("prism.relatedFailures", "relatedFailures(target: QueryTarget): OutcomeEvent[];", PrismSurfaceTypeRef::Unknown),
        method!("prism.blastRadius", "blastRadius(target: QueryTarget): ChangeImpactView | null;", PrismSurfaceTypeRef::NullableNamed("ChangeImpactView")),
        method!("prism.validationRecipe", "validationRecipe(target: QueryTarget): ValidationRecipeView | null;", PrismSurfaceTypeRef::NullableNamed("ValidationRecipeView")),
        method!("prism.readContext", "readContext(target: QueryTarget): ReadContextView | null;", PrismSurfaceTypeRef::NullableNamed("ReadContextView")),
        method!("prism.editContext", "editContext(target: QueryTarget): EditContextView | null;", PrismSurfaceTypeRef::NullableNamed("EditContextView")),
        method!("prism.validationContext", "validationContext(target: QueryTarget): ValidationContextView | null;", PrismSurfaceTypeRef::NullableNamed("ValidationContextView")),
        method!("prism.recentChangeContext", "recentChangeContext(target: QueryTarget): RecentChangeContextView | null;", PrismSurfaceTypeRef::NullableNamed("RecentChangeContextView")),
        method!("prism.discovery", "discovery(target: QueryTarget): DiscoveryBundleView | null;", PrismSurfaceTypeRef::NullableNamed("DiscoveryBundleView")),
        method!("prism.searchBundle", "searchBundle(query: string, options?: SearchBundleOptions): SearchBundleView;", PrismSurfaceTypeRef::Named("SearchBundleView"), PrismRecordArgBundle { bundle_name: "searchBundle", arg_name: "options", arg_index: 1, allowed_keys: SEARCH_BUNDLE_KEYS }),
        method!("prism.targetBundle", "targetBundle(target: QueryTarget | SearchBundleView | DiscoveryBundleView, options?: TargetBundleOptions): TargetBundleView | null;", PrismSurfaceTypeRef::Unknown, PrismRecordArgBundle { bundle_name: "targetBundle", arg_name: "options", arg_index: 1, allowed_keys: TARGET_BUNDLE_KEYS }),
        method!("prism.nextReads", "nextReads(target: QueryTarget, options?: NextReadsOptions): OwnerCandidateView[];", PrismSurfaceTypeRef::ArrayOfNamed("OwnerCandidateView"), PrismRecordArgBundle { bundle_name: "nextReads", arg_name: "options", arg_index: 1, allowed_keys: NEXT_READS_KEYS }),
        method!("prism.whereUsed", "whereUsed(target: QueryTarget, options?: WhereUsedOptions): SymbolView[];", PrismSurfaceTypeRef::ArrayOfNamed("SymbolView"), PrismRecordArgBundle { bundle_name: "whereUsed", arg_name: "options", arg_index: 1, allowed_keys: WHERE_USED_KEYS }),
        method!("prism.entrypointsFor", "entrypointsFor(target: QueryTarget, options?: NextReadsOptions): SymbolView[];", PrismSurfaceTypeRef::ArrayOfNamed("SymbolView"), PrismRecordArgBundle { bundle_name: "nextReads", arg_name: "options", arg_index: 1, allowed_keys: NEXT_READS_KEYS }),
        method!("prism.specFor", "specFor(target: QueryTarget): SymbolView[];", PrismSurfaceTypeRef::ArrayOfNamed("SymbolView")),
        method!("prism.implementationFor", "implementationFor(target: QueryTarget, options?: ImplementationOptions): SymbolView[];", PrismSurfaceTypeRef::ArrayOfNamed("SymbolView"), PrismRecordArgBundle { bundle_name: "implementationFor", arg_name: "options", arg_index: 1, allowed_keys: IMPLEMENTATION_FOR_KEYS }),
        method!("prism.owners", "owners(target: QueryTarget, options?: OwnerLookupOptions): OwnerCandidateView[];", PrismSurfaceTypeRef::ArrayOfNamed("OwnerCandidateView"), PrismRecordArgBundle { bundle_name: "owners", arg_name: "options", arg_index: 1, allowed_keys: OWNERS_KEYS }),
        method!("prism.driftCandidates", "driftCandidates(limit?: number): DriftCandidateView[];", PrismSurfaceTypeRef::ArrayOfNamed("DriftCandidateView")),
        method!("prism.specCluster", "specCluster(target: QueryTarget): SpecImplementationClusterView | null;", PrismSurfaceTypeRef::NullableNamed("SpecImplementationClusterView")),
        method!("prism.explainDrift", "explainDrift(target: QueryTarget): SpecDriftExplanationView | null;", PrismSurfaceTypeRef::NullableNamed("SpecDriftExplanationView")),
        method!("prism.resumeTask", "resumeTask(taskId: string): TaskReplay;", PrismSurfaceTypeRef::Named("TaskReplay")),
        method!("prism.taskJournal", "taskJournal(taskId: string, options?: TaskJournalOptions): TaskJournalView;", PrismSurfaceTypeRef::Named("TaskJournalView"), PrismRecordArgBundle { bundle_name: "taskJournal", arg_name: "options", arg_index: 1, allowed_keys: TASK_JOURNAL_KEYS }),
        method!("prism.changedFiles", "changedFiles(options?: ChangedFilesOptions): ChangedFileView[];", PrismSurfaceTypeRef::ArrayOfNamed("ChangedFileView"), PrismRecordArgBundle { bundle_name: "changedFiles", arg_name: "options", arg_index: 0, allowed_keys: CHANGED_FILES_KEYS }),
        method!("prism.changedSymbols", "changedSymbols(path: string, options?: ChangedFilesOptions): ChangedSymbolView[];", PrismSurfaceTypeRef::ArrayOfNamed("ChangedSymbolView"), PrismRecordArgBundle { bundle_name: "changedSymbols", arg_name: "options", arg_index: 1, allowed_keys: CHANGED_SYMBOLS_KEYS }),
        method!("prism.recentPatches", "recentPatches(options?: RecentPatchesOptions): PatchEventView[];", PrismSurfaceTypeRef::ArrayOfNamed("PatchEventView"), PrismRecordArgBundle { bundle_name: "recentPatches", arg_name: "options", arg_index: 0, allowed_keys: RECENT_PATCHES_KEYS }),
        method!("prism.diffFor", "diffFor(target: QueryTarget, options?: DiffForOptions): DiffHunkView[];", PrismSurfaceTypeRef::ArrayOfNamed("DiffHunkView"), PrismRecordArgBundle { bundle_name: "diffFor", arg_name: "options", arg_index: 1, allowed_keys: DIFF_FOR_KEYS }),
        method!("prism.taskChanges", "taskChanges(taskId: string, options?: ChangedFilesOptions): PatchEventView[];", PrismSurfaceTypeRef::ArrayOfNamed("PatchEventView"), PrismRecordArgBundle { bundle_name: "taskChanges", arg_name: "options", arg_index: 1, allowed_keys: TASK_CHANGES_KEYS }),
        method!("prism.connectionInfo", "connectionInfo(): ConnectionInfoView;", PrismSurfaceTypeRef::Named("ConnectionInfoView")),
        method!("prism.runtimeStatus", "runtimeStatus(): RuntimeStatusView;", PrismSurfaceTypeRef::Named("RuntimeStatusView")),
        method!("prism.runtimeLogs", "runtimeLogs(options?: RuntimeLogOptions): RuntimeLogEventView[];", PrismSurfaceTypeRef::ArrayOfNamed("RuntimeLogEventView"), PrismRecordArgBundle { bundle_name: "runtimeLogs", arg_name: "options", arg_index: 0, allowed_keys: RUNTIME_LOGS_KEYS }),
        method!("prism.runtimeTimeline", "runtimeTimeline(options?: RuntimeTimelineOptions): RuntimeLogEventView[];", PrismSurfaceTypeRef::ArrayOfNamed("RuntimeLogEventView"), PrismRecordArgBundle { bundle_name: "runtimeTimeline", arg_name: "options", arg_index: 0, allowed_keys: RUNTIME_TIMELINE_KEYS }),
        method!("prism.validationFeedback", "validationFeedback(options?: ValidationFeedbackOptions): ValidationFeedbackView[];", PrismSurfaceTypeRef::ArrayOfNamed("ValidationFeedbackView"), PrismRecordArgBundle { bundle_name: "validationFeedback", arg_name: "options", arg_index: 0, allowed_keys: VALIDATION_FEEDBACK_KEYS }),
        method!("prism.memoryRecall", "memoryRecall(options?: MemoryRecallOptions): ScoredMemoryView[];", PrismSurfaceTypeRef::ArrayOfNamed("ScoredMemoryView"), PrismRecordArgBundle { bundle_name: "memoryRecall", arg_name: "options", arg_index: 0, allowed_keys: MEMORY_RECALL_KEYS }),
        method!("prism.memoryOutcomes", "memoryOutcomes(options?: MemoryOutcomeOptions): OutcomeEvent[];", PrismSurfaceTypeRef::Unknown, PrismRecordArgBundle { bundle_name: "memoryOutcomes", arg_name: "options", arg_index: 0, allowed_keys: MEMORY_OUTCOMES_KEYS }),
        method!("prism.memoryEvents", "memoryEvents(options?: MemoryEventOptions): MemoryEventView[];", PrismSurfaceTypeRef::ArrayOfNamed("MemoryEventView"), PrismRecordArgBundle { bundle_name: "memoryEvents", arg_name: "options", arg_index: 0, allowed_keys: MEMORY_EVENTS_KEYS }),
        method!("prism.mcpLog", "mcpLog(options?: McpLogOptions): McpCallLogEntryView[];", PrismSurfaceTypeRef::ArrayOfNamed("McpCallLogEntryView"), PrismRecordArgBundle { bundle_name: "mcpLog", arg_name: "options", arg_index: 0, allowed_keys: MCP_LOG_KEYS }),
        method!("prism.slowMcpCalls", "slowMcpCalls(options?: McpLogOptions): McpCallLogEntryView[];", PrismSurfaceTypeRef::ArrayOfNamed("McpCallLogEntryView"), PrismRecordArgBundle { bundle_name: "mcpLog", arg_name: "options", arg_index: 0, allowed_keys: MCP_LOG_KEYS }),
        method!("prism.mcpTrace", "mcpTrace(id: string): McpCallTraceView | null;", PrismSurfaceTypeRef::NullableNamed("McpCallTraceView")),
        method!("prism.mcpStats", "mcpStats(options?: McpLogOptions): McpCallStatsView;", PrismSurfaceTypeRef::Named("McpCallStatsView"), PrismRecordArgBundle { bundle_name: "mcpLog", arg_name: "options", arg_index: 0, allowed_keys: MCP_LOG_KEYS }),
        method!("prism.queryLog", "queryLog(options?: QueryLogOptions): QueryLogEntryView[];", PrismSurfaceTypeRef::ArrayOfNamed("QueryLogEntryView"), PrismRecordArgBundle { bundle_name: "queryLog", arg_name: "options", arg_index: 0, allowed_keys: QUERY_LOG_KEYS }),
        method!("prism.slowQueries", "slowQueries(options?: QueryLogOptions): QueryLogEntryView[];", PrismSurfaceTypeRef::ArrayOfNamed("QueryLogEntryView"), PrismRecordArgBundle { bundle_name: "queryLog", arg_name: "options", arg_index: 0, allowed_keys: QUERY_LOG_KEYS }),
        method!("prism.queryTrace", "queryTrace(id: string): QueryTraceView | null;", PrismSurfaceTypeRef::NullableNamed("QueryTraceView")),
        method!("prism.diagnostics", "diagnostics(): QueryDiagnostic[];", PrismSurfaceTypeRef::ArrayOfNamed("QueryDiagnostic")),
        method!("prism.connection.info", "info(): ConnectionInfoView;", PrismSurfaceTypeRef::Named("ConnectionInfoView")),
        method!("prism.runtime.status", "status(): RuntimeStatusView;", PrismSurfaceTypeRef::Named("RuntimeStatusView")),
        method!("prism.runtime.logs", "logs(options?: RuntimeLogOptions): RuntimeLogEventView[];", PrismSurfaceTypeRef::ArrayOfNamed("RuntimeLogEventView"), PrismRecordArgBundle { bundle_name: "runtimeLogs", arg_name: "options", arg_index: 0, allowed_keys: RUNTIME_LOGS_KEYS }),
        method!("prism.runtime.timeline", "timeline(options?: RuntimeTimelineOptions): RuntimeLogEventView[];", PrismSurfaceTypeRef::ArrayOfNamed("RuntimeLogEventView"), PrismRecordArgBundle { bundle_name: "runtimeTimeline", arg_name: "options", arg_index: 0, allowed_keys: RUNTIME_TIMELINE_KEYS }),
        method!("prism.memory.recall", "recall(options?: MemoryRecallOptions): ScoredMemoryView[];", PrismSurfaceTypeRef::ArrayOfNamed("ScoredMemoryView"), PrismRecordArgBundle { bundle_name: "memoryRecall", arg_name: "options", arg_index: 0, allowed_keys: MEMORY_RECALL_KEYS }),
        method!("prism.memory.outcomes", "outcomes(options?: MemoryOutcomeOptions): OutcomeEvent[];", PrismSurfaceTypeRef::Unknown, PrismRecordArgBundle { bundle_name: "memoryOutcomes", arg_name: "options", arg_index: 0, allowed_keys: MEMORY_OUTCOMES_KEYS }),
        method!("prism.memory.events", "events(options?: MemoryEventOptions): MemoryEventView[];", PrismSurfaceTypeRef::ArrayOfNamed("MemoryEventView"), PrismRecordArgBundle { bundle_name: "memoryEvents", arg_name: "options", arg_index: 0, allowed_keys: MEMORY_EVENTS_KEYS }),
        method!("prism.curator.jobs", "jobs(options?: CuratorJobQueryOptions): CuratorJobView[];", PrismSurfaceTypeRef::ArrayOfNamed("CuratorJobView"), PrismRecordArgBundle { bundle_name: "curatorJob", arg_name: "options", arg_index: 0, allowed_keys: CURATOR_JOB_KEYS }),
        method!("prism.curator.proposals", "proposals(options?: CuratorProposalQueryOptions): CuratorProposalRecordView[];", PrismSurfaceTypeRef::ArrayOfNamed("CuratorProposalRecordView"), PrismRecordArgBundle { bundle_name: "curatorProposals", arg_name: "options", arg_index: 0, allowed_keys: CURATOR_PROPOSALS_KEYS }),
        method!("prism.curator.job", "job(id: string): CuratorJobView | null;", PrismSurfaceTypeRef::NullableNamed("CuratorJobView")),
    ];
    SPECS
}

pub fn prism_api_paths() -> &'static [&'static str] {
    static PATHS: OnceLock<Vec<&'static str>> = OnceLock::new();
    PATHS
        .get_or_init(|| {
            prism_api_method_specs()
                .iter()
                .map(|spec| spec.path)
                .collect()
        })
        .as_slice()
}

pub fn prism_record_arg_bundle(bundle_name: &str) -> Option<PrismRecordArgBundle> {
    prism_api_method_specs()
        .iter()
        .filter_map(|spec| spec.record_arg)
        .find(|bundle| bundle.bundle_name == bundle_name)
}

pub fn prism_method_spec(path: &str) -> Option<&'static PrismApiMethodSpec> {
    prism_api_method_specs()
        .iter()
        .find(|spec| spec.path == path)
}

pub fn prism_api_declaration_block() -> &'static str {
    static BLOCK: OnceLock<String> = OnceLock::new();
    BLOCK
        .get_or_init(|| {
            let mut top_level = Vec::new();
            let mut namespaced: BTreeMap<&str, Vec<&str>> = BTreeMap::new();
            for spec in prism_api_method_specs()
                .iter()
                .filter(|spec| spec.declaration.is_some())
            {
                let declaration = spec.declaration.unwrap();
                let suffix = spec.path.strip_prefix("prism.").unwrap_or(spec.path);
                let segments = suffix.split('.').collect::<Vec<_>>();
                if segments.len() == 2
                    && matches!(segments[0], "connection" | "runtime" | "memory" | "curator")
                {
                    namespaced.entry(segments[0]).or_default().push(declaration);
                } else {
                    top_level.push(declaration);
                }
            }
            let mut block = String::from("type PrismApi = {\n");
            for line in top_level {
                block.push_str("  ");
                block.push_str(line);
                block.push('\n');
            }
            for (namespace, declarations) in namespaced {
                block.push_str("  ");
                block.push_str(namespace);
                block.push_str(": {\n");
                for declaration in declarations {
                    block.push_str("    ");
                    block.push_str(declaration);
                    block.push('\n');
                }
                block.push_str("  };\n");
            }
            block.push_str("};");
            block
        })
        .as_str()
}

pub fn runtime_option_keys_js_object() -> &'static str {
    static JS: OnceLock<String> = OnceLock::new();
    JS.get_or_init(|| {
        let mut seen = BTreeSet::new();
        let mut entries = Vec::new();
        for bundle in prism_api_method_specs()
            .iter()
            .filter_map(|spec| spec.record_arg)
        {
            if !seen.insert(bundle.bundle_name) {
                continue;
            }
            let keys = bundle
                .allowed_keys
                .iter()
                .map(|key| format!("\"{key}\""))
                .collect::<Vec<_>>()
                .join(", ");
            entries.push(format!(
                "  {}: Object.freeze([{}])",
                bundle.bundle_name, keys
            ));
        }
        format!("Object.freeze({{\n{}\n}})", entries.join(",\n"))
    })
    .as_str()
}

pub fn prism_surface_type_for_method(path: &str) -> Option<PrismSurfaceType> {
    let method = prism_method_spec(path)?;
    Some(resolve_surface_type(method.return_type))
}

pub fn prism_object_property_type(
    surface: &PrismSurfaceType,
    property: &str,
) -> Option<PrismSurfaceType> {
    match surface {
        PrismSurfaceType::Object(properties) => properties.get(property).cloned(),
        PrismSurfaceType::Nullable(inner) => prism_object_property_type(inner, property),
        _ => None,
    }
}

fn resolve_surface_type(kind: PrismSurfaceTypeRef) -> PrismSurfaceType {
    match kind {
        PrismSurfaceTypeRef::Unknown => PrismSurfaceType::Unknown,
        PrismSurfaceTypeRef::Primitive(name) => PrismSurfaceType::Primitive(name),
        PrismSurfaceTypeRef::NullablePrimitive(name) => {
            PrismSurfaceType::Nullable(Box::new(PrismSurfaceType::Primitive(name)))
        }
        PrismSurfaceTypeRef::Named(name) => prism_named_surface_types()
            .get(name)
            .cloned()
            .unwrap_or(PrismSurfaceType::Unknown),
        PrismSurfaceTypeRef::NullableNamed(name) => PrismSurfaceType::Nullable(Box::new(
            prism_named_surface_types()
                .get(name)
                .cloned()
                .unwrap_or(PrismSurfaceType::Unknown),
        )),
        PrismSurfaceTypeRef::ArrayOfNamed(name) => PrismSurfaceType::Array(Box::new(
            prism_named_surface_types()
                .get(name)
                .cloned()
                .unwrap_or(PrismSurfaceType::Unknown),
        )),
    }
}

fn prism_named_surface_types() -> &'static BTreeMap<&'static str, PrismSurfaceType> {
    static TYPES: OnceLock<BTreeMap<&'static str, PrismSurfaceType>> = OnceLock::new();
    TYPES.get_or_init(build_named_surface_types)
}

macro_rules! schema_types {
    ($($name:literal => $ty:ty),* $(,)?) => {
        fn build_named_surface_types() -> BTreeMap<&'static str, PrismSurfaceType> {
            let mut map = BTreeMap::new();
            $(
                let schema = serde_json::to_value(schema_for!($ty)).expect("schema should serialize");
                let surface = surface_type_from_schema(&schema, &schema, &mut BTreeSet::new(), 0);
                map.insert($name, surface);
            )*
            extend_manual_surface_types(&mut map);
            map
        }
    };
}

schema_types! {
    "NodeId" => crate::NodeIdView,
    "QueryDiagnostic" => crate::QueryDiagnostic,
    "SymbolView" => crate::SymbolView,
    "SourceLocationView" => crate::SourceLocationView,
    "SourceExcerptView" => crate::SourceExcerptView,
    "SourceSliceView" => crate::SourceSliceView,
    "FocusedBlockView" => crate::FocusedBlockView,
    "ConceptPacketView" => crate::ConceptPacketView,
    "ConceptRelationView" => crate::ConceptRelationView,
    "ConceptDecodeView" => crate::ConceptDecodeView,
    "ContractPacketView" => crate::ContractPacketView,
    "TextSearchMatchView" => crate::TextSearchMatchView,
    "TextSearchBundleView" => crate::TextSearchBundleView,
    "ToolCatalogEntryView" => crate::ToolCatalogEntryView,
    "ToolSchemaView" => crate::ToolSchemaView,
    "ToolInputValidationView" => crate::ToolInputValidationView,
    "PlanListEntryView" => crate::PlanListEntryView,
    "PlanView" => crate::PlanView,
    "PlanSchedulingView" => crate::PlanSchedulingView,
    "AdHocPlanProjectionSummaryView" => crate::AdHocPlanProjectionSummaryView,
    "AdHocPlanProjectionView" => crate::AdHocPlanProjectionView,
    "AdHocPlanProjectionDiffView" => crate::AdHocPlanProjectionDiffView,
    "PlanExecutionOverlayView" => crate::PlanExecutionOverlayView,
    "GitExecutionOverlayView" => crate::GitExecutionOverlayView,
    "GitPreflightReportView" => crate::GitPreflightReportView,
    "GitPublishReportView" => crate::GitPublishReportView,
    "PlanNodeView" => crate::PlanNodeView,
    "PlanNodeBlockerView" => crate::PlanNodeBlockerView,
    "PlanSummaryView" => crate::PlanSummaryView,
    "PlanNodeRecommendationView" => crate::PlanNodeRecommendationView,
    "CoordinationTaskView" => crate::CoordinationTaskView,
    "TaskGitExecutionView" => crate::TaskGitExecutionView,
    "ClaimView" => crate::ClaimView,
    "ConflictView" => crate::ConflictView,
    "BlockerView" => crate::BlockerView,
    "ArtifactView" => crate::ArtifactView,
    "PolicyViolationRecordView" => crate::PolicyViolationRecordView,
    "ChangeImpactView" => crate::ChangeImpactView,
    "TaskValidationRecipeView" => crate::TaskValidationRecipeView,
    "TaskRiskView" => crate::TaskRiskView,
    "RepoPlaybookView" => crate::RepoPlaybookView,
    "ValidationPlanView" => crate::ValidationPlanView,
    "ImpactView" => crate::ImpactView,
    "AfterEditView" => crate::AfterEditView,
    "CommandMemoryView" => crate::CommandMemoryView,
    "ArtifactRiskView" => crate::ArtifactRiskView,
    "TaskIntentView" => crate::TaskIntentView,
    "CoordinationInboxView" => crate::CoordinationInboxView,
    "TaskContextView" => crate::TaskContextView,
    "LineageView" => crate::LineageView,
    "CoChangeView" => crate::CoChangeView,
    "ValidationRecipeView" => crate::ValidationRecipeView,
    "ReadContextView" => crate::ReadContextView,
    "EditContextView" => crate::EditContextView,
    "ValidationContextView" => crate::ValidationContextView,
    "RecentChangeContextView" => crate::RecentChangeContextView,
    "DiscoveryBundleView" => crate::DiscoveryBundleView,
    "SearchBundleView" => crate::SearchBundleView,
    "OwnerCandidateView" => crate::OwnerCandidateView,
    "DriftCandidateView" => crate::DriftCandidateView,
    "SpecImplementationClusterView" => crate::SpecImplementationClusterView,
    "SpecDriftExplanationView" => crate::SpecDriftExplanationView,
    "TaskReplay" => TaskReplay,
    "TaskJournalView" => crate::TaskJournalView,
    "ChangedFileView" => crate::ChangedFileView,
    "ChangedSymbolView" => crate::ChangedSymbolView,
    "PatchEventView" => crate::PatchEventView,
    "DiffHunkView" => crate::DiffHunkView,
    "ConnectionInfoView" => crate::ConnectionInfoView,
    "RuntimeStatusView" => crate::RuntimeStatusView,
    "RuntimeLogEventView" => crate::RuntimeLogEventView,
    "ValidationFeedbackView" => crate::ValidationFeedbackView,
    "ScoredMemoryView" => crate::ScoredMemoryView,
    "MemoryEventView" => crate::MemoryEventView,
    "CuratorJobView" => crate::CuratorJobView,
    "CuratorProposalRecordView" => crate::CuratorProposalRecordView,
    "McpCallLogEntryView" => crate::McpCallLogEntryView,
    "McpCallTraceView" => crate::McpCallTraceView,
    "McpCallStatsView" => crate::McpCallStatsView,
    "QueryLogEntryView" => crate::QueryLogEntryView,
    "QueryTraceView" => crate::QueryTraceView
}

fn extend_manual_surface_types(map: &mut BTreeMap<&'static str, PrismSurfaceType>) {
    let conflict_view = map
        .get("ConflictView")
        .cloned()
        .unwrap_or(PrismSurfaceType::Unknown);
    map.insert(
        "ClaimPreviewView",
        PrismSurfaceType::Object(BTreeMap::from([
            (
                "conflicts".to_string(),
                PrismSurfaceType::Array(Box::new(conflict_view.clone())),
            ),
            (
                "blocked".to_string(),
                PrismSurfaceType::Primitive("boolean"),
            ),
            (
                "warnings".to_string(),
                PrismSurfaceType::Array(Box::new(conflict_view)),
            ),
        ])),
    );
}

fn surface_type_from_schema(
    root: &Value,
    schema: &Value,
    visiting_refs: &mut BTreeSet<String>,
    depth: usize,
) -> PrismSurfaceType {
    if depth > 8 {
        return PrismSurfaceType::Unknown;
    }
    let resolved = resolve_schema_ref(root, schema);
    if let Some(reference) = resolved.get("$ref").and_then(Value::as_str) {
        if !visiting_refs.insert(reference.to_string()) {
            return PrismSurfaceType::Unknown;
        }
        let value = surface_type_from_schema(
            root,
            &resolve_schema_ref(root, resolved),
            visiting_refs,
            depth + 1,
        );
        visiting_refs.remove(reference);
        return value;
    }
    if let Some(any_of) = resolved.get("anyOf").and_then(Value::as_array) {
        return nullable_or_unknown(root, any_of, visiting_refs, depth);
    }
    if let Some(one_of) = resolved.get("oneOf").and_then(Value::as_array) {
        return nullable_or_unknown(root, one_of, visiting_refs, depth);
    }
    if let Some(type_name) = resolved.get("type").and_then(Value::as_str) {
        return match type_name {
            "string" => PrismSurfaceType::Primitive("string"),
            "integer" | "number" => PrismSurfaceType::Primitive("number"),
            "boolean" => PrismSurfaceType::Primitive("boolean"),
            "array" => {
                let item = resolved
                    .get("items")
                    .map(|items| surface_type_from_schema(root, items, visiting_refs, depth + 1))
                    .unwrap_or(PrismSurfaceType::Unknown);
                PrismSurfaceType::Array(Box::new(item))
            }
            "object" => {
                let mut properties = BTreeMap::new();
                if let Some(map) = resolved.get("properties").and_then(Value::as_object) {
                    for (name, value) in map {
                        properties.insert(
                            name.clone(),
                            surface_type_from_schema(root, value, visiting_refs, depth + 1),
                        );
                    }
                }
                PrismSurfaceType::Object(properties)
            }
            _ => PrismSurfaceType::Unknown,
        };
    }
    if let Some(type_names) = resolved.get("type").and_then(Value::as_array) {
        let mut non_null = type_names
            .iter()
            .filter_map(Value::as_str)
            .filter(|value| *value != "null")
            .collect::<Vec<_>>();
        if type_names
            .iter()
            .any(|value| value.as_str() == Some("null"))
            && non_null.len() == 1
        {
            let inner = surface_type_from_schema(
                root,
                &Value::Object(Map::from_iter([(
                    "type".to_string(),
                    Value::String(non_null.pop().unwrap().to_string()),
                )])),
                visiting_refs,
                depth + 1,
            );
            return PrismSurfaceType::Nullable(Box::new(inner));
        }
    }
    PrismSurfaceType::Unknown
}

fn nullable_or_unknown(
    root: &Value,
    variants: &[Value],
    visiting_refs: &mut BTreeSet<String>,
    depth: usize,
) -> PrismSurfaceType {
    if variants.len() == 2
        && variants
            .iter()
            .any(|value| value.get("type").and_then(Value::as_str) == Some("null"))
    {
        let inner = variants
            .iter()
            .find(|value| value.get("type").and_then(Value::as_str) != Some("null"))
            .map(|value| surface_type_from_schema(root, value, visiting_refs, depth + 1))
            .unwrap_or(PrismSurfaceType::Unknown);
        return PrismSurfaceType::Nullable(Box::new(inner));
    }
    PrismSurfaceType::Unknown
}

fn resolve_schema_ref<'a>(root: &'a Value, schema: &'a Value) -> &'a Value {
    let Some(reference) = schema.get("$ref").and_then(Value::as_str) else {
        return schema;
    };
    let mut current = root;
    for segment in reference.trim_start_matches("#/").split('/') {
        let Some(next) = current.get(segment) else {
            return schema;
        };
        current = next;
    }
    current
}
