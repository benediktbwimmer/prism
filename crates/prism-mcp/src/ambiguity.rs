use anyhow::Result;
use prism_ir::{AnchorRef, CoordinationTaskId, NodeKind};
use prism_js::{QueryDiagnostic, SuggestedQueryView, SymbolView};
use prism_query::Prism;
use rmcp::schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};

use crate::{dedupe_suggested_queries, read_context_queries, search_queries};

const MAX_AMBIGUITY_CANDIDATES: usize = 5;

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub(crate) struct SearchAmbiguityView {
    pub(crate) query: String,
    pub(crate) strategy: String,
    pub(crate) owner_kind: Option<String>,
    pub(crate) path: Option<String>,
    pub(crate) module: Option<String>,
    pub(crate) task_id: Option<String>,
    pub(crate) prefer_callable_code: Option<bool>,
    pub(crate) prefer_editable_targets: Option<bool>,
    pub(crate) prefer_behavioral_owners: Option<bool>,
    pub(crate) candidate_count: usize,
    pub(crate) returned: Option<SymbolView>,
    pub(crate) why: Vec<String>,
    pub(crate) candidates: Vec<AmbiguityCandidateView>,
    pub(crate) suggested_queries: Vec<SuggestedQueryView>,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub(crate) struct AmbiguityCandidateView {
    pub(crate) symbol: SymbolView,
    pub(crate) module: Option<String>,
    pub(crate) bucket: String,
    pub(crate) score: i32,
    pub(crate) reasons: Vec<String>,
    pub(crate) suggested_queries: Vec<SuggestedQueryView>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum TaskScopeMode {
    Prefer,
    Filter,
}

#[derive(Debug, Clone)]
pub(crate) struct SearchAmbiguityContext<'a> {
    pub(crate) query: &'a str,
    pub(crate) strategy: &'a str,
    pub(crate) owner_kind: Option<&'a str>,
    pub(crate) path: Option<&'a str>,
    pub(crate) module: Option<&'a str>,
    pub(crate) task_id: Option<&'a str>,
    pub(crate) prefer_callable_code: Option<bool>,
    pub(crate) prefer_editable_targets: Option<bool>,
    pub(crate) prefer_behavioral_owners: Option<bool>,
    pub(crate) task_scope_mode: TaskScopeMode,
}

#[derive(Debug, Clone)]
struct RankedCandidate {
    symbol: SymbolView,
    module: Option<String>,
    bucket: CandidateBucket,
    score: i32,
    reasons: Vec<String>,
    exact_name_match: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum CandidateBucket {
    Implementation,
    Surface,
    ExampleFixture,
    Tests,
    Container,
    Other,
}

#[derive(Debug, Clone, Copy)]
struct SearchIntent {
    prefer_callable_code: bool,
    prefer_editable_targets: bool,
    prefer_behavioral_owners: bool,
}

#[derive(Debug, Clone)]
pub(crate) struct TaskScope {
    task_id: String,
    nodes: Vec<String>,
    lineages: Vec<String>,
}

pub(crate) fn apply_module_filter(results: &mut Vec<SymbolView>, module: Option<&str>) {
    let Some(module) = module.filter(|value| !value.is_empty()) else {
        return;
    };
    results.retain(|symbol| matches_module(symbol, module));
}

pub(crate) fn rank_search_results(
    prism: &Prism,
    results: &mut Vec<SymbolView>,
    context: &SearchAmbiguityContext<'_>,
    emit_for_search: bool,
) -> Result<Option<SearchAmbiguityView>> {
    let task_scope = context
        .task_id
        .filter(|value| !value.is_empty())
        .and_then(|task_id| build_task_scope(prism, task_id));
    let explicit_task_scope = context.task_scope_mode == TaskScopeMode::Filter;

    if explicit_task_scope {
        let mut scoped = results
            .iter()
            .filter(|candidate| candidate_matches_task_scope(candidate, task_scope.as_ref()))
            .cloned()
            .collect::<Vec<_>>();
        if !scoped.is_empty() {
            *results = std::mem::take(&mut scoped);
        }
    }

    let mut ranked = results
        .drain(..)
        .map(|symbol| rank_candidate(prism, symbol, context, task_scope.as_ref()))
        .collect::<Vec<_>>();
    ranked.sort_by(|left, right| {
        right
            .score
            .cmp(&left.score)
            .then_with(|| left.symbol.id.path.cmp(&right.symbol.id.path))
    });

    *results = ranked
        .iter()
        .map(|candidate| candidate.symbol.clone())
        .collect();

    if ranked.len() <= 1 {
        return Ok(None);
    }

    if emit_for_search && !search_is_ambiguous(&ranked) {
        return Ok(None);
    }

    let why = ambiguity_why(context, task_scope.as_ref(), &ranked, explicit_task_scope);
    let candidates = ranked
        .iter()
        .take(MAX_AMBIGUITY_CANDIDATES)
        .map(|candidate| ambiguity_candidate_view(context, candidate))
        .collect::<Vec<_>>();
    let suggested_queries = ambiguity_queries(context, &candidates);
    Ok(Some(SearchAmbiguityView {
        query: context.query.to_string(),
        strategy: context.strategy.to_string(),
        owner_kind: context.owner_kind.map(str::to_string),
        path: context.path.map(str::to_string),
        module: context.module.map(str::to_string),
        task_id: task_scope.as_ref().map(|scope| scope.task_id.clone()),
        prefer_callable_code: context.prefer_callable_code,
        prefer_editable_targets: context.prefer_editable_targets,
        prefer_behavioral_owners: context.prefer_behavioral_owners,
        candidate_count: ranked.len(),
        returned: ranked.first().map(|candidate| candidate.symbol.clone()),
        why,
        candidates,
        suggested_queries,
    }))
}

pub(crate) fn ambiguity_diagnostic_data(
    ambiguity: &SearchAmbiguityView,
    next_action: &str,
) -> Value {
    json!({
        "query": ambiguity.query,
        "candidateCount": ambiguity.candidate_count,
        "returned": ambiguity.returned,
        "ambiguity": ambiguity,
        "suggestedQueries": ambiguity.suggested_queries,
        "nextAction": next_action,
    })
}

pub(crate) fn weak_search_match_reason(ambiguity: &SearchAmbiguityView) -> Option<&'static str> {
    let top = ambiguity.candidates.first()?;
    if top.bucket == "container" && top.score <= 0 {
        Some("Top candidates are generic containers or support modules rather than strong implementation matches.")
    } else if top.bucket == "tests" {
        Some("Top candidates are test-only matches rather than likely implementation targets.")
    } else if top.score <= 0 {
        Some("The strongest remaining candidate is still weak after ranking and likely needs more intent.")
    } else {
        None
    }
}

pub(crate) fn weak_search_match_diagnostic_data(
    ambiguity: &SearchAmbiguityView,
    reason: &str,
    next_action: &str,
) -> Value {
    json!({
        "query": ambiguity.query,
        "candidateCount": ambiguity.candidate_count,
        "returned": ambiguity.returned,
        "ambiguity": ambiguity,
        "reason": reason,
        "suggestedQueries": ambiguity.suggested_queries,
        "nextAction": next_action,
    })
}

pub(crate) fn search_ambiguity_from_diagnostics(
    diagnostics: &[QueryDiagnostic],
) -> Option<SearchAmbiguityView> {
    diagnostics.iter().find_map(|diagnostic| {
        diagnostic
            .data
            .as_ref()
            .and_then(|data| data.get("ambiguity"))
            .cloned()
            .and_then(|value| serde_json::from_value(value).ok())
    })
}

fn search_is_ambiguous(ranked: &[RankedCandidate]) -> bool {
    let exact_matches = ranked
        .iter()
        .filter(|candidate| candidate.exact_name_match)
        .count();
    if exact_matches > 1 {
        return true;
    }
    ranked
        .first()
        .zip(ranked.get(1))
        .is_some_and(|(first, second)| first.score.saturating_sub(second.score) <= 12)
}

fn rank_candidate(
    prism: &Prism,
    symbol: SymbolView,
    context: &SearchAmbiguityContext<'_>,
    task_scope: Option<&TaskScope>,
) -> RankedCandidate {
    let mut score = 0;
    let mut reasons = Vec::new();
    let normalized_query = context.query.trim();
    let query_lower = normalized_query.to_ascii_lowercase();
    let query_stem = identifier_stem(&query_lower);
    let path = symbol.id.path.as_str();
    let leaf = path.rsplit("::").next().unwrap_or(path);
    let leaf_lower = leaf.to_ascii_lowercase();
    let name_lower = symbol.name.to_ascii_lowercase();
    let path_lower = path.to_ascii_lowercase();
    let direct_match_rank = direct_symbol_match_rank(&symbol, &query_lower);
    let exact_name_match = symbol.name == normalized_query || leaf == normalized_query;
    let bare_identifier_query = is_broad_identifier_query(normalized_query);
    let broad_identifier_query = context.strategy == "direct" && bare_identifier_query;
    let intent = SearchIntent::from_context(context, bare_identifier_query);
    let bucket = classify_candidate_bucket(&symbol);

    if path == normalized_query {
        score += 140;
        reasons.push("Exact symbol path match.".to_string());
    } else if exact_name_match {
        score += 100;
        reasons.push("Exact symbol-name match.".to_string());
    } else if symbol.name.eq_ignore_ascii_case(normalized_query)
        || leaf.eq_ignore_ascii_case(normalized_query)
    {
        score += 90;
        reasons.push("Case-insensitive symbol-name match.".to_string());
    } else if path.ends_with(&format!("::{normalized_query}")) {
        score += 70;
        reasons.push("Leaf path segment matches the query.".to_string());
    }

    if !exact_name_match {
        let leaf_tokens = identifier_tokens(&leaf_lower);
        let name_tokens = identifier_tokens(&name_lower);
        if leaf_tokens.iter().any(|token| *token == query_lower)
            || name_tokens.iter().any(|token| *token == query_lower)
        {
            score += 55;
            reasons.push("Identifier token matches the query.".to_string());
        } else if leaf_tokens
            .iter()
            .chain(name_tokens.iter())
            .any(|token| identifier_stem(token) == query_stem)
        {
            score += 40;
            reasons.push("Identifier token stem matches the query.".to_string());
        } else if leaf_lower.contains(&query_lower) || name_lower.contains(&query_lower) {
            score += 22;
            reasons.push("Identifier contains the query text.".to_string());
        }
    }

    if let Some(module) = context.module.filter(|value| !value.is_empty()) {
        if matches_module(&symbol, module) {
            score += 50;
            reasons.push(format!("Within requested module `{module}`."));
        }
    } else if let Some(module) = module_path(&symbol) {
        if path == module {
            score += 10;
        }
    }

    match symbol.kind {
        NodeKind::Function | NodeKind::Method => {
            score += 24;
            reasons.push("Callable code preferred for broad symbol queries.".to_string());
        }
        NodeKind::Struct
        | NodeKind::Enum
        | NodeKind::Trait
        | NodeKind::Impl
        | NodeKind::TypeAlias => {
            score += 16;
            reasons.push("Implementation type preferred over module/document matches.".to_string());
        }
        NodeKind::Field => {
            score += 4;
        }
        NodeKind::Module if !exact_name_match => {
            score -= 8;
            reasons.push(
                "Module match slightly de-prioritized behind concrete code targets.".to_string(),
            );
        }
        NodeKind::Document
        | NodeKind::Package
        | NodeKind::Workspace
        | NodeKind::MarkdownHeading
        | NodeKind::JsonKey
        | NodeKind::TomlKey
        | NodeKind::YamlKey
            if !exact_name_match =>
        {
            score -= 12;
            reasons.push(
                "Non-code container match de-prioritized for a broad symbol query.".to_string(),
            );
        }
        _ => {}
    }

    if broad_identifier_query {
        match symbol.kind {
            NodeKind::Module
                if direct_match_rank.is_some()
                    && !(intent.prefer_callable_code && intent.prefer_behavioral_owners) =>
            {
                score += 34;
                reasons.push(
                    "Direct module owner match preferred over child symbols that only inherit the query through their path.".to_string(),
                );
            }
            NodeKind::Module if exact_name_match => {
                score += 14;
                reasons.push("Module/file owner preferred for a bare noun query.".to_string());
            }
            NodeKind::Field if exact_name_match => {
                score -= 20;
                reasons.push(
                    "Bare field match de-prioritized behind owning code or module targets."
                        .to_string(),
                );
            }
            NodeKind::Document
            | NodeKind::Package
            | NodeKind::Workspace
            | NodeKind::MarkdownHeading
            | NodeKind::JsonKey
            | NodeKind::TomlKey
            | NodeKind::YamlKey
                if exact_name_match =>
            {
                score -= 24;
                reasons.push(
                    "Exact non-code container match de-prioritized for a bare noun query."
                        .to_string(),
                );
            }
            _ => {}
        }
    }

    if broad_identifier_query
        && direct_match_rank.is_none()
        && path_inherits_query(&path_lower, &query_lower)
    {
        if matches!(
            symbol.kind,
            NodeKind::Function
                | NodeKind::Method
                | NodeKind::Struct
                | NodeKind::Enum
                | NodeKind::Trait
                | NodeKind::Impl
                | NodeKind::TypeAlias
                | NodeKind::Field
        ) {
            score -= 64;
            reasons.push(
                "Child symbol only inherited the query through its containing path; owner modules match first.".to_string(),
            );
        }
    }

    if bare_identifier_query
        && identifier_stem(&query_lower) == "helper"
        && !exact_name_match
        && is_generic_helper_utility_symbol(&symbol)
    {
        score -= 220;
        reasons.push(
            "Generic helper-plumbing utilities de-prioritized behind task-facing helper code."
                .to_string(),
        );
    }

    if bare_identifier_query
        && identifier_stem(&query_lower) == "helper"
        && !exact_name_match
        && is_internal_plain_helpers_symbol(&symbol)
    {
        score -= 80;
        reasons.push(
            "Internal helpers.rs plumbing de-prioritized behind task-facing helper entrypoints."
                .to_string(),
        );
    }

    if intent.prefer_callable_code {
        match symbol.kind {
            NodeKind::Function | NodeKind::Method => {
                score += 18;
                reasons.push("Search intent prefers callable implementation code.".to_string());
            }
            NodeKind::Module => {
                let penalty = if broad_identifier_query && intent.prefer_behavioral_owners {
                    if exact_name_match {
                        72
                    } else {
                        24
                    }
                } else if exact_name_match {
                    if broad_identifier_query {
                        32
                    } else {
                        12
                    }
                } else {
                    10
                };
                if penalty > 0 {
                    score -= penalty;
                    reasons.push(
                        if broad_identifier_query && intent.prefer_behavioral_owners {
                            "Explicit callable behavioral-owner search de-prioritized module containers behind concrete owner code.".to_string()
                        } else {
                            "Search intent de-prioritized module containers behind callable code."
                                .to_string()
                        },
                    );
                }
            }
            _ => {}
        }
    }

    if intent.prefer_editable_targets {
        if is_editable_target(&symbol) {
            score += 14;
            reasons.push("Search intent prefers editable implementation targets.".to_string());
        } else if matches!(
            symbol.kind,
            NodeKind::Document
                | NodeKind::Package
                | NodeKind::Workspace
                | NodeKind::MarkdownHeading
                | NodeKind::JsonKey
                | NodeKind::TomlKey
                | NodeKind::YamlKey
        ) {
            score -= 14;
            reasons.push(
                "Search intent de-prioritized container/document matches behind editable code."
                    .to_string(),
            );
        } else if matches!(symbol.kind, NodeKind::Module) && !exact_name_match {
            score -= 10;
            reasons.push(
                "Search intent de-prioritized non-exact modules behind editable code targets."
                    .to_string(),
            );
        }
    }

    if bare_identifier_query
        && intent.prefer_callable_code
        && intent.prefer_editable_targets
        && !exact_name_match
        && is_facade_file_symbol(&symbol)
    {
        score -= 22;
        reasons.push(
            "Broad implementation search de-prioritized facade entrypoints from lib.rs/main.rs behind deeper owned code.".to_string(),
        );
    }

    if bare_identifier_query && intent.prefer_behavioral_owners {
        if !query_mentions_schema_or_examples(&query_lower)
            && is_schema_example_surface_symbol(&symbol)
        {
            score -= 120;
            reasons.push(
                "Schema/example helpers de-prioritized for a broad implementation search."
                    .to_string(),
            );
        } else if !query_mentions_read_surface(&query_lower)
            && is_read_surface_wrapper_symbol(&symbol)
        {
            score -= 88;
            reasons.push(
                "Session-surface view/resource wrappers de-prioritized behind deeper read implementations."
                    .to_string(),
            );
        }
    }

    if bare_identifier_query {
        if let Some((adjustment, reason)) =
            bucket_adjustment(bucket, &query_lower, &intent, exact_name_match)
        {
            score += adjustment;
            reasons.push(reason.to_string());
        }
    }

    if let Some(task_match) = candidate_task_match(&symbol, task_scope) {
        score += match task_match {
            TaskMatch::ExactNode => 120,
            TaskMatch::SameLineage => 90,
        };
        reasons.push(match task_match {
            TaskMatch::ExactNode => {
                format!(
                    "Matches the exact semantic task scope for `{}`.",
                    task_scope.expect("task scope should exist").task_id
                )
            }
            TaskMatch::SameLineage => {
                format!(
                    "Matches the current lineage already in task scope for `{}`.",
                    task_scope.expect("task scope should exist").task_id
                )
            }
        });
    }

    if intent.prefer_behavioral_owners {
        if let Some(owner_hint) = symbol.owner_hint.as_ref() {
            let owner_boost = if context.strategy == "behavioral" {
                owner_hint.score.min(24) as i32
            } else if broad_identifier_query
                && intent.prefer_callable_code
                && matches!(symbol.kind, NodeKind::Function | NodeKind::Method)
            {
                owner_hint.score.min(24) as i32 + 18
            } else {
                owner_hint.score.min(14) as i32
            };
            score += owner_boost;
            reasons.push(format!(
                "Search intent preferred {} owner hints (score {}, applied {}).",
                owner_hint.kind, owner_hint.score, owner_boost
            ));
        }
    }

    if !query_lower.contains("test") && is_test_symbol(&symbol) {
        let penalty = if bare_identifier_query { 120 } else { 45 };
        score -= penalty;
        reasons.push(if bare_identifier_query {
            "Test-only symbol strongly de-prioritized for a broad non-test query.".to_string()
        } else {
            "Test-only symbol de-prioritized for a non-test query.".to_string()
        });
    }
    if bare_identifier_query
        && !query_lower.contains("test")
        && !query_lower.contains("replay")
        && !query_lower.contains("fixture")
    {
        if is_dependency_metadata_symbol(&symbol) {
            score -= 420;
            reasons.push(
                "Dependency lockfile and vendored package metadata strongly de-prioritized for a broad implementation search."
                    .to_string(),
            );
        } else if is_query_replay_case_symbol(&symbol) {
            score -= 520;
            reasons.push(
                "Query replay harness helpers strongly de-prioritized for a broad implementation search."
                    .to_string(),
            );
        } else if is_replay_or_fixture_symbol(&symbol) {
            score -= 80;
            reasons.push(
                "Replay/fixture scaffolding strongly de-prioritized for a broad implementation search."
                    .to_string(),
            );
        }
    }

    let depth = path.matches("::").count() as i32;
    score += (8 - depth).max(0);
    if symbol.location.is_some() {
        score += 2;
    }
    if prism
        .lineage_of(&prism_ir::NodeId::new(
            symbol.id.crate_name.clone(),
            symbol.id.path.clone(),
            symbol.kind,
        ))
        .is_some()
    {
        score += 1;
    }

    RankedCandidate {
        module: module_path(&symbol),
        symbol,
        bucket,
        score,
        reasons,
        exact_name_match,
    }
}

pub(crate) fn is_broad_identifier_query(query: &str) -> bool {
    let trimmed = query.trim();
    !trimmed.is_empty()
        && !trimmed.contains("::")
        && trimmed
            .chars()
            .all(|ch| ch.is_ascii_alphanumeric() || ch == '_')
}

fn ambiguity_candidate_view(
    context: &SearchAmbiguityContext<'_>,
    candidate: &RankedCandidate,
) -> AmbiguityCandidateView {
    AmbiguityCandidateView {
        symbol: candidate.symbol.clone(),
        module: candidate.module.clone(),
        bucket: candidate.bucket.label().to_string(),
        score: candidate.score,
        reasons: candidate.reasons.clone(),
        suggested_queries: candidate_queries(
            context.query,
            &candidate.symbol,
            candidate.module.as_deref(),
            context,
        ),
    }
}

fn ambiguity_queries(
    context: &SearchAmbiguityContext<'_>,
    candidates: &[AmbiguityCandidateView],
) -> Vec<SuggestedQueryView> {
    let mut suggestions = search_queries(context.query);
    if let Some(first) = candidates.first() {
        if let Some(path) = first.symbol.file_path.as_deref() {
            suggestions.push(SuggestedQueryView {
                label: "Exact Path Search".to_string(),
                query: search_query_call(
                    context.query,
                    Some(path),
                    None,
                    Some(&first.symbol.kind.to_string()),
                    Some(context.strategy),
                    context.prefer_callable_code,
                    context.prefer_editable_targets,
                    context.prefer_behavioral_owners,
                    context.owner_kind,
                    context.task_id,
                ),
                why: "Narrow directly to the chosen file path before retrying the search."
                    .to_string(),
            });
        }
        if let Some(module) = first.module.as_deref() {
            suggestions.push(SuggestedQueryView {
                label: "Module Search".to_string(),
                query: search_query_call(
                    context.query,
                    None,
                    Some(module),
                    Some(&first.symbol.kind.to_string()),
                    Some(context.strategy),
                    context.prefer_callable_code,
                    context.prefer_editable_targets,
                    context.prefer_behavioral_owners,
                    context.owner_kind,
                    context.task_id,
                ),
                why: "Retry within the best candidate's module instead of the whole workspace."
                    .to_string(),
            });
        }
        suggestions.extend(first.suggested_queries.iter().take(2).cloned());
    }
    if let Some(task_id) = context.task_id.filter(|value| !value.is_empty()) {
        suggestions.push(SuggestedQueryView {
            label: "Task-Scoped Search".to_string(),
            query: search_query_call(
                context.query,
                context.path,
                context.module,
                None,
                Some(context.strategy),
                context.prefer_callable_code,
                context.prefer_editable_targets,
                context.prefer_behavioral_owners,
                context.owner_kind,
                Some(task_id),
            ),
            why: "Retry the search inside the semantic scope of the active coordination task."
                .to_string(),
        });
    }
    dedupe_suggested_queries(suggestions)
}

fn ambiguity_why(
    context: &SearchAmbiguityContext<'_>,
    task_scope: Option<&TaskScope>,
    ranked: &[RankedCandidate],
    explicit_task_scope: bool,
) -> Vec<String> {
    let mut why = vec![
        "PRISM ranked candidates by exact name/path match, module scope, task scope, owner hints, and test-versus-implementation heuristics.".to_string(),
    ];
    let exact_matches = ranked
        .iter()
        .filter(|candidate| candidate.exact_name_match)
        .count();
    if exact_matches > 1 {
        why.push(format!(
            "{exact_matches} exact symbol-name matches remain after ranking."
        ));
    }
    if let Some(module) = context.module.filter(|value| !value.is_empty()) {
        why.push(format!("Applied explicit module filter `{module}`."));
    }
    if let Some(path) = context.path.filter(|value| !value.is_empty()) {
        why.push(format!("Applied path filter `{path}` before ranking."));
    }
    let bare_identifier_query = is_broad_identifier_query(context.query);
    let intent = SearchIntent::from_context(context, bare_identifier_query);
    if context.prefer_callable_code == Some(true) {
        why.push("Applied explicit callable-code preference for ranking.".to_string());
    } else if intent.prefer_callable_code && context.strategy == "direct" {
        why.push("Broad direct query defaulted toward callable implementation code.".to_string());
    }
    if context.prefer_editable_targets == Some(true) {
        why.push("Applied explicit editable-target preference for ranking.".to_string());
    } else if intent.prefer_editable_targets && context.strategy == "direct" {
        why.push(
            "Broad direct query defaulted toward editable implementation targets.".to_string(),
        );
    }
    if context.prefer_behavioral_owners == Some(true) {
        why.push("Applied explicit behavioral-owner preference for ranking.".to_string());
    } else if context.strategy == "behavioral" {
        why.push("Behavioral strategy preferred semantic owner hints during ranking.".to_string());
    }
    if bare_identifier_query {
        let bucket_summary = summarize_candidate_buckets(ranked);
        if !bucket_summary.is_empty() {
            why.push(format!(
                "Broad-query bucketing grouped candidates by likely intent: {}.",
                bucket_summary
            ));
        }
    }
    if let Some(scope) = task_scope {
        let scoped_matches = ranked
            .iter()
            .filter(|candidate| candidate_matches_task_scope(&candidate.symbol, Some(scope)))
            .count();
        if explicit_task_scope && scoped_matches > 0 {
            why.push(format!(
                "Applied semantic task narrowing from `{}` to {scoped_matches} in-scope candidates.",
                scope.task_id
            ));
        } else if scoped_matches > 0 {
            why.push(format!(
                "Used semantic task context from `{}` to prefer in-scope candidates.",
                scope.task_id
            ));
        } else {
            why.push(format!(
                "No direct semantic task matches were found for `{}`, so PRISM fell back to global ranking.",
                scope.task_id
            ));
        }
    }
    why
}

fn candidate_queries(
    query: &str,
    symbol: &SymbolView,
    module: Option<&str>,
    context: &SearchAmbiguityContext<'_>,
) -> Vec<SuggestedQueryView> {
    let mut suggestions = read_context_queries(&prism_ir::NodeId::new(
        symbol.id.crate_name.clone(),
        symbol.id.path.clone(),
        symbol.kind,
    ));
    suggestions.truncate(2);
    if let Some(module) = module {
        suggestions.push(SuggestedQueryView {
            label: "Narrow To Module".to_string(),
            query: search_query_call(
                query,
                None,
                Some(module),
                Some(&symbol.kind.to_string()),
                Some(context.strategy),
                context.prefer_callable_code,
                context.prefer_editable_targets,
                context.prefer_behavioral_owners,
                context.owner_kind,
                context.task_id,
            ),
            why: "Retry the search inside this candidate's module path.".to_string(),
        });
    }
    suggestions
}

fn search_query_call(
    query: &str,
    path: Option<&str>,
    module: Option<&str>,
    kind: Option<&str>,
    strategy: Option<&str>,
    prefer_callable_code: Option<bool>,
    prefer_editable_targets: Option<bool>,
    prefer_behavioral_owners: Option<bool>,
    owner_kind: Option<&str>,
    task_id: Option<&str>,
) -> String {
    let query_json = serde_json::to_string(query).expect("query should serialize");
    let mut parts = vec!["limit: 5".to_string()];
    if let Some(strategy) = strategy.filter(|value| !value.is_empty()) {
        parts.push(format!(
            "strategy: {}",
            serde_json::to_string(strategy).expect("strategy should serialize")
        ));
    }
    if let Some(prefer_callable_code) = prefer_callable_code {
        parts.push(format!("preferCallableCode: {prefer_callable_code}"));
    }
    if let Some(prefer_editable_targets) = prefer_editable_targets {
        parts.push(format!("preferEditableTargets: {prefer_editable_targets}"));
    }
    if let Some(prefer_behavioral_owners) = prefer_behavioral_owners {
        parts.push(format!(
            "preferBehavioralOwners: {prefer_behavioral_owners}"
        ));
    }
    if let Some(owner_kind) = owner_kind.filter(|value| !value.is_empty()) {
        parts.push(format!(
            "ownerKind: {}",
            serde_json::to_string(owner_kind).expect("owner kind should serialize")
        ));
    }
    if let Some(kind) = kind.filter(|value| !value.is_empty()) {
        parts.push(format!(
            "kind: {}",
            serde_json::to_string(kind).expect("kind should serialize")
        ));
    }
    if let Some(path) = path.filter(|value| !value.is_empty()) {
        parts.push(format!(
            "path: {}",
            serde_json::to_string(path).expect("path should serialize")
        ));
        parts.push("pathMode: \"exact\"".to_string());
    }
    if let Some(module) = module.filter(|value| !value.is_empty()) {
        parts.push(format!(
            "module: {}",
            serde_json::to_string(module).expect("module should serialize")
        ));
    }
    if let Some(task_id) = task_id.filter(|value| !value.is_empty()) {
        parts.push(format!(
            "taskId: {}",
            serde_json::to_string(task_id).expect("task id should serialize")
        ));
    }
    format!(
        "return prism.search({query_json}, {{ {} }});",
        parts.join(", ")
    )
}

impl SearchIntent {
    fn from_context(context: &SearchAmbiguityContext<'_>, bare_identifier_query: bool) -> Self {
        Self {
            prefer_callable_code: context
                .prefer_callable_code
                .unwrap_or(bare_identifier_query),
            prefer_editable_targets: context
                .prefer_editable_targets
                .unwrap_or(bare_identifier_query),
            prefer_behavioral_owners: context
                .prefer_behavioral_owners
                .unwrap_or(context.strategy == "behavioral"),
        }
    }
}

impl CandidateBucket {
    fn label(self) -> &'static str {
        match self {
            CandidateBucket::Implementation => "implementation",
            CandidateBucket::Surface => "surface",
            CandidateBucket::ExampleFixture => "example_fixture",
            CandidateBucket::Tests => "tests",
            CandidateBucket::Container => "container",
            CandidateBucket::Other => "other",
        }
    }

    fn summary_label(self) -> &'static str {
        match self {
            CandidateBucket::Implementation => "implementation owners",
            CandidateBucket::Surface => "surface wrappers",
            CandidateBucket::ExampleFixture => "examples and fixtures",
            CandidateBucket::Tests => "tests",
            CandidateBucket::Container => "containers",
            CandidateBucket::Other => "other",
        }
    }
}

