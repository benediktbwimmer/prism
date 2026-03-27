use std::path::Path;
use std::sync::{Arc, Mutex};

use anyhow::{anyhow, Context, Result};
use prism_ir::{AnchorRef, ArtifactId, CoordinationTaskId, EdgeKind, LineageId, NodeId, PlanId};
use prism_js::{
    ChangedFileView, ChangedSymbolView, DiffHunkView, DiscoveryBundleView, EditContextView,
    FocusedBlockView, PatchEventView, QueryDiagnostic, QueryEnvelope, ReadContextView,
    RecentChangeContextView, RuntimeLogEventView, RuntimeStatusView, ScoredMemoryView,
    SourceExcerptView, SourceSliceView, SubgraphView, SymbolView, TextSearchMatchView,
    ToolCatalogEntryView, ToolSchemaView, ValidationContextView,
};
use prism_memory::{MemoryModule, OutcomeRecallQuery, RecallQuery};
use prism_query::{EditSliceOptions, Prism, SourceExcerptOptions, Symbol};
use serde_json::{json, Value};

use crate::file_queries::{
    file_around, file_read, DEFAULT_FILE_AROUND_CONTEXT_LINES, DEFAULT_FILE_AROUND_MAX_CHARS,
    DEFAULT_FILE_READ_MAX_CHARS,
};
use crate::runtime_views::{runtime_logs, runtime_status, runtime_timeline};
use crate::text_search::search_text;
use crate::{
    ambiguity_diagnostic_data, apply_module_filter, artifact_risk_view, artifact_view,
    blast_radius_view, blocker_view, change_impact_view, changed_files, changed_symbols,
    claim_view, co_change_view, combined_parse_typescript_error, conflict_view, convert_anchors, convert_node_id,
    coordination_task_view, current_timestamp, diff_for, drift_candidate_view, edge_kind_label,
    edge_view, edit_slice_for_symbol, entrypoints_for, focused_block_for_symbol, js_runtime,
    is_query_parse_error, lineage_view, merge_node_ids, merge_promoted_checks, missing_return_hint, next_reads,
    owner_symbol_views_for_query, owner_symbol_views_for_target, owner_views_for_target,
    parse_capability, parse_claim_mode, parse_event_actor, parse_memory_kind, parse_node_kind,
    parse_outcome_kind, parse_outcome_result, parse_typescript_error, plan_view,
    policy_violation_record_view, promoted_memory_entries, promoted_summary_texts,
    promoted_validation_checks, query_diagnostic, rank_search_results, read_context_view_cached,
    recent_change_context_view_cached, recent_patches, relations_view, result_decode_error,
    runtime_or_serialization_error, scored_memory_view, search_queries, source_excerpt_for_symbol,
    spec_cluster_view, spec_drift_explanation_view, symbol_for, symbol_view, symbol_views_for_ids,
    task_intent_view, task_journal_view, task_risk_view, task_validation_recipe_view,
    tool_catalog_views, tool_schema_view, validation_context_view_cached,
    validation_recipe_view_with, where_used, AnchorListArgs, CallGraphArgs, ChangedFilesArgs,
    ChangedSymbolsArgs, CoordinationTaskTargetArgs, CuratorJobArgs, CuratorJobsArgs, DiffForArgs,
    DiscoveryTargetArgs, EditSliceArgs, FileAroundArgs, FileReadArgs, ImplementationTargetArgs,
    LimitArgs, MemoryOutcomeArgs, MemoryRecallArgs, NodeIdInput, OwnerLookupArgs,
    PendingReviewsArgs, PlanTargetArgs, PolicyViolationQueryArgs, QueryHost, QueryLanguage,
    QueryLogArgs, QueryRun, QueryTraceArgs, RecentPatchesArgs, RuntimeLogArgs, RuntimeTimelineArgs,
    SearchAmbiguityContext, SearchArgs, SearchTextArgs, SemanticContextCache, SimulateClaimArgs,
    SourceExcerptArgs, SymbolQueryArgs, SymbolTargetArgs, TaskChangesArgs, TaskJournalArgs,
    TaskScopeMode, TaskTargetArgs, ToolNameArgs, WhereUsedArgs, DEFAULT_CALL_GRAPH_DEPTH,
    DEFAULT_SEARCH_LIMIT, DEFAULT_TASK_JOURNAL_EVENT_LIMIT, DEFAULT_TASK_JOURNAL_MEMORY_LIMIT,
    INSIGHT_LIMIT, QUERY_RUNTIME_ERROR_MARKER, QUERY_SERIALIZATION_ERROR_MARKER,
    USER_SNIPPET_LOCATION_MARKER, USER_SNIPPET_MARKER,
};

#[derive(Debug, Clone, Copy)]
enum TsSnippetMode {
    StatementBody,
    ImplicitExpression,
}

impl TsSnippetMode {
    fn code(self) -> &'static str {
        match self {
            TsSnippetMode::StatementBody => "statement_body",
            TsSnippetMode::ImplicitExpression => "implicit_expression",
        }
    }
}

struct PreparedTypescriptQuery {
    source: String,
    user_snippet_first_line: usize,
}

struct TypescriptAttempt {
    execution: QueryExecution,
    result: Value,
    json_bytes: usize,
    output_cap_hit: bool,
}

impl QueryHost {
    pub(crate) fn execute(&self, code: &str, language: QueryLanguage) -> Result<QueryEnvelope> {
        match language {
            QueryLanguage::Ts => self.execute_typescript(code),
        }
    }

    #[cfg(test)]
    pub(crate) fn symbol_query(&self, query: &str) -> Result<QueryEnvelope> {
        let query_run = self.begin_query_run("symbolQuery", format!("symbol({query})"));
        let mut execution = None;
        match (|| -> Result<(Value, Vec<QueryDiagnostic>, usize)> {
            self.refresh_workspace()?;
            let created =
                QueryExecution::new(self.clone(), self.current_prism(), query_run.clone());
            execution = Some(created.clone());
            let result = serde_json::to_value(created.best_symbol(query)?)?;
            let diagnostics = created.diagnostics();
            let json_bytes = serde_json::to_vec(&result)?.len();
            Ok((result, diagnostics, json_bytes))
        })() {
            Ok((result, diagnostics, json_bytes)) => {
                query_run.finish_success(
                    self.query_log_store.as_ref(),
                    &result,
                    diagnostics.clone(),
                    json_bytes,
                    false,
                );
                Ok(QueryEnvelope {
                    result,
                    diagnostics,
                })
            }
            Err(error) => {
                query_run.finish_error(
                    self.query_log_store.as_ref(),
                    execution
                        .as_ref()
                        .map(QueryExecution::diagnostics)
                        .unwrap_or_default(),
                    error.to_string(),
                );
                Err(error)
            }
        }
    }

    #[cfg(test)]
    pub(crate) fn search_query(&self, args: SearchArgs) -> Result<QueryEnvelope> {
        let query_run = self.begin_query_run("searchQuery", format!("search({})", args.query));
        let mut execution = None;
        match (|| -> Result<(Value, Vec<QueryDiagnostic>, usize)> {
            self.refresh_workspace()?;
            let created =
                QueryExecution::new(self.clone(), self.current_prism(), query_run.clone());
            execution = Some(created.clone());
            let result = serde_json::to_value(created.search(args)?)?;
            let diagnostics = created.diagnostics();
            let json_bytes = serde_json::to_vec(&result)?.len();
            Ok((result, diagnostics, json_bytes))
        })() {
            Ok((result, diagnostics, json_bytes)) => {
                query_run.finish_success(
                    self.query_log_store.as_ref(),
                    &result,
                    diagnostics.clone(),
                    json_bytes,
                    false,
                );
                Ok(QueryEnvelope {
                    result,
                    diagnostics,
                })
            }
            Err(error) => {
                query_run.finish_error(
                    self.query_log_store.as_ref(),
                    execution
                        .as_ref()
                        .map(QueryExecution::diagnostics)
                        .unwrap_or_default(),
                    error.to_string(),
                );
                Err(error)
            }
        }
    }

    fn execute_typescript(&self, code: &str) -> Result<QueryEnvelope> {
        let query_run = self.begin_query_run("typescript", code);
        let mut execution = None;
        match (|| -> Result<(Value, Vec<QueryDiagnostic>, usize, bool)> {
            self.refresh_workspace()?;
            let mut statement_attempt = match self.execute_typescript_attempt(
                code,
                TsSnippetMode::StatementBody,
                query_run.clone(),
            ) {
                Ok(attempt) => attempt,
                Err(statement_error) => {
                    match self.execute_typescript_attempt(
                        code,
                        TsSnippetMode::ImplicitExpression,
                        query_run.clone(),
                    ) {
                        Ok(expression_attempt) => expression_attempt,
                        Err(expression_error) => {
                            if is_query_parse_error(&statement_error)
                                && is_query_parse_error(&expression_error)
                            {
                                return Err(combined_parse_typescript_error(
                                    statement_error,
                                    expression_error,
                                ));
                            }
                            return Err(statement_error);
                        }
                    }
                }
            };
            execution = Some(statement_attempt.execution.clone());
            if !statement_attempt.output_cap_hit
                && missing_return_hint(code, &statement_attempt.result)
            {
                if let Ok(expression_attempt) = self.execute_typescript_attempt(
                    code,
                    TsSnippetMode::ImplicitExpression,
                    query_run.clone(),
                ) {
                    execution = Some(expression_attempt.execution.clone());
                    statement_attempt = expression_attempt;
                } else {
                    statement_attempt.execution.push_diagnostic(
                        "query_return_missing",
                        "Query returned undefined, which usually means the snippet did not return a final value.",
                        Some(json!({
                            "nextAction": "Add `return ...` as the final statement if you meant the query to produce a result.",
                        })),
                    );
                }
            }
            let diagnostics = statement_attempt.execution.diagnostics();
            Ok((
                statement_attempt.result,
                diagnostics,
                statement_attempt.json_bytes,
                statement_attempt.output_cap_hit,
            ))
        })() {
            Ok((result, diagnostics, json_bytes, output_cap_hit)) => {
                query_run.finish_success(
                    self.query_log_store.as_ref(),
                    &result,
                    diagnostics.clone(),
                    json_bytes,
                    output_cap_hit,
                );
                Ok(QueryEnvelope {
                    result,
                    diagnostics,
                })
            }
            Err(error) => {
                query_run.finish_error(
                    self.query_log_store.as_ref(),
                    execution
                        .as_ref()
                        .map(QueryExecution::diagnostics)
                        .unwrap_or_default(),
                    error.to_string(),
                );
                Err(error)
            }
        }
    }

