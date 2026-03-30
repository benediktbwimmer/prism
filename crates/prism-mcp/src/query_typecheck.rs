use anyhow::Result;
use deno_ast::swc::ast::{
    CallExpr, Callee, Expr, ExprOrSpread, Lit, MemberProp, ObjectLit, Program, Prop, PropName,
    PropOrSpread,
};
use deno_ast::swc::common::Spanned;
use deno_ast::swc::ecma_visit::{Visit, VisitWith};
use deno_ast::{parse_program, MediaType, ModuleSpecifier, ParseParams, ParsedSource, SourcePos};
use serde_json::{json, Map, Value};

use crate::{
    closest_prism_api_path, is_known_prism_api_path, static_typecheck_error, suggest_api_token,
};

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
const EXCERPT_KEYS: &[&str] = &["contextLines", "maxLines", "maxChars"];
const EDIT_SLICE_KEYS: &[&str] = &["beforeLines", "afterLines", "maxLines", "maxChars"];
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
    "callType",
    "call_type",
    "name",
    "taskId",
    "task_id",
    "sessionId",
    "session_id",
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
const RUNTIME_LOGS_KEYS: &[&str] = &["limit", "level", "target", "contains"];
const RUNTIME_TIMELINE_KEYS: &[&str] = &["limit", "contains"];
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

#[derive(Clone, Copy)]
pub(crate) enum StaticCheckMode {
    StatementBody,
    ImplicitExpression,
}

impl StaticCheckMode {
    pub(crate) fn attempted_mode(self) -> &'static str {
        match self {
            Self::StatementBody => "statement_body",
            Self::ImplicitExpression => "implicit_expression",
        }
    }

    fn wrapped_source(self, code: &str) -> String {
        match self {
            Self::StatementBody => format!("async function __prismTypecheck() {{\n{code}\n}}\n"),
            Self::ImplicitExpression => format!("(\n{code}\n);\n"),
        }
    }
}

#[derive(Clone, Copy)]
struct RecordArgSpec {
    method_path: &'static str,
    arg_index: usize,
    arg_name: &'static str,
    allowed_keys: &'static [&'static str],
}