fn is_editable_target(symbol: &SymbolView) -> bool {
    matches!(
        symbol.kind,
        NodeKind::Function
            | NodeKind::Method
            | NodeKind::Struct
            | NodeKind::Enum
            | NodeKind::Trait
            | NodeKind::Impl
            | NodeKind::TypeAlias
            | NodeKind::Field
    )
}

fn classify_candidate_bucket(symbol: &SymbolView) -> CandidateBucket {
    if is_test_symbol(symbol) {
        CandidateBucket::Tests
    } else if is_schema_example_surface_symbol(symbol)
        || is_query_replay_case_symbol(symbol)
        || is_replay_or_fixture_symbol(symbol)
    {
        CandidateBucket::ExampleFixture
    } else if is_read_surface_wrapper_symbol(symbol) || is_facade_file_symbol(symbol) {
        CandidateBucket::Surface
    } else if matches!(
        symbol.kind,
        NodeKind::Module
            | NodeKind::Document
            | NodeKind::Package
            | NodeKind::Workspace
            | NodeKind::MarkdownHeading
            | NodeKind::JsonKey
            | NodeKind::TomlKey
            | NodeKind::YamlKey
    ) {
        CandidateBucket::Container
    } else if is_editable_target(symbol)
        || matches!(
            symbol.kind,
            NodeKind::Function
                | NodeKind::Method
                | NodeKind::Struct
                | NodeKind::Enum
                | NodeKind::Trait
                | NodeKind::Impl
                | NodeKind::TypeAlias
        )
    {
        CandidateBucket::Implementation
    } else {
        CandidateBucket::Other
    }
}

