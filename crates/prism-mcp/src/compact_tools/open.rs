use super::expand::source_slice_view;
use super::text_fragments::{
    compact_open_text_fragment, compact_text_fragment_related_handles, read_text_fragment,
};
use super::workset::{
    is_structured_config_target, prioritized_spec_supporting_reads, structured_symbol_followups,
};
use super::*;

const STRUCTURED_PREVIEW_FOLLOWUP_LIMIT: usize = 8;
const STRUCTURED_PREVIEW_MAX_CHARS: usize = 240;

impl QueryHost {
    pub(crate) fn compact_open(
        &self,
        session: Arc<SessionState>,
        args: PrismOpenArgs,
    ) -> Result<AgentOpenResultView> {
        let mode = agent_open_mode(args.mode.as_ref());
        let query_text = format!("prism_open({}, {:?})", args.handle, mode);
        self.execute_compact_tool(
            Arc::clone(&session),
            "prism_open",
            query_text,
            move |host, _query_run| {
                let prism = host.current_prism();
                let (target, remapped) =
                    resolve_handle_target(host, session.as_ref(), prism.as_ref(), &args.handle)?;
                let result = if is_text_fragment_target(&target) {
                    compact_open_text_fragment(
                        host,
                        session.as_ref(),
                        &args.handle,
                        mode,
                        &target,
                        remapped,
                    )?
                } else {
                    let symbol_id = target_symbol_id(&target)?;
                    let symbol = symbol_for(prism.as_ref(), symbol_id)?;
                    let symbol_view = symbol_view(prism.as_ref(), &symbol)?;
                    let file_path = symbol_view.file_path.clone().ok_or_else(|| {
                        anyhow!("target `{}` has no workspace file path", target.id.path)
                    })?;
                    let related_handles = compact_open_related_handles(
                        host,
                        session.as_ref(),
                        prism.as_ref(),
                        &target,
                    )?;
                    let next_action = compact_open_next_action(&target);

                    match mode {
                        AgentOpenMode::Focus => {
                            if is_structured_config_target(target.kind) {
                                if let Some(preview) = compact_preview_for_structured_target(
                                    host,
                                    session.as_ref(),
                                    prism.as_ref(),
                                    &args.handle,
                                    &target,
                                )? {
                                    compact_open_result_from_excerpt(
                                        &args.handle,
                                        &file_path,
                                        SourceExcerptView {
                                            text: preview.text,
                                            start_line: preview.start_line,
                                            end_line: preview.end_line,
                                            truncated: preview.truncated,
                                        },
                                        remapped,
                                        &next_action,
                                        related_handles.clone(),
                                    )?
                                } else {
                                    let block = focused_block_for_symbol(
                                        prism.as_ref(),
                                        &symbol,
                                        FOCUS_OPEN_OPTIONS,
                                    )?;
                                    compact_open_result_from_block(
                                        &args.handle,
                                        &file_path,
                                        block.slice,
                                        block.excerpt,
                                        remapped,
                                        &next_action,
                                        related_handles.clone(),
                                    )?
                                }
                            } else {
                                let block = focused_block_for_symbol(
                                    prism.as_ref(),
                                    &symbol,
                                    FOCUS_OPEN_OPTIONS,
                                )?;
                                compact_open_result_from_block(
                                    &args.handle,
                                    &file_path,
                                    block.slice,
                                    block.excerpt,
                                    remapped,
                                    &next_action,
                                    related_handles.clone(),
                                )?
                            }
                        }
                        AgentOpenMode::Edit => {
                            let slice = symbol
                                .edit_slice(EDIT_OPEN_OPTIONS)
                                .map(source_slice_view)
                                .ok_or_else(|| {
                                    anyhow!(
                                        "target `{}` did not produce an edit slice",
                                        target.id.path
                                    )
                                })?;
                            compact_open_result_from_slice(
                                &args.handle,
                                &file_path,
                                slice,
                                remapped,
                                &next_action,
                                related_handles.clone(),
                            )?
                        }
                        AgentOpenMode::Raw => {
                            let excerpt = if let Some(excerpt) = symbol
                                .excerpt(SourceExcerptOptions {
                                    context_lines: 0,
                                    max_lines: 0,
                                    max_chars: RAW_OPEN_MAX_CHARS,
                                })
                                .map(|excerpt| SourceExcerptView {
                                    text: excerpt.text,
                                    start_line: excerpt.start_line,
                                    end_line: excerpt.end_line,
                                    truncated: excerpt.truncated,
                                }) {
                                excerpt
                            } else {
                                let location = symbol.location().ok_or_else(|| {
                                    anyhow!(
                                        "target `{}` has no line-addressable source location",
                                        target.id.path
                                    )
                                })?;
                                file_read(
                                    host,
                                    FileReadArgs {
                                        path: file_path.clone(),
                                        start_line: Some(location.start_line),
                                        end_line: Some(location.end_line),
                                        max_chars: Some(RAW_OPEN_MAX_CHARS),
                                    },
                                )?
                            };
                            compact_open_result_from_excerpt(
                                &args.handle,
                                &file_path,
                                excerpt,
                                remapped,
                                &next_action,
                                related_handles.clone(),
                            )?
                        }
                    }
                };
                Ok((result, Vec::new()))
            },
        )
    }
}

