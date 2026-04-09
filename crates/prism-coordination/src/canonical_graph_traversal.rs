use std::collections::{BTreeMap, BTreeSet};

use anyhow::{anyhow, Result};
use prism_ir::{NodeRef, NodeRefKind, PlanId, TaskId};

use crate::canonical_graph::{CanonicalPlanRecord, CanonicalTaskRecord, CoordinationSnapshotV2};

type NodeKey = (NodeRefKind, String);

#[derive(Debug, Clone, Copy)]
pub enum CanonicalNodeRecord<'a> {
    Plan(&'a CanonicalPlanRecord),
    Task(&'a CanonicalTaskRecord),
}

impl<'a> CanonicalNodeRecord<'a> {
    pub fn node_ref(self) -> NodeRef {
        match self {
            Self::Plan(plan) => NodeRef::plan(plan.id.clone()),
            Self::Task(task) => NodeRef::task(task.id.clone()),
        }
    }
}

#[derive(Debug)]
pub struct CanonicalCoordinationGraph<'a> {
    nodes: BTreeMap<NodeKey, CanonicalNodeRecord<'a>>,
    children_by_plan: BTreeMap<String, Vec<NodeRef>>,
    dependency_targets_by_source: BTreeMap<NodeKey, Vec<NodeRef>>,
    dependency_sources_by_target: BTreeMap<NodeKey, Vec<NodeRef>>,
    topological_order: Vec<NodeRef>,
}

impl<'a> CanonicalCoordinationGraph<'a> {
    pub fn new(snapshot: &'a CoordinationSnapshotV2) -> Result<Self> {
        let mut nodes = BTreeMap::new();
        for plan in &snapshot.plans {
            let key = plan_key(&plan.id);
            if nodes
                .insert(key.clone(), CanonicalNodeRecord::Plan(plan))
                .is_some()
            {
                return Err(anyhow!("duplicate canonical plan `{}`", plan.id.0));
            }
        }
        for task in &snapshot.tasks {
            let key = task_key(&task.id);
            if nodes
                .insert(key.clone(), CanonicalNodeRecord::Task(task))
                .is_some()
            {
                return Err(anyhow!("duplicate canonical task `{}`", task.id.0));
            }
        }

        let mut children_by_plan = BTreeMap::new();
        let mut dependency_targets_by_source = BTreeMap::<NodeKey, Vec<NodeRef>>::new();
        let mut dependency_sources_by_target = BTreeMap::<NodeKey, Vec<NodeRef>>::new();
        let mut outgoing = BTreeMap::<NodeKey, Vec<NodeKey>>::new();
        let mut indegree = BTreeMap::<NodeKey, usize>::new();
        for key in nodes.keys() {
            outgoing.entry(key.clone()).or_default();
            indegree.entry(key.clone()).or_insert(0);
        }

        for plan in &snapshot.plans {
            let child_ref = NodeRef::plan(plan.id.clone());
            if let Some(parent_plan_id) = &plan.parent_plan_id {
                if parent_plan_id == &plan.id {
                    return Err(anyhow!(
                        "canonical plan `{}` cannot contain itself",
                        plan.id.0
                    ));
                }
                let parent_key = plan_key(parent_plan_id);
                if !nodes.contains_key(&parent_key) {
                    return Err(anyhow!(
                        "canonical plan `{}` references missing parent plan `{}`",
                        plan.id.0,
                        parent_plan_id.0
                    ));
                }
                add_edge(
                    &mut children_by_plan,
                    &mut outgoing,
                    &mut indegree,
                    &NodeRef::plan(parent_plan_id.clone()),
                    &child_ref,
                );
            }
        }

        for task in &snapshot.tasks {
            let parent_key = plan_key(&task.parent_plan_id);
            if !nodes.contains_key(&parent_key) {
                return Err(anyhow!(
                    "canonical task `{}` references missing parent plan `{}`",
                    task.id.0,
                    task.parent_plan_id.0
                ));
            }
            add_edge(
                &mut children_by_plan,
                &mut outgoing,
                &mut indegree,
                &NodeRef::plan(task.parent_plan_id.clone()),
                &NodeRef::task(task.id.clone()),
            );
        }

        let mut seen_dependencies = BTreeSet::new();
        for dependency in &snapshot.dependencies {
            let source_key = node_key(&dependency.source);
            let target_key = node_key(&dependency.target);
            if !nodes.contains_key(&source_key) {
                return Err(anyhow!(
                    "canonical dependency source `{}` ({:?}) does not exist",
                    dependency.source.id,
                    dependency.source.kind
                ));
            }
            if !nodes.contains_key(&target_key) {
                return Err(anyhow!(
                    "canonical dependency target `{}` ({:?}) does not exist",
                    dependency.target.id,
                    dependency.target.kind
                ));
            }
            if source_key == target_key {
                return Err(anyhow!(
                    "canonical dependency `{}` ({:?}) cannot target itself",
                    dependency.source.id,
                    dependency.source.kind
                ));
            }
            if !seen_dependencies.insert((source_key.clone(), target_key.clone())) {
                return Err(anyhow!(
                    "duplicate canonical dependency `{}` ({:?}) -> `{}` ({:?})",
                    dependency.source.id,
                    dependency.source.kind,
                    dependency.target.id,
                    dependency.target.kind
                ));
            }
            dependency_targets_by_source
                .entry(source_key.clone())
                .or_default()
                .push(dependency.target.clone());
            dependency_sources_by_target
                .entry(target_key.clone())
                .or_default()
                .push(dependency.source.clone());
            outgoing
                .entry(source_key)
                .or_default()
                .push(target_key.clone());
            *indegree.entry(target_key).or_insert(0) += 1;
        }

        sort_node_refs_by_key_map(&mut children_by_plan);
        sort_node_refs_by_key_map_with_node_keys(&mut dependency_targets_by_source);
        sort_node_refs_by_key_map_with_node_keys(&mut dependency_sources_by_target);
        sort_node_key_map(&mut outgoing);

        let topological_order = topological_order(&outgoing, &indegree)?;
        Ok(Self {
            nodes,
            children_by_plan,
            dependency_targets_by_source,
            dependency_sources_by_target,
            topological_order,
        })
    }