    pub(crate) fn co_change_neighbors_value(&self, id: &NodeId) -> Result<Value> {
        let prism = self.current_prism();
        serde_json::to_value(
            prism
                .co_change_neighbors(id, 8)
                .into_iter()
                .map(co_change_view)
                .collect::<Vec<_>>(),
        )
        .map_err(Into::into)
    }

    fn execute_typescript_attempt(
        &self,
        code: &str,
        mode: TsSnippetMode,
        query_run: QueryRun,
    ) -> Result<TypescriptAttempt> {
        let prepared = prepare_typescript_query(code, mode);
        let transpiled = js_runtime::transpile_typescript(&prepared.source).map_err(|error| {
            parse_typescript_error(
                error,
                code,
                prepared.user_snippet_first_line,
                mode.code(),
            )
        })?;
        let execution = QueryExecution::new(self.clone(), self.current_prism(), query_run);
        let raw_result = self
            .worker_pool
            .execute(transpiled, execution.clone())
            .map_err(|error| {
                runtime_or_serialization_error(
                    error,
                    code,
                    prepared.user_snippet_first_line,
                    mode.code(),
                )
            })?;
        let mut result = serde_json::from_str(&raw_result)
            .map_err(|error| result_decode_error(error.into(), &raw_result))?;
        let mut output_cap_hit = false;
        let limits = self.session.limits();
        if raw_result.len() > limits.max_output_json_bytes {
            execution.push_diagnostic(
                "result_truncated",
                format!(
                    "Query output exceeded the {} byte session cap.",
                    limits.max_output_json_bytes
                ),
                Some(json!({
                    "applied": limits.max_output_json_bytes,
                    "observed": raw_result.len(),
                })),
            );
            result = Value::Null;
            output_cap_hit = true;
        }
        Ok(TypescriptAttempt {
            execution,
            result,
            json_bytes: raw_result.len(),
            output_cap_hit,
        })
    }
}

fn prepare_typescript_query(code: &str, mode: TsSnippetMode) -> PreparedTypescriptQuery {
    let user_body = match mode {
        TsSnippetMode::StatementBody => code.to_string(),
        TsSnippetMode::ImplicitExpression => format!("return (\n{}\n);", code),
    };
    let source = format!(
        "(async function() {{\n  const __prismLocationRegex = /(?:file:\\/\\/\\/prism\\/query\\.ts|eval_script):(?<line>\\d+):(?<column>\\d+)/;\n  const __prismParseLocation = (value) => {{\n    const __prismMatch = typeof value === \"string\" ? value.match(__prismLocationRegex) : null;\n    if (!__prismMatch || !__prismMatch.groups) {{\n      return null;\n    }}\n    return {{\n      line: Number(__prismMatch.groups.line),\n      column: Number(__prismMatch.groups.column),\n    }};\n  }};\n  const __prismFormatError = (error) => {{\n    const __prismMessage = error && typeof error === \"object\" && \"message\" in error && error.message\n      ? String(error.message)\n      : String(error);\n    const __prismStack = error && typeof error === \"object\" && \"stack\" in error && error.stack\n      ? String(error.stack)\n      : null;\n    return __prismStack && __prismStack.includes(__prismMessage)\n      ? __prismStack\n      : __prismStack\n        ? `${{__prismMessage}}\\n${{__prismStack}}`\n        : __prismMessage;\n  }};\n  const __prismUserLocation = (error, baseLine) => {{\n    if (typeof baseLine !== \"number\") {{\n      return null;\n    }}\n    const __prismStack = error && typeof error === \"object\" && \"stack\" in error && error.stack\n      ? String(error.stack)\n      : \"\";\n    const __prismLines = __prismStack.split(\"\\n\");\n    const __prismFrame = __prismLines.find((line) => line.includes(\"__prismUserQuery\"))\n      || __prismLines.find((line) => line.includes(\"eval_script:\"));\n    const __prismLocation = __prismParseLocation(__prismFrame);\n    if (!__prismLocation) {{\n      return null;\n    }}\n    return {{\n      line: Math.max(1, __prismLocation.line - baseLine + 1),\n      column: __prismLocation.column,\n    }};\n  }};\n  const __prismThrowTaggedError = (marker, error, userLocation = null) => {{\n    const __prismFormatted = __prismFormatError(error);\n    const __prismHeadline = __prismFormatted.split(\"\\n\")[0] || String(error);\n    const __prismUserLocationLine = userLocation\n      ? `\\n{} ${{userLocation.line}}:${{userLocation.column}}`\n      : \"\";\n    const __prismWrapped = new Error(`${{marker}}\\n${{__prismHeadline}}${{__prismUserLocationLine}}`);\n    __prismWrapped.stack = `${{userLocation ? `{} ${{userLocation.line}}:${{userLocation.column}}\\n` : \"\"}}${{__prismFormatted}}`;\n    throw __prismWrapped;\n  }};\n  let __prismUserSnippetBaseLine = null;\n  const __prismUserQuery = async () => {{\n    const __prismBaseLocation = __prismParseLocation(new Error().stack || \"\");\n    __prismUserSnippetBaseLine = __prismBaseLocation ? __prismBaseLocation.line + 1 : null;\n{}\n{}\n  }};\n  let __prismResult;\n  try {{\n    __prismResult = await __prismUserQuery();\n  }} catch (error) {{\n    __prismThrowTaggedError(\"{}\", error, __prismUserLocation(error, __prismUserSnippetBaseLine));\n  }}\n  try {{\n    return __prismResult === undefined ? \"null\" : JSON.stringify(__prismResult);\n  }} catch (error) {{\n    __prismThrowTaggedError(\"{}\", error);\n  }}\n}})();\n",
        USER_SNIPPET_LOCATION_MARKER,
        USER_SNIPPET_LOCATION_MARKER,
        USER_SNIPPET_MARKER,
        user_body,
        QUERY_RUNTIME_ERROR_MARKER,
        QUERY_SERIALIZATION_ERROR_MARKER,
    );
    let user_snippet_first_line = source
        .lines()
        .position(|line| line.trim() == USER_SNIPPET_MARKER)
        .map(|index| index + 2)
        .unwrap_or(1);
    PreparedTypescriptQuery {
        source,
        user_snippet_first_line,
    }
}

#[derive(Clone)]
pub(crate) struct QueryExecution {
    host: QueryHost,
    prism: Arc<Prism>,
    query_run: QueryRun,
    diagnostics: Arc<Mutex<Vec<QueryDiagnostic>>>,
    semantic_context_cache: Arc<Mutex<SemanticContextCache>>,
}

impl QueryExecution {
    pub(crate) fn new(host: QueryHost, prism: Arc<Prism>, query_run: QueryRun) -> Self {
        Self {
            host,
            prism,
            query_run,
            diagnostics: Arc::new(Mutex::new(Vec::new())),
            semantic_context_cache: Arc::new(Mutex::new(SemanticContextCache::default())),
        }
    }

    pub(crate) fn diagnostics(&self) -> Vec<QueryDiagnostic> {
        self.diagnostics
            .lock()
            .expect("diagnostics lock poisoned")
            .clone()
    }

    pub(crate) fn push_diagnostic(
        &self,
        code: &str,
        message: impl Into<String>,
        data: Option<Value>,
    ) {
        self.diagnostics
            .lock()
            .expect("diagnostics lock poisoned")
            .push(query_diagnostic(code, message, data));
    }

    pub(crate) fn dispatch_enveloped(&self, operation: &str, args_json: &str) -> String {
        match self.dispatch(operation, args_json) {
            Ok(value) => json!({ "ok": true, "value": value }).to_string(),
            Err(error) => json!({ "ok": false, "error": error.to_string() }).to_string(),
        }
    }

