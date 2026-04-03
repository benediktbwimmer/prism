use std::collections::{BTreeSet, VecDeque};
use std::sync::{Arc, Mutex};

use prism_ir::{ChangeTrigger, ObservedChangeSet, WorkContextKind};

use crate::util::current_timestamp;
use crate::BoundWorktreePrincipal;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ActiveWorkContextBinding {
    pub work_id: String,
    pub kind: WorkContextKind,
    pub title: String,
    pub summary: Option<String>,
    pub parent_work_id: Option<String>,
    pub coordination_task_id: Option<String>,
    pub plan_id: Option<String>,
    pub plan_title: Option<String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ObservedChangeFlushTrigger {
    MutationBoundary,
    WorkTransition,
    Disconnect,
    ExplicitCheckpoint,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AccumulatedObservedChange {
    pub trigger: ChangeTrigger,
    pub previous_path: Option<String>,
    pub current_path: Option<String>,
    pub file_count: usize,
    pub added_nodes: usize,
    pub removed_nodes: usize,
    pub updated_nodes: usize,
    pub observed_at: u64,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FlushedObservedChangeSet {
    pub principal: BoundWorktreePrincipal,
    pub work: ActiveWorkContextBinding,
    pub trigger: ObservedChangeFlushTrigger,
    pub changed_paths: Vec<String>,
    pub entries: Vec<AccumulatedObservedChange>,
    pub window_started_at: u64,
    pub window_ended_at: u64,
}

#[derive(Default)]
pub(crate) struct ObservedChangeTracker {
    active_work: Option<ActiveWorkContextBinding>,
    active_accumulator: Option<ActiveObservedAccumulator>,
    flushed: VecDeque<FlushedObservedChangeSet>,
}

#[derive(Debug, Clone)]
struct ActiveObservedAccumulator {
    principal: BoundWorktreePrincipal,
    work: ActiveWorkContextBinding,
    changed_paths: BTreeSet<String>,
    entries: Vec<AccumulatedObservedChange>,
    window_started_at: u64,
    window_ended_at: u64,
}

pub(crate) type SharedObservedChangeTracker = Arc<Mutex<ObservedChangeTracker>>;

impl ObservedChangeTracker {
    pub(crate) fn active_work(&self) -> Option<ActiveWorkContextBinding> {
        self.active_work.clone()
    }

    pub(crate) fn set_active_work(&mut self, work: ActiveWorkContextBinding) {
        if self
            .active_work
            .as_ref()
            .is_some_and(|current| current == &work)
        {
            return;
        }
        self.flush_active(ObservedChangeFlushTrigger::WorkTransition);
        self.active_work = Some(work);
    }

    pub(crate) fn clear_active_work(&mut self) {
        self.flush_active(ObservedChangeFlushTrigger::WorkTransition);
        self.active_work = None;
    }

    pub(crate) fn record(
        &mut self,
        principal: Option<BoundWorktreePrincipal>,
        observed: &[ObservedChangeSet],
    ) {
        let Some(principal) = principal else {
            return;
        };
        let Some(work) = self.active_work.clone() else {
            return;
        };
        if observed.is_empty() {
            return;
        }

        let needs_rollover = self
            .active_accumulator
            .as_ref()
            .is_some_and(|active| active.principal != principal || active.work != work);
        if needs_rollover {
            self.flush_active(ObservedChangeFlushTrigger::WorkTransition);
        }

        let started_at = observed
            .first()
            .map(|change| change.meta.ts)
            .unwrap_or_else(current_timestamp);
        let accumulator =
            self.active_accumulator
                .get_or_insert_with(|| ActiveObservedAccumulator {
                    principal: principal.clone(),
                    work: work.clone(),
                    changed_paths: BTreeSet::new(),
                    entries: Vec::new(),
                    window_started_at: started_at,
                    window_ended_at: started_at,
                });

        for change in observed {
            let observed_at = change.meta.ts;
            accumulator.window_ended_at = accumulator.window_ended_at.max(observed_at);
            if accumulator.entries.is_empty() {
                accumulator.window_started_at = observed_at;
            }
            if let Some(path) = change.previous_path.as_ref() {
                accumulator.changed_paths.insert(path.to_string());
            }
            if let Some(path) = change.current_path.as_ref() {
                accumulator.changed_paths.insert(path.to_string());
            }
            accumulator.entries.push(AccumulatedObservedChange {
                trigger: change.trigger.clone(),
                previous_path: change.previous_path.as_ref().map(|path| path.to_string()),
                current_path: change.current_path.as_ref().map(|path| path.to_string()),
                file_count: change.files.len(),
                added_nodes: change.added.len(),
                removed_nodes: change.removed.len(),
                updated_nodes: change.updated.len(),
                observed_at,
            });
        }
    }

    pub(crate) fn flush(&mut self, trigger: ObservedChangeFlushTrigger) -> usize {
        let before = self.flushed.len();
        self.flush_active(trigger);
        self.flushed.len().saturating_sub(before)
    }

    pub(crate) fn take_flushed(&mut self) -> Vec<FlushedObservedChangeSet> {
        self.flushed.drain(..).collect()
    }

    fn flush_active(&mut self, trigger: ObservedChangeFlushTrigger) {
        let Some(active) = self.active_accumulator.take() else {
            return;
        };
        if active.entries.is_empty() {
            return;
        }
        self.flushed.push_back(FlushedObservedChangeSet {
            principal: active.principal,
            work: active.work,
            trigger,
            changed_paths: active.changed_paths.into_iter().collect(),
            entries: active.entries,
            window_started_at: active.window_started_at,
            window_ended_at: active.window_ended_at,
        });
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use prism_ir::{EventActor, EventId, EventMeta, FileId};

    fn active_work(title: &str) -> ActiveWorkContextBinding {
        ActiveWorkContextBinding {
            work_id: format!("work:{title}"),
            kind: WorkContextKind::AdHoc,
            title: title.to_string(),
            summary: Some(format!("summary for {title}")),
            parent_work_id: None,
            coordination_task_id: None,
            plan_id: None,
            plan_title: None,
        }
    }

    fn principal(name: &str) -> BoundWorktreePrincipal {
        BoundWorktreePrincipal {
            authority_id: "local-daemon".to_string(),
            principal_id: format!("principal:{name}"),
            principal_name: name.to_string(),
        }
    }

    fn observed_change(path: &str, ts: u64) -> ObservedChangeSet {
        ObservedChangeSet {
            meta: EventMeta {
                id: EventId::new(format!("event:{ts}")),
                ts,
                actor: EventActor::System,
                correlation: None,
                causation: None,
                execution_context: None,
            },
            trigger: ChangeTrigger::FsWatch,
            files: vec![FileId(1)],
            previous_path: None,
            current_path: Some(path.into()),
            added: Vec::new(),
            removed: Vec::new(),
            updated: Vec::new(),
            edge_added: Vec::new(),
            edge_removed: Vec::new(),
        }
    }

    #[test]
    fn tracker_requires_principal_and_work_before_accumulating() {
        let mut tracker = ObservedChangeTracker::default();
        let change = observed_change("src/lib.rs", 10);

        tracker.record(None, std::slice::from_ref(&change));
        assert!(tracker.take_flushed().is_empty());

        tracker.set_active_work(active_work("declared-work"));
        tracker.record(None, std::slice::from_ref(&change));
        tracker.flush(ObservedChangeFlushTrigger::MutationBoundary);
        assert!(tracker.take_flushed().is_empty());

        tracker.clear_active_work();
        tracker.record(Some(principal("codex-a")), &[change]);
        tracker.flush(ObservedChangeFlushTrigger::MutationBoundary);
        assert!(tracker.take_flushed().is_empty());
    }

    #[test]
    fn tracker_flushes_accumulated_changes_when_work_changes() {
        let mut tracker = ObservedChangeTracker::default();
        tracker.set_active_work(active_work("first"));
        tracker.record(
            Some(principal("codex-a")),
            &[
                observed_change("src/lib.rs", 10),
                observed_change("src/main.rs", 15),
            ],
        );

        tracker.set_active_work(active_work("second"));
        let flushed = tracker.take_flushed();
        assert_eq!(flushed.len(), 1);
        assert_eq!(
            flushed[0].trigger,
            ObservedChangeFlushTrigger::WorkTransition
        );
        assert_eq!(flushed[0].work.work_id, "work:first");
        assert_eq!(
            flushed[0].changed_paths,
            vec!["src/lib.rs".to_string(), "src/main.rs".to_string()]
        );
        assert_eq!(flushed[0].window_started_at, 10);
        assert_eq!(flushed[0].window_ended_at, 15);
    }
}