fn bucket_adjustment(
    bucket: CandidateBucket,
    query_lower: &str,
    intent: &SearchIntent,
    exact_name_match: bool,
) -> Option<(i32, &'static str)> {
    let implementation_intent = intent.prefer_callable_code
        || intent.prefer_editable_targets
        || intent.prefer_behavioral_owners;
    match bucket {
        CandidateBucket::Implementation => Some((
            if implementation_intent { 28 } else { 0 },
            if implementation_intent {
                "Broad-query bucketing promoted likely implementation owners ahead of wrappers and containers."
            } else {
                "Broad-query bucketing classified this candidate as implementation-owned code."
            },
        )),
        CandidateBucket::Surface if !query_mentions_read_surface(query_lower) => Some((
            if implementation_intent { -44 } else { -18 },
            "Broad-query bucketing de-prioritized surface wrappers behind deeper implementation owners.",
        )),
        CandidateBucket::ExampleFixture if !query_mentions_schema_or_examples(query_lower) => {
            Some((
                if implementation_intent { -56 } else { -26 },
                "Broad-query bucketing de-prioritized examples and fixtures behind implementation code.",
            ))
        }
        CandidateBucket::Tests if !query_lower.contains("test") => Some((
            if implementation_intent { -24 } else { -8 },
            "Broad-query bucketing de-prioritized tests for a non-test query.",
        )),
        CandidateBucket::Container if exact_name_match && !implementation_intent => Some((
            8,
            "Broad-query bucketing preserved the exact owning container for a bare noun query.",
        )),
        CandidateBucket::Container if implementation_intent => Some((
            -18,
            "Broad-query bucketing de-prioritized containers behind implementation owners.",
        )),
        _ => None,
    }
}

