use std::collections::{BTreeMap, BTreeSet};
use std::path::Path;

use anyhow::Result;
use prism_ir::{NodeId, TaskId};
use prism_js::{
    AfterEditView, ContractPacketView, QueryEvidenceView, QueryRecommendationView,
    QueryViewSubjectView,
};
use serde::Deserialize;
use serde_json::Value;

use crate::compact_followups::same_workspace_file;
use crate::query_view_playbook::collect_repo_playbook;
use crate::{
    blast_radius_view, changed_files, contract_packet_view, invalid_query_argument_error,
    next_reads, node_id_view, validation_recipe_view_with, QueryExecution, SymbolTargetArgs,
};

const NEXT_READ_LIMIT: usize = 5;
const TEST_LIMIT: usize = 5;
const DOC_LIMIT: usize = 4;
const RISK_LIMIT: usize = 4;
const PATH_TARGET_LIMIT: usize = 16;

#[derive(Debug, Deserialize, Default)]
#[serde(rename_all = "camelCase")]
struct AfterEditInput {
    task_id: Option<String>,
    target: Option<SymbolTargetArgs>,
    paths: Option<Vec<String>>,
}

#[derive(Debug, Clone)]
struct RecommendationSeed {
    kind: String,
    label: String,
    why: String,
    provenance: Vec<QueryEvidenceView>,
    target: Option<prism_js::NodeIdView>,
    path: Option<String>,
    score: Option<f32>,
    last_seen: Option<u64>,
}

pub(crate) fn after_edit_view(execution: &QueryExecution, input: Value) -> Result<Value> {
    let input: AfterEditInput = if input.is_null() {
        AfterEditInput::default()
    } else {
        serde_json::from_value(input)
            .map_err(|error| invalid_query_argument_error("afterEdit", error.to_string()))?
    };
    let provided_subjects = usize::from(input.task_id.is_some())
        + usize::from(input.target.is_some())
        + usize::from(input.paths.as_ref().is_some_and(|paths| !paths.is_empty()));
    if provided_subjects > 1 {
        return Err(invalid_query_argument_error(
            "afterEdit",
            "provide at most one of `taskId`, `target`, or `paths`",
        ));
    }

    let mut notes = Vec::<String>::new();
    let mut targets = Vec::<NodeId>::new();
    let mut docs = Vec::<RecommendationSeed>::new();
    let mut contracts = Vec::<ContractPacketView>::new();

    let subject = if let Some(task_id) = input.task_id {
        targets = targets_for_task(execution, &task_id, &mut notes)?;
        QueryViewSubjectView {
            kind: "task".to_string(),
            task_id: Some(task_id),
            target: None,
            paths: Vec::new(),
            unresolved_paths: Vec::new(),
        }
    } else if let Some(target) = input.target {
        let id = execution.resolve_target_id(target.id, target.lineage_id)?;
        targets.push(id.clone());
        QueryViewSubjectView {
            kind: "target".to_string(),
            task_id: None,
            target: Some(node_id_view(id)),
            paths: Vec::new(),
            unresolved_paths: Vec::new(),
        }
    } else if let Some(paths) = input.paths {
        let (resolved_targets, unresolved_paths) =
            resolve_targets_for_paths(execution.workspace_root(), execution.prism(), &paths);
        if resolved_targets.is_empty() {
            notes.push(
                "No indexed targets matched the requested paths; follow-up suggestions will fall back to repo guidance where possible."
                    .to_string(),
            );
        }
        if !unresolved_paths.is_empty() {
            notes.push(format!(
                "Some requested paths did not resolve to indexed targets: {}.",
                unresolved_paths.join(", ")
            ));
        }
        targets = resolved_targets;
        QueryViewSubjectView {
            kind: "pathSet".to_string(),
            task_id: None,
            target: None,
            paths,
            unresolved_paths,
        }
    } else if let Some(task) = execution.session().current_task_state() {
        notes.push(format!(
            "Defaulted to the current session task `{}`.",
            task.id.0
        ));
        targets = targets_for_task(execution, &task.id.0, &mut notes)?;
        QueryViewSubjectView {
            kind: "task".to_string(),
            task_id: Some(task.id.0.to_string()),
            target: None,
            paths: Vec::new(),
            unresolved_paths: Vec::new(),
        }
    } else {
        return Err(invalid_query_argument_error(
            "afterEdit",
            "provide `taskId`, `target`, or `paths`, or start a PRISM task before calling `afterEdit()` with no arguments",
        ));
    };

    let mut next_reads_out = Vec::<RecommendationSeed>::new();
    let mut tests = Vec::<RecommendationSeed>::new();
    let mut risk_checks = Vec::<RecommendationSeed>::new();

    for target in &targets {
        collect_target_followups(
            execution,
            target,
            &mut next_reads_out,
            &mut tests,
            &mut docs,
            &mut risk_checks,
            &mut contracts,
            &mut notes,
        )?;
    }

    append_doc_fallbacks(execution.workspace_root(), &mut docs);
    append_validation_fallbacks(execution.workspace_root(), &mut tests, &mut risk_checks);

    Ok(serde_json::to_value(AfterEditView {
        subject,
        next_reads: collect_recommendations(next_reads_out, NEXT_READ_LIMIT),
        tests: collect_recommendations(tests, TEST_LIMIT),
        docs: collect_recommendations(docs, DOC_LIMIT),
        risk_checks: collect_recommendations(risk_checks, RISK_LIMIT),
        contracts: collect_contracts(contracts),
        notes,
    })?)
}

