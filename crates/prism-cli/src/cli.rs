use std::path::PathBuf;

use clap::{Parser, Subcommand, ValueEnum};
use prism_core::PrismRuntimeMode;

use crate::worktree_commands::WorktreeModeArg;

#[derive(Parser)]
#[command(name = "prism")]
#[command(about = "Deterministic local-first code perception")]
pub struct Cli {
    #[arg(
        long,
        help = "Workspace root. When omitted, PRISM resolves the nearest git worktree root from the current directory and falls back to the current directory when no git root is found."
    )]
    pub root: Option<PathBuf>,
    #[command(subcommand)]
    pub command: Command,
}

#[derive(Subcommand)]
pub enum Command {
    Mcp {
        #[command(subcommand)]
        command: McpCommand,
    },
    Runtime {
        #[command(subcommand)]
        command: McpCommand,
    },
    Service {
        #[command(subcommand)]
        command: ServiceCommand,
    },
    Auth {
        #[command(subcommand)]
        command: AuthCommand,
    },
    Worktree {
        #[command(subcommand)]
        command: WorktreeCommand,
    },
    Docs {
        #[command(subcommand)]
        command: DocsCommand,
    },
    Specs {
        #[command(subcommand)]
        command: SpecsCommand,
    },
    Project {
        target: String,
        #[arg(long)]
        at: Option<String>,
        #[arg(long)]
        diff: Option<String>,
    },
    Entrypoints,
    Symbol {
        name: String,
    },
    Lineage {
        name: String,
    },
    CoChange {
        name: String,
        #[arg(long, default_value_t = 8)]
        limit: usize,
    },
    Search {
        query: String,
        #[arg(long, default_value_t = 20)]
        limit: usize,
        #[arg(long)]
        kind: Option<String>,
        #[arg(long)]
        path: Option<String>,
    },
    Relations {
        name: String,
    },
    CallGraph {
        name: String,
        #[arg(long, default_value_t = 3)]
        depth: usize,
    },
    Risk {
        name: String,
    },
    ValidationRecipe {
        name: String,
    },
    TaskResume {
        id: String,
    },
    Feedback {
        #[command(subcommand)]
        command: FeedbackCommand,
    },
    Memory {
        #[command(subcommand)]
        command: MemoryCommand,
    },
    Task {
        #[command(subcommand)]
        command: TaskCommand,
    },
    Outcome {
        #[command(subcommand)]
        command: OutcomeCommand,
    },
    Principal {
        #[command(subcommand)]
        command: PrincipalCommand,
    },
    ProtectedState {
        #[command(subcommand)]
        command: ProtectedStateCommand,
    },
}

#[derive(Subcommand)]
pub enum AuthCommand {
    Bootstrap {
        #[arg(long)]
        name: Option<String>,
        #[arg(long, default_value = "local-daemon")]
        authority: String,
        #[arg(long)]
        role: Option<String>,
        #[arg(long)]
        issuer: String,
        #[arg(long)]
        subject: Option<String>,
        #[arg(long, value_enum)]
        assurance: AuthAssuranceArg,
    },
    Recover {
        #[arg(long)]
        name: Option<String>,
        #[arg(long, default_value = "local-daemon")]
        authority: String,
        #[arg(long)]
        role: Option<String>,
        #[arg(long)]
        issuer: String,
        #[arg(long)]
        subject: Option<String>,
        #[arg(long, value_enum)]
        assurance: AuthAssuranceArg,
    },
    Login {
        #[arg(long)]
        profile: Option<String>,
        #[arg(long)]
        principal: Option<String>,
        #[arg(long)]
        credential: Option<String>,
    },
    Whoami,
}

#[derive(Subcommand)]
pub enum WorktreeCommand {
    List,
    Register {
        #[arg(long)]
        label: Option<String>,
        #[arg(long, value_enum)]
        mode: Option<WorktreeModeArg>,
    },
    Relabel {
        label: Option<String>,
    },
    Takeover {
        #[arg(long)]
        reason: Option<String>,
    },
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, ValueEnum)]
pub enum AuthAssuranceArg {
    High,
    Moderate,
    Legacy,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, ValueEnum)]
#[value(rename_all = "snake_case")]
pub enum PrismRuntimeModeArg {
    Full,
    CoordinationOnly,
    KnowledgeStorage,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, ValueEnum)]
#[value(rename_all = "snake_case")]
pub enum CoordinationAuthorityBackendArg {
    GitSharedRefs,
    Sqlite,
    Postgres,
}

impl From<PrismRuntimeModeArg> for PrismRuntimeMode {
    fn from(value: PrismRuntimeModeArg) -> Self {
        match value {
            PrismRuntimeModeArg::Full => PrismRuntimeMode::Full,
            PrismRuntimeModeArg::CoordinationOnly => PrismRuntimeMode::CoordinationOnly,
            PrismRuntimeModeArg::KnowledgeStorage => PrismRuntimeMode::KnowledgeStorage,
        }
    }
}

