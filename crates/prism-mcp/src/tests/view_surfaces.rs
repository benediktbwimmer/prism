use std::fs;

use super::*;
use crate::tests_support::{
    host_with_prism, temp_workspace, test_session, write_long_excerpt_workspace,
};
use prism_core::index_workspace_session;
use prism_history::HistoryStore;
use prism_ir::{
    ChangeTrigger, Edge, EdgeKind, EventActor, EventId, EventMeta, FileId, Language, Node, NodeId,
    NodeKind, ObservedChangeSet, ObservedNode, Span, SymbolFingerprint,
};
use prism_query::Prism;
use prism_store::Graph;

#[test]
fn js_views_use_camel_case_and_enriched_nested_symbols() {
    let alpha = NodeId::new("demo", "demo::alpha", NodeKind::Function);
    let beta = NodeId::new("demo", "demo::beta", NodeKind::Function);

    let mut graph = Graph::new();
    graph.add_node(Node {
        id: alpha.clone(),
        name: "alpha".into(),
        kind: NodeKind::Function,
        file: FileId(1),
        span: Span::line(1),
        language: Language::Rust,
    });
    graph.add_node(Node {
        id: beta.clone(),
        name: "beta".into(),
        kind: NodeKind::Function,
        file: FileId(1),
        span: Span::line(2),
        language: Language::Rust,
    });
    graph.add_edge(Edge {
        kind: EdgeKind::Calls,
        source: alpha.clone(),
        target: beta,
        origin: prism_ir::EdgeOrigin::Static,
        confidence: 1.0,
    });

    let mut history = HistoryStore::new();
    history.seed_nodes([alpha.clone()]);

    let host = host_with_prism(Prism::with_history(graph, history));
    let result = host
        .execute(
            test_session(&host),
            r#"
const sym = prism.symbol("alpha");
const graph = sym?.callGraph(1);
const lineage = sym?.lineage();
return {
  crateName: sym?.id.crateName,
  callees: sym?.relations().callees.map((node) => node.id.path) ?? [],
  graphNodes: graph?.nodes.map((node) => node.id.path) ?? [],
  graphDepth: graph?.maxDepthReached ?? null,
  lineageId: lineage?.lineageId ?? null,
  lineageStatus: lineage?.status ?? null,
  currentPath: lineage?.current.id.path ?? null,
};
"#,
            QueryLanguage::Ts,
        )
        .expect("query should succeed");

    assert_eq!(result.result["crateName"], "demo");
    assert_eq!(result.result["callees"][0], "demo::beta");
    assert_eq!(result.result["graphNodes"][0], "demo::alpha");
    assert_eq!(result.result["graphNodes"][1], "demo::beta");
    assert_eq!(result.result["graphDepth"], 1);
    assert!(result.result["lineageId"]
        .as_str()
        .unwrap_or_default()
        .starts_with("lineage:"));
    assert_eq!(result.result["lineageStatus"], "active");
    assert_eq!(result.result["currentPath"], "demo::alpha");
}

