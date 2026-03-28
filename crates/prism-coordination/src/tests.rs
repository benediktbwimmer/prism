use prism_ir::{EventActor, EventMeta};

use super::*;

fn meta(id: &str, ts: u64) -> EventMeta {
    EventMeta {
        id: prism_ir::EventId::new(id),
        ts,
        actor: EventActor::Agent,
        correlation: None,
        causation: None,
    }
}

#[test]
fn claim_conflicts_block_hard_exclusive_overlap() {
    let store = CoordinationStore::new();
    let (plan_id, _) = store
        .create_plan(
            meta("event:1", 1),
            PlanCreateInput {
                goal: "Ship coordination".to_string(),
                status: None,
                policy: None,
            },
        )
        .unwrap();
    let (task_id, task) = store
        .create_task(
            meta("event:2", 2),
            TaskCreateInput {
                plan_id,
                title: "Edit auth".to_string(),
                status: Some(prism_ir::CoordinationTaskStatus::Ready),
                assignee: None,
                session: Some(prism_ir::SessionId::new("session:a")),
                anchors: vec![prism_ir::AnchorRef::Kind(prism_ir::NodeKind::Function)],
                depends_on: Vec::new(),
                acceptance: Vec::new(),
                base_revision: prism_ir::WorkspaceRevision {
                    graph_version: 1,
                    git_commit: None,
                },
            },
        )
        .unwrap();

    let first = store
        .acquire_claim(
            meta("event:3", 3),
            prism_ir::SessionId::new("session:a"),
            ClaimAcquireInput {
                task_id: Some(task_id.clone()),
                anchors: task.anchors.clone(),
                capability: prism_ir::Capability::Edit,
                mode: Some(prism_ir::ClaimMode::HardExclusive),
                ttl_seconds: Some(60),
                base_revision: prism_ir::WorkspaceRevision {
                    graph_version: 1,
                    git_commit: None,
                },
                current_revision: prism_ir::WorkspaceRevision {
                    graph_version: 1,
                    git_commit: None,
                },
                agent: None,
            },
        )
        .unwrap();
    assert!(first.0.is_some());

    let second = store
        .acquire_claim(
            meta("event:4", 4),
            prism_ir::SessionId::new("session:b"),
            ClaimAcquireInput {
                task_id: None,
                anchors: task.anchors.clone(),
                capability: prism_ir::Capability::Edit,
                mode: Some(prism_ir::ClaimMode::HardExclusive),
                ttl_seconds: Some(60),
                base_revision: prism_ir::WorkspaceRevision {
                    graph_version: 1,
                    git_commit: None,
                },
                current_revision: prism_ir::WorkspaceRevision {
                    graph_version: 1,
                    git_commit: None,
                },
                agent: None,
            },
        )
        .unwrap();
    assert!(second.0.is_none());
    assert_eq!(second.1[0].severity, prism_ir::ConflictSeverity::Block);
}

#[test]
fn review_policy_gates_completion_but_not_ready_work() {
    let store = CoordinationStore::new();
    let (plan_id, _) = store
        .create_plan(
            meta("event:1", 1),
            PlanCreateInput {
                goal: "Ship reviewed change".to_string(),
                status: None,
                policy: Some(CoordinationPolicy {
                    require_review_for_completion: true,
                    ..CoordinationPolicy::default()
                }),
            },
        )
        .unwrap();
    let (task_id, _) = store
        .create_task(
            meta("event:2", 2),
            TaskCreateInput {
                plan_id,
                title: "Edit main".to_string(),
                status: Some(prism_ir::CoordinationTaskStatus::Ready),
                assignee: None,
                session: Some(prism_ir::SessionId::new("session:a")),
                anchors: vec![prism_ir::AnchorRef::Kind(prism_ir::NodeKind::Function)],
                depends_on: Vec::new(),
                acceptance: Vec::new(),
                base_revision: prism_ir::WorkspaceRevision {
                    graph_version: 1,
                    git_commit: None,
                },
            },
        )
        .unwrap();

    assert_eq!(
        store
            .ready_tasks(
                &prism_ir::PlanId::new("plan:1"),
                prism_ir::WorkspaceRevision {
                    graph_version: 1,
                    git_commit: None,
                },
                2,
            )
            .len(),
        1
    );
    assert!(store
        .update_task(
            meta("event:3", 3),
            TaskUpdateInput {
                task_id: task_id.clone(),
                status: Some(prism_ir::CoordinationTaskStatus::Completed),
                assignee: None,
                session: None,
                title: None,
                anchors: None,
                base_revision: None,
                completion_context: Some(TaskCompletionContext::default()),
            },
            prism_ir::WorkspaceRevision {
                graph_version: 1,
                git_commit: None,
            },
            3,
        )
        .is_err());

    let (artifact_id, _) = store
        .propose_artifact(
            meta("event:4", 4),
            ArtifactProposeInput {
                task_id: task_id.clone(),
                anchors: vec![prism_ir::AnchorRef::Kind(prism_ir::NodeKind::Function)],
                diff_ref: Some("patch:1".to_string()),
                evidence: Vec::new(),
                base_revision: prism_ir::WorkspaceRevision {
                    graph_version: 1,
                    git_commit: None,
                },
                current_revision: prism_ir::WorkspaceRevision {
                    graph_version: 1,
                    git_commit: None,
                },
                required_validations: Vec::new(),
                validated_checks: Vec::new(),
                risk_score: None,
            },
        )
        .unwrap();
    store
        .review_artifact(
            meta("event:5", 5),
            ArtifactReviewInput {
                artifact_id,
                verdict: prism_ir::ReviewVerdict::Approved,
                summary: "looks good".to_string(),
                required_validations: Vec::new(),
                validated_checks: Vec::new(),
                risk_score: None,
            },
            prism_ir::WorkspaceRevision {
                graph_version: 1,
                git_commit: None,
            },
        )
        .unwrap();
    assert_eq!(
        store
            .update_task(
                meta("event:6", 6),
                TaskUpdateInput {
                    task_id,
                    status: Some(prism_ir::CoordinationTaskStatus::Completed),
                    assignee: None,
                    session: None,
                    title: None,
                    anchors: None,
                    base_revision: None,
                    completion_context: Some(TaskCompletionContext::default()),
                },
                prism_ir::WorkspaceRevision {
                    graph_version: 1,
                    git_commit: None,
                },
                6,
            )
            .unwrap()
            .status,
        prism_ir::CoordinationTaskStatus::Completed
    );
}

