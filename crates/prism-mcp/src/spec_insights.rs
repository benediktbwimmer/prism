use std::cmp::Reverse;
use std::collections::{HashMap, HashSet};
use std::thread;

use anyhow::{anyhow, Result};
use prism_ir::{Node, NodeId, NodeKind};
use prism_js::{
    ConfidenceLabel, EvidenceSourceKind, OwnerCandidateView, OwnerHintView,
    SpecDriftExplanationView, SpecImplementationClusterView, SymbolView, TrustSignalsView,
};
use prism_query::{Prism, SourceExcerptOptions};

use crate::{symbol_for, symbol_view, symbol_view_with_owner_hint, symbol_views_for_ids};

const DIRECT_LINK_LIMIT: usize = 12;
pub(crate) const INSIGHT_LIMIT: usize = 6;
const QUERY_TERM_LIMIT: usize = 24;
const ALL_INSIGHT_CATEGORIES: [InsightCategory; 4] = [
    InsightCategory::ReadPath,
    InsightCategory::WritePath,
    InsightCategory::PersistencePath,
    InsightCategory::Tests,
];

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
enum InsightCategory {
    ReadPath,
    WritePath,
    PersistencePath,
    Tests,
}

struct ResolvedSpecTarget {
    spec_id: NodeId,
    notes: Vec<String>,
}

#[derive(Clone)]
struct RankedCandidate {
    id: NodeId,
    category: InsightCategory,
    score: usize,
    matched_terms: Vec<String>,
    why: String,
}

struct SearchScope {
    query_terms: Vec<String>,
    direct_markers: Vec<String>,
    excluded: HashSet<NodeId>,
}

struct CandidateSearchData {
    file_path: String,
    searchable: String,
    behavioral_text: String,
    compact: String,
    matched_terms: Vec<String>,
    excerpt_only: bool,
}

struct RankedCandidateBuckets {
    best_all: HashMap<NodeId, RankedCandidate>,
    best_by_category: HashMap<InsightCategory, HashMap<NodeId, RankedCandidate>>,
}

#[derive(Debug, Clone, Default)]
pub(crate) struct GroupedOwnerCandidateViews {
    pub(crate) all: Vec<OwnerCandidateView>,
    pub(crate) read_path: Vec<OwnerCandidateView>,
    pub(crate) write_path: Vec<OwnerCandidateView>,
    pub(crate) persistence_path: Vec<OwnerCandidateView>,
    pub(crate) tests: Vec<OwnerCandidateView>,
}

pub(crate) fn spec_cluster_view(
    prism: &Prism,
    target: &NodeId,
) -> Result<SpecImplementationClusterView> {
    let resolved = resolve_spec_target(prism, target)?;
    let spec_symbol = symbol_for(prism, &resolved.spec_id)?;
    let spec_view = symbol_view(prism, &spec_symbol)?;
    let relations = spec_symbol.relations();

    let implementations = limit_symbol_views(
        prism,
        dedupe_node_ids(prism.implementation_for(&resolved.spec_id)),
        DIRECT_LINK_LIMIT,
    )?;
    let validations = limit_symbol_views(
        prism,
        dedupe_node_ids(relations.outgoing_validates),
        DIRECT_LINK_LIMIT,
    )?;
    let related = limit_symbol_views(
        prism,
        dedupe_node_ids(relations.outgoing_related),
        DIRECT_LINK_LIMIT,
    )?;

    let direct_ids = implementations
        .iter()
        .map(view_to_id)
        .chain(validations.iter().map(view_to_id))
        .chain(related.iter().map(view_to_id))
        .collect::<Vec<_>>();
    let scope = build_search_scope(
        &spec_symbol.full(),
        spec_symbol.name(),
        &direct_ids,
        std::iter::once(resolved.spec_id.clone())
            .chain(direct_ids.iter().cloned())
            .collect(),
    );

    let grouped = collect_owner_candidates_grouped(prism, &scope, None, None, INSIGHT_LIMIT)?;
    let read_path = grouped.read_path;
    let write_path = grouped.write_path;
    let persistence_path = grouped.persistence_path;
    let tests = grouped.tests;

    Ok(SpecImplementationClusterView {
        spec: spec_view,
        notes: resolved.notes,
        implementations,
        validations,
        related,
        read_path,
        write_path,
        persistence_path,
        tests,
    })
}

