use std::path::Path;
use std::sync::{Arc, Mutex};
use std::time::Instant;

use anyhow::{anyhow, Context, Result};
use prism_coordination::TaskExecutorCaller;
use prism_ir::{AnchorRef, ArtifactId, CoordinationTaskId, EdgeKind, LineageId, NodeId, PlanId};
use prism_js::{
    ChangedFileView, ChangedSymbolView, ConceptDecodeView, ConceptPacketView, ConnectionInfoView,
    ContractPacketView, DiffHunkView, DiscoveryBundleView, EditContextView, FocusedBlockView,
    MemoryEventView, PatchEventView, QueryDiagnostic, QueryEnvelope, ReadContextView,
    RecentChangeContextView, RuntimeLogEventView, RuntimeStatusView, ScoredMemoryView,
    SourceExcerptView, SourceSliceView, SubgraphView, SymbolView, TextSearchMatchView,
    ToolCatalogEntryView, ToolInputValidationView, ToolSchemaView, ValidationContextView,
    ValidationFeedbackView,
};
use prism_memory::{MemoryEventQuery, MemoryModule, OutcomeKind, OutcomeRecallQuery, RecallQuery};
use prism_query::{ConceptDecodeLens, EditSliceOptions, Prism, SourceExcerptOptions, Symbol};
use serde_json::{json, Value};

use crate::coordination_executor::current_executor_caller;
use crate::file_queries::{
    file_around, file_read, DEFAULT_FILE_AROUND_CONTEXT_LINES, DEFAULT_FILE_AROUND_MAX_CHARS,
    DEFAULT_FILE_READ_MAX_CHARS,
};
use crate::peer_runtime_router::execute_remote_prism_query_with_provider;
use crate::query_typecheck::StaticCheckMode;
use crate::runtime_views::{connection_info, runtime_logs, runtime_status, runtime_timeline};
use crate::text_search::search_text;
use crate::{
    ambiguity::is_broad_identifier_query, ambiguity_diagnostic_data, apply_module_filter,
    artifact_risk_view, artifact_view, blast_radius_view, blocker_view, change_impact_view,
    changed_files, changed_symbols, changed_symbols_from_events, claim_view, co_change_view,
    combined_parse_typescript_error, concept_decode_lens_view, concept_packet_view,
    concept_relation_view, concept_resolution_is_ambiguous, conflict_view, contract_packet_view,
    convert_anchors, convert_capability, convert_claim_mode, convert_node_id,
    coordination_plan_v2_view, coordination_task_v2_view, current_timestamp, diff_for,
    diff_for_from_events, drift_candidate_view, edge_kind_label, edge_view, edit_slice_for_symbol,
    entrypoints_for, focused_block_for_symbol, invalid_query_argument_error, is_query_parse_error,
    js_runtime, lineage_view, memory_event_view, merge_node_ids, merge_promoted_checks,
    missing_return_hint, next_reads, node_ref_view, owner_symbol_views_for_query,
    owner_symbol_views_for_target, owner_views_for_target, parse_event_actor,
    parse_memory_event_action, parse_memory_kind, parse_memory_scope, parse_node_kind,
    parse_outcome_kind, parse_outcome_result, parse_plan_scope, parse_plan_status,
    parse_typescript_error, plan_children_v2_view, plan_summary_view, policy_violation_record_view,
    promoted_memory_entries, promoted_summary_texts, promoted_validation_checks, query_diagnostic,
    query_feature_disabled_error, query_method_specs, rank_search_results,
    read_context_view_cached, recent_change_context_view_cached, recent_patches,
    recent_patches_from_events, relations_view, resolve_concepts_for_session, result_decode_error,
    runtime_or_serialization_error, scored_memory_view, search_queries, source_excerpt_for_symbol,
    spec_cluster_view, spec_drift_explanation_view, symbol_for, symbol_view, symbol_views_for_ids,
    task_evidence_status_view, task_intent_view, task_review_status_view, task_risk_view,
    task_validation_recipe_view, tool_catalog_views_with_features, tool_schema_view_with_features,
    validate_tool_input_value_with_features, validation_context_view_cached,
    validation_recipe_view_with, weak_concept_match_reason, weak_search_match_diagnostic_data,
    weak_search_match_reason, where_used, AnchorListArgs, CallGraphArgs, ChangedFilesArgs,
    ChangedSymbolsArgs, ConceptHandleArgs, ConceptQueryArgs, ConceptVerbosity, ContractQueryArgs,
    ContractsQueryArgs, CoordinationTaskTargetArgs, CuratorJobArgs, CuratorJobsArgs,
    CuratorProposalsArgs, DecodeConceptArgs, DiffForArgs, DiscoveryTargetArgs, EditSliceArgs,
    FileAroundArgs, FileReadArgs, ImplementationTargetArgs, LimitArgs, McpLogArgs, McpTraceArgs,
    MemoryEventArgs, MemoryOutcomeArgs, MemoryRecallArgs, NodeIdInput, OwnerLookupArgs,
    PendingReviewsArgs, PlanTargetArgs, PlansQueryArgs, PolicyViolationQueryArgs, QueryHost,
    QueryLanguage, QueryLogArgs, QueryRun, QueryTraceArgs, RecentPatchesArgs, RuntimeLogArgs,
    RuntimeTimelineArgs, SearchAmbiguityContext, SearchArgs, SearchTextArgs, SemanticContextCache,
    SessionState, SimulateClaimArgs, SourceExcerptArgs, SpecIdArgs, SymbolQueryArgs,
    SymbolTargetArgs, TaskChangesArgs, TaskJournalArgs, TaskScopeMode, TaskTargetArgs,
    ToolNameArgs, ToolValidationArgs, ValidationFeedbackArgs, WhereUsedArgs,
    DEFAULT_CALL_GRAPH_DEPTH, DEFAULT_SEARCH_LIMIT, DEFAULT_TASK_JOURNAL_EVENT_LIMIT,
    DEFAULT_TASK_JOURNAL_MEMORY_LIMIT, INSIGHT_LIMIT, QUERY_RUNTIME_ERROR_MARKER,
    QUERY_SERIALIZATION_ERROR_MARKER, USER_SNIPPET_LOCATION_MARKER, USER_SNIPPET_MARKER,
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

    fn static_check_mode(self) -> StaticCheckMode {
        match self {
            TsSnippetMode::StatementBody => StaticCheckMode::StatementBody,
            TsSnippetMode::ImplicitExpression => StaticCheckMode::ImplicitExpression,
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
    output_cap_hit: bool,
}

#[derive(Debug, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
struct RemoteQueryDispatchArgs {
    runtime_id: String,
    path: Vec<String>,
    #[serde(default)]
    args: Vec<Value>,
}

#[derive(Debug, serde::Deserialize)]
struct CodeMutationArgs {
    input: Value,
}

#[derive(Debug, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
struct NativeDeclareWorkArgs {
    input: Value,
}

#[derive(Debug, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
struct NativeClaimAcquireArgs {
    input: Value,
}

#[derive(Debug, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
struct NativeClaimRenewArgs {
    claim: Value,
    input: Value,
}

#[derive(Debug, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
struct NativeClaimReleaseArgs {
    claim: Value,
}

#[derive(Debug, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
struct NativeArtifactProposeArgs {
    input: Value,
}

#[derive(Debug, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
struct NativeArtifactSupersedeArgs {
    artifact: Value,
}

#[derive(Debug, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
struct NativeArtifactReviewArgs {
    artifact: Value,
    input: Value,
}

#[derive(Debug, serde::Deserialize)]
struct FinalizeCodeArgs {
    result: Value,
}

#[derive(Debug, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
struct NativeCreatePlanArgs {
    input: Value,
}

#[derive(Debug, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
struct NativeOpenPlanArgs {
    plan_id: String,
}

#[derive(Debug, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
struct NativeOpenTaskArgs {
    task_id: String,
}

#[derive(Debug, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
struct NativePlanUpdateArgs {
    plan: Value,
    input: Value,
}

#[derive(Debug, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
struct NativePlanArchiveArgs {
    plan: Value,
}

#[derive(Debug, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
struct NativePlanAddTaskArgs {
    plan_handle_id: String,
    input: Value,
}

#[derive(Debug, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
struct NativeTaskDependsOnArgs {
    task: Value,
    depends_on: Value,
    kind: Option<String>,
}

#[derive(Debug, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
struct NativeTaskUpdateArgs {
    task: Value,
    input: Value,
}

#[derive(Debug, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
struct NativeTaskCompleteArgs {
    task: Value,
    input: Value,
}

#[derive(Debug, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
struct NativeTaskHandoffArgs {
    task: Value,
    input: Value,
}

#[derive(Debug, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
struct NativeTaskAcceptHandoffArgs {
    task: Value,
    input: Value,
}

#[derive(Debug, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
struct NativeTaskResumeArgs {
    task: Value,
    input: Value,
}

#[derive(Debug, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
struct NativeTaskReclaimArgs {
    task: Value,
    input: Value,
}

#[derive(Debug, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
struct NodeRefArgs {
    kind: prism_ir::NodeRefKind,
    id: String,
}

#[derive(Debug, Default, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
struct ActionableTasksArgs {
    principal: Option<String>,
}

fn is_identifier_segment(segment: &str) -> bool {
    let mut chars = segment.chars();
    let Some(first) = chars.next() else {
        return false;
    };
    if !(first == '_' || first == '$' || first.is_ascii_alphabetic()) {
        return false;
    }
    chars.all(|ch| ch == '_' || ch == '$' || ch.is_ascii_alphanumeric())
}

fn remote_method_chain(path: &[String]) -> String {
    path.iter().fold(String::from("prism"), |mut acc, segment| {
        if is_identifier_segment(segment) {
            acc.push('.');
            acc.push_str(segment);
        } else {
            let encoded = serde_json::to_string(segment)
                .expect("query path segment should serialize as JSON string");
            acc.push('[');
            acc.push_str(&encoded);
            acc.push(']');
        }
        acc
    })
}

fn contract_resolution_is_ambiguous(resolutions: &[prism_query::ContractResolution]) -> bool {
    let [top, second, ..] = resolutions else {
        return false;
    };
    second.score.saturating_add(35) >= top.score
        || (top.score > 0 && second.score.saturating_mul(100) >= top.score.saturating_mul(85))
}

fn parse_contract_status_filter(value: &str) -> Result<&'static str> {
    match value {
        "candidate" => Ok("candidate"),
        "active" => Ok("active"),
        "deprecated" => Ok("deprecated"),
        "retired" => Ok("retired"),
        _ => Err(invalid_query_argument_error(
            "status",
            format!(
                "Unsupported contract status `{value}`. Expected one of: candidate, active, deprecated, retired."
            ),
        )),
    }
}

fn parse_contract_scope_filter(value: &str) -> Result<&'static str> {
    match value {
        "local" => Ok("local"),
        "session" => Ok("session"),
        "repo" => Ok("repo"),
        _ => Err(invalid_query_argument_error(
            "scope",
            format!("Unsupported contract scope `{value}`. Expected one of: local, session, repo."),
        )),
    }
}

fn parse_contract_kind_filter(value: &str) -> Result<&'static str> {
    match value {
        "interface" => Ok("interface"),
        "behavioral" => Ok("behavioral"),
        "data_shape" => Ok("data_shape"),
        "dependency_boundary" => Ok("dependency_boundary"),
        "lifecycle" => Ok("lifecycle"),
        "protocol" => Ok("protocol"),
        "operational" => Ok("operational"),
        _ => Err(invalid_query_argument_error(
            "kind",
            format!(
                "Unsupported contract kind `{value}`. Expected one of: interface, behavioral, data_shape, dependency_boundary, lifecycle, protocol, operational."
            ),
        )),
    }
}

struct TaskQuerySubject {
    coordination_task_id: CoordinationTaskId,
}

fn resolve_task_query_subject(prism: &Prism, task_id: &str) -> Option<TaskQuerySubject> {
    let coordination_task_id = CoordinationTaskId::new(task_id.to_string());
    prism
        .coordination_task_v2_by_coordination_id(&coordination_task_id)
        .map(|_| TaskQuerySubject {
            coordination_task_id,
        })
}

fn task_query_subject_anchors(prism: &Prism, subject: &TaskQuerySubject) -> Vec<AnchorRef> {
    prism
        .coordination_task_v2_by_coordination_id(&subject.coordination_task_id)
        .map(|task| task.task.anchors)
        .unwrap_or_default()
}

