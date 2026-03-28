use std::collections::HashSet;
use std::path::Path;
use std::sync::Arc;
use std::time::Instant;

use anyhow::{anyhow, Result};
use globset::{GlobBuilder, GlobMatcher};
use prism_ir::{LineageId, NodeId, NodeKind};
use prism_js::{
    AgentExpandKind, AgentExpandResultView, AgentGatherResultView, AgentLocateResultView,
    AgentLocateStatus, AgentOpenMode, AgentOpenResultView, AgentTargetHandleView,
    AgentTextPreviewView, AgentWorksetResultView, QueryDiagnostic, SourceExcerptView,
    SourceLocationView, SourceSliceView, SymbolView, TextSearchMatchView,
};
use prism_query::{EditSliceOptions, Prism};
use serde_json::{json, Value};

use crate::file_queries::file_read;
use crate::session_state::SessionHandleTarget;
use crate::text_search::search_text;
use crate::{
    diff_for, focused_block_for_symbol, next_reads, owner_views_for_target,
    spec_drift_explanation_view, symbol_for, symbol_view, validation_context_view_cached,
    FileReadArgs, PrismExpandArgs, PrismExpandKindInput, PrismGatherArgs, PrismLocateArgs,
    PrismLocateTaskIntentInput, PrismOpenArgs, PrismOpenModeInput, PrismWorksetArgs, QueryHost,
    QueryRun, SearchArgs, SearchTextArgs, SessionState,
};

const DEFAULT_LOCATE_LIMIT: usize = 3;
const MAX_LOCATE_LIMIT: usize = 3;
const LOCATE_BACKEND_MULTIPLIER: usize = 6;
const DEFAULT_GATHER_LIMIT: usize = 3;
const MAX_GATHER_LIMIT: usize = 3;
const FOCUS_OPEN_OPTIONS: EditSliceOptions = EditSliceOptions {
    before_lines: 1,
    after_lines: 1,
    max_lines: 10,
    max_chars: 480,
};
const EDIT_OPEN_OPTIONS: EditSliceOptions = EditSliceOptions {
    before_lines: 1,
    after_lines: 1,
    max_lines: 8,
    max_chars: 360,
};
const PREVIEW_OPEN_OPTIONS: EditSliceOptions = EditSliceOptions {
    before_lines: 0,
    after_lines: 0,
    max_lines: 5,
    max_chars: 220,
};
const OPEN_RELATED_HANDLE_LIMIT: usize = 2;
const OPEN_MAX_JSON_BYTES: usize = 1400;
const RAW_OPEN_MAX_CHARS: usize = 720;
const WORKSET_SUPPORTING_LIMIT: usize = 3;
const WORKSET_TEST_LIMIT: usize = 2;
pub(crate) const WORKSET_MAX_JSON_BYTES: usize = 1024;
const WORKSET_WHY_MAX_CHARS: usize = 160;
const WORKSET_WHY_TIGHT_MAX_CHARS: usize = 72;
const WORKSET_TRUNCATED_NEXT_ACTION: &str =
    "Rerun prism_expand with kind `neighbors` or `validation` for more context.";
const EXPAND_NEIGHBOR_LIMIT: usize = 6;
const EXPAND_DIFF_LIMIT: usize = 5;
const EXPAND_DRIFT_NEXT_READ_LIMIT: usize = 3;
const EXPAND_DRIFT_LIST_LIMIT: usize = 3;
const EXPAND_DRIFT_TEXT_MAX_CHARS: usize = 120;
const EXPAND_DRIFT_MAX_JSON_BYTES: usize = 1400;
const MAX_WHY_SHORT_CHARS: usize = 120;
const LOCATE_SECONDARY_FILE_DIVERSITY_BONUS: i32 = 18;
const LOCATE_SECONDARY_KIND_DIVERSITY_BONUS: i32 = 7;
const TEXT_FRAGMENT_CRATE_NAME: &str = "__prism_text__";
const TEXT_LOCATE_LIMIT_MULTIPLIER: usize = 4;
const TEXT_FRAGMENT_RELATED_LIMIT: usize = 3;

#[derive(Debug, Clone)]
struct RankedLocateCandidate {
    target: RankedLocateTarget,
    score: i32,
    why: String,
}

#[derive(Debug, Clone)]
enum RankedLocateTarget {
    Symbol(SymbolView),
    Text(SessionHandleTarget),
}

#[derive(Debug, Clone, Copy)]
struct LocateIntentProfile {
    code_bias: i32,
    docs_bias: i32,
    test_penalty: i32,
}

#[derive(Debug)]
struct WorksetContext {
    supporting_reads: Vec<AgentTargetHandleView>,
    likely_tests: Vec<AgentTargetHandleView>,
    why: String,
}

#[derive(Debug)]
struct TextSearchCandidate {
    target: SessionHandleTarget,
    matched_text: String,
}

impl QueryHost {
    pub(crate) fn compact_locate(
        &self,
        session: Arc<SessionState>,
        args: PrismLocateArgs,
    ) -> Result<AgentLocateResultView> {
        let query_text = format!("prism_locate({})", args.query);
        self.execute_compact_tool(
            Arc::clone(&session),
            "prism_locate",
            query_text,
            move |host, query_run| {
                let prism = host.current_prism();
                let execution = crate::QueryExecution::new(
                    host.clone(),
                    Arc::clone(&session),
                    Arc::clone(&prism),
                    query_run,
                );
                let mut results = execution.search(compact_search_args(&session, &args))?;
                apply_locate_glob_filter(
                    &mut results,
                    host.workspace.as_ref().map(|workspace| workspace.root()),
                    args.glob.as_deref(),
                )?;

                let applied = compact_locate_limit(args.limit);
                let text_candidates = locate_text_candidates(
                    host,
                    session.as_ref(),
                    &args,
                    applied.saturating_mul(TEXT_LOCATE_LIMIT_MULTIPLIER),
                )?;
                results.extend(semantic_symbols_from_text_candidates(
                    prism.as_ref(),
                    &text_candidates,
                    applied.saturating_mul(TEXT_LOCATE_LIMIT_MULTIPLIER),
                )?);
                dedupe_locate_symbols(&mut results);
                let ranked = rerank_locate_results(results, text_candidates, &args, applied);
                let mut diagnostics = execution.diagnostics();
                diagnostics.extend(locate_text_diagnostics(&ranked, applied));
                let resolved_confidently = locate_resolved_confidently(&ranked, &diagnostics);
                let top_preview = if args.include_top_preview.unwrap_or(false) {
                    if let Some(candidate) = ranked.first() {
                        let preview_handle = compact_ranked_target_view(
                            &session,
                            &candidate.target,
                            Some(args.query.as_str()),
                            Some(candidate.why.clone()),
                        );
                        compact_preview_for_ranked_target(
                            host,
                            prism.as_ref(),
                            &preview_handle.handle,
                            &candidate.target,
                        )?
                    } else {
                        None
                    }
                } else {
                    None
                };
                let candidates = ranked
                    .into_iter()
                    .map(|candidate| {
                        compact_ranked_target_view(
                            &session,
                            &candidate.target,
                            Some(args.query.as_str()),
                            Some(candidate.why),
                        )
                    })
                    .collect::<Vec<_>>();
                Ok((
                    build_locate_result(
                        candidates,
                        diagnostics.clone(),
                        resolved_confidently,
                        top_preview,
                    ),
                    diagnostics,
                ))
            },
        )
    }

    pub(crate) fn compact_gather(
        &self,
        session: Arc<SessionState>,
        args: PrismGatherArgs,
    ) -> Result<AgentGatherResultView> {
        let query_text = format!("prism_gather({})", args.query);
        self.execute_compact_tool(
            Arc::clone(&session),
            "prism_gather",
            query_text,
            move |host, _query_run| {
                let (matches, truncated) = gather_text_matches(host, session.as_ref(), &args)?;
                Ok((
                    AgentGatherResultView {
                        matches,
                        truncated,
                        narrowing_hint: truncated.then_some(
                            "Narrow prism_gather with `path` or `glob`, or select one handle and continue with prism_open.".to_string(),
                        ),
                    },
                    Vec::new(),
                ))
            },
        )
    }

    pub(crate) fn compact_open(
        &self,
        session: Arc<SessionState>,
        args: PrismOpenArgs,
    ) -> Result<AgentOpenResultView> {
        let mode = agent_open_mode(args.mode.as_ref());
        let query_text = format!("prism_open({}, {:?})", args.handle, mode);
        self.execute_compact_tool(
            Arc::clone(&session),
            "prism_open",
            query_text,
            move |host, _query_run| {
                let prism = host.current_prism();
                let (target, remapped) =
                    resolve_handle_target(session.as_ref(), prism.as_ref(), &args.handle)?;
                let result = if is_text_fragment_target(&target) {
                    compact_open_text_fragment(
                        host,
                        session.as_ref(),
                        &args.handle,
                        mode,
                        &target,
                        remapped,
                    )?
                } else {
                    let symbol_id = target_symbol_id(&target)?;
                    let symbol = symbol_for(prism.as_ref(), symbol_id)?;
                    let symbol_view = symbol_view(prism.as_ref(), &symbol)?;
                    let file_path = symbol_view
                        .file_path
                        .clone()
                        .ok_or_else(|| anyhow!("target `{}` has no workspace file path", target.id.path))?;
                    let related_handles =
                        compact_open_related_handles(host, session.as_ref(), prism.as_ref(), &target)?;

                    match mode {
                        AgentOpenMode::Focus => {
                            let block = focused_block_for_symbol(prism.as_ref(), &symbol, FOCUS_OPEN_OPTIONS)?;
                            compact_open_result_from_block(
                                &args.handle,
                                &file_path,
                                block.slice,
                                block.excerpt,
                                remapped,
                                "Rerun prism_open with mode `raw` if you need the exact file window.",
                                related_handles.clone(),
                            )?
                        }
                        AgentOpenMode::Edit => {
                            let slice = symbol
                                .edit_slice(EDIT_OPEN_OPTIONS)
                                .map(source_slice_view)
                                .ok_or_else(|| anyhow!("target `{}` did not produce an edit slice", target.id.path))?;
                            compact_open_result_from_slice(
                                &args.handle,
                                &file_path,
                                slice,
                                remapped,
                                "Rerun prism_open with mode `raw` if you need the exact file window.",
                                related_handles.clone(),
                            )?
                        }
                        AgentOpenMode::Raw => {
                            let location = symbol
                                .location()
                                .ok_or_else(|| anyhow!("target `{}` has no line-addressable source location", target.id.path))?;
                            let excerpt = file_read(
                                host,
                                FileReadArgs {
                                    path: file_path.clone(),
                                    start_line: Some(location.start_line),
                                    end_line: Some(location.end_line),
                                    max_chars: Some(RAW_OPEN_MAX_CHARS),
                                },
                            )?;
                            compact_open_result_from_excerpt(
                                &args.handle,
                                &file_path,
                                excerpt,
                                remapped,
                                "Rerun prism_open with a narrower target if you need a smaller raw window.",
                                related_handles.clone(),
                            )?
                        }
                    }
                };
                Ok((result, Vec::new()))
            },
        )
    }

    pub(crate) fn compact_workset(
        &self,
        session: Arc<SessionState>,
        args: PrismWorksetArgs,
    ) -> Result<AgentWorksetResultView> {
        let query_text = if let Some(handle) = args.handle.as_ref() {
            format!("prism_workset({handle})")
        } else if let Some(query) = args.query.as_ref() {
            format!("prism_workset({query})")
        } else {
            "prism_workset".to_string()
        };
        self.execute_compact_tool(
            Arc::clone(&session),
            "prism_workset",
            query_text,
            move |host, query_run| {
                let prism = host.current_prism();
                let (target, remapped) = resolve_or_select_workset_target(
                    host,
                    Arc::clone(&session),
                    prism.as_ref(),
                    &args,
                    query_run,
                )?;
                let target_view = compact_target_from_session_target(session.as_ref(), &target);
                let workset =
                    workset_context_for_target(host, session.as_ref(), prism.as_ref(), &target)?;
                Ok((
                    budgeted_workset_result(
                        target_view,
                        workset.supporting_reads,
                        workset.likely_tests,
                        workset.why,
                        remapped,
                    )?,
                    Vec::new(),
                ))
            },
        )
    }