    pub fn node(&self, node_ref: &NodeRef) -> Option<CanonicalNodeRecord<'a>> {
        self.nodes.get(&node_key(node_ref)).copied()
    }

    pub fn children_of_plan(&self, plan_id: &PlanId) -> Vec<NodeRef> {
        self.children_by_plan
            .get(&plan_id.0.to_string())
            .cloned()
            .unwrap_or_default()
    }

    pub fn dependency_targets(&self, source: &NodeRef) -> Vec<NodeRef> {
        self.dependency_targets_by_source
            .get(&node_key(source))
            .cloned()
            .unwrap_or_default()
    }

    pub fn dependency_sources(&self, target: &NodeRef) -> Vec<NodeRef> {
        self.dependency_sources_by_target
            .get(&node_key(target))
            .cloned()
            .unwrap_or_default()
    }

    pub fn topological_order(&self) -> &[NodeRef] {
        &self.topological_order
    }
}

fn add_edge(
    children_by_plan: &mut BTreeMap<String, Vec<NodeRef>>,
    outgoing: &mut BTreeMap<NodeKey, Vec<NodeKey>>,
    indegree: &mut BTreeMap<NodeKey, usize>,
    source: &NodeRef,
    target: &NodeRef,
) {
    children_by_plan
        .entry(source.id.clone())
        .or_default()
        .push(target.clone());
    outgoing
        .entry(node_key(source))
        .or_default()
        .push(node_key(target));
    *indegree.entry(node_key(target)).or_insert(0) += 1;
}

fn topological_order(
    outgoing: &BTreeMap<NodeKey, Vec<NodeKey>>,
    indegree: &BTreeMap<NodeKey, usize>,
) -> Result<Vec<NodeRef>> {
    let mut remaining = indegree.clone();
    let mut ready = remaining
        .iter()
        .filter_map(|(key, degree)| (*degree == 0).then_some(key.clone()))
        .collect::<BTreeSet<_>>();
    let mut order = Vec::with_capacity(remaining.len());
    while let Some(next) = ready.iter().next().cloned() {
        ready.remove(&next);
        order.push(node_ref_from_key(&next));
        if let Some(targets) = outgoing.get(&next) {
            for target in targets {
                let degree = remaining
                    .get_mut(target)
                    .expect("target must be initialized in indegree map");
                *degree -= 1;
                if *degree == 0 {
                    ready.insert(target.clone());
                }
            }
        }
    }
    if order.len() == remaining.len() {
        return Ok(order);
    }
    let cycle_nodes = remaining
        .into_iter()
        .filter_map(|(key, degree)| (degree > 0).then_some(node_ref_from_key(&key)))
        .map(|node| format!("{}:{:?}", node.id, node.kind))
        .collect::<Vec<_>>();
    Err(anyhow!(
        "canonical coordination graph contains a cycle across containment and dependencies: {}",
        cycle_nodes.join(", ")
    ))
}

fn sort_node_refs_by_key_map(map: &mut BTreeMap<String, Vec<NodeRef>>) {
    for values in map.values_mut() {
        values.sort_by(node_ref_cmp);
    }
}