fn targets_for_task(
    execution: &QueryExecution,
    task_id: &str,
    notes: &mut Vec<String>,
) -> Result<Vec<NodeId>> {
    let changed_paths = changed_files(
        execution.prism(),
        Some(&TaskId::new(task_id.to_string())),
        None,
        None,
        PATH_TARGET_LIMIT,
    )?
    .into_iter()
    .map(|entry| entry.path)
    .collect::<Vec<_>>();
    if changed_paths.is_empty() {
        notes.push(format!(
            "No changed files were recorded for task `{task_id}`; falling back to task-level docs and validation guidance."
        ));
        return Ok(Vec::new());
    }
    let (targets, unresolved) = resolve_targets_for_paths(
        execution.workspace_root(),
        execution.prism(),
        &changed_paths,
    );
    if !unresolved.is_empty() {
        notes.push(format!(
            "Some changed task paths did not resolve to indexed targets: {}.",
            unresolved.join(", ")
        ));
    }
    Ok(targets)
}

fn collect_target_followups(
    execution: &QueryExecution,
    id: &NodeId,
    next_reads_out: &mut Vec<RecommendationSeed>,
    tests: &mut Vec<RecommendationSeed>,
    docs: &mut Vec<RecommendationSeed>,
    risk_checks: &mut Vec<RecommendationSeed>,
    contracts: &mut Vec<ContractPacketView>,
    notes: &mut Vec<String>,
) -> Result<()> {
    for candidate in next_reads(execution.prism(), id, 3)? {
        next_reads_out.push(RecommendationSeed {
            kind: "read".to_string(),
            label: candidate.symbol.id.path.clone(),
            why: candidate.why.clone(),
            provenance: vec![QueryEvidenceView {
                kind: "next_reads".to_string(),
                detail: format!("Owner-biased read suggestion for `{}`.", id.path),
                path: None,
                line: None,
                target: Some(node_id_view(id.clone())),
            }],
            target: Some(candidate.symbol.id),
            path: None,
            score: Some(candidate.score as f32),
            last_seen: None,
        });
    }

    let recipe = validation_recipe_view_with(execution.prism(), execution.session(), id);
    for check in &recipe.scored_checks {
        tests.push(RecommendationSeed {
            kind: "test".to_string(),
            label: check.label.clone(),
            why: format!(
                "Suggested by the validation recipe for the edited target `{}`.",
                id.path
            ),
            provenance: vec![QueryEvidenceView {
                kind: "validation_recipe".to_string(),
                detail: format!("Validation recipe for `{}`.", id.path),
                path: None,
                line: None,
                target: Some(node_id_view(id.clone())),
            }],
            target: None,
            path: None,
            score: Some(check.score),
            last_seen: Some(check.last_seen),
        });
    }
    for check in &recipe.checks {
        tests.push(RecommendationSeed {
            kind: "test".to_string(),
            label: check.clone(),
            why: format!(
                "Additional validation surfaced for the edited target `{}`.",
                id.path
            ),
            provenance: vec![QueryEvidenceView {
                kind: "validation_recipe".to_string(),
                detail: format!("Validation recipe for `{}`.", id.path),
                path: None,
                line: None,
                target: Some(node_id_view(id.clone())),
            }],
            target: None,
            path: None,
            score: None,
            last_seen: None,
        });
    }

    for spec in execution.prism().spec_for(id) {
        docs.push(RecommendationSeed {
            kind: "doc".to_string(),
            label: spec.path.to_string(),
            why: format!("Specification linked to the edited target `{}`.", id.path),
            provenance: vec![QueryEvidenceView {
                kind: "spec_link".to_string(),
                detail: format!("Spec linked to `{}`.", id.path),
                path: None,
                line: None,
                target: Some(node_id_view(id.clone())),
            }],
            target: Some(node_id_view(spec)),
            path: None,
            score: None,
            last_seen: None,
        });
    }

    let impact = blast_radius_view(execution.prism(), execution.session(), id);
    for node in impact.direct_nodes.iter().take(2) {
        risk_checks.push(RecommendationSeed {
            kind: "risk_check".to_string(),
            label: node.path.clone(),
            why: format!(
                "Downstream target in the semantic blast radius of edited target `{}`.",
                id.path
            ),
            provenance: vec![QueryEvidenceView {
                kind: "blast_radius".to_string(),
                detail: format!("Blast radius for `{}`.", id.path),
                path: None,
                line: None,
                target: Some(node.clone()),
            }],
            target: Some(node.clone()),
            path: None,
            score: None,
            last_seen: None,
        });
    }
    for event in impact.risk_events.iter().take(2) {
        risk_checks.push(RecommendationSeed {
            kind: "risk_check".to_string(),
            label: event.summary.clone(),
            why: format!(
                "Recent failure or risk event related to edited target `{}`.",
                id.path
            ),
            provenance: vec![QueryEvidenceView {
                kind: "risk_event".to_string(),
                detail: format!("Outcome event `{}`.", event.meta.id.0),
                path: None,
                line: None,
                target: Some(node_id_view(id.clone())),
            }],
            target: None,
            path: None,
            score: None,
            last_seen: None,
        });
    }

    collect_contract_followups(
        execution,
        id,
        next_reads_out,
        tests,
        risk_checks,
        contracts,
        notes,
    );

    Ok(())
}

