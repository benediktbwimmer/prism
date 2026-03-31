use std::fs;

use super::*;
use crate::tests_support::{
    host_with_session_internal, temp_workspace, test_session, write_long_excerpt_workspace,
};
use prism_core::index_workspace_session;
use serde_json::Value;

#[test]
fn prism_file_queries_read_exact_ranges_and_around_line_slices() {
    let root = temp_workspace();
    write_long_excerpt_workspace(&root);
    let host = QueryHost::with_session(index_workspace_session(&root).unwrap());

    let result = host
        .execute(
            test_session(&host),
            r#"
return {
  read: prism.file("src/recall.rs").read({ startLine: 2, endLine: 4 }),
  around: prism.file("src/recall.rs").around({ line: 3, before: 1, after: 1 }),
};
"#,
            QueryLanguage::Ts,
        )
        .expect("query should succeed");

    assert_eq!(result.result["read"]["startLine"], 2);
    assert_eq!(result.result["read"]["endLine"], 4);
    let read_text = result.result["read"]["text"].as_str().unwrap_or_default();
    assert!(read_text.contains("let alpha = \"lineage context\";"));
    assert!(read_text.contains("let beta = \"prior outcomes\";"));
    assert!(read_text.contains("let gamma = \"task journal\";"));
    assert!(!read_text.contains("pub fn memory_recall()"));

    assert_eq!(result.result["around"]["startLine"], 2);
    assert_eq!(result.result["around"]["endLine"], 4);
    assert_eq!(result.result["around"]["focus"]["startLine"], 3);
    assert_eq!(result.result["around"]["focus"]["endLine"], 3);
    assert_eq!(result.result["around"]["relativeFocus"]["startLine"], 2);
    assert_eq!(result.result["around"]["relativeFocus"]["endLine"], 2);
    let around_text = result.result["around"]["text"].as_str().unwrap_or_default();
    assert!(around_text.contains("let alpha = \"lineage context\";"));
    assert!(around_text.contains("let beta = \"prior outcomes\";"));
    assert!(around_text.contains("let gamma = \"task journal\";"));
    assert!(!around_text.contains("pub fn memory_recall()"));
}

#[test]
fn prism_text_search_returns_exact_locations_and_honors_filters() {
    let root = temp_workspace();
    write_long_excerpt_workspace(&root);
    let host = QueryHost::with_session(index_workspace_session(&root).unwrap());

    let result = host
        .execute(
            test_session(&host),
            r#"
return {
  literal: prism.searchText("read context", {
    path: "src/recall.rs",
    limit: 2,
    contextLines: 0,
  }),
  regex: prism.searchText("read context|edit context", {
    regex: true,
    path: "src/recall.rs",
    limit: 2,
    contextLines: 0,
  }),
  folded: prism.searchText("READ CONTEXT", {
    path: "src/recall.rs",
    limit: 1,
    contextLines: 0,
  }),
  strict: prism.searchText("READ CONTEXT", {
    path: "src/recall.rs",
    caseSensitive: true,
    limit: 1,
    contextLines: 0,
  }),
  globbed: prism.searchText("Integration Points", {
    glob: "docs/**/*.md",
    limit: 1,
    contextLines: 0,
  }),
};
"#,
            QueryLanguage::Ts,
        )
        .expect("query should succeed");

    let literal = result.result["literal"]
        .as_array()
        .expect("literal results");
    assert_eq!(literal.len(), 2);
    assert_eq!(literal[0]["path"], "src/recall.rs");
    assert_eq!(literal[0]["location"]["startLine"], 8);
    assert_eq!(literal[0]["excerpt"]["startLine"], 8);
    assert!(literal[0]["excerpt"]["text"]
        .as_str()
        .unwrap_or_default()
        .contains("let eta = \"read context\";"));

    let regex = result.result["regex"].as_array().expect("regex results");
    assert_eq!(regex.len(), 2);
    assert_eq!(regex[0]["path"], "src/recall.rs");
    assert_eq!(regex[0]["location"]["startLine"], 8);
    assert_eq!(regex[1]["location"]["startLine"], 9);

    let folded = result.result["folded"].as_array().expect("folded results");
    assert_eq!(folded.len(), 1);
    assert_eq!(folded[0]["location"]["startLine"], 8);

    let strict = result.result["strict"].as_array().expect("strict results");
    assert!(strict.is_empty());

    let globbed = result.result["globbed"]
        .as_array()
        .expect("globbed results");
    assert_eq!(globbed.len(), 1);
    assert_eq!(globbed[0]["path"], "docs/SPEC.md");
    assert_eq!(globbed[0]["location"]["startLine"], 3);
    assert!(globbed[0]["excerpt"]["text"]
        .as_str()
        .unwrap_or_default()
        .contains("## Integration Points"));
}

