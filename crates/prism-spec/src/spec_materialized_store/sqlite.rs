use std::path::{Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};

use anyhow::Result;
use rusqlite::{params, Connection};

use super::traits::SpecMaterializedStore;
use super::types::{
    MaterializedSpecRecord, SpecMaterializationMetadata, SpecMaterializedBackendKind,
    SpecMaterializedCapabilities, SpecMaterializedClearRequest, SpecMaterializedReadEnvelope,
    SpecMaterializedReplaceRequest, SpecMaterializedWriteResult, StoredSpecChecklistItemRecord,
    StoredSpecChecklistPosture, StoredSpecCoverageRecord, StoredSpecDependencyPosture,
    StoredSpecDependencyRecord, StoredSpecStatusRecord, StoredSpecSyncProvenanceRecord,
};
use crate::{
    ParsedSpecDocument, SpecChecklistIdentitySource, SpecChecklistItem,
    SpecChecklistRequirementLevel, SpecDeclaredStatus,
};

pub struct SqliteSpecMaterializedStore {
    db_path: PathBuf,
}

impl SqliteSpecMaterializedStore {
    pub fn new(db_path: &Path) -> Self {
        Self {
            db_path: db_path.to_path_buf(),
        }
    }

    fn open_connection(&self) -> Result<Connection> {
        if let Some(parent) = self.db_path.parent() {
            std::fs::create_dir_all(parent)?;
        }
        let conn = Connection::open(&self.db_path)?;
        self.ensure_schema(&conn)?;
        Ok(conn)
    }

