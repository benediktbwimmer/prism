use std::collections::HashSet;
use std::path::Path;
use std::sync::Arc;
use std::time::Instant;

use anyhow::{anyhow, Result};
use globset::{GlobBuilder, GlobMatcher};
use prism_ir::{EdgeKind, LineageId, NodeId, NodeKind};
use prism_js::{
    AgentExpandKind, AgentExpandResultView, AgentGatherResultView, AgentHandleCategoryView,
    AgentLocateResultView, AgentLocateStatus, AgentOpenMode, AgentOpenResultView,
    AgentTargetHandleView, AgentTextPreviewView, AgentWorksetResultView, QueryDiagnostic,
    SourceExcerptView, SourceLocationView, SourceSliceView, SymbolView, TextSearchMatchView,
};
use prism_query::{EditSliceOptions, Prism, SourceExcerptOptions};
use serde_json::{json, Value};

mod concept;
mod expand;
mod locate;
mod open;
mod suggested_actions;
mod task_brief;
mod text_fragments;
mod workset;

use self::text_fragments::resolve_text_fragment_target;
use crate::compact_followups::{
    compact_validation_checks, same_workspace_file, spec_body_identifier_terms,
};
use crate::file_queries::file_read;
use crate::session_state::{SessionHandleCategory, SessionHandleTarget};
use crate::text_search::search_text;
use crate::{
    diff_for, focused_block_for_symbol, next_reads, owner_views_for_target,
    spec_drift_explanation_view, symbol_for, symbol_view, validation_context_view_cached,
    FileAroundArgs, FileReadArgs, PrismConceptArgs, PrismExpandArgs, PrismExpandKindInput,
    PrismGatherArgs, PrismLocateArgs, PrismLocateTaskIntentInput, PrismOpenArgs,
    PrismOpenModeInput, PrismWorksetArgs, QueryHost, QueryRun, SearchArgs, SearchTextArgs,
    SessionState,
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
    before_lines: 2,
    after_lines: 4,
    max_lines: 16,
    max_chars: 720,
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
const WORKSET_WHY_ULTRA_TIGHT_MAX_CHARS: usize = 48;
const EXPAND_NEIGHBOR_LIMIT: usize = 6;
const EXPAND_DIFF_LIMIT: usize = 5;
const EXPAND_COMPACT_DIFF_LIMIT: usize = 3;
const EXPAND_LINEAGE_HISTORY_LIMIT: usize = 3;
const EXPAND_LINEAGE_EVIDENCE_LIMIT: usize = 2;
const EXPAND_LINEAGE_UNCERTAINTY_LIMIT: usize = 2;
const EXPAND_LINEAGE_TEXT_MAX_CHARS: usize = 120;
const EXPAND_LINEAGE_MAX_JSON_BYTES: usize = 1400;
const EXPAND_DIFF_TEXT_MAX_CHARS: usize = 120;
const EXPAND_DIFF_MAX_JSON_BYTES: usize = 1400;
const EXPAND_DRIFT_NEXT_READ_LIMIT: usize = 3;
const EXPAND_DRIFT_LIST_LIMIT: usize = 3;
const EXPAND_DRIFT_TEXT_MAX_CHARS: usize = 120;
const EXPAND_DRIFT_MAX_JSON_BYTES: usize = 1400;
const EXPAND_IMPACT_TOUCH_LIMIT: usize = 3;
const EXPAND_IMPACT_TEST_LIMIT: usize = 2;
const EXPAND_IMPACT_FAILURE_LIMIT: usize = 3;
const EXPAND_IMPACT_TEXT_MAX_CHARS: usize = 120;
const EXPAND_IMPACT_MAX_JSON_BYTES: usize = 1400;
const EXPAND_TIMELINE_EVENT_LIMIT: usize = 4;
const EXPAND_TIMELINE_PATCH_LIMIT: usize = 2;
const EXPAND_TIMELINE_TEXT_MAX_CHARS: usize = 120;
const EXPAND_TIMELINE_MAX_JSON_BYTES: usize = 1400;
const EXPAND_MEMORY_LIMIT: usize = 3;
const EXPAND_MEMORY_TEXT_MAX_CHARS: usize = 120;
const EXPAND_MEMORY_MATCH_MAX_CHARS: usize = 96;
const EXPAND_MEMORY_MAX_JSON_BYTES: usize = 1400;
const COMPACT_VALIDATION_CHECK_LIMIT: usize = 2;
const COMPACT_VALIDATION_CHECK_MAX_CHARS: usize = 96;
const SPEC_BODY_IDENTIFIER_LIMIT: usize = 8;
const SPEC_IDENTIFIER_SEARCH_LIMIT: usize = 6;
const SPEC_IDENTIFIER_TEXT_LIMIT: usize = 2;
const MAX_WHY_SHORT_CHARS: usize = 120;
const TASK_BRIEF_BLOCKER_LIMIT: usize = 3;
const TASK_BRIEF_CLAIM_HOLDER_LIMIT: usize = 3;
const TASK_BRIEF_CONFLICT_LIMIT: usize = 2;
const TASK_BRIEF_OUTCOME_LIMIT: usize = 4;
const TASK_BRIEF_VALIDATION_LIMIT: usize = 4;
const TASK_BRIEF_NEXT_READ_LIMIT: usize = 2;
const TASK_BRIEF_TEXT_MAX_CHARS: usize = 120;
const TASK_BRIEF_MAX_JSON_BYTES: usize = 1500;
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

#[derive(Debug, Clone)]
struct TextSearchCandidate {
    target: SessionHandleTarget,
    matched_text: String,
}

impl QueryHost {
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
        let query_run = self.begin_query_run(session.as_ref(), kind, kind, query_text);
        match (|| -> Result<(T, Vec<QueryDiagnostic>, usize)> {
            let refresh_started = Instant::now();
            let refresh = self.observe_workspace_for_read()?;
            query_run.record_phase(
                "compact.refreshWorkspace",
                &json!({
                    "refreshPath": refresh.refresh_path,
                    "deferred": refresh.deferred,
                    "episodicReloaded": refresh.episodic_reloaded,
                    "inferenceReloaded": refresh.inference_reloaded,
                    "coordinationReloaded": refresh.coordination_reloaded,
                    "metrics": refresh.metrics.as_json(),
                }),
                refresh_started.elapsed(),
                true,
                None,
            );
            let handler_started = Instant::now();
            let (value, diagnostics) = match build(self, query_run.clone()) {
                Ok((value, diagnostics)) => {
                    query_run.record_phase(
                        "compact.handler",
                        &json!({
                            "tool": kind,
                            "diagnosticCount": diagnostics.len(),
                        }),
                        handler_started.elapsed(),
                        true,
                        None,
                    );
                    (value, diagnostics)
                }
                Err(error) => {
                    query_run.record_phase(
                        "compact.handler",
                        &json!({ "tool": kind }),
                        handler_started.elapsed(),
                        false,
                        Some(error.to_string()),
                    );
                    return Err(error);
                }
            };
            let json_value = serde_json::to_value(&value)?;
            let json_bytes = serde_json::to_vec(&json_value)?.len();
            Ok((value, diagnostics, json_bytes))
        })() {
            Ok((value, diagnostics, json_bytes)) => {
                let result_value = serde_json::to_value(&value)?;
                query_run.finish_success(
                    self.mcp_call_log_store.as_ref(),
                    &result_value,
                    diagnostics,
                    json_bytes,
                    false,
                );
                Ok(value)
            }
            Err(error) => {
                query_run.finish_error(
                    self.mcp_call_log_store.as_ref(),
                    Vec::new(),
                    error.to_string(),
                );
                Err(error)
            }
        }
    }
}