#[test]
fn lineage_views_expose_summaries_and_evidence_details() {
    let renamed = NodeId::new("demo", "demo::new_name", NodeKind::Function);

    let mut graph = Graph::new();
    graph.add_node(Node {
        id: renamed.clone(),
        name: "new_name".into(),
        kind: NodeKind::Function,
        file: FileId(1),
        span: Span::line(1),
        language: Language::Rust,
    });

    let mut history = HistoryStore::new();
    history.seed_nodes([NodeId::new("demo", "demo::old_name", NodeKind::Function)]);
    history.apply(&ObservedChangeSet {
        meta: EventMeta {
            id: EventId::new("change:rename"),
            ts: 7,
            actor: EventActor::System,
            correlation: None,
            causation: None,
            execution_context: None,
        },
        trigger: ChangeTrigger::ManualReindex,
        files: vec![FileId(1)],
        previous_path: Some("/workspace/src/lib.rs".into()),
        current_path: Some("/workspace/src/lib.rs".into()),
        added: vec![ObservedNode {
            node: Node {
                id: renamed.clone(),
                name: "new_name".into(),
                kind: NodeKind::Function,
                file: FileId(1),
                span: Span::line(1),
                language: Language::Rust,
            },
            fingerprint: SymbolFingerprint::with_parts(10, Some(20), Some(20), None),
        }],
        removed: vec![ObservedNode {
            node: Node {
                id: NodeId::new("demo", "demo::old_name", NodeKind::Function),
                name: "old_name".into(),
                kind: NodeKind::Function,
                file: FileId(1),
                span: Span::line(1),
                language: Language::Rust,
            },
            fingerprint: SymbolFingerprint::with_parts(10, Some(20), Some(20), None),
        }],
        updated: Vec::new(),
        edge_added: Vec::new(),
        edge_removed: Vec::new(),
    });

    let host = host_with_prism(Prism::with_history(graph, history));
    let result = host
        .execute(
            test_session(&host),
            r#"
const lineage = prism.symbol("new_name")?.lineage();
return {
  status: lineage?.status ?? null,
  summary: lineage?.summary ?? null,
  uncertainty: lineage?.uncertainty ?? [],
  eventSummary: lineage?.history[0]?.summary ?? null,
  evidenceCodes: lineage?.history[0]?.evidenceDetails.map((item) => item.code) ?? [],
  evidenceLabels: lineage?.history[0]?.evidenceDetails.map((item) => item.label) ?? [],
};
"#,
            QueryLanguage::Ts,
        )
        .expect("query should succeed");

    assert_eq!(result.result["status"], "active");
    assert!(result.result["summary"]
        .as_str()
        .unwrap_or_default()
        .contains("Latest event: Renamed from demo::old_name to demo::new_name."));
    assert_eq!(
        result.result["uncertainty"].as_array().map(Vec::len),
        Some(0)
    );
    assert_eq!(
        result.result["eventSummary"],
        "Renamed from demo::old_name to demo::new_name."
    );
    let evidence_codes = result.result["evidenceCodes"]
        .as_array()
        .expect("evidence codes should be present")
        .iter()
        .filter_map(|value| value.as_str())
        .collect::<Vec<_>>();
    assert!(evidence_codes.contains(&"fingerprint_match"));
    assert!(evidence_codes.contains(&"signature_match"));
    let evidence_labels = result.result["evidenceLabels"]
        .as_array()
        .expect("evidence labels should be present")
        .iter()
        .filter_map(|value| value.as_str())
        .collect::<Vec<_>>();
    assert!(evidence_labels.contains(&"Exact Fingerprint Match"));
}

#[test]
fn symbol_views_expose_source_locations_and_excerpts() {
    let root = temp_workspace();
    let host = QueryHost::with_session(index_workspace_session(&root).unwrap());

    let result = host
        .execute(
            test_session(&host),
            r#"
const sym = prism.symbol("alpha");
return {
  location: sym?.location ?? null,
  sourceExcerpt: sym?.sourceExcerpt ?? null,
  excerpt: sym?.excerpt() ?? null,
  tunedExcerpt: sym?.excerpt({ maxChars: 10 }) ?? null,
};
"#,
            QueryLanguage::Ts,
        )
        .expect("query should succeed");

    assert_eq!(result.result["location"]["startLine"], 1);
    assert_eq!(result.result["location"]["endLine"], 1);
    assert!(
        result.result["location"]["startColumn"]
            .as_u64()
            .expect("startColumn should be numeric")
            >= 1
    );
    assert_eq!(
        result.result["sourceExcerpt"]["text"],
        result.result["excerpt"]["text"]
    );
    assert!(result.result["excerpt"]["text"]
        .as_str()
        .unwrap_or_default()
        .contains("pub fn alpha()"));
    assert_eq!(result.result["tunedExcerpt"]["truncated"], true);
}

