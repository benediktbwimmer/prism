use std::collections::{BTreeMap, BTreeSet};

use anyhow::{anyhow, Result};
use prism_ir::{
    ExecutorClass, NodeRef, PlanBinding, PlanEdge, PlanEdgeKind, PlanExecutionOverlay, PlanGraph,
    PlanId, PlanNode, PlanNodeId, PlanOperatorState, TaskExecutorPolicy, TaskId,
    TaskLifecycleStatus, ValidationRef, WorkspaceRevision,
};
use serde_json::{json, Map, Value};

use crate::{
    AcceptanceCriterion, CanonicalPlanRecord, CanonicalTaskRecord, CoordinationDependencyRecord,
    CoordinationPolicy, CoordinationSnapshot, CoordinationSnapshotV2, PlanScheduling,
    TaskGitExecution,
};

pub fn migrate_legacy_hybrid_snapshot_to_canonical_v2(
    snapshot: &CoordinationSnapshot,
    plan_graphs: &[PlanGraph],
    execution_overlays: &BTreeMap<String, Vec<PlanExecutionOverlay>>,
) -> Result<CoordinationSnapshotV2> {
    let mut migrated = snapshot.to_canonical_snapshot_v2();
    let mut state = MigrationState::new(&mut migrated, plan_graphs, execution_overlays);
    state.migrate()?;
    migrated.validate_graph()?;
    Ok(migrated)
}

struct MigrationState<'a> {
    migrated: &'a mut CoordinationSnapshotV2,
    plan_graphs: &'a [PlanGraph],
    execution_overlays: &'a BTreeMap<String, Vec<PlanExecutionOverlay>>,
    plan_index: BTreeMap<String, usize>,
    task_index: BTreeMap<String, usize>,
    dependency_keys: BTreeSet<(prism_ir::NodeRefKind, String, prism_ir::NodeRefKind, String)>,
}

impl<'a> MigrationState<'a> {
    fn new(
        migrated: &'a mut CoordinationSnapshotV2,
        plan_graphs: &'a [PlanGraph],
        execution_overlays: &'a BTreeMap<String, Vec<PlanExecutionOverlay>>,
    ) -> Self {
        let plan_index = migrated
            .plans
            .iter()
            .enumerate()
            .map(|(index, plan)| (plan.id.0.to_string(), index))
            .collect();
        let task_index = migrated
            .tasks
            .iter()
            .enumerate()
            .map(|(index, task)| (task.id.0.to_string(), index))
            .collect();
        let dependency_keys = migrated
            .dependencies
            .iter()
            .map(|edge| {
                (
                    edge.source.kind,
                    edge.source.id.clone(),
                    edge.target.kind,
                    edge.target.id.clone(),
                )
            })
            .collect();
        Self {
            migrated,
            plan_graphs,
            execution_overlays,
            plan_index,
            task_index,
            dependency_keys,
        }
    }

    fn migrate(&mut self) -> Result<()> {
        let mut graphs = self.plan_graphs.iter().collect::<Vec<_>>();
        graphs.sort_by(|left, right| left.id.0.cmp(&right.id.0));
        for graph in graphs {
            self.migrate_graph(graph)?;
        }
        self.migrated
            .plans
            .sort_by(|left, right| left.id.0.cmp(&right.id.0));
        self.migrated
            .tasks
            .sort_by(|left, right| left.id.0.cmp(&right.id.0));
        self.migrated.dependencies.sort_by(|left, right| {
            let left_key = (
                left.source.kind,
                left.source.id.as_str(),
                left.target.kind,
                left.target.id.as_str(),
            );
            let right_key = (
                right.source.kind,
                right.source.id.as_str(),
                right.target.kind,
                right.target.id.as_str(),
            );
            left_key.cmp(&right_key)
        });
        Ok(())
    }

