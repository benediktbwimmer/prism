use prism_ir::{EventActor, EventMeta};

use super::*;

fn meta(id: &str, ts: u64) -> EventMeta {
    EventMeta {
        id: prism_ir::EventId::new(id),
        ts,
        actor: EventActor::Agent,
        correlation: None,
        causation: None,
        execution_context: None,
    }
}

fn principal_meta(
    id: &str,
    ts: u64,
    authority: &str,
    principal: &str,
    session_id: &str,
) -> EventMeta {
    EventMeta {
        id: prism_ir::EventId::new(id),
        ts,
        actor: EventActor::Principal(prism_ir::PrincipalActor {
            authority_id: prism_ir::PrincipalAuthorityId::new(authority),
            principal_id: prism_ir::PrincipalId::new(principal),
            kind: Some(prism_ir::PrincipalKind::Agent),
            name: Some(principal.to_string()),
        }),
        correlation: None,
        causation: None,
        execution_context: Some(prism_ir::EventExecutionContext {
            repo_id: None,
            worktree_id: None,
            branch_ref: None,
            session_id: Some(session_id.to_string()),
            instance_id: None,
            request_id: None,
            credential_id: None,
            work_context: None,
        }),
    }
}

fn revision() -> prism_ir::WorkspaceRevision {
    prism_ir::WorkspaceRevision {
        graph_version: 1,
        git_commit: None,
    }
}

#[test]
fn claim_conflicts_block_hard_exclusive_overlap() {
    let store = CoordinationStore::new();
    let (plan_id, _) = store
        .create_plan(
            meta("event:1", 1),
            PlanCreateInput {
                title: "Ship coordination".to_string(),
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
                worktree_id: None,
                branch_ref: None,
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
                worktree_id: None,
                branch_ref: None,
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
                worktree_id: None,
                branch_ref: None,
            },
        )
        .unwrap();
    assert!(second.0.is_none());
    assert_eq!(second.1[0].severity, prism_ir::ConflictSeverity::Block);
}

#[test]
fn expired_task_requires_resume_for_same_principal() {
    let store = CoordinationStore::new();
    let (plan_id, _) = store
        .create_plan(
            principal_meta("event:plan", 1, "local", "agent:a", "session:a"),
            PlanCreateInput {
                title: "Resume expired task".to_string(),
                goal: "Resume expired task".to_string(),
                status: Some(prism_ir::PlanStatus::Active),
                policy: None,
            },
        )
        .unwrap();
    let (task_id, _) = store
        .create_task(
            principal_meta("event:task", 2, "local", "agent:a", "session:a"),
            TaskCreateInput {
                plan_id,
                title: "Continue work".to_string(),
                status: Some(prism_ir::CoordinationTaskStatus::InProgress),
                assignee: Some(prism_ir::AgentId::new("agent:a")),
                session: Some(prism_ir::SessionId::new("session:a")),
                worktree_id: None,
                branch_ref: None,
                anchors: Vec::new(),
                depends_on: Vec::new(),
                acceptance: Vec::new(),
                base_revision: revision(),
            },
        )
        .unwrap();

    let error = store
        .update_task(
            principal_meta("event:update", 8000, "local", "agent:a", "session:a"),
            TaskUpdateInput {
                task_id: task_id.clone(),
                kind: None,
                status: Some(prism_ir::CoordinationTaskStatus::Ready),
                assignee: None,
                session: None,
                worktree_id: None,
                branch_ref: None,
                title: None,
                summary: None,
                anchors: None,
                bindings: None,
                depends_on: None,
                acceptance: None,
                validation_refs: None,
                is_abstract: None,
                base_revision: None,
                priority: None,
                tags: None,
                completion_context: None,
            },
            revision(),
            8000,
        )
        .unwrap_err();
    assert!(error.to_string().contains("must be resumed"));

    let resumed = store
        .resume_task(
            principal_meta("event:resume", 8000, "local", "agent:a", "session:a"),
            TaskResumeInput {
                task_id: task_id.clone(),
                agent: Some(prism_ir::AgentId::new("agent:a")),
                worktree_id: Some("worktree:a".to_string()),
                branch_ref: Some("refs/heads/a".to_string()),
            },
        )
        .unwrap();
    assert_eq!(
        resumed.assignee.as_ref().map(|agent| agent.0.as_str()),
        Some("agent:a")
    );
    assert_eq!(resumed.worktree_id.as_deref(), Some("worktree:a"));

    let updated = store
        .update_task(
            principal_meta(
                "event:update:after-resume",
                8001,
                "local",
                "agent:a",
                "session:a",
            ),
            TaskUpdateInput {
                task_id,
                kind: None,
                status: Some(prism_ir::CoordinationTaskStatus::Ready),
                assignee: None,
                session: None,
                worktree_id: None,
                branch_ref: None,
                title: None,
                summary: None,
                anchors: None,
                bindings: None,
                depends_on: None,
                acceptance: None,
                validation_refs: None,
                is_abstract: None,
                base_revision: None,
                priority: None,
                tags: None,
                completion_context: None,
            },
            revision(),
            8001,
        )
        .unwrap();
    assert_eq!(updated.status, prism_ir::CoordinationTaskStatus::Ready);
}

#[test]
fn stale_task_requires_reclaim_for_different_principal() {
    let store = CoordinationStore::new();
    let (plan_id, _) = store
        .create_plan(
            principal_meta("event:plan", 1, "local", "agent:a", "session:a"),
            PlanCreateInput {
                title: "Reclaim stale task".to_string(),
                goal: "Reclaim stale task".to_string(),
                status: Some(prism_ir::PlanStatus::Active),
                policy: None,
            },
        )
        .unwrap();
    let (task_id, _) = store
        .create_task(
            principal_meta("event:task", 2, "local", "agent:a", "session:a"),
            TaskCreateInput {
                plan_id,
                title: "Take over later".to_string(),
                status: Some(prism_ir::CoordinationTaskStatus::InProgress),
                assignee: Some(prism_ir::AgentId::new("agent:a")),
                session: Some(prism_ir::SessionId::new("session:a")),
                worktree_id: None,
                branch_ref: None,
                anchors: Vec::new(),
                depends_on: Vec::new(),
                acceptance: Vec::new(),
                base_revision: revision(),
            },
        )
        .unwrap();

    let error = store
        .update_task(
            principal_meta("event:update", 1905, "local", "agent:b", "session:b"),
            TaskUpdateInput {
                task_id: task_id.clone(),
                kind: None,
                status: Some(prism_ir::CoordinationTaskStatus::Ready),
                assignee: None,
                session: None,
                worktree_id: None,
                branch_ref: None,
                title: None,
                summary: None,
                anchors: None,
                bindings: None,
                depends_on: None,
                acceptance: None,
                validation_refs: None,
                is_abstract: None,
                base_revision: None,
                priority: None,
                tags: None,
                completion_context: None,
            },
            revision(),
            1905,
        )
        .unwrap_err();
    assert!(error.to_string().contains("must be reclaimed"));

    let reclaimed = store
        .reclaim_task(
            principal_meta("event:reclaim", 1905, "local", "agent:b", "session:b"),
            TaskReclaimInput {
                task_id: task_id.clone(),
                agent: Some(prism_ir::AgentId::new("agent:b")),
                worktree_id: Some("worktree:b".to_string()),
                branch_ref: Some("refs/heads/b".to_string()),
            },
        )
        .unwrap();
    assert_eq!(
        reclaimed.assignee.as_ref().map(|agent| agent.0.as_str()),
        Some("agent:b")
    );
    assert_eq!(
        reclaimed.session.as_ref().map(|session| session.0.as_str()),
        Some("session:b")
    );
    assert_eq!(reclaimed.worktree_id.as_deref(), Some("worktree:b"));
}