pub(crate) fn spec_drift_explanation_view(
    prism: &Prism,
    target: &NodeId,
) -> Result<SpecDriftExplanationView> {
    let cluster = spec_cluster_view(prism, target)?;
    let drift_reasons = prism
        .drift_candidates(prism.graph().nodes.len().max(1))
        .into_iter()
        .find(|candidate| candidate.spec == view_to_id(&cluster.spec))
        .map(|candidate| candidate.reasons)
        .unwrap_or_default();
    let expectations = extract_expectations(&symbol_for(prism, &view_to_id(&cluster.spec))?.full());

    let mut observations = Vec::new();
    observations.push(if cluster.implementations.is_empty() {
        "PRISM did not resolve any direct implementation links for this spec target.".to_string()
    } else {
        format!(
            "PRISM resolved {} direct implementation links: {}.",
            cluster.implementations.len(),
            summarize_symbols(&cluster.implementations)
        )
    });
    observations.push(if cluster.validations.is_empty() {
        "PRISM did not resolve direct validation links for this spec target.".to_string()
    } else {
        format!(
            "PRISM resolved {} validation links: {}.",
            cluster.validations.len(),
            summarize_symbols(&cluster.validations)
        )
    });
    if !cluster.read_path.is_empty() {
        observations.push(format!(
            "Operational read paths surfaced through behavioral owners: {}.",
            summarize_candidates(&cluster.read_path)
        ));
    }
    if !cluster.write_path.is_empty() {
        observations.push(format!(
            "Operational write paths surfaced through behavioral owners: {}.",
            summarize_candidates(&cluster.write_path)
        ));
    }
    if !cluster.persistence_path.is_empty() {
        observations.push(format!(
            "Persistence paths surfaced through behavioral owners: {}.",
            summarize_candidates(&cluster.persistence_path)
        ));
    }
    if !cluster.tests.is_empty() {
        observations.push(format!(
            "Relevant tests surfaced from matching files: {}.",
            summarize_candidates(&cluster.tests)
        ));
    }

    let mut gaps = drift_reasons.clone();
    if cluster.implementations.is_empty() {
        gaps.push("no direct implementation links".to_string());
    }
    if !cluster.implementations.is_empty()
        && cluster.read_path.is_empty()
        && cluster.write_path.is_empty()
        && cluster.persistence_path.is_empty()
    {
        gaps.push(
            "direct links resolve mostly nouns, but few operational owners were found".to_string(),
        );
    }
    if cluster.validations.is_empty() && cluster.tests.is_empty() {
        gaps.push("no validation or test owners were surfaced".to_string());
    }
    gaps.sort();
    gaps.dedup();

    let mut next_reads = Vec::new();
    push_unique_candidates(&mut next_reads, &cluster.read_path, INSIGHT_LIMIT);
    push_unique_candidates(&mut next_reads, &cluster.write_path, INSIGHT_LIMIT);
    push_unique_candidates(&mut next_reads, &cluster.persistence_path, INSIGHT_LIMIT);
    push_unique_candidates(&mut next_reads, &cluster.tests, INSIGHT_LIMIT);
    if next_reads.is_empty() {
        push_unique_symbol_reads(&mut next_reads, &cluster.implementations, INSIGHT_LIMIT);
        push_unique_symbol_reads(&mut next_reads, &cluster.validations, INSIGHT_LIMIT);
        push_unique_symbol_reads(&mut next_reads, &cluster.related, INSIGHT_LIMIT);
    }

    Ok(SpecDriftExplanationView {
        spec: cluster.spec.clone(),
        notes: cluster.notes.clone(),
        drift_reasons,
        expectations,
        observations,
        gaps,
        next_reads,
        trust_signals: drift_trust_signals(&cluster),
        cluster,
    })
}

pub(crate) fn owner_views_for_target(
    prism: &Prism,
    target: &NodeId,
    owner_kind: Option<&str>,
    limit: usize,
) -> Result<Vec<OwnerCandidateView>> {
    ensure_known_owner_kind(owner_kind)?;
    let grouped = grouped_owner_views_for_target(prism, target, limit)?;
    Ok(select_grouped_owner_views(&grouped, owner_kind))
}

pub(crate) fn grouped_owner_views_for_target(
    prism: &Prism,
    target: &NodeId,
    limit: usize,
) -> Result<GroupedOwnerCandidateViews> {
    let symbol = symbol_for(prism, target)?;
    let relations = symbol.relations();
    let mut direct_ids = prism.spec_for(target);
    direct_ids.extend(prism.implementation_for(target));
    direct_ids.extend(relations.outgoing_related);
    direct_ids.extend(relations.incoming_related);
    direct_ids.extend(relations.outgoing_validates);
    direct_ids.extend(relations.incoming_validates);
    direct_ids.extend(relations.outgoing_specifies);
    direct_ids.extend(relations.incoming_specifies);
    let direct_ids = dedupe_node_ids(direct_ids);

    let scope = build_search_scope(
        &symbol.full(),
        symbol.name(),
        &direct_ids,
        std::iter::once(target.clone())
            .chain(direct_ids.iter().cloned())
            .collect(),
    );
    collect_owner_candidates_grouped(prism, &scope, None, None, limit)
}