#[derive(Subcommand)]
pub enum McpCommand {
    Status,
    Endpoint,
    PublicUrl {
        url: Option<String>,
        #[arg(long, default_value_t = false)]
        clear: bool,
    },
    Cleanup,
    Bridge {
        #[arg(long, default_value_t = false)]
        no_coordination: bool,
        #[arg(long, default_value_t = false)]
        internal_developer: bool,
        #[arg(long = "runtime-mode", value_enum, default_value_t = PrismRuntimeModeArg::Full)]
        runtime_mode: PrismRuntimeModeArg,
        #[arg(long = "shared-runtime-uri")]
        shared_runtime_uri: Option<String>,
        #[arg(long = "coordination-authority-backend", value_enum)]
        coordination_authority_backend: Option<CoordinationAuthorityBackendArg>,
        #[arg(long = "coordination-authority-sqlite-db")]
        coordination_authority_sqlite_db: Option<PathBuf>,
        #[arg(long = "coordination-authority-postgres-url")]
        coordination_authority_postgres_url: Option<String>,
        #[arg(long, hide = true, default_value_t = false)]
        bootstrap_build_worktree_release: bool,
        #[arg(long, hide = true)]
        bridge_daemon_binary: Option<PathBuf>,
    },
    Start {
        #[arg(long, default_value_t = false)]
        no_coordination: bool,
        #[arg(long, default_value_t = false)]
        internal_developer: bool,
        #[arg(long = "runtime-mode", value_enum, default_value_t = PrismRuntimeModeArg::Full)]
        runtime_mode: PrismRuntimeModeArg,
        #[arg(long = "http-bind")]
        http_bind: Option<String>,
        #[arg(long = "shared-runtime-uri")]
        shared_runtime_uri: Option<String>,
        #[arg(long = "coordination-authority-backend", value_enum)]
        coordination_authority_backend: Option<CoordinationAuthorityBackendArg>,
        #[arg(long = "coordination-authority-sqlite-db")]
        coordination_authority_sqlite_db: Option<PathBuf>,
        #[arg(long = "coordination-authority-postgres-url")]
        coordination_authority_postgres_url: Option<String>,
    },
    Stop {
        #[arg(
            long,
            default_value_t = false,
            help = "Also stop bridge processes. By default only the worktree-local runtime is stopped."
        )]
        kill_bridges: bool,
    },
    Restart {
        #[arg(long, default_value_t = false)]
        no_coordination: bool,
        #[arg(long, default_value_t = false)]
        internal_developer: bool,
        #[arg(long = "runtime-mode", value_enum, default_value_t = PrismRuntimeModeArg::Full)]
        runtime_mode: PrismRuntimeModeArg,
        #[arg(long = "http-bind")]
        http_bind: Option<String>,
        #[arg(long = "shared-runtime-uri")]
        shared_runtime_uri: Option<String>,
        #[arg(long = "coordination-authority-backend", value_enum)]
        coordination_authority_backend: Option<CoordinationAuthorityBackendArg>,
        #[arg(long = "coordination-authority-sqlite-db")]
        coordination_authority_sqlite_db: Option<PathBuf>,
        #[arg(long = "coordination-authority-postgres-url")]
        coordination_authority_postgres_url: Option<String>,
        #[arg(
            long,
            default_value_t = false,
            help = "Also stop bridge processes before restarting. By default bridges are preserved."
        )]
        kill_bridges: bool,
    },
    Health,
    Logs {
        #[arg(long, default_value_t = 50)]
        lines: usize,
    },
}

#[derive(Subcommand)]
pub enum ServiceCommand {
    Up {
        #[arg(long, default_value_t = false)]
        no_coordination: bool,
        #[arg(long, default_value_t = false)]
        internal_developer: bool,
        #[arg(long = "runtime-mode", value_enum, default_value_t = PrismRuntimeModeArg::Full)]
        runtime_mode: PrismRuntimeModeArg,
        #[arg(long = "http-bind")]
        http_bind: Option<String>,
        #[arg(long = "shared-runtime-uri")]
        shared_runtime_uri: Option<String>,
    },
    Stop {
        #[arg(
            long,
            default_value_t = false,
            help = "Also stop bridge processes. By default only the service host process is stopped."
        )]
        kill_bridges: bool,
    },
    Restart {
        #[arg(long, default_value_t = false)]
        no_coordination: bool,
        #[arg(long, default_value_t = false)]
        internal_developer: bool,
        #[arg(long = "runtime-mode", value_enum, default_value_t = PrismRuntimeModeArg::Full)]
        runtime_mode: PrismRuntimeModeArg,
        #[arg(long = "http-bind")]
        http_bind: Option<String>,
        #[arg(long = "shared-runtime-uri")]
        shared_runtime_uri: Option<String>,
        #[arg(
            long,
            default_value_t = false,
            help = "Also stop bridge processes before restarting. By default bridges are preserved."
        )]
        kill_bridges: bool,
    },
    Endpoint,
    EnrollRepo,
    Status,
    Health,
}

