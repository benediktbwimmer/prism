use std::path::Path;

use anyhow::{anyhow, Result};
use prism_core::{
    refresh_spec_materialization, MaterializedSpecQueryEngine, SpecQueryEngine, SpecQueryLookup,
    SqliteSpecMaterializedStore,
};
use prism_js::{
    SpecChecklistItemView, SpecCoverageRecordView, SpecDocumentView, SpecListEntryView,
    SpecSyncBriefView, SpecSyncProvenanceRecordView,
};

use crate::QueryHost;

fn spec_materialized_db_path(root: &Path) -> std::path::PathBuf {
    root.join(".prism")
        .join("state")
        .join("spec-materialized.db")
}

fn with_spec_query_engine<T, F>(host: &QueryHost, f: F) -> Result<T>
where
    F: FnOnce(&dyn SpecQueryEngine) -> Result<T>,
{
    let root = host
        .workspace_root()
        .ok_or_else(|| anyhow!("native spec reads require a workspace-backed host"))?;
    let store = SqliteSpecMaterializedStore::new(&spec_materialized_db_path(root));
    refresh_spec_materialization(&store, root, Some(host.current_prism().coordination_snapshot()))?;
    let engine = MaterializedSpecQueryEngine::new(&store);
    f(&engine)
}

pub(crate) fn list_specs(host: &QueryHost) -> Result<Vec<SpecListEntryView>> {
    with_spec_query_engine(host, |engine| {
        Ok(engine
            .list_specs()?
            .into_iter()
            .map(|entry| SpecListEntryView {
                spec_id: entry.spec_id,
                title: entry.title,
                source_path: entry.source_path,
                declared_status: entry.declared_status,
                overall_status: entry.overall_status,
                created: entry.created,
            })
            .collect())
    })
}

pub(crate) fn spec_document(host: &QueryHost, spec_id: &str) -> Result<Option<SpecDocumentView>> {
    with_spec_query_engine(host, |engine| {
        Ok(match engine.spec(spec_id)? {
            SpecQueryLookup::Found(view) => Some(SpecDocumentView {
                spec_id: view.record.spec_id,
                source_path: view.record.source_path,
                title: view.record.title,
                declared_status: view.record.declared_status,
                overall_status: view.status.map(|status| status.overall_status),
                created: view.record.created,
                content_digest: view.record.content_digest,
                git_revision: view.record.git_revision,
                body: view.record.body,
            }),
            SpecQueryLookup::NotFound => None,
        })
    })
}

pub(crate) fn spec_sync_brief(
    host: &QueryHost,
    spec_id: &str,
) -> Result<Option<SpecSyncBriefView>> {
    with_spec_query_engine(host, |engine| {
        Ok(match engine.sync_brief(spec_id)? {
            SpecQueryLookup::Found(view) => Some(SpecSyncBriefView {
                spec: SpecDocumentView {
                    spec_id: view.spec.record.spec_id,
                    source_path: view.spec.record.source_path,
                    title: view.spec.record.title,
                    declared_status: view.spec.record.declared_status,
                    overall_status: view.spec.status.map(|status| status.overall_status),
                    created: view.spec.record.created,
                    content_digest: view.spec.record.content_digest,
                    git_revision: view.spec.record.git_revision,
                    body: view.spec.record.body,
                },
                required_checklist_items: view
                    .required_checklist_items
                    .into_iter()
                    .map(|item| SpecChecklistItemView {
                        item_id: item.item.item_id,
                        label: item.item.label,
                        checked: item.item.checked,
                        requirement_level: format!("{:?}", item.item.requirement_level)
                            .to_lowercase(),
                        section_path: item.item.section_path,
                        line_number: item.item.line_number,
                    })
                    .collect(),
                coverage: view
                    .coverage
                    .into_iter()
                    .map(|record| SpecCoverageRecordView {
                        checklist_item_id: record.checklist_item_id,
                        coverage_kind: record.coverage_kind,
                        coordination_ref: record.coordination_ref,
                    })
                    .collect(),
                linked_coordination_refs: view
                    .linked_coordination_refs
                    .into_iter()
                    .map(|record| SpecSyncProvenanceRecordView {
                        target_coordination_ref: record.target_coordination_ref,
                        sync_kind: record.sync_kind,
                        source_revision: record.source_revision,
                        covered_checklist_items: record.covered_checklist_items,
                    })
                    .collect(),
            }),
            SpecQueryLookup::NotFound => None,
        })
    })
}

pub(crate) fn spec_coverage(
    host: &QueryHost,
    spec_id: &str,
) -> Result<Vec<SpecCoverageRecordView>> {
    with_spec_query_engine(host, |engine| {
        Ok(match engine.coverage(spec_id)? {
            SpecQueryLookup::Found(view) => view
                .records
                .into_iter()
                .map(|record| SpecCoverageRecordView {
                    checklist_item_id: record.checklist_item_id,
                    coverage_kind: record.coverage_kind,
                    coordination_ref: record.coordination_ref,
                })
                .collect(),
            SpecQueryLookup::NotFound => Vec::new(),
        })
    })
}

pub(crate) fn spec_sync_provenance(
    host: &QueryHost,
    spec_id: &str,
) -> Result<Vec<SpecSyncProvenanceRecordView>> {
    with_spec_query_engine(host, |engine| {
        Ok(match engine.sync_provenance(spec_id)? {
            SpecQueryLookup::Found(view) => view
                .records
                .into_iter()
                .map(|record| SpecSyncProvenanceRecordView {
                    target_coordination_ref: record.target_coordination_ref,
                    sync_kind: record.sync_kind,
                    source_revision: record.source_revision,
                    covered_checklist_items: record.covered_checklist_items,
                })
                .collect(),
            SpecQueryLookup::NotFound => Vec::new(),
        })
    })
}