pub(crate) fn owner_views_for_query(
    prism: &Prism,
    query: &str,
    owner_kind: Option<&str>,
    kind_filter: Option<NodeKind>,
    path_filter: Option<&str>,
    limit: usize,
) -> Result<Vec<OwnerCandidateView>> {
    ensure_known_owner_kind(owner_kind)?;
    let scope = build_search_scope(query, query, &[], HashSet::new());
    let grouped = collect_owner_candidates_grouped(prism, &scope, kind_filter, path_filter, limit)?;
    Ok(select_grouped_owner_views(&grouped, owner_kind))
}

pub(crate) fn owner_symbol_views_for_target(
    prism: &Prism,
    target: &NodeId,
    owner_kind: Option<&str>,
    limit: usize,
) -> Result<Vec<SymbolView>> {
    owner_views_for_target(prism, target, owner_kind, limit)
        .map(|views| views.into_iter().map(|view| view.symbol).collect())
}

pub(crate) fn owner_symbol_views_for_query(
    prism: &Prism,
    query: &str,
    owner_kind: Option<&str>,
    kind_filter: Option<NodeKind>,
    path_filter: Option<&str>,
    limit: usize,
) -> Result<Vec<SymbolView>> {
    owner_views_for_query(prism, query, owner_kind, kind_filter, path_filter, limit)
        .map(|views| views.into_iter().map(|view| view.symbol).collect())
}

fn resolve_spec_target(prism: &Prism, target: &NodeId) -> Result<ResolvedSpecTarget> {
    if is_spec_like(target) {
        return Ok(ResolvedSpecTarget {
            spec_id: target.clone(),
            notes: Vec::new(),
        });
    }

    let mut specs = prism.spec_for(target);
    specs.sort_by(|left, right| left.path.cmp(&right.path));
    specs.dedup();
    match specs.len() {
        0 => Err(anyhow!(
            "target `{}` has no associated spec links",
            target.path
        )),
        1 => {
            let spec_id = specs.remove(0);
            Ok(ResolvedSpecTarget {
                spec_id: spec_id.clone(),
                notes: vec![format!(
                    "Resolved the requested target `{}` to its associated spec `{}`.",
                    target.path, spec_id.path
                )],
            })
        }
        _ => {
            let chosen = specs[0].clone();
            Ok(ResolvedSpecTarget {
                spec_id: chosen.clone(),
                notes: vec![format!(
                    "Target `{}` mapped to multiple specs; using `{}` first. Open `prism.specFor(...)` for the full set.",
                    target.path, chosen.path
                )],
            })
        }
    }
}

fn collect_owner_candidates_grouped(
    prism: &Prism,
    scope: &SearchScope,
    kind_filter: Option<NodeKind>,
    path_filter: Option<&str>,
    limit: usize,
) -> Result<GroupedOwnerCandidateViews> {
    let path_filter = path_filter.map(|value| value.to_ascii_lowercase());
    let path_filter_ref = path_filter.as_deref();
    let nodes = prism.graph().all_nodes().collect::<Vec<_>>();
    let worker_count = owner_candidate_worker_count(nodes.len());
    let mut ranked = if worker_count <= 1 {
        collect_owner_candidates_chunk(prism, &nodes, scope, kind_filter, path_filter_ref)
    } else {
        let chunk_size = nodes.len().div_ceil(worker_count);
        let partials = thread::scope(|scope_threads| {
            let mut tasks = Vec::new();
            for chunk in nodes.chunks(chunk_size) {
                tasks.push(scope_threads.spawn(move || {
                    collect_owner_candidates_chunk(
                        prism,
                        chunk,
                        scope,
                        kind_filter,
                        path_filter_ref,
                    )
                }));
            }
            let mut partials = Vec::with_capacity(tasks.len());
            for task in tasks {
                partials.push(
                    task.join()
                        .expect("owner-candidate worker panicked while scanning nodes"),
                );
            }
            partials
        });
        let mut merged = ranked_candidate_buckets();
        for partial in partials {
            merge_ranked_candidate_buckets(&mut merged, partial);
        }
        merged
    };

    Ok(GroupedOwnerCandidateViews {
        all: build_owner_candidate_views(prism, ranked.best_all.into_values().collect(), limit)?,
        read_path: build_owner_candidate_views(
            prism,
            ranked
                .best_by_category
                .remove(&InsightCategory::ReadPath)
                .unwrap_or_default()
                .into_values()
                .collect(),
            limit,
        )?,
        write_path: build_owner_candidate_views(
            prism,
            ranked
                .best_by_category
                .remove(&InsightCategory::WritePath)
                .unwrap_or_default()
                .into_values()
                .collect(),
            limit,
        )?,
        persistence_path: build_owner_candidate_views(
            prism,
            ranked
                .best_by_category
                .remove(&InsightCategory::PersistencePath)
                .unwrap_or_default()
                .into_values()
                .collect(),
            limit,
        )?,
        tests: build_owner_candidate_views(
            prism,
            ranked
                .best_by_category
                .remove(&InsightCategory::Tests)
                .unwrap_or_default()
                .into_values()
                .collect(),
            limit,
        )?,
    })
}