    pub(crate) fn compact_expand(
        &self,
        session: Arc<SessionState>,
        args: PrismExpandArgs,
    ) -> Result<AgentExpandResultView> {
        let kind = agent_expand_kind(&args.kind);
        let query_text = format!("prism_expand({}, {:?})", args.handle, kind);
        self.execute_compact_tool(
            Arc::clone(&session),
            "prism_expand",
            query_text,
            move |host, _query_run| {
                let prism = host.current_prism();
                let (target, remapped) =
                    resolve_handle_target(session.as_ref(), prism.as_ref(), &args.handle)?;
                let mut top_preview = None;
                let result = match kind {
                    AgentExpandKind::Diagnostics => {
                        if is_text_fragment_target(&target) {
                            compact_text_fragment_diagnostics(&target)
                        } else {
                            let symbol = symbol_for(prism.as_ref(), target_symbol_id(&target)?)?;
                            let symbol = symbol_view(prism.as_ref(), &symbol)?;
                            json!({
                                "query": target.query,
                                "whyShort": target.why_short,
                                "filePath": target.file_path,
                                "ownerHint": symbol.owner_hint,
                            })
                        }
                    }
                    AgentExpandKind::Lineage => {
                        if is_text_fragment_target(&target) {
                            json!({
                                "note": "Lineage is only available for semantic symbol handles. Rerun prism_locate on a symbol target if you need lineage.",
                                "filePath": target.file_path,
                            })
                        } else {
                            serde_json::to_value(crate::lineage_view(
                                prism.as_ref(),
                                target_symbol_id(&target)?,
                            )?)?
                        }
                    }
                    AgentExpandKind::Neighbors => {
                        if is_text_fragment_target(&target) {
                            let (neighbors, preview) = compact_text_fragment_neighbors(
                                host,
                                session.as_ref(),
                                &target,
                                args.include_top_preview.unwrap_or(false),
                            )?;
                            top_preview = preview;
                            json!({ "neighbors": neighbors })
                        } else {
                            let next_read_candidates =
                                next_reads(prism.as_ref(), target_symbol_id(&target)?, EXPAND_NEIGHBOR_LIMIT)?;
                            if args.include_top_preview.unwrap_or(false) {
                                top_preview = if let Some(candidate) = next_read_candidates.first() {
                                    let preview_handle = compact_target_view(
                                        &session,
                                        &candidate.symbol,
                                        target.query.as_deref(),
                                        Some(candidate.why.clone()),
                                    );
                                    compact_preview_for_symbol_view(
                                        prism.as_ref(),
                                        &preview_handle.handle,
                                        &candidate.symbol,
                                    )?
                                } else {
                                    None
                                };
                            }
                            let neighbors = next_read_candidates
                                .into_iter()
                                .take(EXPAND_NEIGHBOR_LIMIT)
                                .map(|candidate| {
                                    compact_target_view(
                                        &session,
                                        &candidate.symbol,
                                        target.query.as_deref(),
                                        Some(candidate.why),
                                    )
                                })
                                .collect::<Vec<_>>();
                            json!({ "neighbors": neighbors })
                        }
                    }
                    AgentExpandKind::Diff => {
                        if is_text_fragment_target(&target) {
                            json!({
                                "note": "Diff expansion is only available for semantic symbol handles. Rerun prism_locate on a symbol target if you need a semantic diff.",
                                "filePath": target.file_path,
                            })
                        } else {
                            let lineage = target
                                .lineage_id
                                .as_ref()
                                .map(|value| LineageId::new(value.clone()));
                            serde_json::to_value(diff_for(
                                prism.as_ref(),
                                Some(target_symbol_id(&target)?),
                                lineage.as_ref(),
                                None,
                                None,
                                EXPAND_DIFF_LIMIT,
                            )?)?
                        }
                    }
                    AgentExpandKind::Validation => {
                        if is_text_fragment_target(&target) {
                            compact_text_fragment_validation(host, session.as_ref(), &target)?
                        } else {
                            let mut cache = crate::SemanticContextCache::default();
                            let validation = validation_context_view_cached(
                                prism.as_ref(),
                                session.as_ref(),
                                &mut cache,
                                target_symbol_id(&target)?,
                            )?;
                            let likely_tests = validation
                                .tests
                                .into_iter()
                                .take(WORKSET_TEST_LIMIT)
                                .map(|candidate| {
                                    compact_target_view(
                                        &session,
                                        &candidate.symbol,
                                        target.query.as_deref(),
                                        Some(candidate.why),
                                    )
                                })
                                .collect::<Vec<_>>();
                            json!({
                                "checks": validation.validation_recipe.checks,
                                "likelyTests": likely_tests,
                                "why": validation.why,
                            })
                        }
                    }
                    AgentExpandKind::Drift => {
                        if is_text_fragment_target(&target) {
                            json!({
                                "note": "Drift expansion needs a semantic spec/doc handle. Rerun prism_locate on a heading or symbol target if you need drift details.",
                                "filePath": target.file_path,
                            })
                        } else {
                            compact_drift_expand_result(session.as_ref(), prism.as_ref(), &target)?
                        }
                    }
                };

                Ok((
                    AgentExpandResultView {
                        handle: args.handle,
                        kind,
                        result,
                        remapped,
                        top_preview,
                    },
                    Vec::new(),
                ))
            },
        )
    }

    fn execute_compact_tool<T, F>(
        &self,
        session: Arc<SessionState>,
        kind: &str,
        query_text: String,
        build: F,
    ) -> Result<T>
    where
        T: serde::Serialize,
        F: FnOnce(&QueryHost, QueryRun) -> Result<(T, Vec<QueryDiagnostic>)>,
    {
        let query_run = self.begin_query_run(session.as_ref(), kind, query_text);
        match (|| -> Result<(T, Vec<QueryDiagnostic>, usize)> {
            let refresh_started = Instant::now();
            let refresh = self.refresh_workspace_for_query()?;
            query_run.record_phase(
                "compact.refreshWorkspace",
                &json!({
                    "refreshPath": refresh.refresh_path,
                    "deferred": refresh.deferred,
                    "episodicReloaded": refresh.episodic_reloaded,
                    "inferenceReloaded": refresh.inference_reloaded,
                    "coordinationReloaded": refresh.coordination_reloaded,
                }),
                refresh_started.elapsed(),
                true,
                None,
            );
            let (value, diagnostics) = build(self, query_run.clone())?;
            let json_value = serde_json::to_value(&value)?;
            let json_bytes = serde_json::to_vec(&json_value)?.len();
            Ok((value, diagnostics, json_bytes))
        })() {
            Ok((value, diagnostics, json_bytes)) => {
                let result_value = serde_json::to_value(&value)?;
                query_run.finish_success(
                    self.query_log_store.as_ref(),
                    &result_value,
                    diagnostics,
                    json_bytes,
                    false,
                );
                Ok(value)
            }
            Err(error) => {
                query_run.finish_error(
                    self.query_log_store.as_ref(),
                    Vec::new(),
                    error.to_string(),
                );
                Err(error)
            }
        }
    }
}

fn compact_search_args(session: &SessionState, args: &PrismLocateArgs) -> SearchArgs {
    let applied = compact_locate_limit(args.limit);
    let backend_limit = applied
        .saturating_mul(LOCATE_BACKEND_MULTIPLIER)
        .min(session.limits().max_result_nodes);
    let (
        strategy,
        owner_kind,
        prefer_callable_code,
        prefer_editable_targets,
        prefer_behavioral_owners,
    ) = locate_intent_defaults(args.task_intent.as_ref());
    SearchArgs {
        query: args.query.clone(),
        limit: Some(backend_limit.max(applied)),
        kind: None,
        path: args.path.clone(),
        module: None,
        task_id: None,
        path_mode: None,
        strategy: Some(strategy.to_string()),
        structured_path: None,
        top_level_only: None,
        prefer_callable_code: Some(prefer_callable_code),
        prefer_editable_targets: Some(prefer_editable_targets),
        prefer_behavioral_owners: Some(prefer_behavioral_owners),
        owner_kind: owner_kind.map(str::to_string),
        include_inferred: Some(true),
    }
}

fn semantic_search_symbols(
    prism: &Prism,
    query: &str,
    kind: NodeKind,
    path: Option<&str>,
    limit: usize,
) -> Result<Vec<SymbolView>> {
    prism
        .search(query, limit, Some(kind), path)
        .into_iter()
        .map(|symbol| symbol_view(prism, &symbol))
        .collect()
}

fn semantic_symbols_from_text_candidates(
    prism: &Prism,
    candidates: &[TextSearchCandidate],
    limit: usize,
) -> Result<Vec<SymbolView>> {
    let mut promoted = Vec::new();
    let mut seen = HashSet::<String>::new();
    for candidate in candidates {
        let Some(file_path) = candidate.target.file_path.as_deref() else {
            continue;
        };
        let Some(kind) = semantic_search_kind_for_path(file_path) else {
            continue;
        };
        for query in semantic_queries_for_text_candidate(candidate) {
            for symbol in semantic_search_symbols(prism, &query, kind, Some(file_path), limit)? {
                if seen.insert(symbol.id.path.clone()) {
                    promoted.push(symbol);
                }
                if promoted.len() >= limit {
                    return Ok(promoted);
                }
            }
        }
    }
    Ok(promoted)
}

fn semantic_symbols_for_text_target(
    host: &QueryHost,
    target: &SessionHandleTarget,
    limit: usize,
) -> Result<Vec<SymbolView>> {
    let Some(file_path) = target.file_path.as_deref() else {
        return Ok(Vec::new());
    };
    let matched_text = read_text_fragment(
        host,
        target,
        target.start_line.unwrap_or(1),
        target.end_line.unwrap_or(target.start_line.unwrap_or(1)),
        PREVIEW_OPEN_OPTIONS.max_chars,
    )?
    .text;
    let pseudo_candidate = TextSearchCandidate {
        target: target.clone(),
        matched_text,
    };
    let prism = host.current_prism();
    let mut promoted =
        semantic_symbols_from_text_candidates(prism.as_ref(), &[pseudo_candidate], limit)?;
    promoted.retain(|symbol| symbol.file_path.as_deref() == Some(file_path));
    Ok(promoted)
}

fn dedupe_locate_symbols(results: &mut Vec<SymbolView>) {
    let mut seen = HashSet::<String>::new();
    results.retain(|symbol| {
        seen.insert(format!(
            "{}::{}::{:?}",
            symbol.id.crate_name, symbol.id.path, symbol.kind
        ))
    });
}

fn compact_locate_limit(requested: Option<usize>) -> usize {
    requested
        .unwrap_or(DEFAULT_LOCATE_LIMIT)
        .clamp(1, MAX_LOCATE_LIMIT)
}

fn locate_intent_defaults(
    intent: Option<&PrismLocateTaskIntentInput>,
) -> (&'static str, Option<&'static str>, bool, bool, bool) {
    match intent.unwrap_or(&PrismLocateTaskIntentInput::Edit) {
        PrismLocateTaskIntentInput::Inspect | PrismLocateTaskIntentInput::Edit => {
            ("direct", None, true, true, true)
        }
        PrismLocateTaskIntentInput::Validate | PrismLocateTaskIntentInput::Test => {
            ("behavioral", Some("test"), true, false, true)
        }
        PrismLocateTaskIntentInput::Explain => ("direct", Some("read"), false, false, true),
    }
}

fn build_locate_result(
    candidates: Vec<AgentTargetHandleView>,
    diagnostics: Vec<QueryDiagnostic>,
    resolved_confidently: bool,
    top_preview: Option<AgentTextPreviewView>,
) -> AgentLocateResultView {
    let status = if candidates.is_empty() {
        AgentLocateStatus::Empty
    } else if !resolved_confidently
        && diagnostics.iter().any(|diagnostic| {
            matches!(
                diagnostic.code.as_str(),
                "ambiguous_search" | "weak_search_match"
            )
        })
    {
        AgentLocateStatus::Ambiguous
    } else {
        AgentLocateStatus::Ok
    };
    AgentLocateResultView {
        candidates,
        status,
        truncated: diagnostics
            .iter()
            .any(|diagnostic| diagnostic.code == "result_truncated"),
        narrowing_hint: matches!(status, AgentLocateStatus::Ambiguous)
            .then(|| diagnostics.iter().find_map(next_action_hint))
            .flatten(),
        top_preview,
    }
}

