use std::collections::{BTreeMap, BTreeSet};
use std::path::Path;

use anyhow::Result;
use prism_ir::{CoordinationTaskId, NodeId, TaskId};
use prism_js::{
    ContractPacketView, ImpactView, QueryEvidenceView, QueryRecommendationView, QueryRiskHintView,
    QueryViewSubjectView,
};
use serde::Deserialize;
use serde_json::Value;

use crate::compact_followups::same_workspace_file;
use crate::query_view_materialization::append_boundary_notes_for_paths;
use crate::query_view_playbook::collect_repo_playbook;
use crate::{
    blast_radius_view, change_impact_view, changed_files, contract_packet_view,
    invalid_query_argument_error, node_id_view, promoted_summary_texts, QueryExecution,
    SymbolTargetArgs,
};

const DOWNSTREAM_LIMIT: usize = 6;
const CHECK_LIMIT: usize = 6;
const RISK_LIMIT: usize = 4;
const PATH_TARGET_LIMIT: usize = 16;

#[derive(Debug, Deserialize, Default)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct ImpactInput {
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

#[derive(Debug, Clone)]
struct RiskSeed {
    summary: String,
    why: String,
    provenance: Vec<QueryEvidenceView>,
}

pub(crate) fn impact_view(execution: &QueryExecution, input: Value) -> Result<Value> {
    let input: ImpactInput = serde_json::from_value(input)
        .map_err(|error| invalid_query_argument_error("impact", error.to_string()))?;
    let subject_count = usize::from(input.task_id.is_some())
        + usize::from(input.target.is_some())
        + usize::from(input.paths.as_ref().is_some_and(|paths| !paths.is_empty()));
    if subject_count != 1 {
        return Err(invalid_query_argument_error(
            "impact",
            "provide exactly one of `taskId`, `target`, or `paths`",
        ));
    }

    let mut notes = Vec::<String>::new();
    let mut downstream = Vec::<RecommendationSeed>::new();
    let mut recommended_checks = Vec::<RecommendationSeed>::new();
    let mut risks = Vec::<RiskSeed>::new();
    let mut contracts = Vec::<ContractPacketView>::new();

    let subject = if let Some(task_id) = input.task_id {
        collect_task_impact(
            execution,
            &task_id,
            &mut downstream,
            &mut recommended_checks,
            &mut risks,
            &mut contracts,
            &mut notes,
        )?;
        QueryViewSubjectView {
            kind: "task".to_string(),
            task_id: Some(task_id),
            target: None,
            paths: Vec::new(),
            unresolved_paths: Vec::new(),
        }
    } else if let Some(target) = input.target {
        let id = execution.resolve_target_id(target.id, target.lineage_id)?;
        collect_target_impact(
            execution,
            &id,
            &mut downstream,
            &mut recommended_checks,
            &mut risks,
            &mut contracts,
            &mut notes,
        );
        QueryViewSubjectView {
            kind: "target".to_string(),
            task_id: None,
            target: Some(node_id_view(id)),
            paths: Vec::new(),
            unresolved_paths: Vec::new(),
        }
    } else {
        let paths = input.paths.unwrap_or_default();
        let (resolved_targets, unresolved_paths) =
            resolve_targets_for_paths(execution.workspace_root(), execution.prism(), &paths);
        if resolved_targets.is_empty() {
            notes.push(
                "No indexed targets matched the requested paths; the impact view could not infer a semantic blast radius."
                    .to_string(),
            );
        }
        if !unresolved_paths.is_empty() {
            notes.push(format!(
                "Some requested paths did not resolve to indexed targets: {}.",
                unresolved_paths.join(", ")
            ));
            append_boundary_notes_for_paths(execution, &unresolved_paths, &mut notes);
        }
        for id in &resolved_targets {
            collect_target_impact(
                execution,
                id,
                &mut downstream,
                &mut recommended_checks,
                &mut risks,
                &mut contracts,
                &mut notes,
            );
        }
        QueryViewSubjectView {
            kind: "pathSet".to_string(),
            task_id: None,
            target: None,
            paths,
            unresolved_paths,
        }
    };

    append_check_fallbacks(execution.workspace_root(), &mut recommended_checks);
    if recommended_checks.is_empty() {
        recommended_checks.push(RecommendationSeed {
            kind: "check".to_string(),
            label: "Inspect the highest-ranked downstream targets".to_string(),
            why: "No explicit validation recipe was available, so start with the strongest downstream targets surfaced by the impact view.".to_string(),
            provenance: vec![QueryEvidenceView {
                kind: "impact_fallback".to_string(),
                detail: "Generated from the impact view when no stronger validation recipe or repo playbook command was available.".to_string(),
                path: None,
                line: None,
                target: None,
            }],
            target: None,
            path: None,
            score: None,
            last_seen: None,
        });
    }
    let downstream = collect_recommendations(downstream, DOWNSTREAM_LIMIT);
    let recommended_checks = collect_recommendations(recommended_checks, CHECK_LIMIT);
    let mut risks = collect_risks(risks, RISK_LIMIT);
    if risks.is_empty() && (!downstream.is_empty() || !recommended_checks.is_empty()) {
        risks.push(QueryRiskHintView {
            summary: "Validate the highest-ranked downstream consumers first.".to_string(),
            why: "The semantic blast radius surfaced likely downstream targets or validations, even though no recent explicit risk event was recorded.".to_string(),
            provenance: vec![QueryEvidenceView {
                kind: "impact_fallback".to_string(),
                detail: "Generated from the semantic blast radius and validation signals.".to_string(),
                path: None,
                line: None,
                target: None,
            }],
        });
    }

    Ok(serde_json::to_value(ImpactView {
        subject,
        downstream,
        risks,
        recommended_checks,
        contracts: collect_contracts(contracts),
        notes,
    })?)
}