#[test]
fn structured_config_keys_expose_precise_locations_and_local_excerpts() {
    let root = temp_workspace();
    fs::create_dir_all(root.join("src")).unwrap();
    fs::create_dir_all(root.join("config")).unwrap();
    fs::write(root.join("src/main.rs"), "fn main() {}\n").unwrap();
    fs::write(
        root.join("config/app.json"),
        "{\n  \"service\": {\n    \"port\": 8080,\n    \"logging\": true\n  },\n  \"other\": 1\n}\n",
    )
    .unwrap();
    fs::write(
        root.join("config/app.yaml"),
        "service:\n  port: 8080\n  logging: true\nother: 1\n",
    )
    .unwrap();
    fs::write(
        root.join("Cargo.toml"),
        "[workspace]\nmembers = [\"crates/alpha\"]\nresolver = \"2\"\n\n[dependencies]\nserde = \"1.0\"\n",
    )
    .unwrap();
    let host = QueryHost::with_session(index_workspace_session(&root).unwrap());

    let result = host
        .execute(
            test_session(&host),
            r#"
const jsonKey = prism.search("port", { path: "config/app.json", kind: "json-key", limit: 1 })[0];
const yamlKey = prism.search("port", { path: "config/app.yaml", kind: "yaml-key", limit: 1 })[0];
const tomlKey = prism.search("members", { path: "Cargo.toml", kind: "toml-key", limit: 1 })[0];
return {
  json: {
    location: jsonKey?.location ?? null,
    excerpt: jsonKey?.excerpt({ contextLines: 0, maxChars: 200 }) ?? null,
  },
  yaml: {
    location: yamlKey?.location ?? null,
    excerpt: yamlKey?.excerpt({ contextLines: 0, maxChars: 200 }) ?? null,
  },
  toml: {
    location: tomlKey?.location ?? null,
    excerpt: tomlKey?.excerpt({ contextLines: 0, maxChars: 200 }) ?? null,
  },
};
"#,
            QueryLanguage::Ts,
        )
        .expect("structured config query should succeed");

    assert_eq!(result.result["json"]["location"]["startLine"], 3);
    assert_eq!(result.result["json"]["location"]["endLine"], 3);
    assert!(result.result["json"]["excerpt"]["text"]
        .as_str()
        .unwrap_or_default()
        .contains("\"port\": 8080"));
    assert!(!result.result["json"]["excerpt"]["text"]
        .as_str()
        .unwrap_or_default()
        .contains("\"other\": 1"));

    assert_eq!(result.result["yaml"]["location"]["startLine"], 2);
    assert_eq!(result.result["yaml"]["location"]["endLine"], 2);
    assert!(result.result["yaml"]["excerpt"]["text"]
        .as_str()
        .unwrap_or_default()
        .contains("port: 8080"));
    assert!(!result.result["yaml"]["excerpt"]["text"]
        .as_str()
        .unwrap_or_default()
        .contains("other: 1"));

    assert_eq!(result.result["toml"]["location"]["startLine"], 2);
    assert_eq!(result.result["toml"]["location"]["endLine"], 2);
    assert!(result.result["toml"]["excerpt"]["text"]
        .as_str()
        .unwrap_or_default()
        .contains("members = [\"crates/alpha\"]"));
    assert!(!result.result["toml"]["excerpt"]["text"]
        .as_str()
        .unwrap_or_default()
        .contains("serde = \"1.0\""));
}