fn collect_owner_candidates_chunk(
    prism: &Prism,
    nodes: &[&Node],
    scope: &SearchScope,
    kind_filter: Option<NodeKind>,
    path_filter: Option<&str>,
) -> RankedCandidateBuckets {
    let mut ranked = ranked_candidate_buckets();
    for node in nodes {
        if scope.excluded.contains(&node.id) {
            continue;
        }
        if kind_filter.is_some_and(|kind| node.kind != kind) {
            continue;
        }
        if path_filter.is_some_and(|filter| !matches_path_filter(prism, node, filter)) {
            continue;
        }

        let search_data = prepare_candidate_search_data(prism, node, scope);
        for category in ALL_INSIGHT_CATEGORIES {
            if !supports_category(node.kind, category) {
                continue;
            }
            let Some(search_data) = search_data.as_ref() else {
                break;
            };
            let Some(candidate) = score_candidate(node, scope, search_data, category) else {
                continue;
            };
            merge_ranked_candidate(&mut ranked, candidate);
        }
    }
    ranked
}

fn ranked_candidate_buckets() -> RankedCandidateBuckets {
    RankedCandidateBuckets {
        best_all: HashMap::new(),
        best_by_category: ALL_INSIGHT_CATEGORIES
            .into_iter()
            .map(|category| (category, HashMap::<NodeId, RankedCandidate>::new()))
            .collect(),
    }
}

fn merge_ranked_candidate_buckets(
    target: &mut RankedCandidateBuckets,
    source: RankedCandidateBuckets,
) {
    let RankedCandidateBuckets {
        best_all,
        best_by_category,
    } = source;

    for (category, candidates) in best_by_category {
        if let Some(category_best) = target.best_by_category.get_mut(&category) {
            for candidate in candidates.into_values() {
                match category_best.get(&candidate.id) {
                    Some(existing) if !is_better_candidate(&candidate, existing) => {}
                    _ => {
                        category_best.insert(candidate.id.clone(), candidate);
                    }
                }
            }
        }
    }

    for candidate in best_all.into_values() {
        match target.best_all.get(&candidate.id) {
            Some(existing) if !is_better_candidate(&candidate, existing) => {}
            _ => {
                target.best_all.insert(candidate.id.clone(), candidate);
            }
        }
    }
}

fn merge_ranked_candidate(target: &mut RankedCandidateBuckets, candidate: RankedCandidate) {
    if let Some(category_best) = target.best_by_category.get_mut(&candidate.category) {
        match category_best.get(&candidate.id) {
            Some(existing) if !is_better_candidate(&candidate, existing) => {}
            _ => {
                category_best.insert(candidate.id.clone(), candidate.clone());
            }
        }
    }
    match target.best_all.get(&candidate.id) {
        Some(existing) if !is_better_candidate(&candidate, existing) => {}
        _ => {
            target.best_all.insert(candidate.id.clone(), candidate);
        }
    }
}

fn owner_candidate_worker_count(node_count: usize) -> usize {
    if node_count < 2 {
        return 1;
    }
    thread::available_parallelism()
        .map(|count| count.get())
        .unwrap_or(1)
        .min(node_count)
}

fn build_owner_candidate_views(
    prism: &Prism,
    mut ranked: Vec<RankedCandidate>,
    limit: usize,
) -> Result<Vec<OwnerCandidateView>> {
    ranked.sort_by(|left, right| {
        right
            .score
            .cmp(&left.score)
            .then_with(|| right.matched_terms.len().cmp(&left.matched_terms.len()))
            .then_with(|| left.id.path.len().cmp(&right.id.path.len()))
            .then_with(|| left.id.path.cmp(&right.id.path))
    });
    ranked.truncate(limit);

    ranked
        .into_iter()
        .map(|candidate| build_owner_candidate_view(prism, candidate))
        .collect()
}

fn select_grouped_owner_views(
    grouped: &GroupedOwnerCandidateViews,
    owner_kind: Option<&str>,
) -> Vec<OwnerCandidateView> {
    match owner_kind.map(normalize_compact).as_deref() {
        None | Some("") | Some("all") => grouped.all.clone(),
        Some("read") | Some("reader") => grouped.read_path.clone(),
        Some("write") | Some("writer") | Some("mutation") => grouped.write_path.clone(),
        Some("persist") | Some("persistence") | Some("storage") => grouped.persistence_path.clone(),
        Some("test") | Some("tests") | Some("validation") => grouped.tests.clone(),
        Some(_) => grouped.all.clone(),
    }
}