    fn migrate_graph(&mut self, graph: &PlanGraph) -> Result<()> {
        let nodes = graph
            .nodes
            .iter()
            .cloned()
            .map(|node| (node.id.0.to_string(), node))
            .collect::<BTreeMap<_, _>>();
        let mut child_parents = BTreeMap::<String, String>::new();
        let mut child_counts = BTreeMap::<String, usize>::new();
        let mut edges = graph.edges.clone();
        edges.sort_by(|left, right| left.id.0.cmp(&right.id.0));
        for edge in &edges {
            if edge.kind != PlanEdgeKind::ChildOf {
                continue;
            }
            let child_id = edge.from.0.to_string();
            let parent_id = edge.to.0.to_string();
            if let Some(existing) = child_parents.insert(child_id.clone(), parent_id.clone()) {
                if existing != parent_id {
                    return Err(anyhow!(
                        "legacy node `{}` has multiple child_of parents (`{}` and `{}`); repair required before v2 migration",
                        child_id,
                        existing,
                        parent_id
                    ));
                }
            }
            *child_counts.entry(parent_id).or_default() += 1;
        }

        let mut node_ids = nodes.keys().cloned().collect::<Vec<_>>();
        node_ids.sort();

        for node_id in &node_ids {
            let node = &nodes[node_id];
            if node.is_abstract {
                self.drop_native_plan_node_task_alias(node_id);
                self.ensure_migrated_child_plan(graph, node, child_parents.get(node_id))?;
            }
        }

        for node_id in &node_ids {
            let node = &nodes[node_id];
            if node.is_abstract {
                continue;
            }
            if is_native_plan_node_alias(node.id.0.as_str())
                && self.task_index.contains_key(node_id)
            {
                self.rewrite_native_plan_node_task_alias(graph, node, child_parents.get(node_id))?;
                continue;
            }
            if self.task_index.contains_key(node_id) {
                continue;
            }
            self.ensure_migrated_leaf_task(graph, node, child_parents.get(node_id))?;
        }

        for node_id in &node_ids {
            let node = &nodes[node_id];
            if !node.is_abstract {
                continue;
            }
            if child_counts.get(node_id).copied().unwrap_or_default() == 0 {
                self.ensure_placeholder_task(node)?;
            }
        }

        for edge in &edges {
            self.migrate_edge(graph, &nodes, edge)?;
        }

        Ok(())
    }

    fn drop_native_plan_node_task_alias(&mut self, task_id: &str) {
        if !is_native_plan_node_alias(task_id) {
            return;
        }
        let Some(index) = self.task_index.get(task_id).copied() else {
            return;
        };
        self.migrated.tasks.remove(index);
        self.migrated.dependencies.retain(|edge| {
            !((edge.source.kind == prism_ir::NodeRefKind::Task && edge.source.id == task_id)
                || (edge.target.kind == prism_ir::NodeRefKind::Task && edge.target.id == task_id))
        });
        self.rebuild_indexes();
    }

    fn rewrite_native_plan_node_task_alias(
        &mut self,
        graph: &PlanGraph,
        node: &PlanNode,
        parent_node_id: Option<&String>,
    ) -> Result<()> {
        let Some(index) = self.task_index.get(node.id.0.as_str()).copied() else {
            return Ok(());
        };
        let overlay = self
            .execution_overlays
            .get(graph.id.0.as_str())
            .and_then(|overlays| overlays.iter().find(|overlay| overlay.node_id == node.id));
        let parent_plan_id = self.resolve_parent_plan_id(graph, parent_node_id)?;
        let metadata = migrated_task_metadata(node, graph.id.clone());
        let task = self
            .migrated
            .tasks
            .get_mut(index)
            .ok_or_else(|| anyhow!("missing native plan-node task alias `{}`", node.id.0))?;
        task.parent_plan_id = parent_plan_id;
        task.title = node.title.clone();
        task.summary = node.summary.clone();
        task.lifecycle_status = migrated_task_lifecycle(node.status);
        task.estimated_minutes = 0;
        task.executor = migrated_task_executor_policy(&node.metadata);
        task.assignee = node.assignee.clone();
        task.session = overlay.and_then(|overlay| overlay.session.clone());
        task.lease_holder = None;
        task.lease_started_at = None;
        task.lease_refreshed_at = None;
        task.lease_stale_at = None;
        task.lease_expires_at = None;
        task.worktree_id = overlay.and_then(|overlay| overlay.worktree_id.clone());
        task.branch_ref = overlay.and_then(|overlay| overlay.branch_ref.clone());
        task.anchors = node.bindings.anchors.clone();
        task.bindings = node.bindings.clone();
        task.acceptance = node
            .acceptance
            .iter()
            .map(migrate_acceptance)
            .collect::<Vec<_>>();
        task.validation_refs = node.validation_refs.clone();
        task.base_revision = node.base_revision.clone();
        task.priority = node.priority;
        task.tags = node.tags.clone();
        task.metadata = metadata;
        task.git_execution = migrated_git_execution(overlay);
        Ok(())
    }

