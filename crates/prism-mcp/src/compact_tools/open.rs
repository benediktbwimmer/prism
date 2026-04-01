use std::path::PathBuf;

use prism_js::AgentSuggestedActionView;

use super::concept::compact_concept_selection;
use super::expand::source_slice_view;
use super::suggested_actions::{
    dedupe_suggested_actions, suggested_expand_action, suggested_open_action,
    suggested_workset_action,
};
use super::text_fragments::{
    compact_open_text_fragment, compact_text_fragment_related_handles, first_non_empty_line,
    read_text_fragment, text_hit_kind,
};
use super::workset::{
    edit_ready_symbol_followups, is_adjacent_spec_handle_view, is_governance_handle_view,
    is_structured_config_target, prioritized_spec_supporting_reads, structured_symbol_followups,
};
use super::*;
use crate::compact_followups::workspace_scoped_path;
use crate::file_queries::file_around;

const STRUCTURED_PREVIEW_FOLLOWUP_LIMIT: usize = 8;
const STRUCTURED_PREVIEW_MAX_CHARS: usize = 240;

impl QueryHost {
    pub(crate) fn compact_open(
        &self,
        session: Arc<SessionState>,
        args: PrismOpenArgs,
    ) -> Result<AgentOpenResultView> {
        let mode = if args.path.is_some() && args.mode.is_none() {
            AgentOpenMode::Raw
        } else {
            agent_open_mode(args.mode.as_ref())
        };
        let target_text = args
            .handle
            .as_deref()
            .or(args.path.as_deref())
            .unwrap_or_default();
        let query_text = format!("prism_open({}, {:?})", target_text, mode);
        self.execute_compact_tool(
            Arc::clone(&session),
            "prism_open",
            query_text,
            move |host, _query_run| {
                if let Some(path) = args.path.as_deref() {
                    let result =
                        compact_open_exact_path(host, session.as_ref(), path, &args, mode)?;
                    return Ok((result, Vec::new()));
                }
                let prism = host.current_prism();
                let handle = args.handle.as_deref().ok_or_else(|| {
                    anyhow!("prism_open requires either a handle or an exact path")
                })?;
                if handle.starts_with("concept://") {
                    let selection =
                        compact_concept_selection(session.as_ref(), prism.as_ref(), handle)?;
                    let (target, _) = resolve_handle_target(
                        host,
                        session.as_ref(),
                        prism.as_ref(),
                        &selection.primary.handle,
                        Some("open"),
                    )?;
                    let related_handles = compact_concept_open_related_handles(
                        &selection.primary.handle,
                        selection.supporting_reads,
                        selection.likely_tests,
                    );
                    let suggested_actions = compact_concept_open_suggested_actions(
                        handle,
                        &selection.packet,
                        related_handles.as_deref(),
                    );
                    let result = compact_open_symbol_result(
                        host,
                        session.as_ref(),
                        prism.as_ref(),
                        handle,
                        crate::session_state::SessionHandleCategory::Concept,
                        mode,
                        &target,
                        false,
                        &compact_concept_open_next_action(
                            &selection.packet,
                            &selection.primary,
                            related_handles.as_deref(),
                        ),
                        Some(selection.primary),
                        related_handles,
                        suggested_actions,
                    )?;
                    return Ok((result, Vec::new()));
                }
                let (target, remapped) = resolve_handle_target(
                    host,
                    session.as_ref(),
                    prism.as_ref(),
                    handle,
                    Some("open"),
                )?;
                let result = if is_text_fragment_target(&target) {
                    compact_open_text_fragment(
                        host,
                        session.as_ref(),
                        handle,
                        mode,
                        &target,
                        remapped,
                    )?
                } else {
                    let related_handles = compact_open_related_handles(
                        host,
                        session.as_ref(),
                        prism.as_ref(),
                        &target,
                    )?;
                    let suggested_actions =
                        compact_open_suggested_actions(handle, &target, related_handles.as_deref());
                    compact_open_symbol_result(
                        host,
                        session.as_ref(),
                        prism.as_ref(),
                        handle,
                        target.handle_category,
                        mode,
                        &target,
                        remapped,
                        &compact_open_next_action(&target),
                        None,
                        related_handles,
                        suggested_actions,
                    )?
                };
                Ok((result, Vec::new()))
            },
        )
    }
}

