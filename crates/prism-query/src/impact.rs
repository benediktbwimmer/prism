use std::collections::BTreeMap;

use prism_coordination::CoordinationTask;
use prism_ir::{
    AnchorRef, ArtifactId, ArtifactStatus, CoordinationTaskId, LineageId, NodeId, TaskId, Timestamp,
};
use prism_memory::{OutcomeEvidence, OutcomeRecallQuery, OutcomeResult};
use prism_projections::CoChangeRecord;

use crate::common::{dedupe_node_ids, dedupe_strings, sort_node_ids};
use crate::types::{
    ArtifactRisk, ChangeImpact, CoChange, ContractHealthStatus, ContractPacket, TaskRisk,
    TaskValidationRecipe, ValidationRecipe,
};
use crate::Prism;

impl Prism {
    pub fn blast_radius(&self, node: &NodeId) -> ChangeImpact {
        self.impact_for_anchors(&[AnchorRef::Node(node.clone())])
    }

    pub fn task_blast_radius_for_anchors(&self, anchors: &[AnchorRef]) -> ChangeImpact {
        self.impact_for_anchors(anchors)
    }

    pub fn validation_recipe_for_anchors(
        &self,
        target: &NodeId,
        members: &[NodeId],
    ) -> ValidationRecipe {
        let anchors = members
            .iter()
            .cloned()
            .map(AnchorRef::Node)
            .collect::<Vec<_>>();
        let impact = self.impact_for_anchors(&anchors);
        ValidationRecipe {
            target: target.clone(),
            checks: impact.likely_validations,
            scored_checks: impact.validation_checks,
            related_nodes: impact.direct_nodes,
            co_change_neighbors: impact.co_change_neighbors,
            recent_failures: impact.risk_events,
        }
    }

    pub fn task_blast_radius(&self, task_id: &CoordinationTaskId) -> Option<ChangeImpact> {
        let task = self.coordination_task(task_id)?;
        let mut impact = self.task_blast_radius_for_anchors(&Self::task_anchor_refs(&task));
        Self::merge_task_validation_checks(&mut impact.likely_validations, &task);
        Some(impact)
    }

    pub fn task_validation_recipe(
        &self,
        task_id: &CoordinationTaskId,
    ) -> Option<TaskValidationRecipe> {
        let task = self.coordination_task(task_id)?;
        let mut recipe =
            self.task_validation_recipe_for_anchors(task_id, &Self::task_anchor_refs(&task));
        Self::merge_task_validation_checks(&mut recipe.checks, &task);
        Some(recipe)
    }

    pub fn task_validation_recipe_for_anchors(
        &self,
        task_id: &CoordinationTaskId,
        anchors: &[AnchorRef],
    ) -> TaskValidationRecipe {
        let impact = self.task_blast_radius_for_anchors(anchors);
        TaskValidationRecipe {
            task_id: task_id.clone(),
            checks: impact.likely_validations,
            scored_checks: impact.validation_checks,
            related_nodes: impact.direct_nodes,
            co_change_neighbors: impact.co_change_neighbors,
            recent_failures: impact.risk_events,
        }
    }

    pub fn validated_checks_for_task(&self, task_id: &CoordinationTaskId) -> Vec<String> {
        let Some(task) = self.coordination_task(task_id) else {
            return Vec::new();
        };
        let mut validated_checks = self
            .artifacts(task_id)
            .into_iter()
            .filter(|artifact| {
                matches!(
                    artifact.status,
                    ArtifactStatus::Approved | ArtifactStatus::Merged
                )
            })
            .flat_map(|artifact| artifact.validated_checks.into_iter())
            .collect::<Vec<_>>();
        validated_checks.extend(self.successful_validation_checks_for_task(task_id, &task));
        dedupe_strings(validated_checks)
    }

    pub fn task_risk_for_anchors(
        &self,
        task_id: &CoordinationTaskId,
        anchors: &[AnchorRef],
        review_required_above_risk_score: Option<f32>,
        stale_task: bool,
    ) -> TaskRisk {
        let impact = self.task_blast_radius_for_anchors(anchors);
        let (contracts, contract_review_notes) = review_contract_context(self, anchors);
        let risk_score = score_change_impact(&impact, stale_task);
        let review_required = review_required_above_risk_score
            .map(|threshold| risk_score >= threshold)
            .unwrap_or(false);
        let risk_events = impact.risk_events.clone();

        TaskRisk {
            task_id: task_id.clone(),
            risk_score,
            review_required,
            stale_task,
            has_approved_artifact: false,
            likely_validations: impact.likely_validations.clone(),
            missing_validations: impact.likely_validations,
            validation_checks: impact.validation_checks,
            co_change_neighbors: impact.co_change_neighbors,
            risk_events,
            contracts,
            contract_review_notes,
            approved_artifact_ids: Vec::new(),
            stale_artifact_ids: Vec::new(),
        }
    }