#[derive(Subcommand)]
pub enum TaskCommand {
    Start {
        id: String,
        #[arg(long)]
        symbol: Option<String>,
        #[arg(long)]
        summary: String,
    },
    Note {
        id: String,
        #[arg(long)]
        symbol: Option<String>,
        #[arg(long)]
        summary: String,
    },
    Patch {
        id: String,
        name: String,
        #[arg(long)]
        summary: Option<String>,
        #[arg(long)]
        staged: bool,
    },
}

#[derive(Subcommand)]
pub enum PrincipalCommand {
    Mint {
        #[arg(long)]
        profile: Option<String>,
        #[arg(long)]
        kind: String,
        #[arg(long)]
        name: String,
        #[arg(long)]
        role: Option<String>,
        #[arg(long)]
        parent: Option<String>,
        #[arg(long)]
        authority: Option<String>,
        #[arg(long = "metadata-json")]
        metadata_json: Option<String>,
        #[arg(long = "capability")]
        capabilities: Vec<String>,
    },
}

#[derive(Subcommand)]
pub enum DocsCommand {
    #[command(alias = "generate")]
    Export {
        #[arg(long = "output-dir")]
        output_dir: PathBuf,
        #[arg(long, value_enum)]
        bundle: Option<DocsBundleArg>,
    },
}

#[derive(Subcommand)]
pub enum SpecsCommand {
    List,
    Show { spec_id: String },
    SyncBrief { spec_id: String },
    Coverage { spec_id: String },
    SyncProvenance { spec_id: String },
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, ValueEnum)]
pub enum DocsBundleArg {
    Zip,
    #[value(name = "tar-gz")]
    TarGz,
}

#[derive(Subcommand)]
pub enum ProtectedStateCommand {
    InstallGitSupport,
    Verify {
        #[arg(long)]
        stream: Option<String>,
    },
    Diagnose {
        #[arg(long)]
        stream: Option<String>,
    },
    MigrateSign,
    Trust {
        #[command(subcommand)]
        command: ProtectedStateTrustCommand,
    },
    Quarantine {
        #[arg(long)]
        stream: String,
    },
    Repair {
        #[arg(long)]
        stream: String,
        #[arg(long, default_value_t = false)]
        to_last_valid: bool,
    },
    #[command(name = "repair-snapshot-artifacts")]
    RepairSnapshotArtifacts,
    #[command(name = "restore-legacy-published-knowledge")]
    RestoreLegacyPublishedKnowledge,
    #[command(name = "repair-path-identity")]
    RepairPathIdentity {
        #[arg(long, default_value_t = false)]
        check: bool,
    },
    ReconcileStream {
        #[arg(long)]
        stream: String,
        #[arg(long = "accepted-head")]
        accepted_head: String,
    },
    #[command(hide = true, name = "merge-driver-stream")]
    MergeDriverStream {
        #[arg(long)]
        ancestor: PathBuf,
        #[arg(long)]
        current: PathBuf,
        #[arg(long)]
        other: PathBuf,
        #[arg(long)]
        path: String,
    },
    #[command(hide = true, name = "merge-driver-derived")]
    MergeDriverDerived {
        #[arg(long)]
        ancestor: PathBuf,
        #[arg(long)]
        current: PathBuf,
        #[arg(long)]
        other: PathBuf,
        #[arg(long)]
        path: String,
    },
    #[command(hide = true, name = "merge-driver-snapshot-derived")]
    MergeDriverSnapshotDerived {
        #[arg(long)]
        ancestor: PathBuf,
        #[arg(long)]
        current: PathBuf,
        #[arg(long)]
        other: PathBuf,
        #[arg(long)]
        path: String,
    },
}

#[derive(Subcommand)]
pub enum ProtectedStateTrustCommand {
    Export {
        #[arg(long = "bundle-id")]
        bundle_id: Option<String>,
        #[arg(long)]
        output: PathBuf,
        #[arg(long = "root-output")]
        root_output: Option<PathBuf>,
    },
    Import {
        #[arg(long)]
        bundle: PathBuf,
        #[arg(long)]
        root: Option<PathBuf>,
    },
}

#[derive(Subcommand)]
pub enum MemoryCommand {
    Recall {
        name: String,
        #[arg(long)]
        text: Option<String>,
        #[arg(long, default_value_t = 5)]
        limit: usize,
        #[arg(long = "kind")]
        kinds: Vec<String>,
    },
    Store {
        name: String,
        #[arg(long)]
        content: String,
        #[arg(long, default_value = "episodic")]
        kind: String,
        #[arg(long, default_value = "local")]
        scope: String,
        #[arg(long = "promoted-from")]
        promoted_from: Vec<String>,
        #[arg(long = "supersedes")]
        supersedes: Vec<String>,
    },
    Events {
        #[arg(long)]
        name: Option<String>,
        #[arg(long)]
        text: Option<String>,
        #[arg(long, default_value_t = 10)]
        limit: usize,
        #[arg(long = "kind")]
        kinds: Vec<String>,
        #[arg(long = "action")]
        actions: Vec<String>,
        #[arg(long)]
        scope: Option<String>,
        #[arg(long = "task-id")]
        task_id: Option<String>,
        #[arg(long)]
        since: Option<u64>,
    },
}

