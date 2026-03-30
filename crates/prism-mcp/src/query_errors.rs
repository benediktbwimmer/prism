use anyhow::anyhow;
use regex::Regex;
use serde_json::{json, Value};
use std::error::Error;
use std::fmt::{Display, Formatter};
use std::sync::OnceLock;

pub(crate) const USER_SNIPPET_MARKER: &str = "/* __PRISM_USER_SNIPPET_START__ */";
pub(crate) const LEGACY_USER_SNIPPET_FIRST_WRAPPED_LINE: usize = 4;
pub(crate) const QUERY_RUNTIME_ERROR_MARKER: &str = "__PRISM_QUERY_RUNTIME_ERROR__";
pub(crate) const QUERY_SERIALIZATION_ERROR_MARKER: &str = "__PRISM_QUERY_SERIALIZATION_ERROR__";
pub(crate) const QUERY_USER_ERROR_MARKER: &str = "__PRISM_QUERY_USER_ERROR__";
pub(crate) const USER_SNIPPET_LOCATION_MARKER: &str = "__PRISM_USER_LOCATION__";
const STATEMENT_BODY_MODE: &str = "statement_body";
const IMPLICIT_EXPRESSION_MODE: &str = "implicit_expression";

#[derive(Debug, Clone)]
pub(crate) struct QueryExecutionError {
    summary: &'static str,
    message: String,
    data: Value,
}

impl QueryExecutionError {
    pub(crate) fn summary(&self) -> &'static str {
        self.summary
    }

    pub(crate) fn code(&self) -> Option<&str> {
        self.data["code"].as_str()
    }

    pub(crate) fn data(&self) -> &Value {
        &self.data
    }
}

impl Display for QueryExecutionError {
    fn fmt(&self, f: &mut Formatter<'_>) -> std::fmt::Result {
        f.write_str(&self.message)
    }
}

impl Error for QueryExecutionError {}

pub(crate) fn invalid_query_argument_error(
    operation: &str,
    detail: impl Into<String>,
) -> anyhow::Error {
    let detail = detail.into();
    let suggestion = invalid_argument_suggestion(&detail);
    let next_action = suggestion
        .as_ref()
        .map(|(field, candidate)| {
            format!(
                "Use the documented argument spelling `{candidate}` instead of `{field}` and retry. Check `prism://api-reference` for the exact surface shape."
            )
        })
        .unwrap_or_else(|| {
            "Check the query method argument names, required fields, and enum spellings, then retry."
                .to_string()
        });
    QueryExecutionError {
        summary: "prism_query arguments invalid",
        message: format!(
            "prism_query arguments invalid for `{operation}`: {detail}\nHint: {next_action}"
        ),
        data: json!({
            "code": "query_invalid_argument",
            "category": "invalid_argument",
            "operation": operation,
            "error": detail,
            "invalidField": suggestion.as_ref().map(|(field, _)| field.clone()),
            "didYouMean": suggestion.as_ref().map(|(_, candidate)| candidate.clone()),
            "nextAction": next_action,
            "examples": valid_query_examples(),
        }),
    }
    .into()
}

pub(crate) fn query_feature_disabled_error(operation: &str, group: &str) -> anyhow::Error {
    let (message, next_action) = match group {
        "internal_developer" => (
            "internal developer queries are disabled unless the PRISM MCP server is started with `--internal-developer`".to_string(),
            "Restart the PRISM MCP server with `--internal-developer`, or switch to an enabled query method.".to_string(),
        ),
        _ => (
            format!("coordination {group} queries are disabled by the PRISM MCP server feature flags"),
            "Restart the PRISM MCP server with the relevant coordination feature enabled, or switch to an enabled query method.".to_string(),
        ),
    };
    QueryExecutionError {
        summary: "prism_query feature disabled",
        message: format!("{message}\nHint: {next_action}"),
        data: json!({
            "code": "query_feature_disabled",
            "category": "feature_disabled",
            "operation": operation,
            "group": group,
            "error": message,
            "nextAction": next_action,
            "examples": valid_query_examples(),
        }),
    }
    .into()
}

fn decode_marshaled_query_error(body: &str) -> Option<QueryExecutionError> {
    let payload = serde_json::Deserializer::from_str(body)
        .into_iter::<Value>()
        .next()?
        .ok()?;
    let data = payload.get("data")?.clone();
    let summary = match data.get("code")?.as_str()? {
        "query_invalid_argument" => "prism_query arguments invalid",
        "query_feature_disabled" => "prism_query feature disabled",
        _ => return None,
    };
    Some(QueryExecutionError {
        summary,
        message: payload.get("message")?.as_str()?.to_string(),
        data,
    })
}

