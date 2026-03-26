use prism_js::QueryDiagnostic;
use serde_json::{Map, Value};

pub(crate) fn query_diagnostic(
    code: &str,
    message: impl Into<String>,
    data: Option<Value>,
) -> QueryDiagnostic {
    let message = message.into();
    QueryDiagnostic {
        code: code.to_owned(),
        message: message.clone(),
        data: enrich_diagnostic_data(code, &message, data),
    }
}

pub(crate) fn normalize_query_diagnostic(diagnostic: QueryDiagnostic) -> QueryDiagnostic {
    query_diagnostic(&diagnostic.code, diagnostic.message, diagnostic.data)
}

fn enrich_diagnostic_data(code: &str, message: &str, data: Option<Value>) -> Option<Value> {
    if data
        .as_ref()
        .and_then(Value::as_object)
        .and_then(|fields| fields.get("nextAction"))
        .and_then(Value::as_str)
        .is_some()
    {
        return data;
    }

    let Some(next_action) = default_next_action(code, message, data.as_ref()) else {
        return data;
    };

    let mut fields = match data {
        Some(Value::Object(fields)) => fields,
        Some(other) => {
            let mut fields = Map::new();
            fields.insert("details".to_string(), other);
            fields
        }
        None => Map::new(),
    };
    fields.insert("nextAction".to_string(), Value::String(next_action));
    Some(Value::Object(fields))
}

fn default_next_action(code: &str, message: &str, data: Option<&Value>) -> Option<String> {
    let action = match code {
        "task_blocked" => {
            "Inspect `prism.blockers(taskId)` or `prism.taskContext(taskId)` and resolve the highest-priority blocker before proceeding."
        }
        "stale_revision" => {
            "Refresh the coordination task against the current workspace revision, then rerun `prism.blockers(taskId)`."
        }
        "validation_required" => {
            "Run `prism.taskValidationRecipe(taskId)` to see the required checks, then record the missing build, test, or fix validation."
        }
        "task_risk_blocked" => {
            "Inspect `prism.taskRisk(taskId)` and pending reviews, then clear the stale artifact or risk-review gate."
        }
        "lineage_uncertain" => {
            "Inspect `prism.lineage(target)` history and compare the candidate symbols with `prism.readContext(target)` before editing."
        }
        "unknown_method" => {
            "Check `prism://capabilities` or `prism://api-reference` for supported query methods, then retry with a valid operation."
        }
        "depth_limited" => {
            "Retry `prism.callGraph(target, { depth: ... })` with a smaller depth or inspect `prism.readContext(target)` first."
        }
        "missing_plan" => {
            "Start the work with an explicit task-start record next time, or inspect the earliest journal events to reconstruct the missing plan context."
        }
        "missing_validation" => {
            "Run the missing build, test, or fix validation and record it, or add a note explaining why validation was intentionally skipped."
        }
        "unresolved_failure" => {
            "Inspect the latest failure, apply the fix, and record a validation outcome, or explicitly abandon the task if the failure is expected."
        }
        "missing_close_summary" => {
            "Finish the task with a completion or abandonment summary so the journal has an explicit final disposition."
        }
        "anchor_unresolved" => {
            if data
                .and_then(Value::as_object)
                .and_then(|fields| fields.get("jobId"))
                .and_then(Value::as_str)
                .is_some()
            {
                "List recent curator jobs with `prism.curator.jobs({ limit: 5 })` or verify the job id before retrying."
            } else {
                return None;
            }
        }
        "result_truncated" => return Some(result_truncated_next_action(message, data).to_string()),
        _ => {
            "Inspect the diagnostic payload and retry with a narrower or more specific PRISM query."
        }
    };
    Some(action.to_string())
}

fn result_truncated_next_action<'a>(message: &'a str, data: Option<&Value>) -> &'a str {
    match data
        .and_then(Value::as_object)
        .and_then(|fields| fields.get("field"))
        .and_then(Value::as_str)
    {
        Some("eventLimit") => {
            return "Retry `prism.taskJournal(taskId, { eventLimit: ... })` with a smaller event window or inspect one journal slice at a time.";
        }
        Some("memoryLimit") => {
            return "Retry `prism.taskJournal(taskId, { memoryLimit: ... })` with a smaller memory window or inspect one journal slice at a time.";
        }
        _ => {}
    }

    if message.starts_with("Query output exceeded") {
        "Return fewer fields, request smaller excerpts, or split the work across multiple PRISM queries."
    } else if message.starts_with("Entrypoints were truncated") {
        "Start from a narrower `prism.search(...)` query or inspect one entrypoint at a time."
    } else if message.starts_with("Call graph for") {
        "Retry with a smaller call-graph depth or inspect one branch at a time with `prism.readContext(...)`."
    } else if message.starts_with("Memory recall limit") {
        "Add `focus`, `text`, or `kinds` filters to `prism.memory.recall(...)` before retrying."
    } else if message.starts_with("Memory outcome query limit") {
        "Add `focus`, `kinds`, `result`, or `since` filters to `prism.memory.outcomes(...)` before retrying."
    } else {
        "Narrow the query with `path`, `kind`, or stronger filters before retrying."
    }
}

