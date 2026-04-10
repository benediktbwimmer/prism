use std::collections::HashSet;

use anyhow::Result;
use prism_core::WorkspaceSession;
use prism_ir::{AnchorRef, CoordinationTaskId, TaskId};
use prism_js::{QueryDiagnostic, TaskJournalView, TaskLifecycleSummaryView};
use prism_memory::{MemoryModule, OutcomeEvent, OutcomeKind, RecallQuery, TaskReplay};
use prism_query::Prism;

use crate::{query_diagnostic, scored_memory_view, session_state::SessionTaskState, SessionState};

pub(crate) const DEFAULT_TASK_JOURNAL_EVENT_LIMIT: usize = 20;
pub(crate) const DEFAULT_TASK_JOURNAL_MEMORY_LIMIT: usize = 8;

#[derive(Debug, Clone)]
pub(crate) struct LoadedTaskJournal {
    pub(crate) replay: TaskReplay,
    pub(crate) journal: TaskJournalView,
}

#[derive(Debug, Clone, Default)]
pub(crate) struct ResolvedTaskMetadata {
    pub(crate) description: Option<String>,
    pub(crate) tags: Vec<String>,
    pub(crate) coordination_task_id: Option<String>,
}

#[allow(dead_code)]
pub(crate) fn task_journal_view(
    session: &SessionState,
    prism: &Prism,
    task_id: &TaskId,
    metadata_override: Option<(Option<String>, Vec<String>)>,
    event_limit: usize,
    memory_limit: usize,
) -> Result<TaskJournalView> {
    let replay = prism.resume_task(task_id);
    task_journal_view_from_replay(
        session,
        prism,
        replay,
        metadata_override,
        event_limit,
        memory_limit,
    )
}

pub(crate) fn load_task_replay(
    workspace: Option<&WorkspaceSession>,
    prism: &Prism,
    task_id: &TaskId,
) -> Result<TaskReplay> {
    if let Some(workspace) = workspace {
        return workspace
            .load_task_replay(task_id)
            .or_else(|_| Ok(prism.resume_task(task_id)));
    }
    Ok(prism.resume_task(task_id))
}

pub(crate) fn task_journal_view_from_replay(
    session: &SessionState,
    prism: &Prism,
    replay: TaskReplay,
    metadata_override: Option<(Option<String>, Vec<String>)>,
    event_limit: usize,
    memory_limit: usize,
) -> Result<TaskJournalView> {
    let task_id = replay.task;
    let events = replay.events;
    let current_task = session.current_task_state();
    let active = current_task.as_ref().is_some_and(|task| task.id == task_id);
    let metadata = derive_task_metadata(
        current_task.as_ref(),
        prism,
        &task_id,
        &events,
        metadata_override,
    );
    let description = metadata.description;
    let tags = metadata.tags;
    let anchors = task_focus(prism, &events);
    let related_memory = if anchors.is_empty() || memory_limit == 0 {
        Vec::new()
    } else {
        session
            .notes
            .recall(&RecallQuery {
                focus: anchors.clone(),
                text: None,
                limit: memory_limit,
                kinds: None,
                since: None,
            })?
            .into_iter()
            .map(scored_memory_view)
            .collect::<Vec<_>>()
    };
    let summary = summarize_lifecycle(&events);
    let disposition = task_disposition(&events, active);
    let diagnostics = lifecycle_diagnostics(&events, &summary, &disposition);
    let recent_events = if event_limit == 0 {
        events.clone()
    } else {
        events.iter().take(event_limit).cloned().collect()
    };

    Ok(TaskJournalView {
        task_id: task_id.0.to_string(),
        description,
        tags,
        disposition,
        active,
        anchors,
        summary,
        diagnostics,
        related_memory,
        recent_events,
    })
}