pub(crate) fn parse_typescript_error(
    error: anyhow::Error,
    code: &str,
    user_snippet_first_line: usize,
    attempted_mode: &'static str,
) -> anyhow::Error {
    build_transpile_error(
        &error.to_string(),
        code,
        user_snippet_first_line,
        attempted_mode,
    )
    .into()
}

pub(crate) fn runtime_or_serialization_error(
    error: anyhow::Error,
    code: &str,
    user_snippet_first_line: usize,
    attempted_mode: &'static str,
) -> anyhow::Error {
    let detail = error.to_string();
    if let Some(payload) = detail.strip_prefix("javascript query evaluation failed: ") {
        if let Some(index) = payload.find(QUERY_USER_ERROR_MARKER) {
            let body = &payload[index + QUERY_USER_ERROR_MARKER.len()..];
            if let Some(query_error) = decode_marshaled_query_error(body.trim_start()) {
                return query_error.into();
            }
        }
        if let Some(body) = payload.strip_prefix(QUERY_SERIALIZATION_ERROR_MARKER) {
            return build_runtime_error(
                "query_result_not_serializable",
                "prism_query result is not JSON-serializable",
                body.trim_start(),
                code,
                user_snippet_first_line,
                attempted_mode,
                Some(
                    "Return only plain JSON-serializable values such as objects, arrays, strings, numbers, booleans, or null. Avoid circular references, functions, Symbols, and BigInts.".to_string(),
                ),
            )
            .into();
        }
        if let Some(body) = payload.strip_prefix(QUERY_RUNTIME_ERROR_MARKER) {
            return build_runtime_error(
                "query_runtime_failed",
                "prism_query runtime failed",
                body.trim_start(),
                code,
                user_snippet_first_line,
                attempted_mode,
                Some(
                    "Inspect the referenced user-snippet line, then retry. If the query composes several calls, verify each intermediate value before the final return.".to_string(),
                ),
            )
            .into();
        }
        return build_runtime_error(
            "query_runtime_failed",
            "prism_query runtime failed",
            payload.trim_start(),
            code,
            user_snippet_first_line,
            attempted_mode,
            Some(
                "Inspect the referenced user-snippet line, then retry. If the query composes several calls, verify each intermediate value before the final return.".to_string(),
            ),
        )
        .into();
    }
    anyhow!(detail)
}

pub(crate) fn result_decode_error(error: anyhow::Error, raw_result: &str) -> anyhow::Error {
    QueryExecutionError {
        summary: "prism_query returned malformed result JSON",
        message: format!(
            "prism_query returned malformed result JSON: {error}\nHint: This usually indicates a PRISM query wrapper bug rather than a user-snippet problem."
        ),
        data: json!({
            "code": "query_result_decode_failed",
            "category": "result_decode",
            "error": error.to_string(),
            "nextAction": "Inspect the query wrapper and JS runtime result handling; the user snippet returned something PRISM failed to decode.",
            "rawResultPreview": truncate_preview(raw_result, 200),
            "examples": valid_query_examples(),
        }),
    }
    .into()
}

pub(crate) fn static_typecheck_error(
    detail: impl Into<String>,
    attempted_mode: &'static str,
    line: usize,
    column: usize,
    extra_data: Value,
) -> anyhow::Error {
    let detail = detail.into();
    let attempted_label = attempted_mode_label(attempted_mode);
    let next_action = extra_data["nextAction"]
        .as_str()
        .unwrap_or(
            "Fix the reported PRISM query API usage and retry. Check `prism://api-reference` for the exact surface shape.",
        )
        .to_string();
    let mut data = json!({
        "code": "query_typecheck_failed",
        "category": "typecheck",
        "error": detail,
        "nextAction": next_action,
        "attemptedMode": attempted_mode,
        "attemptedModeLabel": attempted_label,
        "examples": valid_query_examples(),
    });
    if let Some(fields) = data.as_object_mut() {
        if let Some(extra_fields) = extra_data.as_object() {
            for (key, value) in extra_fields {
                fields.insert(key.clone(), value.clone());
            }
        }
    }
    attach_location(&mut data, &SnippetLocation { line, column });
    QueryExecutionError {
        summary: "prism_query typecheck failed",
        message: format!(
            "prism_query typecheck failed while interpreting the snippet as {attempted_label} at user snippet line {line}, column {column}: {detail}\nHint: {next_action}"
        ),
        data,
    }
    .into()
}