    fn ensure_schema(&self, conn: &Connection) -> Result<()> {
        conn.execute_batch(
            "
            CREATE TABLE IF NOT EXISTS spec_materialized_specs (
                spec_id TEXT PRIMARY KEY,
                source_path TEXT NOT NULL,
                title TEXT NOT NULL,
                declared_status TEXT NOT NULL,
                created TEXT NOT NULL,
                content_digest TEXT NOT NULL,
                git_revision TEXT,
                body TEXT NOT NULL
            );

            CREATE TABLE IF NOT EXISTS spec_materialized_checklist_items (
                item_id TEXT PRIMARY KEY,
                spec_id TEXT NOT NULL,
                identity_source TEXT NOT NULL,
                explicit_id TEXT,
                label TEXT NOT NULL,
                checked INTEGER NOT NULL,
                requirement_level TEXT NOT NULL,
                section_path_json TEXT NOT NULL,
                line_number INTEGER NOT NULL
            );

            CREATE TABLE IF NOT EXISTS spec_materialized_dependencies (
                spec_id TEXT NOT NULL,
                position INTEGER NOT NULL,
                dependency_spec_id TEXT NOT NULL,
                PRIMARY KEY (spec_id, position)
            );

            CREATE TABLE IF NOT EXISTS spec_materialized_status (
                spec_id TEXT PRIMARY KEY,
                declared_status TEXT NOT NULL,
                checklist_posture TEXT NOT NULL,
                dependency_posture TEXT NOT NULL,
                overall_status TEXT NOT NULL
            );

            CREATE TABLE IF NOT EXISTS spec_materialized_coverage (
                spec_id TEXT NOT NULL,
                checklist_item_id TEXT NOT NULL,
                coverage_kind TEXT NOT NULL,
                coordination_ref TEXT,
                PRIMARY KEY (spec_id, checklist_item_id, coverage_kind, coordination_ref)
            );

            CREATE TABLE IF NOT EXISTS spec_materialized_sync_provenance (
                spec_id TEXT NOT NULL,
                target_coordination_ref TEXT NOT NULL,
                sync_kind TEXT NOT NULL,
                source_revision TEXT,
                covered_checklist_items_json TEXT NOT NULL,
                PRIMARY KEY (spec_id, target_coordination_ref, sync_kind)
            );

            CREATE TABLE IF NOT EXISTS spec_materialized_metadata (
                id INTEGER PRIMARY KEY CHECK (id = 1),
                materialized_at INTEGER,
                spec_count INTEGER NOT NULL,
                checklist_item_count INTEGER NOT NULL,
                dependency_count INTEGER NOT NULL
            );
            ",
        )?;
        conn.execute(
            "INSERT OR IGNORE INTO spec_materialized_metadata (
                id,
                materialized_at,
                spec_count,
                checklist_item_count,
                dependency_count
            ) VALUES (1, NULL, 0, 0, 0)",
            [],
        )?;
        Ok(())
    }

    fn load_metadata_from_connection(&self, conn: &Connection) -> Result<SpecMaterializationMetadata> {
        let coverage_record_count = conn.query_row(
            "SELECT COUNT(*) FROM spec_materialized_coverage",
            [],
            |row| row.get::<_, i64>(0),
        )? as usize;
        let sync_provenance_record_count = conn.query_row(
            "SELECT COUNT(*) FROM spec_materialized_sync_provenance",
            [],
            |row| row.get::<_, i64>(0),
        )? as usize;
        let row = conn.query_row(
            "SELECT materialized_at, spec_count, checklist_item_count, dependency_count
             FROM spec_materialized_metadata
             WHERE id = 1",
            [],
            |row| {
                Ok(SpecMaterializationMetadata {
                    backend_kind: SpecMaterializedBackendKind::Sqlite,
                    materialized_at: row.get::<_, Option<i64>>(0)?.map(|value| value as u64),
                    spec_count: row.get::<_, i64>(1)? as usize,
                    checklist_item_count: row.get::<_, i64>(2)? as usize,
                    dependency_count: row.get::<_, i64>(3)? as usize,
                    coverage_record_count,
                    sync_provenance_record_count,
                })
            },
        )?;
        Ok(row)
    }

    fn load_specs_from_connection(&self, conn: &Connection) -> Result<Vec<MaterializedSpecRecord>> {
        let mut stmt = conn.prepare(
            "SELECT spec_id, source_path, title, declared_status, created, content_digest, git_revision, body
             FROM spec_materialized_specs
             ORDER BY spec_id",
        )?;
        let rows = stmt.query_map([], |row| {
            Ok(MaterializedSpecRecord {
                spec_id: row.get(0)?,
                source_path: row.get(1)?,
                title: row.get(2)?,
                declared_status: row.get(3)?,
                created: row.get(4)?,
                content_digest: row.get(5)?,
                git_revision: row.get(6)?,
                body: row.get(7)?,
            })
        })?;
        Ok(rows.collect::<rusqlite::Result<Vec<_>>>()?)
    }

    fn load_checklist_items_from_connection(
        &self,
        conn: &Connection,
    ) -> Result<Vec<StoredSpecChecklistItemRecord>> {
        let mut stmt = conn.prepare(
            "SELECT spec_id, item_id, explicit_id, label, checked, requirement_level, section_path_json, line_number, identity_source
             FROM spec_materialized_checklist_items
             ORDER BY spec_id, line_number, item_id",
        )?;
        let rows = stmt.query_map([], |row| {
            let section_path_json: String = row.get(6)?;
            let section_path: Vec<String> = serde_json::from_str(&section_path_json).map_err(|error| {
                rusqlite::Error::FromSqlConversionFailure(
                    section_path_json.len(),
                    rusqlite::types::Type::Text,
                    Box::new(error),
                )
            })?;
            let requirement_level = match row.get::<_, String>(5)?.as_str() {
                "informational" => SpecChecklistRequirementLevel::Informational,
                _ => SpecChecklistRequirementLevel::Required,
            };
            let identity_source = match row.get::<_, String>(8)?.as_str() {
                "explicit" => SpecChecklistIdentitySource::Explicit,
                _ => SpecChecklistIdentitySource::Generated,
            };
            Ok(StoredSpecChecklistItemRecord {
                spec_id: row.get(0)?,
                item: SpecChecklistItem {
                    item_id: row.get(1)?,
                    explicit_id: row.get(2)?,
                    label: row.get(3)?,
                    checked: row.get::<_, i64>(4)? != 0,
                    requirement_level,
                    section_path,
                    line_number: row.get::<_, i64>(7)? as usize,
                    identity_source,
                },
            })
        })?;
        Ok(rows.collect::<rusqlite::Result<Vec<_>>>()?)
    }

    fn load_dependencies_from_connection(&self, conn: &Connection) -> Result<Vec<StoredSpecDependencyRecord>> {
        let mut stmt = conn.prepare(
            "SELECT spec_id, position, dependency_spec_id
             FROM spec_materialized_dependencies
             ORDER BY spec_id, position",
        )?;
        let rows = stmt.query_map([], |row| {
            Ok(StoredSpecDependencyRecord {
                spec_id: row.get(0)?,
                position: row.get::<_, i64>(1)? as usize,
                dependency_spec_id: row.get(2)?,
            })
        })?;
        Ok(rows.collect::<rusqlite::Result<Vec<_>>>()?)
    }

    fn load_statuses_from_connection(&self, conn: &Connection) -> Result<Vec<StoredSpecStatusRecord>> {
        let mut stmt = conn.prepare(
            "SELECT spec_id, declared_status, checklist_posture, dependency_posture, overall_status
             FROM spec_materialized_status
             ORDER BY spec_id",
        )?;
        let rows = stmt.query_map([], |row| {
            let checklist_posture = match row.get::<_, String>(2)?.as_str() {
                "complete" => StoredSpecChecklistPosture::Complete,
                _ => StoredSpecChecklistPosture::Incomplete,
            };
            let dependency_posture = match row.get::<_, String>(3)?.as_str() {
                "complete" => StoredSpecDependencyPosture::Complete,
                "incomplete" => StoredSpecDependencyPosture::Incomplete,
                "missing" => StoredSpecDependencyPosture::Missing,
                _ => StoredSpecDependencyPosture::Clear,
            };
            Ok(StoredSpecStatusRecord {
                spec_id: row.get(0)?,
                declared_status: row.get(1)?,
                checklist_posture,
                dependency_posture,
                overall_status: row.get(4)?,
            })
        })?;
        Ok(rows.collect::<rusqlite::Result<Vec<_>>>()?)
    }

    fn load_coverage_from_connection(
        &self,
        conn: &Connection,
    ) -> Result<Vec<StoredSpecCoverageRecord>> {
        let mut stmt = conn.prepare(
            "SELECT spec_id, checklist_item_id, coverage_kind, coordination_ref
             FROM spec_materialized_coverage
             ORDER BY spec_id, checklist_item_id, coverage_kind, coordination_ref",
        )?;
        let rows = stmt.query_map([], |row| {
            Ok(StoredSpecCoverageRecord {
                spec_id: row.get(0)?,
                checklist_item_id: row.get(1)?,
                coverage_kind: row.get(2)?,
                coordination_ref: row.get(3)?,
            })
        })?;
        Ok(rows.collect::<rusqlite::Result<Vec<_>>>()?)
    }

    fn load_sync_provenance_from_connection(
        &self,
        conn: &Connection,
    ) -> Result<Vec<StoredSpecSyncProvenanceRecord>> {
        let mut stmt = conn.prepare(
            "SELECT spec_id, target_coordination_ref, sync_kind, source_revision, covered_checklist_items_json
             FROM spec_materialized_sync_provenance
             ORDER BY spec_id, target_coordination_ref, sync_kind",
        )?;
        let rows = stmt.query_map([], |row| {
            let covered_checklist_items_json: String = row.get(4)?;
            let covered_checklist_items: Vec<String> =
                serde_json::from_str(&covered_checklist_items_json).map_err(|error| {
                    rusqlite::Error::FromSqlConversionFailure(
                        covered_checklist_items_json.len(),
                        rusqlite::types::Type::Text,
                        Box::new(error),
                    )
                })?;
            Ok(StoredSpecSyncProvenanceRecord {
                spec_id: row.get(0)?,
                target_coordination_ref: row.get(1)?,
                sync_kind: row.get(2)?,
                source_revision: row.get(3)?,
                covered_checklist_items,
            })
        })?;
        Ok(rows.collect::<rusqlite::Result<Vec<_>>>()?)
    }
}