#[cfg(test)]
mod tests {
    use serde_json::json;

    use super::query_diagnostic;

    #[test]
    fn known_diagnostics_get_next_action_guidance() {
        let cases = vec![
            (
                "task_blocked",
                "Coordination task `coord-task:1` currently has blockers.",
                Some(json!({ "taskId": "coord-task:1", "count": 1 })),
            ),
            (
                "stale_revision",
                "The coordination task is based on a stale workspace revision.",
                None,
            ),
            (
                "validation_required",
                "The coordination task is missing required validations.",
                None,
            ),
            (
                "task_risk_blocked",
                "The coordination task is blocked by risk or stale artifact state.",
                None,
            ),
            (
                "lineage_uncertain",
                "Lineage for `demo::main` contains ambiguous history.",
                Some(json!({ "id": "demo::main" })),
            ),
            (
                "unknown_method",
                "Unknown Prism host operation `bogus`.",
                Some(json!({ "operation": "bogus" })),
            ),
            (
                "depth_limited",
                "Call-graph depth was capped at 10 instead of 50.",
                Some(json!({ "requested": 50, "applied": 10 })),
            ),
            (
                "missing_plan",
                "Task has outcome history but no explicit plan-start record.",
                None,
            ),
            (
                "missing_validation",
                "Task recorded a patch but no later build, test, or validation outcome.",
                Some(json!({ "lastPatchAt": 1 })),
            ),
            (
                "unresolved_failure",
                "Task has a recorded failure without a later fix validation.",
                Some(json!({ "lastFailureAt": 1 })),
            ),
            (
                "missing_close_summary",
                "Task has recorded history but no final completion or abandonment summary.",
                None,
            ),
            (
                "anchor_unresolved",
                "No curator job matched `curator:1`.",
                Some(json!({ "jobId": "curator:1" })),
            ),
            (
                "result_truncated",
                "Query output exceeded the 262144 byte session cap.",
                Some(json!({ "applied": 262144, "observed": 300000 })),
            ),
            (
                "result_truncated",
                "Entrypoints were truncated at 500 entries.",
                Some(json!({ "applied": 500 })),
            ),
            (
                "result_truncated",
                "Call graph for `demo::main` was truncated at 500 nodes.",
                Some(json!({ "query": "demo::main", "applied": 500 })),
            ),
            (
                "result_truncated",
                "Memory recall limit was capped at 500 instead of 1000.",
                Some(json!({ "requested": 1000, "applied": 500 })),
            ),
            (
                "result_truncated",
                "Memory outcome query limit was capped at 500 instead of 1000.",
                Some(json!({ "requested": 1000, "applied": 500 })),
            ),
            (
                "result_truncated",
                "Task journal event limit was capped at 500 instead of 1000.",
                Some(json!({ "requested": 1000, "applied": 500, "field": "eventLimit" })),
            ),
            (
                "result_truncated",
                "Task journal memory limit was capped at 500 instead of 1000.",
                Some(json!({ "requested": 1000, "applied": 500, "field": "memoryLimit" })),
            ),
        ];

        for (code, message, data) in cases {
            let diagnostic = query_diagnostic(code, message, data);
            assert!(
                diagnostic
                    .data
                    .as_ref()
                    .and_then(|value| value["nextAction"].as_str())
                    .is_some_and(|value| !value.is_empty()),
                "{code} should include nextAction"
            );
        }
    }

    #[test]
    fn preserves_existing_next_action_guidance() {
        let diagnostic = query_diagnostic(
            "result_truncated",
            "Search results were truncated.",
            Some(json!({
                "nextAction": "Keep this exact action.",
                "query": "main"
            })),
        );

        assert_eq!(
            diagnostic
                .data
                .as_ref()
                .and_then(|value| value["nextAction"].as_str()),
            Some("Keep this exact action.")
        );
    }
}