pub(crate) fn is_query_parse_error(error: &anyhow::Error) -> bool {
    query_execution_error(error).and_then(|error| error.data()["code"].as_str())
        == Some("query_parse_failed")
}

pub(crate) fn combined_parse_typescript_error(
    statement_error: anyhow::Error,
    expression_error: anyhow::Error,
) -> anyhow::Error {
    let Some(statement) = query_execution_error(&statement_error) else {
        return statement_error;
    };
    let Some(expression) = query_execution_error(&expression_error) else {
        return statement_error;
    };

    let statement_summary = attempt_summary(statement);
    let expression_summary = attempt_summary(expression);
    let location = snippet_location_from_value(statement.data())
        .or_else(|| snippet_location_from_value(expression.data()));
    let line_hint = location
        .as_ref()
        .map(|location| {
            format!(
                " at user snippet line {}, column {}",
                location.line, location.column
            )
        })
        .unwrap_or_default();
    let message = format!(
        "prism_query parse failed{line_hint}: PRISM tried both supported query interpretations and neither parsed.\nStatement-body mode: {statement_summary}\nImplicit-expression mode: {expression_summary}\nHint: Use either a statement-style snippet with an explicit `return ...`, or a single expression such as `({{ ... }})`."
    );
    let mut data = json!({
        "code": "query_parse_failed",
        "category": "parse",
        "nextAction": "Rewrite the query as either `const value = ...; return { ... };` or a single expression such as `({ ... })`, then retry.",
        "attemptedModes": [STATEMENT_BODY_MODE, IMPLICIT_EXPRESSION_MODE],
        "attempts": [
            statement.data().clone(),
            expression.data().clone(),
        ],
        "examples": valid_query_examples(),
    });
    if let Some(location) = location {
        attach_location(&mut data, &location);
    }
    QueryExecutionError {
        summary: "prism_query parse failed",
        message,
        data,
    }
    .into()
}

fn build_transpile_error(
    detail: &str,
    code: &str,
    user_snippet_first_line: usize,
    attempted_mode: &'static str,
) -> QueryExecutionError {
    let location = extract_location(detail)
        .and_then(|location| remap_location(location, code, user_snippet_first_line))
        .or_else(|| snippet_excerpt_location(detail));
    let attempted_label = attempted_mode_label(attempted_mode);
    let line_hint = location
        .as_ref()
        .map(|location| {
            format!(
                " at user snippet line {}, column {}",
                location.line, location.column
            )
        })
        .unwrap_or_default();
    let message = format!(
        "prism_query parse failed while interpreting the snippet as {attempted_label}{line_hint}: {}\nHint: Fix the TypeScript syntax near the reported line and retry. Valid query shapes include `const value = ...; return {{ ... }};` or a single expression like `({{ ... }})`.",
        first_detail_line(detail),
    );
    let next_action = if location.is_some() {
        "Fix the TypeScript syntax near the reported user-snippet location and retry."
    } else {
        "Fix the TypeScript syntax and retry. If the query should return data, ensure the final statement is an explicit `return ...`."
    };
    let mut data = json!({
        "code": "query_parse_failed",
        "category": "parse",
        "error": detail,
        "nextAction": next_action,
        "attemptedMode": attempted_mode,
        "attemptedModeLabel": attempted_label,
        "rewrittenQueryPreview": rewritten_query_preview(code, attempted_mode),
        "examples": valid_query_examples(),
    });
    if let Some(location) = location {
        attach_location(&mut data, &location);
    }
    QueryExecutionError {
        summary: "prism_query parse failed",
        message,
        data,
    }
}