    fn ensure_migrated_child_plan(
        &mut self,
        graph: &PlanGraph,
        node: &PlanNode,
        parent_node_id: Option<&String>,
    ) -> Result<()> {
        let plan_id = migrated_child_plan_id(&node.id);
        if self.plan_index.contains_key(plan_id.0.as_str()) {
            return Ok(());
        }
        let parent_plan_id = Some(self.resolve_parent_plan_id(graph, parent_node_id)?);
        let goal = node.summary.clone().unwrap_or_else(|| node.title.clone());
        let metadata = migrated_plan_metadata(node, graph.id.clone());
        let record = CanonicalPlanRecord {
            id: plan_id.clone(),
            parent_plan_id,
            title: node.title.clone(),
            goal,
            scope: graph.scope,
            kind: graph.kind,
            policy: CoordinationPolicy::default(),
            scheduling: PlanScheduling::default(),
            tags: node.tags.clone(),
            created_from: None,
            metadata,
            operator_state: PlanOperatorState::None,
        };
        self.plan_index
            .insert(plan_id.0.to_string(), self.migrated.plans.len());
        self.migrated.plans.push(record);
        Ok(())
    }

    fn ensure_migrated_leaf_task(
        &mut self,
        graph: &PlanGraph,
        node: &PlanNode,
        parent_node_id: Option<&String>,
    ) -> Result<()> {
        let task_id = migrated_leaf_task_id(&node.id);
        if self.task_index.contains_key(task_id.0.as_str()) {
            return Ok(());
        }
        let overlay = self
            .execution_overlays
            .get(graph.id.0.as_str())
            .and_then(|overlays| overlays.iter().find(|overlay| overlay.node_id == node.id));
        let metadata = migrated_task_metadata(node, graph.id.clone());
        let task = CanonicalTaskRecord {
            id: task_id.clone(),
            parent_plan_id: self.resolve_parent_plan_id(graph, parent_node_id)?,
            title: node.title.clone(),
            summary: node.summary.clone(),
            kind: node.kind,
            status: migrated_task_status(node.status),
            lifecycle_status: migrated_task_lifecycle(node.status),
            estimated_minutes: 0,
            executor: migrated_task_executor_policy(&node.metadata),
            assignee: node.assignee.clone(),
            pending_handoff_to: overlay.and_then(|overlay| overlay.pending_handoff_to.clone()),
            session: overlay.and_then(|overlay| overlay.session.clone()),
            lease_holder: None,
            lease_started_at: None,
            lease_refreshed_at: None,
            lease_stale_at: None,
            lease_expires_at: None,
            worktree_id: overlay.and_then(|overlay| overlay.worktree_id.clone()),
            branch_ref: overlay.and_then(|overlay| overlay.branch_ref.clone()),
            anchors: node.bindings.anchors.clone(),
            bindings: node.bindings.clone(),
            acceptance: node
                .acceptance
                .iter()
                .map(migrate_acceptance)
                .collect::<Vec<_>>(),
            validation_refs: node.validation_refs.clone(),
            is_abstract: node.is_abstract,
            base_revision: node.base_revision.clone(),
            priority: node.priority,
            tags: node.tags.clone(),
            metadata,
            git_execution: migrated_git_execution(overlay),
        };
        self.task_index
            .insert(task_id.0.to_string(), self.migrated.tasks.len());
        self.migrated.tasks.push(task);
        Ok(())
    }

    fn ensure_placeholder_task(&mut self, node: &PlanNode) -> Result<()> {
        let task_id = migrated_placeholder_task_id(&node.id);
        if self.task_index.contains_key(task_id.0.as_str()) {
            return Ok(());
        }
        let task = CanonicalTaskRecord {
            id: task_id.clone(),
            parent_plan_id: migrated_child_plan_id(&node.id),
            title: "Fill migrated empty child plan".to_string(),
            summary: None,
            kind: prism_ir::PlanNodeKind::Edit,
            status: prism_ir::CoordinationTaskStatus::Proposed,
            lifecycle_status: TaskLifecycleStatus::Pending,
            estimated_minutes: 0,
            executor: TaskExecutorPolicy {
                executor_class: ExecutorClass::WorktreeExecutor,
                target_label: None,
                allowed_principals: Vec::new(),
            },
            assignee: None,
            pending_handoff_to: None,
            session: None,
            lease_holder: None,
            lease_started_at: None,
            lease_refreshed_at: None,
            lease_stale_at: None,
            lease_expires_at: None,
            worktree_id: None,
            branch_ref: None,
            anchors: Vec::new(),
            bindings: PlanBinding::default(),
            acceptance: Vec::new(),
            validation_refs: Vec::<ValidationRef>::new(),
            is_abstract: false,
            base_revision: WorkspaceRevision::default(),
            priority: None,
            tags: Vec::new(),
            metadata: json!({
                "migration_placeholder": true,
                "legacy_abstract_node_id": node.id.0,
            }),
            git_execution: TaskGitExecution::default(),
        };
        self.task_index
            .insert(task_id.0.to_string(), self.migrated.tasks.len());
        self.migrated.tasks.push(task);
        Ok(())
    }

