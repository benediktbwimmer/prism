use std::time::{SystemTime, UNIX_EPOCH};

use prism_coordination::{Artifact, BlockerKind, CoordinationPolicy, TaskBlocker};
use prism_ir::{
    AnchorRef, ArtifactId, ArtifactStatus, BlockerCause, BlockerCauseSource, ConflictSeverity,
    CoordinationTaskId, PlanExecutionOverlay, PlanGraph, PlanId, PlanNode, PlanNodeBlocker,
    PlanNodeBlockerKind, PlanNodeId, PlanNodeStatus, TaskId, Timestamp,
};
use prism_memory::{OutcomeRecallQuery, OutcomeResult};
use prism_projections::validation_labels;

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
        let runtime = self.plan_runtime_state();
        self.plan_node_blockers_for_runtime(&runtime, plan_id, node_id, current_timestamp())
    }

    pub(crate) fn plan_node_blockers_for_runtime(
        &self,
        runtime: &NativePlanRuntimeState,
        plan_id: &PlanId,
        node_id: &PlanNodeId,
        now: Timestamp,
    ) -> Vec<PlanNodeBlocker> {
        let Some(projection) = self.hydrated_plan_projection_for_runtime(runtime, plan_id) else {
            return Vec::new();
        };
        let graph = projection.graph;
        let Some(node) = graph.nodes.iter().find(|node| node.id == *node_id) else {
            return Vec::new();
        };
        self.plan_node_blockers_for_hydrated_graph(
            runtime,
            &graph,
            &projection.execution_overlays,
            node,
            now,
        )
    }

    pub(crate) fn plan_node_blockers_for_hydrated_graph(
        &self,
        runtime: &NativePlanRuntimeState,
        graph: &PlanGraph,
        overlays: &[PlanExecutionOverlay],
        node: &PlanNode,
        now: Timestamp,
    ) -> Vec<PlanNodeBlocker> {
        let mut blockers = node_blockers_for_graph(graph, overlays, &node.id);
        if let Some(task_blockers) = self.task_backed_policy_blockers(&graph.id, &node.id, now) {
            blockers.retain(|blocker| blocker.kind != PlanNodeBlockerKind::Dependency);
            blockers.extend(task_blockers);
        } else {
            blockers.extend(self.native_policy_blockers_for_node(
                runtime.policy(&graph.id),
                graph,
                node,
                overlays,
                now,
            ));
        }
        sort_and_dedupe_plan_node_blockers(&mut blockers);
        blockers
    }

    fn task_backed_policy_blockers(
        &self,
        plan_id: &PlanId,
        node_id: &PlanNodeId,
        now: Timestamp,
    ) -> Option<Vec<PlanNodeBlocker>> {
        let task_id = CoordinationTaskId::new(node_id.0.clone());
        let task = self.task(&TaskId::new(task_id.0.clone()))?;
        if task.task.parent_plan_id != *plan_id {
            return None;
        }
        Some(
            self.blockers(&task_id, now)
                .into_iter()
                .map(|blocker| plan_node_blocker_from_task_blocker(&task_id, blocker))
                .collect(),
        )
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
        let mut validated_checks = approved_artifacts
            .iter()
            .flat_map(|artifact| artifact.validated_checks.iter().cloned())
            .collect::<Vec<_>>();
        validated_checks.extend(self.validated_checks_from_successful_outcomes(&graph.id, node));
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
                causes: vec![
                    plan_policy_cause("stale_after_graph_change"),
                    runtime_cause("workspace_revision_mismatch"),
                ],
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
                    causes: vec![
                        plan_policy_cause("stale_after_graph_change"),
                        artifact_state_cause("approved_artifact_stale"),
                    ],
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
                causes: vec![runtime_cause("claim_conflict")],
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
                causes: vec![
                    plan_policy_cause("require_review_for_completion"),
                    artifact_state_cause("missing_approved_artifact"),
                ],
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
                    causes: vec![
                        derived_threshold_cause(
                            "review_required_above_risk_score",
                            "risk_score",
                            threshold,
                            risk_score,
                        ),
                        artifact_state_cause("missing_approved_artifact"),
                    ],
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
                    causes: validation_required_causes(
                        policy.require_validation_for_completion,
                        !required_validation_checks_for_node(graph, node).is_empty(),
                    ),
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
            .task(&TaskId::new(task_id.0.clone()))
            .is_some_and(|task| task.task.parent_plan_id == *plan_id)
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

    fn validated_checks_from_successful_outcomes(
        &self,
        plan_id: &PlanId,
        node: &PlanNode,
    ) -> Vec<String> {
        let anchors = plan_node_validation_anchors(node);
        let mut events = if anchors.is_empty() {
            Vec::new()
        } else {
            self.outcomes.query_events(&OutcomeRecallQuery {
                anchors,
                result: Some(OutcomeResult::Success),
                limit: 0,
                ..OutcomeRecallQuery::default()
            })
        };

        let task_id = TaskId::new(node.id.0.clone());
        let allow_task_correlated_events = match self.task(&task_id) {
            Some(task) => task.task.parent_plan_id == *plan_id,
            None => true,
        };
        if allow_task_correlated_events {
            events.extend(self.outcomes.query_events(&OutcomeRecallQuery {
                task: Some(task_id),
                result: Some(OutcomeResult::Success),
                limit: 0,
                ..OutcomeRecallQuery::default()
            }));
        }

        events.sort_by(|left, right| left.meta.id.0.cmp(&right.meta.id.0));
        events.dedup_by(|left, right| left.meta.id == right.meta.id);
        dedupe_strings(
            events
                .into_iter()
                .flat_map(|event| validation_labels(&event.evidence))
                .collect(),
        )
    }
}

