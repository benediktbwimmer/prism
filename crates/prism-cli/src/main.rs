mod auth_commands;
mod auth_storage;
mod cli;
mod commands;
mod daemon_log;
mod display;
mod mcp;
mod parsing;
mod runtime;

use anyhow::Result;
use clap::Parser;

use crate::cli::Cli;

fn main() -> Result<()> {
    let cli = Cli::parse();
    commands::run(cli)
}