const RECORD_ARG_SPECS: &[RecordArgSpec] = &[
    RecordArgSpec {
        method_path: "prism.search",
        arg_index: 1,
        arg_name: "options",
        allowed_keys: SEARCH_KEYS,
    },
    RecordArgSpec {
        method_path: "prism.concepts",
        arg_index: 1,
        arg_name: "options",
        allowed_keys: CONCEPT_KEYS,
    },
    RecordArgSpec {
        method_path: "prism.concept",
        arg_index: 1,
        arg_name: "options",
        allowed_keys: CONCEPT_KEYS,
    },
    RecordArgSpec {
        method_path: "prism.conceptByHandle",
        arg_index: 1,
        arg_name: "options",
        allowed_keys: CONCEPT_KEYS,
    },
    RecordArgSpec {
        method_path: "prism.decodeConcept",
        arg_index: 0,
        arg_name: "input",
        allowed_keys: DECODE_CONCEPT_KEYS,
    },
    RecordArgSpec {
        method_path: "prism.searchText",
        arg_index: 1,
        arg_name: "options",
        allowed_keys: SEARCH_TEXT_KEYS,
    },
    RecordArgSpec {
        method_path: "prism.plans",
        arg_index: 0,
        arg_name: "options",
        allowed_keys: PLANS_KEYS,
    },
    RecordArgSpec {
        method_path: "prism.policyViolations",
        arg_index: 0,
        arg_name: "input",
        allowed_keys: POLICY_VIOLATIONS_KEYS,
    },
    RecordArgSpec {
        method_path: "prism.claimPreview",
        arg_index: 0,
        arg_name: "input",
        allowed_keys: CLAIM_PREVIEW_KEYS,
    },
    RecordArgSpec {
        method_path: "prism.simulateClaim",
        arg_index: 0,
        arg_name: "input",
        allowed_keys: CLAIM_PREVIEW_KEYS,
    },
    RecordArgSpec {
        method_path: "prism.excerpt",
        arg_index: 1,
        arg_name: "options",
        allowed_keys: EXCERPT_KEYS,
    },
    RecordArgSpec {
        method_path: "prism.editSlice",
        arg_index: 1,
        arg_name: "options",
        allowed_keys: EDIT_SLICE_KEYS,
    },
    RecordArgSpec {
        method_path: "prism.focusedBlock",
        arg_index: 1,
        arg_name: "options",
        allowed_keys: EDIT_SLICE_KEYS,
    },
    RecordArgSpec {
        method_path: "prism.searchBundle",
        arg_index: 1,
        arg_name: "options",
        allowed_keys: SEARCH_BUNDLE_KEYS,
    },
    RecordArgSpec {
        method_path: "prism.textSearchBundle",
        arg_index: 1,
        arg_name: "options",
        allowed_keys: TEXT_SEARCH_BUNDLE_KEYS,
    },
    RecordArgSpec {
        method_path: "prism.targetBundle",
        arg_index: 1,
        arg_name: "options",
        allowed_keys: TARGET_BUNDLE_KEYS,
    },
    RecordArgSpec {
        method_path: "prism.nextReads",
        arg_index: 1,
        arg_name: "options",
        allowed_keys: NEXT_READS_KEYS,
    },
    RecordArgSpec {
        method_path: "prism.whereUsed",
        arg_index: 1,
        arg_name: "options",
        allowed_keys: WHERE_USED_KEYS,
    },
    RecordArgSpec {
        method_path: "prism.implementationFor",
        arg_index: 1,
        arg_name: "options",
        allowed_keys: IMPLEMENTATION_FOR_KEYS,
    },
    RecordArgSpec {
        method_path: "prism.owners",
        arg_index: 1,
        arg_name: "options",
        allowed_keys: OWNERS_KEYS,
    },
    RecordArgSpec {
        method_path: "prism.taskJournal",
        arg_index: 1,
        arg_name: "options",
        allowed_keys: TASK_JOURNAL_KEYS,
    },
    RecordArgSpec {
        method_path: "prism.changedFiles",
        arg_index: 0,
        arg_name: "options",
        allowed_keys: CHANGED_FILES_KEYS,
    },
    RecordArgSpec {
        method_path: "prism.changedSymbols",
        arg_index: 1,
        arg_name: "options",
        allowed_keys: CHANGED_SYMBOLS_KEYS,
    },
    RecordArgSpec {
        method_path: "prism.recentPatches",
        arg_index: 0,
        arg_name: "options",
        allowed_keys: RECENT_PATCHES_KEYS,
    },
    RecordArgSpec {
        method_path: "prism.diffFor",
        arg_index: 1,
        arg_name: "options",
        allowed_keys: DIFF_FOR_KEYS,
    },
    RecordArgSpec {
        method_path: "prism.taskChanges",
        arg_index: 1,
        arg_name: "options",
        allowed_keys: TASK_CHANGES_KEYS,
    },
    RecordArgSpec {
        method_path: "prism.runtimeLogs",
        arg_index: 0,
        arg_name: "options",
        allowed_keys: RUNTIME_LOGS_KEYS,
    },
    RecordArgSpec {
        method_path: "prism.runtimeTimeline",
        arg_index: 0,
        arg_name: "options",
        allowed_keys: RUNTIME_TIMELINE_KEYS,
    },
    RecordArgSpec {
        method_path: "prism.validationFeedback",
        arg_index: 0,
        arg_name: "options",
        allowed_keys: VALIDATION_FEEDBACK_KEYS,
    },
    RecordArgSpec {
        method_path: "prism.memory.recall",
        arg_index: 0,
        arg_name: "options",
        allowed_keys: MEMORY_RECALL_KEYS,
    },
    RecordArgSpec {
        method_path: "prism.memory.outcomes",
        arg_index: 0,
        arg_name: "options",
        allowed_keys: MEMORY_OUTCOMES_KEYS,
    },
    RecordArgSpec {
        method_path: "prism.memory.events",
        arg_index: 0,
        arg_name: "options",
        allowed_keys: MEMORY_EVENTS_KEYS,
    },
    RecordArgSpec {
        method_path: "prism.memoryRecall",
        arg_index: 0,
        arg_name: "options",
        allowed_keys: MEMORY_RECALL_KEYS,
    },
    RecordArgSpec {
        method_path: "prism.memoryOutcomes",
        arg_index: 0,
        arg_name: "options",
        allowed_keys: MEMORY_OUTCOMES_KEYS,
    },
    RecordArgSpec {
        method_path: "prism.memoryEvents",
        arg_index: 0,
        arg_name: "options",
        allowed_keys: MEMORY_EVENTS_KEYS,
    },
    RecordArgSpec {
        method_path: "prism.curator.job",
        arg_index: 0,
        arg_name: "options",
        allowed_keys: CURATOR_JOB_KEYS,
    },
    RecordArgSpec {
        method_path: "prism.curator.jobs",
        arg_index: 0,
        arg_name: "options",
        allowed_keys: CURATOR_JOB_KEYS,
    },
    RecordArgSpec {
        method_path: "prism.curator.proposals",
        arg_index: 0,
        arg_name: "options",
        allowed_keys: CURATOR_PROPOSALS_KEYS,
    },
    RecordArgSpec {
        method_path: "prism.mcpLog",
        arg_index: 0,
        arg_name: "options",
        allowed_keys: MCP_LOG_KEYS,
    },
    RecordArgSpec {
        method_path: "prism.slowMcpCalls",
        arg_index: 0,
        arg_name: "options",
        allowed_keys: MCP_LOG_KEYS,
    },
    RecordArgSpec {
        method_path: "prism.mcpStats",
        arg_index: 0,
        arg_name: "options",
        allowed_keys: MCP_LOG_KEYS,
    },
    RecordArgSpec {
        method_path: "prism.queryLog",
        arg_index: 0,
        arg_name: "options",
        allowed_keys: QUERY_LOG_KEYS,
    },
    RecordArgSpec {
        method_path: "prism.slowQueries",
        arg_index: 0,
        arg_name: "options",
        allowed_keys: QUERY_LOG_KEYS,
    },
    RecordArgSpec {
        method_path: "prism.file(path).read",
        arg_index: 0,
        arg_name: "options",
        allowed_keys: FILE_READ_KEYS,
    },
    RecordArgSpec {
        method_path: "prism.file(path).around",
        arg_index: 0,
        arg_name: "options",
        allowed_keys: FILE_AROUND_KEYS,
    },
    RecordArgSpec {
        method_path: "prism.validationPlan",
        arg_index: 0,
        arg_name: "input",
        allowed_keys: VALIDATION_PLAN_KEYS,
    },
    RecordArgSpec {
        method_path: "prism.impact",
        arg_index: 0,
        arg_name: "input",
        allowed_keys: IMPACT_KEYS,
    },
    RecordArgSpec {
        method_path: "prism.afterEdit",
        arg_index: 0,
        arg_name: "input",
        allowed_keys: IMPACT_KEYS,
    },
    RecordArgSpec {
        method_path: "prism.commandMemory",
        arg_index: 0,
        arg_name: "input",
        allowed_keys: COMMAND_MEMORY_KEYS,
    },
];