fn append_doc_fallbacks(workspace_root: Option<&Path>, docs: &mut Vec<RecommendationSeed>) {
    let Some(root) = workspace_root else {
        return;
    };
    let agents = root.join("AGENTS.md");
    if agents.is_file() {
        docs.push(RecommendationSeed {
            kind: "doc".to_string(),
            label: "AGENTS.md".to_string(),
            why: "Repo instructions often carry the next validation and workflow expectations after an edit.".to_string(),
            provenance: vec![QueryEvidenceView {
                kind: "repo_instruction".to_string(),
                detail: "Repository-local instructions file.".to_string(),
                path: Some("AGENTS.md".to_string()),
                line: None,
                target: None,
            }],
            target: None,
            path: Some("AGENTS.md".to_string()),
            score: None,
            last_seen: None,
        });
    }
}

fn append_validation_fallbacks(
    workspace_root: Option<&Path>,
    tests: &mut Vec<RecommendationSeed>,
    risk_checks: &mut Vec<RecommendationSeed>,
) {
    let playbook = collect_repo_playbook(workspace_root);
    for section in [&playbook.test, &playbook.lint] {
        if let Some(command) = section.commands.first() {
            tests.push(RecommendationSeed {
                kind: "test".to_string(),
                label: command.clone(),
                why: section.why.clone(),
                provenance: section.provenance.clone(),
                target: None,
                path: None,
                score: None,
                last_seen: None,
            });
        }
    }
    for section in [&playbook.build, &playbook.format] {
        if let Some(command) = section.commands.first() {
            risk_checks.push(RecommendationSeed {
                kind: "risk_check".to_string(),
                label: command.clone(),
                why: section.why.clone(),
                provenance: section.provenance.clone(),
                target: None,
                path: None,
                score: None,
                last_seen: None,
            });
        }
    }
}

