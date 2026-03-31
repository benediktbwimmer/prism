use std::collections::HashSet;

use prism_ir::{AnchorRef, EventId, NodeId, TaskId};
use prism_memory::{OutcomeEvent, OutcomeKind, OutcomeRecallQuery, OutcomeResult, TaskReplay};

use crate::Prism;

impl Prism {
    pub fn query_outcomes(&self, query: &OutcomeRecallQuery) -> Vec<OutcomeEvent> {
        let mut expanded = query.clone();
        expanded.anchors = self.expand_anchors(&query.anchors);
        let hot = self.outcomes.query_events(&expanded);
        let cold = self
            .outcome_backend
            .read()
            .expect("outcome backend lock poisoned")
            .as_ref()
            .and_then(|backend| backend.query_outcomes(&expanded).ok())
            .unwrap_or_default();
        merge_outcome_events(hot, cold, query.limit)
    }

    pub fn outcomes_for(&self, anchors: &[AnchorRef], limit: usize) -> Vec<OutcomeEvent> {
        self.query_outcomes(&OutcomeRecallQuery {
            anchors: anchors.to_vec(),
            limit,
            ..OutcomeRecallQuery::default()
        })
    }

    pub fn related_failures(&self, node: &NodeId) -> Vec<OutcomeEvent> {
        self.query_outcomes(&OutcomeRecallQuery {
            anchors: vec![AnchorRef::Node(node.clone())],
            kinds: Some(vec![
                OutcomeKind::FailureObserved,
                OutcomeKind::RegressionObserved,
            ]),
            result: Some(OutcomeResult::Failure),
            limit: 20,
            ..OutcomeRecallQuery::default()
        })
    }

    pub fn resume_task(&self, task: &TaskId) -> TaskReplay {
        let hot = self.outcomes.resume_task(task);
        let cold = self
            .outcome_backend
            .read()
            .expect("outcome backend lock poisoned")
            .as_ref()
            .and_then(|backend| backend.load_task_replay(task).ok())
            .unwrap_or(TaskReplay {
                task: task.clone(),
                events: Vec::new(),
            });
        TaskReplay {
            task: task.clone(),
            events: merge_outcome_events(hot.events, cold.events, 0),
        }
    }
}

fn merge_outcome_events(
    hot: Vec<OutcomeEvent>,
    cold: Vec<OutcomeEvent>,
    limit: usize,
) -> Vec<OutcomeEvent> {
    let mut seen = HashSet::<EventId>::new();
    let mut events = Vec::new();
    for event in hot.into_iter().chain(cold) {
        if !seen.insert(event.meta.id.clone()) {
            continue;
        }
        events.push(event);
    }
    events.sort_by(|left, right| {
        right
            .meta
            .ts
            .cmp(&left.meta.ts)
            .then_with(|| left.meta.id.0.cmp(&right.meta.id.0))
    });
    if limit > 0 {
        events.truncate(limit);
    }
    events
}