fn node_is_workspace_bound(node: &PlanNode) -> bool {
    !node.bindings.anchors.is_empty()
        || node
            .acceptance
            .iter()
            .any(|criterion| !criterion.anchors.is_empty())
}

fn plan_node_validation_anchors(node: &PlanNode) -> Vec<AnchorRef> {
    let mut anchors = node.bindings.anchors.clone();
    for criterion in &node.acceptance {
        for anchor in &criterion.anchors {
            if !anchors.iter().any(|existing| existing == anchor) {
                anchors.push(anchor.clone());
            }
        }
    }
    anchors
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
                        causes: vec![acceptance_cause(&criterion.label, "review_or_validation")],
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
                        causes: vec![
                            acceptance_cause(&criterion.label, "review_only"),
                            artifact_state_cause("missing_approved_artifact"),
                        ],
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
                        causes: vec![acceptance_cause(&criterion.label, "validation_only")],
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
                        causes: vec![
                            acceptance_cause(&criterion.label, "review_required"),
                            artifact_state_cause("missing_approved_artifact"),
                        ],
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
                        causes: vec![acceptance_cause(&criterion.label, "validation_required")],
                    });
                }
            }
        }
    }
    sort_and_dedupe_plan_node_blockers(&mut blockers);
    blockers
}

fn plan_node_blocker_from_task_blocker(
    task_id: &CoordinationTaskId,
    blocker: TaskBlocker,
) -> PlanNodeBlocker {
    let related_task_id = blocker.related_task_id.unwrap_or_else(|| task_id.clone());
    PlanNodeBlocker {
        kind: match blocker.kind {
            BlockerKind::Dependency => PlanNodeBlockerKind::Dependency,
            BlockerKind::ClaimConflict => PlanNodeBlockerKind::ClaimConflict,
            BlockerKind::ReviewRequired => PlanNodeBlockerKind::ReviewRequired,
            BlockerKind::RiskReviewRequired => PlanNodeBlockerKind::RiskReviewRequired,
            BlockerKind::ValidationRequired => PlanNodeBlockerKind::ValidationRequired,
            BlockerKind::StaleRevision => PlanNodeBlockerKind::StaleRevision,
            BlockerKind::ArtifactStale => PlanNodeBlockerKind::ArtifactStale,
        },
        summary: blocker.summary,
        related_node_id: Some(PlanNodeId::new(related_task_id.0)),
        related_artifact_id: blocker.related_artifact_id,
        risk_score: blocker.risk_score,
        validation_checks: blocker.validation_checks,
        causes: blocker.causes,
    }
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
            && left.causes == right.causes
    });
}

fn blocker_cause(source: BlockerCauseSource, code: &str) -> BlockerCause {
    BlockerCause {
        source,
        code: Some(code.to_owned()),
        acceptance_label: None,
        threshold_metric: None,
        threshold_value: None,
        observed_value: None,
    }
}

fn plan_policy_cause(code: &str) -> BlockerCause {
    blocker_cause(BlockerCauseSource::PlanPolicy, code)
}

fn runtime_cause(code: &str) -> BlockerCause {
    blocker_cause(BlockerCauseSource::RuntimeState, code)
}

fn artifact_state_cause(code: &str) -> BlockerCause {
    blocker_cause(BlockerCauseSource::ArtifactState, code)
}

fn acceptance_cause(label: &str, code: &str) -> BlockerCause {
    BlockerCause {
        source: BlockerCauseSource::NodeAcceptance,
        code: Some(code.to_owned()),
        acceptance_label: Some(label.to_owned()),
        threshold_metric: None,
        threshold_value: None,
        observed_value: None,
    }
}

fn derived_threshold_cause(
    code: &str,
    threshold_metric: &str,
    threshold_value: f32,
    observed_value: f32,
) -> BlockerCause {
    BlockerCause {
        source: BlockerCauseSource::DerivedThreshold,
        code: Some(code.to_owned()),
        acceptance_label: None,
        threshold_metric: Some(threshold_metric.to_owned()),
        threshold_value: Some(threshold_value),
        observed_value: Some(observed_value),
    }
}

fn validation_required_causes(
    require_validation_for_completion: bool,
    has_graph_authored_checks: bool,
) -> Vec<BlockerCause> {
    let mut causes = Vec::new();
    if require_validation_for_completion {
        causes.push(plan_policy_cause("require_validation_for_completion"));
    }
    if has_graph_authored_checks {
        causes.push(plan_policy_cause("graph_authored_validation_refs"));
    }
    causes
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