pub(crate) fn typecheck_query(code: &str, mode: StaticCheckMode) -> Result<()> {
    let wrapped = mode.wrapped_source(code);
    let parsed = match parse_program(ParseParams {
        specifier: ModuleSpecifier::parse("file:///prism/typecheck.ts")?,
        text: wrapped.into(),
        media_type: MediaType::TypeScript,
        capture_tokens: false,
        maybe_syntax: None,
        scope_analysis: false,
    }) {
        Ok(parsed) => parsed,
        Err(_) => return Ok(()),
    };
    let mut checker = PrismApiTypechecker {
        parsed: &parsed,
        attempted_mode: mode.attempted_mode(),
        issue: None,
    };
    parsed.program_ref().visit_with(&mut checker);
    match checker.issue {
        Some(error) => Err(error),
        None => Ok(()),
    }
}

struct PrismApiTypechecker<'a> {
    parsed: &'a ParsedSource,
    attempted_mode: &'static str,
    issue: Option<anyhow::Error>,
}

impl PrismApiTypechecker<'_> {
    fn push_issue(&mut self, error: anyhow::Error) {
        if self.issue.is_none() {
            self.issue = Some(error);
        }
    }

    fn line_column(&self, span_lo: deno_ast::swc::common::BytePos) -> (usize, usize) {
        let pos = SourcePos::unsafely_from_byte_pos(span_lo);
        let display = self.parsed.text_info_lazy().line_and_column_display(pos);
        (display.line_number.saturating_sub(1), display.column_number)
    }

    fn unknown_method_error(
        &self,
        observed: &str,
        span_lo: deno_ast::swc::common::BytePos,
    ) -> anyhow::Error {
        let (line, column) = self.line_column(span_lo);
        let suggestion = closest_prism_api_path(observed);
        let next_action = suggestion.as_ref().map_or_else(
            || {
                "Check `prism://api-reference` for the stable PRISM query helpers, then retry."
                    .to_string()
            },
            |suggested| {
                format!(
                    "`{observed}` is not a stable PRISM query helper. Did you mean `{suggested}`? Check `prism://api-reference` for the exact method signature and retry."
                )
            },
        );
        static_typecheck_error(
            format!("`{observed}` is not part of the stable PRISM query API."),
            self.attempted_mode,
            line,
            column,
            json!({
                "observedPath": observed,
                "didYouMean": suggestion,
                "nextAction": next_action,
            }),
        )
    }

    fn invalid_record_error(
        &self,
        method_path: &str,
        argument_name: &str,
        span_lo: deno_ast::swc::common::BytePos,
        extra_data: Value,
        detail: String,
        next_action: String,
    ) -> anyhow::Error {
        let (line, column) = self.line_column(span_lo);
        let mut data = Map::new();
        data.insert("method".to_string(), json!(method_path));
        data.insert("argumentName".to_string(), json!(argument_name));
        data.insert("nextAction".to_string(), json!(next_action));
        if let Some(extra) = extra_data.as_object() {
            for (key, value) in extra {
                data.insert(key.clone(), value.clone());
            }
        }
        static_typecheck_error(
            detail,
            self.attempted_mode,
            line,
            column,
            Value::Object(data),
        )
    }

    fn check_call_expr(&mut self, call: &CallExpr) {
        if self.issue.is_some() {
            return;
        }
        let Some(method_path) = call_path(call) else {
            return;
        };
        if !is_known_prism_api_path(&method_path) {
            self.push_issue(self.unknown_method_error(&method_path, call.span.lo));
            return;
        }
        let Some(spec) = RECORD_ARG_SPECS
            .iter()
            .find(|spec| spec.method_path == method_path)
        else {
            return;
        };
        if spec.method_path == "prism.decodeConcept"
            && call
                .args
                .first()
                .is_some_and(|arg| matches!(&*arg.expr, Expr::Lit(Lit::Str(_))))
        {
            return;
        }
        let Some(argument) = call.args.get(spec.arg_index) else {
            return;
        };
        match record_arg_keys(argument, spec.allowed_keys) {
            RecordArgCheck::Ok => {}
            RecordArgCheck::Skip => {}
            RecordArgCheck::InvalidType(detail) => {
                let next_action = format!(
                    "Pass a plain object for `{}` on `{}` or omit it entirely. Check `prism://api-reference` for the exact shape.",
                    spec.arg_name, spec.method_path
                );
                self.push_issue(self.invalid_record_error(
                    spec.method_path,
                    spec.arg_name,
                    argument.expr.span().lo,
                    json!({ "error": detail }),
                    detail,
                    next_action,
                ));
            }
            RecordArgCheck::UnknownKeys {
                invalid_keys,
                did_you_mean,
            } => {
                let invalid_summary = invalid_keys
                    .iter()
                    .map(|key| format!("`{key}`"))
                    .collect::<Vec<_>>()
                    .join(", ");
                let suggestion_summary = did_you_mean
                    .as_object()
                    .into_iter()
                    .flat_map(|map| map.iter())
                    .map(|(key, value)| format!("`{key}` -> `{}`", value.as_str().unwrap_or("")))
                    .collect::<Vec<_>>()
                    .join(", ");
                let next_action = if suggestion_summary.is_empty() {
                    format!(
                        "Use only documented keys for `{}` and retry. Check `prism://api-reference` for the exact shape.",
                        spec.method_path
                    )
                } else {
                    format!(
                        "Use the documented key spelling instead ({suggestion_summary}) and retry. Check `prism://api-reference` for the exact shape."
                    )
                };
                let detail = format!(
                    "unknown {} {invalid_summary} in `{}` for `{}`.",
                    if invalid_keys.len() == 1 {
                        "key"
                    } else {
                        "keys"
                    },
                    spec.arg_name,
                    spec.method_path
                );
                self.push_issue(self.invalid_record_error(
                    spec.method_path,
                    spec.arg_name,
                    argument.expr.span().lo,
                    json!({
                        "invalidKeys": invalid_keys,
                        "didYouMean": did_you_mean,
                        "error": detail,
                    }),
                    detail,
                    next_action,
                ));
            }
        }
    }
}

