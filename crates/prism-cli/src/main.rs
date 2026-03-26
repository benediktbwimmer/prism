use std::path::PathBuf;
use std::process::Command as ProcessCommand;
use std::time::{SystemTime, UNIX_EPOCH};

use anyhow::{bail, Result};
use clap::{Parser, Subcommand};
use prism_core::{index_workspace_session, WorkspaceSession};
use prism_ir::{AnchorRef, EventActor, EventId, EventMeta, NodeId, NodeKind, TaskId};
use prism_memory::{
    EpisodicMemory, EpisodicMemorySnapshot, MemoryEntry, MemoryKind, MemoryModule, MemorySource,
    OutcomeEvent, OutcomeEvidence, OutcomeKind, OutcomeResult, RecallQuery, ScoredMemory,
};
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
enum TaskCommand {
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
enum MemoryCommand {
    Recall {
        name: String,
        #[arg(long)]
        text: Option<String>,
        #[arg(long, default_value_t = 5)]
        limit: usize,
    },
    Store {
        name: String,
        #[arg(long)]
        content: String,
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

fn main() -> Result<()> {
    let cli = Cli::parse();
    let session = index_workspace_session(&cli.root)?;
    let prism = session.prism();

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
        Command::CoChange { name, limit } => {
            let symbols = prism.symbol(&name);
            if symbols.is_empty() {
                eprintln!("no symbol matched `{name}`");
            }
            for symbol in symbols {
                println!("{}", symbol.signature());
                let neighbors = prism.co_change_neighbors(symbol.id(), limit);
                if neighbors.is_empty() {
                    println!("no co-change history");
                    continue;
                }
                for neighbor in neighbors {
                    println!("  {} ({} co-changes)", neighbor.lineage.0, neighbor.count);
                    for node in neighbor.nodes {
                        println!("    {}", node.path);
                    }
                }
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
                if !impact.validation_checks.is_empty() {
                    println!("scored validations:");
                    for check in impact.validation_checks {
                        println!(
                            "  {} score={:.2} last_seen={}",
                            check.label, check.score, check.last_seen
                        );
                    }
                }
                if !impact.co_change_neighbors.is_empty() {
                    println!("co-change neighbors:");
                    for neighbor in impact.co_change_neighbors {
                        println!("  {} count={}", neighbor.lineage.0, neighbor.count);
                        for node in neighbor.nodes {
                            println!("    {}", node.path);
                        }
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
        Command::ValidationRecipe { name } => {
            let symbols = prism.symbol(&name);
            if symbols.is_empty() {
                eprintln!("no symbol matched `{name}`");
            }
            for symbol in symbols {
                let recipe = prism.validation_recipe(symbol.id());
                println!("{}", symbol.signature());
                if !recipe.checks.is_empty() {
                    println!("checks:");
                    for check in &recipe.checks {
                        println!("  {check}");
                    }
                }
                if !recipe.scored_checks.is_empty() {
                    println!("scored checks:");
                    for check in recipe.scored_checks {
                        println!(
                            "  {} score={:.2} last_seen={}",
                            check.label, check.score, check.last_seen
                        );
                    }
                }
                if !recipe.co_change_neighbors.is_empty() {
                    println!("co-change neighbors:");
                    for neighbor in recipe.co_change_neighbors {
                        println!("  {} count={}", neighbor.lineage.0, neighbor.count);
                        for node in neighbor.nodes {
                            println!("    {}", node.path);
                        }
                    }
                }
                if !recipe.related_nodes.is_empty() {
                    print_relation_section("related nodes", &recipe.related_nodes);
                }
                if !recipe.recent_failures.is_empty() {
                    println!("recent failures:");
                    for event in recipe.recent_failures {
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
        Command::Memory { command } => match command {
            MemoryCommand::Recall { name, text, limit } => {
                let symbol = resolve_single_symbol(&prism, &name)?;
                let memory = load_episodic_memory(&session)?;
                let anchors = prism.anchors_for(&[AnchorRef::Node(symbol.id().clone())]);
                let results = memory.recall(&RecallQuery {
                    focus: anchors,
                    text,
                    limit,
                    kinds: None,
                    since: None,
                })?;
                if results.is_empty() {
                    eprintln!("no memory matched `{name}`");
                } else {
                    println!("{}", symbol.signature());
                    for memory in results {
                        print_scored_memory(memory);
                    }
                }
            }
            MemoryCommand::Store { name, content } => {
                let symbol = resolve_single_symbol(&prism, &name)?;
                let memory = load_episodic_memory(&session)?;
                let mut entry = MemoryEntry::new(MemoryKind::Episodic, content);
                entry.anchors = prism.anchors_for(&[AnchorRef::Node(symbol.id().clone())]);
                entry.source = MemorySource::User;
                let id = memory.store(entry)?;
                session.persist_episodic(&memory.snapshot())?;
                println!("stored memory {}", id.0);
            }
        },
        Command::Task { command } => match command {
            TaskCommand::Start {
                id,
                symbol,
                summary,
            } => {
                let anchors = resolve_optional_anchors(&prism, symbol.as_deref())?;
                let event = OutcomeEvent {
                    meta: EventMeta {
                        id: current_event_id("outcome"),
                        ts: current_timestamp(),
                        actor: EventActor::User,
                        correlation: Some(TaskId::new(id.clone())),
                        causation: None,
                    },
                    anchors,
                    kind: OutcomeKind::PlanCreated,
                    result: OutcomeResult::Success,
                    summary,
                    evidence: Vec::new(),
                    metadata: serde_json::Value::Null,
                };
                let outcome_id = record_outcome_event(&session, event)?;
                println!("recorded task start {}", outcome_id.0);
            }
            TaskCommand::Note {
                id,
                symbol,
                summary,
            } => {
                let anchors = resolve_optional_anchors(&prism, symbol.as_deref())?;
                let event = OutcomeEvent {
                    meta: EventMeta {
                        id: current_event_id("outcome"),
                        ts: current_timestamp(),
                        actor: EventActor::User,
                        correlation: Some(TaskId::new(id.clone())),
                        causation: None,
                    },
                    anchors,
                    kind: OutcomeKind::NoteAdded,
                    result: OutcomeResult::Success,
                    summary,
                    evidence: Vec::new(),
                    metadata: serde_json::Value::Null,
                };
                let outcome_id = record_outcome_event(&session, event)?;
                println!("recorded task note {}", outcome_id.0);
            }
            TaskCommand::Patch {
                id,
                name,
                summary,
                staged,
            } => {
                let symbol = resolve_single_symbol(&prism, &name)?;
                let diff_summary = git_diff_summary(&cli.root, staged)?;
                let summary = summary.unwrap_or_else(|| {
                    if staged {
                        format!("recorded staged patch for {}", symbol.id().path)
                    } else {
                        format!("recorded patch for {}", symbol.id().path)
                    }
                });
                let event = OutcomeEvent {
                    meta: EventMeta {
                        id: current_event_id("outcome"),
                        ts: current_timestamp(),
                        actor: EventActor::User,
                        correlation: Some(TaskId::new(id.clone())),
                        causation: None,
                    },
                    anchors: prism.anchors_for(&[AnchorRef::Node(symbol.id().clone())]),
                    kind: OutcomeKind::PatchApplied,
                    result: OutcomeResult::Success,
                    summary,
                    evidence: vec![OutcomeEvidence::DiffSummary { text: diff_summary }],
                    metadata: serde_json::Value::Null,
                };
                let outcome_id = record_outcome_event(&session, event)?;
                println!("recorded task patch {}", outcome_id.0);
            }
        },
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
                let ts = current_timestamp();
                let event = OutcomeEvent {
                    meta: EventMeta {
                        id: current_event_id("outcome"),
                        ts,
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
                let id = record_outcome_event(&session, event)?;
                println!("recorded outcome {}", id.0);
            }
            OutcomeCommand::Test {
                name,
                task,
                label,
                summary,
                command,
            } => {
                let symbol = resolve_single_symbol(&prism, &name)?;
                let validation =
                    run_validation_command(command, label, summary, OutcomeKind::TestRan)?;
                record_validation_outcome(
                    &session,
                    prism.as_ref(),
                    symbol,
                    task,
                    validation,
                    EventActor::User,
                )?;
            }
            OutcomeCommand::Build {
                name,
                task,
                label,
                summary,
                command,
            } => {
                let symbol = resolve_single_symbol(&prism, &name)?;
                let validation =
                    run_validation_command(command, label, summary, OutcomeKind::BuildRan)?;
                record_validation_outcome(
                    &session,
                    prism.as_ref(),
                    symbol,
                    task,
                    validation,
                    EventActor::User,
                )?;
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

fn resolve_optional_anchors(
    prism: &prism_query::Prism,
    symbol: Option<&str>,
) -> Result<Vec<AnchorRef>> {
    match symbol {
        Some(name) => {
            let symbol = resolve_single_symbol(prism, name)?;
            Ok(prism.anchors_for(&[AnchorRef::Node(symbol.id().clone())]))
        }
        None => Ok(Vec::new()),
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

fn print_scored_memory(memory: ScoredMemory) {
    println!(
        "  [{}] score={:.2} source={} trust={:.2} created_at={}",
        memory.id.0,
        memory.score,
        format!("{:?}", memory.entry.source),
        memory.entry.trust,
        memory.entry.created_at
    );
    println!("    {}", memory.entry.content);
    if let Some(explanation) = memory.explanation {
        println!("    explanation: {explanation}");
    }
}

fn load_episodic_memory(session: &WorkspaceSession) -> Result<EpisodicMemory> {
    let snapshot = session
        .load_episodic_snapshot()?
        .unwrap_or(EpisodicMemorySnapshot {
            entries: Vec::new(),
        });
    Ok(EpisodicMemory::from_snapshot(snapshot))
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

fn git_diff_summary(root: &std::path::Path, staged: bool) -> Result<String> {
    let mut command = ProcessCommand::new("git");
    command.current_dir(root);
    command.arg("diff").arg("--stat");
    if staged {
        command.arg("--cached");
    }

    let output = command.output()?;
    if !output.status.success() {
        bail!(
            "git diff failed: {}",
            String::from_utf8_lossy(&output.stderr).trim()
        );
    }

    let summary = String::from_utf8_lossy(&output.stdout).trim().to_string();
    if summary.is_empty() {
        Ok("no diff".to_string())
    } else {
        Ok(summary)
    }
}

struct ValidationRun {
    kind: OutcomeKind,
    result: OutcomeResult,
    summary: String,
    evidence: Vec<OutcomeEvidence>,
}

fn run_validation_command(
    command: Vec<String>,
    label: Option<String>,
    summary: Option<String>,
    kind: OutcomeKind,
) -> Result<ValidationRun> {
    let executable = command
        .first()
        .cloned()
        .ok_or_else(|| anyhow::anyhow!("validation command cannot be empty"))?;
    let args = command.iter().skip(1).cloned().collect::<Vec<_>>();
    let display = command.join(" ");

    let output = ProcessCommand::new(&executable).args(&args).output()?;
    if !output.stdout.is_empty() {
        print!("{}", String::from_utf8_lossy(&output.stdout));
    }
    if !output.stderr.is_empty() {
        eprint!("{}", String::from_utf8_lossy(&output.stderr));
    }

    let passed = output.status.success();
    let result = if passed {
        OutcomeResult::Success
    } else {
        OutcomeResult::Failure
    };
    let label = label.unwrap_or_else(|| display.clone());
    let summary = summary.unwrap_or_else(|| {
        let verdict = if passed { "passed" } else { "failed" };
        match kind {
            OutcomeKind::TestRan => format!("test `{label}` {verdict}"),
            OutcomeKind::BuildRan => format!("build `{label}` {verdict}"),
            _ => format!("validation `{label}` {verdict}"),
        }
    });
    let evidence = match kind {
        OutcomeKind::TestRan => vec![OutcomeEvidence::Test {
            name: label,
            passed,
        }],
        OutcomeKind::BuildRan => vec![OutcomeEvidence::Build {
            target: label,
            passed,
        }],
        _ => Vec::new(),
    };

    Ok(ValidationRun {
        kind,
        result,
        summary,
        evidence,
    })
}

fn record_validation_outcome(
    session: &WorkspaceSession,
    prism: &prism_query::Prism,
    symbol: Symbol<'_>,
    task: Option<String>,
    validation: ValidationRun,
    actor: EventActor,
) -> Result<()> {
    let ts = current_timestamp();
    let event = OutcomeEvent {
        meta: EventMeta {
            id: current_event_id("outcome"),
            ts,
            actor,
            correlation: task.map(TaskId::new),
            causation: None,
        },
        anchors: prism.anchors_for(&[AnchorRef::Node(symbol.id().clone())]),
        kind: validation.kind,
        result: validation.result,
        summary: validation.summary,
        evidence: validation.evidence,
        metadata: serde_json::Value::Null,
    };
    let id = record_outcome_event(session, event)?;
    println!("recorded outcome {}", id.0);

    if matches!(validation.result, OutcomeResult::Failure) {
        bail!("validation failed");
    }

    Ok(())
}

fn record_outcome_event(session: &WorkspaceSession, event: OutcomeEvent) -> Result<EventId> {
    session.append_outcome(event)
}

fn current_timestamp() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .expect("system clock should be after unix epoch")
        .as_secs()
}

fn current_event_id(prefix: &str) -> EventId {
    let nanos = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .expect("system clock should be after unix epoch")
        .as_nanos();
    EventId::new(format!("{prefix}:{nanos}"))
}
