use prism_js::QueryEnvelope;
use serde_json::Value;

#[derive(Debug, Clone, Copy)]
pub(crate) enum ReplayHostProfile {
    FixtureDefault,
    FixtureTinyOutputCap,
    RepoDefault,
}

#[derive(Clone, Copy)]
pub(crate) enum ReplayExpectation {
    Success(fn(&QueryEnvelope)),
    Error(fn(&str)),
}

#[derive(Clone, Copy)]
pub(crate) struct ReplayCase {
    pub(crate) name: &'static str,
    pub(crate) profile: ReplayHostProfile,
    pub(crate) code: &'static str,
    pub(crate) expectation: ReplayExpectation,
}

pub(crate) fn replay_cases() -> Vec<ReplayCase> {
    vec![
        ReplayCase {
            name: "fixture_async_multi_statement",
            profile: ReplayHostProfile::FixtureDefault,
            code: r#"
const results = await prism.search("alpha", { limit: 2, kind: "function" });
const sym = await prism.symbol("alpha");
return {
  top: results[0]?.id.path ?? null,
  exact: sym?.id.path ?? null,
  count: results.length,
};
"#,
            expectation: ReplayExpectation::Success(assert_async_multi_statement),
        },
        ReplayCase {
            name: "fixture_parse_error",
            profile: ReplayHostProfile::FixtureDefault,
            code: r#"
const broken = ;
return broken;
"#,
            expectation: ReplayExpectation::Error(assert_parse_error),
        },
        ReplayCase {
            name: "fixture_runtime_error",
            profile: ReplayHostProfile::FixtureDefault,
            code: r#"
const value = 1;
throw new Error("boom");
"#,
            expectation: ReplayExpectation::Error(assert_runtime_error),
        },
        ReplayCase {
            name: "fixture_serialization_error",
            profile: ReplayHostProfile::FixtureDefault,
            code: r#"
const value = {};
value.self = value;
return value;
"#,
            expectation: ReplayExpectation::Error(assert_serialization_error),
        },
        ReplayCase {
            name: "fixture_output_cap",
            profile: ReplayHostProfile::FixtureTinyOutputCap,
            code: r#"
const alpha = prism.symbol("alpha")?.full() ?? "";
return {
  alpha,
  repeated: alpha + alpha + alpha + alpha + alpha + alpha,
};
"#,
            expectation: ReplayExpectation::Success(assert_output_cap_result),
        },
        ReplayCase {
            name: "repo_helper_bundle",
            profile: ReplayHostProfile::RepoDefault,
            code: r#"
const bundle = prism.searchBundle("helper", { limit: 5 });
return {
  query: "helper",
  topResultPath: bundle.topResult?.id?.path ?? null,
  summary: bundle.summary,
};
"#,
            expectation: ReplayExpectation::Success(assert_repo_helper_bundle),
        },
        ReplayCase {
            name: "repo_search_bundle",
            profile: ReplayHostProfile::RepoDefault,
            code: r#"
const bundle = prism.searchBundle("search", { limit: 5 });
return {
  query: "search",
  topResultPath: bundle.topResult?.id?.path ?? null,
  summary: bundle.summary,
};
"#,
            expectation: ReplayExpectation::Success(assert_repo_search_bundle),
        },
        ReplayCase {
            name: "repo_status_bundle",
            profile: ReplayHostProfile::RepoDefault,
            code: r#"
const bundle = prism.searchBundle("status", { limit: 5 });
return {
  query: "status",
  topResultPath: bundle.topResult?.id?.path ?? null,
  summary: bundle.summary,
};
"#,
            expectation: ReplayExpectation::Success(assert_repo_status_bundle),
        },
        ReplayCase {
            name: "repo_runtime_bundle",
            profile: ReplayHostProfile::RepoDefault,
            code: r#"
const bundle = prism.searchBundle("runtime", { limit: 5 });
return {
  query: "runtime",
  topResultPath: bundle.topResult?.id?.path ?? null,
  summary: bundle.summary,
};
"#,
            expectation: ReplayExpectation::Success(assert_repo_runtime_bundle),
        },
    ]
}

fn assert_async_multi_statement(envelope: &QueryEnvelope) {
    assert_eq!(envelope.result["top"], "demo::alpha");
    assert_eq!(envelope.result["exact"], "demo::alpha");
    assert_eq!(envelope.result["count"], 1);
}

fn assert_parse_error(message: &str) {
    assert!(message.contains("prism_query parse failed"), "{message}");
    assert!(
        message.contains("user snippet line 2, column 16"),
        "{message}"
    );
    assert!(message.contains("Statement-body mode"), "{message}");
    assert!(message.contains("Implicit-expression mode"), "{message}");
    assert!(
        message.contains("single expression such as `({ ... })`"),
        "{message}"
    );
    assert!(
        message.contains("statement-style snippet with an explicit `return ...`"),
        "{message}"
    );
}

fn assert_runtime_error(message: &str) {
    assert!(message.contains("prism_query runtime failed"), "{message}");
    assert!(message.contains("boom"), "{message}");
    assert!(message.contains("statement-body query"), "{message}");
    assert!(
        message.contains("Inspect the referenced user-snippet line"),
        "{message}"
    );
}

fn assert_serialization_error(message: &str) {
    assert!(
        message.contains("prism_query result is not JSON-serializable"),
        "{message}"
    );
    assert!(message.contains("circular reference"), "{message}");
    assert!(message.contains("statement-body query"), "{message}");
    assert!(message.contains("JSON-serializable values"), "{message}");
}

fn assert_output_cap_result(envelope: &QueryEnvelope) {
    assert_eq!(envelope.result, Value::Null);
    assert!(envelope
        .diagnostics
        .iter()
        .any(|diagnostic| diagnostic.code == "result_truncated"));
}

fn assert_repo_helper_bundle(envelope: &QueryEnvelope) {
    assert_repo_bundle_matches_token(envelope, "helper");
}

fn assert_repo_search_bundle(envelope: &QueryEnvelope) {
    assert_repo_bundle_matches_token(envelope, "search");
}

fn assert_repo_status_bundle(envelope: &QueryEnvelope) {
    assert_repo_bundle_matches_token(envelope, "status");
}

fn assert_repo_runtime_bundle(envelope: &QueryEnvelope) {
    assert_repo_bundle_matches_token(envelope, "runtime");
}

fn assert_repo_bundle_matches_token(envelope: &QueryEnvelope, token: &str) {
    let top_result_path = envelope.result["topResultPath"]
        .as_str()
        .unwrap_or_default()
        .to_ascii_lowercase();
    assert!(
        !top_result_path.is_empty(),
        "expected a top result path for `{token}`: {:?}",
        envelope.result
    );
    assert!(
        top_result_path.contains(token),
        "expected top result path `{top_result_path}` to contain `{token}`"
    );
    assert_eq!(envelope.result["query"], token);
    assert_eq!(envelope.result["summary"]["kind"], "search");
    assert!(
        envelope.result["summary"]["resultCount"]
            .as_u64()
            .unwrap_or_default()
            > 0
    );
}
