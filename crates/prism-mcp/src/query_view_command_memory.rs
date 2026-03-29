use std::collections::{BTreeMap, BTreeSet};

use anyhow::Result;
use prism_ir::{AnchorRef, TaskId};
use prism_js::{
    CommandMemoryCommandView, CommandMemoryView, QueryEvidenceView, QueryViewSubjectView,
    RepoPlaybookSectionView,
};
use prism_memory::{OutcomeEvidence, OutcomeRecallQuery};
use serde::Deserialize;
use serde_json::Value;

use crate::query_view_playbook::collect_repo_playbook;
use crate::{invalid_query_argument_error, node_id_view, QueryExecution};

const COMMAND_LIMIT: usize = 8;
const OBSERVED_LIMIT: usize = 64;

#[derive(Debug, Default, Deserialize)]
#[serde(rename_all = "camelCase")]
struct CommandMemoryInput {
    #[serde(alias = "task_id")]
    task_id: Option<String>,
}

#[derive(Debug, Clone)]
struct CommandCandidate {
    command: String,
    provenance: Vec<QueryEvidenceView>,
    caveats: BTreeSet<String>,
    explicit_playbook: bool,
    inferred_playbook: bool,
    observed_successes: usize,
    observed_failures: usize,
    last_seen: Option<u64>,
}

impl CommandCandidate {
    fn new(command: &str) -> Self {
        Self {
            command: command.to_string(),
            provenance: Vec::new(),
            caveats: BTreeSet::new(),
            explicit_playbook: false,
            inferred_playbook: false,
            observed_successes: 0,
            observed_failures: 0,
            last_seen: None,
        }
    }
}

pub(crate) fn command_memory_view(execution: &QueryExecution, input: Value) -> Result<Value> {
    let input: CommandMemoryInput = if input.is_null() {
        CommandMemoryInput::default()
    } else {
        serde_json::from_value(input)
            .map_err(|error| invalid_query_argument_error("commandMemory", error.to_string()))?
    };
    let mut notes = Vec::new();
    let task_id = if let Some(task_id) = input.task_id {
        Some(task_id)
    } else if let Some(task) = execution.session().current_task_state() {
        notes.push(format!(
            "Defaulted to the current session task `{}`.",
            task.id.0
        ));
        Some(task.id.0.to_string())
    } else {
        None
    };
    let subject = if let Some(task_id) = task_id.clone() {
        QueryViewSubjectView {
            kind: "task".to_string(),
            task_id: Some(task_id),
            target: None,
            paths: Vec::new(),
            unresolved_paths: Vec::new(),
        }
    } else {
        QueryViewSubjectView {
            kind: "repo".to_string(),
            task_id: None,
            target: None,
            paths: Vec::new(),
            unresolved_paths: Vec::new(),
        }
    };

    let mut candidates = BTreeMap::<String, CommandCandidate>::new();
    let playbook = collect_repo_playbook(execution.workspace_root());
    for (section_name, section) in [
        ("build", &playbook.build),
        ("test", &playbook.test),
        ("lint", &playbook.lint),
        ("format", &playbook.format),
        ("workflow", &playbook.workflow),
    ] {
        absorb_playbook_section(&mut candidates, section_name, section);
    }

    let outcomes = execution.prism().query_outcomes(&OutcomeRecallQuery {
        task: task_id.clone().map(TaskId::new),
        limit: OBSERVED_LIMIT,
        ..OutcomeRecallQuery::default()
    });
    for event in &outcomes {
        for evidence in &event.evidence {
            let OutcomeEvidence::Command { argv, passed } = evidence else {
                continue;
            };
            let command = join_argv(argv);
            if command.is_empty() {
                continue;
            }
            let key = normalize_command(&command);
            let candidate = candidates
                .entry(key)
                .or_insert_with(|| CommandCandidate::new(&command));
            candidate.command = command.clone();
            if *passed {
                candidate.observed_successes += 1;
            } else {
                candidate.observed_failures += 1;
            }
            candidate.last_seen = Some(
                candidate
                    .last_seen
                    .map_or(event.meta.ts, |ts| ts.max(event.meta.ts)),
            );
            candidate.provenance.push(QueryEvidenceView {
                kind: "observed_outcome".to_string(),
                detail: event.summary.clone(),
                path: None,
                line: None,
                target: first_anchor_target(&event.anchors),
            });
        }
    }
    if task_id.is_some() && outcomes.is_empty() {
        notes.push(
            "No observed outcome events matched the requested task yet; showing repo workflow signals where possible."
                .to_string(),
        );
    }

    let mut commands = candidates
        .into_values()
        .map(finalize_candidate)
        .collect::<Vec<_>>();
    commands.sort_by(|left, right| {
        right
            .confidence
            .partial_cmp(&left.confidence)
            .unwrap_or(std::cmp::Ordering::Equal)
            .then_with(|| right.last_seen.cmp(&left.last_seen))
            .then_with(|| left.command.cmp(&right.command))
    });
    commands.truncate(COMMAND_LIMIT);
    if commands.is_empty() {
        notes.push(
            "No explicit repo command signals or observed command evidence were available for this scope."
                .to_string(),
        );
    }

    Ok(serde_json::to_value(CommandMemoryView {
        subject,
        commands,
        notes,
    })?)
}