    pub(crate) fn dispatch(&self, operation: &str, args_json: &str) -> Result<Value> {
        let phase_started = std::time::Instant::now();
        let args = if args_json.trim().is_empty() {
            Value::Object(Default::default())
        } else {
            serde_json::from_str(args_json).context("failed to parse host-call arguments")?
        };
        let phase_args = args.clone();

        self.ensure_operation_enabled(operation)?;

        let result = match operation {
            "symbol" => {
                let args: SymbolQueryArgs = serde_json::from_value(args)?;
                Ok(serde_json::to_value(self.best_symbol(&args.query)?)?)
            }
            "symbols" => {
                let args: SymbolQueryArgs = serde_json::from_value(args)?;
                Ok(serde_json::to_value(self.symbols(&args.query)?)?)
            }
            "search" => {
                let args: SearchArgs = serde_json::from_value(args)?;
                Ok(serde_json::to_value(self.search(args)?)?)
            }
            "searchText" => {
                let args: SearchTextArgs = serde_json::from_value(args)?;
                Ok(serde_json::to_value(self.search_text(args)?)?)
            }
            "changedFiles" => {
                let args: ChangedFilesArgs = serde_json::from_value(args)?;
                Ok(serde_json::to_value(self.changed_files(args)?)?)
            }
            "changedSymbols" => {
                let args: ChangedSymbolsArgs = serde_json::from_value(args)?;
                Ok(serde_json::to_value(self.changed_symbols(args)?)?)
            }
            "recentPatches" => {
                let args: RecentPatchesArgs = serde_json::from_value(args)?;
                Ok(serde_json::to_value(self.recent_patches(args)?)?)
            }
            "diffFor" => {
                let args: DiffForArgs = serde_json::from_value(args)?;
                Ok(serde_json::to_value(self.diff_for(args)?)?)
            }
            "taskChanges" => {
                let args: TaskChangesArgs = serde_json::from_value(args)?;
                Ok(serde_json::to_value(self.task_changes(args)?)?)
            }
            "runtimeStatus" => Ok(serde_json::to_value(self.runtime_status()?)?),
            "runtimeLogs" => {
                let args: RuntimeLogArgs = serde_json::from_value(args)?;
                Ok(serde_json::to_value(self.runtime_logs(args)?)?)
            }
            "runtimeTimeline" => {
                let args: RuntimeTimelineArgs = serde_json::from_value(args)?;
                Ok(serde_json::to_value(self.runtime_timeline(args)?)?)
            }
            "queryLog" => {
                let args: QueryLogArgs = serde_json::from_value(args)?;
                Ok(serde_json::to_value(self.host.query_log_entries(args))?)
            }
            "slowQueries" => {
                let args: QueryLogArgs = serde_json::from_value(args)?;
                Ok(serde_json::to_value(self.host.slow_query_entries(args))?)
            }
            "queryTrace" => {
                let args: QueryTraceArgs = serde_json::from_value(args)?;
                let trace = self.host.query_trace_view(&args.id);
                if trace.is_none() {
                    self.push_diagnostic(
                        "anchor_unresolved",
                        format!("No query trace matched `{}`.", args.id),
                        Some(json!({ "queryId": args.id })),
                    );
                }
                Ok(serde_json::to_value(trace)?)
            }
            "entrypoints" => Ok(serde_json::to_value(self.entrypoints()?)?),
            "plan" => {
                let args: PlanTargetArgs = serde_json::from_value(args)?;
                Ok(serde_json::to_value(
                    self.prism
                        .coordination_plan(&PlanId::new(args.plan_id))
                        .map(plan_view),
                )?)
            }
            "coordinationTask" => {
                let args: CoordinationTaskTargetArgs = serde_json::from_value(args)?;
                Ok(serde_json::to_value(
                    self.prism
                        .coordination_task(&CoordinationTaskId::new(args.task_id))
                        .map(coordination_task_view),
                )?)
            }
            "readyTasks" => {
                let args: PlanTargetArgs = serde_json::from_value(args)?;
                Ok(serde_json::to_value(
                    self.prism
                        .ready_tasks(&PlanId::new(args.plan_id), current_timestamp())
                        .into_iter()
                        .map(coordination_task_view)
                        .collect::<Vec<_>>(),
                )?)
            }
            "claims" => {
                let args: AnchorListArgs = serde_json::from_value(args)?;
                Ok(serde_json::to_value(
                    self.prism
                        .claims(&convert_anchors(args.anchors)?, current_timestamp())
                        .into_iter()
                        .map(claim_view)
                        .collect::<Vec<_>>(),
                )?)
            }
            "conflicts" => {
                let args: AnchorListArgs = serde_json::from_value(args)?;
                Ok(serde_json::to_value(
                    self.prism
                        .conflicts(&convert_anchors(args.anchors)?, current_timestamp())
                        .into_iter()
                        .map(conflict_view)
                        .collect::<Vec<_>>(),
                )?)
            }
            "blockers" => {
                let args: CoordinationTaskTargetArgs = serde_json::from_value(args)?;
                let blockers = self.prism.blockers(
                    &CoordinationTaskId::new(args.task_id.clone()),
                    current_timestamp(),
                );
                if !blockers.is_empty() {
                    self.push_diagnostic(
                        "task_blocked",
                        format!(
                            "Coordination task `{}` currently has blockers.",
                            args.task_id
                        ),
                        Some(json!({ "taskId": args.task_id, "count": blockers.len() })),
                    );
                }
                if blockers
                    .iter()
                    .any(|blocker| blocker.kind == prism_coordination::BlockerKind::StaleRevision)
                {
                    self.push_diagnostic(
                        "stale_revision",
                        "The coordination task is based on a stale workspace revision.",
                        None,
                    );
                }
                if blockers.iter().any(|blocker| {
                    blocker.kind == prism_coordination::BlockerKind::ValidationRequired
                }) {
                    self.push_diagnostic(
                        "validation_required",
                        "The coordination task is missing required validations.",
                        None,
                    );
                }
                if blockers.iter().any(|blocker| {
                    blocker.kind == prism_coordination::BlockerKind::RiskReviewRequired
                        || blocker.kind == prism_coordination::BlockerKind::ArtifactStale
                }) {
                    self.push_diagnostic(
                        "task_risk_blocked",
                        "The coordination task is blocked by risk or stale artifact state.",
                        None,
                    );
                }
                Ok(serde_json::to_value(
                    blockers.into_iter().map(blocker_view).collect::<Vec<_>>(),
                )?)
            }
            "pendingReviews" => {
                let args: PendingReviewsArgs = serde_json::from_value(args)?;
                Ok(serde_json::to_value(
                    self.prism
                        .pending_reviews(
                            args.plan_id
                                .as_ref()
                                .map(|plan_id| PlanId::new(plan_id.clone()))
                                .as_ref(),
                        )
                        .into_iter()
                        .map(artifact_view)
                        .collect::<Vec<_>>(),
                )?)
            }
            "artifacts" => {
                let args: CoordinationTaskTargetArgs = serde_json::from_value(args)?;
                Ok(serde_json::to_value(
                    self.prism
                        .artifacts(&CoordinationTaskId::new(args.task_id))
                        .into_iter()
                        .map(artifact_view)
                        .collect::<Vec<_>>(),
                )?)
            }
            "policyViolations" => {
                let args: PolicyViolationQueryArgs = serde_json::from_value(args)?;
                Ok(serde_json::to_value(
                    self.prism
                        .policy_violations(
                            args.plan_id
                                .as_ref()
                                .map(|plan_id| PlanId::new(plan_id.clone()))
                                .as_ref(),
                            args.task_id
                                .as_ref()
                                .map(|task_id| CoordinationTaskId::new(task_id.clone()))
                                .as_ref(),
                            args.limit.unwrap_or(20),
                        )
                        .into_iter()
                        .map(policy_violation_record_view)
                        .collect::<Vec<_>>(),
                )?)
            }
            "taskBlastRadius" => {
                let args: CoordinationTaskTargetArgs = serde_json::from_value(args)?;
                let task_id = CoordinationTaskId::new(args.task_id);
                Ok(serde_json::to_value(
                    self.prism.task_blast_radius(&task_id).map(|impact| {
                        let anchors = self
                            .prism
                            .coordination_task(&task_id)
                            .map(|task| task.anchors)
                            .unwrap_or_default();
                        let mut view = change_impact_view(impact);
                        view.promoted_summaries = promoted_summary_texts(
                            self.host.session.as_ref(),
                            self.prism.as_ref(),
                            &anchors,
                        );
                        view
                    }),
                )?)
            }
            "taskValidationRecipe" => {
                let args: CoordinationTaskTargetArgs = serde_json::from_value(args)?;
                let task_id = CoordinationTaskId::new(args.task_id);
                Ok(serde_json::to_value(
                    self.prism
                        .task_validation_recipe(&task_id)
                        .map(|mut recipe| {
                            let anchors = self
                                .prism
                                .coordination_task(&task_id)
                                .map(|task| task.anchors)
                                .unwrap_or_default();
                            merge_promoted_checks(
                                &mut recipe.scored_checks,
                                promoted_validation_checks(
                                    self.host.session.as_ref(),
                                    self.prism.as_ref(),
                                    &anchors,
                                ),
                            );
                            recipe.checks = recipe
                                .scored_checks
                                .iter()
                                .map(|check| check.label.clone())
                                .collect::<Vec<_>>();
                            recipe.checks.sort();
                            recipe.checks.dedup();
                            task_validation_recipe_view(recipe)
                        }),
                )?)
            }
            "taskRisk" => {
                let args: CoordinationTaskTargetArgs = serde_json::from_value(args)?;
                let task_id = CoordinationTaskId::new(args.task_id);
                Ok(serde_json::to_value(
                    self.prism
                        .task_risk(&task_id, current_timestamp())
                        .map(|risk| {
                            let task = self.prism.coordination_task(&task_id);
                            let anchors = task
                                .as_ref()
                                .map(|task| task.anchors.clone())
                                .unwrap_or_default();
                            let promoted_summaries = promoted_summary_texts(
                                self.host.session.as_ref(),
                                self.prism.as_ref(),
                                &anchors,
                            );
                            let promoted_risk_boost = promoted_memory_entries(
                                self.host.session.as_ref(),
                                self.prism.as_ref(),
                                &anchors,
                                "risk_summary",
                            )
                            .into_iter()
                            .map(|entry| {
                                let severity_weight = match entry
                                    .metadata
                                    .get("severity")
                                    .and_then(Value::as_str)
                                    .unwrap_or("medium")
                                {
                                    "low" => 0.04,
                                    "high" => 0.12,
                                    _ => 0.08,
                                };
                                severity_weight * entry.trust.clamp(0.0, 1.0)
                            })
                            .sum::<f32>()
                            .min(0.25);
                            let boosted_risk_score =
                                (risk.risk_score + promoted_risk_boost).min(1.0);
                            let review_required = risk.review_required
                                || task
                                    .as_ref()
                                    .and_then(|task| self.prism.coordination_plan(&task.plan))
                                    .and_then(|plan| plan.policy.review_required_above_risk_score)
                                    .map(|threshold| boosted_risk_score >= threshold)
                                    .unwrap_or(false);
                            let mut view = task_risk_view(risk, promoted_summaries);
                            view.risk_score = boosted_risk_score;
                            view.review_required = review_required;
                            view
                        }),
                )?)
            }
            "artifactRisk" => {
                let artifact_id = args
                    .get("artifactId")
                    .and_then(Value::as_str)
                    .ok_or_else(|| anyhow!("artifactId is required"))?;
                let artifact_id = ArtifactId::new(artifact_id.to_string());
                Ok(serde_json::to_value(
                    self.prism
                        .artifact_risk(&artifact_id, current_timestamp())
                        .map(|risk| {
                            let anchors = self
                                .prism
                                .coordination_snapshot()
                                .artifacts
                                .into_iter()
                                .find(|artifact| artifact.id == artifact_id)
                                .map(|artifact| artifact.anchors)
                                .unwrap_or_default();
                            let promoted_summaries = promoted_summary_texts(
                                self.host.session.as_ref(),
                                self.prism.as_ref(),
                                &anchors,
                            );
                            let promoted_risk_boost = promoted_memory_entries(
                                self.host.session.as_ref(),
                                self.prism.as_ref(),
                                &anchors,
                                "risk_summary",
                            )
                            .into_iter()
                            .map(|entry| {
                                let severity_weight = match entry
                                    .metadata
                                    .get("severity")
                                    .and_then(Value::as_str)
                                    .unwrap_or("medium")
                                {
                                    "low" => 0.04,
                                    "high" => 0.12,
                                    _ => 0.08,
                                };
                                severity_weight * entry.trust.clamp(0.0, 1.0)
                            })
                            .sum::<f32>()
                            .min(0.25);
                            let mut view = artifact_risk_view(risk, promoted_summaries);
                            view.risk_score = (view.risk_score + promoted_risk_boost).min(1.0);
                            view
                        }),
                )?)
            }
            "taskIntent" => {
                let args: CoordinationTaskTargetArgs = serde_json::from_value(args)?;
                let task_id = CoordinationTaskId::new(args.task_id);
                Ok(serde_json::to_value(
                    self.prism.task_intent(&task_id).map(task_intent_view),
                )?)
            }
            "simulateClaim" => {
                let args: SimulateClaimArgs = serde_json::from_value(args)?;
                Ok(serde_json::to_value(
                    self.prism
                        .simulate_claim(
                            &self.host.session.session_id(),
                            &convert_anchors(args.anchors)?,
                            parse_capability(&args.capability)?,
                            args.mode.as_deref().map(parse_claim_mode).transpose()?,
                            args.task_id
                                .as_ref()
                                .map(|task_id| CoordinationTaskId::new(task_id.clone()))
                                .as_ref(),
                            current_timestamp(),
                        )
                        .into_iter()
                        .map(conflict_view)
                        .collect::<Vec<_>>(),
                )?)
            }
            "full" => {
                let args: SymbolTargetArgs = serde_json::from_value(args)?;
                let id = self.resolve_target_id(args.id, args.lineage_id)?;
                Ok(serde_json::to_value(
                    symbol_for(self.prism.as_ref(), &id)?.full(),
                )?)
            }
            "fileRead" => {
                let args: FileReadArgs = serde_json::from_value(args)?;
                let excerpt = file_read(&self.host, args.clone())?;
                if excerpt.truncated {
                    let max_chars = args.max_chars.unwrap_or(DEFAULT_FILE_READ_MAX_CHARS);
                    self.push_diagnostic(
                        "result_truncated",
                        format!(
                            "File excerpt for `{}` was truncated by the {max_chars} character cap. Next action: raise `maxChars` or narrow the line range.",
                            args.path
                        ),
                        Some(json!({
                            "operation": "fileRead",
                            "path": args.path,
                            "startLine": args.start_line.unwrap_or(1),
                            "endLine": args.end_line,
                            "maxChars": max_chars,
                            "nextAction": "Use prism.file(path).read({ startLine: ..., endLine: ..., maxChars: ... }) with a tighter range or larger maxChars.",
                        })),
                    );
                }
                Ok(serde_json::to_value(excerpt)?)
            }
            "fileAround" => {
                let args: FileAroundArgs = serde_json::from_value(args)?;
                let slice = file_around(&self.host, args.clone())?;
                if slice.truncated {
                    let max_chars = args.max_chars.unwrap_or(DEFAULT_FILE_AROUND_MAX_CHARS);
                    self.push_diagnostic(
                        "result_truncated",
                        format!(
                            "File context for `{}` around line {} was truncated by the {max_chars} character cap. Next action: raise `maxChars` or narrow the line window.",
                            args.path, args.line
                        ),
                        Some(json!({
                            "operation": "fileAround",
                            "path": args.path,
                            "line": args.line,
                            "before": args.before.unwrap_or(DEFAULT_FILE_AROUND_CONTEXT_LINES),
                            "after": args.after.unwrap_or(DEFAULT_FILE_AROUND_CONTEXT_LINES),
                            "maxChars": max_chars,
                            "nextAction": "Use prism.file(path).around({ line: ..., before: ..., after: ..., maxChars: ... }) with a tighter window or larger maxChars.",
                        })),
                    );
                }
                Ok(serde_json::to_value(slice)?)
            }
            "tools" => Ok(serde_json::to_value(self.tools())?),
            "tool" => {
                let args: ToolNameArgs = serde_json::from_value(args)?;
                Ok(serde_json::to_value(self.tool(&args.name)?)?)
            }
            "excerpt" => {
                let args: SourceExcerptArgs = serde_json::from_value(args)?;
                Ok(serde_json::to_value(self.source_excerpt(args)?)?)
            }
            "editSlice" => {
                let args: EditSliceArgs = serde_json::from_value(args)?;
                Ok(serde_json::to_value(self.edit_slice(args)?)?)
            }
            "focusedBlock" => {
                let args: EditSliceArgs = serde_json::from_value(args)?;
                Ok(serde_json::to_value(self.focused_block(args)?)?)
            }
            "relations" => {
                let args: SymbolTargetArgs = serde_json::from_value(args)?;
                let id = self.resolve_target_id(args.id, args.lineage_id)?;
                Ok(serde_json::to_value(relations_view(
                    self.prism.as_ref(),
                    self.host.session.as_ref(),
                    &id,
                )?)?)
            }
            "callGraph" => {
                let args: CallGraphArgs = serde_json::from_value(args)?;
                Ok(serde_json::to_value(self.call_graph(args)?)?)
            }
            "lineage" => {
                let args: SymbolTargetArgs = serde_json::from_value(args)?;
                let id = self.resolve_target_id(args.id, args.lineage_id)?;
                let lineage = lineage_view(self.prism.as_ref(), &id)?;
                if lineage
                    .as_ref()
                    .is_some_and(|view| view.history.iter().any(|event| event.kind == "Ambiguous"))
                {
                    self.push_diagnostic(
                        "lineage_uncertain",
                        format!("Lineage for `{}` contains ambiguous history.", id.path),
                        Some(json!({ "id": id.path })),
                    );
                }
                Ok(serde_json::to_value(lineage)?)
            }
            "relatedFailures" => {
                let args: SymbolTargetArgs = serde_json::from_value(args)?;
                let id = self.resolve_target_id(args.id, args.lineage_id)?;
                serde_json::to_value(self.prism.related_failures(&id)).map_err(Into::into)
            }
            "coChangeNeighbors" => {
                let args: SymbolTargetArgs = serde_json::from_value(args)?;
                let id = self.resolve_target_id(args.id, args.lineage_id)?;
                self.host.co_change_neighbors_value(&id)
            }
            "blastRadius" => {
                let args: SymbolTargetArgs = serde_json::from_value(args)?;
                let id = self.resolve_target_id(args.id, args.lineage_id)?;
                Ok(serde_json::to_value(blast_radius_view(
                    self.prism.as_ref(),
                    self.host.session.as_ref(),
                    &id,
                ))?)
            }
            "validationRecipe" => {
                let args: SymbolTargetArgs = serde_json::from_value(args)?;
                let id = self.resolve_target_id(args.id, args.lineage_id)?;
                Ok(serde_json::to_value(validation_recipe_view_with(
                    self.prism.as_ref(),
                    self.host.session.as_ref(),
                    &id,
                ))?)
            }
            "readContext" => {
                let args: SymbolTargetArgs = serde_json::from_value(args)?;
                let id = self.resolve_target_id(args.id, args.lineage_id)?;
                Ok(serde_json::to_value(self.read_context(&id)?)?)
            }
            "editContext" => {
                let args: SymbolTargetArgs = serde_json::from_value(args)?;
                let id = self.resolve_target_id(args.id, args.lineage_id)?;
                Ok(serde_json::to_value(self.edit_context(&id)?)?)
            }
            "validationContext" => {
                let args: SymbolTargetArgs = serde_json::from_value(args)?;
                let id = self.resolve_target_id(args.id, args.lineage_id)?;
                Ok(serde_json::to_value(self.validation_context(&id)?)?)
            }
            "recentChangeContext" => {
                let args: SymbolTargetArgs = serde_json::from_value(args)?;
                let id = self.resolve_target_id(args.id, args.lineage_id)?;
                Ok(serde_json::to_value(self.recent_change_context(&id)?)?)
            }
            "discoveryBundle" => {
                let args: SymbolTargetArgs = serde_json::from_value(args)?;
                let id = self.resolve_target_id(args.id, args.lineage_id)?;
                Ok(serde_json::to_value(self.discovery_bundle(&id)?)?)
            }
            "nextReads" => {
                let args: DiscoveryTargetArgs = serde_json::from_value(args)?;
                let id = self.resolve_target_id(args.id, args.lineage_id)?;
                let applied = args
                    .limit
                    .unwrap_or(INSIGHT_LIMIT)
                    .min(self.host.session.limits().max_result_nodes);
                Ok(serde_json::to_value(next_reads(
                    self.prism.as_ref(),
                    &id,
                    applied,
                )?)?)
            }
            "whereUsed" => {
                let args: WhereUsedArgs = serde_json::from_value(args)?;
                let id = self.resolve_target_id(args.id, args.lineage_id)?;
                let applied = args
                    .limit
                    .unwrap_or(INSIGHT_LIMIT)
                    .min(self.host.session.limits().max_result_nodes);
                Ok(serde_json::to_value(where_used(
                    self.prism.as_ref(),
                    self.host.session.as_ref(),
                    &id,
                    args.mode.as_deref(),
                    applied,
                )?)?)
            }
            "entrypointsFor" => {
                let args: DiscoveryTargetArgs = serde_json::from_value(args)?;
                let id = self.resolve_target_id(args.id, args.lineage_id)?;
                let applied = args
                    .limit
                    .unwrap_or(INSIGHT_LIMIT)
                    .min(self.host.session.limits().max_result_nodes);
                Ok(serde_json::to_value(entrypoints_for(
                    self.prism.as_ref(),
                    self.host.session.as_ref(),
                    &id,
                    applied,
                )?)?)
            }
            "specFor" => {
                let args: SymbolTargetArgs = serde_json::from_value(args)?;
                let id = self.resolve_target_id(args.id, args.lineage_id)?;
                Ok(serde_json::to_value(symbol_views_for_ids(
                    self.prism.as_ref(),
                    self.prism.spec_for(&id),
                )?)?)
            }
            "implementationFor" => {
                let args: ImplementationTargetArgs = serde_json::from_value(args)?;
                let id = self.resolve_target_id(args.id, args.lineage_id)?;
                if args.mode.as_deref() == Some("owners") {
                    let limit = self
                        .host
                        .session
                        .limits()
                        .max_result_nodes
                        .min(INSIGHT_LIMIT);
                    Ok(serde_json::to_value(owner_symbol_views_for_target(
                        self.prism.as_ref(),
                        &id,
                        args.owner_kind.as_deref(),
                        limit,
                    )?)?)
                } else {
                    Ok(serde_json::to_value(symbol_views_for_ids(
                        self.prism.as_ref(),
                        self.prism.implementation_for(&id),
                    )?)?)
                }
            }
            "driftCandidates" => {
                let args: LimitArgs = serde_json::from_value(args)?;
                Ok(serde_json::to_value(
                    self.prism
                        .drift_candidates(args.limit.unwrap_or(10))
                        .into_iter()
                        .map(drift_candidate_view)
                        .collect::<Vec<_>>(),
                )?)
            }
            "specCluster" => {
                let args: SymbolTargetArgs = serde_json::from_value(args)?;
                let id = self.resolve_target_id(args.id, args.lineage_id)?;
                Ok(serde_json::to_value(self.spec_cluster(&id)?)?)
            }
            "explainDrift" => {
                let args: SymbolTargetArgs = serde_json::from_value(args)?;
                let id = self.resolve_target_id(args.id, args.lineage_id)?;
                Ok(serde_json::to_value(self.explain_drift(&id)?)?)
            }
            "owners" => {
                let args: OwnerLookupArgs = serde_json::from_value(args)?;
                let id = self.resolve_target_id(args.id, args.lineage_id)?;
                let applied = args
                    .limit
                    .unwrap_or(INSIGHT_LIMIT)
                    .min(self.host.session.limits().max_result_nodes);
                Ok(serde_json::to_value(owner_views_for_target(
                    self.prism.as_ref(),
                    &id,
                    args.kind.as_deref(),
                    applied,
                )?)?)
            }
            "resumeTask" => {
                let args: TaskTargetArgs = serde_json::from_value(args)?;
                serde_json::to_value(self.prism.resume_task(&args.task_id)).map_err(Into::into)
            }
            "taskJournal" => {
                let args: TaskJournalArgs = serde_json::from_value(args)?;
                Ok(serde_json::to_value(self.task_journal(args)?)?)
            }
            "memoryRecall" => {
                let args: MemoryRecallArgs = serde_json::from_value(args)?;
                Ok(serde_json::to_value(self.memory_recall(args)?)?)
            }
            "memoryOutcomes" => {
                let args: MemoryOutcomeArgs = serde_json::from_value(args)?;
                Ok(serde_json::to_value(self.memory_outcomes(args)?)?)
            }
            "curatorJobs" => {
                let args: CuratorJobsArgs = serde_json::from_value(args)?;
                Ok(serde_json::to_value(self.host.curator_jobs(args)?)?)
            }
            "curatorJob" => {
                let args: CuratorJobArgs = serde_json::from_value(args)?;
                let job = self.host.curator_job(&args.job_id)?;
                if job.is_none() {
                    self.push_diagnostic(
                        "anchor_unresolved",
                        format!("No curator job matched `{}`.", args.job_id),
                        Some(json!({ "jobId": args.job_id })),
                    );
                }
                Ok(serde_json::to_value(job)?)
            }
            "diagnostics" => Ok(serde_json::to_value(self.diagnostics())?),
            other => {
                self.push_diagnostic(
                    "unknown_method",
                    format!("Unknown Prism host operation `{other}`."),
                    Some(json!({ "operation": other })),
                );
                Err(anyhow!("unsupported host operation `{other}`"))
            }
        };
        self.query_run.record_phase(
            operation,
            &phase_args,
            phase_started.elapsed(),
            result.is_ok(),
            result.as_ref().err().map(ToString::to_string),
        );
        result
    }