#[cfg(test)]
mod tests {
    use std::path::PathBuf;

    use clap::Parser;

    use super::{
        AuthAssuranceArg, AuthCommand, Cli, Command, CoordinationAuthorityBackendArg,
        DocsBundleArg, DocsCommand, McpCommand, PrincipalCommand, PrismRuntimeModeArg,
        ProtectedStateCommand, ProtectedStateTrustCommand, ServiceCommand, SpecsCommand,
        WorktreeCommand,
    };
    use crate::worktree_commands::WorktreeModeArg;

    #[test]
    fn mcp_restart_preserves_bridges_by_default() {
        let cli = Cli::parse_from(["prism", "mcp", "restart"]);
        assert!(cli.root.is_none());
        match cli.command {
            Command::Mcp {
                command:
                    McpCommand::Restart {
                        kill_bridges,
                        no_coordination,
                        internal_developer,
                        runtime_mode,
                        http_bind,
                        shared_runtime_uri,
                        coordination_authority_backend,
                        coordination_authority_sqlite_db,
                        coordination_authority_postgres_url,
                        ..
                    },
            } => {
                assert!(!kill_bridges);
                assert!(!no_coordination);
                assert!(!internal_developer);
                assert_eq!(runtime_mode, PrismRuntimeModeArg::Full);
                assert!(http_bind.is_none());
                assert!(shared_runtime_uri.is_none());
                assert!(coordination_authority_backend.is_none());
                assert!(coordination_authority_sqlite_db.is_none());
                assert!(coordination_authority_postgres_url.is_none());
            }
            _ => panic!("unexpected command"),
        }
    }

    #[test]
    fn service_restart_preserves_bridges_by_default() {
        let cli = Cli::parse_from(["prism", "service", "restart"]);
        assert!(cli.root.is_none());
        match cli.command {
            Command::Service {
                command:
                    ServiceCommand::Restart {
                        kill_bridges,
                        no_coordination,
                        internal_developer,
                        runtime_mode,
                        http_bind,
                        shared_runtime_uri,
                    },
            } => {
                assert!(!kill_bridges);
                assert!(!no_coordination);
                assert!(!internal_developer);
                assert_eq!(runtime_mode, PrismRuntimeModeArg::Full);
                assert!(http_bind.is_none());
                assert!(shared_runtime_uri.is_none());
            }
            _ => panic!("unexpected command"),
        }
    }

    #[test]
    fn service_up_rejects_authority_backend_flags() {
        assert!(Cli::try_parse_from([
            "prism",
            "service",
            "up",
            "--coordination-authority-backend",
            "sqlite",
        ])
        .is_err());
        assert!(Cli::try_parse_from([
            "prism",
            "service",
            "up",
            "--coordination-authority-postgres-url",
            "postgres://example",
        ])
        .is_err());
    }

    #[test]
    fn service_status_parses() {
        let cli = Cli::parse_from(["prism", "service", "status"]);
        assert!(cli.root.is_none());
        match cli.command {
            Command::Service {
                command: ServiceCommand::Status,
            } => {}
            _ => panic!("unexpected command"),
        }
    }

    #[test]
    fn service_endpoint_parses() {
        let cli = Cli::parse_from(["prism", "service", "endpoint"]);
        assert!(cli.root.is_none());
        match cli.command {
            Command::Service {
                command: ServiceCommand::Endpoint,
            } => {}
            _ => panic!("unexpected command"),
        }
    }

    #[test]
    fn service_enroll_repo_parses() {
        let cli = Cli::parse_from(["prism", "service", "enroll-repo"]);
        assert!(cli.root.is_none());
        match cli.command {
            Command::Service {
                command: ServiceCommand::EnrollRepo,
            } => {}
            _ => panic!("unexpected command"),
        }
    }

    #[test]
    fn service_up_rejects_no_ui_flag() {
        assert!(Cli::try_parse_from(["prism", "service", "up", "--no-ui"]).is_err());
    }

    #[test]
    fn mcp_stop_preserves_bridges_by_default() {
        let cli = Cli::parse_from(["prism", "mcp", "stop"]);
        assert!(cli.root.is_none());
        match cli.command {
            Command::Mcp {
                command: McpCommand::Stop { kill_bridges },
            } => assert!(!kill_bridges),
            _ => panic!("unexpected command"),
        }
    }

    #[test]
    fn runtime_stop_preserves_bridges_by_default() {
        let cli = Cli::parse_from(["prism", "runtime", "stop"]);
        assert!(cli.root.is_none());
        match cli.command {
            Command::Runtime {
                command: McpCommand::Stop { kill_bridges },
            } => assert!(!kill_bridges),
            _ => panic!("unexpected command"),
        }
    }