#[test]
fn edit_capacity_limit_blocks_extra_claims() {
    let store = CoordinationStore::new();
    let (plan_id, _) = store
        .create_plan(
            meta("event:1", 1),
            PlanCreateInput {
                goal: "Serialize edits".to_string(),
                status: None,
                policy: Some(CoordinationPolicy {
                    max_parallel_editors_per_anchor: 1,
                    ..CoordinationPolicy::default()
                }),
            },
        )
        .unwrap();
    let (task_id, task) = store
        .create_task(
            meta("event:2", 2),
            TaskCreateInput {
                plan_id,
                title: "Edit main".to_string(),
                status: Some(prism_ir::CoordinationTaskStatus::Proposed),
                assignee: None,
                session: Some(prism_ir::SessionId::new("session:a")),
                anchors: vec![prism_ir::AnchorRef::Kind(prism_ir::NodeKind::Function)],
                depends_on: Vec::new(),
                acceptance: Vec::new(),
                base_revision: prism_ir::WorkspaceRevision {
                    graph_version: 1,
                    git_commit: None,
                },
            },
        )
        .unwrap();
    assert!(store
        .acquire_claim(
            meta("event:3", 3),
            prism_ir::SessionId::new("session:a"),
            ClaimAcquireInput {
                task_id: Some(task_id),
                anchors: task.anchors.clone(),
                capability: prism_ir::Capability::Edit,
                mode: Some(prism_ir::ClaimMode::SoftExclusive),
                ttl_seconds: Some(60),
                base_revision: prism_ir::WorkspaceRevision {
                    graph_version: 1,
                    git_commit: None,
                },
                current_revision: prism_ir::WorkspaceRevision {
                    graph_version: 1,
                    git_commit: None,
                },
                agent: None,
            },
        )
        .unwrap()
        .0
        .is_some());

    let blocked = store
        .acquire_claim(
            meta("event:4", 4),
            prism_ir::SessionId::new("session:b"),
            ClaimAcquireInput {
                task_id: None,
                anchors: task.anchors.clone(),
                capability: prism_ir::Capability::Edit,
                mode: Some(prism_ir::ClaimMode::SoftExclusive),
                ttl_seconds: Some(60),
                base_revision: prism_ir::WorkspaceRevision {
                    graph_version: 1,
                    git_commit: None,
                },
                current_revision: prism_ir::WorkspaceRevision {
                    graph_version: 1,
                    git_commit: None,
                },
                agent: None,
            },
        )
        .unwrap();
    assert!(blocked.0.is_none());
    assert!(blocked
        .1
        .iter()
        .any(|conflict| conflict.severity == prism_ir::ConflictSeverity::Block));
}

#[test]
fn approving_stale_artifact_is_rejected() {
    let store = CoordinationStore::new();
    let (plan_id, _) = store
        .create_plan(
            meta("event:1", 1),
            PlanCreateInput {
                goal: "Catch stale approvals".to_string(),
                status: None,
                policy: Some(CoordinationPolicy {
                    stale_after_graph_change: true,
                    ..CoordinationPolicy::default()
                }),
            },
        )
        .unwrap();
    let (task_id, _) = store
        .create_task(
            meta("event:2", 2),
            TaskCreateInput {
                plan_id,
                title: "Edit main".to_string(),
                status: Some(prism_ir::CoordinationTaskStatus::Proposed),
                assignee: None,
                session: Some(prism_ir::SessionId::new("session:a")),
                anchors: vec![prism_ir::AnchorRef::Kind(prism_ir::NodeKind::Function)],
                depends_on: Vec::new(),
                acceptance: Vec::new(),
                base_revision: prism_ir::WorkspaceRevision {
                    graph_version: 1,
                    git_commit: None,
                },
            },
        )
        .unwrap();
    let (artifact_id, _) = store
        .propose_artifact(
            meta("event:3", 3),
            ArtifactProposeInput {
                task_id,
                anchors: vec![prism_ir::AnchorRef::Kind(prism_ir::NodeKind::Function)],
                diff_ref: Some("patch:1".to_string()),
                evidence: Vec::new(),
                base_revision: prism_ir::WorkspaceRevision {
                    graph_version: 1,
                    git_commit: None,
                },
                current_revision: prism_ir::WorkspaceRevision {
                    graph_version: 1,
                    git_commit: None,
                },
                required_validations: Vec::new(),
                validated_checks: Vec::new(),
                risk_score: None,
            },
        )
        .unwrap();

    assert!(store
        .review_artifact(
            meta("event:4", 4),
            ArtifactReviewInput {
                artifact_id,
                verdict: prism_ir::ReviewVerdict::Approved,
                summary: "approve stale patch".to_string(),
                required_validations: Vec::new(),
                validated_checks: Vec::new(),
                risk_score: None,
            },
            prism_ir::WorkspaceRevision {
                graph_version: 2,
                git_commit: None,
            },
        )
        .is_err());
}

