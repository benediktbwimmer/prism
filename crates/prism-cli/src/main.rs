use std::path::PathBuf;

use anyhow::{bail, Result};
use clap::{Parser, Subcommand};
use prism_core::index_workspace;
use prism_ir::{NodeId, NodeKind, TaskId};
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
    Lineage {
        name: String,
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
    TaskResume {
        id: String,
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
            let symbols = prism.symbol(&name);
            if symbols.is_empty() {
                eprintln!("no symbol matched `{name}`");
            }
            for symbol in symbols {
                print_symbol(symbol);
            }
        }
        Command::Search {
            query,
            limit,
            kind,
            path,
        } => {
            let kind = parse_node_kind_filter(kind.as_deref())?;
            let symbols = prism.search(&query, limit, kind, path.as_deref());
            if symbols.is_empty() {
                eprintln!("no symbol matched `{query}`");
            }
            for symbol in symbols {
                println!("{}", symbol.signature());
            }
        }
        Command::Lineage { name } => {
            let symbols = prism.symbol(&name);
            if symbols.is_empty() {
                eprintln!("no symbol matched `{name}`");
            }
            for symbol in symbols {
                print_lineage(&prism, symbol);
            }
        }
        Command::Relations { name } => {
            let symbols = prism.symbol(&name);
            if symbols.is_empty() {
                eprintln!("no symbol matched `{name}`");
            }
            for symbol in symbols {
                print_relations(symbol);
            }
        }
        Command::CallGraph { name, depth } => {
            let symbols = prism.symbol(&name);
            if symbols.is_empty() {
                eprintln!("no symbol matched `{name}`");
            }
            for symbol in symbols {
                let graph = symbol.call_graph(depth);
                println!("root: {}", graph.root.path);
                for edge in graph.edges {
                    println!("{} -> {}", edge.source.path, edge.target.path);
                }
            }
        }
        Command::Risk { name } => {
            let symbols = prism.symbol(&name);
            if symbols.is_empty() {
                eprintln!("no symbol matched `{name}`");
            }
            for symbol in symbols {
                let impact = prism.blast_radius(symbol.id());
                println!("{}", symbol.signature());
                print_relation_section("directly related", &impact.direct_nodes);
                if !impact.lineages.is_empty() {
                    println!("lineages:");
                    for lineage in impact.lineages {
                        println!("  {}", lineage.0);
                    }
                }
                if !impact.likely_validations.is_empty() {
                    println!("likely validations:");
                    for validation in impact.likely_validations {
                        println!("  {validation}");
                    }
                }
                if !impact.risk_events.is_empty() {
                    println!("risk events:");
                    for event in impact.risk_events {
                        println!("  [{}] {}", event.meta.id.0, event.summary);
                    }
                }
            }
        }
        Command::TaskResume { id } => {
            let replay = prism.resume_task(&TaskId::new(id.clone()));
            if replay.events.is_empty() {
                eprintln!("no events recorded for task `{id}`");
            } else {
                println!("task: {}", replay.task.0);
                for event in replay.events {
                    println!("[{}] {}", event.meta.id.0, event.summary);
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

fn print_lineage(prism: &prism_query::Prism, symbol: Symbol<'_>) {
    println!("{}", symbol.signature());
    let Some(lineage) = prism.lineage_of(symbol.id()) else {
        println!("no lineage");
        return;
    };
    println!("lineage: {}", lineage.0);
    for event in prism.lineage_history(&lineage) {
        let before = event
            .before
            .iter()
            .map(|node| node.path.as_str())
            .collect::<Vec<_>>()
            .join(", ");
        let after = event
            .after
            .iter()
            .map(|node| node.path.as_str())
            .collect::<Vec<_>>()
            .join(", ");
        println!("  {:?}: [{}] -> [{}]", event.kind, before, after);
    }
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

fn parse_node_kind_filter(value: Option<&str>) -> Result<Option<NodeKind>> {
    let Some(value) = value else {
        return Ok(None);
    };

    let kind = match value.to_ascii_lowercase().as_str() {
        "workspace" => NodeKind::Workspace,
        "package" => NodeKind::Package,
        "document" => NodeKind::Document,
        "module" => NodeKind::Module,
        "function" => NodeKind::Function,
        "struct" => NodeKind::Struct,
        "enum" => NodeKind::Enum,
        "trait" => NodeKind::Trait,
        "impl" => NodeKind::Impl,
        "method" => NodeKind::Method,
        "field" => NodeKind::Field,
        "typealias" | "type-alias" => NodeKind::TypeAlias,
        "markdownheading" | "markdown-heading" => NodeKind::MarkdownHeading,
        "jsonkey" | "json-key" => NodeKind::JsonKey,
        "yamlkey" | "yaml-key" => NodeKind::YamlKey,
        other => {
            bail!("unknown node kind `{other}`");
        }
    };

    Ok(Some(kind))
}
