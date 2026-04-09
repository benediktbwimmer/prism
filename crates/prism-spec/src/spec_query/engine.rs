use anyhow::Result;

use crate::{
    MaterializedSpecRecord, SpecMaterializedStore, StoredSpecChecklistItemRecord,
    StoredSpecCoverageRecord, StoredSpecDependencyRecord, StoredSpecStatusRecord,
    StoredSpecSyncProvenanceRecord,
};

use super::types::{
    SpecChecklistView, SpecCoverageView, SpecDependencyView, SpecDocumentView, SpecListEntry,
    SpecMetadataView, SpecQueryLookup, SpecSyncProvenanceView,
};

pub trait SpecQueryEngine {
    fn metadata(&self) -> Result<SpecMetadataView>;
    fn list_specs(&self) -> Result<Vec<SpecListEntry>>;
    fn spec(&self, spec_id: &str) -> Result<SpecQueryLookup<SpecDocumentView>>;
    fn checklist_items(&self, spec_id: &str) -> Result<SpecQueryLookup<SpecChecklistView>>;
    fn dependencies(&self, spec_id: &str) -> Result<SpecQueryLookup<SpecDependencyView>>;
    fn status(&self, spec_id: &str) -> Result<SpecQueryLookup<StoredSpecStatusRecord>>;
    fn coverage(&self, spec_id: &str) -> Result<SpecQueryLookup<SpecCoverageView>>;
    fn sync_provenance(&self, spec_id: &str) -> Result<SpecQueryLookup<SpecSyncProvenanceView>>;
}

pub struct MaterializedSpecQueryEngine<'a, S> {
    store: &'a S,
}

impl<'a, S> MaterializedSpecQueryEngine<'a, S> {
    pub const fn new(store: &'a S) -> Self {
        Self { store }
    }
}

impl<S> SpecQueryEngine for MaterializedSpecQueryEngine<'_, S>
where
    S: SpecMaterializedStore,
{
    fn metadata(&self) -> Result<SpecMetadataView> {
        Ok(SpecMetadataView {
            materialization: self.store.read_metadata()?,
        })
    }

    fn list_specs(&self) -> Result<Vec<SpecListEntry>> {
        let specs = self.store.read_specs()?.value;
        let statuses = self.store.read_status_records()?.value;
        let status_by_id = statuses
            .into_iter()
            .map(|status| (status.spec_id.clone(), status))
            .collect::<std::collections::BTreeMap<_, _>>();
        Ok(specs
            .into_iter()
            .map(|record| SpecListEntry {
                spec_id: record.spec_id.clone(),
                title: record.title,
                source_path: record.source_path,
                declared_status: record.declared_status,
                overall_status: status_by_id
                    .get(&record.spec_id)
                    .map(|status| status.overall_status.clone()),
                created: record.created,
            })
            .collect())
    }

    fn spec(&self, spec_id: &str) -> Result<SpecQueryLookup<SpecDocumentView>> {
        let specs = self.store.read_specs()?.value;
        let statuses = self.store.read_status_records()?.value;
        let Some(record) = specs.into_iter().find(|record| record.spec_id == spec_id) else {
            return Ok(SpecQueryLookup::NotFound);
        };
        let status = statuses.into_iter().find(|status| status.spec_id == spec_id);
        Ok(SpecQueryLookup::Found(SpecDocumentView { record, status }))
    }

    fn checklist_items(&self, spec_id: &str) -> Result<SpecQueryLookup<SpecChecklistView>> {
        lookup_spec_scoped_records(
            self.store.read_specs()?.value,
            self.store.read_checklist_items()?.value,
            spec_id,
            |record: &StoredSpecChecklistItemRecord| record.spec_id.as_str(),
            |records| SpecChecklistView {
                spec_id: spec_id.to_owned(),
                items: records,
            },
        )
    }

    fn dependencies(&self, spec_id: &str) -> Result<SpecQueryLookup<SpecDependencyView>> {
        lookup_spec_scoped_records(
            self.store.read_specs()?.value,
            self.store.read_dependencies()?.value,
            spec_id,
            |record: &StoredSpecDependencyRecord| record.spec_id.as_str(),
            |records| SpecDependencyView {
                spec_id: spec_id.to_owned(),
                dependencies: records,
            },
        )
    }

    fn status(&self, spec_id: &str) -> Result<SpecQueryLookup<StoredSpecStatusRecord>> {
        let specs = self.store.read_specs()?.value;
        if !specs.iter().any(|record| record.spec_id == spec_id) {
            return Ok(SpecQueryLookup::NotFound);
        }
        let status = self
            .store
            .read_status_records()?
            .value
            .into_iter()
            .find(|status| status.spec_id == spec_id);
        Ok(match status {
            Some(status) => SpecQueryLookup::Found(status),
            None => SpecQueryLookup::NotFound,
        })
    }

    fn coverage(&self, spec_id: &str) -> Result<SpecQueryLookup<SpecCoverageView>> {
        lookup_spec_scoped_records(
            self.store.read_specs()?.value,
            self.store.read_coverage_records()?.value,
            spec_id,
            |record: &StoredSpecCoverageRecord| record.spec_id.as_str(),
            |records| SpecCoverageView {
                spec_id: spec_id.to_owned(),
                records,
            },
        )
    }

    fn sync_provenance(&self, spec_id: &str) -> Result<SpecQueryLookup<SpecSyncProvenanceView>> {
        lookup_spec_scoped_records(
            self.store.read_specs()?.value,
            self.store.read_sync_provenance_records()?.value,
            spec_id,
            |record: &StoredSpecSyncProvenanceRecord| record.spec_id.as_str(),
            |records| SpecSyncProvenanceView {
                spec_id: spec_id.to_owned(),
                records,
            },
        )
    }
}