#[test]
fn validation_policy_requires_approved_artifact_checks() {
    let store = CoordinationStore::new();
    let (plan_id, _) = store
        .create_plan(
            meta("event:1", 1),
            PlanCreateInput {
                goal: "Validate risky change".to_string(),
                status: None,
                policy: Some(CoordinationPolicy {
                    require_validation_for_completion: true,
                    ..CoordinationPolicy::default()
                }),
            },
        )
        .unwrap();
    let (task_id, _) = store
        .create_task(
            meta("event:2", 2),
            TaskCreateInput {
                plan_id,
                title: "Edit main".to_string(),
                status: Some(prism_ir::CoordinationTaskStatus::Ready),
                assignee: None,
                session: Some(prism_ir::SessionId::new("session:a")),
                anchors: vec![prism_ir::AnchorRef::Kind(prism_ir::NodeKind::Function)],
                depends_on: Vec::new(),
                acceptance: Vec::new(),
                base_revision: prism_ir::WorkspaceRevision {
                    graph_version: 1,
                    git_commit: None,
                },
            },
        )
        .unwrap();
    let (artifact_id, _) = store
        .propose_artifact(
            meta("event:3", 3),
            ArtifactProposeInput {
                task_id: task_id.clone(),
                anchors: vec![prism_ir::AnchorRef::Kind(prism_ir::NodeKind::Function)],
                diff_ref: Some("patch:1".to_string()),
                evidence: Vec::new(),
                base_revision: prism_ir::WorkspaceRevision {
                    graph_version: 1,
                    git_commit: None,
                },
                current_revision: prism_ir::WorkspaceRevision {
                    graph_version: 1,
                    git_commit: None,
                },
                required_validations: vec!["test:main_integration".to_string()],
                validated_checks: Vec::new(),
                risk_score: Some(0.4),
            },
        )
        .unwrap();

    assert!(store
        .review_artifact(
            meta("event:4", 4),
            ArtifactReviewInput {
                artifact_id: artifact_id.clone(),
                verdict: prism_ir::ReviewVerdict::Approved,
                summary: "missing validation".to_string(),
                required_validations: vec!["test:main_integration".to_string()],
                validated_checks: Vec::new(),
                risk_score: Some(0.4),
            },
            prism_ir::WorkspaceRevision {
                graph_version: 1,
                git_commit: None,
            },
        )
        .is_err());

    store
        .review_artifact(
            meta("event:5", 5),
            ArtifactReviewInput {
                artifact_id,
                verdict: prism_ir::ReviewVerdict::Approved,
                summary: "validated".to_string(),
                required_validations: vec!["test:main_integration".to_string()],
                validated_checks: vec!["test:main_integration".to_string()],
                risk_score: Some(0.4),
            },
            prism_ir::WorkspaceRevision {
                graph_version: 1,
                git_commit: None,
            },
        )
        .unwrap();

    assert_eq!(
        store
            .update_task(
                meta("event:6", 6),
                TaskUpdateInput {
                    task_id,
                    status: Some(prism_ir::CoordinationTaskStatus::Completed),
                    assignee: None,
                    session: None,
                    title: None,
                    anchors: None,
                    base_revision: None,
                    completion_context: Some(TaskCompletionContext {
                        risk_score: Some(0.4),
                        required_validations: vec!["test:main_integration".to_string()],
                    }),
                },
                prism_ir::WorkspaceRevision {
                    graph_version: 1,
                    git_commit: None,
                },
                6,
            )
            .unwrap()
            .status,
        prism_ir::CoordinationTaskStatus::Completed
    );
}

#[test]
fn risk_threshold_requires_review_before_completion() {
    let store = CoordinationStore::new();
    let (plan_id, _) = store
        .create_plan(
            meta("event:1", 1),
            PlanCreateInput {
                goal: "Risky edit".to_string(),
                status: None,
                policy: Some(CoordinationPolicy {
                    review_required_above_risk_score: Some(0.5),
                    ..CoordinationPolicy::default()
                }),
            },
        )
        .unwrap();
    let (task_id, _) = store
        .create_task(
            meta("event:2", 2),
            TaskCreateInput {
                plan_id,
                title: "Edit main".to_string(),
                status: Some(prism_ir::CoordinationTaskStatus::Ready),
                assignee: None,
                session: Some(prism_ir::SessionId::new("session:a")),
                anchors: vec![prism_ir::AnchorRef::Kind(prism_ir::NodeKind::Function)],
                depends_on: Vec::new(),
                acceptance: Vec::new(),
                base_revision: prism_ir::WorkspaceRevision {
                    graph_version: 1,
                    git_commit: None,
                },
            },
        )
        .unwrap();

    assert!(store
        .update_task(
            meta("event:3", 3),
            TaskUpdateInput {
                task_id: task_id.clone(),
                status: Some(prism_ir::CoordinationTaskStatus::Completed),
                assignee: None,
                session: None,
                title: None,
                anchors: None,
                base_revision: None,
                completion_context: Some(TaskCompletionContext {
                    risk_score: Some(0.8),
                    required_validations: Vec::new(),
                }),
            },
            prism_ir::WorkspaceRevision {
                graph_version: 1,
                git_commit: None,
            },
            3,
        )
        .is_err());
}