fn compact_open_symbol_result(
    host: &QueryHost,
    session: &SessionState,
    prism: &Prism,
    display_handle: &str,
    handle_category: crate::session_state::SessionHandleCategory,
    mode: AgentOpenMode,
    target: &SessionHandleTarget,
    remapped: bool,
    next_action: &str,
    promoted_handle: Option<AgentTargetHandleView>,
    related_handles: Option<Vec<AgentTargetHandleView>>,
    suggested_actions: Vec<AgentSuggestedActionView>,
) -> Result<AgentOpenResultView> {
    let symbol_id = target_symbol_id(target)?;
    let symbol = symbol_for(prism, symbol_id)?;
    let symbol_view = symbol_view(prism, &symbol)?;
    let file_path = symbol_view
        .file_path
        .clone()
        .ok_or_else(|| anyhow!("target `{}` has no workspace file path", target.id.path))?;
    let _ = host.ensure_workspace_paths_deep([PathBuf::from(&file_path)])?;
    let base_next_action = next_action;

    match mode {
        AgentOpenMode::Focus => {
            if is_structured_config_target(target.kind) {
                if let Some(preview) = compact_preview_for_structured_target(
                    host,
                    session,
                    prism,
                    display_handle,
                    target,
                )? {
                    return compact_open_result_from_excerpt(
                        display_handle,
                        handle_category,
                        &file_path,
                        SourceExcerptView {
                            text: preview.text,
                            start_line: preview.start_line,
                            end_line: preview.end_line,
                            truncated: preview.truncated,
                        },
                        remapped,
                        &compact_open_adaptive_next_action(
                            target,
                            mode,
                            preview.truncated,
                            &base_next_action,
                            related_handles.as_deref(),
                        ),
                        promoted_handle,
                        related_handles,
                        suggested_actions,
                    );
                }
            }
            let block = focused_block_for_symbol(prism, &symbol, FOCUS_OPEN_OPTIONS)?;
            let block_truncated = block.slice.as_ref().is_some_and(|slice| slice.truncated)
                || block
                    .excerpt
                    .as_ref()
                    .is_some_and(|excerpt| excerpt.truncated);
            compact_open_result_from_block(
                display_handle,
                handle_category,
                &file_path,
                block.slice,
                block.excerpt,
                remapped,
                &compact_open_adaptive_next_action(
                    target,
                    mode,
                    block_truncated,
                    &base_next_action,
                    related_handles.as_deref(),
                ),
                promoted_handle,
                related_handles,
                suggested_actions,
            )
        }
        AgentOpenMode::Edit => {
            let slice = symbol
                .edit_slice(EDIT_OPEN_OPTIONS)
                .map(source_slice_view)
                .ok_or_else(|| {
                    anyhow!("target `{}` did not produce an edit slice", target.id.path)
                })?;
            let slice_truncated = slice.truncated;
            compact_open_result_from_slice(
                display_handle,
                handle_category,
                &file_path,
                slice,
                remapped,
                &compact_open_adaptive_next_action(
                    target,
                    mode,
                    slice_truncated,
                    &base_next_action,
                    related_handles.as_deref(),
                ),
                promoted_handle,
                related_handles,
                suggested_actions,
            )
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
            let excerpt_truncated = excerpt.truncated;
            compact_open_result_from_excerpt(
                display_handle,
                handle_category,
                &file_path,
                excerpt,
                remapped,
                &compact_open_adaptive_next_action(
                    target,
                    mode,
                    excerpt_truncated,
                    &base_next_action,
                    related_handles.as_deref(),
                ),
                promoted_handle,
                related_handles,
                suggested_actions,
            )
        }
    }
}