impl QueryHost {
    pub(crate) fn execute(
        &self,
        session: Arc<SessionState>,
        code: &str,
        language: QueryLanguage,
    ) -> Result<QueryEnvelope> {
        self.execute_code(session, code, language, "prism_query", None)
    }

    pub(crate) fn execute_code(
        &self,
        session: Arc<SessionState>,
        code: &str,
        language: QueryLanguage,
        surface_name: &'static str,
        code_mutation: Option<crate::prism_code_builder::PrismCodeExecutionContext>,
    ) -> Result<QueryEnvelope> {
        match language {
            QueryLanguage::Ts => {
                self.execute_typescript(session, code, surface_name, code_mutation)
            }
        }
    }

    #[cfg(test)]
    pub(crate) fn symbol_query(
        &self,
        session: Arc<SessionState>,
        query: &str,
    ) -> Result<QueryEnvelope> {
        let query_run = self.begin_query_run(
            session.as_ref(),
            "prism_query",
            "symbolQuery",
            format!("symbol({query})"),
        );
        let phase_args = json!({ "tool": "prism_query", "queryKind": "symbolQuery" });
        let mut execution = None;
        let execute_started = Instant::now();
        match (|| -> Result<(Value, Vec<QueryDiagnostic>, usize, std::time::Duration, std::time::Duration)> {
            let refresh = self.observe_workspace_for_read()?;
            crate::refresh_phases::record_query_runtime_sync_phases(&query_run, &refresh);
            let created = QueryExecution::new(
                self.clone(),
                Arc::clone(&session),
                self.current_prism(),
                query_run.clone(),
            );
            execution = Some(created.clone());
            let result = serde_json::to_value(created.best_symbol(query)?)?;
            let diagnostics = created.diagnostics();
            let execute_duration = execute_started.elapsed();
            let encode_started = Instant::now();
            let json_bytes = serde_json::to_vec(&result)?.len();
            let encode_duration = encode_started.elapsed();
            Ok((
                result,
                diagnostics,
                json_bytes,
                execute_duration,
                encode_duration,
            ))
        })() {
            Ok((result, diagnostics, json_bytes, execute_duration, encode_duration)) => {
                query_run.record_phase(
                    "mcp.executeHandler",
                    &phase_args,
                    execute_duration,
                    true,
                    None,
                );
                query_run.record_phase(
                    "mcp.encodeResponse",
                    &phase_args,
                    encode_duration,
                    true,
                    None,
                );
                query_run.finish_success(
                    self.mcp_call_log_store.as_ref(),
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
                query_run.record_phase(
                    "mcp.executeHandler",
                    &phase_args,
                    execute_started.elapsed(),
                    false,
                    Some(error.to_string()),
                );
                query_run.finish_error(
                    self.mcp_call_log_store.as_ref(),
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
    pub(crate) fn search_query(
        &self,
        session: Arc<SessionState>,
        args: SearchArgs,
    ) -> Result<QueryEnvelope> {
        let query_run = self.begin_query_run(
            session.as_ref(),
            "prism_query",
            "searchQuery",
            format!("search({})", args.query),
        );
        let phase_args = json!({ "tool": "prism_query", "queryKind": "searchQuery" });
        let mut execution = None;
        let execute_started = Instant::now();
        match (|| -> Result<(Value, Vec<QueryDiagnostic>, usize, std::time::Duration, std::time::Duration)> {
            let refresh = self.observe_workspace_for_read()?;
            crate::refresh_phases::record_query_runtime_sync_phases(&query_run, &refresh);
            let created = QueryExecution::new(
                self.clone(),
                Arc::clone(&session),
                self.current_prism(),
                query_run.clone(),
            );
            execution = Some(created.clone());
            let result = serde_json::to_value(created.search(args)?)?;
            let diagnostics = created.diagnostics();
            let execute_duration = execute_started.elapsed();
            let encode_started = Instant::now();
            let json_bytes = serde_json::to_vec(&result)?.len();
            let encode_duration = encode_started.elapsed();
            Ok((
                result,
                diagnostics,
                json_bytes,
                execute_duration,
                encode_duration,
            ))
        })() {
            Ok((result, diagnostics, json_bytes, execute_duration, encode_duration)) => {
                query_run.record_phase(
                    "mcp.executeHandler",
                    &phase_args,
                    execute_duration,
                    true,
                    None,
                );
                query_run.record_phase(
                    "mcp.encodeResponse",
                    &phase_args,
                    encode_duration,
                    true,
                    None,
                );
                query_run.finish_success(
                    self.mcp_call_log_store.as_ref(),
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
                query_run.record_phase(
                    "mcp.executeHandler",
                    &phase_args,
                    execute_started.elapsed(),
                    false,
                    Some(error.to_string()),
                );
                query_run.finish_error(
                    self.mcp_call_log_store.as_ref(),
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

    fn execute_typescript(
        &self,
        session: Arc<SessionState>,
        code: &str,
        surface_name: &'static str,
        code_mutation: Option<crate::prism_code_builder::PrismCodeExecutionContext>,
    ) -> Result<QueryEnvelope> {
        let query_run = self
            .begin_query_run(session.as_ref(), surface_name, "typescript", code)
            .with_request_payload(json!({
                "code": code,
                "language": "ts",
            }));
        let phase_args = json!({ "tool": surface_name, "queryKind": "typescript" });
        let mut execution = None;
        let execute_started = Instant::now();
        match (|| -> Result<(
            Value,
            Vec<QueryDiagnostic>,
            usize,
            bool,
            std::time::Duration,
            std::time::Duration,
        )> {
            let refresh_started = Instant::now();
            let refresh = self.observe_workspace_for_read()?;
            crate::refresh_phases::record_query_runtime_sync_phases(&query_run, &refresh);
            let refresh_duration = refresh_started.elapsed();
            let accounted_runtime_sync_duration =
                crate::refresh_phases::accounted_runtime_sync_duration(&refresh);
            let unattributed_runtime_sync_duration =
                crate::refresh_phases::record_query_runtime_sync_gap_phase(
                    &query_run,
                    &refresh,
                    refresh_duration,
                );
            query_run.record_phase(
                "typescript.refreshWorkspace",
                &json!({
                    "refreshPath": refresh.refresh_path,
                    "deferred": refresh.deferred,
                    "episodicReloaded": refresh.episodic_reloaded,
                    "inferenceReloaded": refresh.inference_reloaded,
                    "coordinationReloaded": refresh.coordination_reloaded,
                    "metrics": refresh.metrics.as_json(),
                    "accountedRuntimeSyncMs": accounted_runtime_sync_duration.as_millis(),
                    "unattributedRuntimeSyncMs": unattributed_runtime_sync_duration.as_millis(),
                }),
                refresh_duration,
                true,
                None,
            );
            let mut statement_attempt = match self.execute_typescript_attempt(
                Arc::clone(&session),
                code,
                TsSnippetMode::StatementBody,
                query_run.clone(),
                surface_name,
                code_mutation.clone(),
            ) {
                Ok(attempt) => attempt,
                Err(statement_error) => {
                    if !is_query_parse_error(&statement_error) {
                        return Err(statement_error);
                    }
                    match self.execute_typescript_attempt(
                        Arc::clone(&session),
                        code,
                        TsSnippetMode::ImplicitExpression,
                        query_run.clone(),
                        surface_name,
                        code_mutation.clone(),
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
                    Arc::clone(&session),
                    code,
                    TsSnippetMode::ImplicitExpression,
                    query_run.clone(),
                    surface_name,
                    code_mutation.clone(),
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
            let execute_duration = execute_started.elapsed();
            let encode_started = Instant::now();
            let json_bytes = serde_json::to_vec(&statement_attempt.result)?.len();
            let encode_duration = encode_started.elapsed();
            Ok((
                statement_attempt.result,
                diagnostics,
                json_bytes,
                statement_attempt.output_cap_hit,
                execute_duration,
                encode_duration,
            ))
        })() {
            Ok((
                result,
                diagnostics,
                json_bytes,
                output_cap_hit,
                execute_duration,
                encode_duration,
            )) => {
                query_run.record_phase(
                    "mcp.executeHandler",
                    &phase_args,
                    execute_duration,
                    true,
                    None,
                );
                query_run.record_phase(
                    "mcp.encodeResponse",
                    &phase_args,
                    encode_duration,
                    true,
                    None,
                );
                query_run.finish_success(
                    self.mcp_call_log_store.as_ref(),
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
                query_run.record_phase(
                    "mcp.executeHandler",
                    &phase_args,
                    execute_started.elapsed(),
                    false,
                    Some(error.to_string()),
                );
                query_run.finish_error(
                    self.mcp_call_log_store.as_ref(),
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
        session: Arc<SessionState>,
        code: &str,
        mode: TsSnippetMode,
        query_run: QueryRun,
        surface_name: &'static str,
        code_mutation: Option<crate::prism_code_builder::PrismCodeExecutionContext>,
    ) -> Result<TypescriptAttempt> {
        let prepared_started = Instant::now();
        let prepared = prepare_typescript_query(code, mode);
        query_run.record_phase(
            &format!("typescript.{}.prepare", mode.code()),
            &json!({ "mode": mode.code() }),
            prepared_started.elapsed(),
            true,
            None,
        );
        let typecheck_started = Instant::now();
        if let Err(error) = crate::query_typecheck::typecheck_query(code, mode.static_check_mode())
        {
            query_run.record_phase(
                &format!("typescript.{}.typecheck", mode.code()),
                &json!({ "mode": mode.code() }),
                typecheck_started.elapsed(),
                false,
                Some(error.to_string()),
            );
            return Err(error);
        }
        query_run.record_phase(
            &format!("typescript.{}.typecheck", mode.code()),
            &json!({ "mode": mode.code() }),
            typecheck_started.elapsed(),
            true,
            None,
        );
        let transpile_started = Instant::now();
        let transpiled = match js_runtime::transpile_typescript(&prepared.source) {
            Ok(transpiled) => {
                query_run.record_phase(
                    &format!("typescript.{}.transpile", mode.code()),
                    &json!({ "mode": mode.code() }),
                    transpile_started.elapsed(),
                    true,
                    None,
                );
                transpiled
            }
            Err(error) => {
                let error = parse_typescript_error(
                    error,
                    code,
                    prepared.user_snippet_first_line,
                    mode.code(),
                );
                query_run.record_phase(
                    &format!("typescript.{}.transpile", mode.code()),
                    &json!({ "mode": mode.code() }),
                    transpile_started.elapsed(),
                    false,
                    Some(error.to_string()),
                );
                return Err(error);
            }
        };
        let execution = QueryExecution::new_with_surface(
            self.clone(),
            Arc::clone(&session),
            self.current_prism(),
            query_run,
            surface_name,
            code_mutation,
        );
        let worker_roundtrip_started = Instant::now();
        let worker_reply = match self.worker_pool.execute(transpiled, execution.clone()) {
            Ok(reply) => reply,
            Err(error) => {
                let error = runtime_or_serialization_error(
                    error,
                    code,
                    prepared.user_snippet_first_line,
                    mode.code(),
                );
                execution.query_run().record_phase(
                    &format!("typescript.{}.workerRoundTrip", mode.code()),
                    &json!({ "mode": mode.code() }),
                    worker_roundtrip_started.elapsed(),
                    false,
                    Some(error.to_string()),
                );
                return Err(error);
            }
        };
        execution.query_run().record_phase(
            &format!("typescript.{}.workerQueueWait", mode.code()),
            &json!({
                "mode": mode.code(),
                "workerIndex": worker_reply.worker_index,
            }),
            worker_reply.queue_wait,
            true,
            None,
        );
        execution.query_run().record_phase(
            &format!("typescript.{}.workerEval", mode.code()),
            &json!({
                "mode": mode.code(),
                "workerIndex": worker_reply.worker_index,
            }),
            worker_reply.eval_duration,
            worker_reply.result.is_ok(),
            worker_reply.result.as_ref().err().map(ToString::to_string),
        );
        execution.query_run().record_phase(
            &format!("typescript.{}.workerCleanup", mode.code()),
            &json!({
                "mode": mode.code(),
                "workerIndex": worker_reply.worker_index,
            }),
            worker_reply.cleanup_duration,
            true,
            None,
        );
        execution.query_run().record_phase(
            &format!("typescript.{}.workerRoundTrip", mode.code()),
            &json!({
                "mode": mode.code(),
                "workerIndex": worker_reply.worker_index,
            }),
            worker_roundtrip_started.elapsed(),
            worker_reply.result.is_ok(),
            worker_reply.result.as_ref().err().map(ToString::to_string),
        );
        let raw_result = worker_reply.result.map_err(|error| {
            runtime_or_serialization_error(
                error,
                code,
                prepared.user_snippet_first_line,
                mode.code(),
            )
        })?;
        let decode_started = Instant::now();
        let decoded_result = match serde_json::from_str(&raw_result) {
            Ok(result) => {
                execution.query_run().record_phase(
                    &format!("typescript.{}.decodeResult", mode.code()),
                    &json!({
                        "mode": mode.code(),
                        "jsonBytes": raw_result.len(),
                    }),
                    decode_started.elapsed(),
                    true,
                    None,
                );
                result
            }
            Err(error) => {
                let error = result_decode_error(error.into(), &raw_result);
                execution.query_run().record_phase(
                    &format!("typescript.{}.decodeResult", mode.code()),
                    &json!({
                        "mode": mode.code(),
                        "jsonBytes": raw_result.len(),
                    }),
                    decode_started.elapsed(),
                    false,
                    Some(error.to_string()),
                );
                return Err(error);
            }
        };
        let mut result = execution.finalize_code_result(decoded_result)?;
        let mut output_cap_hit = false;
        let limits = session.limits();
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
        "(async function() {{\n  const __prismLocationRegex = /(?:file:\\/\\/\\/prism\\/query\\.ts|eval_script):(?<line>\\d+):(?<column>\\d+)/;\n  const __prismParseLocation = (value) => {{\n    const __prismMatch = typeof value === \"string\" ? value.match(__prismLocationRegex) : null;\n    if (!__prismMatch || !__prismMatch.groups) {{\n      return null;\n    }}\n    return {{\n      line: Number(__prismMatch.groups.line),\n      column: Number(__prismMatch.groups.column),\n    }};\n  }};\n  const __prismFormatError = (error) => {{\n    const __prismMessage = error && typeof error === \"object\" && \"message\" in error && error.message\n      ? String(error.message)\n      : String(error);\n    const __prismStack = error && typeof error === \"object\" && \"stack\" in error && error.stack\n      ? String(error.stack)\n      : null;\n    return __prismStack && __prismStack.includes(__prismMessage)\n      ? __prismStack\n      : __prismStack\n        ? `${{__prismMessage}}\\n${{__prismStack}}`\n        : __prismMessage;\n  }};\n  const __prismUserLocation = (error, baseLine) => {{\n    if (typeof baseLine !== \"number\") {{\n      return null;\n    }}\n    const __prismStack = error && typeof error === \"object\" && \"stack\" in error && error.stack\n      ? String(error.stack)\n      : \"\";\n    const __prismLines = __prismStack.split(\"\\n\");\n    const __prismFrame = __prismLines.find((line) => line.includes(\"__prismUserQuery\"))\n      || __prismLines.find((line) => line.includes(\"eval_script:\"));\n    const __prismLocation = __prismParseLocation(__prismFrame);\n    if (!__prismLocation) {{\n      return null;\n    }}\n    return {{\n      line: Math.max(1, __prismLocation.line - baseLine + 1),\n      column: __prismLocation.column,\n    }};\n  }};\n  const __prismThrowTaggedError = (marker, error, userLocation = null) => {{\n    const __prismFormatted = __prismFormatError(error);\n    const __prismHeadline = __prismFormatted.split(\"\\n\")[0] || String(error);\n    const __prismUserLocationLine = userLocation\n      ? `\\n{} ${{userLocation.line}}:${{userLocation.column}}`\n      : \"\";\n    const __prismWrapped = new Error(`${{marker}}\\n${{__prismHeadline}}${{__prismUserLocationLine}}`);\n    __prismWrapped.stack = `${{userLocation ? `{} ${{userLocation.line}}:${{userLocation.column}}\\n` : \"\"}}${{__prismFormatted}}`;\n    throw __prismWrapped;\n  }};\n  let __prismUserSnippetBaseLine = null;\n  const __prismUserQuery = async () => {{\n    const __prismBaseLocation = __prismParseLocation(new Error().stack || \"\");\n    __prismUserSnippetBaseLine = __prismBaseLocation ? __prismBaseLocation.line + 1 : null;\n{}\n{}\n  }};\n  let __prismResult;\n  try {{\n    __prismResult = await __prismUserQuery();\n  }} catch (error) {{\n    __prismThrowTaggedError(\"{}\", error, __prismUserLocation(error, __prismUserSnippetBaseLine));\n  }}\n  try {{\n    __prismResult = __prismHost(\"__finalizeCode\", {{ result: __prismResult }});\n    return __prismResult === undefined ? \"null\" : JSON.stringify(__prismResult);\n  }} catch (error) {{\n    __prismThrowTaggedError(\"{}\", error);\n  }}\n}})();\n",
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
    session: Arc<SessionState>,
    prism: Arc<Prism>,
    query_run: QueryRun,
    surface_name: &'static str,
    code_mutation: Option<crate::prism_code_builder::PrismCodeExecutionContext>,
    diagnostics: Arc<Mutex<Vec<QueryDiagnostic>>>,
    semantic_context_cache: Arc<Mutex<SemanticContextCache>>,
}

impl QueryExecution {
    pub(crate) fn new(
        host: QueryHost,
        session: Arc<SessionState>,
        prism: Arc<Prism>,
        query_run: QueryRun,
    ) -> Self {
        Self::new_with_surface(host, session, prism, query_run, "prism_query", None)
    }

    pub(crate) fn new_with_surface(
        host: QueryHost,
        session: Arc<SessionState>,
        prism: Arc<Prism>,
        query_run: QueryRun,
        surface_name: &'static str,
        code_mutation: Option<crate::prism_code_builder::PrismCodeExecutionContext>,
    ) -> Self {
        Self {
            host,
            session,
            prism,
            query_run,
            surface_name,
            code_mutation,
            diagnostics: Arc::new(Mutex::new(Vec::new())),
            semantic_context_cache: Arc::new(Mutex::new(SemanticContextCache::default())),
        }
    }

    pub(crate) fn query_run(&self) -> &QueryRun {
        &self.query_run
    }

    pub(crate) fn query_view_enabled(&self, flag: crate::QueryViewFeatureFlag) -> bool {
        self.host.features.query_view_enabled(flag)
    }

    pub(crate) fn prism(&self) -> &Prism {
        self.prism.as_ref()
    }

    pub(crate) fn session(&self) -> &SessionState {
        self.session.as_ref()
    }

    pub(crate) fn workspace_root(&self) -> Option<&Path> {
        self.host.workspace_root()
    }

    pub(crate) fn workspace_materialization_summary(
        &self,
    ) -> Option<prism_core::WorkspaceMaterializationSummary> {
        self.host
            .workspace_session()
            .map(|workspace| workspace.workspace_materialization_summary())
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
            Err(error) => {
                if let Some(query_error) = error.downcast_ref::<crate::QueryExecutionError>() {
                    if matches!(
                        query_error.code(),
                        Some("query_feature_disabled" | "query_invalid_argument")
                    ) {
                        return json!({
                            "ok": false,
                            "error": query_error.to_string(),
                            "queryError": {
                                "summary": query_error.summary(),
                                "message": query_error.to_string(),
                                "data": query_error.data(),
                            },
                        })
                        .to_string();
                    }
                }
                if let Some(json_error) = error.downcast_ref::<serde_json::Error>() {
                    return json!({
                        "ok": false,
                        "error": format!("{} arguments invalid", self.surface_name),
                        "queryError": {
                            "summary": format!("{} arguments invalid", self.surface_name),
                            "message": format!(
                                "{} arguments invalid for `{}`: {}\nHint: Check the query method argument names, required fields, and value types, then retry.",
                                self.surface_name,
                                operation,
                                json_error,
                            ),
                            "data": {
                                "code": "query_invalid_argument",
                                "category": "invalid_argument",
                                "operation": operation,
                                "error": json_error.to_string(),
                                "nextAction": format!(
                                    "Check the query method argument names, required fields, and value types for `{}` and retry. See `prism://api-reference` for the exact surface shape.",
                                    operation,
                                ),
                            },
                        },
                    })
                    .to_string();
                }
                json!({ "ok": false, "error": error.to_string() }).to_string()
            }
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
        let result = if operation == "__queryViews" {
            Ok(serde_json::to_value(
                self.host.enabled_query_view_capabilities(),
            )?)
        } else if operation == "__peerQuery" {
            let args: RemoteQueryDispatchArgs = serde_json::from_value(args)?;
            Ok(self.dispatch_remote_query(args)?)
        } else if let Some(name) = operation.strip_prefix("__queryView:") {
            self.dispatch_query_view(name, args)
        } else {
            self.ensure_operation_enabled(operation)?;
            match operation {
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
                "connectionInfo" => Ok(serde_json::to_value(self.connection_info()?)?),
                "runtimeStatus" => Ok(serde_json::to_value(self.runtime_status()?)?),
                "runtimeLogs" => {
                    let args: RuntimeLogArgs = serde_json::from_value(args)?;
                    Ok(serde_json::to_value(self.runtime_logs(args)?)?)
                }
                "runtimeTimeline" => {
                    let args: RuntimeTimelineArgs = serde_json::from_value(args)?;
                    Ok(serde_json::to_value(self.runtime_timeline(args)?)?)
                }
                "mcpLog" => {
                    let args: McpLogArgs = serde_json::from_value(args)?;
                    Ok(serde_json::to_value(self.host.mcp_call_entries(args))?)
                }
                "slowMcpCalls" => {
                    let args: McpLogArgs = serde_json::from_value(args)?;
                    Ok(serde_json::to_value(self.host.slow_mcp_call_entries(args))?)
                }
                "mcpTrace" => {
                    let args: McpTraceArgs = serde_json::from_value(args)?;
                    let trace = self.host.mcp_call_trace_view(&args.id);
                    if trace.is_none() {
                        self.push_diagnostic(
                            "anchor_unresolved",
                            format!("No MCP call trace matched `{}`.", args.id),
                            Some(json!({ "callId": args.id })),
                        );
                    }
                    Ok(serde_json::to_value(trace)?)
                }
                "mcpStats" => {
                    let args: McpLogArgs = serde_json::from_value(args)?;
                    Ok(serde_json::to_value(self.host.mcp_call_stats(args))?)
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
                "validationFeedback" => {
                    let args: ValidationFeedbackArgs = serde_json::from_value(args)?;
                    Ok(serde_json::to_value(self.validation_feedback(args)?)?)
                }
                "concepts" => {
                    let args: ConceptQueryArgs = serde_json::from_value(args)?;
                    Ok(serde_json::to_value(self.concepts(args)?)?)
                }
                "concept" => {
                    let args: ConceptQueryArgs = serde_json::from_value(args)?;
                    Ok(serde_json::to_value(self.concept(args)?)?)
                }
                "conceptByHandle" => {
                    let args: ConceptHandleArgs = serde_json::from_value(args)?;
                    Ok(serde_json::to_value(self.concept_by_handle(args)?)?)
                }
                "contract" => {
                    let args: ContractQueryArgs = serde_json::from_value(args)?;
                    Ok(serde_json::to_value(self.contract(args)?)?)
                }
                "contracts" => {
                    let args: ContractsQueryArgs = serde_json::from_value(args)?;
                    Ok(serde_json::to_value(self.contracts(args)?)?)
                }
                "contractsFor" => {
                    let args: SymbolTargetArgs = serde_json::from_value(args)?;
                    Ok(serde_json::to_value(self.contracts_for(args)?)?)
                }
                "specs" => Ok(serde_json::to_value(self.specs()?)?),
                "spec" => {
                    let args: SpecIdArgs = serde_json::from_value(args)?;
                    Ok(serde_json::to_value(self.spec(args)?)?)
                }
                "specSyncBrief" => {
                    let args: SpecIdArgs = serde_json::from_value(args)?;
                    Ok(serde_json::to_value(self.spec_sync_brief(args)?)?)
                }
                "specCoverage" => {
                    let args: SpecIdArgs = serde_json::from_value(args)?;
                    Ok(serde_json::to_value(self.spec_coverage(args)?)?)
                }
                "specSyncProvenance" => {
                    let args: SpecIdArgs = serde_json::from_value(args)?;
                    Ok(serde_json::to_value(self.spec_sync_provenance(args)?)?)
                }
                "conceptRelations" => {
                    let args: ConceptHandleArgs = serde_json::from_value(args)?;
                    Ok(serde_json::to_value(self.concept_relations(args)?)?)
                }
                "decodeConcept" => {
                    let args: DecodeConceptArgs = serde_json::from_value(args)?;
                    Ok(serde_json::to_value(self.decode_concept(args)?)?)
                }
                "entrypoints" => Ok(serde_json::to_value(self.entrypoints()?)?),
                "plans" => {
                    let args: PlansQueryArgs = serde_json::from_value(args)?;
                    Ok(serde_json::to_value(self.plans(args)?)?)
                }
                "plan" => {
                    let args: PlanTargetArgs = serde_json::from_value(args)?;
                    let plan_id = PlanId::new(args.plan_id);
                    let plan = crate::spec_surface::linked_plan_view(&self.host, &plan_id)?;
                    Ok(serde_json::to_value(plan)?)
                }
                "planSummary" => {
                    let args: PlanTargetArgs = serde_json::from_value(args)?;
                    Ok(serde_json::to_value(
                        self.prism
                            .plan_summary(&PlanId::new(args.plan_id))
                            .map(plan_summary_view),
                    )?)
                }
                "children" => {
                    let args: PlanTargetArgs = serde_json::from_value(args)?;
                    let plan_id = PlanId::new(args.plan_id);
                    let children = self.prism.plan_children_v2(&plan_id);
                    Ok(serde_json::to_value(Some(plan_children_v2_view(
                        &plan_id, children,
                    )))?)
                }
                "dependencies" => {
                    let args: NodeRefArgs = serde_json::from_value(args)?;
                    Ok(serde_json::to_value(
                        self.prism
                            .node_dependencies_v2(&prism_ir::NodeRef {
                                kind: args.kind,
                                id: args.id,
                            })
                            .into_iter()
                            .map(node_ref_view)
                            .collect::<Vec<_>>(),
                    )?)
                }
                "dependents" => {
                    let args: NodeRefArgs = serde_json::from_value(args)?;
                    Ok(serde_json::to_value(
                        self.prism
                            .node_dependents_v2(&prism_ir::NodeRef {
                                kind: args.kind,
                                id: args.id,
                            })
                            .into_iter()
                            .map(node_ref_view)
                            .collect::<Vec<_>>(),
                    )?)
                }
                "portfolio" => Ok(serde_json::to_value(
                    self.prism
                        .root_plans_v2()
                        .into_iter()
                        .map(coordination_plan_v2_view)
                        .collect::<Vec<_>>(),
                )?),
                "task" => {
                    let args: CoordinationTaskTargetArgs = serde_json::from_value(args)?;
                    let task = crate::spec_surface::linked_coordination_task_view(
                        &self.host,
                        &CoordinationTaskId::new(args.task_id),
                    )?;
                    Ok(serde_json::to_value(task)?)
                }
                "graphActionableTasks" => Ok(serde_json::to_value(
                    self.prism
                        .graph_actionable_tasks_v2()
                        .into_iter()
                        .map(coordination_task_v2_view)
                        .collect::<Vec<_>>(),
                )?),
                "actionableTasks" => {
                    let args: ActionableTasksArgs = serde_json::from_value(args)?;
                    let tasks = if let Some(principal) = args.principal {
                        self.prism
                            .actionable_tasks_for_executor_v2(&TaskExecutorCaller::new(
                                prism_ir::ExecutorClass::WorktreeExecutor,
                                None,
                                Some(prism_ir::PrincipalId::new(principal)),
                            ))
                    } else if let Some(caller) =
                        current_executor_caller(self.workspace_root(), Some(self.session()))
                    {
                        self.prism.actionable_tasks_for_executor_v2(&caller)
                    } else {
                        self.prism.graph_actionable_tasks_v2()
                    };
                    Ok(serde_json::to_value(
                        tasks
                            .into_iter()
                            .map(coordination_task_v2_view)
                            .collect::<Vec<_>>(),
                    )?)
                }
                "readyTasks" => {
                    let args: PlanTargetArgs = serde_json::from_value(args)?;
                    let plan_id = PlanId::new(args.plan_id);
                    let ready_tasks = if let Some(caller) =
                        current_executor_caller(self.workspace_root(), Some(self.session()))
                    {
                        self.prism.ready_tasks_for_executor_v2(&plan_id, &caller)
                    } else {
                        self.prism.ready_tasks_v2(&plan_id)
                    };
                    Ok(serde_json::to_value(
                        ready_tasks
                            .into_iter()
                            .map(coordination_task_v2_view)
                            .collect::<Vec<_>>(),
                    )?)
                }
                "claims" => {
                    let args: AnchorListArgs = serde_json::from_value(args)?;
                    Ok(serde_json::to_value(
                        self.prism
                            .claims(
                                &convert_anchors(
                                    &self.prism,
                                    self.host.workspace_session_ref(),
                                    self.workspace_root(),
                                    args.anchors,
                                )?,
                                current_timestamp(),
                            )
                            .into_iter()
                            .map(claim_view)
                            .collect::<Vec<_>>(),
                    )?)
                }
                "conflicts" => {
                    let args: AnchorListArgs = serde_json::from_value(args)?;
                    Ok(serde_json::to_value(
                        self.prism
                            .conflicts(
                                &convert_anchors(
                                    &self.prism,
                                    self.host.workspace_session_ref(),
                                    self.workspace_root(),
                                    args.anchors,
                                )?,
                                current_timestamp(),
                            )
                            .into_iter()
                            .map(conflict_view)
                            .collect::<Vec<_>>(),
                    )?)
                }
                "blockers" => {
                    let args: CoordinationTaskTargetArgs = serde_json::from_value(args)?;
                    let task_id = CoordinationTaskId::new(args.task_id.clone());
                    let blockers = self
                        .prism
                        .task_evidence_status(&task_id, current_timestamp())
                        .map(|status| status.blockers)
                        .unwrap_or_else(|| self.prism.blockers(&task_id, current_timestamp()));
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
                    if blockers.iter().any(|blocker| {
                        blocker.kind == prism_coordination::BlockerKind::StaleRevision
                    }) {
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
                "taskEvidenceStatus" => {
                    let args: CoordinationTaskTargetArgs = serde_json::from_value(args)?;
                    Ok(serde_json::to_value(
                        self.prism
                            .task_evidence_status(
                                &CoordinationTaskId::new(args.task_id),
                                current_timestamp(),
                            )
                            .map(task_evidence_status_view),
                    )?)
                }
                "taskReviewStatus" => {
                    let args: CoordinationTaskTargetArgs = serde_json::from_value(args)?;
                    Ok(serde_json::to_value(
                        self.prism
                            .task_review_status(
                                &CoordinationTaskId::new(args.task_id),
                                current_timestamp(),
                            )
                            .map(task_review_status_view),
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
                    Ok(serde_json::to_value(
                        resolve_task_query_subject(self.prism.as_ref(), &args.task_id).and_then(
                            |subject| {
                                let impact = self
                                    .prism
                                    .task_blast_radius(&subject.coordination_task_id)?;
                                let anchors =
                                    task_query_subject_anchors(self.prism.as_ref(), &subject);
                                let mut view = change_impact_view(impact);
                                view.promoted_summaries = promoted_summary_texts(
                                    self.session.as_ref(),
                                    self.prism.as_ref(),
                                    &anchors,
                                );
                                Some(view)
                            },
                        ),
                    )?)
                }
                "taskValidationRecipe" => {
                    let args: CoordinationTaskTargetArgs = serde_json::from_value(args)?;
                    Ok(serde_json::to_value(
                        resolve_task_query_subject(self.prism.as_ref(), &args.task_id).and_then(
                            |subject| {
                                let mut recipe = self
                                    .prism
                                    .task_validation_recipe(&subject.coordination_task_id)?;
                                let anchors =
                                    task_query_subject_anchors(self.prism.as_ref(), &subject);
                                merge_promoted_checks(
                                    &mut recipe.scored_checks,
                                    promoted_validation_checks(
                                        self.session.as_ref(),
                                        self.prism.as_ref(),
                                        &anchors,
                                    ),
                                );
                                recipe.checks.extend(
                                    recipe.scored_checks.iter().map(|check| check.label.clone()),
                                );
                                recipe.checks.sort();
                                recipe.checks.dedup();
                                Some(task_validation_recipe_view(recipe))
                            },
                        ),
                    )?)
                }
                "taskRisk" => {
                    let args: CoordinationTaskTargetArgs = serde_json::from_value(args)?;
                    Ok(serde_json::to_value(
                        resolve_task_query_subject(self.prism.as_ref(), &args.task_id).and_then(
                            |subject| {
                                let task = self.prism.coordination_task_v2_by_coordination_id(
                                    &subject.coordination_task_id,
                                );
                                let anchors =
                                    task_query_subject_anchors(self.prism.as_ref(), &subject);
                                let mut risk = self.prism.task_risk(
                                    &subject.coordination_task_id,
                                    current_timestamp(),
                                )?;
                                let promoted_summaries = promoted_summary_texts(
                                    self.session.as_ref(),
                                    self.prism.as_ref(),
                                    &anchors,
                                );
                                let promoted_risk_boost = promoted_memory_entries(
                                    self.session.as_ref(),
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
                                risk.review_required = risk.review_required
                                    || task
                                        .as_ref()
                                        .and_then(|task| {
                                            self.prism
                                                .coordination_plan_v2(&task.task.parent_plan_id)
                                        })
                                        .and_then(|plan| {
                                            plan.plan.policy.review_required_above_risk_score
                                        })
                                        .map(|threshold| boosted_risk_score >= threshold)
                                        .unwrap_or(false);
                                let mut view =
                                    task_risk_view(self.prism.as_ref(), risk, promoted_summaries);
                                view.risk_score = boosted_risk_score;
                                Some(view)
                            },
                        ),
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
                                    .coordination_artifact(&artifact_id)
                                    .map(|artifact| artifact.anchors)
                                    .unwrap_or_default();
                                let promoted_summaries = promoted_summary_texts(
                                    self.session.as_ref(),
                                    self.prism.as_ref(),
                                    &anchors,
                                );
                                let promoted_risk_boost = promoted_memory_entries(
                                    self.session.as_ref(),
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
                                let mut view = artifact_risk_view(
                                    self.prism.as_ref(),
                                    risk,
                                    promoted_summaries,
                                );
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
                                &self.session.session_id(),
                                &convert_anchors(
                                    &self.prism,
                                    self.host.workspace_session_ref(),
                                    self.workspace_root(),
                                    args.anchors,
                                )?,
                                convert_capability(args.capability),
                                args.mode.map(convert_claim_mode),
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
                "validateToolInput" => {
                    let args: ToolValidationArgs = serde_json::from_value(args)?;
                    Ok(serde_json::to_value(
                        self.validate_tool_input(&args.name, args.input),
                    )?)
                }
                "mutate" => {
                    let args: CodeMutationArgs = serde_json::from_value(args)?;
                    Ok(self.execute_code_mutation(args.input)?)
                }
                "__declareWork" => {
                    let args: NativeDeclareWorkArgs = serde_json::from_value(args)?;
                    Ok(self.execute_native_declare_work(args.input)?)
                }
                "__claimAcquire" => {
                    let args: NativeClaimAcquireArgs = serde_json::from_value(args)?;
                    Ok(self.execute_native_claim_acquire(args.input)?)
                }
                "__claimRenew" => {
                    let args: NativeClaimRenewArgs = serde_json::from_value(args)?;
                    Ok(self.execute_native_claim_renew(args.claim, args.input)?)
                }
                "__claimRelease" => {
                    let args: NativeClaimReleaseArgs = serde_json::from_value(args)?;
                    Ok(self.execute_native_claim_release(args.claim)?)
                }
                "__artifactPropose" => {
                    let args: NativeArtifactProposeArgs = serde_json::from_value(args)?;
                    Ok(self.execute_native_artifact_propose(args.input)?)
                }
                "__artifactSupersede" => {
                    let args: NativeArtifactSupersedeArgs = serde_json::from_value(args)?;
                    Ok(self.execute_native_artifact_supersede(args.artifact)?)
                }
                "__artifactReview" => {
                    let args: NativeArtifactReviewArgs = serde_json::from_value(args)?;
                    Ok(self.execute_native_artifact_review(args.artifact, args.input)?)
                }
                "__finalizeCode" => {
                    let args: FinalizeCodeArgs = serde_json::from_value(args)?;
                    Ok(self.finalize_code_result(args.result)?)
                }
                "__coordinationCreatePlan" => {
                    let args: NativeCreatePlanArgs = serde_json::from_value(args)?;
                    Ok(self.execute_native_create_plan(args.input)?)
                }
                "__coordinationOpenPlan" => {
                    let args: NativeOpenPlanArgs = serde_json::from_value(args)?;
                    Ok(self.execute_native_open_plan(args.plan_id)?)
                }
                "__coordinationOpenTask" => {
                    let args: NativeOpenTaskArgs = serde_json::from_value(args)?;
                    Ok(self.execute_native_open_task(args.task_id)?)
                }
                "__coordinationPlanUpdate" => {
                    let args: NativePlanUpdateArgs = serde_json::from_value(args)?;
                    Ok(self.execute_native_plan_update(args.plan, args.input)?)
                }
                "__coordinationPlanArchive" => {
                    let args: NativePlanArchiveArgs = serde_json::from_value(args)?;
                    Ok(self.execute_native_plan_archive(args.plan)?)
                }
                "__coordinationPlanAddTask" => {
                    let args: NativePlanAddTaskArgs = serde_json::from_value(args)?;
                    Ok(self.execute_native_plan_add_task(args.plan_handle_id, args.input)?)
                }
                "__coordinationTaskDependsOn" => {
                    let args: NativeTaskDependsOnArgs = serde_json::from_value(args)?;
                    Ok(self.execute_native_task_depends_on(
                        args.task,
                        args.depends_on,
                        args.kind,
                    )?)
                }
                "__coordinationTaskUpdate" => {
                    let args: NativeTaskUpdateArgs = serde_json::from_value(args)?;
                    Ok(self.execute_native_task_update(args.task, args.input)?)
                }
                "__coordinationTaskComplete" => {
                    let args: NativeTaskCompleteArgs = serde_json::from_value(args)?;
                    Ok(self.execute_native_task_complete(args.task, args.input)?)
                }
                "__coordinationTaskHandoff" => {
                    let args: NativeTaskHandoffArgs = serde_json::from_value(args)?;
                    Ok(self.execute_native_task_handoff(args.task, args.input)?)
                }
                "__coordinationTaskAcceptHandoff" => {
                    let args: NativeTaskAcceptHandoffArgs = serde_json::from_value(args)?;
                    Ok(self.execute_native_task_accept_handoff(args.task, args.input)?)
                }
                "__coordinationTaskResume" => {
                    let args: NativeTaskResumeArgs = serde_json::from_value(args)?;
                    Ok(self.execute_native_task_resume(args.task, args.input)?)
                }
                "__coordinationTaskReclaim" => {
                    let args: NativeTaskReclaimArgs = serde_json::from_value(args)?;
                    Ok(self.execute_native_task_reclaim(args.task, args.input)?)
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
                        self.session.as_ref(),
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
                    if lineage.as_ref().is_some_and(|view| {
                        view.history.iter().any(|event| event.kind == "Ambiguous")
                    }) {
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
                        self.session.as_ref(),
                        &id,
                    ))?)
                }
                "validationRecipe" => {
                    let args: SymbolTargetArgs = serde_json::from_value(args)?;
                    let id = self.resolve_target_id(args.id, args.lineage_id)?;
                    Ok(serde_json::to_value(validation_recipe_view_with(
                        self.prism.as_ref(),
                        self.session.as_ref(),
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
                        .min(self.session.limits().max_result_nodes);
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
                        .min(self.session.limits().max_result_nodes);
                    Ok(serde_json::to_value(where_used(
                        self.prism.as_ref(),
                        self.session.as_ref(),
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
                        .min(self.session.limits().max_result_nodes);
                    Ok(serde_json::to_value(entrypoints_for(
                        self.prism.as_ref(),
                        self.session.as_ref(),
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
                        let limit = self.session.limits().max_result_nodes.min(INSIGHT_LIMIT);
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
                        .min(self.session.limits().max_result_nodes);
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
                "memoryEvents" => {
                    let args: MemoryEventArgs = serde_json::from_value(args)?;
                    Ok(serde_json::to_value(self.memory_events(args)?)?)
                }
                "curatorJobs" => {
                    let args: CuratorJobsArgs = serde_json::from_value(args)?;
                    Ok(serde_json::to_value(self.host.curator_jobs(args)?)?)
                }
                "curatorProposals" => {
                    let args: CuratorProposalsArgs = serde_json::from_value(args)?;
                    Ok(serde_json::to_value(self.host.curator_proposals(args)?)?)
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
                    let suggestion = suggest_query_operation(other);
                    let next_action = suggestion
                        .as_deref()
                        .map(|suggested| {
                            format!(
                                "Retry with `{suggested}` or inspect `prism://capabilities` for the canonical query methods."
                            )
                        })
                        .unwrap_or_else(|| {
                            "Inspect `prism://capabilities` for the canonical query methods and retry with one of the listed operations.".to_string()
                        });
                    self.push_diagnostic(
                        "unknown_method",
                        format!("Unknown Prism host operation `{other}`."),
                        Some(json!({
                            "operation": other,
                            "didYouMean": suggestion,
                            "nextAction": next_action,
                        })),
                    );
                    Err(anyhow!("unsupported host operation `{other}`"))
                }
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

    fn dispatch_remote_query(&self, args: RemoteQueryDispatchArgs) -> Result<Value> {
        let root = self.workspace_root().ok_or_else(|| {
            anyhow!("runtime-targeted queries require a workspace-backed session")
        })?;
        if args.path.is_empty() {
            return Err(anyhow!("remote query path cannot be empty"));
        }
        let serialized_args =
            serde_json::to_string(&args.args).context("failed to encode remote query arguments")?;
        let code = match args.path.as_slice() {
            [operation] if operation == "fileRead" => r#"
const [__prismFileReadArgs = {}] = __prismRemoteArgs;
return prism.file(__prismFileReadArgs.path).read({
  startLine: __prismFileReadArgs.startLine,
  endLine: __prismFileReadArgs.endLine,
  maxChars: __prismFileReadArgs.maxChars,
});
"#
            .to_string(),
            [operation] if operation == "fileAround" => r#"
const [__prismFileAroundArgs = {}] = __prismRemoteArgs;
return prism.file(__prismFileAroundArgs.path).around({
  line: __prismFileAroundArgs.line,
  before: __prismFileAroundArgs.before,
  after: __prismFileAroundArgs.after,
  maxChars: __prismFileAroundArgs.maxChars,
});
"#
            .to_string(),
            _ => {
                let chain = remote_method_chain(&args.path);
                format!("return {chain}(...__prismRemoteArgs);")
            }
        };
        let code = format!("const __prismRemoteArgs = {serialized_args}; {code}");
        let remote = if let Some(runtime_gateway) = self.host.workspace_runtime_gateway() {
            runtime_gateway.execute_remote_prism_query(
                &args.runtime_id,
                &code,
                QueryLanguage::Ts,
            )?
        } else {
            execute_remote_prism_query_with_provider(
                root,
                self.host.workspace_authority_store_provider(),
                &args.runtime_id,
                &code,
                QueryLanguage::Ts,
            )?
        };
        self.diagnostics
            .lock()
            .expect("diagnostics lock poisoned")
            .extend(remote.response.result.diagnostics.clone());
        Ok(remote.response.result.result)
    }

    fn ensure_operation_enabled(&self, operation: &str) -> Result<()> {
        if let Some(group) = self.host.features.disabled_query_group(operation) {
            return Err(query_feature_disabled_error(operation, group));
        }
        Ok(())
    }

    fn execute_code_mutation(&self, input: Value) -> Result<Value> {
        let Some(code_mutation) = self.code_mutation.as_ref() else {
            return Err(anyhow!(
                "prism.mutate requires an authenticated prism_code invocation"
            ));
        };
        code_mutation.execute_legacy_mutation(input)
    }

    fn execute_native_declare_work(&self, input: Value) -> Result<Value> {
        let Some(code_mutation) = self.code_mutation.as_ref() else {
            return Err(anyhow!(
                "native work declaration requires an authenticated prism_code invocation"
            ));
        };
        code_mutation.declare_work(input)
    }

    fn execute_native_claim_acquire(&self, input: Value) -> Result<Value> {
        let Some(code_mutation) = self.code_mutation.as_ref() else {
            return Err(anyhow!(
                "native claim writes require an authenticated prism_code invocation"
            ));
        };
        code_mutation.claim_acquire(input)
    }

    fn execute_native_claim_renew(&self, claim: Value, input: Value) -> Result<Value> {
        let Some(code_mutation) = self.code_mutation.as_ref() else {
            return Err(anyhow!(
                "native claim writes require an authenticated prism_code invocation"
            ));
        };
        code_mutation.claim_renew(claim, input)
    }

    fn execute_native_claim_release(&self, claim: Value) -> Result<Value> {
        let Some(code_mutation) = self.code_mutation.as_ref() else {
            return Err(anyhow!(
                "native claim writes require an authenticated prism_code invocation"
            ));
        };
        code_mutation.claim_release(claim)
    }

    fn execute_native_artifact_propose(&self, input: Value) -> Result<Value> {
        let Some(code_mutation) = self.code_mutation.as_ref() else {
            return Err(anyhow!(
                "native artifact writes require an authenticated prism_code invocation"
            ));
        };
        code_mutation.artifact_propose(input)
    }

    fn execute_native_artifact_supersede(&self, artifact: Value) -> Result<Value> {
        let Some(code_mutation) = self.code_mutation.as_ref() else {
            return Err(anyhow!(
                "native artifact writes require an authenticated prism_code invocation"
            ));
        };
        code_mutation.artifact_supersede(artifact)
    }

    fn execute_native_artifact_review(&self, artifact: Value, input: Value) -> Result<Value> {
        let Some(code_mutation) = self.code_mutation.as_ref() else {
            return Err(anyhow!(
                "native artifact writes require an authenticated prism_code invocation"
            ));
        };
        code_mutation.artifact_review(artifact, input)
    }

    fn finalize_code_result(&self, result: Value) -> Result<Value> {
        let Some(code_mutation) = self.code_mutation.as_ref() else {
            return Ok(result);
        };
        code_mutation.finalize_result(result)
    }

    fn execute_native_create_plan(&self, input: Value) -> Result<Value> {
        let Some(code_mutation) = self.code_mutation.as_ref() else {
            return Err(anyhow!(
                "native coordination builders require an authenticated prism_code invocation"
            ));
        };
        code_mutation.create_plan(input)
    }

    fn execute_native_open_plan(&self, plan_id: String) -> Result<Value> {
        let Some(code_mutation) = self.code_mutation.as_ref() else {
            return Err(anyhow!(
                "native coordination builders require an authenticated prism_code invocation"
            ));
        };
        code_mutation.open_plan(plan_id)
    }

    fn execute_native_open_task(&self, task_id: String) -> Result<Value> {
        let Some(code_mutation) = self.code_mutation.as_ref() else {
            return Err(anyhow!(
                "native coordination builders require an authenticated prism_code invocation"
            ));
        };
        code_mutation.open_task(task_id)
    }

    fn execute_native_plan_update(&self, plan: Value, input: Value) -> Result<Value> {
        let Some(code_mutation) = self.code_mutation.as_ref() else {
            return Err(anyhow!(
                "native coordination builders require an authenticated prism_code invocation"
            ));
        };
        code_mutation.plan_update(plan, input)
    }

    fn execute_native_plan_archive(&self, plan: Value) -> Result<Value> {
        let Some(code_mutation) = self.code_mutation.as_ref() else {
            return Err(anyhow!(
                "native coordination builders require an authenticated prism_code invocation"
            ));
        };
        code_mutation.plan_archive(plan)
    }

    fn execute_native_plan_add_task(&self, plan_handle_id: String, input: Value) -> Result<Value> {
        let Some(code_mutation) = self.code_mutation.as_ref() else {
            return Err(anyhow!(
                "native coordination builders require an authenticated prism_code invocation"
            ));
        };
        code_mutation.plan_add_task(plan_handle_id, input)
    }

    fn execute_native_task_depends_on(
        &self,
        task: Value,
        depends_on: Value,
        kind: Option<String>,
    ) -> Result<Value> {
        let Some(code_mutation) = self.code_mutation.as_ref() else {
            return Err(anyhow!(
                "native coordination builders require an authenticated prism_code invocation"
            ));
        };
        code_mutation.task_depends_on(task, depends_on, kind)
    }

    fn execute_native_task_update(&self, task: Value, input: Value) -> Result<Value> {
        let Some(code_mutation) = self.code_mutation.as_ref() else {
            return Err(anyhow!(
                "native coordination builders require an authenticated prism_code invocation"
            ));
        };
        code_mutation.task_update(task, input)
    }

    fn execute_native_task_complete(&self, task: Value, input: Value) -> Result<Value> {
        let Some(code_mutation) = self.code_mutation.as_ref() else {
            return Err(anyhow!(
                "native coordination builders require an authenticated prism_code invocation"
            ));
        };
        code_mutation.task_complete(task, input)
    }

    fn execute_native_task_handoff(&self, task: Value, input: Value) -> Result<Value> {
        let Some(code_mutation) = self.code_mutation.as_ref() else {
            return Err(anyhow!(
                "native coordination builders require an authenticated prism_code invocation"
            ));
        };
        code_mutation.task_handoff(task, input)
    }

    fn execute_native_task_accept_handoff(&self, task: Value, input: Value) -> Result<Value> {
        let Some(code_mutation) = self.code_mutation.as_ref() else {
            return Err(anyhow!(
                "native coordination builders require an authenticated prism_code invocation"
            ));
        };
        code_mutation.task_accept_handoff(task, input)
    }

    fn execute_native_task_resume(&self, task: Value, input: Value) -> Result<Value> {
        let Some(code_mutation) = self.code_mutation.as_ref() else {
            return Err(anyhow!(
                "native coordination builders require an authenticated prism_code invocation"
            ));
        };
        code_mutation.task_resume(task, input)
    }

    fn execute_native_task_reclaim(&self, task: Value, input: Value) -> Result<Value> {
        let Some(code_mutation) = self.code_mutation.as_ref() else {
            return Err(anyhow!(
                "native coordination builders require an authenticated prism_code invocation"
            ));
        };
        code_mutation.task_reclaim(task, input)
    }

    pub(crate) fn best_symbol(&self, query: &str) -> Result<Option<SymbolView>> {
        let mut matches = self.symbols(query)?;
        let current_task_id = self.session.effective_current_task().map(|task| task.0);
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
                prefer_callable_code: None,
                prefer_editable_targets: None,
                prefer_behavioral_owners: None,
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
        tool_catalog_views_with_features(&self.host.features)
    }

    fn tool(&self, name: &str) -> Result<Option<ToolSchemaView>> {
        Ok(tool_schema_view_with_features(name, &self.host.features))
    }

    fn validate_tool_input(&self, name: &str, input: Value) -> ToolInputValidationView {
        validate_tool_input_value_with_features(name, input, &self.host.features)
    }

    pub(crate) fn plans(&self, args: PlansQueryArgs) -> Result<Vec<prism_js::PlanListEntryView>> {
        let status = args.status.as_deref().map(parse_plan_status).transpose()?;
        let scope = args.scope.as_deref().map(parse_plan_scope).transpose()?;
        let requested = args.limit.unwrap_or(DEFAULT_SEARCH_LIMIT);
        let applied = requested.min(self.session.limits().max_result_nodes);
        let mut results = self
            .prism
            .plans(status, scope, args.contains.as_deref())
            .into_iter()
            .map(crate::plan_list_entry_view)
            .collect::<Vec<_>>();
        if requested > applied {
            self.push_diagnostic(
                "result_truncated",
                format!(
                    "Plan-list limit was capped at {} instead of {requested}. Next action: narrow with `status`, `scope`, or `contains` before raising the limit.",
                    applied
                ),
                Some(json!({
                    "requested": requested,
                    "applied": applied,
                    "nextAction": "Use prism.plans({ status: ..., scope: ..., contains: ..., limit: ... }) to narrow the result set.",
                })),
            );
        }
        if results.len() > applied {
            let total = results.len();
            results.truncate(applied);
            self.push_diagnostic(
                "result_truncated",
                format!(
                    "Plan discovery results were truncated at {} entries. Next action: narrow with `status`, `scope`, or `contains`, then inspect one plan with `prism.plan(...)` or `prism.planSummary(...)`.",
                    applied
                ),
                Some(json!({
                    "count": total,
                    "applied": applied,
                    "nextAction": "Use prism.plan(planId) or prism.planSummary(planId) after narrowing prism.plans(...).",
                })),
            );
        }
        Ok(results)
    }

    pub(crate) fn search(&self, args: SearchArgs) -> Result<Vec<SymbolView>> {
        let _include_inferred = args.include_inferred.unwrap_or(true);
        let kind = args.kind.as_deref().map(parse_node_kind).transpose()?;
        let requested = args.limit.unwrap_or(DEFAULT_SEARCH_LIMIT);
        let limits = self.session.limits();
        let applied = requested.min(limits.max_result_nodes);
        let path_mode = parse_path_mode(args.path_mode.as_deref())?;
        let explicit_task_id = args.task_id.clone();
        let current_task_id = self.session.effective_current_task().map(|task| task.0);
        let effective_task_id = explicit_task_id.as_deref().or(current_task_id.as_deref());
        let strategy = args.strategy.as_deref().unwrap_or("direct");
        let exact_structured =
            args.structured_path.is_some() || args.top_level_only.unwrap_or(false);
        let needs_post_filter = path_mode == SearchPathMode::Exact || exact_structured;
        let broad_identifier_overfetch = strategy == "direct"
            && kind.is_none()
            && args.path.is_none()
            && args.module.is_none()
            && explicit_task_id.is_none()
            && is_broad_identifier_query(&args.query);
        let backend_limit = if needs_post_filter {
            limits.max_result_nodes.saturating_add(1)
        } else if broad_identifier_overfetch {
            applied
                .saturating_mul(8)
                .max(32)
                .min(limits.max_result_nodes.saturating_add(1))
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

        let prefer_behavioral_owners = args.prefer_behavioral_owners.unwrap_or(false);
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
        if strategy != "behavioral" && prefer_behavioral_owners {
            let owner_results = owner_symbol_views_for_query(
                self.prism.as_ref(),
                &args.query,
                args.owner_kind.as_deref(),
                kind,
                args.path.as_deref(),
                backend_limit,
            )?;
            for candidate in owner_results {
                if let Some(existing) = results
                    .iter_mut()
                    .find(|existing| existing.id == candidate.id)
                {
                    if existing.owner_hint.is_none() && candidate.owner_hint.is_some() {
                        existing.owner_hint = candidate.owner_hint;
                    }
                } else {
                    results.push(candidate);
                }
            }
        }
        apply_search_post_filters(
            &mut results,
            self.host.workspace_root(),
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
                prefer_callable_code: args.prefer_callable_code,
                prefer_editable_targets: args.prefer_editable_targets,
                prefer_behavioral_owners: args.prefer_behavioral_owners,
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
            if let Some(reason) = weak_search_match_reason(ambiguity) {
                self.push_diagnostic(
                    "weak_search_match",
                    format!(
                        "Search for `{}` is too generic to produce a confident first hop. {} Next action: add a behavior term, module, path, or task filter.",
                        args.query, reason
                    ),
                    Some(weak_search_match_diagnostic_data(
                        ambiguity,
                        reason,
                        "Use prism.search(query, { path: ..., module: ..., ownerKind: ..., taskId: ..., limit: ... }) with a behavior term or scope filter, or jump straight to prism.readContext(...) on an intended candidate.",
                    )),
                );
            }
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
        let outcome = search_text(self.host(), args, self.session.limits().max_result_nodes)?;
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
        let applied = requested.min(self.session.limits().max_result_nodes);
        let mut results = if let Some(workspace) = self.host.workspace_session() {
            workspace
                .load_patch_file_summaries(
                    args.task_id.as_ref(),
                    args.since,
                    args.path.as_deref(),
                    applied.saturating_add(1),
                )?
                .into_iter()
                .map(|summary| ChangedFileView {
                    path: summary.path,
                    event_id: summary.event_id.0.to_string(),
                    ts: summary.ts,
                    task_id: summary.task_id,
                    trigger: summary.trigger,
                    actor: summary.actor,
                    reason: summary.reason,
                    work_id: summary.work_id,
                    work_title: summary.work_title,
                    summary: summary.summary,
                    changed_symbol_count: summary.changed_symbol_count,
                    added_count: summary.added_count,
                    removed_count: summary.removed_count,
                    updated_count: summary.updated_count,
                })
                .collect()
        } else {
            changed_files(
                self.prism.as_ref(),
                args.task_id.as_ref(),
                args.since,
                args.path.as_deref(),
                applied.saturating_add(1),
            )?
        };
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
        let applied = requested.min(self.session.limits().max_result_nodes);
        let mut results = if let Some(workspace) = self.host.workspace_session() {
            let candidate_limit = applied.saturating_mul(4).max(applied.saturating_add(16));
            let events = workspace
                .load_patch_event_summaries(
                    None,
                    args.task_id.as_ref(),
                    args.since,
                    Some(&args.path),
                    candidate_limit,
                )?
                .into_iter()
                .map(|summary| workspace.load_outcome_event(&summary.event_id))
                .collect::<Result<Vec<_>>>()?
                .into_iter()
                .flatten()
                .collect::<Vec<_>>();
            changed_symbols_from_events(
                self.prism.as_ref(),
                events,
                &args.path,
                applied.saturating_add(1),
            )?
        } else {
            changed_symbols(
                self.prism.as_ref(),
                &args.path,
                args.task_id.as_ref(),
                args.since,
                applied.saturating_add(1),
            )?
        };
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
        let applied = requested.min(self.session.limits().max_result_nodes);
        let target = args.target.map(convert_node_id).transpose()?;
        let mut results = if let Some(workspace) = self.host.workspace_session() {
            let events = workspace
                .load_patch_event_summaries(
                    target.as_ref(),
                    args.task_id.as_ref(),
                    args.since,
                    args.path.as_deref(),
                    applied.saturating_add(1),
                )?
                .into_iter()
                .map(|summary| workspace.load_outcome_event(&summary.event_id))
                .collect::<Result<Vec<_>>>()?
                .into_iter()
                .flatten()
                .collect::<Vec<_>>();
            recent_patches_from_events(
                self.prism.as_ref(),
                events,
                args.path.as_deref(),
                applied.saturating_add(1),
            )?
        } else {
            recent_patches(
                self.prism.as_ref(),
                target.as_ref(),
                args.task_id.as_ref(),
                args.since,
                args.path.as_deref(),
                applied.saturating_add(1),
            )?
        };
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
        let applied = requested.min(self.session.limits().max_result_nodes);
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

        let mut results = if let (Some(workspace), Some(target_id)) =
            (self.host.workspace_session(), target.as_ref())
        {
            let candidate_limit = applied.saturating_mul(4).max(applied.saturating_add(16));
            let events = workspace
                .load_patch_event_summaries(
                    Some(target_id),
                    args.task_id.as_ref(),
                    args.since,
                    None,
                    candidate_limit,
                )?
                .into_iter()
                .map(|summary| workspace.load_outcome_event(&summary.event_id))
                .collect::<Result<Vec<_>>>()?
                .into_iter()
                .flatten()
                .collect::<Vec<_>>();
            diff_for_from_events(
                self.prism.as_ref(),
                events,
                Some(target_id),
                requested_lineage.as_ref(),
                applied.saturating_add(1),
            )?
        } else {
            diff_for(
                self.prism.as_ref(),
                target.as_ref(),
                requested_lineage.as_ref(),
                args.task_id.as_ref(),
                args.since,
                applied.saturating_add(1),
            )?
        };
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

    pub(crate) fn connection_info(&self) -> Result<ConnectionInfoView> {
        connection_info(&self.host)
    }

    pub(crate) fn runtime_logs(&self, args: RuntimeLogArgs) -> Result<Vec<RuntimeLogEventView>> {
        let requested = args.limit.unwrap_or(DEFAULT_SEARCH_LIMIT);
        let applied = requested.min(self.session.limits().max_result_nodes);
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
        let applied = requested.min(self.session.limits().max_result_nodes);
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
        let applied = requested.min(self.session.limits().max_result_nodes);
        let mut results = if let Some(workspace) = self.host.workspace_session() {
            let events = workspace
                .load_patch_event_summaries(
                    None,
                    Some(&args.task_id),
                    args.since,
                    args.path.as_deref(),
                    applied.saturating_add(1),
                )?
                .into_iter()
                .map(|summary| workspace.load_outcome_event(&summary.event_id))
                .collect::<Result<Vec<_>>>()?
                .into_iter()
                .flatten()
                .collect::<Vec<_>>();
            recent_patches_from_events(
                self.prism.as_ref(),
                events,
                args.path.as_deref(),
                applied.saturating_add(1),
            )?
        } else {
            recent_patches(
                self.prism.as_ref(),
                None,
                Some(&args.task_id),
                args.since,
                args.path.as_deref(),
                applied.saturating_add(1),
            )?
        };
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

    pub(crate) fn validation_feedback(
        &self,
        args: ValidationFeedbackArgs,
    ) -> Result<Vec<ValidationFeedbackView>> {
        let Some(workspace) = self.host.workspace_session() else {
            return Ok(Vec::new());
        };

        let requested = args.limit.unwrap_or(DEFAULT_SEARCH_LIMIT);
        let applied = requested.min(self.session.limits().max_result_nodes);
        let contains = args
            .contains
            .as_ref()
            .map(|value| value.to_ascii_lowercase());
        let verdict = args
            .verdict
            .as_ref()
            .map(|value| value.to_ascii_lowercase());
        let category = args
            .category
            .as_ref()
            .map(|value| value.to_ascii_lowercase());

        let mut results = workspace
            .validation_feedback(None)?
            .into_iter()
            .filter(|entry| {
                args.since.is_none_or(|since| entry.recorded_at >= since)
                    && args
                        .task_id
                        .as_ref()
                        .is_none_or(|task_id| entry.task_id.as_ref() == Some(task_id))
                    && verdict.as_ref().is_none_or(|verdict| {
                        entry.verdict.to_string().eq_ignore_ascii_case(verdict)
                    })
                    && category.as_ref().is_none_or(|category| {
                        entry.category.to_string().eq_ignore_ascii_case(category)
                    })
                    && args
                        .corrected_manually
                        .is_none_or(|corrected| entry.corrected_manually == corrected)
                    && contains
                        .as_ref()
                        .is_none_or(|needle| validation_feedback_contains(entry, needle))
            })
            .map(validation_feedback_view)
            .collect::<Vec<_>>();

        if requested > applied {
            self.push_diagnostic(
                "result_truncated",
                format!(
                    "Validation-feedback limit was capped at {} instead of {requested}. Next action: narrow with `category`, `verdict`, `contains`, or `taskId` before raising the limit.",
                    applied
                ),
                Some(json!({
                    "requested": requested,
                    "applied": applied,
                    "nextAction": "Use prism.validationFeedback({ category: ..., verdict: ..., contains: ..., taskId: ..., limit: ... }) to narrow the result set.",
                })),
            );
        }
        if results.len() > applied {
            results.truncate(applied);
            self.push_diagnostic(
                "result_truncated",
                format!(
                    "Validation feedback was truncated at {} entries. Next action: narrow with `category`, `verdict`, `contains`, or `taskId`.",
                    applied
                ),
                Some(json!({
                    "applied": applied,
                    "nextAction": "Use prism.validationFeedback({ category: ..., verdict: ..., contains: ..., taskId: ..., limit: ... }) to narrow the result set.",
                })),
            );
        }

        Ok(results)
    }

    fn host(&self) -> &QueryHost {
        &self.host
    }

    pub(crate) fn entrypoints(&self) -> Result<Vec<SymbolView>> {
        let limits = self.session.limits();
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
        let limits = self.session.limits();
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
        let limits = self.session.limits();
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

    fn memory_events(&self, args: MemoryEventArgs) -> Result<Vec<MemoryEventView>> {
        let requested = args.limit.unwrap_or(10);
        let limits = self.session.limits();
        let applied = requested.min(limits.max_result_nodes);
        if requested > limits.max_result_nodes {
            self.push_diagnostic(
                "result_truncated",
                format!(
                    "Memory event limit was capped at {} instead of {requested}.",
                    limits.max_result_nodes
                ),
                Some(json!({
                    "requested": requested,
                    "applied": applied,
                })),
            );
        }
        let workspace = self.host.workspace_session().ok_or_else(|| {
            anyhow!("memory event inspection requires a workspace-backed PRISM session")
        })?;

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
        let actions = args
            .actions
            .map(|actions| {
                actions
                    .into_iter()
                    .map(|action| parse_memory_event_action(&action))
                    .collect::<Result<Vec<_>>>()
            })
            .transpose()?;
        let scope = args
            .scope
            .map(|value| parse_memory_scope(&value))
            .transpose()?;
        let events = workspace.memory_events(&MemoryEventQuery {
            memory_id: args.memory_id.map(prism_memory::MemoryId),
            focus,
            text: args.text,
            limit: applied,
            kinds,
            actions,
            scope,
            task_id: args.task_id,
            since: args.since,
        })?;
        Ok(events.into_iter().map(memory_event_view).collect())
    }

    fn task_journal(&self, args: TaskJournalArgs) -> Result<prism_js::TaskJournalView> {
        let event_requested = args.event_limit.unwrap_or(DEFAULT_TASK_JOURNAL_EVENT_LIMIT);
        let memory_requested = args
            .memory_limit
            .unwrap_or(DEFAULT_TASK_JOURNAL_MEMORY_LIMIT);
        let limits = self.session.limits();
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

        let replay = crate::load_task_replay(
            self.host.workspace_session_ref(),
            self.prism.as_ref(),
            &args.task_id,
        )?;
        let journal = crate::task_journal_view_from_replay(
            self.session.as_ref(),
            self.prism.as_ref(),
            replay,
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
        read_context_view_cached(self.prism.as_ref(), self.session.as_ref(), &mut cache, id)
    }

    fn edit_context(&self, id: &NodeId) -> Result<EditContextView> {
        let mut cache = self
            .semantic_context_cache
            .lock()
            .expect("semantic context cache lock poisoned");
        crate::edit_context_view_cached(self.prism.as_ref(), self.session.as_ref(), &mut cache, id)
    }

    fn validation_context(&self, id: &NodeId) -> Result<ValidationContextView> {
        let mut cache = self
            .semantic_context_cache
            .lock()
            .expect("semantic context cache lock poisoned");
        validation_context_view_cached(self.prism.as_ref(), self.session.as_ref(), &mut cache, id)
    }

    fn recent_change_context(&self, id: &NodeId) -> Result<RecentChangeContextView> {
        let mut cache = self
            .semantic_context_cache
            .lock()
            .expect("semantic context cache lock poisoned");
        recent_change_context_view_cached(
            self.prism.as_ref(),
            self.session.as_ref(),
            &mut cache,
            id,
        )
    }

    fn discovery_bundle(&self, id: &NodeId) -> Result<DiscoveryBundleView> {
        let mut cache = self
            .semantic_context_cache
            .lock()
            .expect("semantic context cache lock poisoned");
        crate::discovery_bundle_view_cached_with_trace(
            self.prism.as_ref(),
            self.session.as_ref(),
            &mut cache,
            Some(self.query_run.clone()),
            id,
        )
    }

    fn concepts(&self, args: ConceptQueryArgs) -> Result<Vec<ConceptPacketView>> {
        let verbosity = parse_concept_verbosity(
            args.verbosity.as_deref(),
            "prism.concepts",
            ConceptVerbosity::Summary,
        )?;
        let requested = args.limit.unwrap_or(5);
        let applied = requested.min(self.session.limits().max_result_nodes);
        if requested > applied {
            self.push_diagnostic(
                "result_truncated",
                format!(
                    "Concept query limit was capped at {} instead of {requested}.",
                    applied
                ),
                Some(json!({ "requested": requested, "applied": applied })),
            );
        }
        let concepts = resolve_concepts_for_session(
            self.prism.as_ref(),
            self.session.as_ref(),
            &args.query,
            applied,
        )
        .into_iter()
        .map(|resolution| {
            let packet = resolution.packet.clone();
            concept_packet_view(
                self.prism.as_ref(),
                packet,
                verbosity,
                args.include_binding_metadata.unwrap_or(false),
                Some(resolution),
            )
        })
        .collect::<Vec<_>>();
        if concepts.is_empty() {
            self.push_diagnostic(
                "anchor_unresolved",
                format!("No concept packet matched `{}`.", args.query),
                Some(json!({ "query": args.query })),
            );
        }
        Ok(concepts)
    }

    fn concept(&self, args: ConceptQueryArgs) -> Result<Option<ConceptPacketView>> {
        let verbosity = parse_concept_verbosity(
            args.verbosity.as_deref(),
            "prism.concept",
            ConceptVerbosity::Standard,
        )?;
        let resolutions = resolve_concepts_for_session(
            self.prism.as_ref(),
            self.session.as_ref(),
            &args.query,
            3,
        );
        if concept_resolution_is_ambiguous(&resolutions) {
            self.push_diagnostic(
                "ambiguous_concept",
                format!(
                    "Concept query `{}` matched multiple plausible concepts.",
                    args.query
                ),
                Some(json!({
                    "query": args.query,
                    "candidates": resolutions
                        .iter()
                        .take(3)
                        .map(|resolution| json!({
                            "handle": resolution.packet.handle,
                            "score": resolution.score,
                            "reasons": resolution.reasons,
                        }))
                        .collect::<Vec<_>>(),
                })),
            );
        }
        if let Some(reason) = resolutions
            .first()
            .and_then(|resolution| weak_concept_match_reason(resolution.score))
        {
            self.push_diagnostic(
                "weak_concept_match",
                format!("Concept query `{}` resolved weakly: {reason}.", args.query),
                Some(json!({
                    "query": args.query,
                    "reason": reason,
                    "score": resolutions.first().map(|resolution| resolution.score),
                })),
            );
        }
        let concept = resolutions.into_iter().next().map(|resolution| {
            let packet = resolution.packet.clone();
            concept_packet_view(
                self.prism.as_ref(),
                packet,
                verbosity,
                args.include_binding_metadata.unwrap_or(false),
                Some(resolution),
            )
        });
        if concept.is_none() {
            self.push_diagnostic(
                "anchor_unresolved",
                format!("No concept packet matched `{}`.", args.query),
                Some(json!({ "query": args.query })),
            );
        }
        Ok(concept)
    }

    fn concept_by_handle(&self, args: ConceptHandleArgs) -> Result<Option<ConceptPacketView>> {
        let verbosity = parse_concept_verbosity(
            args.verbosity.as_deref(),
            "prism.conceptByHandle",
            ConceptVerbosity::Standard,
        )?;
        let concept = self.prism.concept_by_handle(&args.handle).map(|packet| {
            concept_packet_view(
                self.prism.as_ref(),
                packet,
                verbosity,
                args.include_binding_metadata.unwrap_or(false),
                None,
            )
        });
        if concept.is_none() {
            self.push_diagnostic(
                "anchor_unresolved",
                format!("No concept packet matched `{}`.", args.handle),
                Some(json!({ "handle": args.handle })),
            );
        }
        Ok(concept)
    }

    fn contract(&self, args: ContractQueryArgs) -> Result<Option<ContractPacketView>> {
        let resolutions = self.prism.resolve_contracts(&args.query, 3);
        if contract_resolution_is_ambiguous(&resolutions) {
            self.push_diagnostic(
                "ambiguous_contract",
                format!(
                    "Contract query `{}` matched multiple plausible contracts.",
                    args.query
                ),
                Some(json!({
                    "query": args.query,
                    "candidates": resolutions
                        .iter()
                        .take(3)
                        .map(|resolution| json!({
                            "handle": resolution.packet.handle,
                            "score": resolution.score,
                            "reasons": resolution.reasons,
                        }))
                        .collect::<Vec<_>>(),
                })),
            );
        }
        let contract = resolutions.into_iter().next().map(|resolution| {
            let packet = resolution.packet.clone();
            contract_packet_view(
                self.prism.as_ref(),
                self.workspace_root(),
                packet,
                Some(resolution),
            )
        });
        if contract.is_none() {
            self.push_diagnostic(
                "anchor_unresolved",
                format!("No contract packet matched `{}`.", args.query),
                Some(json!({ "query": args.query })),
            );
        }
        Ok(contract)
    }

    fn contracts(&self, args: ContractsQueryArgs) -> Result<Vec<ContractPacketView>> {
        let status = args
            .status
            .as_deref()
            .map(parse_contract_status_filter)
            .transpose()?;
        let scope = args
            .scope
            .as_deref()
            .map(parse_contract_scope_filter)
            .transpose()?;
        let kind = args
            .kind
            .as_deref()
            .map(parse_contract_kind_filter)
            .transpose()?;
        let requested = args.limit.unwrap_or(DEFAULT_SEARCH_LIMIT);
        let applied = requested.min(self.session.limits().max_result_nodes);
        let mut results = if let Some(query) = args.contains.as_deref() {
            self.prism
                .resolve_contracts(query, self.session.limits().max_result_nodes)
                .into_iter()
                .map(|resolution| {
                    let packet = resolution.packet.clone();
                    contract_packet_view(
                        self.prism.as_ref(),
                        self.workspace_root(),
                        packet,
                        Some(resolution),
                    )
                })
                .collect::<Vec<_>>()
        } else {
            self.prism
                .curated_contracts()
                .into_iter()
                .map(|packet| {
                    contract_packet_view(self.prism.as_ref(), self.workspace_root(), packet, None)
                })
                .collect::<Vec<_>>()
        };
        results.retain(|contract| {
            status.is_none_or(|value| {
                crate::host_resources::contract_status_label(&contract.status) == value
            }) && scope.is_none_or(|value| {
                crate::host_resources::contract_scope_label(&contract.scope) == value
            }) && kind.is_none_or(|value| {
                crate::host_resources::contract_kind_label(&contract.kind) == value
            })
        });
        if requested > applied {
            self.push_diagnostic(
                "result_truncated",
                format!(
                    "Contract-list limit was capped at {} instead of {requested}. Next action: narrow with `status`, `scope`, `kind`, or `contains` before raising the limit.",
                    applied
                ),
                Some(json!({
                    "requested": requested,
                    "applied": applied,
                    "nextAction": "Use prism.contracts({ status: ..., scope: ..., kind: ..., contains: ..., limit: ... }) to narrow the result set.",
                })),
            );
        }
        if results.len() > applied {
            let total = results.len();
            results.truncate(applied);
            self.push_diagnostic(
                "result_truncated",
                format!(
                    "Contract discovery results were truncated at {} entries. Next action: narrow with `status`, `scope`, `kind`, or `contains`, then inspect one contract with `prism.contract(...)`.",
                    applied
                ),
                Some(json!({
                    "count": total,
                    "applied": applied,
                    "nextAction": "Use prism.contract(query) after narrowing prism.contracts(...).",
                })),
            );
        }
        Ok(results)
    }

    fn contracts_for(&self, args: SymbolTargetArgs) -> Result<Vec<ContractPacketView>> {
        let id = self.resolve_target_id(args.id, args.lineage_id)?;
        Ok(self
            .prism
            .contracts_for_target(&id)
            .into_iter()
            .map(|packet| {
                contract_packet_view(self.prism.as_ref(), self.workspace_root(), packet, None)
            })
            .collect())
    }

    fn specs(&self) -> Result<Vec<prism_js::SpecListEntryView>> {
        crate::spec_surface::list_specs(&self.host)
    }

    fn spec(&self, args: SpecIdArgs) -> Result<Option<prism_js::SpecDocumentView>> {
        let spec = crate::spec_surface::spec_document(&self.host, &args.spec_id)?;
        if spec.is_none() {
            self.push_diagnostic(
                "anchor_unresolved",
                format!("No native spec matched `{}`.", args.spec_id),
                Some(json!({ "specId": args.spec_id })),
            );
        }
        Ok(spec)
    }

    fn spec_sync_brief(&self, args: SpecIdArgs) -> Result<Option<prism_js::SpecSyncBriefView>> {
        let brief = crate::spec_surface::spec_sync_brief(&self.host, &args.spec_id)?;
        if brief.is_none() {
            self.push_diagnostic(
                "anchor_unresolved",
                format!("No native spec matched `{}`.", args.spec_id),
                Some(json!({ "specId": args.spec_id })),
            );
        }
        Ok(brief)
    }

    fn spec_coverage(&self, args: SpecIdArgs) -> Result<Vec<prism_js::SpecCoverageRecordView>> {
        crate::spec_surface::spec_coverage(&self.host, &args.spec_id)
    }

    fn spec_sync_provenance(
        &self,
        args: SpecIdArgs,
    ) -> Result<Vec<prism_js::SpecSyncProvenanceRecordView>> {
        crate::spec_surface::spec_sync_provenance(&self.host, &args.spec_id)
    }

    fn concept_relations(
        &self,
        args: ConceptHandleArgs,
    ) -> Result<Vec<prism_js::ConceptRelationView>> {
        if self.prism.concept_by_handle(&args.handle).is_none() {
            self.push_diagnostic(
                "anchor_unresolved",
                format!("No concept packet matched `{}`.", args.handle),
                Some(json!({ "handle": args.handle })),
            );
            return Ok(Vec::new());
        }
        Ok(self
            .prism
            .concept_relations_for_handle(&args.handle)
            .into_iter()
            .map(|relation| concept_relation_view(self.prism.as_ref(), &args.handle, relation))
            .collect())
    }

    fn decode_concept(&self, args: DecodeConceptArgs) -> Result<Option<ConceptDecodeView>> {
        let lens = parse_concept_lens(&args.lens)?;
        let verbosity = parse_concept_verbosity(
            args.verbosity.as_deref(),
            "prism.decodeConcept",
            ConceptVerbosity::Standard,
        )?;
        let packet: Option<prism_query::ConceptPacket> =
            match (args.handle.as_deref(), args.query.as_deref()) {
                (Some(handle), _) => self.prism.concept_by_handle(handle),
                (None, Some(query)) => resolve_concepts_for_session(
                    self.prism.as_ref(),
                    self.session.as_ref(),
                    query,
                    1,
                )
                .into_iter()
                .next()
                .map(|resolution| resolution.packet),
                (None, None) => {
                    return Err(anyhow!("decodeConcept requires either `handle` or `query`"))
                }
            };
        let Some(packet) = packet else {
            let subject = args
                .handle
                .or(args.query)
                .unwrap_or_else(|| "concept".to_string());
            self.push_diagnostic(
                "anchor_unresolved",
                format!("No concept packet matched `{subject}`."),
                Some(json!({ "subject" : subject })),
            );
            return Ok(None);
        };

        let concept_view = concept_packet_view(
            self.prism.as_ref(),
            packet.clone(),
            verbosity,
            args.include_binding_metadata.unwrap_or(false),
            None,
        );
        let members = symbol_views_for_ids(self.prism.as_ref(), packet.core_members.clone())?;
        let supporting_reads =
            symbol_views_for_ids(self.prism.as_ref(), packet.supporting_members.clone())?;
        let likely_tests = symbol_views_for_ids(self.prism.as_ref(), packet.likely_tests.clone())?;
        let primary = members.first().cloned();
        let anchors = self.prism.anchors_for(
            &packet
                .core_members
                .iter()
                .cloned()
                .map(AnchorRef::Node)
                .collect::<Vec<_>>(),
        );
        let recent_failures = self.prism.query_outcomes(&OutcomeRecallQuery {
            anchors: anchors.clone(),
            kinds: Some(vec![OutcomeKind::FailureObserved]),
            limit: 8,
            ..OutcomeRecallQuery::default()
        });
        let related_memory = self
            .session
            .notes
            .recall(&RecallQuery {
                focus: anchors,
                limit: 4,
                ..RecallQuery::default()
            })?
            .into_iter()
            .map(scored_memory_view)
            .collect::<Vec<_>>();
        let recent_patches = collect_concept_patches(self.prism.as_ref(), &packet.core_members, 4)?;
        let validation_recipe = packet.core_members.first().map(|primary_id| {
            validation_recipe_view_with(self.prism.as_ref(), self.session.as_ref(), primary_id)
        });

        Ok(Some(ConceptDecodeView {
            concept: concept_view,
            lens: concept_decode_lens_view(lens),
            primary,
            members,
            supporting_reads,
            likely_tests,
            recent_failures,
            related_memory,
            recent_patches,
            validation_recipe,
            evidence: packet.evidence,
        }))
    }

    fn memory_outcomes(&self, args: MemoryOutcomeArgs) -> Result<Vec<prism_memory::OutcomeEvent>> {
        let requested = args.limit.unwrap_or(10);
        let limits = self.session.limits();
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

    pub(crate) fn resolve_target_id(
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

fn suggest_query_operation(operation: &str) -> Option<String> {
    let normalized = normalize_operation_label(operation);
    let mut best: Option<(&str, usize)> = None;
    for (candidate, _, _, _) in query_method_specs() {
        let distance = levenshtein_distance(&normalized, &normalize_operation_label(candidate));
        match best {
            Some((_, best_distance)) if distance >= best_distance => {}
            _ => best = Some((candidate, distance)),
        }
    }
    let (candidate, distance) = best?;
    let threshold = normalized.len().max(6) / 3;
    (distance <= threshold.max(2)).then(|| candidate.to_string())
}

fn normalize_operation_label(value: &str) -> String {
    value
        .chars()
        .filter(|ch| ch.is_ascii_alphanumeric())
        .flat_map(|ch| ch.to_lowercase())
        .collect()
}

fn levenshtein_distance(left: &str, right: &str) -> usize {
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

fn parse_concept_lens(value: &str) -> Result<ConceptDecodeLens> {
    match value.trim().to_ascii_lowercase().as_str() {
        "open" => Ok(ConceptDecodeLens::Open),
        "workset" => Ok(ConceptDecodeLens::Workset),
        "validation" => Ok(ConceptDecodeLens::Validation),
        "timeline" => Ok(ConceptDecodeLens::Timeline),
        "memory" => Ok(ConceptDecodeLens::Memory),
        other => Err(invalid_query_argument_error(
            "prism.concept",
            format!("unknown concept lens `{other}`"),
        )),
    }
}

fn parse_concept_verbosity(
    value: Option<&str>,
    operation: &str,
    default: ConceptVerbosity,
) -> Result<ConceptVerbosity> {
    let normalized = value
        .map(|value| value.trim().to_ascii_lowercase())
        .unwrap_or_else(|| match default {
            ConceptVerbosity::Summary => "summary".to_string(),
            ConceptVerbosity::Standard => "standard".to_string(),
            ConceptVerbosity::Full => "full".to_string(),
        });
    match normalized.as_str() {
        "summary" => Ok(ConceptVerbosity::Summary),
        "standard" => Ok(ConceptVerbosity::Standard),
        "full" => Ok(ConceptVerbosity::Full),
        other => Err(invalid_query_argument_error(
            operation,
            format!(
                "unknown concept verbosity `{other}`; expected `summary`, `standard`, or `full`"
            ),
        )),
    }
}

fn collect_concept_patches(
    prism: &Prism,
    members: &[NodeId],
    limit: usize,
) -> Result<Vec<PatchEventView>> {
    let mut patches = Vec::<PatchEventView>::new();
    for member in members {
        for patch in recent_patches(prism, Some(member), None, None, None, limit)? {
            if patches
                .iter()
                .any(|existing| existing.event_id == patch.event_id)
            {
                continue;
            }
            patches.push(patch);
            if patches.len() >= limit {
                return Ok(patches);
            }
        }
    }
    Ok(patches)
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
        other => Err(invalid_query_argument_error(
            "prism.search",
            format!("unsupported search pathMode `{other}`; expected `contains` or `exact`"),
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

fn validation_feedback_contains(entry: &prism_core::ValidationFeedbackEntry, needle: &str) -> bool {
    entry.context.to_ascii_lowercase().contains(needle)
        || entry.prism_said.to_ascii_lowercase().contains(needle)
        || entry.actually_true.to_ascii_lowercase().contains(needle)
        || entry
            .correction
            .as_ref()
            .is_some_and(|value| value.to_ascii_lowercase().contains(needle))
        || entry
            .task_id
            .as_ref()
            .is_some_and(|value| value.to_ascii_lowercase().contains(needle))
        || entry
            .metadata
            .to_string()
            .to_ascii_lowercase()
            .contains(needle)
}

fn validation_feedback_view(entry: prism_core::ValidationFeedbackEntry) -> ValidationFeedbackView {
    ValidationFeedbackView {
        id: entry.id,
        recorded_at: entry.recorded_at,
        task_id: entry.task_id,
        context: entry.context,
        anchors: entry.anchors,
        prism_said: entry.prism_said,
        actually_true: entry.actually_true,
        category: entry.category.to_string(),
        verdict: entry.verdict.to_string(),
        corrected_manually: entry.corrected_manually,
        correction: entry.correction,
        metadata: entry.metadata,
    }
}
