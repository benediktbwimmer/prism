mod auth_commands;
mod cli;
mod commands;
mod daemon_log;
mod display;
mod git_support;
mod github_attestation;
mod mcp;
mod operator_auth;
mod parsing;
mod projection_commands;
mod protected_state_commands;
mod runtime;
mod service;
mod spec_commands;
mod workspace_root;
mod worktree_commands;

use anyhow::Result;
use clap::Parser;

use crate::cli::Cli;

fn main() -> Result<()> {
    let cli = Cli::parse();
    commands::run(cli)
}