fn collect_contract_followups(
    execution: &QueryExecution,
    id: &NodeId,
    next_reads_out: &mut Vec<RecommendationSeed>,
    tests: &mut Vec<RecommendationSeed>,
    risk_checks: &mut Vec<RecommendationSeed>,
    contracts: &mut Vec<ContractPacketView>,
    notes: &mut Vec<String>,
) {
    let packets = execution.prism().contracts_for_target(id);
    if packets.is_empty() {
        return;
    }

    for packet in packets {
        let subject_match = execution
            .prism()
            .contract_subject_matches_target(id, &packet);
        let consumer_match = execution
            .prism()
            .contract_consumer_matches_target(id, &packet);
        contracts.push(contract_packet_view(packet.clone(), None));

        if subject_match {
            for consumer in &packet.consumers {
                for node in execution.prism().contract_target_nodes(consumer, 3) {
                    if node == *id {
                        continue;
                    }
                    next_reads_out.push(RecommendationSeed {
                        kind: "contract_read".to_string(),
                        label: node.path.to_string(),
                        why: format!(
                            "Known consumer of contract `{}` after editing `{}`.",
                            packet.handle, id.path
                        ),
                        provenance: vec![QueryEvidenceView {
                            kind: "contract_consumer".to_string(),
                            detail: format!("Consumer recorded on contract `{}`.", packet.handle),
                            path: None,
                            line: None,
                            target: Some(node_id_view(node.clone())),
                        }],
                        target: Some(node_id_view(node)),
                        path: None,
                        score: None,
                        last_seen: None,
                    });
                }
            }
        }
        if consumer_match {
            for node in execution.prism().contract_target_nodes(&packet.subject, 3) {
                if node == *id {
                    continue;
                }
                next_reads_out.push(RecommendationSeed {
                    kind: "contract_read".to_string(),
                    label: node.path.to_string(),
                    why: format!(
                        "Provider-side subject for contract `{}` after editing consumer `{}`.",
                        packet.handle, id.path
                    ),
                    provenance: vec![QueryEvidenceView {
                        kind: "contract_subject".to_string(),
                        detail: format!("Subject recorded on contract `{}`.", packet.handle),
                        path: None,
                        line: None,
                        target: Some(node_id_view(node.clone())),
                    }],
                    target: Some(node_id_view(node)),
                    path: None,
                    score: None,
                    last_seen: None,
                });
            }
        }

        for validation in &packet.validations {
            tests.push(RecommendationSeed {
                kind: "contract_test".to_string(),
                label: validation.id.clone(),
                why: format!(
                    "Validation tied to contract `{}` after editing `{}`.",
                    packet.handle, id.path
                ),
                provenance: vec![QueryEvidenceView {
                    kind: "contract_validation".to_string(),
                    detail: format!("Validation recorded on contract `{}`.", packet.handle),
                    path: None,
                    line: None,
                    target: Some(node_id_view(id.clone())),
                }],
                target: None,
                path: None,
                score: None,
                last_seen: None,
            });
        }

        for detail in packet
            .compatibility
            .migrating
            .iter()
            .chain(packet.compatibility.risky.iter())
            .chain(packet.compatibility.breaking.iter())
            .take(3)
        {
            risk_checks.push(RecommendationSeed {
                kind: "contract_risk".to_string(),
                label: detail.clone(),
                why: format!(
                    "Compatibility or migration guidance from contract `{}`.",
                    packet.handle
                ),
                provenance: vec![QueryEvidenceView {
                    kind: "contract_compatibility".to_string(),
                    detail: format!("Compatibility note on contract `{}`.", packet.handle),
                    path: None,
                    line: None,
                    target: Some(node_id_view(id.clone())),
                }],
                target: None,
                path: None,
                score: None,
                last_seen: None,
            });
        }

        if !packet.compatibility.migrating.is_empty()
            || !packet.compatibility.risky.is_empty()
            || !packet.compatibility.breaking.is_empty()
        {
            let mut details = packet.compatibility.migrating.clone();
            details.extend(packet.compatibility.risky.clone());
            details.extend(packet.compatibility.breaking.clone());
            notes.push(format!(
                "Contract `{}` compatibility notes: {}.",
                packet.handle,
                details.into_iter().take(3).collect::<Vec<_>>().join(" ")
            ));
        }
    }
}

