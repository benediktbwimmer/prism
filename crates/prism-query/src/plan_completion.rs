use std::time::{SystemTime, UNIX_EPOCH};

use anyhow::{anyhow, Result};
use prism_coordination::{Artifact, CoordinationPolicy};
use prism_ir::{
    ArtifactId, ArtifactStatus, ConflictSeverity, CoordinationTaskId, PlanExecutionOverlay,
    PlanGraph, PlanId, PlanNode, PlanNodeBlocker, PlanNodeBlockerKind, PlanNodeId, PlanNodeStatus,
    Timestamp,
};

use crate::impact::score_change_impact;
use crate::plan_runtime::{
    node_blockers_for_graph, required_validation_checks_for_node, NativePlanRuntimeState,
};
use crate::Prism;

impl Prism {
    pub fn plan_node_blockers(
        &self,
        plan_id: &PlanId,
        node_id: &PlanNodeId,
    ) -> Vec<PlanNodeBlocker> {
        let runtime = self
            .plan_runtime
            .read()
            .expect("plan runtime lock poisoned")
            .clone();
        self.plan_node_blockers_for_runtime(&runtime, plan_id, node_id, current_timestamp())
    }

    pub(crate) fn validate_native_plan_node_completion_preview(
        &self,
        runtime: &NativePlanRuntimeState,
        plan_id: &PlanId,
        node_id: &PlanNodeId,
    ) -> Result<()> {
        let blockers =
            self.plan_node_blockers_for_runtime(runtime, plan_id, node_id, current_timestamp());
        if blockers.is_empty() {
            return Ok(());
        }
        Err(anyhow!(
            "plan node `{}` cannot complete: {}",
            node_id.0,
            blockers
                .into_iter()
                .map(|blocker| blocker.summary)
                .collect::<Vec<_>>()
                .join("; ")
        ))
    }

    pub(crate) fn plan_node_blockers_for_runtime(
        &self,
        runtime: &NativePlanRuntimeState,
        plan_id: &PlanId,
        node_id: &PlanNodeId,
        now: Timestamp,
    ) -> Vec<PlanNodeBlocker> {
        let Some(graph) = runtime.plan_graph(plan_id) else {
            return Vec::new();
        };
        let Some(node) = graph.nodes.iter().find(|node| node.id == *node_id).cloned() else {
            return Vec::new();
        };
        let overlays = runtime.plan_execution(plan_id);
        let mut blockers = node_blockers_for_graph(&graph, &overlays, node_id);
        blockers.extend(self.native_policy_blockers_for_node(
            runtime.policy(plan_id),
            &graph,
            &node,
            &overlays,
            now,
        ));
        sort_and_dedupe_plan_node_blockers(&mut blockers);
        blockers
    }

