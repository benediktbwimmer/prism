use super::text_fragments::{
    compact_text_fragment_likely_tests, compact_text_fragment_supporting_reads, read_text_fragment,
};
use super::*;
use crate::compact_followups::workspace_scoped_path;

impl QueryHost {
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
                        &target,
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
}

pub(super) fn budgeted_workset_result(
    target: &SessionHandleTarget,
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
        next_action: Some(compact_workset_next_action(target)),
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
    }
    Ok(result)
}

fn compact_workset_next_action(target: &SessionHandleTarget) -> String {
    if is_text_fragment_target(target) {
        "Use prism_open on a supporting slice, or prism_expand `neighbors`.".to_string()
    } else if is_structured_config_target(target.kind) {
        "Use prism_open on a same-file key, or prism_expand `validation`.".to_string()
    } else if is_spec_like_kind(target.kind)
        || target.file_path.as_deref().is_some_and(is_docs_path)
    {
        "Use prism_open on an owner, or prism_expand `drift`.".to_string()
    } else {
        "Use prism_open on a supporting read, or prism_expand `validation`.".to_string()
    }
}

pub(super) fn compact_string_list(items: &[String], limit: usize, max_chars: usize) -> Vec<String> {
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
    if is_spec_like_kind(target.kind) || target.file_path.as_deref().is_some_and(is_docs_path) {
        return compact_spec_workset_context(host, session, prism, target);
    }
    if is_structured_config_target(target.kind) {
        let supporting_reads =
            structured_symbol_followups(host, session, prism, target, WORKSET_SUPPORTING_LIMIT)?;
        if !supporting_reads.is_empty() {
            return Ok(WorksetContext {
                supporting_reads,
                likely_tests: Vec::new(),
                why: format!(
                    "{} Same-file structured follow-ups prioritized for config maintenance.",
                    workset_why(target)
                ),
            });
        }
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

pub(super) fn is_structured_config_target(kind: NodeKind) -> bool {
    matches!(
        kind,
        NodeKind::JsonKey | NodeKind::TomlKey | NodeKind::YamlKey
    )
}

pub(super) fn structured_symbol_followups(
    host: &QueryHost,
    session: &SessionState,
    prism: &Prism,
    target: &SessionHandleTarget,
    limit: usize,
) -> Result<Vec<AgentTargetHandleView>> {
    if !is_structured_config_target(target.kind) {
        return Ok(Vec::new());
    }
    let symbol_id = target_symbol_id(target)?;
    let symbol = symbol_for(prism, symbol_id)?;
    let current = symbol_view(prism, &symbol)?;
    let workspace_root = host.workspace.as_ref().map(|workspace| workspace.root());
    let current_file_path = current.file_path.as_deref().or(target.file_path.as_deref());
    let parent_id = structured_parent_symbol_id(symbol_id);
    let current_path = symbol_id.path.as_str();
    let mut followups = Vec::<AgentTargetHandleView>::new();
    let mut seen = HashSet::<String>::new();

    if let Some(parent_id) = parent_id.as_ref() {
        if let Ok(parent_symbol) = symbol_for(prism, parent_id) {
            let parent_view = symbol_view(prism, &parent_symbol)?;
            push_structured_followup(
                &mut followups,
                &mut seen,
                session,
                target,
                current_file_path,
                workspace_root,
                &parent_view,
                "Parent structured key in the same file.".to_string(),
                limit,
            );
        }
    }

    let scoped_file_path =
        current_file_path.map(|path| workspace_scoped_path(workspace_root, path));
    let mut queries = Vec::<String>::new();
    if let Some(parent_id) = parent_id.as_ref() {
        if let Some(parent_name) = parent_id.path.rsplit("::").next() {
            queries.push(parent_name.to_string());
        }
    }
    queries.push(current.name.clone());
    if let Some(query) = target.query.as_deref() {
        queries.push(query.to_string());
    }
    queries.sort();
    queries.dedup();
    for query in queries {
        for candidate in prism.search(
            &query,
            limit.saturating_mul(8).max(8),
            Some(target.kind),
            scoped_file_path.as_deref(),
        ) {
            let view = symbol_view(prism, &candidate)?;
            let Some(relationship) = structured_family_relationship(
                view.id.path.as_str(),
                current_path,
                parent_id.as_ref().map(|id| id.path.as_str()),
            ) else {
                continue;
            };
            let why = match relationship {
                StructuredRelationship::Sibling => {
                    "Sibling structured key in the same file.".to_string()
                }
                StructuredRelationship::Child => {
                    "Nested structured key in the same file.".to_string()
                }
            };
            push_structured_followup(
                &mut followups,
                &mut seen,
                session,
                target,
                current_file_path,
                workspace_root,
                &view,
                why,
                limit,
            );
            if followups.len() >= limit {
                return Ok(followups);
            }
        }
    }

    Ok(followups)
}

fn structured_parent_symbol_id(id: &NodeId) -> Option<NodeId> {
    let (_, after_document) = id.path.split_once("::document::")?;
    let (_, structured) = after_document.split_once("::")?;
    structured.contains("::").then(|| {
        let (parent_path, _) = id.path.rsplit_once("::").expect("parent path");
        NodeId::new(id.crate_name.clone(), parent_path.to_string(), id.kind)
    })
}

#[derive(Debug, Clone, Copy)]
enum StructuredRelationship {
    Sibling,
    Child,
}

fn structured_family_relationship(
    candidate_path: &str,
    current_path: &str,
    parent_path: Option<&str>,
) -> Option<StructuredRelationship> {
    if candidate_path == current_path {
        return None;
    }
    if let Some(parent_path) = parent_path {
        if structured_direct_child_of(candidate_path, parent_path) {
            return Some(StructuredRelationship::Sibling);
        }
    }
    structured_direct_child_of(candidate_path, current_path)
        .then_some(StructuredRelationship::Child)
}

fn structured_direct_child_of(candidate_path: &str, parent_path: &str) -> bool {
    let Some(tail) = candidate_path.strip_prefix(&format!("{parent_path}::")) else {
        return false;
    };
    !tail.is_empty() && !tail.contains("::")
}

fn push_structured_followup(
    followups: &mut Vec<AgentTargetHandleView>,
    seen: &mut HashSet<String>,
    session: &SessionState,
    target: &SessionHandleTarget,
    current_file_path: Option<&str>,
    workspace_root: Option<&Path>,
    candidate: &SymbolView,
    why: String,
    limit: usize,
) {
    let Some(candidate_file_path) = candidate.file_path.as_deref() else {
        return;
    };
    if current_file_path
        .is_some_and(|expected| !same_workspace_file(workspace_root, expected, candidate_file_path))
    {
        return;
    }
    if !seen.insert(candidate.id.path.clone()) {
        return;
    }
    followups.push(compact_target_view(
        session,
        candidate,
        target.query.as_deref(),
        Some(why),
    ));
    if followups.len() > limit {
        followups.truncate(limit);
    }
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
    host: &QueryHost,
    session: &SessionState,
    prism: &Prism,
    target: &SessionHandleTarget,
) -> Result<WorksetContext> {
    let drift = spec_drift_explanation_view(prism, &target.id)?;
    let supporting_reads = prioritized_spec_supporting_reads(
        host,
        session,
        prism,
        target,
        &drift,
        WORKSET_SUPPORTING_LIMIT,
    )?;
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
    keep_without_overlap: bool,
}

#[derive(Debug, Clone)]
struct SpecIdentifierFollowup {
    symbol: SymbolView,
    term: String,
    exact_match: bool,
}

pub(super) fn prioritized_spec_supporting_reads(
    host: &QueryHost,
    session: &SessionState,
    prism: &Prism,
    target: &SessionHandleTarget,
    drift: &prism_js::SpecDriftExplanationView,
    limit: usize,
) -> Result<Vec<AgentTargetHandleView>> {
    Ok(ranked_spec_followups(host, prism, target, drift)?
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
        .collect())
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

fn spec_identifier_followups(
    host: &QueryHost,
    prism: &Prism,
    target: &SessionHandleTarget,
) -> Result<Vec<SpecIdentifierFollowup>> {
    let symbol = symbol_for(prism, &target.id)?;
    let mut followups = Vec::<SpecIdentifierFollowup>::new();
    let mut seen = HashSet::<String>::new();
    for term in spec_body_identifier_terms(
        &spec_identifier_source_text(host, target, &symbol.full())?,
        SPEC_BODY_IDENTIFIER_LIMIT,
    ) {
        let normalized_term = normalize_locate_text(&term);
        let mut matched_any = false;
        for view in spec_identifier_symbol_matches(prism, &term, Some("src/"))?
            .into_iter()
            .chain(spec_identifier_symbol_matches(prism, &term, None)?)
        {
            if !seen.insert(view.id.path.clone()) {
                continue;
            }
            matched_any = true;
            let exact_match = spec_identifier_exact_match(&view, &normalized_term);
            followups.push(SpecIdentifierFollowup {
                symbol: view,
                term: term.clone(),
                exact_match,
            });
        }
        if matched_any {
            continue;
        }
        let outcome = search_text(
            host,
            SearchTextArgs {
                query: term.clone(),
                regex: Some(false),
                case_sensitive: Some(false),
                path: Some("src/".to_string()),
                glob: Some("**/*.rs".to_string()),
                limit: Some(SPEC_IDENTIFIER_TEXT_LIMIT),
                context_lines: Some(0),
            },
            SPEC_IDENTIFIER_SEARCH_LIMIT,
        )?;
        for matched in outcome.results {
            for view in spec_identifier_symbol_matches(prism, &term, Some(matched.path.as_str()))? {
                if !seen.insert(view.id.path.clone()) {
                    continue;
                }
                let exact_match = spec_identifier_exact_match(&view, &normalized_term);
                followups.push(SpecIdentifierFollowup {
                    symbol: view,
                    term: term.clone(),
                    exact_match,
                });
            }
        }
    }
    followups.sort_by(|left, right| {
        right
            .exact_match
            .cmp(&left.exact_match)
            .then_with(|| left.symbol.id.path.cmp(&right.symbol.id.path))
    });
    Ok(followups)
}

fn spec_identifier_symbol_matches(
    prism: &Prism,
    term: &str,
    path: Option<&str>,
) -> Result<Vec<SymbolView>> {
    let mut matches = Vec::<SymbolView>::new();
    let mut seen = HashSet::<String>::new();
    for kind in [
        NodeKind::Function,
        NodeKind::Method,
        NodeKind::Struct,
        NodeKind::Enum,
        NodeKind::Trait,
        NodeKind::Field,
        NodeKind::TypeAlias,
    ] {
        for symbol in prism.search(term, SPEC_IDENTIFIER_SEARCH_LIMIT, Some(kind), path) {
            let view = symbol_view(prism, &symbol)?;
            if !is_code_like_kind(view.kind) || is_test_like_symbol(&view) {
                continue;
            }
            if !seen.insert(view.id.path.clone()) {
                continue;
            }
            matches.push(view);
        }
    }
    Ok(matches)
}

fn spec_identifier_source_text(
    host: &QueryHost,
    target: &SessionHandleTarget,
    full: &str,
) -> Result<String> {
    if spec_body_identifier_terms(&full, 1).is_empty() {
        if let Some(file_path) = target.file_path.as_deref() {
            let excerpt = file_read(
                host,
                FileReadArgs {
                    path: file_path.to_string(),
                    start_line: None,
                    end_line: None,
                    max_chars: None,
                },
            )?;
            let start_line = target.start_line.or_else(|| {
                target
                    .query
                    .as_deref()
                    .and_then(|query| excerpt_start_line_for_query(excerpt.text.as_str(), query))
            });
            if let Some(start_line) = start_line {
                let end_line = target.end_line.unwrap_or(start_line).saturating_add(12);
                let excerpt =
                    read_text_fragment(host, target, start_line, end_line, RAW_OPEN_MAX_CHARS)?;
                if !excerpt.text.trim().is_empty() {
                    return Ok(excerpt.text);
                }
            }
            if let Some(query) = target.query.as_deref() {
                if let Some(section) = excerpt_section_for_query(excerpt.text.as_str(), query) {
                    return Ok(section);
                }
            }
            if !excerpt.text.trim().is_empty() {
                return Ok(excerpt.text);
            }
        }
    }
    Ok(full.to_string())
}

fn excerpt_start_line_for_query(text: &str, query: &str) -> Option<usize> {
    let query = normalize_locate_text(query);
    text.lines().enumerate().find_map(|(index, line)| {
        normalize_locate_text(line)
            .contains(query.as_str())
            .then_some(index + 1)
    })
}

fn excerpt_section_for_query(text: &str, query: &str) -> Option<String> {
    let start_line = excerpt_start_line_for_query(text, query)?;
    let section = text
        .lines()
        .skip(start_line.saturating_sub(1))
        .take(16)
        .collect::<Vec<_>>()
        .join("\n");
    (!section.trim().is_empty()).then_some(section)
}

fn ranked_spec_followups(
    host: &QueryHost,
    prism: &Prism,
    target: &SessionHandleTarget,
    drift: &prism_js::SpecDriftExplanationView,
) -> Result<Vec<RankedCompactFollowup>> {
    let query_tokens = target
        .query
        .as_deref()
        .map(normalize_locate_text)
        .map(|query| locate_query_tokens(&query))
        .unwrap_or_default();
    let mut candidates = Vec::<RankedCompactFollowup>::new();
    let mut seen = HashSet::<String>::new();

    push_ranked_spec_identifier_followups(
        &mut candidates,
        &mut seen,
        spec_identifier_followups(host, prism, target)?.iter(),
        132,
        &query_tokens,
    );
    push_ranked_spec_symbols(
        &mut candidates,
        &mut seen,
        drift.cluster.implementations.iter(),
        "Implementation linked from the spec cluster.",
        140,
        &query_tokens,
        false,
    );
    push_ranked_spec_owners(
        &mut candidates,
        &mut seen,
        drift.cluster.write_path.iter(),
        120,
        &query_tokens,
        false,
    );
    push_ranked_spec_owners(
        &mut candidates,
        &mut seen,
        drift.cluster.read_path.iter(),
        110,
        &query_tokens,
        false,
    );
    push_ranked_spec_owners(
        &mut candidates,
        &mut seen,
        drift.cluster.persistence_path.iter(),
        100,
        &query_tokens,
        false,
    );
    push_ranked_spec_owners(
        &mut candidates,
        &mut seen,
        drift.next_reads.iter(),
        80,
        &query_tokens,
        false,
    );

    let has_token_overlap = candidates
        .iter()
        .any(|candidate| spec_followup_token_overlap(&candidate.symbol, &query_tokens) > 0);
    if has_token_overlap {
        candidates.retain(|candidate| {
            candidate.keep_without_overlap
                || spec_followup_token_overlap(&candidate.symbol, &query_tokens) > 0
        });
    }
    let has_non_test = candidates
        .iter()
        .any(|candidate| !is_test_like_symbol(&candidate.symbol));
    if has_non_test {
        candidates.retain(|candidate| !is_test_like_symbol(&candidate.symbol));
    }

    candidates.sort_by(|left, right| {
        right
            .score
            .cmp(&left.score)
            .then_with(|| left.symbol.id.path.cmp(&right.symbol.id.path))
    });
    Ok(candidates)
}

fn push_ranked_spec_symbols<'a>(
    out: &mut Vec<RankedCompactFollowup>,
    seen: &mut HashSet<String>,
    symbols: impl Iterator<Item = &'a SymbolView>,
    why: &str,
    source_weight: i32,
    query_tokens: &[String],
    keep_without_overlap: bool,
) {
    for symbol in symbols {
        if !seen.insert(symbol.id.path.clone()) {
            continue;
        }
        out.push(RankedCompactFollowup {
            symbol: symbol.clone(),
            why: why.to_string(),
            score: spec_followup_score(symbol, source_weight, query_tokens),
            keep_without_overlap,
        });
    }
}

fn push_ranked_spec_identifier_followups<'a>(
    out: &mut Vec<RankedCompactFollowup>,
    seen: &mut HashSet<String>,
    candidates: impl Iterator<Item = &'a SpecIdentifierFollowup>,
    source_weight: i32,
    query_tokens: &[String],
) {
    for candidate in candidates {
        if !seen.insert(candidate.symbol.id.path.clone()) {
            continue;
        }
        let mut score = spec_followup_score(&candidate.symbol, source_weight, query_tokens);
        if candidate.exact_match {
            score += 48;
        }
        if matches!(candidate.symbol.kind, NodeKind::Function | NodeKind::Method) {
            score += 12;
        }
        out.push(RankedCompactFollowup {
            symbol: candidate.symbol.clone(),
            why: format!(
                "Identifier `{}` lifted from the spec body matched this implementation owner.",
                candidate.term
            ),
            score,
            keep_without_overlap: true,
        });
    }
}

fn push_ranked_spec_owners<'a>(
    out: &mut Vec<RankedCompactFollowup>,
    seen: &mut HashSet<String>,
    owners: impl Iterator<Item = &'a prism_js::OwnerCandidateView>,
    source_weight: i32,
    query_tokens: &[String],
    keep_without_overlap: bool,
) {
    for candidate in owners {
        if !seen.insert(candidate.symbol.id.path.clone()) {
            continue;
        }
        out.push(RankedCompactFollowup {
            symbol: candidate.symbol.clone(),
            why: candidate.why.clone(),
            score: spec_followup_score(&candidate.symbol, source_weight, query_tokens),
            keep_without_overlap,
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
    if is_test_like_symbol(symbol) {
        score -= 80;
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

fn spec_identifier_exact_match(symbol: &SymbolView, normalized_term: &str) -> bool {
    let name = normalize_locate_text(symbol.name.as_str());
    let path = normalize_locate_text(symbol.id.path.as_str());
    name == normalized_term
        || final_segment_normalized(symbol.id.path.as_str()) == normalized_term
        || path.ends_with(normalized_term)
}

fn spec_followup_token_overlap(symbol: &SymbolView, query_tokens: &[String]) -> usize {
    let name = normalize_locate_text(symbol.name.as_str());
    let path = normalize_locate_text(symbol.id.path.as_str());
    query_tokens
        .iter()
        .filter(|token| name.contains(token.as_str()) || path.contains(token.as_str()))
        .count()
}

pub(super) fn resolve_or_select_workset_target(
    host: &QueryHost,
    session: Arc<SessionState>,
    prism: &Prism,
    args: &PrismWorksetArgs,
    query_run: QueryRun,
) -> Result<(SessionHandleTarget, bool)> {
    if let Some(handle) = args.handle.as_deref() {
        return resolve_handle_target(host, session.as_ref(), prism, handle);
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
    resolve_handle_target(host, session.as_ref(), prism, &handle_view.handle)
}