impl SpecMaterializedStore for SqliteSpecMaterializedStore {
    fn capabilities(&self) -> SpecMaterializedCapabilities {
        SpecMaterializedCapabilities {
            supports_replace_from_parsed_batch: true,
            supports_checklist_items: true,
            supports_dependencies: true,
            supports_source_metadata: true,
        }
    }

    fn read_specs(&self) -> Result<SpecMaterializedReadEnvelope<Vec<MaterializedSpecRecord>>> {
        let conn = self.open_connection()?;
        let metadata = self.load_metadata_from_connection(&conn)?;
        let value = self.load_specs_from_connection(&conn)?;
        Ok(SpecMaterializedReadEnvelope::new(metadata, value))
    }

    fn read_checklist_items(
        &self,
    ) -> Result<SpecMaterializedReadEnvelope<Vec<StoredSpecChecklistItemRecord>>> {
        let conn = self.open_connection()?;
        let metadata = self.load_metadata_from_connection(&conn)?;
        let value = self.load_checklist_items_from_connection(&conn)?;
        Ok(SpecMaterializedReadEnvelope::new(metadata, value))
    }

    fn read_dependencies(
        &self,
    ) -> Result<SpecMaterializedReadEnvelope<Vec<StoredSpecDependencyRecord>>> {
        let conn = self.open_connection()?;
        let metadata = self.load_metadata_from_connection(&conn)?;
        let value = self.load_dependencies_from_connection(&conn)?;
        Ok(SpecMaterializedReadEnvelope::new(metadata, value))
    }