#[test]
fn prism_query_log_exposes_recent_slow_and_trace_views() {
    let root = temp_workspace();
    write_long_excerpt_workspace(&root);
    let host = host_with_session_internal(index_workspace_session(&root).unwrap());

    host.execute(
        test_session(&host),
        r#"
return prism.searchText("read context", {
  path: "src/recall.rs",
  limit: 1,
  contextLines: 0,
});
"#,
        QueryLanguage::Ts,
    )
    .expect("text search query should succeed");

    host.execute(
        test_session(&host),
        r#"
return prism.file("src/recall.rs").around({
  line: 8,
  before: 1,
  after: 1,
});
"#,
        QueryLanguage::Ts,
    )
    .expect("file slice query should succeed");

    let result = host
        .execute(
            test_session(&host),
            r#"
const recent = prism.queryLog({ limit: 5, target: "src/recall.rs" });
const slow = prism.slowQueries({
  limit: 5,
  minDurationMs: 0,
  target: "src/recall.rs",
});
return {
  recent,
  slow,
  trace: recent[0] ? prism.queryTrace(recent[0].id) : null,
};
"#,
            QueryLanguage::Ts,
        )
        .expect("query log query should succeed");

    let recent = result.result["recent"]
        .as_array()
        .expect("recent query log");
    assert_eq!(recent.len(), 2);
    assert_eq!(recent[0]["kind"], "typescript");
    assert_eq!(recent[0]["success"], true);
    assert!(recent[0]["sessionId"]
        .as_str()
        .unwrap_or_default()
        .starts_with("session:"));
    assert!(recent[0]["operations"]
        .as_array()
        .is_some_and(|ops| ops.iter().any(|value| value == "fileAround")));
    let touched = recent[0]["touched"].as_array().expect("touched values");
    assert!(touched.iter().any(|value| value == "src/recall.rs"));
    assert!(
        recent[0]["result"]["jsonBytes"]
            .as_u64()
            .expect("json bytes should be present")
            > 0
    );

    let slow = result.result["slow"].as_array().expect("slow query log");
    assert_eq!(slow.len(), 2);
    assert!(
        slow[0]["durationMs"].as_u64().unwrap_or_default()
            >= slow[1]["durationMs"].as_u64().unwrap_or_default()
    );

    assert_eq!(result.result["trace"]["entry"]["id"], recent[0]["id"]);
    assert!(result.result["trace"]["entry"]["operations"]
        .as_array()
        .is_some_and(|ops| ops.iter().any(|value| value == "fileAround")));
    let phases = result.result["trace"]["phases"]
        .as_array()
        .expect("trace phases");
    let operations = phases
        .iter()
        .filter_map(|phase| phase["operation"].as_str())
        .collect::<Vec<_>>();
    assert!(operations.contains(&"typescript.refreshWorkspace"));
    assert!(operations.contains(&"typescript.statement_body.prepare"));
    assert!(operations.contains(&"typescript.statement_body.transpile"));
    assert!(operations.contains(&"typescript.statement_body.workerRoundTrip"));
    assert!(operations.contains(&"mcp.executeHandler"));
    assert!(operations.contains(&"mcp.encodeResponse"));
    assert!(operations.contains(&"fileAround"));
    assert!(phases
        .iter()
        .find(|phase| phase["operation"] == "fileAround")
        .is_some_and(|phase| phase["success"] == true));
}

#[test]
fn prism_dynamic_query_views_follow_runtime_feature_flags() {
    let root = temp_workspace();
    write_long_excerpt_workspace(&root);

    let disabled_host = QueryHost::with_session_and_limits_and_features(
        index_workspace_session(&root).unwrap(),
        QueryLimits::default(),
        PrismMcpFeatures::full().with_query_view(QueryViewFeatureFlag::TestEcho, false),
    );
    let disabled_error = disabled_host
        .execute(
            test_session(&disabled_host),
            r#"return prism.testEcho({ value: 1 });"#,
            QueryLanguage::Ts,
        )
        .expect_err("disabled dynamic query view should fail");
    assert!(disabled_error.to_string().contains("not a function"));

    let enabled_host = QueryHost::with_session_and_limits_and_features(
        index_workspace_session(&root).unwrap(),
        QueryLimits::default(),
        PrismMcpFeatures::full().with_query_view(QueryViewFeatureFlag::TestEcho, true),
    );
    let enabled_result = enabled_host
        .execute(
            test_session(&enabled_host),
            r#"return prism.testEcho({ value: 7, label: "ok" });"#,
            QueryLanguage::Ts,
        )
        .expect("enabled dynamic query view should succeed");
    assert_eq!(enabled_result.result["echo"]["value"], 7);
    assert_eq!(enabled_result.result["echo"]["label"], "ok");
}