    fn ensure_operation_enabled(&self, operation: &str) -> Result<()> {
        if let Some(group) = self.host.features.disabled_query_group(operation) {
            let message = match group {
                "internal_developer" => {
                    "internal developer queries are disabled unless the PRISM MCP server is started with `--internal-developer`"
                }
                _ => {
                    return Err(anyhow!(
                        "coordination {group} queries are disabled by the PRISM MCP server feature flags"
                    ));
                }
            };
            return Err(anyhow!(message));
        }
        Ok(())
    }

    pub(crate) fn best_symbol(&self, query: &str) -> Result<Option<SymbolView>> {
        let mut matches = self.symbols(query)?;
        let current_task_id = self.host.session.current_task().map(|task| task.0);
        let ambiguity = rank_search_results(
            self.prism.as_ref(),
            &mut matches,
            &SearchAmbiguityContext {
                query,
                strategy: "direct",
                owner_kind: None,
                path: None,
                module: None,
                task_id: current_task_id.as_deref(),
                task_scope_mode: TaskScopeMode::Prefer,
            },
            false,
        )?;
        if matches.is_empty() {
            self.push_diagnostic(
                "anchor_unresolved",
                format!(
                    "No symbol matched `{query}`. Next action: run `prism.search(...)` to inspect candidates or switch to behavioral owner search."
                ),
                Some(json!({
                    "query": query,
                    "nextAction": "Run prism.search(query, { limit: 5 }) or prism.search(query, { strategy: \"behavioral\", ownerKind: \"read\", limit: 5 }).",
                    "suggestedQueries": search_queries(query),
                })),
            );
            return Ok(None);
        }
        if matches.len() > 1 {
            let next_action = if current_task_id.is_some() {
                "Run prism.search(query, { module: ..., path: ..., kind: ..., taskId: ..., limit: 5 }) and then call prism.focusedBlock(...) or prism.readContext(...) on the intended result."
            } else {
                "Run prism.search(query, { module: ..., path: ..., kind: ..., limit: 5 }) and then call prism.focusedBlock(...) or prism.readContext(...) on the intended result."
            };
            self.push_diagnostic(
                "ambiguous_symbol",
                format!(
                    "`{query}` matched {} symbols; returning the highest-ranked candidate. Next action: narrow with `path`, `module`, `kind`, or task context, or inspect `prism.focusedBlock(...)` on the intended target.",
                    matches.len()
                ),
                ambiguity
                    .as_ref()
                    .map(|ambiguity| ambiguity_diagnostic_data(ambiguity, next_action))
                    .or_else(|| {
                        Some(json!({
                            "query": query,
                            "matches": matches
                                .iter()
                                .map(|symbol| symbol.id.path.to_string())
                                .collect::<Vec<_>>(),
                            "nextAction": next_action,
                            "suggestedQueries": search_queries(query),
                        }))
                    }),
            );
        }
        Ok(matches.into_iter().next())
    }

