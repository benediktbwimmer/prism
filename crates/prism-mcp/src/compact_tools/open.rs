use super::expand::source_slice_view;
use super::text_fragments::{
    compact_open_text_fragment, compact_text_fragment_related_handles, read_text_fragment,
};
use super::workset::{
    is_structured_config_target, prioritized_spec_supporting_reads, structured_symbol_followups,
};
use super::*;

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
                    let file_path = symbol_view
                        .file_path
                        .clone()
                        .ok_or_else(|| anyhow!("target `{}` has no workspace file path", target.id.path))?;
                    let related_handles =
                        compact_open_related_handles(host, session.as_ref(), prism.as_ref(), &target)?;

                    match mode {
                        AgentOpenMode::Focus => {
                            let block = focused_block_for_symbol(prism.as_ref(), &symbol, FOCUS_OPEN_OPTIONS)?;
                            compact_open_result_from_block(
                                &args.handle,
                                &file_path,
                                block.slice,
                                block.excerpt,
                                remapped,
                                "Rerun prism_open with mode `raw` if you need the exact file window.",
                                related_handles.clone(),
                            )?
                        }
                        AgentOpenMode::Edit => {
                            let slice = symbol
                                .edit_slice(EDIT_OPEN_OPTIONS)
                                .map(source_slice_view)
                                .ok_or_else(|| anyhow!("target `{}` did not produce an edit slice", target.id.path))?;
                            compact_open_result_from_slice(
                                &args.handle,
                                &file_path,
                                slice,
                                remapped,
                                "Rerun prism_open with mode `raw` if you need the exact file window.",
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
                                })
                            {
                                excerpt
                            } else {
                                let location = symbol
                                    .location()
                                    .ok_or_else(|| anyhow!("target `{}` has no line-addressable source location", target.id.path))?;
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
                                "Rerun prism_open with mode `focus` for a bounded local block or `edit` for an edit-oriented slice.",
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
        next_action: slice.truncated.then(|| next_action.to_string()),
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
        next_action: excerpt.truncated.then(|| next_action.to_string()),
        related_handles,
    })
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