fn ensure_known_owner_kind(filter: Option<&str>) -> Result<()> {
    match filter.map(normalize_compact).as_deref() {
        None | Some("") | Some("all") | Some("read") | Some("reader") | Some("write")
        | Some("writer") | Some("mutation") | Some("persist") | Some("persistence")
        | Some("storage") | Some("test") | Some("tests") | Some("validation") => Ok(()),
        Some(other) => Err(anyhow!("unknown owner kind `{other}`")),
    }
}

fn build_owner_candidate_view(
    prism: &Prism,
    candidate: RankedCandidate,
) -> Result<OwnerCandidateView> {
    let symbol = symbol_for(prism, &candidate.id)?;
    let owner_hint = owner_hint_from_candidate(&candidate);
    Ok(OwnerCandidateView {
        symbol: symbol_view_with_owner_hint(prism, &symbol, Some(owner_hint))?,
        kind: category_label(candidate.category).to_string(),
        score: candidate.score,
        matched_terms: candidate.matched_terms,
        why: candidate.why,
        trust_signals: owner_candidate_trust_signals(false, candidate.score),
    })
}

fn owner_hint_from_candidate(candidate: &RankedCandidate) -> OwnerHintView {
    OwnerHintView {
        kind: category_label(candidate.category).to_string(),
        score: candidate.score,
        matched_terms: candidate.matched_terms.clone(),
        why: candidate.why.clone(),
        trust_signals: owner_candidate_trust_signals(false, candidate.score),
    }
}

fn prepare_candidate_search_data(
    prism: &Prism,
    node: &prism_ir::Node,
    scope: &SearchScope,
) -> Option<CandidateSearchData> {
    let file_path = prism
        .graph()
        .runtime_file_path(node.file)
        .map(|path| path.to_string_lossy().into_owned())
        .unwrap_or_default();
    let file_name = std::path::Path::new(&file_path)
        .file_name()
        .and_then(|name| name.to_str())
        .unwrap_or_default()
        .to_ascii_lowercase();
    let mut searchable = format!("{file_path} {} {}", node.id.path, node.name).to_ascii_lowercase();
    let mut behavioral_text = format!("{file_name} {}", node.name).to_ascii_lowercase();
    let mut matched = matched_terms(&scope.query_terms, &searchable);
    let mut excerpt_only = false;
    if matched.is_empty() {
        if let Ok(symbol) = symbol_for(prism, &node.id) {
            if let Some(excerpt) = symbol.excerpt(SourceExcerptOptions::default()) {
                searchable.push(' ');
                searchable.push_str(&excerpt.text.to_ascii_lowercase());
                behavioral_text.push(' ');
                behavioral_text.push_str(&excerpt.text.to_ascii_lowercase());
                matched = matched_terms(&scope.query_terms, &searchable);
                excerpt_only = !matched.is_empty();
            }
        }
    }
    if matched.is_empty() {
        return None;
    }
    if excerpt_only && is_low_signal_excerpt_only_candidate(&file_path, &node.id.path, &node.name) {
        return None;
    }
    let compact = normalize_compact(&searchable);
    Some(CandidateSearchData {
        file_path,
        searchable,
        behavioral_text,
        compact,
        matched_terms: matched,
        excerpt_only,
    })
}

fn score_candidate(
    node: &prism_ir::Node,
    scope: &SearchScope,
    search_data: &CandidateSearchData,
    category: InsightCategory,
) -> Option<RankedCandidate> {
    let category_bonus = category_bonus(
        category,
        &search_data.behavioral_text,
        &search_data.file_path,
    )?;
    let direct_hits = scope
        .direct_markers
        .iter()
        .filter(|marker| search_data.compact.contains(marker.as_str()))
        .count();
    let kind_bonus = match node.kind {
        NodeKind::Function | NodeKind::Method => 2,
        NodeKind::Module => 1,
        _ => 0,
    };
    let source_bonus = usize::from(
        search_data.searchable.contains(".recall(") || search_data.searchable.contains(".store("),
    );
    let score = search_data.matched_terms.len()
        + category_bonus
        + kind_bonus
        + direct_hits * 3
        + source_bonus;
    let score = score.saturating_sub(search_data.excerpt_only as usize);
    let why = match category {
        InsightCategory::ReadPath => {
            "Matched discovery terms inside read-oriented code paths or excerpts.".to_string()
        }
        InsightCategory::WritePath => {
            "Matched discovery terms inside write-oriented code paths or excerpts.".to_string()
        }
        InsightCategory::PersistencePath => {
            "Matched discovery terms inside persistence-oriented code paths or excerpts."
                .to_string()
        }
        InsightCategory::Tests => {
            "Matched discovery terms inside test-oriented files or excerpts.".to_string()
        }
    };

    Some(RankedCandidate {
        id: node.id.clone(),
        category,
        score,
        matched_terms: search_data.matched_terms.clone(),
        why,
    })
}

