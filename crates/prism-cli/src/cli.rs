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
    },
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