fn summarize_candidate_buckets(ranked: &[RankedCandidate]) -> String {
    let mut ordered = Vec::<CandidateBucket>::new();
    let mut counts = Vec::<(CandidateBucket, usize)>::new();
    for candidate in ranked.iter().take(5) {
        if let Some((_, count)) = counts
            .iter_mut()
            .find(|(bucket, _)| *bucket == candidate.bucket)
        {
            *count += 1;
        } else {
            ordered.push(candidate.bucket);
            counts.push((candidate.bucket, 1));
        }
    }
    ordered
        .into_iter()
        .filter_map(|bucket| {
            counts
                .iter()
                .find(|(candidate_bucket, _)| *candidate_bucket == bucket)
                .map(|(_, count)| format!("{} ({count})", bucket.summary_label()))
        })
        .collect::<Vec<_>>()
        .join(", ")
}

pub(crate) fn build_task_scope(prism: &Prism, task_id: &str) -> Option<TaskScope> {
    let task_id = task_id.trim();
    if task_id.is_empty() {
        return None;
    }
    let coord_task_id = CoordinationTaskId::new(task_id.to_string());
    let task = prism.coordination_task(&coord_task_id)?;
    let mut nodes = task
        .anchors
        .iter()
        .filter_map(|anchor| match anchor {
            AnchorRef::Node(node) => Some(node.clone()),
            _ => None,
        })
        .collect::<Vec<_>>();
    if let Some(intent) = prism.task_intent(&coord_task_id) {
        nodes.extend(intent.specs);
        nodes.extend(intent.implementations);
        nodes.extend(intent.validations);
        nodes.extend(intent.related);
    }
    let mut lineages = Vec::new();
    let mut seen_nodes = Vec::new();
    for node in nodes {
        if seen_nodes
            .iter()
            .any(|existing| existing == node.path.as_str())
        {
            continue;
        }
        seen_nodes.push(node.path.to_string());
        if let Some(lineage) = prism.lineage_of(&node) {
            let lineage = lineage.0.to_string();
            if !lineages.contains(&lineage) {
                lineages.push(lineage);
            }
        }
    }
    if seen_nodes.is_empty() && lineages.is_empty() {
        return None;
    }
    Some(TaskScope {
        task_id: task_id.to_string(),
        nodes: seen_nodes,
        lineages,
    })
}