#[test]
fn prism_dynamic_query_views_are_attributed_in_query_history_and_mcp_stats() {
    let root = temp_workspace();
    write_long_excerpt_workspace(&root);

    let host = QueryHost::with_session_and_limits_and_features(
        index_workspace_session(&root).unwrap(),
        QueryLimits::default(),
        PrismMcpFeatures::full()
            .with_internal_developer(true)
            .with_query_view(QueryViewFeatureFlag::TestEcho, true),
    );

    host.execute(
        test_session(&host),
        r#"return prism.testEcho({ value: 7, label: "ok" });"#,
        QueryLanguage::Ts,
    )
    .expect("dynamic query view should succeed");

    let result = host
        .execute(
            test_session(&host),
            r#"
const queryEntry = prism.queryLog({ limit: 5 }).find((entry) => entry.viewName === "testEcho");
const mcpEntry = prism
  .mcpLog({ limit: 5, callType: "tool", name: "prism_query" })
  .find((entry) => entry.viewName === "testEcho");
const trace = mcpEntry ? prism.mcpTrace(mcpEntry.id) : null;
return {
  queryEntry,
  mcpEntry,
  trace,
  stats: prism.mcpStats({ callType: "tool", name: "prism_query" }),
};
"#,
            QueryLanguage::Ts,
        )
        .expect("query history lookup should succeed");

    assert_eq!(result.result["queryEntry"]["viewName"], "testEcho");
    assert_eq!(result.result["mcpEntry"]["viewName"], "testEcho");
    assert_eq!(result.result["trace"]["entry"]["viewName"], "testEcho");
    assert_eq!(
        result.result["trace"]["metadata"]["queryViewName"],
        "testEcho"
    );
    assert_eq!(result.result["stats"]["byViewName"][0]["key"], "testEcho");
    assert_eq!(result.result["stats"]["byViewName"][0]["count"], 1);
    assert_eq!(
        result.result["stats"]["byViewName"][0]["uniqueTaskCount"],
        0
    );
    assert!(
        result.result["stats"]["byViewName"][0]["averageResultJsonBytes"]
            .as_u64()
            .unwrap_or_default()
            > 0
    );
    assert!(
        result.result["stats"]["byViewName"][0]["maxResultJsonBytes"]
            .as_u64()
            .unwrap_or_default()
            > 0
    );
}