fn build_runtime_error(
    code_name: &'static str,
    summary: &'static str,
    detail: &str,
    code: &str,
    user_snippet_first_line: usize,
    attempted_mode: &'static str,
    next_action: Option<String>,
) -> QueryExecutionError {
    let location = extract_explicit_user_location(detail).or_else(|| {
        extract_location(detail)
            .and_then(|location| remap_location(location, code, user_snippet_first_line))
    });
    let line_hint = location
        .as_ref()
        .map(|location| {
            format!(
                " at user snippet line {}, column {}",
                location.line, location.column
            )
        })
        .unwrap_or_default();
    let first_line = first_detail_line(detail);
    let attempted_label = attempted_mode_label(attempted_mode);
    let next_action = merge_runtime_repair_hint(detail, code, next_action);
    let mut message = format!(
        "{summary} while interpreting the snippet as {attempted_label}{line_hint}: {first_line}"
    );
    if let Some(next_action) = next_action.as_deref() {
        message.push_str("\nHint: ");
        message.push_str(next_action);
    }
    let mut data = json!({
        "code": code_name,
        "category": if code_name == "query_result_not_serializable" {
            "serialization"
        } else {
            "runtime"
        },
        "error": detail,
        "nextAction": next_action.unwrap_or_else(|| {
            "Inspect the reported user-snippet location and retry with a simpler query if needed."
                .to_string()
        }),
        "attemptedMode": attempted_mode,
        "attemptedModeLabel": attempted_label,
        "rewrittenQueryPreview": rewritten_query_preview(code, attempted_mode),
        "examples": valid_query_examples(),
    });
    if let Some(location) = location {
        attach_location(&mut data, &location);
    }
    QueryExecutionError {
        summary,
        message,
        data,
    }
}

fn merge_runtime_repair_hint(
    detail: &str,
    code: &str,
    next_action: Option<String>,
) -> Option<String> {
    match runtime_repair_hint(detail, code) {
        Some(hint) => match next_action {
            Some(existing) if existing == hint => Some(existing),
            Some(existing) => Some(format!("{hint} {existing}")),
            None => Some(hint),
        },
        None => next_action,
    }
}

fn runtime_repair_hint(detail: &str, code: &str) -> Option<String> {
    let detail_lower = detail.to_ascii_lowercase();
    if code.contains("prism.runtime.")
        && (detail_lower.contains("not a function")
            || detail_lower.contains("cannot read properties of undefined")
            || detail_lower.contains("undefined"))
    {
        return Some(
            "Use `prism.runtime.status()`, `prism.runtime.logs(...)`, or `prism.runtime.timeline(...)`. The flat aliases `prism.runtimeStatus()`, `prism.runtimeLogs(...)`, and `prism.runtimeTimeline(...)` still work too.".to_string(),
        );
    }
    if (code.contains("prism.memory.")
        || code.contains("prism.memoryRecall")
        || code.contains("prism.memoryOutcomes")
        || code.contains("prism.memoryEvents"))
        && (detail_lower.contains("not a function")
            || detail_lower.contains("cannot read properties of undefined")
            || detail_lower.contains("undefined"))
    {
        return Some(
            "Use `prism.memory.recall(...)`, `prism.memory.outcomes(...)`, or `prism.memory.events(...)`. The flat aliases `prism.memoryRecall(...)`, `prism.memoryOutcomes(...)`, and `prism.memoryEvents(...)` are accepted for compatibility too.".to_string(),
        );
    }
    if let Some((observed, suggested)) = suggest_prism_api_path(detail, code) {
        return Some(format!(
            "`{observed}` is not a PRISM query helper. Did you mean `{suggested}`? Check `prism://api-reference` for the exact method signature and retry."
        ));
    }
    None
}

fn invalid_argument_suggestion(detail: &str) -> Option<(String, String)> {
    let (field, candidates) = parse_unknown_field_detail(detail)?;
    let normalized_field = normalize_api_path(&field);
    let mut best: Option<(&str, usize)> = None;
    for candidate in &candidates {
        let distance = levenshtein(&normalized_field, &normalize_api_path(candidate));
        match best {
            Some((_, best_distance)) if distance >= best_distance => {}
            _ => best = Some((candidate.as_str(), distance)),
        }
    }
    let (candidate, distance) = best?;
    let threshold = normalized_field.len().max(6) / 3;
    (distance <= threshold.max(2)).then(|| (field, candidate.to_string()))
}