pub(crate) fn load_task_journal(
    workspace: Option<&WorkspaceSession>,
    session: &SessionState,
    prism: &Prism,
    task_id: &TaskId,
    metadata_override: Option<(Option<String>, Vec<String>)>,
    event_limit: usize,
    memory_limit: usize,
) -> Result<LoadedTaskJournal> {
    let replay = load_task_replay(workspace, prism, task_id)?;
    let journal = task_journal_view_from_replay(
        session,
        prism,
        replay.clone(),
        metadata_override,
        event_limit,
        memory_limit,
    )?;
    Ok(LoadedTaskJournal { replay, journal })
}

pub(crate) fn derive_task_metadata(
    current_task: Option<&SessionTaskState>,
    prism: &Prism,
    task_id: &TaskId,
    events: &[OutcomeEvent],
    metadata_override: Option<(Option<String>, Vec<String>)>,
) -> ResolvedTaskMetadata {
    let coordination_fallback = coordination_task_fallback(prism, task_id);
    let coordination_task_id = current_task
        .filter(|task| task.id == *task_id)
        .and_then(|task| task.coordination_task_id.clone())
        .or_else(|| {
            coordination_fallback
                .as_ref()
                .map(|(task_id, _)| task_id.clone())
        });

    if let Some(metadata) = metadata_override {
        return ResolvedTaskMetadata {
            description: metadata.0,
            tags: metadata.1,
            coordination_task_id,
        };
    }

    if let Some(task) = current_task {
        if task.id == *task_id {
            return ResolvedTaskMetadata {
                description: task.description.clone(),
                tags: task.tags.clone(),
                coordination_task_id: task.coordination_task_id.clone().or(coordination_task_id),
            };
        }
    }

    let description = events
        .iter()
        .find(|event| event.kind == OutcomeKind::PlanCreated)
        .map(|event| event.summary.clone());
    let tags = events
        .iter()
        .find(|event| event.kind == OutcomeKind::PlanCreated)
        .and_then(|event| event.metadata.get("tags"))
        .and_then(|value| value.as_array())
        .map(|values| {
            values
                .iter()
                .filter_map(|value| value.as_str().map(ToOwned::to_owned))
                .collect::<Vec<_>>()
        })
        .unwrap_or_default();
    if description.is_some() || !tags.is_empty() {
        return ResolvedTaskMetadata {
            description,
            tags,
            coordination_task_id,
        };
    }

    if let Some((coordination_task_id, title)) = coordination_fallback {
        return ResolvedTaskMetadata {
            description: Some(title),
            tags: Vec::new(),
            coordination_task_id: Some(coordination_task_id),
        };
    }

    ResolvedTaskMetadata {
        description,
        tags,
        coordination_task_id,
    }
}

fn coordination_task_fallback(prism: &Prism, task_id: &TaskId) -> Option<(String, String)> {
    let task_id = task_id.0.to_string();
    if !task_id.starts_with("coord-task:") {
        return None;
    }
    prism
        .coordination_task_v2_by_coordination_id(&CoordinationTaskId::new(task_id.clone()))
        .map(|task| (task_id, task.task.title))
}

fn task_focus(prism: &Prism, events: &[OutcomeEvent]) -> Vec<AnchorRef> {
    let mut seen = HashSet::new();
    let anchors = events
        .iter()
        .flat_map(|event| event.anchors.iter().cloned())
        .filter(|anchor| seen.insert(anchor.clone()))
        .collect::<Vec<_>>();
    prism.anchors_for(&anchors)
}