#[test]
fn prism_new_query_views_follow_independent_runtime_feature_flags() {
    let root = temp_workspace();
    write_long_excerpt_workspace(&root);
    fs::write(
        root.join("AGENTS.md"),
        "- `cargo build --release -p prism-cli -p prism-mcp`\n- `./target/release/prism-cli mcp restart --internal-developer`\n- `./target/release/prism-cli mcp status`\n- `./target/release/prism-cli mcp health`\n",
    )
    .unwrap();

    let playbook_only = QueryHost::with_session_and_limits_and_features(
        index_workspace_session(&root).unwrap(),
        QueryLimits::default(),
        PrismMcpFeatures::full()
            .with_query_view(QueryViewFeatureFlag::All, false)
            .with_query_view(QueryViewFeatureFlag::RepoPlaybook, true),
    );
    let playbook = playbook_only
        .execute(
            test_session(&playbook_only),
            r#"return prism.repoPlaybook();"#,
            QueryLanguage::Ts,
        )
        .expect("repoPlaybook should succeed when enabled");
    assert_eq!(
        playbook.result["build"]["commands"][0],
        "cargo build --release -p prism-cli -p prism-mcp"
    );
    let validation_disabled = playbook_only
        .execute(
            test_session(&playbook_only),
            r#"return prism.validationPlan({ paths: ["src/recall.rs"] });"#,
            QueryLanguage::Ts,
        )
        .expect_err("validationPlan should stay hidden when disabled");
    assert!(validation_disabled.to_string().contains("not a function"));

    let validation_only = QueryHost::with_session_and_limits_and_features(
        index_workspace_session(&root).unwrap(),
        QueryLimits::default(),
        PrismMcpFeatures::full()
            .with_query_view(QueryViewFeatureFlag::All, false)
            .with_query_view(QueryViewFeatureFlag::ValidationPlan, true),
    );
    let validation = validation_only
        .execute(
            test_session(&validation_only),
            r#"return prism.validationPlan({ paths: ["src/recall.rs"] });"#,
            QueryLanguage::Ts,
        )
        .expect("validationPlan should succeed when enabled");
    assert_eq!(validation.result["subject"]["kind"], "pathSet");
    assert_eq!(validation.result["subject"]["paths"][0], "src/recall.rs");
    let playbook_disabled = validation_only
        .execute(
            test_session(&validation_only),
            r#"return prism.repoPlaybook();"#,
            QueryLanguage::Ts,
        )
        .expect_err("repoPlaybook should stay hidden when disabled");
    assert!(playbook_disabled.to_string().contains("not a function"));

    let impact_only = QueryHost::with_session_and_limits_and_features(
        index_workspace_session(&root).unwrap(),
        QueryLimits::default(),
        PrismMcpFeatures::full()
            .with_query_view(QueryViewFeatureFlag::All, false)
            .with_query_view(QueryViewFeatureFlag::Impact, true),
    );
    let impact = impact_only
        .execute(
            test_session(&impact_only),
            r#"return prism.impact({ paths: ["src/recall.rs"] });"#,
            QueryLanguage::Ts,
        )
        .expect("impact should succeed when enabled");
    assert_eq!(impact.result["subject"]["kind"], "pathSet");
    assert!(impact.result["recommendedChecks"]
        .as_array()
        .is_some_and(|checks| !checks.is_empty()));
    let after_edit_disabled = impact_only
        .execute(
            test_session(&impact_only),
            r#"return prism.afterEdit({ paths: ["src/recall.rs"] });"#,
            QueryLanguage::Ts,
        )
        .expect_err("afterEdit should stay hidden when disabled");
    assert!(after_edit_disabled.to_string().contains("not a function"));

    let after_edit_only = QueryHost::with_session_and_limits_and_features(
        index_workspace_session(&root).unwrap(),
        QueryLimits::default(),
        PrismMcpFeatures::full()
            .with_query_view(QueryViewFeatureFlag::All, false)
            .with_query_view(QueryViewFeatureFlag::AfterEdit, true),
    );
    let after_edit = after_edit_only
        .execute(
            test_session(&after_edit_only),
            r#"return prism.afterEdit({ paths: ["src/recall.rs"] });"#,
            QueryLanguage::Ts,
        )
        .expect("afterEdit should succeed when enabled");
    assert_eq!(after_edit.result["subject"]["kind"], "pathSet");
    assert!(after_edit.result["tests"]
        .as_array()
        .is_some_and(|checks| !checks.is_empty()));
    let impact_disabled = after_edit_only
        .execute(
            test_session(&after_edit_only),
            r#"return prism.impact({ paths: ["src/recall.rs"] });"#,
            QueryLanguage::Ts,
        )
        .expect_err("impact should stay hidden when disabled");
    assert!(impact_disabled.to_string().contains("not a function"));

    let command_only = QueryHost::with_session_and_limits_and_features(
        index_workspace_session(&root).unwrap(),
        QueryLimits::default(),
        PrismMcpFeatures::full()
            .with_query_view(QueryViewFeatureFlag::All, false)
            .with_query_view(QueryViewFeatureFlag::CommandMemory, true),
    );
    let command_memory = command_only
        .execute(
            test_session(&command_only),
            r#"return prism.commandMemory();"#,
            QueryLanguage::Ts,
        )
        .expect("commandMemory should succeed when enabled");
    assert_eq!(command_memory.result["subject"]["kind"], "repo");
    assert!(command_memory.result["commands"]
        .as_array()
        .is_some_and(|commands| !commands.is_empty()));
    let playbook_disabled = command_only
        .execute(
            test_session(&command_only),
            r#"return prism.repoPlaybook();"#,
            QueryLanguage::Ts,
        )
        .expect_err("repoPlaybook should stay hidden when disabled");
    assert!(playbook_disabled.to_string().contains("not a function"));
}

