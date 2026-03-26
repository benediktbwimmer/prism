use super::*;

#[test]
fn api_reference_mentions_primary_tool() {
    let docs = api_reference_markdown();
    assert!(docs.contains("prism_query"));
    assert!(docs.contains("type PrismApi"));
    assert!(docs.contains("### 12. Pull prior failures without reconstructing anchors manually"));
    assert!(docs.contains("coChangeNeighbors"));
    assert!(docs.contains("validationRecipe"));
    assert!(docs.contains("prism.memory.recall"));
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
    assert!(prelude.contains("curator: Object.freeze"));
    assert!(prelude.contains("__prismCleanupGlobals"));
}
