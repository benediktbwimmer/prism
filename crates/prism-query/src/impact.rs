use prism_ir::{
    AnchorRef, ArtifactId, ArtifactStatus, CoordinationTaskId, LineageId, NodeId, Timestamp,
};
use prism_memory::OutcomeEvent;
use prism_projections::CoChangeRecord;

use crate::common::{dedupe_node_ids, dedupe_strings, sort_node_ids};
use crate::types::{
    ArtifactRisk, ChangeImpact, CoChange, TaskRisk, TaskValidationRecipe, ValidationRecipe,
};
use crate::Prism;

impl Prism {
    pub fn outcomes_for(&self, anchors: &[AnchorRef], limit: usize) -> Vec<OutcomeEvent> {
        self.outcomes
            .outcomes_for(&self.expand_anchors(anchors), limit)
    }

    pub fn related_failures(&self, node: &NodeId) -> Vec<OutcomeEvent> {
        self.outcomes
            .related_failures(&self.expand_anchors(&[AnchorRef::Node(node.clone())]), 20)
    }

    pub fn blast_radius(&self, node: &NodeId) -> ChangeImpact {
        self.impact_for_anchors(&[AnchorRef::Node(node.clone())])
    }

    pub fn task_blast_radius(&self, task_id: &CoordinationTaskId) -> Option<ChangeImpact> {
        let task = self.coordination.task(task_id)?;
        Some(self.impact_for_anchors(&task.anchors))
    }

    pub fn task_validation_recipe(
        &self,
        task_id: &CoordinationTaskId,
    ) -> Option<TaskValidationRecipe> {
        let impact = self.task_blast_radius(task_id)?;
        Some(TaskValidationRecipe {
            task_id: task_id.clone(),
            checks: impact.likely_validations,
            scored_checks: impact.validation_checks,
            related_nodes: impact.direct_nodes,
            co_change_neighbors: impact.co_change_neighbors,
            recent_failures: impact.risk_events,
        })
    }

    pub fn task_risk(&self, task_id: &CoordinationTaskId, _now: Timestamp) -> Option<TaskRisk> {
        let task = self.coordination.task(task_id)?;
        let impact = self.impact_for_anchors(&task.anchors);
        let approved_artifacts = self
            .coordination
            .artifacts(task_id)
            .into_iter()
            .filter(|artifact| {
                matches!(
                    artifact.status,
                    ArtifactStatus::Approved | ArtifactStatus::Merged
                )
            })
            .collect::<Vec<_>>();
        let validated_checks = dedupe_strings(
            approved_artifacts
                .iter()
                .flat_map(|artifact| artifact.validated_checks.iter().cloned())
                .collect(),
        );
        let missing_validations = impact
            .likely_validations
            .iter()
            .filter(|check| !validated_checks.iter().any(|value| value == *check))
            .cloned()
            .collect::<Vec<_>>();
        let approved_artifact_ids = approved_artifacts
            .iter()
            .map(|artifact| artifact.id.clone())
            .collect::<Vec<_>>();
        let stale_artifact_ids = approved_artifacts
            .iter()
            .filter(|artifact| {
                artifact.base_revision.graph_version < self.workspace_revision().graph_version
            })
            .map(|artifact| artifact.id.clone())
            .collect::<Vec<_>>();
        let stale_task = task.base_revision.graph_version < self.workspace_revision().graph_version;
        let risk_score = score_change_impact(&impact, stale_task || !stale_artifact_ids.is_empty());
        let review_required = self
            .coordination
            .plan(&task.plan)
            .and_then(|plan| plan.policy.review_required_above_risk_score)
            .map(|threshold| risk_score >= threshold)
            .unwrap_or(false);
        let risk_events = impact.risk_events.clone();

        Some(TaskRisk {
            task_id: task_id.clone(),
            risk_score,
            review_required,
            stale_task,
            has_approved_artifact: !approved_artifact_ids.is_empty(),
            likely_validations: impact.likely_validations,
            missing_validations,
            validation_checks: impact.validation_checks,
            co_change_neighbors: impact.co_change_neighbors,
            risk_events,
            approved_artifact_ids,
            stale_artifact_ids,
        })
    }

