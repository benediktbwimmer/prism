use anyhow::Result;
use prism_ir::{AnchorRef, CoordinationTaskId};
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
    pub(crate) task_scope_mode: TaskScopeMode,
}

#[derive(Debug, Clone)]
struct RankedCandidate {
    symbol: SymbolView,
    module: Option<String>,
    score: i32,
    reasons: Vec<String>,
    exact_name_match: bool,
}

#[derive(Debug, Clone)]
struct TaskScope {
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

    *results = ranked.iter().map(|candidate| candidate.symbol.clone()).collect();

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
        .map(|candidate| ambiguity_candidate_view(context.query, candidate))
        .collect::<Vec<_>>();
    let suggested_queries = ambiguity_queries(context, &candidates);
    Ok(Some(SearchAmbiguityView {
        query: context.query.to_string(),
        strategy: context.strategy.to_string(),
        owner_kind: context.owner_kind.map(str::to_string),
        path: context.path.map(str::to_string),
        module: context.module.map(str::to_string),
        task_id: task_scope.as_ref().map(|scope| scope.task_id.clone()),
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
    let exact_matches = ranked.iter().filter(|candidate| candidate.exact_name_match).count();
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
    let path = symbol.id.path.as_str();
    let leaf = path.rsplit("::").next().unwrap_or(path);
    let exact_name_match = symbol.name == normalized_query || leaf == normalized_query;

    if path == normalized_query {
        score += 140;
        reasons.push("Exact symbol path match.".to_string());
    } else if exact_name_match {
        score += 100;
        reasons.push("Exact symbol-name match.".to_string());
    } else if symbol.name.eq_ignore_ascii_case(normalized_query) || leaf.eq_ignore_ascii_case(normalized_query) {
        score += 90;
        reasons.push("Case-insensitive symbol-name match.".to_string());
    } else if path.ends_with(&format!("::{normalized_query}")) {
        score += 70;
        reasons.push("Leaf path segment matches the query.".to_string());
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

    if context.strategy == "behavioral" {
        if let Some(owner_hint) = symbol.owner_hint.as_ref() {
            score += owner_hint.score.min(24) as i32;
            reasons.push(format!(
                "Strong {} owner hint (score {}).",
                owner_hint.kind, owner_hint.score
            ));
        }
    }

    if !query_lower.contains("test") && is_test_symbol(&symbol) {
        score -= 20;
        reasons.push("Test-only symbol de-prioritized for a non-test query.".to_string());
    }

    let depth = path.matches("::").count() as i32;
    score += (8 - depth).max(0);
    if symbol.location.is_some() {
        score += 2;
    }
    if prism.lineage_of(&prism_ir::NodeId::new(
        symbol.id.crate_name.clone(),
        symbol.id.path.clone(),
        symbol.kind,
    )).is_some() {
        score += 1;
    }

    RankedCandidate {
        module: module_path(&symbol),
        symbol,
        score,
        reasons,
        exact_name_match,
    }
}

fn ambiguity_candidate_view(
    query: &str,
    candidate: &RankedCandidate,
) -> AmbiguityCandidateView {
    AmbiguityCandidateView {
        symbol: candidate.symbol.clone(),
        module: candidate.module.clone(),
        score: candidate.score,
        reasons: candidate.reasons.clone(),
        suggested_queries: candidate_queries(query, candidate),
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
                    context.owner_kind,
                    context.task_id,
                ),
                why: "Narrow directly to the chosen file path before retrying the search.".to_string(),
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
    let exact_matches = ranked.iter().filter(|candidate| candidate.exact_name_match).count();
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

fn candidate_queries(query: &str, candidate: &RankedCandidate) -> Vec<SuggestedQueryView> {
    let mut suggestions = read_context_queries(&prism_ir::NodeId::new(
        candidate.symbol.id.crate_name.clone(),
        candidate.symbol.id.path.clone(),
        candidate.symbol.kind,
    ));
    suggestions.truncate(2);
    if let Some(module) = candidate.module.as_deref() {
        suggestions.push(SuggestedQueryView {
            label: "Narrow To Module".to_string(),
            query: search_query_call(
                query,
                None,
                Some(module),
                Some(&candidate.symbol.kind.to_string()),
                Some("direct"),
                None,
                None,
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
    format!("return prism.search({query_json}, {{ {} }});", parts.join(", "))
}

fn build_task_scope(prism: &Prism, task_id: &str) -> Option<TaskScope> {
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
        if seen_nodes.iter().any(|existing| existing == node.path.as_str()) {
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

fn candidate_task_match(symbol: &SymbolView, task_scope: Option<&TaskScope>) -> Option<TaskMatch> {
    let task_scope = task_scope?;
    if task_scope.nodes.iter().any(|path| path == &symbol.id.path) {
        return Some(TaskMatch::ExactNode);
    }
    symbol
        .lineage_id
        .as_ref()
        .filter(|lineage| task_scope.lineages.iter().any(|candidate| candidate == *lineage))
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
        || symbol
            .file_path
            .as_deref()
            .is_some_and(|path| path.contains("/tests/") || path.ends_with("_test.rs") || path.ends_with("_tests.rs"))
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum TaskMatch {
    ExactNode,
    SameLineage,
}
