use super::*;

#[test]
fn api_reference_mentions_primary_tool() {
    let docs = api_reference_markdown();
    assert!(docs.contains("PRISM Agent API"));
    assert!(docs.contains("prism_locate"));
    assert!(docs.contains("prism_open"));
    assert!(docs.contains("prism_workset"));
    assert!(docs.contains("prism_expand"));
    assert!(
        docs.contains("`prism_workset` is the comparison baseline for future agent-facing views.")
    );
    assert!(docs.contains(
        "why`, `nextAction`, and `suggestedActions` that let you continue without reconstructing context manually"
    ));
    assert!(docs.contains("prism_task_brief"));
    assert!(docs.contains("prism_concept"));
    assert!(docs.contains("prism_query"));
    assert!(docs.contains("\"kind\": \"impact\""));
    assert!(docs.contains("\"kind\": \"timeline\""));
    assert!(docs.contains("\"kind\": \"memory\""));
    assert!(docs.contains("\"taskId\": \"coord-task:12\""));
    assert!(docs.contains("type PrismApi"));
    assert!(docs.contains("### 12. Pull prior failures without reconstructing anchors manually"));
    assert!(docs.contains("coChangeNeighbors"));
    assert!(docs.contains("validationRecipe"));
    assert!(docs.contains("readContext"));
    assert!(docs
        .contains("symbolBundle(query: string, options?: SymbolBundleOptions): SymbolBundleView;"));
    assert!(docs.contains("editContext"));
    assert!(docs.contains("type QueryTarget = SymbolView | NodeId | { lineageId: string };"));
    assert!(docs.contains("full(target: QueryTarget): string | null;"));
    assert!(docs.contains(
        "excerpt(target: QueryTarget, options?: SourceExcerptOptions): SourceExcerptView | null;"
    ));
    assert!(docs.contains(
        "editSlice(target: QueryTarget, options?: EditSliceOptions): SourceSliceView | null;"
    ));
    assert!(docs.contains(
        "focusedBlock(target: QueryTarget, options?: EditSliceOptions): FocusedBlockView | null;"
    ));
    assert!(docs.contains("validationContext(target"));
    assert!(docs.contains("recentChangeContext(target"));
    assert!(docs.contains(
        "concept(query: string, options?: ConceptQueryOptions): ConceptPacketView | null;"
    ));
    assert!(docs
        .contains("concepts(query: string, options?: ConceptQueryOptions): ConceptPacketView[];"));
    assert!(docs.contains(
        "conceptByHandle(handle: string, options?: { verbosity?: \"summary\" | \"standard\" | \"full\"; includeBindingMetadata?: boolean }): ConceptPacketView | null;"
    ));
    assert!(docs.contains("contract(query: string): ContractPacketView | null;"));
    assert!(docs.contains("contracts(options?: ContractListOptions): ContractPacketView[];"));
    assert!(docs.contains("contractsFor(target: QueryTarget): ContractPacketView[];"));
    assert!(docs.contains("specs(): SpecListEntryView[];"));
    assert!(docs.contains("spec(specId: string): SpecDocumentView | null;"));
    assert!(docs.contains("specSyncBrief(specId: string): SpecSyncBriefView | null;"));
    assert!(docs.contains("specCoverage(specId: string): SpecCoverageRecordView[];"));
    assert!(docs.contains("specSyncProvenance(specId: string): SpecSyncProvenanceRecordView[];"));
    assert!(docs.contains("type SpecListEntryView = {"));
    assert!(docs.contains("type SpecDocumentView = {"));
    assert!(docs.contains("type SpecSyncBriefView = {"));
    assert!(docs.contains("type ContractListOptions = {"));
    assert!(docs.contains("status?: \"candidate\" | \"active\" | \"deprecated\" | \"retired\";"));
    assert!(docs.contains("scope?: \"local\" | \"session\" | \"repo\";"));
    assert!(docs.contains("conceptRelations(handle: string): ConceptRelationView[];"));
    assert!(docs.contains("decodeConcept(input:"));
    assert!(docs.contains("verbosity?: \"summary\" | \"standard\" | \"full\";"));
    assert!(docs.contains("includeBindingMetadata?: boolean;"));
    assert!(docs.contains("memoryRecall(options?: MemoryRecallOptions): ScoredMemoryView[];"));
    assert!(docs.contains("verbosityApplied: \"summary\" | \"standard\" | \"full\";"));
    assert!(docs.contains("truncation?: ConceptPacketTruncationView;"));
    assert!(docs.contains("type ConceptCurationHintsView = {"));
    assert!(docs.contains("curationHints: ConceptCurationHintsView;"));
    assert!(docs.contains("inspectFirst?: NodeId;"));
    assert!(docs.contains("nextAction?: string;"));
    assert!(docs.contains("`prism.concepts(...)` defaults to `summary`"));
    assert!(docs.contains("`prism.decodeConcept(...)` defaults to `standard`"));
    assert!(docs.contains("`query_typecheck_failed`"));
    assert!(docs.contains("repair data such as `didYouMean`"));
    assert!(docs.contains("bindingMetadata?: {"));
    assert!(docs.contains("discovery(target: QueryTarget): DiscoveryBundleView | null;"));
    assert!(docs
        .contains("searchBundle(query: string, options?: SearchBundleOptions): SearchBundleView;"));
    assert!(docs.contains(
        "targetBundle(target: QueryTarget | SearchBundleView | DiscoveryBundleView, options?: TargetBundleOptions): TargetBundleView | null;"
    ));
    assert!(docs.contains("nextReads(target"));
    assert!(docs.contains("whereUsed(target"));
    assert!(docs.contains("entrypointsFor(target"));
    assert!(docs.contains(
        "searchText(query: string, options?: SearchTextOptions): TextSearchMatchView[];"
    ));
    assert!(docs.contains(
        "textSearchBundle(query: string, options?: TextSearchBundleOptions): TextSearchBundleView;"
    ));
    assert!(docs.contains("tools(): ToolCatalogEntryView[];"));
    assert!(docs.contains("tool(name: string): ToolSchemaView | null;"));
    assert!(docs.contains("file(path: string): FileView;"));
    assert!(docs.contains("mcpLog(options?: McpLogOptions): McpCallLogEntryView[];"));
    assert!(docs.contains("slowMcpCalls(options?: McpLogOptions): McpCallLogEntryView[];"));
    assert!(docs.contains("mcpTrace(id: string): McpCallTraceView | null;"));
    assert!(docs.contains("mcpStats(options?: McpLogOptions): McpCallStatsView;"));
    assert!(docs.contains("queryLog(options?: QueryLogOptions): QueryLogEntryView[];"));
    assert!(docs.contains("slowQueries(options?: QueryLogOptions): QueryLogEntryView[];"));
    assert!(docs.contains("queryTrace(id: string): QueryTraceView | null;"));
    assert!(docs.contains("changedFiles(options?: ChangedFilesOptions): ChangedFileView[];"));
    assert!(docs.contains(
        "changedSymbols(path: string, options?: ChangedFilesOptions): ChangedSymbolView[];"
    ));
    assert!(docs.contains("recentPatches(options?: RecentPatchesOptions): PatchEventView[];"));
    assert!(
        docs.contains("diffFor(target: QueryTarget, options?: DiffForOptions): DiffHunkView[];")
    );
    assert!(docs
        .contains("taskChanges(taskId: string, options?: ChangedFilesOptions): PatchEventView[];"));
    assert!(docs.contains("connectionInfo(): ConnectionInfoView;"));
    assert!(docs.contains("runtimeStatus(): RuntimeStatusView;"));
    assert!(docs.contains("runtimeLogs(options?: RuntimeLogOptions): RuntimeLogEventView[];"));
    assert!(
        docs.contains("runtimeTimeline(options?: RuntimeTimelineOptions): RuntimeLogEventView[];")
    );
    assert!(docs.contains("repoPlaybook(): RepoPlaybookView;"));
    assert!(docs.contains(
        "validationPlan(input: { taskId?: string; target?: QueryTarget; paths?: string[] }): ValidationPlanView;"
    ));
    assert!(docs.contains(
        "impact(input: { taskId?: string; target?: QueryTarget; paths?: string[] }): ImpactView;"
    ));
    assert!(docs.contains(
        "afterEdit(input?: { taskId?: string; target?: QueryTarget; paths?: string[] }): AfterEditView;"
    ));
    assert!(docs.contains("commandMemory(input?: { taskId?: string }): CommandMemoryView;"));
    assert!(docs.contains("editSlice(options?: EditSliceOptions)"));
    assert!(docs.contains("relativeFocus"));
    assert!(docs.contains("type SearchTextOptions = {"));
    assert!(docs.contains("module?: string;"));
    assert!(docs.contains("taskId?: string;"));
    assert!(docs.contains("pathMode?: \"contains\" | \"exact\";"));
    assert!(docs.contains("structuredPath?: string;"));
    assert!(docs.contains("topLevelOnly?: boolean;"));
    assert!(docs.contains("preferCallableCode?: boolean;"));
    assert!(docs.contains("preferEditableTargets?: boolean;"));
    assert!(docs.contains("preferBehavioralOwners?: boolean;"));
    assert!(docs.contains("type TextSearchMatchView = {"));
    assert!(docs.contains("type McpLogOptions = {"));
    assert!(docs.contains("type QueryLogOptions = {"));
    assert!(docs.contains("type RuntimeLogOptions = {"));
    assert!(docs.contains("type RuntimeTimelineOptions = {"));
    assert!(docs.contains("type ChangedFilesOptions = {"));
    assert!(docs.contains("type RecentPatchesOptions = {"));
    assert!(docs.contains("type DiffForOptions = {"));
    assert!(docs.contains("type SearchBundleOptions = SearchOptions & {"));
    assert!(docs.contains("type SymbolBundleOptions = {"));
    assert!(docs.contains("type TextSearchBundleOptions = SearchTextOptions & {"));
    assert!(docs.contains("type TargetBundleOptions = DiffForOptions & {"));
    assert!(docs.contains("type BundleSummaryView = {"));
    assert!(docs.contains("suggestedReadLimit?: number;"));
    assert!(docs.contains("type ChangedFileView = {"));
    assert!(docs.contains("type ChangedSymbolView = {"));
    assert!(docs.contains("type PatchEventView = {"));
    assert!(docs.contains("type DiffHunkView = {"));
    assert!(docs.contains("type FocusedBlockView = {"));
    assert!(docs.contains("targetBlock: FocusedBlockView;"));
    assert!(docs.contains("directLinkBlocks: FocusedBlockView[];"));
    assert!(docs.contains("writePathBlocks: FocusedBlockView[];"));
    assert!(docs.contains("testBlocks: FocusedBlockView[];"));
    assert!(docs.contains("type ToolCatalogEntryView = {"));
    assert!(docs.contains("type ToolFieldSchemaView = {"));
    assert!(docs.contains("nestedFields: ToolFieldSchemaView[];"));
    assert!(docs.contains("type ToolActionSchemaView = {"));
    assert!(docs.contains("type ToolSchemaView = {"));
    assert!(docs.contains("type DiscoveryBundleView = {"));
    assert!(docs.contains("type SearchBundleView = {"));
    assert!(docs.contains("type SymbolBundleView = {"));
    assert!(docs.contains("candidates: SymbolView[];"));
    assert!(docs.contains("type TextSearchBundleView = {"));
    assert!(docs.contains("type TargetBundleView = {"));
    assert!(docs.contains("type RuntimeStatusView = {"));
    assert!(docs.contains("mcpCallLogPath?: string;"));
    assert!(docs.contains("mcpCallLogBytes?: number;"));
    assert!(docs.contains("type ConnectionInfoView = {"));
    assert!(docs.contains("mode: string;"));
    assert!(docs.contains("transport: string;"));
    assert!(docs.contains("healthUri?: string;"));
    assert!(docs.contains("bridgeRole: string;"));
    assert!(docs.contains("connection: ConnectionInfoView;"));
    assert!(docs.contains("parentPid: number;"));
    assert!(docs.contains("bridgeState?: string;"));
    assert!(docs.contains("connectedBridgeCount: number;"));
    assert!(docs.contains("orphanBridgeCount: number;"));
    assert!(docs.contains("type RuntimeLogEventView = {"));
    assert!(docs.contains("type McpCallPayloadSummaryView = {"));
    assert!(docs.contains("type McpCallLogEntryView = {"));
    assert!(docs.contains("type McpCallTraceView = {"));
    assert!(docs.contains("type McpCallStatsBucketView = {"));
    assert!(docs.contains("type McpCallStatsView = {"));
    assert!(docs.contains("type QueryLogEntryView = {"));
    assert!(docs.contains("type QueryTraceView = {"));
    assert!(docs.contains("type QueryEvidenceView = {"));
    assert!(docs.contains("type RepoPlaybookView = {"));
    assert!(docs.contains("type ValidationPlanView = {"));
    assert!(docs.contains("type QueryViewSubjectView = {"));
    assert!(docs.contains("type QueryRecommendationView = {"));
    assert!(docs.contains("type QueryRiskHintView = {"));
    assert!(docs.contains("type ImpactView = {"));
    assert!(docs.contains("type AfterEditView = {"));
    assert!(docs.contains("type CommandMemoryCommandView = {"));
    assert!(docs.contains("type CommandMemoryView = {"));
    assert!(docs.contains("kind: \"toml-key\""));
    assert!(docs.contains("read(options?: FileReadOptions): SourceExcerptView;"));
    assert!(docs.contains("around(options: FileAroundOptions): SourceSliceView;"));
    assert!(docs.contains("prism.memory.recall"));
    assert!(docs.contains("prism.memoryRecall(...)"));
    assert!(docs.contains("owners(target"));
    assert!(docs.contains("strategy?: \"direct\" | \"behavioral\""));
    assert!(docs.contains("specCluster"));
    assert!(docs.contains("explainDrift"));
    assert!(docs.contains("type LinkedSpecSummaryView = {"));
    assert!(docs.contains("linkedSpecs: LinkedSpecSummaryView[];"));
    assert!(docs.contains("prism://tool-schemas"));
    assert!(docs.contains("prism://schema/tool/{toolName}"));
    assert!(docs.contains("prism://capabilities"));
    assert!(docs.contains("Inspect tool payload requirements without leaving `prism_query`"));
    assert!(docs.contains(
        "prism://search/{query}?limit={limit}&cursor={cursor}&strategy={strategy}&ownerKind={ownerKind}&kind={kind}&path={path}&module={module}&taskId={taskId}&pathMode={pathMode}&structuredPath={structuredPath}&topLevelOnly={topLevelOnly}&preferCallableCode={preferCallableCode}&preferEditableTargets={preferEditableTargets}&preferBehavioralOwners={preferBehavioralOwners}&includeInferred={includeInferred}"
    ));
    assert!(docs.contains("prism.curator.jobs"));
    assert!(docs.contains("prism_mutate"));
    assert!(!docs.contains("prism_session"));
    assert!(docs.contains("curator_apply_proposal"));
    assert!(docs.contains("curator_promote_edge"));
    assert!(docs.contains("curator_promote_memory"));
}

