use std::path::Path;

use anyhow::{anyhow, Result};
use prism_coordination::{CoordinationSpecRef, CoordinationTaskSpecRef};
use prism_core::{
    refresh_spec_materialization, MaterializedSpecQueryEngine, SpecQueryEngine, SpecQueryLookup,
    SqliteSpecMaterializedStore,
};
use prism_js::{
    LinkedSpecSummaryView, SpecChecklistItemView, SpecCoverageRecordView, SpecDocumentView,
    SpecListEntryView, SpecSyncBriefView, SpecSyncProvenanceRecordView,
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
    refresh_spec_materialization(
        &store,
        root,
        Some(host.current_prism().coordination_snapshot()),
    )?;
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

pub(crate) fn linked_plan_spec_summaries(
    host: &QueryHost,
    spec_refs: &[CoordinationSpecRef],
) -> Result<Vec<LinkedSpecSummaryView>> {
    with_spec_query_engine(host, |engine| {
        spec_refs
            .iter()
            .map(|spec_ref| {
                build_linked_spec_summary(
                    engine,
                    &spec_ref.spec_id,
                    &spec_ref.source_path,
                    spec_ref.source_revision.as_deref(),
                    None,
                    &[],
                )
            })
            .collect()
    })
}

pub(crate) fn linked_task_spec_summaries(
    host: &QueryHost,
    spec_refs: &[CoordinationTaskSpecRef],
) -> Result<Vec<LinkedSpecSummaryView>> {
    with_spec_query_engine(host, |engine| {
        spec_refs
            .iter()
            .map(|spec_ref| {
                build_linked_spec_summary(
                    engine,
                    &spec_ref.spec_id,
                    &spec_ref.source_path,
                    spec_ref.source_revision.as_deref(),
                    Some(spec_ref.sync_kind.as_str()),
                    &spec_ref.covered_checklist_items,
                )
            })
            .collect()
    })
}

fn build_linked_spec_summary(
    engine: &dyn SpecQueryEngine,
    spec_id: &str,
    source_path: &str,
    linked_source_revision: Option<&str>,
    sync_kind: Option<&str>,
    covered_checklist_items: &[String],
) -> Result<LinkedSpecSummaryView> {
    Ok(match engine.spec(spec_id)? {
        SpecQueryLookup::Found(view) => {
            let current_source_revision = view.record.git_revision.clone();
            let drift_status = match (linked_source_revision, current_source_revision.as_deref()) {
                (Some(linked), Some(current)) if linked != current => "stale_link",
                _ => "in_sync",
            };
            LinkedSpecSummaryView {
                spec_id: view.record.spec_id,
                source_path: source_path.to_string(),
                linked_source_revision: linked_source_revision.map(str::to_owned),
                current_source_revision,
                drift_status: drift_status.to_string(),
                title: Some(view.record.title),
                declared_status: Some(view.record.declared_status),
                overall_status: view.status.map(|status| status.overall_status),
                sync_kind: sync_kind.map(str::to_owned),
                covered_checklist_items: covered_checklist_items.to_vec(),
            }
        }
        SpecQueryLookup::NotFound => LinkedSpecSummaryView {
            spec_id: spec_id.to_string(),
            source_path: source_path.to_string(),
            linked_source_revision: linked_source_revision.map(str::to_owned),
            current_source_revision: None,
            drift_status: "missing_local_spec".to_string(),
            title: None,
            declared_status: None,
            overall_status: None,
            sync_kind: sync_kind.map(str::to_owned),
            covered_checklist_items: covered_checklist_items.to_vec(),
        },
    })
}