    fn migrate_edge(
        &mut self,
        graph: &PlanGraph,
        nodes: &BTreeMap<String, PlanNode>,
        edge: &PlanEdge,
    ) -> Result<()> {
        let Some(source_node) = nodes.get(edge.from.0.as_str()) else {
            return Err(anyhow!(
                "legacy plan graph `{}` references unknown source node `{}`",
                graph.id.0,
                edge.from.0
            ));
        };
        let Some(target_node) = nodes.get(edge.to.0.as_str()) else {
            return Err(anyhow!(
                "legacy plan graph `{}` references unknown target node `{}`",
                graph.id.0,
                edge.to.0
            ));
        };
        match edge.kind {
            PlanEdgeKind::DependsOn | PlanEdgeKind::Blocks => {
                let source = self.node_ref_for_migration_node(source_node);
                let target = self.node_ref_for_migration_node(target_node);
                let key = (
                    source.kind,
                    source.id.clone(),
                    target.kind,
                    target.id.clone(),
                );
                if self.dependency_keys.insert(key) {
                    self.migrated
                        .dependencies
                        .push(CoordinationDependencyRecord { source, target });
                }
            }
            PlanEdgeKind::ChildOf => {}
            PlanEdgeKind::Validates
            | PlanEdgeKind::HandoffTo
            | PlanEdgeKind::Informs
            | PlanEdgeKind::RelatedTo => {
                self.append_legacy_edge_metadata(source_node, target_node, edge)?;
            }
        }
        Ok(())
    }

    fn append_legacy_edge_metadata(
        &mut self,
        source_node: &PlanNode,
        target_node: &PlanNode,
        edge: &PlanEdge,
    ) -> Result<()> {
        let entry = json!({
            "kind": format!("{:?}", edge.kind).to_ascii_lowercase(),
            "target": {
                "kind": if target_node.is_abstract { "plan" } else { "task" },
                "id": self.node_ref_for_migration_node(target_node).id,
                "legacyNodeId": target_node.id.0,
            },
            "summary": edge.summary,
            "metadata": edge.metadata,
        });
        if source_node.is_abstract {
            let plan_id = migrated_child_plan_id(&source_node.id);
            let Some(index) = self.plan_index.get(plan_id.0.as_str()).copied() else {
                return Err(anyhow!("missing migrated child plan `{}`", plan_id.0));
            };
            append_legacy_edge_entry(&mut self.migrated.plans[index].metadata, entry);
        } else {
            let task_id = if self.task_index.contains_key(source_node.id.0.as_str()) {
                TaskId::new(source_node.id.0.clone())
            } else {
                migrated_leaf_task_id(&source_node.id)
            };
            let Some(index) = self.task_index.get(task_id.0.as_str()).copied() else {
                return Err(anyhow!("missing migrated task `{}`", task_id.0));
            };
            append_legacy_edge_entry(&mut self.migrated.tasks[index].metadata, entry);
        }
        Ok(())
    }

    fn node_ref_for_migration_node(&self, node: &PlanNode) -> NodeRef {
        if node.is_abstract {
            NodeRef::plan(migrated_child_plan_id(&node.id))
        } else if self.task_index.contains_key(node.id.0.as_str()) {
            NodeRef::task(TaskId::new(node.id.0.clone()))
        } else {
            NodeRef::task(migrated_leaf_task_id(&node.id))
        }
    }

    fn resolve_parent_plan_id(
        &self,
        graph: &PlanGraph,
        parent_node_id: Option<&String>,
    ) -> Result<PlanId> {
        match parent_node_id {
            Some(parent_node_id) => Ok(migrated_child_plan_id(&PlanNodeId::new(parent_node_id))),
            None => Ok(graph.id.clone()),
        }
    }

    fn rebuild_indexes(&mut self) {
        self.plan_index = self
            .migrated
            .plans
            .iter()
            .enumerate()
            .map(|(index, plan)| (plan.id.0.to_string(), index))
            .collect();
        self.task_index = self
            .migrated
            .tasks
            .iter()
            .enumerate()
            .map(|(index, task)| (task.id.0.to_string(), index))
            .collect();
        self.dependency_keys = self
            .migrated
            .dependencies
            .iter()
            .map(|edge| {
                (
                    edge.source.kind,
                    edge.source.id.clone(),
                    edge.target.kind,
                    edge.target.id.clone(),
                )
            })
            .collect();
    }
}