fn parse_unknown_field_detail(detail: &str) -> Option<(String, Vec<String>)> {
    static UNKNOWN_FIELD_RE: OnceLock<Regex> = OnceLock::new();
    let regex = UNKNOWN_FIELD_RE.get_or_init(|| {
        Regex::new(r"unknown field `([^`]+)`, expected one of (.+)$")
            .expect("valid unknown-field regex")
    });
    let captures = regex.captures(detail)?;
    let field = captures.get(1)?.as_str().to_string();
    let candidates = captures
        .get(2)?
        .as_str()
        .split(',')
        .map(|candidate| candidate.trim().trim_matches('`').trim_matches('"'))
        .filter(|candidate| !candidate.is_empty())
        .map(str::to_string)
        .collect::<Vec<_>>();
    (!candidates.is_empty()).then_some((field, candidates))
}

fn suggest_prism_api_path(detail: &str, code: &str) -> Option<(String, String)> {
    let observed = extract_prism_api_path(detail).or_else(|| extract_prism_api_path(code))?;
    let suggestion = closest_prism_api_path(&observed)?;
    (normalize_api_path(&observed) != normalize_api_path(&suggestion))
        .then_some((observed, suggestion))
}

fn extract_prism_api_path(value: &str) -> Option<String> {
    static PATH_RE: OnceLock<Regex> = OnceLock::new();
    let regex = PATH_RE.get_or_init(|| {
        Regex::new(r"prism(?:\.[A-Za-z_][A-Za-z0-9_]*)+").expect("valid prism api path regex")
    });
    regex
        .find(value)
        .map(|matched| matched.as_str().to_string())
}

pub(crate) fn closest_prism_api_path(path: &str) -> Option<String> {
    let candidates = known_prism_api_paths(path);
    let normalized = normalize_api_path(path);
    let mut best: Option<(&str, usize)> = None;
    for candidate in candidates {
        let distance = levenshtein(&normalized, &normalize_api_path(candidate));
        match best {
            Some((_, best_distance)) if distance >= best_distance => {}
            _ => best = Some((candidate, distance)),
        }
    }
    let (candidate, distance) = best?;
    let threshold = normalized.len().max(6) / 3;
    (distance <= threshold.max(2)).then(|| candidate.to_string())
}

pub(crate) fn suggest_api_token(value: &str, candidates: &[&str]) -> Option<String> {
    let normalized_value = normalize_api_path(value);
    let mut best: Option<(&str, usize)> = None;
    for candidate in candidates {
        let distance = levenshtein(&normalized_value, &normalize_api_path(candidate));
        match best {
            Some((_, best_distance)) if distance >= best_distance => {}
            _ => best = Some((candidate, distance)),
        }
    }
    let (candidate, distance) = best?;
    let threshold = normalized_value.len().max(6) / 3;
    (distance <= threshold.max(2)).then(|| candidate.to_string())
}

fn known_prism_api_paths(path: &str) -> &'static [&'static str] {
    let paths = prism_js::prism_api_paths();
    if path.starts_with("prism.runtime.") {
        &paths[paths
            .iter()
            .position(|candidate| *candidate == "prism.runtime.status")
            .unwrap_or(0)
            ..paths
                .iter()
                .position(|candidate| *candidate == "prism.memory.recall")
                .unwrap_or(paths.len())]
    } else if path.starts_with("prism.memory.") {
        &paths[paths
            .iter()
            .position(|candidate| *candidate == "prism.memory.recall")
            .unwrap_or(0)
            ..paths
                .iter()
                .position(|candidate| *candidate == "prism.curator.jobs")
                .unwrap_or(paths.len())]
    } else if path.starts_with("prism.connection.") {
        &paths[paths
            .iter()
            .position(|candidate| *candidate == "prism.connection.info")
            .unwrap_or(0)
            ..paths
                .iter()
                .position(|candidate| *candidate == "prism.runtime.status")
                .unwrap_or(paths.len())]
    } else if path.starts_with("prism.curator.") {
        &paths[paths
            .iter()
            .position(|candidate| *candidate == "prism.curator.jobs")
            .unwrap_or(0)..]
    } else {
        paths
    }
}

fn normalize_api_path(value: &str) -> String {
    value
        .chars()
        .filter(|ch| ch.is_ascii_alphanumeric())
        .flat_map(|ch| ch.to_lowercase())
        .collect()
}