fn collect_task_impact(
    execution: &QueryExecution,
    task_id: &str,
    downstream: &mut Vec<RecommendationSeed>,
    recommended_checks: &mut Vec<RecommendationSeed>,
    risks: &mut Vec<RiskSeed>,
    contracts: &mut Vec<ContractPacketView>,
    notes: &mut Vec<String>,
) -> Result<()> {
    let task = TaskId::new(task_id.to_string());
    let mut handled = false;

    if let Some(coordination_task_id) = coordination_task_id(task_id) {
        if let Some(impact) = execution.prism().task_blast_radius(&coordination_task_id) {
            let anchors = execution
                .prism()
                .coordination_task_v2_by_coordination_id(&coordination_task_id)
                .map(|task| task.task.anchors)
                .unwrap_or_default();
            let mut view = change_impact_view(impact);
            view.promoted_summaries =
                promoted_summary_texts(execution.session(), execution.prism(), &anchors);
            collect_from_impact_view(
                task_id,
                "task_blast_radius",
                &view,
                downstream,
                recommended_checks,
                risks,
            );
            for anchor in &anchors {
                if let prism_ir::AnchorRef::Node(node) = anchor {
                    collect_contract_impact(
                        execution,
                        node,
                        downstream,
                        recommended_checks,
                        risks,
                        contracts,
                        notes,
                    );
                }
            }
            handled = true;
        } else {
            notes.push(format!(
                "No coordination-task blast radius was recorded for `{task_id}`; falling back to changed-file aggregation."
            ));
        }
    }

    if handled {
        return Ok(());
    }

    let changed_paths = changed_files(
        execution.prism(),
        Some(&task),
        None,
        None,
        PATH_TARGET_LIMIT,
    )?
    .into_iter()
    .map(|entry| entry.path)
    .collect::<Vec<_>>();
    if changed_paths.is_empty() {
        notes.push(format!(
            "No recent changed files were recorded for task `{task_id}`."
        ));
        return Ok(());
    }
    let (targets, unresolved) = resolve_targets_for_paths(
        execution.workspace_root(),
        execution.prism(),
        &changed_paths,
    );
    if !unresolved.is_empty() {
        append_boundary_notes_for_paths(execution, &unresolved, notes);
    }
    if targets.is_empty() {
        notes.push(format!(
            "Changed files for `{task_id}` did not resolve to indexed semantic targets."
        ));
        return Ok(());
    }
    for target in &targets {
        collect_target_impact(
            execution,
            target,
            downstream,
            recommended_checks,
            risks,
            contracts,
            notes,
        );
    }
    Ok(())
}