fn next_action_hint(diagnostic: &QueryDiagnostic) -> Option<String> {
    diagnostic
        .data
        .as_ref()
        .and_then(|value| value.get("nextAction"))
        .and_then(Value::as_str)
        .map(ToString::to_string)
}

fn apply_locate_glob_filter(
    results: &mut Vec<SymbolView>,
    workspace_root: Option<&Path>,
    glob: Option<&str>,
) -> Result<()> {
    let Some(glob) = glob.filter(|value| !value.trim().is_empty()) else {
        return Ok(());
    };
    let matcher = GlobBuilder::new(glob)
        .literal_separator(false)
        .backslash_escape(true)
        .build()
        .map(|compiled| compiled.compile_matcher())
        .map_err(|error| anyhow!("invalid glob pattern `{glob}`: {error}"))?;
    results.retain(|symbol| locate_glob_matches(&matcher, workspace_root, symbol));
    Ok(())
}

fn locate_glob_matches(
    matcher: &GlobMatcher,
    workspace_root: Option<&Path>,
    symbol: &SymbolView,
) -> bool {
    let Some(file_path) = symbol.file_path.as_deref() else {
        return false;
    };
    if matcher.is_match(file_path) {
        return true;
    }
    workspace_root
        .and_then(|root| Path::new(file_path).strip_prefix(root).ok())
        .is_some_and(|relative| matcher.is_match(relative))
}

fn compact_target_view(
    session: &SessionState,
    symbol: &SymbolView,
    query: Option<&str>,
    why_override: Option<String>,
) -> AgentTargetHandleView {
    let why_short = compact_why_short(symbol, why_override.as_deref(), query);
    let handle = session.intern_target_handle(SessionHandleTarget {
        id: NodeId::new(
            symbol.id.crate_name.clone(),
            symbol.id.path.clone(),
            symbol.kind,
        ),
        lineage_id: symbol.lineage_id.clone(),
        name: symbol.name.clone(),
        kind: symbol.kind,
        file_path: symbol.file_path.clone(),
        query: query.map(ToString::to_string),
        why_short: why_short.clone(),
        start_line: None,
        end_line: None,
        start_column: None,
        end_column: None,
    });
    AgentTargetHandleView {
        handle,
        kind: symbol.kind,
        path: symbol.id.path.clone(),
        name: symbol.name.clone(),
        why_short,
        file_path: symbol.file_path.clone(),
    }
}

fn compact_target_from_session_target(
    session: &SessionState,
    target: &SessionHandleTarget,
) -> AgentTargetHandleView {
    let handle = session.intern_target_handle(target.clone());
    AgentTargetHandleView {
        handle,
        kind: target.kind,
        path: target.id.path.to_string(),
        name: target.name.clone(),
        why_short: target.why_short.clone(),
        file_path: target.file_path.clone(),
    }
}

fn compact_ranked_target_view(
    session: &SessionState,
    target: &RankedLocateTarget,
    query: Option<&str>,
    why_override: Option<String>,
) -> AgentTargetHandleView {
    match target {
        RankedLocateTarget::Symbol(symbol) => {
            compact_target_view(session, symbol, query, why_override)
        }
        RankedLocateTarget::Text(text_target) => {
            let mut target = text_target.clone();
            if let Some(why_short) = why_override {
                target.why_short = clamp_string(&why_short, MAX_WHY_SHORT_CHARS);
            }
            compact_target_from_session_target(session, &target)
        }
    }
}

fn rerank_locate_results(
    results: Vec<SymbolView>,
    text_candidates: Vec<TextSearchCandidate>,
    args: &PrismLocateArgs,
    limit: usize,
) -> Vec<RankedLocateCandidate> {
    let query_normalized = normalize_locate_text(&args.query);
    let tokens = locate_query_tokens(&query_normalized);
    let identifier_terms = locate_identifier_terms(&args.query);
    let path_scope = args.path.as_deref().map(str::to_ascii_lowercase);
    let profile = locate_intent_profile(args);
    let semantic_results = results.clone();
    let mut ranked = results
        .into_iter()
        .enumerate()
        .map(|(index, symbol)| {
            rank_locate_candidate(
                index,
                symbol,
                &query_normalized,
                &tokens,
                &identifier_terms,
                path_scope.as_deref(),
                profile,
            )
        })
        .collect::<Vec<_>>();
    ranked.extend(
        text_candidates
            .into_iter()
            .enumerate()
            .map(|(index, candidate)| {
                rank_text_locate_candidate(
                    index,
                    candidate,
                    &semantic_results,
                    &query_normalized,
                    &tokens,
                    &identifier_terms,
                    path_scope.as_deref(),
                    profile,
                )
            }),
    );
    ranked.sort_by(|left, right| {
        right
            .score
            .cmp(&left.score)
            .then_with(|| ranked_target_path(&left.target).cmp(ranked_target_path(&right.target)))
    });
    select_locate_candidates(ranked, limit)
}

fn rank_locate_candidate(
    index: usize,
    symbol: SymbolView,
    query_normalized: &str,
    tokens: &[String],
    identifier_terms: &[String],
    path_scope: Option<&str>,
    profile: LocateIntentProfile,
) -> RankedLocateCandidate {
    let query_uses_identifiers = !identifier_terms.is_empty();
    let name_raw = symbol.name.trim().to_ascii_lowercase();
    let path_raw = symbol.id.path.trim().to_ascii_lowercase();
    let final_segment_raw = path_raw.split("::").last().unwrap_or(path_raw.as_str());
    let file_raw = symbol
        .file_path
        .as_deref()
        .unwrap_or_default()
        .trim()
        .to_ascii_lowercase();
    let name_normalized = normalize_locate_text(&symbol.name);
    let path_normalized = normalize_locate_text(&symbol.id.path);
    let file_normalized = normalize_locate_text(symbol.file_path.as_deref().unwrap_or_default());
    let semantic_label_normalized = normalize_locate_text(semantic_match_label(&symbol));
    let mut score = 0_i32;
    let mut reasons = Vec::<String>::new();

    for term in identifier_terms {
        if name_raw == *term {
            score += 420;
            reasons.push(format!(
                "Exact identifier `{term}` matched the candidate name."
            ));
        } else if final_segment_raw == term {
            score += 360;
            reasons.push(format!(
                "Exact identifier `{term}` matched the candidate path tail."
            ));
        } else if path_raw.contains(term) {
            score += 110;
            reasons.push(format!(
                "Identifier-like query term `{term}` matched the candidate path."
            ));
        } else if file_raw.contains(term) {
            score += 84;
            reasons.push(format!(
                "Identifier-like query term `{term}` matched the candidate file path."
            ));
        }
    }

    if !query_normalized.is_empty() {
        if name_normalized == *query_normalized {
            score += 240;
            reasons.push("Exact query matched the candidate name.".to_string());
        } else if semantic_label_normalized == *query_normalized {
            score += 240;
            reasons.push("Normalized semantic label matched the query.".to_string());
        } else if final_segment_normalized(&symbol.id.path) == query_normalized {
            score += 210;
            reasons.push("Exact query matched the candidate path tail.".to_string());
        } else if name_normalized.contains(query_normalized) {
            score += if query_uses_identifiers { 60 } else { 170 };
            reasons.push("Exact query phrase matched the candidate name.".to_string());
        } else if semantic_label_normalized.contains(query_normalized) {
            score += if query_uses_identifiers { 60 } else { 170 };
            reasons.push("Normalized semantic label matched the query phrase.".to_string());
        } else if path_normalized.contains(query_normalized) {
            score += if query_uses_identifiers { 48 } else { 150 };
            reasons.push("Exact query phrase matched the candidate path.".to_string());
        } else if file_normalized.contains(query_normalized) {
            score += if query_uses_identifiers { 24 } else { 110 };
            reasons.push("Exact query phrase matched the candidate file path.".to_string());
        } else if query_normalized.ends_with(semantic_label_normalized.as_str()) {
            score += 128;
            reasons.push("Query tail matched the semantic label.".to_string());
        }
    }

    let mut matched_tokens = 0;
    for token in tokens {
        if token.is_empty() {
            continue;
        }
        if name_normalized.contains(token) {
            score += if query_uses_identifiers { 20 } else { 34 };
            matched_tokens += 1;
        } else if semantic_label_normalized.contains(token) {
            score += if query_uses_identifiers { 20 } else { 34 };
            matched_tokens += 1;
        } else if path_normalized.contains(token) {
            score += if query_uses_identifiers { 14 } else { 26 };
            matched_tokens += 1;
        } else if file_normalized.contains(token) {
            score += if query_uses_identifiers { 8 } else { 16 };
            matched_tokens += 1;
        }
    }
    if !tokens.is_empty() && matched_tokens == tokens.len() {
        score += if query_uses_identifiers { 24 } else { 72 };
        reasons.push(format!(
            "Matched all {} significant query terms.",
            tokens.len()
        ));
    } else if matched_tokens > 0 {
        reasons.push(format!(
            "Matched {matched_tokens}/{} significant query terms.",
            tokens.len()
        ));
    }

    if is_code_like_kind(symbol.kind) {
        score += profile.code_bias;
        if profile.code_bias > 0 {
            reasons.push("Locate intent favored callable or editable code.".to_string());
        }
    }
    if is_docs_like_kind(symbol.kind) {
        score += profile.docs_bias;
        if profile.docs_bias > 0 {
            reasons.push("Locate intent favored docs or structured spec surfaces.".to_string());
        }
    }
    if symbol.file_path.as_deref().is_some_and(is_docs_path) {
        score += profile.docs_bias / 2;
    }
    if path_scope.is_some_and(|scope| locate_path_scope_matches(scope, symbol.file_path.as_deref()))
    {
        score += 150;
        reasons.push("Matched the requested path scope.".to_string());
    }
    if profile.test_penalty > 0 && is_test_like_symbol(&symbol) {
        score -= profile.test_penalty;
    }
    if matches!(symbol.kind, NodeKind::Module) {
        score -= 18;
    }

    score -= index as i32;
    RankedLocateCandidate {
        target: RankedLocateTarget::Symbol(symbol),
        score,
        why: clamp_string(
            &reasons
                .into_iter()
                .next()
                .unwrap_or_else(|| "Locate ranked this as a strong first-hop target.".to_string()),
            MAX_WHY_SHORT_CHARS,
        ),
    }
}

fn rank_text_locate_candidate(
    index: usize,
    candidate: TextSearchCandidate,
    semantic_results: &[SymbolView],
    query_normalized: &str,
    tokens: &[String],
    identifier_terms: &[String],
    path_scope: Option<&str>,
    profile: LocateIntentProfile,
) -> RankedLocateCandidate {
    let query_uses_identifiers = !identifier_terms.is_empty();
    let file_raw = candidate
        .target
        .file_path
        .as_deref()
        .unwrap_or_default()
        .to_ascii_lowercase();
    let file_normalized = normalize_locate_text(&file_raw);
    let matched_text_normalized = normalize_locate_text(&candidate.matched_text);
    let mut score = if query_uses_identifiers { 245 } else { 208 };
    let mut reasons = vec![format!(
        "Exact text hit in {} near line {}.",
        candidate.target.file_path.as_deref().unwrap_or_default(),
        candidate.target.start_line.unwrap_or_default()
    )];

    for term in identifier_terms {
        if matched_text_normalized == *term {
            score += 120;
            reasons.insert(0, format!("Exact identifier `{term}` matched file text."));
            break;
        }
        if file_raw.contains(term) {
            score += 42;
        }
    }
    if !query_normalized.is_empty() && matched_text_normalized.contains(query_normalized) {
        score += if query_uses_identifiers { 84 } else { 96 };
    }
    let matched_tokens = tokens
        .iter()
        .filter(|token| {
            matched_text_normalized.contains(token.as_str())
                || file_normalized.contains(token.as_str())
        })
        .count();
    if !tokens.is_empty() && matched_tokens == tokens.len() {
        score += 38;
    } else if matched_tokens > 0 {
        score += (matched_tokens as i32) * 10;
    }
    if path_scope.is_some_and(|scope| {
        locate_path_scope_matches(scope, candidate.target.file_path.as_deref())
    }) {
        score += 150;
        reasons.insert(0, "Matched the requested path scope.".to_string());
    }
    if is_docs_like_kind(candidate.target.kind) {
        score += profile.docs_bias.max(0);
    } else if matches!(
        candidate.target.kind,
        NodeKind::JsonKey | NodeKind::TomlKey | NodeKind::YamlKey
    ) {
        score += profile.docs_bias.max(0) / 2;
    } else {
        score += profile.code_bias / 2;
    }
    if profile.test_penalty > 0
        && candidate
            .target
            .file_path
            .as_deref()
            .is_some_and(is_test_like_path)
    {
        score -= profile.test_penalty / 2;
    }
    if text_candidate_shadowed_by_semantic_result(semantic_results, &candidate, query_normalized) {
        score -= 220;
    }

    score -= index as i32;
    RankedLocateCandidate {
        target: RankedLocateTarget::Text(candidate.target),
        score,
        why: clamp_string(
            &reasons.into_iter().next().unwrap_or_else(|| {
                "Exact text hit looked like the best first-hop read.".to_string()
            }),
            MAX_WHY_SHORT_CHARS,
        ),
    }
}