    fn tools(&self) -> Vec<ToolCatalogEntryView> {
        tool_catalog_views()
    }

    fn tool(&self, name: &str) -> Result<Option<ToolSchemaView>> {
        Ok(tool_schema_view(name))
    }

    pub(crate) fn search(&self, args: SearchArgs) -> Result<Vec<SymbolView>> {
        let _include_inferred = args.include_inferred.unwrap_or(true);
        let kind = args.kind.as_deref().map(parse_node_kind).transpose()?;
        let requested = args.limit.unwrap_or(DEFAULT_SEARCH_LIMIT);
        let limits = self.host.session.limits();
        let applied = requested.min(limits.max_result_nodes);
        let path_mode = parse_path_mode(args.path_mode.as_deref())?;
        let explicit_task_id = args.task_id.clone();
        let current_task_id = self.host.session.current_task().map(|task| task.0);
        let effective_task_id = explicit_task_id.as_deref().or(current_task_id.as_deref());
        let exact_structured =
            args.structured_path.is_some() || args.top_level_only.unwrap_or(false);
        let needs_post_filter = path_mode == SearchPathMode::Exact || exact_structured;
        let backend_limit = if needs_post_filter {
            limits.max_result_nodes.saturating_add(1)
        } else {
            applied.saturating_add(1)
        };

        if requested > limits.max_result_nodes {
            self.push_diagnostic(
                "result_truncated",
                format!(
                    "Search limit was capped at {} instead of {requested}. Next action: narrow the query with `path`, `module`, `kind`, or `taskId` before raising the limit.",
                    limits.max_result_nodes
                ),
                Some(json!({
                    "requested": requested,
                    "applied": applied,
                    "nextAction": "Use prism.search(query, { path: ..., module: ..., kind: ..., taskId: ..., limit: ... }) to narrow the result set.",
                    "suggestedQueries": search_queries(&args.query),
                })),
            );
        }

        let strategy = args.strategy.as_deref().unwrap_or("direct");
        let mut results = if strategy == "behavioral" {
            owner_symbol_views_for_query(
                self.prism.as_ref(),
                &args.query,
                args.owner_kind.as_deref(),
                kind,
                args.path.as_deref(),
                backend_limit,
            )?
        } else {
            self.prism
                .search(&args.query, backend_limit, kind, args.path.as_deref())
                .iter()
                .map(|symbol| symbol_view(self.prism.as_ref(), symbol))
                .collect::<Result<Vec<_>>>()?
        };
        apply_search_post_filters(
            &mut results,
            self.host
                .workspace
                .as_ref()
                .map(|workspace| workspace.root()),
            args.path.as_deref(),
            path_mode,
            args.structured_path.as_deref(),
            args.top_level_only.unwrap_or(false),
        );
        apply_module_filter(&mut results, args.module.as_deref());

        let ambiguity = rank_search_results(
            self.prism.as_ref(),
            &mut results,
            &SearchAmbiguityContext {
                query: &args.query,
                strategy,
                owner_kind: args.owner_kind.as_deref(),
                path: args.path.as_deref(),
                module: args.module.as_deref(),
                task_id: effective_task_id,
                task_scope_mode: if explicit_task_id.is_some() {
                    TaskScopeMode::Filter
                } else {
                    TaskScopeMode::Prefer
                },
            },
            true,
        )?;

        if let Some(ambiguity) = ambiguity.as_ref() {
            self.push_diagnostic(
                "ambiguous_search",
                format!(
                    "Search for `{}` returned multiple strong candidates. Next action: narrow with `path`, `module`, `ownerKind`, or `taskId`, or inspect `prism.focusedBlock(...)` on one candidate.",
                    args.query
                ),
                Some(ambiguity_diagnostic_data(
                    ambiguity,
                    "Use prism.search(query, { path: ..., module: ..., ownerKind: ..., taskId: ..., limit: ... }) to narrow the candidates, or run prism.focusedBlock(...) on the intended result.",
                )),
            );
        }

        if results.len() > applied {
            results.truncate(applied);
            self.push_diagnostic(
                "result_truncated",
                format!(
                    "Search results for `{}` were truncated at {} entries. Next action: narrow with `path`, `module`, `kind`, or `taskId`, then open `prism.focusedBlock(...)` or `prism.readContext(...)` on the top candidate.",
                    args.query, applied
                ),
                Some(json!({
                    "query": args.query,
                    "applied": applied,
                    "strategy": strategy,
                    "nextAction": "Use a narrower prism.search(...) call with path/module/task filters and then inspect prism.focusedBlock(...) or prism.readContext(...) on one candidate.",
                    "suggestedQueries": search_queries(&args.query),
                })),
            );
        }

        Ok(results)
    }

