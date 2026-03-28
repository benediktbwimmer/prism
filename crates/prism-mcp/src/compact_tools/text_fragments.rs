use super::open::{
    compact_open_result_from_excerpt, compact_preview_for_symbol_view,
    compact_preview_for_text_target,
};
use super::*;
use crate::compact_followups::workspace_scoped_path;

impl QueryHost {
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
}

fn semantic_search_symbols(
    prism: &Prism,
    query: &str,
    kind: NodeKind,
    workspace_root: Option<&Path>,
    path: Option<&str>,
    limit: usize,
) -> Result<Vec<SymbolView>> {
    let scoped_path = path.map(|value| workspace_scoped_path(workspace_root, value));
    prism
        .search(query, limit, Some(kind), scoped_path.as_deref())
        .into_iter()
        .map(|symbol| symbol_view(prism, &symbol))
        .collect()
}

pub(super) fn semantic_symbols_from_text_candidates(
    prism: &Prism,
    candidates: &[TextSearchCandidate],
    workspace_root: Option<&Path>,
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
            for symbol in semantic_search_symbols(
                prism,
                &query,
                kind,
                workspace_root,
                Some(file_path),
                limit,
            )? {
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

pub(super) fn semantic_symbols_for_text_target(
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
    let workspace_root = host.workspace.as_ref().map(|workspace| workspace.root());
    let mut promoted = semantic_symbols_from_text_candidates(
        prism.as_ref(),
        &[pseudo_candidate.clone()],
        workspace_root,
        limit,
    )?;
    let query_segments = target
        .query
        .as_deref()
        .map(semantic_query_segments)
        .unwrap_or_default();
    let structured_target = matches!(
        semantic_search_kind_for_path(file_path),
        Some(NodeKind::JsonKey | NodeKind::TomlKey | NodeKind::YamlKey)
    );
    promoted.retain(|symbol| {
        if !semantic_symbol_matches_text_query(symbol, &query_segments) {
            return false;
        }
        match symbol.file_path.as_deref() {
            Some(symbol_path) => same_workspace_file(workspace_root, file_path, symbol_path),
            None => {
                structured_target
                    && matches!(
                        symbol.kind,
                        NodeKind::JsonKey | NodeKind::TomlKey | NodeKind::YamlKey
                    )
            }
        }
    });
    if promoted.is_empty() && structured_target {
        let Some(search_kind) = semantic_search_kind_for_path(file_path) else {
            return Ok(promoted);
        };
        let scoped_file_path = workspace_scoped_path(workspace_root, file_path);
        let mut fallback = Vec::new();
        let mut seen = HashSet::<String>::new();
        for query in semantic_queries_for_text_candidate(&pseudo_candidate) {
            for symbol in prism.search(
                &query,
                limit.saturating_mul(4),
                Some(search_kind),
                Some(scoped_file_path.as_str()),
            ) {
                let view = symbol_view(prism.as_ref(), &symbol)?;
                if !semantic_symbol_matches_text_query(&view, &query_segments) {
                    continue;
                }
                if view.file_path.as_deref().is_some_and(|symbol_path| {
                    !same_workspace_file(workspace_root, file_path, symbol_path)
                }) {
                    continue;
                }
                if !seen.insert(view.id.path.clone()) {
                    continue;
                }
                fallback.push(view);
                if fallback.len() >= limit {
                    return Ok(fallback);
                }
            }
        }
        promoted = fallback;
    }
    Ok(promoted)
}

fn semantic_symbol_matches_text_query(symbol: &SymbolView, query_segments: &[String]) -> bool {
    if query_segments.is_empty() {
        return true;
    }
    let name = normalize_locate_text(symbol.name.as_str());
    let path = normalize_locate_text(symbol.id.path.as_str());
    query_segments.iter().all(|segment| {
        let normalized = normalize_locate_text(segment);
        name.contains(normalized.as_str()) || path.contains(normalized.as_str())
    })
}

pub(super) fn locate_text_candidates(
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

pub(super) fn locate_text_diagnostics(
    ranked: &[RankedLocateCandidate],
    limit: usize,
) -> Vec<QueryDiagnostic> {
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
    let target = session_target_from_text_match(query, &matched);
    let _ = session.intern_target_handle(target.clone());
    TextSearchCandidate {
        target,
        matched_text: matched.excerpt.text,
    }
}

fn session_target_from_text_match(
    query: &str,
    matched: &TextSearchMatchView,
) -> SessionHandleTarget {
    let kind = text_hit_kind(&matched.path);
    let display_path = format!("{}:{}", matched.path, matched.location.start_line);
    let basename = Path::new(&matched.path)
        .file_name()
        .and_then(|value| value.to_str())
        .unwrap_or(matched.path.as_str())
        .to_string();
    SessionHandleTarget {
        id: NodeId::new(TEXT_FRAGMENT_CRATE_NAME, display_path, kind),
        lineage_id: None,
        name: format!("{basename}:{}", matched.location.start_line),
        kind,
        file_path: Some(matched.path.clone()),
        query: Some(query.to_string()),
        why_short: clamp_string(
            &format!("Exact text hit for `{query}`."),
            MAX_WHY_SHORT_CHARS,
        ),
        start_line: Some(matched.location.start_line),
        end_line: Some(matched.location.end_line),
        start_column: Some(matched.location.start_column),
        end_column: Some(matched.location.end_column),
    }
}

pub(super) fn resolve_text_fragment_target(
    host: &QueryHost,
    session: &SessionState,
    handle: &str,
    mut target: SessionHandleTarget,
) -> Result<(SessionHandleTarget, bool)> {
    let file_path = target
        .file_path
        .clone()
        .ok_or_else(|| anyhow!("text-fragment handle `{handle}` is missing a file path"))?;
    let query = target
        .query
        .clone()
        .filter(|value| !value.trim().is_empty())
        .ok_or_else(|| {
            anyhow!(
                "text-fragment handle `{handle}` is missing its search query; rerun prism_gather or prism_locate to select a fresh exact-text slice"
            )
        })?;

    if text_fragment_excerpt_matches_query(host, &target)? {
        session.refresh_target_handle(handle, target.clone());
        return Ok((target, false));
    }

    let remapped_target = remap_text_fragment_target(host, session, &target, &query)?
        .ok_or_else(|| {
            anyhow!(
                "text-fragment handle `{handle}` is stale; rerun prism_gather or prism_locate to select a fresh exact-text slice in `{file_path}`"
            )
        })?;
    let remapped = remapped_target.id.path != target.id.path
        || remapped_target.file_path != target.file_path
        || remapped_target.start_line != target.start_line
        || remapped_target.end_line != target.end_line
        || remapped_target.start_column != target.start_column
        || remapped_target.end_column != target.end_column;
    target = remapped_target;
    session.refresh_target_handle(handle, target.clone());
    Ok((target, remapped))
}

fn text_fragment_excerpt_matches_query(
    host: &QueryHost,
    target: &SessionHandleTarget,
) -> Result<bool> {
    let (start_line, end_line) = text_fragment_line_span(target);
    let excerpt = read_text_fragment(host, target, start_line, end_line, RAW_OPEN_MAX_CHARS)?;
    Ok(text_fragment_excerpt_matches_target_text(
        target,
        excerpt.text.as_str(),
    ))
}

fn text_fragment_excerpt_matches_target_text(target: &SessionHandleTarget, text: &str) -> bool {
    let normalized_text = normalize_locate_text(text);
    text_fragment_query_variants(target)
        .into_iter()
        .any(|query| {
            let normalized_query = normalize_locate_text(&query);
            !normalized_query.is_empty() && normalized_text.contains(normalized_query.as_str())
        })
}

fn remap_text_fragment_target(
    host: &QueryHost,
    session: &SessionState,
    target: &SessionHandleTarget,
    query: &str,
) -> Result<Option<SessionHandleTarget>> {
    let file_path = target.file_path.as_deref().ok_or_else(|| {
        anyhow!(
            "text-fragment target `{}` is missing a file path",
            target.id.path
        )
    })?;
    let mut best: Option<(usize, SessionHandleTarget)> = None;
    let mut searched_queries = Vec::<String>::new();

    for search_query in std::iter::once(query.to_string()).chain(
        text_fragment_query_variants(target)
            .into_iter()
            .filter(|candidate| candidate != query),
    ) {
        if searched_queries
            .iter()
            .any(|existing| existing == &search_query)
        {
            continue;
        }
        searched_queries.push(search_query.clone());
        let outcome = search_text(
            host,
            SearchTextArgs {
                query: search_query,
                regex: Some(false),
                case_sensitive: Some(false),
                path: Some(file_path.to_string()),
                glob: None,
                limit: Some(TEXT_FRAGMENT_RELATED_LIMIT + EXPAND_NEIGHBOR_LIMIT),
                context_lines: Some(0),
            },
            session.limits().max_result_nodes,
        )?;
        for matched in outcome.results {
            let candidate = session_target_from_text_match(query, &matched);
            let distance = target
                .start_line
                .unwrap_or(1)
                .abs_diff(candidate.start_line.unwrap_or(1));
            let replace = best.as_ref().is_none_or(|(best_distance, best_target)| {
                distance < *best_distance
                    || (distance == *best_distance && candidate.start_line < best_target.start_line)
            });
            if replace {
                best = Some((distance, candidate));
            }
        }
        if best.is_some() {
            break;
        }
    }

    Ok(best.map(|(_, candidate)| candidate))
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

pub(super) fn compact_text_fragment_supporting_reads(
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

pub(super) fn compact_text_fragment_likely_tests(
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

pub(super) fn compact_open_text_fragment(
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
    let related_handles = compact_text_fragment_related_handles(host, session, target)?;
    compact_open_result_from_excerpt(
        handle,
        &file_path,
        excerpt,
        remapped,
        &text_fragment_staged_next_action(related_handles.as_deref()),
        related_handles,
    )
}

fn compact_gather_match_result(
    host: &QueryHost,
    session: &SessionState,
    handle: &str,
    target: &SessionHandleTarget,
) -> Result<AgentOpenResultView> {
    let file_path = target
        .file_path
        .clone()
        .ok_or_else(|| anyhow!("text-fragment handle `{handle}` is missing a file path"))?;
    let (start_line, end_line) = text_fragment_line_span(target);
    let excerpt = read_text_fragment(
        host,
        target,
        start_line.saturating_sub(1).max(1),
        end_line + 1,
        FOCUS_OPEN_OPTIONS.max_chars,
    )?;
    let related_handles =
        compact_text_fragment_supporting_reads(host, session, target, OPEN_RELATED_HANDLE_LIMIT)?;
    let next_action = text_fragment_staged_next_action(Some(&related_handles));
    compact_open_result_from_excerpt(
        handle,
        &file_path,
        excerpt,
        false,
        &next_action,
        (!related_handles.is_empty()).then_some(related_handles),
    )
}

fn text_fragment_staged_next_action(related_handles: Option<&[AgentTargetHandleView]>) -> String {
    if related_handles.is_some_and(|handles| {
        handles
            .iter()
            .any(|handle| !matches!(handle.kind, NodeKind::Document))
    }) {
        "Use prism_workset on the strongest semantic related handle, or prism_open on it for local context.".to_string()
    } else {
        "Use prism_workset here, or prism_gather for tighter slices.".to_string()
    }
}

pub(super) fn compact_text_fragment_related_handles(
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
    let workspace_root = host.workspace.as_ref().map(|workspace| workspace.root());
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
            if !same_workspace_file(workspace_root, file_path, &matched.path) {
                continue;
            }
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

pub(super) fn compact_text_fragment_neighbors(
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
    let workspace_root = host.workspace.as_ref().map(|workspace| workspace.root());
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
            if !same_workspace_file(workspace_root, file_path, &matched.path) {
                continue;
            }
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

pub(super) fn compact_text_fragment_diagnostics(target: &SessionHandleTarget) -> Value {
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

pub(super) fn compact_text_fragment_validation(
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
            compact_gather_match_result(host, session, &handle, &target)
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

pub(super) fn read_text_fragment(
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
