use std::collections::HashSet;

use prism_ir::{new_prefixed_id, AnchorRef, EventActor, EventId, EventMeta, ObservedChangeSet};
use prism_memory::{OutcomeEvent, OutcomeKind, OutcomeResult};
use prism_projections::ValidationDelta;

use crate::util::current_timestamp;
use crate::WorkspaceIndexer;

pub(crate) fn default_outcome_meta(prefix: &str) -> EventMeta {
    EventMeta {
        id: EventId::new(new_prefixed_id(prefix)),
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
    EventId::new(new_prefixed_id(prefix))
}

fn patch_summary(observed: &ObservedChangeSet) -> String {
    format!(
        "observed file change: {} added, {} removed, {} updated symbols",
        observed.added.len(),
        observed.removed.len(),
        observed.updated.len(),
    )
}

fn patch_file_paths<S: prism_store::Store>(
    indexer: &WorkspaceIndexer<S>,
    observed: &ObservedChangeSet,
) -> Vec<String> {
    observed
        .files
        .iter()
        .filter_map(|file_id| {
            indexer
                .graph
                .file_path(*file_id)
                .map(|path| path.to_string_lossy().into_owned())
        })
        .collect()
}

fn changed_symbol_metadata<S: prism_store::Store>(
    indexer: &WorkspaceIndexer<S>,
    status: &str,
    node: &prism_ir::ObservedNode,
) -> serde_json::Value {
    serde_json::json!({
        "status": status,
        "id": node.node.id,
        "name": node.node.name,
        "kind": node.node.kind,
        "filePath": indexer
            .graph
            .file_path(node.node.file)
            .map(|path| path.to_string_lossy().into_owned()),
        "span": node.node.span,
    })
}

fn updated_symbol_metadata<S: prism_store::Store>(
    indexer: &WorkspaceIndexer<S>,
    before: &prism_ir::ObservedNode,
    after: &prism_ir::ObservedNode,
) -> [serde_json::Value; 2] {
    [
        changed_symbol_metadata(indexer, "updated_before", before),
        changed_symbol_metadata(indexer, "updated_after", after),
    ]
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
                "filePaths": patch_file_paths(self, observed),
                "changedSymbols": observed
                    .added
                    .iter()
                    .map(|node| changed_symbol_metadata(self, "added", node))
                    .chain(
                        observed
                            .removed
                            .iter()
                            .map(|node| changed_symbol_metadata(self, "removed", node))
                    )
                    .chain(
                        observed
                            .updated
                            .iter()
                            .flat_map(|(before, after)| updated_symbol_metadata(self, before, after))
                    )
                    .collect::<Vec<_>>(),
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
