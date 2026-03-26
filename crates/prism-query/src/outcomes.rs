use prism_ir::{AnchorRef, NodeId, TaskId};
use prism_memory::{OutcomeEvent, OutcomeKind, OutcomeRecallQuery, OutcomeResult, TaskReplay};

use crate::Prism;

impl Prism {
    pub fn query_outcomes(&self, query: &OutcomeRecallQuery) -> Vec<OutcomeEvent> {
        let mut expanded = query.clone();
        expanded.anchors = self.expand_anchors(&query.anchors);
        self.outcomes.query_events(&expanded)
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
        self.outcomes.resume_task(task)
    }
}