pub(super) fn compact_open_result_from_block(
    handle: &str,
    file_path: &str,
    slice: Option<SourceSliceView>,
    excerpt: Option<SourceExcerptView>,
    remapped: bool,
    next_action: &str,
    related_handles: Option<Vec<AgentTargetHandleView>>,
) -> Result<AgentOpenResultView> {
    if let Some(slice) = slice {
        return Ok(compact_open_result_from_slice(
            handle,
            file_path,
            slice,
            remapped,
            next_action,
            related_handles,
        )?);
    }
    if let Some(excerpt) = excerpt {
        return Ok(compact_open_result_from_excerpt(
            handle,
            file_path,
            excerpt,
            remapped,
            next_action,
            related_handles,
        )?);
    }
    Err(anyhow!("target did not produce any bounded source content"))
}

pub(super) fn compact_open_result_from_slice(
    handle: &str,
    file_path: &str,
    slice: SourceSliceView,
    remapped: bool,
    next_action: &str,
    related_handles: Option<Vec<AgentTargetHandleView>>,
) -> Result<AgentOpenResultView> {
    budgeted_open_result(AgentOpenResultView {
        handle: handle.to_string(),
        file_path: file_path.to_string(),
        start_line: slice.start_line,
        end_line: slice.end_line,
        text: slice.text,
        truncated: slice.truncated,
        remapped,
        next_action: Some(next_action.to_string()),
        related_handles,
    })
}

pub(super) fn compact_open_result_from_excerpt(
    handle: &str,
    file_path: &str,
    excerpt: SourceExcerptView,
    remapped: bool,
    next_action: &str,
    related_handles: Option<Vec<AgentTargetHandleView>>,
) -> Result<AgentOpenResultView> {
    budgeted_open_result(AgentOpenResultView {
        handle: handle.to_string(),
        file_path: file_path.to_string(),
        start_line: excerpt.start_line,
        end_line: excerpt.end_line,
        text: excerpt.text,
        truncated: excerpt.truncated,
        remapped,
        next_action: Some(next_action.to_string()),
        related_handles,
    })
}

fn compact_open_next_action(target: &SessionHandleTarget) -> String {
    if is_text_fragment_target(target) {
        "Use prism_workset here, or prism_gather for tighter slices.".to_string()
    } else if is_structured_config_target(target.kind) {
        "Use prism_expand `validation` or `neighbors`.".to_string()
    } else if is_spec_like_kind(target.kind)
        || target.file_path.as_deref().is_some_and(is_docs_path)
    {
        "Use prism_workset for owners, or prism_expand `drift`.".to_string()
    } else {
        "Use prism_workset for reads/tests, or prism_expand `neighbors`.".to_string()
    }
}

pub(super) fn budgeted_open_result(mut result: AgentOpenResultView) -> Result<AgentOpenResultView> {
    if let Some(related_handles) = result.related_handles.as_mut() {
        strip_file_paths(related_handles);
        if related_handles.len() > OPEN_RELATED_HANDLE_LIMIT {
            related_handles.truncate(OPEN_RELATED_HANDLE_LIMIT);
        }
    }
    while open_json_bytes(&result)? > OPEN_MAX_JSON_BYTES {
        let Some(related_handles) = result.related_handles.as_mut() else {
            break;
        };
        related_handles.pop();
        if related_handles.is_empty() {
            result.related_handles = None;
        }
    }
    Ok(result)
}