impl Visit for PrismApiTypechecker<'_> {
    fn visit_program(&mut self, program: &Program) {
        if self.issue.is_none() {
            program.visit_children_with(self);
        }
    }

    fn visit_call_expr(&mut self, call: &CallExpr) {
        self.check_call_expr(call);
        if self.issue.is_none() {
            call.visit_children_with(self);
        }
    }
}

enum RecordArgCheck {
    Ok,
    Skip,
    InvalidType(String),
    UnknownKeys {
        invalid_keys: Vec<String>,
        did_you_mean: Value,
    },
}

fn record_arg_keys(argument: &ExprOrSpread, allowed_keys: &[&str]) -> RecordArgCheck {
    match &*argument.expr {
        Expr::Object(object) => validate_object_keys(object, allowed_keys),
        Expr::Lit(Lit::Null(_)) | Expr::Ident(_) | Expr::Member(_) | Expr::Call(_) => {
            RecordArgCheck::Skip
        }
        Expr::Paren(paren) => match &*paren.expr {
            Expr::Object(object) => validate_object_keys(object, allowed_keys),
            _ => RecordArgCheck::Skip,
        },
        _ => RecordArgCheck::InvalidType(
            "record-shaped arguments must be plain objects when provided.".to_string(),
        ),
    }
}