#[test]
fn expired_claim_can_be_renewed_by_same_principal() {
    let store = CoordinationStore::new();
    let (plan_id, _) = store
        .create_plan(
            principal_meta("event:plan", 1, "local", "agent:a", "session:a"),
            PlanCreateInput {
                title: "Renew expired claim".to_string(),
                goal: "Renew expired claim".to_string(),
                status: Some(prism_ir::PlanStatus::Active),
                policy: None,
            },
        )
        .unwrap();
    let (task_id, task) = store
        .create_task(
            principal_meta("event:task", 2, "local", "agent:a", "session:a"),
            TaskCreateInput {
                plan_id,
                title: "Hold edit claim".to_string(),
                status: Some(prism_ir::CoordinationTaskStatus::Ready),
                assignee: Some(prism_ir::AgentId::new("agent:a")),
                session: Some(prism_ir::SessionId::new("session:a")),
                worktree_id: None,
                branch_ref: None,
                anchors: vec![prism_ir::AnchorRef::Kind(prism_ir::NodeKind::Function)],
                depends_on: Vec::new(),
                acceptance: Vec::new(),
                base_revision: revision(),
            },
        )
        .unwrap();
    let (claim_id, _, _) = store
        .acquire_claim(
            principal_meta("event:claim", 3, "local", "agent:a", "session:a"),
            prism_ir::SessionId::new("session:a"),
            ClaimAcquireInput {
                task_id: Some(task_id),
                anchors: task.anchors.clone(),
                capability: prism_ir::Capability::Edit,
                mode: Some(prism_ir::ClaimMode::HardExclusive),
                ttl_seconds: None,
                base_revision: revision(),
                current_revision: revision(),
                agent: Some(prism_ir::AgentId::new("agent:a")),
                worktree_id: None,
                branch_ref: None,
            },
        )
        .unwrap();

    let renewed = store
        .renew_claim(
            principal_meta("event:renew", 8000, "local", "agent:a", "session:a"),
            &prism_ir::SessionId::new("session:a"),
            &claim_id.expect("claim id"),
            None,
            "explicit",
        )
        .unwrap();
    assert_eq!(renewed.status, prism_ir::ClaimStatus::Active);
    assert!(renewed.expires_at > 8000);
}

#[test]
fn stale_claim_no_longer_blocks_new_acquire() {
    let store = CoordinationStore::new();
    let (plan_id, _) = store
        .create_plan(
            principal_meta("event:plan", 1, "local", "agent:a", "session:a"),
            PlanCreateInput {
                title: "Allow takeover after stale claim".to_string(),
                goal: "Allow takeover after stale claim".to_string(),
                status: Some(prism_ir::PlanStatus::Active),
                policy: None,
            },
        )
        .unwrap();
    let (task_id, task) = store
        .create_task(
            principal_meta("event:task", 2, "local", "agent:a", "session:a"),
            TaskCreateInput {
                plan_id,
                title: "Edit alpha".to_string(),
                status: Some(prism_ir::CoordinationTaskStatus::Ready),
                assignee: Some(prism_ir::AgentId::new("agent:a")),
                session: Some(prism_ir::SessionId::new("session:a")),
                worktree_id: None,
                branch_ref: None,
                anchors: vec![prism_ir::AnchorRef::Kind(prism_ir::NodeKind::Function)],
                depends_on: Vec::new(),
                acceptance: Vec::new(),
                base_revision: revision(),
            },
        )
        .unwrap();
    store
        .acquire_claim(
            principal_meta("event:claim:a", 3, "local", "agent:a", "session:a"),
            prism_ir::SessionId::new("session:a"),
            ClaimAcquireInput {
                task_id: Some(task_id),
                anchors: task.anchors.clone(),
                capability: prism_ir::Capability::Edit,
                mode: Some(prism_ir::ClaimMode::HardExclusive),
                ttl_seconds: None,
                base_revision: revision(),
                current_revision: revision(),
                agent: Some(prism_ir::AgentId::new("agent:a")),
                worktree_id: None,
                branch_ref: None,
            },
        )
        .unwrap();

    let second = store
        .acquire_claim(
            principal_meta("event:claim:b", 1905, "local", "agent:b", "session:b"),
            prism_ir::SessionId::new("session:b"),
            ClaimAcquireInput {
                task_id: None,
                anchors: task.anchors.clone(),
                capability: prism_ir::Capability::Edit,
                mode: Some(prism_ir::ClaimMode::HardExclusive),
                ttl_seconds: None,
                base_revision: revision(),
                current_revision: revision(),
                agent: Some(prism_ir::AgentId::new("agent:b")),
                worktree_id: None,
                branch_ref: None,
            },
        )
        .unwrap();
    assert!(second.0.is_some());
}