fn candidate_matches_task_scope(symbol: &SymbolView, task_scope: Option<&TaskScope>) -> bool {
    candidate_task_match(symbol, task_scope).is_some()
}

pub(crate) fn candidate_task_match(
    symbol: &SymbolView,
    task_scope: Option<&TaskScope>,
) -> Option<TaskMatch> {
    let task_scope = task_scope?;
    if task_scope.nodes.iter().any(|path| path == &symbol.id.path) {
        return Some(TaskMatch::ExactNode);
    }
    symbol
        .lineage_id
        .as_ref()
        .filter(|lineage| {
            task_scope
                .lineages
                .iter()
                .any(|candidate| candidate == *lineage)
        })
        .map(|_| TaskMatch::SameLineage)
}

fn module_path(symbol: &SymbolView) -> Option<String> {
    symbol
        .id
        .path
        .rsplit_once("::")
        .map(|(module, _)| module.to_string())
}

fn matches_module(symbol: &SymbolView, module: &str) -> bool {
    symbol.id.path == module
        || symbol
            .id
            .path
            .strip_prefix(module)
            .is_some_and(|suffix| suffix.starts_with("::"))
}

fn is_test_symbol(symbol: &SymbolView) -> bool {
    symbol.id.path.contains("::tests::")
        || symbol.file_path.as_deref().is_some_and(|path| {
            path_contains_dir(path, "tests")
                || path.ends_with("_test.rs")
                || path.ends_with("_tests.rs")
                || path.ends_with("_test.py")
                || path.ends_with("_tests.py")
                || path.ends_with("conftest.py")
                || path.rsplit('/').next().is_some_and(|file_name| {
                    file_name.starts_with("test_") && file_name.ends_with(".py")
                })
        })
}

