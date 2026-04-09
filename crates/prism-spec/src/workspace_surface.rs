use std::path::{Path, PathBuf};

use anyhow::Result;
use prism_coordination::CoordinationSnapshot;

use crate::{
    refresh_spec_materialization, MaterializedSpecQueryEngine, SpecMaterializationRefreshResult,
    SpecQueryEngine, SqliteSpecMaterializedStore,
};

pub fn default_spec_materialized_db_path(repo_root: &Path) -> PathBuf {
    repo_root
        .join(".prism")
        .join("state")
        .join("spec-materialized.db")
}

pub struct WorkspaceSpecSurface {
    repo_root: PathBuf,
    materialized_db_path: PathBuf,
    store: SqliteSpecMaterializedStore,
}

impl WorkspaceSpecSurface {
    pub fn new(repo_root: &Path) -> Self {
        let materialized_db_path = default_spec_materialized_db_path(repo_root);
        let store = SqliteSpecMaterializedStore::new(&materialized_db_path);
        Self {
            repo_root: repo_root.to_path_buf(),
            materialized_db_path,
            store,
        }
    }

    pub fn repo_root(&self) -> &Path {
        &self.repo_root
    }

    pub fn materialized_db_path(&self) -> &Path {
        &self.materialized_db_path
    }

    pub fn refresh(
        &self,
        coordination: Option<CoordinationSnapshot>,
    ) -> Result<SpecMaterializationRefreshResult> {
        refresh_spec_materialization(&self.store, &self.repo_root, coordination)
    }

    pub fn with_query_engine<T, F>(
        &self,
        coordination: Option<CoordinationSnapshot>,
        f: F,
    ) -> Result<T>
    where
        F: FnOnce(&dyn SpecQueryEngine) -> Result<T>,
    {
        self.refresh(coordination)?;
        let engine = MaterializedSpecQueryEngine::new(&self.store);
        f(&engine)
    }
}

#[cfg(test)]
mod tests {
    use std::fs;
    use std::path::{Path, PathBuf};
    use std::sync::atomic::{AtomicU64, Ordering};
    use std::time::{SystemTime, UNIX_EPOCH};

    use prism_coordination::CoordinationSnapshot;

    use crate::WorkspaceSpecSurface;

    static NEXT_TEMP_REPO: AtomicU64 = AtomicU64::new(0);

    fn temp_repo(label: &str) -> PathBuf {
        let nonce = NEXT_TEMP_REPO.fetch_add(1, Ordering::Relaxed);
        let unique = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        let root =
            std::env::temp_dir().join(format!("prism-spec-surface-{label}-{unique}-{nonce}"));
        let _ = fs::remove_dir_all(&root);
        fs::create_dir_all(root.join(".git")).unwrap();
        fs::write(root.join("Cargo.toml"), "[workspace]\nmembers = []\n").unwrap();
        fs::write(root.join(".git/HEAD"), "ref: refs/heads/main\n").unwrap();
        root
    }

    fn write_spec(root: &Path, relative_path: &str, contents: &str) {
        let path = root.join(relative_path);
        fs::create_dir_all(path.parent().unwrap()).unwrap();
        fs::write(path, contents).unwrap();
    }

    #[test]
    fn workspace_spec_surface_uses_default_materialized_db_path() {
        let root = temp_repo("db-path");
        let surface = WorkspaceSpecSurface::new(&root);
        assert_eq!(
            surface.materialized_db_path(),
            root.join(".prism/state/spec-materialized.db")
        );
    }

    #[test]
    fn workspace_spec_surface_refreshes_and_queries_specs() {
        let root = temp_repo("query");
        write_spec(
            &root,
            ".prism/specs/2026-04-09-a.md",
            "---\nid: spec:a\ntitle: Alpha\nstatus: in_progress\ncreated: 2026-04-09\n---\n\n- [ ] implement <!-- id: item-1 -->\n",
        );

        let surface = WorkspaceSpecSurface::new(&root);
        let specs = surface
            .with_query_engine(Some(CoordinationSnapshot::default()), |engine| {
                engine.list_specs()
            })
            .unwrap();

        assert_eq!(specs.len(), 1);
        assert_eq!(specs[0].spec_id, "spec:a");
        assert_eq!(specs[0].title, "Alpha");
    }
}