#[test]
fn prism_repo_playbook_and_validation_plan_views_return_provenance_and_log_by_name() {
    let root = temp_workspace();
    write_long_excerpt_workspace(&root);
    fs::write(
        root.join("AGENTS.md"),
        "- `cargo build --release -p prism-cli -p prism-mcp`\n- `./target/release/prism-cli mcp restart --internal-developer`\n- `./target/release/prism-cli mcp status`\n- `./target/release/prism-cli mcp health`\n- Prefer the release binaries for restart and verification instead of `cargo run`.\n",
    )
    .unwrap();

    let host = QueryHost::with_session_and_limits_and_features(
        index_workspace_session(&root).unwrap(),
        QueryLimits::default(),
        PrismMcpFeatures::full()
            .with_internal_developer(true)
            .with_query_view(QueryViewFeatureFlag::RepoPlaybook, true)
            .with_query_view(QueryViewFeatureFlag::ValidationPlan, true),
    );

    let playbook = host
        .execute(
            test_session(&host),
            r#"return prism.repoPlaybook();"#,
            QueryLanguage::Ts,
        )
        .expect("repoPlaybook should succeed");
    assert_eq!(
        playbook.result["workflow"]["commands"][1],
        "./target/release/prism-cli mcp restart --internal-developer"
    );
    assert_eq!(
        playbook.result["workflow"]["provenance"][0]["path"],
        "AGENTS.md"
    );
    assert!(playbook.result["gotchas"]
        .as_array()
        .is_some_and(|gotchas| !gotchas.is_empty()));

    let validation = host
        .execute(
            test_session(&host),
            r#"return prism.validationPlan({ paths: ["src/recall.rs"] });"#,
            QueryLanguage::Ts,
        )
        .expect("validationPlan should succeed");
    assert_eq!(validation.result["subject"]["kind"], "pathSet");
    assert!(validation.result["fast"]
        .as_array()
        .is_some_and(|fast| !fast.is_empty()));
    assert!(validation.result["fast"][0]["why"]
        .as_str()
        .is_some_and(|why| !why.is_empty()));
    assert!(validation.result["fast"][0]["provenance"]
        .as_array()
        .is_some_and(|provenance| !provenance.is_empty()));

    let result = host
        .execute(
            test_session(&host),
            r#"
const entries = prism.queryLog({ limit: 10 })
  .filter((entry) => entry.viewName === "repoPlaybook" || entry.viewName === "validationPlan")
  .map((entry) => entry.viewName);
return {
  entries,
  stats: prism.mcpStats({ callType: "tool", name: "prism_query" }).byViewName
    .filter((entry) => entry.key === "repoPlaybook" || entry.key === "validationPlan"),
};
"#,
            QueryLanguage::Ts,
        )
        .expect("query log lookup should succeed");

    let entries = result.result["entries"]
        .as_array()
        .expect("entries should be array");
    assert!(entries.iter().any(|entry| entry == "repoPlaybook"));
    assert!(entries.iter().any(|entry| entry == "validationPlan"));
    let stats = result.result["stats"]
        .as_array()
        .expect("stats should be array");
    assert!(stats.iter().any(|entry| entry["key"] == "repoPlaybook"));
    assert!(stats.iter().any(|entry| entry["key"] == "validationPlan"));
}