fn absorb_playbook_section(
    candidates: &mut BTreeMap<String, CommandCandidate>,
    section_name: &str,
    section: &RepoPlaybookSectionView,
) {
    for command in &section.commands {
        let candidate = candidates
            .entry(normalize_command(command))
            .or_insert_with(|| CommandCandidate::new(command));
        candidate.command = command.clone();
        candidate.provenance.extend(section.provenance.clone());
        match section.status.as_str() {
            "explicit" => {
                candidate.explicit_playbook = true;
            }
            "inferred" => {
                candidate.inferred_playbook = true;
                candidate.caveats.insert(format!(
                    "Repo guidance inferred this {section_name} command from workspace signals rather than explicit docs."
                ));
            }
            _ => {}
        }
    }
}

fn finalize_candidate(candidate: CommandCandidate) -> CommandMemoryCommandView {
    let mut caveats = candidate.caveats.into_iter().collect::<Vec<_>>();
    let mut confidence = 0.2;
    if candidate.explicit_playbook {
        confidence += 0.2;
    }
    if candidate.inferred_playbook {
        confidence += 0.05;
    }
    if candidate.observed_successes > 0 {
        confidence += 0.45;
        confidence += 0.05 * candidate.observed_successes.saturating_sub(1).min(2) as f32;
    } else if candidate.observed_failures > 0 {
        confidence += 0.15;
        caveats.push(
            "Only failing observed runs exist so far; treat this as a risky command until a success is recorded."
                .to_string(),
        );
    }
    if candidate.observed_successes > 0 && candidate.observed_failures > 0 {
        confidence -= 0.1;
        caveats.push(
            "Observed outcomes are mixed across runs; validate the command against the current task scope."
                .to_string(),
        );
    }
    if !candidate.explicit_playbook
        && !candidate.inferred_playbook
        && candidate.observed_successes > 0
    {
        caveats.push(
            "This command is observed in repo outcomes but is not currently documented in repo workflow guidance."
                .to_string(),
        );
    }
    confidence = confidence.clamp(0.0, 0.98);

    let why = match (
        candidate.explicit_playbook,
        candidate.inferred_playbook,
        candidate.observed_successes,
        candidate.observed_failures,
    ) {
        (true, _, successes, _) if successes > 0 => format!(
            "Explicit repo guidance matches {successes} successful observed run(s) in this repo."
        ),
        (true, _, _, failures) if failures > 0 => {
            "Explicit repo guidance exists, but the observed evidence so far is only failing."
                .to_string()
        }
        (true, _, _, _) => "Explicit repo workflow guidance documents this command.".to_string(),
        (_, true, successes, _) if successes > 0 => format!(
            "An inferred repo workflow command also has {successes} successful observed run(s)."
        ),
        (_, true, _, _) => {
            "Repo workflow signals imply this command, but explicit docs do not name it yet."
                .to_string()
        }
        (_, _, successes, _) if successes > 0 => format!(
            "Observed {successes} successful run(s) for this command in repo outcome history."
        ),
        _ => "Observed this command in repo outcome history, but not as a confirmed success yet."
            .to_string(),
    };

    CommandMemoryCommandView {
        command: candidate.command,
        confidence,
        why,
        provenance: dedupe_provenance(candidate.provenance),
        caveats,
        last_seen: candidate.last_seen,
    }
}

fn dedupe_provenance(items: Vec<QueryEvidenceView>) -> Vec<QueryEvidenceView> {
    let mut seen = BTreeSet::new();
    let mut deduped = Vec::new();
    for item in items {
        let key = format!(
            "{}|{}|{}|{}|{}",
            item.kind,
            item.detail,
            item.path.as_deref().unwrap_or_default(),
            item.line.unwrap_or_default(),
            item.target
                .as_ref()
                .map(|target| format!("{}::{}", target.crate_name, target.path))
                .unwrap_or_default()
        );
        if seen.insert(key) {
            deduped.push(item);
        }
    }
    deduped
}

fn first_anchor_target(anchors: &[AnchorRef]) -> Option<prism_js::NodeIdView> {
    anchors.iter().find_map(|anchor| match anchor {
        AnchorRef::Node(node) => Some(node_id_view(node.clone())),
        _ => None,
    })
}

fn join_argv(argv: &[String]) -> String {
    argv.join(" ").trim().to_string()
}

fn normalize_command(command: &str) -> String {
    command.split_whitespace().collect::<Vec<_>>().join(" ")
}