#[test]
fn review_policy_gates_completion_but_not_ready_work() {
    let store = CoordinationStore::new();
    let (plan_id, _) = store
        .create_plan(
            meta("event:1", 1),
            PlanCreateInput {
                title: "Ship reviewed change".to_string(),
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
                plan_id: plan_id.clone(),
                title: "Edit main".to_string(),
                status: Some(prism_ir::CoordinationTaskStatus::Ready),
                assignee: None,
                session: Some(prism_ir::SessionId::new("session:a")),
                worktree_id: None,
                branch_ref: None,
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
                &plan_id,
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
                kind: None,
                status: Some(prism_ir::CoordinationTaskStatus::Completed),
                assignee: None,
                session: None,
                worktree_id: None,
                branch_ref: None,
                title: None,
                summary: None,
                anchors: None,
                bindings: None,
                depends_on: None,
                acceptance: None,
                validation_refs: None,
                is_abstract: None,
                base_revision: None,
                priority: None,
                tags: None,
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
                worktree_id: None,
                branch_ref: None,
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
                    kind: None,
                    status: Some(prism_ir::CoordinationTaskStatus::Completed),
                    assignee: None,
                    session: None,
                    worktree_id: None,
                    branch_ref: None,
                    title: None,
                    summary: None,
                    anchors: None,
                    bindings: None,
                    depends_on: None,
                    acceptance: None,
                    validation_refs: None,
                    is_abstract: None,
                    base_revision: None,
                    priority: None,
                    tags: None,
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
fn incremental_coordination_read_model_matches_snapshot_rebuild() {
    let store = CoordinationStore::new();
    let (plan_id, _) = store
        .create_plan(
            meta("event:plan", 1),
            PlanCreateInput {
                title: "Ship reviewed change".to_string(),
                goal: "Ship reviewed change".to_string(),
                status: None,
                policy: Some(CoordinationPolicy {
                    require_review_for_completion: true,
                    ..CoordinationPolicy::default()
                }),
            },
        )
        .unwrap();
    let (task_id, task) = store
        .create_task(
            meta("event:task", 2),
            TaskCreateInput {
                plan_id,
                title: "Edit main".to_string(),
                status: Some(prism_ir::CoordinationTaskStatus::Ready),
                assignee: None,
                session: Some(prism_ir::SessionId::new("session:a")),
                worktree_id: Some("worktree:a".to_string()),
                branch_ref: Some("refs/heads/main".to_string()),
                anchors: vec![prism_ir::AnchorRef::Kind(prism_ir::NodeKind::Function)],
                depends_on: Vec::new(),
                acceptance: Vec::new(),
                base_revision: revision(),
            },
        )
        .unwrap();

    let base_snapshot = store.snapshot();
    let base_read_model = coordination_read_model_from_snapshot(&base_snapshot);

    store
        .update_task(
            meta("event:task-review", 3),
            TaskUpdateInput {
                task_id: task_id.clone(),
                kind: None,
                status: Some(prism_ir::CoordinationTaskStatus::InReview),
                assignee: None,
                session: None,
                worktree_id: None,
                branch_ref: None,
                title: None,
                summary: None,
                anchors: None,
                bindings: None,
                depends_on: None,
                acceptance: None,
                validation_refs: None,
                is_abstract: None,
                base_revision: None,
                priority: None,
                tags: None,
                completion_context: None,
            },
            revision(),
            3,
        )
        .unwrap();
    store
        .acquire_claim(
            meta("event:claim", 4),
            prism_ir::SessionId::new("session:a"),
            ClaimAcquireInput {
                task_id: Some(task_id.clone()),
                anchors: task.anchors.clone(),
                capability: prism_ir::Capability::Edit,
                mode: Some(prism_ir::ClaimMode::SoftExclusive),
                ttl_seconds: Some(60),
                base_revision: revision(),
                current_revision: revision(),
                agent: None,
                worktree_id: Some("worktree:a".to_string()),
                branch_ref: Some("refs/heads/main".to_string()),
            },
        )
        .unwrap();
    store
        .propose_artifact(
            meta("event:artifact", 5),
            ArtifactProposeInput {
                task_id: task_id.clone(),
                anchors: task.anchors.clone(),
                diff_ref: Some("patch:main".to_string()),
                evidence: Vec::new(),
                base_revision: revision(),
                current_revision: revision(),
                required_validations: Vec::new(),
                validated_checks: Vec::new(),
                risk_score: Some(0.2),
                worktree_id: Some("worktree:a".to_string()),
                branch_ref: Some("refs/heads/main".to_string()),
            },
        )
        .unwrap();
    assert!(store
        .update_task(
            meta("event:reject", 6),
            TaskUpdateInput {
                task_id: task_id.clone(),
                kind: None,
                status: Some(prism_ir::CoordinationTaskStatus::Completed),
                assignee: None,
                session: None,
                worktree_id: None,
                branch_ref: None,
                title: None,
                summary: None,
                anchors: None,
                bindings: None,
                depends_on: None,
                acceptance: None,
                validation_refs: None,
                is_abstract: None,
                base_revision: None,
                priority: None,
                tags: None,
                completion_context: None,
            },
            revision(),
            6,
        )
        .is_err());

    let final_snapshot = store.snapshot();
    let appended_events = final_snapshot.events[base_snapshot.events.len()..].to_vec();

    assert_eq!(
        coordination_read_model_from_seed(
            &final_snapshot,
            Some(&base_read_model),
            &appended_events
        ),
        coordination_read_model_from_snapshot(&final_snapshot)
    );
}

#[test]
fn incremental_coordination_queue_read_model_matches_snapshot_rebuild() {
    let store = CoordinationStore::new();
    let (plan_id, _) = store
        .create_plan(
            meta("event:plan", 1),
            PlanCreateInput {
                title: "Ship handoff".to_string(),
                goal: "Ship handoff".to_string(),
                status: None,
                policy: None,
            },
        )
        .unwrap();
    let (task_id, task) = store
        .create_task(
            meta("event:task", 2),
            TaskCreateInput {
                plan_id,
                title: "Edit main".to_string(),
                status: Some(prism_ir::CoordinationTaskStatus::Ready),
                assignee: Some(prism_ir::AgentId::new("agent:a")),
                session: Some(prism_ir::SessionId::new("session:a")),
                worktree_id: Some("worktree:a".to_string()),
                branch_ref: Some("refs/heads/main".to_string()),
                anchors: vec![prism_ir::AnchorRef::Kind(prism_ir::NodeKind::Function)],
                depends_on: Vec::new(),
                acceptance: Vec::new(),
                base_revision: revision(),
            },
        )
        .unwrap();

    let base_snapshot = store.snapshot();
    let base_queue_model = coordination_queue_read_model_from_snapshot(&base_snapshot);

    store
        .handoff(
            meta("event:handoff", 3),
            HandoffInput {
                task_id: task_id.clone(),
                to_agent: Some(prism_ir::AgentId::new("agent:b")),
                summary: "Need review".to_string(),
                base_revision: revision(),
            },
            revision(),
        )
        .unwrap();
    store
        .accept_handoff(
            meta("event:handoff-accept", 4),
            HandoffAcceptInput {
                task_id: task_id.clone(),
                agent: Some(prism_ir::AgentId::new("agent:b")),
                worktree_id: Some("worktree:b".to_string()),
                branch_ref: Some("refs/heads/feature".to_string()),
            },
        )
        .unwrap();
    store
        .acquire_claim(
            meta("event:claim", 5),
            prism_ir::SessionId::new("session:b"),
            ClaimAcquireInput {
                task_id: Some(task_id.clone()),
                anchors: task.anchors.clone(),
                capability: prism_ir::Capability::Edit,
                mode: Some(prism_ir::ClaimMode::SoftExclusive),
                ttl_seconds: Some(60),
                base_revision: revision(),
                current_revision: revision(),
                agent: Some(prism_ir::AgentId::new("agent:b")),
                worktree_id: Some("worktree:b".to_string()),
                branch_ref: Some("refs/heads/feature".to_string()),
            },
        )
        .unwrap();
    store
        .propose_artifact(
            meta("event:artifact", 6),
            ArtifactProposeInput {
                task_id,
                anchors: task.anchors.clone(),
                diff_ref: Some("patch:feature".to_string()),
                evidence: Vec::new(),
                base_revision: revision(),
                current_revision: revision(),
                required_validations: Vec::new(),
                validated_checks: Vec::new(),
                risk_score: Some(0.1),
                worktree_id: Some("worktree:b".to_string()),
                branch_ref: Some("refs/heads/feature".to_string()),
            },
        )
        .unwrap();

    let final_snapshot = store.snapshot();
    let appended_events = final_snapshot.events[base_snapshot.events.len()..].to_vec();

    assert_eq!(
        coordination_queue_read_model_from_seed(
            &final_snapshot,
            Some(&base_queue_model),
            &appended_events
        ),
        coordination_queue_read_model_from_snapshot(&final_snapshot)
    );
}

#[test]
fn edit_capacity_limit_blocks_extra_claims() {
    let store = CoordinationStore::new();
    let (plan_id, _) = store
        .create_plan(
            meta("event:1", 1),
            PlanCreateInput {
                title: "Serialize edits".to_string(),
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
                worktree_id: None,
                branch_ref: None,
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
                worktree_id: None,
                branch_ref: None,
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
                worktree_id: None,
                branch_ref: None,
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
                title: "Catch stale approvals".to_string(),
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
                worktree_id: None,
                branch_ref: None,
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
                worktree_id: None,
                branch_ref: None,
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
                title: "Validate risky change".to_string(),
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
                worktree_id: None,
                branch_ref: None,
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
                worktree_id: None,
                branch_ref: None,
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
                    kind: None,
                    status: Some(prism_ir::CoordinationTaskStatus::Completed),
                    assignee: None,
                    session: None,
                    worktree_id: None,
                    branch_ref: None,
                    title: None,
                    summary: None,
                    anchors: None,
                    bindings: None,
                    depends_on: None,
                    acceptance: None,
                    validation_refs: None,
                    is_abstract: None,
                    base_revision: None,
                    priority: None,
                    tags: None,
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
                title: "Risky edit".to_string(),
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
                worktree_id: None,
                branch_ref: None,
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
                kind: None,
                status: Some(prism_ir::CoordinationTaskStatus::Completed),
                assignee: None,
                session: None,
                worktree_id: None,
                branch_ref: None,
                title: None,
                summary: None,
                anchors: None,
                bindings: None,
                depends_on: None,
                acceptance: None,
                validation_refs: None,
                is_abstract: None,
                base_revision: None,
                priority: None,
                tags: None,
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
                title: "Project coordination into a plan graph".to_string(),
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
                worktree_id: None,
                branch_ref: None,
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
                worktree_id: None,
                branch_ref: None,
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
                title: "Separate runtime execution overlay".to_string(),
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
                worktree_id: None,
                branch_ref: None,
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
                title: "Enforce task lifecycle".to_string(),
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
                worktree_id: None,
                branch_ref: None,
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
                kind: None,
                status: Some(prism_ir::CoordinationTaskStatus::Completed),
                assignee: None,
                session: None,
                worktree_id: None,
                branch_ref: None,
                title: None,
                summary: None,
                anchors: None,
                bindings: None,
                depends_on: None,
                acceptance: None,
                validation_refs: None,
                is_abstract: None,
                base_revision: Some(prism_ir::WorkspaceRevision {
                    graph_version: 1,
                    git_commit: None,
                }),
                priority: None,
                tags: None,
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
                title: "Reject stale writes".to_string(),
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
                worktree_id: None,
                branch_ref: None,
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
                worktree_id: None,
                branch_ref: None,
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
                worktree_id: None,
                branch_ref: None,
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
                title: "Close coordinated work".to_string(),
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
                worktree_id: None,
                branch_ref: None,
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
                worktree_id: None,
                branch_ref: None,
            },
        )
        .unwrap();

    let error = store
        .update_plan(
            meta("event:4", 4),
            PlanUpdateInput {
                plan_id: plan_id.clone(),
                title: None,
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
                kind: None,
                status: Some(prism_ir::CoordinationTaskStatus::Completed),
                assignee: None,
                session: None,
                worktree_id: None,
                branch_ref: None,
                title: None,
                summary: None,
                anchors: None,
                bindings: None,
                depends_on: None,
                acceptance: None,
                validation_refs: None,
                is_abstract: None,
                base_revision: Some(prism_ir::WorkspaceRevision {
                    graph_version: 1,
                    git_commit: None,
                }),
                priority: None,
                tags: None,
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
                title: None,
                status: Some(prism_ir::PlanStatus::Completed),
                goal: None,
                policy: None,
            },
        )
        .unwrap();
    assert_eq!(plan.status, prism_ir::PlanStatus::Completed);
}

#[test]
fn completing_last_task_auto_completes_task_execution_plan() {
    let store = CoordinationStore::new();
    let (plan_id, _) = store
        .create_plan(
            meta("event:1", 1),
            PlanCreateInput {
                title: "Close execution plan automatically".to_string(),
                goal: "Close execution plan automatically".to_string(),
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
                title: "Finish the only task".to_string(),
                status: Some(prism_ir::CoordinationTaskStatus::Ready),
                assignee: None,
                session: Some(prism_ir::SessionId::new("session:a")),
                worktree_id: None,
                branch_ref: None,
                anchors: vec![prism_ir::AnchorRef::Kind(prism_ir::NodeKind::Function)],
                depends_on: Vec::new(),
                acceptance: Vec::new(),
                base_revision: revision(),
            },
        )
        .unwrap();

    store
        .update_task(
            meta("event:3", 3),
            TaskUpdateInput {
                task_id,
                kind: None,
                status: Some(prism_ir::CoordinationTaskStatus::Completed),
                assignee: None,
                session: None,
                worktree_id: None,
                branch_ref: None,
                title: None,
                summary: None,
                anchors: None,
                bindings: None,
                depends_on: None,
                acceptance: None,
                validation_refs: None,
                is_abstract: None,
                base_revision: Some(revision()),
                priority: None,
                tags: None,
                completion_context: Some(TaskCompletionContext::default()),
            },
            revision(),
            3,
        )
        .unwrap();

    let plan = store.plan(&plan_id).expect("plan");
    assert_eq!(plan.status, prism_ir::PlanStatus::Completed);
    let events = store.events();
    let event = events.last().expect("plan auto-close event");
    assert_eq!(event.kind, prism_ir::CoordinationEventKind::PlanUpdated);
    assert_eq!(event.plan.as_ref(), Some(&plan_id));
    assert_eq!(event.metadata["autoTransition"], "all_tasks_completed");
    assert_eq!(
        event.meta.causation.as_ref().map(|id| id.0.as_str()),
        Some("event:3")
    );
}

#[test]
fn completing_one_of_multiple_tasks_keeps_plan_active() {
    let store = CoordinationStore::new();
    let (plan_id, _) = store
        .create_plan(
            meta("event:1", 1),
            PlanCreateInput {
                title: "Only close after every task is done".to_string(),
                goal: "Only close after every task is done".to_string(),
                status: None,
                policy: None,
            },
        )
        .unwrap();
    let (first_task_id, _) = store
        .create_task(
            meta("event:2", 2),
            TaskCreateInput {
                plan_id: plan_id.clone(),
                title: "Complete first task".to_string(),
                status: Some(prism_ir::CoordinationTaskStatus::Ready),
                assignee: None,
                session: Some(prism_ir::SessionId::new("session:a")),
                worktree_id: None,
                branch_ref: None,
                anchors: vec![prism_ir::AnchorRef::Kind(prism_ir::NodeKind::Function)],
                depends_on: Vec::new(),
                acceptance: Vec::new(),
                base_revision: revision(),
            },
        )
        .unwrap();
    store
        .create_task(
            meta("event:3", 3),
            TaskCreateInput {
                plan_id: plan_id.clone(),
                title: "Leave second task ready".to_string(),
                status: Some(prism_ir::CoordinationTaskStatus::Ready),
                assignee: None,
                session: Some(prism_ir::SessionId::new("session:b")),
                worktree_id: None,
                branch_ref: None,
                anchors: vec![prism_ir::AnchorRef::Kind(prism_ir::NodeKind::Method)],
                depends_on: Vec::new(),
                acceptance: Vec::new(),
                base_revision: revision(),
            },
        )
        .unwrap();

    store
        .update_task(
            meta("event:4", 4),
            TaskUpdateInput {
                task_id: first_task_id,
                kind: None,
                status: Some(prism_ir::CoordinationTaskStatus::Completed),
                assignee: None,
                session: None,
                worktree_id: None,
                branch_ref: None,
                title: None,
                summary: None,
                anchors: None,
                bindings: None,
                depends_on: None,
                acceptance: None,
                validation_refs: None,
                is_abstract: None,
                base_revision: Some(revision()),
                priority: None,
                tags: None,
                completion_context: Some(TaskCompletionContext::default()),
            },
            revision(),
            4,
        )
        .unwrap();

    let plan = store.plan(&plan_id).expect("plan");
    assert_eq!(plan.status, prism_ir::PlanStatus::Active);
    let events = store.events();
    let event = events.last().expect("task status event");
    assert_eq!(
        event.kind,
        prism_ir::CoordinationEventKind::TaskStatusChanged
    );
}

#[test]
fn releasing_last_active_claim_auto_completes_plan() {
    let store = CoordinationStore::new();
    let (plan_id, _) = store
        .create_plan(
            meta("event:1", 1),
            PlanCreateInput {
                title: "Close after claim release".to_string(),
                goal: "Close after claim release".to_string(),
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
                title: "Finish task before releasing claim".to_string(),
                status: Some(prism_ir::CoordinationTaskStatus::Ready),
                assignee: None,
                session: Some(prism_ir::SessionId::new("session:a")),
                worktree_id: None,
                branch_ref: None,
                anchors: vec![prism_ir::AnchorRef::Kind(prism_ir::NodeKind::Function)],
                depends_on: Vec::new(),
                acceptance: Vec::new(),
                base_revision: revision(),
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
                base_revision: revision(),
                current_revision: revision(),
                agent: None,
                worktree_id: None,
                branch_ref: None,
            },
        )
        .unwrap()
        .0
        .expect("claim id");

    store
        .update_task(
            meta("event:4", 4),
            TaskUpdateInput {
                task_id,
                kind: None,
                status: Some(prism_ir::CoordinationTaskStatus::Completed),
                assignee: None,
                session: None,
                worktree_id: None,
                branch_ref: None,
                title: None,
                summary: None,
                anchors: None,
                bindings: None,
                depends_on: None,
                acceptance: None,
                validation_refs: None,
                is_abstract: None,
                base_revision: Some(revision()),
                priority: None,
                tags: None,
                completion_context: Some(TaskCompletionContext::default()),
            },
            revision(),
            4,
        )
        .unwrap();
    assert_eq!(
        store.plan(&plan_id).expect("plan before release").status,
        prism_ir::PlanStatus::Active
    );

    store
        .release_claim(
            meta("event:5", 5),
            &prism_ir::SessionId::new("session:a"),
            &claim_id,
        )
        .unwrap();

    let plan = store.plan(&plan_id).expect("plan after release");
    assert_eq!(plan.status, prism_ir::PlanStatus::Completed);
    let events = store.events();
    let event = events.last().expect("plan auto-close event");
    assert_eq!(event.kind, prism_ir::CoordinationEventKind::PlanUpdated);
    assert_eq!(event.metadata["autoTransition"], "all_tasks_completed");
    assert_eq!(
        event.meta.causation.as_ref().map(|id| id.0.as_str()),
        Some("event:5")
    );
}

#[test]
fn closed_plan_rejects_new_task_and_records_violation() {
    let store = CoordinationStore::new();
    let (plan_id, _) = store
        .create_plan(
            meta("event:1", 1),
            PlanCreateInput {
                title: "Archive repo work".to_string(),
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
                title: None,
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
                worktree_id: None,
                branch_ref: None,
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
fn archived_plan_transition_requires_terminal_status_and_stays_closed() {
    let store = CoordinationStore::new();
    let (plan_id, _) = store
        .create_plan(
            meta("event:1", 1),
            PlanCreateInput {
                title: "Archive repo work".to_string(),
                goal: "Archive repo work".to_string(),
                status: None,
                policy: None,
            },
        )
        .unwrap();

    let invalid = store
        .update_plan(
            meta("event:2", 2),
            PlanUpdateInput {
                plan_id: plan_id.clone(),
                title: None,
                status: Some(prism_ir::PlanStatus::Archived),
                goal: None,
                policy: None,
            },
        )
        .unwrap_err();
    assert!(invalid
        .to_string()
        .contains("invalid coordination plan transition"));

    let abandoned = store
        .update_plan(
            meta("event:3", 3),
            PlanUpdateInput {
                plan_id: plan_id.clone(),
                title: None,
                status: Some(prism_ir::PlanStatus::Abandoned),
                goal: None,
                policy: None,
            },
        )
        .unwrap();
    assert_eq!(abandoned.status, prism_ir::PlanStatus::Abandoned);

    let archived = store
        .update_plan(
            meta("event:4", 4),
            PlanUpdateInput {
                plan_id: plan_id.clone(),
                title: None,
                status: Some(prism_ir::PlanStatus::Archived),
                goal: None,
                policy: None,
            },
        )
        .unwrap();
    assert_eq!(archived.status, prism_ir::PlanStatus::Archived);

    let error = store
        .create_task(
            meta("event:5", 5),
            TaskCreateInput {
                plan_id,
                title: "Should not exist".to_string(),
                status: None,
                assignee: None,
                session: Some(prism_ir::SessionId::new("session:a")),
                worktree_id: None,
                branch_ref: None,
                anchors: vec![prism_ir::AnchorRef::Kind(prism_ir::NodeKind::Function)],
                depends_on: Vec::new(),
                acceptance: Vec::new(),
                base_revision: revision(),
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
fn plan_update_events_record_patch_metadata() {
    let store = CoordinationStore::new();
    let (plan_id, _) = store
        .create_plan(
            meta("event:1", 1),
            PlanCreateInput {
                title: "Original title".to_string(),
                goal: "Original goal".to_string(),
                status: Some(prism_ir::PlanStatus::Draft),
                policy: None,
            },
        )
        .unwrap();

    store
        .update_plan(
            meta("event:2", 2),
            PlanUpdateInput {
                plan_id: plan_id.clone(),
                title: Some("Refined title".to_string()),
                status: Some(prism_ir::PlanStatus::Active),
                goal: Some("Refined goal".to_string()),
                policy: None,
            },
        )
        .unwrap();

    let updated = store.plan(&plan_id).unwrap();
    assert_eq!(updated.title, "Refined title");
    assert_eq!(updated.goal, "Refined goal");

    let event = store.events().last().unwrap().clone();
    assert_eq!(event.kind, prism_ir::CoordinationEventKind::PlanUpdated);
    assert_eq!(event.metadata["status"], "Active");
    assert_eq!(event.metadata["previousStatus"], "Draft");
    assert_eq!(event.metadata["patch"]["title"], "set");
    assert_eq!(event.metadata["patch"]["status"], "set");
    assert_eq!(event.metadata["patch"]["goal"], "set");
    assert_eq!(event.metadata["patchValues"]["title"], "Refined title");
    assert_eq!(event.metadata["patchValues"]["goal"], "Refined goal");
    assert!(event.metadata["patch"].get("policy").is_none());
}

#[test]
fn draft_plan_hides_ready_work_until_activation() {
    let store = CoordinationStore::new();
    let (plan_id, plan) = store
        .create_plan(
            meta("event:1", 1),
            PlanCreateInput {
                title: "Stage a coordinated rollout".to_string(),
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
                worktree_id: None,
                branch_ref: None,
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
                title: None,
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
fn task_update_events_record_sparse_patch_metadata() {
    let store = CoordinationStore::new();
    let (plan_id, _) = store
        .create_plan(
            meta("event:1", 1),
            PlanCreateInput {
                title: "Track task patches".to_string(),
                goal: "Track task patches".to_string(),
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
                title: "Investigate".to_string(),
                status: Some(prism_ir::CoordinationTaskStatus::Ready),
                assignee: Some(prism_ir::AgentId::new("agent:a")),
                session: Some(prism_ir::SessionId::new("session:a")),
                worktree_id: None,
                branch_ref: None,
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

    store
        .update_task(
            meta("event:3", 3),
            TaskUpdateInput {
                task_id,
                kind: None,
                status: Some(prism_ir::CoordinationTaskStatus::InProgress),
                assignee: Some(None),
                session: Some(None),
                worktree_id: None,
                branch_ref: None,
                title: Some("Investigate deeply".to_string()),
                summary: None,
                anchors: None,
                bindings: None,
                depends_on: None,
                acceptance: None,
                validation_refs: None,
                is_abstract: None,
                base_revision: Some(prism_ir::WorkspaceRevision {
                    graph_version: 1,
                    git_commit: None,
                }),
                priority: None,
                tags: None,
                completion_context: None,
            },
            prism_ir::WorkspaceRevision {
                graph_version: 1,
                git_commit: None,
            },
            3,
        )
        .unwrap();

    let event = store.events().last().unwrap().clone();
    assert_eq!(event.kind, prism_ir::CoordinationEventKind::TaskAssigned);
    assert_eq!(event.metadata["status"], "InProgress");
    assert_eq!(event.metadata["previousStatus"], "Ready");
    assert!(event.metadata["assignee"].is_null());
    assert_eq!(event.metadata["patch"]["status"], "set");
    assert_eq!(event.metadata["patch"]["assignee"], "clear");
    assert_eq!(event.metadata["patch"]["session"], "clear");
    assert_eq!(event.metadata["patch"]["title"], "set");
    assert_eq!(event.metadata["patch"]["baseRevision"], "set");
}

#[test]
fn snapshot_load_replays_plan_and_task_patch_events() {
    let store = CoordinationStore::new();
    let (plan_id, _) = store
        .create_plan(
            meta("event:1", 1),
            PlanCreateInput {
                title: "Original goal".to_string(),
                goal: "Original goal".to_string(),
                status: Some(prism_ir::PlanStatus::Draft),
                policy: None,
            },
        )
        .unwrap();
    store
        .update_plan(
            meta("event:2", 2),
            PlanUpdateInput {
                plan_id: plan_id.clone(),
                title: None,
                status: Some(prism_ir::PlanStatus::Active),
                goal: Some("Refined goal".to_string()),
                policy: None,
            },
        )
        .unwrap();
    let (task_id, _) = store
        .create_task(
            meta("event:3", 3),
            TaskCreateInput {
                plan_id: plan_id.clone(),
                title: "Investigate".to_string(),
                status: Some(prism_ir::CoordinationTaskStatus::Ready),
                assignee: Some(prism_ir::AgentId::new("agent:a")),
                session: Some(prism_ir::SessionId::new("session:a")),
                worktree_id: None,
                branch_ref: None,
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
    let (dependency_id, _) = store
        .create_task(
            meta("event:4", 4),
            TaskCreateInput {
                plan_id: plan_id.clone(),
                title: "Dependency".to_string(),
                status: Some(prism_ir::CoordinationTaskStatus::Ready),
                assignee: None,
                session: None,
                worktree_id: None,
                branch_ref: None,
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
    store
        .update_task(
            meta("event:5", 5),
            TaskUpdateInput {
                task_id: task_id.clone(),
                kind: None,
                status: Some(prism_ir::CoordinationTaskStatus::InProgress),
                assignee: Some(None),
                session: Some(None),
                worktree_id: None,
                branch_ref: None,
                title: Some("Investigate deeply".to_string()),
                summary: None,
                anchors: None,
                bindings: None,
                depends_on: Some(vec![dependency_id.clone()]),
                acceptance: None,
                validation_refs: None,
                is_abstract: None,
                base_revision: Some(prism_ir::WorkspaceRevision {
                    graph_version: 1,
                    git_commit: None,
                }),
                priority: None,
                tags: None,
                completion_context: None,
            },
            prism_ir::WorkspaceRevision {
                graph_version: 1,
                git_commit: None,
            },
            5,
        )
        .unwrap();

    let mut snapshot = store.snapshot();
    let plan = snapshot
        .plans
        .iter_mut()
        .find(|plan| plan.id == plan_id)
        .expect("plan should be present");
    plan.goal = "stale goal".to_string();
    plan.status = prism_ir::PlanStatus::Draft;
    plan.root_tasks = vec![task_id.clone()];
    let task = snapshot
        .tasks
        .iter_mut()
        .find(|task| task.id == task_id)
        .expect("task should be present");
    task.title = "stale title".to_string();
    task.status = prism_ir::CoordinationTaskStatus::Ready;
    task.assignee = Some(prism_ir::AgentId::new("agent:stale"));
    task.session = Some(prism_ir::SessionId::new("session:stale"));
    task.depends_on.clear();

    let reloaded = CoordinationStore::from_snapshot(snapshot);
    let plan = reloaded.plan(&plan_id).expect("plan should reload");
    assert_eq!(plan.goal, "Refined goal");
    assert_eq!(plan.status, prism_ir::PlanStatus::Active);
    assert_eq!(plan.root_tasks, vec![dependency_id]);
    let task = reloaded.task(&task_id).expect("task should reload");
    assert_eq!(task.title, "Investigate deeply");
    assert_eq!(task.status, prism_ir::CoordinationTaskStatus::InProgress);
    assert_eq!(task.assignee, None);
    assert_eq!(task.session, None);
}

#[test]
fn snapshot_load_replays_patches_without_losing_native_plan_and_node_metadata() {
    let store = CoordinationStore::new();
    let (plan_id, _) = store
        .create_plan(
            meta("event:1", 1),
            PlanCreateInput {
                title: "Original goal".to_string(),
                goal: "Original goal".to_string(),
                status: Some(prism_ir::PlanStatus::Draft),
                policy: None,
            },
        )
        .unwrap();
    store
        .update_plan(
            meta("event:2", 2),
            PlanUpdateInput {
                plan_id: plan_id.clone(),
                title: None,
                status: Some(prism_ir::PlanStatus::Active),
                goal: Some("Refined goal".to_string()),
                policy: None,
            },
        )
        .unwrap();
    let (task_id, _) = store
        .create_task(
            meta("event:3", 3),
            TaskCreateInput {
                plan_id: plan_id.clone(),
                title: "Investigate".to_string(),
                status: Some(prism_ir::CoordinationTaskStatus::Ready),
                assignee: Some(prism_ir::AgentId::new("agent:a")),
                session: Some(prism_ir::SessionId::new("session:a")),
                worktree_id: None,
                branch_ref: None,
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
    store
        .update_task(
            meta("event:4", 4),
            TaskUpdateInput {
                task_id: task_id.clone(),
                kind: None,
                status: Some(prism_ir::CoordinationTaskStatus::InProgress),
                assignee: Some(None),
                session: Some(None),
                worktree_id: None,
                branch_ref: None,
                title: Some("Investigate deeply".to_string()),
                summary: None,
                anchors: Some(vec![prism_ir::AnchorRef::Kind(prism_ir::NodeKind::Method)]),
                bindings: None,
                depends_on: None,
                acceptance: None,
                validation_refs: None,
                is_abstract: None,
                base_revision: Some(prism_ir::WorkspaceRevision {
                    graph_version: 1,
                    git_commit: None,
                }),
                priority: None,
                tags: None,
                completion_context: None,
            },
            prism_ir::WorkspaceRevision {
                graph_version: 1,
                git_commit: None,
            },
            4,
        )
        .unwrap();

    let mut snapshot = store.snapshot();
    let plan = snapshot
        .plans
        .iter_mut()
        .find(|plan| plan.id == plan_id)
        .expect("stored plan");
    plan.title = "Native plan title".to_string();
    plan.kind = prism_ir::PlanKind::Migration;
    plan.revision = 7;
    plan.tags = vec!["persistence".to_string(), "ux".to_string()];
    plan.created_from = Some("concept://persistence_runtime".to_string());
    plan.metadata = serde_json::json!({ "source": "native-plan" });
    let task = snapshot
        .tasks
        .iter_mut()
        .find(|task| task.id == task_id)
        .expect("stored task");
    task.kind = prism_ir::PlanNodeKind::Validate;
    task.summary = Some("Keep authored summary".to_string());
    task.bindings = prism_ir::PlanBinding {
        anchors: vec![prism_ir::AnchorRef::Kind(prism_ir::NodeKind::Method)],
        concept_handles: vec!["concept://validation_pipeline".to_string()],
        artifact_refs: vec!["artifact:alpha".to_string()],
        memory_refs: vec!["memory:alpha".to_string()],
        outcome_refs: vec!["outcome:alpha".to_string()],
    };
    task.validation_refs = vec![prism_ir::ValidationRef {
        id: "validation:alpha".to_string(),
    }];
    task.is_abstract = true;
    task.priority = Some(4);
    task.tags = vec!["native".to_string(), "preserve".to_string()];
    task.metadata = serde_json::json!({ "source": "native-node" });
    let plan_create = snapshot
        .events
        .iter_mut()
        .find(|event| event.kind == prism_ir::CoordinationEventKind::PlanCreated)
        .expect("plan create event");
    plan_create.metadata["plan"] = serde_json::to_value(Plan {
        id: plan_id.clone(),
        goal: "Original goal".to_string(),
        title: "Native plan title".to_string(),
        status: prism_ir::PlanStatus::Draft,
        policy: CoordinationPolicy::default(),
        scope: prism_ir::PlanScope::Repo,
        kind: prism_ir::PlanKind::Migration,
        revision: 7,
        tags: vec!["persistence".to_string(), "ux".to_string()],
        created_from: Some("concept://persistence_runtime".to_string()),
        metadata: serde_json::json!({ "source": "native-plan" }),
        root_tasks: vec![task_id.clone()],
    })
    .unwrap();
    let task_create = snapshot
        .events
        .iter_mut()
        .find(|event| event.kind == prism_ir::CoordinationEventKind::TaskCreated)
        .expect("task create event");
    task_create.metadata["task"] = serde_json::to_value(CoordinationTask {
        id: task_id.clone(),
        plan: plan_id.clone(),
        kind: prism_ir::PlanNodeKind::Validate,
        title: "Investigate".to_string(),
        summary: Some("Keep authored summary".to_string()),
        status: prism_ir::CoordinationTaskStatus::Ready,
        assignee: Some(prism_ir::AgentId::new("agent:a")),
        pending_handoff_to: None,
        session: Some(prism_ir::SessionId::new("session:a")),
        lease_holder: None,
        lease_started_at: None,
        lease_refreshed_at: None,
        lease_stale_at: None,
        lease_expires_at: None,
        worktree_id: None,
        branch_ref: None,
        anchors: vec![prism_ir::AnchorRef::Kind(prism_ir::NodeKind::Function)],
        bindings: prism_ir::PlanBinding {
            anchors: vec![prism_ir::AnchorRef::Kind(prism_ir::NodeKind::Function)],
            concept_handles: vec!["concept://validation_pipeline".to_string()],
            artifact_refs: vec!["artifact:alpha".to_string()],
            memory_refs: vec!["memory:alpha".to_string()],
            outcome_refs: vec!["outcome:alpha".to_string()],
        },
        depends_on: Vec::new(),
        acceptance: Vec::new(),
        validation_refs: vec![prism_ir::ValidationRef {
            id: "validation:alpha".to_string(),
        }],
        is_abstract: true,
        base_revision: prism_ir::WorkspaceRevision {
            graph_version: 1,
            git_commit: None,
        },
        priority: Some(4),
        tags: vec!["native".to_string(), "preserve".to_string()],
        metadata: serde_json::json!({ "source": "native-node" }),
    })
    .unwrap();

    let reloaded = CoordinationStore::from_snapshot(snapshot);
    let plan = reloaded.plan(&plan_id).expect("plan should reload");
    assert_eq!(plan.goal, "Refined goal");
    assert_eq!(plan.title, "Native plan title");
    assert_eq!(plan.kind, prism_ir::PlanKind::Migration);
    assert_eq!(plan.revision, 7);
    assert_eq!(plan.tags, vec!["persistence", "ux"]);
    assert_eq!(
        plan.created_from.as_deref(),
        Some("concept://persistence_runtime")
    );
    assert_eq!(plan.metadata["source"], "native-plan");

    let task = reloaded.task(&task_id).expect("task should reload");
    assert_eq!(task.kind, prism_ir::PlanNodeKind::Validate);
    assert_eq!(task.title, "Investigate deeply");
    assert_eq!(task.summary.as_deref(), Some("Keep authored summary"));
    assert_eq!(task.status, prism_ir::CoordinationTaskStatus::InProgress);
    assert_eq!(
        task.anchors,
        vec![prism_ir::AnchorRef::Kind(prism_ir::NodeKind::Method)]
    );
    assert_eq!(task.bindings.anchors, task.anchors);
    assert_eq!(
        task.bindings.concept_handles,
        vec!["concept://validation_pipeline"]
    );
    assert_eq!(
        task.validation_refs
            .iter()
            .map(|value| value.id.as_str())
            .collect::<Vec<_>>(),
        vec!["validation:alpha"]
    );
    assert!(task.is_abstract);
    assert_eq!(task.priority, Some(4));
    assert_eq!(task.tags, vec!["native", "preserve"]);
    assert_eq!(task.metadata["source"], "native-node");
}

#[test]
fn snapshot_load_replays_handoff_events() {
    let store = CoordinationStore::new();
    let (plan_id, _) = store
        .create_plan(
            meta("event:1", 1),
            PlanCreateInput {
                title: "Handle handoffs".to_string(),
                goal: "Handle handoffs".to_string(),
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
                title: "Review work".to_string(),
                status: Some(prism_ir::CoordinationTaskStatus::InProgress),
                assignee: Some(prism_ir::AgentId::new("agent:a")),
                session: Some(prism_ir::SessionId::new("session:a")),
                worktree_id: None,
                branch_ref: None,
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
    store
        .handoff(
            meta("event:3", 3),
            HandoffInput {
                task_id: task_id.clone(),
                to_agent: Some(prism_ir::AgentId::new("agent:b")),
                summary: "Need review".to_string(),
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
    store
        .accept_handoff(
            meta("event:4", 4),
            HandoffAcceptInput {
                task_id: task_id.clone(),
                agent: Some(prism_ir::AgentId::new("agent:b")),
                worktree_id: None,
                branch_ref: None,
            },
        )
        .unwrap();

    let mut snapshot = store.snapshot();
    let task = snapshot
        .tasks
        .iter_mut()
        .find(|task| task.id == task_id)
        .expect("task should be present");
    task.assignee = Some(prism_ir::AgentId::new("agent:stale"));
    task.pending_handoff_to = Some(prism_ir::AgentId::new("agent:b"));
    task.session = Some(prism_ir::SessionId::new("session:stale"));
    task.status = prism_ir::CoordinationTaskStatus::Blocked;

    let reloaded = CoordinationStore::from_snapshot(snapshot);
    let task = reloaded.task(&task_id).expect("task should reload");
    assert_eq!(task.assignee, Some(prism_ir::AgentId::new("agent:b")));
    assert_eq!(task.pending_handoff_to, None);
    assert_eq!(task.session, None);
    assert_eq!(task.status, prism_ir::CoordinationTaskStatus::Ready);
}

#[test]
fn snapshot_replay_reconstructs_continuity_state_from_events() {
    let store = CoordinationStore::new();
    let (plan_id, _) = store
        .create_plan(
            meta("event:plan", 1),
            PlanCreateInput {
                title: "Replay continuity events".to_string(),
                goal: "Replay continuity events".to_string(),
                status: None,
                policy: None,
            },
        )
        .unwrap();
    let (task_id, task) = store
        .create_task(
            meta("event:task", 2),
            TaskCreateInput {
                plan_id,
                title: "Replay claim and artifact state".to_string(),
                status: Some(prism_ir::CoordinationTaskStatus::Ready),
                assignee: None,
                session: Some(prism_ir::SessionId::new("session:a")),
                worktree_id: Some("worktree:a".into()),
                branch_ref: Some("refs/heads/main".into()),
                anchors: vec![prism_ir::AnchorRef::Kind(prism_ir::NodeKind::Function)],
                depends_on: Vec::new(),
                acceptance: Vec::new(),
                base_revision: prism_ir::WorkspaceRevision::default(),
            },
        )
        .unwrap();
    let (claim_id, _, _) = store
        .acquire_claim(
            meta("event:claim", 3),
            prism_ir::SessionId::new("session:a"),
            ClaimAcquireInput {
                task_id: Some(task_id.clone()),
                anchors: task.anchors.clone(),
                capability: prism_ir::Capability::Edit,
                mode: Some(prism_ir::ClaimMode::HardExclusive),
                ttl_seconds: Some(60),
                base_revision: prism_ir::WorkspaceRevision::default(),
                current_revision: prism_ir::WorkspaceRevision::default(),
                agent: None,
                worktree_id: Some("worktree:a".into()),
                branch_ref: Some("refs/heads/main".into()),
            },
        )
        .unwrap();
    let claim_id = claim_id.expect("claim id");
    let released = store
        .release_claim(
            meta("event:claim:release", 4),
            &prism_ir::SessionId::new("session:a"),
            &claim_id,
        )
        .unwrap();
    assert_eq!(released.status, prism_ir::ClaimStatus::Released);

    let (artifact_id, _) = store
        .propose_artifact(
            meta("event:artifact", 5),
            ArtifactProposeInput {
                task_id,
                anchors: task.anchors.clone(),
                diff_ref: Some("patch:1".into()),
                evidence: Vec::new(),
                base_revision: prism_ir::WorkspaceRevision::default(),
                current_revision: prism_ir::WorkspaceRevision::default(),
                required_validations: Vec::new(),
                validated_checks: Vec::new(),
                risk_score: None,
                worktree_id: Some("worktree:a".into()),
                branch_ref: Some("refs/heads/main".into()),
            },
        )
        .unwrap();
    let (review_id, _, reviewed_artifact) = store
        .review_artifact(
            meta("event:artifact:review", 6),
            ArtifactReviewInput {
                artifact_id: artifact_id.clone(),
                verdict: prism_ir::ReviewVerdict::Approved,
                summary: "approved".into(),
                required_validations: Vec::new(),
                validated_checks: Vec::new(),
                risk_score: None,
            },
            prism_ir::WorkspaceRevision::default(),
        )
        .unwrap();
    assert_eq!(reviewed_artifact.status, prism_ir::ArtifactStatus::Approved);

    let replayed = coordination_snapshot_from_events(&store.events(), None).expect("snapshot");
    assert_eq!(replayed.claims.len(), 1);
    assert_eq!(replayed.claims[0].id, claim_id);
    assert_eq!(replayed.claims[0].status, prism_ir::ClaimStatus::Released);
    assert_eq!(replayed.artifacts.len(), 1);
    assert_eq!(replayed.artifacts[0].id, artifact_id);
    assert_eq!(
        replayed.artifacts[0].status,
        prism_ir::ArtifactStatus::Approved
    );
    assert_eq!(replayed.reviews.len(), 1);
    assert_eq!(replayed.reviews[0].id, review_id);
}

#[test]
fn handoff_acceptance_blocks_updates_until_target_accepts() {
    let store = CoordinationStore::new();
    let (plan_id, _) = store
        .create_plan(
            meta("event:1", 1),
            PlanCreateInput {
                title: "Transfer alpha safely".to_string(),
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
                worktree_id: None,
                branch_ref: None,
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
                kind: None,
                status: Some(prism_ir::CoordinationTaskStatus::InProgress),
                assignee: None,
                session: None,
                worktree_id: None,
                branch_ref: None,
                title: None,
                summary: None,
                anchors: None,
                bindings: None,
                depends_on: None,
                acceptance: None,
                validation_refs: None,
                is_abstract: None,
                base_revision: Some(prism_ir::WorkspaceRevision {
                    graph_version: 1,
                    git_commit: None,
                }),
                priority: None,
                tags: None,
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
                worktree_id: None,
                branch_ref: None,
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
                worktree_id: None,
                branch_ref: None,
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
                worktree_id: None,
                branch_ref: None,
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
                worktree_id: None,
                branch_ref: None,
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
                worktree_id: None,
                branch_ref: None,
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
                worktree_id: None,
                branch_ref: None,
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
                worktree_id: None,
                branch_ref: None,
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
                title: "Protect claim ownership".to_string(),
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
                worktree_id: None,
                branch_ref: None,
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
                worktree_id: None,
                branch_ref: None,
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
            "explicit",
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

#[test]
fn heartbeat_task_refreshes_active_lease_for_same_principal() {
    let store = CoordinationStore::new();
    let (plan_id, _) = store
        .create_plan(
            principal_meta("event:plan", 1, "local", "agent:a", "session:a"),
            PlanCreateInput {
                title: "Refresh task lease".to_string(),
                goal: "Refresh task lease".to_string(),
                status: Some(prism_ir::PlanStatus::Active),
                policy: None,
            },
        )
        .unwrap();
    let (task_id, original) = store
        .create_task(
            principal_meta("event:task", 2, "local", "agent:a", "session:a"),
            TaskCreateInput {
                plan_id: plan_id.clone(),
                title: "Edit alpha".to_string(),
                status: Some(prism_ir::CoordinationTaskStatus::Ready),
                assignee: Some(prism_ir::AgentId::new("agent:a")),
                session: Some(prism_ir::SessionId::new("session:a")),
                worktree_id: None,
                branch_ref: None,
                anchors: vec![prism_ir::AnchorRef::Kind(prism_ir::NodeKind::Function)],
                depends_on: Vec::new(),
                acceptance: Vec::new(),
                base_revision: revision(),
            },
        )
        .unwrap();

    let heartbeated = store
        .heartbeat_task(
            principal_meta("event:heartbeat", 100, "local", "agent:a", "session:a"),
            &task_id,
            "explicit",
        )
        .unwrap();

    assert_eq!(heartbeated.lease_refreshed_at, Some(100));
    assert!(heartbeated.lease_stale_at > original.lease_stale_at);
    let event = store.events().last().unwrap().clone();
    assert_eq!(event.kind, prism_ir::CoordinationEventKind::TaskHeartbeated);
    assert_eq!(event.metadata["renewalProvenance"], "explicit");
    assert_eq!(event.metadata["leaseRenewalMode"], "strict");
}

#[test]
fn stale_task_heartbeat_requires_resume() {
    let store = CoordinationStore::new();
    let (plan_id, _) = store
        .create_plan(
            principal_meta("event:plan", 1, "local", "agent:a", "session:a"),
            PlanCreateInput {
                title: "Reject stale task heartbeat".to_string(),
                goal: "Reject stale task heartbeat".to_string(),
                status: Some(prism_ir::PlanStatus::Active),
                policy: None,
            },
        )
        .unwrap();
    let (task_id, _) = store
        .create_task(
            principal_meta("event:task", 2, "local", "agent:a", "session:a"),
            TaskCreateInput {
                plan_id: plan_id.clone(),
                title: "Edit alpha".to_string(),
                status: Some(prism_ir::CoordinationTaskStatus::Ready),
                assignee: Some(prism_ir::AgentId::new("agent:a")),
                session: Some(prism_ir::SessionId::new("session:a")),
                worktree_id: None,
                branch_ref: None,
                anchors: vec![prism_ir::AnchorRef::Kind(prism_ir::NodeKind::Function)],
                depends_on: Vec::new(),
                acceptance: Vec::new(),
                base_revision: revision(),
            },
        )
        .unwrap();

    let error = store
        .heartbeat_task(
            principal_meta("event:heartbeat", 1900, "local", "agent:a", "session:a"),
            &task_id,
            "explicit",
        )
        .unwrap_err();

    assert!(error.to_string().contains("heartbeat rejected"));
    let rejection = store.events().last().unwrap().clone();
    assert_eq!(
        rejection.kind,
        prism_ir::CoordinationEventKind::MutationRejected
    );
    let violations = store.policy_violations(Some(&plan_id), Some(&task_id), 10);
    assert!(violations.iter().any(|record| {
        record
            .violations
            .iter()
            .any(|violation| violation.code == PolicyViolationCode::TaskResumeRequired)
    }));
}
