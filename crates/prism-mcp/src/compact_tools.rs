use std::path::Path;
use std::sync::Arc;
use std::time::Instant;

use anyhow::{anyhow, Result};
use globset::{GlobBuilder, GlobMatcher};
use prism_ir::{LineageId, NodeId};
use prism_js::{
    AgentExpandKind, AgentExpandResultView, AgentLocateResultView, AgentLocateStatus,
    AgentOpenMode, AgentOpenResultView, AgentTargetHandleView, AgentWorksetResultView,
    QueryDiagnostic, SourceExcerptView, SourceLocationView, SourceSliceView, SymbolView,
};
use prism_query::{EditSliceOptions, Prism};
use serde_json::{json, Value};

use crate::{
    diff_for, focused_block_for_symbol, next_reads, owner_views_for_target, symbol_for,
    symbol_view, validation_context_view_cached, FileReadArgs, PrismExpandArgs,
    PrismExpandKindInput, PrismLocateArgs,
    PrismLocateTaskIntentInput, PrismOpenArgs, PrismOpenModeInput, PrismWorksetArgs, QueryHost,
    QueryRun, SearchArgs, SessionState,
};
use crate::file_queries::file_read;
use crate::session_state::SessionHandleTarget;

const DEFAULT_LOCATE_LIMIT: usize = 3;
const MAX_LOCATE_LIMIT: usize = 3;
const LOCATE_BACKEND_MULTIPLIER: usize = 6;
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
const RAW_OPEN_MAX_CHARS: usize = 720;
const WORKSET_SUPPORTING_LIMIT: usize = 3;
const WORKSET_TEST_LIMIT: usize = 2;
const EXPAND_NEIGHBOR_LIMIT: usize = 6;
const EXPAND_DIFF_LIMIT: usize = 5;
const MAX_WHY_SHORT_CHARS: usize = 120;

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
                let candidates = results
                    .into_iter()
                    .take(applied)
                    .map(|symbol| compact_target_view(&session, &symbol, Some(args.query.as_str()), None))
                    .collect::<Vec<_>>();
                let diagnostics = execution.diagnostics();
                Ok((
                    build_locate_result(candidates, diagnostics.clone()),
                    diagnostics,
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
                let symbol = symbol_for(prism.as_ref(), &target.id)?;
                let symbol_view = symbol_view(prism.as_ref(), &symbol)?;
                let file_path = symbol_view
                    .file_path
                    .clone()
                    .ok_or_else(|| anyhow!("target `{}` has no workspace file path", target.id.path))?;

                let result = match mode {
                    AgentOpenMode::Focus => {
                        let block = focused_block_for_symbol(prism.as_ref(), &symbol, FOCUS_OPEN_OPTIONS)?;
                        compact_open_result_from_block(
                            &args.handle,
                            &file_path,
                            block.slice,
                            block.excerpt,
                            remapped,
                            "Rerun prism_open with mode `raw` if you need the exact file window.",
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
                        )
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
                        )
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
                let supporting_reads = next_reads(prism.as_ref(), &target.id, WORKSET_SUPPORTING_LIMIT)?
                    .into_iter()
                    .take(WORKSET_SUPPORTING_LIMIT)
                    .map(|candidate| {
                        compact_target_view(
                            &session,
                            &candidate.symbol,
                            target.query.as_deref(),
                            Some(candidate.why),
                        )
                    })
                    .collect::<Vec<_>>();
                let likely_tests =
                    owner_views_for_target(prism.as_ref(), &target.id, Some("test"), WORKSET_TEST_LIMIT)?
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
                let why = workset_why(&target);
                Ok((
                    AgentWorksetResultView {
                        primary: target_view,
                        supporting_reads,
                        likely_tests,
                        why,
                        remapped,
                    },
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
                let result = match kind {
                    AgentExpandKind::Diagnostics => {
                        let symbol = symbol_for(prism.as_ref(), &target.id)?;
                        let symbol = symbol_view(prism.as_ref(), &symbol)?;
                        json!({
                            "query": target.query,
                            "whyShort": target.why_short,
                            "filePath": target.file_path,
                            "ownerHint": symbol.owner_hint,
                        })
                    }
                    AgentExpandKind::Lineage => serde_json::to_value(crate::lineage_view(
                        prism.as_ref(),
                        &target.id,
                    )?)?,
                    AgentExpandKind::Neighbors => {
                        let neighbors = next_reads(prism.as_ref(), &target.id, EXPAND_NEIGHBOR_LIMIT)?
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
                    AgentExpandKind::Diff => {
                        let lineage = target
                            .lineage_id
                            .as_ref()
                            .map(|value| LineageId::new(value.clone()));
                        serde_json::to_value(diff_for(
                            prism.as_ref(),
                            Some(&target.id),
                            lineage.as_ref(),
                            None,
                            None,
                            EXPAND_DIFF_LIMIT,
                        )?)?
                    }
                    AgentExpandKind::Validation => {
                        let mut cache = crate::SemanticContextCache::default();
                        let validation = validation_context_view_cached(
                            prism.as_ref(),
                            session.as_ref(),
                            &mut cache,
                            &target.id,
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
                };

                Ok((
                    AgentExpandResultView {
                        handle: args.handle,
                        kind,
                        result,
                        remapped,
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
    let (strategy, owner_kind, prefer_callable_code, prefer_editable_targets, prefer_behavioral_owners) =
        locate_intent_defaults(args.task_intent.as_ref());
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

fn compact_locate_limit(requested: Option<usize>) -> usize {
    requested.unwrap_or(DEFAULT_LOCATE_LIMIT).clamp(1, MAX_LOCATE_LIMIT)
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
        PrismLocateTaskIntentInput::Explain => ("behavioral", Some("read"), false, false, true),
    }
}

fn build_locate_result(
    candidates: Vec<AgentTargetHandleView>,
    diagnostics: Vec<QueryDiagnostic>,
) -> AgentLocateResultView {
    let status = if candidates.is_empty() {
        AgentLocateStatus::Empty
    } else if diagnostics
        .iter()
        .any(|diagnostic| matches!(diagnostic.code.as_str(), "ambiguous_search" | "weak_search_match"))
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
        narrowing_hint: diagnostics.iter().find_map(next_action_hint),
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

fn compact_why_short(symbol: &SymbolView, why_override: Option<&str>, query: Option<&str>) -> String {
    let base = why_override
        .filter(|value| !value.trim().is_empty())
        .or_else(|| symbol.owner_hint.as_ref().map(|hint| hint.why.as_str()))
        .map(ToString::to_string)
        .or_else(|| {
            symbol.file_path.as_ref().map(|file_path| {
                format!("{} in {}", symbol.kind, file_path)
            })
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
    let mut target = session
        .handle_target(handle)
        .ok_or_else(|| anyhow!("unknown handle `{handle}`; rerun prism_locate to select a target"))?;
    let mut remapped = false;
    if symbol_for(prism, &target.id).is_err() {
        let lineage_id = target
            .lineage_id
            .clone()
            .ok_or_else(|| anyhow!("target handle `{handle}` is stale; rerun prism_locate to select a fresh target"))?;
        let resolved = resolve_lineage_target(prism, &LineageId::new(lineage_id), Some(&target.id))?;
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
) -> Result<AgentOpenResultView> {
    if let Some(slice) = slice {
        return Ok(compact_open_result_from_slice(
            handle, file_path, slice, remapped, next_action,
        ));
    }
    if let Some(excerpt) = excerpt {
        return Ok(compact_open_result_from_excerpt(
            handle, file_path, excerpt, remapped, next_action,
        ));
    }
    Err(anyhow!("target did not produce any bounded source content"))
}

fn compact_open_result_from_slice(
    handle: &str,
    file_path: &str,
    slice: SourceSliceView,
    remapped: bool,
    next_action: &str,
) -> AgentOpenResultView {
    AgentOpenResultView {
        handle: handle.to_string(),
        file_path: file_path.to_string(),
        start_line: slice.start_line,
        end_line: slice.end_line,
        text: slice.text,
        truncated: slice.truncated,
        remapped,
        next_action: slice.truncated.then(|| next_action.to_string()),
        related_handles: None,
    }
}

fn compact_open_result_from_excerpt(
    handle: &str,
    file_path: &str,
    excerpt: SourceExcerptView,
    remapped: bool,
    next_action: &str,
) -> AgentOpenResultView {
    AgentOpenResultView {
        handle: handle.to_string(),
        file_path: file_path.to_string(),
        start_line: excerpt.start_line,
        end_line: excerpt.end_line,
        text: excerpt.text,
        truncated: excerpt.truncated,
        remapped,
        next_action: excerpt.truncated.then(|| next_action.to_string()),
        related_handles: None,
    }
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
    }
}

fn workset_why(target: &SessionHandleTarget) -> String {
    match target.query.as_deref() {
        Some(query) => format!("Primary target from `{query}`. {}", target.why_short),
        None => target.why_short.clone(),
    }
}