fn levenshtein(left: &str, right: &str) -> usize {
    if left == right {
        return 0;
    }
    if left.is_empty() {
        return right.chars().count();
    }
    if right.is_empty() {
        return left.chars().count();
    }
    let right_chars: Vec<char> = right.chars().collect();
    let mut previous: Vec<usize> = (0..=right_chars.len()).collect();
    let mut current = vec![0usize; right_chars.len() + 1];
    for (row, left_char) in left.chars().enumerate() {
        current[0] = row + 1;
        for (column, right_char) in right_chars.iter().enumerate() {
            let substitution_cost = usize::from(left_char != *right_char);
            current[column + 1] = (current[column] + 1)
                .min(previous[column + 1] + 1)
                .min(previous[column] + substitution_cost);
        }
        previous.clone_from_slice(&current);
    }
    previous[right_chars.len()]
}

fn attach_location(target: &mut Value, location: &SnippetLocation) {
    if let Some(fields) = target.as_object_mut() {
        fields.insert("line".to_string(), json!(location.line));
        fields.insert("column".to_string(), json!(location.column));
        fields.insert(
            "location".to_string(),
            json!({
                "line": location.line,
                "column": location.column,
            }),
        );
    }
}

fn first_detail_line(detail: &str) -> String {
    detail
        .lines()
        .find(|line| !line.trim().is_empty())
        .unwrap_or(detail)
        .trim()
        .to_string()
}

fn valid_query_examples() -> Value {
    json!([
        "const sym = await prism.symbol(\"target\");\nreturn { path: sym?.id.path ?? null };",
        "({ top: (await prism.search(\"target\", { limit: 1 }))[0]?.id.path ?? null })"
    ])
}

fn attempted_mode_label(attempted_mode: &str) -> &'static str {
    match attempted_mode {
        STATEMENT_BODY_MODE => "a statement-body query",
        IMPLICIT_EXPRESSION_MODE => "an implicit-expression query",
        _ => "the submitted query",
    }
}

fn rewritten_query_preview(code: &str, attempted_mode: &str) -> Value {
    match attempted_mode {
        STATEMENT_BODY_MODE => Value::String(truncate_preview(code, 200)),
        IMPLICIT_EXPRESSION_MODE => Value::String(truncate_preview(
            &format!("return (\n{}\n);", code.trim()),
            200,
        )),
        _ => Value::Null,
    }
}

fn query_execution_error(error: &anyhow::Error) -> Option<&QueryExecutionError> {
    error.downcast_ref::<QueryExecutionError>()
}

fn attempt_summary(error: &QueryExecutionError) -> String {
    let attempted = error.data()["attemptedModeLabel"]
        .as_str()
        .unwrap_or("query");
    let first = first_detail_line(error.data()["error"].as_str().unwrap_or_default());
    format!("{attempted}: {first}")
}

fn snippet_location_from_value(value: &Value) -> Option<SnippetLocation> {
    let object = value.get("location")?.as_object()?;
    Some(SnippetLocation {
        line: object.get("line")?.as_u64()? as usize,
        column: object.get("column")?.as_u64()? as usize,
    })
}

fn truncate_preview(value: &str, max_chars: usize) -> String {
    let mut preview = value.chars().take(max_chars).collect::<String>();
    if value.chars().count() > max_chars {
        preview.push_str("...");
    }
    preview
}

#[derive(Debug, Clone, Copy)]
struct WrappedLocation {
    line: usize,
    column: usize,
}

#[derive(Debug, Clone, Copy)]
struct SnippetLocation {
    line: usize,
    column: usize,
}

fn location_regex() -> &'static Regex {
    static REGEX: OnceLock<Regex> = OnceLock::new();
    REGEX.get_or_init(|| {
        Regex::new(r"(?:file:///prism/query\.ts|eval_script):(?P<line>\d+):(?P<column>\d+)")
            .expect("query location regex should compile")
    })
}

fn user_query_frame_regex() -> &'static Regex {
    static REGEX: OnceLock<Regex> = OnceLock::new();
    REGEX.get_or_init(|| {
        Regex::new(r"__prismUserQuery \(eval_script:(?P<line>\d+):(?P<column>\d+)\)")
            .expect("user-query frame regex should compile")
    })
}

fn snippet_excerpt_regex() -> &'static Regex {
    static REGEX: OnceLock<Regex> = OnceLock::new();
    REGEX.get_or_init(|| {
        Regex::new(r"(?m)^(?P<spaces>\s*)~+$").expect("snippet marker regex should compile")
    })
}

fn explicit_user_location_regex() -> &'static Regex {
    static REGEX: OnceLock<Regex> = OnceLock::new();
    REGEX.get_or_init(|| {
        Regex::new(r"__PRISM_USER_LOCATION__\s+(?P<line>\d+):(?P<column>\d+)")
            .expect("explicit user location regex should compile")
    })
}