fn semantic_match_label(symbol: &SymbolView) -> &str {
    if matches!(symbol.kind, NodeKind::MarkdownHeading) {
        trim_leading_section_ordinal(symbol.name.as_str())
    } else {
        symbol.name.as_str()
    }
}

fn text_candidate_shadowed_by_semantic_result(
    semantic_results: &[SymbolView],
    candidate: &TextSearchCandidate,
    query_normalized: &str,
) -> bool {
    let Some(file_path) = candidate.target.file_path.as_deref() else {
        return false;
    };
    let candidate_text = normalize_locate_text(candidate.matched_text.as_str());
    semantic_results.iter().any(|symbol| {
        symbol.file_path.as_deref() == Some(file_path)
            && is_docs_like_kind(symbol.kind)
            && semantic_result_matches_text_query(symbol, query_normalized)
            && candidate_text.contains(query_normalized)
    })
}

fn semantic_result_matches_text_query(symbol: &SymbolView, query_normalized: &str) -> bool {
    let label = normalize_locate_text(semantic_match_label(symbol));
    let path_tail = final_segment_normalized(&symbol.id.path);
    label == *query_normalized
        || label.contains(query_normalized)
        || query_normalized.ends_with(label.as_str())
        || path_tail == *query_normalized
}

fn select_locate_candidates(
    mut ranked: Vec<RankedLocateCandidate>,
    limit: usize,
) -> Vec<RankedLocateCandidate> {
    let mut selected = Vec::<RankedLocateCandidate>::new();
    while selected.len() < limit && !ranked.is_empty() {
        let best_index = ranked
            .iter()
            .enumerate()
            .max_by_key(|(_, candidate)| {
                candidate.score + locate_diversity_bonus(candidate, &selected)
            })
            .map(|(index, _)| index)
            .expect("ranked candidates should not be empty");
        selected.push(ranked.remove(best_index));
    }
    selected
}

fn locate_resolved_confidently(
    ranked: &[RankedLocateCandidate],
    diagnostics: &[QueryDiagnostic],
) -> bool {
    if ranked.is_empty() {
        return false;
    }
    if !diagnostics.iter().any(|diagnostic| {
        matches!(
            diagnostic.code.as_str(),
            "ambiguous_search" | "weak_search_match"
        )
    }) {
        return true;
    }
    let top = &ranked[0];
    let runner_up = ranked.get(1).map(|candidate| candidate.score).unwrap_or(0);
    let score_gap = top.score - runner_up;
    score_gap >= 60
        && (top.why.starts_with("Exact identifier")
            || top.why.starts_with("Exact query matched")
            || top.why.starts_with("Matched the requested path scope"))
}

fn locate_diversity_bonus(
    candidate: &RankedLocateCandidate,
    selected: &[RankedLocateCandidate],
) -> i32 {
    if selected.is_empty() {
        return 0;
    }
    let mut bonus = 0;
    let candidate_file = ranked_target_file_path(&candidate.target);
    if candidate_file.is_some()
        && selected
            .iter()
            .all(|item| ranked_target_file_path(&item.target) != candidate_file)
    {
        bonus += LOCATE_SECONDARY_FILE_DIVERSITY_BONUS;
    }
    if selected
        .iter()
        .all(|item| ranked_target_kind(&item.target) != ranked_target_kind(&candidate.target))
    {
        bonus += LOCATE_SECONDARY_KIND_DIVERSITY_BONUS;
    }
    bonus
}

fn ranked_target_path(target: &RankedLocateTarget) -> &str {
    match target {
        RankedLocateTarget::Symbol(symbol) => symbol.id.path.as_str(),
        RankedLocateTarget::Text(target) => target.id.path.as_str(),
    }
}

fn ranked_target_file_path(target: &RankedLocateTarget) -> Option<&str> {
    match target {
        RankedLocateTarget::Symbol(symbol) => symbol.file_path.as_deref(),
        RankedLocateTarget::Text(target) => target.file_path.as_deref(),
    }
}

fn ranked_target_kind(target: &RankedLocateTarget) -> NodeKind {
    match target {
        RankedLocateTarget::Symbol(symbol) => symbol.kind,
        RankedLocateTarget::Text(target) => target.kind,
    }
}

fn locate_intent_profile(args: &PrismLocateArgs) -> LocateIntentProfile {
    let docs_path_bias = args.path.as_deref().is_some_and(is_docs_path)
        || args
            .glob
            .as_deref()
            .is_some_and(|glob| glob.contains("docs/") || glob.ends_with(".md"));
    match args
        .task_intent
        .as_ref()
        .unwrap_or(&PrismLocateTaskIntentInput::Edit)
    {
        PrismLocateTaskIntentInput::Edit => LocateIntentProfile {
            code_bias: 95,
            docs_bias: if docs_path_bias { 20 } else { -80 },
            test_penalty: 110,
        },
        PrismLocateTaskIntentInput::Validate | PrismLocateTaskIntentInput::Test => {
            LocateIntentProfile {
                code_bias: 72,
                docs_bias: if docs_path_bias { 16 } else { -48 },
                test_penalty: 0,
            }
        }
        PrismLocateTaskIntentInput::Explain => LocateIntentProfile {
            code_bias: 18,
            docs_bias: if docs_path_bias { 110 } else { 58 },
            test_penalty: 72,
        },
        PrismLocateTaskIntentInput::Inspect => LocateIntentProfile {
            code_bias: 32,
            docs_bias: if docs_path_bias { 80 } else { 20 },
            test_penalty: 64,
        },
    }
}

fn locate_text_candidates(
    host: &QueryHost,
    session: &SessionState,
    args: &PrismLocateArgs,
    limit: usize,
) -> Result<Vec<TextSearchCandidate>> {
    if !should_include_text_hits(args) {
        return Ok(Vec::new());
    }
    let outcome = search_text(
        host,
        SearchTextArgs {
            query: args.query.clone(),
            regex: Some(false),
            case_sensitive: Some(false),
            path: args.path.clone(),
            glob: args.glob.clone(),
            limit: Some(limit.max(1)),
            context_lines: Some(0),
        },
        session.limits().max_result_nodes,
    )?;
    Ok(outcome
        .results
        .into_iter()
        .map(|matched| text_candidate_from_match(session, &args.query, matched))
        .collect())
}

fn should_include_text_hits(args: &PrismLocateArgs) -> bool {
    if args.path.is_some() || args.glob.is_some() {
        return true;
    }
    let query = args.query.trim();
    if query.is_empty() {
        return false;
    }
    query.contains('_')
        || query.contains('.')
        || query.contains('/')
        || query.contains(':')
        || query.contains('"')
        || query.contains('`')
        || query.contains('[')
        || query.contains(']')
        || locate_identifier_terms(query).len() == 1
}

fn locate_text_diagnostics(ranked: &[RankedLocateCandidate], limit: usize) -> Vec<QueryDiagnostic> {
    let mut diagnostics = Vec::new();
    if ranked.len() > limit {
        diagnostics.push(QueryDiagnostic {
            code: "result_truncated".to_string(),
            message: format!(
                "Locate results were compacted to {limit} targets. Narrow with `path` or `glob` if you need a smaller exact-text set."
            ),
            data: Some(json!({
                "applied": limit,
                "nextAction": "Use prism_locate with `path` or `glob`, or use prism_gather to inspect 2 to 3 exact slices directly.",
            })),
        });
    }
    diagnostics
}

fn text_candidate_from_match(
    session: &SessionState,
    query: &str,
    matched: TextSearchMatchView,
) -> TextSearchCandidate {
    let kind = text_hit_kind(&matched.path);
    let display_path = format!("{}:{}", matched.path, matched.location.start_line);
    let basename = Path::new(&matched.path)
        .file_name()
        .and_then(|value| value.to_str())
        .unwrap_or(matched.path.as_str())
        .to_string();
    let target = SessionHandleTarget {
        id: NodeId::new(TEXT_FRAGMENT_CRATE_NAME, display_path.clone(), kind),
        lineage_id: None,
        name: format!("{basename}:{}", matched.location.start_line),
        kind,
        file_path: Some(matched.path),
        query: Some(query.to_string()),
        why_short: clamp_string(
            &format!("Exact text hit for `{query}`."),
            MAX_WHY_SHORT_CHARS,
        ),
        start_line: Some(matched.location.start_line),
        end_line: Some(matched.location.end_line),
        start_column: Some(matched.location.start_column),
        end_column: Some(matched.location.end_column),
    };
    let _ = session.intern_target_handle(target.clone());
    TextSearchCandidate {
        target,
        matched_text: matched.excerpt.text,
    }
}

fn text_hit_kind(path: &str) -> NodeKind {
    if path.ends_with(".json") {
        NodeKind::JsonKey
    } else if path.ends_with(".toml") {
        NodeKind::TomlKey
    } else if path.ends_with(".yaml") || path.ends_with(".yml") {
        NodeKind::YamlKey
    } else {
        NodeKind::Document
    }
}

fn semantic_search_kind_for_path(path: &str) -> Option<NodeKind> {
    if path.ends_with(".md") {
        Some(NodeKind::MarkdownHeading)
    } else if path.ends_with(".json") {
        Some(NodeKind::JsonKey)
    } else if path.ends_with(".toml") {
        Some(NodeKind::TomlKey)
    } else if path.ends_with(".yaml") || path.ends_with(".yml") {
        Some(NodeKind::YamlKey)
    } else {
        None
    }
}

fn semantic_queries_for_text_candidate(candidate: &TextSearchCandidate) -> Vec<String> {
    let mut queries = Vec::<String>::new();
    push_unique_query(&mut queries, candidate.target.query.as_deref());
    let Some(file_path) = candidate.target.file_path.as_deref() else {
        return queries;
    };
    if file_path.ends_with(".md") {
        for query in markdown_heading_queries(candidate.matched_text.as_str()) {
            push_unique_query(&mut queries, Some(query.as_str()));
        }
    } else {
        for query in structured_key_queries(candidate.matched_text.as_str()) {
            push_unique_query(&mut queries, Some(query.as_str()));
        }
    }
    if let Some(raw_query) = candidate.target.query.as_deref() {
        for query in semantic_query_segments(raw_query) {
            push_unique_query(&mut queries, Some(query.as_str()));
        }
    }
    queries
}

fn push_unique_query(queries: &mut Vec<String>, query: Option<&str>) {
    let Some(query) = query.map(str::trim).filter(|value| !value.is_empty()) else {
        return;
    };
    if !queries.iter().any(|existing| existing == query) {
        queries.push(query.to_string());
    }
}

fn markdown_heading_queries(text: &str) -> Vec<String> {
    let Some(line) = first_non_empty_line(text) else {
        return Vec::new();
    };
    let stripped = line.trim_start().trim_start_matches('#').trim();
    let without_ordinal = trim_leading_section_ordinal(stripped);
    let mut queries = Vec::new();
    push_unique_query(&mut queries, Some(stripped));
    push_unique_query(&mut queries, Some(without_ordinal));
    queries
}

fn structured_key_queries(text: &str) -> Vec<String> {
    let Some(line) = first_non_empty_line(text).map(str::trim) else {
        return Vec::new();
    };
    let raw = if line.starts_with('[') && line.ends_with(']') {
        line.trim_start_matches('[').trim_end_matches(']').trim()
    } else if let Some((left, _)) = line.split_once('=') {
        left.trim()
    } else if let Some((left, _)) = line.split_once(':') {
        left.trim()
    } else {
        line
    };
    let cleaned = raw.trim_matches(|ch: char| ch == '"' || ch == '\'' || ch == '`');
    let mut queries = Vec::new();
    push_unique_query(&mut queries, Some(cleaned));
    for segment in semantic_query_segments(cleaned) {
        push_unique_query(&mut queries, Some(segment.as_str()));
    }
    queries
}

