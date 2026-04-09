use std::path::{Path, PathBuf};

use anyhow::{anyhow, Result};

use super::git_shared_refs::GitSharedRefsCoordinationAuthorityStore;
use super::traits::CoordinationAuthorityStore;

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum CoordinationAuthorityBackendConfig {
    GitSharedRefs,
    Sqlite { db_path: PathBuf },
    Postgres { connection_url: String },
}

impl Default for CoordinationAuthorityBackendConfig {
    fn default() -> Self {
        Self::GitSharedRefs
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CoordinationAuthorityStoreProvider {
    config: CoordinationAuthorityBackendConfig,
}

impl CoordinationAuthorityStoreProvider {
    pub fn new(config: CoordinationAuthorityBackendConfig) -> Self {
        Self { config }
    }

    pub fn config(&self) -> &CoordinationAuthorityBackendConfig {
        &self.config
    }

    pub fn open(&self, root: &Path) -> Result<Box<dyn CoordinationAuthorityStore>> {
        open_coordination_authority_store(root, &self.config)
    }
}

impl Default for CoordinationAuthorityStoreProvider {
    fn default() -> Self {
        Self::new(CoordinationAuthorityBackendConfig::default())
    }
}

pub fn default_coordination_authority_store_provider() -> CoordinationAuthorityStoreProvider {
    CoordinationAuthorityStoreProvider::default()
}

pub fn open_coordination_authority_store(
    root: &Path,
    config: &CoordinationAuthorityBackendConfig,
) -> Result<Box<dyn CoordinationAuthorityStore>> {
    match config {
        CoordinationAuthorityBackendConfig::GitSharedRefs => {
            Ok(Box::new(GitSharedRefsCoordinationAuthorityStore::new(root)))
        }
        CoordinationAuthorityBackendConfig::Sqlite { db_path } => Err(anyhow!(
            "sqlite-backed coordination authority is not implemented yet (configured db path: {})",
            db_path.display()
        )),
        CoordinationAuthorityBackendConfig::Postgres { connection_url } => Err(anyhow!(
            "postgres-backed coordination authority is not implemented yet (configured connection: {})",
            connection_url
        )),
    }
}

pub fn open_default_coordination_authority_store(
    root: &Path,
) -> Result<Box<dyn CoordinationAuthorityStore>> {
    default_coordination_authority_store_provider().open(root)
}

#[cfg(test)]
mod tests {
    use std::path::Path;

    use super::{
        default_coordination_authority_store_provider, open_coordination_authority_store,
        CoordinationAuthorityBackendConfig, CoordinationAuthorityStoreProvider,
    };

    #[test]
    fn default_backend_config_is_git_shared_refs() {
        assert_eq!(
            CoordinationAuthorityBackendConfig::default(),
            CoordinationAuthorityBackendConfig::GitSharedRefs
        );
    }

    #[test]
    fn default_provider_uses_default_backend_config() {
        let provider = default_coordination_authority_store_provider();
        assert_eq!(
            provider.config(),
            &CoordinationAuthorityBackendConfig::GitSharedRefs
        );
    }

    #[test]
    fn provider_opens_using_its_config() {
        let provider =
            CoordinationAuthorityStoreProvider::new(CoordinationAuthorityBackendConfig::Sqlite {
                db_path: Path::new("coordination-authority.db").to_path_buf(),
            });
        let error = match provider.open(Path::new(".")) {
            Ok(_) => panic!("sqlite backend should not open yet"),
            Err(error) => error,
        };
        assert!(error
            .to_string()
            .contains("sqlite-backed coordination authority is not implemented yet"));
    }

    #[test]
    fn opening_sqlite_backend_is_explicitly_unimplemented() {
        let error = match open_coordination_authority_store(
            Path::new("."),
            &CoordinationAuthorityBackendConfig::Sqlite {
                db_path: Path::new("coordination-authority.db").to_path_buf(),
            },
        ) {
            Ok(_) => panic!("sqlite backend should not open yet"),
            Err(error) => error,
        };
        assert!(error
            .to_string()
            .contains("sqlite-backed coordination authority is not implemented yet"));
    }

    #[test]
    fn opening_postgres_backend_is_explicitly_unimplemented() {
        let error = match open_coordination_authority_store(
            Path::new("."),
            &CoordinationAuthorityBackendConfig::Postgres {
                connection_url: "postgres://localhost/prism".to_string(),
            },
        ) {
            Ok(_) => panic!("postgres backend should not open yet"),
            Err(error) => error,
        };
        assert!(error
            .to_string()
            .contains("postgres-backed coordination authority is not implemented yet"));
    }
}
