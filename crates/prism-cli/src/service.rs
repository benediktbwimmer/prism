use std::path::Path;

use anyhow::Result;

use crate::cli::{McpCommand, ServiceCommand};
use crate::mcp::{self, DaemonRestartOptions, DaemonStartOptions};
use crate::service_state;

pub(crate) fn handle(root: &Path, command: ServiceCommand) -> Result<()> {
    match command {
        ServiceCommand::Up {
            no_coordination,
            internal_developer,
            runtime_mode,
            http_bind,
            shared_runtime_uri,
            coordination_authority_backend,
            coordination_authority_sqlite_db,
            coordination_authority_postgres_url,
        } => {
            mcp::start_with_options(
                root,
                DaemonStartOptions {
                    no_coordination,
                    internal_developer,
                    runtime_mode: runtime_mode.into(),
                    ui: true,
                    http_bind,
                    shared_runtime_uri,
                    coordination_authority_backend,
                    coordination_authority_sqlite_db,
                    coordination_authority_postgres_url,
                },
            )?;
            service_state::sync_local_service_endpoint(root)
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
            coordination_authority_backend,
            coordination_authority_sqlite_db,
            coordination_authority_postgres_url,
            kill_bridges,
        } => {
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
                        coordination_authority_backend,
                        coordination_authority_sqlite_db,
                        coordination_authority_postgres_url,
                    },
                },
            )?;
            service_state::sync_local_service_endpoint(root)
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
        ServiceCommand::Status => mcp::handle(root, McpCommand::Status),
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
            coordination_authority_backend,
            coordination_authority_sqlite_db,
            coordination_authority_postgres_url,
        } => Some(DaemonStartOptions {
            no_coordination,
            internal_developer,
            runtime_mode: runtime_mode.into(),
            ui: true,
            http_bind,
            shared_runtime_uri,
            coordination_authority_backend,
            coordination_authority_sqlite_db,
            coordination_authority_postgres_url,
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
            coordination_authority_backend,
            coordination_authority_sqlite_db,
            coordination_authority_postgres_url,
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
                coordination_authority_backend,
                coordination_authority_sqlite_db,
                coordination_authority_postgres_url,
            },
        }),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use std::path::PathBuf;

    use prism_core::PrismRuntimeMode;

    use crate::cli::{CoordinationAuthorityBackendArg, PrismRuntimeModeArg, ServiceCommand};

    use super::{restart_options, start_options};

    #[test]
    fn service_up_builds_service_start_options() {
        let translated = start_options(ServiceCommand::Up {
            no_coordination: false,
            internal_developer: true,
            runtime_mode: crate::cli::PrismRuntimeModeArg::CoordinationOnly,
            http_bind: Some("127.0.0.1:43123".to_string()),
            shared_runtime_uri: Some("http://runtime.example".to_string()),
            coordination_authority_backend: Some(CoordinationAuthorityBackendArg::Postgres),
            coordination_authority_sqlite_db: Some(PathBuf::from("service-authority.db")),
            coordination_authority_postgres_url: Some("postgres://example".to_string()),
        })
        .expect("expected service start options");

        assert!(translated.internal_developer);
        assert_eq!(translated.runtime_mode, PrismRuntimeMode::CoordinationOnly);
        assert!(translated.ui);
        assert_eq!(translated.http_bind.as_deref(), Some("127.0.0.1:43123"));
        assert_eq!(translated.shared_runtime_uri.as_deref(), Some("http://runtime.example"));
        assert_eq!(
            translated.coordination_authority_backend,
            Some(CoordinationAuthorityBackendArg::Postgres)
        );
        assert_eq!(
            translated.coordination_authority_sqlite_db,
            Some(PathBuf::from("service-authority.db"))
        );
        assert_eq!(
            translated.coordination_authority_postgres_url.as_deref(),
            Some("postgres://example")
        );
    }

    #[test]
    fn service_restart_builds_service_restart_options() {
        let translated = restart_options(ServiceCommand::Restart {
            no_coordination: false,
            internal_developer: false,
            runtime_mode: PrismRuntimeModeArg::Full,
            http_bind: Some("127.0.0.1:43123".to_string()),
            shared_runtime_uri: None,
            coordination_authority_backend: Some(CoordinationAuthorityBackendArg::Sqlite),
            coordination_authority_sqlite_db: Some(PathBuf::from("service-authority.db")),
            coordination_authority_postgres_url: None,
            kill_bridges: true,
        })
        .expect("expected service restart options");

        assert!(translated.kill_bridges);
        assert!(translated.start.ui);
        assert_eq!(translated.start.http_bind.as_deref(), Some("127.0.0.1:43123"));
        assert_eq!(
            translated.start.coordination_authority_backend,
            Some(CoordinationAuthorityBackendArg::Sqlite)
        );
    }
}