#[test]
fn plan_graph_compat_preserves_task_ids_and_dependency_edges() {
    let store = CoordinationStore::new();
    let (plan_id, _) = store
        .create_plan(
            meta("event:1", 1),
            PlanCreateInput {
                goal: "Project coordination into a plan graph".to_string(),
                status: None,
                policy: None,
            },
        )
        .unwrap();
    let (dep_id, _) = store
        .create_task(
            meta("event:2", 2),
            TaskCreateInput {
                plan_id: plan_id.clone(),
                title: "Investigate".to_string(),
                status: Some(prism_ir::CoordinationTaskStatus::Ready),
                assignee: None,
                session: None,
                anchors: vec![prism_ir::AnchorRef::Kind(prism_ir::NodeKind::Function)],
                depends_on: Vec::new(),
                acceptance: Vec::new(),
                base_revision: prism_ir::WorkspaceRevision {
                    graph_version: 1,
                    git_commit: None,
                },
            },
        )
        .unwrap();
    let (task_id, _) = store
        .create_task(
            meta("event:3", 3),
            TaskCreateInput {
                plan_id: plan_id.clone(),
                title: "Edit".to_string(),
                status: Some(prism_ir::CoordinationTaskStatus::InProgress),
                assignee: None,
                session: None,
                anchors: vec![prism_ir::AnchorRef::Kind(prism_ir::NodeKind::Function)],
                depends_on: vec![dep_id.clone()],
                acceptance: Vec::new(),
                base_revision: prism_ir::WorkspaceRevision {
                    graph_version: 1,
                    git_commit: None,
                },
            },
        )
        .unwrap();

    let graph = store.plan_graph(&plan_id).expect("plan graph should exist");
    assert_eq!(graph.id, plan_id);
    assert!(graph
        .root_nodes
        .iter()
        .any(|node_id| node_id.0.as_str() == dep_id.0.as_str()));
    assert!(graph.nodes.iter().any(|node| {
        node.id.0.as_str() == task_id.0.as_str()
            && node.status == prism_ir::PlanNodeStatus::InProgress
    }));
    assert!(graph.edges.iter().any(|edge| {
        edge.kind == prism_ir::PlanEdgeKind::DependsOn
            && edge.from.0.as_str() == task_id.0.as_str()
            && edge.to.0.as_str() == dep_id.0.as_str()
    }));
}

#[test]
fn plan_graph_execution_overlays_keep_runtime_state_outside_canonical_nodes() {
    let store = CoordinationStore::new();
    let (plan_id, _) = store
        .create_plan(
            meta("event:1", 1),
            PlanCreateInput {
                goal: "Separate runtime execution overlay".to_string(),
                status: None,
                policy: None,
            },
        )
        .unwrap();
    let (task_id, _) = store
        .create_task(
            meta("event:2", 2),
            TaskCreateInput {
                plan_id: plan_id.clone(),
                title: "Review".to_string(),
                status: Some(prism_ir::CoordinationTaskStatus::Blocked),
                assignee: Some(prism_ir::AgentId::new("agent:owner")),
                session: Some(prism_ir::SessionId::new("session:runtime")),
                anchors: Vec::new(),
                depends_on: Vec::new(),
                acceptance: Vec::new(),
                base_revision: prism_ir::WorkspaceRevision {
                    graph_version: 1,
                    git_commit: None,
                },
            },
        )
        .unwrap();
    store
        .handoff(
            meta("event:3", 3),
            HandoffInput {
                task_id: task_id.clone(),
                to_agent: Some(prism_ir::AgentId::new("agent:reviewer")),
                summary: "Please review".to_string(),
                base_revision: prism_ir::WorkspaceRevision {
                    graph_version: 1,
                    git_commit: None,
                },
            },
            prism_ir::WorkspaceRevision {
                graph_version: 1,
                git_commit: None,
            },
        )
        .unwrap();

    let graph = store.plan_graph(&plan_id).expect("plan graph should exist");
    let node = graph
        .nodes
        .iter()
        .find(|node| node.id.0.as_str() == task_id.0.as_str())
        .expect("compat graph should include node");
    assert_eq!(node.assignee, Some(prism_ir::AgentId::new("agent:owner")));

    let overlays = store.plan_execution_overlays(&plan_id);
    assert!(overlays.iter().any(|overlay| {
        overlay.node_id.0.as_str() == task_id.0.as_str()
            && overlay.pending_handoff_to == Some(prism_ir::AgentId::new("agent:reviewer"))
            && overlay.session == Some(prism_ir::SessionId::new("session:runtime"))
    }));
}