    fn native_policy_blockers_for_node(
        &self,
        policy: Option<CoordinationPolicy>,
        graph: &PlanGraph,
        node: &PlanNode,
        _overlays: &[PlanExecutionOverlay],
        now: Timestamp,
    ) -> Vec<PlanNodeBlocker> {
        let Some(policy) = policy else {
            return Vec::new();
        };
        if node.is_abstract || matches!(node.status, PlanNodeStatus::Abandoned) {
            return Vec::new();
        }

        let workspace_revision = self.workspace_revision();
        let artifacts = self.artifacts_for_plan_node(&graph.id, node);
        let approved_artifacts = artifacts
            .iter()
            .filter(|artifact| {
                matches!(
                    artifact.status,
                    ArtifactStatus::Approved | ArtifactStatus::Merged
                )
            })
            .cloned()
            .collect::<Vec<_>>();
        let validated_checks = approved_artifacts
            .iter()
            .flat_map(|artifact| artifact.validated_checks.iter().cloned())
            .collect::<Vec<_>>();
        let validated_checks = dedupe_strings(validated_checks);
        let stale_artifact_ids = approved_artifacts
            .iter()
            .filter(|artifact| {
                artifact.base_revision.graph_version < workspace_revision.graph_version
            })
            .map(|artifact| artifact.id.clone())
            .collect::<Vec<_>>();
        let stale_workspace_binding = policy.stale_after_graph_change
            && node_is_workspace_bound(node)
            && node.base_revision.graph_version < workspace_revision.graph_version;

        let mut blockers = Vec::new();
        if stale_workspace_binding {
            blockers.push(PlanNodeBlocker {
                kind: PlanNodeBlockerKind::StaleRevision,
                summary: format!(
                    "plan node is based on graph version {} but current revision is {}",
                    node.base_revision.graph_version, workspace_revision.graph_version
                ),
                related_node_id: Some(node.id.clone()),
                related_artifact_id: None,
                risk_score: None,
                validation_checks: Vec::new(),
            });
        }
        if policy.stale_after_graph_change {
            if let Some(stale_artifact_id) = stale_artifact_ids.first() {
                blockers.push(PlanNodeBlocker {
                    kind: PlanNodeBlockerKind::ArtifactStale,
                    summary: format!(
                        "approved artifact `{}` is stale against graph version {}",
                        stale_artifact_id.0, workspace_revision.graph_version
                    ),
                    related_node_id: Some(node.id.clone()),
                    related_artifact_id: Some(stale_artifact_id.clone()),
                    risk_score: None,
                    validation_checks: Vec::new(),
                });
            }
        }

        let claim_conflicts = self
            .conflicts(&node.bindings.anchors, now)
            .into_iter()
            .filter(|conflict| conflict.severity == ConflictSeverity::Block)
            .collect::<Vec<_>>();
        for conflict in claim_conflicts {
            blockers.push(PlanNodeBlocker {
                kind: PlanNodeBlockerKind::ClaimConflict,
                summary: conflict.summary,
                related_node_id: Some(node.id.clone()),
                related_artifact_id: None,
                risk_score: None,
                validation_checks: Vec::new(),
            });
        }

        let impact = self.impact_for_anchors(&node.bindings.anchors);
        let risk_score = score_change_impact(
            &impact,
            stale_workspace_binding || !stale_artifact_ids.is_empty(),
        );

        if policy.require_review_for_completion && approved_artifacts.is_empty() {
            blockers.push(PlanNodeBlocker {
                kind: PlanNodeBlockerKind::ReviewRequired,
                summary: "node requires an approved artifact review".to_string(),
                related_node_id: Some(node.id.clone()),
                related_artifact_id: artifacts.first().map(|artifact| artifact.id.clone()),
                risk_score: Some(risk_score),
                validation_checks: Vec::new(),
            });
        }

        if let Some(threshold) = policy.review_required_above_risk_score {
            if risk_score >= threshold && approved_artifacts.is_empty() {
                blockers.push(PlanNodeBlocker {
                    kind: PlanNodeBlockerKind::RiskReviewRequired,
                    summary: format!(
                        "node risk score {:.2} requires review before completion",
                        risk_score
                    ),
                    related_node_id: Some(node.id.clone()),
                    related_artifact_id: None,
                    risk_score: Some(risk_score),
                    validation_checks: Vec::new(),
                });
            }
        }

        let mut baseline_required_validations = if policy.require_validation_for_completion {
            impact.likely_validations.clone()
        } else {
            Vec::new()
        };
        baseline_required_validations.extend(required_validation_checks_for_node(graph, node));
        let baseline_required_validations = dedupe_strings(baseline_required_validations);
        if !baseline_required_validations.is_empty() {
            let missing = baseline_required_validations
                .iter()
                .filter(|check| !validated_checks.iter().any(|value| value == *check))
                .cloned()
                .collect::<Vec<_>>();
            if !missing.is_empty() {
                blockers.push(PlanNodeBlocker {
                    kind: PlanNodeBlockerKind::ValidationRequired,
                    summary: if required_validation_checks_for_node(graph, node).is_empty() {
                        format!("node is missing required validations: {}", missing.join(", "))
                    } else {
                        format!(
                            "node is missing required validations, including graph-authored checks: {}",
                            missing.join(", ")
                        )
                    },
                    related_node_id: Some(node.id.clone()),
                    related_artifact_id: approved_artifacts
                        .first()
                        .map(|artifact| artifact.id.clone()),
                    risk_score: Some(risk_score),
                    validation_checks: missing,
                });
            }
        }

        blockers.extend(acceptance_blockers(
            node,
            &approved_artifacts,
            &validated_checks,
            risk_score,
        ));
        sort_and_dedupe_plan_node_blockers(&mut blockers);
        blockers
    }