    pub fn artifact_risk(&self, artifact_id: &ArtifactId, now: Timestamp) -> Option<ArtifactRisk> {
        let artifact = self.coordinating_artifact(artifact_id)?;
        let task_risk = self.task_risk(&artifact.task, now)?;
        let required_validations = if artifact.required_validations.is_empty() {
            task_risk.likely_validations.clone()
        } else {
            artifact.required_validations.clone()
        };
        let validated_checks = dedupe_strings(artifact.validated_checks.clone());
        let missing_validations = required_validations
            .iter()
            .filter(|check| !validated_checks.iter().any(|value| value == *check))
            .cloned()
            .collect::<Vec<_>>();
        Some(ArtifactRisk {
            artifact_id: artifact.id.clone(),
            task_id: artifact.task.clone(),
            risk_score: task_risk.risk_score,
            review_required: task_risk.review_required,
            stale: artifact.base_revision.graph_version < self.workspace_revision().graph_version,
            required_validations,
            validated_checks,
            missing_validations,
            co_change_neighbors: task_risk.co_change_neighbors,
            risk_events: task_risk.risk_events,
        })
    }

    pub fn co_change_neighbors(&self, node: &NodeId, limit: usize) -> Vec<CoChange> {
        let Some(lineage) = self.lineage_of(node) else {
            return Vec::new();
        };

        self.projections
            .read()
            .expect("projection lock poisoned")
            .co_change_neighbors(&lineage, limit)
            .into_iter()
            .map(|neighbor: CoChangeRecord| {
                let mut nodes = self.history.current_nodes_for_lineage(&neighbor.lineage);
                sort_node_ids(&mut nodes);
                CoChange {
                    lineage: neighbor.lineage,
                    count: neighbor.count,
                    nodes,
                }
            })
            .collect()
    }

    pub fn validation_recipe(&self, node: &NodeId) -> ValidationRecipe {
        let impact = self.blast_radius(node);
        ValidationRecipe {
            target: node.clone(),
            checks: impact.likely_validations,
            scored_checks: impact.validation_checks,
            related_nodes: impact.direct_nodes,
            co_change_neighbors: impact.co_change_neighbors,
            recent_failures: impact.risk_events,
        }
    }

    fn impact_for_anchors(&self, anchors: &[AnchorRef]) -> ChangeImpact {
        let expanded = self.expand_anchors(anchors);
        let base_nodes = self.resolve_anchor_nodes(&expanded);
        if base_nodes.is_empty() {
            let mut lineages = expanded
                .iter()
                .filter_map(|anchor| match anchor {
                    AnchorRef::Lineage(lineage) => Some(lineage.clone()),
                    _ => None,
                })
                .collect::<Vec<_>>();
            lineages.sort_by(|left, right| left.0.cmp(&right.0));
            lineages.dedup();
            let direct_nodes = lineages
                .iter()
                .flat_map(|lineage| self.history.current_nodes_for_lineage(lineage))
                .collect::<Vec<_>>();
            let validation_checks = self
                .projections
                .read()
                .expect("projection lock poisoned")
                .validation_checks_for_lineages(&lineages, 8);
            let likely_validations = validation_checks
                .iter()
                .map(|check| check.label.clone())
                .collect::<Vec<_>>();
            let co_change_neighbors = self.co_change_neighbors_for_lineages(&lineages, 8);
            let risk_events = self.outcomes.related_failures(&expanded, 20);
            let mut direct_nodes = dedupe_node_ids(direct_nodes);
            sort_node_ids(&mut direct_nodes);
            return ChangeImpact {
                direct_nodes,
                lineages,
                likely_validations,
                validation_checks,
                co_change_neighbors,
                risk_events,
            };
        }

        let mut merged = ChangeImpact::default();
        for node in base_nodes {
            merge_change_impact(&mut merged, self.node_blast_radius(&node));
        }
        merged
    }