fn lookup_spec_scoped_records<T, F, B>(
    specs: Vec<MaterializedSpecRecord>,
    records: Vec<T>,
    spec_id: &str,
    record_spec_id: F,
    build: B,
) -> Result<SpecQueryLookup<B::Output>>
where
    F: Fn(&T) -> &str,
    B: LookupBuilder<T>,
{
    if !specs.iter().any(|record| record.spec_id == spec_id) {
        return Ok(SpecQueryLookup::NotFound);
    }
    let filtered = records
        .into_iter()
        .filter(|record| record_spec_id(record) == spec_id)
        .collect();
    Ok(SpecQueryLookup::Found(build.build(filtered)))
}

trait LookupBuilder<T> {
    type Output;

    fn build(self, records: Vec<T>) -> Self::Output;
}

impl<T, F, O> LookupBuilder<T> for F
where
    F: FnOnce(Vec<T>) -> O,
{
    type Output = O;

    fn build(self, records: Vec<T>) -> Self::Output {
        self(records)
    }
}

#[cfg(test)]
mod tests {
    use std::fs;
    use std::path::PathBuf;
    use std::sync::atomic::{AtomicU64, Ordering};
    use std::time::{SystemTime, UNIX_EPOCH};

    use crate::{
        discover_spec_sources, parse_spec_sources, SpecMaterializedReplaceRequest,
        SpecMaterializedStore, SqliteSpecMaterializedStore,
    };

    use super::{MaterializedSpecQueryEngine, SpecQueryEngine, SpecQueryLookup};

    static NEXT_TEMP_REPO: AtomicU64 = AtomicU64::new(0);

    fn temp_repo(label: &str) -> PathBuf {
        let nonce = NEXT_TEMP_REPO.fetch_add(1, Ordering::Relaxed);
        let unique = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        let root = std::env::temp_dir().join(format!("prism-spec-query-{label}-{unique}-{nonce}"));
        let _ = fs::remove_dir_all(&root);
        fs::create_dir_all(root.join(".git")).unwrap();
        fs::write(root.join("Cargo.toml"), "[workspace]\nmembers = []\n").unwrap();
        fs::write(root.join(".git/HEAD"), "ref: refs/heads/main\n").unwrap();
        root
    }

    fn write_spec(root: &PathBuf, relative_path: &str, contents: &str) {
        let path = root.join(relative_path);
        fs::create_dir_all(path.parent().unwrap()).unwrap();
        fs::write(path, contents).unwrap();
    }

