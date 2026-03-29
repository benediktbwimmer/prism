use std::collections::{BTreeMap, BTreeSet};
use std::path::Path;

use anyhow::Result;
use prism_ir::NodeId;
use prism_js::{
    NodeIdView, QueryEvidenceView, ValidationPlanCheckView, ValidationPlanSubjectView,
    ValidationPlanView,
};
use serde::Deserialize;
use serde_json::Value;

use crate::compact_followups::same_workspace_file;
use crate::query_view_playbook::collect_repo_playbook;
use crate::{
    invalid_query_argument_error, node_id_view, validation_recipe_view_with, QueryExecution,
    SymbolTargetArgs,
};
use prism_ir::CoordinationTaskId;

const FAST_CHECK_LIMIT: usize = 3;
const BROADER_CHECK_LIMIT: usize = 4;
const PATH_TARGET_LIMIT: usize = 16;

#[derive(Debug, Deserialize, Default)]
#[serde(rename_all = "camelCase")]
struct ValidationPlanInput {
    task_id: Option<String>,
    target: Option<SymbolTargetArgs>,
    paths: Option<Vec<String>>,
}

#[derive(Debug, Clone)]
struct CheckSeed {
    label: String,
    why: String,
    provenance: Vec<QueryEvidenceView>,
    score: Option<f32>,
    last_seen: Option<u64>,
}

