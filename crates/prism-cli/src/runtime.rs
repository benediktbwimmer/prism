use std::process::Command as ProcessCommand;
use std::time::{SystemTime, UNIX_EPOCH};

use anyhow::{bail, Result};
use prism_core::WorkspaceSession;
use prism_ir::{new_prefixed_id, AnchorRef, EventActor, EventId, EventMeta, TaskId};
use prism_memory::{
    EpisodicMemorySnapshot, MemoryEntry, MemoryEvent, MemoryEventKind, MemoryId, MemoryKind,
    MemoryScope, MemorySource, OutcomeEvent, OutcomeEvidence, OutcomeKind, OutcomeResult,
    RecallQuery, SessionMemory,
};
use prism_query::Symbol;
use serde_json::json;

pub struct ValidationRun {
    pub kind: OutcomeKind,
    pub result: OutcomeResult,
    pub summary: String,
    pub evidence: Vec<OutcomeEvidence>,
}

pub fn resolve_single_symbol<'a>(prism: &'a prism_query::Prism, name: &str) -> Result<Symbol<'a>> {
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

pub fn resolve_optional_anchors(
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

pub fn load_session_memory(session: &WorkspaceSession) -> Result<SessionMemory> {
    let snapshot = session
        .load_episodic_snapshot()?
        .unwrap_or(EpisodicMemorySnapshot {
            entries: Vec::new(),
        });
    Ok(SessionMemory::from_snapshot(snapshot))
}

pub fn build_memory_entry(
    prism: &prism_query::Prism,
    symbol: Symbol<'_>,
    kind: MemoryKind,
    scope: MemoryScope,
    content: String,
) -> MemoryEntry {
    let mut entry = MemoryEntry::new(kind, content);
    entry.anchors = prism.anchors_for(&[AnchorRef::Node(symbol.id().clone())]);
    entry.scope = scope;
    entry.source = MemorySource::User;
    if scope == MemoryScope::Repo {
        let now = current_timestamp();
        entry.metadata = json!({
            "provenance": {
                "origin": "cli_store",
                "kind": "manual_memory",
            },
            "evidence": {
                "eventIds": [],
                "memoryIds": [],
                "validationChecks": [],
                "coChangeLineages": [],
            },
            "publication": {
                "publishedAt": now,
                "lastReviewedAt": now,
                "status": "active",
            }
        });
        entry.trust = 0.85;
    } else if scope == MemoryScope::Session {
        entry.metadata = json!({
            "provenance": {
                "origin": "cli_store",
                "kind": "manual_memory",
            },
            "evidence": {
                "eventIds": [],
                "memoryIds": [],
                "validationChecks": [],
                "coChangeLineages": [],
            }
        });
        entry.trust = 0.75;
    }
    entry
}

pub fn build_memory_event(
    entry: MemoryEntry,
    task_id: Option<String>,
    promoted_from: Vec<MemoryId>,
    supersedes: Vec<MemoryId>,
) -> MemoryEvent {
    let action = if promoted_from.is_empty() && supersedes.is_empty() {
        MemoryEventKind::Stored
    } else {
        MemoryEventKind::Promoted
    };
    MemoryEvent::from_entry(action, entry, task_id, promoted_from, supersedes)
}

pub fn build_task_event(
    anchors: Vec<AnchorRef>,
    task_id: String,
    summary: String,
    kind: OutcomeKind,
) -> OutcomeEvent {
    OutcomeEvent {
        meta: EventMeta {
            id: current_event_id("outcome"),
            ts: current_timestamp(),
            actor: EventActor::User,
            correlation: Some(TaskId::new(task_id)),
            causation: None,
        },
        anchors,
        kind,
        result: OutcomeResult::Success,
        summary,
        evidence: Vec::new(),
        metadata: serde_json::Value::Null,
    }
}

pub fn git_diff_summary(root: &std::path::Path, staged: bool) -> Result<String> {
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

pub fn run_validation_command(
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

pub fn record_validation_outcome(
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

pub fn record_outcome_event(session: &WorkspaceSession, event: OutcomeEvent) -> Result<EventId> {
    session.append_outcome(event)
}

pub fn build_recall_query(
    prism: &prism_query::Prism,
    symbol: &Symbol<'_>,
    text: Option<String>,
    limit: usize,
    kinds: Option<Vec<MemoryKind>>,
) -> RecallQuery {
    let anchors = prism.anchors_for(&[AnchorRef::Node(symbol.id().clone())]);
    RecallQuery {
        focus: anchors,
        text,
        limit,
        kinds,
        since: None,
    }
}

pub fn current_timestamp() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .expect("system clock should be after unix epoch")
        .as_secs()
}

pub fn current_event_id(prefix: &str) -> EventId {
    EventId::new(new_prefixed_id(prefix))
}