    fn read_status_records(
        &self,
    ) -> Result<SpecMaterializedReadEnvelope<Vec<StoredSpecStatusRecord>>> {
        let conn = self.open_connection()?;
        let metadata = self.load_metadata_from_connection(&conn)?;
        let value = self.load_statuses_from_connection(&conn)?;
        Ok(SpecMaterializedReadEnvelope::new(metadata, value))
    }

    fn read_coverage_records(
        &self,
    ) -> Result<SpecMaterializedReadEnvelope<Vec<StoredSpecCoverageRecord>>> {
        let conn = self.open_connection()?;
        let metadata = self.load_metadata_from_connection(&conn)?;
        let value = self.load_coverage_from_connection(&conn)?;
        Ok(SpecMaterializedReadEnvelope::new(metadata, value))
    }

    fn read_sync_provenance_records(
        &self,
    ) -> Result<SpecMaterializedReadEnvelope<Vec<StoredSpecSyncProvenanceRecord>>> {
        let conn = self.open_connection()?;
        let metadata = self.load_metadata_from_connection(&conn)?;
        let value = self.load_sync_provenance_from_connection(&conn)?;
        Ok(SpecMaterializedReadEnvelope::new(metadata, value))
    }

    fn read_metadata(&self) -> Result<SpecMaterializationMetadata> {
        let conn = self.open_connection()?;
        self.load_metadata_from_connection(&conn)
    }

    fn replace_materialization(
        &self,
        request: SpecMaterializedReplaceRequest,
    ) -> Result<SpecMaterializedWriteResult> {
        let conn = self.open_connection()?;
        let tx = conn.unchecked_transaction()?;
        tx.execute("DELETE FROM spec_materialized_checklist_items", [])?;
        tx.execute("DELETE FROM spec_materialized_dependencies", [])?;
        tx.execute("DELETE FROM spec_materialized_coverage", [])?;
        tx.execute("DELETE FROM spec_materialized_sync_provenance", [])?;
        tx.execute("DELETE FROM spec_materialized_status", [])?;
        tx.execute("DELETE FROM spec_materialized_specs", [])?;

        let mut checklist_item_count = 0usize;
        let mut dependency_count = 0usize;
        for spec in &request.parsed {
            tx.execute(
                "INSERT INTO spec_materialized_specs (
                    spec_id,
                    source_path,
                    title,
                    declared_status,
                    created,
                    content_digest,
                    git_revision,
                    body
                ) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8)",
                params![
                    spec.spec_id,
                    spec.source_metadata.repo_relative_path.to_string_lossy(),
                    spec.title,
                    spec.status.as_str(),
                    spec.created,
                    spec.source_metadata.content_digest,
                    spec.source_metadata.git_revision,
                    spec.body,
                ],
            )?;

