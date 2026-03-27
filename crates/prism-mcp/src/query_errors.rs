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
pub(crate) const USER_SNIPPET_LOCATION_MARKER: &str = "__PRISM_USER_LOCATION__";

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

pub(crate) fn parse_typescript_error(
    error: anyhow::Error,
    code: &str,
    user_snippet_first_line: usize,
) -> anyhow::Error {
    build_transpile_error(&error.to_string(), code, user_snippet_first_line).into()
}

pub(crate) fn runtime_or_serialization_error(
    error: anyhow::Error,
    code: &str,
    user_snippet_first_line: usize,
) -> anyhow::Error {
    let detail = error.to_string();
    if let Some(payload) = detail.strip_prefix("javascript query evaluation failed: ") {
        if let Some(body) = payload.strip_prefix(QUERY_SERIALIZATION_ERROR_MARKER) {
            return build_runtime_error(
                "query_result_not_serializable",
                "prism_query result is not JSON-serializable",
                body.trim_start(),
                code,
                user_snippet_first_line,
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
        }),
    }
    .into()
}

fn build_transpile_error(
    detail: &str,
    code: &str,
    user_snippet_first_line: usize,
) -> QueryExecutionError {
    let location = extract_location(detail)
        .and_then(|location| remap_location(location, code, user_snippet_first_line))
        .or_else(|| snippet_excerpt_location(detail));
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
        "prism_query parse failed{line_hint}: {}\nHint: Fix the TypeScript syntax near the reported line and retry. If you intended to return an object literal, write `return {{ ... }}` explicitly.",
        first_detail_line(detail)
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
    let mut message = format!("{summary}{line_hint}: {first_line}");
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
        build_transpile_error, missing_return_hint, runtime_or_serialization_error,
        QUERY_RUNTIME_ERROR_MARKER, QUERY_SERIALIZATION_ERROR_MARKER, USER_SNIPPET_LOCATION_MARKER,
    };
    use anyhow::anyhow;
    use serde_json::Value;

    #[test]
    fn remaps_transpile_locations_back_to_user_snippet_lines() {
        let error = build_transpile_error(
            "Expression expected at file:///prism/query.ts:4:16\n\n  const broken = ;\n                 ~",
            "const broken = ;\nreturn broken;",
            4,
        );
        assert!(error.to_string().contains("user snippet line 1, column 16"));
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
        );
        let runtime = runtime.downcast::<super::QueryExecutionError>().unwrap();
        assert_eq!(runtime.data()["code"], "query_runtime_failed");
        assert!(runtime
            .to_string()
            .contains("user snippet line 2, column 17"));

        let serialization = runtime_or_serialization_error(
            anyhow!(format!(
                "javascript query evaluation failed: {QUERY_SERIALIZATION_ERROR_MARKER}\ncircular reference\n    at stringify (native)"
            )),
            "const value = {};\nvalue.self = value;\nreturn value;",
            4,
        );
        let serialization = serialization
            .downcast::<super::QueryExecutionError>()
            .unwrap();
        assert_eq!(
            serialization.data()["code"],
            "query_result_not_serializable"
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
}