    #[test]
    fn mcp_public_url_parses_value() {
        let cli = Cli::parse_from(["prism", "mcp", "public-url", "https://runtime.example"]);
        match cli.command {
            Command::Mcp {
                command: McpCommand::PublicUrl { url, clear },
            } => {
                assert_eq!(url.as_deref(), Some("https://runtime.example"));
                assert!(!clear);
            }
            _ => panic!("unexpected command"),
        }
    }

    #[test]
    fn mcp_public_url_parses_clear() {
        let cli = Cli::parse_from(["prism", "mcp", "public-url", "--clear"]);
        match cli.command {
            Command::Mcp {
                command: McpCommand::PublicUrl { url, clear },
            } => {
                assert!(url.is_none());
                assert!(clear);
            }
            _ => panic!("unexpected command"),
        }
    }

    #[test]
    fn mcp_restart_kill_bridges_flag_is_opt_in() {
        let cli = Cli::parse_from(["prism", "mcp", "restart", "--kill-bridges"]);
        assert!(cli.root.is_none());
        match cli.command {
            Command::Mcp {
                command: McpCommand::Restart { kill_bridges, .. },
            } => assert!(kill_bridges),
            _ => panic!("unexpected command"),
        }
    }

    #[test]
    fn mcp_restart_internal_developer_flag_is_opt_in() {
        let cli = Cli::parse_from(["prism", "mcp", "restart", "--internal-developer"]);
        assert!(cli.root.is_none());
        match cli.command {
            Command::Mcp {
                command:
                    McpCommand::Restart {
                        internal_developer, ..
                    },
            } => assert!(internal_developer),
            _ => panic!("unexpected command"),
        }
    }

    #[test]
    fn mcp_restart_rejects_ui_flag() {
        assert!(Cli::try_parse_from(["prism", "mcp", "restart", "--ui"]).is_err());
    }

    #[test]
    fn mcp_start_accepts_http_bind_override() {
        let cli = Cli::parse_from(["prism", "mcp", "start", "--http-bind", "127.0.0.1:43123"]);
        assert!(cli.root.is_none());
        match cli.command {
            Command::Mcp {
                command: McpCommand::Start { http_bind, .. },
            } => assert_eq!(http_bind.as_deref(), Some("127.0.0.1:43123")),
            _ => panic!("unexpected command"),
        }
    }

    #[test]
    fn mcp_cleanup_parses() {
        let cli = Cli::parse_from(["prism", "mcp", "cleanup"]);
        assert!(cli.root.is_none());
        match cli.command {
            Command::Mcp {
                command: McpCommand::Cleanup,
            } => {}
            _ => panic!("unexpected command"),
        }
    }

    #[test]
    fn mcp_endpoint_parses() {
        let cli = Cli::parse_from(["prism", "mcp", "endpoint"]);
        assert!(cli.root.is_none());
        match cli.command {
            Command::Mcp {
                command: McpCommand::Endpoint,
            } => {}
            _ => panic!("unexpected command"),
        }
    }

    #[test]
    fn docs_export_parses() {
        let cli = Cli::parse_from(["prism", "docs", "export", "--output-dir", "out/prism"]);
        assert!(cli.root.is_none());
        match cli.command {
            Command::Docs {
                command: DocsCommand::Export { output_dir, bundle },
            } => {
                assert_eq!(output_dir, PathBuf::from("out/prism"));
                assert_eq!(bundle, None);
            }
            _ => panic!("unexpected command"),
        }
    }

    #[test]
    fn mcp_bridge_rejects_ui_flag() {
        assert!(Cli::try_parse_from(["prism", "mcp", "bridge", "--ui"]).is_err());
    }

    #[test]
    fn mcp_restart_runtime_mode_flag_parses() {
        let cli = Cli::parse_from([
            "prism",
            "mcp",
            "restart",
            "--runtime-mode",
            "coordination_only",
        ]);
        match cli.command {
            Command::Mcp {
                command: McpCommand::Restart { runtime_mode, .. },
            } => assert_eq!(runtime_mode, PrismRuntimeModeArg::CoordinationOnly),
            _ => panic!("unexpected command"),
        }
    }

    #[test]
    fn mcp_start_coordination_authority_backend_flags_parse() {
        let cli = Cli::parse_from([
            "prism",
            "mcp",
            "start",
            "--coordination-authority-backend",
            "sqlite",
            "--coordination-authority-sqlite-db",
            "service-authority.db",
        ]);
        match cli.command {
            Command::Mcp {
                command:
                    McpCommand::Start {
                        coordination_authority_backend,
                        coordination_authority_sqlite_db,
                        coordination_authority_postgres_url,
                        ..
                    },
            } => {
                assert_eq!(
                    coordination_authority_backend,
                    Some(CoordinationAuthorityBackendArg::Sqlite)
                );
                assert_eq!(
                    coordination_authority_sqlite_db,
                    Some(PathBuf::from("service-authority.db"))
                );
                assert!(coordination_authority_postgres_url.is_none());
            }
            _ => panic!("unexpected command"),
        }
    }