fn sort_node_refs_by_key_map_with_node_keys(map: &mut BTreeMap<NodeKey, Vec<NodeRef>>) {
    for values in map.values_mut() {
        values.sort_by(node_ref_cmp);
    }
}

fn sort_node_key_map(map: &mut BTreeMap<NodeKey, Vec<NodeKey>>) {
    for values in map.values_mut() {
        values.sort();
    }
}

fn node_ref_cmp(left: &NodeRef, right: &NodeRef) -> std::cmp::Ordering {
    (left.kind, &left.id).cmp(&(right.kind, &right.id))
}

fn node_key(node_ref: &NodeRef) -> NodeKey {
    (node_ref.kind, node_ref.id.clone())
}

fn plan_key(plan_id: &PlanId) -> NodeKey {
    (NodeRefKind::Plan, plan_id.0.to_string())
}

fn task_key(task_id: &TaskId) -> NodeKey {
    (NodeRefKind::Task, task_id.0.to_string())
}

fn node_ref_from_key(key: &NodeKey) -> NodeRef {
    match key.0 {
        NodeRefKind::Plan => NodeRef::plan(PlanId::new(key.1.clone())),
        NodeRefKind::Task => NodeRef::task(TaskId::new(key.1.clone())),
    }
}

#[cfg(test)]
mod tests {
    use prism_ir::{
        AgentId, ExecutorClass, PlanBinding, PlanKind, PlanOperatorState, PlanScope,
        TaskExecutorPolicy, TaskLifecycleStatus, ValidationRef, WorkspaceRevision,
    };
    use serde_json::Value;

    use crate::git_execution::TaskGitExecution;
    use crate::types::{AcceptanceCriterion, CoordinationPolicy, LeaseHolder, PlanScheduling};

    use super::*;
    use crate::canonical_graph::CoordinationDependencyRecord;

    fn plan(id: &str, parent_plan_id: Option<&str>) -> CanonicalPlanRecord {
        CanonicalPlanRecord {
            id: PlanId::new(id),
            parent_plan_id: parent_plan_id.map(PlanId::new),
            title: id.to_string(),
            goal: "goal".to_string(),
            scope: PlanScope::Repo,
            kind: PlanKind::TaskExecution,
            policy: CoordinationPolicy::default(),
            scheduling: PlanScheduling::default(),
            tags: Vec::new(),
            created_from: None,
            spec_refs: Vec::new(),
            metadata: Value::Null,
            operator_state: PlanOperatorState::None,
        }
    }

    fn task(id: &str, parent_plan_id: &str) -> CanonicalTaskRecord {
        CanonicalTaskRecord {
            id: TaskId::new(id),
            parent_plan_id: PlanId::new(parent_plan_id),
            title: id.to_string(),
            summary: None,
            lifecycle_status: TaskLifecycleStatus::Pending,
            estimated_minutes: 0,
            executor: TaskExecutorPolicy {
                executor_class: ExecutorClass::WorktreeExecutor,
                target_label: None,
                allowed_principals: Vec::new(),
            },
            assignee: Some(AgentId::new("agent:test")),
            pending_handoff_to: None,
            session: None,
            lease_holder: None::<LeaseHolder>,
            lease_started_at: None,
            lease_refreshed_at: None,
            lease_stale_at: None,
            lease_expires_at: None,
            worktree_id: None,
            branch_ref: None,
            anchors: Vec::new(),
            bindings: PlanBinding::default(),
            acceptance: Vec::<AcceptanceCriterion>::new(),
            validation_refs: Vec::<ValidationRef>::new(),
            base_revision: WorkspaceRevision::default(),
            priority: None,
            tags: Vec::new(),
            spec_refs: Vec::new(),
            metadata: Value::Null,
            git_execution: TaskGitExecution::default(),
        }
    }