fn collect_target_impact(
    execution: &QueryExecution,
    id: &NodeId,
    downstream: &mut Vec<RecommendationSeed>,
    recommended_checks: &mut Vec<RecommendationSeed>,
    risks: &mut Vec<RiskSeed>,
    contracts: &mut Vec<ContractPacketView>,
    notes: &mut Vec<String>,
) {
    let view = blast_radius_view(execution.prism(), execution.session(), id);
    collect_from_impact_view(
        &id.path,
        "blast_radius",
        &view,
        downstream,
        recommended_checks,
        risks,
    );
    collect_contract_impact(
        execution,
        id,
        downstream,
        recommended_checks,
        risks,
        contracts,
        notes,
    );
}

fn collect_from_impact_view(
    source_label: &str,
    source_kind: &str,
    view: &prism_js::ChangeImpactView,
    downstream: &mut Vec<RecommendationSeed>,
    recommended_checks: &mut Vec<RecommendationSeed>,
    risks: &mut Vec<RiskSeed>,
) {
    for node in &view.direct_nodes {
        downstream.push(RecommendationSeed {
            kind: "target".to_string(),
            label: node.path.clone(),
            why: format!(
                "Likely downstream target from the semantic blast radius for `{source_label}`."
            ),
            provenance: vec![QueryEvidenceView {
                kind: source_kind.to_string(),
                detail: format!("Blast radius anchored at `{source_label}`."),
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

    for neighbor in &view.co_change_neighbors {
        for node in &neighbor.nodes {
            downstream.push(RecommendationSeed {
                kind: "coChange".to_string(),
                label: node.path.clone(),
                why: format!(
                    "Co-change neighbor for `{source_label}` with lineage `{}` seen {} times.",
                    neighbor.lineage, neighbor.count
                ),
                provenance: vec![QueryEvidenceView {
                    kind: "co_change_neighbor".to_string(),
                    detail: format!(
                        "Lineage `{}` co-changed {} times with `{source_label}`.",
                        neighbor.lineage, neighbor.count
                    ),
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
    }

    for check in &view.validation_checks {
        recommended_checks.push(RecommendationSeed {
            kind: "check".to_string(),
            label: check.label.clone(),
            why: format!(
                "High-confidence validation from the semantic blast radius for `{source_label}`."
            ),
            provenance: vec![QueryEvidenceView {
                kind: source_kind.to_string(),
                detail: format!(
                    "Validation check surfaced by the impact view for `{source_label}`."
                ),
                path: None,
                line: None,
                target: None,
            }],
            target: None,
            path: None,
            score: Some(check.score),
            last_seen: Some(check.last_seen),
        });
    }

    for label in &view.likely_validations {
        recommended_checks.push(RecommendationSeed {
            kind: "check".to_string(),
            label: label.clone(),
            why: format!(
                "Additional likely validation from the blast radius for `{source_label}`."
            ),
            provenance: vec![QueryEvidenceView {
                kind: source_kind.to_string(),
                detail: format!(
                    "Likely validation surfaced by the impact view for `{source_label}`."
                ),
                path: None,
                line: None,
                target: None,
            }],
            target: None,
            path: None,
            score: None,
            last_seen: None,
        });
    }

    for event in &view.risk_events {
        risks.push(RiskSeed {
            summary: event.summary.clone(),
            why: format!("Recent recorded risk or failure event related to `{source_label}`."),
            provenance: vec![QueryEvidenceView {
                kind: "risk_event".to_string(),
                detail: format!("Outcome event `{}`.", event.meta.id.0),
                path: None,
                line: None,
                target: None,
            }],
        });
    }

    for summary in &view.promoted_summaries {
        risks.push(RiskSeed {
            summary: summary.clone(),
            why: format!("Promoted summary preserved as durable context for `{source_label}`."),
            provenance: vec![QueryEvidenceView {
                kind: "promoted_summary".to_string(),
                detail: format!("Promoted summary for `{source_label}`."),
                path: None,
                line: None,
                target: None,
            }],
        });
    }
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

fn collect_risks(seeds: Vec<RiskSeed>, limit: usize) -> Vec<QueryRiskHintView> {
    let mut seen = BTreeSet::<String>::new();
    let mut risks = Vec::new();
    for seed in seeds {
        let key = format!("{}::{}", seed.summary, seed.why);
        if !seen.insert(key) {
            continue;
        }
        risks.push(QueryRiskHintView {
            summary: seed.summary,
            why: seed.why,
            provenance: seed.provenance,
        });
        if risks.len() >= limit {
            break;
        }
    }
    risks
}

fn append_check_fallbacks(
    workspace_root: Option<&Path>,
    recommended_checks: &mut Vec<RecommendationSeed>,
) {
    let playbook = collect_repo_playbook(workspace_root);
    for section in [&playbook.test, &playbook.lint, &playbook.build] {
        if let Some(command) = section.commands.first() {
            recommended_checks.push(RecommendationSeed {
                kind: "check".to_string(),
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

fn collect_contract_impact(
    execution: &QueryExecution,
    id: &NodeId,
    downstream: &mut Vec<RecommendationSeed>,
    recommended_checks: &mut Vec<RecommendationSeed>,
    risks: &mut Vec<RiskSeed>,
    contracts: &mut Vec<ContractPacketView>,
    notes: &mut Vec<String>,
) {
    let packets = execution.prism().contracts_for_target(id);
    if packets.is_empty() {
        return;
    }

    let subject_match = packets.iter().any(|packet| {
        execution
            .prism()
            .contract_subject_matches_target(id, packet)
    });
    let consumer_match = packets.iter().any(|packet| {
        execution
            .prism()
            .contract_consumer_matches_target(id, packet)
    });
    if subject_match && consumer_match {
        notes.push(format!(
            "Target `{}` is both a contract subject and a known consumer, so this change may affect a promise boundary directly.",
            id.path
        ));
    } else if subject_match {
        notes.push(format!(
            "Target `{}` appears in a contract subject, so behavior changes may be contract-affecting.",
            id.path
        ));
    } else if consumer_match {
        notes.push(format!(
            "Target `{}` is a known contract consumer; validate compatibility against the governing promise.",
            id.path
        ));
    }

    for packet in packets {
        contracts.push(contract_packet_view(
            execution.prism(),
            execution.workspace_root(),
            packet.clone(),
            None,
        ));

        for consumer in &packet.consumers {
            for node in execution.prism().contract_target_nodes(consumer, 4) {
                if node == *id {
                    continue;
                }
                downstream.push(RecommendationSeed {
                    kind: "contract_consumer".to_string(),
                    label: node.path.to_string(),
                    why: format!(
                        "Known consumer of contract `{}` relevant to `{}`.",
                        packet.handle, id.path
                    ),
                    provenance: vec![QueryEvidenceView {
                        kind: "contract_consumer".to_string(),
                        detail: format!("Consumer captured on contract `{}`.", packet.handle),
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
            recommended_checks.push(RecommendationSeed {
                kind: "contract_check".to_string(),
                label: validation.id.clone(),
                why: format!(
                    "Validation linked to contract `{}` for target `{}`.",
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
            .breaking
            .iter()
            .chain(packet.compatibility.risky.iter())
            .chain(packet.compatibility.migrating.iter())
            .take(3)
        {
            risks.push(RiskSeed {
                summary: detail.clone(),
                why: format!(
                    "Compatibility guidance recorded on contract `{}` relevant to `{}`.",
                    packet.handle, id.path
                ),
                provenance: vec![QueryEvidenceView {
                    kind: "contract_compatibility".to_string(),
                    detail: format!("Compatibility note on contract `{}`.", packet.handle),
                    path: None,
                    line: None,
                    target: Some(node_id_view(id.clone())),
                }],
            });
        }
        for assumption in packet.assumptions.iter().take(2) {
            risks.push(RiskSeed {
                summary: assumption.clone(),
                why: format!(
                    "Assumption that limits when contract `{}` holds for `{}`.",
                    packet.handle, id.path
                ),
                provenance: vec![QueryEvidenceView {
                    kind: "contract_assumption".to_string(),
                    detail: format!("Assumption recorded on contract `{}`.", packet.handle),
                    path: None,
                    line: None,
                    target: Some(node_id_view(id.clone())),
                }],
            });
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

fn coordination_task_id(task_id: &str) -> Option<CoordinationTaskId> {
    task_id
        .starts_with("coord-task:")
        .then(|| CoordinationTaskId::new(task_id.to_string()))
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
            let file_path = prism.graph().runtime_file_path(node.file)?;
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