    pub(crate) fn search_text(&self, args: SearchTextArgs) -> Result<Vec<TextSearchMatchView>> {
        let outcome = search_text(
            self.host(),
            args,
            self.host.session.limits().max_result_nodes,
        )?;
        if outcome.requested > outcome.applied {
            self.push_diagnostic(
                "result_truncated",
                format!(
                    "Text search limit was capped at {} instead of {}. Next action: narrow with `path` or `glob` before raising the limit.",
                    outcome.applied, outcome.requested
                ),
                Some(json!({
                    "requested": outcome.requested,
                    "applied": outcome.applied,
                    "nextAction": "Use prism.searchText(query, { path: ..., glob: ..., limit: ... }) to narrow the result set.",
                })),
            );
        }
        if outcome.limit_hit {
            self.push_diagnostic(
                "result_truncated",
                format!(
                    "Text search results were truncated at {} entries. Next action: narrow with `path` or `glob`, then inspect one match with `prism.file(path).around(...)`.",
                    outcome.applied
                ),
                Some(json!({
                    "applied": outcome.applied,
                    "nextAction": "Use prism.searchText(query, { path: ..., glob: ..., limit: ... }) and then inspect one result with prism.file(path).around(...).",
                })),
            );
        }
        Ok(outcome.results)
    }

    pub(crate) fn changed_files(&self, args: ChangedFilesArgs) -> Result<Vec<ChangedFileView>> {
        let requested = args.limit.unwrap_or(DEFAULT_SEARCH_LIMIT);
        let applied = requested.min(self.host.session.limits().max_result_nodes);
        let mut results = changed_files(
            self.prism.as_ref(),
            args.task_id.as_ref(),
            args.since,
            args.path.as_deref(),
            applied.saturating_add(1),
        )?;
        if requested > applied {
            self.push_diagnostic(
                "result_truncated",
                format!(
                    "Changed-file limit was capped at {} instead of {requested}. Next action: narrow with `path` or `taskId` before raising the limit.",
                    applied
                ),
                Some(json!({
                    "requested": requested,
                    "applied": applied,
                    "nextAction": "Use prism.changedFiles({ path: ..., taskId: ..., limit: ... }) to narrow the result set.",
                })),
            );
        }
        if results.len() > applied {
            results.truncate(applied);
            self.push_diagnostic(
                "result_truncated",
                format!(
                    "Changed files were truncated at {} entries. Next action: narrow with `path` or inspect one result with `prism.changedSymbols(...)`.",
                    applied
                ),
                Some(json!({
                    "applied": applied,
                    "nextAction": "Use prism.changedFiles({ path: ..., taskId: ..., limit: ... }) or prism.changedSymbols(path, ...) to inspect one file.",
                })),
            );
        }
        Ok(results)
    }

    pub(crate) fn changed_symbols(
        &self,
        args: ChangedSymbolsArgs,
    ) -> Result<Vec<ChangedSymbolView>> {
        let requested = args.limit.unwrap_or(DEFAULT_SEARCH_LIMIT);
        let applied = requested.min(self.host.session.limits().max_result_nodes);
        let mut results = changed_symbols(
            self.prism.as_ref(),
            &args.path,
            args.task_id.as_ref(),
            args.since,
            applied.saturating_add(1),
        )?;
        if requested > applied {
            self.push_diagnostic(
                "result_truncated",
                format!(
                    "Changed-symbol limit was capped at {} instead of {requested}. Next action: narrow with `taskId` or a more specific path before raising the limit.",
                    applied
                ),
                Some(json!({
                    "requested": requested,
                    "applied": applied,
                    "nextAction": "Use prism.changedSymbols(path, { taskId: ..., limit: ... }) with a specific file path to narrow the result set.",
                })),
            );
        }
        if results.len() > applied {
            results.truncate(applied);
            self.push_diagnostic(
                "result_truncated",
                format!(
                    "Changed symbols for `{}` were truncated at {} entries. Next action: narrow with `taskId` or inspect one patch event with `prism.recentPatches(...)`.",
                    args.path, applied
                ),
                Some(json!({
                    "path": args.path,
                    "applied": applied,
                    "nextAction": "Use prism.changedSymbols(path, { taskId: ..., limit: ... }) or prism.recentPatches({ path: ..., limit: ... }) to narrow the result set.",
                })),
            );
        }
        Ok(results)
    }

    pub(crate) fn recent_patches(&self, args: RecentPatchesArgs) -> Result<Vec<PatchEventView>> {
        let requested = args.limit.unwrap_or(DEFAULT_SEARCH_LIMIT);
        let applied = requested.min(self.host.session.limits().max_result_nodes);
        let target = args.target.map(convert_node_id).transpose()?;
        let mut results = recent_patches(
            self.prism.as_ref(),
            target.as_ref(),
            args.task_id.as_ref(),
            args.since,
            args.path.as_deref(),
            applied.saturating_add(1),
        )?;
        if requested > applied {
            self.push_diagnostic(
                "result_truncated",
                format!(
                    "Recent-patch limit was capped at {} instead of {requested}. Next action: narrow with `target`, `path`, or `taskId` before raising the limit.",
                    applied
                ),
                Some(json!({
                    "requested": requested,
                    "applied": applied,
                    "nextAction": "Use prism.recentPatches({ target: ..., path: ..., taskId: ..., limit: ... }) to narrow the result set.",
                })),
            );
        }
        if results.len() > applied {
            results.truncate(applied);
            self.push_diagnostic(
                "result_truncated",
                format!(
                    "Recent patches were truncated at {} entries. Next action: narrow with `target`, `path`, or `taskId`.",
                    applied
                ),
                Some(json!({
                    "applied": applied,
                    "nextAction": "Use prism.recentPatches({ target: ..., path: ..., taskId: ..., limit: ... }) to narrow the result set.",
                })),
            );
        }
        Ok(results)
    }

    pub(crate) fn diff_for(&self, args: DiffForArgs) -> Result<Vec<DiffHunkView>> {
        let requested = args.limit.unwrap_or(DEFAULT_SEARCH_LIMIT);
        let applied = requested.min(self.host.session.limits().max_result_nodes);
        let requested_id = args.id.map(convert_node_id).transpose()?;
        let requested_lineage = args.lineage_id.map(LineageId::new);

        let target = match (requested_id.as_ref(), requested_lineage.as_ref()) {
            (Some(id), None) => {
                if symbol_for(self.prism.as_ref(), id).is_ok() {
                    Some(id.clone())
                } else {
                    return Err(anyhow!("unknown symbol `{}`", id.path));
                }
            }
            (Some(id), Some(lineage)) => {
                if symbol_for(self.prism.as_ref(), id).is_ok() {
                    if self.prism.lineage_of(id).as_ref() != Some(lineage) {
                        self.push_diagnostic(
                            "target_lineage_mismatch",
                            format!(
                                "Target `{}` resolved directly, but its current lineage does not match `{}`.",
                                id.path, lineage.0
                            ),
                            Some(json!({
                                "id": id,
                                "lineageId": lineage.0,
                            })),
                        );
                    }
                    Some(id.clone())
                } else {
                    let resolved = self.resolve_lineage_target(lineage, Some(id))?;
                    if &resolved != id {
                        self.push_diagnostic(
                            "target_remapped_via_lineage",
                            format!(
                                "Resolved current target `{}` from stable lineage `{}`.",
                                resolved.path, lineage.0
                            ),
                            Some(json!({
                                "requestedId": id,
                                "resolvedId": resolved.clone(),
                                "lineageId": lineage.0,
                            })),
                        );
                    }
                    Some(resolved)
                }
            }
            (None, Some(lineage)) => self
                .prism
                .current_nodes_for_lineage(lineage)
                .into_iter()
                .next(),
            (None, None) => {
                return Err(anyhow!("target must include `id` or `lineageId`"));
            }
        };

        let mut results = diff_for(
            self.prism.as_ref(),
            target.as_ref(),
            requested_lineage.as_ref(),
            args.task_id.as_ref(),
            args.since,
            applied.saturating_add(1),
        )?;
        if requested > applied {
            self.push_diagnostic(
                "result_truncated",
                format!(
                    "Target diff hunks were truncated at {} entries. Next action: narrow with `since` or `taskId`.",
                    applied
                ),
                Some(json!({
                    "requested": requested,
                    "applied": applied,
                    "nextAction": "Use prism.diffFor(target, { since: ..., taskId: ..., limit: ... }) to narrow the result set.",
                })),
            );
        }
        if results.len() > applied {
            results.truncate(applied);
            self.push_diagnostic(
                "result_truncated",
                format!(
                    "Target diff hunks were truncated at {} entries. Next action: narrow with `since` or `taskId`.",
                    applied
                ),
                Some(json!({
                    "applied": applied,
                    "nextAction": "Use prism.diffFor(target, { since: ..., taskId: ..., limit: ... }) to narrow the result set.",
                })),
            );
        }
        Ok(results)
    }

    pub(crate) fn runtime_status(&self) -> Result<RuntimeStatusView> {
        runtime_status(&self.host)
    }

    pub(crate) fn runtime_logs(&self, args: RuntimeLogArgs) -> Result<Vec<RuntimeLogEventView>> {
        let requested = args.limit.unwrap_or(DEFAULT_SEARCH_LIMIT);
        let applied = requested.min(self.host.session.limits().max_result_nodes);
        let mut results = runtime_logs(
            &self.host,
            RuntimeLogArgs {
                limit: Some(applied),
                ..args
            },
        )?;
        if requested > applied {
            self.push_diagnostic(
                "result_truncated",
                format!(
                    "Runtime-log limit was capped at {} instead of {requested}. Next action: narrow with `level`, `target`, or `contains` before raising the limit.",
                    applied
                ),
                Some(json!({
                    "requested": requested,
                    "applied": applied,
                    "nextAction": "Use prism.runtimeLogs({ level: ..., target: ..., contains: ..., limit: ... }) to narrow the result set.",
                })),
            );
        }
        if results.len() > applied {
            results.truncate(applied);
        }
        Ok(results)
    }