fn is_replay_or_fixture_symbol(symbol: &SymbolView) -> bool {
    let symbol_path = symbol.id.path.to_ascii_lowercase();
    let file_path = symbol
        .file_path
        .as_deref()
        .unwrap_or_default()
        .to_ascii_lowercase();
    symbol_path.contains("query_replay_cases")
        || symbol_path.contains("fixture")
        || file_path.contains("query_replay_cases")
        || path_contains_dir(&file_path, "fixtures")
        || path_contains_dir(&file_path, "testdata")
        || file_path.ends_with("_fixture.rs")
        || file_path.ends_with("_fixtures.rs")
        || file_path.ends_with("_fixture.py")
        || file_path.ends_with("_fixtures.py")
}

fn is_query_replay_case_symbol(symbol: &SymbolView) -> bool {
    let symbol_path = symbol.id.path.to_ascii_lowercase();
    let file_path = symbol
        .file_path
        .as_deref()
        .unwrap_or_default()
        .to_ascii_lowercase();
    symbol_path.contains("query_replay_cases")
        || file_path.contains("query_replay_cases.rs")
        || file_path.contains("query_replay_cases.py")
        || (file_path.contains("query_replay_cases") && symbol_path.contains("assert_"))
}

fn is_facade_file_symbol(symbol: &SymbolView) -> bool {
    let file_path = symbol
        .file_path
        .as_deref()
        .unwrap_or_default()
        .to_ascii_lowercase();
    path_matches_suffix(&file_path, "src/lib.rs") || path_matches_suffix(&file_path, "src/main.rs")
}

