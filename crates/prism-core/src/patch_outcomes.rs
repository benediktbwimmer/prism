use std::collections::HashSet;
use std::sync::atomic::{AtomicU64, Ordering};

use prism_ir::{AnchorRef, EventActor, EventId, EventMeta, ObservedChangeSet};
use prism_memory::{OutcomeEvent, OutcomeKind, OutcomeResult};
use prism_projections::ValidationDelta;

use crate::util::current_timestamp;
use crate::WorkspaceIndexer;

static NEXT_AUTO_OUTCOME_ID: AtomicU64 = AtomicU64::new(1);

pub(crate) fn default_outcome_meta(prefix: &str) -> EventMeta {
    let sequence = NEXT_AUTO_OUTCOME_ID.fetch_add(1, Ordering::Relaxed);
    EventMeta {
        id: EventId::new(format!("{prefix}:{sequence}")),
        ts: current_timestamp(),
        actor: EventActor::System,
        correlation: None,
        causation: None,
    }
}

pub(crate) fn observed_is_empty(observed: &ObservedChangeSet) -> bool {
    observed.added.is_empty()
        && observed.removed.is_empty()
        && observed.updated.is_empty()
        && observed.edge_added.is_empty()
        && observed.edge_removed.is_empty()
}

fn auto_outcome_event_id(prefix: &str) -> EventId {
    let sequence = NEXT_AUTO_OUTCOME_ID.fetch_add(1, Ordering::Relaxed);
    EventId::new(format!("{prefix}:{sequence}"))
}

fn patch_summary(observed: &ObservedChangeSet) -> String {
    format!(
        "observed file change: {} added, {} removed, {} updated symbols",
        observed.added.len(),
        observed.removed.len(),
        observed.updated.len(),
    )
}

pub(crate) fn dedupe_anchors(anchors: Vec<AnchorRef>) -> Vec<AnchorRef> {
    let mut seen = HashSet::new();
    let mut deduped = Vec::new();
    for anchor in anchors {
        if seen.insert(anchor.clone()) {
            deduped.push(anchor);
        }
    }
    deduped
}

impl<S: prism_store::Store> WorkspaceIndexer<S> {
    pub(crate) fn record_patch_outcome(
        &mut self,
        observed: &ObservedChangeSet,
    ) -> Vec<ValidationDelta> {
        if !self.had_prior_snapshot || observed_is_empty(observed) {
            return Vec::new();
        }

        let mut anchors = observed
            .files
            .iter()
            .copied()
            .filter(|file_id| file_id.0 != 0)
            .map(AnchorRef::File)
            .collect::<Vec<_>>();
        anchors.extend(
            observed
                .added
                .iter()
                .map(|node| AnchorRef::Node(node.node.id.clone())),
        );
        anchors.extend(
            observed
                .removed
                .iter()
                .map(|node| AnchorRef::Node(node.node.id.clone())),
        );
        anchors.extend(observed.updated.iter().flat_map(|(before, after)| {
            [
                AnchorRef::Node(before.node.id.clone()),
                AnchorRef::Node(after.node.id.clone()),
            ]
        }));

        let event = OutcomeEvent {
            meta: EventMeta {
                id: auto_outcome_event_id("outcome"),
                ts: observed.meta.ts,
                actor: EventActor::System,
                correlation: observed.meta.correlation.clone(),
                causation: Some(observed.meta.id.clone()),
            },
            anchors: dedupe_anchors(anchors),
            kind: OutcomeKind::PatchApplied,
            result: OutcomeResult::Success,
            summary: patch_summary(observed),
            evidence: Vec::new(),
            metadata: serde_json::json!({
                "trigger": format!("{:?}", observed.trigger),
                "files": observed.files.iter().map(|file_id| file_id.0).collect::<Vec<_>>(),
            }),
        };
        let deltas = prism_projections::validation_deltas_for_event(&event, |node| {
            self.history.lineage_of(node)
        });
        self.projections
            .apply_outcome_event(&event, |node| self.history.lineage_of(node));
        let _ = self.outcomes.store_event(event);
        deltas
    }
}