fn semantic_query_segments(query: &str) -> Vec<String> {
    let mut segments = Vec::<String>::new();
    for segment in query
        .split(|ch: char| !ch.is_ascii_alphanumeric() && ch != '_')
        .filter(|segment| segment.len() >= 2 && !is_locate_stopword(segment))
    {
        if !segments.iter().any(|existing| existing == segment) {
            segments.push(segment.to_string());
        }
    }
    segments.reverse();
    segments
}

fn text_fragment_query_variants(target: &SessionHandleTarget) -> Vec<String> {
    let mut queries = Vec::<String>::new();
    push_unique_query(&mut queries, target.query.as_deref());
    if let Some(query) = target.query.as_deref() {
        for segment in semantic_query_segments(query) {
            push_unique_query(&mut queries, Some(segment.as_str()));
        }
    }
    queries
}

fn first_non_empty_line(text: &str) -> Option<&str> {
    text.lines().map(str::trim).find(|line| !line.is_empty())
}

fn trim_leading_section_ordinal(text: &str) -> &str {
    let trimmed = text.trim();
    let Some(first_alpha) = trimmed.find(|ch: char| ch.is_ascii_alphabetic()) else {
        return trimmed;
    };
    let prefix = &trimmed[..first_alpha];
    if prefix.is_empty()
        || !prefix
            .chars()
            .all(|ch| ch.is_ascii_digit() || matches!(ch, '.' | ')' | '(' | '-' | ' '))
    {
        return trimmed;
    }
    trimmed[first_alpha..].trim_start()
}

fn locate_query_tokens(query_normalized: &str) -> Vec<String> {
    let mut tokens = Vec::<String>::new();
    for token in query_normalized.split_whitespace() {
        if token.len() < 2 || is_locate_stopword(token) {
            continue;
        }
        if !tokens.iter().any(|existing| existing == token) {
            tokens.push(token.to_string());
        }
    }
    tokens
}

fn locate_identifier_terms(query: &str) -> Vec<String> {
    let mut terms = Vec::<String>::new();
    for token in query.split_whitespace() {
        let token = token
            .trim_matches(|ch: char| matches!(ch, '`' | '"' | '\''))
            .trim()
            .to_ascii_lowercase();
        if token.len() < 2 || !is_identifier_like_term(&token) {
            continue;
        }
        if !terms.iter().any(|existing| existing == &token) {
            terms.push(token);
        }
    }
    terms
}

fn normalize_locate_text(value: &str) -> String {
    value
        .chars()
        .map(|ch| {
            if ch.is_ascii_alphanumeric() {
                ch.to_ascii_lowercase()
            } else {
                ' '
            }
        })
        .collect::<String>()
        .split_whitespace()
        .collect::<Vec<_>>()
        .join(" ")
}

fn final_segment_normalized(path: &str) -> String {
    normalize_locate_text(path.split("::").last().unwrap_or(path))
}

fn locate_path_scope_matches(path_scope: &str, file_path: Option<&str>) -> bool {
    let Some(file_path) = file_path else {
        return false;
    };
    let file_path = file_path.to_ascii_lowercase();
    file_path.contains(path_scope)
        || Path::new(&file_path)
            .file_name()
            .and_then(|name| name.to_str())
            .is_some_and(|name| name == path_scope)
}

fn is_locate_stopword(token: &str) -> bool {
    matches!(
        token,
        "a" | "an" | "and" | "for" | "in" | "of" | "or" | "the" | "to" | "with"
    )
}

fn is_identifier_like_term(token: &str) -> bool {
    token.contains('_')
        || token.contains("::")
        || token.contains('/')
        || token.contains('.')
        || token.contains('-')
}

fn is_test_like_symbol(symbol: &SymbolView) -> bool {
    symbol.file_path.as_deref().is_some_and(is_test_like_path)
        || symbol.id.path.to_ascii_lowercase().contains("::tests::")
}

fn is_test_like_path(path: &str) -> bool {
    let path = path.to_ascii_lowercase();
    path.contains("/tests/")
        || path.ends_with("/test.rs")
        || path.ends_with("/tests.rs")
        || path.ends_with("_test.rs")
        || path.ends_with("_tests.rs")
}

fn is_code_like_kind(kind: NodeKind) -> bool {
    matches!(
        kind,
        NodeKind::Function
            | NodeKind::Method
            | NodeKind::Struct
            | NodeKind::Enum
            | NodeKind::Trait
            | NodeKind::Impl
            | NodeKind::Field
            | NodeKind::TypeAlias
    )
}

fn is_docs_like_kind(kind: NodeKind) -> bool {
    matches!(
        kind,
        NodeKind::Document
            | NodeKind::MarkdownHeading
            | NodeKind::JsonKey
            | NodeKind::TomlKey
            | NodeKind::YamlKey
    )
}

fn is_docs_path(path: &str) -> bool {
    path.contains("/docs/") || path.starts_with("docs/") || path.ends_with(".md")
}

fn budgeted_workset_result(
    primary: AgentTargetHandleView,
    supporting_reads: Vec<AgentTargetHandleView>,
    likely_tests: Vec<AgentTargetHandleView>,
    why: String,
    remapped: bool,
) -> Result<AgentWorksetResultView> {
    let mut result = AgentWorksetResultView {
        primary,
        supporting_reads,
        likely_tests,
        why: clamp_string(&why, WORKSET_WHY_MAX_CHARS),
        truncated: false,
        remapped,
        next_action: None,
    };
    let mut trimmed = false;

    if workset_json_bytes(&result)? > WORKSET_MAX_JSON_BYTES {
        trimmed |= strip_file_paths(&mut result.supporting_reads);
        trimmed |= strip_file_paths(&mut result.likely_tests);
    }
    while workset_json_bytes(&result)? > WORKSET_MAX_JSON_BYTES && !result.likely_tests.is_empty() {
        result.likely_tests.pop();
        trimmed = true;
    }
    while workset_json_bytes(&result)? > WORKSET_MAX_JSON_BYTES
        && !result.supporting_reads.is_empty()
    {
        result.supporting_reads.pop();
        trimmed = true;
    }
    if workset_json_bytes(&result)? > WORKSET_MAX_JSON_BYTES && result.primary.file_path.is_some() {
        result.primary.file_path = None;
        trimmed = true;
    }
    if workset_json_bytes(&result)? > WORKSET_MAX_JSON_BYTES {
        let tightened = clamp_string(&result.why, WORKSET_WHY_TIGHT_MAX_CHARS);
        if tightened != result.why {
            result.why = tightened;
            trimmed = true;
        }
    }

    if trimmed {
        result.truncated = true;
        result.next_action = Some(WORKSET_TRUNCATED_NEXT_ACTION.to_string());
    }
    Ok(result)
}

fn budgeted_open_result(mut result: AgentOpenResultView) -> Result<AgentOpenResultView> {
    if let Some(related_handles) = result.related_handles.as_mut() {
        strip_file_paths(related_handles);
        if related_handles.len() > OPEN_RELATED_HANDLE_LIMIT {
            related_handles.truncate(OPEN_RELATED_HANDLE_LIMIT);
        }
    }
    while open_json_bytes(&result)? > OPEN_MAX_JSON_BYTES {
        let Some(related_handles) = result.related_handles.as_mut() else {
            break;
        };
        related_handles.pop();
        if related_handles.is_empty() {
            result.related_handles = None;
        }
    }
    Ok(result)
}

fn compact_string_list(items: &[String], limit: usize, max_chars: usize) -> Vec<String> {
    let mut compact = Vec::<String>::new();
    for item in items {
        let item = item.trim();
        if item.is_empty() {
            continue;
        }
        let item = clamp_string(item, max_chars);
        if compact.iter().any(|existing| existing == &item) {
            continue;
        }
        compact.push(item);
        if compact.len() >= limit {
            break;
        }
    }
    compact
}

fn workset_context_for_target(
    host: &QueryHost,
    session: &SessionState,
    prism: &Prism,
    target: &SessionHandleTarget,
) -> Result<WorksetContext> {
    if is_text_fragment_target(target) {
        return compact_text_fragment_workset_context(host, session, target);
    }
    if is_docs_like_kind(target.kind) || target.file_path.as_deref().is_some_and(is_docs_path) {
        return compact_spec_workset_context(session, prism, target);
    }
    Ok(WorksetContext {
        supporting_reads: next_reads(prism, target_symbol_id(target)?, WORKSET_SUPPORTING_LIMIT)?
            .into_iter()
            .take(WORKSET_SUPPORTING_LIMIT)
            .map(|candidate| {
                compact_target_view(
                    session,
                    &candidate.symbol,
                    target.query.as_deref(),
                    Some(candidate.why),
                )
            })
            .collect(),
        likely_tests: owner_views_for_target(
            prism,
            target_symbol_id(target)?,
            Some("test"),
            WORKSET_TEST_LIMIT,
        )?
        .into_iter()
        .take(WORKSET_TEST_LIMIT)
        .map(|candidate| {
            compact_target_view(
                session,
                &candidate.symbol,
                target.query.as_deref(),
                Some(candidate.why),
            )
        })
        .collect(),
        why: workset_why(target),
    })
}

fn compact_text_fragment_workset_context(
    host: &QueryHost,
    session: &SessionState,
    target: &SessionHandleTarget,
) -> Result<WorksetContext> {
    let supporting_reads =
        compact_text_fragment_supporting_reads(host, session, target, WORKSET_SUPPORTING_LIMIT)?;
    Ok(WorksetContext {
        supporting_reads,
        likely_tests: compact_text_fragment_likely_tests(
            host,
            session,
            target,
            WORKSET_TEST_LIMIT,
        )?,
        why: workset_why(target),
    })
}

fn compact_spec_workset_context(
    session: &SessionState,
    prism: &Prism,
    target: &SessionHandleTarget,
) -> Result<WorksetContext> {
    let drift = spec_drift_explanation_view(prism, &target.id)?;
    let supporting_reads =
        prioritized_spec_supporting_reads(session, target, &drift, WORKSET_SUPPORTING_LIMIT);
    let likely_tests = prioritized_spec_test_reads(session, target, &drift, WORKSET_TEST_LIMIT);
    Ok(WorksetContext {
        supporting_reads,
        likely_tests,
        why: spec_workset_why(target, &drift.gaps, &drift.drift_reasons),
    })
}

#[derive(Debug, Clone)]
struct RankedCompactFollowup {
    symbol: SymbolView,
    why: String,
    score: i32,
}

fn compact_text_fragment_supporting_reads(
    host: &QueryHost,
    session: &SessionState,
    target: &SessionHandleTarget,
    limit: usize,
) -> Result<Vec<AgentTargetHandleView>> {
    let mut supporting_reads = Vec::new();
    let mut seen = HashSet::<String>::new();
    for symbol in semantic_symbols_for_text_target(host, target, limit)? {
        if !seen.insert(symbol.id.path.clone()) {
            continue;
        }
        supporting_reads.push(compact_target_view(
            session,
            &symbol,
            target.query.as_deref(),
            Some(text_fragment_semantic_why(&symbol)),
        ));
        if supporting_reads.len() >= limit {
            return Ok(supporting_reads);
        }
    }
    if let Some(related) = compact_text_fragment_related_handles(host, session, target)? {
        for handle in related {
            if !seen.insert(handle.path.clone()) {
                continue;
            }
            supporting_reads.push(handle);
            if supporting_reads.len() >= limit {
                break;
            }
        }
    }
    Ok(supporting_reads)
}

fn compact_text_fragment_likely_tests(
    host: &QueryHost,
    session: &SessionState,
    target: &SessionHandleTarget,
    limit: usize,
) -> Result<Vec<AgentTargetHandleView>> {
    let prism = host.current_prism();
    let mut likely_tests = Vec::new();
    let mut seen = HashSet::<String>::new();
    for symbol in semantic_symbols_for_text_target(host, target, limit)? {
        let symbol_id = NodeId::new(
            symbol.id.crate_name.clone(),
            symbol.id.path.clone(),
            symbol.kind,
        );
        for candidate in owner_views_for_target(prism.as_ref(), &symbol_id, Some("test"), limit)? {
            if !seen.insert(candidate.symbol.id.path.clone()) {
                continue;
            }
            likely_tests.push(compact_target_view(
                session,
                &candidate.symbol,
                target.query.as_deref(),
                Some(candidate.why),
            ));
            if likely_tests.len() >= limit {
                return Ok(likely_tests);
            }
        }
    }
    Ok(likely_tests)
}

