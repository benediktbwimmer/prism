use super::open::compact_preview_for_ranked_target;
use super::text_fragments::{
    locate_text_candidates, locate_text_diagnostics, semantic_symbols_from_text_candidates,
};
use super::*;

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
                    host.workspace.as_ref().map(|workspace| workspace.root()),
                    applied.saturating_mul(TEXT_LOCATE_LIMIT_MULTIPLIER),
                )?);
                results.extend(exact_identifier_locate_symbols(
                    prism.as_ref(),
                    &args,
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