fn compact_open_exact_path(
    host: &QueryHost,
    session: &SessionState,
    path: &str,
    args: &PrismOpenArgs,
    mode: AgentOpenMode,
) -> Result<AgentOpenResultView> {
    if matches!(mode, AgentOpenMode::Focus) {
        return Err(anyhow!(
            "path-based prism_open currently supports raw mode, or edit mode when `line` is set"
        ));
    }
    let scoped_path = workspace_scoped_path(host.workspace_root(), path);
    let max_chars = Some(args.max_chars.unwrap_or(match mode {
        AgentOpenMode::Focus => RAW_OPEN_MAX_CHARS,
        AgentOpenMode::Edit => EDIT_OPEN_OPTIONS.max_chars,
        AgentOpenMode::Raw => RAW_OPEN_MAX_CHARS,
    }));
    if let Some(line) = args.line {
        let (before, after) = match mode {
            AgentOpenMode::Focus => (args.before_lines, args.after_lines),
            AgentOpenMode::Edit => (
                Some(args.before_lines.unwrap_or(EDIT_OPEN_OPTIONS.before_lines)),
                Some(args.after_lines.unwrap_or(EDIT_OPEN_OPTIONS.after_lines)),
            ),
            AgentOpenMode::Raw => (args.before_lines, args.after_lines),
        };
        let slice = file_around(
            host,
            FileAroundArgs {
                path: scoped_path.clone(),
                line,
                before,
                after,
                max_chars,
            },
        )?;
        let target = session_target_from_exact_path_slice(&scoped_path, &slice);
        let handle = session.intern_target_handle(target);
        return compact_open_result_from_slice(
            &handle,
            crate::session_state::SessionHandleCategory::TextFragment,
            &scoped_path,
            slice,
            false,
            "Use prism_locate with `path` if you need a semantic symbol in this file, or prism_open again with a tighter `line` window.",
            None,
            None,
            Vec::new(),
        );
    }
    if matches!(mode, AgentOpenMode::Edit) {
        return Err(anyhow!(
            "path-based prism_open edit mode requires `line` so the server can center an edit-ready window"
        ));
    }

    let excerpt = file_read(
        host,
        FileReadArgs {
            path: scoped_path.clone(),
            start_line: Some(1),
            end_line: None,
            max_chars,
        },
    )?;
    let target = session_target_from_exact_path_excerpt(&scoped_path, &excerpt);
    let handle = session.intern_target_handle(target);
    compact_open_result_from_excerpt(
        &handle,
        crate::session_state::SessionHandleCategory::TextFragment,
        &scoped_path,
        excerpt,
        false,
        "Use prism_open with `line` for a tighter file window, or prism_locate with `path` for a semantic target in this file.",
        None,
        None,
        Vec::new(),
    )
}

fn session_target_from_exact_path_excerpt(
    path: &str,
    excerpt: &SourceExcerptView,
) -> SessionHandleTarget {
    let kind = text_hit_kind(path);
    let basename = Path::new(path)
        .file_name()
        .and_then(|value| value.to_str())
        .unwrap_or(path)
        .to_string();
    SessionHandleTarget {
        id: NodeId::new(
            TEXT_FRAGMENT_CRATE_NAME,
            format!("{path}:{}", excerpt.start_line),
            kind,
        ),
        lineage_id: None,
        handle_category: crate::session_state::SessionHandleCategory::TextFragment,
        name: format!("{basename}:{}", excerpt.start_line),
        kind,
        file_path: Some(path.to_string()),
        query: first_non_empty_line(&excerpt.text).map(|line| line.to_string()),
        why_short: clamp_string(
            &format!("Exact path open for `{basename}`."),
            MAX_WHY_SHORT_CHARS,
        ),
        start_line: Some(excerpt.start_line),
        end_line: Some(excerpt.end_line),
        start_column: None,
        end_column: None,
    }
}

fn session_target_from_exact_path_slice(
    path: &str,
    slice: &SourceSliceView,
) -> SessionHandleTarget {
    let mut target = session_target_from_exact_path_excerpt(
        path,
        &SourceExcerptView {
            text: slice.text.clone(),
            start_line: slice.start_line,
            end_line: slice.end_line,
            truncated: slice.truncated,
        },
    );
    target.start_column = Some(slice.focus.start_column);
    target.end_column = Some(slice.focus.end_column);
    target
}