    pub(crate) fn runtime_timeline(
        &self,
        args: RuntimeTimelineArgs,
    ) -> Result<Vec<RuntimeLogEventView>> {
        let requested = args.limit.unwrap_or(DEFAULT_SEARCH_LIMIT);
        let applied = requested.min(self.host.session.limits().max_result_nodes);
        let mut results = runtime_timeline(
            &self.host,
            RuntimeTimelineArgs {
                limit: Some(applied.saturating_add(1)),
                ..args
            },
        )?;
        if requested > applied {
            self.push_diagnostic(
                "result_truncated",
                format!(
                    "Runtime-timeline limit was capped at {} instead of {requested}. Next action: narrow with `contains` before raising the limit.",
                    applied
                ),
                Some(json!({
                    "requested": requested,
                    "applied": applied,
                    "nextAction": "Use prism.runtimeTimeline({ contains: ..., limit: ... }) to narrow the result set.",
                })),
            );
        }
        if results.len() > applied {
            results.truncate(applied);
            self.push_diagnostic(
                "result_truncated",
                format!(
                    "Runtime timeline was truncated at {} entries. Next action: narrow with `contains` or inspect broader logs with `prism.runtimeLogs(...)`.",
                    applied
                ),
                Some(json!({
                    "applied": applied,
                    "nextAction": "Use prism.runtimeTimeline({ contains: ..., limit: ... }) or prism.runtimeLogs({ target: ..., contains: ..., limit: ... }) to narrow the result set.",
                })),
            );
        }
        Ok(results)
    }

    pub(crate) fn task_changes(&self, args: TaskChangesArgs) -> Result<Vec<PatchEventView>> {
        let requested = args.limit.unwrap_or(DEFAULT_SEARCH_LIMIT);
        let applied = requested.min(self.host.session.limits().max_result_nodes);
        let mut results = recent_patches(
            self.prism.as_ref(),
            None,
            Some(&args.task_id),
            args.since,
            args.path.as_deref(),
            applied.saturating_add(1),
        )?;
        if requested > applied {
            self.push_diagnostic(
                "result_truncated",
                format!(
                    "Task-change limit was capped at {} instead of {requested}. Next action: narrow with `path` before raising the limit.",
                    applied
                ),
                Some(json!({
                    "requested": requested,
                    "applied": applied,
                    "taskId": args.task_id.0,
                    "nextAction": "Use prism.taskChanges(taskId, { path: ..., limit: ... }) to narrow the result set.",
                })),
            );
        }
        if results.len() > applied {
            results.truncate(applied);
            self.push_diagnostic(
                "result_truncated",
                format!(
                    "Task changes for `{}` were truncated at {} entries. Next action: narrow with `path`.",
                    args.task_id.0, applied
                ),
                Some(json!({
                    "taskId": args.task_id.0,
                    "applied": applied,
                    "nextAction": "Use prism.taskChanges(taskId, { path: ..., limit: ... }) to narrow the result set.",
                })),
            );
        }
        Ok(results)
    }

    fn host(&self) -> &QueryHost {
        &self.host
    }

    pub(crate) fn entrypoints(&self) -> Result<Vec<SymbolView>> {
        let limits = self.host.session.limits();
        let mut results = self.symbols_from(self.prism.entrypoints())?;
        if results.len() > limits.max_result_nodes {
            results.truncate(limits.max_result_nodes);
            self.push_diagnostic(
                "result_truncated",
                format!(
                    "Entrypoints were truncated at {} entries.",
                    limits.max_result_nodes
                ),
                Some(json!({
                    "applied": limits.max_result_nodes,
                })),
            );
        }
        Ok(results)
    }

    fn call_graph(&self, args: CallGraphArgs) -> Result<SubgraphView> {
        let limits = self.host.session.limits();
        let id = self.resolve_target_id(args.id, args.lineage_id)?;
        let requested = args.depth.unwrap_or(DEFAULT_CALL_GRAPH_DEPTH);
        let applied = requested.min(limits.max_call_graph_depth);
        if requested > limits.max_call_graph_depth {
            self.push_diagnostic(
                "depth_limited",
                format!(
                    "Call-graph depth was capped at {} instead of {requested}.",
                    limits.max_call_graph_depth
                ),
                Some(json!({
                    "requested": requested,
                    "applied": applied,
                })),
            );
        }
        let mut graph = symbol_for(self.prism.as_ref(), &id)?.call_graph(applied);
        let mut queue = vec![(id.clone(), 0usize)];
        let mut seen = std::collections::HashSet::from([id.clone()]);

        while let Some((current, depth)) = queue.pop() {
            if depth >= applied {
                continue;
            }
            for record in self
                .host
                .session
                .inferred_edges
                .edges_from(&current, Some(EdgeKind::Calls))
            {
                graph.edges.push(record.edge.clone());
                graph.nodes.push(record.edge.target.clone());
                if seen.insert(record.edge.target.clone()) {
                    queue.push((record.edge.target, depth + 1));
                }
            }
        }

        graph.nodes = merge_node_ids(graph.nodes, std::iter::empty());
        graph.edges.sort_by(|left, right| {
            left.source
                .path
                .cmp(&right.source.path)
                .then_with(|| left.target.path.cmp(&right.target.path))
                .then_with(|| edge_kind_label(left.kind).cmp(edge_kind_label(right.kind)))
        });
        graph.edges.dedup_by(|left, right| {
            left.kind == right.kind && left.source == right.source && left.target == right.target
        });
        if graph.nodes.len() > limits.max_result_nodes {
            let keep = graph
                .nodes
                .iter()
                .take(limits.max_result_nodes)
                .cloned()
                .collect::<std::collections::HashSet<_>>();
            graph.nodes.truncate(limits.max_result_nodes);
            graph
                .edges
                .retain(|edge| keep.contains(&edge.source) && keep.contains(&edge.target));
            graph.truncated = true;
            self.push_diagnostic(
                "result_truncated",
                format!(
                    "Call graph for `{}` was truncated at {} nodes.",
                    id.path, limits.max_result_nodes
                ),
                Some(json!({
                    "query": id.path,
                    "applied": limits.max_result_nodes,
                })),
            );
        }
        graph.max_depth_reached = Some(applied);
        Ok(SubgraphView {
            nodes: symbol_views_for_ids(self.prism.as_ref(), graph.nodes)?,
            edges: graph.edges.into_iter().map(edge_view).collect(),
            truncated: graph.truncated,
            max_depth_reached: graph.max_depth_reached,
        })
    }

    fn memory_recall(&self, args: MemoryRecallArgs) -> Result<Vec<ScoredMemoryView>> {
        let requested = args.limit.unwrap_or(5);
        let limits = self.host.session.limits();
        let applied = requested.min(limits.max_result_nodes);
        if requested > limits.max_result_nodes {
            self.push_diagnostic(
                "result_truncated",
                format!(
                    "Memory recall limit was capped at {} instead of {requested}.",
                    limits.max_result_nodes
                ),
                Some(json!({
                    "requested": requested,
                    "applied": applied,
                })),
            );
        }

        let mut focus = Vec::new();
        if let Some(ids) = args.focus {
            for id in ids {
                focus.push(AnchorRef::Node(convert_node_id(id)?));
            }
        }
        let focus = self.prism.anchors_for(&focus);
        let kinds = args
            .kinds
            .map(|kinds| {
                kinds
                    .into_iter()
                    .map(|kind| parse_memory_kind(&kind))
                    .collect::<Result<Vec<_>>>()
            })
            .transpose()?;
        let results = self
            .host
            .session
            .notes
            .recall(&RecallQuery {
                focus,
                text: args.text,
                limit: applied,
                kinds,
                since: args.since,
            })?
            .into_iter()
            .map(scored_memory_view)
            .collect();
        Ok(results)
    }

    fn task_journal(&self, args: TaskJournalArgs) -> Result<prism_js::TaskJournalView> {
        let event_requested = args.event_limit.unwrap_or(DEFAULT_TASK_JOURNAL_EVENT_LIMIT);
        let memory_requested = args
            .memory_limit
            .unwrap_or(DEFAULT_TASK_JOURNAL_MEMORY_LIMIT);
        let limits = self.host.session.limits();
        let event_limit = event_requested.min(limits.max_result_nodes);
        let memory_limit = memory_requested.min(limits.max_result_nodes);

        if event_requested > limits.max_result_nodes {
            self.push_diagnostic(
                "result_truncated",
                format!(
                    "Task journal event limit was capped at {} instead of {event_requested}.",
                    limits.max_result_nodes
                ),
                Some(json!({
                    "requested": event_requested,
                    "applied": event_limit,
                    "field": "eventLimit",
                })),
            );
        }
        if memory_requested > limits.max_result_nodes {
            self.push_diagnostic(
                "result_truncated",
                format!(
                    "Task journal memory limit was capped at {} instead of {memory_requested}.",
                    limits.max_result_nodes
                ),
                Some(json!({
                    "requested": memory_requested,
                    "applied": memory_limit,
                    "field": "memoryLimit",
                })),
            );
        }

        let journal = task_journal_view(
            self.host.session.as_ref(),
            self.prism.as_ref(),
            &args.task_id,
            None,
            event_limit,
            memory_limit,
        )?;
        for diagnostic in &journal.diagnostics {
            self.push_diagnostic(
                &diagnostic.code,
                diagnostic.message.clone(),
                diagnostic.data.clone(),
            );
        }
        Ok(journal)
    }

    fn spec_cluster(&self, id: &NodeId) -> Result<prism_js::SpecImplementationClusterView> {
        spec_cluster_view(self.prism.as_ref(), id)
    }

    fn explain_drift(&self, id: &NodeId) -> Result<prism_js::SpecDriftExplanationView> {
        spec_drift_explanation_view(self.prism.as_ref(), id)
    }

    fn read_context(&self, id: &NodeId) -> Result<ReadContextView> {
        let mut cache = self
            .semantic_context_cache
            .lock()
            .expect("semantic context cache lock poisoned");
        read_context_view_cached(
            self.prism.as_ref(),
            self.host.session.as_ref(),
            &mut cache,
            id,
        )
    }

    fn edit_context(&self, id: &NodeId) -> Result<EditContextView> {
        let mut cache = self
            .semantic_context_cache
            .lock()
            .expect("semantic context cache lock poisoned");
        crate::edit_context_view_cached(
            self.prism.as_ref(),
            self.host.session.as_ref(),
            &mut cache,
            id,
        )
    }

    fn validation_context(&self, id: &NodeId) -> Result<ValidationContextView> {
        let mut cache = self
            .semantic_context_cache
            .lock()
            .expect("semantic context cache lock poisoned");
        validation_context_view_cached(
            self.prism.as_ref(),
            self.host.session.as_ref(),
            &mut cache,
            id,
        )
    }

    fn recent_change_context(&self, id: &NodeId) -> Result<RecentChangeContextView> {
        let mut cache = self
            .semantic_context_cache
            .lock()
            .expect("semantic context cache lock poisoned");
        recent_change_context_view_cached(
            self.prism.as_ref(),
            self.host.session.as_ref(),
            &mut cache,
            id,
        )
    }

