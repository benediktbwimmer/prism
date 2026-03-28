use prism_ir::NodeKind;
use prism_js::{AgentExpandKind, AgentOpenMode, AgentSuggestedActionView, AgentTargetHandleView};

pub(super) fn suggested_workset_action(handle: impl Into<String>) -> AgentSuggestedActionView {
    AgentSuggestedActionView {
        tool: "prism_workset".to_string(),
        handle: Some(handle.into()),
        open_mode: None,
        expand_kind: None,
    }
}

pub(super) fn suggested_open_action(
    handle: impl Into<String>,
    open_mode: AgentOpenMode,
) -> AgentSuggestedActionView {
    AgentSuggestedActionView {
        tool: "prism_open".to_string(),
        handle: Some(handle.into()),
        open_mode: Some(open_mode),
        expand_kind: None,
    }
}

pub(super) fn suggested_expand_action(
    handle: impl Into<String>,
    expand_kind: AgentExpandKind,
) -> AgentSuggestedActionView {
    AgentSuggestedActionView {
        tool: "prism_expand".to_string(),
        handle: Some(handle.into()),
        open_mode: None,
        expand_kind: Some(expand_kind),
    }
}

pub(super) fn strongest_semantic_related_handle(
    related_handles: Option<&[AgentTargetHandleView]>,
) -> Option<AgentTargetHandleView> {
    related_handles.and_then(|handles| {
        handles
            .iter()
            .find(|handle| !matches!(handle.kind, NodeKind::Document))
            .cloned()
    })
}

pub(super) fn dedupe_suggested_actions(
    actions: impl IntoIterator<Item = AgentSuggestedActionView>,
) -> Vec<AgentSuggestedActionView> {
    let mut deduped = Vec::<AgentSuggestedActionView>::new();
    for action in actions {
        if deduped.iter().any(|existing| {
            existing.tool == action.tool
                && existing.handle == action.handle
                && existing.open_mode == action.open_mode
                && existing.expand_kind == action.expand_kind
        }) {
            continue;
        }
        deduped.push(action);
    }
    deduped
}
