use std::collections::{HashSet, VecDeque};
use std::fs;
use std::path::Path;
use std::sync::{Arc, RwLock};

use prism_coordination::{
    Artifact, CoordinationConflict, CoordinationSnapshot, CoordinationStore, CoordinationTask,
    Plan, TaskBlocker, WorkClaim,
};
use prism_history::{HistorySnapshot, HistoryStore};
use prism_ir::{
    AnchorRef, ArtifactId, ArtifactStatus, Capability, ClaimMode, CoordinationTaskId, Edge,
    EdgeKind, LineageEvent, LineageId, Node, NodeId, NodeKind, PlanId, SessionId, Skeleton,
    Subgraph, TaskId, Timestamp, WorkspaceRevision,
};
use prism_memory::{OutcomeEvent, OutcomeMemory, OutcomeMemorySnapshot, TaskReplay};
use prism_projections::{CoChangeRecord, IntentIndex, ProjectionIndex, ProjectionSnapshot};
use prism_store::Graph;
use serde::{Deserialize, Serialize};

pub struct Prism {
    graph: Arc<Graph>,
    history: Arc<HistoryStore>,
    outcomes: Arc<OutcomeMemory>,
    coordination: Arc<CoordinationStore>,
    projections: RwLock<ProjectionIndex>,
    intent: RwLock<IntentIndex>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct QueryLimits {
    pub max_result_nodes: usize,
    pub max_call_graph_depth: usize,
    pub max_output_json_bytes: usize,
}

impl Default for QueryLimits {
    fn default() -> Self {
        Self {
            max_result_nodes: 500,
            max_call_graph_depth: 10,
            max_output_json_bytes: 256 * 1024,
        }
    }
}

#[derive(Debug, Clone, Default)]
pub struct ChangeImpact {
    pub direct_nodes: Vec<NodeId>,
    pub lineages: Vec<LineageId>,
    pub likely_validations: Vec<String>,
    pub validation_checks: Vec<ValidationCheck>,
    pub co_change_neighbors: Vec<CoChange>,
    pub risk_events: Vec<OutcomeEvent>,
}

pub use prism_projections::ValidationCheck;

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct CoChange {
    pub lineage: LineageId,
    pub count: u32,
    pub nodes: Vec<NodeId>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ValidationRecipe {
    pub target: NodeId,
    pub checks: Vec<String>,
    pub scored_checks: Vec<ValidationCheck>,
    pub related_nodes: Vec<NodeId>,
    pub co_change_neighbors: Vec<CoChange>,
    pub recent_failures: Vec<OutcomeEvent>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct TaskValidationRecipe {
    pub task_id: CoordinationTaskId,
    pub checks: Vec<String>,
    pub scored_checks: Vec<ValidationCheck>,
    pub related_nodes: Vec<NodeId>,
    pub co_change_neighbors: Vec<CoChange>,
    pub recent_failures: Vec<OutcomeEvent>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct TaskRisk {
    pub task_id: CoordinationTaskId,
    pub risk_score: f32,
    pub review_required: bool,
    pub stale_task: bool,
    pub has_approved_artifact: bool,
    pub likely_validations: Vec<String>,
    pub missing_validations: Vec<String>,
    pub validation_checks: Vec<ValidationCheck>,
    pub co_change_neighbors: Vec<CoChange>,
    pub risk_events: Vec<OutcomeEvent>,
    pub approved_artifact_ids: Vec<ArtifactId>,
    pub stale_artifact_ids: Vec<ArtifactId>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ArtifactRisk {
    pub artifact_id: ArtifactId,
    pub task_id: CoordinationTaskId,
    pub risk_score: f32,
    pub review_required: bool,
    pub stale: bool,
    pub required_validations: Vec<String>,
    pub validated_checks: Vec<String>,
    pub missing_validations: Vec<String>,
    pub co_change_neighbors: Vec<CoChange>,
    pub risk_events: Vec<OutcomeEvent>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct DriftCandidate {
    pub spec: NodeId,
    pub implementations: Vec<NodeId>,
    pub validations: Vec<NodeId>,
    pub related: Vec<NodeId>,
    pub reasons: Vec<String>,
    pub recent_failures: Vec<OutcomeEvent>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct TaskIntent {
    pub task_id: CoordinationTaskId,
    pub specs: Vec<NodeId>,
    pub implementations: Vec<NodeId>,
    pub validations: Vec<NodeId>,
    pub related: Vec<NodeId>,
    pub drift_candidates: Vec<DriftCandidate>,
}

impl Prism {
    pub fn new(graph: Graph) -> Self {
        let mut history = HistoryStore::new();
        history.seed_nodes(graph.all_nodes().map(|node| node.id.clone()));
        Self::with_history(graph, history)
    }

    pub fn with_history(graph: Graph, history: HistoryStore) -> Self {
        Self::with_history_and_outcomes(graph, history, OutcomeMemory::new())
    }

    pub fn with_history_and_outcomes(
        graph: Graph,
        history: HistoryStore,
        outcomes: OutcomeMemory,
    ) -> Self {
        let projections = ProjectionIndex::derive(&history.snapshot(), &outcomes.snapshot());
        Self::with_history_outcomes_coordination_and_projections(
            graph,
            history,
            outcomes,
            CoordinationStore::new(),
            projections,
        )
    }

    pub fn with_history_outcomes_and_projections(
        graph: Graph,
        history: HistoryStore,
        outcomes: OutcomeMemory,
        projections: ProjectionIndex,
    ) -> Self {
        Self::with_history_outcomes_coordination_and_projections(
            graph,
            history,
            outcomes,
            CoordinationStore::new(),
            projections,
        )
    }

    pub fn with_history_outcomes_coordination_and_projections(
        graph: Graph,
        history: HistoryStore,
        outcomes: OutcomeMemory,
        coordination: CoordinationStore,
        projections: ProjectionIndex,
    ) -> Self {
        let intent = IntentIndex::derive(
            graph.all_nodes().collect::<Vec<_>>(),
            graph.edges.iter().collect::<Vec<_>>(),
        );
        Self {
            graph: Arc::new(graph),
            history: Arc::new(history),
            outcomes: Arc::new(outcomes),
            coordination: Arc::new(coordination),
            projections: RwLock::new(projections),
            intent: RwLock::new(intent),
        }
    }

    pub fn graph(&self) -> &Graph {
        self.graph.as_ref()
    }

    pub fn lineage_of(&self, node: &NodeId) -> Option<LineageId> {
        self.history.lineage_of(node)
    }

    pub fn lineage_history(&self, lineage: &LineageId) -> Vec<LineageEvent> {
        self.history.lineage_history(lineage)
    }

    pub fn outcome_memory(&self) -> Arc<OutcomeMemory> {
        Arc::clone(&self.outcomes)
    }

    pub fn coordination(&self) -> Arc<CoordinationStore> {
        Arc::clone(&self.coordination)
    }

    pub fn anchors_for(&self, anchors: &[AnchorRef]) -> Vec<AnchorRef> {
        self.expand_anchors(anchors)
    }

    pub fn history_snapshot(&self) -> HistorySnapshot {
        self.history.snapshot()
    }

    pub fn outcome_snapshot(&self) -> OutcomeMemorySnapshot {
        self.outcomes.snapshot()
    }

    pub fn coordination_snapshot(&self) -> CoordinationSnapshot {
        self.coordination.snapshot()
    }

    pub fn projection_snapshot(&self) -> ProjectionSnapshot {
        self.projections
            .read()
            .expect("projection lock poisoned")
            .snapshot()
    }

    pub fn refresh_projections(&self) {
        let next = ProjectionIndex::derive(&self.history.snapshot(), &self.outcomes.snapshot());
        *self.projections.write().expect("projection lock poisoned") = next;
    }

    pub fn spec_for(&self, node: &NodeId) -> Vec<NodeId> {
        self.intent
            .read()
            .expect("intent lock poisoned")
            .specs_for(node)
    }

    pub fn implementation_for(&self, spec: &NodeId) -> Vec<NodeId> {
        self.intent
            .read()
            .expect("intent lock poisoned")
            .implementations_for(spec)
    }

    pub fn drift_candidates(&self, limit: usize) -> Vec<DriftCandidate> {
        let specs = self
            .intent
            .read()
            .expect("intent lock poisoned")
            .known_specs();
        self.drift_candidates_for_specs(&specs, limit)
    }

    pub fn apply_outcome_event_to_projections(&self, event: &OutcomeEvent) {
        self.projections
            .write()
            .expect("projection lock poisoned")
            .apply_outcome_event(event, |node| self.history.lineage_of(node));
    }

    pub fn apply_lineage_events_to_projections(&self, events: &[LineageEvent]) {
        self.projections
            .write()
            .expect("projection lock poisoned")
            .apply_lineage_events(events);
    }

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

    pub fn task_intent(&self, task_id: &CoordinationTaskId) -> Option<TaskIntent> {
        let task = self.coordination.task(task_id)?;
        let intent = self.intent.read().expect("intent lock poisoned");
        let task_nodes = self.resolve_anchor_nodes(&task.anchors);
        let mut specs = task_nodes
            .iter()
            .flat_map(|node| intent.specs_for(node))
            .collect::<Vec<_>>();
        specs.extend(
            task_nodes
                .iter()
                .filter(|node| is_intent_source(node))
                .cloned(),
        );
        let specs = dedupe_node_ids(specs);

        let mut implementations = Vec::new();
        let mut validations = Vec::new();
        let mut related = Vec::new();
        for spec in &specs {
            implementations.extend(intent.implementations_for(spec));
            validations.extend(intent.validations_for(spec));
            related.extend(intent.related_for(spec));
        }
        Some(TaskIntent {
            task_id: task_id.clone(),
            specs: specs.clone(),
            implementations: dedupe_node_ids(implementations),
            validations: dedupe_node_ids(validations),
            related: dedupe_node_ids(related),
            drift_candidates: self.drift_candidates_for_specs(&specs, 10),
        })
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

    pub fn resume_task(&self, task: &TaskId) -> TaskReplay {
        self.outcomes.resume_task(task)
    }

    pub fn workspace_revision(&self) -> WorkspaceRevision {
        WorkspaceRevision {
            graph_version: self.history_snapshot().events.len() as u64,
            git_commit: None,
        }
    }

    pub fn coordination_plan(&self, plan_id: &PlanId) -> Option<Plan> {
        self.coordination.plan(plan_id)
    }

    pub fn coordination_task(&self, task_id: &CoordinationTaskId) -> Option<CoordinationTask> {
        self.coordination.task(task_id)
    }

    pub fn ready_tasks(&self, plan_id: &PlanId, now: Timestamp) -> Vec<CoordinationTask> {
        self.coordination
            .ready_tasks(plan_id, self.workspace_revision(), now)
    }

    pub fn claims(&self, anchors: &[AnchorRef], now: Timestamp) -> Vec<WorkClaim> {
        let anchors = self.coordination_scope_anchors(anchors);
        self.coordination.claims_for_anchor(&anchors, now)
    }

    pub fn conflicts(&self, anchors: &[AnchorRef], now: Timestamp) -> Vec<CoordinationConflict> {
        let anchors = self.coordination_scope_anchors(anchors);
        self.coordination.conflicts_for_anchor(&anchors, now)
    }

    pub fn blockers(&self, task_id: &CoordinationTaskId, now: Timestamp) -> Vec<TaskBlocker> {
        let mut blockers = self
            .coordination
            .blockers(task_id, self.workspace_revision(), now);
        if let Some(risk) = self.task_risk(task_id, now) {
            if !risk.stale_artifact_ids.is_empty() {
                blockers.push(TaskBlocker {
                    kind: prism_coordination::BlockerKind::ArtifactStale,
                    summary: format!(
                        "approved artifact is stale against graph version {}",
                        self.workspace_revision().graph_version
                    ),
                    related_task_id: Some(task_id.clone()),
                    related_artifact_id: risk.stale_artifact_ids.first().cloned(),
                    risk_score: Some(risk.risk_score),
                    validation_checks: Vec::new(),
                });
            }
            if risk.review_required && !risk.has_approved_artifact {
                blockers.push(TaskBlocker {
                    kind: prism_coordination::BlockerKind::RiskReviewRequired,
                    summary: format!(
                        "task risk score {:.2} requires review before completion",
                        risk.risk_score
                    ),
                    related_task_id: Some(task_id.clone()),
                    related_artifact_id: None,
                    risk_score: Some(risk.risk_score),
                    validation_checks: Vec::new(),
                });
            }
            if !risk.missing_validations.is_empty() {
                blockers.push(TaskBlocker {
                    kind: prism_coordination::BlockerKind::ValidationRequired,
                    summary: format!(
                        "task is missing required validations: {}",
                        risk.missing_validations.join(", ")
                    ),
                    related_task_id: Some(task_id.clone()),
                    related_artifact_id: risk.approved_artifact_ids.first().cloned(),
                    risk_score: Some(risk.risk_score),
                    validation_checks: risk.missing_validations.clone(),
                });
            }
        }
        dedupe_blockers(blockers)
    }

    pub fn pending_reviews(&self, plan_id: Option<&PlanId>) -> Vec<Artifact> {
        self.coordination.pending_reviews(plan_id)
    }

    pub fn artifacts(&self, task_id: &CoordinationTaskId) -> Vec<Artifact> {
        self.coordination.artifacts(task_id)
    }

    pub fn simulate_claim(
        &self,
        session_id: &SessionId,
        anchors: &[AnchorRef],
        capability: Capability,
        mode: Option<ClaimMode>,
        task_id: Option<&CoordinationTaskId>,
        now: Timestamp,
    ) -> Vec<CoordinationConflict> {
        let anchors = self.coordination_scope_anchors(anchors);
        self.coordination.simulate_claim(
            session_id,
            &anchors,
            capability,
            mode,
            task_id,
            self.workspace_revision(),
            now,
        )
    }

    pub fn coordination_scope_anchors(&self, anchors: &[AnchorRef]) -> Vec<AnchorRef> {
        let mut scoped = self.expand_anchors(anchors);
        let seed_nodes = self.resolve_anchor_nodes(&scoped);
        let mut processed_nodes = seed_nodes.into_iter().take(24).collect::<Vec<_>>();
        sort_node_ids(&mut processed_nodes);
        processed_nodes.dedup();

        for node in processed_nodes {
            scoped.push(AnchorRef::Node(node.clone()));
            if let Some(lineage) = self.lineage_of(&node) {
                scoped.push(AnchorRef::Lineage(lineage));
            }

            for neighbor in self.graph_neighbors(&node).into_iter().take(8) {
                scoped.push(AnchorRef::Node(neighbor.clone()));
                if let Some(lineage) = self.lineage_of(&neighbor) {
                    scoped.push(AnchorRef::Lineage(lineage));
                }
            }

            for neighbor in self.co_change_neighbors(&node, 4) {
                scoped.push(AnchorRef::Lineage(neighbor.lineage.clone()));
                for current in neighbor.nodes.into_iter().take(4) {
                    scoped.push(AnchorRef::Node(current));
                }
            }
        }

        scoped.sort_by(anchor_sort_key);
        scoped.dedup();
        scoped
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

    fn drift_candidates_for_specs(&self, specs: &[NodeId], limit: usize) -> Vec<DriftCandidate> {
        self.intent
            .read()
            .expect("intent lock poisoned")
            .drift_candidates(specs, limit)
            .into_iter()
            .map(|candidate| DriftCandidate {
                recent_failures: candidate
                    .implementations
                    .iter()
                    .flat_map(|node| self.related_failures(node))
                    .take(10)
                    .collect(),
                spec: candidate.spec,
                implementations: candidate.implementations,
                validations: candidate.validations,
                related: candidate.related,
                reasons: candidate.reasons,
            })
            .collect()
    }

    pub fn symbol(&self, query: &str) -> Vec<Symbol<'_>> {
        let matches = self.sorted_matches(query);
        let Some(best_score) = matches.first().map(|entry| entry.score) else {
            return Vec::new();
        };

        matches
            .into_iter()
            .take_while(|entry| entry.score == best_score)
            .map(|entry| Symbol {
                prism: self,
                id: entry.node.id.clone(),
            })
            .collect()
    }

    pub fn search(
        &self,
        query: &str,
        limit: usize,
        kind: Option<NodeKind>,
        path_filter: Option<&str>,
    ) -> Vec<Symbol<'_>> {
        let path_filter = path_filter.map(|value| value.trim().to_ascii_lowercase());
        self.sorted_matches(query)
            .into_iter()
            .filter(|entry| kind.map_or(true, |kind| entry.node.kind == kind))
            .filter(|entry| {
                path_filter
                    .as_deref()
                    .map_or(true, |filter| self.matches_path_filter(entry.node, filter))
            })
            .take(limit)
            .map(|entry| Symbol {
                prism: self,
                id: entry.node.id.clone(),
            })
            .collect()
    }

    pub fn entrypoints(&self) -> Vec<Symbol<'_>> {
        let mains: Vec<_> = self
            .graph
            .all_nodes()
            .filter(|node| matches!(node.kind, NodeKind::Function | NodeKind::Method))
            .filter(|node| node.name == "main")
            .map(|node| Symbol {
                prism: self,
                id: node.id.clone(),
            })
            .collect();
        if !mains.is_empty() {
            return mains;
        }

        self.graph
            .all_nodes()
            .filter(|node| matches!(node.kind, NodeKind::Function | NodeKind::Method))
            .filter(|node| {
                self.graph
                    .edges_to(&node.id, Some(EdgeKind::Calls))
                    .is_empty()
            })
            .map(|node| Symbol {
                prism: self,
                id: node.id.clone(),
            })
            .collect()
    }

    fn sorted_matches(&self, query: &str) -> Vec<Match<'_>> {
        let query = query.trim();
        if query.is_empty() {
            return Vec::new();
        }

        let query_lower = query.to_ascii_lowercase();
        let mut matches = self
            .graph
            .all_nodes()
            .filter_map(|node| {
                match_score(node, query, &query_lower).map(|score| Match {
                    score,
                    is_test: is_test_node(node),
                    path_len: node.id.path.len(),
                    path: node.id.path.as_str().to_owned(),
                    node,
                })
            })
            .collect::<Vec<_>>();

        matches.sort_by(|left, right| {
            left.score
                .cmp(&right.score)
                .then_with(|| left.is_test.cmp(&right.is_test))
                .then_with(|| left.path_len.cmp(&right.path_len))
                .then_with(|| left.path.cmp(&right.path))
        });

        matches
    }

    fn matches_path_filter(&self, node: &Node, path_filter: &str) -> bool {
        self.graph
            .file_path(node.file)
            .map(|path| {
                path.to_string_lossy()
                    .to_ascii_lowercase()
                    .contains(path_filter)
            })
            .unwrap_or(false)
            || node
                .id
                .path
                .as_str()
                .to_ascii_lowercase()
                .contains(path_filter)
            || node
                .name
                .as_str()
                .to_ascii_lowercase()
                .contains(path_filter)
    }

    fn expand_anchors(&self, anchors: &[AnchorRef]) -> Vec<AnchorRef> {
        let mut expanded = anchors.to_vec();
        for anchor in anchors {
            if let AnchorRef::Node(node) = anchor {
                if let Some(lineage) = self.lineage_of(node) {
                    expanded.push(AnchorRef::Lineage(lineage));
                }
            }
        }
        expanded.sort_by(anchor_sort_key);
        expanded.dedup();
        expanded
    }

    fn graph_neighbors(&self, node: &NodeId) -> Vec<NodeId> {
        let mut neighbors = self
            .graph
            .edges_from(node, None)
            .into_iter()
            .map(|edge| edge.target.clone())
            .chain(
                self.graph
                    .edges_to(node, None)
                    .into_iter()
                    .map(|edge| edge.source.clone()),
            )
            .collect::<Vec<_>>();
        sort_node_ids(&mut neighbors);
        neighbors
    }

    fn coordinating_artifact(&self, artifact_id: &ArtifactId) -> Option<Artifact> {
        self.coordination.events();
        self.coordination
            .snapshot()
            .artifacts
            .into_iter()
            .find(|artifact| &artifact.id == artifact_id)
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

    fn resolve_anchor_nodes(&self, anchors: &[AnchorRef]) -> Vec<NodeId> {
        let mut nodes = Vec::new();
        for anchor in anchors {
            match anchor {
                AnchorRef::Node(node) => nodes.push(node.clone()),
                AnchorRef::Lineage(lineage) => {
                    nodes.extend(self.history.current_nodes_for_lineage(lineage));
                }
                AnchorRef::File(file) => {
                    nodes.extend(
                        self.graph
                            .all_nodes()
                            .filter(|node| node.file == *file)
                            .map(|node| node.id.clone()),
                    );
                }
                AnchorRef::Kind(kind) => {
                    nodes.extend(
                        self.graph
                            .all_nodes()
                            .filter(|node| node.kind == *kind)
                            .map(|node| node.id.clone()),
                    );
                }
            }
        }
        let mut nodes = dedupe_node_ids(nodes);
        sort_node_ids(&mut nodes);
        nodes
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

fn dedupe_strings(mut values: Vec<String>) -> Vec<String> {
    values.sort();
    values.dedup();
    values
}

fn dedupe_node_ids(mut nodes: Vec<NodeId>) -> Vec<NodeId> {
    sort_node_ids(&mut nodes);
    nodes.dedup();
    nodes
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

fn dedupe_blockers(mut blockers: Vec<TaskBlocker>) -> Vec<TaskBlocker> {
    blockers.sort_by(|left, right| {
        format!("{:?}", left.kind)
            .cmp(&format!("{:?}", right.kind))
            .then_with(|| left.summary.cmp(&right.summary))
    });
    blockers.dedup_by(|left, right| left.kind == right.kind && left.summary == right.summary);
    blockers
}

fn is_intent_source(node: &NodeId) -> bool {
    matches!(
        node.kind,
        NodeKind::Document | NodeKind::MarkdownHeading | NodeKind::JsonKey | NodeKind::YamlKey
    )
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

struct Match<'a> {
    score: u8,
    is_test: bool,
    path_len: usize,
    path: String,
    node: &'a Node,
}

pub struct Symbol<'a> {
    prism: &'a Prism,
    id: NodeId,
}

#[derive(Debug, Clone, Default)]
pub struct Relations {
    pub outgoing_calls: Vec<NodeId>,
    pub incoming_calls: Vec<NodeId>,
    pub outgoing_imports: Vec<NodeId>,
    pub incoming_imports: Vec<NodeId>,
    pub outgoing_implements: Vec<NodeId>,
    pub incoming_implements: Vec<NodeId>,
    pub outgoing_specifies: Vec<NodeId>,
    pub incoming_specifies: Vec<NodeId>,
    pub outgoing_validates: Vec<NodeId>,
    pub incoming_validates: Vec<NodeId>,
    pub outgoing_related: Vec<NodeId>,
    pub incoming_related: Vec<NodeId>,
}

impl<'a> Symbol<'a> {
    pub fn id(&self) -> &NodeId {
        &self.id
    }

    pub fn node(&self) -> &Node {
        self.prism
            .graph
            .node(&self.id)
            .expect("symbol node must exist in graph")
    }

    pub fn name(&self) -> &str {
        self.node().name.as_str()
    }

    pub fn signature(&self) -> String {
        format!("{} {}", self.node().kind, self.id.path)
    }

    pub fn skeleton(&self) -> Skeleton {
        let calls = self.targets(EdgeKind::Calls);
        Skeleton { calls }
    }

    pub fn imports(&self) -> Vec<NodeId> {
        self.targets(EdgeKind::Imports)
    }

    pub fn imported_by(&self) -> Vec<NodeId> {
        self.sources(EdgeKind::Imports)
    }

    pub fn implements(&self) -> Vec<NodeId> {
        self.targets(EdgeKind::Implements)
    }

    pub fn implemented_by(&self) -> Vec<NodeId> {
        self.sources(EdgeKind::Implements)
    }

    pub fn callers(&self) -> Vec<NodeId> {
        self.sources(EdgeKind::Calls)
    }

    pub fn relations(&self) -> Relations {
        Relations {
            outgoing_calls: self.targets(EdgeKind::Calls),
            incoming_calls: self.sources(EdgeKind::Calls),
            outgoing_imports: self.targets(EdgeKind::Imports),
            incoming_imports: self.sources(EdgeKind::Imports),
            outgoing_implements: self.targets(EdgeKind::Implements),
            incoming_implements: self.sources(EdgeKind::Implements),
            outgoing_specifies: self.targets(EdgeKind::Specifies),
            incoming_specifies: self.sources(EdgeKind::Specifies),
            outgoing_validates: self.targets(EdgeKind::Validates),
            incoming_validates: self.sources(EdgeKind::Validates),
            outgoing_related: self.targets(EdgeKind::RelatedTo),
            incoming_related: self.sources(EdgeKind::RelatedTo),
        }
    }

    pub fn full(&self) -> String {
        let node = self.node();
        let Some(path) = self.prism.graph.file_path(node.file) else {
            return String::new();
        };
        let Ok(source) = fs::read_to_string(path) else {
            return String::new();
        };

        let start = usize::min(node.span.start as usize, source.len());
        let end = usize::min(node.span.end as usize, source.len());
        source.get(start..end).unwrap_or_default().to_owned()
    }

    pub fn call_graph(&self, depth: usize) -> Subgraph {
        let mut visited = HashSet::new();
        let mut nodes = Vec::new();
        let mut edges = Vec::<Edge>::new();
        let mut queue = VecDeque::from([(self.id.clone(), 0usize)]);
        let mut max_depth_reached: Option<usize> = None;

        while let Some((current, current_depth)) = queue.pop_front() {
            if !visited.insert(current.clone()) {
                continue;
            }
            nodes.push(current.clone());
            max_depth_reached =
                Some(max_depth_reached.map_or(current_depth, |max| max.max(current_depth)));

            if current_depth >= depth {
                continue;
            }

            for edge in self.prism.graph.edges_from(&current, Some(EdgeKind::Calls)) {
                edges.push(edge.clone());
                queue.push_back((edge.target.clone(), current_depth + 1));
            }
        }

        Subgraph {
            root: self.id.clone(),
            nodes,
            edges,
            truncated: false,
            max_depth_reached,
        }
    }

    fn targets(&self, kind: EdgeKind) -> Vec<NodeId> {
        self.prism
            .graph
            .edges_from(&self.id, Some(kind))
            .into_iter()
            .map(|edge| edge.target.clone())
            .collect()
    }

    fn sources(&self, kind: EdgeKind) -> Vec<NodeId> {
        self.prism
            .graph
            .edges_to(&self.id, Some(kind))
            .into_iter()
            .map(|edge| edge.source.clone())
            .collect()
    }
}

fn match_score(node: &Node, query: &str, query_lower: &str) -> Option<u8> {
    let name = node.name.as_str();
    let path = node.id.path.as_str();
    let name_lower = name.to_ascii_lowercase();
    let path_lower = path.to_ascii_lowercase();

    if path == query {
        Some(0)
    } else if name == query {
        Some(1)
    } else if last_path_segment(path) == Some(query) {
        Some(2)
    } else if node.kind == NodeKind::Document && document_stem(name).as_deref() == Some(query_lower)
    {
        Some(3)
    } else if name_lower == query_lower {
        Some(4)
    } else if path_lower == query_lower {
        Some(5)
    } else if path.ends_with(&format!("::{query}")) {
        Some(6)
    } else if path_lower.ends_with(&format!("::{}", query_lower)) {
        Some(7)
    } else if node.kind == NodeKind::Document && has_token(&name_lower, query_lower) {
        Some(8)
    } else if has_token(&name_lower, query_lower) {
        Some(9)
    } else if has_token(&path_lower, query_lower) {
        Some(10)
    } else if node.kind == NodeKind::Document && has_token_prefix(&name_lower, query_lower) {
        Some(11)
    } else if has_token_prefix(&name_lower, query_lower) {
        Some(12)
    } else if has_token_prefix(&path_lower, query_lower) {
        Some(13)
    } else {
        None
    }
}

fn last_path_segment(path: &str) -> Option<&str> {
    path.rsplit("::").next()
}

fn document_stem(name: &str) -> Option<String> {
    Path::new(name)
        .file_stem()
        .and_then(|stem| stem.to_str())
        .map(|stem| stem.to_ascii_lowercase())
}

fn has_token(value: &str, query: &str) -> bool {
    tokens(value).any(|token| token == query)
}

fn has_token_prefix(value: &str, query: &str) -> bool {
    tokens(value).any(|token| token.starts_with(query))
}

fn tokens(value: &str) -> impl Iterator<Item = &str> {
    value
        .split(|ch: char| !ch.is_ascii_alphanumeric())
        .filter(|token| !token.is_empty())
}

fn is_test_node(node: &Node) -> bool {
    let path = node.id.path.as_str();
    path.contains("::tests::") || path.ends_with("::tests")
}

fn sort_node_ids(nodes: &mut Vec<NodeId>) {
    nodes.sort_by(|left, right| {
        left.crate_name
            .cmp(&right.crate_name)
            .then_with(|| left.path.cmp(&right.path))
            .then_with(|| left.kind.to_string().cmp(&right.kind.to_string()))
    });
    nodes.dedup();
}

fn anchor_sort_key(left: &AnchorRef, right: &AnchorRef) -> std::cmp::Ordering {
    anchor_label(left).cmp(&anchor_label(right))
}

fn anchor_label(anchor: &AnchorRef) -> String {
    match anchor {
        AnchorRef::Node(node) => format!("node:{}:{}:{}", node.crate_name, node.path, node.kind),
        AnchorRef::Lineage(lineage) => format!("lineage:{}", lineage.0),
        AnchorRef::File(file) => format!("file:{}", file.0),
        AnchorRef::Kind(kind) => format!("kind:{kind}"),
    }
}

#[cfg(test)]
mod tests {
    use prism_coordination::{
        ArtifactProposeInput, CoordinationPolicy, CoordinationStore, PlanCreateInput,
        TaskCreateInput,
    };
    use prism_history::HistoryStore;
    use prism_ir::{
        AnchorRef, ChangeTrigger, Edge, EdgeKind, EventActor, EventId, EventMeta, FileId, Language,
        Node, NodeId, NodeKind, ObservedChangeSet, ObservedNode, SessionId, Span, TaskId,
        WorkspaceRevision,
    };
    use prism_memory::{OutcomeEvent, OutcomeEvidence, OutcomeKind, OutcomeMemory, OutcomeResult};
    use prism_projections::ProjectionIndex;
    use prism_store::Graph;

    use super::Prism;

    #[test]
    fn finds_documents_by_file_stem_and_path_fragment() {
        let mut graph = Graph::new();
        graph.add_node(Node {
            id: NodeId::new("demo", "demo::document::docs::SPEC_md", NodeKind::Document),
            name: "docs/SPEC.md".into(),
            kind: NodeKind::Document,
            file: FileId(1),
            span: Span::whole_file(1),
            language: Language::Markdown,
        });
        graph.add_node(Node {
            id: NodeId::new(
                "demo",
                "demo::document::docs::SPEC_md::overview",
                NodeKind::MarkdownHeading,
            ),
            name: "Overview".into(),
            kind: NodeKind::MarkdownHeading,
            file: FileId(1),
            span: Span::line(1),
            language: Language::Markdown,
        });
        graph.add_node(Node {
            id: NodeId::new(
                "demo",
                "demo::document::docs::SPEC_md::spec_details",
                NodeKind::MarkdownHeading,
            ),
            name: "Spec Details".into(),
            kind: NodeKind::MarkdownHeading,
            file: FileId(1),
            span: Span::line(2),
            language: Language::Markdown,
        });
        graph.add_node(Node {
            id: NodeId::new(
                "demo",
                "demo::tests::search_respects_limit",
                NodeKind::Function,
            ),
            name: "search_respects_limit".into(),
            kind: NodeKind::Function,
            file: FileId(2),
            span: Span::line(1),
            language: Language::Rust,
        });

        let prism = Prism::new(graph);
        let symbol_matches = prism.symbol("SPEC");
        assert_eq!(symbol_matches.len(), 1);
        assert_eq!(symbol_matches[0].node().kind, NodeKind::Document);
        assert!(prism
            .symbol("docs/SPEC.md")
            .into_iter()
            .any(|symbol| symbol.node().kind == NodeKind::Document));
        assert!(prism
            .search("SPEC", 10, None, None)
            .into_iter()
            .any(|symbol| symbol.node().kind == NodeKind::MarkdownHeading));
        assert!(!prism
            .search("SPEC", 10, None, None)
            .into_iter()
            .any(|symbol| symbol.id().path == "demo::tests::search_respects_limit"));
    }

    #[test]
    fn prefers_exact_name_matches_before_fuzzy_matches() {
        let mut graph = Graph::new();
        graph.add_node(Node {
            id: NodeId::new("demo", "demo::alpha", NodeKind::Function),
            name: "alpha".into(),
            kind: NodeKind::Function,
            file: FileId(1),
            span: Span::line(1),
            language: Language::Rust,
        });
        graph.add_node(Node {
            id: NodeId::new(
                "demo",
                "demo::document::notes::alpha_md",
                NodeKind::Document,
            ),
            name: "notes/alpha.md".into(),
            kind: NodeKind::Document,
            file: FileId(2),
            span: Span::whole_file(1),
            language: Language::Markdown,
        });

        let prism = Prism::new(graph);
        let symbols = prism.symbol("alpha");

        assert_eq!(symbols[0].node().kind, NodeKind::Function);
    }

    #[test]
    fn search_respects_limit() {
        let mut graph = Graph::new();
        for index in 0..3 {
            graph.add_node(Node {
                id: NodeId::new(
                    "demo",
                    format!("demo::document::notes::alpha_{index}"),
                    NodeKind::Document,
                ),
                name: format!("notes/alpha-{index}.md").into(),
                kind: NodeKind::Document,
                file: FileId(index + 1),
                span: Span::whole_file(1),
                language: Language::Markdown,
            });
        }

        let prism = Prism::new(graph);
        assert_eq!(prism.search("alpha", 2, None, None).len(), 2);
    }

    #[test]
    fn search_can_filter_by_kind_and_path() {
        use std::path::Path;

        let mut graph = Graph::new();
        let spec_file = graph.ensure_file(Path::new("/workspace/docs/SPEC.md"));
        let source_file = graph.ensure_file(Path::new("/workspace/src/spec.rs"));

        graph.add_node(Node {
            id: NodeId::new("demo", "demo::document::docs::SPEC_md", NodeKind::Document),
            name: "docs/SPEC.md".into(),
            kind: NodeKind::Document,
            file: spec_file,
            span: Span::whole_file(1),
            language: Language::Markdown,
        });
        graph.add_node(Node {
            id: NodeId::new("demo", "demo::inspect_spec", NodeKind::Function),
            name: "inspect_spec".into(),
            kind: NodeKind::Function,
            file: source_file,
            span: Span::line(1),
            language: Language::Rust,
        });

        let prism = Prism::new(graph);

        let documents = prism.search("spec", 10, Some(NodeKind::Document), Some("docs/"));
        assert_eq!(documents.len(), 1);
        assert_eq!(documents[0].node().kind, NodeKind::Document);

        let functions = prism.search("spec", 10, Some(NodeKind::Function), Some("src/"));
        assert_eq!(functions.len(), 1);
        assert_eq!(functions[0].node().kind, NodeKind::Function);
    }

    #[test]
    fn exposes_lineage_queries_when_history_is_present() {
        let mut graph = Graph::new();
        let node_id = NodeId::new("demo", "demo::alpha", NodeKind::Function);
        graph.add_node(Node {
            id: node_id.clone(),
            name: "alpha".into(),
            kind: NodeKind::Function,
            file: FileId(1),
            span: Span::line(1),
            language: Language::Rust,
        });

        let mut history = HistoryStore::new();
        history.seed_nodes([node_id.clone()]);
        let prism = Prism::with_history(graph, history);

        let lineage = prism.lineage_of(&node_id).unwrap();
        assert!(prism.lineage_history(&lineage).is_empty());
    }

    #[test]
    fn outcome_queries_expand_node_to_lineage() {
        let mut graph = Graph::new();
        let old_id = NodeId::new("demo", "demo::alpha", NodeKind::Function);
        let new_id = NodeId::new("demo", "demo::renamed_alpha", NodeKind::Function);
        graph.add_node(Node {
            id: new_id.clone(),
            name: "renamed_alpha".into(),
            kind: NodeKind::Function,
            file: FileId(1),
            span: Span::line(1),
            language: Language::Rust,
        });

        let mut history = HistoryStore::new();
        history.seed_nodes([old_id.clone()]);
        let lineage = history.apply(&prism_ir::ObservedChangeSet {
            meta: EventMeta {
                id: EventId::new("observed:1"),
                ts: 1,
                actor: EventActor::System,
                correlation: None,
                causation: None,
            },
            trigger: prism_ir::ChangeTrigger::ManualReindex,
            files: vec![FileId(1)],
            added: vec![prism_ir::ObservedNode {
                node: Node {
                    id: new_id.clone(),
                    name: "renamed_alpha".into(),
                    kind: NodeKind::Function,
                    file: FileId(1),
                    span: Span::line(1),
                    language: Language::Rust,
                },
                fingerprint: prism_ir::SymbolFingerprint::with_parts(1, Some(2), Some(2), None),
            }],
            removed: vec![prism_ir::ObservedNode {
                node: Node {
                    id: old_id.clone(),
                    name: "alpha".into(),
                    kind: NodeKind::Function,
                    file: FileId(1),
                    span: Span::line(1),
                    language: Language::Rust,
                },
                fingerprint: prism_ir::SymbolFingerprint::with_parts(1, Some(2), Some(2), None),
            }],
            updated: Vec::new(),
            edge_added: Vec::new(),
            edge_removed: Vec::new(),
        })[0]
            .lineage
            .clone();

        let outcomes = OutcomeMemory::new();
        outcomes
            .store_event(OutcomeEvent {
                meta: EventMeta {
                    id: EventId::new("outcome:1"),
                    ts: 2,
                    actor: EventActor::Agent,
                    correlation: Some(TaskId::new("task:rename")),
                    causation: None,
                },
                anchors: vec![AnchorRef::Lineage(lineage)],
                kind: OutcomeKind::FailureObserved,
                result: OutcomeResult::Failure,
                summary: "rename caused a failure".into(),
                evidence: vec![OutcomeEvidence::Test {
                    name: "rename_flow".into(),
                    passed: false,
                }],
                metadata: serde_json::Value::Null,
            })
            .unwrap();

        let prism = Prism::with_history_and_outcomes(graph, history, outcomes);
        let failures = prism.related_failures(&new_id);
        assert_eq!(failures.len(), 1);
        assert!(failures[0].summary.contains("failure"));
    }

    #[test]
    fn blast_radius_includes_validations_and_neighbors() {
        let mut graph = Graph::new();
        let alpha = NodeId::new("demo", "demo::alpha", NodeKind::Function);
        let beta = NodeId::new("demo", "demo::beta", NodeKind::Function);
        graph.add_node(Node {
            id: alpha.clone(),
            name: "alpha".into(),
            kind: NodeKind::Function,
            file: FileId(1),
            span: Span::line(1),
            language: Language::Rust,
        });
        graph.add_node(Node {
            id: beta.clone(),
            name: "beta".into(),
            kind: NodeKind::Function,
            file: FileId(1),
            span: Span::line(2),
            language: Language::Rust,
        });
        graph.add_edge(Edge {
            kind: EdgeKind::Calls,
            source: alpha.clone(),
            target: beta.clone(),
            origin: prism_ir::EdgeOrigin::Static,
            confidence: 1.0,
        });

        let mut history = HistoryStore::new();
        history.seed_nodes([alpha.clone(), beta.clone()]);

        let outcomes = OutcomeMemory::new();
        outcomes
            .store_event(OutcomeEvent {
                meta: EventMeta {
                    id: EventId::new("outcome:2"),
                    ts: 2,
                    actor: EventActor::Agent,
                    correlation: Some(TaskId::new("task:beta")),
                    causation: None,
                },
                anchors: vec![AnchorRef::Node(alpha.clone())],
                kind: OutcomeKind::TestRan,
                result: OutcomeResult::Success,
                summary: "alpha requires unit test".into(),
                evidence: vec![OutcomeEvidence::Test {
                    name: "alpha_unit".into(),
                    passed: true,
                }],
                metadata: serde_json::Value::Null,
            })
            .unwrap();

        let prism = Prism::with_history_and_outcomes(graph, history, outcomes);
        let impact = prism.blast_radius(&alpha);
        assert!(impact.direct_nodes.contains(&beta));
        assert!(impact
            .likely_validations
            .iter()
            .any(|validation| validation == "test:alpha_unit"));
        assert!(impact
            .validation_checks
            .iter()
            .any(|check| check.label == "test:alpha_unit" && check.score > 0.0));
    }

    #[test]
    fn blast_radius_uses_co_change_history_and_neighbor_validations() {
        let mut graph = Graph::new();
        let alpha = NodeId::new("demo", "demo::alpha", NodeKind::Function);
        let beta = NodeId::new("demo", "demo::beta", NodeKind::Function);
        graph.add_node(Node {
            id: alpha.clone(),
            name: "alpha".into(),
            kind: NodeKind::Function,
            file: FileId(1),
            span: Span::line(1),
            language: Language::Rust,
        });
        graph.add_node(Node {
            id: beta.clone(),
            name: "beta".into(),
            kind: NodeKind::Function,
            file: FileId(1),
            span: Span::line(2),
            language: Language::Rust,
        });

        let mut history = HistoryStore::new();
        history.seed_nodes([alpha.clone(), beta.clone()]);
        history.apply(&ObservedChangeSet {
            meta: EventMeta {
                id: EventId::new("observed:cochange"),
                ts: 10,
                actor: EventActor::System,
                correlation: None,
                causation: None,
            },
            trigger: ChangeTrigger::ManualReindex,
            files: vec![FileId(1)],
            added: Vec::new(),
            removed: Vec::new(),
            updated: vec![
                (
                    ObservedNode {
                        node: Node {
                            id: alpha.clone(),
                            name: "alpha".into(),
                            kind: NodeKind::Function,
                            file: FileId(1),
                            span: Span::line(1),
                            language: Language::Rust,
                        },
                        fingerprint: prism_ir::SymbolFingerprint::with_parts(
                            10,
                            Some(20),
                            None,
                            None,
                        ),
                    },
                    ObservedNode {
                        node: Node {
                            id: alpha.clone(),
                            name: "alpha".into(),
                            kind: NodeKind::Function,
                            file: FileId(1),
                            span: Span::line(1),
                            language: Language::Rust,
                        },
                        fingerprint: prism_ir::SymbolFingerprint::with_parts(
                            10,
                            Some(21),
                            None,
                            None,
                        ),
                    },
                ),
                (
                    ObservedNode {
                        node: Node {
                            id: beta.clone(),
                            name: "beta".into(),
                            kind: NodeKind::Function,
                            file: FileId(1),
                            span: Span::line(2),
                            language: Language::Rust,
                        },
                        fingerprint: prism_ir::SymbolFingerprint::with_parts(
                            11,
                            Some(30),
                            None,
                            None,
                        ),
                    },
                    ObservedNode {
                        node: Node {
                            id: beta.clone(),
                            name: "beta".into(),
                            kind: NodeKind::Function,
                            file: FileId(1),
                            span: Span::line(2),
                            language: Language::Rust,
                        },
                        fingerprint: prism_ir::SymbolFingerprint::with_parts(
                            11,
                            Some(31),
                            None,
                            None,
                        ),
                    },
                ),
            ],
            edge_added: Vec::new(),
            edge_removed: Vec::new(),
        });

        let beta_lineage = history.lineage_of(&beta).unwrap();
        let outcomes = OutcomeMemory::new();
        outcomes
            .store_event(OutcomeEvent {
                meta: EventMeta {
                    id: EventId::new("outcome:cochange"),
                    ts: 11,
                    actor: EventActor::Agent,
                    correlation: Some(TaskId::new("task:beta")),
                    causation: None,
                },
                anchors: vec![AnchorRef::Lineage(beta_lineage)],
                kind: OutcomeKind::FailureObserved,
                result: OutcomeResult::Failure,
                summary: "beta changes usually need the integration test".into(),
                evidence: vec![OutcomeEvidence::Test {
                    name: "beta_integration".into(),
                    passed: false,
                }],
                metadata: serde_json::Value::Null,
            })
            .unwrap();

        let prism = Prism::with_history_and_outcomes(graph, history, outcomes);
        let impact = prism.blast_radius(&alpha);

        assert!(impact.direct_nodes.contains(&beta));
        assert!(impact
            .co_change_neighbors
            .iter()
            .any(|neighbor| neighbor.count == 1 && neighbor.nodes.contains(&beta)));
        assert!(impact
            .likely_validations
            .iter()
            .any(|validation| validation == "test:beta_integration"));
        assert!(impact
            .validation_checks
            .iter()
            .any(|check| check.label == "test:beta_integration" && check.score > 0.0));
        assert!(impact
            .risk_events
            .iter()
            .any(|event| event.summary.contains("integration test")));
    }

    #[test]
    fn coordination_queries_expand_into_neighboring_symbols() {
        let mut graph = Graph::new();
        let alpha = NodeId::new("demo", "demo::alpha", NodeKind::Function);
        let beta = NodeId::new("demo", "demo::beta", NodeKind::Function);
        graph.add_node(Node {
            id: alpha.clone(),
            name: "alpha".into(),
            kind: NodeKind::Function,
            file: FileId(1),
            span: Span::line(1),
            language: Language::Rust,
        });
        graph.add_node(Node {
            id: beta.clone(),
            name: "beta".into(),
            kind: NodeKind::Function,
            file: FileId(1),
            span: Span::line(2),
            language: Language::Rust,
        });
        graph.add_edge(Edge {
            kind: EdgeKind::Calls,
            source: alpha.clone(),
            target: beta.clone(),
            origin: prism_ir::EdgeOrigin::Static,
            confidence: 1.0,
        });

        let mut history = HistoryStore::new();
        history.seed_nodes([alpha.clone(), beta.clone()]);
        let coordination = CoordinationStore::new();
        let (plan_id, _) = coordination
            .create_plan(
                EventMeta {
                    id: EventId::new("coord:plan"),
                    ts: 1,
                    actor: EventActor::Agent,
                    correlation: None,
                    causation: None,
                },
                PlanCreateInput {
                    goal: "Coordinate alpha".into(),
                    policy: None,
                },
            )
            .unwrap();
        let (task_id, _) = coordination
            .create_task(
                EventMeta {
                    id: EventId::new("coord:task"),
                    ts: 2,
                    actor: EventActor::Agent,
                    correlation: None,
                    causation: None,
                },
                TaskCreateInput {
                    plan_id,
                    title: "Edit alpha".into(),
                    status: Some(prism_ir::CoordinationTaskStatus::Ready),
                    assignee: None,
                    session: Some(SessionId::new("session:a")),
                    anchors: vec![AnchorRef::Node(alpha.clone())],
                    depends_on: Vec::new(),
                    acceptance: Vec::new(),
                    base_revision: WorkspaceRevision {
                        graph_version: 1,
                        git_commit: None,
                    },
                },
            )
            .unwrap();
        coordination
            .acquire_claim(
                EventMeta {
                    id: EventId::new("coord:claim"),
                    ts: 3,
                    actor: EventActor::Agent,
                    correlation: None,
                    causation: None,
                },
                SessionId::new("session:a"),
                prism_coordination::ClaimAcquireInput {
                    task_id: Some(task_id),
                    anchors: vec![AnchorRef::Node(alpha.clone())],
                    capability: prism_ir::Capability::Edit,
                    mode: Some(prism_ir::ClaimMode::HardExclusive),
                    ttl_seconds: Some(120),
                    base_revision: WorkspaceRevision {
                        graph_version: 1,
                        git_commit: None,
                    },
                    agent: None,
                },
            )
            .unwrap();

        let prism = Prism::with_history_outcomes_coordination_and_projections(
            graph,
            history,
            OutcomeMemory::new(),
            coordination,
            ProjectionIndex::default(),
        );

        let claims = prism.claims(&[AnchorRef::Node(beta.clone())], 4);
        assert_eq!(claims.len(), 1);

        let simulated = prism.simulate_claim(
            &SessionId::new("session:b"),
            &[AnchorRef::Node(beta)],
            prism_ir::Capability::Edit,
            Some(prism_ir::ClaimMode::HardExclusive),
            None,
            4,
        );
        assert!(simulated
            .iter()
            .any(|conflict| conflict.severity == prism_ir::ConflictSeverity::Block));
    }

    #[test]
    fn validation_recipe_reuses_blast_radius_signal() {
        let mut graph = Graph::new();
        let alpha = NodeId::new("demo", "demo::alpha", NodeKind::Function);
        graph.add_node(Node {
            id: alpha.clone(),
            name: "alpha".into(),
            kind: NodeKind::Function,
            file: FileId(1),
            span: Span::line(1),
            language: Language::Rust,
        });

        let mut history = HistoryStore::new();
        history.seed_nodes([alpha.clone()]);

        let outcomes = OutcomeMemory::new();
        outcomes
            .store_event(OutcomeEvent {
                meta: EventMeta {
                    id: EventId::new("outcome:5"),
                    ts: 5,
                    actor: EventActor::Agent,
                    correlation: Some(TaskId::new("task:validate")),
                    causation: None,
                },
                anchors: vec![AnchorRef::Node(alpha.clone())],
                kind: OutcomeKind::FailureObserved,
                result: OutcomeResult::Failure,
                summary: "alpha broke an integration test".into(),
                evidence: vec![OutcomeEvidence::Test {
                    name: "alpha_integration".into(),
                    passed: false,
                }],
                metadata: serde_json::Value::Null,
            })
            .unwrap();

        let prism = Prism::with_history_and_outcomes(graph, history, outcomes);
        let recipe = prism.validation_recipe(&alpha);
        assert_eq!(recipe.target, alpha);
        assert_eq!(recipe.checks, vec!["test:alpha_integration"]);
        assert_eq!(recipe.scored_checks.len(), 1);
        assert_eq!(recipe.scored_checks[0].label, "test:alpha_integration");
        assert_eq!(recipe.recent_failures.len(), 1);
        assert_eq!(
            recipe.recent_failures[0].summary,
            "alpha broke an integration test"
        );
    }

    #[test]
    fn resume_task_returns_correlated_events() {
        let graph = Graph::new();
        let history = HistoryStore::new();
        let outcomes = OutcomeMemory::new();
        let task = TaskId::new("task:fix");
        outcomes
            .store_event(OutcomeEvent {
                meta: EventMeta {
                    id: EventId::new("outcome:3"),
                    ts: 3,
                    actor: EventActor::Agent,
                    correlation: Some(task.clone()),
                    causation: None,
                },
                anchors: Vec::new(),
                kind: OutcomeKind::PatchApplied,
                result: OutcomeResult::Success,
                summary: "applied patch".into(),
                evidence: Vec::new(),
                metadata: serde_json::Value::Null,
            })
            .unwrap();
        outcomes
            .store_event(OutcomeEvent {
                meta: EventMeta {
                    id: EventId::new("outcome:4"),
                    ts: 4,
                    actor: EventActor::Agent,
                    correlation: Some(task.clone()),
                    causation: Some(EventId::new("outcome:3")),
                },
                anchors: Vec::new(),
                kind: OutcomeKind::FixValidated,
                result: OutcomeResult::Success,
                summary: "validated patch".into(),
                evidence: Vec::new(),
                metadata: serde_json::Value::Null,
            })
            .unwrap();

        let prism = Prism::with_history_and_outcomes(graph, history, outcomes);
        let replay = prism.resume_task(&task);
        assert_eq!(replay.events.len(), 2);
        assert_eq!(replay.events[0].summary, "validated patch");
    }

    #[test]
    fn task_and_artifact_risk_join_coordination_with_change_intelligence() {
        let mut graph = Graph::new();
        let alpha = NodeId::new("demo", "demo::alpha", NodeKind::Function);
        graph.add_node(Node {
            id: alpha.clone(),
            name: "alpha".into(),
            kind: NodeKind::Function,
            file: FileId(1),
            span: Span::line(1),
            language: Language::Rust,
        });

        let mut history = HistoryStore::new();
        history.seed_nodes([alpha.clone()]);

        let outcomes = OutcomeMemory::new();
        outcomes
            .store_event(OutcomeEvent {
                meta: EventMeta {
                    id: EventId::new("outcome:risk"),
                    ts: 4,
                    actor: EventActor::Agent,
                    correlation: Some(TaskId::new("task:risk")),
                    causation: None,
                },
                anchors: vec![AnchorRef::Node(alpha.clone())],
                kind: OutcomeKind::FailureObserved,
                result: OutcomeResult::Failure,
                summary: "alpha changes usually break integration".into(),
                evidence: vec![OutcomeEvidence::Test {
                    name: "alpha_integration".into(),
                    passed: false,
                }],
                metadata: serde_json::Value::Null,
            })
            .unwrap();

        let coordination = CoordinationStore::new();
        let (plan_id, _) = coordination
            .create_plan(
                EventMeta {
                    id: EventId::new("coord:plan"),
                    ts: 1,
                    actor: EventActor::Agent,
                    correlation: None,
                    causation: None,
                },
                PlanCreateInput {
                    goal: "Risky edit".into(),
                    policy: Some(CoordinationPolicy {
                        review_required_above_risk_score: Some(0.2),
                        require_validation_for_completion: true,
                        ..CoordinationPolicy::default()
                    }),
                },
            )
            .unwrap();
        let (task_id, _) = coordination
            .create_task(
                EventMeta {
                    id: EventId::new("coord:task"),
                    ts: 2,
                    actor: EventActor::Agent,
                    correlation: None,
                    causation: None,
                },
                TaskCreateInput {
                    plan_id,
                    title: "Edit alpha".into(),
                    status: None,
                    assignee: None,
                    session: Some(SessionId::new("session:a")),
                    anchors: vec![AnchorRef::Node(alpha.clone())],
                    depends_on: Vec::new(),
                    acceptance: Vec::new(),
                    base_revision: WorkspaceRevision {
                        graph_version: 1,
                        git_commit: None,
                    },
                },
            )
            .unwrap();
        let (artifact_id, _) = coordination
            .propose_artifact(
                EventMeta {
                    id: EventId::new("coord:artifact"),
                    ts: 3,
                    actor: EventActor::Agent,
                    correlation: None,
                    causation: None,
                },
                ArtifactProposeInput {
                    task_id: task_id.clone(),
                    anchors: vec![AnchorRef::Node(alpha.clone())],
                    diff_ref: Some("patch:1".into()),
                    evidence: Vec::new(),
                    base_revision: WorkspaceRevision {
                        graph_version: 1,
                        git_commit: None,
                    },
                    required_validations: vec!["test:alpha_integration".into()],
                    validated_checks: Vec::new(),
                    risk_score: Some(0.7),
                },
            )
            .unwrap();

        let projections = ProjectionIndex::derive(&history.snapshot(), &outcomes.snapshot());
        let prism = Prism::with_history_outcomes_coordination_and_projections(
            graph,
            history,
            outcomes,
            coordination,
            projections,
        );

        let task_risk = prism.task_risk(&task_id, 5).unwrap();
        assert!(task_risk.review_required);
        assert_eq!(task_risk.likely_validations, vec!["test:alpha_integration"]);
        assert_eq!(
            task_risk.missing_validations,
            vec!["test:alpha_integration"]
        );

        let artifact_risk = prism.artifact_risk(&artifact_id, 5).unwrap();
        assert!(artifact_risk.review_required);
        assert_eq!(
            artifact_risk.missing_validations,
            vec!["test:alpha_integration"]
        );

        let blockers = prism.blockers(&task_id, 5);
        assert!(blockers.iter().any(|blocker| {
            blocker.kind == prism_coordination::BlockerKind::RiskReviewRequired
        }));
        assert!(blockers.iter().any(|blocker| {
            blocker.kind == prism_coordination::BlockerKind::ValidationRequired
        }));
    }

    #[test]
    fn exposes_intent_links_and_task_intent() {
        let mut graph = Graph::new();
        let spec = NodeId::new(
            "demo",
            "demo::document::docs::spec_md::behavior",
            NodeKind::MarkdownHeading,
        );
        let alpha = NodeId::new("demo", "demo::alpha", NodeKind::Function);
        let alpha_test = NodeId::new("demo", "demo::alpha_test", NodeKind::Function);
        graph.add_node(Node {
            id: spec.clone(),
            name: "Behavior".into(),
            kind: NodeKind::MarkdownHeading,
            file: FileId(1),
            span: Span::line(1),
            language: Language::Markdown,
        });
        graph.add_node(Node {
            id: alpha.clone(),
            name: "alpha".into(),
            kind: NodeKind::Function,
            file: FileId(2),
            span: Span::line(1),
            language: Language::Rust,
        });
        graph.add_node(Node {
            id: alpha_test.clone(),
            name: "alpha_test".into(),
            kind: NodeKind::Function,
            file: FileId(2),
            span: Span::line(2),
            language: Language::Rust,
        });
        graph.add_edge(Edge {
            kind: EdgeKind::Specifies,
            source: spec.clone(),
            target: alpha.clone(),
            origin: prism_ir::EdgeOrigin::Static,
            confidence: 0.8,
        });
        graph.add_edge(Edge {
            kind: EdgeKind::Validates,
            source: spec.clone(),
            target: alpha_test.clone(),
            origin: prism_ir::EdgeOrigin::Static,
            confidence: 0.8,
        });

        let coordination = CoordinationStore::new();
        let (plan_id, _) = coordination
            .create_plan(
                EventMeta {
                    id: EventId::new("coord:plan:intent"),
                    ts: 1,
                    actor: EventActor::Agent,
                    correlation: None,
                    causation: None,
                },
                PlanCreateInput {
                    goal: "Ship alpha".into(),
                    policy: None,
                },
            )
            .unwrap();
        let (task_id, _) = coordination
            .create_task(
                EventMeta {
                    id: EventId::new("coord:task:intent"),
                    ts: 2,
                    actor: EventActor::Agent,
                    correlation: None,
                    causation: None,
                },
                TaskCreateInput {
                    plan_id,
                    title: "Update alpha".into(),
                    status: None,
                    assignee: None,
                    session: Some(SessionId::new("session:intent")),
                    anchors: vec![AnchorRef::Node(alpha.clone())],
                    depends_on: Vec::new(),
                    acceptance: Vec::new(),
                    base_revision: WorkspaceRevision::default(),
                },
            )
            .unwrap();

        let prism = Prism::with_history_outcomes_coordination_and_projections(
            graph,
            HistoryStore::new(),
            OutcomeMemory::new(),
            coordination,
            ProjectionIndex::default(),
        );

        assert_eq!(prism.spec_for(&alpha), vec![spec.clone()]);
        assert_eq!(prism.implementation_for(&spec), vec![alpha.clone()]);

        let task_intent = prism.task_intent(&task_id).unwrap();
        assert_eq!(task_intent.specs, vec![spec.clone()]);
        assert_eq!(task_intent.implementations, vec![alpha.clone()]);
        assert_eq!(task_intent.validations, vec![alpha_test.clone()]);
        assert!(task_intent.drift_candidates.is_empty());
    }

    #[test]
    fn drift_candidates_flag_specs_without_validations() {
        let mut graph = Graph::new();
        let spec = NodeId::new(
            "demo",
            "demo::document::docs::spec_md::contract",
            NodeKind::MarkdownHeading,
        );
        let alpha = NodeId::new("demo", "demo::alpha", NodeKind::Function);
        graph.add_node(Node {
            id: spec.clone(),
            name: "Contract".into(),
            kind: NodeKind::MarkdownHeading,
            file: FileId(1),
            span: Span::line(1),
            language: Language::Markdown,
        });
        graph.add_node(Node {
            id: alpha.clone(),
            name: "alpha".into(),
            kind: NodeKind::Function,
            file: FileId(2),
            span: Span::line(1),
            language: Language::Rust,
        });
        graph.add_edge(Edge {
            kind: EdgeKind::Specifies,
            source: spec.clone(),
            target: alpha,
            origin: prism_ir::EdgeOrigin::Static,
            confidence: 0.8,
        });

        let prism = Prism::new(graph);
        let drift = prism.drift_candidates(10);
        assert_eq!(drift.len(), 1);
        assert_eq!(drift[0].spec, spec);
        assert!(drift[0]
            .reasons
            .iter()
            .any(|reason| reason == "no validation links"));
    }
}