            for item in &spec.checklist_items {
                checklist_item_count += 1;
                tx.execute(
                    "INSERT INTO spec_materialized_checklist_items (
                        item_id,
                        spec_id,
                        identity_source,
                        explicit_id,
                        label,
                        checked,
                        requirement_level,
                        section_path_json,
                        line_number
                    ) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9)",
                    params![
                        item.item_id,
                        spec.spec_id,
                        match item.identity_source {
                            SpecChecklistIdentitySource::Explicit => "explicit",
                            SpecChecklistIdentitySource::Generated => "generated",
                        },
                        item.explicit_id,
                        item.label,
                        i64::from(item.checked),
                        match item.requirement_level {
                            SpecChecklistRequirementLevel::Required => "required",
                            SpecChecklistRequirementLevel::Informational => "informational",
                        },
                        serde_json::to_string(&item.section_path)?,
                        item.line_number as i64,
                    ],
                )?;
            }

            for (position, dependency) in spec.dependencies.iter().enumerate() {
                dependency_count += 1;
                tx.execute(
                    "INSERT INTO spec_materialized_dependencies (
                        spec_id,
                        position,
                        dependency_spec_id
                    ) VALUES (?1, ?2, ?3)",
                    params![spec.spec_id, position as i64, dependency.spec_id],
                )?;
            }
        }

        let status_records = derive_status_records(&request.parsed);
        for status in status_records {
            tx.execute(
                "INSERT INTO spec_materialized_status (
                    spec_id,
                    declared_status,
                    checklist_posture,
                    dependency_posture,
                    overall_status
                ) VALUES (?1, ?2, ?3, ?4, ?5)",
                params![
                    status.spec_id,
                    status.declared_status,
                    match status.checklist_posture {
                        StoredSpecChecklistPosture::Complete => "complete",
                        StoredSpecChecklistPosture::Incomplete => "incomplete",
                    },
                    match status.dependency_posture {
                        StoredSpecDependencyPosture::Clear => "clear",
                        StoredSpecDependencyPosture::Complete => "complete",
                        StoredSpecDependencyPosture::Incomplete => "incomplete",
                        StoredSpecDependencyPosture::Missing => "missing",
                    },
                    status.overall_status,
                ],
            )?;
        }

        tx.execute(
            "UPDATE spec_materialized_metadata
             SET materialized_at = ?1,
                 spec_count = ?2,
                 checklist_item_count = ?3,
                 dependency_count = ?4
             WHERE id = 1",
            params![
                current_timestamp_millis() as i64,
                request.parsed.len() as i64,
                checklist_item_count as i64,
                dependency_count as i64,
            ],
        )?;
        tx.commit()?;

        Ok(SpecMaterializedWriteResult {
            metadata: self.read_metadata()?,
        })
    }

    fn clear_materialization(
        &self,
        request: SpecMaterializedClearRequest,
    ) -> Result<SpecMaterializedWriteResult> {
        let conn = self.open_connection()?;
        let tx = conn.unchecked_transaction()?;
        if request.clear_checklist_items {
            tx.execute("DELETE FROM spec_materialized_checklist_items", [])?;
        }
        if request.clear_dependencies {
            tx.execute("DELETE FROM spec_materialized_dependencies", [])?;
        }
        tx.execute("DELETE FROM spec_materialized_coverage", [])?;
        tx.execute("DELETE FROM spec_materialized_sync_provenance", [])?;
        tx.execute("DELETE FROM spec_materialized_status", [])?;
        if request.clear_specs {
            tx.execute("DELETE FROM spec_materialized_specs", [])?;
        }
        if request.clear_metadata {
            tx.execute(
                "UPDATE spec_materialized_metadata
                 SET materialized_at = NULL,
                     spec_count = 0,
                     checklist_item_count = 0,
                     dependency_count = 0
                 WHERE id = 1",
                [],
            )?;
        }
        tx.commit()?;
        Ok(SpecMaterializedWriteResult {
            metadata: self.read_metadata()?,
        })
    }
}