#[test]
fn invalid_task_transition_is_rejected() {
    let store = CoordinationStore::new();
    let (plan_id, _) = store
        .create_plan(
            meta("event:1", 1),
            PlanCreateInput {
                goal: "Enforce task lifecycle".to_string(),
                status: None,
                policy: None,
            },
        )
        .unwrap();
    let (task_id, _) = store
        .create_task(
            meta("event:2", 2),
            TaskCreateInput {
                plan_id,
                title: "Edit main".to_string(),
                status: Some(prism_ir::CoordinationTaskStatus::Proposed),
                assignee: None,
                session: Some(prism_ir::SessionId::new("session:a")),
                anchors: vec![prism_ir::AnchorRef::Kind(prism_ir::NodeKind::Function)],
                depends_on: Vec::new(),
                acceptance: Vec::new(),
                base_revision: prism_ir::WorkspaceRevision {
                    graph_version: 1,
                    git_commit: None,
                },
            },
        )
        .unwrap();

    let error = store
        .update_task(
            meta("event:3", 3),
            TaskUpdateInput {
                task_id,
                status: Some(prism_ir::CoordinationTaskStatus::Completed),
                assignee: None,
                session: None,
                title: None,
                anchors: None,
                base_revision: Some(prism_ir::WorkspaceRevision {
                    graph_version: 1,
                    git_commit: None,
                }),
                completion_context: Some(TaskCompletionContext::default()),
            },
            prism_ir::WorkspaceRevision {
                graph_version: 1,
                git_commit: None,
            },
            3,
        )
        .unwrap_err();
    assert!(error
        .to_string()
        .contains("invalid coordination task transition"));
}

#[test]
fn stale_claim_and_artifact_mutations_are_rejected() {
    let store = CoordinationStore::new();
    let (plan_id, _) = store
        .create_plan(
            meta("event:1", 1),
            PlanCreateInput {
                goal: "Reject stale writes".to_string(),
                status: None,
                policy: Some(CoordinationPolicy {
                    stale_after_graph_change: true,
                    ..CoordinationPolicy::default()
                }),
            },
        )
        .unwrap();
    let (task_id, task) = store
        .create_task(
            meta("event:2", 2),
            TaskCreateInput {
                plan_id,
                title: "Edit main".to_string(),
                status: Some(prism_ir::CoordinationTaskStatus::Ready),
                assignee: None,
                session: Some(prism_ir::SessionId::new("session:a")),
                anchors: vec![prism_ir::AnchorRef::Kind(prism_ir::NodeKind::Function)],
                depends_on: Vec::new(),
                acceptance: Vec::new(),
                base_revision: prism_ir::WorkspaceRevision {
                    graph_version: 1,
                    git_commit: None,
                },
            },
        )
        .unwrap();

    let claim_error = store
        .acquire_claim(
            meta("event:3", 3),
            prism_ir::SessionId::new("session:a"),
            ClaimAcquireInput {
                task_id: Some(task_id.clone()),
                anchors: task.anchors.clone(),
                capability: prism_ir::Capability::Edit,
                mode: Some(prism_ir::ClaimMode::SoftExclusive),
                ttl_seconds: Some(60),
                base_revision: prism_ir::WorkspaceRevision {
                    graph_version: 1,
                    git_commit: None,
                },
                current_revision: prism_ir::WorkspaceRevision {
                    graph_version: 2,
                    git_commit: None,
                },
                agent: None,
            },
        )
        .unwrap_err();
    assert!(claim_error
        .to_string()
        .contains("claim acquisition cannot use stale base revision"));

    let artifact_error = store
        .propose_artifact(
            meta("event:4", 4),
            ArtifactProposeInput {
                task_id,
                anchors: task.anchors,
                diff_ref: Some("patch:1".to_string()),
                evidence: Vec::new(),
                base_revision: prism_ir::WorkspaceRevision {
                    graph_version: 1,
                    git_commit: None,
                },
                current_revision: prism_ir::WorkspaceRevision {
                    graph_version: 2,
                    git_commit: None,
                },
                required_validations: Vec::new(),
                validated_checks: Vec::new(),
                risk_score: None,
            },
        )
        .unwrap_err();
    assert!(artifact_error
        .to_string()
        .contains("artifact proposal for task"));
}

