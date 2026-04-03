use std::collections::{BTreeMap, HashSet};

use prism_ir::{new_prefixed_id, AnchorRef, EventActor, EventId, EventMeta, ObservedChangeSet};
use prism_memory::{OutcomeEvent, OutcomeKind, OutcomeResult};
use prism_projections::ValidationDelta;
use tracing::warn;

use crate::path_identity::repo_relative_string;
use crate::published_knowledge::validate_repo_patch_event;
use crate::repo_patch_events::append_repo_patch_event;
use crate::util::current_timestamp;
use crate::WorkspaceIndexer;

const MAX_PATCH_CHANGED_SYMBOLS: usize = 256;

pub(crate) fn default_outcome_meta(prefix: &str) -> EventMeta {
    EventMeta {
        id: EventId::new(new_prefixed_id(prefix)),
        ts: current_timestamp(),
        actor: EventActor::System,
        correlation: None,
        causation: None,
        execution_context: None,
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

fn patch_reason(observed: &ObservedChangeSet) -> String {
    if let Some(work) = observed
        .meta
        .execution_context
        .as_ref()
        .and_then(|context| context.work_context.as_ref())
    {
        return format!("work {} ({})", work.title, work.work_id);
    }
    if let Some(task_id) = observed.meta.correlation.as_ref() {
        return format!("task {}", task_id.0);
    }
    format!("trigger {:?}", observed.trigger)
}

fn patch_is_repo_publishable(event: &OutcomeEvent) -> bool {
    event.kind == OutcomeKind::PatchApplied
        && event.result == OutcomeResult::Success
        && !matches!(event.meta.actor, EventActor::System)
        && event
            .meta
            .execution_context
            .as_ref()
            .and_then(|context| context.work_context.as_ref())
            .is_some()
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
                .map(|path| repo_relative_string(&indexer.root, path))
        })
        .collect()
}

#[derive(Debug, Clone)]
struct PatchChangedFileSummary {
    file_path: String,
    changed_symbol_count: usize,
    added_count: usize,
    removed_count: usize,
    updated_count: usize,
}

#[derive(Default)]
struct PatchMetadataBuilder {
    changed_symbols: Vec<serde_json::Value>,
    file_summaries: BTreeMap<String, PatchChangedFileSummary>,
    total_changed_symbol_count: usize,
}

pub(crate) struct RecordedPatchOutcome {
    pub(crate) event: OutcomeEvent,
    pub(crate) validation_deltas: Vec<ValidationDelta>,
}

impl PatchMetadataBuilder {
    fn push<S: prism_store::Store>(
        &mut self,
        indexer: &WorkspaceIndexer<S>,
        status: &str,
        node: &prism_ir::ObservedNode,
    ) {
        self.total_changed_symbol_count += 1;
        if let Some(file_path) = indexer
            .graph
            .file_path(node.node.file)
            .map(|path| repo_relative_string(&indexer.root, path))
        {
            let summary = self
                .file_summaries
                .entry(file_path.clone())
                .or_insert_with(|| PatchChangedFileSummary {
                    file_path,
                    changed_symbol_count: 0,
                    added_count: 0,
                    removed_count: 0,
                    updated_count: 0,
                });
            summary.changed_symbol_count += 1;
            if status == "added" {
                summary.added_count += 1;
            } else if status == "removed" {
                summary.removed_count += 1;
            } else {
                summary.updated_count += 1;
            }
        }

        if self.changed_symbols.len() < MAX_PATCH_CHANGED_SYMBOLS {
            self.changed_symbols
                .push(changed_symbol_metadata(indexer, status, node));
        }
    }
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
            .map(|path| repo_relative_string(&indexer.root, path)),
        "span": node.node.span,
    })
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
    ) -> Option<RecordedPatchOutcome> {
        if !self.had_prior_snapshot || observed_is_empty(observed) {
            return None;
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

        let mut patch_metadata = PatchMetadataBuilder::default();
        for node in &observed.added {
            patch_metadata.push(self, "added", node);
        }
        for node in &observed.removed {
            patch_metadata.push(self, "removed", node);
        }
        for (before, after) in &observed.updated {
            patch_metadata.push(self, "updated_before", before);
            patch_metadata.push(self, "updated_after", after);
        }

        let event = OutcomeEvent {
            meta: EventMeta {
                id: auto_outcome_event_id("outcome"),
                ts: observed.meta.ts,
                actor: observed.meta.actor.clone(),
                correlation: observed.meta.correlation.clone(),
                causation: Some(observed.meta.id.clone()),
                execution_context: observed.meta.execution_context.clone(),
            },
            anchors: dedupe_anchors(anchors),
            kind: OutcomeKind::PatchApplied,
            result: OutcomeResult::Success,
            summary: patch_summary(observed),
            evidence: Vec::new(),
            metadata: serde_json::json!({
                "trigger": format!("{:?}", observed.trigger),
                "reason": patch_reason(observed),
                "files": observed.files.iter().map(|file_id| file_id.0).collect::<Vec<_>>(),
                "filePaths": patch_file_paths(self, observed),
                "changedFilesSummary": patch_metadata
                    .file_summaries
                    .values()
                    .map(|summary| serde_json::json!({
                        "filePath": summary.file_path,
                        "changedSymbolCount": summary.changed_symbol_count,
                        "addedCount": summary.added_count,
                        "removedCount": summary.removed_count,
                        "updatedCount": summary.updated_count,
                    }))
                    .collect::<Vec<_>>(),
                "changedSymbols": patch_metadata.changed_symbols,
                "changedSymbolsTotalCount": patch_metadata.total_changed_symbol_count,
                "changedSymbolsTruncated":
                    patch_metadata.total_changed_symbol_count > MAX_PATCH_CHANGED_SYMBOLS,
            }),
        };
        let validation_deltas = prism_projections::validation_deltas_for_event(&event, |node| {
            self.history.lineage_of(node)
        });
        self.projections
            .apply_outcome_event(&event, |node| self.history.lineage_of(node));
        let _ = self.outcomes.store_event(event.clone());
        if patch_is_repo_publishable(&event) {
            if let Err(error) = validate_repo_patch_event(&event)
                .and_then(|_| append_repo_patch_event(&self.root, &event))
            {
                warn!(
                    root = %self.root.display(),
                    event_id = %event.meta.id.0,
                    error = %error,
                    "failed to append repo patch provenance event"
                );
            }
        }
        Some(RecordedPatchOutcome {
            event,
            validation_deltas,
        })
    }
}
