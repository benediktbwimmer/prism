use super::*;

#[test]
fn api_reference_mentions_primary_tool() {
    let docs = api_reference_markdown();
    assert!(docs.contains("prism_query"));
    assert!(docs.contains("type PrismApi"));
    assert!(docs.contains("### 12. Pull prior failures without reconstructing anchors manually"));
    assert!(docs.contains("coChangeNeighbors"));
    assert!(docs.contains("validationRecipe"));
    assert!(docs.contains("readContext"));
    assert!(docs.contains("editContext"));
    assert!(docs.contains("validationContext(target"));
    assert!(docs.contains("recentChangeContext(target"));
    assert!(docs.contains("nextReads(target"));
    assert!(docs.contains("whereUsed(target"));
    assert!(docs.contains("entrypointsFor(target"));
    assert!(docs.contains(
        "searchText(query: string, options?: SearchTextOptions): TextSearchMatchView[];"
    ));
    assert!(docs.contains("file(path: string): FileView;"));
    assert!(docs.contains("queryLog(options?: QueryLogOptions): QueryLogEntryView[];"));
    assert!(docs.contains("slowQueries(options?: QueryLogOptions): QueryLogEntryView[];"));
    assert!(docs.contains("queryTrace(id: string): QueryTraceView | null;"));
    assert!(docs.contains("editSlice(options?: EditSliceOptions)"));
    assert!(docs.contains("relativeFocus"));
    assert!(docs.contains("type SearchTextOptions = {"));
    assert!(docs.contains("type TextSearchMatchView = {"));
    assert!(docs.contains("type QueryLogOptions = {"));
    assert!(docs.contains("type QueryLogEntryView = {"));
    assert!(docs.contains("type QueryTraceView = {"));
    assert!(docs.contains("kind: \"toml-key\""));
    assert!(docs.contains("read(options?: FileReadOptions): SourceExcerptView;"));
    assert!(docs.contains("around(options: FileAroundOptions): SourceSliceView;"));
    assert!(docs.contains("prism.memory.recall"));
    assert!(docs.contains("owners(target"));
    assert!(docs.contains("strategy?: \"direct\" | \"behavioral\""));
    assert!(docs.contains("specCluster"));
    assert!(docs.contains("explainDrift"));
    assert!(docs.contains("prism://tool-schemas"));
    assert!(docs.contains("prism://schema/tool/{toolName}"));
    assert!(docs.contains("prism://capabilities"));
    assert!(docs.contains("prism.curator.jobs"));
    assert!(docs.contains("prism_session"));
    assert!(docs.contains("prism_mutate"));
    assert!(docs.contains("curator_promote_edge"));
    assert!(docs.contains("curator_promote_memory"));
}

#[test]
fn prelude_exposes_global_prism() {
    let prelude = runtime_prelude();
    assert!(prelude.contains("globalThis.prism"));
    assert!(prelude.contains("__prismHostCall"));
    assert!(prelude.contains("owners(target, options = {})"));
    assert!(prelude.contains("implementationFor(target, options = {})"));
    assert!(prelude.contains("readContext(target)"));
    assert!(prelude.contains("editContext(target)"));
    assert!(prelude.contains("validationContext(target)"));
    assert!(prelude.contains("recentChangeContext(target)"));
    assert!(prelude.contains("searchText(query, options = {})"));
    assert!(prelude.contains("file(path)"));
    assert!(prelude.contains("queryLog(options = {})"));
    assert!(prelude.contains("slowQueries(options = {})"));
    assert!(prelude.contains("queryTrace(id)"));
    assert!(prelude.contains("editSlice(options = {})"));
    assert!(prelude.contains("nextReads(target, options = {})"));
    assert!(prelude.contains("whereUsed(target, options = {})"));
    assert!(prelude.contains("entrypointsFor(target, options = {})"));
    assert!(prelude.contains("specCluster(target)"));
    assert!(prelude.contains("explainDrift(target)"));
    assert!(prelude.contains("curator: Object.freeze"));
    assert!(prelude.contains("__prismCleanupGlobals"));
}