    fn discovery_bundle(&self, id: &NodeId) -> Result<DiscoveryBundleView> {
        let mut cache = self
            .semantic_context_cache
            .lock()
            .expect("semantic context cache lock poisoned");
        crate::discovery_bundle_view_cached(
            self.prism.as_ref(),
            self.host.session.as_ref(),
            &mut cache,
            id,
        )
    }

    fn memory_outcomes(&self, args: MemoryOutcomeArgs) -> Result<Vec<prism_memory::OutcomeEvent>> {
        let requested = args.limit.unwrap_or(10);
        let limits = self.host.session.limits();
        let applied = requested.min(limits.max_result_nodes);
        if requested > limits.max_result_nodes {
            self.push_diagnostic(
                "result_truncated",
                format!(
                    "Memory outcome query limit was capped at {} instead of {requested}.",
                    limits.max_result_nodes
                ),
                Some(json!({
                    "requested": requested,
                    "applied": applied,
                })),
            );
        }

        let mut focus = Vec::new();
        if let Some(ids) = args.focus {
            for id in ids {
                focus.push(AnchorRef::Node(convert_node_id(id)?));
            }
        }

        let kinds = args
            .kinds
            .map(|kinds| {
                kinds
                    .into_iter()
                    .map(|kind| parse_outcome_kind(&kind))
                    .collect::<Result<Vec<_>>>()
            })
            .transpose()?;
        let result = args
            .result
            .as_deref()
            .map(parse_outcome_result)
            .transpose()?;
        let actor = args.actor.as_deref().map(parse_event_actor).transpose()?;

        Ok(self.prism.query_outcomes(&OutcomeRecallQuery {
            anchors: focus,
            task: args.task_id,
            kinds,
            result,
            actor,
            since: args.since,
            limit: applied,
        }))
    }

    fn source_excerpt(&self, args: SourceExcerptArgs) -> Result<Option<SourceExcerptView>> {
        let id = self.resolve_target_id(args.id, args.lineage_id)?;
        let symbol = symbol_for(self.prism.as_ref(), &id)?;
        let defaults = SourceExcerptOptions::default();
        Ok(source_excerpt_for_symbol(
            &symbol,
            SourceExcerptOptions {
                context_lines: args.context_lines.unwrap_or(defaults.context_lines),
                max_lines: args.max_lines.unwrap_or(defaults.max_lines),
                max_chars: args.max_chars.unwrap_or(defaults.max_chars),
            },
        ))
    }

    fn edit_slice(&self, args: EditSliceArgs) -> Result<Option<SourceSliceView>> {
        let id = self.resolve_target_id(args.id, args.lineage_id)?;
        let symbol = symbol_for(self.prism.as_ref(), &id)?;
        let defaults = EditSliceOptions::default();
        Ok(edit_slice_for_symbol(
            &symbol,
            EditSliceOptions {
                before_lines: args.before_lines.unwrap_or(defaults.before_lines),
                after_lines: args.after_lines.unwrap_or(defaults.after_lines),
                max_lines: args.max_lines.unwrap_or(defaults.max_lines),
                max_chars: args.max_chars.unwrap_or(defaults.max_chars),
            },
        ))
    }

    fn focused_block(&self, args: EditSliceArgs) -> Result<Option<FocusedBlockView>> {
        let id = self.resolve_target_id(args.id, args.lineage_id)?;
        let symbol = symbol_for(self.prism.as_ref(), &id)?;
        let defaults = EditSliceOptions::default();
        Ok(Some(focused_block_for_symbol(
            self.prism.as_ref(),
            &symbol,
            EditSliceOptions {
                before_lines: args.before_lines.unwrap_or(defaults.before_lines),
                after_lines: args.after_lines.unwrap_or(defaults.after_lines),
                max_lines: args.max_lines.unwrap_or(defaults.max_lines),
                max_chars: args.max_chars.unwrap_or(defaults.max_chars),
            },
        )?))
    }

    fn symbols(&self, query: &str) -> Result<Vec<SymbolView>> {
        self.symbols_from(self.prism.symbol(query))
    }

    fn resolve_target_id(
        &self,
        id: Option<NodeIdInput>,
        lineage_id: Option<String>,
    ) -> Result<NodeId> {
        let requested_id = id.map(convert_node_id).transpose()?;
        let requested_lineage = lineage_id.map(LineageId::new);

        if let Some(id) = requested_id.as_ref() {
            if symbol_for(self.prism.as_ref(), id).is_ok() {
                if let Some(lineage) = requested_lineage.as_ref() {
                    if self.prism.lineage_of(id).as_ref() != Some(lineage) {
                        self.push_diagnostic(
                            "target_lineage_mismatch",
                            format!(
                                "Target `{}` resolved directly, but its current lineage does not match `{}`.",
                                id.path, lineage.0
                            ),
                            Some(json!({
                                "id": id,
                                "lineageId": lineage.0,
                            })),
                        );
                    }
                }
                return Ok(id.clone());
            }
        }

        let Some(lineage) = requested_lineage else {
            if let Some(id) = requested_id {
                return Err(anyhow!("unknown symbol `{}`", id.path));
            }
            return Err(anyhow!("target must include `id` or `lineageId`"));
        };

        let resolved = self.resolve_lineage_target(&lineage, requested_id.as_ref())?;
        if requested_id.as_ref() != Some(&resolved) {
            self.push_diagnostic(
                "target_remapped_via_lineage",
                format!(
                    "Resolved current target `{}` from stable lineage `{}`.",
                    resolved.path, lineage.0
                ),
                Some(json!({
                    "requestedId": requested_id,
                    "resolvedId": resolved,
                    "lineageId": lineage.0,
                })),
            );
        }
        Ok(resolved)
    }

    fn resolve_lineage_target(
        &self,
        lineage: &LineageId,
        requested_id: Option<&NodeId>,
    ) -> Result<NodeId> {
        let candidates = self.prism.current_nodes_for_lineage(lineage);
        if candidates.is_empty() {
            return Err(anyhow!(
                "lineage `{}` does not currently resolve to any nodes",
                lineage.0
            ));
        }

        if let Some(requested) = requested_id {
            if let Some(exact) = candidates.iter().find(|candidate| *candidate == requested) {
                return Ok(exact.clone());
            }

            let same_crate_and_kind = candidates
                .iter()
                .filter(|candidate| {
                    candidate.crate_name == requested.crate_name && candidate.kind == requested.kind
                })
                .cloned()
                .collect::<Vec<_>>();
            if same_crate_and_kind.len() == 1 {
                return Ok(same_crate_and_kind[0].clone());
            }

            let same_kind = candidates
                .iter()
                .filter(|candidate| candidate.kind == requested.kind)
                .cloned()
                .collect::<Vec<_>>();
            if same_kind.len() == 1 {
                return Ok(same_kind[0].clone());
            }
        }

        if candidates.len() == 1 {
            return Ok(candidates[0].clone());
        }

        Err(anyhow!(
            "lineage `{}` is ambiguous and currently resolves to {} nodes",
            lineage.0,
            candidates.len()
        ))
    }

    fn symbols_from<'a, I>(&self, symbols: I) -> Result<Vec<SymbolView>>
    where
        I: IntoIterator<Item = Symbol<'a>>,
    {
        symbols
            .into_iter()
            .map(|symbol| symbol_view(self.prism.as_ref(), &symbol))
            .collect()
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum SearchPathMode {
    Contains,
    Exact,
}

fn parse_path_mode(value: Option<&str>) -> Result<SearchPathMode> {
    match value.unwrap_or("contains") {
        "contains" => Ok(SearchPathMode::Contains),
        "exact" => Ok(SearchPathMode::Exact),
        other => Err(anyhow!(
            "unsupported search pathMode `{other}`; expected `contains` or `exact`"
        )),
    }
}

fn apply_search_post_filters(
    results: &mut Vec<SymbolView>,
    workspace_root: Option<&Path>,
    path_filter: Option<&str>,
    path_mode: SearchPathMode,
    structured_path: Option<&str>,
    top_level_only: bool,
) {
    results.retain(|result| {
        matches_search_path(result, workspace_root, path_filter, path_mode)
            && matches_structured_path(result, structured_path)
            && matches_top_level_only(result, top_level_only)
    });
}

fn matches_search_path(
    result: &SymbolView,
    workspace_root: Option<&Path>,
    path_filter: Option<&str>,
    path_mode: SearchPathMode,
) -> bool {
    let Some(path_filter) = path_filter else {
        return true;
    };
    let Some(file_path) = result.file_path.as_deref() else {
        return false;
    };
    let requested = normalize_query_path(path_filter);
    let absolute = normalize_query_path(file_path);
    let relative = workspace_root
        .and_then(|root| Path::new(file_path).strip_prefix(root).ok())
        .map(|path| normalize_query_path(&path.to_string_lossy()));
    match path_mode {
        SearchPathMode::Contains => {
            absolute.contains(&requested)
                || relative
                    .as_deref()
                    .map(|path| path.contains(&requested))
                    .unwrap_or(false)
        }
        SearchPathMode::Exact => {
            absolute == requested
                || relative
                    .as_deref()
                    .map(|path| path == requested.as_str())
                    .unwrap_or(false)
        }
    }
}

fn matches_structured_path(result: &SymbolView, structured_path: Option<&str>) -> bool {
    let Some(structured_path) = structured_path else {
        return true;
    };
    let Some(segments) = structured_segments(&result.id.path) else {
        return false;
    };
    segments == normalize_structured_path(structured_path)
}

fn matches_top_level_only(result: &SymbolView, top_level_only: bool) -> bool {
    if !top_level_only {
        return true;
    }
    structured_segments(&result.id.path)
        .map(|segments| segments.len() == 1)
        .unwrap_or(false)
}

fn structured_segments(path: &str) -> Option<Vec<String>> {
    let (_, after_document) = path.split_once("::document::")?;
    let (_, structured) = after_document.split_once("::")?;
    Some(
        structured
            .split("::")
            .filter(|segment| !segment.is_empty())
            .map(ToString::to_string)
            .collect(),
    )
}

fn normalize_structured_path(path: &str) -> Vec<String> {
    path.replace("::", ".")
        .replace('/', ".")
        .split('.')
        .filter(|segment| !segment.is_empty())
        .map(ToString::to_string)
        .collect()
}

fn normalize_query_path(path: &str) -> String {
    path.trim_start_matches("./").replace('\\', "/")
}