#[test]
fn prelude_exposes_global_prism() {
    let prelude = runtime_prelude();
    assert!(prelude.contains("globalThis.prism"));
    assert!(prelude.contains("new Proxy(__prismBase"));
    assert!(prelude.contains("__prismHostCall"));
    assert!(prelude.contains("__queryViews"));
    assert!(prelude.contains("__queryView:${name}"));
    assert!(prelude.contains("__prismNormalizeDynamicViewInput"));
    assert!(prelude.contains("__prismNormalizeTargetPayload"));
    assert!(prelude.contains("full(target)"));
    assert!(prelude.contains("symbolBundle: __prismSymbolBundle"));
    assert!(prelude.contains("excerpt(target, options = {})"));
    assert!(prelude.contains("editSlice(target, options = {})"));
    assert!(prelude.contains("focusedBlock(target, options = {})"));
    assert!(prelude.contains("owners(target, options = {})"));
    assert!(prelude.contains("implementationFor(target, options = {})"));
    assert!(prelude.contains("readContext(target)"));
    assert!(prelude.contains("editContext(target)"));
    assert!(prelude.contains("validationContext(target)"));
    assert!(prelude.contains("recentChangeContext(target)"));
    assert!(prelude.contains("discovery(target)"));
    assert!(prelude.contains("searchBundle(query, options = {})"));
    assert!(prelude.contains("textSearchBundle(query, options = {})"));
    assert!(prelude.contains("targetBundle(target, options = {})"));
    assert!(prelude.contains("searchText(query, options = {})"));
    assert!(prelude.contains("concepts(query, options = {})"));
    assert!(prelude.contains("concept(query, options = {})"));
    assert!(prelude.contains("conceptByHandle(handle, options = {})"));
    assert!(prelude.contains("contract(query)"));
    assert!(prelude.contains("contracts(options = {})"));
    assert!(prelude.contains("contractsFor(target)"));
    assert!(prelude.contains("conceptRelations(handle)"));
    assert!(prelude.contains("decodeConcept(input)"));
    assert!(prelude.contains("__prismBundleSummary("));
    assert!(prelude.contains("__prismWithLocalDiagnostics("));
    assert!(prelude.contains("__prismResolveSuggestedReads("));
    assert!(prelude.contains("__prismTextSearchSemanticQuery(query, options = {})"));
    assert!(prelude.contains("tools()"));
    assert!(prelude.contains("tool(name)"));
    assert!(prelude.contains("pathMode: options?.pathMode ?? options?.path_mode"));
    assert!(prelude.contains("module: options?.module"));
    assert!(prelude.contains("taskId: options?.taskId ?? options?.task_id"));
    assert!(prelude.contains("structuredPath: options?.structuredPath ?? options?.structured_path"));
    assert!(prelude.contains("topLevelOnly: options?.topLevelOnly ?? options?.top_level_only"));
    assert!(prelude.contains("preferCallableCode:"));
    assert!(prelude.contains("options?.preferCallableCode ?? options?.prefer_callable_code"));
    assert!(prelude.contains("preferEditableTargets:"));
    assert!(prelude.contains("options?.preferEditableTargets ?? options?.prefer_editable_targets"));
    assert!(prelude.contains("preferBehavioralOwners:"));
    assert!(
        prelude.contains("options?.preferBehavioralOwners ?? options?.prefer_behavioral_owners")
    );
    assert!(prelude.contains("file(path)"));
    assert!(prelude.contains("mcpLog(options = {})"));
    assert!(prelude.contains("slowMcpCalls(options = {})"));
    assert!(prelude.contains("mcpTrace(id)"));
    assert!(prelude.contains("mcpStats(options = {})"));
    assert!(prelude.contains("queryLog(options = {})"));
    assert!(prelude.contains("slowQueries(options = {})"));
    assert!(prelude.contains("queryTrace(id)"));
    assert!(prelude.contains("changedFiles(options = {})"));
    assert!(prelude.contains("changedSymbols(path, options = {})"));
    assert!(prelude.contains("recentPatches(options = {})"));
    assert!(prelude.contains("diffFor(target, options = {})"));
    assert!(prelude.contains("taskChanges(taskId, options = {})"));
    assert!(prelude.contains("connectionInfo()"));
    assert!(prelude.contains("runtimeStatus()"));
    assert!(prelude.contains("runtimeLogs(options = {})"));
    assert!(prelude.contains("runtimeTimeline(options = {})"));
    assert!(prelude.contains("runtime: Object.freeze({"));
    assert!(prelude.contains("connection: Object.freeze({"));
    assert!(prelude.contains("memoryRecall(options = {})"));
    assert!(prelude.contains("return prism.memory.recall(options);"));
    assert!(prelude.contains("function __prismValidateOptions(methodPath, options, allowedKeys)"));
    assert!(prelude.contains("didYouMean"));
    assert!(prelude.contains("verbosity: options?.verbosity ?? \"summary\""));
    assert!(prelude.contains("verbosity: options?.verbosity ?? \"standard\""));
    assert!(prelude.contains("verbosity: input?.verbosity ?? \"standard\""));
    assert!(prelude.contains("return prism.connectionInfo();"));
    assert!(prelude.contains("status() {"));
    assert!(prelude.contains("return prism.runtimeStatus();"));
    assert!(prelude.contains("logs(options = {}) {"));
    assert!(prelude.contains("return prism.runtimeLogs(options);"));
    assert!(prelude.contains("timeline(options = {}) {"));
    assert!(prelude.contains("return prism.runtimeTimeline(options);"));
    assert!(prelude.contains("editSlice(options = {})"));
    assert!(prelude.contains("nextReads(target, options = {})"));
    assert!(prelude.contains("whereUsed(target, options = {})"));
    assert!(prelude.contains("entrypointsFor(target, options = {})"));
    assert!(prelude.contains("specCluster(target)"));
    assert!(prelude.contains("explainDrift(target)"));
    assert!(prelude.contains("curator: Object.freeze"));
    assert!(prelude.contains("__prismCleanupGlobals"));
}