pub(super) fn compact_open_result_from_block(
    handle: &str,
    handle_category: crate::session_state::SessionHandleCategory,
    file_path: &str,
    slice: Option<SourceSliceView>,
    excerpt: Option<SourceExcerptView>,
    remapped: bool,
    next_action: &str,
    promoted_handle: Option<AgentTargetHandleView>,
    related_handles: Option<Vec<AgentTargetHandleView>>,
    suggested_actions: Vec<AgentSuggestedActionView>,
) -> Result<AgentOpenResultView> {
    if let Some(slice) = slice {
        return Ok(compact_open_result_from_slice(
            handle,
            handle_category,
            file_path,
            slice,
            remapped,
            next_action,
            promoted_handle,
            related_handles,
            suggested_actions,
        )?);
    }
    if let Some(excerpt) = excerpt {
        return Ok(compact_open_result_from_excerpt(
            handle,
            handle_category,
            file_path,
            excerpt,
            remapped,
            next_action,
            promoted_handle,
            related_handles,
            suggested_actions,
        )?);
    }
    Err(anyhow!("target did not produce any bounded source content"))
}

pub(super) fn compact_open_result_from_slice(
    handle: &str,
    handle_category: crate::session_state::SessionHandleCategory,
    file_path: &str,
    slice: SourceSliceView,
    remapped: bool,
    next_action: &str,
    promoted_handle: Option<AgentTargetHandleView>,
    related_handles: Option<Vec<AgentTargetHandleView>>,
    suggested_actions: Vec<AgentSuggestedActionView>,
) -> Result<AgentOpenResultView> {
    budgeted_open_result(AgentOpenResultView {
        handle: handle.to_string(),
        handle_category: agent_handle_category_view(handle_category),
        file_path: file_path.to_string(),
        start_line: slice.start_line,
        end_line: slice.end_line,
        text: slice.text,
        truncated: slice.truncated,
        remapped,
        freshness: result_freshness(remapped),
        next_action: Some(next_action.to_string()),
        promoted_handle,
        related_handles,
        suggested_actions,
    })
}

