use std::collections::{HashMap, HashSet};
use std::fs;

use anyhow::Result;
use prism_ir::{AnchorRef, FileId, NodeId, NodeKind, Span, TaskId};
use prism_js::{
    ChangedFileView, ChangedSymbolView, PatchEventView, SourceExcerptView, SourceLocationView,
};
use prism_memory::{OutcomeEvent, OutcomeKind, OutcomeRecallQuery};
use prism_query::{source_excerpt_for_span, source_location_for_span, Prism, SourceExcerptOptions};
use serde::Deserialize;

use crate::node_id_view;

const CHANGE_EXCERPT_OPTIONS: SourceExcerptOptions = SourceExcerptOptions {
    context_lines: 0,
    max_lines: 4,
    max_chars: 240,
};

#[derive(Debug, Clone, Deserialize, Default)]
#[serde(rename_all = "camelCase")]
struct PatchMetadata {
    trigger: Option<String>,
    files: Option<Vec<u32>>,
    file_paths: Option<Vec<String>>,
    changed_symbols: Option<Vec<PatchChangedSymbol>>,
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
    files: Vec<String>,
    changed_symbols: Vec<PatchChangedSymbol>,
}

pub(crate) fn changed_files(
    prism: &Prism,
    task_id: Option<&TaskId>,
    since: Option<u64>,
    path: Option<&str>,
    limit: usize,
) -> Result<Vec<ChangedFileView>> {
    let mut source_cache = HashMap::<String, Option<String>>::new();
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
            let symbols = parsed
                .changed_symbols
                .iter()
                .filter(|symbol| {
                    symbol_file_path(prism, symbol).is_some_and(|value| value == *file_path)
                })
                .map(|symbol| changed_symbol_view(prism, symbol, &mut source_cache))
                .collect::<Result<Vec<_>>>()?;
            let added_count = symbols
                .iter()
                .filter(|symbol| symbol.status == "added")
                .count();
            let removed_count = symbols
                .iter()
                .filter(|symbol| symbol.status == "removed")
                .count();
            let updated_count = symbols
                .iter()
                .filter(|symbol| symbol.status == "changed" || symbol.status.starts_with("updated"))
                .count();
            views.push(ChangedFileView {
                path: file_path.clone(),
                event_id: parsed.event_id.clone(),
                ts: parsed.ts,
                task_id: parsed.task_id.clone(),
                trigger: parsed.trigger.clone(),
                summary: parsed.summary.clone(),
                changed_symbol_count: symbols.len(),
                added_count,
                removed_count,
                updated_count,
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
    let mut source_cache = HashMap::<String, Option<String>>::new();
    let mut views = Vec::new();
    for event in patch_events(prism, None, task_id, since) {
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
    let mut source_cache = HashMap::<String, Option<String>>::new();
    let mut views = Vec::new();
    for event in patch_events(prism, target, task_id, since) {
        let parsed = parse_patch_event(prism, &event);
        if path.is_some_and(|filter| {
            !parsed
                .files
                .iter()
                .any(|file_path| matches_path(file_path, filter))
                && !parsed
                    .changed_symbols
                    .iter()
                    .filter_map(|symbol| symbol_file_path(prism, symbol))
                    .any(|file_path| matches_path(&file_path, filter))
        }) {
            continue;
        }
        views.push(patch_event_view(prism, &parsed, &mut source_cache)?);
        if limit > 0 && views.len() >= limit {
            return Ok(views);
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
        files: patch_files(prism, event, &metadata),
        changed_symbols: patch_changed_symbols(prism, event, &metadata),
    }
}

fn patch_files(prism: &Prism, event: &OutcomeEvent, metadata: &PatchMetadata) -> Vec<String> {
    let mut seen = HashSet::<String>::new();
    let mut files = Vec::new();
    if let Some(file_paths) = metadata.file_paths.as_ref() {
        for file_path in file_paths {
            if seen.insert(file_path.clone()) {
                files.push(file_path.clone());
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
) -> Result<PatchEventView> {
    Ok(PatchEventView {
        event_id: event.event_id.clone(),
        ts: event.ts,
        task_id: event.task_id.clone(),
        trigger: event.trigger.clone(),
        summary: event.summary.clone(),
        files: event.files.clone(),
        changed_symbols: event
            .changed_symbols
            .iter()
            .map(|symbol| changed_symbol_view(prism, symbol, source_cache))
            .collect::<Result<Vec<_>>>()?,
    })
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
        cached_source(source_cache, &file_path)
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

fn symbol_file_path(prism: &Prism, symbol: &PatchChangedSymbol) -> Option<String> {
    symbol.file_path.clone().or_else(|| {
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
    cache: &'a mut HashMap<String, Option<String>>,
    path: &str,
) -> Option<&'a str> {
    if !cache.contains_key(path) {
        cache.insert(path.to_string(), fs::read_to_string(path).ok());
    }
    cache.get(path).and_then(|value| value.as_deref())
}

fn matches_path(candidate: &str, filter: &str) -> bool {
    candidate == filter || candidate.ends_with(filter) || candidate.contains(filter)
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