#[test]
fn prism_impact_and_after_edit_views_return_explainable_results_and_log_by_name() {
    let root = temp_workspace();
    write_long_excerpt_workspace(&root);
    fs::write(
        root.join("AGENTS.md"),
        "- `cargo build --release -p prism-cli -p prism-mcp`\n- `./target/release/prism-cli mcp restart --internal-developer`\n- `./target/release/prism-cli mcp status`\n- `./target/release/prism-cli mcp health`\n",
    )
    .unwrap();

    let host = QueryHost::with_session_and_limits_and_features(
        index_workspace_session(&root).unwrap(),
        QueryLimits::default(),
        PrismMcpFeatures::full()
            .with_internal_developer(true)
            .with_query_view(QueryViewFeatureFlag::Impact, true)
            .with_query_view(QueryViewFeatureFlag::AfterEdit, true),
    );

    let impact = host
        .execute(
            test_session(&host),
            r#"return prism.impact({ paths: ["src/recall.rs"] });"#,
            QueryLanguage::Ts,
        )
        .expect("impact should succeed");
    assert_eq!(impact.result["subject"]["kind"], "pathSet");
    assert!(impact.result["downstream"]
        .as_array()
        .is_some_and(|items| !items.is_empty()));
    assert!(impact.result["recommendedChecks"][0]["why"]
        .as_str()
        .is_some_and(|why| !why.is_empty()));
    assert!(impact.result["recommendedChecks"][0]["provenance"]
        .as_array()
        .is_some_and(|items| !items.is_empty()));

    let after_edit = host
        .execute(
            test_session(&host),
            r#"return prism.afterEdit({ paths: ["src/recall.rs"] });"#,
            QueryLanguage::Ts,
        )
        .expect("afterEdit should succeed");
    assert_eq!(after_edit.result["subject"]["kind"], "pathSet");
    assert!(after_edit.result["nextReads"]
        .as_array()
        .is_some_and(|items| !items.is_empty()));
    assert!(after_edit.result["tests"]
        .as_array()
        .is_some_and(|items| !items.is_empty()));
    assert!(after_edit.result["docs"]
        .as_array()
        .is_some_and(|items| !items.is_empty()));
    assert!(after_edit.result["nextReads"][0]["why"]
        .as_str()
        .is_some_and(|why| !why.is_empty()));
    assert!(after_edit.result["nextReads"][0]["provenance"]
        .as_array()
        .is_some_and(|items| !items.is_empty()));

    let result = host
        .execute(
            test_session(&host),
            r#"
const entries = prism.queryLog({ limit: 10 })
  .filter((entry) => entry.viewName === "impact" || entry.viewName === "afterEdit")
  .map((entry) => entry.viewName);
const afterEditEntry = prism.queryLog({ limit: 10 })
  .find((entry) => entry.viewName === "afterEdit");
return {
  entries,
  stats: prism.mcpStats({ callType: "tool", name: "prism_query" }).byViewName
    .filter((entry) => entry.key === "impact" || entry.key === "afterEdit"),
  trace: afterEditEntry ? prism.queryTrace(afterEditEntry.id) : null,
};
"#,
            QueryLanguage::Ts,
        )
        .expect("query log lookup should succeed");

    let entries = result.result["entries"]
        .as_array()
        .expect("entries should be array");
    assert!(entries.iter().any(|entry| entry == "impact"));
    assert!(entries.iter().any(|entry| entry == "afterEdit"));
    let stats = result.result["stats"]
        .as_array()
        .expect("stats should be array");
    assert!(stats.iter().any(|entry| entry["key"] == "impact"));
    assert!(stats.iter().any(|entry| entry["key"] == "afterEdit"));
    let trace_operations = result.result["trace"]["phases"]
        .as_array()
        .expect("afterEdit trace phases")
        .iter()
        .filter_map(|phase| phase["operation"].as_str())
        .collect::<Vec<_>>();
    assert!(trace_operations.contains(&"afterEdit.resolvePathTargets"));
    assert!(trace_operations.contains(&"afterEdit.target.nextReads"));
    assert!(trace_operations.contains(&"afterEdit.target.validationRecipe"));
    assert!(trace_operations.contains(&"afterEdit.target.specLinks"));
    assert!(trace_operations.contains(&"afterEdit.target.blastRadius"));
    assert!(trace_operations.contains(&"afterEdit.target.contractPackets"));
    assert!(trace_operations.contains(&"afterEdit.appendDocFallbacks"));
    assert!(trace_operations.contains(&"afterEdit.appendValidationFallbacks"));
    assert!(trace_operations.contains(&"afterEdit.buildResult"));
}

#[test]
fn prism_impact_and_after_edit_note_out_of_scope_boundaries_for_unresolved_paths() {
    let root = temp_workspace();
    write_long_excerpt_workspace(&root);
    fs::create_dir_all(root.join("www")).unwrap();
    fs::write(
        root.join("www/app.js"),
        "export function boot() { console.log('boot'); }\n",
    )
    .unwrap();

    let host = QueryHost::with_session_and_limits_and_features(
        index_workspace_session(&root).unwrap(),
        QueryLimits::default(),
        PrismMcpFeatures::full()
            .with_internal_developer(true)
            .with_query_view(QueryViewFeatureFlag::Impact, true)
            .with_query_view(QueryViewFeatureFlag::AfterEdit, true),
    );

    let impact = host
        .execute(
            test_session(&host),
            r#"return prism.impact({ paths: ["www/app.js"] });"#,
            QueryLanguage::Ts,
        )
        .expect("impact should succeed");
    assert_eq!(impact.result["subject"]["unresolvedPaths"][0], "www/app.js");
    assert!(impact.result["notes"].as_array().is_some_and(|notes| notes
        .iter()
        .filter_map(|note| note.as_str())
        .any(|note| note.contains("www/app.js")
            && note.contains("outside the current indexed scope")
            && note.contains("`www`"))));

    let after_edit = host
        .execute(
            test_session(&host),
            r#"return prism.afterEdit({ paths: ["www/app.js"] });"#,
            QueryLanguage::Ts,
        )
        .expect("afterEdit should succeed");
    assert_eq!(
        after_edit.result["subject"]["unresolvedPaths"][0],
        "www/app.js"
    );
    assert!(after_edit.result["notes"]
        .as_array()
        .is_some_and(|notes| notes
            .iter()
            .filter_map(|note| note.as_str())
            .any(|note| note.contains("www/app.js")
                && note.contains("outside the current indexed scope")
                && note.contains("`www`"))));
}