pub(crate) fn validation_plan_view(execution: &QueryExecution, input: Value) -> Result<Value> {
    let input: ValidationPlanInput = serde_json::from_value(input)
        .map_err(|error| invalid_query_argument_error("validationPlan", error.to_string()))?;
    let subject_count = usize::from(input.task_id.is_some())
        + usize::from(input.target.is_some())
        + usize::from(input.paths.as_ref().is_some_and(|paths| !paths.is_empty()));
    if subject_count != 1 {
        return Err(invalid_query_argument_error(
            "validationPlan",
            "provide exactly one of `taskId`, `target`, or `paths`",
        ));
    }

    let mut notes = Vec::<String>::new();
    let mut related_targets = Vec::<NodeId>::new();
    let mut scored = Vec::<CheckSeed>::new();
    let mut broader = Vec::<CheckSeed>::new();

    let subject = if let Some(task_id) = input.task_id {
        let coordination_task_id = CoordinationTaskId::new(task_id.clone());
        if let Some(recipe) = execution
            .prism()
            .task_validation_recipe(&coordination_task_id)
        {
            scored.extend(recipe.scored_checks.iter().map(|check| CheckSeed {
                label: check.label.clone(),
                why: format!(
                    "High-confidence task validation for `{task_id}` from the recorded task validation recipe."
                ),
                provenance: vec![QueryEvidenceView {
                    kind: "task_validation_recipe".to_string(),
                    detail: format!("Task validation recipe for `{task_id}`."),
                    path: None,
                    line: None,
                    target: None,
                }],
                score: Some(check.score),
                last_seen: Some(check.last_seen),
            }));
            broader.extend(recipe.checks.iter().map(|check| CheckSeed {
                label: check.clone(),
                why: format!(
                    "Additional task-level validation for `{task_id}` from the recorded task validation recipe."
                ),
                provenance: vec![QueryEvidenceView {
                    kind: "task_validation_recipe".to_string(),
                    detail: format!("Task validation recipe for `{task_id}`."),
                    path: None,
                    line: None,
                    target: None,
                }],
                score: None,
                last_seen: None,
            }));
            related_targets.extend(recipe.related_nodes);
        } else {
            notes.push(format!(
                "No recorded task validation recipe was found for `{task_id}`; falling back to repo-wide workflow guidance."
            ));
        }
        ValidationPlanSubjectView {
            kind: "task".to_string(),
            task_id: Some(task_id),
            target: None,
            paths: Vec::new(),
            unresolved_paths: Vec::new(),
        }
    } else if let Some(target) = input.target {
        let id = execution.resolve_target_id(target.id, target.lineage_id)?;
        let recipe = validation_recipe_view_with(execution.prism(), execution.session(), &id);
        scored.extend(recipe.scored_checks.iter().map(|check| CheckSeed {
            label: check.label.clone(),
            why: format!(
                "High-confidence target validation for `{}` from the semantic validation recipe.",
                id.path
            ),
            provenance: vec![QueryEvidenceView {
                kind: "validation_recipe".to_string(),
                detail: format!("Semantic validation recipe for `{}`.", id.path),
                path: None,
                line: None,
                target: Some(node_id_view(id.clone())),
            }],
            score: Some(check.score),
            last_seen: Some(check.last_seen),
        }));
        broader.extend(recipe.checks.iter().map(|check| CheckSeed {
            label: check.clone(),
            why: format!(
                "Additional target validation for `{}` from the semantic validation recipe.",
                id.path
            ),
            provenance: vec![QueryEvidenceView {
                kind: "validation_recipe".to_string(),
                detail: format!("Semantic validation recipe for `{}`.", id.path),
                path: None,
                line: None,
                target: Some(node_id_view(id.clone())),
            }],
            score: None,
            last_seen: None,
        }));
        related_targets.push(id.clone());
        related_targets.extend(recipe.related_nodes.into_iter().map(node_id_from_view));
        ValidationPlanSubjectView {
            kind: "target".to_string(),
            task_id: None,
            target: Some(node_id_view(id)),
            paths: Vec::new(),
            unresolved_paths: Vec::new(),
        }
    } else {
        let paths = input.paths.unwrap_or_default();
        let (resolved_targets, unresolved_paths) =
            resolve_targets_for_paths(execution.prism(), execution.workspace_root(), &paths);
        if resolved_targets.is_empty() {
            notes.push(
                "No indexed targets matched the requested paths; falling back to repo-wide workflow guidance."
                    .to_string(),
            );
        }
        if !unresolved_paths.is_empty() {
            notes.push(format!(
                "Some requested paths did not resolve to indexed targets: {}.",
                unresolved_paths.join(", ")
            ));
        }
        for id in &resolved_targets {
            let recipe = validation_recipe_view_with(execution.prism(), execution.session(), id);
            scored.extend(recipe.scored_checks.iter().map(|check| CheckSeed {
                label: check.label.clone(),
                why: format!(
                    "High-confidence validation for `{}` because it was resolved from the requested path set.",
                    id.path
                ),
                provenance: vec![
                    QueryEvidenceView {
                        kind: "validation_recipe".to_string(),
                        detail: format!("Semantic validation recipe for `{}`.", id.path),
                        path: None,
                        line: None,
                        target: Some(node_id_view(id.clone())),
                    },
                    QueryEvidenceView {
                        kind: "path_resolution".to_string(),
                        detail: "Resolved from the requested path set.".to_string(),
                        path: None,
                        line: None,
                        target: Some(node_id_view(id.clone())),
                    },
                ],
                score: Some(check.score),
                last_seen: Some(check.last_seen),
            }));
            broader.extend(recipe.checks.iter().map(|check| CheckSeed {
                label: check.clone(),
                why: format!(
                    "Additional validation for `{}` because it was resolved from the requested path set.",
                    id.path
                ),
                provenance: vec![
                    QueryEvidenceView {
                        kind: "validation_recipe".to_string(),
                        detail: format!("Semantic validation recipe for `{}`.", id.path),
                        path: None,
                        line: None,
                        target: Some(node_id_view(id.clone())),
                    },
                    QueryEvidenceView {
                        kind: "path_resolution".to_string(),
                        detail: "Resolved from the requested path set.".to_string(),
                        path: None,
                        line: None,
                        target: Some(node_id_view(id.clone())),
                    },
                ],
                score: None,
                last_seen: None,
            }));
        }
        related_targets.extend(resolved_targets.iter().cloned());
        ValidationPlanSubjectView {
            kind: "pathSet".to_string(),
            task_id: None,
            target: None,
            paths,
            unresolved_paths,
        }
    };

    let mut fast = collect_ranked_checks(scored, FAST_CHECK_LIMIT);
    let mut broader_checks = collect_ranked_checks(broader, FAST_CHECK_LIMIT + BROADER_CHECK_LIMIT);
    broader_checks.retain(|candidate| {
        !fast
            .iter()
            .any(|existing| existing.label == candidate.label)
    });
    broader_checks.truncate(BROADER_CHECK_LIMIT);

    if fast.is_empty() && broader_checks.is_empty() {
        let fallback = repo_playbook_fallback(execution.workspace_root());
        fast = fallback.0;
        broader_checks = fallback.1;
        if fast.is_empty() && broader_checks.is_empty() {
            notes.push(
                "Repo-wide workflow guidance also lacked validation commands for this workspace."
                    .to_string(),
            );
        }
    }

    related_targets.sort_by(|left, right| {
        left.path
            .cmp(&right.path)
            .then_with(|| left.crate_name.cmp(&right.crate_name))
            .then_with(|| format!("{:?}", left.kind).cmp(&format!("{:?}", right.kind)))
    });
    related_targets.dedup();
    let mut related_target_views = related_targets
        .into_iter()
        .map(node_id_view)
        .collect::<Vec<NodeIdView>>();
    related_target_views.sort_by(|left, right| {
        left.path
            .cmp(&right.path)
            .then_with(|| left.crate_name.cmp(&right.crate_name))
            .then_with(|| format!("{:?}", left.kind).cmp(&format!("{:?}", right.kind)))
    });

    Ok(serde_json::to_value(ValidationPlanView {
        subject,
        fast,
        broader: broader_checks,
        related_targets: related_target_views,
        notes,
    })?)
}