#[test]
fn plan_completion_requires_terminal_tasks_and_no_active_claims() {
    let store = CoordinationStore::new();
    let (plan_id, _) = store
        .create_plan(
            meta("event:1", 1),
            PlanCreateInput {
                goal: "Close coordinated work".to_string(),
                status: None,
                policy: None,
            },
        )
        .unwrap();
    let (task_id, task) = store
        .create_task(
            meta("event:2", 2),
            TaskCreateInput {
                plan_id: plan_id.clone(),
                title: "Finish implementation".to_string(),
                status: Some(prism_ir::CoordinationTaskStatus::Ready),
                assignee: None,
                session: Some(prism_ir::SessionId::new("session:a")),
                anchors: vec![prism_ir::AnchorRef::Kind(prism_ir::NodeKind::Function)],
                depends_on: Vec::new(),
                acceptance: Vec::new(),
                base_revision: prism_ir::WorkspaceRevision {
                    graph_version: 1,
                    git_commit: None,
                },
            },
        )
        .unwrap();
    let (claim_id, _, _) = store
        .acquire_claim(
            meta("event:3", 3),
            prism_ir::SessionId::new("session:a"),
            ClaimAcquireInput {
                task_id: Some(task_id.clone()),
                anchors: task.anchors.clone(),
                capability: prism_ir::Capability::Edit,
                mode: Some(prism_ir::ClaimMode::SoftExclusive),
                ttl_seconds: Some(60),
                base_revision: prism_ir::WorkspaceRevision {
                    graph_version: 1,
                    git_commit: None,
                },
                current_revision: prism_ir::WorkspaceRevision {
                    graph_version: 1,
                    git_commit: None,
                },
                agent: None,
            },
        )
        .unwrap();

    let error = store
        .update_plan(
            meta("event:4", 4),
            PlanUpdateInput {
                plan_id: plan_id.clone(),
                status: Some(prism_ir::PlanStatus::Completed),
                goal: None,
                policy: None,
            },
        )
        .unwrap_err();
    assert!(error
        .to_string()
        .contains("coordination plan cannot be completed"));

    let events = store.events();
    let rejection = events.last().unwrap();
    assert_eq!(
        rejection.kind,
        prism_ir::CoordinationEventKind::MutationRejected
    );
    let codes = rejection.metadata["violations"]
        .as_array()
        .unwrap()
        .iter()
        .filter_map(|value| value["code"].as_str())
        .collect::<Vec<_>>();
    assert!(codes.contains(&"incomplete_plan_tasks"));
    assert!(codes.contains(&"active_plan_claims"));

    store
        .release_claim(
            meta("event:5", 5),
            &prism_ir::SessionId::new("session:a"),
            &claim_id.unwrap(),
        )
        .unwrap();
    store
        .update_task(
            meta("event:6", 6),
            TaskUpdateInput {
                task_id,
                status: Some(prism_ir::CoordinationTaskStatus::Completed),
                assignee: None,
                session: None,
                title: None,
                anchors: None,
                base_revision: Some(prism_ir::WorkspaceRevision {
                    graph_version: 1,
                    git_commit: None,
                }),
                completion_context: Some(TaskCompletionContext::default()),
            },
            prism_ir::WorkspaceRevision {
                graph_version: 1,
                git_commit: None,
            },
            6,
        )
        .unwrap();
    let plan = store
        .update_plan(
            meta("event:7", 7),
            PlanUpdateInput {
                plan_id,
                status: Some(prism_ir::PlanStatus::Completed),
                goal: None,
                policy: None,
            },
        )
        .unwrap();
    assert_eq!(plan.status, prism_ir::PlanStatus::Completed);
}

#[test]
fn closed_plan_rejects_new_task_and_records_violation() {
    let store = CoordinationStore::new();
    let (plan_id, _) = store
        .create_plan(
            meta("event:1", 1),
            PlanCreateInput {
                goal: "Archive repo work".to_string(),
                status: None,
                policy: None,
            },
        )
        .unwrap();
    store
        .update_plan(
            meta("event:2", 2),
            PlanUpdateInput {
                plan_id: plan_id.clone(),
                status: Some(prism_ir::PlanStatus::Abandoned),
                goal: None,
                policy: None,
            },
        )
        .unwrap();

    let error = store
        .create_task(
            meta("event:3", 3),
            TaskCreateInput {
                plan_id: plan_id.clone(),
                title: "Should not exist".to_string(),
                status: None,
                assignee: None,
                session: Some(prism_ir::SessionId::new("session:a")),
                anchors: vec![prism_ir::AnchorRef::Kind(prism_ir::NodeKind::Function)],
                depends_on: Vec::new(),
                acceptance: Vec::new(),
                base_revision: prism_ir::WorkspaceRevision {
                    graph_version: 1,
                    git_commit: None,
                },
            },
        )
        .unwrap_err();
    assert!(error
        .to_string()
        .contains("coordination task creation rejected"));

    let rejection = store.events().last().unwrap().clone();
    assert_eq!(
        rejection.kind,
        prism_ir::CoordinationEventKind::MutationRejected
    );
    assert_eq!(rejection.metadata["violations"][0]["code"], "plan_closed");
}

#[test]
fn draft_plan_hides_ready_work_until_activation() {
    let store = CoordinationStore::new();
    let (plan_id, plan) = store
        .create_plan(
            meta("event:1", 1),
            PlanCreateInput {
                goal: "Stage a coordinated rollout".to_string(),
                status: Some(prism_ir::PlanStatus::Draft),
                policy: None,
            },
        )
        .unwrap();
    assert_eq!(plan.status, prism_ir::PlanStatus::Draft);

    store
        .create_task(
            meta("event:2", 2),
            TaskCreateInput {
                plan_id: plan_id.clone(),
                title: "Prepare alpha".to_string(),
                status: Some(prism_ir::CoordinationTaskStatus::Ready),
                assignee: None,
                session: Some(prism_ir::SessionId::new("session:a")),
                anchors: vec![prism_ir::AnchorRef::Kind(prism_ir::NodeKind::Function)],
                depends_on: Vec::new(),
                acceptance: Vec::new(),
                base_revision: prism_ir::WorkspaceRevision {
                    graph_version: 1,
                    git_commit: None,
                },
            },
        )
        .unwrap();

    assert!(store
        .ready_tasks(
            &plan_id,
            prism_ir::WorkspaceRevision {
                graph_version: 1,
                git_commit: None,
            },
            2,
        )
        .is_empty());

    store
        .update_plan(
            meta("event:3", 3),
            PlanUpdateInput {
                plan_id: plan_id.clone(),
                status: Some(prism_ir::PlanStatus::Active),
                goal: None,
                policy: None,
            },
        )
        .unwrap();

    assert_eq!(
        store
            .ready_tasks(
                &plan_id,
                prism_ir::WorkspaceRevision {
                    graph_version: 1,
                    git_commit: None,
                },
                3,
            )
            .len(),
        1
    );
}