    fn artifacts_for_plan_node(&self, plan_id: &PlanId, node: &PlanNode) -> Vec<Artifact> {
        let mut artifacts = Vec::new();
        let task_id = CoordinationTaskId::new(node.id.0.clone());
        if self
            .coordination_task(&task_id)
            .is_some_and(|task| task.plan == *plan_id)
        {
            artifacts.extend(self.artifacts(&task_id));
        }
        for artifact_ref in &node.bindings.artifact_refs {
            let artifact_id = ArtifactId::new(artifact_ref.clone());
            if let Some(artifact) = self.coordination_artifact(&artifact_id) {
                artifacts.push(artifact);
            }
        }
        artifacts.sort_by(|left, right| left.id.0.cmp(&right.id.0));
        artifacts.dedup_by(|left, right| left.id == right.id);
        artifacts
    }
}

fn node_is_workspace_bound(node: &PlanNode) -> bool {
    !node.bindings.anchors.is_empty()
        || node
            .acceptance
            .iter()
            .any(|criterion| !criterion.anchors.is_empty())
}

fn acceptance_blockers(
    node: &PlanNode,
    approved_artifacts: &[Artifact],
    validated_checks: &[String],
    risk_score: f32,
) -> Vec<PlanNodeBlocker> {
    let has_review = !approved_artifacts.is_empty();
    let mut blockers = Vec::new();
    for criterion in &node.acceptance {
        let required_checks = dedupe_strings(
            criterion
                .required_checks
                .iter()
                .map(|check| check.id.clone())
                .collect::<Vec<_>>(),
        );
        let missing_checks = required_checks
            .iter()
            .filter(|check| !validated_checks.iter().any(|value| value == *check))
            .cloned()
            .collect::<Vec<_>>();
        let any_check_satisfied = required_checks
            .iter()
            .any(|check| validated_checks.iter().any(|value| value == check));

        match criterion.evidence_policy {
            prism_ir::AcceptanceEvidencePolicy::Any => {
                if !has_review && !any_check_satisfied && !required_checks.is_empty() {
                    blockers.push(PlanNodeBlocker {
                        kind: PlanNodeBlockerKind::ValidationRequired,
                        summary: format!(
                            "acceptance criterion `{}` requires review or one of: {}",
                            criterion.label,
                            required_checks.join(", ")
                        ),
                        related_node_id: Some(node.id.clone()),
                        related_artifact_id: approved_artifacts
                            .first()
                            .map(|artifact| artifact.id.clone()),
                        risk_score: Some(risk_score),
                        validation_checks: required_checks,
                    });
                }
            }
            prism_ir::AcceptanceEvidencePolicy::ReviewOnly => {
                if !has_review {
                    blockers.push(PlanNodeBlocker {
                        kind: PlanNodeBlockerKind::ReviewRequired,
                        summary: format!(
                            "acceptance criterion `{}` requires an approved review artifact",
                            criterion.label
                        ),
                        related_node_id: Some(node.id.clone()),
                        related_artifact_id: approved_artifacts
                            .first()
                            .map(|artifact| artifact.id.clone()),
                        risk_score: Some(risk_score),
                        validation_checks: Vec::new(),
                    });
                }
            }
            prism_ir::AcceptanceEvidencePolicy::ValidationOnly => {
                if !missing_checks.is_empty() {
                    blockers.push(PlanNodeBlocker {
                        kind: PlanNodeBlockerKind::ValidationRequired,
                        summary: format!(
                            "acceptance criterion `{}` is missing validations: {}",
                            criterion.label,
                            missing_checks.join(", ")
                        ),
                        related_node_id: Some(node.id.clone()),
                        related_artifact_id: approved_artifacts
                            .first()
                            .map(|artifact| artifact.id.clone()),
                        risk_score: Some(risk_score),
                        validation_checks: missing_checks,
                    });
                }
            }
            prism_ir::AcceptanceEvidencePolicy::ReviewAndValidation
            | prism_ir::AcceptanceEvidencePolicy::All => {
                if !has_review {
                    blockers.push(PlanNodeBlocker {
                        kind: PlanNodeBlockerKind::ReviewRequired,
                        summary: format!(
                            "acceptance criterion `{}` requires an approved review artifact",
                            criterion.label
                        ),
                        related_node_id: Some(node.id.clone()),
                        related_artifact_id: approved_artifacts
                            .first()
                            .map(|artifact| artifact.id.clone()),
                        risk_score: Some(risk_score),
                        validation_checks: Vec::new(),
                    });
                }
                if !missing_checks.is_empty() {
                    blockers.push(PlanNodeBlocker {
                        kind: PlanNodeBlockerKind::ValidationRequired,
                        summary: format!(
                            "acceptance criterion `{}` is missing validations: {}",
                            criterion.label,
                            missing_checks.join(", ")
                        ),
                        related_node_id: Some(node.id.clone()),
                        related_artifact_id: approved_artifacts
                            .first()
                            .map(|artifact| artifact.id.clone()),
                        risk_score: Some(risk_score),
                        validation_checks: missing_checks,
                    });
                }
            }
        }
    }
    sort_and_dedupe_plan_node_blockers(&mut blockers);
    blockers
}