#[test]
fn prism_command_memory_merges_playbook_and_observed_command_evidence() {
    let root = temp_workspace();
    write_long_excerpt_workspace(&root);
    fs::write(
        root.join("AGENTS.md"),
        "- `cargo build --release -p prism-cli -p prism-mcp`\n- `./target/release/prism-cli mcp restart --internal-developer`\n- `./target/release/prism-cli mcp status`\n- `./target/release/prism-cli mcp health`\n",
    )
    .unwrap();

    let host = QueryHost::with_session_and_limits_and_features(
        index_workspace_session(&root).unwrap(),
        QueryLimits::default(),
        PrismMcpFeatures::full()
            .with_internal_developer(true)
            .with_query_view(QueryViewFeatureFlag::CommandMemory, true),
    );
    let task_id = "task:command-memory";
    host.store_outcome(
        test_session(&host).as_ref(),
        PrismOutcomeArgs {
            kind: OutcomeKindInput::TestRan,
            anchors: Vec::new(),
            summary: "targeted prism-mcp command passed".to_string(),
            result: Some(OutcomeResultInput::Success),
            evidence: Some(vec![OutcomeEvidenceInput::Command {
                argv: vec![
                    "cargo".to_string(),
                    "test".to_string(),
                    "-p".to_string(),
                    "prism-mcp".to_string(),
                    "prism_command_memory_merges_playbook_and_observed_command_evidence"
                        .to_string(),
                ],
                passed: true,
            }]),
            task_id: Some(task_id.to_string()),
        },
    )
    .unwrap();
    host.store_outcome(
        test_session(&host).as_ref(),
        PrismOutcomeArgs {
            kind: OutcomeKindInput::TestRan,
            anchors: Vec::new(),
            summary: "targeted prism-mcp command failed".to_string(),
            result: Some(OutcomeResultInput::Failure),
            evidence: Some(vec![OutcomeEvidenceInput::Command {
                argv: vec![
                    "cargo".to_string(),
                    "test".to_string(),
                    "-p".to_string(),
                    "prism-mcp".to_string(),
                    "prism_command_memory_failure".to_string(),
                ],
                passed: false,
            }]),
            task_id: Some(task_id.to_string()),
        },
    )
    .unwrap();

    let result = host
        .execute(
            test_session(&host),
            &format!(
                r#"
const memory = prism.commandMemory({{ taskId: "{task_id}" }});
return {{
  subject: memory.subject,
  successful: memory.commands.find((entry) => entry.command.includes("prism_command_memory_merges_playbook_and_observed_command_evidence")),
  failing: memory.commands.find((entry) => entry.command.includes("prism_command_memory_failure")),
  playbook: memory.commands.find((entry) => entry.command === "cargo build --release -p prism-cli -p prism-mcp"),
}};
"#
            ),
            QueryLanguage::Ts,
        )
        .expect("commandMemory should succeed");

    assert_eq!(result.result["subject"]["kind"], "task");
    assert_eq!(result.result["subject"]["taskId"], task_id);
    assert!(result.result["successful"]["confidence"]
        .as_f64()
        .is_some_and(|confidence| confidence > 0.6));
    assert!(result.result["successful"]["provenance"]
        .as_array()
        .is_some_and(|items| !items.is_empty()));
    assert!(result.result["failing"]["caveats"]
        .as_array()
        .is_some_and(|items| !items.is_empty()));
    assert!(result.result["playbook"]["provenance"]
        .as_array()
        .is_some_and(|items| !items.is_empty()));
    let stats_result = host
        .execute(
            test_session(&host),
            r#"
return prism.mcpStats({ callType: "tool", name: "prism_query" }).byViewName
  .filter((entry) => entry.key === "commandMemory");
"#,
            QueryLanguage::Ts,
        )
        .expect("commandMemory stats lookup should succeed");
    let stats = stats_result
        .result
        .as_array()
        .expect("stats should be array");
    assert!(stats.iter().any(|entry| entry["key"] == "commandMemory"));
}

