use std::path::Path;

use anyhow::Result;
use prism_core::{CoordinationAuthorityBackendConfig, PrismPaths};

use crate::cli::{McpCommand, ServiceCommand};
use crate::mcp::{self, DaemonRestartOptions, DaemonStartOptions};
use crate::service_state;

const PRISM_POSTGRES_DSN_ENV: &str = "PRISM_POSTGRES_DSN";

pub(crate) fn handle(root: &Path, command: ServiceCommand) -> Result<()> {
    match command {
        ServiceCommand::Up {
            no_coordination,
            internal_developer,
            runtime_mode,
            http_bind,
            shared_runtime_uri,
        } => {
            let authority = service_coordination_authority_backend(root)?;
            mcp::start_with_options(
                root,
                DaemonStartOptions {
                    no_coordination,
                    internal_developer,
                    runtime_mode: runtime_mode.into(),
                    ui: true,
                    http_bind,
                    shared_runtime_uri,
                    coordination_authority_backend: Some(authority_backend_arg(&authority)),
                    coordination_authority_sqlite_db: authority_sqlite_db(&authority),
                    coordination_authority_postgres_url: authority_postgres_url(&authority),
                },
            )?;
            service_state::sync_local_service_endpoint(root)?;
            print_service_backend_posture(&authority);
            Ok(())
        }
        ServiceCommand::Stop { kill_bridges } => {
            mcp::handle(root, McpCommand::Stop { kill_bridges })?;
            service_state::clear_local_service_endpoint(root)
        }
        ServiceCommand::Restart {
            no_coordination,
            internal_developer,
            runtime_mode,
            http_bind,
            shared_runtime_uri,
            kill_bridges,
        } => {
            let authority = service_coordination_authority_backend(root)?;
            mcp::restart_with_options(
                root,
                DaemonRestartOptions {
                    kill_bridges,
                    start: DaemonStartOptions {
                        no_coordination,
                        internal_developer,
                        runtime_mode: runtime_mode.into(),
                        ui: true,
                        http_bind,
                        shared_runtime_uri,
                        coordination_authority_backend: Some(authority_backend_arg(&authority)),
                        coordination_authority_sqlite_db: authority_sqlite_db(&authority),
                        coordination_authority_postgres_url: authority_postgres_url(&authority),
                    },
                },
            )?;
            service_state::sync_local_service_endpoint(root)?;
            print_service_backend_posture(&authority);
            Ok(())
        }
        ServiceCommand::Endpoint => {
            println!("{}", service_state::render_endpoint(root)?);
            Ok(())
        }
        ServiceCommand::EnrollRepo => {
            let record = service_state::enroll_current_repo(root)?;
            println!("enrolled repo");
            println!("canonical_root = {}", record.canonical_root);
            println!("enrolled_at_ms = {}", record.enrolled_at_ms);
            Ok(())
        }
        ServiceCommand::Status => mcp::status_with_coordination_authority_override(
            root,
            Some(service_coordination_authority_backend(root)?),
        ),
        ServiceCommand::Health => mcp::handle(root, McpCommand::Health),
    }
}

fn start_options(command: ServiceCommand) -> Option<DaemonStartOptions> {
    match command {
        ServiceCommand::Up {
            no_coordination,
            internal_developer,
            runtime_mode,
            http_bind,
            shared_runtime_uri,
        } => Some(DaemonStartOptions {
            no_coordination,
            internal_developer,
            runtime_mode: runtime_mode.into(),
            ui: true,
            http_bind,
            shared_runtime_uri,
            coordination_authority_backend: None,
            coordination_authority_sqlite_db: None,
            coordination_authority_postgres_url: None,
        }),
        _ => None,
    }
}

fn restart_options(command: ServiceCommand) -> Option<DaemonRestartOptions> {
    match command {
        ServiceCommand::Restart {
            no_coordination,
            internal_developer,
            runtime_mode,
            http_bind,
            shared_runtime_uri,
            kill_bridges,
        } => Some(DaemonRestartOptions {
            kill_bridges,
            start: DaemonStartOptions {
                no_coordination,
                internal_developer,
                runtime_mode: runtime_mode.into(),
                ui: true,
                http_bind,
                shared_runtime_uri,
                coordination_authority_backend: None,
                coordination_authority_sqlite_db: None,
                coordination_authority_postgres_url: None,
            },
        }),
        _ => None,
    }
}

fn service_coordination_authority_backend(
    root: &Path,
) -> Result<CoordinationAuthorityBackendConfig> {
    let postgres_dsn = std::env::var(PRISM_POSTGRES_DSN_ENV)
        .ok()
        .map(|value| value.trim().to_string())
        .filter(|value| !value.is_empty());
    match postgres_dsn {
        Some(connection_url) => Ok(CoordinationAuthorityBackendConfig::Postgres { connection_url }),
        None => Ok(CoordinationAuthorityBackendConfig::Sqlite {
            db_path: PrismPaths::for_workspace_root(root)?.coordination_authority_db_path()?,
        }),
    }
}

fn authority_backend_arg(
    config: &CoordinationAuthorityBackendConfig,
) -> crate::cli::CoordinationAuthorityBackendArg {
    match config {
        CoordinationAuthorityBackendConfig::GitSharedRefs => {
            crate::cli::CoordinationAuthorityBackendArg::GitSharedRefs
        }
        CoordinationAuthorityBackendConfig::Sqlite { .. } => {
            crate::cli::CoordinationAuthorityBackendArg::Sqlite
        }
        CoordinationAuthorityBackendConfig::Postgres { .. } => {
            crate::cli::CoordinationAuthorityBackendArg::Postgres
        }
    }
}