fn migrated_child_plan_id(node_id: &PlanNodeId) -> PlanId {
    PlanId::new(format!("plan:migrated:{}", node_id.0))
}

fn is_native_plan_node_alias(id: &str) -> bool {
    id.starts_with("plan-node:")
}

fn migrated_leaf_task_id(node_id: &PlanNodeId) -> TaskId {
    TaskId::new(format!("task:migrated:{}", node_id.0))
}

fn migrated_placeholder_task_id(node_id: &PlanNodeId) -> TaskId {
    TaskId::new(format!("task:migrated:{}:placeholder", node_id.0))
}

fn migrated_task_lifecycle(status: prism_ir::PlanNodeStatus) -> TaskLifecycleStatus {
    match status {
        prism_ir::PlanNodeStatus::Proposed
        | prism_ir::PlanNodeStatus::Ready
        | prism_ir::PlanNodeStatus::Blocked
        | prism_ir::PlanNodeStatus::Waiting => TaskLifecycleStatus::Pending,
        prism_ir::PlanNodeStatus::InProgress
        | prism_ir::PlanNodeStatus::InReview
        | prism_ir::PlanNodeStatus::Validating => TaskLifecycleStatus::Active,
        prism_ir::PlanNodeStatus::Completed => TaskLifecycleStatus::Completed,
        prism_ir::PlanNodeStatus::Abandoned => TaskLifecycleStatus::Abandoned,
    }
}

fn migrated_task_status(status: prism_ir::PlanNodeStatus) -> prism_ir::CoordinationTaskStatus {
    match status {
        prism_ir::PlanNodeStatus::Proposed => prism_ir::CoordinationTaskStatus::Proposed,
        prism_ir::PlanNodeStatus::Ready => prism_ir::CoordinationTaskStatus::Ready,
        prism_ir::PlanNodeStatus::InProgress => prism_ir::CoordinationTaskStatus::InProgress,
        prism_ir::PlanNodeStatus::Blocked | prism_ir::PlanNodeStatus::Waiting => {
            prism_ir::CoordinationTaskStatus::Blocked
        }
        prism_ir::PlanNodeStatus::InReview => prism_ir::CoordinationTaskStatus::InReview,
        prism_ir::PlanNodeStatus::Validating => prism_ir::CoordinationTaskStatus::Validating,
        prism_ir::PlanNodeStatus::Completed => prism_ir::CoordinationTaskStatus::Completed,
        prism_ir::PlanNodeStatus::Abandoned => prism_ir::CoordinationTaskStatus::Abandoned,
    }
}

fn migrated_task_executor_policy(metadata: &Value) -> TaskExecutorPolicy {
    metadata
        .get("executor")
        .cloned()
        .and_then(|value| serde_json::from_value::<TaskExecutorPolicy>(value).ok())
        .unwrap_or(TaskExecutorPolicy {
            executor_class: ExecutorClass::WorktreeExecutor,
            target_label: None,
            allowed_principals: Vec::new(),
        })
}

fn migrated_task_metadata(node: &PlanNode, legacy_plan_id: PlanId) -> Value {
    let mut metadata = metadata_object(node.metadata.clone());
    metadata
        .entry("legacy_node_id".to_string())
        .or_insert_with(|| Value::String(node.id.0.to_string()));
    metadata
        .entry("legacy_plan_id".to_string())
        .or_insert_with(|| Value::String(legacy_plan_id.0.to_string()));
    metadata
        .entry("legacy_kind".to_string())
        .or_insert_with(|| Value::String(format!("{:?}", node.kind).to_ascii_lowercase()));
    if let Some(phase) = legacy_node_phase(node.status) {
        metadata
            .entry("legacy_phase".to_string())
            .or_insert_with(|| Value::String(phase.to_string()));
    }
    metadata
        .entry("legacy_is_abstract".to_string())
        .or_insert(Value::Bool(false));
    Value::Object(metadata)
}

fn migrated_plan_metadata(node: &PlanNode, legacy_plan_id: PlanId) -> Value {
    let mut metadata = metadata_object(node.metadata.clone());
    metadata
        .entry("legacy_node_id".to_string())
        .or_insert_with(|| Value::String(node.id.0.to_string()));
    metadata
        .entry("legacy_plan_id".to_string())
        .or_insert_with(|| Value::String(legacy_plan_id.0.to_string()));
    metadata
        .entry("legacy_kind".to_string())
        .or_insert_with(|| Value::String(format!("{:?}", node.kind).to_ascii_lowercase()));
    metadata
        .entry("legacy_node_status".to_string())
        .or_insert_with(|| Value::String(format!("{:?}", node.status).to_ascii_lowercase()));
    metadata
        .entry("legacy_is_abstract".to_string())
        .or_insert(Value::Bool(true));
    Value::Object(metadata)
}