fn summarize_lifecycle(events: &[OutcomeEvent]) -> TaskLifecycleSummaryView {
    let mut summary = TaskLifecycleSummaryView {
        plan_count: 0,
        patch_count: 0,
        build_count: 0,
        test_count: 0,
        failure_count: 0,
        validation_count: 0,
        note_count: 0,
        started_at: None,
        last_updated_at: None,
        final_summary: None,
    };

    for event in events {
        summary.started_at = Some(
            summary
                .started_at
                .map_or(event.meta.ts, |current| current.min(event.meta.ts)),
        );
        summary.last_updated_at = Some(
            summary
                .last_updated_at
                .map_or(event.meta.ts, |current| current.max(event.meta.ts)),
        );
        match event.kind {
            OutcomeKind::PlanCreated => summary.plan_count += 1,
            OutcomeKind::PatchApplied => summary.patch_count += 1,
            OutcomeKind::BuildRan => summary.build_count += 1,
            OutcomeKind::TestRan => summary.test_count += 1,
            OutcomeKind::FailureObserved | OutcomeKind::RegressionObserved => {
                summary.failure_count += 1
            }
            OutcomeKind::FixValidated => summary.validation_count += 1,
            OutcomeKind::NoteAdded => {
                summary.note_count += 1;
                if let Some(disposition) = task_lifecycle_disposition(event) {
                    summary.final_summary = Some(event.summary.clone());
                    if disposition == "completed" || disposition == "abandoned" {
                        summary.last_updated_at = Some(event.meta.ts);
                    }
                }
            }
            _ => {}
        }
    }

    summary
}

fn task_disposition(events: &[OutcomeEvent], active: bool) -> String {
    if let Some(event) = events
        .iter()
        .find(|event| task_lifecycle_disposition(event).is_some())
    {
        return task_lifecycle_disposition(event)
            .unwrap_or("open")
            .to_string();
    }
    if active {
        "active".to_string()
    } else {
        "open".to_string()
    }
}

fn lifecycle_diagnostics(
    events: &[OutcomeEvent],
    summary: &TaskLifecycleSummaryView,
    disposition: &str,
) -> Vec<QueryDiagnostic> {
    let mut diagnostics = Vec::new();

    if !events.is_empty() && summary.plan_count == 0 {
        diagnostics.push(query_diagnostic(
            "missing_plan",
            "Task has outcome history but no explicit plan-start record.",
            None,
        ));
    }

    let last_patch = latest_timestamp(events, |event| event.kind == OutcomeKind::PatchApplied);
    let last_validation = latest_timestamp(events, |event| {
        matches!(
            event.kind,
            OutcomeKind::BuildRan | OutcomeKind::TestRan | OutcomeKind::FixValidated
        )
    });
    if let Some(last_patch_ts) = last_patch {
        if last_validation.is_none_or(|validated| validated < last_patch_ts) {
            diagnostics.push(query_diagnostic(
                "missing_validation",
                "Task recorded a patch but no later build, test, or validation outcome.",
                Some(serde_json::json!({ "lastPatchAt": last_patch_ts })),
            ));
        }
    }

    let last_failure = latest_timestamp(events, |event| {
        matches!(
            event.kind,
            OutcomeKind::FailureObserved | OutcomeKind::RegressionObserved
        )
    });
    let last_fix = latest_timestamp(events, |event| event.kind == OutcomeKind::FixValidated);
    if let Some(last_failure_ts) = last_failure {
        if disposition != "abandoned" && last_fix.is_none_or(|fix| fix < last_failure_ts) {
            diagnostics.push(query_diagnostic(
                "unresolved_failure",
                "Task has a recorded failure without a later fix validation.",
                Some(serde_json::json!({ "lastFailureAt": last_failure_ts })),
            ));
        }
    }

    if disposition == "open" && !events.is_empty() {
        diagnostics.push(query_diagnostic(
            "missing_close_summary",
            "Task has recorded history but no final completion or abandonment summary.",
            None,
        ));
    }

    diagnostics
}

fn latest_timestamp<F>(events: &[OutcomeEvent], predicate: F) -> Option<u64>
where
    F: Fn(&OutcomeEvent) -> bool,
{
    events
        .iter()
        .filter(|event| predicate(event))
        .map(|event| event.meta.ts)
        .max()
}

fn task_lifecycle_disposition(event: &OutcomeEvent) -> Option<&str> {
    event
        .metadata
        .get("taskLifecycle")
        .and_then(|value| value.get("disposition"))
        .and_then(|value| value.as_str())
}