#[test]
fn handoff_acceptance_blocks_updates_until_target_accepts() {
    let store = CoordinationStore::new();
    let (plan_id, _) = store
        .create_plan(
            meta("event:1", 1),
            PlanCreateInput {
                goal: "Transfer alpha safely".to_string(),
                status: None,
                policy: None,
            },
        )
        .unwrap();
    let (task_id, task) = store
        .create_task(
            meta("event:2", 2),
            TaskCreateInput {
                plan_id,
                title: "Edit alpha".to_string(),
                status: Some(prism_ir::CoordinationTaskStatus::Ready),
                assignee: Some(prism_ir::AgentId::new("agent-a")),
                session: Some(prism_ir::SessionId::new("session:a")),
                anchors: vec![prism_ir::AnchorRef::Kind(prism_ir::NodeKind::Function)],
                depends_on: Vec::new(),
                acceptance: Vec::new(),
                base_revision: prism_ir::WorkspaceRevision {
                    graph_version: 1,
                    git_commit: None,
                },
            },
        )
        .unwrap();

    let handed_off = store
        .handoff(
            meta("event:3", 3),
            HandoffInput {
                task_id: task_id.clone(),
                to_agent: Some(prism_ir::AgentId::new("agent-b")),
                summary: "handoff alpha to agent-b".to_string(),
                base_revision: prism_ir::WorkspaceRevision {
                    graph_version: 1,
                    git_commit: None,
                },
            },
            prism_ir::WorkspaceRevision {
                graph_version: 1,
                git_commit: None,
            },
        )
        .unwrap();
    assert_eq!(handed_off.status, prism_ir::CoordinationTaskStatus::Blocked);
    assert_eq!(
        handed_off.pending_handoff_to,
        Some(prism_ir::AgentId::new("agent-b"))
    );
    assert_eq!(handed_off.assignee, task.assignee);

    let blocked_update = store
        .update_task(
            meta("event:4", 4),
            TaskUpdateInput {
                task_id: task_id.clone(),
                status: Some(prism_ir::CoordinationTaskStatus::InProgress),
                assignee: None,
                session: None,
                title: None,
                anchors: None,
                base_revision: Some(prism_ir::WorkspaceRevision {
                    graph_version: 1,
                    git_commit: None,
                }),
                completion_context: None,
            },
            prism_ir::WorkspaceRevision {
                graph_version: 1,
                git_commit: None,
            },
            4,
        )
        .unwrap_err();
    assert!(blocked_update.to_string().contains("pending handoff"));

    let wrong_agent = store
        .accept_handoff(
            meta("event:5", 5),
            HandoffAcceptInput {
                task_id: task_id.clone(),
                agent: None,
            },
        )
        .unwrap_err();
    assert!(wrong_agent
        .to_string()
        .contains("requires an acting agent identity"));

    let wrong_agent = store
        .accept_handoff(
            meta("event:6", 6),
            HandoffAcceptInput {
                task_id: task_id.clone(),
                agent: Some(prism_ir::AgentId::new("agent-c")),
            },
        )
        .unwrap_err();
    assert!(wrong_agent.to_string().contains("cannot be accepted"));

    let accepted = store
        .accept_handoff(
            meta("event:7", 7),
            HandoffAcceptInput {
                task_id: task_id.clone(),
                agent: Some(prism_ir::AgentId::new("agent-b")),
            },
        )
        .unwrap();
    assert_eq!(accepted.status, prism_ir::CoordinationTaskStatus::Ready);
    assert_eq!(accepted.assignee, Some(prism_ir::AgentId::new("agent-b")));
    assert_eq!(accepted.pending_handoff_to, None);
    assert_eq!(accepted.session, None);
    assert_eq!(
        store.events().last().unwrap().kind,
        prism_ir::CoordinationEventKind::HandoffAccepted
    );
}

