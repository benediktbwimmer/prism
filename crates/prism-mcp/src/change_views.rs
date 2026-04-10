use std::collections::{HashMap, HashSet};
use std::fs;

use anyhow::Result;
use prism_ir::{AnchorRef, EventActor, FileId, LineageId, NodeId, NodeKind, Span, TaskId};
use prism_js::{
    ChangedFileView, ChangedSymbolView, DiffHunkView, PatchEventView, SourceExcerptView,
    SourceLocationView,
};
use prism_memory::{OutcomeEvent, OutcomeKind, OutcomeRecallQuery};
use prism_query::{Prism, SourceExcerptOptions, source_excerpt_for_span, source_location_for_span};
use serde::Deserialize;

use crate::node_id_view;

const CHANGE_EXCERPT_OPTIONS: SourceExcerptOptions = SourceExcerptOptions {
    context_lines: 0,
    max_lines: 4,
    max_chars: 240,
};
const PATCH_EVENT_SYMBOL_PREVIEW_LIMIT: usize = 16;

#[derive(Debug, Clone, Deserialize, Default)]
#[serde(rename_all = "camelCase")]
struct PatchMetadata {
    trigger: Option<String>,
    reason: Option<String>,
    files: Option<Vec<u32>>,
    file_paths: Option<Vec<String>>,
    changed_files_summary: Option<Vec<PatchChangedFileSummary>>,
    changed_symbols: Option<Vec<PatchChangedSymbol>>,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
struct PatchChangedFileSummary {
    file_path: String,
    changed_symbol_count: usize,
    added_count: usize,
    removed_count: usize,
    updated_count: usize,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
struct PatchChangedSymbol {
    status: String,
    id: Option<NodeId>,
    name: String,
    kind: NodeKind,
    file_path: Option<String>,
    span: Span,
}

#[derive(Debug, Clone)]
struct ParsedPatchEvent {
    event_id: String,
    ts: u64,
    task_id: Option<String>,
    summary: String,
    trigger: Option<String>,
    actor: Option<String>,
    reason: Option<String>,
    work_id: Option<String>,
    work_title: Option<String>,
    files: Vec<String>,
    changed_files_summary: Vec<PatchChangedFileSummary>,
    changed_symbols: Vec<PatchChangedSymbol>,
}

pub(crate) fn changed_files(
    prism: &Prism,
    task_id: Option<&TaskId>,
    since: Option<u64>,
    path: Option<&str>,
    limit: usize,
) -> Result<Vec<ChangedFileView>> {
    let mut seen = HashSet::<String>::new();
    let mut views = Vec::new();
    for event in patch_events(prism, None, task_id, since) {
        let parsed = parse_patch_event(prism, &event);
        for file_path in &parsed.files {
            if path.is_some_and(|filter| !matches_path(file_path, filter)) {
                continue;
            }
            if !seen.insert(file_path.clone()) {
                continue;
            }
            let counts = parsed
                .changed_files_summary
                .iter()
                .find(|summary| summary.file_path == *file_path)
                .map(changed_file_counts_from_summary)
                .unwrap_or_else(|| changed_file_counts(prism, &parsed.changed_symbols, file_path));
            views.push(ChangedFileView {
                path: file_path.clone(),
                event_id: parsed.event_id.clone(),
                ts: parsed.ts,
                task_id: parsed.task_id.clone(),
                trigger: parsed.trigger.clone(),
                actor: parsed.actor.clone(),
                reason: parsed.reason.clone(),
                work_id: parsed.work_id.clone(),
                work_title: parsed.work_title.clone(),
                summary: parsed.summary.clone(),
                changed_symbol_count: counts.changed_symbol_count,
                added_count: counts.added_count,
                removed_count: counts.removed_count,
                updated_count: counts.updated_count,
            });
            if limit > 0 && views.len() >= limit {
                return Ok(views);
            }
        }
    }
    Ok(views)
}

pub(crate) fn changed_symbols(
    prism: &Prism,
    path: &str,
    task_id: Option<&TaskId>,
    since: Option<u64>,
    limit: usize,
) -> Result<Vec<ChangedSymbolView>> {
    changed_symbols_from_events(
        prism,
        patch_events(prism, None, task_id, since),
        path,
        limit,
    )
}

pub(crate) fn changed_symbols_from_events<I>(
    prism: &Prism,
    events: I,
    path: &str,
    limit: usize,
) -> Result<Vec<ChangedSymbolView>>
where
    I: IntoIterator<Item = OutcomeEvent>,
{
    let mut source_cache = HashMap::<String, Option<String>>::new();
    let mut views = Vec::new();
    for event in events {
        let parsed = parse_patch_event(prism, &event);
        for symbol in &parsed.changed_symbols {
            if !symbol_file_path(prism, symbol)
                .is_some_and(|file_path| matches_path(&file_path, path))
            {
                continue;
            }
            views.push(changed_symbol_view(prism, symbol, &mut source_cache)?);
            if limit > 0 && views.len() >= limit {
                return Ok(views);
            }
        }
    }
    Ok(views)
}

pub(crate) fn recent_patches(
    prism: &Prism,
    target: Option<&NodeId>,
    task_id: Option<&TaskId>,
    since: Option<u64>,
    path: Option<&str>,
    limit: usize,
) -> Result<Vec<PatchEventView>> {
    recent_patches_from_events(
        prism,
        patch_events(prism, target, task_id, since),
        path,
        limit,
    )
}

pub(crate) fn recent_patches_from_events<I>(
    prism: &Prism,
    events: I,
    path: Option<&str>,
    limit: usize,
) -> Result<Vec<PatchEventView>>
where
    I: IntoIterator<Item = OutcomeEvent>,
{
    let mut source_cache = HashMap::<String, Option<String>>::new();
    let mut views = Vec::new();
    for event in events {
        let parsed = parse_patch_event(prism, &event);
        if path.is_some_and(|filter| !event_matches_path(prism, &parsed, filter)) {
            continue;
        }
        views.push(patch_event_view(
            prism,
            &parsed,
            &mut source_cache,
            PATCH_EVENT_SYMBOL_PREVIEW_LIMIT,
        )?);
        if limit > 0 && views.len() >= limit {
            return Ok(views);
        }
    }
    Ok(views)
}

pub(crate) fn diff_for(
    prism: &Prism,
    target: Option<&NodeId>,
    lineage_id: Option<&LineageId>,
    task_id: Option<&TaskId>,
    since: Option<u64>,
    limit: usize,
) -> Result<Vec<DiffHunkView>> {
    diff_for_from_events(
        prism,
        patch_events(prism, None, task_id, since),
        target,
        lineage_id,
        limit,
    )
}

pub(crate) fn diff_for_from_events<I>(
    prism: &Prism,
    events: I,
    target: Option<&NodeId>,
    lineage_id: Option<&LineageId>,
    limit: usize,
) -> Result<Vec<DiffHunkView>>
where
    I: IntoIterator<Item = OutcomeEvent>,
{
    let target_lineage = lineage_id
        .cloned()
        .or_else(|| target.and_then(|id| prism.lineage_of(id)));
    let mut source_cache = HashMap::<String, Option<String>>::new();
    let mut views = Vec::new();
    for event in events {
        let parsed = parse_patch_event(prism, &event);
        for symbol in &parsed.changed_symbols {
            if !matches_diff_target(prism, symbol, target, target_lineage.as_ref()) {
                continue;
            }
            views.push(diff_hunk_view(prism, &parsed, symbol, &mut source_cache)?);
            if limit > 0 && views.len() >= limit {
                return Ok(views);
            }
        }
    }
    Ok(views)
}

fn patch_events(
    prism: &Prism,
    target: Option<&NodeId>,
    task_id: Option<&TaskId>,
    since: Option<u64>,
) -> Vec<OutcomeEvent> {
    let mut query = OutcomeRecallQuery {
        kinds: Some(vec![OutcomeKind::PatchApplied]),
        task: task_id.cloned(),
        since,
        limit: 0,
        ..OutcomeRecallQuery::default()
    };
    if let Some(target) = target {
        query.anchors = vec![AnchorRef::Node(target.clone())];
    }
    prism.query_outcomes(&query)
}

fn parse_patch_event(prism: &Prism, event: &OutcomeEvent) -> ParsedPatchEvent {
    let metadata =
        serde_json::from_value::<PatchMetadata>(event.metadata.clone()).unwrap_or_default();
    let task_id = event
        .meta
        .correlation
        .as_ref()
        .map(|task| task.0.to_string());
    ParsedPatchEvent {
        event_id: event.meta.id.0.to_string(),
        ts: event.meta.ts,
        task_id,
        summary: event.summary.clone(),
        trigger: metadata.trigger.clone(),
        actor: patch_actor_label(&event.meta.actor),
        reason: metadata
            .reason
            .clone()
            .or_else(|| patch_reason_from_event(event)),
        work_id: event
            .meta
            .execution_context
            .as_ref()
            .and_then(|context| context.work_context.as_ref())
            .map(|work| work.work_id.clone()),
        work_title: event
            .meta
            .execution_context
            .as_ref()
            .and_then(|context| context.work_context.as_ref())
            .map(|work| work.title.clone()),
        files: patch_files(prism, event, &metadata),
        changed_files_summary: metadata.changed_files_summary.clone().unwrap_or_default(),
        changed_symbols: patch_changed_symbols(prism, event, &metadata),
    }
}

fn patch_files(prism: &Prism, event: &OutcomeEvent, metadata: &PatchMetadata) -> Vec<String> {
    let mut seen = HashSet::<String>::new();
    let mut files = Vec::new();
    if let Some(file_paths) = metadata.file_paths.as_ref() {
        for file_path in file_paths {
            let file_path = portable_file_path(prism, file_path);
            if seen.insert(file_path.clone()) {
                files.push(file_path);
            }
        }
    }
    if files.is_empty() {
        if let Some(file_ids) = metadata.files.as_ref() {
            for file_id in file_ids {
                if let Some(path) = prism.graph().file_path(FileId(*file_id)) {
                    let file_path = path.to_string_lossy().into_owned();
                    if seen.insert(file_path.clone()) {
                        files.push(file_path);
                    }
                }
            }
        }
        for anchor in &event.anchors {
            if let AnchorRef::File(file_id) = anchor {
                if let Some(path) = prism.graph().file_path(*file_id) {
                    let file_path = path.to_string_lossy().into_owned();
                    if seen.insert(file_path.clone()) {
                        files.push(file_path);
                    }
                }
            }
        }
    }
    files
}

fn patch_changed_symbols(
    prism: &Prism,
    event: &OutcomeEvent,
    metadata: &PatchMetadata,
) -> Vec<PatchChangedSymbol> {
    if let Some(changed_symbols) = metadata.changed_symbols.as_ref() {
        return changed_symbols.clone();
    }

    let mut symbols = Vec::new();
    let mut seen = HashSet::<String>::new();
    for anchor in &event.anchors {
        let AnchorRef::Node(id) = anchor else {
            continue;
        };
        let Some(node) = prism.graph().node(id) else {
            continue;
        };
        let key = format!("{}::{}::{:?}", node.id.crate_name, node.id.path, node.kind);
        if !seen.insert(key) {
            continue;
        }
        symbols.push(PatchChangedSymbol {
            status: "changed".to_string(),
            id: Some(node.id.clone()),
            name: node.name.to_string(),
            kind: node.kind,
            file_path: prism
                .graph()
                .file_path(node.file)
                .map(|path| path.to_string_lossy().into_owned()),
            span: node.span,
        });
    }
    symbols
}

fn patch_event_view(
    prism: &Prism,
    event: &ParsedPatchEvent,
    source_cache: &mut HashMap<String, Option<String>>,
    symbol_preview_limit: usize,
) -> Result<PatchEventView> {
    let changed_symbol_count = patch_changed_symbol_count(event);
    let changed_symbols = event
        .changed_symbols
        .iter()
        .take(symbol_preview_limit)
        .map(|symbol| changed_symbol_view(prism, symbol, source_cache))
        .collect::<Result<Vec<_>>>()?;
    Ok(PatchEventView {
        event_id: event.event_id.clone(),
        ts: event.ts,
        task_id: event.task_id.clone(),
        trigger: event.trigger.clone(),
        actor: event.actor.clone(),
        reason: event.reason.clone(),
        work_id: event.work_id.clone(),
        work_title: event.work_title.clone(),
        summary: event.summary.clone(),
        files: event.files.clone(),
        changed_symbol_count,
        changed_symbols_truncated: changed_symbol_count > changed_symbols.len(),
        changed_symbols,
    })
}

fn patch_changed_symbol_count(event: &ParsedPatchEvent) -> usize {
    let summary_total: usize = event
        .changed_files_summary
        .iter()
        .map(|summary| summary.changed_symbol_count)
        .sum();
    if summary_total > 0 {
        summary_total
    } else {
        event.changed_symbols.len()
    }
}

fn patch_actor_label(actor: &EventActor) -> Option<String> {
    match actor {
        EventActor::Principal(principal) => Some(
            principal
                .name
                .clone()
                .unwrap_or_else(|| principal.scoped_id()),
        ),
        EventActor::Agent => Some("agent".to_string()),
        EventActor::User => Some("user".to_string()),
        EventActor::GitAuthor { name, .. } => Some(name.to_string()),
        EventActor::CI => Some("ci".to_string()),
        EventActor::System => None,
    }
}

fn patch_reason_from_event(event: &OutcomeEvent) -> Option<String> {
    event
        .meta
        .execution_context
        .as_ref()
        .and_then(|context| context.work_context.as_ref())
        .map(|work| format!("work {} ({})", work.title, work.work_id))
        .or_else(|| {
            event
                .meta
                .correlation
                .as_ref()
                .map(|task_id| format!("task {}", task_id.0))
        })
}

fn diff_hunk_view(
    prism: &Prism,
    event: &ParsedPatchEvent,
    symbol: &PatchChangedSymbol,
    source_cache: &mut HashMap<String, Option<String>>,
) -> Result<DiffHunkView> {
    Ok(DiffHunkView {
        event_id: event.event_id.clone(),
        ts: event.ts,
        task_id: event.task_id.clone(),
        trigger: event.trigger.clone(),
        summary: event.summary.clone(),
        symbol: changed_symbol_view(prism, symbol, source_cache)?,
    })
}

#[derive(Default)]
struct ChangedFileCounts {
    changed_symbol_count: usize,
    added_count: usize,
    removed_count: usize,
    updated_count: usize,
}

fn changed_file_counts_from_summary(summary: &PatchChangedFileSummary) -> ChangedFileCounts {
    ChangedFileCounts {
        changed_symbol_count: summary.changed_symbol_count,
        added_count: summary.added_count,
        removed_count: summary.removed_count,
        updated_count: summary.updated_count,
    }
}

fn changed_file_counts(
    prism: &Prism,
    changed_symbols: &[PatchChangedSymbol],
    file_path: &str,
) -> ChangedFileCounts {
    let mut counts = ChangedFileCounts::default();
    for symbol in changed_symbols {
        if !symbol_file_path_equals(prism, symbol, file_path) {
            continue;
        }
        counts.changed_symbol_count += 1;
        match changed_symbol_status_bucket(&symbol.status) {
            ChangedSymbolStatusBucket::Added => counts.added_count += 1,
            ChangedSymbolStatusBucket::Removed => counts.removed_count += 1,
            ChangedSymbolStatusBucket::Updated => counts.updated_count += 1,
            ChangedSymbolStatusBucket::Other => {}
        }
    }
    counts
}

fn event_matches_path(prism: &Prism, event: &ParsedPatchEvent, filter: &str) -> bool {
    if event
        .files
        .iter()
        .any(|file_path| matches_path(file_path, filter))
    {
        return true;
    }
    if event
        .changed_symbols
        .iter()
        .filter_map(|symbol| symbol.file_path.as_deref())
        .any(|file_path| matches_path(file_path, filter))
    {
        return true;
    }
    event
        .changed_symbols
        .iter()
        .any(|symbol| symbol.file_path.is_none() && symbol_file_path_matches(prism, symbol, filter))
}

fn matches_diff_target(
    prism: &Prism,
    symbol: &PatchChangedSymbol,
    target: Option<&NodeId>,
    target_lineage: Option<&LineageId>,
) -> bool {
    if let Some(lineage) = target_lineage {
        if symbol
            .id
            .as_ref()
            .and_then(|id| prism.lineage_of(id))
            .as_ref()
            .is_some_and(|candidate| candidate == lineage)
        {
            return true;
        }
    }
    target
        .zip(symbol.id.as_ref())
        .is_some_and(|(expected, candidate)| expected == candidate)
}

fn changed_symbol_view(
    prism: &Prism,
    symbol: &PatchChangedSymbol,
    source_cache: &mut HashMap<String, Option<String>>,
) -> Result<ChangedSymbolView> {
    let file_path = symbol_file_path(prism, symbol).unwrap_or_default();
    let source = if file_path.is_empty() {
        None
    } else {
        cached_source(prism, source_cache, &file_path)
    };
    let (location, excerpt) = source
        .map(|source| {
            (
                Some(source_location_view(source_location_for_span(
                    source,
                    symbol.span.start as usize,
                    symbol.span.end as usize,
                ))),
                Some(source_excerpt_view(source_excerpt_for_span(
                    source,
                    symbol.span.start as usize,
                    symbol.span.end as usize,
                    CHANGE_EXCERPT_OPTIONS.context_lines,
                    CHANGE_EXCERPT_OPTIONS.max_chars,
                ))),
            )
        })
        .unwrap_or((None, None));
    Ok(ChangedSymbolView {
        status: symbol.status.clone(),
        id: symbol.id.clone().map(node_id_view),
        name: symbol.name.clone(),
        kind: symbol.kind,
        file_path,
        location,
        excerpt,
        lineage_id: symbol
            .id
            .as_ref()
            .and_then(|id| prism.lineage_of(id))
            .map(|lineage| lineage.0.to_string()),
    })
}

#[derive(Copy, Clone)]
enum ChangedSymbolStatusBucket {
    Added,
    Removed,
    Updated,
    Other,
}

fn changed_symbol_status_bucket(status: &str) -> ChangedSymbolStatusBucket {
    if status == "added" {
        ChangedSymbolStatusBucket::Added
    } else if status == "removed" {
        ChangedSymbolStatusBucket::Removed
    } else if status == "changed" || status.starts_with("updated") {
        ChangedSymbolStatusBucket::Updated
    } else {
        ChangedSymbolStatusBucket::Other
    }
}

fn symbol_file_path_equals(prism: &Prism, symbol: &PatchChangedSymbol, expected: &str) -> bool {
    let expected = portable_file_path(prism, expected);
    symbol
        .file_path
        .as_deref()
        .map(|path| portable_file_path(prism, path) == expected)
        .unwrap_or_else(|| {
            symbol
                .id
                .as_ref()
                .and_then(|id| prism.graph().node(id))
                .and_then(|node| prism.graph().file_path(node.file))
                .is_some_and(|path| path.to_string_lossy().as_ref() == expected)
        })
}

fn symbol_file_path_matches(prism: &Prism, symbol: &PatchChangedSymbol, filter: &str) -> bool {
    let filter = portable_file_path(prism, filter);
    symbol
        .file_path
        .as_deref()
        .map(|path| matches_path(&portable_file_path(prism, path), &filter))
        .unwrap_or_else(|| {
            symbol
                .id
                .as_ref()
                .and_then(|id| prism.graph().node(id))
                .and_then(|node| prism.graph().file_path(node.file))
                .is_some_and(|path| matches_path(path.to_string_lossy().as_ref(), &filter))
        })
}

fn symbol_file_path(prism: &Prism, symbol: &PatchChangedSymbol) -> Option<String> {
    symbol
        .file_path
        .as_deref()
        .map(|path| portable_file_path(prism, path))
        .or_else(|| {
            symbol.id.as_ref().and_then(|id| {
                prism
                    .graph()
                    .node(id)
                    .and_then(|node| prism.graph().file_path(node.file))
                    .map(|path| path.to_string_lossy().into_owned())
            })
        })
}

fn cached_source<'a>(
    prism: &Prism,
    cache: &'a mut HashMap<String, Option<String>>,
    path: &str,
) -> Option<&'a str> {
    if !cache.contains_key(path) {
        let runtime_path = prism.graph().runtime_path(std::path::Path::new(path));
        cache.insert(path.to_string(), fs::read_to_string(runtime_path).ok());
    }
    cache.get(path).and_then(|value| value.as_deref())
}

fn matches_path(candidate: &str, filter: &str) -> bool {
    candidate == filter || candidate.ends_with(filter) || candidate.contains(filter)
}

fn portable_file_path(prism: &Prism, path: &str) -> String {
    prism
        .graph()
        .portable_path(std::path::Path::new(path))
        .to_string_lossy()
        .into_owned()
}

fn source_location_view(location: prism_query::SourceLocation) -> SourceLocationView {
    SourceLocationView {
        start_line: location.start_line,
        start_column: location.start_column,
        end_line: location.end_line,
        end_column: location.end_column,
    }
}

fn source_excerpt_view(excerpt: prism_query::SourceExcerpt) -> SourceExcerptView {
    SourceExcerptView {
        text: excerpt.text,
        start_line: excerpt.start_line,
        end_line: excerpt.end_line,
        truncated: excerpt.truncated,
    }
}