fn matched_terms(query_terms: &[String], searchable: &str) -> Vec<String> {
    query_terms
        .iter()
        .filter(|term| searchable.contains(term.as_str()))
        .take(8)
        .cloned()
        .collect::<Vec<_>>()
}

fn is_low_signal_excerpt_only_candidate(file_path: &str, id_path: &str, name: &str) -> bool {
    let file_path = file_path.to_ascii_lowercase();
    let id_path = id_path.to_ascii_lowercase();
    let name = name.to_ascii_lowercase();
    let haystacks = [file_path.as_str(), id_path.as_str(), name.as_str()];
    haystacks.iter().copied().any(|value| {
        contains_any(
            value,
            &[
                "schema_example",
                "schema_examples",
                "payload_example",
                "replay_case",
                "replay_cases",
                "query_replay_cases",
                "fixture",
                "fixtures",
                "testdata",
                "snapshot",
                "snapshots",
                "example",
                "examples",
            ],
        )
    })
}

fn build_search_scope(
    body_text: &str,
    title: &str,
    direct_ids: &[NodeId],
    excluded: HashSet<NodeId>,
) -> SearchScope {
    let query_terms = collect_query_terms(body_text, title, direct_ids);
    let direct_markers = direct_ids
        .iter()
        .map(short_name)
        .map(|value| normalize_compact(&value))
        .filter(|value| !value.is_empty())
        .collect::<Vec<_>>();
    SearchScope {
        query_terms,
        direct_markers,
        excluded,
    }
}

fn limit_symbol_views(
    prism: &Prism,
    mut ids: Vec<NodeId>,
    limit: usize,
) -> Result<Vec<SymbolView>> {
    ids.sort_by(|left, right| left.path.cmp(&right.path));
    ids.truncate(limit);
    symbol_views_for_ids(prism, ids)
}

fn dedupe_node_ids(ids: Vec<NodeId>) -> Vec<NodeId> {
    let mut seen = HashSet::new();
    let mut deduped = Vec::new();
    for id in ids {
        if seen.insert(id.clone()) {
            deduped.push(id);
        }
    }
    deduped
}

fn collect_query_terms(body_text: &str, title: &str, direct_ids: &[NodeId]) -> Vec<String> {
    let mut terms = HashSet::new();
    for token in tokenize_terms(title) {
        terms.insert(token);
    }
    for token in tokenize_terms(body_text) {
        terms.insert(token);
    }
    for id in direct_ids {
        for token in tokenize_terms(&short_name(id)) {
            terms.insert(token);
        }
    }

    let mut ordered = terms.into_iter().collect::<Vec<_>>();
    ordered.sort_by_key(|term| Reverse(term.len()));
    ordered.truncate(QUERY_TERM_LIMIT);
    ordered
}

fn extract_expectations(section_text: &str) -> Vec<String> {
    let mut expectations = Vec::new();
    let mut in_code_block = false;
    for raw_line in section_text.lines() {
        let line = raw_line.trim();
        if line.starts_with("```") {
            in_code_block = !in_code_block;
            continue;
        }
        if in_code_block || line.is_empty() || line.starts_with('#') {
            continue;
        }
        if let Some(bullet) = line.strip_prefix("* ").or_else(|| line.strip_prefix("- ")) {
            expectations.push(normalize_sentence(bullet));
        } else if let Some(numbered) = line.split_once(". ").map(|(_, value)| value) {
            expectations.push(normalize_sentence(numbered));
        } else if expectations.len() < 2 {
            expectations.push(normalize_sentence(line));
        }
        if expectations.len() >= 5 {
            break;
        }
    }
    expectations
}

fn tokenize_terms(value: &str) -> Vec<String> {
    split_identifier_like(value)
        .into_iter()
        .map(|token| token.to_ascii_lowercase())
        .filter(|token| token.len() >= 4 && !is_stopword(token))
        .collect()
}

fn split_identifier_like(value: &str) -> Vec<String> {
    let mut tokens = Vec::new();
    let mut current = String::new();
    let mut previous_lowercase = false;

    for ch in value.chars() {
        if !ch.is_ascii_alphanumeric() {
            if !current.is_empty() {
                tokens.push(current.clone());
                current.clear();
            }
            previous_lowercase = false;
            continue;
        }

        let is_uppercase = ch.is_ascii_uppercase();
        if is_uppercase && previous_lowercase && !current.is_empty() {
            tokens.push(current.clone());
            current.clear();
        }

        current.push(ch);
        previous_lowercase = ch.is_ascii_lowercase();
    }

    if !current.is_empty() {
        tokens.push(current);
    }

    tokens
}