    #[test]
    fn mcp_start_rejects_ui_flag() {
        assert!(Cli::try_parse_from(["prism", "mcp", "start", "--ui"]).is_err());
    }

    #[test]
    fn docs_export_with_bundle_parses() {
        let cli = Cli::parse_from([
            "prism",
            "docs",
            "export",
            "--output-dir",
            "out/prism",
            "--bundle",
            "tar-gz",
        ]);
        assert!(cli.root.is_none());
        match cli.command {
            Command::Docs {
                command: DocsCommand::Export { output_dir, bundle },
            } => {
                assert_eq!(output_dir, PathBuf::from("out/prism"));
                assert_eq!(bundle, Some(DocsBundleArg::TarGz));
            }
            _ => panic!("unexpected command"),
        }
    }

    #[test]
    fn specs_list_parses() {
        let cli = Cli::parse_from(["prism", "specs", "list"]);
        assert!(cli.root.is_none());
        match cli.command {
            Command::Specs {
                command: SpecsCommand::List,
            } => {}
            _ => panic!("unexpected command"),
        }
    }

    #[test]
    fn specs_show_parses() {
        let cli = Cli::parse_from(["prism", "specs", "show", "spec:alpha"]);
        assert!(cli.root.is_none());
        match cli.command {
            Command::Specs {
                command: SpecsCommand::Show { spec_id },
            } => assert_eq!(spec_id, "spec:alpha"),
            _ => panic!("unexpected command"),
        }
    }

    #[test]
    fn specs_sync_brief_parses() {
        let cli = Cli::parse_from(["prism", "specs", "sync-brief", "spec:alpha"]);
        assert!(cli.root.is_none());
        match cli.command {
            Command::Specs {
                command: SpecsCommand::SyncBrief { spec_id },
            } => assert_eq!(spec_id, "spec:alpha"),
            _ => panic!("unexpected command"),
        }
    }

    #[test]
    fn auth_bootstrap_parses() {
        let cli = Cli::parse_from([
            "prism",
            "auth",
            "bootstrap",
            "--issuer",
            "github-device-flow",
            "--assurance",
            "high",
        ]);
        assert!(cli.root.is_none());
        match cli.command {
            Command::Auth {
                command:
                    AuthCommand::Bootstrap {
                        name,
                        authority,
                        role,
                        issuer,
                        subject,
                        assurance,
                    },
            } => {
                assert!(name.is_none());
                assert_eq!(authority, "local-daemon");
                assert!(role.is_none());
                assert_eq!(issuer, "github-device-flow");
                assert!(subject.is_none());
                assert_eq!(assurance, AuthAssuranceArg::High);
            }
            _ => panic!("unexpected command"),
        }
    }

    #[test]
    fn auth_recover_parses() {
        let cli = Cli::parse_from([
            "prism",
            "auth",
            "recover",
            "--name",
            "Bene",
            "--issuer",
            "ssh-signature",
            "--subject",
            "bene@laptop",
            "--assurance",
            "moderate",
        ]);
        assert!(cli.root.is_none());
        match cli.command {
            Command::Auth {
                command:
                    AuthCommand::Recover {
                        name,
                        issuer,
                        subject,
                        assurance,
                        ..
                    },
            } => {
                assert_eq!(name.as_deref(), Some("Bene"));
                assert_eq!(issuer, "ssh-signature");
                assert_eq!(subject.as_deref(), Some("bene@laptop"));
                assert_eq!(assurance, AuthAssuranceArg::Moderate);
            }
            _ => panic!("unexpected command"),
        }
    }

    #[test]
    fn auth_login_parses_principal_selector() {
        let cli = Cli::parse_from(["prism", "auth", "login", "--principal", "principal:owner"]);
        assert!(cli.root.is_none());
        match cli.command {
            Command::Auth {
                command:
                    AuthCommand::Login {
                        profile,
                        principal,
                        credential,
                    },
            } => {
                assert_eq!(principal.as_deref(), Some("principal:owner"));
                assert!(profile.is_none());
                assert!(credential.is_none());
            }
            _ => panic!("unexpected command"),
        }
    }

    #[test]
    fn auth_whoami_parses() {
        let cli = Cli::parse_from(["prism", "auth", "whoami"]);
        assert!(cli.root.is_none());
        match cli.command {
            Command::Auth {
                command: AuthCommand::Whoami,
            } => {}
            _ => panic!("unexpected command"),
        }
    }

    #[test]
    fn worktree_register_parses() {
        let cli = Cli::parse_from([
            "prism", "worktree", "register", "--label", "codex-d", "--mode", "agent",
        ]);
        assert!(cli.root.is_none());
        match cli.command {
            Command::Worktree {
                command: WorktreeCommand::Register { label, mode },
            } => {
                assert_eq!(label.as_deref(), Some("codex-d"));
                assert_eq!(mode, Some(WorktreeModeArg::Agent));
            }
            _ => panic!("unexpected command"),
        }
    }