fn text_fragment_semantic_why(symbol: &SymbolView) -> String {
    match symbol.kind {
        NodeKind::MarkdownHeading => {
            "Semantic heading aligned with the exact text hit.".to_string()
        }
        NodeKind::JsonKey | NodeKind::TomlKey | NodeKind::YamlKey => {
            "Structured key aligned with the exact text hit.".to_string()
        }
        _ => "Semantic owner aligned with the exact text hit.".to_string(),
    }
}

fn prioritized_spec_supporting_reads(
    session: &SessionState,
    target: &SessionHandleTarget,
    drift: &prism_js::SpecDriftExplanationView,
    limit: usize,
) -> Vec<AgentTargetHandleView> {
    ranked_spec_followups(target, drift)
        .into_iter()
        .take(limit)
        .map(|candidate| {
            compact_target_view(
                session,
                &candidate.symbol,
                target.query.as_deref(),
                Some(candidate.why),
            )
        })
        .collect()
}

fn prioritized_spec_test_reads(
    session: &SessionState,
    target: &SessionHandleTarget,
    drift: &prism_js::SpecDriftExplanationView,
    limit: usize,
) -> Vec<AgentTargetHandleView> {
    drift
        .cluster
        .tests
        .iter()
        .take(limit)
        .map(|candidate| {
            compact_target_view(
                session,
                &candidate.symbol,
                target.query.as_deref(),
                Some(candidate.why.clone()),
            )
        })
        .collect()
}

fn ranked_spec_followups(
    target: &SessionHandleTarget,
    drift: &prism_js::SpecDriftExplanationView,
) -> Vec<RankedCompactFollowup> {
    let query_tokens = target
        .query
        .as_deref()
        .map(normalize_locate_text)
        .map(|query| locate_query_tokens(&query))
        .unwrap_or_default();
    let mut candidates = Vec::<RankedCompactFollowup>::new();
    let mut seen = HashSet::<String>::new();

    push_ranked_spec_symbols(
        &mut candidates,
        &mut seen,
        drift.cluster.implementations.iter(),
        "Implementation linked from the spec cluster.",
        140,
        &query_tokens,
    );
    push_ranked_spec_owners(
        &mut candidates,
        &mut seen,
        drift.cluster.write_path.iter(),
        120,
        &query_tokens,
    );
    push_ranked_spec_owners(
        &mut candidates,
        &mut seen,
        drift.cluster.read_path.iter(),
        110,
        &query_tokens,
    );
    push_ranked_spec_owners(
        &mut candidates,
        &mut seen,
        drift.cluster.persistence_path.iter(),
        100,
        &query_tokens,
    );
    push_ranked_spec_owners(
        &mut candidates,
        &mut seen,
        drift.next_reads.iter(),
        80,
        &query_tokens,
    );

    let has_token_overlap = candidates
        .iter()
        .any(|candidate| spec_followup_token_overlap(&candidate.symbol, &query_tokens) > 0);
    if has_token_overlap {
        candidates
            .retain(|candidate| spec_followup_token_overlap(&candidate.symbol, &query_tokens) > 0);
    }

    candidates.sort_by(|left, right| {
        right
            .score
            .cmp(&left.score)
            .then_with(|| left.symbol.id.path.cmp(&right.symbol.id.path))
    });
    candidates
}

fn push_ranked_spec_symbols<'a>(
    out: &mut Vec<RankedCompactFollowup>,
    seen: &mut HashSet<String>,
    symbols: impl Iterator<Item = &'a SymbolView>,
    why: &str,
    source_weight: i32,
    query_tokens: &[String],
) {
    for symbol in symbols {
        if !seen.insert(symbol.id.path.clone()) {
            continue;
        }
        out.push(RankedCompactFollowup {
            symbol: symbol.clone(),
            why: why.to_string(),
            score: spec_followup_score(symbol, source_weight, query_tokens),
        });
    }
}

fn push_ranked_spec_owners<'a>(
    out: &mut Vec<RankedCompactFollowup>,
    seen: &mut HashSet<String>,
    owners: impl Iterator<Item = &'a prism_js::OwnerCandidateView>,
    source_weight: i32,
    query_tokens: &[String],
) {
    for candidate in owners {
        if !seen.insert(candidate.symbol.id.path.clone()) {
            continue;
        }
        out.push(RankedCompactFollowup {
            symbol: candidate.symbol.clone(),
            why: candidate.why.clone(),
            score: spec_followup_score(&candidate.symbol, source_weight, query_tokens),
        });
    }
}

fn spec_followup_score(symbol: &SymbolView, source_weight: i32, query_tokens: &[String]) -> i32 {
    let mut score = source_weight;
    let overlap = spec_followup_token_overlap(symbol, query_tokens) as i32;
    score += overlap * 26;
    if is_code_like_kind(symbol.kind) {
        score += 18;
    }
    if matches!(symbol.kind, NodeKind::Module | NodeKind::Document) {
        score -= 20;
    }
    if symbol
        .file_path
        .as_deref()
        .is_some_and(|path| path.ends_with("/lib.rs"))
    {
        score -= 16;
    }
    score
}

fn spec_followup_token_overlap(symbol: &SymbolView, query_tokens: &[String]) -> usize {
    let name = normalize_locate_text(symbol.name.as_str());
    let path = normalize_locate_text(symbol.id.path.as_str());
    query_tokens
        .iter()
        .filter(|token| name.contains(token.as_str()) || path.contains(token.as_str()))
        .count()
}

fn compact_why_short(
    symbol: &SymbolView,
    why_override: Option<&str>,
    query: Option<&str>,
) -> String {
    let base = why_override
        .filter(|value| !value.trim().is_empty())
        .or_else(|| symbol.owner_hint.as_ref().map(|hint| hint.why.as_str()))
        .map(ToString::to_string)
        .or_else(|| {
            symbol
                .file_path
                .as_ref()
                .map(|file_path| format!("{} in {}", symbol.kind, file_path))
        })
        .or_else(|| query.map(|query| format!("Matched `{query}`.")))
        .unwrap_or_else(|| format!("{} target", symbol.kind));
    clamp_string(&base, MAX_WHY_SHORT_CHARS)
}

fn clamp_string(value: &str, max_chars: usize) -> String {
    let mut truncated = value.chars().take(max_chars).collect::<String>();
    if value.chars().count() > max_chars && max_chars > 1 {
        truncated.truncate(max_chars.saturating_sub(1));
        truncated.push('…');
    }
    truncated
}

fn resolve_or_select_workset_target(
    host: &QueryHost,
    session: Arc<SessionState>,
    prism: &Prism,
    args: &PrismWorksetArgs,
    query_run: QueryRun,
) -> Result<(SessionHandleTarget, bool)> {
    if let Some(handle) = args.handle.as_deref() {
        return resolve_handle_target(session.as_ref(), prism, handle);
    }
    let query = args
        .query
        .as_deref()
        .ok_or_else(|| anyhow!("prism_workset requires `handle` or `query`"))?;
    let execution = crate::QueryExecution::new(
        host.clone(),
        Arc::clone(&session),
        host.current_prism(),
        query_run,
    );
    let symbol = execution
        .search(SearchArgs {
            query: query.to_string(),
            limit: Some(1),
            kind: None,
            path: None,
            module: None,
            task_id: None,
            path_mode: None,
            strategy: Some("direct".to_string()),
            structured_path: None,
            top_level_only: None,
            prefer_callable_code: Some(true),
            prefer_editable_targets: Some(true),
            prefer_behavioral_owners: Some(true),
            owner_kind: None,
            include_inferred: Some(true),
        })?
        .into_iter()
        .next()
        .ok_or_else(|| anyhow!("no target matched `{query}`; rerun prism_locate first"))?;
    let handle_view = compact_target_view(session.as_ref(), &symbol, Some(query), None);
    resolve_handle_target(session.as_ref(), prism, &handle_view.handle)
}

fn resolve_handle_target(
    session: &SessionState,
    prism: &Prism,
    handle: &str,
) -> Result<(SessionHandleTarget, bool)> {
    let mut target = session.handle_target(handle).ok_or_else(|| {
        anyhow!("unknown handle `{handle}`; rerun prism_locate to select a target")
    })?;
    let mut remapped = false;
    if is_text_fragment_target(&target) {
        return Ok((target, false));
    }
    if symbol_for(prism, &target.id).is_err() {
        let lineage_id = target.lineage_id.clone().ok_or_else(|| {
            anyhow!(
                "target handle `{handle}` is stale; rerun prism_locate to select a fresh target"
            )
        })?;
        let resolved =
            resolve_lineage_target(prism, &LineageId::new(lineage_id), Some(&target.id))?;
        if resolved != target.id {
            target.id = resolved;
            target.kind = target.id.kind;
            remapped = true;
        }
    }
    let symbol = symbol_for(prism, &target.id)?;
    let symbol_view = symbol_view(prism, &symbol)?;
    target.name = symbol_view.name;
    target.kind = symbol_view.kind;
    target.file_path = symbol_view.file_path;
    target.lineage_id = symbol_view.lineage_id.or(target.lineage_id);
    session.refresh_target_handle(handle, target.clone());
    Ok((target, remapped))
}

fn is_text_fragment_target(target: &SessionHandleTarget) -> bool {
    target.id.crate_name.as_str() == TEXT_FRAGMENT_CRATE_NAME
        && target.start_line.is_some()
        && target.end_line.is_some()
        && target.file_path.is_some()
}

fn target_symbol_id(target: &SessionHandleTarget) -> Result<&NodeId> {
    if is_text_fragment_target(target) {
        return Err(anyhow!(
            "target `{}` is a text-fragment handle; rerun prism_locate on a semantic symbol if you need symbol-only behavior",
            target.id.path
        ));
    }
    Ok(&target.id)
}