    fn materialized_query_engine(root: &PathBuf) -> MaterializedSpecQueryEngine<'static, SqliteSpecMaterializedStore> {
        let discovered = discover_spec_sources(root).unwrap();
        let parsed = parse_spec_sources(&discovered);
        assert!(parsed.diagnostics.is_empty());
        let store = Box::leak(Box::new(SqliteSpecMaterializedStore::new(
            &root.join(".tmp/spec-materialized.db"),
        )));
        store
            .replace_materialization(SpecMaterializedReplaceRequest { parsed: parsed.parsed })
            .unwrap();
        MaterializedSpecQueryEngine::new(store)
    }

    #[test]
    fn list_specs_includes_derived_status_and_stable_order() {
        let root = temp_repo("list");
        write_spec(
            &root,
            ".prism/specs/2026-04-09-b.md",
            "---\nid: spec:b\ntitle: Beta\nstatus: completed\ncreated: 2026-04-09\n---\n\n- [x] done\n",
        );
        write_spec(
            &root,
            ".prism/specs/2026-04-09-a.md",
            "---\nid: spec:a\ntitle: Alpha\nstatus: in_progress\ncreated: 2026-04-09\n---\n\n- [ ] first <!-- id: first -->\n",
        );
        let engine = materialized_query_engine(&root);

        let specs = engine.list_specs().unwrap();
        assert_eq!(specs.len(), 2);
        assert_eq!(specs[0].spec_id, "spec:a");
        assert_eq!(specs[0].overall_status.as_deref(), Some("in_progress"));
        assert_eq!(specs[1].spec_id, "spec:b");
        assert_eq!(specs[1].overall_status.as_deref(), Some("completed"));
    }

    #[test]
    fn checklist_dependency_and_empty_coverage_queries_are_spec_scoped() {
        let root = temp_repo("scoped");
        write_spec(
            &root,
            ".prism/specs/2026-04-09-a.md",
            "---\nid: spec:a\ntitle: Alpha\nstatus: draft\ncreated: 2026-04-09\ndepends_on:\n  - spec:b\n---\n\n- [ ] first <!-- id: first -->\n",
        );
        write_spec(
            &root,
            ".prism/specs/2026-04-09-b.md",
            "---\nid: spec:b\ntitle: Beta\nstatus: completed\ncreated: 2026-04-09\n---\n\n- [x] done\n",
        );
        let engine = materialized_query_engine(&root);

        match engine.checklist_items("spec:a").unwrap() {
            SpecQueryLookup::Found(view) => {
                assert_eq!(view.items.len(), 1);
                assert_eq!(view.items[0].spec_id, "spec:a");
                assert_eq!(view.items[0].item.item_id, "spec:a::checklist::first");
            }
            SpecQueryLookup::NotFound => panic!("expected checklist items"),
        }

        match engine.dependencies("spec:a").unwrap() {
            SpecQueryLookup::Found(view) => {
                assert_eq!(view.dependencies.len(), 1);
                assert_eq!(view.dependencies[0].dependency_spec_id, "spec:b");
            }
            SpecQueryLookup::NotFound => panic!("expected dependencies"),
        }

        match engine.coverage("spec:a").unwrap() {
            SpecQueryLookup::Found(view) => assert!(view.records.is_empty()),
            SpecQueryLookup::NotFound => panic!("expected empty coverage view"),
        }
    }

    #[test]
    fn missing_spec_queries_return_not_found_explicitly() {
        let root = temp_repo("missing");
        write_spec(
            &root,
            ".prism/specs/2026-04-09-a.md",
            "---\nid: spec:a\ntitle: Alpha\nstatus: draft\ncreated: 2026-04-09\n---\n\n- [ ] first\n",
        );
        let engine = materialized_query_engine(&root);

        assert!(matches!(engine.spec("spec:missing").unwrap(), SpecQueryLookup::NotFound));
        assert!(matches!(
            engine.checklist_items("spec:missing").unwrap(),
            SpecQueryLookup::NotFound
        ));
        assert!(matches!(
            engine.sync_provenance("spec:missing").unwrap(),
            SpecQueryLookup::NotFound
        ));
    }
}
