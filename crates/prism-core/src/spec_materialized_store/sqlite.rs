use std::path::{Path, PathBuf};

use anyhow::Result;
use rusqlite::{params, Connection};

use super::traits::SpecMaterializedStore;
use super::types::{
    MaterializedSpecRecord, SpecMaterializationMetadata, SpecMaterializedBackendKind,
    SpecMaterializedCapabilities, SpecMaterializedClearRequest, SpecMaterializedReadEnvelope,
    SpecMaterializedReplaceRequest, SpecMaterializedWriteResult, StoredSpecDependencyRecord,
};
use crate::prism_paths::PrismPaths;
use crate::util::current_timestamp_millis;
use crate::{
    SpecChecklistIdentitySource, SpecChecklistItem, SpecChecklistRequirementLevel,
};

pub struct SqliteSpecMaterializedStore {
    root: PathBuf,
}

impl SqliteSpecMaterializedStore {
    pub fn new(root: &Path) -> Self {
        Self {
            root: root.to_path_buf(),
        }
    }

    fn open_connection(&self) -> Result<Connection> {
        let path = PrismPaths::for_workspace_root(&self.root)?.worktree_cache_db_path()?;
        let conn = Connection::open(path)?;
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

    fn load_checklist_items_from_connection(&self, conn: &Connection) -> Result<Vec<SpecChecklistItem>> {
        let mut stmt = conn.prepare(
            "SELECT item_id, explicit_id, label, checked, requirement_level, section_path_json, line_number, identity_source
             FROM spec_materialized_checklist_items
             ORDER BY spec_id, line_number, item_id",
        )?;
        let rows = stmt.query_map([], |row| {
            let section_path_json: String = row.get(5)?;
            let section_path: Vec<String> = serde_json::from_str(&section_path_json).map_err(|error| {
                rusqlite::Error::FromSqlConversionFailure(
                    section_path_json.len(),
                    rusqlite::types::Type::Text,
                    Box::new(error),
                )
            })?;
            let requirement_level = match row.get::<_, String>(4)?.as_str() {
                "informational" => SpecChecklistRequirementLevel::Informational,
                _ => SpecChecklistRequirementLevel::Required,
            };
            let identity_source = match row.get::<_, String>(7)?.as_str() {
                "explicit" => SpecChecklistIdentitySource::Explicit,
                _ => SpecChecklistIdentitySource::Generated,
            };
            Ok(SpecChecklistItem {
                item_id: row.get(0)?,
                explicit_id: row.get(1)?,
                label: row.get(2)?,
                checked: row.get::<_, i64>(3)? != 0,
                requirement_level,
                section_path,
                line_number: row.get::<_, i64>(6)? as usize,
                identity_source,
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

    fn read_checklist_items(&self) -> Result<SpecMaterializedReadEnvelope<Vec<SpecChecklistItem>>> {
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

#[cfg(test)]
mod tests {
    use std::fs;
    use std::path::PathBuf;
    use std::sync::atomic::{AtomicU64, Ordering};
    use std::time::{SystemTime, UNIX_EPOCH};

    use super::SqliteSpecMaterializedStore;
    use crate::prism_paths::set_test_prism_home_override;
    use crate::{
        discover_spec_sources, parse_spec_sources, SpecMaterializedClearRequest,
        SpecMaterializedReplaceRequest, SpecMaterializedStore,
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
        let home = temp_repo("replace-home");
        let _guard = set_test_prism_home_override(&home);
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

        let store = SqliteSpecMaterializedStore::new(&root);
        let write_result = store
            .replace_materialization(SpecMaterializedReplaceRequest {
                parsed: parsed.parsed.clone(),
            })
            .unwrap();
        assert_eq!(write_result.metadata.spec_count, 2);
        assert_eq!(write_result.metadata.checklist_item_count, 2);
        assert_eq!(write_result.metadata.dependency_count, 1);

        let specs = store.read_specs().unwrap();
        assert_eq!(specs.value.len(), 2);
        assert_eq!(specs.value[0].spec_id, "spec:a");

        let checklist_items = store.read_checklist_items().unwrap();
        assert_eq!(checklist_items.value.len(), 2);
        assert_eq!(checklist_items.value[0].item_id, "spec:a::checklist::first");

        let dependencies = store.read_dependencies().unwrap();
        assert_eq!(dependencies.value.len(), 1);
        assert_eq!(dependencies.value[0].dependency_spec_id, "spec:b");
    }

    #[test]
    fn sqlite_spec_materialized_store_clear_removes_persisted_state() {
        let root = temp_repo("clear");
        let home = temp_repo("clear-home");
        let _guard = set_test_prism_home_override(&home);
        write_spec(
            &root,
            ".prism/specs/2026-04-09-a.md",
            "---\nid: spec:a\ntitle: Alpha\nstatus: draft\ncreated: 2026-04-09\n---\n\n- [ ] first\n",
        );

        let discovered = discover_spec_sources(&root).unwrap();
        let parsed = parse_spec_sources(&discovered);
        let store = SqliteSpecMaterializedStore::new(&root);
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
    }
}