fn compact_open_related_handles(
    host: &QueryHost,
    session: &SessionState,
    prism: &Prism,
    target: &SessionHandleTarget,
) -> Result<Option<Vec<AgentTargetHandleView>>> {
    if is_text_fragment_target(target) {
        return compact_text_fragment_related_handles(host, session, target);
    }
    if is_spec_like_kind(target.kind) || target.file_path.as_deref().is_some_and(is_docs_path) {
        let drift = spec_drift_explanation_view(prism, &target.id)?;
        let related = prioritized_spec_supporting_reads(
            host,
            session,
            prism,
            target,
            &drift,
            OPEN_RELATED_HANDLE_LIMIT,
        )?;
        return Ok((!related.is_empty()).then_some(related));
    }
    if is_structured_config_target(target.kind) {
        let related =
            structured_symbol_followups(host, session, prism, target, OPEN_RELATED_HANDLE_LIMIT)?;
        return Ok((!related.is_empty()).then_some(related));
    }
    let mut seen = HashSet::<String>::new();
    let mut related_handles = Vec::new();
    for candidate in next_reads(
        prism,
        target_symbol_id(target)?,
        OPEN_RELATED_HANDLE_LIMIT + 1,
    )? {
        if candidate.symbol.id.crate_name == target.id.crate_name
            && candidate.symbol.id.path == target.id.path
            && candidate.symbol.kind == target.kind
        {
            continue;
        }
        if !seen.insert(candidate.symbol.id.path.clone()) {
            continue;
        }
        let mut handle = compact_target_view(
            session,
            &candidate.symbol,
            target.query.as_deref(),
            Some(candidate.why),
        );
        handle.file_path = None;
        related_handles.push(handle);
        if related_handles.len() >= OPEN_RELATED_HANDLE_LIMIT {
            break;
        }
    }
    Ok((!related_handles.is_empty()).then_some(related_handles))
}

pub(super) fn compact_preview_for_symbol_view(
    prism: &Prism,
    handle: &str,
    symbol: &SymbolView,
) -> Result<Option<AgentTextPreviewView>> {
    let id = NodeId::new(
        symbol.id.crate_name.clone(),
        symbol.id.path.clone(),
        symbol.kind,
    );
    let symbol = symbol_for(prism, &id)?;
    let file_path = symbol_view(prism, &symbol)?
        .file_path
        .as_deref()
        .ok_or_else(|| anyhow!("target `{}` has no workspace file path", id.path))?
        .to_string();
    let block = focused_block_for_symbol(prism, &symbol, PREVIEW_OPEN_OPTIONS)?;
    Ok(match (block.slice, block.excerpt) {
        (Some(slice), _) => Some(AgentTextPreviewView {
            handle: handle.to_string(),
            file_path,
            start_line: slice.start_line,
            end_line: slice.end_line,
            text: slice.text,
            truncated: slice.truncated,
        }),
        (None, Some(excerpt)) => Some(AgentTextPreviewView {
            handle: handle.to_string(),
            file_path,
            start_line: excerpt.start_line,
            end_line: excerpt.end_line,
            text: excerpt.text,
            truncated: excerpt.truncated,
        }),
        (None, None) => None,
    })
}