fn authority_sqlite_db(config: &CoordinationAuthorityBackendConfig) -> Option<std::path::PathBuf> {
    match config {
        CoordinationAuthorityBackendConfig::Sqlite { db_path } => Some(db_path.clone()),
        _ => None,
    }
}

fn authority_postgres_url(config: &CoordinationAuthorityBackendConfig) -> Option<String> {
    match config {
        CoordinationAuthorityBackendConfig::Postgres { connection_url } => {
            Some(connection_url.clone())
        }
        _ => None,
    }
}

fn print_service_backend_posture(config: &CoordinationAuthorityBackendConfig) {
    match config {
        CoordinationAuthorityBackendConfig::Sqlite { .. } => {
            println!("coordination_authority_backend = sqlite");
            eprintln!(
                "warning: sqlite-backed PRISM Service is supported only for a single-instance topology; multi-instance deployments must set {PRISM_POSTGRES_DSN_ENV}"
            );
        }
        CoordinationAuthorityBackendConfig::Postgres { .. } => {
            println!("coordination_authority_backend = postgres");
        }
        CoordinationAuthorityBackendConfig::GitSharedRefs => {
            println!("coordination_authority_backend = git_shared_refs");
        }
    }
}

#[cfg(test)]
mod tests {
    use std::fs;
    use std::sync::{Mutex, OnceLock};
    use std::time::{SystemTime, UNIX_EPOCH};

    use prism_core::PrismRuntimeMode;

    use crate::cli::{PrismRuntimeModeArg, ServiceCommand};

    use super::{
        restart_options, service_coordination_authority_backend, start_options,
        CoordinationAuthorityBackendConfig, PRISM_POSTGRES_DSN_ENV,
    };

    static ENV_LOCK: OnceLock<Mutex<()>> = OnceLock::new();

    fn temp_root() -> std::path::PathBuf {
        let unique = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        let root = std::env::temp_dir().join(format!("prism-service-test-{unique}"));
        let _ = fs::remove_dir_all(&root);
        fs::create_dir_all(&root).unwrap();
        root
    }

    #[test]
    fn service_up_builds_service_start_options() {
        let translated = start_options(ServiceCommand::Up {
            no_coordination: false,
            internal_developer: true,
            runtime_mode: crate::cli::PrismRuntimeModeArg::CoordinationOnly,
            http_bind: Some("127.0.0.1:43123".to_string()),
            shared_runtime_uri: Some("http://runtime.example".to_string()),
        })
        .expect("expected service start options");

        assert!(translated.internal_developer);
        assert_eq!(translated.runtime_mode, PrismRuntimeMode::CoordinationOnly);
        assert!(translated.ui);
        assert_eq!(translated.http_bind.as_deref(), Some("127.0.0.1:43123"));
        assert_eq!(
            translated.shared_runtime_uri.as_deref(),
            Some("http://runtime.example")
        );
        assert!(translated.coordination_authority_backend.is_none());
        assert!(translated.coordination_authority_sqlite_db.is_none());
        assert!(translated.coordination_authority_postgres_url.is_none());
    }

    #[test]
    fn service_restart_builds_service_restart_options() {
        let translated = restart_options(ServiceCommand::Restart {
            no_coordination: false,
            internal_developer: false,
            runtime_mode: PrismRuntimeModeArg::Full,
            http_bind: Some("127.0.0.1:43123".to_string()),
            shared_runtime_uri: None,
            kill_bridges: true,
        })
        .expect("expected service restart options");

        assert!(translated.kill_bridges);
        assert!(translated.start.ui);
        assert_eq!(
            translated.start.http_bind.as_deref(),
            Some("127.0.0.1:43123")
        );
        assert!(translated.start.coordination_authority_backend.is_none());
        assert!(translated.start.coordination_authority_sqlite_db.is_none());
        assert!(translated
            .start
            .coordination_authority_postgres_url
            .is_none());
    }

    #[test]
    fn service_backend_defaults_to_sqlite_without_postgres_env() {
        let _guard = ENV_LOCK.get_or_init(|| Mutex::new(())).lock().unwrap();
        let root = temp_root();
        unsafe { std::env::remove_var(PRISM_POSTGRES_DSN_ENV) };
        let backend =
            service_coordination_authority_backend(&root).expect("service backend should resolve");
        match backend {
            CoordinationAuthorityBackendConfig::Sqlite { db_path } => {
                assert!(db_path.ends_with("authority.db"));
            }
            other => panic!("expected sqlite backend, got {other:?}"),
        }
        let _ = fs::remove_dir_all(root);
    }

    #[test]
    fn service_backend_prefers_postgres_env() {
        let _guard = ENV_LOCK.get_or_init(|| Mutex::new(())).lock().unwrap();
        let root = temp_root();
        unsafe { std::env::set_var(PRISM_POSTGRES_DSN_ENV, "postgres://example/prism") };
        let backend =
            service_coordination_authority_backend(&root).expect("service backend should resolve");
        unsafe { std::env::remove_var(PRISM_POSTGRES_DSN_ENV) };
        match backend {
            CoordinationAuthorityBackendConfig::Postgres { connection_url } => {
                assert_eq!(connection_url, "postgres://example/prism");
            }
            other => panic!("expected postgres backend, got {other:?}"),
        }
        let _ = fs::remove_dir_all(root);
    }
}
