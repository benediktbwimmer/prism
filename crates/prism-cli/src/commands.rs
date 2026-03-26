use anyhow::Result;
use prism_core::{index_workspace_session, ValidationFeedbackRecord, WorkspaceSession};
use prism_ir::{AnchorRef, EventActor, EventMeta, TaskId};
use prism_memory::{MemoryModule, OutcomeEvent, OutcomeEvidence, OutcomeKind, OutcomeResult};

use crate::cli::{Cli, Command, FeedbackCommand, MemoryCommand, OutcomeCommand, TaskCommand};
use crate::display::{
    print_lineage, print_relation_section, print_relations, print_scored_memory, print_symbol,
    print_validation_feedback,
};
use crate::parsing::{
    build_evidence, parse_memory_kind, parse_node_kind_filter, parse_outcome_kind,
    parse_outcome_result, parse_validation_feedback_category, parse_validation_feedback_verdict,
};
use crate::runtime::{
    build_memory_entry, build_recall_query, build_task_event, current_event_id, current_timestamp,
    git_diff_summary, load_session_memory, record_outcome_event, record_validation_outcome,
    resolve_optional_anchors, resolve_single_symbol, run_validation_command,
};

pub fn run(cli: Cli) -> Result<()> {
    let session = index_workspace_session(&cli.root)?;
    let prism = session.prism();
    let root = cli.root.clone();

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
        Command::Feedback { command } => {
            handle_feedback_command(&session, prism.as_ref(), command)?
        }
        Command::Memory { command } => handle_memory_command(&session, prism.as_ref(), command)?,
        Command::Task { command } => handle_task_command(&root, &session, prism.as_ref(), command)?,
        Command::Outcome { command } => handle_outcome_command(&session, prism.as_ref(), command)?,
    }

    Ok(())
}

fn handle_memory_command(
    session: &WorkspaceSession,
    prism: &prism_query::Prism,
    command: MemoryCommand,
) -> Result<()> {
    match command {
        MemoryCommand::Recall {
            name,
            text,
            limit,
            kinds,
        } => {
            let symbol = resolve_single_symbol(prism, &name)?;
            let memory = load_session_memory(session)?;
            let kinds = if kinds.is_empty() {
                None
            } else {
                Some(
                    kinds
                        .iter()
                        .map(|kind| parse_memory_kind(kind))
                        .collect::<Result<Vec<_>>>()?,
                )
            };
            let results = memory.recall(&build_recall_query(prism, &symbol, text, limit, kinds))?;
            if results.is_empty() {
                eprintln!("no memory matched `{name}`");
            } else {
                println!("{}", symbol.signature());
                for memory in results {
                    print_scored_memory(memory);
                }
            }
        }
        MemoryCommand::Store {
            name,
            content,
            kind,
        } => {
            let symbol = resolve_single_symbol(prism, &name)?;
            let memory = load_session_memory(session)?;
            let id = memory.store(build_memory_entry(
                prism,
                symbol,
                parse_memory_kind(&kind)?,
                content,
            ))?;
            session.persist_episodic(&memory.snapshot())?;
            println!("stored memory {}", id.0);
        }
    }

    Ok(())
}

fn handle_feedback_command(
    session: &WorkspaceSession,
    prism: &prism_query::Prism,
    command: FeedbackCommand,
) -> Result<()> {
    match command {
        FeedbackCommand::Record {
            context,
            prism_said,
            actually_true,
            category,
            verdict,
            task_id,
            symbols,
            corrected_manually,
            correction,
        } => {
            let mut anchors = Vec::new();
            for name in symbols {
                let symbol = resolve_single_symbol(prism, &name)?;
                for anchor in prism.anchors_for(&[AnchorRef::Node(symbol.id().clone())]) {
                    if !anchors.contains(&anchor) {
                        anchors.push(anchor);
                    }
                }
            }
            let entry = session.append_validation_feedback(ValidationFeedbackRecord {
                task_id,
                context,
                anchors,
                prism_said,
                actually_true,
                category: parse_validation_feedback_category(&category)?,
                verdict: parse_validation_feedback_verdict(&verdict)?,
                corrected_manually,
                correction,
                metadata: serde_json::Value::Null,
            })?;
            println!("recorded feedback {}", entry.id);
        }
        FeedbackCommand::List { limit } => {
            let entries = session.validation_feedback(Some(limit))?;
            if entries.is_empty() {
                eprintln!("no validation feedback recorded");
            } else {
                for entry in entries {
                    print_validation_feedback(entry);
                }
            }
        }
    }

    Ok(())
}

fn handle_task_command(
    root: &std::path::Path,
    session: &WorkspaceSession,
    prism: &prism_query::Prism,
    command: TaskCommand,
) -> Result<()> {
    match command {
        TaskCommand::Start {
            id,
            symbol,
            summary,
        } => {
            let anchors = resolve_optional_anchors(prism, symbol.as_deref())?;
            let outcome_id = record_outcome_event(
                session,
                build_task_event(anchors, id, summary, OutcomeKind::PlanCreated),
            )?;
            println!("recorded task start {}", outcome_id.0);
        }
        TaskCommand::Note {
            id,
            symbol,
            summary,
        } => {
            let anchors = resolve_optional_anchors(prism, symbol.as_deref())?;
            let outcome_id = record_outcome_event(
                session,
                build_task_event(anchors, id, summary, OutcomeKind::NoteAdded),
            )?;
            println!("recorded task note {}", outcome_id.0);
        }
        TaskCommand::Patch {
            id,
            name,
            summary,
            staged,
        } => {
            let symbol = resolve_single_symbol(prism, &name)?;
            let diff_summary = git_diff_summary(root, staged)?;
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
            let outcome_id = record_outcome_event(session, event)?;
            println!("recorded task patch {}", outcome_id.0);
        }
    }

    Ok(())
}

fn handle_outcome_command(
    session: &WorkspaceSession,
    prism: &prism_query::Prism,
    command: OutcomeCommand,
) -> Result<()> {
    match command {
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
            let symbol = resolve_single_symbol(prism, &name)?;
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
            let id = record_outcome_event(session, event)?;
            println!("recorded outcome {}", id.0);
        }
        OutcomeCommand::Test {
            name,
            task,
            label,
            summary,
            command,
        } => {
            let symbol = resolve_single_symbol(prism, &name)?;
            let validation = run_validation_command(command, label, summary, OutcomeKind::TestRan)?;
            record_validation_outcome(session, prism, symbol, task, validation, EventActor::User)?;
        }
        OutcomeCommand::Build {
            name,
            task,
            label,
            summary,
            command,
        } => {
            let symbol = resolve_single_symbol(prism, &name)?;
            let validation =
                run_validation_command(command, label, summary, OutcomeKind::BuildRan)?;
            record_validation_outcome(session, prism, symbol, task, validation, EventActor::User)?;
        }
    }

    Ok(())
}