pub(super) fn compact_preview_for_structured_target(
    host: &QueryHost,
    session: &SessionState,
    prism: &Prism,
    handle: &str,
    target: &SessionHandleTarget,
) -> Result<Option<AgentTextPreviewView>> {
    if !is_structured_config_target(target.kind) {
        return Ok(None);
    }
    let symbol_id = target_symbol_id(target)?;
    let symbol = symbol_for(prism, symbol_id)?;
    let symbol_view = symbol_view(prism, &symbol)?;
    let file_path = symbol_view
        .file_path
        .clone()
        .or_else(|| target.file_path.clone())
        .ok_or_else(|| anyhow!("target `{}` has no workspace file path", target.id.path))?;
    let mut segments = Vec::<StructuredPreviewSegment>::new();
    if let Some(segment) =
        structured_preview_segment_for_symbol(host, &file_path, &symbol, target.id.path.as_str())?
    {
        segments.push(segment);
    }
    for related in structured_symbol_followups(
        host,
        session,
        prism,
        target,
        STRUCTURED_PREVIEW_FOLLOWUP_LIMIT,
    )? {
        let (related_target, _) = resolve_handle_target(host, session, prism, &related.handle)?;
        let related_symbol_id = target_symbol_id(&related_target)?;
        let related_symbol = symbol_for(prism, related_symbol_id)?;
        if let Some(segment) = structured_preview_segment_for_symbol(
            host,
            &file_path,
            &related_symbol,
            related_target.id.path.as_str(),
        )? {
            segments.push(segment);
        }
    }
    segments.sort_by_key(|segment| (segment.start_line, segment.path.clone()));
    segments.dedup_by(|left, right| left.start_line == right.start_line && left.path == right.path);
    if segments.is_empty() {
        return Ok(None);
    }
    let start_line = segments
        .first()
        .map(|segment| segment.start_line)
        .unwrap_or(1);
    let end_line = segments
        .last()
        .map(|segment| segment.end_line)
        .unwrap_or(start_line);
    let mut text = String::new();
    let mut previous_end_line: Option<usize> = None;
    let mut truncated = false;
    for segment in segments {
        if !text.is_empty() {
            if previous_end_line.is_some_and(|line| segment.start_line > line.saturating_add(1)) {
                text.push_str("\n...\n");
                truncated = true;
            } else {
                text.push('\n');
            }
        }
        text.push_str(segment.text.trim_end_matches('\n'));
        truncated |= segment.truncated;
        previous_end_line = Some(segment.end_line);
    }
    Ok(Some(AgentTextPreviewView {
        handle: handle.to_string(),
        file_path,
        start_line,
        end_line,
        text,
        truncated,
    }))
}

#[derive(Debug, Clone)]
struct StructuredPreviewSegment {
    path: String,
    start_line: usize,
    end_line: usize,
    text: String,
    truncated: bool,
}

fn structured_preview_segment_for_symbol(
    host: &QueryHost,
    file_path: &str,
    symbol: &prism_query::Symbol,
    path: &str,
) -> Result<Option<StructuredPreviewSegment>> {
    if let Some(excerpt) = symbol
        .excerpt(SourceExcerptOptions {
            context_lines: 0,
            max_lines: 0,
            max_chars: STRUCTURED_PREVIEW_MAX_CHARS,
        })
        .map(|excerpt| SourceExcerptView {
            text: excerpt.text,
            start_line: excerpt.start_line,
            end_line: excerpt.end_line,
            truncated: excerpt.truncated,
        })
    {
        return Ok(Some(StructuredPreviewSegment {
            path: path.to_string(),
            start_line: excerpt.start_line,
            end_line: excerpt.end_line,
            text: excerpt.text,
            truncated: excerpt.truncated,
        }));
    }
    let Some(location) = symbol.location() else {
        return Ok(None);
    };
    let excerpt = file_read(
        host,
        FileReadArgs {
            path: file_path.to_string(),
            start_line: Some(location.start_line),
            end_line: Some(location.end_line),
            max_chars: Some(STRUCTURED_PREVIEW_MAX_CHARS),
        },
    )?;
    Ok(Some(StructuredPreviewSegment {
        path: path.to_string(),
        start_line: excerpt.start_line,
        end_line: excerpt.end_line,
        text: excerpt.text,
        truncated: excerpt.truncated,
    }))
}

pub(super) fn compact_preview_for_ranked_target(
    host: &QueryHost,
    prism: &Prism,
    handle: &str,
    target: &RankedLocateTarget,
) -> Result<Option<AgentTextPreviewView>> {
    match target {
        RankedLocateTarget::Symbol(symbol) => {
            compact_preview_for_symbol_view(prism, handle, symbol)
        }
        RankedLocateTarget::Text(text_target) => {
            compact_preview_for_text_target(host, handle, text_target)
        }
    }
}

pub(super) fn compact_preview_for_text_target(
    host: &QueryHost,
    handle: &str,
    target: &SessionHandleTarget,
) -> Result<Option<AgentTextPreviewView>> {
    let excerpt = read_text_fragment(
        host,
        target,
        target.start_line.unwrap_or(1),
        target.end_line.unwrap_or(1),
        PREVIEW_OPEN_OPTIONS.max_chars,
    )?;
    Ok(Some(AgentTextPreviewView {
        handle: handle.to_string(),
        file_path: target.file_path.clone().unwrap_or_default(),
        start_line: excerpt.start_line,
        end_line: excerpt.end_line,
        text: excerpt.text,
        truncated: excerpt.truncated,
    }))
}