    pub fn task_risk(&self, task_id: &CoordinationTaskId, _now: Timestamp) -> Option<TaskRisk> {
        let task = self.coordination_task(task_id)?;
        let anchors = Self::task_anchor_refs(&task);
        let impact = self.impact_for_anchors(&anchors);
        let (contracts, contract_review_notes) = review_contract_context(self, &anchors);
        let likely_validations =
            Self::merged_task_validation_checks(impact.likely_validations.clone(), &task);
        let approved_artifacts = self
            .artifacts(task_id)
            .into_iter()
            .filter(|artifact| {
                matches!(
                    artifact.status,
                    ArtifactStatus::Approved | ArtifactStatus::Merged
                )
            })
            .collect::<Vec<_>>();
        let validated_checks = self.validated_checks_for_task(task_id);
        let missing_validations = likely_validations
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
        let stale_task = task_is_workspace_bound(&task)
            && task.base_revision.graph_version < self.workspace_revision().graph_version;
        let risk_score = score_change_impact(&impact, stale_task || !stale_artifact_ids.is_empty());
        let review_required = self
            .coordination_plan(&task.plan)
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
            likely_validations,
            missing_validations,
            validation_checks: impact.validation_checks,
            co_change_neighbors: impact.co_change_neighbors,
            risk_events,
            contracts,
            contract_review_notes,
            approved_artifact_ids,
            stale_artifact_ids,
        })
    }

    fn task_anchor_refs(task: &CoordinationTask) -> Vec<AnchorRef> {
        if task.bindings.anchors.is_empty() {
            return task.anchors.clone();
        }
        task.bindings.anchors.clone()
    }

    fn task_validation_checks(task: &CoordinationTask) -> Vec<String> {
        dedupe_strings(
            task.validation_refs
                .iter()
                .map(|validation| validation.id.clone())
                .collect(),
        )
    }

    fn merge_task_validation_checks(target: &mut Vec<String>, task: &CoordinationTask) {
        target.extend(Self::task_validation_checks(task));
        *target = dedupe_strings(std::mem::take(target));
    }

    fn merged_task_validation_checks(checks: Vec<String>, task: &CoordinationTask) -> Vec<String> {
        let mut checks = checks;
        Self::merge_task_validation_checks(&mut checks, task);
        checks
    }

    fn successful_validation_checks_for_task(
        &self,
        task_id: &CoordinationTaskId,
        task: &CoordinationTask,
    ) -> Vec<String> {
        let anchors = Self::task_anchor_refs(task);
        let mut events = if anchors.is_empty() {
            Vec::new()
        } else {
            self.query_outcomes(&OutcomeRecallQuery {
                anchors,
                result: Some(OutcomeResult::Success),
                limit: 0,
                ..OutcomeRecallQuery::default()
            })
        };
        events.extend(self.query_outcomes(&OutcomeRecallQuery {
            task: Some(TaskId::new(task_id.0.clone())),
            result: Some(OutcomeResult::Success),
            limit: 0,
            ..OutcomeRecallQuery::default()
        }));
        events.sort_by(|left, right| left.meta.id.0.cmp(&right.meta.id.0));
        events.dedup_by(|left, right| left.meta.id == right.meta.id);
        dedupe_strings(
            events
                .into_iter()
                .flat_map(|event| outcome_validation_labels(&event.evidence))
                .collect(),
        )
    }

    pub fn artifact_risk(&self, artifact_id: &ArtifactId, now: Timestamp) -> Option<ArtifactRisk> {
        let artifact = self.coordinating_artifact(artifact_id)?;
        if !self.runtime_capabilities().cognition_enabled() {
            let required_validations = artifact.required_validations.clone();
            let validated_checks = dedupe_strings(artifact.validated_checks.clone());
            let missing_validations = required_validations
                .iter()
                .filter(|check| !validated_checks.iter().any(|value| value == *check))
                .cloned()
                .collect::<Vec<_>>();
            let risk_score = artifact.risk_score.unwrap_or_default();
            let review_required = self
                .coordination_task(&artifact.task)
                .and_then(|task| self.coordination_plan(&task.plan))
                .and_then(|plan| plan.policy.review_required_above_risk_score)
                .map(|threshold| risk_score >= threshold)
                .unwrap_or(false);
            return Some(ArtifactRisk {
                artifact_id: artifact.id.clone(),
                task_id: artifact.task.clone(),
                risk_score,
                review_required,
                stale: artifact.base_revision.graph_version
                    < self.workspace_revision().graph_version,
                required_validations,
                validated_checks,
                missing_validations,
                co_change_neighbors: Vec::new(),
                risk_events: Vec::new(),
                contracts: Vec::new(),
                contract_review_notes: Vec::new(),
            });
        }
        let task_risk = self.task_risk(&artifact.task, now)?;
        let (contracts, contract_review_notes) = if artifact.anchors.is_empty() {
            (
                task_risk.contracts.clone(),
                task_risk.contract_review_notes.clone(),
            )
        } else {
            let mut anchors = artifact.anchors.clone();
            let task = self.coordination_task(&artifact.task)?;
            anchors.extend(task.anchors);
            review_contract_context(self, &anchors)
        };
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
            contracts,
            contract_review_notes,
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

    pub(crate) fn impact_for_anchors(&self, anchors: &[AnchorRef]) -> ChangeImpact {
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

fn outcome_validation_labels(evidence: &[OutcomeEvidence]) -> Vec<String> {
    let mut labels = evidence
        .iter()
        .filter_map(|evidence| match evidence {
            OutcomeEvidence::Test { name, .. } => Some(normalize_validation_label(name, "test:")),
            OutcomeEvidence::Build { target, .. } => {
                Some(normalize_validation_label(target, "build:"))
            }
            OutcomeEvidence::Command { argv, passed } if *passed => match argv.as_slice() {
                [tool, subcommand, ..] if tool == "cargo" && subcommand == "test" => {
                    Some(format!("test:{}", argv.join(" ")))
                }
                [tool, subcommand, ..] if tool == "cargo" && subcommand == "build" => {
                    Some(format!("build:{}", argv.join(" ")))
                }
                _ => None,
            },
            _ => None,
        })
        .collect::<Vec<_>>();
    labels.sort();
    labels.dedup();
    labels
}

fn normalize_validation_label(value: &str, default_prefix: &str) -> String {
    let value = value.trim();
    if value.starts_with("test:")
        || value.starts_with("build:")
        || value.starts_with("validation:")
        || value.starts_with("command:")
    {
        value.to_string()
    } else {
        format!("{default_prefix}{value}")
    }
}

fn task_is_workspace_bound(task: &prism_coordination::CoordinationTask) -> bool {
    !task.anchors.is_empty()
        || task
            .acceptance
            .iter()
            .any(|criterion| !criterion.anchors.is_empty())
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

fn review_contract_context(
    prism: &Prism,
    anchors: &[AnchorRef],
) -> (Vec<ContractPacket>, Vec<String>) {
    let expanded = prism.expand_anchors(anchors);
    let nodes = prism.resolve_anchor_nodes(&expanded);
    let contracts = review_contracts_for_nodes(prism, &nodes);
    let notes = review_contract_notes(prism, &nodes, &contracts);
    (contracts, notes)
}

fn review_contracts_for_nodes(prism: &Prism, nodes: &[NodeId]) -> Vec<ContractPacket> {
    let mut contracts = BTreeMap::<String, ContractPacket>::new();
    for node in nodes {
        for packet in prism.contracts_for_target(node) {
            contracts.entry(packet.handle.clone()).or_insert(packet);
        }
    }
    contracts.into_values().collect()
}

fn review_contract_notes(
    prism: &Prism,
    nodes: &[NodeId],
    contracts: &[ContractPacket],
) -> Vec<String> {
    let mut notes = Vec::<String>::new();
    for packet in contracts {
        let subject_match = nodes
            .iter()
            .any(|node| prism.contract_subject_matches_target(node, packet));
        let consumer_match = nodes
            .iter()
            .any(|node| prism.contract_consumer_matches_target(node, packet));
        let health = prism.contract_health_by_handle(&packet.handle);
        let consumer_count = health
            .as_ref()
            .map(|health| health.signals.consumer_count)
            .unwrap_or(packet.consumers.len());

        if subject_match
            && (!packet.compatibility.additive.is_empty()
                || !packet.compatibility.risky.is_empty()
                || !packet.compatibility.breaking.is_empty()
                || !packet.compatibility.migrating.is_empty())
        {
            notes.push(format!(
                "Subject-side edit touches contract `{}`; review compatibility guidance before widening, breaking, or migrating the promise.",
                packet.handle
            ));
        }
        if subject_match && packet.validations.is_empty() && !packet.guarantees.is_empty() {
            notes.push(format!(
                "Subject-side edit touches contract `{}` with guarantee clauses but no explicit validations.",
                packet.handle
            ));
        }
        if subject_match && consumer_count >= 2 {
            notes.push(format!(
                "Subject-side edit touches contract `{}` with {} recorded consumers.",
                packet.handle, consumer_count
            ));
        }
        if let Some(health) = health {
            if matches!(
                health.status,
                ContractHealthStatus::Watch
                    | ContractHealthStatus::Degraded
                    | ContractHealthStatus::Stale
            ) {
                notes.push(format!(
                    "Contract `{}` health is {}{}",
                    packet.handle,
                    contract_health_label(health.status),
                    health
                        .reasons
                        .first()
                        .map(|reason| format!(": {reason}"))
                        .unwrap_or_default()
                ));
            }
        }
        if consumer_match && !subject_match {
            notes.push(format!(
                "Edit touches a recorded consumer governed by contract `{}`.",
                packet.handle
            ));
        }
    }
    dedupe_strings(notes)
}

fn contract_health_label(status: ContractHealthStatus) -> &'static str {
    match status {
        ContractHealthStatus::Healthy => "healthy",
        ContractHealthStatus::Watch => "watch",
        ContractHealthStatus::Degraded => "degraded",
        ContractHealthStatus::Stale => "stale",
        ContractHealthStatus::Superseded => "superseded",
        ContractHealthStatus::Retired => "retired",
    }
}

pub(crate) fn score_change_impact(impact: &ChangeImpact, stale: bool) -> f32 {
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