    #[test]
    fn worktree_relabel_parses() {
        let cli = Cli::parse_from(["prism", "worktree", "relabel", "operator-a"]);
        assert!(cli.root.is_none());
        match cli.command {
            Command::Worktree {
                command: WorktreeCommand::Relabel { label },
            } => {
                assert_eq!(label.as_deref(), Some("operator-a"));
            }
            _ => panic!("unexpected command"),
        }
    }

    #[test]
    fn worktree_takeover_parses() {
        let cli = Cli::parse_from(["prism", "worktree", "takeover", "--reason", "stuck bridge"]);
        assert!(cli.root.is_none());
        match cli.command {
            Command::Worktree {
                command: WorktreeCommand::Takeover { reason },
            } => assert_eq!(reason.as_deref(), Some("stuck bridge")),
            _ => panic!("unexpected command"),
        }
    }

    #[test]
    fn principal_mint_parses_capabilities_and_parent() {
        let cli = Cli::parse_from([
            "prism",
            "principal",
            "mint",
            "--kind",
            "agent",
            "--name",
            "Worker",
            "--parent",
            "principal:owner",
            "--capability",
            "mutate_coordination",
            "--capability",
            "mutate_repo_memory",
        ]);
        assert!(cli.root.is_none());
        match cli.command {
            Command::Principal {
                command:
                    PrincipalCommand::Mint {
                        kind,
                        name,
                        parent,
                        capabilities,
                        ..
                    },
            } => {
                assert_eq!(kind, "agent");
                assert_eq!(name, "Worker");
                assert_eq!(parent.as_deref(), Some("principal:owner"));
                assert_eq!(capabilities.len(), 2);
            }
            _ => panic!("unexpected command"),
        }
    }

    #[test]
    fn protected_state_migrate_sign_parses() {
        let cli = Cli::parse_from(["prism", "protected-state", "migrate-sign"]);
        assert!(cli.root.is_none());
        match cli.command {
            Command::ProtectedState {
                command: ProtectedStateCommand::MigrateSign,
            } => {}
            _ => panic!("unexpected command"),
        }
    }

    #[test]
    fn protected_state_install_git_support_parses() {
        let cli = Cli::parse_from(["prism", "protected-state", "install-git-support"]);
        assert!(cli.root.is_none());
        match cli.command {
            Command::ProtectedState {
                command: ProtectedStateCommand::InstallGitSupport,
            } => {}
            _ => panic!("unexpected command"),
        }
    }

    #[test]
    fn protected_state_repair_snapshot_artifacts_parses() {
        let cli = Cli::parse_from(["prism", "protected-state", "repair-snapshot-artifacts"]);
        assert!(cli.root.is_none());
        match cli.command {
            Command::ProtectedState {
                command: ProtectedStateCommand::RepairSnapshotArtifacts,
            } => {}
            _ => panic!("unexpected command"),
        }
    }

    #[test]
    fn protected_state_restore_legacy_published_knowledge_parses() {
        let cli = Cli::parse_from([
            "prism",
            "protected-state",
            "restore-legacy-published-knowledge",
        ]);
        assert!(cli.root.is_none());
        match cli.command {
            Command::ProtectedState {
                command: ProtectedStateCommand::RestoreLegacyPublishedKnowledge,
            } => {}
            _ => panic!("unexpected command"),
        }
    }

    #[test]
    fn protected_state_verify_accepts_optional_stream() {
        let cli = Cli::parse_from([
            "prism",
            "protected-state",
            "verify",
            "--stream",
            "concepts:events",
        ]);
        assert!(cli.root.is_none());
        match cli.command {
            Command::ProtectedState {
                command: ProtectedStateCommand::Verify { stream },
            } => assert_eq!(stream.as_deref(), Some("concepts:events")),
            _ => panic!("unexpected command"),
        }
    }

    #[test]
    fn hidden_merge_driver_stream_parses() {
        let cli = Cli::parse_from([
            "prism",
            "protected-state",
            "merge-driver-stream",
            "--ancestor",
            "base.jsonl",
            "--current",
            "current.jsonl",
            "--other",
            "other.jsonl",
            "--path",
            ".prism/concepts/events.jsonl",
        ]);
        match cli.command {
            Command::ProtectedState {
                command:
                    ProtectedStateCommand::MergeDriverStream {
                        ancestor,
                        current,
                        other,
                        path,
                    },
            } => {
                assert_eq!(ancestor, PathBuf::from("base.jsonl"));
                assert_eq!(current, PathBuf::from("current.jsonl"));
                assert_eq!(other, PathBuf::from("other.jsonl"));
                assert_eq!(path, ".prism/concepts/events.jsonl");
            }
            _ => panic!("unexpected command"),
        }
    }

