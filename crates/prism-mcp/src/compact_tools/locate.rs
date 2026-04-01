use super::open::compact_preview_for_ranked_target;
use super::text_fragments::{
    locate_text_candidates, locate_text_diagnostics, semantic_symbols_from_text_candidates,
};
use super::*;
use crate::{build_task_scope, candidate_task_match, TaskMatch};

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
                    host.workspace_root(),
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
                    host.workspace_root(),
                    applied.saturating_mul(TEXT_LOCATE_LIMIT_MULTIPLIER),
                )?);
                results.extend(exact_identifier_locate_symbols(
                    prism.as_ref(),
                    &args,
                    applied.saturating_mul(TEXT_LOCATE_LIMIT_MULTIPLIER),
                )?);
                dedupe_locate_symbols(&mut results);
                let ranked =
                    rerank_locate_results(prism.as_ref(), results, text_candidates, &args, applied);
                let mut diagnostics = execution.diagnostics();
                diagnostics.extend(locate_text_diagnostics(&ranked, applied));
                let resolved_confidently = locate_resolved_confidently(&ranked, &diagnostics);
                let selection_reason = ranked
                    .first()
                    .map(|candidate| candidate.selection_reason.clone());
                let top_preview = if args.include_top_preview.unwrap_or(false) {
                    if let Some(candidate) = ranked.first() {
                        let preview_handle = compact_ranked_target_view(
                            &session,
                            &candidate.target,
                            Some(args.query.as_str()),
                            Some(candidate.why_short.clone()),
                            None,
                            Some(candidate.confidence_label),
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
                            Some(candidate.why_short),
                            candidate.why_not_top,
                            Some(candidate.confidence_label),
                        )
                    })
                    .collect::<Vec<_>>();
                Ok((
                    build_locate_result(
                        candidates,
                        selection_reason,
                        diagnostics.clone(),
                        resolved_confidently,
                        top_preview,
                    ),
                    diagnostics,
                ))
            },
        )
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
    ) = locate_intent_defaults(args);
    SearchArgs {
        query: args.query.clone(),
        limit: Some(backend_limit.max(applied)),
        kind: None,
        path: args.path.clone(),
        module: None,
        task_id: args.task_id.clone(),
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

fn exact_identifier_locate_symbols(
    prism: &Prism,
    args: &PrismLocateArgs,
    limit: usize,
) -> Result<Vec<SymbolView>> {
    let mut promoted = Vec::new();
    let mut seen = HashSet::<String>::new();
    let mut queries = locate_identifier_terms(&args.query);
    let trimmed_query = args
        .query
        .trim()
        .trim_matches(|ch: char| matches!(ch, '`' | '"' | '\''));
    if trimmed_query.len() >= 2
        && is_identifier_like_term(trimmed_query)
        && !queries.iter().any(|existing| existing == trimmed_query)
    {
        queries.push(trimmed_query.to_string());
    }

    for query in queries {
        for symbol in prism.search(query.as_str(), limit.max(1), None, args.path.as_deref()) {
            let view = symbol_view(prism, &symbol)?;
            if !symbol_exactly_matches_identifier_term(&view, query.as_str()) {
                continue;
            }
            let key = format!("{}::{}::{:?}", view.id.crate_name, view.id.path, view.kind);
            if seen.insert(key) {
                promoted.push(view);
            }
            if promoted.len() >= limit {
                return Ok(promoted);
            }
        }
    }

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

fn symbol_exactly_matches_identifier_term(symbol: &SymbolView, term: &str) -> bool {
    let normalized_term = term.trim().to_ascii_lowercase();
    if normalized_term.is_empty() {
        return false;
    }
    let name = symbol.name.trim().to_ascii_lowercase();
    let path = symbol.id.path.trim().to_ascii_lowercase();
    let path_tail = path.split("::").last().unwrap_or(path.as_str());
    let file_path = symbol
        .file_path
        .as_deref()
        .unwrap_or_default()
        .trim()
        .to_ascii_lowercase();
    name == normalized_term
        || path_tail == normalized_term
        || path == normalized_term
        || path.ends_with(format!("::{normalized_term}").as_str())
        || Path::new(&file_path)
            .file_name()
            .and_then(|name| name.to_str())
            .is_some_and(|name| name.eq_ignore_ascii_case(normalized_term.as_str()))
}

fn compact_locate_limit(requested: Option<usize>) -> usize {
    requested
        .unwrap_or(DEFAULT_LOCATE_LIMIT)
        .clamp(1, MAX_LOCATE_LIMIT)
}

fn locate_intent_defaults(
    args: &PrismLocateArgs,
) -> (&'static str, Option<&'static str>, bool, bool, bool) {
    match effective_locate_intent(args) {
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
    selection_reason: Option<String>,
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
        selection_reason,
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

fn rerank_locate_results(
    prism: &prism_query::Prism,
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
    let task_scope = args
        .task_id
        .as_deref()
        .filter(|task_id| !task_id.trim().is_empty())
        .and_then(|task_id| build_task_scope(prism, task_id));
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
                task_scope.as_ref(),
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
    let mut selected = select_locate_candidates(ranked, limit);
    annotate_close_alternative_explanations(&mut selected);
    selected
}

fn rank_locate_candidate(
    index: usize,
    symbol: SymbolView,
    query_normalized: &str,
    tokens: &[String],
    identifier_terms: &[String],
    path_scope: Option<&str>,
    profile: LocateIntentProfile,
    task_scope: Option<&crate::TaskScope>,
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
    let ownership_query_terms = locate_ownership_query_terms(tokens);
    let mut score = 0_i32;
    let mut reasons = Vec::<String>::new();
    let mut signals = LocateReasonSignals::default();

    if let Some(owner_boost) = ownership_locate_boost(
        &symbol,
        &ownership_query_terms,
        profile,
        &name_normalized,
        &path_normalized,
        &file_normalized,
    ) {
        score += owner_boost;
        signals.ownership_boundary = true;
        reasons.push(format!(
            "Ownership-style query favored this owner-like boundary target (+{owner_boost})."
        ));
    }

    for term in identifier_terms {
        if name_raw == *term {
            score += 420;
            signals.exact_identifier = true;
            reasons.push(format!(
                "Exact identifier `{term}` matched the candidate name."
            ));
        } else if final_segment_raw == term {
            score += 360;
            signals.exact_identifier = true;
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
            signals.exact_query_match = true;
            reasons.push("Exact query matched the candidate name.".to_string());
        } else if semantic_label_normalized == *query_normalized {
            score += 240;
            signals.exact_query_match = true;
            reasons.push("Normalized semantic label matched the query.".to_string());
        } else if final_segment_normalized(&symbol.id.path) == query_normalized {
            score += 210;
            signals.exact_query_match = true;
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
    signals.matched_tokens = matched_tokens;
    signals.total_tokens = tokens.len();
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
            signals.code_bias = true;
            reasons.push("Locate intent favored callable or editable code.".to_string());
        }
    }
    if is_docs_like_kind(symbol.kind) {
        score += profile.docs_bias;
        if profile.docs_bias > 0 {
            signals.docs_bias = true;
            reasons.push("Locate intent favored docs or structured spec surfaces.".to_string());
        }
    }
    if symbol.file_path.as_deref().is_some_and(is_docs_path) {
        score += profile.docs_bias / 2;
    }
    if path_scope.is_some_and(|scope| locate_path_scope_matches(scope, symbol.file_path.as_deref()))
    {
        score += 150;
        signals.path_scope = true;
        reasons.push("Matched the requested path scope.".to_string());
    }
    if let Some(task_match) = candidate_task_match(&symbol, task_scope) {
        score += match task_match {
            TaskMatch::ExactNode => 120,
            TaskMatch::SameLineage => 90,
        };
        signals.task_scope_strength = match task_match {
            TaskMatch::ExactNode => 2,
            TaskMatch::SameLineage => 1,
        };
        reasons.push(match task_match {
            TaskMatch::ExactNode => "Matched the exact requested task scope.".to_string(),
            TaskMatch::SameLineage => "Matched the requested task's current lineage.".to_string(),
        });
    }
    if profile.test_penalty > 0 && is_test_like_symbol(&symbol) {
        score -= profile.test_penalty;
    }
    if matches!(symbol.kind, NodeKind::Module) {
        score -= 18;
    }

    score -= index as i32;
    let why_short = clamp_string(
        &reasons
            .first()
            .cloned()
            .unwrap_or_else(|| "Locate ranked this as a strong first-hop target.".to_string()),
        MAX_WHY_SHORT_CHARS,
    );
    RankedLocateCandidate {
        target: RankedLocateTarget::Symbol(symbol),
        score,
        selection_reason: locate_selection_reason(&reasons),
        why_short,
        why_not_top: None,
        signals,
        confidence_label: locate_confidence_label(&signals),
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
    let ownership_query_terms = locate_ownership_query_terms(tokens);
    let mut score = if query_uses_identifiers { 245 } else { 208 };
    let mut reasons = vec![format!(
        "Exact text hit in {} near line {}.",
        candidate.target.file_path.as_deref().unwrap_or_default(),
        candidate.target.start_line.unwrap_or_default()
    )];
    let mut signals = LocateReasonSignals {
        exact_text_hit: true,
        ..LocateReasonSignals::default()
    };

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
    signals.matched_tokens = matched_tokens;
    signals.total_tokens = tokens.len();
    if !tokens.is_empty() && matched_tokens == tokens.len() {
        score += 38;
    } else if matched_tokens > 0 {
        score += (matched_tokens as i32) * 10;
    }
    if path_scope.is_some_and(|scope| {
        locate_path_scope_matches(scope, candidate.target.file_path.as_deref())
    }) {
        score += 150;
        signals.path_scope = true;
        reasons.insert(0, "Matched the requested path scope.".to_string());
    }
    if is_docs_like_kind(candidate.target.kind) {
        score += profile.docs_bias.max(0);
        signals.docs_bias = profile.docs_bias > 0;
    } else if matches!(
        candidate.target.kind,
        NodeKind::JsonKey | NodeKind::TomlKey | NodeKind::YamlKey
    ) {
        score += profile.docs_bias.max(0) / 2;
        signals.docs_bias = profile.docs_bias > 0;
    } else {
        score += profile.code_bias / 2;
        signals.code_bias = profile.code_bias > 0;
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
    if !ownership_query_terms.is_empty() {
        let owner_hits = ownership_term_hit_count(
            &matched_text_normalized,
            &file_normalized,
            &ownership_query_terms,
        );
        if owner_hits == 0 {
            score -= 120;
        } else {
            score -= 24 * (ownership_query_terms.len().saturating_sub(owner_hits) as i32);
        }
    }

    score -= index as i32;
    let why_short = clamp_string(
        &reasons
            .first()
            .cloned()
            .unwrap_or_else(|| "Exact text hit looked like the best first-hop read.".to_string()),
        MAX_WHY_SHORT_CHARS,
    );
    RankedLocateCandidate {
        target: RankedLocateTarget::Text(candidate.target),
        score,
        selection_reason: locate_selection_reason(&reasons),
        why_short,
        why_not_top: None,
        signals,
        confidence_label: locate_confidence_label(&signals),
    }
}

fn annotate_close_alternative_explanations(ranked: &mut [RankedLocateCandidate]) {
    let Some(top) = ranked.first().cloned() else {
        return;
    };
    for candidate in ranked.iter_mut().skip(1).take(LOCATE_WHY_NOT_TOP_LIMIT) {
        let gap = top.score.saturating_sub(candidate.score);
        if gap > LOCATE_CLOSE_ALTERNATIVE_MAX_GAP {
            continue;
        }
        candidate.why_not_top = Some(locate_why_not_top(&top, candidate, gap));
    }
}

fn locate_why_not_top(
    top: &RankedLocateCandidate,
    candidate: &RankedLocateCandidate,
    gap: i32,
) -> String {
    let explanation = locate_top_advantage(top, candidate)
        .unwrap_or_else(|| "the winner kept a stronger overall mix of ranking signals".to_string());
    clamp_string(
        &format!("Lost to top candidate because {explanation}. It scored {gap} points lower."),
        LOCATE_WHY_NOT_TOP_MAX_CHARS,
    )
}

fn locate_top_advantage(
    top: &RankedLocateCandidate,
    candidate: &RankedLocateCandidate,
) -> Option<String> {
    if top.signals.task_scope_strength > candidate.signals.task_scope_strength {
        return Some(if top.signals.task_scope_strength == 2 {
            "the winner matched the exact requested task scope and this candidate did not"
                .to_string()
        } else {
            "the winner matched the requested task's current lineage and this candidate did not"
                .to_string()
        });
    }
    if top.signals.path_scope && !candidate.signals.path_scope {
        return Some(
            "the winner matched the requested path scope and this candidate did not".to_string(),
        );
    }
    if top.signals.ownership_boundary && !candidate.signals.ownership_boundary {
        return Some(
            "the winner had the stronger ownership-style boundary signal for this query"
                .to_string(),
        );
    }
    if top.signals.exact_identifier && !candidate.signals.exact_identifier {
        return Some(
            "the winner had an exact identifier match and this candidate did not".to_string(),
        );
    }
    if top.signals.exact_query_match && !candidate.signals.exact_query_match {
        return Some("the winner matched the exact query phrase more directly".to_string());
    }
    if top.signals.exact_text_hit && !candidate.signals.exact_text_hit {
        return Some("the winner had the stronger exact text hit for this query".to_string());
    }
    if top.signals.matched_tokens > candidate.signals.matched_tokens && top.signals.total_tokens > 0
    {
        return Some(format!(
            "the winner matched more significant query terms ({}/{} vs {}/{})",
            top.signals.matched_tokens,
            top.signals.total_tokens,
            candidate.signals.matched_tokens,
            candidate.signals.total_tokens
        ));
    }
    if top.signals.code_bias && !candidate.signals.code_bias {
        return Some(
            "the current locate intent favored callable or editable code for the winner"
                .to_string(),
        );
    }
    if top.signals.docs_bias && !candidate.signals.docs_bias {
        return Some(
            "the current locate intent favored docs or structured spec targets for the winner"
                .to_string(),
        );
    }
    None
}

fn locate_confidence_label(signals: &LocateReasonSignals) -> ConfidenceLabel {
    if signals.task_scope_strength > 0
        || signals.path_scope
        || signals.exact_identifier
        || signals.exact_query_match
        || signals.exact_text_hit
    {
        ConfidenceLabel::High
    } else if signals.ownership_boundary
        || (signals.total_tokens > 0 && signals.matched_tokens == signals.total_tokens)
        || signals.matched_tokens >= 2
    {
        ConfidenceLabel::Medium
    } else {
        ConfidenceLabel::Low
    }
}

fn locate_selection_reason(reasons: &[String]) -> String {
    let normalized = reasons
        .iter()
        .map(|reason| reason.trim())
        .filter(|reason| !reason.is_empty())
        .collect::<Vec<_>>();
    let mut unique = Vec::<&str>::new();

    if let Some(reason) = normalized
        .iter()
        .copied()
        .find(|reason| matches!(locate_selection_reason_bucket(reason), 0))
    {
        unique.push(reason);
    }
    if let Some(reason) = normalized
        .iter()
        .copied()
        .find(|reason| matches!(locate_selection_reason_bucket(reason), 1))
        .filter(|reason| !unique.iter().any(|existing| *existing == *reason))
    {
        unique.push(reason);
    }
    if let Some(reason) = normalized
        .iter()
        .copied()
        .find(|reason| matches!(locate_selection_reason_bucket(reason), 2))
        .filter(|reason| !unique.iter().any(|existing| *existing == *reason))
    {
        unique.push(reason);
    }
    for reason in normalized {
        if unique.iter().any(|existing| *existing == reason) {
            continue;
        }
        unique.push(reason);
        if unique.len() == 3 {
            break;
        }
    }
    clamp_string(
        &if unique.is_empty() {
            "Top candidate won the compact locate ranking.".to_string()
        } else {
            format!("Top candidate won because {}.", unique.join(" "))
        },
        LOCATE_SELECTION_REASON_MAX_CHARS,
    )
}

fn locate_selection_reason_bucket(reason: &str) -> u8 {
    if reason.starts_with("Exact identifier")
        || reason.starts_with("Exact query matched")
        || reason.starts_with("Exact text hit")
        || reason.starts_with("Ownership-style query favored")
        || reason.starts_with("Matched the exact requested task scope")
        || reason.starts_with("Matched the requested task's current lineage")
        || reason.starts_with("Matched the requested path scope")
    {
        0
    } else if reason.starts_with("Matched all ") || reason.starts_with("Matched ") {
        1
    } else if reason.starts_with("Locate intent favored") {
        2
    } else {
        3
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
    if top.why_short.starts_with("Ownership-style query favored") {
        return score_gap >= 24;
    }
    score_gap >= 60
        && (top.why_short.starts_with("Exact identifier")
            || top.why_short.starts_with("Exact query matched")
            || top
                .why_short
                .starts_with("Matched the requested path scope")
            || top.why_short.starts_with("Ownership-style query favored"))
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

#[cfg(test)]
mod tests {
    use super::*;

    fn ranked_text_candidate(
        path: &str,
        score: i32,
        signals: LocateReasonSignals,
    ) -> RankedLocateCandidate {
        RankedLocateCandidate {
            target: RankedLocateTarget::Text(SessionHandleTarget {
                id: NodeId::new(TEXT_FRAGMENT_CRATE_NAME, path, NodeKind::Function),
                lineage_id: None,
                handle_category: SessionHandleCategory::TextFragment,
                name: path.to_string(),
                kind: NodeKind::Function,
                file_path: Some("/repo/src/lib.rs".to_string()),
                query: None,
                why_short: "ranked candidate".to_string(),
                start_line: Some(1),
                end_line: Some(1),
                start_column: None,
                end_column: None,
            }),
            score,
            why_short: "ranked candidate".to_string(),
            why_not_top: None,
            selection_reason: "selection".to_string(),
            signals,
            confidence_label: locate_confidence_label(&signals),
        }
    }

    #[test]
    fn close_alternative_explanation_calls_out_task_scope_advantage() {
        let mut ranked = vec![
            ranked_text_candidate(
                "demo::task_alpha_handler",
                420,
                LocateReasonSignals {
                    task_scope_strength: 2,
                    matched_tokens: 2,
                    total_tokens: 2,
                    code_bias: true,
                    ..LocateReasonSignals::default()
                },
            ),
            ranked_text_candidate(
                "demo::alpha_handler",
                282,
                LocateReasonSignals {
                    matched_tokens: 2,
                    total_tokens: 2,
                    code_bias: true,
                    ..LocateReasonSignals::default()
                },
            ),
        ];

        annotate_close_alternative_explanations(&mut ranked);

        assert!(ranked[0].why_not_top.is_none());
        assert!(ranked[1]
            .why_not_top
            .as_deref()
            .is_some_and(|reason| reason.contains("requested task scope")));
    }
}

fn locate_intent_profile(args: &PrismLocateArgs) -> LocateIntentProfile {
    let docs_path_bias = locate_docs_path_bias(args);
    match effective_locate_intent(args) {
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

fn locate_ownership_query_terms(tokens: &[String]) -> Vec<&'static str> {
    let mut terms = Vec::new();
    let has_route_context = tokens
        .iter()
        .any(|token| matches!(token.as_str(), "route" | "routes" | "routing" | "router"));
    let has_shell_context = tokens
        .iter()
        .any(|token| matches!(token.as_str(), "shell" | "app" | "page"));
    let has_layout_context = tokens.iter().any(|token| token == "layout");
    if tokens
        .iter()
        .any(|token| matches!(token.as_str(), "route" | "routes" | "routing" | "router"))
    {
        terms.push("route");
    }
    if has_shell_context || (has_layout_context && has_route_context) {
        terms.push("shell");
    }
    if tokens.iter().any(|token| {
        matches!(
            token.as_str(),
            "entry" | "entrypoint" | "entrypoints" | "asset" | "assets" | "boundary"
        )
    }) {
        terms.push("entry");
    }
    terms
}

fn ownership_locate_boost(
    symbol: &SymbolView,
    ownership_query_terms: &[&'static str],
    profile: LocateIntentProfile,
    name_normalized: &str,
    path_normalized: &str,
    file_normalized: &str,
) -> Option<i32> {
    if ownership_query_terms.is_empty() {
        return None;
    }
    if !matches!(
        effective_owner_friendly_kind(symbol.kind),
        OwnerFriendlyKind::Callable | OwnerFriendlyKind::Module
    ) {
        return None;
    }
    let owner_hits =
        ownership_term_hit_count(path_normalized, file_normalized, ownership_query_terms).max(
            ownership_term_hit_count(name_normalized, path_normalized, ownership_query_terms),
        );
    let owner_hint_boost = symbol
        .owner_hint
        .as_ref()
        .map(|hint| hint.score.min(28) as i32)
        .unwrap_or(0);
    if owner_hits == 0 && owner_hint_boost == 0 {
        return None;
    }

    let kind_boost = match effective_owner_friendly_kind(symbol.kind) {
        OwnerFriendlyKind::Module => 22,
        OwnerFriendlyKind::Callable => 12,
        OwnerFriendlyKind::Other => 0,
    };
    Some(120 + (owner_hits as i32 * 44) + owner_hint_boost + kind_boost + profile.code_bias / 6)
}

#[derive(Clone, Copy)]
enum OwnerFriendlyKind {
    Module,
    Callable,
    Other,
}

fn effective_owner_friendly_kind(kind: NodeKind) -> OwnerFriendlyKind {
    match kind {
        NodeKind::Module => OwnerFriendlyKind::Module,
        NodeKind::Function | NodeKind::Method => OwnerFriendlyKind::Callable,
        _ => OwnerFriendlyKind::Other,
    }
}

fn ownership_term_hit_count(
    primary: &str,
    secondary: &str,
    ownership_query_terms: &[&'static str],
) -> usize {
    ownership_query_terms
        .iter()
        .filter(|term| {
            ownership_term_matches(primary, term) || ownership_term_matches(secondary, term)
        })
        .count()
}

fn ownership_term_matches(candidate: &str, term: &str) -> bool {
    match term {
        "route" => {
            candidate.contains("route")
                || candidate.contains("router")
                || candidate.contains("routing")
        }
        "shell" => {
            candidate.contains("shell")
                || candidate.contains("layout")
                || candidate.contains("page")
                || candidate.contains("app")
        }
        "entry" => {
            candidate.contains("entry")
                || candidate.contains("asset")
                || candidate.contains("boundary")
        }
        _ => false,
    }
}

fn effective_locate_intent(args: &PrismLocateArgs) -> PrismLocateTaskIntentInput {
    if let Some(intent) = args.task_intent.clone() {
        return intent;
    }
    if locate_docs_path_bias(args) {
        return PrismLocateTaskIntentInput::Explain;
    }
    PrismLocateTaskIntentInput::Edit
}

fn locate_docs_path_bias(args: &PrismLocateArgs) -> bool {
    args.path.as_deref().is_some_and(is_docs_path)
        || args
            .glob
            .as_deref()
            .is_some_and(|glob| glob.contains("docs/") || glob.ends_with(".md"))
}