fn resolve_lineage_target(
    prism: &Prism,
    lineage: &LineageId,
    requested_id: Option<&NodeId>,
) -> Result<NodeId> {
    let candidates = prism.current_nodes_for_lineage(lineage);
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

fn compact_open_result_from_block(
    handle: &str,
    file_path: &str,
    slice: Option<SourceSliceView>,
    excerpt: Option<SourceExcerptView>,
    remapped: bool,
    next_action: &str,
    related_handles: Option<Vec<AgentTargetHandleView>>,
) -> Result<AgentOpenResultView> {
    if let Some(slice) = slice {
        return Ok(compact_open_result_from_slice(
            handle,
            file_path,
            slice,
            remapped,
            next_action,
            related_handles,
        )?);
    }
    if let Some(excerpt) = excerpt {
        return Ok(compact_open_result_from_excerpt(
            handle,
            file_path,
            excerpt,
            remapped,
            next_action,
            related_handles,
        )?);
    }
    Err(anyhow!("target did not produce any bounded source content"))
}

fn compact_open_result_from_slice(
    handle: &str,
    file_path: &str,
    slice: SourceSliceView,
    remapped: bool,
    next_action: &str,
    related_handles: Option<Vec<AgentTargetHandleView>>,
) -> Result<AgentOpenResultView> {
    budgeted_open_result(AgentOpenResultView {
        handle: handle.to_string(),
        file_path: file_path.to_string(),
        start_line: slice.start_line,
        end_line: slice.end_line,
        text: slice.text,
        truncated: slice.truncated,
        remapped,
        next_action: slice.truncated.then(|| next_action.to_string()),
        related_handles,
    })
}

fn compact_open_result_from_excerpt(
    handle: &str,
    file_path: &str,
    excerpt: SourceExcerptView,
    remapped: bool,
    next_action: &str,
    related_handles: Option<Vec<AgentTargetHandleView>>,
) -> Result<AgentOpenResultView> {
    budgeted_open_result(AgentOpenResultView {
        handle: handle.to_string(),
        file_path: file_path.to_string(),
        start_line: excerpt.start_line,
        end_line: excerpt.end_line,
        text: excerpt.text,
        truncated: excerpt.truncated,
        remapped,
        next_action: excerpt.truncated.then(|| next_action.to_string()),
        related_handles,
    })
}

fn compact_open_related_handles(
    host: &QueryHost,
    session: &SessionState,
    prism: &Prism,
    target: &SessionHandleTarget,
) -> Result<Option<Vec<AgentTargetHandleView>>> {
    if is_text_fragment_target(target) {
        return compact_text_fragment_related_handles(host, session, target);
    }
    let mut seen = HashSet::<String>::new();
    let mut related_handles = Vec::new();
    for candidate in next_reads(
        prism,
        target_symbol_id(target)?,
        OPEN_RELATED_HANDLE_LIMIT + 1,
    )? {
        if candidate.symbol.id.crate_name == target.id.crate_name
            && candidate.symbol.id.path == target.id.path
            && candidate.symbol.kind == target.kind
        {
            continue;
        }
        if !seen.insert(candidate.symbol.id.path.clone()) {
            continue;
        }
        let mut handle = compact_target_view(
            session,
            &candidate.symbol,
            target.query.as_deref(),
            Some(candidate.why),
        );
        handle.file_path = None;
        related_handles.push(handle);
        if related_handles.len() >= OPEN_RELATED_HANDLE_LIMIT {
            break;
        }
    }
    Ok((!related_handles.is_empty()).then_some(related_handles))
}

fn compact_preview_for_symbol_view(
    prism: &Prism,
    handle: &str,
    symbol: &SymbolView,
) -> Result<Option<AgentTextPreviewView>> {
    let id = NodeId::new(
        symbol.id.crate_name.clone(),
        symbol.id.path.clone(),
        symbol.kind,
    );
    let symbol = symbol_for(prism, &id)?;
    let file_path = symbol_view(prism, &symbol)?
        .file_path
        .as_deref()
        .ok_or_else(|| anyhow!("target `{}` has no workspace file path", id.path))?
        .to_string();
    let block = focused_block_for_symbol(prism, &symbol, PREVIEW_OPEN_OPTIONS)?;
    Ok(match (block.slice, block.excerpt) {
        (Some(slice), _) => Some(AgentTextPreviewView {
            handle: handle.to_string(),
            file_path,
            start_line: slice.start_line,
            end_line: slice.end_line,
            text: slice.text,
            truncated: slice.truncated,
        }),
        (None, Some(excerpt)) => Some(AgentTextPreviewView {
            handle: handle.to_string(),
            file_path,
            start_line: excerpt.start_line,
            end_line: excerpt.end_line,
            text: excerpt.text,
            truncated: excerpt.truncated,
        }),
        (None, None) => None,
    })
}

fn compact_preview_for_ranked_target(
    host: &QueryHost,
    prism: &Prism,
    handle: &str,
    target: &RankedLocateTarget,
) -> Result<Option<AgentTextPreviewView>> {
    match target {
        RankedLocateTarget::Symbol(symbol) => {
            compact_preview_for_symbol_view(prism, handle, symbol)
        }
        RankedLocateTarget::Text(text_target) => {
            compact_preview_for_text_target(host, handle, text_target)
        }
    }
}

fn compact_preview_for_text_target(
    host: &QueryHost,
    handle: &str,
    target: &SessionHandleTarget,
) -> Result<Option<AgentTextPreviewView>> {
    let excerpt = read_text_fragment(
        host,
        target,
        target.start_line.unwrap_or(1),
        target.end_line.unwrap_or(1),
        PREVIEW_OPEN_OPTIONS.max_chars,
    )?;
    Ok(Some(AgentTextPreviewView {
        handle: handle.to_string(),
        file_path: target.file_path.clone().unwrap_or_default(),
        start_line: excerpt.start_line,
        end_line: excerpt.end_line,
        text: excerpt.text,
        truncated: excerpt.truncated,
    }))
}

fn compact_open_text_fragment(
    host: &QueryHost,
    session: &SessionState,
    handle: &str,
    mode: AgentOpenMode,
    target: &SessionHandleTarget,
    remapped: bool,
) -> Result<AgentOpenResultView> {
    let file_path = target
        .file_path
        .clone()
        .ok_or_else(|| anyhow!("text-fragment handle `{handle}` is missing a file path"))?;
    let (start_line, end_line) = text_fragment_line_span(target);
    let excerpt = match mode {
        AgentOpenMode::Focus => read_text_fragment(
            host,
            target,
            start_line.saturating_sub(1).max(1),
            end_line + 1,
            FOCUS_OPEN_OPTIONS.max_chars,
        )?,
        AgentOpenMode::Edit => read_text_fragment(
            host,
            target,
            start_line.saturating_sub(1).max(1),
            end_line + 1,
            EDIT_OPEN_OPTIONS.max_chars,
        )?,
        AgentOpenMode::Raw => {
            read_text_fragment(host, target, start_line, end_line, RAW_OPEN_MAX_CHARS)?
        }
    };
    compact_open_result_from_excerpt(
        handle,
        &file_path,
        excerpt,
        remapped,
        "Rerun prism_gather with a narrower `path` or `glob` if you need a smaller exact-text window.",
        compact_text_fragment_related_handles(host, session, target)?,
    )
}

fn compact_text_fragment_related_handles(
    host: &QueryHost,
    session: &SessionState,
    target: &SessionHandleTarget,
) -> Result<Option<Vec<AgentTargetHandleView>>> {
    let Some(_query) = target.query.as_deref() else {
        return Ok(None);
    };
    let Some(file_path) = target.file_path.as_deref() else {
        return Ok(None);
    };
    let mut related = Vec::new();
    let mut seen = HashSet::<String>::new();
    for symbol in semantic_symbols_for_text_target(host, target, OPEN_RELATED_HANDLE_LIMIT)? {
        if !seen.insert(symbol.id.path.clone()) {
            continue;
        }
        related.push(compact_target_view(
            session,
            &symbol,
            target.query.as_deref(),
            Some(text_fragment_semantic_why(&symbol)),
        ));
        if related.len() >= OPEN_RELATED_HANDLE_LIMIT {
            return Ok(Some(related));
        }
    }
    for query in text_fragment_query_variants(target) {
        let outcome = search_text(
            host,
            SearchTextArgs {
                query: query.to_string(),
                regex: Some(false),
                case_sensitive: Some(false),
                path: Some(file_path.to_string()),
                glob: None,
                limit: Some(TEXT_FRAGMENT_RELATED_LIMIT + 1),
                context_lines: Some(0),
            },
            session.limits().max_result_nodes,
        )?;
        for matched in outcome.results {
            if Some(matched.location.start_line) == target.start_line
                && Some(matched.location.end_line) == target.end_line
            {
                continue;
            }
            let handle = compact_target_from_session_target(
                session,
                &text_candidate_from_match(session, query.as_str(), matched).target,
            );
            if !seen.insert(handle.path.clone()) {
                continue;
            }
            related.push(handle);
            if related.len() >= OPEN_RELATED_HANDLE_LIMIT {
                return Ok(Some(related));
            }
        }
    }
    Ok((!related.is_empty()).then_some(related))
}

fn compact_text_fragment_neighbors(
    host: &QueryHost,
    session: &SessionState,
    target: &SessionHandleTarget,
    include_top_preview: bool,
) -> Result<(Vec<AgentTargetHandleView>, Option<AgentTextPreviewView>)> {
    let Some(_query) = target.query.as_deref() else {
        return Ok((Vec::new(), None));
    };
    let Some(file_path) = target.file_path.as_deref() else {
        return Ok((Vec::new(), None));
    };
    let mut neighbors = Vec::new();
    let mut top_preview = None;
    for query in text_fragment_query_variants(target) {
        let outcome = search_text(
            host,
            SearchTextArgs {
                query: query.to_string(),
                regex: Some(false),
                case_sensitive: Some(false),
                path: Some(file_path.to_string()),
                glob: None,
                limit: Some(EXPAND_NEIGHBOR_LIMIT + 1),
                context_lines: Some(0),
            },
            session.limits().max_result_nodes,
        )?;
        for matched in outcome.results {
            if Some(matched.location.start_line) == target.start_line
                && Some(matched.location.end_line) == target.end_line
            {
                continue;
            }
            let handle_target = text_candidate_from_match(session, query.as_str(), matched).target;
            let view = compact_target_from_session_target(session, &handle_target);
            if include_top_preview && top_preview.is_none() {
                top_preview = compact_preview_for_text_target(host, &view.handle, &handle_target)?;
            }
            neighbors.push(view);
            if neighbors.len() >= EXPAND_NEIGHBOR_LIMIT {
                return Ok((neighbors, top_preview));
            }
        }
    }
    if neighbors.is_empty() {
        let prism = host.current_prism();
        for symbol in semantic_symbols_for_text_target(host, target, EXPAND_NEIGHBOR_LIMIT)? {
            let view = compact_target_view(
                session,
                &symbol,
                target.query.as_deref(),
                Some(text_fragment_semantic_why(&symbol)),
            );
            if include_top_preview && top_preview.is_none() {
                top_preview =
                    compact_preview_for_symbol_view(prism.as_ref(), &view.handle, &symbol)?;
            }
            neighbors.push(view);
            if neighbors.len() >= EXPAND_NEIGHBOR_LIMIT {
                break;
            }
        }
    }
    Ok((neighbors, top_preview))
}

fn compact_text_fragment_diagnostics(target: &SessionHandleTarget) -> Value {
    json!({
        "query": target.query,
        "whyShort": target.why_short,
        "filePath": target.file_path,
        "range": {
            "startLine": target.start_line,
            "endLine": target.end_line,
            "startColumn": target.start_column,
            "endColumn": target.end_column,
        },
    })
}

fn compact_text_fragment_validation(
    host: &QueryHost,
    session: &SessionState,
    target: &SessionHandleTarget,
) -> Result<Value> {
    let mut likely_tests =
        compact_text_fragment_likely_tests(host, session, target, WORKSET_TEST_LIMIT)?;
    if likely_tests.is_empty() {
        if let Some(query) = target.query.as_deref() {
            likely_tests = search_text(
                host,
                SearchTextArgs {
                    query: query.to_string(),
                    regex: Some(false),
                    case_sensitive: Some(false),
                    path: None,
                    glob: Some("**/*test*".to_string()),
                    limit: Some(WORKSET_TEST_LIMIT),
                    context_lines: Some(0),
                },
                session.limits().max_result_nodes,
            )?
            .results
            .into_iter()
            .map(|matched| {
                compact_target_from_session_target(
                    session,
                    &text_candidate_from_match(session, query, matched).target,
                )
            })
            .take(WORKSET_TEST_LIMIT)
            .collect::<Vec<_>>();
        }
    }
    let checks = if likely_tests.is_empty() {
        vec![
            "Confirm the adjacent config/script updates stay consistent with the exact text hit."
                .to_string(),
        ]
    } else {
        vec!["Confirm the linked semantic owner and its likely tests still agree with the exact text hit.".to_string()]
    };
    Ok(json!({
        "checks": checks,
        "likelyTests": likely_tests,
        "why": workset_why(target),
    }))
}

fn gather_text_matches(
    host: &QueryHost,
    session: &SessionState,
    args: &PrismGatherArgs,
) -> Result<(Vec<AgentOpenResultView>, bool)> {
    let requested = compact_gather_limit(args.limit);
    let outcome = search_text(
        host,
        SearchTextArgs {
            query: args.query.clone(),
            regex: Some(false),
            case_sensitive: Some(false),
            path: args.path.clone(),
            glob: args.glob.clone(),
            limit: Some(requested.saturating_add(1)),
            context_lines: Some(0),
        },
        session.limits().max_result_nodes,
    )?;
    let truncated = outcome.results.len() > requested;
    let matches = outcome
        .results
        .into_iter()
        .take(requested)
        .map(|matched| {
            let target = text_candidate_from_match(session, &args.query, matched).target;
            let handle = session.intern_target_handle(target.clone());
            compact_open_text_fragment(host, session, &handle, AgentOpenMode::Focus, &target, false)
        })
        .collect::<Result<Vec<_>>>()?;
    Ok((matches, truncated))
}

fn text_fragment_line_span(target: &SessionHandleTarget) -> (usize, usize) {
    (
        target.start_line.unwrap_or(1),
        target.end_line.unwrap_or(target.start_line.unwrap_or(1)),
    )
}

fn read_text_fragment(
    host: &QueryHost,
    target: &SessionHandleTarget,
    start_line: usize,
    end_line: usize,
    max_chars: usize,
) -> Result<SourceExcerptView> {
    let file_path = target.file_path.clone().ok_or_else(|| {
        anyhow!(
            "text-fragment target `{}` is missing a file path",
            target.id.path
        )
    })?;
    file_read(
        host,
        FileReadArgs {
            path: file_path,
            start_line: Some(start_line),
            end_line: Some(end_line),
            max_chars: Some(max_chars),
        },
    )
}

fn compact_gather_limit(limit: Option<usize>) -> usize {
    limit
        .unwrap_or(DEFAULT_GATHER_LIMIT)
        .clamp(1, MAX_GATHER_LIMIT)
}

fn compact_drift_expand_result(
    session: &SessionState,
    prism: &Prism,
    target: &SessionHandleTarget,
) -> Result<Value> {
    let drift = spec_drift_explanation_view(prism, &target.id)?;
    let drift_reasons = if drift.drift_reasons.is_empty() {
        if drift.gaps.is_empty() {
            compact_string_list(
                &drift.notes,
                EXPAND_DRIFT_LIST_LIMIT,
                EXPAND_DRIFT_TEXT_MAX_CHARS,
            )
        } else {
            compact_string_list(
                &drift.gaps,
                EXPAND_DRIFT_LIST_LIMIT,
                EXPAND_DRIFT_TEXT_MAX_CHARS,
            )
        }
    } else {
        compact_string_list(
            &drift.drift_reasons,
            EXPAND_DRIFT_LIST_LIMIT,
            EXPAND_DRIFT_TEXT_MAX_CHARS,
        )
    };
    let mut next_reads =
        prioritized_spec_supporting_reads(session, target, &drift, EXPAND_DRIFT_NEXT_READ_LIMIT);
    strip_file_paths(&mut next_reads);
    let mut result = json!({
        "driftReasons": drift_reasons,
        "expectations": compact_string_list(
            &drift.expectations,
            EXPAND_DRIFT_LIST_LIMIT,
            EXPAND_DRIFT_TEXT_MAX_CHARS,
        ),
        "gaps": compact_string_list(
            &drift.gaps,
            EXPAND_DRIFT_LIST_LIMIT,
            EXPAND_DRIFT_TEXT_MAX_CHARS,
        ),
        "nextReads": next_reads,
        "confidence": drift.trust_signals.confidence_label,
        "evidenceSources": drift.trust_signals.evidence_sources,
    });

    while drift_json_bytes(&result)? > EXPAND_DRIFT_MAX_JSON_BYTES {
        if next_reads.pop().is_some() {
            result["nextReads"] = serde_json::to_value(&next_reads)?;
            continue;
        }
        if let Some(expectations) = result
            .get_mut("expectations")
            .and_then(Value::as_array_mut)
            .filter(|items| !items.is_empty())
        {
            expectations.pop();
            continue;
        }
        if let Some(evidence_sources) = result
            .get_mut("evidenceSources")
            .and_then(Value::as_array_mut)
            .filter(|items| !items.is_empty())
        {
            evidence_sources.pop();
            continue;
        }
        break;
    }

    Ok(result)
}

fn source_slice_view(slice: prism_query::EditSlice) -> SourceSliceView {
    SourceSliceView {
        text: slice.text,
        start_line: slice.start_line,
        end_line: slice.end_line,
        focus: SourceLocationView {
            start_line: slice.focus.start_line,
            start_column: slice.focus.start_column,
            end_line: slice.focus.end_line,
            end_column: slice.focus.end_column,
        },
        relative_focus: SourceLocationView {
            start_line: slice.relative_focus.start_line,
            start_column: slice.relative_focus.start_column,
            end_line: slice.relative_focus.end_line,
            end_column: slice.relative_focus.end_column,
        },
        truncated: slice.truncated,
    }
}

fn agent_open_mode(mode: Option<&PrismOpenModeInput>) -> AgentOpenMode {
    match mode.unwrap_or(&PrismOpenModeInput::Focus) {
        PrismOpenModeInput::Focus => AgentOpenMode::Focus,
        PrismOpenModeInput::Edit => AgentOpenMode::Edit,
        PrismOpenModeInput::Raw => AgentOpenMode::Raw,
    }
}

fn agent_expand_kind(kind: &PrismExpandKindInput) -> AgentExpandKind {
    match kind {
        PrismExpandKindInput::Diagnostics => AgentExpandKind::Diagnostics,
        PrismExpandKindInput::Lineage => AgentExpandKind::Lineage,
        PrismExpandKindInput::Neighbors => AgentExpandKind::Neighbors,
        PrismExpandKindInput::Diff => AgentExpandKind::Diff,
        PrismExpandKindInput::Validation => AgentExpandKind::Validation,
        PrismExpandKindInput::Drift => AgentExpandKind::Drift,
    }
}

fn workset_why(target: &SessionHandleTarget) -> String {
    let why = if is_text_fragment_target(target) {
        match (
            target.query.as_deref(),
            target.file_path.as_deref(),
            target.start_line,
        ) {
            (Some(query), Some(file_path), Some(start_line)) => format!(
                "Exact text hit for `{query}` in {file_path}:{start_line}. {}",
                target.why_short
            ),
            (Some(query), _, _) => format!("Exact text hit for `{query}`. {}", target.why_short),
            _ => target.why_short.clone(),
        }
    } else {
        match target.query.as_deref() {
            Some(query) => format!("Primary target from `{query}`. {}", target.why_short),
            None => target.why_short.clone(),
        }
    };
    clamp_string(&why, WORKSET_WHY_MAX_CHARS)
}

fn spec_workset_why(
    target: &SessionHandleTarget,
    gaps: &[String],
    drift_reasons: &[String],
) -> String {
    let base = workset_why(target);
    let gap = gaps
        .iter()
        .chain(drift_reasons.iter())
        .find(|value| !value.trim().is_empty())
        .map(|value| clamp_string(value, 72));
    match gap {
        Some(gap) => clamp_string(
            &format!("{base} Gap summary: {gap}."),
            WORKSET_WHY_MAX_CHARS,
        ),
        None => base,
    }
}

fn strip_file_paths(targets: &mut [AgentTargetHandleView]) -> bool {
    let mut changed = false;
    for target in targets {
        if target.file_path.take().is_some() {
            changed = true;
        }
    }
    changed
}

fn workset_json_bytes(result: &AgentWorksetResultView) -> Result<usize> {
    Ok(serde_json::to_vec(result)?.len())
}

fn open_json_bytes(result: &AgentOpenResultView) -> Result<usize> {
    Ok(serde_json::to_vec(result)?.len())
}

fn drift_json_bytes(result: &Value) -> Result<usize> {
    Ok(serde_json::to_vec(result)?.len())
}

#[cfg(test)]
mod tests {
    use prism_ir::NodeKind;

    use super::*;

    fn handle_view(index: usize, file_path: Option<&str>) -> AgentTargetHandleView {
        AgentTargetHandleView {
            handle: format!("handle:{index}"),
            kind: NodeKind::Function,
            path: format!("demo::module_{index}::very_long_function_name_for_budget_tests"),
            name: format!("very_long_function_name_for_budget_tests_{index}"),
            why_short: "Matched ranking hint from a compact budget regression test.".to_string(),
            file_path: file_path.map(ToString::to_string),
        }
    }

    fn open_result(
        related_handles: Option<Vec<AgentTargetHandleView>>,
        text_len: usize,
    ) -> AgentOpenResultView {
        AgentOpenResultView {
            handle: "handle:primary".to_string(),
            file_path: "src/main.rs".to_string(),
            start_line: 1,
            end_line: 12,
            text: "x".repeat(text_len),
            truncated: false,
            remapped: false,
            next_action: None,
            related_handles,
        }
    }

    fn string_list(prefix: &str, count: usize) -> Vec<String> {
        (0..count)
            .map(|index| format!("{prefix} {index} with extra compact drift budget text"))
            .collect()
    }

    #[test]
    fn workset_budget_leaves_small_results_untrimmed() {
        let result = budgeted_workset_result(
            handle_view(1, Some("src/main.rs")),
            vec![handle_view(2, Some("src/helper.rs"))],
            vec![],
            "Primary target from `main`. Function in src/main.rs".to_string(),
            false,
        )
        .expect("budgeted workset should serialize");

        assert!(!result.truncated);
        assert!(result.next_action.is_none());
        assert!(workset_json_bytes(&result).expect("json bytes") <= WORKSET_MAX_JSON_BYTES);
    }

    #[test]
    fn workset_budget_trims_context_before_exceeding_budget() {
        let long_path =
            "src/really/deeply/nested/module/with/a/very/long/path/for/compact/workset/tests.rs";
        let result = budgeted_workset_result(
            handle_view(1, Some(long_path)),
            vec![
                handle_view(2, Some(long_path)),
                handle_view(3, Some(long_path)),
                handle_view(4, Some(long_path)),
            ],
            vec![handle_view(5, Some(long_path)), handle_view(6, Some(long_path))],
            "Primary target from `very_long_function_name_for_budget_tests`. This sentence is deliberately verbose so the workset budgeting logic has to trim optional context instead of returning a bloated compact response.".to_string(),
            false,
        )
        .expect("budgeted workset should serialize");

        assert!(result.truncated);
        assert_eq!(
            result.next_action.as_deref(),
            Some(WORKSET_TRUNCATED_NEXT_ACTION)
        );
        assert!(workset_json_bytes(&result).expect("json bytes") <= WORKSET_MAX_JSON_BYTES);
        assert_eq!(result.primary.handle, "handle:1");
        assert!(
            result.supporting_reads.len() < 3
                || result
                    .supporting_reads
                    .iter()
                    .all(|target| target.file_path.is_none())
        );
    }

    #[test]
    fn open_budget_keeps_related_handles_small_and_compact() {
        let long_path =
            "src/really/deeply/nested/module/with/a/very/long/path/for/compact/open/tests.rs";
        let result = budgeted_open_result(open_result(
            Some(vec![
                handle_view(1, Some(long_path)),
                handle_view(2, Some(long_path)),
                handle_view(3, Some(long_path)),
            ]),
            RAW_OPEN_MAX_CHARS,
        ))
        .expect("budgeted open should serialize");

        assert!(open_json_bytes(&result).expect("json bytes") <= OPEN_MAX_JSON_BYTES);
        assert!(result
            .related_handles
            .as_ref()
            .is_none_or(|targets| targets.len() <= OPEN_RELATED_HANDLE_LIMIT));
        assert!(result
            .related_handles
            .as_ref()
            .is_none_or(|targets| { targets.iter().all(|target| target.file_path.is_none()) }));
    }

    #[test]
    fn compact_string_list_dedupes_and_clamps() {
        let compact = compact_string_list(
            &[
                "same repeated item that is too long for the compact list".to_string(),
                "same repeated item that is too long for the compact list".to_string(),
                "different item".to_string(),
            ],
            3,
            18,
        );

        assert_eq!(compact.len(), 2);
        assert!(compact[0].chars().count() <= 18);
    }

    #[test]
    fn drift_budget_trims_optional_context_before_exceeding_budget() {
        let mut next_reads = vec![
            handle_view(1, Some("src/one.rs")),
            handle_view(2, Some("src/two.rs")),
            handle_view(3, Some("src/three.rs")),
        ];
        strip_file_paths(&mut next_reads);
        let mut result = json!({
            "driftReasons": string_list("drift reason", 4),
            "expectations": string_list("expectation", 4),
            "gaps": string_list("gap", 4),
            "nextReads": next_reads,
            "confidence": "high",
            "evidenceSources": ["inferred", "memory", "direct_graph"],
        });

        assert!(drift_json_bytes(&result).expect("json bytes") > EXPAND_DRIFT_MAX_JSON_BYTES);

        while drift_json_bytes(&result).expect("json bytes") > EXPAND_DRIFT_MAX_JSON_BYTES {
            if let Some(next_reads) = result["nextReads"]
                .as_array_mut()
                .filter(|items| !items.is_empty())
            {
                next_reads.pop();
                continue;
            }
            if let Some(expectations) = result["expectations"]
                .as_array_mut()
                .filter(|items| !items.is_empty())
            {
                expectations.pop();
                continue;
            }
            if let Some(evidence_sources) = result["evidenceSources"]
                .as_array_mut()
                .filter(|items| !items.is_empty())
            {
                evidence_sources.pop();
                continue;
            }
            break;
        }

        assert!(drift_json_bytes(&result).expect("json bytes") <= EXPAND_DRIFT_MAX_JSON_BYTES);
    }
}