    #[test]
    fn hidden_merge_driver_snapshot_derived_parses() {
        let cli = Cli::parse_from([
            "prism",
            "protected-state",
            "merge-driver-snapshot-derived",
            "--ancestor",
            "base.json",
            "--current",
            "current.json",
            "--other",
            "other.json",
            "--path",
            ".prism/state/manifest.json",
        ]);
        match cli.command {
            Command::ProtectedState {
                command:
                    ProtectedStateCommand::MergeDriverSnapshotDerived {
                        ancestor,
                        current,
                        other,
                        path,
                    },
            } => {
                assert_eq!(ancestor, PathBuf::from("base.json"));
                assert_eq!(current, PathBuf::from("current.json"));
                assert_eq!(other, PathBuf::from("other.json"));
                assert_eq!(path, ".prism/state/manifest.json");
            }
            _ => panic!("unexpected command"),
        }
    }

    #[test]
    fn protected_state_trust_export_parses() {
        let cli = Cli::parse_from([
            "prism",
            "protected-state",
            "trust",
            "export",
            "--bundle-id",
            "trust-bundle:test",
            "--output",
            "bundle.json",
        ]);
        assert!(cli.root.is_none());
        match cli.command {
            Command::ProtectedState {
                command:
                    ProtectedStateCommand::Trust {
                        command:
                            ProtectedStateTrustCommand::Export {
                                bundle_id,
                                output,
                                root_output,
                            },
                    },
            } => {
                assert_eq!(bundle_id.as_deref(), Some("trust-bundle:test"));
                assert_eq!(output, PathBuf::from("bundle.json"));
                assert!(root_output.is_none());
            }
            _ => panic!("unexpected command"),
        }
    }

    #[test]
    fn protected_state_repair_to_last_valid_is_opt_in() {
        let cli = Cli::parse_from([
            "prism",
            "protected-state",
            "repair",
            "--stream",
            "concepts:events",
            "--to-last-valid",
        ]);
        assert!(cli.root.is_none());
        match cli.command {
            Command::ProtectedState {
                command:
                    ProtectedStateCommand::Repair {
                        stream,
                        to_last_valid,
                    },
            } => {
                assert_eq!(stream, "concepts:events");
                assert!(to_last_valid);
            }
            _ => panic!("unexpected command"),
        }
    }

    #[test]
    fn protected_state_repair_path_identity_accepts_check_mode() {
        let cli = Cli::parse_from([
            "prism",
            "protected-state",
            "repair-path-identity",
            "--check",
        ]);
        assert!(cli.root.is_none());
        match cli.command {
            Command::ProtectedState {
                command: ProtectedStateCommand::RepairPathIdentity { check },
            } => assert!(check),
            _ => panic!("unexpected command"),
        }
    }

    #[test]
    fn explicit_root_still_parses() {
        let cli = Cli::parse_from(["prism", "--root", "worktree", "mcp", "status"]);
        assert_eq!(cli.root, Some(PathBuf::from("worktree")));
        match cli.command {
            Command::Mcp {
                command: McpCommand::Status,
            } => {}
            _ => panic!("unexpected command"),
        }
    }
}

#[derive(Subcommand)]
pub enum FeedbackCommand {
    Record {
        #[arg(long)]
        context: String,
        #[arg(long = "prism-said")]
        prism_said: String,
        #[arg(long = "actually-true")]
        actually_true: String,
        #[arg(long)]
        category: String,
        #[arg(long)]
        verdict: String,
        #[arg(long = "task-id")]
        task_id: Option<String>,
        #[arg(long = "symbol")]
        symbols: Vec<String>,
        #[arg(long)]
        corrected_manually: bool,
        #[arg(long)]
        correction: Option<String>,
    },
    List {
        #[arg(long, default_value_t = 20)]
        limit: usize,
    },
}

#[derive(Subcommand)]
pub enum OutcomeCommand {
    Record {
        name: String,
        #[arg(long)]
        kind: String,
        #[arg(long)]
        result: String,
        #[arg(long)]
        summary: String,
        #[arg(long)]
        task: Option<String>,
        #[arg(long = "test")]
        tests: Vec<String>,
        #[arg(long = "failing-test")]
        failing_tests: Vec<String>,
        #[arg(long = "build")]
        builds: Vec<String>,
        #[arg(long = "failing-build")]
        failing_builds: Vec<String>,
        #[arg(long = "issue")]
        issues: Vec<String>,
        #[arg(long = "commit")]
        commits: Vec<String>,
    },
    Test {
        name: String,
        #[arg(long)]
        task: Option<String>,
        #[arg(long)]
        label: Option<String>,
        #[arg(long)]
        summary: Option<String>,
        #[arg(trailing_var_arg = true, allow_hyphen_values = true, required = true)]
        command: Vec<String>,
    },
    Build {
        name: String,
        #[arg(long)]
        task: Option<String>,
        #[arg(long)]
        label: Option<String>,
        #[arg(long)]
        summary: Option<String>,
        #[arg(trailing_var_arg = true, allow_hyphen_values = true, required = true)]
        command: Vec<String>,
    },
}
