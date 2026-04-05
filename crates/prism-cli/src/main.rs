mod auth_commands;
mod cli;
mod commands;
mod daemon_log;
mod display;
mod git_support;
mod mcp;
mod operator_auth;
mod parsing;
mod projection_commands;
mod protected_state_commands;
mod runtime;
mod worktree_commands;
mod workspace_root;

use anyhow::Result;
use clap::Parser;

use crate::cli::Cli;

fn main() -> Result<()> {
    let cli = Cli::parse();
    commands::run(cli)
}