fn extract_location(detail: &str) -> Option<WrappedLocation> {
    if let Some(captures) = user_query_frame_regex().captures(detail) {
        return Some(WrappedLocation {
            line: captures.name("line")?.as_str().parse().ok()?,
            column: captures.name("column")?.as_str().parse().ok()?,
        });
    }
    let captures = location_regex().captures(detail)?;
    Some(WrappedLocation {
        line: captures.name("line")?.as_str().parse().ok()?,
        column: captures.name("column")?.as_str().parse().ok()?,
    })
}

fn extract_explicit_user_location(detail: &str) -> Option<SnippetLocation> {
    let captures = explicit_user_location_regex().captures(detail)?;
    Some(SnippetLocation {
        line: captures.name("line")?.as_str().parse().ok()?,
        column: captures.name("column")?.as_str().parse().ok()?,
    })
}

fn remap_location(
    location: WrappedLocation,
    code: &str,
    user_snippet_first_line: usize,
) -> Option<SnippetLocation> {
    let max_lines = code.lines().count().max(1);
    for first_line in [
        user_snippet_first_line,
        LEGACY_USER_SNIPPET_FIRST_WRAPPED_LINE,
    ] {
        if location.line < first_line {
            continue;
        }
        let line = location.line - (first_line - 1);
        if line <= max_lines {
            return Some(SnippetLocation {
                line,
                column: location.column,
            });
        }
    }
    None
}

fn snippet_excerpt_location(detail: &str) -> Option<SnippetLocation> {
    let marker = snippet_excerpt_regex().find(detail)?;
    let line_start = detail[..marker.start()].rfind('\n')? + 1;
    Some(SnippetLocation {
        line: 1,
        column: marker.start().saturating_sub(line_start) + 1,
    })
}

pub(crate) fn missing_return_hint(code: &str, result: &Value) -> bool {
    result.is_null() && !code.contains("return")
}

#[cfg(test)]
mod tests {
    use super::{
        build_transpile_error, combined_parse_typescript_error, is_query_parse_error,
        missing_return_hint, runtime_or_serialization_error, IMPLICIT_EXPRESSION_MODE,
        QUERY_RUNTIME_ERROR_MARKER, QUERY_SERIALIZATION_ERROR_MARKER, STATEMENT_BODY_MODE,
        USER_SNIPPET_LOCATION_MARKER,
    };
    use anyhow::anyhow;
    use serde_json::Value;

    #[test]
    fn remaps_transpile_locations_back_to_user_snippet_lines() {
        let error = build_transpile_error(
            "Expression expected at file:///prism/query.ts:4:16\n\n  const broken = ;\n                 ~",
            "const broken = ;\nreturn broken;",
            4,
            STATEMENT_BODY_MODE,
        );
        assert!(error.to_string().contains("user snippet line 1, column 16"));
        assert!(error.to_string().contains("statement-body query"));
        assert_eq!(error.data()["line"], 1);
        assert_eq!(error.data()["column"], 16);
    }

    #[test]
    fn classifies_runtime_and_serialization_markers() {
        let runtime = runtime_or_serialization_error(
            anyhow!(format!(
                "javascript query evaluation failed: {QUERY_RUNTIME_ERROR_MARKER}\n{USER_SNIPPET_LOCATION_MARKER} 2:17\nboom\n    at __prismUserQuery (eval_script:5:17)"
            )),
            "const value = 1;\nthrow new Error(\"boom\");",
            4,
            STATEMENT_BODY_MODE,
        );
        let runtime = runtime.downcast::<super::QueryExecutionError>().unwrap();
        assert_eq!(runtime.data()["code"], "query_runtime_failed");
        assert!(runtime
            .to_string()
            .contains("user snippet line 2, column 17"));
        assert!(runtime.to_string().contains("statement-body query"));

        let serialization = runtime_or_serialization_error(
            anyhow!(format!(
                "javascript query evaluation failed: {QUERY_SERIALIZATION_ERROR_MARKER}\ncircular reference\n    at stringify (native)"
            )),
            "const value = {};\nvalue.self = value;\nreturn value;",
            4,
            STATEMENT_BODY_MODE,
        );
        let serialization = serialization
            .downcast::<super::QueryExecutionError>()
            .unwrap();
        assert_eq!(
            serialization.data()["code"],
            "query_result_not_serializable"
        );
        assert_eq!(
            serialization.data()["attemptedMode"],
            Value::String(STATEMENT_BODY_MODE.to_string())
        );
    }