fn compact_target_view(
    session: &SessionState,
    symbol: &SymbolView,
    query: Option<&str>,
    why_override: Option<String>,
) -> AgentTargetHandleView {
    let why_short = compact_why_short(symbol, why_override.as_deref(), query);
    let location = symbol.location.as_ref();
    let handle = session.intern_target_handle(SessionHandleTarget {
        id: NodeId::new(
            symbol.id.crate_name.clone(),
            symbol.id.path.clone(),
            symbol.kind,
        ),
        lineage_id: symbol.lineage_id.clone(),
        handle_category: SessionHandleCategory::Symbol,
        name: symbol.name.clone(),
        kind: symbol.kind,
        file_path: symbol.file_path.clone(),
        query: query.map(ToString::to_string),
        why_short: why_short.clone(),
        start_line: location.map(|location| location.start_line),
        end_line: location.map(|location| location.end_line),
        start_column: location.map(|location| location.start_column),
        end_column: location.map(|location| location.end_column),
    });
    AgentTargetHandleView {
        handle,
        handle_category: agent_handle_category_view(SessionHandleCategory::Symbol),
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
        handle_category: agent_handle_category_view(target.handle_category),
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

fn is_spec_like_kind(kind: NodeKind) -> bool {
    matches!(kind, NodeKind::Document | NodeKind::MarkdownHeading)
}

fn is_docs_path(path: &str) -> bool {
    path.contains("/docs/") || path.starts_with("docs/") || path.ends_with(".md")
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

fn resolve_handle_target(
    host: &QueryHost,
    session: &SessionState,
    prism: &Prism,
    handle: &str,
    preferred_concept_lens: Option<&str>,
) -> Result<(SessionHandleTarget, bool)> {
    let mut target = session.handle_target(handle).ok_or_else(|| {
        concept_handle_followup_error(prism, handle, preferred_concept_lens).unwrap_or_else(|| {
            anyhow!("unknown handle `{handle}`; rerun prism_locate to select a target")
        })
    })?;
    let mut remapped = false;
    if is_text_fragment_target(&target) {
        return resolve_text_fragment_target(host, session, handle, target);
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
    target.start_line = symbol_view
        .location
        .as_ref()
        .map(|location| location.start_line);
    target.end_line = symbol_view
        .location
        .as_ref()
        .map(|location| location.end_line);
    target.start_column = symbol_view
        .location
        .as_ref()
        .map(|location| location.start_column);
    target.end_column = symbol_view
        .location
        .as_ref()
        .map(|location| location.end_column);
    session.refresh_target_handle(handle, target.clone());
    Ok((target, remapped))
}

fn concept_handle_followup_error(
    prism: &Prism,
    handle: &str,
    preferred_lens: Option<&str>,
) -> Option<anyhow::Error> {
    if !handle.starts_with("concept://") {
        return None;
    }
    let packet = prism.concept_by_handle(handle)?;
    let followup = preferred_lens.map_or_else(
        || {
            format!(
                "Use prism_concept with `handle`: `{handle}` and an appropriate `lens` (`open`, `workset`, `validation`, `timeline`, or `memory`)."
            )
        },
        |lens| format!("Use prism_concept with `handle`: `{handle}` and `lens`: `{lens}`."),
    );
    Some(anyhow!(
        "handle `{handle}` resolves to concept `{}` rather than a compact session target handle. {followup}",
        packet.canonical_name
    ))
}

fn is_text_fragment_target(target: &SessionHandleTarget) -> bool {
    target.handle_category == SessionHandleCategory::TextFragment
}

fn agent_handle_category_view(category: SessionHandleCategory) -> AgentHandleCategoryView {
    match category {
        SessionHandleCategory::Symbol => AgentHandleCategoryView::Symbol,
        SessionHandleCategory::TextFragment => AgentHandleCategoryView::TextFragment,
        SessionHandleCategory::Concept => AgentHandleCategoryView::Concept,
    }
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
        PrismExpandKindInput::Health => AgentExpandKind::Health,
        PrismExpandKindInput::Validation => AgentExpandKind::Validation,
        PrismExpandKindInput::Impact => AgentExpandKind::Impact,
        PrismExpandKindInput::Timeline => AgentExpandKind::Timeline,
        PrismExpandKindInput::Memory => AgentExpandKind::Memory,
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

    use super::open::budgeted_open_result;
    use super::workset::{budgeted_workset_result, compact_string_list};
    use super::*;

    fn handle_view(index: usize, file_path: Option<&str>) -> AgentTargetHandleView {
        AgentTargetHandleView {
            handle: format!("handle:{index}"),
            handle_category: AgentHandleCategoryView::Symbol,
            kind: NodeKind::Function,
            path: format!("demo::module_{index}::very_long_function_name_for_budget_tests"),
            name: format!("very_long_function_name_for_budget_tests_{index}"),
            why_short: "Matched ranking hint from a compact budget regression test.".to_string(),
            file_path: file_path.map(ToString::to_string),
        }
    }

    fn handle_target(index: usize, file_path: Option<&str>) -> SessionHandleTarget {
        SessionHandleTarget {
            id: NodeId::new(
                "demo",
                format!("demo::module_{index}::very_long_function_name_for_budget_tests"),
                NodeKind::Function,
            ),
            lineage_id: None,
            handle_category: SessionHandleCategory::Symbol,
            name: format!("very_long_function_name_for_budget_tests_{index}"),
            kind: NodeKind::Function,
            file_path: file_path.map(ToString::to_string),
            query: None,
            why_short: "Matched ranking hint from a compact budget regression test.".to_string(),
            start_line: None,
            end_line: None,
            start_column: None,
            end_column: None,
        }
    }

    fn open_result(
        related_handles: Option<Vec<AgentTargetHandleView>>,
        text_len: usize,
    ) -> AgentOpenResultView {
        AgentOpenResultView {
            handle: "handle:primary".to_string(),
            handle_category: AgentHandleCategoryView::Symbol,
            file_path: "src/main.rs".to_string(),
            start_line: 1,
            end_line: 12,
            text: "x".repeat(text_len),
            truncated: false,
            remapped: false,
            next_action: None,
            promoted_handle: None,
            related_handles,
            suggested_actions: Vec::new(),
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
            &handle_target(1, Some("src/main.rs")),
            handle_view(1, Some("src/main.rs")),
            vec![handle_view(2, Some("src/helper.rs"))],
            vec![],
            "Primary target from `main`. Function in src/main.rs".to_string(),
            false,
        )
        .expect("budgeted workset should serialize");

        assert!(!result.truncated);
        assert!(result
            .next_action
            .as_deref()
            .is_some_and(|value| value.contains("prism_open")));
        assert!(workset_json_bytes(&result).expect("json bytes") <= WORKSET_MAX_JSON_BYTES);
    }

    #[test]
    fn workset_budget_trims_context_before_exceeding_budget() {
        let long_path =
            "src/really/deeply/nested/module/with/a/very/long/path/for/compact/workset/tests.rs";
        let result = budgeted_workset_result(
            &handle_target(1, Some(long_path)),
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
        assert!(result
            .next_action
            .as_deref()
            .is_some_and(|value| value.contains("prism_open")));
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