#[test]
fn symbol_views_expose_edit_slices_with_exact_focus_mapping() {
    let root = temp_workspace();
    write_long_excerpt_workspace(&root);
    let host = QueryHost::with_session(index_workspace_session(&root).unwrap());

    let result = host
        .execute(
            test_session(&host),
            r#"
const sym = prism.search("memory_recall", { path: "src/recall.rs", kind: "function", limit: 1 })[0];
return {
  location: sym?.location ?? null,
  editSlice: sym?.editSlice({ beforeLines: 2, afterLines: 2, maxLines: 6, maxChars: 120 }) ?? null,
};
"#,
            QueryLanguage::Ts,
        )
        .expect("query should succeed");

    assert_eq!(
        result.result["editSlice"]["focus"]["startLine"],
        result.result["location"]["startLine"]
    );
    assert_eq!(
        result.result["editSlice"]["focus"]["endLine"],
        result.result["location"]["endLine"]
    );
    assert_eq!(
        result.result["editSlice"]["startLine"],
        result.result["location"]["startLine"]
    );
    assert_eq!(
        result.result["editSlice"]["endLine"],
        result.result["location"]["endLine"]
    );
    assert_eq!(result.result["editSlice"]["relativeFocus"]["startLine"], 1);
    assert_eq!(
        result.result["editSlice"]["relativeFocus"]["endLine"]
            .as_u64()
            .expect("relative focus end line should be numeric"),
        result.result["location"]["endLine"]
            .as_u64()
            .expect("end line should be numeric")
            - result.result["location"]["startLine"]
                .as_u64()
                .expect("start line should be numeric")
            + 1
    );
    assert!(result.result["editSlice"]["text"]
        .as_str()
        .unwrap_or_default()
        .contains("pub fn memory_recall()"));
    assert_eq!(result.result["editSlice"]["truncated"], true);
}

#[test]
fn focused_blocks_return_exact_local_context_for_code_and_doc_targets() {
    let root = temp_workspace();
    write_long_excerpt_workspace(&root);
    let host = QueryHost::with_session(index_workspace_session(&root).unwrap());

    let result = host
        .execute(
            test_session(&host),
            r#"
const fnSym = prism.search("memory_recall", { path: "src/recall.rs", kind: "function", limit: 1 })[0];
const spec = prism.search("Integration Points", { path: "docs/SPEC.md", kind: "markdown-heading", limit: 1 })[0];
return {
  functionBlock: fnSym ? prism.focusedBlock(fnSym, { beforeLines: 1, afterLines: 1, maxLines: 6, maxChars: 180 }) : null,
  specBlock: spec ? prism.focusedBlock(spec, { maxLines: 4, maxChars: 160 }) : null,
  readQueries: fnSym ? prism.readContext(fnSym).suggestedQueries.map((query) => query.label) : [],
  editQueries: fnSym ? prism.editContext(fnSym).suggestedQueries.map((query) => query.label) : [],
  validationQueries: fnSym ? prism.validationContext(fnSym).suggestedQueries.map((query) => query.label) : [],
};
"#,
            QueryLanguage::Ts,
        )
        .expect("focused-block query should succeed");

    assert_eq!(result.result["functionBlock"]["strategy"], "edit_slice");
    assert_eq!(
        result.result["functionBlock"]["symbol"]["name"],
        "memory_recall"
    );
    assert_eq!(
        result.result["functionBlock"]["slice"]["focus"]["startLine"],
        1
    );
    assert!(result.result["functionBlock"]["slice"]["text"]
        .as_str()
        .unwrap_or_default()
        .contains("pub fn memory_recall()"));

    let spec_block = &result.result["specBlock"];
    assert_eq!(spec_block["symbol"]["kind"], "MarkdownHeading");
    assert!(spec_block["strategy"] == "edit_slice" || spec_block["strategy"] == "excerpt_fallback");
    let spec_text = spec_block["slice"]["text"]
        .as_str()
        .or_else(|| spec_block["excerpt"]["text"].as_str())
        .unwrap_or_default();
    assert!(spec_text.contains("## Integration Points"));

    for key in ["readQueries", "editQueries", "validationQueries"] {
        assert!(result.result[key]
            .as_array()
            .expect("query labels should be an array")
            .iter()
            .any(|label| label == "Focused Block"));
    }
}