fn is_schema_example_surface_symbol(symbol: &SymbolView) -> bool {
    let symbol_path = symbol.id.path.to_ascii_lowercase();
    let file_path = symbol
        .file_path
        .as_deref()
        .unwrap_or_default()
        .to_ascii_lowercase();
    let name = symbol.name.to_ascii_lowercase();
    contains_any(
        &symbol_path,
        &[
            "schema_example",
            "schema_examples",
            "payload_example",
            "payload_examples",
            "session_payload_example",
        ],
    ) || contains_any(
        &file_path,
        &[
            "schema_example",
            "schema_examples",
            "payload_example",
            "payload_examples",
            "examples/",
            "_example.rs",
            "_examples.rs",
        ],
    ) || contains_any(
        &name,
        &[
            "schema_example",
            "schema_examples",
            "payload_example",
            "payload_examples",
        ],
    )
}

fn is_read_surface_wrapper_symbol(symbol: &SymbolView) -> bool {
    let symbol_path = symbol.id.path.to_ascii_lowercase();
    let file_path = symbol
        .file_path
        .as_deref()
        .unwrap_or_default()
        .to_ascii_lowercase();
    let name = symbol.name.to_ascii_lowercase();
    contains_any(
        &symbol_path,
        &[
            "dashboard",
            "resource",
            "_view",
            "_uri",
            "_link",
            "read_models",
        ],
    ) || contains_any(
        &file_path,
        &[
            "dashboard",
            "resource",
            "_view.rs",
            "_uri.rs",
            "_link.rs",
            "read_models",
        ],
    ) || contains_any(&name, &["view", "resource", "uri", "link", "dashboard"])
}

