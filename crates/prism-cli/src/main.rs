use std::path::PathBuf;

use anyhow::Result;
use clap::{Parser, Subcommand};
use prism_core::index_workspace;
use prism_ir::NodeId;
use prism_query::{Relations, Symbol};

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
    Relations {
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
        Command::Relations { name } => {
            for symbol in prism.symbol(&name) {
                print_relations(symbol);
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
    print_relation_section("calls", &symbol.skeleton().calls);
    print_relation_section("imports", &symbol.imports());
    print_relation_section("implements", &symbol.implements());
    print_relation_section("called by", &symbol.callers());
    print_relation_section("imported by", &symbol.imported_by());
    print_relation_section("implemented by", &symbol.implemented_by());
}

fn print_relations(symbol: Symbol<'_>) {
    println!("{}", symbol.signature());
    let relations = symbol.relations();
    print_named_relations(relations);
}

fn print_named_relations(relations: Relations) {
    print_relation_section("calls", &relations.outgoing_calls);
    print_relation_section("called by", &relations.incoming_calls);
    print_relation_section("imports", &relations.outgoing_imports);
    print_relation_section("imported by", &relations.incoming_imports);
    print_relation_section("implements", &relations.outgoing_implements);
    print_relation_section("implemented by", &relations.incoming_implements);
}

fn print_relation_section(label: &str, values: &[NodeId]) {
    if values.is_empty() {
        return;
    }
    println!("{label}:");
    for value in values {
        println!("  {}", value.path);
    }
}