pub(super) fn compact_open_result_from_excerpt(
    handle: &str,
    handle_category: crate::session_state::SessionHandleCategory,
    file_path: &str,
    excerpt: SourceExcerptView,
    remapped: bool,
    next_action: &str,
    promoted_handle: Option<AgentTargetHandleView>,
    related_handles: Option<Vec<AgentTargetHandleView>>,
    suggested_actions: Vec<AgentSuggestedActionView>,
) -> Result<AgentOpenResultView> {
    budgeted_open_result(AgentOpenResultView {
        handle: handle.to_string(),
        handle_category: agent_handle_category_view(handle_category),
        file_path: file_path.to_string(),
        start_line: excerpt.start_line,
        end_line: excerpt.end_line,
        text: excerpt.text,
        truncated: excerpt.truncated,
        remapped,
        freshness: result_freshness(remapped),
        next_action: Some(next_action.to_string()),
        promoted_handle,
        related_handles,
        suggested_actions,
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

fn compact_open_adaptive_next_action(
    target: &SessionHandleTarget,
    mode: AgentOpenMode,
    truncated: bool,
    base_next_action: &str,
    related_handles: Option<&[AgentTargetHandleView]>,
) -> String {
    if !truncated || is_text_fragment_target(target) {
        let has_adjacent_spec =
            related_handles.is_some_and(|handles| handles.iter().any(is_adjacent_spec_handle_view));
        if !truncated
            && (is_spec_like_kind(target.kind)
                || target.file_path.as_deref().is_some_and(is_docs_path))
            && related_handles
                .and_then(|handles| handles.first())
                .is_some_and(is_governance_handle_view)
        {
            if has_adjacent_spec {
                return "Use prism_open on the first governing section or adjacent spec, or prism_workset for owners, or prism_expand `drift`.".to_string();
            }
            return "Use prism_open on the first governing section, or prism_workset for owners, or prism_expand `drift`.".to_string();
        }
        if !truncated
            && (is_spec_like_kind(target.kind)
                || target.file_path.as_deref().is_some_and(is_docs_path))
            && related_handles
                .and_then(|handles| handles.first())
                .is_some_and(is_adjacent_spec_handle_view)
        {
            return "Use prism_open on the first adjacent spec, or prism_workset for owners, or prism_expand `drift`.".to_string();
        }
        return base_next_action.to_string();
    }
    if matches!(mode, AgentOpenMode::Edit) {
        if let Some(related) = related_handles.and_then(|handles| handles.first()) {
            return format!(
                "Edit slice hit compact limits for a large or mixed-purpose target. Open related owner block `{}` with prism_open, or use prism_workset for tighter follow-through.",
                related.path
            );
        }
        return "Edit slice hit compact limits for a large or mixed-purpose target. Use prism_workset for decomposition-aware follow-through, or prism_expand `neighbors` to narrow the edit scope.".to_string();
    }
    if (is_spec_like_kind(target.kind) || target.file_path.as_deref().is_some_and(is_docs_path))
        && base_next_action.contains("doc or spec section")
        && base_next_action.contains("code owner")
    {
        return format!(
            "Open hit compact limits for a large or mixed-purpose target. Continue with {}",
            base_next_action
        );
    }
    if let Some(related) = related_handles.and_then(|handles| handles.first()) {
        return format!(
            "Open hit compact limits for a large or mixed-purpose target. Use prism_open on related handle `{}`, or continue with {}",
            related.path, base_next_action
        );
    }
    format!(
        "Open hit compact limits for a large or mixed-purpose target. Continue with {}",
        base_next_action
    )
}

fn compact_concept_open_next_action(
    packet: &prism_query::ConceptPacket,
    primary: &AgentTargetHandleView,
    related_handles: Option<&[AgentTargetHandleView]>,
) -> String {
    if matches!(
        primary.kind,
        prism_ir::NodeKind::MarkdownHeading | prism_ir::NodeKind::Document
    ) {
        if let Some(next_owner) = related_handles.and_then(|handles| handles.first()) {
            return format!(
                "Read this doc or spec section first, then open `{}` to continue into the code owner path.",
                next_owner.path
            );
        }
    }
    if packet
        .decode_lenses
        .iter()
        .any(|lens| matches!(lens, prism_query::ConceptDecodeLens::Validation))
    {
        "Use prism_workset on this concept, or prism_expand `validation` for broader concept context.".to_string()
    } else if packet
        .decode_lenses
        .iter()
        .any(|lens| matches!(lens, prism_query::ConceptDecodeLens::Timeline))
    {
        "Use prism_workset on this concept, or prism_expand `timeline` for broader concept context."
            .to_string()
    } else if packet
        .decode_lenses
        .iter()
        .any(|lens| matches!(lens, prism_query::ConceptDecodeLens::Memory))
    {
        "Use prism_workset on this concept, or prism_expand `memory` for broader concept context."
            .to_string()
    } else {
        "Use prism_workset on this concept, or prism_open on another concept member.".to_string()
    }
}

pub(super) fn budgeted_open_result(mut result: AgentOpenResultView) -> Result<AgentOpenResultView> {
    if let Some(promoted_handle) = result.promoted_handle.as_mut() {
        promoted_handle.file_path = None;
    }
    if let Some(related_handles) = result.related_handles.as_mut() {
        strip_file_paths(related_handles);
        if related_handles.len() > OPEN_RELATED_HANDLE_LIMIT {
            related_handles.truncate(OPEN_RELATED_HANDLE_LIMIT);
        }
    }
    while open_json_bytes(&result)? > OPEN_MAX_JSON_BYTES {
        if result.suggested_actions.len() > 1 {
            result.suggested_actions.pop();
            continue;
        }
        if let Some(related_handles) = result.related_handles.as_mut() {
            related_handles.pop();
            if related_handles.is_empty() {
                result.related_handles = None;
            }
            continue;
        }
        if result.promoted_handle.is_some() {
            result.promoted_handle = None;
            continue;
        }
        break;
    }
    Ok(result)
}

fn compact_concept_open_related_handles(
    primary_handle: &str,
    supporting_reads: Vec<AgentTargetHandleView>,
    likely_tests: Vec<AgentTargetHandleView>,
) -> Option<Vec<AgentTargetHandleView>> {
    let mut seen = HashSet::<String>::new();
    let mut related_handles = Vec::new();
    for handle in supporting_reads.into_iter().chain(likely_tests) {
        if handle.handle == primary_handle || !seen.insert(handle.handle.clone()) {
            continue;
        }
        related_handles.push(handle);
    }
    (!related_handles.is_empty()).then_some(related_handles)
}

fn compact_concept_open_suggested_actions(
    current_handle: &str,
    packet: &prism_query::ConceptPacket,
    related_handles: Option<&[AgentTargetHandleView]>,
) -> Vec<AgentSuggestedActionView> {
    let mut actions = vec![suggested_workset_action(current_handle)];
    if packet
        .decode_lenses
        .iter()
        .any(|lens| matches!(lens, prism_query::ConceptDecodeLens::Validation))
    {
        actions.push(suggested_expand_action(
            current_handle,
            AgentExpandKind::Validation,
        ));
    }
    if packet
        .decode_lenses
        .iter()
        .any(|lens| matches!(lens, prism_query::ConceptDecodeLens::Timeline))
    {
        actions.push(suggested_expand_action(
            current_handle,
            AgentExpandKind::Timeline,
        ));
    }
    if packet
        .decode_lenses
        .iter()
        .any(|lens| matches!(lens, prism_query::ConceptDecodeLens::Memory))
    {
        actions.push(suggested_expand_action(
            current_handle,
            AgentExpandKind::Memory,
        ));
    }
    if actions.len() == 1 {
        if let Some(handle) = related_handles.and_then(|handles| handles.first()) {
            actions.push(suggested_open_action(
                handle.handle.clone(),
                AgentOpenMode::Focus,
            ));
        }
    }
    dedupe_suggested_actions(actions)
}

fn compact_open_suggested_actions(
    current_handle: &str,
    target: &SessionHandleTarget,
    related_handles: Option<&[AgentTargetHandleView]>,
) -> Vec<AgentSuggestedActionView> {
    let first_related = related_handles.and_then(|handles| handles.first());
    let mut actions = Vec::new();

    if is_text_fragment_target(target) {
        if let Some(promoted) =
            super::suggested_actions::strongest_semantic_related_handle(related_handles)
        {
            actions.push(suggested_workset_action(promoted.handle.clone()));
            actions.push(suggested_open_action(promoted.handle, AgentOpenMode::Focus));
        } else {
            actions.push(suggested_workset_action(current_handle));
            actions.push(suggested_expand_action(
                current_handle,
                AgentExpandKind::Neighbors,
            ));
        }
        return dedupe_suggested_actions(actions);
    }

    match (
        is_structured_config_target(target.kind),
        is_spec_like_kind(target.kind) || target.file_path.as_deref().is_some_and(is_docs_path),
    ) {
        (true, _) => {
            actions.push(suggested_expand_action(
                current_handle,
                AgentExpandKind::Validation,
            ));
            if let Some(handle) = first_related {
                actions.push(suggested_open_action(
                    handle.handle.clone(),
                    AgentOpenMode::Focus,
                ));
            } else {
                actions.push(suggested_expand_action(
                    current_handle,
                    AgentExpandKind::Neighbors,
                ));
            }
        }
        (_, true) => {
            actions.push(suggested_workset_action(current_handle));
            if let Some(handle) = first_related {
                actions.push(suggested_open_action(
                    handle.handle.clone(),
                    AgentOpenMode::Focus,
                ));
            }
            actions.push(suggested_expand_action(
                current_handle,
                AgentExpandKind::Drift,
            ));
        }
        _ => {
            actions.push(suggested_workset_action(current_handle));
            if let Some(handle) = first_related {
                actions.push(suggested_open_action(
                    handle.handle.clone(),
                    AgentOpenMode::Focus,
                ));
            } else {
                actions.push(suggested_expand_action(
                    current_handle,
                    AgentExpandKind::Neighbors,
                ));
            }
        }
    }

    dedupe_suggested_actions(actions)
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
    let mut related_handles =
        edit_ready_symbol_followups(host, session, prism, target, OPEN_RELATED_HANDLE_LIMIT)?;
    for handle in &mut related_handles {
        handle.file_path = None;
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
        let (related_target, _) =
            resolve_handle_target(host, session, prism, &related.handle, None)?;
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