    fn node_blast_radius(&self, node: &NodeId) -> ChangeImpact {
        let mut direct_nodes = self.graph_neighbors(node);
        let mut lineages = direct_nodes
            .iter()
            .filter_map(|neighbor| self.lineage_of(neighbor))
            .collect::<Vec<_>>();
        let co_change_neighbors = self.co_change_neighbors(node, 8);
        if let Some(lineage) = self.lineage_of(node) {
            lineages.push(lineage);
        }
        lineages.extend(
            co_change_neighbors
                .iter()
                .map(|neighbor| neighbor.lineage.clone()),
        );
        lineages.sort_by(|left, right| left.0.cmp(&right.0));
        lineages.dedup();

        direct_nodes.extend(
            lineages
                .iter()
                .flat_map(|lineage| self.history.current_nodes_for_lineage(lineage)),
        );
        direct_nodes.retain(|candidate| candidate != node);
        sort_node_ids(&mut direct_nodes);

        let mut impact_anchors = vec![AnchorRef::Node(node.clone())];
        impact_anchors.extend(direct_nodes.iter().cloned().map(AnchorRef::Node));
        impact_anchors.extend(lineages.iter().cloned().map(AnchorRef::Lineage));
        let impact_anchors = self.expand_anchors(&impact_anchors);

        let risk_events = self.outcomes.related_failures(&impact_anchors, 20);
        let validation_checks = self
            .projections
            .read()
            .expect("projection lock poisoned")
            .validation_checks_for_lineages(&lineages, 8);
        let likely_validations = validation_checks
            .iter()
            .map(|check| check.label.clone())
            .collect();

        ChangeImpact {
            direct_nodes,
            lineages,
            likely_validations,
            validation_checks,
            co_change_neighbors,
            risk_events,
        }
    }

    fn co_change_neighbors_for_lineages(
        &self,
        lineages: &[LineageId],
        limit: usize,
    ) -> Vec<CoChange> {
        let mut combined = Vec::new();
        for lineage in lineages {
            combined.extend(
                self.projections
                    .read()
                    .expect("projection lock poisoned")
                    .co_change_neighbors(lineage, limit),
            );
        }
        combined.sort_by(|left, right| {
            right
                .count
                .cmp(&left.count)
                .then_with(|| left.lineage.0.cmp(&right.lineage.0))
        });
        combined.dedup_by(|left, right| left.lineage == right.lineage);
        combined
            .into_iter()
            .take(limit)
            .map(|neighbor| {
                let mut nodes = self.history.current_nodes_for_lineage(&neighbor.lineage);
                sort_node_ids(&mut nodes);
                CoChange {
                    lineage: neighbor.lineage,
                    count: neighbor.count,
                    nodes,
                }
            })
            .collect()
    }
}

fn merge_change_impact(target: &mut ChangeImpact, other: ChangeImpact) {
    target.direct_nodes.extend(other.direct_nodes);
    target.lineages.extend(other.lineages);
    target.likely_validations.extend(other.likely_validations);
    target.validation_checks.extend(other.validation_checks);
    target.co_change_neighbors.extend(other.co_change_neighbors);
    target.risk_events.extend(other.risk_events);

    target.direct_nodes = dedupe_node_ids(std::mem::take(&mut target.direct_nodes));
    target.lineages.sort_by(|left, right| left.0.cmp(&right.0));
    target.lineages.dedup();
    target.likely_validations = dedupe_strings(std::mem::take(&mut target.likely_validations));
    target.validation_checks.sort_by(|left, right| {
        right
            .score
            .partial_cmp(&left.score)
            .unwrap_or(std::cmp::Ordering::Equal)
            .then_with(|| left.label.cmp(&right.label))
    });
    target
        .validation_checks
        .dedup_by(|left, right| left.label == right.label);
    target.co_change_neighbors.sort_by(|left, right| {
        right
            .count
            .cmp(&left.count)
            .then_with(|| left.lineage.0.cmp(&right.lineage.0))
    });
    target
        .co_change_neighbors
        .dedup_by(|left, right| left.lineage == right.lineage);
    target
        .risk_events
        .sort_by(|left, right| right.meta.ts.cmp(&left.meta.ts));
    target
        .risk_events
        .dedup_by(|left, right| left.meta.id == right.meta.id);
}

fn score_change_impact(impact: &ChangeImpact, stale: bool) -> f32 {
    let failure_score = (impact.risk_events.len() as f32 * 0.25).min(0.5);
    let validation_score = (impact.validation_checks.len() as f32 * 0.08).min(0.2);
    let co_change_score = (impact
        .co_change_neighbors
        .iter()
        .take(3)
        .map(|neighbor| neighbor.count as f32)
        .sum::<f32>()
        * 0.04)
        .min(0.2);
    let scope_score = (impact.direct_nodes.len() as f32 * 0.02).min(0.1);
    let stale_score = if stale { 0.15 } else { 0.0 };
    (failure_score + validation_score + co_change_score + scope_score + stale_score).min(1.0)
}
