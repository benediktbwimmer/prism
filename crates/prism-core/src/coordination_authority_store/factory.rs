use std::path::{Path, PathBuf};

use anyhow::{anyhow, Context, Result};
use serde::Deserialize;

use super::git_shared_refs::GitSharedRefsCoordinationAuthorityStore;
use super::sqlite::SqliteCoordinationAuthorityStore;
use super::traits::CoordinationAuthorityStore;
use crate::PrismPaths;

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

#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "snake_case")]
enum CoordinationAuthorityBackendName {
    GitSharedRefs,
    Sqlite,
    Postgres,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
struct WorkspaceServiceConfigFile {
    coordination_authority: Option<WorkspaceCoordinationAuthorityConfigFile>,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
struct WorkspaceCoordinationAuthorityConfigFile {
    backend: CoordinationAuthorityBackendName,
    sqlite_db_path: Option<PathBuf>,
    postgres_connection_url: Option<String>,
}

pub fn configured_coordination_authority_store_provider(
    root: &Path,
) -> Result<CoordinationAuthorityStoreProvider> {
    resolve_coordination_authority_store_provider(root, None)
}

pub fn resolve_coordination_authority_store_provider(
    root: &Path,
    override_config: Option<CoordinationAuthorityBackendConfig>,
) -> Result<CoordinationAuthorityStoreProvider> {
    let config = match override_config {
        Some(config) => normalize_coordination_authority_backend_config(root, config)?,
        None => load_workspace_coordination_authority_backend_config(root)?,
    };
    Ok(CoordinationAuthorityStoreProvider::new(config))
}

fn load_workspace_coordination_authority_backend_config(
    root: &Path,
) -> Result<CoordinationAuthorityBackendConfig> {
    let paths = PrismPaths::for_workspace_root(root)?;
    let config_path = paths.service_config_path();
    if !config_path.exists() {
        return default_service_coordination_authority_backend(root);
    }
    let bytes = std::fs::read(&config_path)
        .with_context(|| format!("failed to read {}", config_path.display()))?;
    let file: WorkspaceServiceConfigFile = serde_json::from_slice(&bytes)
        .with_context(|| format!("failed to parse {}", config_path.display()))?;
    match file.coordination_authority {
        Some(config) => coordination_authority_backend_config_from_file(root, config),
        None => default_service_coordination_authority_backend(root),
    }
}

fn coordination_authority_backend_config_from_file(
    root: &Path,
    config: WorkspaceCoordinationAuthorityConfigFile,
) -> Result<CoordinationAuthorityBackendConfig> {
    match config.backend {
        CoordinationAuthorityBackendName::GitSharedRefs => {
            Ok(CoordinationAuthorityBackendConfig::GitSharedRefs)
        }
        CoordinationAuthorityBackendName::Sqlite => {
            let db_path = match config.sqlite_db_path {
                Some(path) => resolve_configured_path(root, path),
                None => PrismPaths::for_workspace_root(root)?.coordination_authority_db_path()?,
            };
            Ok(CoordinationAuthorityBackendConfig::Sqlite { db_path })
        }
        CoordinationAuthorityBackendName::Postgres => {
            let connection_url = config.postgres_connection_url.ok_or_else(|| {
                anyhow!(
                    "workspace service config selects postgres coordination authority without \
                     `postgresConnectionUrl`"
                )
            })?;
            Ok(CoordinationAuthorityBackendConfig::Postgres { connection_url })
        }
    }
}

fn normalize_coordination_authority_backend_config(
    root: &Path,
    config: CoordinationAuthorityBackendConfig,
) -> Result<CoordinationAuthorityBackendConfig> {
    Ok(match config {
        CoordinationAuthorityBackendConfig::GitSharedRefs => {
            CoordinationAuthorityBackendConfig::GitSharedRefs
        }
        CoordinationAuthorityBackendConfig::Sqlite { db_path } => {
            CoordinationAuthorityBackendConfig::Sqlite {
                db_path: resolve_configured_path(root, db_path),
            }
        }
        CoordinationAuthorityBackendConfig::Postgres { connection_url } => {
            CoordinationAuthorityBackendConfig::Postgres { connection_url }
        }
    })
}

fn default_service_coordination_authority_backend(
    root: &Path,
) -> Result<CoordinationAuthorityBackendConfig> {
    Ok(CoordinationAuthorityBackendConfig::Sqlite {
        db_path: PrismPaths::for_workspace_root(root)?.coordination_authority_db_path()?,
    })
}

fn resolve_configured_path(root: &Path, path: PathBuf) -> PathBuf {
    if path.is_absolute() {
        path
    } else {
        root.join(path)
    }
}

pub fn open_coordination_authority_store(
    root: &Path,
    config: &CoordinationAuthorityBackendConfig,
) -> Result<Box<dyn CoordinationAuthorityStore>> {
    match config {
        CoordinationAuthorityBackendConfig::GitSharedRefs => {
            Ok(Box::new(GitSharedRefsCoordinationAuthorityStore::new(root)))
        }
        CoordinationAuthorityBackendConfig::Sqlite { db_path } => Ok(Box::new(
            SqliteCoordinationAuthorityStore::new(root, db_path),
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
    use std::fs;
    use std::path::{Path, PathBuf};
    use std::sync::atomic::{AtomicU64, Ordering};
    use std::time::{SystemTime, UNIX_EPOCH};

    use super::{
        configured_coordination_authority_store_provider,
        default_coordination_authority_store_provider, open_coordination_authority_store,
        resolve_coordination_authority_store_provider, CoordinationAuthorityBackendConfig,
        CoordinationAuthorityStoreProvider,
    };

    static NEXT_TEMP_ROOT: AtomicU64 = AtomicU64::new(0);

    fn temp_root() -> std::path::PathBuf {
        let nonce = NEXT_TEMP_ROOT.fetch_add(1, Ordering::Relaxed);
        let unique = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        let root = std::env::temp_dir().join(format!("prism-authority-factory-{unique}-{nonce}"));
        let _ = fs::remove_dir_all(&root);
        fs::create_dir_all(&root).unwrap();
        root
    }

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
        let root = temp_root();
        let provider =
            CoordinationAuthorityStoreProvider::new(CoordinationAuthorityBackendConfig::Sqlite {
                db_path: root.join("coordination-authority.db"),
            });
        let store = provider
            .open(&root)
            .expect("sqlite backend should now open");
        assert!(store.capabilities().supports_transactions);
        let _ = fs::remove_dir_all(root);
    }

    #[test]
    fn opening_sqlite_backend_returns_sqlite_store() {
        let root = temp_root();
        let store = open_coordination_authority_store(
            &root,
            &CoordinationAuthorityBackendConfig::Sqlite {
                db_path: root.join("coordination-authority.db"),
            },
        )
        .expect("sqlite backend should open");
        assert!(store.capabilities().supports_retained_history);
        let _ = fs::remove_dir_all(root);
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

    #[test]
    fn configured_provider_defaults_to_repo_scoped_sqlite_authority() {
        let root = temp_root();
        let provider = configured_coordination_authority_store_provider(&root)
            .expect("configured provider should resolve");
        match provider.config() {
            CoordinationAuthorityBackendConfig::Sqlite { db_path } => {
                assert!(db_path.ends_with("authority.db"));
            }
            other => panic!("expected sqlite backend, got {other:?}"),
        }
        let _ = fs::remove_dir_all(root);
    }

    #[test]
    fn configured_provider_honors_service_config_file() {
        let root = temp_root();
        let prism_dir = root.join(".prism");
        fs::create_dir_all(&prism_dir).unwrap();
        fs::write(
            prism_dir.join("service.json"),
            r#"{
  "coordinationAuthority": {
    "backend": "git_shared_refs"
  }
}"#,
        )
        .unwrap();

        let provider = configured_coordination_authority_store_provider(&root)
            .expect("configured provider should resolve");
        assert_eq!(
            provider.config(),
            &CoordinationAuthorityBackendConfig::GitSharedRefs
        );
        let _ = fs::remove_dir_all(root);
    }

    #[test]
    fn override_config_takes_precedence_over_service_file() {
        let root = temp_root();
        let prism_dir = root.join(".prism");
        fs::create_dir_all(&prism_dir).unwrap();
        fs::write(
            prism_dir.join("service.json"),
            r#"{
  "coordinationAuthority": {
    "backend": "git_shared_refs"
  }
}"#,
        )
        .unwrap();

        let provider = resolve_coordination_authority_store_provider(
            &root,
            Some(CoordinationAuthorityBackendConfig::Sqlite {
                db_path: PathBuf::from("custom-authority.db"),
            }),
        )
        .expect("override should resolve");
        match provider.config() {
            CoordinationAuthorityBackendConfig::Sqlite { db_path } => {
                assert_eq!(db_path, &root.join("custom-authority.db"));
            }
            other => panic!("expected sqlite backend, got {other:?}"),
        }
        let _ = fs::remove_dir_all(root);
    }
}