fn is_generic_helper_utility_symbol(symbol: &SymbolView) -> bool {
    let symbol_path = symbol.id.path.to_ascii_lowercase();
    let file_path = symbol
        .file_path
        .as_deref()
        .unwrap_or_default()
        .to_ascii_lowercase();
    let name = symbol.name.to_ascii_lowercase();
    let utility_module = contains_any(
        &symbol_path,
        &[
            "query_helpers",
            "discovery_helpers",
            "_helpers::",
            "_contexts::",
        ],
    ) || contains_any(
        &file_path,
        &[
            "query_helpers.rs",
            "discovery_helpers.rs",
            "_helpers.rs",
            "_contexts.rs",
        ],
    );
    let plain_helpers_module =
        symbol_path.contains("::helpers::") || file_path.ends_with("/helpers.rs");
    let utility_name = contains_any(
        &name,
        &[
            "anchor_",
            "anchors_",
            "claim_",
            "is_",
            "compact_",
            "collect_",
            "conflict_",
            "cached_",
            "context_",
            "dedupe_",
            "edit_slice_",
            "focused_",
            "next_",
            "normalize_",
            "owner_",
            "overlap",
            "sort_key",
            "candidate_",
            "source_",
            "severity",
            "summary_",
            "summarize_",
        ],
    );
    utility_module
        || (plain_helpers_module && utility_name)
        || (name.contains("helper") && utility_name)
}

fn is_internal_plain_helpers_symbol(symbol: &SymbolView) -> bool {
    let symbol_path = symbol.id.path.to_ascii_lowercase();
    let file_path = symbol
        .file_path
        .as_deref()
        .unwrap_or_default()
        .to_ascii_lowercase();
    let in_plain_helpers_module =
        symbol_path.contains("::helpers::") || file_path.ends_with("/helpers.rs");
    if !in_plain_helpers_module {
        return false;
    }
    let snippet = symbol
        .source_excerpt
        .as_ref()
        .map(|excerpt| excerpt.text.trim_start())
        .unwrap_or("");
    snippet.starts_with("pub(crate) fn") || snippet.starts_with("fn ")
}

fn is_dependency_metadata_symbol(symbol: &SymbolView) -> bool {
    let symbol_path = symbol.id.path.to_ascii_lowercase();
    let file_path = symbol
        .file_path
        .as_deref()
        .unwrap_or_default()
        .to_ascii_lowercase();
    file_path.ends_with("package-lock.json")
        || file_path.ends_with("cargo.lock")
        || file_path.ends_with("pnpm-lock.yaml")
        || file_path.ends_with("yarn.lock")
        || symbol_path.contains("node_modules/")
        || symbol_path.contains("package_lock")
        || symbol_path.contains("pnpm_lock")
}

fn identifier_tokens(value: &str) -> Vec<&str> {
    value
        .split(|ch: char| !ch.is_ascii_alphanumeric())
        .filter(|token| !token.is_empty())
        .collect()
}

fn query_mentions_schema_or_examples(query_lower: &str) -> bool {
    contains_any(query_lower, &["schema", "example", "examples", "payload"])
}

fn query_mentions_read_surface(query_lower: &str) -> bool {
    contains_any(
        query_lower,
        &[
            "view",
            "views",
            "resource",
            "resources",
            "uri",
            "link",
            "dashboard",
        ],
    )
}

fn contains_any(value: &str, needles: &[&str]) -> bool {
    needles.iter().any(|needle| value.contains(needle))
}

fn direct_symbol_match_rank(symbol: &SymbolView, query_lower: &str) -> Option<u8> {
    let path = symbol.id.path.as_str();
    let leaf = path.rsplit("::").next().unwrap_or(path);
    let leaf_lower = leaf.to_ascii_lowercase();
    let name_lower = symbol.name.to_ascii_lowercase();
    let query_stem = identifier_stem(query_lower);

    if leaf_lower == query_lower || name_lower == query_lower {
        Some(0)
    } else if identifier_tokens(&leaf_lower)
        .iter()
        .chain(identifier_tokens(&name_lower).iter())
        .any(|token| *token == query_lower)
    {
        Some(1)
    } else if identifier_tokens(&leaf_lower)
        .iter()
        .chain(identifier_tokens(&name_lower).iter())
        .any(|token| identifier_stem(token) == query_stem)
    {
        Some(2)
    } else if identifier_tokens(&leaf_lower)
        .iter()
        .chain(identifier_tokens(&name_lower).iter())
        .any(|token| token.starts_with(query_lower))
    {
        Some(3)
    } else if leaf_lower.contains(query_lower) || name_lower.contains(query_lower) {
        Some(4)
    } else {
        None
    }
}

fn path_inherits_query(path_lower: &str, query_lower: &str) -> bool {
    identifier_tokens(path_lower).iter().any(|token| {
        *token == query_lower
            || identifier_stem(token) == identifier_stem(query_lower)
            || token.starts_with(query_lower)
    })
}

fn path_contains_dir(path: &str, dir: &str) -> bool {
    path == dir || path.starts_with(&format!("{dir}/")) || path.contains(&format!("/{dir}/"))
}

fn path_matches_suffix(path: &str, suffix: &str) -> bool {
    path == suffix || path.ends_with(&format!("/{suffix}"))
}

fn identifier_stem(value: &str) -> String {
    if value.len() > 4 && value.ends_with("ies") {
        let mut stem = value[..value.len() - 3].to_string();
        stem.push('y');
        return stem;
    }
    if value.len() > 3 && value.ends_with("es") {
        return value[..value.len() - 2].to_string();
    }
    if value.len() > 3 && value.ends_with('s') {
        return value[..value.len() - 1].to_string();
    }
    value.to_string()
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum TaskMatch {
    ExactNode,
    SameLineage,
}