fn normalize_sentence(value: &str) -> String {
    value.split_whitespace().collect::<Vec<_>>().join(" ")
}

fn category_bonus(
    category: InsightCategory,
    behavioral_text: &str,
    file_path: &str,
) -> Option<usize> {
    match category {
        InsightCategory::ReadPath => {
            if contains_any(
                behavioral_text,
                &[
                    "read", "load", "query", "search", "recall", "inspect", "lookup", "resolve",
                    "fetch", "tail", "scan",
                ],
            ) {
                Some(4)
            } else if contains_any(
                behavioral_text,
                &[
                    "context", "journal", "history", "status", "timeline", "log", "logs",
                ],
            ) {
                Some(3)
            } else if contains_any(
                behavioral_text,
                &["view", "resource", "uri", "link", "dashboard"],
            ) {
                Some(1)
            } else {
                None
            }
        }
        InsightCategory::WritePath => contains_any(
            behavioral_text,
            &[
                "write", "store", "mutate", "apply", "record", "update", "finish", "abandon",
                "promote", "infer", "append",
            ],
        )
        .then_some(3),
        InsightCategory::PersistencePath => contains_any(
            behavioral_text,
            &[
                "persist", "snapshot", "load", "save", "sqlite", "commit", "refresh", "cache",
                "reload",
            ],
        )
        .then_some(3),
        InsightCategory::Tests => {
            file_path.to_ascii_lowercase().contains("test")
                || behavioral_text.contains("#[test]")
                || behavioral_text.contains("assert")
        }
        .then_some(4),
    }
}

fn supports_category(kind: NodeKind, category: InsightCategory) -> bool {
    match category {
        InsightCategory::Tests => matches!(
            kind,
            NodeKind::Function | NodeKind::Method | NodeKind::Module
        ),
        _ => matches!(
            kind,
            NodeKind::Function
                | NodeKind::Method
                | NodeKind::Module
                | NodeKind::Struct
                | NodeKind::Trait
        ),
    }
}

fn is_better_candidate(left: &RankedCandidate, right: &RankedCandidate) -> bool {
    left.score > right.score
        || (left.score == right.score
            && (left.matched_terms.len() > right.matched_terms.len()
                || (left.matched_terms.len() == right.matched_terms.len()
                    && left.id.path.len() < right.id.path.len())))
}

fn summarize_symbols(values: &[SymbolView]) -> String {
    values
        .iter()
        .take(4)
        .map(|view| view.id.path.as_str())
        .collect::<Vec<_>>()
        .join(", ")
}

fn summarize_candidates(values: &[OwnerCandidateView]) -> String {
    values
        .iter()
        .take(4)
        .map(|value| value.symbol.id.path.as_str())
        .collect::<Vec<_>>()
        .join(", ")
}

fn push_unique_candidates(
    target: &mut Vec<OwnerCandidateView>,
    source: &[OwnerCandidateView],
    limit: usize,
) {
    let mut seen = target
        .iter()
        .map(|candidate| candidate.symbol.id.path.clone())
        .collect::<HashSet<_>>();
    for candidate in source {
        if target.len() >= limit {
            break;
        }
        if seen.insert(candidate.symbol.id.path.clone()) {
            target.push(candidate.clone());
        }
    }
}

fn push_unique_symbol_reads(
    target: &mut Vec<OwnerCandidateView>,
    source: &[SymbolView],
    limit: usize,
) {
    let mut seen = target
        .iter()
        .map(|candidate| candidate.symbol.id.path.clone())
        .collect::<HashSet<_>>();
    for symbol in source {
        if target.len() >= limit {
            break;
        }
        if seen.insert(symbol.id.path.clone()) {
            let mut symbol = symbol.clone();
            symbol.owner_hint = Some(OwnerHintView {
                kind: "direct".to_string(),
                score: 0,
                matched_terms: Vec::new(),
                why: "Direct spec link surfaced by PRISM intent relations.".to_string(),
                trust_signals: owner_candidate_trust_signals(true, 0),
            });
            target.push(OwnerCandidateView {
                symbol,
                kind: "direct".to_string(),
                score: 0,
                matched_terms: Vec::new(),
                why: "Direct spec link surfaced by PRISM intent relations.".to_string(),
                trust_signals: owner_candidate_trust_signals(true, 0),
            });
        }
    }
}

