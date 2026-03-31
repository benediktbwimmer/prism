use std::path::PathBuf;

use clap::{Parser, Subcommand};

#[derive(Parser)]
#[command(name = "prism")]
#[command(about = "Deterministic local-first code perception")]
pub struct Cli {
    #[arg(long, default_value = ".")]
    pub root: PathBuf,
    #[command(subcommand)]
    pub command: Command,
}

#[derive(Subcommand)]
pub enum Command {
    Mcp {
        #[command(subcommand)]
        command: McpCommand,
    },
    Docs {
        #[command(subcommand)]
        command: DocsCommand,
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
}

#[derive(Subcommand)]
pub enum McpCommand {
    Status,
    Endpoint,
    Cleanup,
    Start {
        #[arg(long, default_value_t = false)]
        no_coordination: bool,
        #[arg(long, default_value_t = false)]
        internal_developer: bool,
        #[arg(long = "http-bind")]
        http_bind: Option<String>,
        #[arg(long = "shared-runtime-sqlite")]
        shared_runtime_sqlite: Option<PathBuf>,
        #[arg(long = "shared-runtime-uri")]
        shared_runtime_uri: Option<String>,
    },
    Stop {
        #[arg(
            long,
            default_value_t = false,
            help = "Also stop bridge processes. By default only the daemon is stopped."
        )]
        kill_bridges: bool,
    },
    Restart {
        #[arg(long, default_value_t = false)]
        no_coordination: bool,
        #[arg(long, default_value_t = false)]
        internal_developer: bool,
        #[arg(long = "http-bind")]
        http_bind: Option<String>,
        #[arg(long = "shared-runtime-sqlite")]
        shared_runtime_sqlite: Option<PathBuf>,
        #[arg(long = "shared-runtime-uri")]
        shared_runtime_uri: Option<String>,
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
pub enum DocsCommand {
    Generate,
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
    use clap::Parser;

    use super::{Cli, Command, DocsCommand, McpCommand};

    #[test]
    fn mcp_restart_preserves_bridges_by_default() {
        let cli = Cli::parse_from(["prism", "mcp", "restart"]);
        match cli.command {
            Command::Mcp {
                command:
                    McpCommand::Restart {
                        kill_bridges,
                        no_coordination,
                        internal_developer,
                        http_bind,
                        shared_runtime_sqlite,
                        shared_runtime_uri,
                    },
            } => {
                assert!(!kill_bridges);
                assert!(!no_coordination);
                assert!(!internal_developer);
                assert!(http_bind.is_none());
                assert!(shared_runtime_sqlite.is_none());
                assert!(shared_runtime_uri.is_none());
            }
            _ => panic!("unexpected command"),
        }
    }

    #[test]
    fn mcp_stop_preserves_bridges_by_default() {
        let cli = Cli::parse_from(["prism", "mcp", "stop"]);
        match cli.command {
            Command::Mcp {
                command: McpCommand::Stop { kill_bridges },
            } => assert!(!kill_bridges),
            _ => panic!("unexpected command"),
        }
    }

    #[test]
    fn mcp_restart_kill_bridges_flag_is_opt_in() {
        let cli = Cli::parse_from(["prism", "mcp", "restart", "--kill-bridges"]);
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
    fn mcp_start_accepts_http_bind_override() {
        let cli = Cli::parse_from(["prism", "mcp", "start", "--http-bind", "127.0.0.1:43123"]);
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
        match cli.command {
            Command::Mcp {
                command: McpCommand::Endpoint,
            } => {}
            _ => panic!("unexpected command"),
        }
    }

    #[test]
    fn docs_generate_parses() {
        let cli = Cli::parse_from(["prism", "docs", "generate"]);
        match cli.command {
            Command::Docs {
                command: DocsCommand::Generate,
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