fn validate_object_keys(object: &ObjectLit, allowed_keys: &[&str]) -> RecordArgCheck {
    let mut invalid_keys = Vec::new();
    let mut did_you_mean = Map::new();
    for prop in &object.props {
        let Some(name) = property_name(prop) else {
            continue;
        };
        if allowed_keys.contains(&name.as_str()) {
            continue;
        }
        if let Some(suggestion) = suggest_api_token(&name, allowed_keys) {
            did_you_mean.insert(name.clone(), Value::String(suggestion));
        }
        invalid_keys.push(name);
    }
    if invalid_keys.is_empty() {
        RecordArgCheck::Ok
    } else {
        RecordArgCheck::UnknownKeys {
            invalid_keys,
            did_you_mean: Value::Object(did_you_mean),
        }
    }
}

fn property_name(prop: &PropOrSpread) -> Option<String> {
    match prop {
        PropOrSpread::Spread(_) => None,
        PropOrSpread::Prop(prop) => match &**prop {
            Prop::KeyValue(key_value) => prop_name(&key_value.key),
            Prop::Shorthand(ident) => Some(ident.sym.to_string()),
            Prop::Assign(assign) => Some(assign.key.sym.to_string()),
            Prop::Getter(getter) => prop_name(&getter.key),
            Prop::Setter(setter) => prop_name(&setter.key),
            Prop::Method(method) => prop_name(&method.key),
        },
    }
}

fn prop_name(name: &PropName) -> Option<String> {
    match name {
        PropName::Ident(ident) => Some(ident.sym.to_string()),
        PropName::Str(value) => Some(value.value.to_string_lossy().to_string()),
        PropName::Num(value) => Some(value.value.to_string()),
        PropName::BigInt(value) => Some(value.value.to_string()),
        PropName::Computed(_) => None,
    }
}

fn call_path(call: &CallExpr) -> Option<String> {
    let Callee::Expr(expr) = &call.callee else {
        return None;
    };
    expr_path(expr)
}

fn expr_path(expr: &Expr) -> Option<String> {
    match expr {
        Expr::Ident(ident) if ident.sym == "prism" => Some("prism".to_string()),
        Expr::Member(member) => {
            let object_path = match &*member.obj {
                Expr::Call(call) => {
                    let base = call_path(call)?;
                    (base == "prism.file").then(|| "prism.file(path)".to_string())
                }
                other => expr_path(other),
            }?;
            let prop = member_prop_name(&member.prop)?;
            Some(format!("{object_path}.{prop}"))
        }
        _ => None,
    }
}

fn member_prop_name(prop: &MemberProp) -> Option<String> {
    match prop {
        MemberProp::Ident(ident) => Some(ident.sym.to_string()),
        MemberProp::PrivateName(_) | MemberProp::Computed(_) => None,
    }
}
