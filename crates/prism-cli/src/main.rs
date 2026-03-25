use std::fs;
use std::path::PathBuf;
use std::time::{SystemTime, UNIX_EPOCH};

use anyhow::{bail, Result};
use clap::{Parser, Subcommand};
use prism_core::index_workspace;
use prism_ir::{AnchorRef, EventActor, EventId, EventMeta, NodeId, NodeKind, TaskId};
use prism_memory::{OutcomeEvent, OutcomeEvidence, OutcomeKind, OutcomeResult};
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
    Outcome {
        #[command(subcommand)]
        command: OutcomeCommand,
    },
}

#[derive(Subcommand)]
enum OutcomeCommand {
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
        Command::Outcome { command } => match command {
            OutcomeCommand::Record {
                name,
                kind,
                result,
                summary,
                task,
                tests,
                failing_tests,
                builds,
                failing_builds,
                issues,
                commits,
            } => {
                let symbol = resolve_single_symbol(&prism, &name)?;
                let anchors = prism.anchors_for(&[AnchorRef::Node(symbol.id().clone())]);
                let event = OutcomeEvent {
                    meta: EventMeta {
                        id: EventId::new(format!("outcome:{}", current_timestamp())),
                        ts: current_timestamp(),
                        actor: EventActor::User,
                        correlation: task.map(TaskId::new),
                        causation: None,
                    },
                    anchors,
                    kind: parse_outcome_kind(&kind)?,
                    result: parse_outcome_result(&result)?,
                    summary,
                    evidence: build_evidence(
                        tests,
                        failing_tests,
                        builds,
                        failing_builds,
                        issues,
                        commits,
                    ),
                    metadata: serde_json::Value::Null,
                };
                let id = prism.outcome_memory().store_event(event)?;
                save_outcomes(&cli.root, &prism)?;
                println!("recorded outcome {}", id.0);
            }
        },
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

fn resolve_single_symbol<'a>(prism: &'a prism_query::Prism, name: &str) -> Result<Symbol<'a>> {
    let mut symbols = prism.symbol(name);
    match symbols.len() {
        0 => bail!("no symbol matched `{name}`"),
        1 => Ok(symbols.remove(0)),
        _ => {
            let matches = symbols
                .into_iter()
                .map(|symbol| symbol.signature())
                .collect::<Vec<_>>()
                .join(", ");
            bail!("symbol `{name}` is ambiguous: {matches}");
        }
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

fn parse_outcome_kind(value: &str) -> Result<OutcomeKind> {
    match value.to_ascii_lowercase().as_str() {
        "note-added" | "note" => Ok(OutcomeKind::NoteAdded),
        "hypothesis-proposed" | "hypothesis" => Ok(OutcomeKind::HypothesisProposed),
        "plan-created" | "plan" => Ok(OutcomeKind::PlanCreated),
        "patch-applied" | "patch" => Ok(OutcomeKind::PatchApplied),
        "build-ran" | "build" => Ok(OutcomeKind::BuildRan),
        "test-ran" | "test" => Ok(OutcomeKind::TestRan),
        "review-feedback" | "review" => Ok(OutcomeKind::ReviewFeedback),
        "failure-observed" | "failure" => Ok(OutcomeKind::FailureObserved),
        "regression-observed" | "regression" => Ok(OutcomeKind::RegressionObserved),
        "fix-validated" | "validated" => Ok(OutcomeKind::FixValidated),
        "rollback-performed" | "rollback" => Ok(OutcomeKind::RollbackPerformed),
        "migration-required" | "migration" => Ok(OutcomeKind::MigrationRequired),
        "incident-linked" | "incident" => Ok(OutcomeKind::IncidentLinked),
        "perf-signal-observed" | "perf" => Ok(OutcomeKind::PerfSignalObserved),
        other => bail!("unknown outcome kind `{other}`"),
    }
}

fn parse_outcome_result(value: &str) -> Result<OutcomeResult> {
    match value.to_ascii_lowercase().as_str() {
        "success" => Ok(OutcomeResult::Success),
        "failure" => Ok(OutcomeResult::Failure),
        "partial" => Ok(OutcomeResult::Partial),
        "unknown" => Ok(OutcomeResult::Unknown),
        other => bail!("unknown outcome result `{other}`"),
    }
}

fn build_evidence(
    tests: Vec<String>,
    failing_tests: Vec<String>,
    builds: Vec<String>,
    failing_builds: Vec<String>,
    issues: Vec<String>,
    commits: Vec<String>,
) -> Vec<OutcomeEvidence> {
    let mut evidence = Vec::new();
    for name in tests {
        evidence.push(OutcomeEvidence::Test { name, passed: true });
    }
    for name in failing_tests {
        evidence.push(OutcomeEvidence::Test {
            name,
            passed: false,
        });
    }
    for target in builds {
        evidence.push(OutcomeEvidence::Build {
            target,
            passed: true,
        });
    }
    for target in failing_builds {
        evidence.push(OutcomeEvidence::Build {
            target,
            passed: false,
        });
    }
    for id in issues {
        evidence.push(OutcomeEvidence::Issue { id });
    }
    for sha in commits {
        evidence.push(OutcomeEvidence::Commit { sha });
    }
    evidence
}

fn save_outcomes(root: &std::path::Path, prism: &prism_query::Prism) -> Result<()> {
    let path = root.join(".prism").join("outcomes.json");
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)?;
    }
    fs::write(path, serde_json::to_vec_pretty(&prism.outcome_snapshot())?)?;
    Ok(())
}

fn current_timestamp() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .expect("system clock should be after unix epoch")
        .as_secs()
}