fn current_timestamp_millis() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .expect("system clock should be after unix epoch")
        .as_millis() as u64
}

fn derive_status_records(parsed: &[ParsedSpecDocument]) -> Vec<StoredSpecStatusRecord> {
    let checklist_complete = parsed
        .iter()
        .map(|spec| {
            (
                spec.spec_id.clone(),
                spec.checklist_items.iter().all(|item| {
                    item.checked
                        || matches!(
                            item.requirement_level,
                            SpecChecklistRequirementLevel::Informational
                        )
                }),
            )
        })
        .collect::<std::collections::BTreeMap<_, _>>();

    let mut declared_by_id = std::collections::BTreeMap::<String, SpecDeclaredStatus>::new();
    for spec in parsed {
        declared_by_id.insert(spec.spec_id.clone(), spec.status.clone());
    }

    parsed
        .iter()
        .map(|spec| {
            let checklist_posture = if checklist_complete
                .get(&spec.spec_id)
                .copied()
                .unwrap_or(false)
            {
                StoredSpecChecklistPosture::Complete
            } else {
                StoredSpecChecklistPosture::Incomplete
            };

            let dependency_posture = if spec.dependencies.is_empty() {
                StoredSpecDependencyPosture::Clear
            } else if spec
                .dependencies
                .iter()
                .any(|dependency| !declared_by_id.contains_key(&dependency.spec_id))
            {
                StoredSpecDependencyPosture::Missing
            } else if spec.dependencies.iter().all(|dependency| {
                matches!(
                    declared_by_id.get(&dependency.spec_id),
                    Some(SpecDeclaredStatus::Completed | SpecDeclaredStatus::Superseded)
                )
            }) {
                StoredSpecDependencyPosture::Complete
            } else {
                StoredSpecDependencyPosture::Incomplete
            };

            let overall_status = match spec.status {
                SpecDeclaredStatus::Completed => "completed",
                SpecDeclaredStatus::Superseded => "superseded",
                SpecDeclaredStatus::Abandoned => "abandoned",
                SpecDeclaredStatus::Blocked => "blocked",
                SpecDeclaredStatus::Draft => {
                    if matches!(
                        dependency_posture,
                        StoredSpecDependencyPosture::Missing
                            | StoredSpecDependencyPosture::Incomplete
                    ) {
                        "blocked"
                    } else {
                        "draft"
                    }
                }
                SpecDeclaredStatus::InProgress => {
                    if matches!(
                        dependency_posture,
                        StoredSpecDependencyPosture::Missing
                            | StoredSpecDependencyPosture::Incomplete
                    ) {
                        "blocked"
                    } else {
                        "in_progress"
                    }
                }
            }
            .to_owned();

            StoredSpecStatusRecord {
                spec_id: spec.spec_id.clone(),
                declared_status: spec.status.as_str().to_owned(),
                checklist_posture,
                dependency_posture,
                overall_status,
            }
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use std::fs;
    use std::path::PathBuf;
    use std::sync::atomic::{AtomicU64, Ordering};
    use std::time::{SystemTime, UNIX_EPOCH};

    use super::SqliteSpecMaterializedStore;
    use crate::{
        discover_spec_sources, parse_spec_sources, SpecMaterializedClearRequest,
        SpecMaterializedReplaceRequest, SpecMaterializedStore,
        StoredSpecChecklistPosture, StoredSpecDependencyPosture,
    };

    static NEXT_TEMP_REPO: AtomicU64 = AtomicU64::new(0);

    fn temp_repo(label: &str) -> PathBuf {
        let nonce = NEXT_TEMP_REPO.fetch_add(1, Ordering::Relaxed);
        let unique = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        let root = std::env::temp_dir().join(format!("prism-spec-store-{label}-{unique}-{nonce}"));
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

    #[test]
    fn sqlite_spec_materialized_store_replaces_and_reads_parsed_batch() {
        let root = temp_repo("replace");
        write_spec(
            &root,
            ".prism/specs/2026-04-09-a.md",
            "---\nid: spec:a\ntitle: Alpha\nstatus: draft\ncreated: 2026-04-09\ndepends_on:\n  - spec:b\n---\n\n## Build\n\n- [ ] first <!-- id: first -->\n",
        );
        write_spec(
            &root,
            ".prism/specs/2026-04-09-b.md",
            "---\nid: spec:b\ntitle: Beta\nstatus: completed\ncreated: 2026-04-09\n---\n\n- [x] done\n",
        );

        let discovered = discover_spec_sources(&root).unwrap();
        let parsed = parse_spec_sources(&discovered);
        assert!(parsed.diagnostics.is_empty());

        let store = SqliteSpecMaterializedStore::new(&root.join(".tmp/spec-materialized.db"));
        let write_result = store
            .replace_materialization(SpecMaterializedReplaceRequest {
                parsed: parsed.parsed.clone(),
            })
            .unwrap();
        assert_eq!(write_result.metadata.spec_count, 2);
        assert_eq!(write_result.metadata.checklist_item_count, 2);
        assert_eq!(write_result.metadata.dependency_count, 1);
        assert_eq!(write_result.metadata.coverage_record_count, 0);
        assert_eq!(write_result.metadata.sync_provenance_record_count, 0);

        let specs = store.read_specs().unwrap();
        assert_eq!(specs.value.len(), 2);
        assert_eq!(specs.value[0].spec_id, "spec:a");

        let checklist_items = store.read_checklist_items().unwrap();
        assert_eq!(checklist_items.value.len(), 2);
        assert_eq!(checklist_items.value[0].spec_id, "spec:a");
        assert_eq!(checklist_items.value[0].item.item_id, "spec:a::checklist::first");

        let dependencies = store.read_dependencies().unwrap();
        assert_eq!(dependencies.value.len(), 1);
        assert_eq!(dependencies.value[0].dependency_spec_id, "spec:b");

        let statuses = store.read_status_records().unwrap();
        assert_eq!(statuses.value.len(), 2);
        assert_eq!(statuses.value[0].spec_id, "spec:a");
        assert_eq!(
            statuses.value[0].checklist_posture,
            StoredSpecChecklistPosture::Incomplete
        );
        assert_eq!(
            statuses.value[0].dependency_posture,
            StoredSpecDependencyPosture::Complete
        );
    }

    #[test]
    fn sqlite_spec_materialized_store_clear_removes_persisted_state() {
        let root = temp_repo("clear");
        write_spec(
            &root,
            ".prism/specs/2026-04-09-a.md",
            "---\nid: spec:a\ntitle: Alpha\nstatus: draft\ncreated: 2026-04-09\n---\n\n- [ ] first\n",
        );

        let discovered = discover_spec_sources(&root).unwrap();
        let parsed = parse_spec_sources(&discovered);
        let store = SqliteSpecMaterializedStore::new(&root.join(".tmp/spec-materialized.db"));
        store
            .replace_materialization(SpecMaterializedReplaceRequest {
                parsed: parsed.parsed,
            })
            .unwrap();

        let cleared = store
            .clear_materialization(SpecMaterializedClearRequest::all())
            .unwrap();
        assert_eq!(cleared.metadata.spec_count, 0);
        assert_eq!(store.read_specs().unwrap().value.len(), 0);
        assert_eq!(store.read_checklist_items().unwrap().value.len(), 0);
        assert_eq!(store.read_dependencies().unwrap().value.len(), 0);
        assert_eq!(store.read_status_records().unwrap().value.len(), 0);
        assert_eq!(store.read_coverage_records().unwrap().value.len(), 0);
        assert_eq!(store.read_sync_provenance_records().unwrap().value.len(), 0);
    }
}