#[test]
fn prism_mcp_log_exposes_canonical_call_history_trace_and_stats() {
    let root = temp_workspace();
    write_long_excerpt_workspace(&root);
    let host = host_with_session_internal(index_workspace_session(&root).unwrap());

    host.execute(
        test_session(&host),
        r#"
return prism.searchText("read context", {
  path: "src/recall.rs",
  limit: 1,
  contextLines: 0,
});
"#,
        QueryLanguage::Ts,
    )
    .expect("text search query should succeed");

    host.execute(
        test_session(&host),
        r#"
return prism.file("src/recall.rs").around({
  line: 8,
  before: 1,
  after: 1,
});
"#,
        QueryLanguage::Ts,
    )
    .expect("file slice query should succeed");

    let result = host
        .execute(
            test_session(&host),
            r#"
const recent = prism.mcpLog({
  limit: 5,
  callType: "tool",
  name: "prism_query",
  contains: "src/recall.rs",
});
const slow = prism.slowMcpCalls({
  limit: 5,
  callType: "tool",
  name: "prism_query",
  minDurationMs: 0,
  contains: "src/recall.rs",
});
return {
  recent,
  slow,
  trace: recent[0] ? prism.mcpTrace(recent[0].id) : null,
  stats: prism.mcpStats({
    callType: "tool",
    name: "prism_query",
    contains: "src/recall.rs",
  }),
};
"#,
            QueryLanguage::Ts,
        )
        .expect("mcp log query should succeed");

    let recent = result.result["recent"].as_array().expect("recent mcp log");
    assert_eq!(recent.len(), 2);
    assert_eq!(recent[0]["callType"], "tool");
    assert_eq!(recent[0]["name"], "prism_query");
    assert_eq!(recent[0]["success"], true);
    assert!(recent[0]["serverInstanceId"]
        .as_str()
        .unwrap_or_default()
        .starts_with("mcp-instance:"));
    assert!(recent[0]["processId"].as_u64().unwrap_or_default() > 0);
    assert_eq!(recent[0]["traceAvailable"], true);
    assert!(
        recent[0]["request"]["jsonBytes"]
            .as_u64()
            .expect("request bytes should be present")
            > 0
    );
    assert!(
        recent[0]["response"]["jsonBytes"]
            .as_u64()
            .expect("response bytes should be present")
            > 0
    );

    let slow = result.result["slow"].as_array().expect("slow mcp log");
    assert_eq!(slow.len(), 2);
    assert!(
        slow[0]["durationMs"].as_u64().unwrap_or_default()
            >= slow[1]["durationMs"].as_u64().unwrap_or_default()
    );

    let trace = &result.result["trace"];
    assert_eq!(trace["entry"]["id"], recent[0]["id"]);
    assert_eq!(trace["metadata"]["tool"], "prism_query");
    assert!(trace["metadata"]["queryText"]
        .as_str()
        .unwrap_or_default()
        .contains("src/recall.rs"));
    assert_ne!(trace["requestPreview"], Value::Null);
    assert!(
        trace["requestPreview"].is_string() || trace["requestPreview"].is_object(),
        "request preview should stay serializable"
    );
    assert!(trace["phases"].as_array().is_some_and(|phases| phases
        .iter()
        .any(|phase| phase["operation"] == "fileAround")));

    assert_eq!(result.result["stats"]["totalCalls"], 2);
    assert_eq!(result.result["stats"]["successCount"], 2);
    assert_eq!(result.result["stats"]["errorCount"], 0);
    assert!(result.result["stats"]["byName"]
        .as_array()
        .is_some_and(|buckets| buckets.iter().any(|bucket| bucket["key"] == "prism_query")));
    assert!(result.result["stats"]["byViewName"]
        .as_array()
        .is_some_and(|buckets| buckets.is_empty()));

    let status = host
        .execute(
            test_session(&host),
            "return prism.runtimeStatus();",
            QueryLanguage::Ts,
        )
        .expect("runtime status query should succeed")
        .result;
    assert!(status["mcpCallLogPath"]
        .as_str()
        .unwrap_or_default()
        .ends_with(".prism/prism-mcp-call-log.jsonl"));
    assert!(status["mcpCallLogBytes"].as_u64().unwrap_or_default() > 0);
}

#[test]
fn prism_query_log_touched_prefers_semantic_targets() {
    let root = temp_workspace();
    let host = host_with_session_internal(index_workspace_session(&root).unwrap());

    host.execute(
        test_session(&host),
        r#"
return prism.runtimeLogs({ level: "WARN", limit: 2 });
"#,
        QueryLanguage::Ts,
    )
    .expect("runtime log query should succeed");

    let result = host
        .execute(
            test_session(&host),
            r#"
return prism.queryLog({ limit: 1, operation: "runtimeLogs" })[0];
"#,
            QueryLanguage::Ts,
        )
        .expect("query log lookup should succeed");

    let touched = result.result["touched"].as_array().expect("touched values");
    assert!(!touched.iter().any(|value| value == "WARN"));
}