#[test]
fn overlap_kind_changes_conflict_severity() {
    let store = CoordinationStore::new();
    let file_warn = store
        .acquire_claim(
            meta("event:1", 1),
            prism_ir::SessionId::new("session:a"),
            ClaimAcquireInput {
                task_id: None,
                anchors: vec![prism_ir::AnchorRef::File(prism_ir::FileId(1))],
                capability: prism_ir::Capability::Edit,
                mode: Some(prism_ir::ClaimMode::SoftExclusive),
                ttl_seconds: Some(60),
                base_revision: prism_ir::WorkspaceRevision {
                    graph_version: 1,
                    git_commit: None,
                },
                current_revision: prism_ir::WorkspaceRevision {
                    graph_version: 1,
                    git_commit: None,
                },
                agent: None,
            },
        )
        .unwrap();
    assert!(file_warn.0.is_some());

    let file_conflict = store
        .acquire_claim(
            meta("event:2", 2),
            prism_ir::SessionId::new("session:b"),
            ClaimAcquireInput {
                task_id: None,
                anchors: vec![prism_ir::AnchorRef::File(prism_ir::FileId(1))],
                capability: prism_ir::Capability::Edit,
                mode: Some(prism_ir::ClaimMode::SoftExclusive),
                ttl_seconds: Some(60),
                base_revision: prism_ir::WorkspaceRevision {
                    graph_version: 1,
                    git_commit: None,
                },
                current_revision: prism_ir::WorkspaceRevision {
                    graph_version: 1,
                    git_commit: None,
                },
                agent: None,
            },
        )
        .unwrap();
    assert!(file_conflict.0.is_some());
    assert!(file_conflict
        .1
        .iter()
        .any(|conflict| conflict.severity == prism_ir::ConflictSeverity::Warn));

    let kind_conflict = store
        .acquire_claim(
            meta("event:3", 3),
            prism_ir::SessionId::new("session:c"),
            ClaimAcquireInput {
                task_id: None,
                anchors: vec![prism_ir::AnchorRef::Kind(prism_ir::NodeKind::Function)],
                capability: prism_ir::Capability::Edit,
                mode: Some(prism_ir::ClaimMode::Advisory),
                ttl_seconds: Some(60),
                base_revision: prism_ir::WorkspaceRevision {
                    graph_version: 1,
                    git_commit: None,
                },
                current_revision: prism_ir::WorkspaceRevision {
                    graph_version: 1,
                    git_commit: None,
                },
                agent: None,
            },
        )
        .unwrap();
    assert!(kind_conflict.0.is_some());

    let second_kind_conflict = store
        .acquire_claim(
            meta("event:4", 4),
            prism_ir::SessionId::new("session:d"),
            ClaimAcquireInput {
                task_id: None,
                anchors: vec![prism_ir::AnchorRef::Kind(prism_ir::NodeKind::Function)],
                capability: prism_ir::Capability::Edit,
                mode: Some(prism_ir::ClaimMode::Advisory),
                ttl_seconds: Some(60),
                base_revision: prism_ir::WorkspaceRevision {
                    graph_version: 1,
                    git_commit: None,
                },
                current_revision: prism_ir::WorkspaceRevision {
                    graph_version: 1,
                    git_commit: None,
                },
                agent: None,
            },
        )
        .unwrap();
    assert!(second_kind_conflict.0.is_some());
    assert!(second_kind_conflict
        .1
        .iter()
        .any(|conflict| conflict.severity == prism_ir::ConflictSeverity::Info));
}

#[test]
fn claim_ownership_is_enforced_and_audited() {
    let store = CoordinationStore::new();
    let (plan_id, _) = store
        .create_plan(
            meta("event:1", 1),
            PlanCreateInput {
                goal: "Protect claim ownership".to_string(),
                status: None,
                policy: None,
            },
        )
        .unwrap();
    let (task_id, task) = store
        .create_task(
            meta("event:2", 2),
            TaskCreateInput {
                plan_id: plan_id.clone(),
                title: "Edit alpha".to_string(),
                status: Some(prism_ir::CoordinationTaskStatus::Ready),
                assignee: None,
                session: Some(prism_ir::SessionId::new("session:a")),
                anchors: vec![prism_ir::AnchorRef::Kind(prism_ir::NodeKind::Function)],
                depends_on: Vec::new(),
                acceptance: Vec::new(),
                base_revision: prism_ir::WorkspaceRevision {
                    graph_version: 1,
                    git_commit: None,
                },
            },
        )
        .unwrap();
    let claim_id = store
        .acquire_claim(
            meta("event:3", 3),
            prism_ir::SessionId::new("session:a"),
            ClaimAcquireInput {
                task_id: Some(task_id.clone()),
                anchors: task.anchors.clone(),
                capability: prism_ir::Capability::Edit,
                mode: Some(prism_ir::ClaimMode::SoftExclusive),
                ttl_seconds: Some(60),
                base_revision: prism_ir::WorkspaceRevision {
                    graph_version: 1,
                    git_commit: None,
                },
                current_revision: prism_ir::WorkspaceRevision {
                    graph_version: 1,
                    git_commit: None,
                },
                agent: None,
            },
        )
        .unwrap()
        .0
        .unwrap();

    let renew_error = store
        .renew_claim(
            meta("event:4", 4),
            &prism_ir::SessionId::new("session:b"),
            &claim_id,
            Some(120),
        )
        .unwrap_err();
    assert!(renew_error.to_string().contains("cannot be renewed"));

    let release_error = store
        .release_claim(
            meta("event:5", 5),
            &prism_ir::SessionId::new("session:b"),
            &claim_id,
        )
        .unwrap_err();
    assert!(release_error.to_string().contains("cannot be released"));

    let violations = store.policy_violations(Some(&plan_id), Some(&task_id), 10);
    assert_eq!(violations.len(), 2);
    assert!(violations.iter().all(|record| {
        record
            .violations
            .iter()
            .any(|violation| violation.code == PolicyViolationCode::ClaimNotOwned)
    }));

    let released = store
        .release_claim(
            meta("event:6", 6),
            &prism_ir::SessionId::new("session:a"),
            &claim_id,
        )
        .unwrap();
    assert_eq!(released.status, prism_ir::ClaimStatus::Released);
}