    #[test]
    fn combines_parse_errors_from_both_interpretations() {
        let statement: anyhow::Error = build_transpile_error(
            "Expression expected at file:///prism/query.ts:4:16",
            "const broken = ;\nreturn broken;",
            4,
            STATEMENT_BODY_MODE,
        )
        .into();
        let expression: anyhow::Error = build_transpile_error(
            "Expected ',', got ':' at file:///prism/query.ts:4:7",
            "const broken = ;\nreturn broken;",
            4,
            IMPLICIT_EXPRESSION_MODE,
        )
        .into();

        assert!(is_query_parse_error(&statement));
        assert!(is_query_parse_error(&expression));
        let combined = combined_parse_typescript_error(statement, expression);
        let combined = combined.downcast::<super::QueryExecutionError>().unwrap();
        assert!(combined.to_string().contains("Statement-body mode"));
        assert!(combined.to_string().contains("Implicit-expression mode"));
        assert_eq!(combined.data()["attemptedModes"][0], STATEMENT_BODY_MODE);
        assert_eq!(
            combined.data()["attemptedModes"][1],
            IMPLICIT_EXPRESSION_MODE
        );
    }

    #[test]
    fn missing_return_hint_only_triggers_for_implicit_null_results() {
        assert!(missing_return_hint(
            "const sym = prism.symbol(\"main\");",
            &Value::Null
        ));
        assert!(!missing_return_hint("return null;", &Value::Null));
        assert!(!missing_return_hint(
            "const sym = prism.symbol(\"main\");",
            &Value::Bool(true)
        ));
    }

    #[test]
    fn runtime_namespace_hint_suggests_valid_runtime_helpers() {
        let runtime = runtime_or_serialization_error(
            anyhow!(format!(
                "javascript query evaluation failed: {QUERY_RUNTIME_ERROR_MARKER}\n{USER_SNIPPET_LOCATION_MARKER} 1:21\nTypeError: prism.runtime.inspect is not a function\n    at __prismUserQuery (eval_script:4:21)"
            )),
            "return prism.runtime.inspect();",
            4,
            STATEMENT_BODY_MODE,
        );
        let runtime = runtime.downcast::<super::QueryExecutionError>().unwrap();
        let next_action = runtime.data()["nextAction"]
            .as_str()
            .expect("nextAction should be present");
        assert!(next_action.contains("prism.runtime.status()"));
        assert!(next_action.contains("prism.runtimeLogs(...)"));
    }

    #[test]
    fn memory_namespace_hint_suggests_valid_memory_helpers() {
        let runtime = runtime_or_serialization_error(
            anyhow!(format!(
                "javascript query evaluation failed: {QUERY_RUNTIME_ERROR_MARKER}\n{USER_SNIPPET_LOCATION_MARKER} 1:21\nTypeError: prism.memoryRecall is not a function\n    at __prismUserQuery (eval_script:4:21)"
            )),
            "return prism.memoryRecall({ limit: 1 });",
            4,
            STATEMENT_BODY_MODE,
        );
        let runtime = runtime.downcast::<super::QueryExecutionError>().unwrap();
        let next_action = runtime.data()["nextAction"]
            .as_str()
            .expect("nextAction should be present");
        assert!(next_action.contains("prism.memory.recall(...)"));
        assert!(next_action.contains("prism.memoryRecall(...)"));
    }

    #[test]
    fn mistyped_top_level_method_suggests_closest_prism_helper() {
        let runtime = runtime_or_serialization_error(
            anyhow!(format!(
                "javascript query evaluation failed: {QUERY_RUNTIME_ERROR_MARKER}\n{USER_SNIPPET_LOCATION_MARKER} 1:14\nTypeError: prism.seach is not a function\n    at __prismUserQuery (eval_script:4:14)"
            )),
            "return prism.seach(\"main\");",
            4,
            STATEMENT_BODY_MODE,
        );
        let runtime = runtime.downcast::<super::QueryExecutionError>().unwrap();
        let next_action = runtime.data()["nextAction"]
            .as_str()
            .expect("nextAction should be present");
        assert!(next_action.contains("prism.search"));
        assert!(next_action.contains("prism://api-reference"));
    }
}