fn sort_and_dedupe_plan_node_blockers(blockers: &mut Vec<PlanNodeBlocker>) {
    blockers.sort_by(|left, right| {
        blocker_kind_rank(left.kind)
            .cmp(&blocker_kind_rank(right.kind))
            .then_with(|| left.summary.cmp(&right.summary))
            .then_with(|| {
                left.related_node_id
                    .as_ref()
                    .map(|id| id.0.as_str())
                    .cmp(&right.related_node_id.as_ref().map(|id| id.0.as_str()))
            })
            .then_with(|| {
                left.related_artifact_id
                    .as_ref()
                    .map(|id| id.0.as_str())
                    .cmp(&right.related_artifact_id.as_ref().map(|id| id.0.as_str()))
            })
    });
    blockers.dedup_by(|left, right| {
        left.kind == right.kind
            && left.summary == right.summary
            && left.related_node_id == right.related_node_id
            && left.related_artifact_id == right.related_artifact_id
            && left.validation_checks == right.validation_checks
    });
}

fn blocker_kind_rank(kind: PlanNodeBlockerKind) -> u8 {
    match kind {
        PlanNodeBlockerKind::Dependency => 0,
        PlanNodeBlockerKind::BlockingNode => 1,
        PlanNodeBlockerKind::ChildIncomplete => 2,
        PlanNodeBlockerKind::ValidationGate => 3,
        PlanNodeBlockerKind::Handoff => 4,
        PlanNodeBlockerKind::ClaimConflict => 5,
        PlanNodeBlockerKind::ReviewRequired => 6,
        PlanNodeBlockerKind::RiskReviewRequired => 7,
        PlanNodeBlockerKind::ValidationRequired => 8,
        PlanNodeBlockerKind::StaleRevision => 9,
        PlanNodeBlockerKind::ArtifactStale => 10,
    }
}

fn dedupe_strings(mut values: Vec<String>) -> Vec<String> {
    values.sort();
    values.dedup();
    values
}

pub(crate) fn current_timestamp() -> Timestamp {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs()
}
