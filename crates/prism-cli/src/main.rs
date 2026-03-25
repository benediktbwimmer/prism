use std::path::PathBuf;

use anyhow::Result;
use clap::{Parser, Subcommand};
use prism_core::index_workspace;
use prism_query::Symbol;

#[derive(Parser)]
#[command(name = "prism")]
#[command(about = "Deterministic local-first code perception")]
struct Cli {
    #[arg(long, default_value = ".")]
    root: PathBuf,
    #[command(subcommand)]
    command: Command,
}

#[derive(Subcommand)]
enum Command {
    Entrypoints,
    Symbol {
        name: String,
    },
    CallGraph {
        name: String,
        #[arg(long, default_value_t = 3)]
        depth: usize,
    },
}

fn main() -> Result<()> {
    let cli = Cli::parse();
    let prism = index_workspace(&cli.root)?;

    match cli.command {
        Command::Entrypoints => {
            for symbol in prism.entrypoints() {
                println!("{}", symbol.signature());
            }
        }
        Command::Symbol { name } => {
            for symbol in prism.symbol(&name) {
                print_symbol(symbol);
            }
        }
        Command::CallGraph { name, depth } => {
            for symbol in prism.symbol(&name) {
                let graph = symbol.call_graph(depth);
                println!("root: {}", graph.root.path);
                for edge in graph.edges {
                    println!("{} -> {}", edge.source.path, edge.target.path);
                }
            }
        }
    }

    Ok(())
}

fn print_symbol(symbol: Symbol<'_>) {
    println!("{}", symbol.signature());
    let full = symbol.full();
    if !full.trim().is_empty() {
        println!("{full}");
    }
    let skeleton = symbol.skeleton();
    if !skeleton.calls.is_empty() {
        println!("calls:");
        for call in skeleton.calls {
            println!("  {}", call.path);
        }
    }
}
