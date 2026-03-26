mod cli;
mod commands;
mod display;
mod parsing;
mod runtime;

use anyhow::Result;
use clap::Parser;

use crate::cli::Cli;

fn main() -> Result<()> {
    let cli = Cli::parse();
    commands::run(cli)
}
