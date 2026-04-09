use anyhow::Result;
use prism_ir::TaskId;

use crate::{
    anchor_resource_view_links, dedupe_resource_link_views, derive_task_metadata,
    event_resource_view_link, load_task_journal, load_task_replay, parse_resource_page,
    schema_resource_uri, schema_resource_view_link, schemas_resource_view_link,
    session_resource_view_link, task_resource_view_link, QueryHost, ResolvedTaskMetadata,
    SessionState, TaskResourcePayload, DEFAULT_RESOURCE_PAGE_LIMIT,
    DEFAULT_TASK_JOURNAL_EVENT_LIMIT, DEFAULT_TASK_JOURNAL_MEMORY_LIMIT,
};

pub(crate) fn resolved_task_metadata(
    host: &QueryHost,
    session: &SessionState,
    task_id: &TaskId,
) -> ResolvedTaskMetadata {
    let prism = host.current_prism();
    let replay = load_task_replay(host.workspace_session_ref(), prism.as_ref(), task_id)
        .unwrap_or_else(|_| prism.resume_task(task_id));
    derive_task_metadata(
        session.effective_current_task_state().as_ref(),
        prism.as_ref(),
        task_id,
        &replay.events,
        None,
    )
}

pub(crate) fn task_resource(
    host: &QueryHost,
    session: &SessionState,
    uri: &str,
    task_id: &TaskId,
) -> Result<TaskResourcePayload> {
    let prism = host.current_prism();
    let task_journal = load_task_journal(
        host.workspace_session_ref(),
        session,
        prism.as_ref(),
        task_id,
        None,
        DEFAULT_TASK_JOURNAL_EVENT_LIMIT,
        DEFAULT_TASK_JOURNAL_MEMORY_LIMIT,
    )?;
    let paged = crate::paginate_items(
        task_journal.replay.events,
        parse_resource_page(
            uri,
            DEFAULT_RESOURCE_PAGE_LIMIT,
            session.limits().max_result_nodes,
        )?,
    );
    let mut related_resources = vec![
        session_resource_view_link(),
        task_resource_view_link(task_journal.replay.task.0.as_str()),
        schema_resource_view_link("task"),
        schemas_resource_view_link(),
    ];
    related_resources.extend(
        paged
            .items
            .iter()
            .map(|event| event_resource_view_link(event.meta.id.0.as_str())),
    );
    related_resources.extend(paged.items.iter().flat_map(|event| {
        anchor_resource_view_links(prism.as_ref(), host.workspace_root(), &event.anchors)
    }));
    Ok(TaskResourcePayload {
        uri: uri.to_string(),
        schema_uri: schema_resource_uri("task"),
        task_id: task_journal.replay.task.0.to_string(),
        journal: task_journal.journal,
        events: paged.items,
        page: paged.page,
        truncated: paged.truncated,
        related_resources: dedupe_resource_link_views(related_resources),
    })
}