fn collect_ranked_checks(seeds: Vec<CheckSeed>, limit: usize) -> Vec<ValidationPlanCheckView> {
    let mut merged = BTreeMap::<String, CheckSeed>::new();
    for seed in seeds {
        merged
            .entry(seed.label.clone())
            .and_modify(|existing| {
                if seed.score.unwrap_or_default() > existing.score.unwrap_or_default() {
                    existing.score = seed.score;
                    existing.last_seen = seed.last_seen;
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
            .then_with(|| left.label.cmp(&right.label))
    });
    values
        .into_iter()
        .take(limit)
        .map(|seed| ValidationPlanCheckView {
            label: seed.label,
            why: seed.why,
            provenance: seed.provenance,
            score: seed.score,
            last_seen: seed.last_seen,
        })
        .collect()
}

fn repo_playbook_fallback(
    workspace_root: Option<&Path>,
) -> (Vec<ValidationPlanCheckView>, Vec<ValidationPlanCheckView>) {
    let playbook = collect_repo_playbook(workspace_root);
    let mut fast = Vec::new();
    let mut broader = Vec::new();

    for section in [&playbook.lint, &playbook.test] {
        if let Some(command) = section.commands.first() {
            fast.push(ValidationPlanCheckView {
                label: command.clone(),
                why: section.why.clone(),
                provenance: section.provenance.clone(),
                score: None,
                last_seen: None,
            });
        }
    }
    for section in [&playbook.build, &playbook.format] {
        if let Some(command) = section.commands.first() {
            broader.push(ValidationPlanCheckView {
                label: command.clone(),
                why: section.why.clone(),
                provenance: section.provenance.clone(),
                score: None,
                last_seen: None,
            });
        }
    }

    fast.truncate(FAST_CHECK_LIMIT);
    broader.truncate(BROADER_CHECK_LIMIT);
    (fast, broader)
}

fn resolve_targets_for_paths(
    prism: &prism_query::Prism,
    workspace_root: Option<&Path>,
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
    if ids.len() > PATH_TARGET_LIMIT {
        ids.truncate(PATH_TARGET_LIMIT);
    }
    let unresolved = paths
        .iter()
        .filter(|path| !matched_paths.contains(*path))
        .cloned()
        .collect::<Vec<_>>();
    (ids, unresolved)
}

fn node_id_from_view(view: NodeIdView) -> NodeId {
    NodeId::new(&view.crate_name, &view.path, view.kind)
}