fn migrated_git_execution(overlay: Option<&PlanExecutionOverlay>) -> TaskGitExecution {
    overlay
        .and_then(|overlay| overlay.git_execution.clone())
        .map(|overlay| TaskGitExecution {
            status: overlay.status,
            pending_task_status: overlay.pending_task_status,
            source_ref: overlay.source_ref,
            target_ref: overlay.target_ref,
            publish_ref: overlay.publish_ref,
            target_branch: overlay.target_branch,
            source_commit: overlay.source_commit,
            publish_commit: overlay.publish_commit,
            target_commit_at_publish: overlay.target_commit_at_publish,
            review_artifact_ref: overlay.review_artifact_ref,
            integration_commit: overlay.integration_commit,
            integration_evidence: overlay.integration_evidence,
            integration_mode: overlay.integration_mode,
            integration_status: overlay.integration_status,
            last_preflight: None,
            last_publish: None,
        })
        .unwrap_or_default()
}

fn legacy_node_phase(status: prism_ir::PlanNodeStatus) -> Option<&'static str> {
    match status {
        prism_ir::PlanNodeStatus::Blocked | prism_ir::PlanNodeStatus::Waiting => Some("blocked"),
        prism_ir::PlanNodeStatus::InReview => Some("in_review"),
        prism_ir::PlanNodeStatus::Validating => Some("validating"),
        prism_ir::PlanNodeStatus::Proposed
        | prism_ir::PlanNodeStatus::Ready
        | prism_ir::PlanNodeStatus::InProgress
        | prism_ir::PlanNodeStatus::Completed
        | prism_ir::PlanNodeStatus::Abandoned => None,
    }
}

fn migrate_acceptance(criterion: &prism_ir::PlanAcceptanceCriterion) -> AcceptanceCriterion {
    AcceptanceCriterion {
        label: criterion.label.clone(),
        anchors: criterion.anchors.clone(),
    }
}

fn metadata_object(value: Value) -> Map<String, Value> {
    match value {
        Value::Object(object) => object,
        Value::Null => Map::new(),
        other => {
            let mut object = Map::new();
            object.insert("legacy_metadata_value".to_string(), other);
            object
        }
    }
}

