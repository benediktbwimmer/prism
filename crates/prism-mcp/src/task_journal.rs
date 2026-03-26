use std::collections::HashSet;

use anyhow::Result;
use prism_ir::{AnchorRef, TaskId};
use prism_js::{QueryDiagnostic, TaskJournalView, TaskLifecycleSummaryView};
use prism_memory::{MemoryModule, OutcomeEvent, OutcomeKind, RecallQuery};
use prism_query::Prism;

use crate::{scored_memory_view, session_state::SessionTaskState, SessionState};

pub(crate) const DEFAULT_TASK_JOURNAL_EVENT_LIMIT: usize = 20;
pub(crate) const DEFAULT_TASK_JOURNAL_MEMORY_LIMIT: usize = 8;

pub(crate) fn task_journal_view(
    session: &SessionState,
    prism: &Prism,
    task_id: &TaskId,
    metadata_override: Option<(Option<String>, Vec<String>)>,
    event_limit: usize,
    memory_limit: usize,
) -> Result<TaskJournalView> {
    let replay = prism.resume_task(task_id);
    let events = replay.events;
    let current_task = session.current_task_state();
    let active = current_task.as_ref().is_some_and(|task| task.id == *task_id);
    let (description, tags) =
        derive_task_metadata(current_task.as_ref(), task_id, &events, metadata_override);
    let anchors = task_focus(prism, &events);
    let related_memory = if anchors.is_empty() {
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

pub(crate) fn derive_task_metadata(
    current_task: Option<&SessionTaskState>,
    task_id: &TaskId,
    events: &[OutcomeEvent],
    metadata_override: Option<(Option<String>, Vec<String>)>,
) -> (Option<String>, Vec<String>) {
    if let Some(metadata) = metadata_override {
        return metadata;
    }

    if let Some(task) = current_task {
        if task.id == *task_id {
            return (task.description.clone(), task.tags.clone());
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
    (description, tags)
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
            OutcomeKind::FailureObserved | OutcomeKind::RegressionObserved => summary.failure_count += 1,
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
    if let Some(event) = events.iter().find(|event| task_lifecycle_disposition(event).is_some()) {
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

    if summary.plan_count == 0 {
        diagnostics.push(QueryDiagnostic {
            code: "missing_plan".to_string(),
            message: "Task has outcome history but no explicit plan-start record.".to_string(),
            data: None,
        });
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
            diagnostics.push(QueryDiagnostic {
                code: "missing_validation".to_string(),
                message: "Task recorded a patch but no later build, test, or validation outcome."
                    .to_string(),
                data: Some(serde_json::json!({ "lastPatchAt": last_patch_ts })),
            });
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
            diagnostics.push(QueryDiagnostic {
                code: "unresolved_failure".to_string(),
                message: "Task has a recorded failure without a later fix validation.".to_string(),
                data: Some(serde_json::json!({ "lastFailureAt": last_failure_ts })),
            });
        }
    }

    if disposition == "open" && !events.is_empty() {
        diagnostics.push(QueryDiagnostic {
            code: "missing_close_summary".to_string(),
            message: "Task has recorded history but no final completion or abandonment summary."
                .to_string(),
            data: None,
        });
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
    event.metadata
        .get("taskLifecycle")
        .and_then(|value| value.get("disposition"))
        .and_then(|value| value.as_str())
}