    #[test]
    fn graph_indexes_containment_and_dependencies() {
        let snapshot = CoordinationSnapshotV2 {
            plans: vec![
                plan("plan:root", None),
                plan("plan:child", Some("plan:root")),
            ],
            tasks: vec![
                task("task:root", "plan:root"),
                task("task:child", "plan:child"),
            ],
            dependencies: vec![CoordinationDependencyRecord {
                source: NodeRef::task(TaskId::new("task:child")),
                target: NodeRef::task(TaskId::new("task:root")),
            }],
            ..CoordinationSnapshotV2::default()
        };

        let graph = CanonicalCoordinationGraph::new(&snapshot).expect("graph should validate");
        assert!(matches!(
            graph.node(&NodeRef::plan(PlanId::new("plan:child"))),
            Some(CanonicalNodeRecord::Plan(_))
        ));
        assert_eq!(
            graph.children_of_plan(&PlanId::new("plan:root")),
            vec![
                NodeRef::plan(PlanId::new("plan:child")),
                NodeRef::task(TaskId::new("task:root")),
            ]
        );
        assert_eq!(
            graph.children_of_plan(&PlanId::new("plan:child")),
            vec![NodeRef::task(TaskId::new("task:child"))]
        );
        assert_eq!(
            graph.dependency_targets(&NodeRef::task(TaskId::new("task:child"))),
            vec![NodeRef::task(TaskId::new("task:root"))]
        );
        assert_eq!(
            graph.dependency_sources(&NodeRef::task(TaskId::new("task:root"))),
            vec![NodeRef::task(TaskId::new("task:child"))]
        );

        let order = graph.topological_order();
        let root_idx = order
            .iter()
            .position(|node| *node == NodeRef::plan(PlanId::new("plan:root")))
            .unwrap();
        let child_idx = order
            .iter()
            .position(|node| *node == NodeRef::plan(PlanId::new("plan:child")))
            .unwrap();
        let task_root_idx = order
            .iter()
            .position(|node| *node == NodeRef::task(TaskId::new("task:root")))
            .unwrap();
        let task_child_idx = order
            .iter()
            .position(|node| *node == NodeRef::task(TaskId::new("task:child")))
            .unwrap();
        assert!(root_idx < child_idx);
        assert!(root_idx < task_root_idx);
        assert!(child_idx < task_child_idx);
    }

    #[test]
    fn graph_rejects_duplicate_dependency_edges() {
        let snapshot = CoordinationSnapshotV2 {
            plans: vec![plan("plan:root", None)],
            tasks: vec![task("task:a", "plan:root"), task("task:b", "plan:root")],
            dependencies: vec![
                CoordinationDependencyRecord {
                    source: NodeRef::task(TaskId::new("task:a")),
                    target: NodeRef::task(TaskId::new("task:b")),
                },
                CoordinationDependencyRecord {
                    source: NodeRef::task(TaskId::new("task:a")),
                    target: NodeRef::task(TaskId::new("task:b")),
                },
            ],
            ..CoordinationSnapshotV2::default()
        };

        let error = CanonicalCoordinationGraph::new(&snapshot).unwrap_err();
        assert!(error.to_string().contains("duplicate canonical dependency"));
    }

    #[test]
    fn graph_rejects_self_dependency() {
        let snapshot = CoordinationSnapshotV2 {
            plans: vec![plan("plan:root", None)],
            tasks: vec![task("task:a", "plan:root")],
            dependencies: vec![CoordinationDependencyRecord {
                source: NodeRef::task(TaskId::new("task:a")),
                target: NodeRef::task(TaskId::new("task:a")),
            }],
            ..CoordinationSnapshotV2::default()
        };

        let error = CanonicalCoordinationGraph::new(&snapshot).unwrap_err();
        assert!(error.to_string().contains("cannot target itself"));
    }

    #[test]
    fn graph_rejects_missing_parent_plan() {
        let snapshot = CoordinationSnapshotV2 {
            tasks: vec![task("task:orphan", "plan:missing")],
            ..CoordinationSnapshotV2::default()
        };

        let error = CanonicalCoordinationGraph::new(&snapshot).unwrap_err();
        assert!(error.to_string().contains("missing parent plan"));
    }

    #[test]
    fn graph_rejects_containment_cycle() {
        let snapshot = CoordinationSnapshotV2 {
            plans: vec![
                plan("plan:a", Some("plan:b")),
                plan("plan:b", Some("plan:a")),
            ],
            ..CoordinationSnapshotV2::default()
        };

        let error = CanonicalCoordinationGraph::new(&snapshot).unwrap_err();
        assert!(error.to_string().contains("contains a cycle"));
    }

    #[test]
    fn graph_rejects_mixed_containment_dependency_cycle() {
        let snapshot = CoordinationSnapshotV2 {
            plans: vec![
                plan("plan:root", None),
                plan("plan:child", Some("plan:root")),
            ],
            tasks: vec![task("task:child", "plan:child")],
            dependencies: vec![CoordinationDependencyRecord {
                source: NodeRef::task(TaskId::new("task:child")),
                target: NodeRef::plan(PlanId::new("plan:root")),
            }],
            ..CoordinationSnapshotV2::default()
        };

        let error = CanonicalCoordinationGraph::new(&snapshot).unwrap_err();
        assert!(error.to_string().contains("contains a cycle"));
    }
}