fn collect_contracts(contracts: Vec<ContractPacketView>) -> Vec<ContractPacketView> {
    let mut merged = BTreeMap::<String, ContractPacketView>::new();
    for contract in contracts {
        merged.entry(contract.handle.clone()).or_insert(contract);
    }
    merged.into_values().collect()
}

fn collect_recommendations(
    seeds: Vec<RecommendationSeed>,
    limit: usize,
) -> Vec<QueryRecommendationView> {
    let mut merged = BTreeMap::<String, RecommendationSeed>::new();
    for seed in seeds {
        let key = recommendation_key(&seed);
        merged
            .entry(key)
            .and_modify(|existing| {
                if seed.score.unwrap_or_default() > existing.score.unwrap_or_default() {
                    existing.score = seed.score;
                    existing.last_seen = seed.last_seen;
                    existing.why = seed.why.clone();
                }
                for item in &seed.provenance {
                    if !existing.provenance.iter().any(|current| current == item) {
                        existing.provenance.push(item.clone());
                    }
                }
            })
            .or_insert(seed);
    }

    let mut values = merged.into_values().collect::<Vec<_>>();
    values.sort_by(|left, right| {
        right
            .score
            .unwrap_or_default()
            .partial_cmp(&left.score.unwrap_or_default())
            .unwrap_or(std::cmp::Ordering::Equal)
            .then_with(|| left.kind.cmp(&right.kind))
            .then_with(|| left.label.cmp(&right.label))
    });

    values
        .into_iter()
        .take(limit)
        .map(|seed| QueryRecommendationView {
            kind: seed.kind,
            label: seed.label,
            why: seed.why,
            provenance: seed.provenance,
            target: seed.target,
            path: seed.path,
            score: seed.score,
            last_seen: seed.last_seen,
        })
        .collect()
}

fn recommendation_key(seed: &RecommendationSeed) -> String {
    format!(
        "{}::{}::{}::{}",
        seed.kind,
        seed.label,
        seed.target
            .as_ref()
            .map(|target| target.path.as_str())
            .unwrap_or_default(),
        seed.path.as_deref().unwrap_or_default()
    )
}

fn resolve_targets_for_paths(
    workspace_root: Option<&Path>,
    prism: &prism_query::Prism,
    paths: &[String],
) -> (Vec<NodeId>, Vec<String>) {
    let mut matched_paths = BTreeSet::<String>::new();
    let mut ids = prism
        .graph()
        .all_nodes()
        .filter_map(|node| {
            let file_path = prism.graph().file_path(node.file)?;
            let actual = file_path.to_string_lossy().into_owned();
            let matched = paths
                .iter()
                .find(|path| same_workspace_file(workspace_root, path.as_str(), &actual))?;
            matched_paths.insert(matched.clone());
            Some(node.id.clone())
        })
        .collect::<Vec<_>>();
    ids.sort_by(|left, right| {
        left.path
            .cmp(&right.path)
            .then_with(|| left.crate_name.cmp(&right.crate_name))
            .then_with(|| format!("{:?}", left.kind).cmp(&format!("{:?}", right.kind)))
    });
    ids.dedup();
    ids.truncate(PATH_TARGET_LIMIT);
    let unresolved = paths
        .iter()
        .filter(|path| !matched_paths.contains(path.as_str()))
        .cloned()
        .collect::<Vec<_>>();
    (ids, unresolved)
}