fn append_legacy_edge_entry(metadata: &mut Value, entry: Value) {
    if !metadata.is_object() {
        let mut object = Map::new();
        if !metadata.is_null() {
            object.insert(
                "legacy_metadata_value".to_string(),
                std::mem::replace(metadata, Value::Null),
            );
        }
        *metadata = Value::Object(object);
    }
    let object = metadata
        .as_object_mut()
        .expect("metadata should be normalized to an object");
    let legacy_edges = object
        .entry("legacy_edges".to_string())
        .or_insert_with(|| Value::Array(Vec::new()));
    if !legacy_edges.is_array() {
        let existing = std::mem::replace(legacy_edges, Value::Array(Vec::new()));
        *legacy_edges = Value::Array(vec![existing]);
    }
    if let Value::Array(edges) = legacy_edges {
        edges.push(entry);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use prism_ir::{
        AgentId, CoordinationTaskStatus, PlanKind, PlanNodeKind, PlanNodeStatus, PlanScope,
        PlanStatus, SessionId, ValidationRef, WorkspaceRevision,
    };

    fn edge(plan_id: &PlanId, from: &str, to: &str, kind: PlanEdgeKind) -> PlanEdge {
        PlanEdge {
            id: prism_ir::PlanEdgeId::new(format!("{from}:{:?}:{to}", kind)),
            plan_id: plan_id.clone(),
            from: PlanNodeId::new(from),
            to: PlanNodeId::new(to),
            kind,
            summary: Some(format!("{kind:?}")),
            metadata: Value::Null,
        }
    }

    fn node(plan_id: &PlanId, node_id: &str, title: &str, is_abstract: bool) -> PlanNode {
        PlanNode {
            id: PlanNodeId::new(node_id),
            plan_id: plan_id.clone(),
            kind: if is_abstract {
                PlanNodeKind::Note
            } else {
                PlanNodeKind::Edit
            },
            title: title.to_string(),
            summary: None,
            status: PlanNodeStatus::Ready,
            bindings: PlanBinding::default(),
            acceptance: Vec::new(),
            validation_refs: Vec::new(),
            is_abstract,
            assignee: None,
            base_revision: WorkspaceRevision::default(),
            priority: None,
            tags: Vec::new(),
            metadata: Value::Null,
        }
    }

    #[test]
    fn hybrid_migration_converts_standalone_nodes_into_child_plans_and_tasks() {
        let plan_id = PlanId::new("plan:demo");
        let snapshot = CoordinationSnapshot {
            plans: vec![crate::Plan {
                id: plan_id.clone(),
                goal: "goal".into(),
                title: "Demo".into(),
                status: PlanStatus::Active,
                policy: CoordinationPolicy::default(),
                scope: PlanScope::Repo,
                kind: PlanKind::TaskExecution,
                revision: 0,
                scheduling: PlanScheduling::default(),
                tags: Vec::new(),
                created_from: None,
                metadata: Value::Null,
            }],
            tasks: vec![crate::CoordinationTask {
                id: prism_ir::CoordinationTaskId::new("coord-task:existing"),
                plan: plan_id.clone(),
                kind: PlanNodeKind::Edit,
                title: "Existing".into(),
                summary: None,
                status: CoordinationTaskStatus::InReview,
                assignee: Some(AgentId::new("agent:demo")),
                pending_handoff_to: None,
                session: Some(SessionId::new("session:demo")),
                lease_holder: None,
                lease_started_at: None,
                lease_refreshed_at: None,
                lease_stale_at: None,
                lease_expires_at: None,
                worktree_id: None,
                branch_ref: None,
                anchors: Vec::new(),
                bindings: PlanBinding::default(),
                depends_on: Vec::new(),
                coordination_depends_on: Vec::new(),
                integrated_depends_on: Vec::new(),
                acceptance: Vec::new(),
                validation_refs: vec![ValidationRef {
                    id: "validation:existing".into(),
                }],
                is_abstract: false,
                base_revision: WorkspaceRevision::default(),
                priority: None,
                tags: Vec::new(),
                metadata: Value::Null,
                git_execution: TaskGitExecution::default(),
            }],
            claims: Vec::new(),
            artifacts: Vec::new(),
            reviews: Vec::new(),
            events: Vec::new(),
            next_plan: 0,
            next_task: 0,
            next_claim: 0,
            next_artifact: 0,
            next_review: 0,
        };
        let graph = PlanGraph {
            id: plan_id.clone(),
            scope: PlanScope::Repo,
            kind: PlanKind::TaskExecution,
            title: "Demo".into(),
            goal: "Demo".into(),
            status: PlanStatus::Active,
            revision: 0,
            root_nodes: vec![
                PlanNodeId::new("plan-node:parent"),
                PlanNodeId::new("coord-task:existing"),
            ],
            tags: Vec::new(),
            created_from: None,
            metadata: Value::Null,
            nodes: vec![
                node(&plan_id, "plan-node:parent", "Parent", true),
                node(&plan_id, "plan-node:leaf", "Leaf", false),
                PlanNode {
                    id: PlanNodeId::new("coord-task:existing"),
                    plan_id: plan_id.clone(),
                    kind: PlanNodeKind::Validate,
                    title: "Existing".into(),
                    summary: None,
                    status: PlanNodeStatus::InReview,
                    bindings: PlanBinding::default(),
                    acceptance: Vec::new(),
                    validation_refs: vec![ValidationRef {
                        id: "validation:existing".into(),
                    }],
                    is_abstract: false,
                    assignee: Some(AgentId::new("agent:demo")),
                    base_revision: WorkspaceRevision::default(),
                    priority: None,
                    tags: Vec::new(),
                    metadata: Value::Null,
                },
            ],
            edges: vec![
                edge(
                    &plan_id,
                    "plan-node:leaf",
                    "plan-node:parent",
                    PlanEdgeKind::ChildOf,
                ),
                edge(
                    &plan_id,
                    "coord-task:existing",
                    "plan-node:leaf",
                    PlanEdgeKind::DependsOn,
                ),
                edge(
                    &plan_id,
                    "plan-node:leaf",
                    "coord-task:existing",
                    PlanEdgeKind::Validates,
                ),
            ],
        };

        let migrated =
            migrate_legacy_hybrid_snapshot_to_canonical_v2(&snapshot, &[graph], &BTreeMap::new())
                .expect("migration should succeed");

        assert!(migrated
            .plans
            .iter()
            .any(|plan| plan.id.0 == "plan:migrated:plan-node:parent"
                && plan.parent_plan_id == Some(plan_id.clone())));
        assert!(migrated.tasks.iter().any(|task| {
            task.id.0 == "task:migrated:plan-node:leaf"
                && task.parent_plan_id.0 == "plan:migrated:plan-node:parent"
        }));
        assert!(migrated.tasks.iter().any(|task| {
            task.id.0 == "coord-task:existing"
                && task.status == CoordinationTaskStatus::InReview
        }));
        assert!(migrated.dependencies.iter().any(|edge| {
            edge.source == NodeRef::task(TaskId::new("coord-task:existing"))
                && edge.target == NodeRef::task(TaskId::new("task:migrated:plan-node:leaf"))
        }));
        let migrated_leaf = migrated
            .tasks
            .iter()
            .find(|task| task.id.0 == "task:migrated:plan-node:leaf")
            .expect("migrated leaf task");
        assert_eq!(
            migrated_leaf
                .metadata
                .get("legacy_edges")
                .and_then(Value::as_array)
                .map(Vec::len),
            Some(1)
        );
    }

    #[test]
    fn hybrid_migration_inserts_placeholder_task_for_empty_abstract_node() {
        let plan_id = PlanId::new("plan:demo");
        let snapshot = CoordinationSnapshot {
            plans: vec![crate::Plan {
                id: plan_id.clone(),
                goal: "goal".into(),
                title: "Demo".into(),
                status: PlanStatus::Active,
                policy: CoordinationPolicy::default(),
                scope: PlanScope::Repo,
                kind: PlanKind::TaskExecution,
                revision: 0,
                scheduling: PlanScheduling::default(),
                tags: Vec::new(),
                created_from: None,
                metadata: Value::Null,
            }],
            ..CoordinationSnapshot::default()
        };
        let graph = PlanGraph {
            id: plan_id.clone(),
            scope: PlanScope::Repo,
            kind: PlanKind::TaskExecution,
            title: "Demo".into(),
            goal: "Demo".into(),
            status: PlanStatus::Active,
            revision: 0,
            root_nodes: vec![PlanNodeId::new("plan-node:empty")],
            tags: Vec::new(),
            created_from: None,
            metadata: Value::Null,
            nodes: vec![node(&plan_id, "plan-node:empty", "Empty", true)],
            edges: Vec::new(),
        };

        let migrated =
            migrate_legacy_hybrid_snapshot_to_canonical_v2(&snapshot, &[graph], &BTreeMap::new())
                .expect("migration should succeed");

        assert!(migrated.tasks.iter().any(|task| {
            task.id.0 == "task:migrated:plan-node:empty:placeholder"
                && task.parent_plan_id.0 == "plan:migrated:plan-node:empty"
        }));
    }

    #[test]
    fn hybrid_migration_rejects_multi_parent_child_of_shapes() {
        let plan_id = PlanId::new("plan:demo");
        let snapshot = CoordinationSnapshot {
            plans: vec![crate::Plan {
                id: plan_id.clone(),
                goal: "goal".into(),
                title: "Demo".into(),
                status: PlanStatus::Active,
                policy: CoordinationPolicy::default(),
                scope: PlanScope::Repo,
                kind: PlanKind::TaskExecution,
                revision: 0,
                scheduling: PlanScheduling::default(),
                tags: Vec::new(),
                created_from: None,
                metadata: Value::Null,
            }],
            ..CoordinationSnapshot::default()
        };
        let graph = PlanGraph {
            id: plan_id.clone(),
            scope: PlanScope::Repo,
            kind: PlanKind::TaskExecution,
            title: "Demo".into(),
            goal: "Demo".into(),
            status: PlanStatus::Active,
            revision: 0,
            root_nodes: vec![
                PlanNodeId::new("plan-node:parent-a"),
                PlanNodeId::new("plan-node:parent-b"),
            ],
            tags: Vec::new(),
            created_from: None,
            metadata: Value::Null,
            nodes: vec![
                node(&plan_id, "plan-node:parent-a", "Parent A", true),
                node(&plan_id, "plan-node:parent-b", "Parent B", true),
                node(&plan_id, "plan-node:child", "Child", false),
            ],
            edges: vec![
                edge(
                    &plan_id,
                    "plan-node:child",
                    "plan-node:parent-a",
                    PlanEdgeKind::ChildOf,
                ),
                edge(
                    &plan_id,
                    "plan-node:child",
                    "plan-node:parent-b",
                    PlanEdgeKind::ChildOf,
                ),
            ],
        };

        let error =
            migrate_legacy_hybrid_snapshot_to_canonical_v2(&snapshot, &[graph], &BTreeMap::new())
                .expect_err("multi-parent child_of should fail");
        assert!(error.to_string().contains("repair required"));
    }
}