fn owner_candidate_trust_signals(direct_graph: bool, score: usize) -> TrustSignalsView {
    let confidence_label = if direct_graph {
        ConfidenceLabel::High
    } else if score >= 10 {
        ConfidenceLabel::High
    } else if score >= 6 {
        ConfidenceLabel::Medium
    } else {
        ConfidenceLabel::Low
    };
    let mut why = Vec::new();
    let evidence_sources = if direct_graph {
        why.push(
            "This candidate comes from direct PRISM graph links rather than behavioral ranking."
                .to_string(),
        );
        vec![EvidenceSourceKind::DirectGraph]
    } else {
        why.push(
            "This candidate comes from inferred behavioral ranking over names, paths, and excerpts."
                .to_string(),
        );
        if score >= 10 {
            why.push(
                "Multiple matched terms and category bonuses pushed the heuristic score into the high-confidence range."
                    .to_string(),
            );
        } else if score >= 6 {
            why.push(
                "The ranking found a meaningful term/category match, but it remains heuristic."
                    .to_string(),
            );
        } else {
            why.push(
                "The match is weak enough that it should be confirmed against direct links or follow-up context."
                    .to_string(),
            );
        }
        vec![EvidenceSourceKind::Inferred]
    };
    TrustSignalsView {
        confidence_label,
        evidence_sources,
        why,
    }
}

fn drift_trust_signals(cluster: &SpecImplementationClusterView) -> TrustSignalsView {
    let has_direct_graph = !cluster.implementations.is_empty()
        || !cluster.validations.is_empty()
        || !cluster.related.is_empty();
    let has_inferred = !cluster.read_path.is_empty()
        || !cluster.write_path.is_empty()
        || !cluster.persistence_path.is_empty()
        || !cluster.tests.is_empty();
    let confidence_label = if has_direct_graph && has_inferred {
        ConfidenceLabel::High
    } else if has_direct_graph || has_inferred {
        ConfidenceLabel::Medium
    } else {
        ConfidenceLabel::Low
    };
    let mut evidence_sources = Vec::new();
    let mut why = Vec::new();
    if has_direct_graph {
        evidence_sources.push(EvidenceSourceKind::DirectGraph);
        why.push(
            "The drift explanation is grounded in direct spec, implementation, validation, or related graph links."
                .to_string(),
        );
    }
    if has_inferred {
        evidence_sources.push(EvidenceSourceKind::Inferred);
        why.push(
            "Behavioral owner paths are included as inferred heuristics to fill gaps in direct links."
                .to_string(),
        );
    }
    if !has_direct_graph && !has_inferred {
        why.push(
            "No direct or inferred supporting evidence was found, so this explanation should be treated cautiously."
                .to_string(),
        );
    }
    TrustSignalsView {
        confidence_label,
        evidence_sources,
        why,
    }
}

fn short_name(id: &NodeId) -> String {
    id.path
        .rsplit("::")
        .next()
        .unwrap_or(id.path.as_str())
        .to_string()
}

fn normalize_compact(value: &str) -> String {
    value
        .chars()
        .filter(|ch| ch.is_ascii_alphanumeric())
        .flat_map(char::to_lowercase)
        .collect()
}

fn contains_any(value: &str, needles: &[&str]) -> bool {
    needles.iter().any(|needle| value.contains(needle))
}

fn matches_path_filter(prism: &Prism, node: &prism_ir::Node, path_filter: &str) -> bool {
    prism
        .graph()
        .runtime_file_path(node.file)
        .map(|path| {
            path.to_string_lossy()
                .to_ascii_lowercase()
                .contains(path_filter)
        })
        .unwrap_or(false)
        || node
            .id
            .path
            .as_str()
            .to_ascii_lowercase()
            .contains(path_filter)
        || node
            .name
            .as_str()
            .to_ascii_lowercase()
            .contains(path_filter)
}

fn is_spec_like(id: &NodeId) -> bool {
    matches!(
        id.kind,
        NodeKind::Document
            | NodeKind::MarkdownHeading
            | NodeKind::JsonKey
            | NodeKind::TomlKey
            | NodeKind::YamlKey
    )
}

fn is_stopword(token: &str) -> bool {
    matches!(
        token,
        "about"
            | "after"
            | "also"
            | "anchor"
            | "anchors"
            | "because"
            | "before"
            | "being"
            | "both"
            | "from"
            | "have"
            | "into"
            | "just"
            | "long"
            | "must"
            | "only"
            | "over"
            | "same"
            | "that"
            | "their"
            | "them"
            | "then"
            | "there"
            | "these"
            | "this"
            | "those"
            | "when"
            | "with"
            | "worth"
    )
}

fn category_label(category: InsightCategory) -> &'static str {
    match category {
        InsightCategory::ReadPath => "read",
        InsightCategory::WritePath => "write",
        InsightCategory::PersistencePath => "persist",
        InsightCategory::Tests => "test",
    }
}

fn view_to_id(view: &SymbolView) -> NodeId {
    NodeId::new(view.id.crate_name.clone(), view.id.path.clone(), view.kind)
}
