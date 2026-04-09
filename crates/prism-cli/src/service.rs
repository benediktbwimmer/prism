use std::path::Path;

use anyhow::Result;

use crate::cli::{McpCommand, ServiceCommand};
use crate::mcp;

pub(crate) fn handle(root: &Path, command: ServiceCommand) -> Result<()> {
    mcp::handle(root, translate(command))
}

fn translate(command: ServiceCommand) -> McpCommand {
    match command {
        ServiceCommand::Up {
            no_coordination,
            internal_developer,
            runtime_mode,
            ui,
            http_bind,
            shared_runtime_uri,
            coordination_authority_backend,
            coordination_authority_sqlite_db,
            coordination_authority_postgres_url,
        } => McpCommand::Start {
            no_coordination,
            internal_developer,
            runtime_mode,
            ui,
            http_bind,
            shared_runtime_uri,
            coordination_authority_backend,
            coordination_authority_sqlite_db,
            coordination_authority_postgres_url,
        },
        ServiceCommand::Stop { kill_bridges } => McpCommand::Stop { kill_bridges },
        ServiceCommand::Restart {
            no_coordination,
            internal_developer,
            runtime_mode,
            ui,
            http_bind,
            shared_runtime_uri,
            coordination_authority_backend,
            coordination_authority_sqlite_db,
            coordination_authority_postgres_url,
            kill_bridges,
        } => McpCommand::Restart {
            no_coordination,
            internal_developer,
            runtime_mode,
            ui,
            http_bind,
            shared_runtime_uri,
            coordination_authority_backend,
            coordination_authority_sqlite_db,
            coordination_authority_postgres_url,
            kill_bridges,
        },
        ServiceCommand::Status => McpCommand::Status,
        ServiceCommand::Health => McpCommand::Health,
    }
}

#[cfg(test)]
mod tests {
    use std::path::PathBuf;

    use crate::cli::{CoordinationAuthorityBackendArg, McpCommand, PrismRuntimeModeArg, ServiceCommand};

    use super::translate;

    #[test]
    fn service_up_translates_to_mcp_start() {
        let translated = translate(ServiceCommand::Up {
            no_coordination: false,
            internal_developer: true,
            runtime_mode: PrismRuntimeModeArg::CoordinationOnly,
            ui: true,
            http_bind: Some("127.0.0.1:43123".to_string()),
            shared_runtime_uri: Some("http://runtime.example".to_string()),
            coordination_authority_backend: Some(CoordinationAuthorityBackendArg::Postgres),
            coordination_authority_sqlite_db: Some(PathBuf::from("service-authority.db")),
            coordination_authority_postgres_url: Some("postgres://example".to_string()),
        });

        match translated {
            McpCommand::Start {
                internal_developer,
                runtime_mode,
                ui,
                http_bind,
                shared_runtime_uri,
                coordination_authority_backend,
                coordination_authority_sqlite_db,
                coordination_authority_postgres_url,
                ..
            } => {
                assert!(internal_developer);
                assert_eq!(runtime_mode, PrismRuntimeModeArg::CoordinationOnly);
                assert!(ui);
                assert_eq!(http_bind.as_deref(), Some("127.0.0.1:43123"));
                assert_eq!(shared_runtime_uri.as_deref(), Some("http://runtime.example"));
                assert_eq!(
                    coordination_authority_backend,
                    Some(CoordinationAuthorityBackendArg::Postgres)
                );
                assert_eq!(
                    coordination_authority_sqlite_db,
                    Some(PathBuf::from("service-authority.db"))
                );
                assert_eq!(
                    coordination_authority_postgres_url.as_deref(),
                    Some("postgres://example")
                );
            }
            _ => panic!("unexpected command"),
        }
    }
}
